//! A straight channel: the box domain of a new case.

use crate::error::Result;
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, count, size};

/// A 2D channel, one cell deep, with the fluid coming in on the left.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// Streamwise length, in metres.
    pub length: f64,
    /// Height between the two walls, in metres.
    pub height: f64,
    /// The one cell's depth, in metres. It only sets the cell volume: the
    /// flow is 2D, the front and back patches are `empty`.
    pub depth: f64,
    pub cells_x: usize,
    pub cells_y: usize,
    /// Inlet speed, in m/s.
    pub speed: f64,
    /// Kinematic viscosity, in m²/s.
    pub viscosity: f64,
}

impl Default for Channel {
    /// Reynolds number 100 on the height: a case that converges in seconds.
    fn default() -> Self {
        Self {
            length: 1.0,
            height: 0.1,
            depth: 0.01,
            cells_x: 100,
            cells_y: 20,
            speed: 1.0,
            viscosity: 1.0e-3,
        }
    }
}

impl Channel {
    /// Sets one number by its name (`length`, `height`, `depth`, `cells-x`,
    /// `cells-y`, `speed`, `viscosity`), for a command line or a script.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "length" => self.length = value,
            "height" => self.height = value,
            "depth" => self.depth = value,
            "speed" => self.speed = value,
            "viscosity" => self.viscosity = value,
            "cells-x" => self.cells_x = crate::whole(name, value)?,
            "cells-y" => self.cells_y = crate::whole(name, value)?,
            _ => return Err(crate::unknown(name, "a channel")),
        }
        Ok(())
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        let length = size("the length", self.length, 1.0e6)?;
        let height = size("the height", self.height, 1.0e6)?;
        let depth = size("the depth", self.depth, 1.0e6)?;
        let nx = count("the cells along the channel", self.cells_x, 1, 10_000)?;
        let ny = count("the cells across the channel", self.cells_y, 1, 10_000)?;
        let speed = size("the inlet speed", self.speed, 1.0e5)?;
        let nu = size("the viscosity", self.viscosity, 1.0e3)?;
        if (nx as u64) * (ny as u64) > MAX_CELLS {
            return Err(crate::CaseError::Invalid(format!(
                "{nx} × {ny} cells is more than the {MAX_CELLS} a new case may have"
            )));
        }

        let block_mesh_dict = format!(
            "{}vertices\n(\n    (0 0 0)\n    ({length} 0 0)\n    ({length} {height} 0)\n    (0 {height} 0)\n    (0 0 {depth})\n    ({length} 0 {depth})\n    ({length} {height} {depth})\n    (0 {height} {depth})\n);\n\n\
             blocks\n(\n    hex (0 1 2 3 4 5 6 7) ({nx} {ny} 1) simpleGrading (1 1 1)\n);\n\n\
             edges\n(\n);\n\n\
             boundary\n(\n\
             \x20   inlet\n    {{\n        type patch;\n        faces ((0 4 7 3));\n    }}\n\
             \x20   outlet\n    {{\n        type patch;\n        faces ((1 2 6 5));\n    }}\n\
             \x20   walls\n    {{\n        type wall;\n        faces ((0 1 5 4) (3 7 6 2));\n    }}\n\
             \x20   frontAndBack\n    {{\n        type empty;\n        faces ((0 3 2 1) (4 5 6 7));\n    }}\n);\n",
            header("dictionary", "", "blockMeshDict"),
        );

        let mut files = vec![File {
            path: "system/blockMeshDict",
            text: block_mesh_dict.clone(),
        }];
        files.extend(laminar_settings(nu));
        files.extend(vec![
            File {
                path: "0/U",
                text: format!(
                    "{}dimensions      [0 1 -1 0 0 0 0];\n\ninternalField   uniform ({speed} 0 0);\n\nboundaryField\n{{\n    inlet\n    {{\n        type            fixedValue;\n        value           uniform ({speed} 0 0);\n    }}\n    outlet\n    {{\n        type            zeroGradient;\n    }}\n    walls\n    {{\n        type            noSlip;\n    }}\n    frontAndBack\n    {{\n        type            empty;\n    }}\n}}\n",
                    header("volVectorField", "", "U")
                ),
            },
            File {
                path: "0/p",
                text: format!(
                    "{}dimensions      [0 2 -2 0 0 0 0];\n\ninternalField   uniform 0;\n\nboundaryField\n{{\n    inlet\n    {{\n        type            zeroGradient;\n    }}\n    outlet\n    {{\n        type            fixedValue;\n        value           uniform 0;\n    }}\n    walls\n    {{\n        type            zeroGradient;\n    }}\n    frontAndBack\n    {{\n        type            empty;\n    }}\n}}\n",
                    header("volScalarField", "", "p")
                ),
            },
        ]);
        Ok(FileSet {
            files,
            block_mesh_dict,
            raw_mesh: None,
            application: "simpleFoam",
        })
    }
}

/// The settings of a laminar incompressible steady case (`simpleFoam`): the control dictionary,
/// the schemes, the solvers and the viscosity.
pub(crate) fn laminar_settings(nu: f64) -> Vec<File> {
    vec![
        File {
            path: "system/controlDict",
            text: format!(
                "{}application     simpleFoam;\nstartFrom       startTime;\nstartTime       0;\nstopAt          endTime;\nendTime         2000;\ndeltaT          1;\nwriteControl    timeStep;\nwriteInterval   100;\npurgeWrite      0;\nwriteFormat     ascii;\nwritePrecision  8;\ntimeFormat      general;\ntimePrecision   6;\nrunTimeModifiable true;\n",
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
                "{}solvers\n{{\n    p\n    {{\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-07;\n        relTol          0.05;\n    }}\n\n    U\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-08;\n        relTol          0.1;\n    }}\n}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 0;\n    residualControl\n    {{\n        p               1e-05;\n        U               1e-06;\n    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.3;\n    }}\n    equations\n    {{\n        U               0.7;\n    }}\n}}\n",
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
    ]
}
