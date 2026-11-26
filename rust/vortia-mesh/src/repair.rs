// SPDX-License-Identifier: GPL-3.0-or-later
//! Making a surface watertight where that can be done without guessing, and saying what was
//! done and what is left.
//!
//! A surface that comes out of a modelling tool or a scanner is often *almost* closed: the
//! same corner written twice a hair apart, a triangle with no area, a face listed twice, a
//! triangle wound the wrong way round, a pin-hole where three triangles were never made. The
//! mesher needs a closed surface with a consistent inside, so [`repair`] does, in this order:
//!
//! 1. welds the vertices that are closer than a tolerance (by default a millionth of the size);
//! 2. drops the triangles that have no area, and the repeated ones;
//! 3. turns triangles to agree with their neighbours, so every shared edge is walked in opposite
//!    directions by its two triangles;
//! 4. fills the small holes (boundary loops of up to [`Options::max_hole`] edges) with a fan from
//!    the loop's centre;
//! 5. turns a closed piece that came out inside out, so its volume is positive.
//!
//! What it cannot decide stays as it was and is counted in the [`Report`]: holes that are large
//! or that branch, edges that more than two triangles share, and pieces whose triangles cannot
//! all be wound consistently. The named parts of the surface are kept: a triangle that stays
//! stays in its part, and the triangles that fill holes are a part of their own, `repair`.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::{MeshError, Result};
use crate::stl::{Stl, Summary, SurfacePart};

/// What [`repair`] may do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Vertices closer than this are one. 0 takes a millionth of the surface's diagonal.
    pub weld: f64,
    /// The most edges a hole may have to be filled.
    pub max_hole: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            weld: 0.0,
            max_hole: 64,
        }
    }
}

/// What was found and what was done.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// The surface as it was given.
    pub before: Summary,
    /// The surface as it is now.
    pub after: Summary,
    /// The tolerance used to weld, in the surface's own unit.
    pub tolerance: f64,
    /// Vertices merged into another.
    pub welded: usize,
    /// Triangles with no area, taken out.
    pub degenerate: usize,
    /// Triangles listed again, taken out.
    pub repeated: usize,
    /// Triangles turned to agree with their neighbours.
    pub reoriented: usize,
    /// Closed pieces that were inside out and are turned.
    pub turned_out: usize,
    /// Holes closed with triangles.
    pub holes_filled: usize,
    /// Triangles added to close them.
    pub triangles_added: usize,
    /// Shared edges walked the same way by both triangles, before and after.
    pub inconsistent_before: usize,
    pub inconsistent_after: usize,
    /// Pieces that hang together by their edges, after the repair.
    pub pieces: usize,
}

impl Report {
    /// Whether anything was changed.
    pub fn changed(&self) -> bool {
        self.welded
            + self.degenerate
            + self.repeated
            + self.reoriented
            + self.turned_out
            + self.holes_filled
            > 0
    }

    /// Whether the surface is now closed and consistently wound: the mesher can take it.
    pub fn watertight(&self) -> bool {
        self.after.closed() && self.inconsistent_after == 0
    }
}

