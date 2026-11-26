// SPDX-License-Identifier: MIT OR Apache-2.0
//! Steady heat conduction in a solid region: `−∇·(k ∇T) = q`.
//!
//! The solid half of conjugate heat transfer. A solid is a region of its own
//! (its own mesh, with a patch where it touches the fluid), so the flow
//! equations never see it; the two are tied together through the temperature
//! and the heat flux on that interface patch. This module solves one region
//! for a given set of conditions and reads back what leaves through each
//! patch, which is all the coupling loop needs.
//!
//! The conditions are the ones a thermal engineer states: an insulated or
//! symmetric side, a fixed temperature, a prescribed heat flux, a convective
//! side with a film coefficient `h` and an ambient `T_ref`, and the interface,
//! which is the same Robin law with `h` and `T_ref` coming from the fluid on
//! every face. A Robin face puts the film resistance `1/h` in series with
//! the half cell of conduction `1/(k δ)`, so it is exact for a linear profile
//! on any mesh.

use vortia_fv::bc::{Boundary, ScalarBc};
use vortia_fv::ops::{self, SnGrad};
use vortia_fv::solve::{Controls, Performance};
use vortia_fv::{FvGeometry, LduMatrix};
use vortia_mesh::PolyMesh;

use crate::control::LinearSolver;
use crate::error::{Result, SolverError};

/// What a patch of the solid does with heat.
#[derive(Debug, Clone, PartialEq)]
pub enum SolidBc {
    /// No heat crosses it: an insulated wall, or a plane of symmetry.
    Insulated,
    /// The unused direction of a 2D region.
    Empty,
    /// A fixed temperature, in K.
    Fixed(f64),
    /// A heat flux into the solid, in W/m².
    Flux(f64),
    /// A film coefficient `h` (W/m²K) to an ambient at `t_ref`.
    Convective { h: f64, t_ref: f64 },
    /// The same law with `h` and `t_ref` for every face of the patch, as the
    /// fluid side gives them.
    Interface { h: Vec<f64>, t_ref: Vec<f64> },
}

/// How a solid is discretised and solved.
#[derive(Debug, Clone, Copy)]
pub struct Numerics {
    /// The surface-normal gradient of the conduction term.
    pub sn_grad: SnGrad,
    /// Explicit non-orthogonal corrections after the first solve (only
    /// `Corrected` has any).
    pub n_corr: usize,
    pub solver: LinearSolver,
    pub controls: Controls,
}

/// A solid region: its conductivity, the heat generated in it and one
/// condition per patch, in patch order.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    /// Thermal conductivity in W/m·K.
    pub k: f64,
    /// Heat generated per unit volume in W/m³.
    pub generation: f64,
    pub conditions: Vec<SolidBc>,
}

fn setup<T>(message: impl Into<String>) -> Result<T> {
    Err(SolverError::Setup(message.into()))
}

