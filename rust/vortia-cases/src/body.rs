// SPDX-License-Identifier: GPL-3.0-or-later
//! The flow around a body: any closed triangle surface (a sphere, a car, a wing, an F1 car)
//! placed in a wind tunnel, meshed by the octree mesher of `vortia-mesh` and run as steady
//! incompressible flow (laminar, Spalart–Allmaras or k-ω SST).
//!
//! The air comes from `-x`; the body keeps its own coordinates, so the stream's direction is
//! the one the model was drawn for. The four lateral sides are symmetry planes (a tunnel with
//! slip walls), which is why they are kept far from the body.
//!
//! The wall is snapped onto the surface and can carry prism layers (thin cells stacked on it).
//! The drag of a smooth body is within a few percent of published values on the sphere
//! benchmark; refine the surface and check that the numbers stop moving before trusting them.

use vortia_mesh::bodymesh::{self, BodySpec, LayerSpec, Region, SideKind};
use vortia_mesh::stl::Stl;

use crate::channel::laminar_settings;
use crate::error::{CaseError, Result};
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, count, size};

/// What the flow around the body is solved with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyModel {
    /// No turbulence model: a low-Reynolds-number flow.
    Laminar,
    /// Spalart–Allmaras with Spalding's wall function.
    SpalartAllmaras,
    /// k-ω SST with wall functions.
    KOmegaSst,
}

/// A closed surface in a stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// The body, in metres, closed.
    pub surface: Stl,
    /// Stream speed, in m/s.
    pub speed: f64,
    /// Reynolds number on `length`. The viscosity follows from it.
    pub reynolds: f64,
    /// The reference length, in metres; 0 takes the body's extent along the stream.
    pub length: f64,
    /// Distance from the body to the inlet, in reference lengths.
    pub upstream: f64,
    /// Distance from the body to the outlet, in reference lengths.
    pub downstream: f64,
    /// Distance from the body to each lateral side, in reference lengths.
    pub lateral: f64,
    /// Base cells along the reference length.
    pub cells: usize,
    /// How many times a base cell is halved at the body.
    pub levels: u8,
    /// How many times it is halved in the wake box behind the body.
    pub wake_levels: u8,
    /// Length of the wake box behind the body, in reference lengths.
    pub wake: f64,
    pub model: BodyModel,
    /// Free-stream turbulence intensity (a fraction of the speed), for k-ω SST.
    pub intensity: f64,
    /// Prism layers on the wall (0 for none).
    pub layers: usize,
    /// How much thicker each layer is than the one under it.
    pub layer_growth: f64,
    /// The y+ the first layer is sized for (from a flat-plate friction estimate); 0 sizes it as
    /// a tenth of the surface cell instead.
    pub yplus: f64,
}

