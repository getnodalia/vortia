// SPDX-License-Identifier: GPL-3.0-or-later
//! CAD files as the closed triangle surfaces the body mesher takes.
//!
//! - [`step`]: STEP (ISO 10303) solids, read and tessellated by Nodalia's solidcore kernel.
//! - [`load`]: any surface file by its extension (STEP here, STL and OBJ in `vortia-mesh`).
//!
//! Lengths come out in metres, as the rest of Vortia has them.

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

pub mod step;

use std::path::Path;

use vortia_mesh::Result;
use vortia_mesh::stl::Stl;

/// Reads a surface file: STEP (`.step`, `.stp`) through the CAD kernel, STL and OBJ as they are.
pub fn load(path: &Path) -> Result<Stl> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("step" | "stp") => step::read(path).map(|s| s.surface),
        _ => Stl::load(path),
    }
}
