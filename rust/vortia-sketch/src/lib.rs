//! A dimensioned 2D sketch of a flow profile (R2 of `docs/architecture/rust-only.md`): points, the
//! lines and arcs between them, the dimensions that fix them, a solver that moves the free points
//! until the dimensions hold, and the wall profile a mesh is made from (R3).

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

mod error;
mod file;
mod model;
mod profile;
mod region;
mod solve;
pub mod templates;

pub use error::{Result, SketchError};
pub use file::{FORMAT, MAX_BYTES, VERSION};
pub use model::{
    Constraint, Entity, MAX_CONSTRAINTS, MAX_ENTITIES, MAX_POINTS, MAX_SIZE, Point, PointId, Sketch,
};
pub use profile::{Profile, stations};
pub use region::{DEFAULT_PATCH, Edge, Loops};
pub use solve::Solution;

#[cfg(test)]
mod tests;
