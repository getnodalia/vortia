//! A section analysis: what goes through a plane across the domain, worked out from the fields
//! the solver wrote. The plane cuts the cells; each cut is a polygon that carries the value of
//! its cell, so the integrals are exact for the cell-wise constant fields a finite-volume run
//! stores.
//!
//! The resultant combines the momentum flux and the pressure term through the section. It is
//! not an engine thrust or a complete force on a body: that needs the walls as well.

use vortia_core::Vec3;

use crate::case::{Case, FieldValues};
use crate::error::{PostError, Result};

/// Where the density of the fluid comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Density {
    /// The `rho` field of the time (compressible solvers write it).
    Field,
    /// One value, for a solver that does not write `rho`.
    Constant(f64),
}

/// What to cut and how to normalise it.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionOptions {
    /// The axis the plane is perpendicular to: 0, 1 or 2 for x, y or z.
    pub axis: usize,
    /// `1.0` or `-1.0`: which way along the axis counts as through the section.
    pub direction: f64,
    /// Where the plane cuts the axis.
    pub position: f64,
    pub density: Density,
    /// The pressure is `p/ρ` (an incompressible solver): it is multiplied by the density.
    pub kinematic_pressure: bool,
    /// Pressure the load is measured against.
    pub reference_pressure: f64,
    pub reference_density: f64,
    pub reference_speed: f64,
    pub reference_area: f64,
    /// The angle of an axisymmetric wedge (degrees, at most 5): the integrals are scaled to
    /// the full revolution. Only about the x axis.
    pub wedge_angle: Option<f64>,
}

impl Default for SectionOptions {
    fn default() -> Self {
        Self {
            axis: 0,
            direction: 1.0,
            position: 0.0,
            density: Density::Field,
            kinematic_pressure: false,
            reference_pressure: 101_325.0,
            reference_density: 1.225,
            reference_speed: 100.0,
            reference_area: 1.0,
            wedge_angle: None,
        }
    }
}

/// What goes through the section. Stored in SI units when the solver is; per unit density
/// (m³/s, m⁴/s²…) when the pressure is kinematic and the density is 1.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionAnalysis {
    pub area: f64,
    pub mass_flow: f64,
    pub forward_mass_flow: f64,
    pub reverse_mass_flow: f64,
    pub volume_flow: f64,
    pub mean_pressure: f64,
    pub mean_density: f64,
    pub mean_normal_velocity: f64,
    pub momentum_flux: f64,
    pub pressure_load: f64,
    /// Momentum flux plus the pressure load.
    pub resultant: f64,
    pub kinetic_power: f64,
    /// The resultant over `½ ρ_ref U_ref² A_ref`.
    pub coefficient: f64,
    /// `(p̄ − p_ref) / (½ ρ_ref U_ref²)`.
    pub mean_cp: f64,
    /// Cells the plane cuts, and the faces two cells share on it (counted once).
    pub cells: usize,
    pub shared_faces: usize,
    /// How much the integrals were scaled by: `360 / wedge angle`, or 1.
    pub area_scale: f64,
}

fn along(v: Vec3, axis: usize) -> f64 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// The area and the centre of a convex polygon.
fn area_and_centre(points: &[Vec3]) -> Option<(f64, Vec3)> {
    let first = *points.first()?;
    let mut weighted = Vec3::default();
    let mut area = 0.0;
    for pair in points.get(1..)?.windows(2) {
        let (a, b) = (pair.first()?, pair.get(1)?);
        let n = (*a - first).cross(*b - first);
        let tri = 0.5 * n.norm();
        area += tri;
        weighted = weighted + (first + *a + *b) * (tri / 3.0);
    }
    (area > 0.0 && area.is_finite()).then(|| (area, weighted * (1.0 / area)))
}

/// One piece of the section: its area, where it is, and its cell's values.
struct Piece {
    area: f64,
    centre: Vec3,
    rho: f64,
    u: Vec3,
    p: f64,
}

