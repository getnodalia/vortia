//! Questions asked of a closed triangulated surface: which triangles are near a box, does a
//! box touch the surface, and is a point inside it.
//!
//! The triangles are binned in a uniform grid over their bounding box, so a question costs the
//! few triangles near it and not the whole surface. "Inside" is the parity of the crossings of
//! a ray along +x, with the ray perturbed symbolically so that a point on an edge or a vertex
//! counts each crossing exactly once on a watertight surface.

use crate::stl::Stl;

/// The component `a` (0, 1 or 2) of a point; any other `a` reads as 0.
pub(super) fn at<T: Copy + Default>(v: &[T; 3], a: usize) -> T {
    v.get(a).copied().unwrap_or_default()
}

/// Sets component `a` of a point; an index past 2 does nothing.
pub(super) fn put<T>(v: &mut [T; 3], a: usize, x: T) {
    if let Some(slot) = v.get_mut(a) {
        *slot = x;
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[derive(Clone, Copy)]
pub(super) struct Tri {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub c: [f64; 3],
}

pub(super) struct TriGrid {
    tris: Vec<Tri>,
    lo: [f64; 3],
    hi: [f64; 3],
    cell: [f64; 3],
    n: [usize; 3],
    start: Vec<u32>,
    items: Vec<u32>,
}

/// The most cells a grid axis is cut into.
const MAX_AXIS: usize = 128;

impl TriGrid {
    /// Bins the triangles of `surface`. The surface has been checked to be finite and non-empty.
    pub(super) fn new(surface: &Stl) -> Self {
        let corner = |i: u32| {
            surface
                .vertices
                .get(i as usize)
                .copied()
                .unwrap_or_default()
        };
        let tris: Vec<Tri> = surface
            .triangles
            .iter()
            .map(|t| Tri {
                a: corner(t[0]),
                b: corner(t[1]),
                c: corner(t[2]),
            })
            .collect();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for t in &tris {
            for p in [t.a, t.b, t.c] {
                for k in 0..3 {
                    let (l, h) = (at(&lo, k).min(at(&p, k)), at(&hi, k).max(at(&p, k)));
                    put(&mut lo, k, l);
                    put(&mut hi, k, h);
                }
            }
        }
        if tris.is_empty() {
            lo = [0.0; 3];
            hi = [0.0; 3];
        }
        // A flat surface still gets a thin slab, so every triangle has a cell.
        let mut n = [1usize; 3];
        let mut cell = [1.0f64; 3];
        let per_axis = ((tris.len() as f64).cbrt() * 0.8).ceil().max(1.0) as usize;
        for k in 0..3 {
            let extent = (at(&hi, k) - at(&lo, k)).max(1e-12);
            put(&mut hi, k, at(&lo, k) + extent);
            let count = per_axis.clamp(1, MAX_AXIS);
            put(&mut n, k, count);
            put(&mut cell, k, extent / count as f64);
        }
        let total = n[0] * n[1] * n[2];
        let mut grid = Self {
            tris,
            lo,
            hi,
            cell,
            n,
            start: vec![0; total + 1],
            items: Vec::new(),
        };
        // Two passes over the same ranges: count, then fill.
        let ranges: Vec<([usize; 3], [usize; 3])> = grid
            .tris
            .iter()
            .map(|t| {
                let tlo = [
                    t.a[0].min(t.b[0]).min(t.c[0]),
                    t.a[1].min(t.b[1]).min(t.c[1]),
                    t.a[2].min(t.b[2]).min(t.c[2]),
                ];
                let thi = [
                    t.a[0].max(t.b[0]).max(t.c[0]),
                    t.a[1].max(t.b[1]).max(t.c[1]),
                    t.a[2].max(t.b[2]).max(t.c[2]),
                ];
                grid.index_range(tlo, thi)
            })
            .collect();
        for (a, b) in &ranges {
            for z in a[2]..=b[2] {
                for y in a[1]..=b[1] {
                    for x in a[0]..=b[0] {
                        let c = grid.flat([x, y, z]);
                        if let Some(s) = grid.start.get_mut(c + 1) {
                            *s += 1;
                        }
                    }
                }
            }
        }
        for c in 0..total {
            let prev = grid.start.get(c).copied().unwrap_or(0);
            if let Some(s) = grid.start.get_mut(c + 1) {
                *s += prev;
            }
        }
        let filled = grid.start.last().copied().unwrap_or(0) as usize;
        grid.items = vec![0; filled];
        let mut next: Vec<u32> = grid.start.clone();
        for (t, (a, b)) in ranges.iter().enumerate() {
            for z in a[2]..=b[2] {
                for y in a[1]..=b[1] {
                    for x in a[0]..=b[0] {
                        let c = grid.flat([x, y, z]);
                        if let Some(slot) = next.get_mut(c) {
                            if let Some(item) = grid.items.get_mut(*slot as usize) {
                                *item = t as u32;
                            }
                            *slot += 1;
                        }
                    }
                }
            }
        }
        grid
    }

    fn flat(&self, c: [usize; 3]) -> usize {
        (c[2] * self.n[1] + c[1]) * self.n[0] + c[0]
    }

    fn axis_index(&self, k: usize, x: f64) -> usize {
        let t = ((x - at(&self.lo, k)) / at(&self.cell, k)).floor();
        if t <= 0.0 {
            0
        } else {
            (t as usize).min(at(&self.n, k) - 1)
        }
    }

    fn index_range(&self, lo: [f64; 3], hi: [f64; 3]) -> ([usize; 3], [usize; 3]) {
        let mut a = [0usize; 3];
        let mut b = [0usize; 3];
        for k in 0..3 {
            put(&mut a, k, self.axis_index(k, at(&lo, k)));
            put(&mut b, k, self.axis_index(k, at(&hi, k)));
        }
        (a, b)
    }

    /// The triangles whose bounding boxes touch the box, each once, ascending.
    fn near(&self, lo: [f64; 3], hi: [f64; 3], out: &mut Vec<u32>) {
        out.clear();
        for k in 0..3 {
            if at(&hi, k) < at(&self.lo, k) || at(&lo, k) > at(&self.hi, k) {
                return;
            }
        }
        let (a, b) = self.index_range(lo, hi);
        for z in a[2]..=b[2] {
            for y in a[1]..=b[1] {
                for x in a[0]..=b[0] {
                    let c = self.flat([x, y, z]);
                    if let (Some(&s), Some(&e)) = (self.start.get(c), self.start.get(c + 1))
                        && let Some(items) = self.items.get(s as usize..e as usize)
                    {
                        out.extend_from_slice(items);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Whether the closed box touches the surface.
    pub(super) fn touches(&self, lo: [f64; 3], hi: [f64; 3], scratch: &mut Vec<u32>) -> bool {
        self.near(lo, hi, scratch);
        let centre = [
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        ];
        let half = [
            0.5 * (hi[0] - lo[0]),
            0.5 * (hi[1] - lo[1]),
            0.5 * (hi[2] - lo[2]),
        ];
        let eps = 1e-9 * (half[0] + half[1] + half[2]);
        scratch.iter().any(|&t| {
            self.tris
                .get(t as usize)
                .is_some_and(|t| tri_box_overlap(centre, half, t, eps))
        })
    }

    /// Whether the point is inside the surface (which is taken to be closed).
    pub(super) fn inside(&self, p: [f64; 3], scratch: &mut Vec<u32>) -> bool {
        if p[1] < self.lo[1] || p[1] > self.hi[1] || p[2] < self.lo[2] || p[2] > self.hi[2] {
            return false;
        }
        if p[0] > self.hi[0] {
            return false;
        }
        let from = [p[0].max(self.lo[0]), p[1], p[2]];
        let to = [self.hi[0], p[1], p[2]];
        self.near(from, to, scratch);
        let mut crossings = 0usize;
        for &t in scratch.iter() {
            let Some(t) = self.tris.get(t as usize) else {
                continue;
            };
            if crosses_ray(t, p) {
                crossings += 1;
            }
        }
        crossings % 2 == 1
    }
}

/// The sign of the 2D orientation of `p` against the edge `a -> b` in the (y, z) plane, the
/// same number with the opposite sign for the reversed edge, with a tie broken by the order of
/// the end points so a point on a shared edge falls in exactly one of the two triangles.
fn side(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> bool {
    let forward = (a[1], a[2]) < (b[1], b[2]);
    let (s, e) = if forward { (a, b) } else { (b, a) };
    let mut v = (e[1] - s[1]) * (p[2] - s[2]) - (e[2] - s[2]) * (p[1] - s[1]);
    if !forward {
        v = -v;
    }
    if v == 0.0 { forward } else { v > 0.0 }
}

/// Whether the ray from `p` along +x crosses the triangle.
fn crosses_ray(t: &Tri, p: [f64; 3]) -> bool {
    let s0 = side(t.a, t.b, p);
    let s1 = side(t.b, t.c, p);
    let s2 = side(t.c, t.a, p);
    if s0 != s1 || s1 != s2 {
        return false;
    }
    let n = cross(sub(t.b, t.a), sub(t.c, t.a));
    if n[0] == 0.0 {
        return false;
    }
    let x = t.a[0] - (n[1] * (p[1] - t.a[1]) + n[2] * (p[2] - t.a[2])) / n[0];
    x > p[0]
}

/// Whether the triangle and the box with this centre and half sizes share a point
/// (Akenine-Möller's separating-axis test, with the touching case counted as overlap).
fn tri_box_overlap(c: [f64; 3], h: [f64; 3], t: &Tri, eps: f64) -> bool {
    let v = [sub(t.a, c), sub(t.b, c), sub(t.c, c)];
    let edges = [sub(v[1], v[0]), sub(v[2], v[1]), sub(v[0], v[2])];
    for k in 0..3 {
        let (c0, c1, c2) = (at(&v[0], k), at(&v[1], k), at(&v[2], k));
        let (lo, hi) = (c0.min(c1).min(c2), c0.max(c1).max(c2));
        if lo > at(&h, k) + eps || hi < -at(&h, k) - eps {
            return false;
        }
    }
    let n = cross(edges[0], edges[1]);
    let r = h[0] * n[0].abs() + h[1] * n[1].abs() + h[2] * n[2].abs();
    if dot(n, v[0]).abs() > r + eps {
        return false;
    }
    for e in edges {
        for k in 0..3 {
            let mut basis = [0.0; 3];
            put(&mut basis, k, 1.0);
            let axis = cross(basis, e);
            let p0 = dot(axis, v[0]);
            let p1 = dot(axis, v[1]);
            let p2 = dot(axis, v[2]);
            let r = h[0] * axis[0].abs() + h[1] * axis[1].abs() + h[2] * axis[2].abs();
            if p0.min(p1).min(p2) > r + eps || p0.max(p1).max(p2) < -r - eps {
                return false;
            }
        }
    }
    true
}