impl Report {
    /// What was found and done, as English sentences for a terminal; the app words them itself.
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut done = |n: usize, text: &str| {
            if n > 0 {
                lines.push(format!("{n} {text}"));
            }
        };
        done(self.welded, "vertices welded into their neighbours");
        done(self.degenerate, "triangles with no area removed");
        done(self.repeated, "repeated triangles removed");
        done(
            self.reoriented,
            "triangles turned to agree with their neighbours",
        );
        done(self.turned_out, "closed pieces turned right way out");
        done(
            self.holes_filled,
            &format!("holes filled with {} triangles", self.triangles_added),
        );
        if !self.changed() {
            lines.push("nothing needed repairing".to_owned());
        }
        let after = &self.after;
        lines.push(if self.watertight() {
            format!(
                "the surface is closed and consistently wound: {} triangles, {} piece{}",
                after.triangles,
                self.pieces,
                if self.pieces == 1 { "" } else { "s" }
            )
        } else {
            let mut left = Vec::new();
            if after.open_edges > 0 {
                left.push(format!("{} open edges", after.open_edges));
            }
            if after.non_manifold_edges > 0 {
                left.push(format!(
                    "{} edges shared by more than two triangles",
                    after.non_manifold_edges
                ));
            }
            if self.inconsistent_after > 0 {
                left.push(format!(
                    "{} edges wound the same way by both their triangles",
                    self.inconsistent_after
                ));
            }
            format!("still not watertight: {}", left.join(", "))
        });
        lines
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The directions, `(from, to)`, each shared edge is walked in, by edge: `(low, high)`.
fn edge_walks(triangles: &[[u32; 3]]) -> HashMap<(u32, u32), Vec<(usize, bool)>> {
    let mut edges: HashMap<(u32, u32), Vec<(usize, bool)>> = HashMap::new();
    for (t, &[a, b, c]) in triangles.iter().enumerate() {
        for (from, to) in [(a, b), (b, c), (c, a)] {
            edges
                .entry((from.min(to), from.max(to)))
                .or_default()
                .push((t, from < to));
        }
    }
    edges
}

/// How many shared edges are walked the same way by their two triangles.
pub fn inconsistent_edges(stl: &Stl) -> usize {
    edge_walks(&stl.triangles)
        .values()
        .filter(|walks| matches!(walks.as_slice(), [(_, a), (_, b)] if a == b))
        .count()
}

/// Welds the vertices closer than `tolerance`: each vertex's new index, and the vertices kept.
fn weld(vertices: &[[f64; 3]], tolerance: f64) -> (Vec<u32>, Vec<[f64; 3]>) {
    let cell = |v: &[f64; 3]| v.map(|c| (c / tolerance).floor() as i64);
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut kept: Vec<[f64; 3]> = Vec::new();
    let mut to: Vec<u32> = Vec::with_capacity(vertices.len());
    for v in vertices {
        let at = cell(v);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let key = [
                        at[0].saturating_add(dx),
                        at[1].saturating_add(dy),
                        at[2].saturating_add(dz),
                    ];
                    for &r in grid.get(&key).map_or(&[][..], Vec::as_slice) {
                        let p = kept.get(r as usize).copied().unwrap_or(*v);
                        let d = sub(p, *v);
                        if dot(d, d) <= tolerance * tolerance {
                            found = Some(r);
                            break 'search;
                        }
                    }
                }
            }
        }
        let index = found.unwrap_or_else(|| {
            let r = u32::try_from(kept.len()).unwrap_or(u32::MAX);
            kept.push(*v);
            grid.entry(at).or_default().push(r);
            r
        });
        to.push(index);
    }
    (to, kept)
}

