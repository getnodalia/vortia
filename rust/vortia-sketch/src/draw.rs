//! Shapes drawn in one go (a rectangle, a circle, a slot, a polygon), corners rounded or cut, and
//! constraints added to what is drawn. Like every other edit here, each one happens whole or
//! leaves the sketch as it was.

use crate::error::{Result, SketchError};
use crate::model::{Constraint, Entity, MAX_CONSTRAINTS, PointId, Sketch};

/// The most sides a polygon may have.
pub const MAX_SIDES: usize = 64;

fn invalid(message: impl Into<String>) -> SketchError {
    SketchError::Invalid(message.into())
}

fn finite(values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("a point needs two numbers"))
    }
}

impl Sketch {
    fn xy(&self, id: PointId) -> Result<(f64, f64)> {
        self.points
            .get(id)
            .map(|p| (p.x, p.y))
            .ok_or_else(|| invalid(format!("point {id} is not in the sketch")))
    }

    /// An axis-aligned rectangle with opposite corners `a` and `b`: four lines, held horizontal
    /// and vertical. The four entities, from the bottom edge counter-clockwise.
    pub fn draw_rectangle(
        &mut self,
        a: (f64, f64),
        b: (f64, f64),
        tolerance: f64,
    ) -> Result<Vec<usize>> {
        self.transaction(|s| {
            finite(&[a.0, a.1, b.0, b.1])?;
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            if x1 - x0 <= tolerance || y1 - y0 <= tolerance {
                return Err(invalid("a rectangle needs a width and a height"));
            }
            let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
            let mut ids = Vec::with_capacity(4);
            for (x, y) in corners {
                let id = s.point_at(x, y, tolerance)?;
                if ids.contains(&id) {
                    return Err(invalid("a rectangle needs four different corners"));
                }
                ids.push(id);
            }
            let [p0, p1, p2, p3] = ids[..] else {
                return Err(invalid("a rectangle needs four corners"));
            };
            let edges = vec![
                s.line(p0, p1),
                s.line(p1, p2),
                s.line(p2, p3),
                s.line(p3, p0),
            ];
            s.constrain(Constraint::Horizontal { a: p0, b: p1 });
            s.constrain(Constraint::Vertical { a: p1, b: p2 });
            s.constrain(Constraint::Horizontal { a: p3, b: p2 });
            s.constrain(Constraint::Vertical { a: p0, b: p3 });
            Ok(edges)
        })
    }

    /// A circle about `center` through `through`. The entity it is.
    pub fn draw_circle(
        &mut self,
        center: (f64, f64),
        through: (f64, f64),
        tolerance: f64,
    ) -> Result<usize> {
        self.transaction(|s| {
            finite(&[center.0, center.1, through.0, through.1])?;
            if (through.0 - center.0).hypot(through.1 - center.1) <= tolerance {
                return Err(invalid("a circle needs a radius"));
            }
            let c = s.point_at(center.0, center.1, tolerance)?;
            let rim = s.point_at(through.0, through.1, tolerance)?;
            if c == rim {
                return Err(invalid(
                    "a circle needs a centre and a point on it, different",
                ));
            }
            s.entities.push(Entity::Circle { center: c, rim });
            Ok(s.entities.len() - 1)
        })
    }

    /// A slot, a stadium: two round ends about the points `a` and `b`, `half_width` from them,
    /// joined by two straight sides. The four entities: a side, the end about `b`, the end about
    /// `a`, the other side.
    pub fn draw_slot(
        &mut self,
        a: (f64, f64),
        b: (f64, f64),
        half_width: f64,
        tolerance: f64,
    ) -> Result<Vec<usize>> {
        self.transaction(|s| {
            finite(&[a.0, a.1, b.0, b.1, half_width])?;
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let length = dx.hypot(dy);
            if length <= tolerance || half_width <= tolerance {
                return Err(invalid("a slot needs a length and a width"));
            }
            let (ux, uy) = (dx / length, dy / length);
            // The left of the way from `a` to `b`.
            let (nx, ny) = (-uy * half_width, ux * half_width);
            let c1 = s.point(a.0, a.1);
            let c2 = s.point(b.0, b.1);
            let a1 = s.point(a.0 + nx, a.1 + ny);
            let a2 = s.point(a.0 - nx, a.1 - ny);
            let b1 = s.point(b.0 + nx, b.1 + ny);
            let b2 = s.point(b.0 - nx, b.1 - ny);
            let side = s.line(a1, b1);
            let end_b = s.arc(b2, b1, c2);
            let end_a = s.arc(a1, a2, c1);
            let other = s.line(a2, b2);
            s.constrain(Constraint::Equal { a: end_a, b: end_b });
            for (line, arc) in [(side, end_a), (side, end_b), (other, end_a), (other, end_b)] {
                s.constrain(Constraint::Tangent { line, round: arc });
            }
            Ok(vec![side, end_b, end_a, other])
        })
    }

