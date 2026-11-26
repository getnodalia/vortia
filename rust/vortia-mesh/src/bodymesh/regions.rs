// SPDX-License-Identifier: MIT OR Apache-2.0
//! A mesh of two regions cut into one mesh for each, with the faces between them as a patch on
//! both sides.
//!
//! The octree meshes the air and the body together so that the wall can be snapped onto the
//! surface once, with the cells on both sides following it. Here that mesh is split: the air
//! cells make the fluid, the body's cells the solid, and each face that had a cell of each side
//! becomes a face of the patch `interface` of both, with its normal out of the region it is
//! seen from. Both patches list those faces in the same order, so face `i` of one is face `i` of
//! the other.

use std::collections::HashMap;

use vortia_core::Vec3;
use vortia_foamfile::{Dictionary, FaceList, RawPatch, RawPolyMesh};

use crate::error::{MeshError, Result};

/// The faces of a patch: ring (old point numbers) and owner (new cell number).
type PatchFaces = Vec<(Vec<usize>, usize)>;

/// A region being built: faces by the old point numbers, which are renumbered at the end.
#[derive(Default)]
struct Part {
    /// Internal faces: ring, owner and neighbour (new cell numbers).
    inner: Vec<(Vec<usize>, usize, usize)>,
    /// The interface: ring and owner.
    interface: PatchFaces,
    /// The patches of the first mesh that have faces here, in order: the patch's index, then
    /// its faces.
    sides: Vec<(usize, PatchFaces)>,
}

impl Part {
    fn side(&mut self, patch: usize, ring: Vec<usize>, owner: usize) {
        if let Some((_, faces)) = self.sides.iter_mut().find(|(p, _)| *p == patch) {
            faces.push((ring, owner));
        } else {
            self.sides.push((patch, vec![(ring, owner)]));
        }
    }

    fn finish(self, mesh: &RawPolyMesh, interface: &str, n_cells: usize) -> Result<RawPolyMesh> {
        let mut used: Vec<usize> = self
            .inner
            .iter()
            .map(|(r, _, _)| r)
            .chain(self.interface.iter().map(|(r, _)| r))
            .chain(
                self.sides
                    .iter()
                    .flat_map(|(_, f)| f.iter().map(|(r, _)| r)),
            )
            .flatten()
            .copied()
            .collect();
        used.sort_unstable();
        used.dedup();
        let number: HashMap<usize, usize> = used
            .iter()
            .enumerate()
            .map(|(new, &old)| (old, new))
            .collect();
        let points: Vec<Vec3> = used
            .iter()
            .map(|&old| {
                mesh.points.get(old).copied().ok_or_else(|| {
                    MeshError::Invalid("a face names a point that is not there".into())
                })
            })
            .collect::<Result<_>>()?;

        let mut list = FaceList {
            offsets: vec![0],
            points: Vec::new(),
        };
        let mut owner = Vec::new();
        let mut neighbour = Vec::new();
        let mut push = |ring: &[usize], own: usize| {
            list.points
                .extend(ring.iter().filter_map(|p| number.get(p).copied()));
            list.offsets.push(list.points.len());
            owner.push(own);
        };
        for (ring, own, nei) in &self.inner {
            push(ring, *own);
            neighbour.push(*nei);
        }
        let mut patches = Vec::new();
        let mut start = neighbour.len();
        if !self.interface.is_empty() {
            for (ring, own) in &self.interface {
                push(ring, *own);
            }
            patches.push(RawPatch {
                name: interface.to_owned(),
                kind: "wall".into(),
                start_face: start,
                n_faces: self.interface.len(),
                groups: Vec::new(),
                dict: Dictionary::new(),
            });
            start += self.interface.len();
        }
        for (index, faces) in &self.sides {
            let Some(old) = mesh.patches.get(*index) else {
                continue;
            };
            for (ring, own) in faces {
                push(ring, *own);
            }
            patches.push(RawPatch {
                name: old.name.clone(),
                kind: old.kind.clone(),
                start_face: start,
                n_faces: faces.len(),
                groups: old.groups.clone(),
                dict: old.dict.clone(),
            });
            start += faces.len();
        }
        if n_cells == 0 {
            return Err(MeshError::Invalid("a region has no cells".into()));
        }
        Ok(RawPolyMesh {
            points,
            faces: list,
            owner,
            neighbour,
            patches,
            cell_zones: Vec::new(),
            face_zones: Vec::new(),
        })
    }
}

