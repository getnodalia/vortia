//! Vortia's own unstructured 2D mesher (R3 of `docs/architecture/rust-only.md`): a region
//! bounded by named lines and arcs becomes triangles that respect its boundary, and the triangles
//! become prisms one cell thick, ready for the solvers.
//!
//! The region is a list of closed loops: the first is the outer boundary and the others are
//! holes. Each loop is a chain of [`Segment`]s, each a polyline with the name of the patch its
//! faces will belong to and, if it asks for one, a cell size of its own. Triangle sizes follow
//! the boundary's sizes into the region and grow with the distance from it ([`Options::grading`]),
//! and no angle is smaller than [`Options::min_angle`] (a guarantee Ruppert's refinement gives
//! when the region's own corners are not sharper than about 30°).

mod delaunay;
mod extrude;

use delaunay::{Dt, P, Sizer};
use vortia_foamfile::RawPolyMesh;

use crate::error::{MeshError, Result};

pub use extrude::Extrusion;

/// The most boundary points a region may have after its segments are cut into cells.
pub const MAX_BOUNDARY_POINTS: usize = 20_000;

/// One piece of a loop: a polyline (an arc is a polyline of chords) and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// From the start to the end, both included. The end is the next segment's start.
    pub points: Vec<[f64; 2]>,
    /// The patch the faces on this segment belong to.
    pub patch: String,
    /// What the patch is: `patch`, `wall`, `symmetryPlane`, …
    pub kind: String,
    /// The size of the cells along this segment, if not the region's.
    pub size: Option<f64>,
}

/// A region of the plane: closed loops of segments, the first the outer boundary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    pub loops: Vec<Vec<Segment>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The cell size along a segment that gives none, in the region's units.
    pub size: f64,
    /// The largest cell anywhere, far from the boundary.
    pub max_size: f64,
    /// How fast cells may grow with the distance from the boundary: 0.3 is 30 %.
    pub grading: f64,
    /// The smallest angle of a triangle the refinement aims at, in degrees (at most 28).
    pub min_angle: f64,
    /// The most cells the mesh may have.
    pub max_cells: usize,
    pub extrusion: Extrusion,
}

impl Options {
    pub fn new(size: f64, extrusion: Extrusion) -> Self {
        Self {
            size,
            max_size: size * 3.0,
            grading: 0.3,
            min_angle: 20.0,
            max_cells: 2_000_000,
            extrusion,
        }
    }
}

fn invalid(message: impl Into<String>) -> MeshError {
    MeshError::Invalid(message.into())
}

fn number(name: &str, v: f64, low: f64, high: f64) -> Result<()> {
    if v.is_finite() && v > low && v <= high {
        Ok(())
    } else {
        Err(invalid(format!(
            "{name} must be a number above {low} and at most {high}, not {v}"
        )))
    }
}

/// The boundary cut into cells: the points, which patch each edge to the next point belongs to,
/// and each point's size.
struct Boundary {
    points: Vec<P>,
    /// `(from, to, patch)`, indices into `points`.
    edges: Vec<(usize, usize, usize)>,
    sizes: Vec<f64>,
    patches: Vec<String>,
    kinds: Vec<String>,
}

fn cut(region: &Region, options: &Options) -> Result<Boundary> {
    let mut patches: Vec<String> = Vec::new();
    let mut kinds: Vec<String> = Vec::new();
    let mut points: Vec<P> = Vec::new();
    let mut sizes: Vec<f64> = Vec::new();
    let mut edges = Vec::new();
    if region.loops.is_empty() {
        return Err(invalid("the region has no outer boundary"));
    }
    for segments in &region.loops {
        if segments.is_empty() {
            return Err(invalid("a loop of the region has no segments"));
        }
        let first = points.len();
        let mut loop_sizes = Vec::new();
        let mut patch_of = Vec::new();
        for (i, seg) in segments.iter().enumerate() {
            if seg.points.len() < 2 {
                return Err(invalid("a segment needs at least two points"));
            }
            if seg
                .points
                .iter()
                .any(|p| !(p[0].is_finite() && p[1].is_finite()))
            {
                return Err(invalid("a segment has a point that is not a number"));
            }
            let h = seg.size.unwrap_or(options.size);
            number("a segment's cell size", h, 0.0, f64::MAX)?;
            // Each segment starts where the one before it ended.
            let before = segments
                .get(if i == 0 { segments.len() - 1 } else { i - 1 })
                .and_then(|s| s.points.last());
            if let (Some(a), Some(b)) = (before, seg.points.first()) {
                let gap = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
                if gap > 1e-9 * options.size.max(1e-300) + 1e-9 * h {
                    return Err(invalid(format!(
                        "the segments of a loop must meet end to end: '{}' starts {gap} away from where the one before ends",
                        seg.patch
                    )));
                }
            }
            let patch = match patches.iter().position(|p| *p == seg.patch) {
                Some(p) => p,
                None => {
                    patches.push(seg.patch.clone());
                    kinds.push(seg.kind.clone());
                    patches.len() - 1
                }
            };
            for pair in seg.points.windows(2) {
                let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
                    continue;
                };
                let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let n = (length / h).ceil().max(1.0);
                if n > MAX_BOUNDARY_POINTS as f64 {
                    return Err(invalid("the cell size is too small for the boundary"));
                }
                for k in 0..n as usize {
                    let f = k as f64 / n;
                    points.push([a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])]);
                    loop_sizes.push(h);
                    patch_of.push(patch);
                    if points.len() > MAX_BOUNDARY_POINTS {
                        return Err(invalid("the cell size is too small for the boundary"));
                    }
                }
            }
        }
        let count = points.len() - first;
        if count < 3 {
            return Err(invalid("a loop of the region needs at least three points"));
        }
        for k in 0..count {
            let next = (k + 1) % count;
            edges.push((
                first + k,
                first + next,
                patch_of.get(k).copied().unwrap_or(0),
            ));
            // A corner is as fine as the finer of the two segments that meet there.
            let before = loop_sizes
                .get(if k == 0 { count - 1 } else { k - 1 })
                .copied()
                .unwrap_or(f64::MAX);
            sizes.push(loop_sizes.get(k).copied().unwrap_or(f64::MAX).min(before));
        }
    }
    Ok(Boundary {
        points,
        edges,
        sizes,
        patches,
        kinds,
    })
}

