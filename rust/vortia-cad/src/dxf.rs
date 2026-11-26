// SPDX-License-Identifier: GPL-3.0-or-later
//! DXF drawings as the 2D outlines the triangle mesher takes.
//!
//! A drawing is read with Nodalia's own DXF reader (the one Forja uses). Lines, arcs, circles,
//! ellipses, polylines (with their arc bulges) and splines are cut into straight pieces, joined
//! end to end into closed loops, and kept in millimetres, which is the sketch's unit. Two things
//! can then be made of them:
//!
//! - [`Drawing::sketch`]: the drawing is the region itself. The layer of each piece names its
//!   patch (`inlet`, `outlet`, `symmetry`, anything else is a wall), as an edge's name does in a
//!   sketch; layer `0` is a wall.
//! - [`Drawing::around`]: the loops are bodies (an airfoil section, a car's side profile, a
//!   cylinder) and a wind tunnel is drawn around them, with the patches a flow around a body
//!   needs.
//!
//! Not read: blocks (`INSERT`), text, dimensions and hatches are left out, and so is everything
//! in paper space or on a layer that is switched off; [`Drawing::notes`] says how many.

use std::collections::BTreeMap;
use std::path::Path;

use nodalia_dxf::model::{Document, Entity, EntityKind, Point, Spline, ocs_to_wcs};
use vortia_mesh::{MeshError, Result};
use vortia_sketch::{Entity as SketchEntity, MAX_POINTS, Sketch};

/// The largest file read.
pub const MAX_BYTES: u64 = 128 << 20;
/// The most pieces (lines, arcs, polylines…) a drawing may have, so chaining stays quick.
pub const MAX_PIECES: usize = 20_000;
/// The coarsest an arc is cut, in degrees per chord.
const ARC_STEP: f64 = 3.0;
/// How far, in the drawing's size, two ends may be and still be the same corner.
const JOIN: f64 = 1.0e-5;
/// The points kept free for the tunnel around the bodies.
const TUNNEL_POINTS: usize = 8;

fn invalid(message: impl Into<String>) -> MeshError {
    MeshError::Invalid(message.into())
}

type P = (f64, f64);

/// A run of a loop: one drawn line, arc or polyline, on one layer, in millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub layer: String,
    /// From the start to the end, both included.
    pub points: Vec<P>,
}

/// A closed outline of the drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    /// A circle, kept whole.
    Circle {
        layer: String,
        center: P,
        radius: f64,
    },
    /// Edges that follow each other and close on the first one's start.
    Loop(Vec<Edge>),
}

/// What was read from a DXF file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub outlines: Vec<Outline>,
    /// What the reader did that the person may want to know: the unit that was assumed, what was
    /// left out.
    pub notes: Vec<String>,
}

/// The wind tunnel [`Drawing::around`] draws, in multiples of the bodies' largest size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tunnel {
    pub upstream: f64,
    pub downstream: f64,
    /// Above and below.
    pub lateral: f64,
}

impl Default for Tunnel {
    fn default() -> Self {
        Self {
            upstream: 5.0,
            downstream: 12.0,
            lateral: 6.0,
        }
    }
}

/// The patch of the bodies in [`Drawing::around`].
pub const BODY_PATCH: &str = "body";

/// Reads a DXF file.
pub fn read(path: &Path) -> Result<Drawing> {
    let size = std::fs::metadata(path)
        .map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?
        .len();
    if size > MAX_BYTES {
        return Err(invalid(format!(
            "{} is {} MB; a DXF is read up to {} MB",
            path.display(),
            size >> 20,
            MAX_BYTES >> 20
        )));
    }
    let bytes =
        std::fs::read(path).map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
    parse(&bytes)
}

