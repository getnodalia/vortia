//! Editing a sketch the way a canvas does: points picked where the pointer is (joining a point
//! already there), lines and arcs drawn between them, points dragged, edges deleted, dimensions
//! added at the value the drawing already has. Every edit either happens whole or leaves the sketch
//! as it was.

use crate::error::{Result, SketchError};
use crate::model::{Constraint, Entity, MAX_ENTITIES, MAX_POINTS, PointId, Sketch};
use crate::region::{DEFAULT_PATCH, Edge};

fn invalid(message: impl Into<String>) -> SketchError {
    SketchError::Invalid(message.into())
}

fn finite(x: f64, y: f64) -> Result<()> {
    if x.is_finite() && y.is_finite() {
        Ok(())
    } else {
        Err(invalid("a point needs two numbers"))
    }
}

/// How a dimension measures what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    /// The straight distance between two points.
    Distance,
    /// The distance along `x`, from the first point to the second.
    Along,
    /// The distance along `y`, from the first point to the second.
    Height,
}

impl Sketch {
    /// The point within `tolerance` of `(x, y)`, the nearest when there are several.
    pub fn point_near(&self, x: f64, y: f64, tolerance: f64) -> Option<PointId> {
        self.points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (p.x - x).hypot(p.y - y)))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The point at `(x, y)`: one that is already within `tolerance`, or a new free one.
    pub fn point_at(&mut self, x: f64, y: f64, tolerance: f64) -> Result<PointId> {
        finite(x, y)?;
        if let Some(id) = self.point_near(x, y, tolerance) {
            return Ok(id);
        }
        if self.points.len() >= MAX_POINTS {
            return Err(invalid(format!("a sketch has at most {MAX_POINTS} points")));
        }
        Ok(self.point(x, y))
    }

    /// Runs `edit` on a copy and keeps it only when it works and the sketch is still valid.
    pub(crate) fn transaction<T>(
        &mut self,
        edit: impl FnOnce(&mut Sketch) -> Result<T>,
    ) -> Result<T> {
        let mut next = self.clone();
        let out = edit(&mut next)?;
        next.validate()?;
        *self = next;
        Ok(out)
    }

    /// A line from `from` to `to`, joining the points already within `tolerance` of either end.
    /// The entity it is.
    pub fn draw_line(&mut self, from: (f64, f64), to: (f64, f64), tolerance: f64) -> Result<usize> {
        self.transaction(|s| {
            if s.entities.len() >= MAX_ENTITIES {
                return Err(invalid(format!(
                    "a sketch has at most {MAX_ENTITIES} lines and arcs"
                )));
            }
            let a = s.point_at(from.0, from.1, tolerance)?;
            let b = s.point_at(to.0, to.1, tolerance)?;
            if a == b {
                return Err(invalid("a line needs two different points"));
            }
            Ok(s.line(a, b))
        })
    }

    /// An arc about `center` that starts at `start` and runs counter-clockwise to the point of its
    /// circle that lies towards `toward`. The entity it is.
    pub fn draw_arc(
        &mut self,
        center: (f64, f64),
        start: (f64, f64),
        toward: (f64, f64),
        tolerance: f64,
    ) -> Result<usize> {
        self.transaction(|s| {
            if s.entities.len() >= MAX_ENTITIES {
                return Err(invalid(format!(
                    "a sketch has at most {MAX_ENTITIES} lines and arcs"
                )));
            }
            finite(toward.0, toward.1)?;
            let c = s.point_at(center.0, center.1, tolerance)?;
            let (cx, cy) = s
                .points
                .get(c)
                .map(|p| (p.x, p.y))
                .ok_or_else(|| invalid("the centre is not in the sketch"))?;
            let a = s.point_at(start.0, start.1, tolerance)?;
            let (ax, ay) = s
                .points
                .get(a)
                .map(|p| (p.x, p.y))
                .ok_or_else(|| invalid("the start is not in the sketch"))?;
            let radius = (ax - cx).hypot(ay - cy);
            let (dx, dy) = (toward.0 - cx, toward.1 - cy);
            let length = dx.hypot(dy);
            if radius <= 0.0 || length <= 0.0 {
                return Err(invalid(
                    "an arc needs a centre, a start away from it and an end away from it",
                ));
            }
            let end = (cx + dx / length * radius, cy + dy / length * radius);
            // The end is on the circle, which may be where another point already is.
            let b = s.point_at(end.0, end.1, tolerance.min(radius * 0.01))?;
            if a == b || a == c || b == c {
                return Err(invalid(
                    "an arc needs a start, an end and a centre, all different",
                ));
            }
            Ok(s.arc(a, b, c))
        })
    }

    /// Puts point `id` at `(x, y)` and solves again, so the dimensions that hold it keep holding.
    /// The place it ends up, which is not `(x, y)` when a dimension holds it back.
    pub fn move_point(&mut self, id: PointId, x: f64, y: f64) -> Result<(f64, f64)> {
        self.transaction(|s| {
            finite(x, y)?;
            let p = s
                .points
                .get_mut(id)
                .ok_or_else(|| invalid(format!("point {id} is not in the sketch")))?;
            if p.fixed {
                return Err(invalid("a fixed point cannot be moved"));
            }
            p.x = x;
            p.y = y;
            s.solve()?;
            s.points
                .get(id)
                .map(|p| (p.x, p.y))
                .ok_or_else(|| invalid(format!("point {id} is not in the sketch")))
        })
    }

    /// Every line and arc as it is drawn, closed outline or not, with the patch it was named
    /// for. Arcs are cut in steps of at most `step_degrees`.
    pub fn edges(&self, step_degrees: f64) -> Result<Vec<Edge>> {
        self.validate()?;
        self.entities
            .iter()
            .enumerate()
            .map(|(i, e)| {
                Ok(Edge {
                    points: self.sample(e, step_degrees)?,
                    patch: self
                        .names
                        .get(&i)
                        .cloned()
                        .unwrap_or_else(|| DEFAULT_PATCH.to_owned()),
                })
            })
            .collect()
    }

    /// The line or arc whose edge passes within `tolerance` of `(x, y)`, the nearest.
    pub fn entity_near(&self, x: f64, y: f64, tolerance: f64) -> Option<usize> {
        self.entities
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let points = self.sample(e, 5.0).ok()?;
                let d = points
                    .windows(2)
                    .filter_map(|w| match w {
                        [p, q] => Some(distance_to_segment((x, y), *p, *q)),
                        _ => None,
                    })
                    .fold(f64::MAX, f64::min);
                (d <= tolerance).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Deletes the line or arc `entity`, its dimension if it was an arc's radius, and the points
    /// nothing uses any more with the dimensions that were on them.
    pub fn remove_edge(&mut self, entity: usize) -> Result<()> {
        self.transaction(|s| {
            if entity >= s.entities.len() {
                return Err(invalid(format!("edge {entity} is not in the sketch")));
            }
            s.entities.remove(entity);
            s.names = std::mem::take(&mut s.names)
                .into_iter()
                .filter(|(i, _)| *i != entity)
                .map(|(i, name)| (if i > entity { i - 1 } else { i }, name))
                .collect();
            // What named the edge goes with it, and the edges after it are one place nearer.
            s.constraints = std::mem::take(&mut s.constraints)
                .into_iter()
                .filter_map(|c| {
                    c.map_edges(|i| match i.cmp(&entity) {
                        std::cmp::Ordering::Less => Some(i),
                        std::cmp::Ordering::Equal => None,
                        std::cmp::Ordering::Greater => Some(i - 1),
                    })
                })
                .collect();
            s.prune();
            Ok(())
        })
    }

    /// Takes out the points no line or arc uses, with the dimensions that touch them, and
    /// renumbers the rest.
    pub(crate) fn prune(&mut self) {
        let mut used = vec![false; self.points.len()];
        for e in &self.entities {
            for id in e.points() {
                if let Some(u) = used.get_mut(id) {
                    *u = true;
                }
            }
        }
        let mut renumber = vec![None; self.points.len()];
        let mut next = 0;
        for (old, keep) in used.iter().enumerate() {
            if *keep {
                if let Some(r) = renumber.get_mut(old) {
                    *r = Some(next);
                }
                next += 1;
            }
        }
        let new_id = |id: PointId| renumber.get(id).copied().flatten();
        self.points = self
            .points
            .iter()
            .zip(&used)
            .filter(|(_, keep)| **keep)
            .map(|(p, _)| *p)
            .collect();
        for e in &mut self.entities {
            match e {
                Entity::Line { a, b } => {
                    *a = new_id(*a).unwrap_or(*a);
                    *b = new_id(*b).unwrap_or(*b);
                }
                Entity::Arc { start, end, center } => {
                    *start = new_id(*start).unwrap_or(*start);
                    *end = new_id(*end).unwrap_or(*end);
                    *center = new_id(*center).unwrap_or(*center);
                }
                Entity::Circle { center, rim } => {
                    *center = new_id(*center).unwrap_or(*center);
                    *rim = new_id(*rim).unwrap_or(*rim);
                }
            }
        }
        self.constraints = std::mem::take(&mut self.constraints)
            .into_iter()
            .filter_map(|c| c.map_points(new_id))
            .collect();
    }

    /// Holds the points `a` and `b` at the measure they have now, and returns the constraint's
    /// position among the dimensions. A dimension of zero is refused: it would hold them together.
    pub fn dimension_points(&mut self, a: PointId, b: PointId, measure: Measure) -> Result<usize> {
        self.transaction(|s| {
            let (p, q) = match (s.points.get(a), s.points.get(b)) {
                (Some(p), Some(q)) if a != b => (*p, *q),
                _ => return Err(invalid("a dimension needs two different points")),
            };
            let constraint = match measure {
                Measure::Distance => Constraint::Distance {
                    a,
                    b,
                    value: (q.x - p.x).hypot(q.y - p.y),
                },
                Measure::Along => Constraint::DeltaX {
                    a,
                    b,
                    value: q.x - p.x,
                },
                Measure::Height => Constraint::DeltaY {
                    a,
                    b,
                    value: q.y - p.y,
                },
            };
            if let Constraint::Distance { value, .. } = constraint
                && value <= 0.0
            {
                return Err(invalid("those points are in the same place"));
            }
            if s.constraints.len() >= crate::model::MAX_CONSTRAINTS {
                return Err(invalid("a sketch has no room for more dimensions"));
            }
            s.constrain(constraint);
            Ok(s.constraints.len() - 1)
        })
    }
}

fn distance_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}
