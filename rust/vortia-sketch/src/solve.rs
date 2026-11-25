//! Moving the free points until the dimensions hold: damped Gauss-Newton on the coordinates, with
//! the derivatives taken by differences. A sketch has tens of points, so a dense solve is plenty.

use crate::error::{Result, SketchError};
use crate::model::{Constraint, Entity, Sketch};

/// How well the dimensions hold after a solve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solution {
    pub iterations: usize,
    /// The largest miss of any dimension, in millimetres (or the same number for an angle).
    pub residual: f64,
    /// Coordinates the solver was free to move, and equations it had to meet.
    pub unknowns: usize,
    pub equations: usize,
}

const MAX_ITERATIONS: usize = 200;
const TOLERANCE: f64 = 1.0e-9;

/// The centre of an arc or circle.
fn centre(e: &Entity) -> Option<usize> {
    match *e {
        Entity::Arc { center, .. } | Entity::Circle { center, .. } => Some(center),
        Entity::Line { .. } => None,
    }
}

/// The radius of an arc or circle at the coordinates `at` gives.
fn radius(e: &Entity, at: &impl Fn(usize) -> (f64, f64)) -> Option<f64> {
    let (c, s) = match *e {
        Entity::Arc { start, center, .. } => (at(center), at(start)),
        Entity::Circle { center, rim } => (at(center), at(rim)),
        Entity::Line { .. } => return None,
    };
    Some((c.0 - s.0).hypot(c.1 - s.1))
}

/// The length of a line or the radius of an arc or circle.
fn size(e: &Entity, at: &impl Fn(usize) -> (f64, f64)) -> Option<f64> {
    match *e {
        Entity::Line { a, b } => {
            let (p, q) = (at(a), at(b));
            Some((q.0 - p.0).hypot(q.1 - p.1))
        }
        _ => radius(e, at),
    }
}

/// The vector along line `entity`, from its first point to its second.
fn direction(
    sketch: &Sketch,
    entity: usize,
    at: &impl Fn(usize) -> (f64, f64),
) -> Option<(f64, f64)> {
    match sketch.entities.get(entity)? {
        Entity::Line { a, b } => {
            let (p, q) = (at(*a), at(*b));
            Some((q.0 - p.0, q.1 - p.1))
        }
        _ => None,
    }
}

fn residuals(sketch: &Sketch, xy: &[f64], out: &mut Vec<f64>) {
    out.clear();
    let at = |id: usize| -> (f64, f64) {
        (
            xy.get(2 * id).copied().unwrap_or(0.0),
            xy.get(2 * id + 1).copied().unwrap_or(0.0),
        )
    };
    for e in &sketch.entities {
        if let Entity::Arc { start, end, center } = *e {
            let (c, s, f) = (at(center), at(start), at(end));
            out.push((c.0 - s.0).hypot(c.1 - s.1) - (c.0 - f.0).hypot(c.1 - f.1));
        }
    }
    for c in &sketch.constraints {
        match *c {
            Constraint::Horizontal { a, b } => out.push(at(b).1 - at(a).1),
            Constraint::Vertical { a, b } => out.push(at(b).0 - at(a).0),
            Constraint::Distance { a, b, value } => {
                let (p, q) = (at(a), at(b));
                out.push((q.0 - p.0).hypot(q.1 - p.1) - value);
            }
            Constraint::DeltaX { a, b, value } => out.push(at(b).0 - at(a).0 - value),
            Constraint::DeltaY { a, b, value } => out.push(at(b).1 - at(a).1 - value),
            Constraint::Radius { arc, value } => {
                if let Some(r) = sketch.entities.get(arc).and_then(|e| radius(e, &at)) {
                    out.push(r - value);
                }
            }
            Constraint::Coincident { a, b } => {
                let (p, q) = (at(a), at(b));
                out.push(q.0 - p.0);
                out.push(q.1 - p.1);
            }
            Constraint::Parallel { a, b } => {
                if let (Some(u), Some(v)) = (direction(sketch, a, &at), direction(sketch, b, &at)) {
                    // The sine of the angle between them: zero when they run the same way.
                    out.push(
                        (u.0 * v.1 - u.1 * v.0) / (u.0.hypot(u.1) * v.0.hypot(v.1)).max(1.0e-12),
                    );
                }
            }
            Constraint::Perpendicular { a, b } => {
                if let (Some(u), Some(v)) = (direction(sketch, a, &at), direction(sketch, b, &at)) {
                    out.push(
                        (u.0 * v.0 + u.1 * v.1) / (u.0.hypot(u.1) * v.0.hypot(v.1)).max(1.0e-12),
                    );
                }
            }
            Constraint::Equal { a, b } => {
                if let (Some(p), Some(q)) = (
                    sketch.entities.get(a).and_then(|e| size(e, &at)),
                    sketch.entities.get(b).and_then(|e| size(e, &at)),
                ) {
                    out.push(p - q);
                }
            }
            Constraint::Tangent { line, round } => {
                if let (Some(Entity::Line { a, b }), Some(e)) =
                    (sketch.entities.get(line), sketch.entities.get(round))
                    && let Some(r) = radius(e, &at)
                    && let Some(c) = centre(e)
                {
                    let (p, q, c) = (at(*a), at(*b), at(c));
                    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                    let length = dx.hypot(dy).max(1.0e-12);
                    // A line and an arc that meet at an end touch there: the line is square on
                    // the radius. Otherwise the line is a radius away from the centre.
                    let shared = [*a, *b]
                        .into_iter()
                        .find(|id| e.points().iter().any(|q| q == id) && centre(e) != Some(*id));
                    match shared {
                        Some(joint) => {
                            let j = at(joint);
                            out.push((dx * (j.0 - c.0) + dy * (j.1 - c.1)) / length);
                        }
                        None => {
                            let across = (dx * (c.1 - p.1) - dy * (c.0 - p.0)).abs() / length;
                            out.push(across - r);
                        }
                    }
                }
            }
            Constraint::Angle { a, b, degrees } => {
                // Cross product against the wanted direction: zero when the line points that way,
                // and smooth where an angle difference would jump at ±180°.
                let (p, q) = (at(a), at(b));
                let want = degrees.to_radians();
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                out.push(dy * want.cos() - dx * want.sin());
            }
        }
    }
}