/// Reads the bytes of a DXF file.
pub fn parse(bytes: &[u8]) -> Result<Drawing> {
    let tags = nodalia_dxf::parse(bytes).map_err(|e| invalid(format!("not a DXF drawing: {e}")))?;
    let document = Document::read(&tags);
    if document.entities.is_empty() {
        return Err(invalid("the drawing has no entities"));
    }
    let mut notes = Vec::new();
    let (to_mm, unit_note) = unit(document.header.insunits());
    notes.extend(unit_note);
    let off: Vec<String> = document
        .layers()
        .into_iter()
        .filter(|l| !l.on || l.frozen)
        .map(|l| l.name)
        .collect();

    let mut pieces: Vec<Piece> = Vec::new();
    let mut circles: Vec<(String, P, f64)> = Vec::new();
    let mut left_out: BTreeMap<String, usize> = BTreeMap::new();
    for record in &document.entities {
        let entity = Entity::read(record);
        if entity.common.paper_space || !entity.common.visible || off.contains(&entity.common.layer)
        {
            continue;
        }
        let layer = entity.common.layer.clone();
        let normal = entity.common.extrusion;
        let xy = |p: Point| {
            let w = ocs_to_wcs(normal, p);
            (w[0] * to_mm, w[1] * to_mm)
        };
        let mut piece = |points: Vec<P>, closed: bool| {
            pieces.push(Piece {
                layer: layer.clone(),
                points,
                closed,
            });
        };
        match &entity.kind {
            EntityKind::Line { start, end } => {
                // A line is in world coordinates.
                let p = |q: &Point| (q[0] * to_mm, q[1] * to_mm);
                piece(vec![p(start), p(end)], false);
            }
            EntityKind::Circle { center, radius } => {
                let c = xy(*center);
                circles.push((layer.clone(), c, radius * to_mm));
            }
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let mut sweep = end_angle - start_angle;
                while sweep <= 0.0 {
                    sweep += 360.0;
                }
                let points = arc(
                    (center[0], center[1]),
                    *radius,
                    start_angle.to_radians(),
                    sweep.to_radians(),
                )
                .into_iter()
                .map(|p| xy([p.0, p.1, center[2]]))
                .collect();
                piece(points, false);
            }
            EntityKind::Ellipse {
                center,
                major,
                ratio,
                start,
                end,
            } => {
                let points = ellipse(*center, *major, *ratio, *start, *end, normal)
                    .into_iter()
                    .map(|p| (p.0 * to_mm, p.1 * to_mm))
                    .collect();
                piece(points, false);
            }
            EntityKind::LwPolyline(poly) => {
                let corners: Vec<(P, f64)> = poly
                    .vertices
                    .iter()
                    .map(|v| ((v.x, v.y), v.bulge))
                    .collect();
                let points = along(&corners, poly.closed)
                    .into_iter()
                    .map(|p| xy([p.0, p.1, poly.elevation]))
                    .collect();
                piece(points, poly.closed);
            }
            EntityKind::Polyline(poly) if !poly.is_mesh() && !poly.is_polyface() => {
                let corners: Vec<(P, f64)> = poly
                    .vertices
                    .iter()
                    .filter(|v| !v.is_spline_frame())
                    .map(|v| ((v.location[0], v.location[1]), v.bulge))
                    .collect();
                let flat = !poly.is_3d();
                let points = along(&corners, poly.closed())
                    .into_iter()
                    .map(|p| {
                        if flat {
                            xy([p.0, p.1, poly.elevation])
                        } else {
                            (p.0 * to_mm, p.1 * to_mm)
                        }
                    })
                    .collect();
                piece(points, poly.closed());
            }
            EntityKind::Spline(spline) => {
                let points: Vec<P> = spline_points(spline)
                    .into_iter()
                    .map(|p| (p.0 * to_mm, p.1 * to_mm))
                    .collect();
                piece(points, spline.flags & 1 != 0);
            }
            _ => {
                *left_out.entry(record.kind.to_owned()).or_default() += 1;
            }
        }
        if pieces.len() > MAX_PIECES {
            return Err(invalid(format!(
                "the drawing has more than {MAX_PIECES} lines, arcs and polylines"
            )));
        }
    }
    if !left_out.is_empty() {
        let list: Vec<String> = left_out.iter().map(|(k, n)| format!("{n} {k}")).collect();
        notes.push(format!("left out: {}", list.join(", ")));
    }
    let outlines = outlines(pieces, circles, &mut notes)?;
    if outlines.is_empty() {
        return Err(invalid(
            "the drawing has no closed outline: draw the section as lines, arcs, polylines or a circle that close on themselves",
        ));
    }
    Ok(Drawing { outlines, notes })
}

