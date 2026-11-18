//! Starting a case: what the Home page's "New" does.
//!
//! A case is chosen by its geometry (a box channel, a cavity, a flat plate, a planar nozzle or an
//! axisymmetric nozzle) and a handful of numbers. [`create`] writes the whole
//! folder in OpenFOAM's layout: `system/blockMeshDict`, the mesh made from it
//! by the engine's own `blockMesh`, the initial fields and the settings, so the
//! case opens and runs at once and every file stays editable by hand or by
//! OpenFOAM.
//!
//! The block meshes are generated here, not by an external mesher.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod cavity;
mod channel;
mod error;
mod files;
mod jet;
mod nozzle;
mod plate;

use std::path::Path;

pub use cavity::Cavity;
pub use channel::Channel;
pub use error::{CaseError, Result};
pub use jet::Jet;
pub use nozzle::{Nozzle, NozzleKind};
pub use plate::Plate;

/// The most cells a new case may have: the same ceiling the Qt application
/// puts on a block mesh.
pub const MAX_CELLS: u64 = 2_000_000;

/// What a case is made of.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A straight 2D channel: laminar incompressible flow from left to right.
    Channel(Channel),
    /// A converging-diverging nozzle: compressible flow, k-ω SST.
    Nozzle(Nozzle),
    /// A lid-driven square cavity: laminar incompressible flow, closed on every side.
    Cavity(Cavity),
    /// A flat plate in a uniform stream: a laminar boundary layer, steady and incompressible.
    Plate(Plate),
    /// A jet from an orifice into the open atmosphere: compressible flow, k-ω SST.
    Jet(Jet),
}

impl Geometry {
    /// What the case is called in a list.
    pub fn title(&self) -> &'static str {
        match self {
            Geometry::Channel(_) => "Channel",
            Geometry::Nozzle(n) => n.kind.title(),
            Geometry::Cavity(_) => "Cavity",
            Geometry::Plate(_) => "Flat plate",
            Geometry::Jet(_) => "Jet into the atmosphere",
        }
    }
}

/// What [`create`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub cells: usize,
    pub points: usize,
    /// The solver the case's `controlDict` names.
    pub application: &'static str,
}

/// Writes a new case into `dir`, which must not exist or must be empty.
pub fn create(dir: &Path, geometry: &Geometry) -> Result<Created> {
    if dir.exists() && !files::is_empty_dir(dir) {
        return Err(CaseError::Invalid(format!(
            "'{}' already exists and is not empty",
            dir.display()
        )));
    }
    let set = match geometry {
        Geometry::Channel(c) => c.files()?,
        Geometry::Nozzle(n) => n.files()?,
        Geometry::Cavity(c) => c.files()?,
        Geometry::Plate(p) => p.files()?,
        Geometry::Jet(j) => j.files()?,
    };
    files::write_all(dir, &set.files)?;
    let mesh = files::mesh(dir, &set.block_mesh_dict)?;
    Ok(Created {
        cells: mesh.cells,
        points: mesh.points,
        application: set.application,
    })
}

/// A finite, positive size no larger than `limit`.
pub(crate) fn size(name: &str, value: f64, limit: f64) -> Result<f64> {
    if value.is_finite() && value > 0.0 && value <= limit {
        Ok(value)
    } else {
        Err(CaseError::Invalid(format!(
            "{name} must be a positive number up to {limit}, not {value}"
        )))
    }
}

/// A count between `min` and `max`.
pub(crate) fn count(name: &str, value: usize, min: usize, max: usize) -> Result<usize> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(CaseError::Invalid(format!(
            "{name} must be between {min} and {max}, not {value}"
        )))
    }
}

/// A whole number from a value that was typed as a number.
pub(crate) fn whole(name: &str, value: f64) -> Result<usize> {
    if value >= 0.0 && value.fract() == 0.0 && value <= 1.0e9 {
        Ok(value as usize)
    } else {
        Err(CaseError::Invalid(format!(
            "{name} must be a whole number, not {value}"
        )))
    }
}

pub(crate) fn unknown(name: &str, of: &str) -> CaseError {
    CaseError::Invalid(format!("'{name}' is not a setting of {of}"))
}

#[cfg(test)]
mod tests;
