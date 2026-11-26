// SPDX-License-Identifier: GPL-3.0-or-later
//! The body as a solid that conducts heat to the air: conjugate heat transfer around a body.
//!
//! A [`Heating`] gives the body a conductivity (or a material from a short table) and the heat
//! it generates. The case then has two regions: the air in `constant/polyMesh` with its `T`
//! (`conjugate` on the walls) and the body in `constant/solid` with its own `T` and
//! `solidProperties`. The solver couples them (`vortia-solvers`, `conjugate`).

use vortia_foamfile::RawPolyMesh;
use vortia_mesh::PolyMesh;
use vortia_mesh::bodymesh::BodySpec;
use vortia_mesh::geometry::MeshGeometry;

use crate::body::Body;
use crate::error::{CaseError, Result};
use crate::files::{File, header};

/// The temperature of the stream and of the body at the start, in K (20 °C).
pub(crate) const AMBIENT: f64 = 293.15;
/// The Prandtl number of air.
pub(crate) const PRANDTL: f64 = 0.71;
/// Air's density (1.2 kg/m³) times its specific heat (1005 J/kgK), in J/m³K.
pub(crate) const AIR_RHO_CP: f64 = 1206.0;

/// Thermal conductivities at room temperature, in W/mK (textbook values).
const MATERIALS: [(&str, f64); 8] = [
    ("aluminium", 237.0),
    ("copper", 401.0),
    ("glass", 1.0),
    ("plastic", 0.2),
    ("silicon", 148.0),
    ("stainless-steel", 16.0),
    ("steel", 50.0),
    ("titanium", 22.0),
];

/// The materials a body can be made of, by name.
pub fn material_names() -> Vec<&'static str> {
    MATERIALS.iter().map(|(name, _)| *name).collect()
}

pub(crate) fn conductivity(name: &str) -> Result<f64> {
    let key = name.to_ascii_lowercase();
    MATERIALS
        .iter()
        .find(|(n, _)| *n == key)
        .map(|&(_, k)| k)
        .ok_or_else(|| {
            CaseError::Invalid(format!(
                "'{name}' is not a material: {}",
                material_names().join(", ")
            ))
        })
}

/// What the body is made of and how much heat it gives off.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Heating {
    /// Thermal conductivity, in W/mK.
    pub conductivity: f64,
    /// Heat generated inside the body, in W (spread evenly over its volume).
    pub heat: f64,
}

/// A [`Heating`] with everything it needs checked.
pub(crate) struct Checked {
    conductivity: f64,
    heat: f64,
    ambient: f64,
    prandtl: f64,
    rho_cp: f64,
}

impl Heating {
    pub(crate) fn checked(self, ambient: f64, prandtl: f64, rho_cp: f64) -> Result<Checked> {
        let positive = |name: &str, v: f64| {
            if v.is_finite() && v > 0.0 {
                Ok(v)
            } else {
                Err(CaseError::Invalid(format!(
                    "{name} must be a positive number, not {v}"
                )))
            }
        };
        if !self.heat.is_finite() {
            return Err(CaseError::Invalid("the heat must be a number".into()));
        }
        if self.conductivity == 0.0 {
            return Err(CaseError::Invalid(format!(
                "the body needs a conductivity (conductivity=W/mK) or a material: {}",
                material_names().join(", ")
            )));
        }
        Ok(Checked {
            conductivity: positive("the conductivity", self.conductivity)?,
            heat: self.heat,
            ambient: positive("the ambient temperature", ambient)?,
            prandtl: positive("the Prandtl number", prandtl)?,
            rho_cp: positive("the air's rho c_p", rho_cp)?,
        })
    }
}

impl Checked {
    /// Adds the temperature to the fluid's settings: its convection scheme, its solver and the
    /// air's `Pr` and `rhoCp`.
    pub(crate) fn amend(&self, files: &mut [File]) -> Result<()> {
        let edit = |files: &mut [File], path: &str, from: &str, to: String| -> Result<()> {
            let file = files.iter_mut().find(|f| f.path == path).ok_or_else(|| {
                CaseError::Invalid(format!("the case has no {path} to add the temperature to"))
            })?;
            if !file.text.contains(from) {
                return Err(CaseError::Invalid(format!(
                    "{path} has no '{}' to add the temperature after",
                    from.trim()
                )));
            }
            file.text = file.text.replacen(from, &to, 1);
            Ok(())
        };
        edit(
            files,
            "system/fvSchemes",
            "    div((nuEff",
            "    div(phi,T)      bounded Gauss upwind;\n    div((nuEff".into(),
        )?;
        edit(
            files,
            "system/fvSolution",
            "solvers\n{\n",
            "solvers\n{\n    T\n    {\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-10;\n        relTol          0.001;\n    }\n\n".into(),
        )?;
        let path = "constant/transportProperties";
        let at = files
            .iter()
            .find(|f| f.path == path)
            .and_then(|f| f.text.find("nu "))
            .is_some();
        if !at {
            return Err(CaseError::Invalid(format!("{path} has no nu")));
        }
        if let Some(file) = files.iter_mut().find(|f| f.path == path) {
            file.text.push_str(&format!(
                "\nPr              {};\nPrt             0.85;\nrhoCp           {};\n",
                self.prandtl, self.rho_cp
            ));
        }
        Ok(())
    }

    /// The fluid's `T`, the solid's `T` and `solidProperties`.
    pub(crate) fn files(
        &self,
        spec: &BodySpec,
        walls: &[String],
        solid: &RawPolyMesh,
    ) -> Result<Vec<File>> {
        let mesh = PolyMesh::from_raw(solid.clone()).map_err(|e| CaseError::Mesh(e.to_string()))?;
        let volume = MeshGeometry::compute(&mesh).total_volume();
        if !(volume.is_finite() && volume > 0.0) {
            return Err(CaseError::Mesh("the body's mesh has no volume".into()));
        }
        let t = self.ambient;
        let fixed = format!("type            fixedValue;\nvalue           uniform {t};");
        let outlet = format!(
            "type            inletOutlet;\ninletValue      uniform {t};\nvalue           uniform {t};"
        );
        let fluid = Body::field(
            "volScalarField",
            "T",
            "[0 0 0 1 0 0 0]",
            &format!("uniform {t}"),
            spec,
            walls,
            ["type            conjugate;", &fixed, &outlet],
        );
        let solid_t = File {
            path: "0/solid/T",
            text: format!(
                "{}dimensions      [0 0 0 1 0 0 0];\n\ninternalField   uniform {t};\n\nboundaryField\n{{\n    {}\n    {{\n        type            conjugate;\n    }}\n}}\n",
                header("volScalarField", "", "T"),
                spec.body
            ),
        };
        let properties = File {
            path: "constant/solid/solidProperties",
            text: format!(
                "{}k               {};\nQ               {};\nrelaxation      1;\ninterface       {};\n",
                header("dictionary", "constant/solid", "solidProperties"),
                self.conductivity,
                self.heat / volume,
                spec.body
            ),
        };
        Ok(vec![fluid, solid_t, properties])
    }
}
