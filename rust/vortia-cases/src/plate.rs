//! A flat plate in a uniform stream: the laminar boundary layer that grows from its leading
//! edge, whose solution Blasius found exactly.

use crate::error::Result;
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, count, size};

/// A plate along the bottom of a box, one cell deep, with the stream coming in on the left over
/// a stretch of slip wall, so that the plate starts inside the flow and not on the inlet.
#[derive(Debug, Clone, PartialEq)]
pub struct Plate {
    /// Length of the plate (and of the box), in metres.
    pub length: f64,
    /// Length of the slip wall ahead of the plate, in metres: the stream meets the leading edge
    /// as it would in open air, and not as a uniform profile cut at the corner.
    pub upstream: f64,
    /// Height of the box above the plate, in metres: well above the boundary layer.
    pub height: f64,
    /// The one cell's depth, in metres. The flow is 2D: front and back are `empty`.
    pub depth: f64,
    pub cells_x: usize,
    /// Cells along the slip wall ahead of the plate.
    pub cells_up: usize,
    pub cells_y: usize,
    /// How much taller the cell at the top is than the one at the plate.
    pub grading: f64,
    /// Speed of the stream, in m/s.
    pub speed: f64,
    /// Kinematic viscosity, in m²/s.
    pub viscosity: f64,
    /// Turbulent from the leading edge (k-ω SST with wall functions) instead of laminar. Then the
    /// box starts at the leading edge: there is no slip wall ahead, so `upstream` and `cells_up`
    /// are not used.
    pub turbulent: bool,
    /// Free-stream turbulence intensity of the turbulent plate (a fraction of the stream speed).
    pub intensity: f64,
    /// Mixing length of the free-stream eddies of the turbulent plate, in metres.
    pub length_scale: f64,
}

impl Default for Plate {
    /// Reynolds number 10⁵ on the length: a laminar layer a fiftieth of the box high at the end of
    /// the plate, with cells packed against it.
    fn default() -> Self {
        Self {
            length: 1.0,
            upstream: 0.2,
            height: 0.5,
            depth: 0.01,
            cells_x: 100,
            cells_up: 20,
            cells_y: 100,
            grading: 300.0,
            speed: 1.0,
            viscosity: 1.0e-5,
            turbulent: false,
            intensity: 0.01,
            length_scale: 1.0e-3,
        }
    }
}

impl Plate {
    /// The turbulent plate of `benchmarks/turbulent-flat-plate-sst`: ten metres long at 10 m/s in
    /// air, Reynolds number 10⁷, cells packed against the plate so that the first one is in the
    /// logarithmic layer, as wall functions require.
    pub fn turbulent() -> Self {
        Self {
            length: 10.0,
            upstream: 0.0,
            height: 2.0,
            depth: 0.1,
            cells_x: 200,
            cells_up: 0,
            cells_y: 60,
            grading: 100.0,
            speed: 10.0,
            viscosity: 1.0e-5,
            turbulent: true,
            intensity: 0.01,
            length_scale: 1.0e-3,
        }
    }