/// The faces of `mesh` that have a cell of each region.
pub(super) fn wall_faces(mesh: &RawPolyMesh, solid: &[bool]) -> Vec<usize> {
    let region = |c: usize| solid.get(c).copied().unwrap_or(false);
    mesh.neighbour
        .iter()
        .enumerate()
        .filter(|&(f, &n)| mesh.owner.get(f).is_some_and(|&o| region(o) != region(n)))
        .map(|(f, _)| f)
        .collect()
}

/// Cuts `mesh` into the mesh of the cells where `solid` is false (the fluid) and the one where
/// it is true. The faces between the two are the patch `interface` of each.
pub(super) fn split(
    mesh: &RawPolyMesh,
    solid: &[bool],
    interface: &str,
) -> Result<(RawPolyMesh, RawPolyMesh)> {
    let region = |c: usize| solid.get(c).copied().unwrap_or(false);
    let mut number = vec![0usize; solid.len()];
    let mut counts = [0usize; 2];
    for (c, n) in number.iter_mut().enumerate() {
        let r = usize::from(region(c));
        *n = counts.get(r).copied().unwrap_or(0);
        if let Some(k) = counts.get_mut(r) {
            *k += 1;
        }
    }
    let cell = |c: usize| number.get(c).copied().unwrap_or(0);
    let mut parts = [Part::default(), Part::default()];

    let n_internal = mesh.neighbour.len();
    let mut patch_of = vec![0usize; mesh.owner.len().saturating_sub(n_internal)];
    for (pi, p) in mesh.patches.iter().enumerate() {
        for f in p.start_face..p.start_face + p.n_faces {
            if let Some(x) = f.checked_sub(n_internal).and_then(|i| patch_of.get_mut(i)) {
                *x = pi;
            }
        }
    }
    for (f, &own) in mesh.owner.iter().enumerate() {
        let ring = mesh.faces.face(f).to_vec();
        let Some(&nei) = mesh.neighbour.get(f) else {
            let patch = f
                .checked_sub(n_internal)
                .and_then(|i| patch_of.get(i))
                .copied()
                .unwrap_or(0);
            let r = usize::from(region(own));
            if let Some(p) = parts.get_mut(r) {
                p.side(patch, ring, cell(own));
            }
            continue;
        };
        if region(own) == region(nei) {
            let r = usize::from(region(own));
            if let Some(p) = parts.get_mut(r) {
                p.inner.push((ring, cell(own), cell(nei)));
            }
            continue;
        }
        // A face between the two regions: each sees it from its own cell, with the normal out.
        let mut reversed = ring.clone();
        reversed.reverse();
        let (fluid_cell, solid_cell, fluid_ring, solid_ring) = if region(own) {
            (nei, own, reversed, ring)
        } else {
            (own, nei, ring, reversed)
        };
        if let Some(p) = parts.first_mut() {
            p.interface.push((fluid_ring, cell(fluid_cell)));
        }
        if let Some(p) = parts.get_mut(1) {
            p.interface.push((solid_ring, cell(solid_cell)));
        }
    }
    let [fluid, body] = parts;
    Ok((
        fluid.finish(mesh, interface, counts.first().copied().unwrap_or(0))?,
        body.finish(mesh, interface, counts.get(1).copied().unwrap_or(0))?,
    ))
}

/// How the faces of the wall of one mesh are the faces of the interface of another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceMatch {
    /// For each face of the wall patches given (in the order they are given, patch after
    /// patch), the face of the interface patch it is.
    pub solid_face: Vec<usize>,
}

/// A mesh whose patch faces can be looked at, read from a case or being built.
pub trait FaceMesh {
    fn mesh_points(&self) -> &[Vec3];
    fn mesh_face(&self, face: usize) -> &[usize];
    /// The faces of the patch called `name`.
    fn mesh_patch(&self, name: &str) -> Option<std::ops::Range<usize>>;
}

impl FaceMesh for RawPolyMesh {
    fn mesh_points(&self) -> &[Vec3] {
        &self.points
    }
    fn mesh_face(&self, face: usize) -> &[usize] {
        self.faces.face(face)
    }
    fn mesh_patch(&self, name: &str) -> Option<std::ops::Range<usize>> {
        self.patches
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.start_face..p.start_face + p.n_faces)
    }
}

