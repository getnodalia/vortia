//! The force and moment the flow puts on a set of wall patches, split into the pressure and the
//! viscous part, along a drag and a lift direction, and as the dimensionless coefficients an
//! engineer compares between designs and runs: `C = F / (½ ρ U² A)` and `Cm = M / (½ ρ U² A L)`.
//!
//! Definitions follow OpenFOAM's `forces` and `forceCoeffs` function objects: the pressure force
//! is `Σ p S` over the faces (`S` pointing out of the fluid into the wall), the viscous force the
//! sum of the wall stress `× |S|` as [`Case::wall`] reports it, and the moment is taken about a
//! centre of rotation. An incompressible case has forces per unit density; give it a density and
//! they are in newtons.

use serde_json::json;
use vortia_core::Vec3;

use crate::case::{Case, FieldValues};
use crate::error::{PostError, Result};
use crate::report::{Metric, csv_text, escape};
use crate::seal::seal;

/// What a saved force report calls itself.
pub const FORCE_SCHEMA: &str = "vortia.force-report";

/// What the coefficients are made dimensionless with, and where the directions are.
#[derive(Debug, Clone, PartialEq)]
pub struct ForceReference {
    /// The density of an incompressible case, to give its forces in newtons. A gas has its own,
    /// so for it this is the density in `½ ρ U²`, and it must be given.
    pub density: Option<f64>,
    /// The speed of the stream, in m/s.
    pub speed: f64,
    /// The area, in m².
    pub area: f64,
    /// The length of the moment coefficient, in m.
    pub length: f64,
    /// The centre of rotation of the moments.
    pub centre: Vec3,
    /// The direction of the drag (a stream's direction), and the one of the lift. They must be
    /// perpendicular; the side force is along the third.
    pub drag: Vec3,
    pub lift: Vec3,
}

impl Default for ForceReference {
    fn default() -> Self {
        Self {
            density: None,
            speed: 1.0,
            area: 1.0,
            length: 1.0,
            centre: Vec3::default(),
            drag: Vec3::new(1.0, 0.0, 0.0),
            lift: Vec3::new(0.0, 1.0, 0.0),
        }
    }
}

impl ForceReference {
    /// The three directions, as unit vectors: drag, lift and the side.
    fn axes(&self) -> Result<[Vec3; 3]> {
        let unit = |v: Vec3, what: &str| {
            let n = v.norm();
            if n.is_finite() && n > 1e-12 {
                Ok(v * (1.0 / n))
            } else {
                Err(PostError::Invalid(format!("the {what} direction is zero")))
            }
        };
        let drag = unit(self.drag, "drag")?;
        let lift = unit(self.lift, "lift")?;
        if drag.dot(lift).abs() > 1e-6 {
            return Err(PostError::Invalid(
                "the drag and lift directions must be perpendicular".into(),
            ));
        }
        Ok([drag, lift, drag.cross(lift)])
    }

    fn check(&self) -> Result<()> {
        let positive = |v: f64, what: &str| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(PostError::Invalid(format!(
                    "the reference {what} must be a positive number, not {v}"
                )))
            }
        };
        positive(self.speed, "speed")?;
        positive(self.area, "area")?;
        positive(self.length, "length")?;
        if let Some(rho) = self.density {
            positive(rho, "density")?;
        }
        if ![self.centre.x, self.centre.y, self.centre.z]
            .iter()
            .all(|c| c.is_finite())
        {
            return Err(PostError::Invalid(
                "the centre of rotation must be finite".into(),
            ));
        }
        self.axes().map(|_| ())
    }
}

/// The forces on some walls at one time.
#[derive(Debug, Clone, PartialEq)]
pub struct ForceReport {
    pub label: String,
    pub case: String,
    pub time: String,
    pub patches: Vec<String>,
    pub reference: ForceReference,
    /// Whether the forces are per unit density (an incompressible case with no density given).
    pub per_unit_density: bool,
    pub area: f64,
    pub pressure: Vec3,
    pub viscous: Vec3,
    /// Moments about the reference centre.
    pub pressure_moment: Vec3,
    pub viscous_moment: Vec3,
}

/// The longest label kept.
const MAX_LABEL: usize = 160;