impl Solid {
    /// Refuses what would make the solve meaningless: a conductivity that is
    /// not positive, a patch with no condition or a condition of the wrong
    /// size, a negative film coefficient, a value that is not a number.
    pub fn check(&self, mesh: &PolyMesh) -> Result<()> {
        if !(self.k.is_finite() && self.k > 0.0) {
            return setup("the thermal conductivity must be positive");
        }
        if !self.generation.is_finite() {
            return setup("the heat generation must be a number");
        }
        if self.conditions.len() != mesh.patches().len() {
            return setup(format!(
                "{} conditions for {} patches",
                self.conditions.len(),
                mesh.patches().len()
            ));
        }
        for (patch, bc) in mesh.patches().iter().zip(&self.conditions) {
            let name = &patch.name;
            match bc {
                SolidBc::Insulated | SolidBc::Empty => {}
                SolidBc::Fixed(t) if !t.is_finite() => {
                    return setup(format!("patch '{name}': the temperature is not a number"));
                }
                SolidBc::Flux(q) if !q.is_finite() => {
                    return setup(format!("patch '{name}': the heat flux is not a number"));
                }
                SolidBc::Convective { h, t_ref } if !(h.is_finite() && *h >= 0.0) => {
                    let _ = t_ref;
                    return setup(format!(
                        "patch '{name}': the film coefficient must not be negative"
                    ));
                }
                SolidBc::Convective { t_ref, .. } if !t_ref.is_finite() => {
                    return setup(format!("patch '{name}': the ambient is not a number"));
                }
                SolidBc::Interface { h, t_ref } => {
                    if h.len() != patch.size || t_ref.len() != patch.size {
                        return setup(format!(
                            "patch '{name}': the interface needs {} values",
                            patch.size
                        ));
                    }
                    if h.iter().any(|v| !(v.is_finite() && *v >= 0.0)) {
                        return setup(format!(
                            "patch '{name}': the film coefficients must not be negative"
                        ));
                    }
                    if t_ref.iter().any(|v| !v.is_finite()) {
                        return setup(format!("patch '{name}': the ambient is not a number"));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The conditions as the engine's boundary, with every Robin face turned
    /// into a mixed one: the fraction of the ambient is `h / (h + k δ)`.
    fn boundary(&self, mesh: &PolyMesh, dc: &[f64]) -> Boundary {
        let robin = |patch: &vortia_mesh::polymesh::Patch,
                     h: &dyn Fn(usize) -> f64,
                     t: &dyn Fn(usize) -> f64| {
            let (fraction, ref_value): (Vec<f64>, Vec<f64>) = patch
                .faces()
                .enumerate()
                .map(|(i, f)| {
                    let kd = self.k * dc.get(f).copied().unwrap_or(0.0);
                    let h = h(i);
                    let fraction = if h + kd > 0.0 { h / (h + kd) } else { 0.0 };
                    (fraction, t(i))
                })
                .unzip();
            ScalarBc::Mixed {
                fraction,
                ref_value,
                ref_gradient: Vec::new(),
            }
        };
        let conditions = mesh
            .patches()
            .iter()
            .zip(&self.conditions)
            .map(|(patch, bc)| match bc {
                SolidBc::Insulated => ScalarBc::ZeroGradient,
                SolidBc::Empty => ScalarBc::Empty,
                SolidBc::Fixed(t) => ScalarBc::FixedValue(vec![*t; patch.size]),
                SolidBc::Flux(q) => ScalarBc::FixedGradient(vec![q / self.k; patch.size]),
                SolidBc::Convective { h, t_ref } => robin(patch, &|_| *h, &|_| *t_ref),
                SolidBc::Interface { h, t_ref } => {
                    robin(patch, &|i| h.get(i).copied().unwrap_or(0.0), &|i| {
                        t_ref.get(i).copied().unwrap_or(0.0)
                    })
                }
            })
            .collect();
        Boundary { conditions }
    }

    /// Solves for `t`, one value per cell, which is the guess it starts from.
    pub fn solve(
        &self,
        mesh: &PolyMesh,
        fv: &FvGeometry,
        t: &mut [f64],
        numerics: &Numerics,
    ) -> Result<Performance> {
        let &Numerics {
            sn_grad,
            n_corr,
            solver,
            ref controls,
        } = numerics;
        self.check(mesh)?;
        if t.len() != mesh.n_cells() {
            return setup("the temperature has one value per cell");
        }
        let bc = self.boundary(mesh, sn_grad.coeffs(fv));
        let mut last = None;
        for _ in 0..=n_corr {
            let mut m = LduMatrix::new(mesh);
            ops::diffusion(&mut m, mesh, fv, &bc, self.k, sn_grad, t)?;
            for (s, v) in m.source.iter_mut().zip(&fv.geo.cell_volumes) {
                *s += self.generation * v;
            }
            last = Some(solver.solve(&m, t, controls)?);
        }
        last.ok_or_else(|| SolverError::Setup("no solve was run".into()))
    }

    /// The temperature on every face of patch `patch`, which is what the
    /// fluid sees across an interface. `None` for the faces of an empty one.
    pub fn patch_temperature(
        &self,
        mesh: &PolyMesh,
        fv: &FvGeometry,
        t: &[f64],
        sn_grad: SnGrad,
        patch: usize,
    ) -> Vec<Option<f64>> {
        let bc = self.boundary(mesh, sn_grad.coeffs(fv));
        let all = bc.face_values(mesh, t, sn_grad.coeffs(fv));
        let n_internal = mesh.n_internal_faces();
        mesh.patches()
            .get(patch)
            .map(|p| {
                p.faces()
                    .map(|f| {
                        f.checked_sub(n_internal)
                            .and_then(|i| all.get(i))
                            .copied()
                            .flatten()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The heat leaving the solid through each face of patch `patch`, in
    /// W/m²: `k δ (T_cell − T_face)`, which is what the cell conducts to the
    /// face and so what the other side receives.
    pub fn patch_heat_flux(
        &self,
        mesh: &PolyMesh,
        fv: &FvGeometry,
        t: &[f64],
        sn_grad: SnGrad,
        patch: usize,
    ) -> Vec<f64> {
        let dc = sn_grad.coeffs(fv);
        let faces = self.patch_temperature(mesh, fv, t, sn_grad, patch);
        let Some(p) = mesh.patches().get(patch) else {
            return Vec::new();
        };
        p.faces()
            .zip(faces)
            .map(|(f, tf)| {
                let inside = mesh
                    .owner()
                    .get(f)
                    .and_then(|&c| t.get(c))
                    .copied()
                    .unwrap_or(0.0);
                tf.map_or(0.0, |tf| {
                    self.k * dc.get(f).copied().unwrap_or(0.0) * (inside - tf)
                })
            })
            .collect()
    }

    /// The heat leaving through the whole of patch `patch`, in W.
    pub fn patch_heat(
        &self,
        mesh: &PolyMesh,
        fv: &FvGeometry,
        t: &[f64],
        sn_grad: SnGrad,
        patch: usize,
    ) -> f64 {
        let Some(p) = mesh.patches().get(patch) else {
            return 0.0;
        };
        self.patch_heat_flux(mesh, fv, t, sn_grad, patch)
            .iter()
            .zip(p.faces())
            .map(|(q, f)| q * fv.mag_sf.get(f).copied().unwrap_or(0.0))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortia_core::{Aabb, Vec3};
    use vortia_mesh::blocks::structured_box;

    const L: f64 = 0.1;

    /// A bar along x of `n` cells, 1 cm² in section: patches xMin, xMax,
    /// yMin, yMax, zMin, zMax.
    fn bar(n: usize) -> (PolyMesh, FvGeometry) {
        let bounds = Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(L, 0.01, 0.01),
        };
        let mesh = structured_box(bounds, [n, 1, 1]).unwrap();
        let fv = FvGeometry::new(&mesh);
        (mesh, fv)
    }

    fn sides(x_min: SolidBc, x_max: SolidBc) -> Vec<SolidBc> {
        let mut c = vec![x_min, x_max];
        c.extend(std::iter::repeat_n(SolidBc::Insulated, 4));
        c
    }

    fn tight() -> Numerics {
        Numerics {
            sn_grad: SnGrad::Corrected,
            n_corr: 0,
            solver: LinearSolver::PcgDic,
            controls: Controls {
                tolerance: 1e-14,
                rel_tol: 0.0,
                max_iter: 5000,
                min_iter: 0,
            },
        }
    }

    fn run(solid: &Solid, mesh: &PolyMesh, fv: &FvGeometry) -> Vec<f64> {
        let mut t = vec![0.0; mesh.n_cells()];
        let perf = solid.solve(mesh, fv, &mut t, &tight()).unwrap();
        assert!(perf.converged, "{perf}");
        t
    }

    fn x_of(fv: &FvGeometry, cell: usize) -> f64 {
        fv.geo.cell_centres.get(cell).unwrap().x
    }

    #[test]
    fn a_slab_between_two_temperatures_has_a_straight_profile_and_the_flux_of_fourier() {
        let (mesh, fv) = bar(20);
        let solid = Solid {
            k: 50.0,
            generation: 0.0,
            conditions: sides(SolidBc::Fixed(100.0), SolidBc::Fixed(20.0)),
        };
        let t = run(&solid, &mesh, &fv);
        for (c, v) in t.iter().enumerate() {
            let exact = 100.0 - 80.0 * x_of(&fv, c) / L;
            assert!((v - exact).abs() < 1e-9, "cell {c}: {v} against {exact}");
        }
        // q = k ΔT / L = 50 · 80 / 0.1 = 40 kW/m²; the section is 1e-4 m².
        let out = solid.patch_heat(&mesh, &fv, &t, SnGrad::Corrected, 1);
        assert!((out - 4.0).abs() < 1e-9, "{out} W");
        let into = solid.patch_heat(&mesh, &fv, &t, SnGrad::Corrected, 0);
        assert!((into + 4.0).abs() < 1e-9, "{into} W");
    }

    #[test]
    fn generation_between_two_cold_faces_peaks_at_q_l_squared_over_8k() {
        let (mesh, fv) = bar(40);
        let (k, q) = (20.0, 5.0e5);
        let solid = Solid {
            k,
            generation: q,
            conditions: sides(SolidBc::Fixed(0.0), SolidBc::Fixed(0.0)),
        };
        let t = run(&solid, &mesh, &fv);
        for (c, v) in t.iter().enumerate() {
            let x = x_of(&fv, c);
            let exact = q * x * (L - x) / (2.0 * k);
            assert!(
                (v - exact).abs() < 2e-3 * q * L * L / (8.0 * k),
                "x = {x}: {v} against {exact}"
            );
        }
        // All the heat made, q·V = 5e5 · 1e-5 m³ = 5 W, leaves by the two ends.
        let total: f64 = [0, 1]
            .iter()
            .map(|&p| solid.patch_heat(&mesh, &fv, &t, SnGrad::Corrected, p))
            .sum();
        assert!((total - 5.0).abs() < 1e-6, "{total} W");
    }

    #[test]
    fn a_fixed_end_and_a_film_end_carry_the_heat_through_the_resistances_in_series() {
        let (mesh, fv) = bar(8);
        let (k, h) = (15.0, 120.0);
        let solid = Solid {
            k,
            generation: 0.0,
            conditions: sides(
                SolidBc::Fixed(150.0),
                SolidBc::Convective { h, t_ref: 30.0 },
            ),
        };
        let t = run(&solid, &mesh, &fv);
        let q = (150.0 - 30.0) / (L / k + 1.0 / h);
        let out = solid.patch_heat_flux(&mesh, &fv, &t, SnGrad::Corrected, 1);
        assert!(
            out.iter().all(|v| (v - q).abs() < 1e-7 * q),
            "{out:?} against {q}"
        );
        // The wall of the film side is at T_ref + q / h.
        let faces = solid.patch_temperature(&mesh, &fv, &t, SnGrad::Corrected, 1);
        let tw = faces.first().copied().flatten().unwrap();
        assert!((tw - (30.0 + q / h)).abs() < 1e-7, "{tw}");
    }

    #[test]
    fn an_interface_is_a_film_whose_coefficient_and_ambient_come_from_each_face() {
        let (mesh, fv) = bar(8);
        let (k, h) = (15.0, 120.0);
        let patch = mesh.patches().get(1).unwrap().size;
        let film = Solid {
            k,
            generation: 0.0,
            conditions: sides(
                SolidBc::Fixed(150.0),
                SolidBc::Convective { h, t_ref: 30.0 },
            ),
        };
        let interface = Solid {
            conditions: sides(
                SolidBc::Fixed(150.0),
                SolidBc::Interface {
                    h: vec![h; patch],
                    t_ref: vec![30.0; patch],
                },
            ),
            ..film.clone()
        };
        let a = run(&film, &mesh, &fv);
        let b = run(&interface, &mesh, &fv);
        assert_eq!(a, b);
    }

    #[test]
    fn a_prescribed_flux_raises_the_end_by_q_l_over_k() {
        let (mesh, fv) = bar(10);
        let k = 40.0;
        let solid = Solid {
            k,
            generation: 0.0,
            conditions: sides(SolidBc::Flux(2.0e4), SolidBc::Fixed(25.0)),
        };
        let t = run(&solid, &mesh, &fv);
        let faces = solid.patch_temperature(&mesh, &fv, &t, SnGrad::Corrected, 0);
        let hot = faces.first().copied().flatten().unwrap();
        assert!((hot - (25.0 + 2.0e4 * L / k)).abs() < 1e-7, "{hot}");
    }

    #[test]
    fn the_heat_a_block_makes_leaves_through_its_films_whatever_their_sizes() {
        let bounds = Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(0.04, 0.03, 0.02),
        };
        let mesh = structured_box(bounds, [4, 3, 3]).unwrap();
        let fv = FvGeometry::new(&mesh);
        let film = |h: f64, t_ref: f64| SolidBc::Convective { h, t_ref };
        let solid = Solid {
            k: 200.0,
            generation: 2.0e6,
            conditions: vec![
                film(10.0, 20.0),
                film(40.0, 25.0),
                film(25.0, 20.0),
                film(5.0, 30.0),
                SolidBc::Insulated,
                film(60.0, 20.0),
            ],
        };
        let t = run(&solid, &mesh, &fv);
        let made = 2.0e6 * 0.04 * 0.03 * 0.02;
        let total: f64 = (0..6)
            .map(|p| solid.patch_heat(&mesh, &fv, &t, SnGrad::Corrected, p))
            .sum();
        assert!((total - made).abs() < 1e-6 * made, "{total} W of {made} W");
    }

    #[test]
    fn a_thin_fin_follows_the_cosh_profile_and_loses_k_a_m_theta_tanh_ml() {
        // A 2 mm square rod, 10 cm long, held at 100 °C at the base, with a
        // film on its four sides and an insulated tip: the textbook fin, whose
        // Biot number (h t / 2k = 1.3e-4) is small enough for the 1D solution.
        let (k, h, w) = (200.0, 25.0, 0.002);
        let bounds = Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(L, w, w),
        };
        let mesh = structured_box(bounds, [40, 2, 2]).unwrap();
        let fv = FvGeometry::new(&mesh);
        let film = SolidBc::Convective { h, t_ref: 20.0 };
        let solid = Solid {
            k,
            generation: 0.0,
            conditions: vec![
                SolidBc::Fixed(100.0),
                SolidBc::Insulated,
                film.clone(),
                film.clone(),
                film.clone(),
                film,
            ],
        };
        let t = run(&solid, &mesh, &fv);
        let m = (h * 4.0 * w / (k * w * w)).sqrt();
        let theta_b = 80.0;
        for (c, v) in t.iter().enumerate() {
            let x = x_of(&fv, c);
            let exact = 20.0 + theta_b * (m * (L - x)).cosh() / (m * L).cosh();
            assert!((v - exact).abs() < 0.05, "x = {x}: {v} against {exact}");
        }
        let base = -solid.patch_heat(&mesh, &fv, &t, SnGrad::Corrected, 0);
        let exact = k * w * w * m * theta_b * (m * L).tanh();
        assert!(
            (base - exact).abs() < 0.01 * exact,
            "{base} W against {exact} W"
        );
    }

    #[test]
    fn a_solid_that_makes_no_sense_is_refused_with_the_reason() {
        let (mesh, fv) = bar(4);
        let ok = Solid {
            k: 1.0,
            generation: 0.0,
            conditions: sides(SolidBc::Fixed(1.0), SolidBc::Fixed(0.0)),
        };
        let mut t = vec![0.0; mesh.n_cells()];
        let go = |s: &Solid, t: &mut [f64]| s.solve(&mesh, &fv, t, &tight());
        assert!(go(&ok, &mut t).is_ok());
        for bad in [
            Solid {
                k: 0.0,
                ..ok.clone()
            },
            Solid {
                k: f64::NAN,
                ..ok.clone()
            },
            Solid {
                generation: f64::INFINITY,
                ..ok.clone()
            },
            Solid {
                conditions: vec![SolidBc::Insulated],
                ..ok.clone()
            },
            Solid {
                conditions: sides(
                    SolidBc::Convective {
                        h: -1.0,
                        t_ref: 0.0,
                    },
                    SolidBc::Insulated,
                ),
                ..ok.clone()
            },
            Solid {
                conditions: sides(
                    SolidBc::Interface {
                        h: vec![1.0; 3],
                        t_ref: vec![0.0; 3],
                    },
                    SolidBc::Insulated,
                ),
                ..ok.clone()
            },
        ] {
            assert!(go(&bad, &mut t).is_err(), "{bad:?}");
        }
        let mut short = vec![0.0; 2];
        assert!(go(&ok, &mut short).is_err());
    }
}
