//! Prism layers over the wall: thin cells stacked on the body, each as wide as a wall face, so the
//! first cell is small enough for a wall function (or the boundary layer itself) to work on.
//!
//! The wall points are moved out along their normals to the far side of the stack, so the cells
//! that touched the wall now touch the top of the stack, and the room between is filled with
//! `count` cells per wall face. The stack is never thicker than a fraction of the cell it grows
//! out of, and when the result has an inverted cell the stack is made thinner and tried again.
//! The method (extrude the wall layer by layer, with a limit from the neighbouring cells) is the
//! usual one for layer addition; it is written here from that description.

use std::collections::HashMap;

use vortia_core::Vec3;
use vortia_foamfile::{FaceList, RawPatch, RawPolyMesh};

use crate::check::check;
use crate::error::{MeshError, Result};
use crate::geometry::MeshGeometry;
use crate::polymesh::PolyMesh;

/// The most layers asked for.
pub const MAX_LAYERS: usize = 20;

type P = [f64; 3];
/// The wall faces on each side of an edge: the face, and the edge's ends as that face runs them.
type EdgeFaces = HashMap<(usize, usize), Vec<(usize, usize, usize)>>;

/// The layers to grow on the wall.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerSpec {
    /// How many cells deep.
    pub count: usize,
    /// The thickness of the layer on the wall, in metres.
    pub first: f64,
    /// How much thicker each layer is than the one under it (1 for all alike).
    pub ratio: f64,
    /// The stack is at most this fraction of the size of the wall face it grows out of.
    pub reach: f64,
}

impl LayerSpec {
    /// The thickness of the whole stack, when it is not limited.
    pub fn thickness(&self) -> f64 {
        (0..self.count)
            .map(|k| self.first * self.ratio.powi(k as i32))
            .sum()
    }
}

/// What the layers did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerStats {
    /// The cells added.
    pub cells: usize,
    /// The thickness asked for, and the mean over the wall points that came out.
    pub wanted: f64,
    pub mean: f64,
    /// The wall points whose stack had to be thinner than asked.
    pub thinned: usize,
    /// How many times the stack was made thinner because a cell had turned inside out.
    pub rounds: usize,
}

fn bad<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
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
fn p3(v: Vec3) -> P {
    [v.x, v.y, v.z]
}

fn check_spec(spec: &LayerSpec) -> Result<()> {
    if spec.count == 0 || spec.count > MAX_LAYERS {
        return bad(format!("between 1 and {MAX_LAYERS} layers can be grown"));
    }
    if !(spec.first.is_finite() && spec.first > 0.0) {
        return bad("the first layer needs a thickness above zero");
    }
    if !(spec.ratio.is_finite() && (1.0..=3.0).contains(&spec.ratio)) {
        return bad("each layer is 1 to 3 times as thick as the one under it");
    }
    if !(spec.reach.is_finite() && spec.reach > 0.0 && spec.reach <= 0.9) {
        return bad("the stack may be up to 0.9 of the cell it grows out of, above zero");
    }
    Ok(())
}

/// The area vector of a ring, along its right-hand normal (the sum of the triangles round the
/// average point, correct for a face that is not flat).
fn area_vector(ring: &[usize], pts: &[P]) -> P {
    let at = |i: usize| pts.get(i).copied().unwrap_or_default();
    let mut mean = [0.0; 3];
    for &i in ring {
        mean = add(mean, at(i));
    }
    let mean = scale(mean, 1.0 / ring.len().max(1) as f64);
    let mut sum = [0.0; 3];
    for (k, &i) in ring.iter().enumerate() {
        let next = ring.get(k + 1).or(ring.first()).copied().unwrap_or(i);
        sum = add(sum, cross(sub(at(i), mean), sub(at(next), mean)));
    }
    scale(sum, 0.5)
}

/// A face to write: its points, the cell that owns it, the one across it (none on the boundary)
/// and the patch it belongs to (none inside).
struct Made {
    ring: Vec<usize>,
    owner: usize,
    neighbour: Option<usize>,
    patch: Option<usize>,
}