/// Solves `A x = b` for a symmetric positive definite `A` of size `n` (row-major), by Gaussian
/// elimination with partial pivoting. `None` when a pivot vanishes.
fn gauss(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| {
            let (x, y) = (
                a.get(i * n + col).copied().unwrap_or(0.0).abs(),
                a.get(j * n + col).copied().unwrap_or(0.0).abs(),
            );
            x.total_cmp(&y)
        })?;
        let pv = a.get(pivot * n + col).copied()?;
        if pv.abs() < 1.0e-300 {
            return None;
        }
        if pivot != col {
            for k in 0..n {
                a.swap(col * n + k, pivot * n + k);
            }
            b.swap(col, pivot);
        }
        let d = a.get(col * n + col).copied()?;
        for row in col + 1..n {
            let f = a.get(row * n + col).copied()? / d;
            if f == 0.0 {
                continue;
            }
            for k in col..n {
                let sub = a.get(col * n + k).copied()? * f;
                if let Some(v) = a.get_mut(row * n + k) {
                    *v -= sub;
                }
            }
            let sub = b.get(col).copied()? * f;
            if let Some(v) = b.get_mut(row) {
                *v -= sub;
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let mut s = b.get(row).copied()?;
        for k in row + 1..n {
            s -= a.get(row * n + k).copied()? * x.get(k).copied()?;
        }
        *x.get_mut(row)? = s / a.get(row * n + row).copied()?;
    }
    Some(x)
}

/// The Jacobian of the residuals by central differences, `m` rows by one column per free
/// coordinate, row-major.
fn jacobian(sketch: &Sketch, xy: &mut [f64], free: &[usize], m: usize) -> Vec<f64> {
    let n = free.len();
    let mut jac = vec![0.0; m * n];
    let mut plus = Vec::new();
    let mut minus = Vec::new();
    for (col, &idx) in free.iter().enumerate() {
        let keep = xy.get(idx).copied().unwrap_or(0.0);
        let h = 1.0e-6 * keep.abs().max(1.0);
        if let Some(v) = xy.get_mut(idx) {
            *v = keep + h;
        }
        residuals(sketch, xy, &mut plus);
        if let Some(v) = xy.get_mut(idx) {
            *v = keep - h;
        }
        residuals(sketch, xy, &mut minus);
        if let Some(v) = xy.get_mut(idx) {
            *v = keep;
        }
        for row in 0..m {
            let d = (plus.get(row).copied().unwrap_or(0.0)
                - minus.get(row).copied().unwrap_or(0.0))
                / (2.0 * h);
            if let Some(v) = jac.get_mut(row * n + col) {
                *v = d;
            }
        }
    }
    jac
}