/// Repairs a copy of the surface.
pub fn repair(stl: &Stl, options: Options) -> Result<(Stl, Report)> {
    let before = stl.summary();
    if stl.triangles.is_empty() {
        return invalid("the surface has no triangles");
    }
    let diagonal = sub(before.high, before.low);
    let diagonal = dot(diagonal, diagonal).sqrt();
    if !(diagonal.is_finite() && diagonal > 0.0) {
        return invalid("the surface has no size");
    }
    let tolerance = if options.weld > 0.0 {
        options.weld
    } else {
        1.0e-6 * diagonal
    };
    if !tolerance.is_finite() {
        return invalid("the welding tolerance must be a number");
    }
    let inconsistent_before = inconsistent_edges(stl);

    // 1. Weld.
    let (renumber, mut vertices) = weld(&stl.vertices, tolerance);
    let welded = stl.vertices.len().saturating_sub(vertices.len());
    let part_of = stl.part_of_each();

    // 2. No area, and repeated.
    let mut degenerate = 0;
    let mut repeated = 0;
    let mut seen: HashSet<[u32; 3]> = HashSet::new();
    let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(stl.triangles.len());
    let mut owner: Vec<usize> = Vec::with_capacity(stl.triangles.len());
    for (t, tri) in stl.triangles.iter().enumerate() {
        let map = |i: u32| renumber.get(i as usize).copied();
        let [Some(a), Some(b), Some(c)] = tri.map(map) else {
            return invalid("a triangle names a vertex that does not exist");
        };
        let corner = |i: u32| vertices.get(i as usize).copied().unwrap_or_default();
        let twice = cross(sub(corner(b), corner(a)), sub(corner(c), corner(a)));
        if a == b || b == c || a == c || dot(twice, twice) == 0.0 {
            degenerate += 1;
            continue;
        }
        let mut key = [a, b, c];
        key.sort_unstable();
        if !seen.insert(key) {
            repeated += 1;
            continue;
        }
        triangles.push([a, b, c]);
        owner.push(
            part_of
                .as_ref()
                .and_then(|p| p.get(t))
                .copied()
                .unwrap_or(0),
        );
    }
    if triangles.is_empty() {
        return invalid("every triangle of the surface has no area");
    }

    // 3. Wind neighbours alike, piece by piece.
    let walks = edge_walks(&triangles);
    let mut flipped = vec![false; triangles.len()];
    let mut piece_of = vec![usize::MAX; triangles.len()];
    let mut pieces = 0;
    for start in 0..triangles.len() {
        if piece_of.get(start).copied() != Some(usize::MAX) {
            continue;
        }
        if let Some(p) = piece_of.get_mut(start) {
            *p = pieces;
        }
        let mut queue = VecDeque::from([start]);
        while let Some(t) = queue.pop_front() {
            let Some(&[a, b, c]) = triangles.get(t) else {
                continue;
            };
            let turned = flipped.get(t).copied().unwrap_or(false);
            for (from, to) in [(a, b), (b, c), (c, a)] {
                let Some(shared) = walks.get(&(from.min(to), from.max(to))) else {
                    continue;
                };
                let [(x, dx), (y, dy)] = shared.as_slice() else {
                    continue;
                };
                let (other, other_up) = if *x == t { (*y, *dy) } else { (*x, *dx) };
                if other == t || piece_of.get(other).copied() != Some(usize::MAX) {
                    continue;
                }
                // Walked `from -> to` here, so the neighbour must walk `to -> from`.
                let here_up = (from < to) != turned;
                let must_go_up = !here_up;
                if let (Some(p), Some(f)) = (piece_of.get_mut(other), flipped.get_mut(other)) {
                    *p = pieces;
                    *f = other_up != must_go_up;
                }
                queue.push_back(other);
            }
        }
        pieces += 1;
    }
    let mut reoriented = 0;
    for (tri, turned) in triangles.iter_mut().zip(&flipped) {
        if *turned {
            tri.swap(1, 2);
            reoriented += 1;
        }
    }

    // 4. Small holes: boundary edges that only one triangle has, followed round.
    let directed: HashSet<(u32, u32)> = triangles
        .iter()
        .flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)])
        .collect();
    let mut outgoing: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(u, v) in &directed {
        if !directed.contains(&(v, u)) {
            outgoing.entry(u).or_default().push(v);
        }
    }
    let mut holes_filled = 0;
    let mut triangles_added = 0;
    let mut filled: Vec<[u32; 3]> = Vec::new();
    let mut used: HashSet<u32> = HashSet::new();
    let mut starts: Vec<u32> = outgoing.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if used.contains(&start) {
            continue;
        }
        let mut ring = vec![start];
        let mut closed = false;
        let mut at = start;
        while ring.len() <= options.max_hole {
            let Some([next]) = outgoing.get(&at).map(Vec::as_slice) else {
                break;
            };
            if *next == start {
                closed = true;
                break;
            }
            if ring.contains(next) {
                break;
            }
            ring.push(*next);
            at = *next;
        }
        if !closed || ring.len() < 3 {
            continue;
        }
        // A branching vertex anywhere on the ring means the hole is not a simple one.
        if ring
            .iter()
            .any(|v| outgoing.get(v).is_some_and(|o| o.len() != 1))
        {
            continue;
        }
        used.extend(ring.iter().copied());
        if let [a, b, c] = ring.as_slice() {
            // The ring runs a -> b -> c -> a; the new triangle must run the other way round.
            filled.push([*a, *c, *b]);
        } else {
            let corners: Vec<[f64; 3]> = ring
                .iter()
                .map(|&i| vertices.get(i as usize).copied().unwrap_or_default())
                .collect();
            let n = corners.len() as f64;
            let centre = corners.iter().fold([0.0; 3], |s, p| {
                [s[0] + p[0] / n, s[1] + p[1] / n, s[2] + p[2] / n]
            });
            let Ok(c) = u32::try_from(vertices.len()) else {
                return invalid("too many vertices");
            };
            vertices.push(centre);
            for (k, &u) in ring.iter().enumerate() {
                let v = ring.get((k + 1) % ring.len()).copied().unwrap_or(start);
                filled.push([v, u, c]);
            }
        }
        holes_filled += 1;
    }
    triangles_added += filled.len();
    triangles.extend(filled);
    let kept_original = owner.len();

    // 5. Closed pieces that came out inside out.
    let walks = edge_walks(&triangles);
    let mut piece_of = vec![usize::MAX; triangles.len()];
    let mut pieces = 0;
    let mut turned_out = 0;
    for start in 0..triangles.len() {
        if piece_of.get(start).copied() != Some(usize::MAX) {
            continue;
        }
        let mut members = vec![start];
        if let Some(p) = piece_of.get_mut(start) {
            *p = pieces;
        }
        let mut next = 0;
        let mut open = false;
        while let Some(&t) = members.get(next) {
            next += 1;
            let Some(&[a, b, c]) = triangles.get(t) else {
                continue;
            };
            for (from, to) in [(a, b), (b, c), (c, a)] {
                let Some(shared) = walks.get(&(from.min(to), from.max(to))) else {
                    continue;
                };
                if shared.len() != 2 {
                    open = true;
                }
                for &(other, _) in shared {
                    if piece_of.get(other).copied() == Some(usize::MAX) {
                        if let Some(p) = piece_of.get_mut(other) {
                            *p = pieces;
                        }
                        members.push(other);
                    }
                }
            }
        }
        pieces += 1;
        if open {
            continue;
        }
        let volume: f64 = members
            .iter()
            .filter_map(|&t| triangles.get(t))
            .map(|&[a, b, c]| {
                let p = |i: u32| vertices.get(i as usize).copied().unwrap_or_default();
                dot(p(a), cross(p(b), p(c))) / 6.0
            })
            .sum();
        if volume < 0.0 {
            for t in members {
                if let Some(tri) = triangles.get_mut(t) {
                    tri.swap(1, 2);
                }
            }
            turned_out += 1;
        }
    }

    // The surface: the vertices that are still used, and the parts as runs of what stayed.
    let mut renumber = vec![u32::MAX; vertices.len()];
    let mut compact: Vec<[f64; 3]> = Vec::new();
    for tri in &mut triangles {
        for i in tri.iter_mut() {
            let slot = renumber.get_mut(*i as usize);
            let new = match slot {
                Some(s) if *s != u32::MAX => *s,
                Some(s) => {
                    let n = u32::try_from(compact.len()).unwrap_or(u32::MAX);
                    *s = n;
                    compact.push(vertices.get(*i as usize).copied().unwrap_or_default());
                    n
                }
                None => return invalid("a triangle names a vertex that does not exist"),
            };
            *i = new;
        }
    }
    let mut parts: Vec<SurfacePart> = Vec::new();
    if part_of.is_some() {
        let names: Vec<&str> = stl.parts.iter().map(|p| p.name.as_str()).collect();
        let mut start = 0;
        for k in 0..=kept_original {
            let here = owner.get(k).copied();
            let begin = owner.get(start).copied();
            if k == kept_original || here != begin {
                if let Some(index) = begin {
                    parts.push(SurfacePart {
                        name: names.get(index).copied().unwrap_or("part").to_owned(),
                        start,
                        end: k,
                    });
                }
                start = k;
            }
        }
        if triangles.len() > kept_original {
            parts.push(SurfacePart {
                name: "repair".to_owned(),
                start: kept_original,
                end: triangles.len(),
            });
        }
    }
    let out = Stl {
        vertices: compact,
        triangles,
        parts,
    };
    let after = out.summary();
    let report = Report {
        before,
        after,
        tolerance,
        welded,
        degenerate,
        repeated,
        reoriented,
        turned_out,
        holes_filled,
        triangles_added,
        inconsistent_before,
        inconsistent_after: inconsistent_edges(&out),
        pieces,
    };
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINTS: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    const QUADS: [[u32; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [1, 2, 6, 5],
        [3, 0, 4, 7],
    ];

    /// The unit cube, outward, with the vertices shared.
    fn cube() -> Stl {
        let triangles = QUADS
            .iter()
            .flat_map(|&[a, b, c, d]| [[a, b, c], [a, c, d]])
            .collect();
        Stl {
            vertices: POINTS.to_vec(),
            triangles,
            parts: Vec::new(),
        }
    }

    fn volume(stl: &Stl) -> f64 {
        stl.summary().volume.unwrap()
    }

    #[test]
    fn a_closed_surface_is_left_as_it_is() {
        let (out, report) = repair(&cube(), Options::default()).unwrap();
        assert!(!report.changed(), "{report:?}");
        assert!(report.watertight());
        assert_eq!(out.triangles.len(), 12);
        assert_eq!(report.pieces, 1);
    }

    #[test]
    fn corners_written_twice_a_hair_apart_are_welded() {
        // Each triangle with its own copy of its corners, each nudged by a billionth.
        let cube = cube();
        let mut loose = Stl::default();
        for (t, tri) in cube.triangles.iter().enumerate() {
            let mut ids = [0u32; 3];
            for (k, &i) in tri.iter().enumerate() {
                let mut p = cube.vertices[i as usize];
                p[0] += 1.0e-9 * (t + k) as f64;
                ids[k] = loose.vertices.len() as u32;
                loose.vertices.push(p);
            }
            loose.triangles.push(ids);
        }
        assert!(!loose.summary().closed());
        let (out, report) = repair(&loose, Options::default()).unwrap();
        assert_eq!(report.welded, 36 - 8);
        assert!(report.watertight(), "{report:?}");
        assert!((volume(&out) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn triangles_with_no_area_and_repeated_ones_are_taken_out() {
        let mut stl = cube();
        stl.triangles.push([0, 0, 1]);
        // The other half of the bottom face, turned the other way round: the same triangle.
        stl.triangles.push([0, 1, 2]);
        let repeated = stl.triangles[0];
        stl.triangles.push([repeated[0], repeated[2], repeated[1]]);
        stl.triangles.push(repeated);
        let (out, report) = repair(&stl, Options::default()).unwrap();
        assert_eq!(report.degenerate, 1);
        assert_eq!(report.repeated, 3);
        assert!(report.watertight(), "{report:?}");
        assert_eq!(out.triangles.len(), 12);
    }

    #[test]
    fn triangles_wound_the_wrong_way_are_turned_to_agree() {
        let mut stl = cube();
        for t in [1, 4, 9] {
            stl.triangles[t].swap(1, 2);
        }
        assert!(inconsistent_edges(&stl) > 0);
        let (out, report) = repair(&stl, Options::default()).unwrap();
        assert!(report.reoriented > 0);
        assert_eq!(report.inconsistent_after, 0);
        assert!(report.watertight());
        assert!((volume(&out) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_surface_that_is_inside_out_is_turned_outward() {
        let mut stl = cube();
        for tri in &mut stl.triangles {
            tri.swap(1, 2);
        }
        let (out, report) = repair(&stl, Options::default()).unwrap();
        assert_eq!(report.turned_out, 1);
        let signed: f64 = out
            .triangles
            .iter()
            .map(|&[a, b, c]| {
                let p = |i: u32| out.vertices[i as usize];
                dot(p(a), cross(p(b), p(c))) / 6.0
            })
            .sum();
        assert!((signed - 1.0).abs() < 1e-12, "{signed}");
    }

    #[test]
    fn small_holes_are_filled_and_large_ones_are_reported() {
        // Take one triangle out: a hole of three edges.
        let mut stl = cube();
        stl.triangles.remove(5);
        assert_eq!(stl.summary().open_edges, 3);
        let (out, report) = repair(&stl, Options::default()).unwrap();
        assert_eq!(report.holes_filled, 1);
        assert_eq!(report.triangles_added, 1);
        assert!(report.watertight(), "{report:?}");
        assert!((volume(&out) - 1.0).abs() < 1e-12);
        // Take a whole face out: a hole of four edges, filled from its centre.
        let mut square = cube();
        square.triangles.drain(0..2);
        let (out, report) = repair(&square, Options::default()).unwrap();
        assert_eq!((report.holes_filled, report.triangles_added), (1, 4));
        assert!(report.watertight(), "{report:?}");
        assert!((volume(&out) - 1.0).abs() < 1e-12);
        // With the limit below the hole it stays, and the report says it is not closed.
        let (_, report) = repair(
            &square,
            Options {
                max_hole: 3,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(report.holes_filled, 0);
        assert!(!report.watertight());
        assert_eq!(report.after.open_edges, 4);
    }

    #[test]
    fn the_named_parts_survive_and_the_filling_is_a_part_of_its_own() {
        let mut stl = cube();
        stl.parts = vec![
            SurfacePart {
                name: "bottom".into(),
                start: 0,
                end: 2,
            },
            SurfacePart {
                name: "rest".into(),
                start: 2,
                end: 12,
            },
        ];
        stl.triangles.remove(5);
        stl.parts[1].end = 11;
        let (out, report) = repair(&stl, Options::default()).unwrap();
        assert!(report.watertight());
        let names: Vec<_> = out
            .parts
            .iter()
            .map(|p| (p.name.as_str(), p.start, p.end))
            .collect();
        assert_eq!(
            names,
            [("bottom", 0, 2), ("rest", 2, 11), ("repair", 11, 12)]
        );
        assert!(out.part_of_each().is_some());
    }

    #[test]
    fn what_cannot_be_decided_is_left_and_counted() {
        // Three triangles on one edge: non-manifold, nothing to do about it.
        let stl = Stl {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            triangles: vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]],
            parts: Vec::new(),
        };
        let (_, report) = repair(&stl, Options::default()).unwrap();
        assert_eq!(report.after.non_manifold_edges, 1);
        assert!(!report.watertight());
    }

    #[test]
    fn nothing_to_repair_is_an_error_not_a_panic() {
        assert!(repair(&Stl::default(), Options::default()).is_err());
        let flat = Stl {
            vertices: vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            parts: Vec::new(),
        };
        assert!(repair(&flat, Options::default()).is_err());
        let bad = Stl {
            vertices: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            triangles: vec![[0, 1, 7]],
            parts: Vec::new(),
        };
        assert!(repair(&bad, Options::default()).is_err());
        let huge = Options {
            weld: f64::NAN,
            ..Options::default()
        };
        assert!(
            repair(&cube(), huge).is_ok(),
            "a NaN tolerance means the default"
        );
    }
}