/// Grows the layers described by `spec` on the patch `wall` of `mesh`. The mesh is left as it
/// was when this refuses. A wall that does not close on itself (every edge between exactly two
/// of its faces) cannot be layered.
pub fn add_layers(mesh: &mut RawPolyMesh, wall: &str, spec: &LayerSpec) -> Result<LayerStats> {
    check_spec(spec)?;
    let Some(patch_index) = mesh.patches.iter().position(|p| p.name == wall) else {
        return bad(format!("the mesh has no patch '{wall}' to grow layers on"));
    };
    let (start, count) = match mesh.patches.get(patch_index) {
        Some(p) => (p.start_face, p.n_faces),
        None => return bad("the wall patch is missing"),
    };
    if count == 0 {
        return bad(format!("the patch '{wall}' has no faces"));
    }
    let pts: Vec<P> = mesh.points.iter().map(|&v| p3(v)).collect();
    let wall_faces: Vec<usize> = (start..start + count).collect();

    // The wall points, their normals (towards the fluid) and the size of the faces they are on.
    let mut normal: HashMap<usize, P> = HashMap::new();
    let mut room: HashMap<usize, f64> = HashMap::new();
    for &f in &wall_faces {
        let ring = mesh.faces.face(f);
        let s = area_vector(ring, &pts);
        let size = norm(s).sqrt();
        for &p in ring {
            let n = normal.entry(p).or_insert([0.0; 3]);
            // The wall face points out of the fluid, into the body.
            *n = sub(*n, s);
            let r = room.entry(p).or_insert(f64::INFINITY);
            *r = r.min(size);
        }
    }
    for (p, n) in &mut normal {
        let length = norm(*n);
        if !(length.is_finite() && length > 0.0) {
            return bad(format!("point {p} of the wall has no normal"));
        }
        *n = scale(*n, 1.0 / length);
    }

    // The edges of the wall: each between two of its faces, which is what makes the sides of the
    // stack.
    let mut edges: EdgeFaces = HashMap::new();
    for (index, &f) in wall_faces.iter().enumerate() {
        let ring = mesh.faces.face(f);
        for (k, &a) in ring.iter().enumerate() {
            let b = ring.get(k + 1).or(ring.first()).copied().unwrap_or(a);
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((index, a, b));
        }
    }
    if let Some(((a, b), sides)) = edges.iter().find(|(_, s)| s.len() != 2) {
        return bad(format!(
            "the wall does not close on itself: the edge between points {a} and {b} has {} \
             faces of the wall, not two",
            sides.len()
        ));
    }

    // How much of the stack each point may have, smoothed so that neighbours do not differ much.
    let wanted = spec.thickness();
    let mut fraction: HashMap<usize, f64> = normal
        .keys()
        .map(|&p| {
            let allowed = spec.reach * room.get(&p).copied().unwrap_or(0.0);
            (p, (allowed / wanted).min(1.0))
        })
        .collect();
    let near: Vec<(usize, usize)> = edges.keys().copied().collect();
    for _ in 0..3 {
        let mut next = fraction.clone();
        for &(a, b) in &near {
            let (Some(&fa), Some(&fb)) = (fraction.get(&a), fraction.get(&b)) else {
                continue;
            };
            if let Some(x) = next.get_mut(&a) {
                *x = x.min(0.5 * (fa + fb) + 0.25 * (fa - fb).abs().min(fa));
            }
            if let Some(x) = next.get_mut(&b) {
                *x = x.min(0.5 * (fa + fb) + 0.25 * (fa - fb).abs().min(fb));
            }
        }
        fraction = next;
    }

    // Try the stack, and a thinner one when a cell turns inside out.
    let mut shrink = 1.0;
    for round in 0..6 {
        let (built, stats) = build(
            mesh,
            patch_index,
            &wall_faces,
            &normal,
            &fraction,
            spec,
            shrink,
        )?;
        if inside_out(&built) == 0 {
            let thinned = fraction.values().filter(|&&f| f * shrink < 0.999).count();
            let mean = fraction.values().sum::<f64>() / fraction.len().max(1) as f64 * shrink;
            *mesh = built;
            return Ok(LayerStats {
                thinned,
                mean: mean * wanted,
                wanted,
                rounds: round,
                ..stats
            });
        }
        shrink *= 0.6;
    }
    bad(format!(
        "the layers do not fit: cells turn inside out even with the stack at {:.0} % of the \
         thickness asked for. Ask for thinner layers or refine the body surface",
        shrink * 100.0 / 0.6
    ))
}