/// How many of the rows of the row-major `m` by `n` matrix `a` are independent.
fn rank(mut a: Vec<f64>, m: usize, n: usize) -> usize {
    let scale = a.iter().fold(0.0_f64, |w, v| w.max(v.abs()));
    if scale == 0.0 {
        return 0;
    }
    let tolerance = 1.0e-7 * scale;
    let mut rank = 0;
    for col in 0..n {
        if rank >= m {
            break;
        }
        // The row at or below `rank` with the largest entry in this column.
        let Some(pivot) = (rank..m).max_by(|&i, &j| {
            let (x, y) = (
                a.get(i * n + col).copied().unwrap_or(0.0).abs(),
                a.get(j * n + col).copied().unwrap_or(0.0).abs(),
            );
            x.total_cmp(&y)
        }) else {
            break;
        };
        let pv = a.get(pivot * n + col).copied().unwrap_or(0.0);
        if pv.abs() <= tolerance {
            continue;
        }
        for k in 0..n {
            a.swap(rank * n + k, pivot * n + k);
        }
        for row in rank + 1..m {
            let f = a.get(row * n + col).copied().unwrap_or(0.0) / pv;
            if f == 0.0 {
                continue;
            }
            for k in col..n {
                let sub = a.get(rank * n + k).copied().unwrap_or(0.0) * f;
                if let Some(v) = a.get_mut(row * n + k) {
                    *v -= sub;
                }
            }
        }
        rank += 1;
    }
    rank
}

impl Sketch {
    /// How many ways the free points can still move without breaking a dimension: the free
    /// coordinates less the independent equations that hold them. Zero is a fully dimensioned
    /// sketch; a drawn outline with no dimensions has a lot.
    pub fn degrees_of_freedom(&self) -> Result<usize> {
        self.validate()?;
        let mut xy: Vec<f64> = self.points.iter().flat_map(|p| [p.x, p.y]).collect();
        let free: Vec<usize> = self
            .points
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.fixed)
            .flat_map(|(i, _)| [2 * i, 2 * i + 1])
            .collect();
        let mut r = Vec::new();
        residuals(self, &xy, &mut r);
        let m = r.len();
        let jac = jacobian(self, &mut xy, &free, m);
        Ok(free.len().saturating_sub(rank(jac, m, free.len())))
    }

    /// Moves the free points so that every dimension holds. On an error the sketch is as it was.
    pub fn solve(&mut self) -> Result<Solution> {
        self.validate()?;
        let mut xy: Vec<f64> = self.points.iter().flat_map(|p| [p.x, p.y]).collect();
        let free: Vec<usize> = self
            .points
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.fixed)
            .flat_map(|(i, _)| [2 * i, 2 * i + 1])
            .collect();
        let n = free.len();
        let mut r = Vec::new();
        residuals(self, &xy, &mut r);
        let m = r.len();
        let worst = |r: &[f64]| r.iter().fold(0.0_f64, |w, v| w.max(v.abs()));
        let mut lambda = 1.0e-3;
        let mut iterations = 0;
        while worst(&r) > TOLERANCE && iterations < MAX_ITERATIONS && n > 0 && m > 0 {
            iterations += 1;
            let jac = jacobian(self, &mut xy, &free, m);
            // Normal equations: (JᵀJ + λI) δ = −Jᵀr.
            let mut a = vec![0.0; n * n];
            let mut g = vec![0.0; n];
            for row in 0..m {
                let rr = r.get(row).copied().unwrap_or(0.0);
                for i in 0..n {
                    let ji = jac.get(row * n + i).copied().unwrap_or(0.0);
                    if ji == 0.0 {
                        continue;
                    }
                    if let Some(v) = g.get_mut(i) {
                        *v -= ji * rr;
                    }
                    for k in 0..n {
                        let jk = jac.get(row * n + k).copied().unwrap_or(0.0);
                        if let Some(v) = a.get_mut(i * n + k) {
                            *v += ji * jk;
                        }
                    }
                }
            }
            // Try steps with more damping until one lowers the miss.
            let before = worst(&r);
            let mut moved = false;
            for _ in 0..30 {
                let mut damped = a.clone();
                for i in 0..n {
                    if let Some(v) = damped.get_mut(i * n + i) {
                        *v += lambda;
                    }
                }
                let Some(step) = gauss(damped, g.clone(), n) else {
                    lambda *= 10.0;
                    continue;
                };
                let mut trial = xy.clone();
                for (&idx, d) in free.iter().zip(&step) {
                    if let Some(v) = trial.get_mut(idx) {
                        *v += d;
                    }
                }
                let mut tr = Vec::new();
                residuals(self, &trial, &mut tr);
                if tr.iter().all(|v| v.is_finite()) && worst(&tr) < before {
                    xy = trial;
                    r = tr;
                    lambda = (lambda / 10.0).max(1.0e-12);
                    moved = true;
                    break;
                }
                lambda *= 10.0;
            }
            if !moved {
                break;
            }
        }
        let residual = worst(&r);
        if !residual.is_finite() || residual > 1.0e-6 {
            return Err(SketchError::Unsolved(format!(
                "the dimensions cannot all be met: the worst is off by {residual:.3e} mm. \
                 Two of them may contradict each other."
            )));
        }
        for (p, c) in self.points.iter_mut().zip(xy.as_chunks::<2>().0) {
            if !p.fixed {
                p.x = c[0];
                p.y = c[1];
            }
        }
        Ok(Solution {
            iterations,
            residual,
            unknowns: n,
            equations: m,
        })
    }
}
