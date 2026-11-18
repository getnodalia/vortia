//! From triangles to prisms: each triangle of the region becomes a cell one layer thick, either
//! of a given thickness (a planar case, the two ends `empty`) or a thin wedge of a body of
//! revolution about the `x` axis (the two ends `wedge`, the axis collapsed).

use std::collections::VecDeque;

use vortia_core::Vec3;
use vortia_foamfile::{Dictionary, FaceList, RawPatch, RawPolyMesh};

use super::delaunay::{Dt, NONE, P};
use crate::error::{MeshError, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum Extrusion {
    /// A layer of this thickness in `z`, in the region's units.
    Planar { thickness: f64 },
    /// A wedge of this opening in degrees (0.01 to 5) about the `x` axis; `y` is the radius.
    Wedge { degrees: f64 },
}

impl Extrusion {
    pub fn validate(&self) -> Result<()> {
        match *self {
            Extrusion::Planar { thickness } if thickness.is_finite() && thickness > 0.0 => Ok(()),
            Extrusion::Planar { thickness } => Err(MeshError::Invalid(format!(
                "the thickness must be above zero, not {thickness}"
            ))),
            Extrusion::Wedge { degrees }
                if degrees.is_finite() && (0.01..=5.0).contains(&degrees) =>
            {
                Ok(())
            }
            Extrusion::Wedge { degrees } => Err(MeshError::Invalid(format!(
                "the wedge angle must be above 0.01° and at most 5°, not {degrees}"
            ))),
        }
    }
}

/// A polygon with the repeats next to each other taken out; `None` if nothing like a face is left.
fn face(mut ids: Vec<usize>) -> Option<Vec<usize>> {
    ids.dedup();
    while ids.len() > 1 && ids.first() == ids.last() {
        ids.pop();
    }
    (ids.len() >= 3).then_some(ids)
}

pub(super) fn prisms(
    dt: &Dt,
    patch_names: &[String],
    patch_kinds: &[String],
    extrusion: &Extrusion,
    to_world: impl Fn(P) -> P,
    tol: f64,
    max_cells: usize,
) -> Result<RawPolyMesh> {
    let inside: Vec<usize> = (0..dt.tris.len())
        .filter(|&t| {
            let tri = dt.tri(t);
            tri.alive && tri.inside
        })
        .collect();
    if inside.is_empty() {
        return Err(MeshError::Invalid(
            "the boundary does not enclose any area".into(),
        ));
    }
    if inside.len() > max_cells {
        return Err(MeshError::Invalid(format!(
            "the mesh would have {} cells, more than the {max_cells} allowed: make the size larger",
            inside.len()
        )));
    }

    // Number the cells breadth first from the left, so neighbours get near numbers.
    let centroid_x = |t: usize| {
        let tri = dt.tri(t);
        (0..3)
            .map(|i| dt.pts.get(tri.vertex(i)).map_or(0.0, |p| p[0]))
            .sum::<f64>()
    };
    let mut seeds = inside.clone();
    seeds.sort_by(|&a, &b| centroid_x(a).total_cmp(&centroid_x(b)));
    let mut cell_of = vec![NONE; dt.tris.len()];
    let mut queued = vec![false; dt.tris.len()];
    let mut order: Vec<usize> = Vec::with_capacity(inside.len());
    for seed in seeds {
        if queued.get(seed).copied() != Some(false) {
            continue;
        }
        if let Some(q) = queued.get_mut(seed) {
            *q = true;
        }
        let mut queue = VecDeque::from([seed]);
        while let Some(t) = queue.pop_front() {
            if let Some(c) = cell_of.get_mut(t) {
                *c = order.len();
            }
            order.push(t);
            let tri = dt.tri(t);
            for i in 0..3 {
                let nb = tri.neighbour(i);
                if nb == NONE || !dt.tri(nb).inside {
                    continue;
                }
                if queued.get(nb).copied() == Some(false) {
                    if let Some(q) = queued.get_mut(nb) {
                        *q = true;
                    }
                    queue.push_back(nb);
                }
            }
        }
    }

    // The points: a back and a front for each vertex in use, one for a vertex on the axis.
    let wedge = matches!(extrusion, Extrusion::Wedge { .. });
    let mut vertex_of = vec![NONE; dt.pts.len()];
    let (mut back, mut front): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut points: Vec<Vec3> = Vec::new();
    for &t in &order {
        let tri = dt.tri(t);
        for i in 0..3 {
            let v = tri.vertex(i);
            if vertex_of.get(v).copied() != Some(NONE) {
                continue;
            }
            let [x, y] = to_world(dt.pts.get(v).copied().unwrap_or([0.0, 0.0]));
            let index = back.len();
            if let Some(slot) = vertex_of.get_mut(v) {
                *slot = index;
            }
            match *extrusion {
                Extrusion::Planar { thickness } => {
                    points.push(Vec3::new(x, y, 0.0));
                    back.push(points.len() - 1);
                    points.push(Vec3::new(x, y, thickness));
                    front.push(points.len() - 1);
                }
                Extrusion::Wedge { degrees } => {
                    if y.abs() <= tol {
                        points.push(Vec3::new(x, 0.0, 0.0));
                        back.push(points.len() - 1);
                        front.push(points.len() - 1);
                    } else {
                        let half = degrees.to_radians() / 2.0;
                        let (r, z) = (y * half.cos(), y * half.sin());
                        points.push(Vec3::new(x, r, -z));
                        back.push(points.len() - 1);
                        points.push(Vec3::new(x, r, z));
                        front.push(points.len() - 1);
                    }
                }
            }
        }
    }
    let side = |a: usize, b: usize| {
        let (a, b) = (
            vertex_of.get(a).copied().unwrap_or(NONE),
            vertex_of.get(b).copied().unwrap_or(NONE),
        );
        let get = |list: &Vec<usize>, i: usize| list.get(i).copied().unwrap_or(0);
        face(vec![
            get(&back, a),
            get(&back, b),
            get(&front, b),
            get(&front, a),
        ])
    };

    // The side faces: between two cells (the lower number owns it) or on the boundary.
    let mut internal: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut boundary: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); patch_names.len()];
    for (c, &t) in order.iter().enumerate() {
        let tri = dt.tri(t);
        for i in 0..3 {
            let (a, b) = tri.edge(i);
            let nb = tri.neighbour(i);
            let k = if a < b { (a, b) } else { (b, a) };
            if let Some(&patch) = dt.segs.get(&k) {
                if let Some(list) = boundary.get_mut(patch) {
                    list.push((c, a, b));
                }
            } else if nb != NONE {
                let n = cell_of.get(nb).copied().unwrap_or(NONE);
                if n != NONE && n > c {
                    internal.push((c, n, a, b));
                }
            }
        }
    }
    internal.sort_by_key(|&(c, n, _, _)| (c, n));

    let mut faces = FaceList {
        offsets: vec![0],
        points: Vec::new(),
    };
    let mut owner = Vec::new();
    let mut neighbour = Vec::new();
    let add = |faces: &mut FaceList, f: &[usize]| {
        faces.points.extend_from_slice(f);
        faces.offsets.push(faces.points.len());
    };
    for &(c, n, a, b) in &internal {
        if let Some(f) = side(a, b) {
            add(&mut faces, &f);
            owner.push(c);
            neighbour.push(n);
        }
    }
    let mut patches = Vec::new();
    let mut make = |name: &str, kind: &str, start: usize, owner: &Vec<usize>| {
        patches.push(RawPatch {
            name: name.to_owned(),
            kind: kind.to_owned(),
            start_face: start,
            n_faces: owner.len() - start,
            groups: Vec::new(),
            dict: Dictionary::new(),
        });
    };
    for ((name, kind), list) in patch_names.iter().zip(patch_kinds).zip(&boundary) {
        let start = owner.len();
        let mut sorted = list.clone();
        sorted.sort_by_key(|&(c, _, _)| c);
        for &(c, a, b) in &sorted {
            if let Some(f) = side(a, b) {
                add(&mut faces, &f);
                owner.push(c);
            }
        }
        if owner.len() > start {
            make(name, kind, start, &owner);
        }
    }
    let corner = |list: &Vec<usize>, tri: &super::delaunay::Tri, i: usize| {
        let v = vertex_of.get(tri.vertex(i)).copied().unwrap_or(NONE);
        list.get(v).copied().unwrap_or(0)
    };
    let ends = |front_side: bool, faces: &mut FaceList, owner: &mut Vec<usize>| {
        for (c, &t) in order.iter().enumerate() {
            let tri = dt.tri(t);
            let f = if front_side {
                vec![
                    corner(&front, &tri, 0),
                    corner(&front, &tri, 1),
                    corner(&front, &tri, 2),
                ]
            } else {
                vec![
                    corner(&back, &tri, 0),
                    corner(&back, &tri, 2),
                    corner(&back, &tri, 1),
                ]
            };
            faces.points.extend_from_slice(&f);
            faces.offsets.push(faces.points.len());
            owner.push(c);
        }
    };
    if wedge {
        let start = owner.len();
        ends(true, &mut faces, &mut owner);
        make("wedgeFront", "wedge", start, &owner);
        let start = owner.len();
        ends(false, &mut faces, &mut owner);
        make("wedgeBack", "wedge", start, &owner);
    } else {
        let start = owner.len();
        ends(true, &mut faces, &mut owner);
        ends(false, &mut faces, &mut owner);
        make("frontAndBack", "empty", start, &owner);
    }

    Ok(RawPolyMesh {
        points,
        faces,
        owner,
        neighbour,
        patches,
        cell_zones: Vec::new(),
        face_zones: Vec::new(),
    })
}
