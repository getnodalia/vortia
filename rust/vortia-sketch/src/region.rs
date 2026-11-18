//! From a closed sketch to a region of the plane: the chains of lines and arcs read as loops, the
//! largest the outer boundary and the others holes, each edge carrying the patch name it was given.

use std::collections::HashMap;

use crate::error::{Result, SketchError};
use crate::model::{PointId, Sketch};
use crate::profile::ends;

/// The patch an unnamed edge belongs to.
pub const DEFAULT_PATCH: &str = "wall";

/// One named piece of a loop: points in millimetres from its start to its end.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub points: Vec<(f64, f64)>,
    pub patch: String,
}

/// The loops of a region: the first is the outer boundary and the others are holes.
pub type Loops = Vec<Vec<Edge>>;

fn bad(message: impl Into<String>) -> SketchError {
    SketchError::Profile(message.into())
}

fn area(edges: &[Edge]) -> f64 {
    let mut twice = 0.0;
    for edge in edges {
        for w in edge.points.windows(2) {
            if let [p, q] = w {
                twice += p.0 * q.1 - q.0 * p.1;
            }
        }
    }
    twice / 2.0
}

/// Whether `p` is inside the loop (even-odd rule).
fn contains(edges: &[Edge], p: (f64, f64)) -> bool {
    let mut inside = false;
    for edge in edges {
        for w in edge.points.windows(2) {
            if let [a, b] = w
                && (a.1 > p.1) != (b.1 > p.1)
                && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0
            {
                inside = !inside;
            }
        }
    }
    inside
}

impl Sketch {
    /// The sketch as a region: every line and arc has to belong to a closed loop that does not
    /// touch itself or another loop, so each point has exactly two of them. Arcs are cut in steps of
    /// at most `step_degrees`.
    pub fn loops(&self, step_degrees: f64) -> Result<Loops> {
        self.validate()?;
        if !(step_degrees.is_finite() && (0.1..=90.0).contains(&step_degrees)) {
            return Err(bad("an arc is cut in steps of 0.1° to 90°"));
        }
        if self.entities.is_empty() {
            return Err(bad("the sketch has no edges: draw a closed outline"));
        }
        let mut at: HashMap<PointId, Vec<usize>> = HashMap::new();
        for (i, e) in self.entities.iter().enumerate() {
            let (a, b) = ends(e);
            at.entry(a).or_default().push(i);
            at.entry(b).or_default().push(i);
        }
        if at.values().any(|v| v.len() != 2) {
            return Err(bad(
                "every corner of a closed outline joins exactly two edges: there is an open end or a branch",
            ));
        }
        let mut seen = vec![false; self.entities.len()];
        let mut loops: Loops = Vec::new();
        for start in 0..self.entities.len() {
            if seen.get(start).copied().unwrap_or(true) {
                continue;
            }
            let mut edges: Vec<Edge> = Vec::new();
            let mut current = start;
            let Some(first) = self.entities.get(start) else {
                continue;
            };
            let origin = ends(first).0;
            let mut here = origin;
            loop {
                if let Some(s) = seen.get_mut(current) {
                    *s = true;
                }
                let Some(entity) = self.entities.get(current) else {
                    break;
                };
                let (a, b) = ends(entity);
                let forward = a == here;
                let mut points = self.sample(entity, step_degrees)?;
                if !forward {
                    points.reverse();
                }
                let patch = self
                    .names
                    .get(&current)
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_PATCH.to_owned());
                edges.push(Edge { points, patch });
                here = if forward { b } else { a };
                if here == origin {
                    break;
                }
                let next = at.get(&here).and_then(|v| {
                    v.iter()
                        .copied()
                        .find(|i| !seen.get(*i).copied().unwrap_or(true))
                });
                match next {
                    Some(n) => current = n,
                    None => return Err(bad("an outline does not close on itself")),
                }
            }
            loops.push(edges);
        }
        // The outer boundary is the loop that encloses the most.
        loops.sort_by(|p, q| area(q).abs().total_cmp(&area(p).abs()));
        if let Some((outer, holes)) = loops.split_first() {
            for hole in holes {
                let inside = hole
                    .first()
                    .and_then(|e| e.points.first())
                    .is_some_and(|&p| contains(outer, p));
                if !inside {
                    return Err(bad(
                        "a second outline lies outside the first: a hole has to be inside the outer boundary",
                    ));
                }
            }
        }
        Ok(loops)
    }
}