impl Body {
    pub fn new(surface: Stl) -> Self {
        Self {
            surface,
            speed: 1.0,
            reynolds: 1.0e6,
            length: 0.0,
            upstream: 3.0,
            downstream: 8.0,
            lateral: 4.0,
            cells: 2,
            levels: 4,
            wake_levels: 2,
            wake: 3.0,
            model: BodyModel::SpalartAllmaras,
            intensity: 0.01,
            layers: 0,
            layer_growth: 1.2,
            yplus: 0.0,
        }
    }

    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "speed" => self.speed = value,
            "reynolds" => self.reynolds = value,
            "length" => self.length = value,
            "upstream" => self.upstream = value,
            "downstream" => self.downstream = value,
            "lateral" => self.lateral = value,
            "wake" => self.wake = value,
            "intensity" => self.intensity = value,
            "layer-growth" => self.layer_growth = value,
            "yplus" => self.yplus = value,
            "layers" => self.layers = crate::whole(name, value)?,
            "cells" => self.cells = crate::whole(name, value)?,
            "levels" => {
                self.levels = u8::try_from(crate::whole(name, value)?).unwrap_or(u8::MAX);
            }
            "wake-levels" => {
                self.wake_levels = u8::try_from(crate::whole(name, value)?).unwrap_or(u8::MAX);
            }
            "model" => {
                self.model = if value == 0.0 {
                    BodyModel::Laminar
                } else if value == 1.0 {
                    BodyModel::SpalartAllmaras
                } else if value == 2.0 {
                    BodyModel::KOmegaSst
                } else {
                    return Err(CaseError::Invalid(
                        "model is 0 (laminar), 1 (Spalart–Allmaras) or 2 (k-ω SST)".into(),
                    ));
                }
            }
            _ => return Err(crate::unknown(name, "a body")),
        }
        Ok(())
    }

    /// The reference length: the one asked for, or the body's extent along the stream.
    fn reference_length(&self, extent: f64) -> f64 {
        if self.length > 0.0 {
            self.length
        } else {
            extent
        }
    }

    /// The mesher's specification for this body.
    pub fn spec(&self) -> Result<BodySpec> {
        let summary = self.surface.summary();
        if !summary.closed() {
            return Err(CaseError::Invalid(
                "the surface is not closed: every edge must belong to exactly two triangles".into(),
            ));
        }
        let extent = summary.high[0] - summary.low[0];
        let length = size("the reference length", self.reference_length(extent), 1.0e5)?;
        size("the upstream distance", self.upstream, 1.0e3)?;
        size("the downstream distance", self.downstream, 1.0e3)?;
        size("the lateral distance", self.lateral, 1.0e3)?;
        let cells = count("the base cells along the length", self.cells, 1, 256)?;
        if self.levels > bodymesh::MAX_LEVEL || self.wake_levels > bodymesh::MAX_LEVEL {
            return Err(CaseError::Invalid(format!(
                "the surface can be refined {} times at most",
                bodymesh::MAX_LEVEL
            )));
        }
        let base = length / cells as f64;
        // The domain reaches whole base cells past the body, so its cells are cubes.
        let reach = |from: f64, to: f64, before: f64, after: f64| -> (f64, usize) {
            let low = from - before * length;
            let n = ((to + after * length - low) / base).ceil().max(1.0);
            (low, n as usize)
        };
        let (lx, nx) = reach(
            summary.low[0],
            summary.high[0],
            self.upstream,
            self.downstream,
        );
        let (ly, ny) = reach(summary.low[1], summary.high[1], self.lateral, self.lateral);
        let (lz, nz) = reach(summary.low[2], summary.high[2], self.lateral, self.lateral);
        let cap = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let mut spec = BodySpec::tunnel(
            [lx, ly, lz],
            [
                lx + nx as f64 * base,
                ly + ny as f64 * base,
                lz + nz as f64 * base,
            ],
            [cap(nx), cap(ny), cap(nz)],
            self.levels,
        );
        spec.max_cells = MAX_CELLS as usize;
        for side in spec.sides.iter_mut().skip(2) {
            side.kind = SideKind::Symmetry;
        }
        if self.wake_levels > 0 && self.wake > 0.0 {
            let pad = 0.25 * length;
            spec.regions.push(Region {
                low: [
                    summary.high[0] - 0.5 * length,
                    summary.low[1] - pad,
                    summary.low[2] - pad,
                ],
                high: [
                    summary.high[0] + self.wake * length,
                    summary.high[1] + pad,
                    summary.high[2] + pad,
                ],
                level: self.wake_levels.min(self.levels),
            });
        }
        if self.layers > 0 {
            if self.layers > bodymesh::MAX_LAYERS {
                return Err(CaseError::Invalid(format!(
                    "at most {} prism layers can be grown",
                    bodymesh::MAX_LAYERS
                )));
            }
            let growth = size("the layer growth", self.layer_growth, 3.0)?;
            if growth < 1.0 {
                return Err(CaseError::Invalid(
                    "each layer is at least as thick as the one under it (growth 1 to 3)".into(),
                ));
            }
            let first = if self.yplus > 0.0 {
                let yplus = size("the target y+", self.yplus, 1.0e5)?;
                let reynolds = size("the Reynolds number", self.reynolds, 1.0e10)?;
                // y = y+ nu / u_tau, with u_tau = U sqrt(cf / 2) and the flat-plate cf.
                let friction = 0.026 * reynolds.powf(-1.0 / 7.0);
                yplus * length / (reynolds * (0.5 * friction).sqrt())
            } else {
                0.1 * base / f64::from(1u32 << self.levels)
            };
            spec.layers = Some(LayerSpec {
                count: self.layers,
                first,
                ratio: growth,
                reach: 0.5,
            });
        }
        Ok(spec)
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        let speed = size("the stream speed", self.speed, 1.0e5)?;
        let reynolds = size("the Reynolds number", self.reynolds, 1.0e10)?;
        let intensity = size("the turbulence intensity", self.intensity, 1.0)?;
        let spec = self.spec()?;
        let extent = self.surface.summary().high[0] - self.surface.summary().low[0];
        let length = self.reference_length(extent);
        let nu = speed * length / reynolds;
        let (raw, _) = bodymesh::mesh_around(&self.surface, &spec)
            .map_err(|e| CaseError::Mesh(e.to_string()))?;

        let mut files = match self.model {
            BodyModel::Laminar => laminar_settings(nu),
            _ => self.turbulent_settings(nu),
        };
        files.extend(self.fields(&spec, speed, nu, intensity));
        Ok(FileSet {
            files,
            block_mesh_dict: String::new(),
            raw_mesh: Some(raw),
            application: "simpleFoam",
        })
    }

    /// The patch dictionaries of one field: the body, the inlet, the outlet and the four sides.
    fn field(
        class: &str,
        name: &str,
        dims: &str,
        internal: &str,
        spec: &BodySpec,
        patches: [&str; 3],
    ) -> File {
        let [body, inlet, outlet] = patches;
        let block = |patch: &str, text: &str| {
            let mut s = format!("    {patch}\n    {{\n");
            for line in text.lines() {
                s.push_str(&format!("        {}\n", line.trim_start()));
            }
            s.push_str("    }\n");
            s
        };
        let mut boundary = String::new();
        for (index, side) in spec.sides.iter().enumerate() {
            let text = match (index, side.kind) {
                (0, _) => inlet,
                (1, _) => outlet,
                (_, SideKind::Symmetry) => "type            symmetryPlane;",
                _ => outlet,
            };
            boundary.push_str(&block(&side.name, text));
        }
        boundary.push_str(&block(&spec.body, body));
        File {
            path: match name {
                "U" => "0/U",
                "p" => "0/p",
                "nuTilda" => "0/nuTilda",
                "nut" => "0/nut",
                "k" => "0/k",
                _ => "0/omega",
            },
            text: format!(
                "{}dimensions      {dims};\n\ninternalField   {internal};\n\nboundaryField\n{{\n{boundary}}}\n",
                header(class, "", name),
            ),
        }
    }

    fn fields(&self, spec: &BodySpec, speed: f64, nu: f64, intensity: f64) -> Vec<File> {
        let fixed = |v: &str| format!("type            fixedValue;\nvalue           uniform {v};");
        let inlet_out = |v: &str| {
            format!(
                "type            inletOutlet;\ninletValue      uniform {v};\nvalue           uniform {v};"
            )
        };
        let zero_gradient = "type            zeroGradient;";
        let calculated = "type            calculated;\nvalue           uniform 0;";
        let stream = format!("({speed} 0 0)");
        let mut files = vec![
            Self::field(
                "volVectorField",
                "U",
                "[0 1 -1 0 0 0 0]",
                &format!("uniform {stream}"),
                spec,
                ["type            noSlip;", &fixed(&stream), zero_gradient],
            ),
            Self::field(
                "volScalarField",
                "p",
                "[0 2 -2 0 0 0 0]",
                "uniform 0",
                spec,
                [zero_gradient, zero_gradient, &fixed("0")],
            ),
        ];
        match self.model {
            BodyModel::Laminar => {}
            BodyModel::SpalartAllmaras => {
                let nu_tilda = 4.0 * nu;
                files.push(Self::field(
                    "volScalarField",
                    "nuTilda",
                    "[0 2 -1 0 0 0 0]",
                    &format!("uniform {nu_tilda}"),
                    spec,
                    [
                        &fixed("0"),
                        &fixed(&nu_tilda.to_string()),
                        &inlet_out(&nu_tilda.to_string()),
                    ],
                ));
                files.push(Self::field(
                    "volScalarField",
                    "nut",
                    "[0 2 -1 0 0 0 0]",
                    "uniform 0",
                    spec,
                    [
                        "type            nutUSpaldingWallFunction;\nvalue           uniform 0;",
                        calculated,
                        calculated,
                    ],
                ));
            }
            BodyModel::KOmegaSst => {
                let k = 1.5 * (speed * intensity).powi(2);
                let omega = k / (10.0 * nu);
                for (name, dims, value, wall) in [
                    ("k", "[0 2 -2 0 0 0 0]", k, "kqRWallFunction"),
                    ("omega", "[0 0 -1 0 0 0 0]", omega, "omegaWallFunction"),
                ] {
                    files.push(Self::field(
                        "volScalarField",
                        name,
                        dims,
                        &format!("uniform {value}"),
                        spec,
                        [
                            &format!("type            {wall};\nvalue           uniform {value};"),
                            &fixed(&value.to_string()),
                            &inlet_out(&value.to_string()),
                        ],
                    ));
                }
                files.push(Self::field(
                    "volScalarField",
                    "nut",
                    "[0 2 -1 0 0 0 0]",
                    &format!("uniform {}", k / omega),
                    spec,
                    [
                        "type            nutkWallFunction;\nvalue           uniform 0;",
                        calculated,
                        calculated,
                    ],
                ));
            }
        }
        files
    }

    /// The settings of a turbulent run: schemes, solvers and the model.
    fn turbulent_settings(&self, nu: f64) -> Vec<File> {
        let (model, schemes, solved, residuals, relaxation): (_, _, &[&str], _, _) = if self.model
            == BodyModel::SpalartAllmaras
        {
            (
                "SpalartAllmaras",
                "    div(phi,nuTilda) bounded Gauss upwind;\n",
                &["nuTilda"],
                "        nuTilda         1e-06;\n",
                "        nuTilda         0.5;\n",
            )
        } else {
            (
                "kOmegaSST",
                "    div(phi,k)      bounded Gauss upwind;\n    div(phi,omega)  bounded Gauss upwind;\n",
                &["k", "omega"],
                "        k               1e-06;\n        omega           1e-06;\n",
                "        k               0.5;\n        omega           0.5;\n",
            )
        };
        let extra_solvers: String = solved
            .iter()
            .map(|name| {
                format!(
                    "    {name}\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n\n"
                )
            })
            .collect();
        vec![
            File {
                path: "system/controlDict",
                text: format!(
                    "{}application     simpleFoam;\nstartFrom       startTime;\nstartTime       0;\nstopAt          endTime;\nendTime         3000;\ndeltaT          1;\nwriteControl    timeStep;\nwriteInterval   500;\npurgeWrite      0;\nwriteFormat     ascii;\nwritePrecision  8;\ntimeFormat      general;\ntimePrecision   6;\nrunTimeModifiable true;\n",
                    header("dictionary", "system", "controlDict")
                ),
            },
            File {
                path: "system/fvSchemes",
                text: format!(
                    "{}ddtSchemes\n{{\n    default         steadyState;\n}}\n\ngradSchemes\n{{\n    default         Gauss linear;\n    limited         cellLimited Gauss linear 1;\n}}\n\ndivSchemes\n{{\n    default         none;\n    div(phi,U)      bounded Gauss linearUpwind limited;\n{schemes}    div((nuEff*dev2(T(grad(U))))) Gauss linear;\n}}\n\nlaplacianSchemes\n{{\n    default         Gauss linear corrected;\n}}\n\ninterpolationSchemes\n{{\n    default         linear;\n}}\n\nsnGradSchemes\n{{\n    default         corrected;\n}}\n\nwallDist\n{{\n    method          meshWave;\n}}\n",
                    header("dictionary", "system", "fvSchemes")
                ),
            },
            File {
                path: "system/fvSolution",
                text: format!(
                    "{}solvers\n{{\n    p\n    {{\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-08;\n        relTol          0.01;\n    }}\n\n    U\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n\n{extra_solvers}}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 2;\n    residualControl\n    {{\n        p               1e-05;\n        U               1e-06;\n{residuals}    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.2;\n    }}\n    equations\n    {{\n        U               0.5;\n{relaxation}    }}\n}}\n",
                    header("dictionary", "system", "fvSolution")
                ),
            },
            File {
                path: "constant/transportProperties",
                text: format!(
                    "{}transportModel Newtonian;\n\nnu              {nu};\n",
                    header("dictionary", "constant", "transportProperties")
                ),
            },
            File {
                path: "constant/turbulenceProperties",
                text: format!(
                    "{}simulationType RAS;\n\nRAS\n{{\n    RASModel        {model};\n    turbulence      on;\n    printCoeffs     on;\n}}\n",
                    header("dictionary", "constant", "turbulenceProperties")
                ),
            },
        ]
    }
}

/// The patch on the body, for the force commands.
pub const BODY_PATCH: &str = "body";