    /// A regular polygon of `sides` sides with a corner at `vertex`, about `center`. The entities,
    /// one per side.
    pub fn draw_polygon(
        &mut self,
        center: (f64, f64),
        vertex: (f64, f64),
        sides: usize,
        tolerance: f64,
    ) -> Result<Vec<usize>> {
        self.transaction(|s| {
            finite(&[center.0, center.1, vertex.0, vertex.1])?;
            if !(3..=MAX_SIDES).contains(&sides) {
                return Err(invalid(format!(
                    "a polygon has 3 to {MAX_SIDES} sides, not {sides}"
                )));
            }
            let radius = (vertex.0 - center.0).hypot(vertex.1 - center.1);
            if radius <= tolerance {
                return Err(invalid("a polygon needs a radius"));
            }
            let a0 = (vertex.1 - center.1).atan2(vertex.0 - center.0);
            let corners: Vec<PointId> = (0..sides)
                .map(|i| {
                    let a = a0 + std::f64::consts::TAU * i as f64 / sides as f64;
                    s.point(center.0 + radius * a.cos(), center.1 + radius * a.sin())
                })
                .collect();
            let mut edges = Vec::with_capacity(sides);
            for (i, &from) in corners.iter().enumerate() {
                let to = corners.get((i + 1) % sides).copied().unwrap_or(from);
                edges.push(s.line(from, to));
            }
            Ok(edges)
        })
    }

