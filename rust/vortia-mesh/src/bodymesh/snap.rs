//! Moving the wall of the stepped mesh onto the surface of the body.
//!
//! The octree gives a wall that follows cube faces. Here each point of the wall is moved to
//! the nearest point of the surface, and the move is spread into the cells around it so they
//! follow without folding. Where a cell would be squashed, turned inside out or skewed too
//! far, the moves of its points are reduced (halved, again and again) until it is acceptable.
//!
//! The method is the usual one for snapping a Cartesian mesh to a surface (displace the wall
//! points, smooth the displacement inwards, back off where quality fails); it is written here
//! from that description.

use vortia_core::Vec3;
use vortia_foamfile::RawPolyMesh;

use super::grid::TriGrid;

/// What snapping did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapStats {
    /// The points on the wall.
    pub wall_points: usize,
    /// The wall points that ended on the surface.
    pub snapped: usize,
    /// The back-off rounds that were needed.
    pub rounds: usize,
    /// The cells still below the quality limits at the end (they were so before snapping).
    pub poor_cells: usize,
}

type P = [f64; 3];

fn get<T: Copy + Default>(v: &[T], i: usize) -> T {
    v.get(i).copied().unwrap_or_default()
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: P) -> f64 {
    dot(a, a).sqrt()
}

