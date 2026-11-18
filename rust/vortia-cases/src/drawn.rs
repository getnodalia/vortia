//! A region drawn in the sketch: closed lines and arcs whose edges have names, meshed with
//! triangles and run as laminar incompressible flow.
//!
//! What an edge does follows its name: `inlet` is where the fluid comes in at the case's speed,
//! straight through the edge; `outlet` is held at zero pressure; `symmetry` is a symmetry plane;
//! any other name is a wall. Every name is its own patch, so results can be read per edge.

use std::fmt::Write as _;

use vortia_core::Vec3;
use vortia_mesh::tri2d::{self, Extrusion, Options, Region, Segment};
use vortia_sketch::{Edge, Loops, Sketch};

use crate::channel::laminar_settings;
use crate::error::{CaseError, Result};
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, size};

/// A drawn region and the conditions it runs at. Lengths are in millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    pub sketch: Sketch,
    /// The size of the triangles along the edges.
    pub cell_size: f64,
    /// The one cell's depth. It only sets the cell volume: the flow is 2D.
    pub thickness: f64,
    /// Inlet speed, in m/s.
    pub speed: f64,
    /// Kinematic viscosity, in m²/s.
    pub viscosity: f64,
}

impl Default for Drawn {
    fn default() -> Self {
        Self {
            sketch: Sketch::default(),
            cell_size: 5.0,
            thickness: 10.0,
            speed: 1.0,
            viscosity: 1.0e-3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Inlet,
    Outlet,
    Symmetry,
    Wall,
}

fn role(name: &str) -> Role {
    match name {
        "inlet" => Role::Inlet,
        "outlet" => Role::Outlet,
        "symmetry" => Role::Symmetry,
        _ => Role::Wall,
    }
}

fn kind(role: Role) -> &'static str {
    match role {
        Role::Inlet | Role::Outlet => "patch",
        Role::Symmetry => "symmetryPlane",
        Role::Wall => "wall",
    }
}

/// Twice the signed area of a loop: positive when it runs counter-clockwise.
fn area(edges: &[Edge]) -> f64 {
    let mut twice = 0.0;
    for edge in edges {
        for w in edge.points.windows(2) {
            if let [p, q] = w {
                twice += p.0 * q.1 - q.0 * p.1;
            }
        }
    }
    twice
}

impl Drawn {
    /// Sets one number by its name (`cell-size`, `thickness`, `speed`, `viscosity`).
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "cell-size" => self.cell_size = value,
            "thickness" => self.thickness = value,
            "speed" => self.speed = value,
            "viscosity" => self.viscosity = value,
            _ => return Err(crate::unknown(name, "a drawn region")),
        }
        Ok(())
    }

    /// The direction the fluid flows through each inlet patch: into the region, perpendicular to
    /// the edges.
    fn inlet_direction(loops: &Loops) -> Vec<(String, (f64, f64))> {
        let mut out: Vec<(String, (f64, f64))> = Vec::new();
        for (i, l) in loops.iter().enumerate() {
            // The region is on the left of an outer loop run counter-clockwise, and of a hole
            // run clockwise.
            let left = (i == 0) == (area(l) > 0.0);
            for e in l {
                if role(&e.patch) != Role::Inlet {
                    continue;
                }
                let (Some(a), Some(b)) = (e.points.first(), e.points.last()) else {
                    continue;
                };
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let n = if left { (-dy, dx) } else { (dy, -dx) };
                match out.iter_mut().find(|(name, _)| *name == e.patch) {
                    Some((_, v)) => *v = (v.0 + n.0, v.1 + n.1),
                    None => out.push((e.patch.clone(), n)),
                }
            }
        }
        out
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        let h = size("the cell size", self.cell_size, 1.0e6)?;
        let depth = size("the thickness", self.thickness, 1.0e6)?;
        let speed = size("the inlet speed", self.speed, 1.0e5)?;
        let nu = size("the viscosity", self.viscosity, 1.0e3)?;
        let loops = self
            .sketch
            .loops(5.0)
            .map_err(|e| CaseError::Invalid(e.to_string()))?;
        let mut names: Vec<String> = Vec::new();
        for e in loops.iter().flatten() {
            if !names.contains(&e.patch) {
                names.push(e.patch.clone());
            }
        }
        let has = |r: Role| names.iter().any(|n| role(n) == r);
        if !has(Role::Inlet) || !has(Role::Outlet) {
            return Err(CaseError::Invalid(
                "a drawn region needs an edge named 'inlet' and one named 'outlet'".into(),
            ));
        }
        let inlets = Self::inlet_direction(&loops);

        let region = Region {
            loops: loops
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|e| Segment {
                            points: e.points.iter().map(|p| [p.0, p.1]).collect(),
                            patch: e.patch.clone(),
                            kind: kind(role(&e.patch)).to_owned(),
                            size: None,
                        })
                        .collect()
                })
                .collect(),
        };
        let mut options = Options::new(h, Extrusion::Planar { thickness: depth });
        options.max_cells = MAX_CELLS as usize;
        let mut raw = tri2d::mesh(&region, &options).map_err(|e| CaseError::Mesh(e.to_string()))?;
        for p in &mut raw.points {
            *p = Vec3::new(p.x * 1e-3, p.y * 1e-3, p.z * 1e-3);
        }

        let mut u = String::new();
        let mut p = String::new();
        for name in &names {
            let (ut, pt) = match role(name) {
                Role::Inlet => {
                    let (nx, ny) = inlets
                        .iter()
                        .find(|(n, _)| n == name)
                        .map_or((1.0, 0.0), |&(_, v)| v);
                    let norm = nx.hypot(ny);
                    let (vx, vy) = if norm > 0.0 {
                        (speed * nx / norm + 0.0, speed * ny / norm + 0.0)
                    } else {
                        (speed, 0.0)
                    };
                    (
                        format!(
                            "type            fixedValue;\n        value           uniform ({vx} {vy} 0);"
                        ),
                        "type            zeroGradient;".to_owned(),
                    )
                }
                Role::Outlet => (
                    "type            zeroGradient;".to_owned(),
                    "type            fixedValue;\n        value           uniform 0;".to_owned(),
                ),
                Role::Symmetry => (
                    "type            symmetryPlane;".to_owned(),
                    "type            symmetryPlane;".to_owned(),
                ),
                Role::Wall => (
                    "type            noSlip;".to_owned(),
                    "type            zeroGradient;".to_owned(),
                ),
            };
            let _ = write!(u, "    {name}\n    {{\n        {ut}\n    }}\n");
            let _ = write!(p, "    {name}\n    {{\n        {pt}\n    }}\n");
        }
        let empty = "    frontAndBack\n    {\n        type            empty;\n    }\n";
        let mut files = laminar_settings(nu);
        files.push(File {
            path: "0/U",
            text: format!(
                "{}dimensions      [0 1 -1 0 0 0 0];\n\ninternalField   uniform (0 0 0);\n\nboundaryField\n{{\n{u}{empty}}}\n",
                header("volVectorField", "", "U")
            ),
        });
        files.push(File {
            path: "0/p",
            text: format!(
                "{}dimensions      [0 2 -2 0 0 0 0];\n\ninternalField   uniform 0;\n\nboundaryField\n{{\n{p}{empty}}}\n",
                header("volScalarField", "", "p")
            ),
        });
        Ok(FileSet {
            files,
            block_mesh_dict: String::new(),
            raw_mesh: Some(raw),
            application: "simpleFoam",
        })
    }
}
