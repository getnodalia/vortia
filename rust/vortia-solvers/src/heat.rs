// SPDX-License-Identifier: MIT OR Apache-2.0
//! Heat carried by a steady incompressible flow: the temperature `T` as a
//! scalar that the flow convects and diffuses,
//!
//! `∇·(φ T) − ∇·(α_eff ∇T) = 0`,  `α_eff = ν/Pr + ν_t/Prt`.
//!
//! It is the energy equation of OpenFOAM's `buoyantBoussinesqSimpleFoam`
//! without the buoyancy: the temperature does not act on the flow. It is on
//! when the start time has a `T` and `constant/transportProperties` gives the
//! laminar Prandtl number `Pr` (the turbulent one, `Prt`, is 0.85 unless it
//! says otherwise). On a wall with a ν_t wall function the face value of
//! `ν_t` is the one the momentum equation uses, so the heat flux through the
//! wall follows the same law as the stress: `α_eff` there is
//! `ν/Pr + ν_t,w/Prt`.
//!
//! This is the constant-`Prt` law without a thermal sublayer correction, so
//! it is as good as `Prt` is for the fluid: close for gases and for a
//! boundary layer in water only to the extent that `Pr` is near one.

use vortia_foamfile::Dictionary;
use vortia_fv::{FvGeometry, Performance};
use vortia_mesh::PolyMesh;

use crate::case::{Case, read_dict};
use crate::error::{Result, SolverError};
use crate::rans::{EquationSetup, InletSource, Kind, ModelField, solve, transport};

/// `[0 0 0 1 0 0 0]`.
const TEMPERATURE: [i32; 7] = [0, 0, 0, 1, 0, 0, 0];

/// The Prandtl number of the turbulent heat flux when `transportProperties`
/// has no `Prt`.
pub(crate) const DEFAULT_PRT: f64 = 0.85;

/// The temperature of a run and what it needs to be solved.
pub(crate) struct Heat {
    t: ModelField,
    setup: EquationSetup,
    nu: f64,
    pr: f64,
    prt: f64,
}

/// A number of `transportProperties`, with or without its dimensions.
fn number(props: &Dictionary, key: &str) -> Option<f64> {
    props
        .values(key)
        .and_then(|v| v.last())
        .and_then(vortia_foamfile::Value::as_scalar)
}

impl Heat {
    /// The temperature, when the case has one to solve: a `T` in the start
    /// time and a `Pr` in `transportProperties`. `None` otherwise, so cases
    /// that carry a `T` for another reason are left alone.
    pub(crate) fn read(
        case: &Case,
        mesh: &PolyMesh,
        start: &std::path::Path,
        nu: f64,
        relax: impl Fn(&str) -> Option<f64>,
    ) -> Result<Option<Self>> {
        let props = read_dict(&case.dir.join("constant").join("transportProperties"))?;
        let Some(pr) = number(&props, "Pr") else {
            return Ok(None);
        };
        if !(start.join("T").is_file() || start.join("T.gz").is_file()) {
            return Ok(None);
        }
        let prt = number(&props, "Prt").unwrap_or(DEFAULT_PRT);
        for (name, value) in [("Pr", pr), ("Prt", prt)] {
            if !(value.is_finite() && value > 0.0) {
                return Err(SolverError::Setup(format!(
                    "transportProperties: {name} must be positive"
                )));
            }
        }
        let t = ModelField::read(mesh, start, "T", TEMPERATURE, |pf| match pf.kind.as_str() {
            "fixedValue" => Some(Kind::Fixed(Vec::new())),
            "zeroGradient" => Some(Kind::ZeroGradient),
            "inletOutlet" => Some(Kind::Mixed {
                source: InletSource::Value,
                ref_value: Vec::new(),
                fraction: Vec::new(),
            }),
            _ => None,
        })?;
        let setup = EquationSetup::read(case, "T", "alphaEff", relax("T"))?;
        Ok(Some(Self {
            t,
            setup,
            nu,
            pr,
            prt,
        }))
    }

    /// `α_eff` on every face from `ν + ν_t` there.
    fn alpha_eff(&self, nu_eff_faces: &[f64]) -> Vec<f64> {
        nu_eff_faces
            .iter()
            .map(|&n| (n - self.nu).max(0.0) / self.prt + self.nu / self.pr)
            .collect()
    }

    /// One solve of the temperature with the flux `phi` and the effective
    /// viscosity of the faces `nu_eff_faces` (`ν` alone when laminar).
    pub(crate) fn solve(
        &mut self,
        mesh: &PolyMesh,
        fv: &FvGeometry,
        phi: &[f64],
        nu_eff_faces: &[f64],
        log: &mut dyn FnMut(&str),
    ) -> Result<(String, Performance)> {
        let gamma = self.alpha_eff(nu_eff_faces);
        let eqn = transport(mesh, fv, &self.t, &self.setup, phi, &gamma)?;
        solve(mesh, eqn, &mut self.t, &self.setup, &[], "T", log)
    }

    /// Writes `T` into `dir/time`.
    pub(crate) fn write(&self, mesh: &PolyMesh, dir: &std::path::Path, time: &str) -> Result<()> {
        let mut out = self.t.to_write(mesh);
        out.drop_zero_gradient_values();
        out.write(dir, time)?;
        Ok(())
    }
}