/// The factor from the file's unit to millimetres, and a note when the unit is a guess.
fn unit(insunits: i64) -> (f64, Option<String>) {
    match insunits {
        1 => (25.4, None),
        2 => (304.8, None),
        4 => (1.0, None),
        5 => (10.0, None),
        6 => (1000.0, None),
        9 => (0.0254, None),
        10 => (914.4, None),
        13 => (0.001, None),
        14 => (100.0, None),
        0 => (
            1.0,
            Some("the file has no unit: millimetres assumed (scale= changes it)".to_owned()),
        ),
        other => (
            1.0,
            Some(format!(
                "the file's unit (code {other}) is not one Vortia reads: millimetres assumed"
            )),
        ),
    }
}

/// One drawn line, arc or polyline, before it is joined to the others.
#[derive(Clone, Debug)]
struct Piece {
    layer: String,
    points: Vec<P>,
    closed: bool,
}

fn dist(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The points of an arc about `c`, counter-clockwise from `from` through `sweep` radians.
fn arc(c: P, radius: f64, from: f64, sweep: f64) -> Vec<P> {
    if !(radius.is_finite() && radius > 0.0 && sweep.is_finite()) {
        return Vec::new();
    }
    let n = ((sweep.abs().to_degrees() / ARC_STEP).ceil() as usize).clamp(1, 4096);
    (0..=n)
        .map(|i| {
            let a = from + sweep * (i as f64) / (n as f64);
            (c.0 + radius * a.cos(), c.1 + radius * a.sin())
        })
        .collect()
}

/// The points along polyline corners with their bulges (the tangent of a quarter of the arc to
/// the next corner), closing back on the first when `closed`.
fn along(corners: &[(P, f64)], closed: bool) -> Vec<P> {
    let mut out: Vec<P> = Vec::new();
    let count = corners.len();
    let segments = if closed {
        count
    } else {
        count.saturating_sub(1)
    };
    for i in 0..segments {
        let (Some(&(a, bulge)), Some(&(b, _))) = (corners.get(i), corners.get((i + 1) % count))
        else {
            continue;
        };
        if out.is_empty() {
            out.push(a);
        }
        let d = dist(a, b);
        if bulge.abs() > 1e-9 && d > 0.0 && bulge.is_finite() {
            // The included angle is four times the arc's quarter-angle.
            let theta = 4.0 * bulge.atan();
            let h = d / 2.0 / (theta / 2.0).tan();
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let normal = (-(b.1 - a.1) / d, (b.0 - a.0) / d);
            let centre = (mid.0 + normal.0 * h, mid.1 + normal.1 * h);
            let radius = (d / 2.0 / (theta / 2.0).sin()).abs();
            let from = (a.1 - centre.1).atan2(a.0 - centre.0);
            let mut arc = arc(centre, radius, from, theta);
            if let Some(last) = arc.last_mut() {
                *last = b;
            }
            out.extend(arc.into_iter().skip(1));
        } else {
            out.push(b);
        }
    }
    if corners.len() == 1 {
        out.extend(corners.first().map(|c| c.0));
    }
    out
}

/// An ellipse (or a part of it) in world coordinates, as x and y.
fn ellipse(c: Point, major: Point, ratio: f64, start: f64, end: f64, normal: Point) -> Vec<P> {
    let a = (major[0], major[1], major[2]);
    let len = (a.0 * a.0 + a.1 * a.1 + a.2 * a.2).sqrt();
    let n = ocs_to_wcs(normal, [0.0, 0.0, 1.0]);
    // The minor axis lies in the plane of the ellipse: normal × major, as long as the ratio says.
    let m = [
        n[1] * a.2 - n[2] * a.1,
        n[2] * a.0 - n[0] * a.2,
        n[0] * a.1 - n[1] * a.0,
    ];
    let m_len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    if !(len.is_finite() && len > 0.0 && m_len > 0.0 && ratio.is_finite() && ratio > 0.0) {
        return Vec::new();
    }
    let k = ratio * len / m_len;
    let mut sweep = end - start;
    while sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    let steps = ((sweep.to_degrees() / ARC_STEP).ceil() as usize).clamp(1, 4096);
    (0..=steps)
        .map(|i| {
            let t = start + sweep * (i as f64) / (steps as f64);
            (
                c[0] + a.0 * t.cos() + m[0] * k * t.sin(),
                c[1] + a.1 * t.cos() + m[1] * k * t.sin(),
            )
        })
        .collect()
}

/// A spline as points: its NURBS curve when it has control points, its fit points otherwise.
fn spline_points(s: &Spline) -> Vec<P> {
    let degree = usize::try_from(s.degree).unwrap_or(0);
    let n = s.control.len();
    let valid = (1..=10).contains(&degree)
        && n > degree
        && s.knots.len() == n + degree + 1
        && s.knots.windows(2).all(|w| matches!(w, [a, b] if a <= b));
    if valid {
        let weights: Vec<f64> = if s.weights.len() == n {
            s.weights.clone()
        } else {
            vec![1.0; n]
        };
        let (Some(&t0), Some(&t1)) = (s.knots.get(degree), s.knots.get(n)) else {
            return Vec::new();
        };
        if t1 > t0 {
            let samples = (n * 8).clamp(32, 4000);
            let mut out = Vec::with_capacity(samples + 1);
            for i in 0..=samples {
                let t = t0 + (t1 - t0) * (i as f64) / (samples as f64);
                if let Some(p) = de_boor(s, &weights, degree, t) {
                    out.push(p);
                }
            }
            if out.len() >= 2 {
                return out;
            }
        }
    }
    s.fit.iter().map(|p| (p[0], p[1])).collect()
}

/// The point of a NURBS curve at `t`, by de Boor's algorithm on weighted coordinates.
fn de_boor(s: &Spline, weights: &[f64], degree: usize, t: f64) -> Option<P> {
    let n = s.control.len();
    // The span `k` with knots[k] <= t < knots[k + 1], the last one at the end of the domain.
    let mut k = degree;
    while k + 1 < n && s.knots.get(k + 1).is_some_and(|&next| next <= t) {
        k += 1;
    }
    let mut d: Vec<[f64; 3]> = Vec::with_capacity(degree + 1);
    for j in 0..=degree {
        let i = k + j - degree;
        let c = s.control.get(i)?;
        let w = *weights.get(i)?;
        d.push([c[0] * w, c[1] * w, w]);
    }
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = k + j - degree;
            let left = *s.knots.get(i)?;
            let right = *s.knots.get(i + degree + 1 - r)?;
            let span = right - left;
            let alpha = if span > 0.0 { (t - left) / span } else { 0.0 };
            let (prev, cur) = (*d.get(j - 1)?, *d.get(j)?);
            *d.get_mut(j)? = [
                (1.0 - alpha) * prev[0] + alpha * cur[0],
                (1.0 - alpha) * prev[1] + alpha * cur[1],
                (1.0 - alpha) * prev[2] + alpha * cur[2],
            ];
        }
    }
    let end = d.get(degree)?;
    (end[2].abs() > 1e-12).then(|| (end[0] / end[2], end[1] / end[2]))
}

