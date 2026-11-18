//! A Delaunay triangulation made point by point (Bowyer–Watson) and then refined in the way
//! Ruppert's algorithm does: a boundary segment with a vertex inside its diametral circle is
//! split at its midpoint, and a triangle that is too big or too thin gets a vertex at its
//! circumcentre unless that would encroach a segment, in which case the segment is split instead.
//!
//! Every position is held in unit coordinates (the region scaled to fit a unit square), so the
//! tolerances below mean the same thing for a nozzle in metres and a part in millimetres.

use std::collections::{BinaryHeap, HashMap};

use crate::error::{MeshError, Result};

pub(super) const NONE: usize = usize::MAX;

pub(super) type P = [f64; 2];

/// The most the refinement may grow to before it gives up, so a sharp corner or a size that is
/// far too small cannot run away.
const MAX_STEPS_PER_VERTEX: usize = 64;

fn orient(a: P, b: P, c: P) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Positive when `d` is inside the circle through the counter-clockwise triangle `a b c`.
fn in_circle(a: P, b: P, c: P, d: P) -> f64 {
    let (adx, ady) = (a[0] - d[0], a[1] - d[1]);
    let (bdx, bdy) = (b[0] - d[0], b[1] - d[1]);
    let (cdx, cdy) = (c[0] - d[0], c[1] - d[1]);
    let (ad, bd, cd) = (
        adx * adx + ady * ady,
        bdx * bdx + bdy * bdy,
        cdx * cdx + cdy * cdy,
    );
    adx * (bdy * cd - bd * cdy) - ady * (bdx * cd - bd * cdx) + ad * (bdx * cdy - bdy * cdx)
}

