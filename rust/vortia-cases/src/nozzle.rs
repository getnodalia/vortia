//! A converging-diverging nozzle: its block mesh and its compressible case.
//!
//! Two meshes of the same nozzle: a planar one (a slot with both walls, one
//! cell deep) and an axisymmetric one (a 5° wedge, the axis collapsed). Both
//! have the wall profile of a smoothstep from the inlet to the throat and on
//! to the outlet.
//!
//! The Qt application's planar mesh is the half section with a symmetry plane
//! on the axis. The engine's compressible solver has no symmetry condition
//! for the temperature and turbulence fields yet, so the planar mesh here is
//! the whole slot, which it does run.

use std::fmt::Write as _;

use crate::error::{CaseError, Result};
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, count, size};

/// Which mesh of the nozzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NozzleKind {
    /// The whole slot between two walls, one cell deep.
    Planar,
    /// A thin wedge of the body of revolution, the axis collapsed.
    Axisymmetric,
}

impl NozzleKind {
    pub fn title(self) -> &'static str {
        match self {
            NozzleKind::Planar => "Planar nozzle",
            NozzleKind::Axisymmetric => "Axisymmetric nozzle",
        }
    }
}

/// A nozzle and the conditions it runs at. Lengths are in millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct Nozzle {
    pub kind: NozzleKind,
    pub inlet_diameter: f64,
    pub throat_diameter: f64,
    pub outlet_diameter: f64,
    pub converging_length: f64,
    pub diverging_length: f64,
    pub converging_segments: usize,
    pub diverging_segments: usize,
    pub axial_cells_per_segment: usize,
    /// Cells across the radius. Across the whole slot (an even number) on the
    /// planar mesh.
    pub radial_cells: usize,
    /// Last over first cell width across the radius, towards the wall. The
    /// planar mesh's cells are graded towards both walls.
    pub radial_grading: f64,
    /// The wedge's opening, in degrees (0.01 to 5). Axisymmetric only.
    pub wedge_angle: f64,
    /// The planar mesh's one-cell depth, in millimetres. Planar only.
    pub thickness: f64,
    /// Speed of the flow entering, in m/s.
    pub inlet_speed: f64,
    /// Temperature of the flow entering, in K.
    pub inlet_temperature: f64,
    /// Static pressure at the outlet, in Pa.
    pub outlet_pressure: f64,
}

impl Nozzle {
    /// The Qt application's default nozzle, at a pressure ratio that stays subsonic.
    pub fn new(kind: NozzleKind) -> Self {
        Self {
            kind,
            inlet_diameter: 100.0,
            throat_diameter: 52.0,
            outlet_diameter: 120.0,
            converging_length: 158.0,
            diverging_length: 420.0,
            converging_segments: 18,
            diverging_segments: 30,
            axial_cells_per_segment: 3,
            radial_cells: 40,
            radial_grading: 2.0,
            wedge_angle: 5.0,
            thickness: 1.0,
            inlet_speed: 20.0,
            inlet_temperature: 300.0,
            outlet_pressure: 101_325.0,
        }
    }