/// The centre and the area vector (along the right-hand normal of the ring) of a face.
fn face_geometry(ring: &[usize], pts: &[P]) -> (P, P) {
    let n = ring.len();
    if n == 0 {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut mean = [0.0; 3];
    for &i in ring {
        mean = add(mean, get(pts, i));
    }
    mean = scale(mean, 1.0 / n as f64);
    let mut area = [0.0; 3];
    let mut centre = [0.0; 3];
    let mut weight = 0.0;
    for k in 0..n {
        let a = get(pts, get(ring, k));
        let b = get(pts, get(ring, (k + 1) % n));
        let s = scale(cross(sub(a, mean), sub(b, mean)), 0.5);
        let w = norm(s);
        area = add(area, s);
        centre = add(centre, scale(add(add(a, b), mean), w / 3.0));
        weight += w;
    }
    if weight > 0.0 {
        centre = scale(centre, 1.0 / weight);
    } else {
        centre = mean;
    }
    (centre, area)
}

/// The mesh as the quality check sees it.
struct Topology {
    rings: Vec<Vec<usize>>,
    owner: Vec<usize>,
    neighbour: Vec<usize>,
    n_cells: usize,
    /// The faces of each cell.
    cell_faces: Vec<Vec<usize>>,
}

struct Shape {
    volume: Vec<f64>,
    /// The area of each face.
    area: Vec<f64>,
    /// The skewness of each face.
    skew: Vec<f64>,
}

/// How far the face centre lies from where the line between the cell centres crosses the
/// face, against the cell spacing and the face size (the measure the mesh check reports).
fn skewness(ring: &[usize], pts: &[P], fc: P, sf: P, c_own: P, c_nei: Option<P>) -> f64 {
    let cpf = sub(fc, c_own);
    let (d, scale_d) = match c_nei {
        Some(n) => (sub(n, c_own), 0.2),
        None => {
            let u = scale(sf, 1.0 / (norm(sf) + 1e-300));
            (scale(u, dot(u, cpf)), 0.4)
        }
    };
    let sv = sub(cpf, scale(d, dot(sf, cpf) / (dot(sf, d) + 1e-150)));
    let hat = scale(sv, 1.0 / (norm(sv) + 1e-300));
    let mut fd = scale_d * norm(d) + 1e-150;
    for &p in ring {
        fd = fd.max(dot(hat, sub(get(pts, p), fc)).abs());
    }
    norm(sv) / fd
}

impl Topology {
    fn new(mesh: &RawPolyMesh) -> Self {
        let faces = &mesh.faces;
        let rings: Vec<Vec<usize>> = (0..mesh.owner.len())
            .map(|f| {
                let a = get(&faces.offsets, f);
                let b = get(&faces.offsets, f + 1);
                faces
                    .points
                    .get(a..b)
                    .map(<[usize]>::to_vec)
                    .unwrap_or_default()
            })
            .collect();
        let n_cells = mesh.owner.iter().copied().max().map_or(0, |m| m + 1);
        let mut cell_faces = vec![Vec::new(); n_cells];
        for (f, &c) in mesh.owner.iter().enumerate() {
            if let Some(list) = cell_faces.get_mut(c) {
                list.push(f);
            }
        }
        for (f, &c) in mesh.neighbour.iter().enumerate() {
            if let Some(list) = cell_faces.get_mut(c) {
                list.push(f);
            }
        }
        Self {
            rings,
            owner: mesh.owner.clone(),
            neighbour: mesh.neighbour.clone(),
            n_cells,
            cell_faces,
        }
    }

    /// The volume of each cell and the area of each face, and the cells that fail `limits`
    /// (a mask).
    fn measure(&self, pts: &[P], limits: Option<(&Shape, &Limits)>) -> (Shape, Vec<bool>) {
        let n_faces = self.owner.len();
        let mut fc = Vec::with_capacity(n_faces);
        let mut sf = Vec::with_capacity(n_faces);
        let mut area = Vec::with_capacity(n_faces);
        for ring in &self.rings {
            let (c, s) = face_geometry(ring, pts);
            fc.push(c);
            area.push(norm(s));
            sf.push(s);
        }
        // A first centre: the mean of the face centres. Volumes and centres follow from the
        // pyramids on that point.
        let mut est = vec![[0.0; 3]; self.n_cells];
        for (c, faces) in self.cell_faces.iter().enumerate() {
            let mut m = [0.0; 3];
            for &f in faces {
                m = add(m, get(&fc, f));
            }
            if let Some(e) = est.get_mut(c) {
                *e = scale(m, 1.0 / faces.len().max(1) as f64);
            }
        }
        let mut volume = vec![0.0; self.n_cells];
        let mut centre = vec![[0.0; 3]; self.n_cells];
        for (c, faces) in self.cell_faces.iter().enumerate() {
            let e = get(&est, c);
            let (mut v, mut m) = (0.0, [0.0; 3]);
            for &f in faces {
                let s = if get(&self.owner, f) == c {
                    get(&sf, f)
                } else {
                    scale(get(&sf, f), -1.0)
                };
                let pyramid = dot(s, sub(get(&fc, f), e)) / 3.0;
                v += pyramid;
                // The pyramid's centroid is a quarter of the way from the face centre to the
                // apex.
                m = add(m, scale(add(scale(get(&fc, f), 3.0), e), pyramid / 4.0));
            }
            if let Some(x) = volume.get_mut(c) {
                *x = v;
            }
            if let Some(x) = centre.get_mut(c) {
                *x = if v.abs() > 1e-300 {
                    scale(m, 1.0 / v)
                } else {
                    e
                };
            }
        }
        let mut skew = Vec::with_capacity(n_faces);
        for (f, ring) in self.rings.iter().enumerate() {
            let c_own = get(&centre, get(&self.owner, f));
            let c_nei = self.neighbour.get(f).map(|&n| get(&centre, n));
            skew.push(skewness(ring, pts, get(&fc, f), get(&sf, f), c_own, c_nei));
        }
        let mut bad = vec![false; self.n_cells];
        if let Some((original, lim)) = limits {
            for (c, faces) in self.cell_faces.iter().enumerate() {
                let v = get(&volume, c);
                let mut poor = v.is_nan() || v <= lim.volume * get(&original.volume, c);
                let ce = get(&centre, c);
                for &f in faces {
                    if poor {
                        break;
                    }
                    let s = if get(&self.owner, f) == c {
                        get(&sf, f)
                    } else {
                        scale(get(&sf, f), -1.0)
                    };
                    if dot(s, sub(get(&fc, f), ce)) <= 0.0
                        || get(&area, f) < lim.area * get(&original.area, f)
                    {
                        poor = true;
                    }
                }
                if let Some(x) = bad.get_mut(c) {
                    *x = poor;
                }
            }
            for (f, &s) in skew.iter().enumerate() {
                if s.is_nan() || s > lim.skew.max(get(&original.skew, f)) {
                    for c in [get(&self.owner, f)]
                        .into_iter()
                        .chain(self.neighbour.get(f).copied())
                    {
                        if let Some(x) = bad.get_mut(c) {
                            *x = true;
                        }
                    }
                }
            }
            for (f, &n) in self.neighbour.iter().enumerate() {
                let o = get(&self.owner, f);
                let d = sub(get(&centre, n), get(&centre, o));
                let s = get(&sf, f);
                let len = norm(d) * norm(s);
                if len > 0.0 && dot(d, s) / len < lim.cos_non_orthogonal {
                    for c in [o, n] {
                        if let Some(x) = bad.get_mut(c) {
                            *x = true;
                        }
                    }
                }
            }
        }
        (Shape { volume, area, skew }, bad)
    }
}

/// How much a cell may be spoiled before the move that does it is reduced.
struct Limits {
    /// The smallest share of its volume a cell keeps.
    volume: f64,
    /// The smallest share of its area a face keeps.
    area: f64,
    /// The largest skewness a face may have (or the one it had before, if that was more).
    skew: f64,
    /// The cosine of the largest angle between a face and the line between the cell centres.
    cos_non_orthogonal: f64,
}

const LIMITS: Limits = Limits {
    volume: 0.1,
    area: 0.02,
    skew: 3.5,
    cos_non_orthogonal: 0.4226, // 65 degrees
};

/// Smoothing sweeps and the factor each one multiplies the average by.
const SWEEPS: usize = 40;
const DECAY: f64 = 0.9;
/// The back-off rounds.
const ROUNDS: usize = 24;
/// The share of a move kept in each back-off round.
const BACK_OFF: f64 = 0.75;
/// How many times the wall is re-snapped and the cells around it re-spread.
const PASSES: usize = 6;

/// Moves the points of the first patch (the body) onto `grid`'s surface and the points around
/// them with it. `reach` is how far a point may travel to the surface.
pub(super) fn snap(mesh: &mut RawPolyMesh, grid: &TriGrid, reach: f64) -> SnapStats {
    let mut stats = SnapStats::default();
    if mesh.patches.is_empty() {
        return stats;
    }
    let n_points = mesh.points.len();
    let top = Topology::new(mesh);
    let pts: Vec<P> = mesh.points.iter().map(|p| [p.x, p.y, p.z]).collect();

    // Wall points, and the points that must stay: those on the other patches.
    let mut on_wall = vec![false; n_points];
    let mut pinned = vec![false; n_points];
    for (pi, patch) in mesh.patches.iter().enumerate() {
        for f in patch.start_face..patch.start_face + patch.n_faces {
            for &p in top.rings.get(f).map(Vec::as_slice).unwrap_or_default() {
                let flag = if pi == 0 { &mut on_wall } else { &mut pinned };
                if let Some(x) = flag.get_mut(p) {
                    *x = true;
                }
            }
        }
    }

    // The points next to each point, from the sides of the faces.
    let mut links: Vec<Vec<usize>> = vec![Vec::new(); n_points];
    for ring in &top.rings {
        let n = ring.len();
        for k in 0..n {
            let (a, b) = (get(ring, k), get(ring, (k + 1) % n));
            if let Some(l) = links.get_mut(a) {
                l.push(b);
            }
            if let Some(l) = links.get_mut(b) {
                l.push(a);
            }
        }
    }
    let fixed: Vec<bool> = (0..n_points)
        .map(|p| get(&on_wall, p) || get(&pinned, p))
        .collect();
    let free_wall: Vec<usize> = (0..n_points)
        .filter(|&p| get(&on_wall, p) && !get(&pinned, p))
        .collect();
    stats.wall_points = free_wall.len();

    let (original, _) = top.measure(&pts, None);
    let mut pts = pts;
    let mut scratch = Vec::new();
    for _ in 0..PASSES {
        // Where each wall point wants to go from where it is now.
        let mut shift = vec![[0.0; 3]; n_points];
        for &p in &free_wall {
            let x = get(&pts, p);
            if let Some(q) = grid.nearest(x, reach, &mut scratch)
                && let Some(s) = shift.get_mut(p)
            {
                *s = sub(q, x);
            }
        }
        // Spread the moves inwards: each free point takes a damped average of its neighbours.
        for _ in 0..SWEEPS {
            let mut next = shift.clone();
            for p in 0..n_points {
                if get(&fixed, p) {
                    continue;
                }
                let near = links.get(p).map(Vec::as_slice).unwrap_or_default();
                if near.is_empty() {
                    continue;
                }
                let mut m = [0.0; 3];
                for &q in near {
                    m = add(m, get(&shift, q));
                }
                if let Some(x) = next.get_mut(p) {
                    *x = scale(m, DECAY / near.len() as f64);
                }
            }
            shift = next;
        }

        // Move as far as the cells allow: points of a cell that fails move less, until none do.
        let mut share = vec![1.0f64; n_points];
        let place = |share: &[f64]| -> Vec<P> {
            (0..n_points)
                .map(|p| add(get(&pts, p), scale(get(&shift, p), get(share, p))))
                .collect()
        };
        let mut moved = place(&share);
        for round in 0..=ROUNDS {
            let (_, bad) = top.measure(&moved, Some((&original, &LIMITS)));
            if !bad.iter().any(|&b| b) {
                break;
            }
            stats.rounds += 1;
            let last = round == ROUNDS;
            for (c, _) in bad.iter().enumerate().filter(|&(_, &b)| b) {
                for &f in top.cell_faces.get(c).map(Vec::as_slice).unwrap_or_default() {
                    for &p in top.rings.get(f).map(Vec::as_slice).unwrap_or_default() {
                        if let Some(s) = share.get_mut(p) {
                            *s = if last { 0.0 } else { *s * BACK_OFF };
                        }
                    }
                }
            }
            moved = place(&share);
        }
        pts = moved;
    }
    // Cells that are still poor were so before the move (their points stayed put).
    let (_, mask) = top.measure(&pts, Some((&original, &LIMITS)));
    stats.poor_cells = mask.iter().filter(|&&b| b).count();

    stats.snapped = free_wall
        .iter()
        .filter(|&&p| {
            let x = get(&pts, p);
            grid.nearest(x, reach, &mut scratch)
                .is_some_and(|q| norm(sub(q, x)) < 0.004 * reach)
        })
        .count();
    for (p, x) in mesh.points.iter_mut().zip(&pts) {
        *p = Vec3::new(x[0], x[1], x[2]);
    }
    stats
}
