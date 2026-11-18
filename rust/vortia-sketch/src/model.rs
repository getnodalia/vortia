//! The sketch: points, the lines and arcs between them, and the dimensions that fix them.
//!
//! Lengths are in millimetres. `x` runs along the flow and `y` is the distance from the axis, so a
//! sketch is the half of the flow's section above the axis.

use std::collections::BTreeMap;

use crate::error::{Result, SketchError};

/// The most points, entities and constraints a sketch may have, so a file cannot ask for more
/// work than a window can do.
pub const MAX_POINTS: usize = 400;
pub const MAX_ENTITIES: usize = 400;
pub const MAX_CONSTRAINTS: usize = 1200;
/// The largest coordinate or dimension, in millimetres.
pub const MAX_SIZE: f64 = 1.0e6;

pub type PointId = usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    /// A fixed point is not moved by the constraints.
    pub fixed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entity {
    Line {
        a: PointId,
        b: PointId,
    },
    /// An arc about `center`, counter-clockwise from `start` to `end`. Both ends are as far from
    /// the centre as each other: the sketch keeps it so.
    Arc {
        start: PointId,
        end: PointId,
        center: PointId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    /// The two points are at the same height.
    Horizontal { a: PointId, b: PointId },
    /// The two points are at the same `x`.
    Vertical { a: PointId, b: PointId },
    /// The straight distance between the points.
    Distance { a: PointId, b: PointId, value: f64 },
    /// `x` of `b` minus `x` of `a`.
    DeltaX { a: PointId, b: PointId, value: f64 },
    /// `y` of `b` minus `y` of `a`.
    DeltaY { a: PointId, b: PointId, value: f64 },
    /// The radius of the arc that is entity `arc`.
    Radius { arc: usize, value: f64 },
    /// The angle of the line from `a` to `b` above the `x` direction, in degrees.
    Angle {
        a: PointId,
        b: PointId,
        degrees: f64,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sketch {
    pub points: Vec<Point>,
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
    /// The patch an entity's edge belongs to when the sketch is a closed region, by entity index.
    /// An entity without a name is in the patch `wall`.
    pub names: BTreeMap<usize, String>,
}

fn finite(name: &str, v: f64, limit: f64) -> Result<()> {
    if v.is_finite() && v.abs() <= limit {
        Ok(())
    } else {
        Err(SketchError::Invalid(format!(
            "{name} must be a number within ±{limit}, not {v}"
        )))
    }
}

impl Sketch {
    pub fn point(&mut self, x: f64, y: f64) -> PointId {
        self.points.push(Point { x, y, fixed: false });
        self.points.len() - 1
    }

    pub fn fixed_point(&mut self, x: f64, y: f64) -> PointId {
        self.points.push(Point { x, y, fixed: true });
        self.points.len() - 1
    }

    pub fn line(&mut self, a: PointId, b: PointId) -> usize {
        self.entities.push(Entity::Line { a, b });
        self.entities.len() - 1
    }

    pub fn arc(&mut self, start: PointId, end: PointId, center: PointId) -> usize {
        self.entities.push(Entity::Arc { start, end, center });
        self.entities.len() - 1
    }

    /// Names the patch the edge of `entity` belongs to.
    pub fn name(&mut self, entity: usize, patch: &str) {
        self.names.insert(entity, patch.to_owned());
    }

    pub fn constrain(&mut self, c: Constraint) {
        self.constraints.push(c);
    }

    /// Whether every reference is to something that is there and every number is usable.
    pub fn validate(&self) -> Result<()> {
        if self.points.len() > MAX_POINTS
            || self.entities.len() > MAX_ENTITIES
            || self.constraints.len() > MAX_CONSTRAINTS
        {
            return Err(SketchError::Invalid(format!(
                "a sketch has at most {MAX_POINTS} points, {MAX_ENTITIES} lines and arcs and {MAX_CONSTRAINTS} dimensions"
            )));
        }
        for p in &self.points {
            finite("a coordinate", p.x, MAX_SIZE)?;
            finite("a coordinate", p.y, MAX_SIZE)?;
        }
        let n = self.points.len();
        let known = |id: PointId| {
            if id < n {
                Ok(())
            } else {
                Err(SketchError::Invalid(format!(
                    "point {id} is not in the sketch"
                )))
            }
        };
        for e in &self.entities {
            match *e {
                Entity::Line { a, b } => {
                    known(a)?;
                    known(b)?;
                    if a == b {
                        return Err(SketchError::Invalid("a line needs two points".into()));
                    }
                }
                Entity::Arc { start, end, center } => {
                    known(start)?;
                    known(end)?;
                    known(center)?;
                    if start == end || start == center || end == center {
                        return Err(SketchError::Invalid(
                            "an arc needs a start, an end and a centre, all different".into(),
                        ));
                    }
                }
            }
        }
        for (entity, name) in &self.names {
            if *entity >= self.entities.len() {
                return Err(SketchError::Invalid(format!(
                    "entity {entity} is not in the sketch, so it cannot be named"
                )));
            }
            if name.is_empty()
                || name.len() > 64
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(SketchError::Invalid(format!(
                    "a patch name is letters, digits and underscores, up to 64: '{name}'"
                )));
            }
        }
        for c in &self.constraints {
            match *c {
                Constraint::Horizontal { a, b } | Constraint::Vertical { a, b } => {
                    known(a)?;
                    known(b)?;
                }
                Constraint::Distance { a, b, value } => {
                    known(a)?;
                    known(b)?;
                    finite("a distance", value, MAX_SIZE)?;
                    if value <= 0.0 {
                        return Err(SketchError::Invalid("a distance must be above zero".into()));
                    }
                }
                Constraint::DeltaX { a, b, value } | Constraint::DeltaY { a, b, value } => {
                    known(a)?;
                    known(b)?;
                    finite("a dimension", value, MAX_SIZE)?;
                }
                Constraint::Radius { arc, value } => {
                    if !matches!(self.entities.get(arc), Some(Entity::Arc { .. })) {
                        return Err(SketchError::Invalid(format!(
                            "entity {arc} is not an arc, so it has no radius"
                        )));
                    }
                    finite("a radius", value, MAX_SIZE)?;
                    if value <= 0.0 {
                        return Err(SketchError::Invalid("a radius must be above zero".into()));
                    }
                }
                Constraint::Angle { a, b, degrees } => {
                    known(a)?;
                    known(b)?;
                    finite("an angle", degrees, 360.0)?;
                }
            }
        }
        Ok(())
    }
}