    /// Sets one number by its name (`inlet-diameter`, `throat-diameter`,
    /// `outlet-diameter`, `converging-length`, `diverging-length`,
    /// `converging-segments`, `diverging-segments`, `cells-per-segment`,
    /// `radial-cells`, `radial-grading`, `wedge-angle`, `thickness`,
    /// `inlet-speed`, `inlet-temperature`, `outlet-pressure`), for a command
    /// line or a script.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "inlet-diameter" => self.inlet_diameter = value,
            "throat-diameter" => self.throat_diameter = value,
            "outlet-diameter" => self.outlet_diameter = value,
            "converging-length" => self.converging_length = value,
            "diverging-length" => self.diverging_length = value,
            "radial-grading" => self.radial_grading = value,
            "wedge-angle" => self.wedge_angle = value,
            "thickness" => self.thickness = value,
            "inlet-speed" => self.inlet_speed = value,
            "inlet-temperature" => self.inlet_temperature = value,
            "outlet-pressure" => self.outlet_pressure = value,
            "converging-segments" => self.converging_segments = crate::whole(name, value)?,
            "diverging-segments" => self.diverging_segments = crate::whole(name, value)?,
            "cells-per-segment" => self.axial_cells_per_segment = crate::whole(name, value)?,
            "radial-cells" => self.radial_cells = crate::whole(name, value)?,
            _ => return Err(crate::unknown(name, "a nozzle")),
        }
        Ok(())
    }

    /// The wall profile: `(x, radius)` at each station, in millimetres.
    pub(crate) fn stations(&self) -> Vec<(f64, f64)> {
        let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
        let (ri, rt, ro) = (
            self.inlet_diameter / 2.0,
            self.throat_diameter / 2.0,
            self.outlet_diameter / 2.0,
        );
        let (nc, nd) = (self.converging_segments, self.diverging_segments);
        let mut out = Vec::with_capacity(nc + nd + 1);
        for i in 0..=nc {
            let t = i as f64 / nc as f64;
            out.push((self.converging_length * t, ri + (rt - ri) * smooth(t)));
        }
        for i in 1..=nd {
            let t = i as f64 / nd as f64;
            out.push((
                self.converging_length + self.diverging_length * t,
                rt + (ro - rt) * smooth(t),
            ));
        }
        out
    }

    fn validate(&self) -> Result<()> {
        let sizes = [
            ("the inlet diameter", self.inlet_diameter),
            ("the throat diameter", self.throat_diameter),
            ("the outlet diameter", self.outlet_diameter),
            ("the converging length", self.converging_length),
            ("the diverging length", self.diverging_length),
        ];
        for (name, value) in sizes {
            size(name, value, 1.0e6)?;
        }
        if self.throat_diameter > self.inlet_diameter || self.throat_diameter > self.outlet_diameter
        {
            return Err(CaseError::Invalid(
                "the throat must not be wider than the inlet or the outlet".into(),
            ));
        }
        count("the converging segments", self.converging_segments, 2, 200)?;
        count("the diverging segments", self.diverging_segments, 2, 200)?;
        count(
            "the cells per segment",
            self.axial_cells_per_segment,
            1,
            200,
        )?;
        count("the radial cells", self.radial_cells, 2, 1000)?;
        if self.kind == NozzleKind::Planar && !self.radial_cells.is_multiple_of(2) {
            return Err(CaseError::Invalid(format!(
                "the planar nozzle needs an even number of cells across, not {}",
                self.radial_cells
            )));
        }
        size("the radial grading", self.radial_grading, 1000.0)?;
        match self.kind {
            NozzleKind::Planar => {
                size("the thickness", self.thickness, 1000.0)?;
            }
            NozzleKind::Axisymmetric => {
                if !(self.wedge_angle > 0.01 && self.wedge_angle <= 5.0) {
                    return Err(CaseError::Invalid(format!(
                        "the wedge angle must be above 0.01° and at most 5°, not {}",
                        self.wedge_angle
                    )));
                }
            }
        }
        size("the inlet speed", self.inlet_speed, 1.0e5)?;
        size("the inlet temperature", self.inlet_temperature, 1.0e5)?;
        size("the outlet pressure", self.outlet_pressure, 1.0e10)?;
        let segments = (self.converging_segments + self.diverging_segments) as u64;
        let cells = segments * self.axial_cells_per_segment as u64 * self.radial_cells as u64;
        if cells > MAX_CELLS {
            return Err(CaseError::Invalid(format!(
                "{cells} cells is more than the {MAX_CELLS} a new case may have"
            )));
        }
        Ok(())
    }

    /// The nozzle's `blockMeshDict`.
    fn block_mesh_dict(&self) -> String {
        let stations = self.stations();
        let n = stations.len().saturating_sub(1);
        let (ax, rad) = (self.axial_cells_per_segment, self.radial_cells);
        let grading = self.radial_grading;
        let per = match self.kind {
            NozzleKind::Planar => 4,
            NozzleKind::Axisymmetric => 3,
        };
        let mut s = header("dictionary", "", "blockMeshDict");
        s.push_str("scale   0.001;\n\nvertices\n(\n");
        let half = self.wedge_angle.to_radians() / 2.0;
        for &(x, r) in &stations {
            match self.kind {
                NozzleKind::Planar => {
                    let (t, m) = (self.thickness, -r);
                    let _ = writeln!(
                        s,
                        "    ({x} {m} 0)\n    ({x} {r} 0)\n    ({x} {m} {t})\n    ({x} {r} {t})"
                    );
                }
                NozzleKind::Axisymmetric => {
                    let (y, z) = (r * half.cos(), r * half.sin());
                    let _ = writeln!(s, "    ({x} 0 0)\n    ({x} {y} {})\n    ({x} {y} {z})", -z);
                }
            }
        }
        s.push_str(");\n\nblocks\n(\n");
        for i in 0..n {
            let (a, b) = (per * i, per * (i + 1));
            match self.kind {
                NozzleKind::Planar => {
                    let _ = writeln!(
                        s,
                        "    hex ({a} {b} {} {} {} {} {} {}) ({ax} {rad} 1) simpleGrading (1 ((0.5 0.5 {grading}) (0.5 0.5 {})) 1)",
                        b + 1,
                        a + 1,
                        a + 2,
                        b + 2,
                        b + 3,
                        a + 3,
                        1.0 / grading
                    );
                }
                NozzleKind::Axisymmetric => {
                    let _ = writeln!(
                        s,
                        "    hex ({a} {b} {} {} {a} {b} {} {}) ({ax} {rad} 1) simpleGrading (1 {grading} 1)",
                        b + 1,
                        a + 1,
                        b + 2,
                        a + 2
                    );
                }
            }
        }
        s.push_str(");\n\nedges\n(\n);\n\nboundary\n(\n");
        let last = per * n;
        let mut patch = |name: &str, kind: &str, faces: &[[usize; 4]]| {
            let _ = writeln!(
                s,
                "    {name}\n    {{\n        type {kind};\n        faces\n        ("
            );
            for f in faces {
                let _ = writeln!(s, "            ({} {} {} {})", f[0], f[1], f[2], f[3]);
            }
            s.push_str("        );\n    }\n");
        };
        let each = |f: &dyn Fn(usize, usize) -> [usize; 4]| -> Vec<[usize; 4]> {
            (0..n).map(|i| f(per * i, per * (i + 1))).collect()
        };
        match self.kind {
            NozzleKind::Planar => {
                patch("inlet", "patch", &[[0, 1, 3, 2]]);
                patch("outlet", "patch", &[[last, last + 1, last + 3, last + 2]]);
                let mut walls = each(&|a, b| [a, a + 2, b + 2, b]);
                walls.extend(each(&|a, b| [a + 1, a + 3, b + 3, b + 1]));
                patch("nozzleWall", "wall", &walls);
                let mut both = Vec::new();
                for i in 0..n {
                    let (a, b) = (per * i, per * (i + 1));
                    both.push([a, a + 1, b + 1, b]);
                    both.push([a + 2, b + 2, b + 3, a + 3]);
                }
                patch("frontAndBack", "empty", &both);
            }
            NozzleKind::Axisymmetric => {
                patch("inlet", "patch", &[[0, 0, 2, 1]]);
                patch("outlet", "patch", &[[last, last + 1, last + 2, last]]);
                patch(
                    "nozzleWall",
                    "wall",
                    &each(&|a, b| [a + 1, a + 2, b + 2, b + 1]),
                );
                patch("wedgeFront", "wedge", &each(&|a, b| [a, a + 1, b + 1, b]));
                patch("wedgeBack", "wedge", &each(&|a, b| [a, b, b + 2, a + 2]));
                patch("axis", "empty", &each(&|a, b| [a, b, b, a]));
            }
        }
        s.push_str(");\n");
        s
    }

    /// The patches, with what each is.
    fn patches(&self) -> &'static [(&'static str, Role)] {
        match self.kind {
            NozzleKind::Planar => &[
                ("inlet", Role::Inlet),
                ("outlet", Role::Outlet),
                ("nozzleWall", Role::Wall),
                ("frontAndBack", Role::Empty),
            ],
            NozzleKind::Axisymmetric => &[
                ("inlet", Role::Inlet),
                ("outlet", Role::Outlet),
                ("nozzleWall", Role::Wall),
                ("wedgeFront", Role::Wedge),
                ("wedgeBack", Role::Wedge),
                ("axis", Role::Empty),
            ],
        }
    }

    /// A field file: its class, dimensions, the internal value and, for each
    /// role that is not a constraint, the boundary condition's body.
    fn field(
        &self,
        class: &str,
        name: &str,
        dimensions: &str,
        internal: &str,
        body: impl Fn(Role) -> String,
    ) -> File {
        let mut s = header(class, "", name);
        let _ = write!(
            s,
            "dimensions      {dimensions};\n\ninternalField   {internal};\n\nboundaryField\n{{\n"
        );
        for &(patch, role) in self.patches() {
            let text = match role {
                Role::Empty => "type            empty;".to_owned(),
                Role::Wedge => "type            wedge;".to_owned(),
                _ => body(role),
            };
            let _ = write!(s, "    {patch}\n    {{\n");
            for line in text.lines() {
                let _ = writeln!(s, "        {}", line.trim_start());
            }
            s.push_str("    }\n");
        }
        s.push_str("}\n");
        File {
            path: field_path(name),
            text: s,
        }
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        self.validate()?;
        let block_mesh_dict = self.block_mesh_dict();
        let (speed, t0, pb) = (
            self.inlet_speed,
            self.inlet_temperature,
            self.outlet_pressure,
        );
        let mixing = 0.05 * self.inlet_diameter / 1000.0;

        let mut files = vec![
            File {
                path: "system/blockMeshDict",
                text: block_mesh_dict.clone(),
            },
            self.field("volVectorField", "U", "[0 1 -1 0 0 0 0]", &format!("uniform ({speed} 0 0)"), |r| match r {
                Role::Inlet => format!("type            fixedValue;\nvalue           uniform ({speed} 0 0);"),
                Role::Outlet => "type            pressureInletOutletVelocity;\nvalue           uniform (0 0 0);".to_owned(),
                _ => "type            noSlip;".to_owned(),
            }),
            self.field("volScalarField", "p", "[1 -1 -2 0 0 0 0]", &format!("uniform {pb}"), |r| match r {
                Role::Inlet => "type            zeroGradient;".to_owned(),
                Role::Outlet => format!("type            fixedValue;\nvalue           uniform {pb};"),
                _ => "type            zeroGradient;".to_owned(),
            }),
            self.field("volScalarField", "T", "[0 0 0 1 0 0 0]", &format!("uniform {t0}"), |r| match r {
                Role::Inlet => format!("type            fixedValue;\nvalue           uniform {t0};"),
                Role::Outlet => format!("type            inletOutlet;\ninletValue      uniform {t0};\nvalue           uniform {t0};"),
                _ => "type            zeroGradient;".to_owned(),
            }),
            self.field("volScalarField", "k", "[0 2 -2 0 0 0 0]", "uniform 1", |r| match r {
                Role::Inlet => "type            turbulentIntensityKineticEnergyInlet;\nintensity       0.05;\nvalue           uniform 1;".into(),
                Role::Outlet => "type            inletOutlet;\ninletValue      uniform 1;\nvalue           uniform 1;".into(),
                _ => "type            kqRWallFunction;\nvalue           uniform 1;".into(),
            }),
            self.field("volScalarField", "omega", "[0 0 -1 0 0 0 0]", "uniform 1000", |r| match r {
                Role::Inlet => format!("type            turbulentMixingLengthFrequencyInlet;\nmixingLength    {mixing};\nvalue           uniform 1000;"),
                Role::Outlet => "type            inletOutlet;\ninletValue      uniform 1000;\nvalue           uniform 1000;".into(),
                _ => "type            omegaWallFunction;\nvalue           uniform 1000;".into(),
            }),
            self.field("volScalarField", "nut", "[0 2 -1 0 0 0 0]", "uniform 0", |r| match r {
                Role::Wall => "type            nutkWallFunction;\nvalue           uniform 0;".into(),
                _ => "type            calculated;\nvalue           uniform 0;".into(),
            }),
            self.field("volScalarField", "alphat", "[1 -1 -1 0 0 0 0]", "uniform 0", |r| match r {
                Role::Wall => "type            compressible::alphatWallFunction;\nvalue           uniform 0;".into(),
                _ => "type            calculated;\nvalue           uniform 0;".into(),
            }),
        ];
        files.extend(settings());
        Ok(FileSet {
            files,
            block_mesh_dict,
            application: "rhoSimpleFoam",
        })
    }
}