    /// Sets one number by its name (`length`, `upstream`, `height`, `depth`, `cells-x`,
    /// `cells-up`, `cells-y`, `grading`, `speed`, `viscosity`, `intensity`, `length-scale`), for a command line or a script.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "length" => self.length = value,
            "upstream" => self.upstream = value,
            "height" => self.height = value,
            "depth" => self.depth = value,
            "grading" => self.grading = value,
            "speed" => self.speed = value,
            "viscosity" => self.viscosity = value,
            "intensity" => self.intensity = value,
            "length-scale" => self.length_scale = value,
            "cells-x" => self.cells_x = crate::whole(name, value)?,
            "cells-up" => self.cells_up = crate::whole(name, value)?,
            "cells-y" => self.cells_y = crate::whole(name, value)?,
            _ => return Err(crate::unknown(name, "a plate")),
        }
        Ok(())
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        if self.turbulent {
            return self.turbulent_files();
        }
        let length = size("the length", self.length, 1.0e6)?;
        let upstream = size("the slip wall ahead of the plate", self.upstream, 1.0e6)?;
        let height = size("the height", self.height, 1.0e6)?;
        let depth = size("the depth", self.depth, 1.0e6)?;
        let nx = count("the cells along the plate", self.cells_x, 1, 10_000)?;
        let nu_up = count("the cells ahead of the plate", self.cells_up, 1, 10_000)?;
        let ny = count("the cells above the plate", self.cells_y, 1, 10_000)?;
        let grading = size("the grading", self.grading, 1.0e4)?;
        let speed = size("the stream speed", self.speed, 1.0e5)?;
        let nu = size("the viscosity", self.viscosity, 1.0e3)?;
        if ((nx + nu_up) as u64) * (ny as u64) > MAX_CELLS {
            return Err(crate::CaseError::Invalid(format!(
                "{} × {ny} cells is more than the {MAX_CELLS} a new case may have",
                nx + nu_up
            )));
        }

        let block_mesh_dict = format!(
            "{}vertices\n(\n    (-{upstream} 0 0)\n    (0 0 0)\n    ({length} 0 0)\n    ({length} {height} 0)\n    (0 {height} 0)\n    (-{upstream} {height} 0)\n    (-{upstream} 0 {depth})\n    (0 0 {depth})\n    ({length} 0 {depth})\n    ({length} {height} {depth})\n    (0 {height} {depth})\n    (-{upstream} {height} {depth})\n);\n\n\
             blocks\n(\n    hex (0 1 4 5 6 7 10 11) ({nu_up} {ny} 1) simpleGrading (1 {grading} 1)\n    hex (1 2 3 4 7 8 9 10) ({nx} {ny} 1) simpleGrading (1 {grading} 1)\n);\n\n\
             edges\n(\n);\n\n\
             boundary\n(\n\
             \x20   inlet\n    {{\n        type patch;\n        faces ((0 6 11 5));\n    }}\n\
             \x20   outlet\n    {{\n        type patch;\n        faces ((2 3 9 8));\n    }}\n\
             \x20   slip\n    {{\n        type patch;\n        faces ((0 1 7 6));\n    }}\n\
             \x20   plate\n    {{\n        type wall;\n        faces ((1 2 8 7));\n    }}\n\
             \x20   top\n    {{\n        type patch;\n        faces ((5 11 10 4) (4 10 9 3));\n    }}\n\
             \x20   frontAndBack\n    {{\n        type empty;\n        faces ((0 5 4 1) (1 4 3 2) (6 7 10 11) (7 8 9 10));\n    }}\n);\n",
            header("dictionary", "", "blockMeshDict"),
        );

        let files = vec![
            File {
                path: "system/blockMeshDict",
                text: block_mesh_dict.clone(),
            },
            File {
                path: "system/controlDict",
                text: format!(
                    "{}application     simpleFoam;\nstartFrom       startTime;\nstartTime       0;\nstopAt          endTime;\nendTime         3000;\ndeltaT          1;\nwriteControl    timeStep;\nwriteInterval   100;\npurgeWrite      0;\nwriteFormat     ascii;\nwritePrecision  8;\ntimeFormat      general;\ntimePrecision   6;\nrunTimeModifiable true;\n",
                    header("dictionary", "system", "controlDict")
                ),
            },
            File {
                path: "system/fvSchemes",
                text: format!(
                    "{}ddtSchemes\n{{\n    default         steadyState;\n}}\n\ngradSchemes\n{{\n    default         Gauss linear;\n}}\n\ndivSchemes\n{{\n    default         none;\n    div(phi,U)      bounded Gauss linearUpwind grad(U);\n    div((nuEff*dev2(T(grad(U))))) Gauss linear;\n}}\n\nlaplacianSchemes\n{{\n    default         Gauss linear corrected;\n}}\n\ninterpolationSchemes\n{{\n    default         linear;\n}}\n\nsnGradSchemes\n{{\n    default         corrected;\n}}\n",
                    header("dictionary", "system", "fvSchemes")
                ),
            },
            File {
                path: "system/fvSolution",
                text: format!(
                    "{}solvers\n{{\n    p\n    {{\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-08;\n        relTol          0.01;\n    }}\n\n    U\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 0;\n    residualControl\n    {{\n        p               1e-06;\n        U               1e-07;\n    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.3;\n    }}\n    equations\n    {{\n        U               0.7;\n    }}\n}}\n",
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
                    "{}simulationType laminar;\n",
                    header("dictionary", "constant", "turbulenceProperties")
                ),
            },
            File {
                path: "0/U",
                text: format!(
                    "{}dimensions      [0 1 -1 0 0 0 0];\n\ninternalField   uniform ({speed} 0 0);\n\nboundaryField\n{{\n    inlet\n    {{\n        type            fixedValue;\n        value           uniform ({speed} 0 0);\n    }}\n    outlet\n    {{\n        type            zeroGradient;\n    }}\n    slip\n    {{\n        type            slip;\n    }}\n    plate\n    {{\n        type            noSlip;\n    }}\n    top\n    {{\n        type            zeroGradient;\n    }}\n    frontAndBack\n    {{\n        type            empty;\n    }}\n}}\n",
                    header("volVectorField", "", "U")
                ),
            },
            File {
                path: "0/p",
                text: format!(
                    "{}dimensions      [0 2 -2 0 0 0 0];\n\ninternalField   uniform 0;\n\nboundaryField\n{{\n    inlet\n    {{\n        type            zeroGradient;\n    }}\n    outlet\n    {{\n        type            fixedValue;\n        value           uniform 0;\n    }}\n    slip\n    {{\n        type            zeroGradient;\n    }}\n    plate\n    {{\n        type            zeroGradient;\n    }}\n    top\n    {{\n        type            fixedValue;\n        value           uniform 0;\n    }}\n    frontAndBack\n    {{\n        type            empty;\n    }}\n}}\n",
                    header("volScalarField", "", "p")
                ),
            },
        ];
        Ok(FileSet {
            files,
            block_mesh_dict,
            application: "simpleFoam",
        })
    }

    /// The turbulent plate: one block from the leading edge, `inlet`, `outlet`, `plate` and
    /// `freestream` (the far boundary, where the displaced fluid leaves at ambient pressure).
    fn turbulent_files(&self) -> Result<FileSet> {
        let length = size("the length", self.length, 1.0e6)?;
        let height = size("the height", self.height, 1.0e6)?;
        let depth = size("the depth", self.depth, 1.0e6)?;
        let nx = count("the cells along the plate", self.cells_x, 1, 10_000)?;
        let ny = count("the cells above the plate", self.cells_y, 1, 10_000)?;
        let grading = size("the grading", self.grading, 1.0e4)?;
        let speed = size("the stream speed", self.speed, 1.0e5)?;
        let nu = size("the viscosity", self.viscosity, 1.0e3)?;
        let intensity = size("the turbulence intensity", self.intensity, 1.0)?;
        let mixing = size("the mixing length", self.length_scale, 1.0e6)?;
        if (nx as u64) * (ny as u64) > MAX_CELLS {
            return Err(crate::CaseError::Invalid(format!(
                "{nx} × {ny} cells is more than the {MAX_CELLS} a new case may have"
            )));
        }
        // Inside the box, before the inlet's own conditions are imposed: the stream's eddies.
        let k = 1.5 * (speed * intensity).powi(2);
        let omega = k.sqrt() / (0.09_f64.powf(0.25) * mixing);

        let block_mesh_dict = format!(
            "{}vertices\n(\n    (0 0 0)\n    ({length} 0 0)\n    ({length} {height} 0)\n    (0 {height} 0)\n    (0 0 {depth})\n    ({length} 0 {depth})\n    ({length} {height} {depth})\n    (0 {height} {depth})\n);\n\n\
             blocks\n(\n    hex (0 1 2 3 4 5 6 7) ({nx} {ny} 1) simpleGrading (1 {grading} 1)\n);\n\n\
             edges\n(\n);\n\n\
             boundary\n(\n\
             \x20   inlet\n    {{\n        type patch;\n        faces ((0 4 7 3));\n    }}\n\
             \x20   outlet\n    {{\n        type patch;\n        faces ((1 2 6 5));\n    }}\n\
             \x20   plate\n    {{\n        type wall;\n        faces ((0 1 5 4));\n    }}\n\
             \x20   freestream\n    {{\n        type patch;\n        faces ((3 7 6 2));\n    }}\n\
             \x20   frontAndBack\n    {{\n        type empty;\n        faces ((0 3 2 1) (4 5 6 7));\n    }}\n);\n",
            header("dictionary", "", "blockMeshDict"),
        );

        let field = |class: &str,
                     name: &'static str,
                     dims: &str,
                     internal: &str,
                     patches: [&str; 4]| {
            let [inlet, outlet, plate, freestream] = patches;
            let block = |patch: &str, body: &str| {
                let mut s = format!("    {patch}\n    {{\n");
                for line in body.lines() {
                    s.push_str(&format!("        {}\n", line.trim_start()));
                }
                s.push_str("    }\n");
                s
            };
            let text = format!(
                "{}dimensions      {dims};\n\ninternalField   {internal};\n\nboundaryField\n{{\n{}{}{}{}{}}}\n",
                header(class, "", name),
                block("inlet", inlet),
                block("outlet", outlet),
                block("plate", plate),
                block("freestream", freestream),
                block("frontAndBack", "type            empty;"),
            );
            File {
                path: match name {
                    "U" => "0/U",
                    "p" => "0/p",
                    "k" => "0/k",
                    "omega" => "0/omega",
                    _ => "0/nut",
                },
                text,
            }
        };
        let inlet_out = |v: &str| {
            format!(
                "type            inletOutlet;\ninletValue      uniform {v};\nvalue           uniform {v};"
            )
        };
        let mut files = vec![
            File {
                path: "system/blockMeshDict",
                text: block_mesh_dict.clone(),
            },
            field(
                "volVectorField",
                "U",
                "[0 1 -1 0 0 0 0]",
                &format!("uniform ({speed} 0 0)"),
                [
                    &format!("type            fixedValue;\nvalue           uniform ({speed} 0 0);"),
                    "type            zeroGradient;",
                    "type            noSlip;",
                    "type            zeroGradient;",
                ],
            ),
            field(
                "volScalarField",
                "p",
                "[0 2 -2 0 0 0 0]",
                "uniform 0",
                [
                    "type            zeroGradient;",
                    "type            fixedValue;\nvalue           uniform 0;",
                    "type            zeroGradient;",
                    "type            fixedValue;\nvalue           uniform 0;",
                ],
            ),
            field(
                "volScalarField",
                "k",
                "[0 2 -2 0 0 0 0]",
                &format!("uniform {k}"),
                [
                    &format!("type            fixedValue;\nvalue           uniform {k};"),
                    &inlet_out(&k.to_string()),
                    &format!("type            kqRWallFunction;\nvalue           uniform {k};"),
                    &inlet_out(&k.to_string()),
                ],
            ),
            field(
                "volScalarField",
                "omega",
                "[0 0 -1 0 0 0 0]",
                &format!("uniform {omega}"),
                [
                    &format!("type            fixedValue;\nvalue           uniform {omega};"),
                    &inlet_out(&omega.to_string()),
                    &format!(
                        "type            omegaWallFunction;\nvalue           uniform {omega};"
                    ),
                    &inlet_out(&omega.to_string()),
                ],
            ),
            field(
                "volScalarField",
                "nut",
                "[0 2 -1 0 0 0 0]",
                &format!("uniform {}", k / omega),
                [
                    "type            calculated;\nvalue           uniform 0;",
                    "type            calculated;\nvalue           uniform 0;",
                    "type            nutkWallFunction;\nvalue           uniform 0;",
                    "type            calculated;\nvalue           uniform 0;",
                ],
            ),
        ];
        files.extend([
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
                    "{}ddtSchemes\n{{\n    default         steadyState;\n}}\n\ngradSchemes\n{{\n    default         Gauss linear;\n}}\n\ndivSchemes\n{{\n    default         none;\n    div(phi,U)      bounded Gauss linearUpwind grad(U);\n    div(phi,k)      bounded Gauss upwind;\n    div(phi,omega)  bounded Gauss upwind;\n    div((nuEff*dev2(T(grad(U))))) Gauss linear;\n}}\n\nlaplacianSchemes\n{{\n    default         Gauss linear corrected;\n}}\n\ninterpolationSchemes\n{{\n    default         linear;\n}}\n\nsnGradSchemes\n{{\n    default         corrected;\n}}\n\nwallDist\n{{\n    method          meshWave;\n}}\n",
                    header("dictionary", "system", "fvSchemes")
                ),
            },
            File {
                path: "system/fvSolution",
                text: format!(
                    "{}solvers\n{{\n    p\n    {{\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-08;\n        relTol          0.01;\n    }}\n\n    U\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n\n    k\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n\n    omega\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 0;\n    residualControl\n    {{\n        p               1e-05;\n        U               1e-06;\n        k               1e-06;\n        omega           1e-06;\n    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.3;\n    }}\n    equations\n    {{\n        U               0.7;\n        k               0.7;\n        omega           0.7;\n    }}\n}}\n",
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
                    "{}simulationType RAS;\n\nRAS\n{{\n    RASModel        kOmegaSST;\n    turbulence      on;\n    printCoeffs     on;\n}}\n",
                    header("dictionary", "constant", "turbulenceProperties")
                ),
            },
        ]);
        Ok(FileSet {
            files,
            block_mesh_dict,
            application: "simpleFoam",
        })
    }
}