/// The bounding box of some points.
fn extent(points: impl Iterator<Item = P>) -> Option<(P, P)> {
    points.fold(None, |acc, p| match acc {
        None => Some((p, p)),
        Some((lo, hi)) => Some((
            (lo.0.min(p.0), lo.1.min(p.1)),
            (hi.0.max(p.0), hi.1.max(p.1)),
        )),
    })
}

/// Joins the pieces end to end into closed outlines.
fn outlines(
    pieces: Vec<Piece>,
    circles: Vec<(String, P, f64)>,
    notes: &mut Vec<String>,
) -> Result<Vec<Outline>> {
    let size = extent(
        pieces.iter().flat_map(|p| p.points.iter().copied()).chain(
            circles
                .iter()
                .flat_map(|(_, c, r)| [(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)]),
        ),
    )
    .map_or(1.0, |(lo, hi)| dist(lo, hi));
    let tol = (size * JOIN).max(1e-9);
    let mut out: Vec<Outline> = Vec::new();
    for (layer, center, radius) in circles {
        if radius.is_finite() && radius > 0.0 {
            out.push(Outline::Circle {
                layer,
                center,
                radius,
            });
        }
    }

    // Drop repeated points and pieces with nothing in them.
    let mut free: Vec<Option<Piece>> = pieces
        .into_iter()
        .filter_map(|mut p| {
            // A closed run ends where it began, however its curve was cut.
            if let (true, Some(&first)) = (p.closed, p.points.first()) {
                p.points.push(first);
            }
            p.points.dedup_by(|a, b| dist(*a, *b) <= tol);
            (p.points.len() >= 2).then_some(p)
        })
        .map(Some)
        .collect();

    for i in 0..free.len() {
        let Some(first) = free.get_mut(i).and_then(Option::take) else {
            continue;
        };
        let mut chain: Vec<Piece> = vec![first];
        let mut turned = false;
        while let (Some(head), Some(tail)) = (
            chain.first().and_then(|e| e.points.first()).copied(),
            chain.last().and_then(|e| e.points.last()).copied(),
        ) {
            if dist(head, tail) <= tol {
                break;
            }
            let next = free.iter().position(|p| {
                p.as_ref().is_some_and(|p| {
                    p.points.first().is_some_and(|&a| dist(a, tail) <= tol)
                        || p.points.last().is_some_and(|&b| dist(b, tail) <= tol)
                })
            });
            match next {
                Some(j) => {
                    let Some(mut p) = free.get_mut(j).and_then(Option::take) else {
                        break;
                    };
                    if p.points.first().is_none_or(|&a| dist(a, tail) > tol) {
                        p.points.reverse();
                    }
                    chain.push(p);
                }
                None if !turned => {
                    // Nothing continues the tail: continue from the other end.
                    turned = true;
                    chain.reverse();
                    for p in &mut chain {
                        p.points.reverse();
                    }
                }
                None => {
                    return Err(invalid(format!(
                        "an outline is open near ({:.3}, {:.3}) mm: its lines and arcs do not close on themselves (the joins are tolerated to {:.1e} mm)",
                        tail.0, tail.1, tol
                    )));
                }
            }
        }
        // Make the corners exact: every edge starts where the one before ended.
        let mut previous: Option<P> = None;
        for p in &mut chain {
            if let (Some(at), Some(first)) = (previous, p.points.first_mut()) {
                *first = at;
            }
            previous = p.points.last().copied();
        }
        if let (Some(start), Some(last)) = (
            chain.first().and_then(|e| e.points.first()).copied(),
            chain.last_mut().and_then(|e| e.points.last_mut()),
        ) {
            *last = start;
        }
        let layer_names: Vec<String> = chain.iter().map(|p| p.layer.clone()).collect();
        let mut edges: Vec<Edge> = chain
            .into_iter()
            .zip(layer_names)
            .map(|(p, layer)| Edge {
                layer,
                points: p.points,
            })
            .collect();
        // A single closed run is cut in two at its farthest corner, so the outline keeps its
        // shape when its runs are simplified.
        if let [only] = edges.as_mut_slice() {
            if only.points.len() < 4 {
                notes.push("an outline with fewer than three corners was left out".to_owned());
                continue;
            }
            let start = only.points.first().copied().unwrap_or((0.0, 0.0));
            let far = only
                .points
                .iter()
                .enumerate()
                .max_by(|a, b| dist(*a.1, start).total_cmp(&dist(*b.1, start)))
                .map_or(1, |(k, _)| k);
            let second = only.points.split_off(far);
            let mut first = std::mem::take(&mut only.points);
            let layer = only.layer.clone();
            if let Some(&mid) = second.first() {
                first.push(mid);
            }
            edges = vec![
                Edge {
                    layer: layer.clone(),
                    points: first,
                },
                Edge {
                    layer,
                    points: second,
                },
            ];
        }
        out.push(Outline::Loop(edges));
    }
    Ok(out)
}

