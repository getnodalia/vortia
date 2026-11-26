//! A jet into the open atmosphere: a small orifice in the wall of a large chamber, the flow
//! leaving it at a pressure above the ambient one.
//!
//! The domain is a thin wedge of a cylinder (the axis collapsed). On the plane `x = 0` the
//! orifice is the inlet, with a total pressure and temperature, and the rest is a wall; the far
//! end and the side are the open atmosphere, where the pressure is the ambient one and the flow
//! may come in or go out. This is the case of `examples/nozzle-practice-2026`, made from its
//! numbers. Lengths are in millimetres.

use std::fmt::Write as _;

use crate::error::{CaseError, Result};
use crate::files::{File, FileSet, header};
use crate::nozzle::settings;
use crate::{MAX_CELLS, count, size};

/// A jet and the conditions it runs at.
#[derive(Debug, Clone, PartialEq)]
pub struct Jet {
    /// Radius of the orifice, in millimetres.
    pub orifice_radius: f64,
    /// How far the atmosphere extends from the wall, in millimetres.
    pub length: f64,
    /// Radius of the atmosphere region, in millimetres.
    pub radius: f64,
    /// Cells along the axis.
    pub cells_axial: usize,
    /// Cells across the orifice.
    pub cells_orifice: usize,
    /// Cells from the orifice to the outer radius.
    pub cells_outer: usize,
    /// Last over first cell width from the orifice to the outer radius.
    pub radial_grading: f64,
    /// The wedge's opening, in degrees (0.01 to 5).
    pub wedge_angle: f64,
    /// Total pressure of the flow entering, in Pa.
    pub total_pressure: f64,
    /// Total temperature of the flow entering, in K.
    pub total_temperature: f64,
    /// Pressure of the atmosphere, in Pa.
    pub ambient_pressure: f64,
}

impl Default for Jet {
    /// The practice case: a 0.1 mm orifice at a pressure ratio of 1.18.
    fn default() -> Self {
        Self {
            orifice_radius: 0.1,
            length: 3.0,
            radius: 1.6,
            cells_axial: 75,
            cells_orifice: 30,
            cells_outer: 60,
            radial_grading: 8.0,
            wedge_angle: 5.0,
            total_pressure: 120_000.0,
            total_temperature: 300.0,
            ambient_pressure: 101_325.0,
        }
    }
}

