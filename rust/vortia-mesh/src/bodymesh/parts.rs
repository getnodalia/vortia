// SPDX-License-Identifier: GPL-3.0-or-later
//! The wall by parts: a surface made of named pieces (the objects of an OBJ, the solids of an STL
//! or a STEP file) gets one wall patch for each, so the force on a wing, on a wheel or on a
//! body can be read apart from the rest.
//!
//! Each face of the wall takes the part of the triangle of the surface nearest to its centre.
//! The wall faces are reordered so that every part's faces are together, and the wall patch is
//! replaced by one patch for each part (`<body>_<part>`). Pieces whose names come out the same
//! are one patch.

use vortia_foamfile::{FaceList, RawPatch, RawPolyMesh};

use super::BodySpec;
use super::grid::{TriGrid, at};
use crate::error::{MeshError, Result};
use crate::stl::Stl;

/// The most wall patches a body is split into.
pub const MAX_PARTS: usize = 256;

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// A part's name as a patch name: letters, digits and underscores only.
pub fn patch_name(body: &str, part: &str) -> String {
    let clean: String = part
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let clean = clean.trim_matches('_');
    if clean.is_empty() {
        format!("{body}_part")
    } else {
        format!("{body}_{clean}")
    }
}

/// The patch names the parts of `surface` would give, in the order they first appear, or `None`
/// when the surface has fewer than two parts, its parts do not cover its triangles, or there are
/// more than [`MAX_PARTS`].
pub fn part_patches(surface: &Stl, body: &str) -> Option<Vec<String>> {
    surface.part_of_each()?;
    let mut names: Vec<String> = Vec::new();
    for part in &surface.parts {
        let name = patch_name(body, &part.name);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    (names.len() > 1 && names.len() <= MAX_PARTS).then_some(names)
}

/// Splits the wall patch of `mesh` by the parts of `surface`; returns each new patch's name and
/// face count, in patch order. Parts that no wall face is nearest to give no patch.
pub(super) fn split_wall(
    mesh: &mut RawPolyMesh,
    grid: &TriGrid,
    surface: &Stl,
    spec: &BodySpec,
) -> Result<Vec<(String, usize)>> {
    let Some(names) = part_patches(surface, &spec.body) else {
        return invalid(
            "the surface cannot be split by part: it needs two to 256 named parts that cover \
             every triangle",
        );
    };
    let Some(of_triangle) = surface.part_of_each() else {
        return invalid("the parts of the surface do not cover its triangles");
    };
    let name_of_part: Vec<usize> = surface
        .parts
        .iter()
        .map(|part| {
            let name = patch_name(&spec.body, &part.name);
            names.iter().position(|n| *n == name).unwrap_or(0)
        })
        .collect();
    let Some(wall) = mesh.patches.iter().position(|p| p.name == spec.body) else {
        return invalid(format!("the mesh has no patch '{}' to split", spec.body));
    };
    let (start, count) = match mesh.patches.get(wall) {
        Some(p) => (p.start_face, p.n_faces),
        None => return invalid("the wall patch is missing"),
    };
    if start + count > mesh.faces.len() {
        return invalid("the wall patch runs past the faces of the mesh");
    }
    let reach = (0..3)
        .map(|a| (at(&spec.high, a) - at(&spec.low, a)).powi(2))
        .sum::<f64>()
        .sqrt();

    let mut scratch = Vec::new();
    let which: Vec<usize> = (start..start + count)
        .map(|face| {
            let ring = mesh.faces.face(face);
            let mut centre = [0.0; 3];
            for &p in ring {
                if let Some(v) = mesh.points.get(p) {
                    centre[0] += v.x;
                    centre[1] += v.y;
                    centre[2] += v.z;
                }
            }
            let n = ring.len().max(1) as f64;
            let centre = centre.map(|c| c / n);
            grid.nearest_triangle(centre, reach, &mut scratch)
                .and_then(|(t, _)| of_triangle.get(t))
                .and_then(|&part| name_of_part.get(part))
                .copied()
                .unwrap_or(0)
        })
        .collect();

    // The faces of one part together, each part's faces keeping their order.
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&k| which.get(k).copied().unwrap_or(0));
    let mut faces = FaceList {
        offsets: vec![0],
        points: Vec::with_capacity(mesh.faces.points.len()),
    };
    let mut owner = Vec::with_capacity(mesh.owner.len());
    let mut push = |face: usize, mesh: &RawPolyMesh| {
        faces.points.extend_from_slice(mesh.faces.face(face));
        faces.offsets.push(faces.points.len());
        owner.push(mesh.owner.get(face).copied().unwrap_or(0));
    };
    for face in 0..start {
        push(face, mesh);
    }
    for &k in &order {
        push(start + k, mesh);
    }
    for face in start + count..mesh.faces.len() {
        push(face, mesh);
    }
    let mut per_part = vec![0usize; names.len()];
    for &w in &which {
        if let Some(n) = per_part.get_mut(w) {
            *n += 1;
        }
    }
    mesh.faces = faces;
    mesh.owner = owner;

    let Some(template) = mesh.patches.get(wall).cloned() else {
        return invalid("the wall patch is missing");
    };
    let mut replacement = Vec::new();
    let mut first = start;
    for (name, &n) in names.iter().zip(&per_part) {
        if n == 0 {
            continue;
        }
        replacement.push(RawPatch {
            name: name.clone(),
            start_face: first,
            n_faces: n,
            ..template.clone()
        });
        first += n;
    }
    let result = replacement
        .iter()
        .map(|p| (p.name.clone(), p.n_faces))
        .collect();
    mesh.patches.splice(wall..=wall, replacement);
    Ok(result)
}