/// The name a layer gives a patch: lower case, letters, digits and underscores.
fn patch(layer: &str) -> String {
    let name: String = layer
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if name.is_empty() || name == "0" {
        "wall".to_owned()
    } else {
        name
    }
}

/// The points of a polyline left after dropping those closer than `eps` to the line between their
/// neighbours (Douglas–Peucker), keeping both ends.
fn simplify(points: &[P], eps: f64) -> Vec<P> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack = vec![(0usize, n - 1)];
    while let Some((lo, hi)) = stack.pop() {
        let (Some(&a), Some(&b)) = (points.get(lo), points.get(hi)) else {
            continue;
        };
        let mut worst = (0.0, lo);
        for k in lo + 1..hi {
            let Some(&p) = points.get(k) else { continue };
            let d = line_distance(p, a, b);
            if d > worst.0 {
                worst = (d, k);
            }
        }
        if worst.0 > eps {
            if let Some(k) = keep.get_mut(worst.1) {
                *k = true;
            }
            stack.push((lo, worst.1));
            stack.push((worst.1, hi));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

fn line_distance(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return dist(p, a);
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len
}

impl Drawing {
    /// Multiplies every length: for a drawing whose file gave no unit, or the wrong one.
    pub fn scale(&mut self, factor: f64) -> Result<()> {
        if !(factor.is_finite() && factor > 0.0 && factor <= 1.0e6) {
            return Err(invalid(format!(
                "the scale must be a number above 0 and at most 1e6, not {factor}"
            )));
        }
        let at = |p: &mut P| *p = (p.0 * factor, p.1 * factor);
        for o in &mut self.outlines {
            match o {
                Outline::Circle { center, radius, .. } => {
                    at(center);
                    *radius *= factor;
                }
                Outline::Loop(edges) => {
                    for e in edges {
                        e.points.iter_mut().for_each(at);
                    }
                }
            }
        }
        Ok(())
    }

    /// The bounding box of everything drawn: lower left and upper right.
    pub fn bounds(&self) -> Option<(P, P)> {
        extent(self.outlines.iter().flat_map(|o| match o {
            Outline::Circle { center, radius, .. } => vec![
                (center.0 - radius, center.1 - radius),
                (center.0 + radius, center.1 + radius),
            ],
            Outline::Loop(edges) => edges.iter().flat_map(|e| e.points.clone()).collect(),
        }))
    }

    /// How many sketch points the outlines take.
    fn points(&self) -> usize {
        self.outlines
            .iter()
            .map(|o| match o {
                Outline::Circle { .. } => 2,
                Outline::Loop(edges) => {
                    edges.iter().map(|e| e.points.len().saturating_sub(1)).sum()
                }
            })
            .sum()
    }

    /// Thins the polylines until the outlines fit the sketch's limit, as little as it takes.
    fn fit(&mut self, budget: usize) -> Result<()> {
        if self.points() <= budget {
            return Ok(());
        }
        let size = self.bounds().map_or(1.0, |(lo, hi)| dist(lo, hi));
        let mut eps = size * 1.0e-5;
        for _ in 0..60 {
            let mut thin = self.clone();
            for o in &mut thin.outlines {
                if let Outline::Loop(edges) = o {
                    for e in edges {
                        e.points = simplify(&e.points, eps);
                    }
                }
            }
            if thin.points() <= budget {
                self.notes.push(format!(
                    "the outline had {} corners and a sketch holds {MAX_POINTS}: it was simplified to within {eps:.3} mm",
                    self.points()
                ));
                self.outlines = thin.outlines;
                return Ok(());
            }
            eps *= 1.6;
        }
        Err(invalid(format!(
            "the drawing has more detail than a sketch's {MAX_POINTS} points can hold"
        )))
    }

    /// The drawing as a sketch region: each layer names its patch.
    ///
    /// A drawing with more corners than a sketch holds is thinned first, and the note says how far.
    pub fn sketch(&mut self) -> Result<Sketch> {
        self.fit(MAX_POINTS)?;
        let mut sketch = Sketch::default();
        self.add_to(&mut sketch, patch)?;
        sketch.validate().map_err(|e| invalid(e.to_string()))?;
        Ok(sketch)
    }

    /// The drawing's outlines as bodies in a wind tunnel: the flow comes in at the left (`inlet`),
    /// leaves at the right (`outlet`), the top and bottom are `symmetry` planes, and every body is
    /// the `body` patch.
    pub fn around(&mut self, tunnel: Tunnel) -> Result<Sketch> {
        for (name, v) in [
            ("upstream", tunnel.upstream),
            ("downstream", tunnel.downstream),
            ("lateral", tunnel.lateral),
        ] {
            if !(v.is_finite() && v > 0.0 && v <= 1000.0) {
                return Err(invalid(format!(
                    "the tunnel's {name} length is a multiple of the body between 0 and 1000, not {v}"
                )));
            }
        }
        let (lo, hi) = self
            .bounds()
            .ok_or_else(|| invalid("the drawing has nothing in it"))?;
        let size = (hi.0 - lo.0).max(hi.1 - lo.1);
        if !(size.is_finite() && size > 0.0) {
            return Err(invalid("the drawing has no size"));
        }
        self.fit(MAX_POINTS - TUNNEL_POINTS)?;
        let (x0, x1) = (
            lo.0 - tunnel.upstream * size,
            hi.0 + tunnel.downstream * size,
        );
        let (y0, y1) = (lo.1 - tunnel.lateral * size, hi.1 + tunnel.lateral * size);
        let mut sketch = Sketch::default();
        let corners = [
            sketch.point(x0, y0),
            sketch.point(x1, y0),
            sketch.point(x1, y1),
            sketch.point(x0, y1),
        ];
        for (i, name) in ["symmetry", "outlet", "symmetry", "inlet"]
            .into_iter()
            .enumerate()
        {
            let (Some(&a), Some(&b)) = (corners.get(i), corners.get((i + 1) % 4)) else {
                continue;
            };
            let e = sketch.line(a, b);
            sketch.name(e, name);
        }
        self.add_to(&mut sketch, |_| BODY_PATCH.to_owned())?;
        sketch.validate().map_err(|e| invalid(e.to_string()))?;
        Ok(sketch)
    }

    fn add_to(&self, sketch: &mut Sketch, name: impl Fn(&str) -> String) -> Result<()> {
        for outline in &self.outlines {
            match outline {
                Outline::Circle {
                    layer,
                    center,
                    radius,
                } => {
                    let c = sketch.point(center.0, center.1);
                    let rim = sketch.point(center.0 + radius, center.1);
                    sketch
                        .entities
                        .push(SketchEntity::Circle { center: c, rim });
                    sketch.name(sketch.entities.len() - 1, &name(layer));
                }
                Outline::Loop(edges) => {
                    let Some(start) = edges.first().and_then(|e| e.points.first()) else {
                        continue;
                    };
                    let first = sketch.point(start.0, start.1);
                    let mut previous = first;
                    for (i, edge) in edges.iter().enumerate() {
                        let n = edge.points.len();
                        for (k, p) in edge.points.iter().enumerate().skip(1) {
                            let closes = i + 1 == edges.len() && k + 1 == n;
                            let id = if closes {
                                first
                            } else {
                                sketch.point(p.0, p.1)
                            };
                            let e = sketch.line(previous, id);
                            sketch.name(e, &name(&edge.layer));
                            previous = id;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
