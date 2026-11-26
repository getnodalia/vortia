//! A volume mesh of the air around a body: the flow domain is cut into cubes that get smaller
//! towards the body, the cubes inside the body are taken out, and what is left is a polyhedral
//! mesh the solvers read as they read any other.
//!
//! This is the first stage of meshing a real shape (an STL, or a STEP made into triangles):
//! the body is stepped (its wall follows cube faces). The octree is balanced so neighbouring
//! cubes differ by one level at most, and where a coarse face meets finer ones the nodes of
//! the finer ones are part of the coarse outline, so every cell is closed.
//!
//! The method is the usual one for Cartesian meshes (octree refinement, removal of the cubes
//! in the body); it is written here from that description.

mod assemble;
mod grid;
mod layers;
mod octree;
mod parts;
mod regions;
mod snap;

pub use layers::{LayerSpec, LayerStats, MAX_LAYERS, add_layers};
pub use octree::Region;
pub use parts::{MAX_PARTS, part_patches, patch_name};
pub use regions::{InterfaceMatch, match_interface};
pub use snap::SnapStats;

use vortia_foamfile::RawPolyMesh;

use crate::error::{MeshError, Result};
use crate::stl::Stl;
use grid::{TriGrid, at};
use octree::{Lattice, Octree, Refinement};

/// The deepest level a cell can be refined to.
pub const MAX_LEVEL: u8 = 10;

/// What a side of the domain is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideKind {
    #[default]
    Patch,
    Wall,
    Symmetry,
}

impl SideKind {
    pub fn foam_name(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Wall => "wall",
            Self::Symmetry => "symmetryPlane",
        }
    }
}

/// A side of the domain: its patch name and kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Side {
    pub name: String,
    pub kind: SideKind,
}

impl Default for Side {
    fn default() -> Self {
        Self {
            name: "side".into(),
            kind: SideKind::Patch,
        }
    }
}

/// How to mesh the air around a body.
#[derive(Clone, Debug, PartialEq)]
pub struct BodySpec {
    /// The corners of the domain.
    pub low: [f64; 3],
    pub high: [f64; 3],
    /// The base cells along each axis (they are cubes when the domain divides evenly).
    pub base_cells: [u32; 3],
    /// How many times a base cell is halved where the body touches it.
    pub surface_level: u8,
    /// Boxes refined to a level of their own, such as a wake.
    pub regions: Vec<Region>,
    /// The most cells the mesh may have.
    pub max_cells: usize,
    /// The sides, in the order x-, x+, y-, y+, z-, z+.
    pub sides: [Side; 6],
    /// The name of the patch on the body.
    pub body: String,
    /// Whether the wall is moved onto the surface (otherwise it stays stepped).
    pub snap: bool,
    /// Prism layers grown from the wall (none when `None`).
    pub layers: Option<LayerSpec>,
    /// Whether the wall is split into one patch for each named part of the surface (`<body>_<part>`).
    /// A surface with no parts, or one, keeps a single wall.
    pub by_part: bool,
    /// Whether the body is meshed too, as a solid: the faces of its mesh on the surface are the
    /// same as the wall's, and both meshes follow the surface together.
    pub solid: bool,
}

impl BodySpec {
    /// A wind-tunnel domain with the usual names: the air comes in at x-, goes out at x+, and
    /// the other four sides are free.
    pub fn tunnel(low: [f64; 3], high: [f64; 3], base_cells: [u32; 3], surface_level: u8) -> Self {
        let side = |name: &str| Side {
            name: name.into(),
            kind: SideKind::Patch,
        };
        Self {
            low,
            high,
            base_cells,
            surface_level,
            regions: Vec::new(),
            max_cells: 20_000_000,
            sides: [
                side("inlet"),
                side("outlet"),
                side("ymin"),
                side("ymax"),
                side("zmin"),
                side("zmax"),
            ],
            body: "body".into(),
            snap: true,
            layers: None,
            by_part: false,
            solid: false,
        }
    }
}

/// What came out.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyStats {
    /// The cells of the octree, in the air or not.
    pub leaves: usize,
    /// The cells of the mesh.
    pub cells: usize,
    pub faces: usize,
    /// The faces on the body.
    pub body_faces: usize,
    /// What snapping the wall to the surface did (all zero when it was off).
    pub snapping: snap::SnapStats,
    /// What growing prism layers did (all zero when there were none).
    pub layering: layers::LayerStats,
    /// The wall patches the parts of the surface made, with their faces (empty when the wall is one).
    pub parts: Vec<(String, usize)>,
    /// The cells of the solid (zero when the body was not meshed).
    pub solid_cells: usize,
}