/// Whether two boundary edges that share no end cross each other.
fn crossing(a: P, b: P, c: P, d: P) -> bool {
    let o = |p: P, q: P, r: P| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn check_simple(b: &Boundary) -> Result<()> {
    let n = b.edges.len();
    for i in 0..n {
        let Some(&(a0, a1, _)) = b.edges.get(i) else {
            continue;
        };
        let (pa, pb) = (
            b.points.get(a0).copied().unwrap_or_default(),
            b.points.get(a1).copied().unwrap_or_default(),
        );
        let (lo_x, hi_x) = (pa[0].min(pb[0]), pa[0].max(pb[0]));
        let (lo_y, hi_y) = (pa[1].min(pb[1]), pa[1].max(pb[1]));
        for j in (i + 1)..n {
            let Some(&(c0, c1, _)) = b.edges.get(j) else {
                continue;
            };
            if [c0, c1].contains(&a0) || [c0, c1].contains(&a1) {
                continue;
            }
            let (pc, pd) = (
                b.points.get(c0).copied().unwrap_or_default(),
                b.points.get(c1).copied().unwrap_or_default(),
            );
            if pc[0].max(pd[0]) < lo_x
                || pc[0].min(pd[0]) > hi_x
                || pc[1].max(pd[1]) < lo_y
                || pc[1].min(pd[1]) > hi_y
            {
                continue;
            }
            if crossing(pa, pb, pc, pd) {
                let name = |k: usize| {
                    b.edges
                        .get(k)
                        .and_then(|e| b.patches.get(e.2))
                        .cloned()
                        .unwrap_or_default()
                };
                return Err(invalid(format!(
                    "the boundary crosses itself, between '{}' and '{}'",
                    name(i),
                    name(j)
                )));
            }
        }
    }
    Ok(())
}

/// Meshes the region: a prism for each triangle, in the order the solver needs, with a patch for
/// each name the segments gave and the patches the extrusion adds.
pub fn mesh(region: &Region, options: &Options) -> Result<RawPolyMesh> {
    number("the cell size", options.size, 0.0, f64::MAX)?;
    number("the largest cell", options.max_size, 0.0, f64::MAX)?;
    number("the grading", options.grading, 0.0, 10.0)?;
    number("the smallest angle", options.min_angle, 0.0, 28.0)?;
    options.extrusion.validate()?;
    if options.max_cells == 0 || options.max_cells > 20_000_000 {
        return Err(invalid("the most cells is between 1 and 20 million"));
    }
    let boundary = cut(region, options)?;
    check_simple(&boundary)?;

    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &boundary.points {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let scale = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !(scale.is_finite() && scale > 0.0) {
        return Err(invalid("the region has no size"));
    }
    let to_unit = |p: P| [(p[0] - lo[0]) / scale + 0.1, (p[1] - lo[1]) / scale + 0.1];
    let unit: Vec<P> = boundary.points.iter().map(|&p| to_unit(p)).collect();
    let sized: Vec<(P, f64)> = unit
        .iter()
        .zip(&boundary.sizes)
        .map(|(&p, &s)| (p, s / scale))
        .collect();
    let sizer = Sizer::new(&sized, options.max_size / scale, options.grading);

    let smallest = boundary.sizes.iter().copied().fold(f64::MAX, f64::min) / scale;
    let mut dt = Dt::new(smallest.max(options.size / scale / 4.0));
    let limit = options.max_cells / 2 + boundary.points.len();
    dt.build(&unit, &boundary.edges, limit)?;
    let ratio = 1.0 / (2.0 * options.min_angle.to_radians().sin());
    dt.refine(&sizer, ratio, limit)?;

    extrude::prisms(
        &dt,
        &boundary.patches,
        &boundary.kinds,
        &options.extrusion,
        |p| [lo[0] + (p[0] - 0.1) * scale, lo[1] + (p[1] - 0.1) * scale],
        1e-9 * scale,
        options.max_cells,
    )
}

#[cfg(test)]
mod tests;