fn dist2(a: P, b: P) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// The circumcentre and circumradius of a triangle, if it has any area.
fn circumcircle(a: P, b: P, c: P) -> Option<(P, f64)> {
    let d = 2.0 * orient(a, b, c);
    if d.abs() < 1e-300 {
        return None;
    }
    let (a2, b2, c2) = (
        a[0] * a[0] + a[1] * a[1],
        b[0] * b[0] + b[1] * b[1],
        c[0] * c[0] + c[1] * c[1],
    );
    let ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    let centre = [ux, uy];
    let r = dist2(centre, a).sqrt();
    (ux.is_finite() && uy.is_finite() && r.is_finite()).then_some((centre, r))
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Tri {
    /// The corners, counter-clockwise.
    pub v: [usize; 3],
    /// `n[i]` is the triangle across the edge opposite `v[i]`.
    pub n: [usize; 3],
    pub alive: bool,
    /// Whether the triangle is part of the region (not outside it, nor in a hole).
    pub inside: bool,
    stamp: u32,
    generation: u32,
}

impl Tri {
    const DEAD: Tri = Tri {
        v: [NONE; 3],
        n: [NONE; 3],
        alive: false,
        inside: false,
        stamp: 0,
        generation: 0,
    };

    pub fn vertex(&self, i: usize) -> usize {
        self.v.get(i % 3).copied().unwrap_or(NONE)
    }

    pub fn neighbour(&self, i: usize) -> usize {
        self.n.get(i % 3).copied().unwrap_or(NONE)
    }

    /// The edge opposite corner `i`, as the two corners in counter-clockwise order.
    pub fn edge(&self, i: usize) -> (usize, usize) {
        (self.vertex(i + 1), self.vertex(i + 2))
    }
}

fn key(a: usize, b: usize) -> (usize, usize) {
    if a < b { (a, b) } else { (b, a) }
}

/// A hash grid of the vertices, for the question "is anything inside this disc".
struct Grid {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    fn at(&self, p: P) -> (i64, i64) {
        (
            (p[0] / self.cell).floor() as i64,
            (p[1] / self.cell).floor() as i64,
        )
    }
}

/// The size a triangle may have at a point: the boundary's own sizes, growing with the distance
/// from the boundary at `grading` per unit, up to `max`.
pub(super) struct Sizer {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<(P, f64)>>,
    smin: f64,
    max: f64,
    grading: f64,
}

impl Sizer {
    pub fn new(points: &[(P, f64)], max: f64, grading: f64) -> Self {
        let cell = (max / 2.0).max(1e-9);
        let mut cells: HashMap<(i64, i64), Vec<(P, f64)>> = HashMap::new();
        let mut smin = max;
        for &(p, s) in points {
            let c = ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
            cells.entry(c).or_default().push((p, s));
            smin = smin.min(s);
        }
        Self {
            cell,
            cells,
            smin,
            max,
            grading,
        }
    }

    pub fn at(&self, x: P) -> f64 {
        let (ci, cj) = (
            (x[0] / self.cell).floor() as i64,
            (x[1] / self.cell).floor() as i64,
        );
        let mut best = self.max;
        for r in 0_i64..4096 {
            let nearest = ((r - 1).max(0) as f64) * self.cell;
            if self.smin + self.grading * nearest >= best {
                break;
            }
            for di in -r..=r {
                for dj in -r..=r {
                    if di.abs().max(dj.abs()) != r {
                        continue;
                    }
                    if let Some(list) = self.cells.get(&(ci + di, cj + dj)) {
                        for &(p, s) in list {
                            best = best.min(s + self.grading * dist2(p, x).sqrt());
                        }
                    }
                }
            }
        }
        best
    }
}

pub(super) struct Dt {
    pub pts: Vec<P>,
    pub tris: Vec<Tri>,
    free: Vec<usize>,
    last: usize,
    stamp: u32,
    grid: Grid,
    /// A triangle that has each vertex as a corner.
    vert_tri: Vec<usize>,
    /// The protected boundary edges and the patch each belongs to.
    pub segs: HashMap<(usize, usize), usize>,
    /// Vertices that could not be inserted (a degenerate cavity), counted for the report.
    pub skipped: usize,
}

struct Encroach {
    a: usize,
    b: usize,
}

impl Dt {
    /// A triangulation of a super-triangle that holds every point of the unit square with room.
    pub fn new(cell: f64) -> Self {
        let pts = vec![[-30.0, -30.0], [31.0, -30.0], [0.5, 31.0]];
        let tri = Tri {
            v: [0, 1, 2],
            n: [NONE; 3],
            alive: true,
            inside: false,
            stamp: 0,
            generation: 0,
        };
        Self {
            pts,
            tris: vec![tri],
            free: Vec::new(),
            last: 0,
            stamp: 0,
            grid: Grid {
                cell: cell.max(1e-9),
                cells: HashMap::new(),
            },
            vert_tri: vec![0, 0, 0],
            segs: HashMap::new(),
            skipped: 0,
        }
    }

    fn p(&self, i: usize) -> P {
        self.pts.get(i).copied().unwrap_or([0.0, 0.0])
    }

    pub fn tri(&self, i: usize) -> Tri {
        self.tris.get(i).copied().unwrap_or(Tri::DEAD)
    }

    fn corners(&self, t: &Tri) -> (P, P, P) {
        (
            self.p(t.vertex(0)),
            self.p(t.vertex(1)),
            self.p(t.vertex(2)),
        )
    }

    /// The triangle that holds `p`, found by walking from `start`.
    fn locate(&self, p: P, start: usize) -> Option<usize> {
        let mut t = if self.tri(start).alive {
            start
        } else {
            self.last
        };
        if !self.tri(t).alive {
            t = self.tris.iter().position(|t| t.alive)?;
        }
        for _ in 0..(2 * self.tris.len() + 16) {
            let tri = self.tri(t);
            let mut moved = false;
            for i in 0..3 {
                let (a, b) = tri.edge(i);
                if orient(self.p(a), self.p(b), p) < 0.0 {
                    let nb = tri.neighbour(i);
                    if nb == NONE {
                        return None;
                    }
                    t = nb;
                    moved = true;
                    break;
                }
            }
            if !moved {
                return Some(t);
            }
        }
        // A walk that goes round in circles: look at every triangle.
        self.tris.iter().position(|tri| {
            tri.alive
                && (0..3).all(|i| {
                    let (a, b) = tri.edge(i);
                    orient(self.p(a), self.p(b), p) >= -1e-14
                })
        })
    }

    /// The triangles whose circumcircle holds `p` (the ones a new vertex at `p` removes).
    fn cavity(&mut self, p: P, start: usize) -> Option<Vec<usize>> {
        let t0 = self.locate(p, start)?;
        self.stamp = self.stamp.wrapping_add(1);
        let s = self.stamp;
        if let Some(t) = self.tris.get_mut(t0) {
            t.stamp = s;
        }
        let mut stack = vec![t0];
        let mut cavity = Vec::new();
        while let Some(t) = stack.pop() {
            cavity.push(t);
            let tri = self.tri(t);
            for i in 0..3 {
                let nb = tri.neighbour(i);
                if nb == NONE {
                    continue;
                }
                let other = self.tri(nb);
                if other.stamp == s || !other.alive {
                    continue;
                }
                let (a, b) = tri.edge(i);
                let (pa, pb) = (self.p(a), self.p(b));
                let on_edge = orient(pa, pb, p).abs() <= 1e-13 * dist2(pa, pb).max(1e-300);
                let (c0, c1, c2) = self.corners(&other);
                if on_edge || in_circle(c0, c1, c2, p) > 0.0 {
                    if let Some(o) = self.tris.get_mut(nb) {
                        o.stamp = s;
                    }
                    stack.push(nb);
                }
            }
        }
        Some(cavity)
    }

    /// Every edge of the triangles in `cavity`, each once.
    fn cavity_edges(&self, cavity: &[usize]) -> Vec<(usize, usize)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for &t in cavity {
            let tri = self.tri(t);
            for i in 0..3 {
                let (a, b) = tri.edge(i);
                if seen.insert(key(a, b)) {
                    out.push((a, b));
                }
            }
        }
        out
    }

    /// The protected segments among `edges` that a vertex at `p` would encroach.
    fn encroached_by(&self, p: P, edges: &[(usize, usize)]) -> Vec<Encroach> {
        edges
            .iter()
            .filter(|&&(a, b)| self.segs.contains_key(&key(a, b)))
            .filter(|&&(a, b)| {
                let (pa, pb) = (self.p(a), self.p(b));
                let dot = (pa[0] - p[0]) * (pb[0] - p[0]) + (pa[1] - p[1]) * (pb[1] - p[1]);
                dot < -1e-12 * dist2(pa, pb)
            })
            .map(|&(a, b)| Encroach { a, b })
            .collect()
    }

    /// Whether some vertex other than its ends lies inside the diametral circle of `a`-`b`.
    fn vertex_encroaches(&self, a: usize, b: usize) -> bool {
        let (pa, pb) = (self.p(a), self.p(b));
        let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
        let len2 = dist2(pa, pb);
        let radius = len2.sqrt() / 2.0;
        let (lo, hi) = (
            self.grid.at([mid[0] - radius, mid[1] - radius]),
            self.grid.at([mid[0] + radius, mid[1] + radius]),
        );
        for i in lo.0..=hi.0 {
            for j in lo.1..=hi.1 {
                let Some(list) = self.grid.cells.get(&(i, j)) else {
                    continue;
                };
                for &v in list {
                    if v == a || v == b {
                        continue;
                    }
                    let q = self.p(v);
                    let dot = (pa[0] - q[0]) * (pb[0] - q[0]) + (pa[1] - q[1]) * (pb[1] - q[1]);
                    if dot < -1e-12 * len2 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Puts a vertex at `p`, replacing the triangles of `cavity`. `None` when the cavity is not
    /// a star around `p` (a degenerate position), in which case nothing has changed.
    fn insert_into(&mut self, p: P, cavity: &[usize]) -> Option<(usize, Vec<usize>)> {
        let s = self.stamp;
        // The ring of edges around the cavity, with the triangle outside each.
        struct Ring {
            a: usize,
            b: usize,
            outside: usize,
            old: usize,
            inside: bool,
        }
        let mut ring = Vec::new();
        for &t in cavity {
            let tri = self.tri(t);
            for i in 0..3 {
                let nb = tri.neighbour(i);
                if nb != NONE && self.tri(nb).stamp == s && self.tri(nb).alive {
                    continue;
                }
                let (a, b) = tri.edge(i);
                ring.push(Ring {
                    a,
                    b,
                    outside: nb,
                    old: t,
                    inside: tri.inside,
                });
            }
        }
        if ring.len() < 3
            || ring
                .iter()
                .any(|r| orient(self.p(r.a), self.p(r.b), p) <= 1e-18)
        {
            return None;
        }
        let v = self.pts.len();
        self.pts.push(p);
        self.vert_tri.push(NONE);
        let cell = self.grid.at(p);
        self.grid.cells.entry(cell).or_default().push(v);

        // Room for the new triangles: dead slots first.
        let mut ids = Vec::with_capacity(ring.len());
        for _ in &ring {
            ids.push(match self.free.pop() {
                Some(id) => id,
                None => {
                    self.tris.push(Tri::DEAD);
                    self.tris.len() - 1
                }
            });
        }
        // The triangles outside the ring now face a new triangle.
        for (r, &id) in ring.iter().zip(&ids) {
            if r.outside == NONE {
                continue;
            }
            if let Some(o) = self.tris.get_mut(r.outside) {
                for slot in o.n.iter_mut() {
                    if *slot == r.old {
                        *slot = id;
                    }
                }
            }
        }
        // The new triangles, `a b p`, and who shares the edges at `p`.
        let first: HashMap<usize, usize> =
            ring.iter().zip(&ids).map(|(r, &id)| (r.a, id)).collect();
        let second: HashMap<usize, usize> =
            ring.iter().zip(&ids).map(|(r, &id)| (r.b, id)).collect();
        for &t in cavity {
            if let Some(tri) = self.tris.get_mut(t) {
                tri.alive = false;
            }
        }
        for (r, &id) in ring.iter().zip(&ids) {
            let generation = self.tri(id).generation.wrapping_add(1);
            let tri = Tri {
                v: [r.a, r.b, v],
                n: [
                    first.get(&r.b).copied().unwrap_or(NONE),
                    second.get(&r.a).copied().unwrap_or(NONE),
                    r.outside,
                ],
                alive: true,
                inside: r.inside,
                stamp: 0,
                generation,
            };
            if let Some(slot) = self.tris.get_mut(id) {
                *slot = tri;
            }
            for c in [r.a, r.b, v] {
                if let Some(t) = self.vert_tri.get_mut(c) {
                    *t = id;
                }
            }
        }
        for &t in cavity {
            if !ids.contains(&t) {
                self.free.push(t);
            }
        }
        self.last = ids.first().copied().unwrap_or(self.last);
        Some((v, ids))
    }

    /// Puts a vertex at `p`. Used while there are no protected segments to respect.
    pub fn insert_plain(&mut self, p: P) -> Option<usize> {
        let start = self.last;
        let cavity = self.cavity(p, start)?;
        match self.insert_into(p, &cavity) {
            Some((v, _)) => Some(v),
            None => {
                self.skipped += 1;
                None
            }
        }
    }

    /// Splits the segment `a`-`b` at its midpoint. Returns the new vertex and the segments that
    /// vertex encroaches in turn.
    fn split(&mut self, a: usize, b: usize) -> Option<(usize, Vec<Encroach>, Vec<usize>)> {
        let patch = self.segs.remove(&key(a, b))?;
        let (pa, pb) = (self.p(a), self.p(b));
        let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
        let start = self.vert_tri.get(a).copied().unwrap_or(self.last);
        let Some(cavity) = self.cavity(mid, start) else {
            self.segs.insert(key(a, b), patch);
            self.skipped += 1;
            return None;
        };
        let edges = self.cavity_edges(&cavity);
        let Some((m, new)) = self.insert_into(mid, &cavity) else {
            self.segs.insert(key(a, b), patch);
            self.skipped += 1;
            return None;
        };
        self.segs.insert(key(a, m), patch);
        self.segs.insert(key(m, b), patch);
        let encroached = self.encroached_by(mid, &edges);
        Some((m, encroached, new))
    }

    /// Splits every segment that has a vertex in its diametral circle, and those that splitting
    /// puts in the same state, until none is left.
    fn protect_segments(&mut self, limit: usize) -> Result<()> {
        let mut queue: Vec<(usize, usize)> = self.segs.keys().copied().collect();
        while let Some((a, b)) = queue.pop() {
            if !self.segs.contains_key(&key(a, b)) {
                continue;
            }
            if self.pts.len() > limit {
                return Err(too_many());
            }
            if !self.vertex_encroaches(a, b) {
                continue;
            }
            let Some((m, encroached, _)) = self.split(a, b) else {
                continue;
            };
            queue.push((a, m));
            queue.push((m, b));
            queue.extend(encroached.into_iter().map(|e| (e.a, e.b)));
        }
        Ok(())
    }

    /// Marks the triangles inside the region: the outside is reached from the super-triangle,
    /// and every protected segment crossed turns the inside to the outside and back.
    pub fn label(&mut self) {
        let mut label: Vec<Option<bool>> = vec![None; self.tris.len()];
        // A triangle with a corner of the super-triangle is always outside.
        let start = self
            .tris
            .iter()
            .position(|t| t.alive && t.v.iter().any(|&v| v < 3));
        let Some(start) = start else {
            return;
        };
        if let Some(l) = label.get_mut(start) {
            *l = Some(false);
        }
        let mut stack = vec![start];
        while let Some(t) = stack.pop() {
            let tri = self.tri(t);
            let here = label.get(t).copied().flatten().unwrap_or(false);
            for i in 0..3 {
                let nb = tri.neighbour(i);
                if nb == NONE || !self.tri(nb).alive {
                    continue;
                }
                if label.get(nb).copied().flatten().is_some() {
                    continue;
                }
                let (a, b) = tri.edge(i);
                let crossing = self.segs.contains_key(&key(a, b));
                if let Some(l) = label.get_mut(nb) {
                    *l = Some(here ^ crossing);
                }
                stack.push(nb);
            }
        }
        for (t, l) in self.tris.iter_mut().zip(&label) {
            t.inside = l.unwrap_or(false) && t.alive;
        }
    }

    /// How badly a triangle needs a vertex: above 1 when it is too thin or too big.
    fn badness(&self, t: usize, sizer: &Sizer, ratio: f64) -> f64 {
        let tri = self.tri(t);
        if !tri.alive || !tri.inside {
            return 0.0;
        }
        let (a, b, c) = self.corners(&tri);
        let Some((centre, r)) = circumcircle(a, b, c) else {
            return 0.0;
        };
        let shortest = dist2(a, b).min(dist2(b, c)).min(dist2(c, a)).sqrt();
        if shortest < 1e-12 {
            return 0.0;
        }
        let centroid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        let _ = centre;
        let shape = r / shortest / ratio;
        let size = r / (0.65 * sizer.at(centroid));
        shape.max(size)
    }

    /// Refines until no triangle in the region is thinner than `ratio` (circumradius over
    /// shortest edge) or bigger than the size field asks for.
    pub fn refine(&mut self, sizer: &Sizer, ratio: f64, limit: usize) -> Result<()> {
        let mut heap: BinaryHeap<(u64, usize, u32)> = BinaryHeap::new();
        let push = |dt: &Dt, heap: &mut BinaryHeap<(u64, usize, u32)>, t: usize| {
            let bad = dt.badness(t, sizer, ratio);
            if bad > 1.0 {
                heap.push((bad.to_bits(), t, dt.tri(t).generation));
            }
        };
        for t in 0..self.tris.len() {
            push(self, &mut heap, t);
        }
        let mut steps = 0_usize;
        while let Some((_, t, generation)) = heap.pop() {
            let tri = self.tri(t);
            if !tri.alive || tri.generation != generation || !tri.inside {
                continue;
            }
            steps += 1;
            if self.pts.len() > limit || steps > limit.saturating_mul(MAX_STEPS_PER_VERTEX) {
                return Err(too_many());
            }
            let (a, b, c) = self.corners(&tri);
            let Some((centre, _)) = circumcircle(a, b, c) else {
                continue;
            };
            let Some(cavity) = self.cavity(centre, t) else {
                continue;
            };
            let edges = self.cavity_edges(&cavity);
            let encroached = self.encroached_by(centre, &edges);
            if !encroached.is_empty() {
                // Split the segments, and look at the triangle again.
                let mut queue: Vec<(usize, usize)> =
                    encroached.into_iter().map(|e| (e.a, e.b)).collect();
                while let Some((x, y)) = queue.pop() {
                    if !self.segs.contains_key(&key(x, y)) {
                        continue;
                    }
                    let Some((m, more, new)) = self.split(x, y) else {
                        continue;
                    };
                    queue.extend(more.into_iter().map(|e| (e.a, e.b)));
                    for id in new {
                        push(self, &mut heap, id);
                    }
                    // The halves may have a vertex inside them already.
                    for (p, q) in [(x, m), (m, y)] {
                        if self.vertex_encroaches(p, q) {
                            queue.push((p, q));
                        }
                    }
                }
                let again = self.tri(t);
                if again.alive && again.inside {
                    push(self, &mut heap, t);
                }
                continue;
            }
            // A vertex outside the region is a sign of a degenerate triangle: leave it be.
            if cavity.iter().any(|&q| !self.tri(q).inside) {
                continue;
            }
            if let Some((_, new)) = self.insert_into(centre, &cavity) {
                for id in new {
                    push(self, &mut heap, id);
                }
            } else {
                self.skipped += 1;
            }
        }
        Ok(())
    }

    /// Triangulates the boundary points, protects the boundary segments and labels the region.
    /// `ring` gives the boundary segments as pairs of the points' indices in `points`, and the
    /// patch each belongs to.
    pub fn build(
        &mut self,
        points: &[P],
        segments: &[(usize, usize, usize)],
        limit: usize,
    ) -> Result<()> {
        let mut ids = Vec::with_capacity(points.len());
        for &p in points {
            match self.insert_plain(p) {
                Some(v) => ids.push(v),
                None => {
                    return Err(MeshError::Invalid(
                        "two points of the boundary are the same, or the boundary is degenerate"
                            .into(),
                    ));
                }
            }
        }
        for &(a, b, patch) in segments {
            let (Some(&va), Some(&vb)) = (ids.get(a), ids.get(b)) else {
                continue;
            };
            self.segs.insert(key(va, vb), patch);
        }
        self.protect_segments(limit)?;
        self.label();
        Ok(())
    }
}

fn too_many() -> MeshError {
    MeshError::Invalid(
        "the mesh would have too many cells: make the size larger, or the region simpler".into(),
    )
}