impl ForceReport {
    pub fn total(&self) -> Vec3 {
        self.pressure + self.viscous
    }

    /// `½ ρ U² A`: what the force coefficients divide by, in the units of the forces.
    fn dynamic_force(&self) -> f64 {
        let rho = self.reference.density.unwrap_or(1.0);
        0.5 * rho * self.reference.speed * self.reference.speed * self.reference.area
    }

    fn along(&self, v: Vec3, axis: usize) -> f64 {
        self.reference
            .axes()
            .ok()
            .and_then(|a| a.get(axis).copied())
            .map_or(f64::NAN, |a| v.dot(a))
    }

    fn force_unit(&self) -> &'static str {
        if self.per_unit_density {
            "m³/s²"
        } else {
            "N"
        }
    }

    fn moment_unit(&self) -> &'static str {
        if self.per_unit_density {
            "m⁴/s²"
        } else {
            "N·m"
        }
    }

    /// Every number of the report, keyed as in JSON and CSV.
    pub fn metrics(&self) -> Vec<Metric> {
        let f = self.force_unit();
        let n = self.moment_unit();
        let q = self.dynamic_force();
        let total = self.total();
        let moment = self.pressure_moment + self.viscous_moment;
        let m = |key, title, unit, value| Metric {
            key,
            title,
            unit,
            value,
        };
        vec![
            m("area", "Wall area", "m²", self.area),
            m("drag", "Drag", f, self.along(total, 0)),
            m(
                "pressureDrag",
                "Pressure drag",
                f,
                self.along(self.pressure, 0),
            ),
            m(
                "viscousDrag",
                "Viscous drag",
                f,
                self.along(self.viscous, 0),
            ),
            m("lift", "Lift", f, self.along(total, 1)),
            m(
                "pressureLift",
                "Pressure lift",
                f,
                self.along(self.pressure, 1),
            ),
            m(
                "viscousLift",
                "Viscous lift",
                f,
                self.along(self.viscous, 1),
            ),
            m("side", "Side force", f, self.along(total, 2)),
            m(
                "moment",
                "Moment about the side axis",
                n,
                self.along(moment, 2),
            ),
            m("cd", "Drag coefficient", "", self.along(total, 0) / q),
            m("cl", "Lift coefficient", "", self.along(total, 1) / q),
            m(
                "cm",
                "Moment coefficient",
                "",
                self.along(moment, 2) / (q * self.reference.length),
            ),
        ]
    }

    fn label(&self) -> String {
        self.label.trim().chars().take(MAX_LABEL).collect()
    }

    fn options_json(&self) -> serde_json::Value {
        let r = &self.reference;
        let v = |p: Vec3| json!([p.x, p.y, p.z]);
        json!({
            "patches": self.patches,
            "density": r.density,
            "speed": r.speed,
            "area": r.area,
            "length": r.length,
            "centre": v(r.centre),
            "drag": v(r.drag),
            "lift": v(r.lift),
            "perUnitDensity": self.per_unit_density,
        })
    }

    /// The report as JSON in the envelope of [`seal`], with its digest.
    pub fn json(&self) -> String {
        let metrics: serde_json::Map<String, serde_json::Value> = self
            .metrics()
            .into_iter()
            .map(|m| (m.key.to_owned(), json!(m.value)))
            .collect();
        seal(
            FORCE_SCHEMA,
            json!({
                "label": self.label(),
                "case": self.case,
                "time": self.time,
                "options": self.options_json(),
                "metrics": metrics,
                "assessment": "stored fields only; convergence and physical validity are not established by this report",
            }),
        )
    }

    /// The report as CSV, one row per number.
    pub fn csv(&self) -> String {
        let mut out = String::from("label,case,time,metric,value,unit\n");
        for m in self.metrics() {
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                csv_text(&self.label()),
                csv_text(&self.case),
                csv_text(&self.time),
                m.key,
                m.value,
                csv_text(m.unit),
            ));
        }
        out
    }

    /// The report as a page that needs nothing else to open.
    pub fn html(&self) -> String {
        let rows: String = self
            .metrics()
            .iter()
            .map(|m| {
                format!(
                    "<tr><td>{}</td><td class=\"n\">{}</td><td>{}</td></tr>\n",
                    escape(m.title),
                    m.value,
                    escape(m.unit)
                )
            })
            .collect();
        let r = &self.reference;
        format!(
            "<!doctype html>\n<html lang=\"en\"><meta charset=\"utf-8\"><title>{title}</title>\n\
             <style>body{{font:15px system-ui,sans-serif;max-width:900px;margin:40px auto;padding:0 24px;color:#25313e}}\
             table{{border-collapse:collapse;width:100%;margin:20px 0}}td,th{{border-bottom:1px solid #d9dfe6;padding:8px 10px;text-align:left}}\
             td.n{{font-family:ui-monospace,monospace;text-align:right}}h1{{color:#177886}}small{{color:#5b6672}}</style>\n\
             <h1>{title}</h1>\n<p>{case} · time {time}</p>\n\
             <p>Walls {patches}. Drag along ({dx}, {dy}, {dz}), lift along ({lx}, {ly}, {lz}); moments about ({cx}, {cy}, {cz}). \
             Coefficients use {speed} m/s, {area} m² and {length} m{density}.</p>\n\
             <p><small>Pressure force is the sum of p times the face area; the viscous force is the wall stress \
             times the face area. The report does not show that the run converged or that the model suits the flow.</small></p>\n\
             <table><tr><th>Quantity</th><th>Value</th><th>Unit</th></tr>\n{rows}</table>\n</html>\n",
            title = escape(&self.label()),
            case = escape(&self.case),
            time = escape(&self.time),
            patches = escape(&self.patches.join(", ")),
            dx = r.drag.x,
            dy = r.drag.y,
            dz = r.drag.z,
            lx = r.lift.x,
            ly = r.lift.y,
            lz = r.lift.z,
            cx = r.centre.x,
            cy = r.centre.y,
            cz = r.centre.z,
            speed = r.speed,
            area = r.area,
            length = r.length,
            density = r
                .density
                .map_or_else(String::new, |d| format!(", density {d} kg/m³")),
        )
    }
}

