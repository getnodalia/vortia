//! From the sketch to the wall profile a mesh is made from: the open chain of lines and arcs read
//! from the inlet to the outlet, and then cut into stations spaced evenly along its length.

use std::collections::HashMap;

use crate::error::{Result, SketchError};
use crate::model::{Entity, PointId, Sketch};

/// The wall as `(x, radius)` points in millimetres, `x` rising from the inlet.
pub type Profile = Vec<(f64, f64)>;

fn bad(message: &str) -> SketchError {
    SketchError::Profile(message.to_owned())
}

impl Sketch {
    /// The wall chain, arcs cut into steps of at most `step_degrees`. The sketch has to be one
    /// open chain of lines and arcs whose `x` only rises and whose `y` stays above the axis.
    pub fn profile(&self, step_degrees: f64) -> Result<Profile> {
        self.validate()?;
        if !(step_degrees.is_finite() && (0.1..=90.0).contains(&step_degrees)) {
            return Err(bad("an arc is cut in steps of 0.1° to 90°"));
        }
        if self.entities.is_empty() {
            return Err(bad("the sketch has no wall: draw a line or an arc"));
        }
        // The entities at each point.
        let mut at: HashMap<PointId, Vec<usize>> = HashMap::new();
        for (i, e) in self.entities.iter().enumerate() {
            let (a, b) = ends(e);
            at.entry(a).or_default().push(i);
            at.entry(b).or_default().push(i);
        }
        let mut open: Vec<PointId> = at
            .iter()
            .filter(|(_, v)| v.len() == 1)
            .map(|(p, _)| *p)
            .collect();
        if at.values().any(|v| v.len() > 2) || open.len() != 2 {
            return Err(bad(
                "the wall must be one open chain of lines and arcs, with no branches and no closed loop",
            ));
        }
        open.sort_by(|a, b| {
            let (pa, pb) = (self.points.get(*a), self.points.get(*b));
            let (xa, xb) = (pa.map_or(0.0, |p| p.x), pb.map_or(0.0, |p| p.x));
            xa.total_cmp(&xb)
        });
        let Some(&first) = open.first() else {
            return Err(bad("the sketch has no wall"));
        };
        let mut out: Profile = Vec::new();
        let (mut here, mut seen) = (first, vec![false; self.entities.len()]);
        while let Some(next) = at.get(&here).and_then(|v| {
            v.iter()
                .copied()
                .find(|i| !seen.get(*i).copied().unwrap_or(true))
        }) {
            if let Some(s) = seen.get_mut(next) {
                *s = true;
            }
            let Some(entity) = self.entities.get(next) else {
                break;
            };
            let (a, b) = ends(entity);
            let forward = a == here;
            let mut pts = self.sample(entity, step_degrees)?;
            if !forward {
                pts.reverse();
            }
            if !out.is_empty() {
                pts.remove(0);
            }
            out.extend(pts);
            here = if forward { b } else { a };
        }
        if seen.iter().any(|s| !*s) {
            return Err(bad(
                "the lines and arcs of the wall are not all joined in one chain",
            ));
        }
        if out.windows(2).any(|w| match w {
            [p, q] => q.0 <= p.0,
            _ => false,
        }) {
            return Err(bad(
                "the wall must run from the inlet to the outlet with x always rising",
            ));
        }
        if out.iter().any(|p| p.1 <= 0.0) {
            return Err(bad("the wall must stay above the axis (y above zero)"));
        }
        Ok(out)
    }

    /// The points of one entity from its first end to its second.
    fn sample(&self, e: &Entity, step_degrees: f64) -> Result<Profile> {
        let at = |id: PointId| -> Result<(f64, f64)> {
            self.points
                .get(id)
                .map(|p| (p.x, p.y))
                .ok_or_else(|| bad("a point is missing"))
        };
        match *e {
            Entity::Line { a, b } => Ok(vec![at(a)?, at(b)?]),
            Entity::Arc { start, end, center } => {
                let (s, f, c) = (at(start)?, at(end)?, at(center)?);
                let radius = (s.0 - c.0).hypot(s.1 - c.1);
                if radius <= 0.0
                    || (radius - (f.0 - c.0).hypot(f.1 - c.1)).abs() > 1.0e-6 * radius.max(1.0)
                {
                    return Err(bad(
                        "an arc's start and end must be as far from its centre as each other; solve the sketch",
                    ));
                }
                let a0 = (s.1 - c.1).atan2(s.0 - c.0);
                let mut sweep = (f.1 - c.1).atan2(f.0 - c.0) - a0;
                // Counter-clockwise from start to end.
                while sweep <= 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                let steps = ((sweep.to_degrees() / step_degrees).ceil() as usize).clamp(1, 4000);
                let mut pts = Vec::with_capacity(steps + 1);
                for i in 0..=steps {
                    if i == 0 {
                        pts.push(s);
                    } else if i == steps {
                        pts.push(f);
                    } else {
                        let a = a0 + sweep * i as f64 / steps as f64;
                        pts.push((c.0 + radius * a.cos(), c.1 + radius * a.sin()));
                    }
                }
                Ok(pts)
            }
        }
    }
}

fn ends(e: &Entity) -> (PointId, PointId) {
    match *e {
        Entity::Line { a, b } => (a, b),
        Entity::Arc { start, end, .. } => (start, end),
    }
}

/// `count + 1` stations spaced evenly along the length of `profile`, the first and last where the
/// profile starts and ends.
pub fn stations(profile: &Profile, count: usize) -> Result<Profile> {
    if !(2..=2000).contains(&count) {
        return Err(bad("a profile is cut into 2 to 2000 segments"));
    }
    if profile.len() < 2 {
        return Err(bad("a profile needs at least two points"));
    }
    let mut length = vec![0.0];
    for w in profile.windows(2) {
        if let [p, q] = w {
            let last = length.last().copied().unwrap_or(0.0);
            length.push(last + (q.0 - p.0).hypot(q.1 - p.1));
        }
    }
    let total = length.last().copied().unwrap_or(0.0);
    if !(total.is_finite() && total > 0.0) {
        return Err(bad("the profile has no length"));
    }
    let mut out = Vec::with_capacity(count + 1);
    let mut seg = 0;
    for i in 0..=count {
        let s = total * i as f64 / count as f64;
        while seg + 2 < length.len() && length.get(seg + 1).copied().unwrap_or(total) < s {
            seg += 1;
        }
        let (l0, l1) = (
            length.get(seg).copied().unwrap_or(0.0),
            length.get(seg + 1).copied().unwrap_or(total),
        );
        let (Some(p), Some(q)) = (profile.get(seg), profile.get(seg + 1)) else {
            return Err(bad("the profile is too short"));
        };
        let t = if l1 > l0 {
            ((s - l0) / (l1 - l0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
    }
    Ok(out)
}