impl Jet {
    /// Sets one number by its name (`orifice-radius`, `length`, `radius`, `axial-cells`,
    /// `orifice-cells`, `outer-cells`, `radial-grading`, `wedge-angle`, `total-pressure`,
    /// `total-temperature`, `ambient-pressure`), for a command line or a script.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "orifice-radius" => self.orifice_radius = value,
            "length" => self.length = value,
            "radius" => self.radius = value,
            "radial-grading" => self.radial_grading = value,
            "wedge-angle" => self.wedge_angle = value,
            "total-pressure" => self.total_pressure = value,
            "total-temperature" => self.total_temperature = value,
            "ambient-pressure" => self.ambient_pressure = value,
            "axial-cells" => self.cells_axial = crate::whole(name, value)?,
            "orifice-cells" => self.cells_orifice = crate::whole(name, value)?,
            "outer-cells" => self.cells_outer = crate::whole(name, value)?,
            _ => return Err(crate::unknown(name, "a jet")),
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        size("the orifice radius", self.orifice_radius, 1.0e6)?;
        size("the length", self.length, 1.0e6)?;
        size("the radius", self.radius, 1.0e6)?;
        if self.radius <= self.orifice_radius {
            return Err(CaseError::Invalid(
                "the atmosphere must be wider than the orifice".into(),
            ));
        }
        count("the axial cells", self.cells_axial, 1, 2000)?;
        count("the cells across the orifice", self.cells_orifice, 1, 1000)?;
        count("the cells outside the orifice", self.cells_outer, 1, 1000)?;
        size("the radial grading", self.radial_grading, 1000.0)?;
        if !(self.wedge_angle > 0.01 && self.wedge_angle <= 5.0) {
            return Err(CaseError::Invalid(format!(
                "the wedge angle must be above 0.01° and at most 5°, not {}",
                self.wedge_angle
            )));
        }
        size("the total pressure", self.total_pressure, 1.0e10)?;
        size("the total temperature", self.total_temperature, 1.0e5)?;
        size("the ambient pressure", self.ambient_pressure, 1.0e10)?;
        let cells = self.cells_axial as u64 * (self.cells_orifice + self.cells_outer) as u64;
        if cells > MAX_CELLS {
            return Err(CaseError::Invalid(format!(
                "{cells} cells is more than the {MAX_CELLS} a new case may have"
            )));
        }
        Ok(())
    }

    /// The jet's `blockMeshDict`: two blocks across (the orifice, then the rest), one along.
    fn block_mesh_dict(&self) -> String {
        let half = self.wedge_angle.to_radians() / 2.0;
        let (ax, o, r) = (self.cells_axial, self.cells_orifice, self.cells_outer);
        let grading = self.radial_grading;
        // Per end of the chamber: the axis, then each radius as its two sides of the wedge.
        let (l, r0, r1) = (self.length, self.orifice_radius, self.radius);
        let mut s = header("dictionary", "", "blockMeshDict");
        s.push_str("scale   0.001;\n\nvertices\n(\n");
        for x in [0.0, l] {
            let _ = writeln!(s, "    ({x} 0 0)");
            for radius in [r0, r1] {
                let (y, z) = (radius * half.cos(), radius * half.sin());
                let _ = writeln!(s, "    ({x} {y} {})\n    ({x} {y} {z})", -z);
            }
        }
        s.push_str(");\n\n");
        // Vertex numbers: the end x = 0 is 0 (axis), 1 and 2 (orifice, minus and plus side), 3
        // and 4 (outer radius); the end x = L adds five.
        let at = |end: usize, level: usize, plus: bool| 5 * end + 1 + 2 * level + usize::from(plus);
        s.push_str("blocks\n(\n");
        let _ = writeln!(
            s,
            "    hex (0 5 {} {} 0 5 {} {}) ({ax} {o} 1) simpleGrading (1 1 1)",
            at(1, 0, false),
            at(0, 0, false),
            at(1, 0, true),
            at(0, 0, true),
        );
        let _ = writeln!(
            s,
            "    hex ({} {} {} {} {} {} {} {}) ({ax} {r} 1) simpleGrading (1 {grading} 1)",
            at(0, 0, false),
            at(1, 0, false),
            at(1, 1, false),
            at(0, 1, false),
            at(0, 0, true),
            at(1, 0, true),
            at(1, 1, true),
            at(0, 1, true),
        );
        s.push_str(");\n\nedges\n(\n);\n\nboundary\n(\n");
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
        patch("axis", "empty", &[[0, 5, 5, 0]]);
        patch("inlet", "patch", &[[0, 0, at(0, 0, true), at(0, 0, false)]]);
        patch(
            "walls",
            "wall",
            &[[
                at(0, 0, false),
                at(0, 0, true),
                at(0, 1, true),
                at(0, 1, false),
            ]],
        );
        patch(
            "atmosphere",
            "patch",
            &[
                // The far end of both blocks, then the outer radius.
                [5, at(1, 0, false), at(1, 0, true), 5],
                [
                    at(1, 0, false),
                    at(1, 1, false),
                    at(1, 1, true),
                    at(1, 0, true),
                ],
                [
                    at(0, 1, false),
                    at(0, 1, true),
                    at(1, 1, true),
                    at(1, 1, false),
                ],
            ],
        );
        patch(
            "front",
            "wedge",
            &[
                [0, at(0, 0, false), at(1, 0, false), 5],
                [
                    at(0, 0, false),
                    at(0, 1, false),
                    at(1, 1, false),
                    at(1, 0, false),
                ],
            ],
        );
        patch(
            "back",
            "wedge",
            &[
                [0, 5, at(1, 0, true), at(0, 0, true)],
                [
                    at(0, 0, true),
                    at(1, 0, true),
                    at(1, 1, true),
                    at(0, 1, true),
                ],
            ],
        );
        s.push_str(");\n");
        s
    }

    fn field(
        class: &str,
        name: &'static str,
        path: &'static str,
        dimensions: &str,
        internal: &str,
        bodies: [&str; 3],
    ) -> File {
        let mut s = header(class, "", name);
        let _ = write!(
            s,
            "dimensions      {dimensions};\n\ninternalField   {internal};\n\nboundaryField\n{{\n"
        );
        let [inlet, walls, atmosphere] = bodies;
        for (patch, text) in [
            ("axis", "type            empty;"),
            ("inlet", inlet),
            ("walls", walls),
            ("atmosphere", atmosphere),
            ("front", "type            wedge;"),
            ("back", "type            wedge;"),
        ] {
            let _ = write!(s, "    {patch}\n    {{\n");
            for line in text.lines() {
                let _ = writeln!(s, "        {}", line.trim_start());
            }
            s.push_str("    }\n");
        }
        s.push_str("}\n");
        File { path, text: s }
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        self.validate()?;
        let block_mesh_dict = self.block_mesh_dict();
        let (p0, t0, pa) = (
            self.total_pressure,
            self.total_temperature,
            self.ambient_pressure,
        );
        // Ten times the orifice's own scale, as the practice case has it.
        let mixing = 0.1 * self.orifice_radius / 1000.0;
        let mut files = vec![
            File {
                path: "system/blockMeshDict",
                text: block_mesh_dict.clone(),
            },
            Self::field(
                "volVectorField",
                "U",
                "0/U",
                "[0 1 -1 0 0 0 0]",
                "uniform (0 0 0)",
                [
                    "type            pressureInletVelocity;\nvalue           uniform (0 0 0);",
                    "type            noSlip;",
                    "type            pressureInletOutletVelocity;\nvalue           uniform (0 0 0);",
                ],
            ),
            Self::field(
                "volScalarField",
                "p",
                "0/p",
                "[1 -1 -2 0 0 0 0]",
                &format!("uniform {pa}"),
                [
                    &format!(
                        "type            totalPressure;\np0              uniform {p0};\ngamma           1.4;\nvalue           uniform {p0};"
                    ),
                    "type            zeroGradient;",
                    &format!("type            fixedValue;\nvalue           uniform {pa};"),
                ],
            ),
            Self::field(
                "volScalarField",
                "T",
                "0/T",
                "[0 0 0 1 0 0 0]",
                &format!("uniform {t0}"),
                [
                    &format!(
                        "type            totalTemperature;\nT0              uniform {t0};\ngamma           1.4;\nvalue           uniform {t0};"
                    ),
                    "type            zeroGradient;",
                    &format!(
                        "type            inletOutlet;\ninletValue      uniform {t0};\nvalue           uniform {t0};"
                    ),
                ],
            ),
            Self::field(
                "volScalarField",
                "k",
                "0/k",
                "[0 2 -2 0 0 0 0]",
                "uniform 1",
                [
                    "type            turbulentIntensityKineticEnergyInlet;\nintensity       0.05;\nvalue           uniform 1;",
                    "type            kqRWallFunction;\nvalue           uniform 1;",
                    "type            inletOutlet;\ninletValue      uniform 1;\nvalue           uniform 1;",
                ],
            ),
            Self::field(
                "volScalarField",
                "omega",
                "0/omega",
                "[0 0 -1 0 0 0 0]",
                "uniform 1000",
                [
                    &format!(
                        "type            turbulentMixingLengthFrequencyInlet;\nmixingLength    {mixing};\nvalue           uniform 1000;"
                    ),
                    "type            omegaWallFunction;\nvalue           uniform 1000;",
                    "type            inletOutlet;\ninletValue      uniform 1000;\nvalue           uniform 1000;",
                ],
            ),
            Self::field(
                "volScalarField",
                "nut",
                "0/nut",
                "[0 2 -1 0 0 0 0]",
                "uniform 0",
                [
                    "type            calculated;\nvalue           uniform 0;",
                    "type            nutkWallFunction;\nvalue           uniform 0;",
                    "type            calculated;\nvalue           uniform 0;",
                ],
            ),
            Self::field(
                "volScalarField",
                "alphat",
                "0/alphat",
                "[1 -1 -1 0 0 0 0]",
                "uniform 0",
                [
                    "type            calculated;\nvalue           uniform 0;",
                    "type            compressible::alphatWallFunction;\nvalue           uniform 0;",
                    "type            calculated;\nvalue           uniform 0;",
                ],
            ),
        ];
        files.extend(settings());
        Ok(FileSet {
            files,
            block_mesh_dict,
            raw_mesh: None,
            solid_mesh: None,
            application: "rhoSimpleFoam",
        })
    }
}
