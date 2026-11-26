//! From the leaves of the octree to a polyhedral mesh: which cells are in the air, the faces
//! between them with the nodes where a coarse face meets finer ones, and the patches.

use std::collections::{HashMap, HashSet, VecDeque};

use vortia_core::Vec3;
use vortia_foamfile::{Dictionary, FaceList, RawPatch, RawPolyMesh};

use super::grid::{TriGrid, at, put};
use super::octree::{Key, Octree};
use super::{BodySpec, BodyStats};
use crate::error::{MeshError, Result};

/// A face before it has its nodes: a rectangle in the lattice, with the cells on its two sides.
struct Face {
    axis: usize,
    plane: i64,
    u: [i64; 2],
    v: [i64; 2],
    /// The cell on the negative side and the one on the positive side, in the order of the
    /// kept cells; `None` where there is none (the domain edge) or it is not kept.
    lo: Option<usize>,
    hi: Option<usize>,
    /// 0 is the body, 1 to 6 the sides; `None` for an internal face.
    patch: Option<usize>,
}

fn origin(tree: &Octree, k: Key) -> ([i64; 3], i64) {
    let size = 1i64 << (tree.lattice.max_level - k.level);
    let mut o = [0i64; 3];
    for a in 0..3 {
        put(&mut o, a, i64::from(at(&k.at, a)) * size);
    }
    (o, size)
}

/// Which leaves are in the air: outside the body and joined to the far field.
fn air(tree: &Octree, grid: &TriGrid, leaves: &[Key]) -> Result<Vec<bool>> {
    let mut scratch = Vec::new();
    let index: HashMap<Key, usize> = leaves.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let cut: Vec<bool> = leaves
        .iter()
        .map(|&k| {
            let (lo, hi) = tree.lattice.cell_box(k);
            grid.touches(lo, hi, &mut scratch)
        })
        .collect();
    let centre = |k: Key| {
        let (lo, hi) = tree.lattice.cell_box(k);
        [
            0.5 * (at(&lo, 0) + at(&hi, 0)),
            0.5 * (at(&lo, 1) + at(&hi, 1)),
            0.5 * (at(&lo, 2) + at(&hi, 2)),
        ]
    };

    // Cells the surface does not touch lie wholly on one side, and so does everything joined
    // to them without crossing a cut cell: one test per such region.
    let mut outside = vec![false; leaves.len()];
    let mut seen = vec![false; leaves.len()];
    for start in 0..leaves.len() {
        if seen.get(start).copied().unwrap_or(true) || cut.get(start).copied().unwrap_or(true) {
            continue;
        }
        let Some(&first) = leaves.get(start) else {
            continue;
        };
        let is_outside = !grid.inside(centre(first), &mut scratch);
        let mut queue = VecDeque::from([start]);
        if let Some(s) = seen.get_mut(start) {
            *s = true;
        }
        while let Some(i) = queue.pop_front() {
            if let Some(o) = outside.get_mut(i) {
                *o = is_outside;
            }
            let Some(&k) = leaves.get(i) else { continue };
            for axis in 0..3 {
                for sign in [-1i64, 1] {
                    for n in tree.across(k, axis, sign) {
                        let Some(&j) = index.get(&n) else { continue };
                        if !cut.get(j).copied().unwrap_or(true)
                            && let Some(s) = seen.get_mut(j)
                            && !*s
                        {
                            *s = true;
                            queue.push_back(j);
                        }
                    }
                }
            }
        }
    }
    // A cut cell is kept when its centre is in the air.
    let candidate: Vec<bool> = leaves
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            if cut.get(i).copied().unwrap_or(false) {
                !grid.inside(centre(k), &mut scratch)
            } else {
                outside.get(i).copied().unwrap_or(false)
            }
        })
        .collect();

    // Only the air joined to the far field is kept: a pocket shut inside a body is dropped.
    let seed = tree
        .covering(tree.lattice.max_level, [0, 0, 0])
        .and_then(|k| index.get(&k).copied())
        .filter(|&i| candidate.get(i).copied().unwrap_or(false))
        .ok_or_else(|| {
            MeshError::Invalid(
                "the corner of the domain is inside the body: the domain must surround it".into(),
            )
        })?;
    let mut kept = vec![false; leaves.len()];
    let mut queue = VecDeque::from([seed]);
    if let Some(s) = kept.get_mut(seed) {
        *s = true;
    }
    while let Some(i) = queue.pop_front() {
        let Some(&k) = leaves.get(i) else { continue };
        for axis in 0..3 {
            for sign in [-1i64, 1] {
                for n in tree.across(k, axis, sign) {
                    let Some(&j) = index.get(&n) else { continue };
                    if candidate.get(j).copied().unwrap_or(false)
                        && let Some(s) = kept.get_mut(j)
                        && !*s
                    {
                        *s = true;
                        queue.push_back(j);
                    }
                }
            }
        }
    }
    Ok(kept)
}

