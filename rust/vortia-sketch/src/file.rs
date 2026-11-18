//! A sketch in a file: JSON, so a person or a script can read it too.

use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Result, SketchError};
use crate::model::{Constraint, Entity, Point, Sketch};

pub const FORMAT: &str = "vortia-sketch";
pub const VERSION: u64 = 1;
/// The largest sketch file read, in bytes.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> SketchError {
    SketchError::Invalid(message.into())
}

impl Sketch {
    pub fn to_json(&self) -> Value {
        let points: Vec<Value> = self
            .points
            .iter()
            .map(|p| json!({"x": p.x, "y": p.y, "fixed": p.fixed}))
            .collect();
        let entities: Vec<Value> = self
            .entities
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut v = match *e {
                    Entity::Line { a, b } => json!({"type": "line", "a": a, "b": b}),
                    Entity::Arc { start, end, center } => {
                        json!({"type": "arc", "start": start, "end": end, "center": center})
                    }
                };
                if let (Some(name), Some(map)) = (self.names.get(&i), v.as_object_mut()) {
                    map.insert("name".to_owned(), json!(name));
                }
                v
            })
            .collect();
        let constraints: Vec<Value> = self
            .constraints
            .iter()
            .map(|c| match *c {
                Constraint::Horizontal { a, b } => json!({"type": "horizontal", "a": a, "b": b}),
                Constraint::Vertical { a, b } => json!({"type": "vertical", "a": a, "b": b}),
                Constraint::Distance { a, b, value } => {
                    json!({"type": "distance", "a": a, "b": b, "value": value})
                }
                Constraint::DeltaX { a, b, value } => {
                    json!({"type": "deltaX", "a": a, "b": b, "value": value})
                }
                Constraint::DeltaY { a, b, value } => {
                    json!({"type": "deltaY", "a": a, "b": b, "value": value})
                }
                Constraint::Radius { arc, value } => {
                    json!({"type": "radius", "arc": arc, "value": value})
                }
                Constraint::Angle { a, b, degrees } => {
                    json!({"type": "angle", "a": a, "b": b, "degrees": degrees})
                }
            })
            .collect();
        json!({
            "format": FORMAT,
            "version": VERSION,
            "units": "mm",
            "points": points,
            "entities": entities,
            "constraints": constraints,
        })
    }

    pub fn from_json(value: &Value) -> Result<Self> {
        if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(invalid("this is not a Vortia sketch"));
        }
        match value.get("version").and_then(Value::as_u64) {
            Some(v) if v <= VERSION => {}
            _ => return Err(invalid("this sketch was written by a newer Vortia")),
        }
        if value.get("units").and_then(Value::as_str) != Some("mm") {
            return Err(invalid("a sketch is in millimetres"));
        }
        let list = |key: &str| -> Result<&Vec<Value>> {
            value
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| invalid(format!("the sketch has no '{key}' list")))
        };
        let num = |v: &Value, key: &str| -> Result<f64> {
            v.get(key)
                .and_then(Value::as_f64)
                .ok_or_else(|| invalid(format!("'{key}' must be a number")))
        };
        let id = |v: &Value, key: &str| -> Result<usize> {
            v.get(key)
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| invalid(format!("'{key}' must be a whole number")))
        };
        let mut s = Sketch::default();
        let (points, entities, constraints) =
            (list("points")?, list("entities")?, list("constraints")?);
        if points.len() > crate::model::MAX_POINTS
            || entities.len() > crate::model::MAX_ENTITIES
            || constraints.len() > crate::model::MAX_CONSTRAINTS
        {
            return Err(invalid("the sketch is larger than a sketch may be"));
        }
        for p in points {
            s.points.push(Point {
                x: num(p, "x")?,
                y: num(p, "y")?,
                fixed: p.get("fixed").and_then(Value::as_bool).unwrap_or(false),
            });
        }
        for (i, e) in entities.iter().enumerate() {
            if let Some(name) = e.get("name").and_then(Value::as_str) {
                s.names.insert(i, name.to_owned());
            }
            s.entities
                .push(match e.get("type").and_then(Value::as_str) {
                    Some("line") => Entity::Line {
                        a: id(e, "a")?,
                        b: id(e, "b")?,
                    },
                    Some("arc") => Entity::Arc {
                        start: id(e, "start")?,
                        end: id(e, "end")?,
                        center: id(e, "center")?,
                    },
                    other => {
                        return Err(invalid(format!("an entity of type {other:?} is not known")));
                    }
                });
        }
        for c in constraints {
            s.constraints
                .push(match c.get("type").and_then(Value::as_str) {
                    Some("horizontal") => Constraint::Horizontal {
                        a: id(c, "a")?,
                        b: id(c, "b")?,
                    },
                    Some("vertical") => Constraint::Vertical {
                        a: id(c, "a")?,
                        b: id(c, "b")?,
                    },
                    Some("distance") => Constraint::Distance {
                        a: id(c, "a")?,
                        b: id(c, "b")?,
                        value: num(c, "value")?,
                    },
                    Some("deltaX") => Constraint::DeltaX {
                        a: id(c, "a")?,
                        b: id(c, "b")?,
                        value: num(c, "value")?,
                    },
                    Some("deltaY") => Constraint::DeltaY {
                        a: id(c, "a")?,
                        b: id(c, "b")?,
                        value: num(c, "value")?,
                    },
                    Some("radius") => Constraint::Radius {
                        arc: id(c, "arc")?,
                        value: num(c, "value")?,
                    },
                    Some("angle") => Constraint::Angle {
                        a: id(c, "a")?,
                        b: id(c, "b")?,
                        degrees: num(c, "degrees")?,
                    },
                    other => {
                        return Err(invalid(format!(
                            "a dimension of type {other:?} is not known"
                        )));
                    }
                });
        }
        s.validate()?;
        Ok(s)
    }

    /// Reads a sketch file.
    pub fn open(path: &Path) -> Result<Self> {
        let io = |e: std::io::Error| SketchError::Io(format!("{}: {e}", path.display()));
        let meta = std::fs::metadata(path).map_err(io)?;
        if meta.len() > MAX_BYTES {
            return Err(invalid("the sketch file is too large"));
        }
        let text = std::fs::read_to_string(path).map_err(io)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| invalid(format!("the sketch file is not valid JSON: {e}")))?;
        Self::from_json(&value)
    }

    /// Writes the sketch to `path`, atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let text = serde_json::to_string_pretty(&self.to_json())
            .map_err(|e| SketchError::Io(e.to_string()))?;
        vortia_foamfile::write::atomic(path, format!("{text}\n").as_bytes())
            .map_err(|e| SketchError::Io(format!("{}: {e}", path.display())))
    }
}