    /// The two lines that meet at `corner` and nothing else does.
    pub fn corner_lines(&self, corner: PointId) -> Option<(usize, usize)> {
        let at: Vec<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.points().contains(&corner))
            .map(|(i, _)| i)
            .collect();
        match at.as_slice() {
            &[a, b]
                if matches!(self.entities.get(a), Some(Entity::Line { .. }))
                    && matches!(self.entities.get(b), Some(Entity::Line { .. })) =>
            {
                Some((a, b))
            }
            _ => None,
        }
    }

    /// Cuts the corner between two lines back: finds where each line is `back` from the corner (or
    /// the radius's tangent length), moves the line's end there and gives them the points. The two
    /// new ends, with the point the corner had on the line, and the line each belongs to.
    fn cut_corner(&mut self, corner: PointId, back: Back) -> Result<Cut> {
        let (e1, e2) = self
            .corner_lines(corner)
            .ok_or_else(|| invalid("a corner is where exactly two lines meet"))?;
        let other = |s: &Sketch, e: usize| -> Result<PointId> {
            match s.entities.get(e) {
                Some(Entity::Line { a, b }) => Ok(if *a == corner { *b } else { *a }),
                _ => Err(invalid("a corner is where exactly two lines meet")),
            }
        };
        let (a, b) = (other(self, e1)?, other(self, e2)?);
        let (p, pa, pb) = (self.xy(corner)?, self.xy(a)?, self.xy(b)?);
        let (l1, l2) = (
            (pa.0 - p.0).hypot(pa.1 - p.1),
            (pb.0 - p.0).hypot(pb.1 - p.1),
        );
        if l1 <= 0.0 || l2 <= 0.0 {
            return Err(invalid("a line of no length has no corner"));
        }
        let u1 = ((pa.0 - p.0) / l1, (pa.1 - p.1) / l1);
        let u2 = ((pb.0 - p.0) / l2, (pb.1 - p.1) / l2);
        let theta = (u1.0 * u2.0 + u1.1 * u2.1).clamp(-1.0, 1.0).acos();
        if !(0.02..=std::f64::consts::PI - 0.02).contains(&theta) {
            return Err(invalid(
                "those two lines are too nearly straight to have a corner",
            ));
        }
        let distance = match back {
            Back::Distance(d) => d,
            Back::Radius(r) => r / (theta / 2.0).tan(),
        };
        if !(distance.is_finite() && distance > 0.0) {
            return Err(invalid("a corner is cut back by a distance above zero"));
        }
        if distance >= 0.999 * l1.min(l2) {
            return Err(invalid(
                "that is more than the lines are long: take a smaller radius or distance",
            ));
        }
        let t1 = self.point(p.0 + u1.0 * distance, p.1 + u1.1 * distance);
        let t2 = self.point(p.0 + u2.0 * distance, p.1 + u2.1 * distance);
        for (entity, end) in [(e1, t1), (e2, t2)] {
            if let Some(Entity::Line { a, b }) = self.entities.get_mut(entity) {
                if *a == corner {
                    *a = end;
                } else {
                    *b = end;
                }
            }
        }
        // What kept a line horizontal, vertical or at an angle still does, from the new end; what
        // measured the corner is gone with it.
        let kept: Vec<Constraint> = std::mem::take(&mut self.constraints)
            .into_iter()
            .filter_map(|c| match c {
                Constraint::Horizontal { a: x, b: y } => follow(corner, x, y, [(a, t1), (b, t2)])
                    .map(|(x, y)| Constraint::Horizontal { a: x, b: y }),
                Constraint::Vertical { a: x, b: y } => follow(corner, x, y, [(a, t1), (b, t2)])
                    .map(|(x, y)| Constraint::Vertical { a: x, b: y }),
                Constraint::Angle {
                    a: x,
                    b: y,
                    degrees,
                } => follow(corner, x, y, [(a, t1), (b, t2)]).map(|(x, y)| Constraint::Angle {
                    a: x,
                    b: y,
                    degrees,
                }),
                other => {
                    let touches = match other {
                        Constraint::Distance { a, b, .. }
                        | Constraint::DeltaX { a, b, .. }
                        | Constraint::DeltaY { a, b, .. }
                        | Constraint::Coincident { a, b } => a == corner || b == corner,
                        _ => false,
                    };
                    (!touches).then_some(other)
                }
            })
            .collect();
        self.constraints = kept;
        Ok(Cut {
            first: (e1, t1),
            second: (e2, t2),
            corner: p,
            toward: (u1, u2),
            theta,
        })
    }

    /// Rounds the corner at `corner` with an arc of `radius` that touches both lines. The arc is an
    /// entity, held tangent to both with its radius as a dimension.
    pub fn fillet(&mut self, corner: PointId, radius: f64) -> Result<usize> {
        self.transaction(|s| {
            if !(radius.is_finite() && radius > 0.0) {
                return Err(invalid("a fillet needs a radius above zero"));
            }
            let cut = s.cut_corner(corner, Back::Radius(radius))?;
            let half = cut.theta / 2.0;
            let (u1, u2) = cut.toward;
            let bisector = (u1.0 + u2.0, u1.1 + u2.1);
            let length = bisector.0.hypot(bisector.1);
            if length <= 0.0 {
                return Err(invalid(
                    "those two lines are too nearly straight to have a corner",
                ));
            }
            let reach = radius / half.sin();
            let centre = s.point(
                cut.corner.0 + bisector.0 / length * reach,
                cut.corner.1 + bisector.1 / length * reach,
            );
            let (t1, t2) = (cut.first.1, cut.second.1);
            let (p1, p2, c) = (s.xy(t1)?, s.xy(t2)?, s.xy(centre)?);
            // The arc runs counter-clockwise, so it starts at whichever end that makes the short way.
            let ccw = (p1.0 - c.0) * (p2.1 - c.1) - (p1.1 - c.1) * (p2.0 - c.0) > 0.0;
            let arc = if ccw {
                s.arc(t1, t2, centre)
            } else {
                s.arc(t2, t1, centre)
            };
            s.constrain(Constraint::Tangent {
                line: cut.first.0,
                round: arc,
            });
            s.constrain(Constraint::Tangent {
                line: cut.second.0,
                round: arc,
            });
            s.constrain(Constraint::Radius { arc, value: radius });
            s.prune();
            s.solve()?;
            Ok(arc)
        })
    }

    /// Cuts the corner at `corner` with a straight line `distance` back along each line. The line
    /// it is.
    pub fn chamfer(&mut self, corner: PointId, distance: f64) -> Result<usize> {
        self.transaction(|s| {
            let cut = s.cut_corner(corner, Back::Distance(distance))?;
            let line = s.line(cut.first.1, cut.second.1);
            s.prune();
            Ok(line)
        })
    }

    /// Adds `constraint` and solves, so a constraint the drawing cannot meet is refused and the
    /// sketch is as it was. The constraint's position.
    pub fn add_constraint(&mut self, constraint: Constraint) -> Result<usize> {
        self.transaction(|s| {
            if s.constraints.len() >= MAX_CONSTRAINTS {
                return Err(invalid("a sketch has no room for more constraints"));
            }
            s.constrain(constraint);
            s.validate()?;
            s.solve()?;
            Ok(s.constraints.len() - 1)
        })
    }

    /// Fixes point `id` where it is, or lets it move again.
    pub fn set_fixed(&mut self, id: PointId, fixed: bool) -> Result<()> {
        self.transaction(|s| {
            s.points
                .get_mut(id)
                .map(|p| p.fixed = fixed)
                .ok_or_else(|| invalid(format!("point {id} is not in the sketch")))
        })
    }
}

/// How far back a corner is cut.
#[derive(Clone, Copy)]
enum Back {
    Distance(f64),
    Radius(f64),
}

/// What cutting a corner did: the line each side belongs to with its new end, and the corner's
/// place and the way the two lines leave it.
struct Cut {
    first: (usize, PointId),
    second: (usize, PointId),
    corner: (f64, f64),
    toward: ((f64, f64), (f64, f64)),
    theta: f64,
}

/// The ends of a constraint on the line from the old corner `corner` to the other end `x` or `y`
/// of one of the two lines, renamed to the line's new end; `None` when it was not one of those.
fn follow(
    corner: PointId,
    x: PointId,
    y: PointId,
    moved: [(PointId, PointId); 2],
) -> Option<(PointId, PointId)> {
    let new = |far: PointId| {
        moved
            .iter()
            .find(|(old, _)| *old == far)
            .map(|(_, new)| *new)
    };
    if x == corner {
        new(y).map(|n| (n, y))
    } else if y == corner {
        new(x).map(|n| (x, n))
    } else {
        Some((x, y))
    }
}
