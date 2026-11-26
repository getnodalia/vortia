// SPDX-License-Identifier: GPL-3.0-or-later
//! STEP solids as a triangle surface.
//!
//! Every solid of the file is read into its B-rep record, meshed with its faces joined at their
//! shared edges (so a closed solid gives a closed surface), and put in metres. A file of several
//! solids gives one surface of all of them; it is only a body to mesh around if they do not
//! overlap.

use std::path::Path;

use solidcore_step::{p21, topology};
use solidcore_tess::{MeshTolerance, plan};
use vortia_mesh::stl::Stl;
use vortia_mesh::{MeshError, Result};

/// The biggest STEP file read (bytes).
pub const MAX_BYTES: u64 = 256 << 20;
/// The most triangles a surface may have.
pub const MAX_TRIANGLES: usize = 20_000_000;
/// The chord tolerance, as a share of the body's box diagonal, when none is asked for.
pub const DEFAULT_CHORD: f64 = 1.0 / 1000.0;

/// What was read.
#[derive(Clone, Debug, PartialEq)]
pub struct StepSurface {
    pub surface: Stl,
    /// The solids in the surface.
    pub solids: usize,
    /// What went wrong in the solids that could not be used, and faces left out.
    pub notes: Vec<String>,
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// Reads and tessellates the STEP file at `path` (chord tolerance [`DEFAULT_CHORD`] of the
/// body's size).
pub fn read(path: &Path) -> Result<StepSurface> {
    let size = std::fs::metadata(path)
        .map_err(|e| MeshError::Invalid(format!("could not read {}: {e}", path.display())))?
        .len();
    if size > MAX_BYTES {
        return invalid(format!(
            "{} is {} MB, over the {} MB a STEP file may have",
            path.display(),
            size >> 20,
            MAX_BYTES >> 20
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| MeshError::Invalid(format!("could not read {}: {e}", path.display())))?;
    parse(&bytes, DEFAULT_CHORD)
}

/// Reads and tessellates STEP text. `chord` is the farthest a facet may stand off the exact
/// surface, as a share of the body's box diagonal.
pub fn parse(bytes: &[u8], chord: f64) -> Result<StepSurface> {
    if !chord.is_finite() || chord <= 0.0 || chord > 0.5 {
        return invalid(
            "the STEP chord tolerance is a share of the body's size, above 0 and at most 0.5",
        );
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| MeshError::Invalid("the STEP file is not UTF-8 text".into()))?;
    let exchange =
        p21::parse(text).map_err(|e| MeshError::Invalid(format!("not a STEP file: {e}")))?;
    let read = topology::read_solids(&exchange);
    let mut notes: Vec<String> = read
        .failures
        .iter()
        .map(|f| format!("a solid could not be read: {f:?}"))
        .collect();
    if read.solids.is_empty() {
        return invalid(format!(
            "the STEP file has no solid to read{}",
            notes.first().map_or(String::new(), |n| format!(" ({n})"))
        ));
    }

    let mut out = Stl {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for solid in &read.solids {
        // The first plan is coarse and only gives the body's size; the second is the one used.
        let coarse = plan(&solid.brep, &MeshTolerance::chord(1.0e6), 1.0)
            .map_err(|e| MeshError::Invalid(format!("could not mesh {}: {e}", solid.name)))?
            .0
            .welded()
            .map_err(|e| MeshError::Invalid(format!("could not join {}: {e}", solid.name)))?
            .0;
        let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in &coarse.positions {
            for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                if let Some(l) = low.get_mut(k) {
                    *l = l.min(c);
                }
                if let Some(h) = high.get_mut(k) {
                    *h = h.max(c);
                }
            }
        }
        let diagonal = low
            .iter()
            .zip(&high)
            .map(|(l, h)| (h - l) * (h - l))
            .sum::<f64>()
            .sqrt();
        if !diagonal.is_finite() || diagonal <= 0.0 {
            notes.push(format!("{} has no size", solid.name));
            continue;
        }
        let (mesh_plan, stats) = plan(
            &solid.brep,
            &MeshTolerance::chord(chord * diagonal),
            diagonal,
        )
        .map_err(|e| MeshError::Invalid(format!("could not mesh {}: {e}", solid.name)))?;
        let (welded, refused) = mesh_plan
            .welded()
            .map_err(|e| MeshError::Invalid(format!("could not join {}: {e}", solid.name)))?;
        if !refused.is_empty() {
            notes.push(format!(
                "{}: {} faces could not be meshed",
                solid.name,
                refused.len()
            ));
        }
        let _ = stats;
        let base = u32::try_from(out.vertices.len())
            .map_err(|_| MeshError::Invalid("the surface has too many vertices".into()))?;
        // STEP lengths are read in millimetres; Vortia works in metres.
        out.vertices.extend(
            welded
                .positions
                .iter()
                .map(|p| [p.x * 1.0e-3, p.y * 1.0e-3, p.z * 1.0e-3]),
        );
        out.triangles
            .extend(welded.triangles.iter().map(|t| t.map(|i| i + base)));
        if out.triangles.len() > MAX_TRIANGLES {
            return invalid("the surface has more triangles than the mesher takes");
        }
    }
    if out.triangles.is_empty() {
        return invalid("the STEP file gave no triangles");
    }
    Ok(StepSurface {
        surface: out,
        solids: read.solids.len(),
        notes,
    })
}