/// Appends the nodes along the edge `a -> b` (without `b`): its end and every node of the
/// mesh that lies on it, found by halving.
fn edge_nodes(a: [i64; 3], b: [i64; 3], nodes: &HashSet<[i64; 3]>, out: &mut Vec<[i64; 3]>) {
    let mut length = 0;
    for k in 0..3 {
        length += (at(&b, k) - at(&a, k)).abs();
    }
    if length >= 2 {
        let mut m = [0i64; 3];
        for k in 0..3 {
            put(&mut m, k, (at(&a, k) + at(&b, k)) / 2);
        }
        if nodes.contains(&m) {
            edge_nodes(a, m, nodes, out);
            edge_nodes(m, b, nodes, out);
            return;
        }
    }
    out.push(a);
}

pub(super) fn assemble(
    tree: &Octree,
    grid: &TriGrid,
    spec: &BodySpec,
) -> Result<(RawPolyMesh, BodyStats)> {
    let mut leaves: Vec<Key> = tree.leaves.iter().copied().collect();
    leaves.sort_unstable();
    let kept = air(tree, grid, &leaves)?;

    // Cell numbers: the kept leaves in order of position, so neighbouring cells are near.
    let mut cells: Vec<Key> = leaves
        .iter()
        .zip(&kept)
        .filter(|(_, keep)| **keep)
        .map(|(&k, _)| k)
        .collect();
    cells.sort_by_key(|&k| {
        let (o, _) = origin(tree, k);
        (at(&o, 2), at(&o, 1), at(&o, 0), k.level)
    });
    let number: HashMap<Key, usize> = cells.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    if cells.is_empty() {
        return Err(MeshError::Invalid("no cell is left in the air".into()));
    }

    let mut faces: Vec<Face> = Vec::new();
    for &c in &leaves {
        let me = number.get(&c).copied();
        let (o, size) = origin(tree, c);
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for sign in [-1i64, 1] {
                let near = tree.across(c, axis, sign);
                let plane = at(&o, axis) + if sign > 0 { size } else { 0 };
                let rect = ([at(&o, u), at(&o, u) + size], [at(&o, v), at(&o, v) + size]);
                let side = |this: Option<usize>, other: Option<usize>| {
                    if sign > 0 {
                        (this, other)
                    } else {
                        (other, this)
                    }
                };
                let Some(&first) = near.first() else {
                    // The edge of the domain.
                    if me.is_some() {
                        let (lo, hi) = side(me, None);
                        let patch = 1 + axis * 2 + usize::from(sign > 0);
                        faces.push(Face {
                            axis,
                            plane,
                            u: rect.0,
                            v: rect.1,
                            lo,
                            hi,
                            patch: Some(patch),
                        });
                    }
                    continue;
                };
                if first.level > c.level {
                    // Finer neighbours make this face themselves.
                    continue;
                }
                if first.level == c.level && sign < 0 {
                    // Two cells of one size: made once, from the negative one.
                    continue;
                }
                let other = number.get(&first).copied();
                if me.is_none() && other.is_none() {
                    continue;
                }
                let (lo, hi) = side(me, other);
                faces.push(Face {
                    axis,
                    plane,
                    u: rect.0,
                    v: rect.1,
                    lo,
                    hi,
                    patch: (me.is_none() || other.is_none()).then_some(0),
                });
            }
        }
    }

    // Every corner a face has: where a coarse face meets finer ones, the finer corners lie on
    // the coarse edges and have to be in its outline too.
    let mut nodes: HashSet<[i64; 3]> = HashSet::new();
    let corners = |f: &Face| {
        let (u, v) = ((f.axis + 1) % 3, (f.axis + 2) % 3);
        [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(i, j)| {
            let mut p = [0i64; 3];
            put(&mut p, f.axis, f.plane);
            put(&mut p, u, f.u.get(i).copied().unwrap_or(0));
            put(&mut p, v, f.v.get(j).copied().unwrap_or(0));
            p
        })
    };
    for f in &faces {
        nodes.extend(corners(f));
    }

    // Order: internal faces by owner then neighbour, then each patch by owner.
    let owner_of = |f: &Face| match (f.lo, f.hi) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => usize::MAX,
    };
    let other_of = |f: &Face| match (f.lo, f.hi) {
        (Some(a), Some(b)) => a.max(b),
        _ => usize::MAX,
    };
    faces.sort_by_key(|f| (f.patch.map_or(0, |p| p + 1), owner_of(f), other_of(f)));

    let mut points: Vec<Vec3> = Vec::new();
    let mut ids: HashMap<[i64; 3], usize> = HashMap::new();
    let lattice_point = |p: [i64; 3]| {
        let units = (1i64 << tree.lattice.max_level) as f64;
        let mut x = [0.0; 3];
        for a in 0..3 {
            let size = at(&tree.lattice.base_size, a) / units;
            put(
                &mut x,
                a,
                at(&tree.lattice.low, a) + at(&p, a) as f64 * size,
            );
        }
        Vec3::new(x[0], x[1], x[2])
    };
    let mut list = FaceList {
        offsets: vec![0],
        points: Vec::new(),
    };
    let mut owner = Vec::with_capacity(faces.len());
    let mut neighbour = Vec::new();
    let mut counts = [0usize; 7];
    for f in &faces {
        let [p0, p1, p2, p3] = corners(f);
        let mut ring = Vec::new();
        edge_nodes(p0, p1, &nodes, &mut ring);
        edge_nodes(p1, p2, &nodes, &mut ring);
        edge_nodes(p2, p3, &nodes, &mut ring);
        edge_nodes(p3, p0, &nodes, &mut ring);
        // The loop runs so that its normal is along +axis, from the negative cell to the
        // positive one; it is reversed when the owner is the positive cell.
        let owner_cell = owner_of(f);
        if f.lo != Some(owner_cell) {
            ring.reverse();
        }
        for p in ring {
            let next = points.len();
            let id = *ids.entry(p).or_insert_with(|| {
                points.push(lattice_point(p));
                next
            });
            list.points.push(id);
        }
        list.offsets.push(list.points.len());
        owner.push(owner_cell);
        match f.patch {
            None => neighbour.push(other_of(f)),
            Some(p) => {
                if let Some(c) = counts.get_mut(p) {
                    *c += 1;
                }
            }
        }
    }

    let mut patches = Vec::new();
    let mut start = neighbour.len();
    for (p, &n) in counts.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let (name, kind) = if p == 0 {
            (spec.body.clone(), "wall".to_owned())
        } else {
            let side = spec.sides.get(p - 1).cloned().unwrap_or_default();
            (side.name, side.kind.foam_name().to_owned())
        };
        patches.push(RawPatch {
            name,
            kind,
            start_face: start,
            n_faces: n,
            groups: Vec::new(),
            dict: Dictionary::new(),
        });
        start += n;
    }
    let stats = BodyStats {
        leaves: leaves.len(),
        cells: cells.len(),
        faces: faces.len(),
        body_faces: counts.first().copied().unwrap_or(0),
        snapping: Default::default(),
    };
    Ok((
        RawPolyMesh {
            points,
            faces: list,
            owner,
            neighbour,
            patches,
            cell_zones: Vec::new(),
            face_zones: Vec::new(),
        },
        stats,
    ))
}
