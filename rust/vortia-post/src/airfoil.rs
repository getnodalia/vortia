// SPDX-License-Identifier: GPL-3.0-or-later
//! The results an aerodynamicist reads off an airfoil run: the lift, drag and pitching-moment
//! coefficients about the quarter chord, and the pressure and skin-friction coefficients along
//! the surface (`Cp = p / ½U²`, `Cf = τ / ½U²`).
//!
//! The case is one that `vortia new airfoil` made: its `constant/airfoilProperties` says the
//! chord, the span, the stream speed and the angle of attack, and the profile is the wall patch
//! `airfoil`. The stream is at the angle of attack and the chord lies on the x axis, so the drag
//! is along the stream and the lift across it.

use vortia_core::Vec3;
use vortia_foamfile::FoamFile;

use crate::case::{Case, FieldValues};
use crate::error::{PostError, Result};
use crate::forces::ForceReference;

/// The wall patch of an airfoil case.
pub const AIRFOIL_PATCH: &str = "airfoil";

/// What `constant/airfoilProperties` says.
#[derive(Debug, Clone, PartialEq)]
pub struct AirfoilSettings {
    pub naca: f64,
    /// Angle of attack, in degrees.
    pub alpha: f64,
    pub chord: f64,
    /// The span of the slice the 2D mesh is, in m.
    pub depth: f64,
    pub speed: f64,
    pub reynolds: f64,
}

/// The pressure and skin friction on one face of the profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePoint {
    /// Along the chord, as a fraction of it.
    pub x: f64,
    /// Across it, as a fraction of the chord.
    pub y: f64,
    pub cp: f64,
    /// The friction coefficient along the surface (negative where the flow has reversed).
    pub cf: f64,
    /// On the upper (suction) surface of the profile, as against the lower.
    pub upper: bool,
}

/// The coefficients of an airfoil run and its surface distributions.
#[derive(Debug, Clone, PartialEq)]
pub struct AirfoilReport {
    pub time: String,
    pub settings: AirfoilSettings,
    pub cl: f64,
    pub cd: f64,
    /// About the quarter chord, positive nose up.
    pub cm: f64,
    /// The pressure and the viscous parts of the drag coefficient.
    pub cd_pressure: f64,
    pub cd_viscous: f64,
    /// From the leading edge to the trailing edge on each surface, upper first.
    pub surface: Vec<SurfacePoint>,
}

impl AirfoilReport {
    /// The surface as CSV: `x/c, y/c, Cp, Cf, surface`, the upper surface from the leading to
    /// the trailing edge, then the lower.
    pub fn surface_csv(&self) -> String {
        let mut text = String::from("x/c,y/c,cp,cf,surface\n");
        for p in &self.surface {
            text.push_str(&format!(
                "{:.9},{:.9},{:.9e},{:.9e},{}\n",
                p.x,
                p.y,
                p.cp,
                p.cf,
                if p.upper { "upper" } else { "lower" }
            ));
        }
        text
    }

    /// The most negative `Cp` of the profile: the suction peak.
    pub fn suction_peak(&self) -> Option<f64> {
        self.surface.iter().map(|p| p.cp).reduce(f64::min)
    }
}

impl Case {
    /// What `constant/airfoilProperties` says; an error when the case is not an airfoil case.
    pub fn airfoil_settings(&self) -> Result<AirfoilSettings> {
        let file = FoamFile::open(self.path().join("constant").join("airfoilProperties")).map_err(
            |_| {
                PostError::Invalid(
                    "this is not an airfoil case: constant/airfoilProperties is missing".into(),
                )
            },
        )?;
        let (_, dict) = file.dictionary()?;
        let number = |key: &str| {
            dict.values(key)
                .and_then(|v| v.last())
                .and_then(vortia_foamfile::Value::as_scalar)
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    PostError::Invalid(format!("airfoilProperties has no number '{key}'"))
                })
        };
        let settings = AirfoilSettings {
            naca: number("naca")?,
            alpha: number("alpha")?,
            chord: number("chord")?,
            depth: number("depth")?,
            speed: number("speed")?,
            reynolds: number("reynolds")?,
        };
        if settings.chord <= 0.0 || settings.depth <= 0.0 || settings.speed <= 0.0 {
            return Err(PostError::Invalid(
                "airfoilProperties needs a positive chord, depth and speed".into(),
            ));
        }
        Ok(settings)
    }

    /// The lift, drag and moment coefficients of the profile, and `Cp` and `Cf` along it.
    pub fn airfoil(&self, time: &str) -> Result<AirfoilReport> {
        let s = self.airfoil_settings()?;
        let (sin_a, cos_a) = s.alpha.to_radians().sin_cos();
        let reference = ForceReference {
            density: None,
            speed: s.speed,
            area: s.chord * s.depth,
            length: s.chord,
            centre: Vec3::new(0.25 * s.chord, 0.0, 0.0),
            drag: Vec3::new(cos_a, sin_a, 0.0),
            lift: Vec3::new(-sin_a, cos_a, 0.0),
        };
        let forces = self.forces(time, &[AIRFOIL_PATCH.to_owned()], &reference)?;
        let value = |key: &str| {
            forces
                .metrics()
                .into_iter()
                .find(|m| m.key == key)
                .map_or(f64::NAN, |m| m.value)
        };
        let q = 0.5 * s.speed * s.speed * s.chord * s.depth;

        let patch = self
            .mesh()
            .patch(AIRFOIL_PATCH)
            .ok_or_else(|| PostError::Invalid("no patch 'airfoil'".into()))?
            .clone();
        let wall = self.wall(time, AIRFOIL_PATCH)?;
        let FieldValues::Scalar(p) = self.patch_field(time, "p", AIRFOIL_PATCH)? else {
            return Err(PostError::Invalid("p is not a scalar field".into()));
        };
        let dynamic = 0.5 * s.speed * s.speed;
        let mut surface = Vec::with_capacity(patch.size);
        for (k, face) in patch.faces().enumerate() {
            let (Some(&sf), Some(&fc), Some(&shear), Some(&pk)) = (
                self.geometry().face_areas.get(face),
                self.geometry().face_centres.get(face),
                wall.shear.get(k),
                p.get(k),
            ) else {
                return Err(PostError::Invalid("an airfoil face has no result".into()));
            };
            // The face's normal points into the body, so on the upper surface it points down.
            let upper = sf.y < 0.0;
            // The surface's own direction, towards the trailing edge, for the sign of Cf.
            let tangent = Vec3::new(-sf.y, sf.x, 0.0);
            let sign = if upper { 1.0 } else { -1.0 };
            let along = -shear.dot(tangent) * sign / sf.norm().max(1e-300);
            surface.push(SurfacePoint {
                x: fc.x / s.chord,
                y: fc.y / s.chord,
                cp: pk / dynamic,
                cf: along / dynamic,
                upper,
            });
        }
        surface.sort_by(|a, b| {
            b.upper
                .cmp(&a.upper)
                .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
        });
        Ok(AirfoilReport {
            time: time.to_owned(),
            settings: s,
            cl: value("cl"),
            cd: value("cd"),
            cm: value("cm"),
            cd_pressure: value("pressureDrag") / q,
            cd_viscous: value("viscousDrag") / q,
            surface,
        })
    }
}