/// The cells with no volume or a negative one, or an open cell.
fn inside_out(raw: &RawPolyMesh) -> usize {
    let Ok(mesh) = PolyMesh::from_raw(raw.clone()) else {
        return usize::MAX;
    };
    let geometry = MeshGeometry::compute(&mesh);
    let report = check(&mesh, &geometry);
    report.non_positive_volumes + report.open_cells + report.wrongly_oriented
}

/// The mesh with the stack of thickness `shrink` times the allowed one.
fn build(
    mesh: &RawPolyMesh,
    patch_index: usize,
    wall_faces: &[usize],
    normal: &HashMap<usize, P>,
    fraction: &HashMap<usize, f64>,
    spec: &LayerSpec,
    shrink: f64,
) -> Result<(RawPolyMesh, LayerStats)> {
    let n_old_cells = mesh
        .owner
        .iter()
        .chain(&mesh.neighbour)
        .max()
        .map_or(0, |m| m + 1);
    let layers = spec.count;

    // Where each wall point is at each level: 0 is the wall, `layers` the far side of the stack.
    let mut points: Vec<Vec3> = mesh.points.clone();
    let mut level: HashMap<(usize, usize), usize> = HashMap::new();
    let mut depth = vec![0.0; layers + 1];
    let mut so_far = 0.0;
    for (k, d) in depth.iter_mut().enumerate().skip(1) {
        so_far += spec.first * spec.ratio.powi(k as i32 - 1);
        *d = so_far;
    }
    let total = depth.last().copied().unwrap_or(1.0).max(f64::MIN_POSITIVE);
    let mut keys: Vec<usize> = normal.keys().copied().collect();
    keys.sort_unstable();
    for &p in &keys {
        let (Some(n), Some(f), Some(&origin)) =
            (normal.get(&p), fraction.get(&p), mesh.points.get(p))
        else {
            continue;
        };
        level.insert((p, 0), p);
        for (k, &d) in depth.iter().enumerate().skip(1) {
            let at = d / total * f * shrink * total;
            let moved = Vec3::new(
                origin.x + n[0] * at,
                origin.y + n[1] * at,
                origin.z + n[2] * at,
            );
            level.insert((p, k), points.len());
            points.push(moved);
        }
    }
    let on_level = |p: usize, k: usize| level.get(&(p, k)).copied().unwrap_or(p);

    let mut faces: Vec<Made> = Vec::new();
    let wall_set: std::collections::HashSet<usize> = wall_faces.iter().copied().collect();
    // Every face that is not on the wall now meets the far side of the stack.
    for f in 0..mesh.faces.len() {
        if wall_set.contains(&f) {
            continue;
        }
        let ring: Vec<usize> = mesh
            .faces
            .face(f)
            .iter()
            .map(|&p| on_level(p, layers))
            .collect();
        let patch = mesh
            .patches
            .iter()
            .position(|q| f >= q.start_face && f < q.start_face + q.n_faces);
        faces.push(Made {
            ring,
            owner: mesh.owner.get(f).copied().unwrap_or(0),
            neighbour: mesh.neighbour.get(f).copied(),
            patch,
        });
    }

    // The cell of wall face `index` at layer `k` (1 on the wall).
    let cell = |index: usize, k: usize| n_old_cells + index * layers + (k - 1);
    for (index, &f) in wall_faces.iter().enumerate() {
        let ring = mesh.faces.face(f);
        let owner = mesh.owner.get(f).copied().unwrap_or(0);
        let at_level = |k: usize| -> Vec<usize> { ring.iter().map(|&p| on_level(p, k)).collect() };
        // On the wall, outward from the first cell.
        faces.push(Made {
            ring: at_level(0),
            owner: cell(index, 1),
            neighbour: None,
            patch: Some(patch_index),
        });
        // Between two layers, from the lower one up.
        for k in 1..layers {
            let mut up = at_level(k);
            up.reverse();
            faces.push(Made {
                ring: up,
                owner: cell(index, k),
                neighbour: Some(cell(index, k + 1)),
                patch: None,
            });
        }
        // The top of the stack, against the cell the wall face belonged to.
        faces.push(Made {
            ring: at_level(layers),
            owner,
            neighbour: Some(cell(index, layers)),
            patch: None,
        });
    }
    // The sides: one quad per edge of the wall and layer, between the cells of the two faces.
    let mut sides: EdgeFaces = HashMap::new();
    for (index, &f) in wall_faces.iter().enumerate() {
        let ring = mesh.faces.face(f);
        for (k, &a) in ring.iter().enumerate() {
            let b = ring.get(k + 1).or(ring.first()).copied().unwrap_or(a);
            sides
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((index, a, b));
        }
    }
    let mut ordered: Vec<_> = sides.into_iter().collect();
    ordered.sort_unstable_by_key(|(edge, _)| *edge);
    for (_, pair) in ordered {
        let (Some(&(first, a, b)), Some(&(second, _, _))) = (pair.first(), pair.get(1)) else {
            continue;
        };
        for k in 1..=layers {
            let ring = vec![
                on_level(a, k - 1),
                on_level(b, k - 1),
                on_level(b, k),
                on_level(a, k),
            ];
            let (c1, c2) = (cell(first, k), cell(second, k));
            let (owner, neighbour) = (c1.min(c2), c1.max(c2));
            faces.push(Made {
                ring,
                owner,
                neighbour: Some(neighbour),
                patch: None,
            });
        }
    }

    // Each internal face leaves its owner along its normal: a face made without looking at which
    // way round it is turned if it does not.
    let cells_total = n_old_cells + wall_faces.len() * layers;
    let mut centre = vec![[0.0; 3]; cells_total];
    let mut seen = vec![0usize; cells_total];
    let at = |i: usize| points.get(i).map(|v| p3(*v)).unwrap_or_default();
    for face in &faces {
        for cell in std::iter::once(face.owner).chain(face.neighbour) {
            if let (Some(c), Some(n)) = (centre.get_mut(cell), seen.get_mut(cell)) {
                for &p in &face.ring {
                    *c = add(*c, at(p));
                }
                *n += face.ring.len();
            }
        }
    }
    for (c, &n) in centre.iter_mut().zip(&seen) {
        *c = scale(*c, 1.0 / n.max(1) as f64);
    }
    let pts: Vec<P> = points.iter().map(|&v| p3(v)).collect();
    for face in faces.iter_mut().filter(|f| f.patch.is_none()) {
        let Some(neighbour) = face.neighbour else {
            continue;
        };
        let s = area_vector(&face.ring, &pts);
        let along = sub(
            centre.get(neighbour).copied().unwrap_or_default(),
            centre.get(face.owner).copied().unwrap_or_default(),
        );
        if dot(s, along) < 0.0 {
            face.ring.reverse();
        }
    }

    // Order: internal faces by owner then neighbour, then each patch by owner.
    let n_patches = mesh.patches.len();
    faces.sort_by_key(|f| {
        (
            f.patch.map_or(0, |p| p + 1),
            f.owner,
            f.neighbour.unwrap_or(usize::MAX),
        )
    });
    let mut list = FaceList {
        offsets: vec![0],
        points: Vec::new(),
    };
    let mut owner = Vec::with_capacity(faces.len());
    let mut neighbour = Vec::new();
    let mut counts = vec![0usize; n_patches];
    for face in &faces {
        list.points.extend(&face.ring);
        list.offsets.push(list.points.len());
        owner.push(face.owner);
        match (face.patch, face.neighbour) {
            (None, Some(n)) => neighbour.push(n),
            (Some(p), _) => {
                if let Some(c) = counts.get_mut(p) {
                    *c += 1;
                }
            }
            (None, None) => return bad("an internal face has no cell across it"),
        }
    }
    let mut patches: Vec<RawPatch> = Vec::with_capacity(n_patches);
    let mut next = neighbour.len();
    for (old, &n) in mesh.patches.iter().zip(&counts) {
        patches.push(RawPatch {
            start_face: next,
            n_faces: n,
            ..old.clone()
        });
        next += n;
    }
    let raw = RawPolyMesh {
        points,
        faces: list,
        owner,
        neighbour,
        patches,
        cell_zones: Vec::new(),
        face_zones: Vec::new(),
    };
    let stats = LayerStats {
        cells: wall_faces.len() * layers,
        ..LayerStats::default()
    };
    Ok((raw, stats))
}
