// SPDX-License-Identifier: GPL-3.0-or-later
//! The pressure and skin-friction coefficients on the faces of any wall (`Cp = p / q`,
//! `Cf = |τ| / q` with `q = ½ ρ U²`), as a list of faces with their position, so they can be
//! plotted along a coordinate or painted on the body. The airfoil report has the same numbers
//! for a profile, sorted along its chord; this is the version for a body of any shape.

use vortia_core::Vec3;

use crate::case::{Case, FieldValues};
use crate::error::{PostError, Result};
use crate::forces::ForceReference;

/// The most faces a report keeps (a wall with more is refused, not cut short).
pub const MAX_SURFACE_FACES: usize = 5_000_000;

/// One face of a wall and what the flow does on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFace {
    pub centre: Vec3,
    /// The unit normal, out of the fluid and into the body.
    pub normal: Vec3,
    pub area: f64,
    /// The pressure coefficient, from the pressure the run solved (gauge, so the free stream is 0).
    pub cp: f64,
    /// The size of the friction coefficient.
    pub cf: f64,
    /// The friction coefficient along the stream: negative where the flow has reversed.
    pub cf_stream: f64,
}

/// The faces of some walls at one time.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceReport {
    pub time: String,
    /// The walls, and how many faces each has: the faces are in this order.
    pub patches: Vec<(String, usize)>,
    /// `q` that the coefficients are divided by: `½ U²` per unit density, or `½ ρ U²`.
    pub dynamic_pressure: f64,
    pub faces: Vec<SurfaceFace>,
}

impl SurfaceReport {
    /// The smallest and largest pressure coefficient, over the faces with a finite one.
    pub fn cp_range(&self) -> Option<(f64, f64)> {
        range(self.faces.iter().map(|f| f.cp))
    }

    /// The smallest and largest friction coefficient along the stream.
    pub fn cf_range(&self) -> Option<(f64, f64)> {
        range(self.faces.iter().map(|f| f.cf_stream))
    }

    /// The area under flow that has turned back on itself (`Cf` along the stream below zero).
    pub fn reversed_area(&self) -> f64 {
        self.faces
            .iter()
            .filter(|f| f.cf_stream < 0.0)
            .map(|f| f.area)
            .sum()
    }

    /// The faces as CSV, one row each: the patch, the centre, the normal, the area, `Cp`, `Cf`
    /// and `Cf` along the stream.
    pub fn csv(&self) -> String {
        let mut text = String::from("patch,x,y,z,nx,ny,nz,area,cp,cf,cf_stream\n");
        let mut faces = self.faces.iter();
        for (name, count) in &self.patches {
            for f in faces.by_ref().take(*count) {
                text.push_str(&format!(
                    "{},{:.9e},{:.9e},{:.9e},{:.6},{:.6},{:.6},{:.9e},{:.9e},{:.9e},{:.9e}\n",
                    crate::report::csv_text(name),
                    f.centre.x,
                    f.centre.y,
                    f.centre.z,
                    f.normal.x,
                    f.normal.y,
                    f.normal.z,
                    f.area,
                    f.cp,
                    f.cf,
                    f.cf_stream,
                ));
            }
        }
        text
    }
}

fn range(values: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    let finite: Vec<f64> = values.filter(|v| v.is_finite()).collect();
    let min = finite.iter().copied().reduce(f64::min)?;
    let max = finite.iter().copied().reduce(f64::max)?;
    Some((min, max))
}

impl Case {
    /// `Cp` and `Cf` on every face of `patches` at `time`. The reference gives the stream speed,
    /// its direction (the drag direction) and, for a gas, the density; `free_pressure` is the
    /// pressure of the undisturbed stream, taken from the wall pressure first (0 for an
    /// incompressible run, whose pressure is gauge; a gas has its absolute one).
    pub fn surface_coefficients(
        &self,
        time: &str,
        patches: &[String],
        reference: &ForceReference,
        free_pressure: f64,
    ) -> Result<SurfaceReport> {
        if !free_pressure.is_finite() {
            return Err(PostError::Invalid(
                "the free-stream pressure must be a finite number".into(),
            ));
        }
        if patches.is_empty() {
            return Err(PostError::Invalid(
                "the coefficients need at least one wall".into(),
            ));
        }
        if !(reference.speed.is_finite() && reference.speed > 0.0) {
            return Err(PostError::Invalid(
                "the reference speed must be a positive number".into(),
            ));
        }
        let along = {
            let n = reference.drag.norm();
            if !(n.is_finite() && n > 1e-12) {
                return Err(PostError::Invalid("the stream direction is zero".into()));
            }
            reference.drag * (1.0 / n)
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut faces = Vec::new();
        let mut listed = Vec::new();
        let mut per_unit_density = true;
        let mut q_kinematic = 0.5 * reference.speed * reference.speed;
        for name in patches {
            if !seen.insert(name.as_str()) {
                return Err(PostError::Invalid(format!(
                    "the wall '{name}' is listed twice"
                )));
            }
            let patch = self
                .mesh()
                .patch(name)
                .ok_or_else(|| PostError::Invalid(format!("no patch '{name}'")))?
                .clone();
            if faces.len() + patch.size > MAX_SURFACE_FACES {
                return Err(PostError::Invalid(format!(
                    "the walls have more than {MAX_SURFACE_FACES} faces"
                )));
            }
            let wall = self.wall(time, name)?;
            per_unit_density &= wall.kinematic;
            let FieldValues::Scalar(p) = self.patch_field(time, "p", name).map_err(|_| {
                PostError::Invalid("the coefficients need the pressure: the time has no 'p'".into())
            })?
            else {
                return Err(PostError::Invalid("p is not a scalar field".into()));
            };
            for (k, face) in patch.faces().enumerate() {
                let (Some(&sf), Some(&fc), Some(&shear), Some(&a), Some(&pk)) = (
                    self.geometry().face_areas.get(face),
                    self.geometry().face_centres.get(face),
                    wall.shear.get(k),
                    wall.areas.get(k),
                    p.get(k),
                ) else {
                    return Err(PostError::Invalid("a wall face has no result".into()));
                };
                let unit = if a > 0.0 { sf * (1.0 / a) } else { sf };
                // The stress by the wall on the fluid; the force on the wall is its opposite.
                faces.push(SurfaceFace {
                    centre: fc,
                    normal: unit,
                    area: a,
                    cp: pk - free_pressure,
                    cf: shear.norm(),
                    cf_stream: -shear.dot(along),
                });
            }
            listed.push((name.clone(), patch.size));
        }
        if !per_unit_density {
            let rho = reference.density.ok_or_else(|| {
                PostError::Invalid("a gas needs the reference density for its coefficients".into())
            })?;
            if !(rho.is_finite() && rho > 0.0) {
                return Err(PostError::Invalid(
                    "the reference density must be a positive number".into(),
                ));
            }
            q_kinematic *= rho;
        }
        let q = q_kinematic;
        for f in &mut faces {
            f.cp /= q;
            f.cf /= q;
            f.cf_stream /= q;
        }
        Ok(SurfaceReport {
            time: time.to_owned(),
            patches: listed,
            dynamic_pressure: q,
            faces,
        })
    }
}