impl Case {
    /// The analysis of the plane `options` describes, at `time`.
    pub fn section(&self, time: &str, options: &SectionOptions) -> Result<SectionAnalysis> {
        check(options)?;
        let (axis, direction) = (options.axis, options.direction);
        let u = match self.field(time, "U")? {
            FieldValues::Vector(v) => v,
            FieldValues::Scalar(_) => {
                return Err(PostError::Invalid("U is not a vector field".into()));
            }
        };
        let p = match self.field(time, "p")? {
            FieldValues::Scalar(v) => v,
            FieldValues::Vector(_) => {
                return Err(PostError::Invalid("p is not a scalar field".into()));
            }
        };
        let rho = match options.density {
            Density::Field => match self.field(time, "rho") {
                Ok(FieldValues::Scalar(v)) => Some(v),
                _ => {
                    return Err(PostError::Invalid(
                        "the section needs the density: this time has no rho; give a constant density"
                            .into(),
                    ));
                }
            },
            Density::Constant(_) => None,
        };

        let mut origin = Vec3::default();
        let mut normal = Vec3::default();
        match axis {
            0 => {
                origin.x = options.position;
                normal.x = 1.0;
            }
            1 => {
                origin.y = options.position;
                normal.y = 1.0;
            }
            _ => {
                origin.z = options.position;
                normal.z = 1.0;
            }
        }
        let polygons = self.slice(origin, normal)?;
        if polygons.is_empty() {
            return Err(PostError::Invalid(
                "the plane does not cut the domain; move it inside".into(),
            ));
        }
        let mut pieces: Vec<Piece> = Vec::with_capacity(polygons.len());
        let mut shared = 0usize;
        for polygon in &polygons {
            let Some((area, centre)) = area_and_centre(&polygon.points) else {
                continue;
            };
            let cell = polygon.cell;
            let density = match (&rho, options.density) {
                (Some(r), _) => r.get(cell).copied(),
                (None, Density::Constant(c)) => Some(c),
                (None, Density::Field) => None,
            };
            let (Some(density), Some(&velocity), Some(&pressure)) =
                (density, u.get(cell), p.get(cell))
            else {
                return Err(PostError::Invalid(format!(
                    "the fields do not cover cell {cell}"
                )));
            };
            if !(density > 0.0 && density.is_finite()) {
                return Err(PostError::Invalid(
                    "the section holds a density that is not positive".into(),
                ));
            }
            if !(pressure.is_finite()
                && velocity.x.is_finite()
                && velocity.y.is_finite()
                && velocity.z.is_finite())
            {
                return Err(PostError::Invalid(
                    "the section holds a field value that is not a number".into(),
                ));
            }
            let p_pa = if options.kinematic_pressure {
                pressure * density
            } else {
                pressure
            };
            // The plane lying on the faces between cells cuts the cells on both sides: the
            // same face twice. It counts once, with the mean of the two cells.
            let twin = pieces.iter_mut().find(|q| {
                (q.centre - centre).norm() <= 1.0e-9 * area.sqrt().max(1.0e-30)
                    && (q.area - area).abs() <= 1.0e-9 * area
            });
            match twin {
                Some(q) => {
                    q.rho = 0.5 * (q.rho + density);
                    q.u = (q.u + velocity) * 0.5;
                    q.p = 0.5 * (q.p + p_pa);
                    shared += 1;
                }
                None => pieces.push(Piece {
                    area,
                    centre,
                    rho: density,
                    u: velocity,
                    p: p_pa,
                }),
            }
        }

        let scale = options.wedge_angle.map_or(1.0, |a| 360.0 / a);
        let (mut area, mut mass, mut forward, mut reverse, mut volume) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut pressure, mut density, mut normal_v) = (0.0, 0.0, 0.0);
        let (mut momentum, mut load, mut power) = (0.0, 0.0, 0.0);
        for q in &pieces {
            let un = along(q.u, axis) * direction;
            let flux = q.rho * un;
            area += q.area;
            mass += q.area * flux;
            forward += q.area * flux.max(0.0);
            reverse += q.area * (-flux).max(0.0);
            volume += q.area * un;
            pressure += q.area * q.p;
            density += q.area * q.rho;
            normal_v += q.area * un;
            momentum += q.area * q.rho * un * un;
            load += q.area * (q.p - options.reference_pressure);
            power += q.area * 0.5 * q.rho * q.u.dot(q.u) * un;
        }
        if area <= 0.0 {
            return Err(PostError::Invalid(
                "the plane does not cut any cell with an area; move it inside".into(),
            ));
        }
        let dynamic = 0.5 * options.reference_density * options.reference_speed.powi(2);
        let scaled = |v: f64| v * scale;
        let resultant = scaled(momentum) + scaled(load);
        let result = SectionAnalysis {
            area: scaled(area),
            mass_flow: scaled(mass),
            forward_mass_flow: scaled(forward),
            reverse_mass_flow: scaled(reverse),
            volume_flow: scaled(volume),
            mean_pressure: pressure / area,
            mean_density: density / area,
            mean_normal_velocity: normal_v / area,
            momentum_flux: scaled(momentum),
            pressure_load: scaled(load),
            resultant,
            kinetic_power: scaled(power),
            coefficient: resultant / (dynamic * options.reference_area),
            mean_cp: (pressure / area - options.reference_pressure) / dynamic,
            cells: pieces.len(),
            shared_faces: shared,
            area_scale: scale,
        };
        let numbers = [
            result.area,
            result.mass_flow,
            result.momentum_flux,
            result.pressure_load,
            result.kinetic_power,
            result.coefficient,
            result.mean_cp,
        ];
        if numbers.iter().any(|v| !v.is_finite()) {
            return Err(PostError::Invalid(
                "a section integral is not a number; review the fields and the references".into(),
            ));
        }
        Ok(result)
    }
}

fn check(o: &SectionOptions) -> Result<()> {
    let finite = [
        o.position,
        o.reference_pressure,
        o.reference_density,
        o.reference_speed,
        o.reference_area,
    ];
    if o.axis > 2 || (o.direction != 1.0 && o.direction != -1.0) {
        return Err(PostError::Invalid(
            "the section is perpendicular to the x, y or z axis, and goes the positive or negative way"
                .into(),
        ));
    }
    if finite.iter().any(|v| !v.is_finite()) {
        return Err(PostError::Invalid(
            "the section inputs must be numbers".into(),
        ));
    }
    if !(o.reference_density > 0.0 && o.reference_speed > 0.0 && o.reference_area > 0.0) {
        return Err(PostError::Invalid(
            "the reference density, speed and area must be above zero".into(),
        ));
    }
    if let Density::Constant(c) = o.density
        && !(c > 0.0 && c.is_finite())
    {
        return Err(PostError::Invalid("the density must be above zero".into()));
    }
    if let Some(a) = o.wedge_angle
        && (o.axis != 0 || !(a > 0.0 && a <= 5.0))
    {
        return Err(PostError::Invalid(
            "a full revolution needs a wedge about the x axis of at most five degrees".into(),
        ));
    }
    Ok(())
}