impl Case {
    /// The force and moment on `patches` at `time`.
    pub fn forces(
        &self,
        time: &str,
        patches: &[String],
        reference: &ForceReference,
    ) -> Result<ForceReport> {
        reference.check()?;
        if patches.is_empty() {
            return Err(PostError::Invalid("forces need at least one wall".into()));
        }
        let mut seen = std::collections::BTreeSet::new();
        let (mut area, mut pressure, mut viscous) = (0.0, Vec3::default(), Vec3::default());
        let (mut pressure_moment, mut viscous_moment) = (Vec3::default(), Vec3::default());
        let mut per_unit_density = true;
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
            let wall = self.wall(time, name)?;
            per_unit_density &= wall.kinematic;
            let FieldValues::Scalar(p) = self.patch_field(time, "p", name).map_err(|_| {
                PostError::Invalid("forces need the pressure: the time has no 'p'".into())
            })?
            else {
                return Err(PostError::Invalid("p is not a scalar field".into()));
            };
            area += wall.area;
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
                let arm = fc - reference.centre;
                let fp = sf * pk;
                let fv = shear * (-a);
                pressure = pressure + fp;
                viscous = viscous + fv;
                pressure_moment = pressure_moment + arm.cross(fp);
                viscous_moment = viscous_moment + arm.cross(fv);
            }
        }
        // An incompressible case with a density given has its forces in newtons.
        let rho = if per_unit_density {
            reference.density
        } else {
            None
        };
        let scale = rho.unwrap_or(1.0);
        if !per_unit_density && reference.density.is_none() {
            return Err(PostError::Invalid(
                "a gas needs the reference density for its coefficients".into(),
            ));
        }
        Ok(ForceReport {
            label: String::new(),
            case: String::new(),
            time: time.to_owned(),
            patches: patches.to_vec(),
            reference: reference.clone(),
            per_unit_density: per_unit_density && rho.is_none(),
            area,
            pressure: pressure * scale,
            viscous: viscous * scale,
            pressure_moment: pressure_moment * scale,
            viscous_moment: viscous_moment * scale,
        })
    }
}
