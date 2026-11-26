// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conjugate heat transfer: the fluid's temperature and a solid's, tied together at the wall
//! they share.
//!
//! The solid is a region of its own (`constant/solid/polyMesh`, its patch `interface` being the
//! faces of the fluid's wall seen from the other side). On each pass the fluid wall takes a
//! Robin condition built from the solid, and the solid a Robin condition built from the fluid:
//! with `K_f = ρc_p α_eff δ_f` and `K_s = k δ_s` the conductances of the half cells either
//! side, the wall is at `T_w = (K_f T_f + K_s T_s) / (K_f + K_s)` and the heat that crosses
//! is `K_f K_s / (K_f + K_s) (T_s − T_f)` from both sides at once. Solving the fluid with the
//! solid's last temperature and the solid with the fluid's new one, again and again, is
//! `chtMultiRegionFoam`'s coupling for a steady run; when it has converged the two heat
//! flows are the same number, whatever the meshes (the faces are the same faces).
//!
//! [`report`] reads back what a run left in the solid.
//!
//! The flow does not depend on the temperature (no buoyancy), so only the two temperature
//! equations are iterated.

use std::path::Path;

use vortia_fields::{Data, ScalarField};
use vortia_foamfile::RawPolyMesh;
use vortia_fv::FvGeometry;
use vortia_fv::ops::SnGrad;
use vortia_fv::solve::Performance;
use vortia_mesh::PolyMesh;
use vortia_mesh::bodymesh::match_interface;

use crate::case::{Case, read_dict};
use crate::control::{LinearSolver, solve_line};
use crate::error::{Result, SolverError};
use crate::rans::{Kind, ModelField};
use crate::solid::{Numerics, Solid, SolidBc};

/// The name the solid's residual goes by in the logs and in `residualControl`.
pub(crate) const SOLID_FIELD: &str = "Tsolid";

/// The faces of one coupled fluid patch and the faces of the solid's interface they are.
struct Link {
    /// The fluid's patch.
    patch: usize,
    /// For each face of that patch, the face of the solid's interface patch.
    solid_face: Vec<usize>,
}

pub(crate) struct Conjugate {
    mesh: PolyMesh,
    fv: FvGeometry,
    solid: Solid,
    /// The temperature of the solid's cells.
    t: Vec<f64>,
    /// The solid's `T` as read, which the written field is made from.
    file: ScalarField,
    numerics: Numerics,
    /// The fluid's `ρ c_p` in J/m³K: its `α_eff` is `ν/Pr`, in m²/s.
    rho_cp: f64,
    /// How much of the new solid temperature each pass takes.
    relax: f64,
    interface: usize,
    links: Vec<Link>,
    /// For each face of the solid's interface patch, the fluid face it is.
    fluid_face: Vec<usize>,
}

fn setup<T>(message: impl Into<String>) -> Result<T> {
    Err(SolverError::Setup(message.into()))
}

/// Whether `case` has a solid.
pub(crate) fn has_solid(case: &Case) -> bool {
    case.dir
        .join("constant")
        .join("solid")
        .join("polyMesh")
        .is_dir()
}

