//! A lid-driven cavity: the square box whose top wall moves, the classic test of a
//! incompressible solver (Ghia, Ghia and Shin, 1982).

use crate::error::Result;
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, count, size};

/// A square cavity of one cell depth, closed on every side, with the lid moving to the right.
#[derive(Debug, Clone, PartialEq)]
pub struct Cavity {
    /// Side of the square, in metres.
    pub side: f64,
    /// The one cell's depth, in metres. The flow is 2D: front and back are `empty`.
    pub depth: f64,
    /// Cells along each side.
    pub cells: usize,
    /// Speed of the lid, in m/s.
    pub lid_speed: f64,
    /// Kinematic viscosity, in m²/s.
    pub viscosity: f64,
}

impl Default for Cavity {
    /// Reynolds number 100 on the side and the lid speed, the first case of Ghia et al.
    fn default() -> Self {
        Self {
            side: 1.0,
            depth: 0.1,
            cells: 48,
            lid_speed: 1.0,
            viscosity: 1.0e-2,
        }
    }
}

impl Cavity {
    /// Sets one number by its name (`side`, `depth`, `cells`, `lid-speed`, `viscosity`), for a
    /// command line or a script.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "side" => self.side = value,
            "depth" => self.depth = value,
            "lid-speed" => self.lid_speed = value,
            "viscosity" => self.viscosity = value,
            "cells" => self.cells = crate::whole(name, value)?,
            _ => return Err(crate::unknown(name, "a cavity")),
        }
        Ok(())
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        let side = size("the side", self.side, 1.0e6)?;
        let depth = size("the depth", self.depth, 1.0e6)?;
        let n = count("the cells along a side", self.cells, 1, 10_000)?;
        let lid = size("the lid speed", self.lid_speed, 1.0e5)?;
        let nu = size("the viscosity", self.viscosity, 1.0e3)?;
        if (n as u64) * (n as u64) > MAX_CELLS {
            return Err(crate::CaseError::Invalid(format!(
                "{n} × {n} cells is more than the {MAX_CELLS} a new case may have"
            )));
        }

        let block_mesh_dict = format!(
            "{}vertices\n(\n    (0 0 0)\n    ({side} 0 0)\n    ({side} {side} 0)\n    (0 {side} 0)\n    (0 0 {depth})\n    ({side} 0 {depth})\n    ({side} {side} {depth})\n    (0 {side} {depth})\n);\n\n\
             blocks\n(\n    hex (0 1 2 3 4 5 6 7) ({n} {n} 1) simpleGrading (1 1 1)\n);\n\n\
             edges\n(\n);\n\n\
             boundary\n(\n\
             \x20   movingWall\n    {{\n        type wall;\n        faces ((3 7 6 2));\n    }}\n\
             \x20   fixedWalls\n    {{\n        type wall;\n        faces ((0 4 7 3) (2 6 5 1) (1 5 4 0));\n    }}\n\
             \x20   frontAndBack\n    {{\n        type empty;\n        faces ((0 3 2 1) (4 5 6 7));\n    }}\n);\n",
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
                    "{}solvers\n{{\n    p\n    {{\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-08;\n        relTol          0.01;\n    }}\n\n    U\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 0;\n    pRefCell        0;\n    pRefValue       0;\n    residualControl\n    {{\n        p               1e-06;\n        U               1e-07;\n    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.3;\n    }}\n    equations\n    {{\n        U               0.7;\n    }}\n}}\n",
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
                    "{}dimensions      [0 1 -1 0 0 0 0];\n\ninternalField   uniform (0 0 0);\n\nboundaryField\n{{\n    movingWall\n    {{\n        type            fixedValue;\n        value           uniform ({lid} 0 0);\n    }}\n    fixedWalls\n    {{\n        type            noSlip;\n    }}\n    frontAndBack\n    {{\n        type            empty;\n    }}\n}}\n",
                    header("volVectorField", "", "U")
                ),
            },
            File {
                path: "0/p",
                text: format!(
                    "{}dimensions      [0 2 -2 0 0 0 0];\n\ninternalField   uniform 0;\n\nboundaryField\n{{\n    movingWall\n    {{\n        type            zeroGradient;\n    }}\n    fixedWalls\n    {{\n        type            zeroGradient;\n    }}\n    frontAndBack\n    {{\n        type            empty;\n    }}\n}}\n",
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
}