/// The air around a body and, when asked for, the body.
#[derive(Clone, Debug)]
pub struct BodyMeshes {
    pub fluid: RawPolyMesh,
    /// The body's own mesh, with the patch `body` where the fluid's wall is.
    pub solid: Option<RawPolyMesh>,
    pub stats: BodyStats,
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

fn check_spec(surface: &Stl, spec: &BodySpec) -> Result<()> {
    let finite = |v: &[f64; 3]| v.iter().all(|x| x.is_finite());
    if !finite(&spec.low) || !finite(&spec.high) {
        return invalid("the domain corners are not finite");
    }
    if (0..3).any(|a| at(&spec.low, a) >= at(&spec.high, a)) {
        return invalid("the domain has no volume: each low corner must be below the high one");
    }
    if spec.base_cells.iter().any(|&n| n == 0 || n > 4096) {
        return invalid("the base mesh needs between 1 and 4096 cells along each axis");
    }
    if spec.surface_level > MAX_LEVEL {
        return invalid(format!("the surface level is at most {MAX_LEVEL}"));
    }
    if spec.regions.iter().any(|r| {
        r.level > MAX_LEVEL
            || !finite(&r.low)
            || !finite(&r.high)
            || (0..3).any(|a| at(&r.low, a) > at(&r.high, a))
    }) {
        return invalid("a refinement region has a bad box or a level over the maximum");
    }
    let base: u64 = spec.base_cells.iter().map(|&n| u64::from(n)).product();
    if base as usize > spec.max_cells {
        return invalid("the base mesh alone is over the cell limit");
    }
    let summary = surface.summary();
    if !summary.closed() {
        return invalid(format!(
            "the surface is not watertight ({} open edges, {} edges shared by more than two \
             triangles): a body has to be closed to be meshed around",
            summary.open_edges, summary.non_manifold_edges
        ));
    }
    if summary.degenerate > 0 {
        return invalid(format!("{} triangles have no area", summary.degenerate));
    }
    for a in 0..3 {
        if at(&summary.low, a) <= at(&spec.low, a) || at(&summary.high, a) >= at(&spec.high, a) {
            return invalid("the body is not inside the domain: enlarge the domain on every side");
        }
    }
    Ok(())
}

/// Meshes the air around `surface`, a closed surface, inside the domain of `spec`.
pub fn mesh_around(surface: &Stl, spec: &BodySpec) -> Result<(RawPolyMesh, BodyStats)> {
    let plain = BodySpec {
        solid: false,
        ..spec.clone()
    };
    let BodyMeshes { fluid, stats, .. } = mesh_body(surface, &plain)?;
    Ok((fluid, stats))
}

/// Meshes the air around `surface` and, when `spec.solid` is set, the body inside it.
pub fn mesh_body(surface: &Stl, spec: &BodySpec) -> Result<BodyMeshes> {
    check_spec(surface, spec)?;
    let grid = TriGrid::new(surface);
    let mut size = [0.0; 3];
    for a in 0..3 {
        grid::put(
            &mut size,
            a,
            (at(&spec.high, a) - at(&spec.low, a)) / f64::from(at(&spec.base_cells, a)),
        );
    }
    let lattice = Lattice {
        low: spec.low,
        base_size: size,
        base: spec.base_cells,
        max_level: spec
            .surface_level
            .max(spec.regions.iter().map(|r| r.level).max().unwrap_or(0)),
    };
    let tree = Octree::build(
        lattice,
        &Refinement {
            surface: &grid,
            surface_level: spec.surface_level,
            regions: &spec.regions,
            max_cells: spec.max_cells,
        },
    )?;
    let assemble::Assembled {
        mut mesh,
        mut stats,
        solid,
    } = assemble::assemble(&tree, &grid, spec, spec.solid)?;
    let finest = size.iter().copied().fold(0.0, f64::max) / f64::from(1u32 << spec.surface_level);
    let mut body = None;
    if spec.solid {
        // One mesh of both regions is snapped, so cells on both sides follow the wall; then it
        // is cut in two along the wall.
        let wall = regions::wall_faces(&mesh, &solid);
        if spec.snap {
            stats.snapping = snap::snap_between_regions(&mut mesh, &grid, 2.5 * finest, &wall);
        }
        let (fluid, inside) = regions::split(&mesh, &solid, &spec.body)?;
        stats.solid_cells = solid.iter().filter(|&&s| s).count();
        stats.cells = solid.len() - stats.solid_cells;
        stats.faces = fluid.faces.len();
        stats.body_faces = wall.len();
        mesh = fluid;
        body = Some(inside);
    } else if spec.snap {
        stats.snapping = snap::snap(&mut mesh, &grid, 2.5 * finest);
    }
    if let Some(layer_spec) = &spec.layers {
        stats.layering = layers::add_layers(&mut mesh, &spec.body, layer_spec)?;
        stats.cells += stats.layering.cells;
        stats.faces = mesh.faces.len();
        stats.body_faces = mesh
            .patches
            .iter()
            .find(|p| p.name == spec.body)
            .map_or(0, |p| p.n_faces);
    }
    if spec.by_part && parts::part_patches(surface, &spec.body).is_some() {
        stats.parts = parts::split_wall(&mut mesh, &grid, surface, spec)?;
    }
    Ok(BodyMeshes {
        fluid: mesh,
        solid: body,
        stats,
    })
}

#[cfg(test)]
mod tests;
