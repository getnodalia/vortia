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

mod airfoil;
mod body;
mod cavity;
mod channel;
mod drawn;
mod error;
mod files;
mod heating;
mod jet;
mod nozzle;
mod plate;

use std::path::Path;

pub use airfoil::{Airfoil, AirfoilModel};
pub use body::{BODY_PATCH, Body, BodyModel};
pub use cavity::Cavity;
pub use channel::Channel;
pub use drawn::Drawn;
pub use error::{CaseError, Result};
pub use heating::{Heating, material_names};
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
    /// A region drawn as a closed sketch with named edges: laminar incompressible flow.
    Drawn(Drawn),
    /// A NACA 4-digit airfoil at an angle of attack: turbulent incompressible flow on a C-grid.
    Airfoil(Airfoil),
    /// A closed surface (a sphere, a car, a wing) in a wind tunnel, meshed by octree refinement.
    Body(Body),
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
            Geometry::Drawn(_) => "Drawn region",
            Geometry::Airfoil(_) => "Airfoil",
            Geometry::Body(_) => "Flow around a body",
        }
    }
}

/// What [`create`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub cells: usize,
    pub points: usize,
    /// The cells of the solid written beside the fluid (0 when the case has none).
    pub solid_cells: usize,
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
        Geometry::Drawn(d) => d.files()?,
        Geometry::Airfoil(a) => a.files()?,
        Geometry::Body(b) => b.files()?,
    };
    files::write_all(dir, &set.files)?;
    let mesh = match &set.raw_mesh {
        Some(raw) => files::write_raw(dir, raw)?,
        None => files::mesh(dir, &set.block_mesh_dict)?,
    };
    let solid_cells = match &set.solid_mesh {
        Some(raw) => files::write_solid(dir, raw)?.cells,
        None => 0,
    };
    Ok(Created {
        cells: mesh.cells,
        points: mesh.points,
        solid_cells,
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