impl FaceMesh for crate::PolyMesh {
    fn mesh_points(&self) -> &[Vec3] {
        self.points()
    }
    fn mesh_face(&self, face: usize) -> &[usize] {
        self.face(face)
    }
    fn mesh_patch(&self, name: &str) -> Option<std::ops::Range<usize>> {
        self.patch(name).map(|p| p.faces())
    }
}

fn centre(mesh: &impl FaceMesh, face: usize) -> [f64; 3] {
    let ring = mesh.mesh_face(face);
    let mut m = [0.0; 3];
    for p in ring {
        if let Some(v) = mesh.mesh_points().get(*p) {
            m = [m[0] + v.x, m[1] + v.y, m[2] + v.z];
        }
    }
    let n = ring.len().max(1) as f64;
    [m[0] / n, m[1] / n, m[2] / n]
}

fn patch_faces(mesh: &impl FaceMesh, name: &str) -> Result<std::ops::Range<usize>> {
    mesh.mesh_patch(name)
        .ok_or_else(|| MeshError::Invalid(format!("the mesh has no patch '{name}'")))
}

/// Pairs each face of the patches `walls` of `fluid` with the face of `interface` of `solid` that
/// covers the same place. Both are the same faces when the two were cut from one mesh, so a face
/// is taken to be the one whose centre is within a millionth of the face's size; a face with no
/// partner, or two faces on one, is an error saying where.
pub fn match_interface(
    fluid: &impl FaceMesh,
    walls: &[&str],
    solid: &impl FaceMesh,
    interface: &str,
) -> Result<InterfaceMatch> {
    let theirs = patch_faces(solid, interface)?;
    let mut size = 0.0f64;
    let mut all = Vec::new();
    for f in theirs.clone() {
        let c = centre(solid, f);
        let ring = solid.mesh_face(f);
        let reach = ring
            .iter()
            .filter_map(|&p| solid.mesh_points().get(p))
            .map(|v| ((v.x - c[0]).powi(2) + (v.y - c[1]).powi(2) + (v.z - c[2]).powi(2)).sqrt())
            .fold(0.0, f64::max);
        size = size.max(reach);
        all.push((f, c));
    }
    if all.is_empty() || !(size.is_finite() && size > 0.0) {
        return Err(MeshError::Invalid(format!(
            "the patch '{interface}' has no faces to match"
        )));
    }
    // A hash of the centres on a grid of the size of the largest face: a partner is in the
    // cell of the face or one of its neighbours.
    let key = |c: [f64; 3]| c.map(|x| (x / size).floor() as i64);
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (i, (_, c)) in all.iter().enumerate() {
        grid.entry(key(*c)).or_default().push(i);
    }
    let mut taken = vec![false; all.len()];
    let mut out = Vec::new();
    for name in walls {
        for f in patch_faces(fluid, name)? {
            let c = centre(fluid, f);
            let k = key(c);
            let mut best: Option<(usize, f64)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let cell = [k[0] + dx, k[1] + dy, k[2] + dz];
                        for &i in grid.get(&cell).map(Vec::as_slice).unwrap_or_default() {
                            let Some((_, o)) = all.get(i) else { continue };
                            let d = ((o[0] - c[0]).powi(2)
                                + (o[1] - c[1]).powi(2)
                                + (o[2] - c[2]).powi(2))
                            .sqrt();
                            if best.is_none_or(|(_, b)| d < b) {
                                best = Some((i, d));
                            }
                        }
                    }
                }
            }
            let near = best.filter(|&(_, d)| d <= 1e-6 * size);
            let Some((i, _)) = near else {
                return Err(MeshError::Invalid(format!(
                    "face {f} of the wall '{name}' (at {:.6}, {:.6}, {:.6}) has no face of '{interface}' on it",
                    c[0], c[1], c[2]
                )));
            };
            if taken.get(i).copied().unwrap_or(true) {
                return Err(MeshError::Invalid(format!(
                    "two faces of the wall '{name}' meet one face of '{interface}'"
                )));
            }
            if let Some(t) = taken.get_mut(i) {
                *t = true;
            }
            out.push(all.get(i).map_or(0, |&(f, _)| f - theirs.start));
        }
    }
    if out.len() != all.len() {
        return Err(MeshError::Invalid(format!(
            "the walls have {} faces and '{interface}' has {}",
            out.len(),
            all.len()
        )));
    }
    Ok(InterfaceMatch { solid_face: out })
}