/// What a patch is, for choosing its boundary conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Inlet,
    Outlet,
    Wall,
    Wedge,
    Empty,
}

fn field_path(name: &str) -> &'static str {
    match name {
        "U" => "0/U",
        "p" => "0/p",
        "T" => "0/T",
        "k" => "0/k",
        "omega" => "0/omega",
        "nut" => "0/nut",
        _ => "0/alphat",
    }
}

/// `controlDict`, schemes, solvers and the fluid: the settings of the Qt
/// application's nozzle practice, which `rhoSimpleFoam` runs.
fn settings() -> Vec<File> {
    let solver = |name: &str| {
        format!(
            "    {name}\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-08;\n        relTol          0;\n    }}\n"
        )
    };
    let mut solvers = String::from(
        "    p\n    {\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-07;\n        relTol          0.01;\n    }\n    pFinal\n    {\n        $p;\n        relTol          0;\n    }\n",
    );
    for name in ["rho", "U", "e", "h", "k", "omega"] {
        solvers.push_str(&solver(name));
    }
    vec![
        File {
            path: "system/controlDict",
            text: format!(
                "{}application     rhoSimpleFoam;\nstartFrom       startTime;\nstartTime       0;\nstopAt          endTime;\nendTime         500;\ndeltaT          1;\nwriteControl    timeStep;\nwriteInterval   100;\npurgeWrite      0;\nwriteFormat     ascii;\nwritePrecision  6;\nwriteCompression off;\ntimeFormat      general;\ntimePrecision   6;\nrunTimeModifiable true;\n",
                header("dictionary", "system", "controlDict")
            ),
        },
        File {
            path: "system/fvSchemes",
            text: format!(
                "{}ddtSchemes\n{{\n    default         steadyState;\n}}\n\ngradSchemes\n{{\n    default         Gauss linear;\n    limited         cellLimited Gauss linear 1;\n    grad(U)         $limited;\n    grad(k)         $limited;\n    grad(omega)     $limited;\n}}\n\ndivSchemes\n{{\n    default         none;\n    div(phi,U)      bounded Gauss linearUpwind grad(U);\n    energy          bounded Gauss linearUpwind grad(U);\n    div(phi,e)      $energy;\n    div(phi,h)      $energy;\n    div(phi,K)      $energy;\n    div(phi,Ekp)    $energy;\n    turbulence      bounded Gauss upwind;\n    div(phi,k)      $turbulence;\n    div(phi,omega)  $turbulence;\n    div(phid,p)     Gauss upwind;\n    div((phi|interpolate(rho)),p)  bounded Gauss upwind;\n    div(((rho*nuEff)*dev2(T(grad(U)))))    Gauss linear;\n}}\n\nlaplacianSchemes\n{{\n    default         Gauss linear corrected;\n}}\n\ninterpolationSchemes\n{{\n    default         linear;\n}}\n\nsnGradSchemes\n{{\n    default         corrected;\n}}\n\nwallDist\n{{\n    method          meshWave;\n}}\n",
                header("dictionary", "system", "fvSchemes")
            ),
        },
        File {
            path: "system/fvSolution",
            text: format!(
                "{}solvers\n{{\n{solvers}}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 0;\n    consistent      yes;\n    residualControl\n    {{\n        p               1e-04;\n        U               1e-05;\n        T               1e-05;\n        k               1e-05;\n        omega           1e-05;\n    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.3;\n        rho             0.5;\n    }}\n    equations\n    {{\n        U               0.7;\n        e               0.7;\n        h               0.7;\n        k               0.7;\n        omega           0.7;\n    }}\n}}\n",
                header("dictionary", "system", "fvSolution")
            ),
        },
        File {
            path: "constant/thermophysicalProperties",
            text: format!(
                "{}thermoType\n{{\n    type            hePsiThermo;\n    mixture         pureMixture;\n    transport       const;\n    thermo          hConst;\n    equationOfState perfectGas;\n    specie          specie;\n    energy          sensibleInternalEnergy;\n}}\n\nmixture\n{{\n    specie\n    {{\n        molWeight       28.9;\n    }}\n    thermodynamics\n    {{\n        Cp              1005;\n        Hf              0;\n    }}\n    transport\n    {{\n        mu              1.82e-05;\n        Pr              0.71;\n    }}\n}}\n",
                header("dictionary", "constant", "thermophysicalProperties")
            ),
        },
        File {
            path: "constant/turbulenceProperties",
            text: format!(
                "{}simulationType RAS;\n\nRAS\n{{\n    RASModel        kOmegaSST;\n    turbulence      on;\n    printCoeffs     on;\n}}\n",
                header("dictionary", "constant", "turbulenceProperties")
            ),
        },
    ]
}