impl Conjugate {
    /// The solid of `case`, to be coupled to the fluid patches `coupled` (the patch indices
    /// whose temperature condition is `conjugate`). `rho_cp` is the fluid's `ρ c_p`.
    pub(crate) fn read(
        case: &Case,
        fluid: &PolyMesh,
        start: &Path,
        coupled: &[usize],
        rho_cp: f64,
    ) -> Result<Self> {
        let dir = case.dir.join("constant").join("solid");
        let props = read_dict(&dir.join("solidProperties"))?;
        let number = |key: &str, default: Option<f64>| -> Result<f64> {
            props
                .scalar(key)
                .or(default)
                .filter(|v| v.is_finite())
                .ok_or_else(|| SolverError::Setup(format!("solidProperties has no valid '{key}'")))
        };
        let k = number("k", None)?;
        let generation = number("Q", Some(0.0))?;
        let relax = number("relaxation", Some(1.0))?;
        if !(relax > 0.0 && relax <= 1.0) {
            return setup("solidProperties: relaxation must be in (0, 1]");
        }
        let interface_name = props.word("interface").unwrap_or("body").to_owned();

        let mesh = PolyMesh::from_raw(RawPolyMesh::read_dir(dir.join("polyMesh"))?)?;
        let fv = FvGeometry::new(&mesh);
        let problems = vortia_mesh::check::check(&mesh, &fv.geo).unsolvable();
        if !problems.is_empty() {
            return setup(format!(
                "the solid's mesh cannot be solved on: {}",
                problems.join(", ")
            ));
        }
        let interface = mesh
            .patches()
            .iter()
            .position(|p| p.name == interface_name)
            .ok_or_else(|| {
                SolverError::Setup(format!("the solid has no patch '{interface_name}'"))
            })?;
        let interface_size = mesh.patches().get(interface).map_or(0, |p| p.size);

        let walls: Vec<&str> = coupled
            .iter()
            .filter_map(|&p| fluid.patches().get(p).map(|p| p.name.as_str()))
            .collect();
        let matched = match_interface(fluid, &walls, &mesh, &interface_name)?;
        let mut links = Vec::new();
        let mut fluid_face = vec![usize::MAX; interface_size];
        let mut at = 0;
        for &p in coupled {
            let Some(patch) = fluid.patches().get(p) else {
                continue;
            };
            let solid_face: Vec<usize> = matched
                .solid_face
                .get(at..at + patch.size)
                .map(<[usize]>::to_vec)
                .unwrap_or_default();
            at += patch.size;
            for (f, &j) in patch.faces().zip(&solid_face) {
                if let Some(slot) = fluid_face.get_mut(j) {
                    *slot = f;
                }
            }
            links.push(Link {
                patch: p,
                solid_face,
            });
        }
        if fluid_face.contains(&usize::MAX) {
            return setup(format!(
                "the solid's patch '{interface_name}' has faces that no coupled wall of the fluid covers"
            ));
        }

        let file = ScalarField::read(start.join("solid").join("T"))?;
        file.check_against(&mesh)?;
        let t = file
            .internal
            .expand(mesh.n_cells())
            .ok_or_else(|| SolverError::Setup("the solid's T does not fit its mesh".into()))?;

        let (solver, controls) = LinearSolver::read(case, "T")?;
        let numerics = Numerics {
            sn_grad: SnGrad::Corrected,
            n_corr: case.non_orthogonal_correctors("SIMPLE"),
            solver,
            controls,
        };
        let conditions = mesh
            .patches()
            .iter()
            .enumerate()
            .map(|(i, p)| -> Result<SolidBc> {
                if i == interface {
                    return Ok(SolidBc::Interface {
                        h: vec![0.0; p.size],
                        t_ref: vec![0.0; p.size],
                    });
                }
                if p.kind.is_constraint_of_dimension() {
                    return Ok(SolidBc::Empty);
                }
                let Some(pf) = file.patch_field(p) else {
                    return Ok(SolidBc::Insulated);
                };
                match (pf.kind.as_str(), &pf.value) {
                    ("zeroGradient", _) => Ok(SolidBc::Insulated),
                    ("fixedValue", Some(Data::Uniform(v))) => Ok(SolidBc::Fixed(*v)),
                    ("fixedGradient", _) => {
                        let g = pf
                            .settings
                            .values("gradient")
                            .and_then(|v| v.last())
                            .and_then(vortia_foamfile::Value::as_scalar)
                            .ok_or_else(|| {
                                SolverError::Setup(format!(
                                    "solid T: patch '{}' needs a uniform gradient",
                                    p.name
                                ))
                            })?;
                        Ok(SolidBc::Flux(k * g))
                    }
                    (kind, _) => setup(format!(
                        "solid T: patch '{}' has '{kind}', which the solid does not take (fixedValue with a uniform value, fixedGradient, zeroGradient)",
                        p.name
                    )),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let solid = Solid {
            k,
            generation,
            conditions,
        };
        solid.check(&mesh)?;
        Ok(Self {
            mesh,
            fv,
            solid,
            t,
            file,
            numerics,
            rho_cp,
            relax,
            interface,
            links,
            fluid_face,
        })
    }

    /// The conductance of the fluid's half cell at face `f` of the fluid, in W/m²K.
    fn fluid_conductance(&self, gamma: &[f64], delta: &[f64], f: usize) -> f64 {
        self.rho_cp * gamma.get(f).copied().unwrap_or(0.0) * delta.get(f).copied().unwrap_or(0.0)
    }

    /// Sets the wall of the fluid from the solid: the solid's cell temperature next to the
    /// wall, taken with the share `K_s / (K_f + K_s)` that makes the wall's temperature the
    /// one both sides agree on.
    pub(crate) fn apply_to_fluid(
        &self,
        fluid: &PolyMesh,
        t: &mut ModelField,
        gamma: &[f64],
        delta: &[f64],
    ) {
        let dc_s = self.numerics.sn_grad.coeffs(&self.fv);
        let Some(interface) = self.mesh.patches().get(self.interface) else {
            return;
        };
        for link in &self.links {
            let Some(patch) = fluid.patches().get(link.patch) else {
                continue;
            };
            let (mut ref_value, mut fraction) = (Vec::new(), Vec::new());
            for (f, &j) in patch.faces().zip(&link.solid_face) {
                let face = interface.start + j;
                let ts = self
                    .mesh
                    .owner()
                    .get(face)
                    .and_then(|&c| self.t.get(c))
                    .copied()
                    .unwrap_or(0.0);
                let kf = self.fluid_conductance(gamma, delta, f);
                let ks = self.solid.k * dc_s.get(face).copied().unwrap_or(0.0);
                ref_value.push(ts);
                fraction.push(if kf + ks > 0.0 { ks / (kf + ks) } else { 0.0 });
            }
            if let Some(kind) = t.kinds.get_mut(link.patch) {
                *kind = Kind::Coupled {
                    ref_value,
                    fraction,
                };
            }
        }
    }

    /// Solves the solid with the fluid's wall: each face a film of the fluid's conductance to the
    /// fluid's temperature next to it. Returns the solve as the engine logs it.
    pub(crate) fn solve_solid(
        &mut self,
        fluid: &PolyMesh,
        t: &ModelField,
        gamma: &[f64],
        delta: &[f64],
    ) -> Result<(String, Performance)> {
        let (h, t_ref): (Vec<f64>, Vec<f64>) = self
            .fluid_face
            .iter()
            .map(|&f| {
                let outside = fluid
                    .owner()
                    .get(f)
                    .and_then(|&c| t.cells.get(c))
                    .copied()
                    .unwrap_or(0.0);
                (self.fluid_conductance(gamma, delta, f), outside)
            })
            .unzip();
        if let Some(c) = self.solid.conditions.get_mut(self.interface) {
            *c = SolidBc::Interface { h, t_ref };
        }
        let mut next = self.t.clone();
        let perf = self
            .solid
            .solve(&self.mesh, &self.fv, &mut next, &self.numerics)?;
        for (old, new) in self.t.iter_mut().zip(&next) {
            *old += self.relax * (new - *old);
        }
        Ok((SOLID_FIELD.to_owned(), perf))
    }

    /// The heat that leaves the fluid through the coupled walls and the heat that enters the
    /// solid through its interface, in W (negative when it flows the other way): the same
    /// number when the coupling has converged.
    pub(crate) fn balance(
        &self,
        fluid: &PolyMesh,
        fv: &FvGeometry,
        t: &ModelField,
        gamma: &[f64],
        delta: &[f64],
    ) -> (f64, f64) {
        let n_internal = fluid.n_internal_faces();
        let mut from_fluid = 0.0;
        for link in &self.links {
            let Some(patch) = fluid.patches().get(link.patch) else {
                continue;
            };
            for f in patch.faces() {
                let wall = f
                    .checked_sub(n_internal)
                    .and_then(|b| t.faces.get(b))
                    .copied()
                    .flatten();
                let inside = fluid.owner().get(f).and_then(|&c| t.cells.get(c)).copied();
                if let (Some(w), Some(i)) = (wall, inside) {
                    from_fluid += self.fluid_conductance(gamma, delta, f)
                        * fv.mag_sf.get(f).copied().unwrap_or(0.0)
                        * (i - w);
                }
            }
        }
        let into_solid = -self.solid.patch_heat(
            &self.mesh,
            &self.fv,
            &self.t,
            self.numerics.sn_grad,
            self.interface,
        );
        (from_fluid, into_solid)
    }

    /// The solid's temperature in its cells and the largest one.
    pub(crate) fn peak(&self) -> f64 {
        self.t.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }

    /// The log line of a solve of the solid.
    pub(crate) fn log_line(perf: &(String, Performance)) -> String {
        solve_line(&perf.0, &perf.1)
    }

    /// Writes the solid's `T` into `<dir>/<time>/solid`, with the temperature of its wall on
    /// the interface.
    pub(crate) fn write(&self, dir: &Path, time: &str) -> Result<()> {
        let mut out = self.file.clone();
        out.internal = Data::NonUniform(self.t.clone());
        let n_internal = self.mesh.n_internal_faces();
        let mut wall = vec![None; self.mesh.n_faces().saturating_sub(n_internal)];
        let faces = self.solid.patch_temperature(
            &self.mesh,
            &self.fv,
            &self.t,
            self.numerics.sn_grad,
            self.interface,
        );
        if let Some(patch) = self.mesh.patches().get(self.interface) {
            for (f, tf) in patch.faces().zip(faces) {
                if let Some(slot) = f.checked_sub(n_internal).and_then(|b| wall.get_mut(b)) {
                    *slot = tf;
                }
            }
        }
        out.store_patch_values(&self.mesh, &["conjugate"], &wall);
        out.drop_zero_gradient_values();
        out.write(dir.join(time), "solid")?;
        Ok(())
    }
}

/// What a run left in the solid: its temperatures and the heat that crossed its wall.
#[derive(Debug, Clone, PartialEq)]
pub struct SolidReport {
    pub time: String,
    pub cells: usize,
    /// The solid's volume, in m³.
    pub volume: f64,
    /// Thermal conductivity, in W/mK.
    pub conductivity: f64,
    /// The heat generated inside, in W.
    pub generated: f64,
    pub t_min: f64,
    /// The mean over the volume.
    pub t_mean: f64,
    pub t_max: f64,
    /// The area of the wall it shares with the fluid, in m².
    pub wall_area: f64,
    /// The mean temperature of that wall (by area).
    pub wall_mean: f64,
    /// The heat that leaves the solid through the wall, in W.
    pub heat_out: f64,
}

/// The solid of `case_dir` as the solver left it at `time`.
pub fn report(case_dir: &Path, time: &str) -> Result<SolidReport> {
    let dir = case_dir.join("constant").join("solid");
    let props = read_dict(&dir.join("solidProperties"))?;
    let number = |key: &str, default: f64| props.scalar(key).unwrap_or(default);
    let k = number("k", f64::NAN);
    if !k.is_finite() {
        return setup("solidProperties has no valid 'k'");
    }
    let generation = number("Q", 0.0);
    let interface_name = props.word("interface").unwrap_or("body").to_owned();
    let mesh = PolyMesh::from_raw(RawPolyMesh::read_dir(dir.join("polyMesh"))?)?;
    let fv = FvGeometry::new(&mesh);
    let file = ScalarField::read(case_dir.join(time).join("solid").join("T"))?;
    file.check_against(&mesh)?;
    let t = file
        .internal
        .expand(mesh.n_cells())
        .ok_or_else(|| SolverError::Setup("the solid's T does not fit its mesh".into()))?;
    let volumes = &fv.geo.cell_volumes;
    let volume: f64 = volumes.iter().sum();
    if !(volume.is_finite() && volume > 0.0) {
        return setup("the solid has no volume");
    }
    let (t_min, t_max) = t
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &v| {
            (l.min(v), h.max(v))
        });
    let t_mean = t.iter().zip(volumes).map(|(t, v)| t * v).sum::<f64>() / volume;

    let patch = mesh
        .patches()
        .iter()
        .find(|p| p.name == interface_name)
        .ok_or_else(|| SolverError::Setup(format!("the solid has no patch '{interface_name}'")))?;
    let walls = file
        .patch_field(patch)
        .and_then(|pf| pf.value.as_ref())
        .and_then(|v| v.expand(patch.size))
        .ok_or_else(|| {
            SolverError::Setup(format!(
                "the solid's T has no wall temperature on '{interface_name}': run the case first"
            ))
        })?;
    let dc = SnGrad::Corrected.coeffs(&fv);
    let (mut area, mut weighted, mut heat) = (0.0, 0.0, 0.0);
    for (f, &tw) in patch.faces().zip(&walls) {
        let a = fv.mag_sf.get(f).copied().unwrap_or(0.0);
        let inside = mesh
            .owner()
            .get(f)
            .and_then(|&c| t.get(c))
            .copied()
            .unwrap_or(tw);
        area += a;
        weighted += a * tw;
        heat += k * dc.get(f).copied().unwrap_or(0.0) * (inside - tw) * a;
    }
    Ok(SolidReport {
        time: time.to_owned(),
        cells: mesh.n_cells(),
        volume,
        conductivity: k,
        generated: generation * volume,
        t_min,
        t_mean,
        t_max,
        wall_area: area,
        wall_mean: if area > 0.0 { weighted / area } else { 0.0 },
        heat_out: heat,
    })
}
