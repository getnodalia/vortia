//! Looking into results: probes, line sampling and integrals over patches
//! and the volume, read from a case on disk.
//!
//! First slice of the post-processing the simulation roadmap
//! (`docs/architecture/simulation-depth.md`, P1) puts ahead of every new
//! solver. The definitions follow OpenFOAM's `probes`, `sets` (cell
//! interpolation), `patchAverage`, `patchIntegrate` and `volFieldValue`
//! function objects, so the numbers can be compared with `postProcess` on the
//! same files.

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
mod balance;
mod case;
mod compare;
mod csv;
mod error;
mod export;
mod forces;
mod gci;
mod report;
mod seal;
mod section;
mod sha256;
mod slice;
mod stats;
mod streamline;
mod wall;

pub use airfoil::{AIRFOIL_PATCH, AirfoilReport, AirfoilSettings, SurfacePoint};
pub use balance::{BalanceHistory, MassBalance};
pub use case::{Case, FieldReport, FieldValues, MAX_LINE_POINTS, PatchReport, Sample};
pub use compare::{DEFINITIONS_DIFFER, Row, compare, comparison_csv};
pub use csv::line_csv;
pub use error::{PostError, Result};
pub use export::MAX_EXPORT_CELLS;
pub use forces::{FORCE_SCHEMA, ForceReference, ForceReport};
pub use gci::{Gci, gci};
pub use report::{Metric, SECTION_SCHEMA, SectionReport};
pub use seal::{MAX_REPORT_BYTES, Sealed, digest, seal};
pub use section::{Density, SectionAnalysis, SectionOptions};
pub use sha256::sha256_hex;
pub use slice::SlicePolygon;
pub use stats::{PatchStats, VolumeStats};
pub use streamline::{MAX_STREAMLINE_POINTS, Streamline, StreamlineEnd};
pub use wall::WallReport;

#[cfg(test)]
mod tests;
