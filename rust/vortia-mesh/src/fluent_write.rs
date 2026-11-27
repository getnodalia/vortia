//! The engine's own `foamMeshToFluent`: a `polyMesh` as a Fluent `.msh` file
//! (ASCII), so a mesh made here opens in a tool that reads Fluent meshes.
//!
//! The file follows the layout [`crate::fluent`] reads: the dimension, the
//! points, the cells of each zone with their element type, the faces of the
//! interior and of every boundary patch with the two cells each one separates
//! (Fluent's first cell is the face's neighbour, its second the owner, and a
//! boundary face lists its one cell first and is given pointing into the
//! domain), and a zone line per group with its type and name.
//!
//! - A mesh with an `empty` patch (a 2-D case, one cell thick) is written as
//!   a true 2-D mesh: points of the front plane, a line for each face and a
//!   triangle or quadrilateral for each cell. It has to be a straight
//!   extrusion in z, and is refused otherwise.
//! - Every other cell is a tetrahedron, hexahedron, pyramid or prism when its
//!   faces say so, and a polyhedron otherwise.
//! - Fluent's boundary types come from the patch kinds (`wall`, `symmetry`);
//!   a plain `patch` is typed from its name or by [`WriteOptions`], and the
//!   result lists every patch whose type was assumed.
//! - Cell zones are kept when they split the cells between them; otherwise
//!   there is one zone, `fluid`.
//!
//! What was checked is that the file reads back, through this engine's reader,
//! as the same mesh (points, volumes, patches, cell shapes). It has not been
//! opened in Fluent itself.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use vortia_core::Vec3;
use vortia_foamfile::RawPolyMesh;

use crate::error::{MeshError, Result};
use crate::polymesh::{PatchKind, PolyMesh};

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// The ways to ask for a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WriteOptions {
    /// Patch name and the Fluent boundary type it is to have (`velocity-inlet`,
    /// `pressure-outlet`, …), for the patches the mesh itself does not type.
    pub boundary_types: Vec<(String, String)>,
}

/// The text of the file and what the writer had to assume.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub text: String,
    pub warnings: Vec<String>,
}

/// The types a boundary can be given, with the number Fluent's face sections
/// carry for each.
const BOUNDARY_TYPES: [(&str, u32); 10] = [
    ("wall", 3),
    ("pressure-inlet", 4),
    ("inlet-vent", 4),
    ("intake-fan", 4),
    ("pressure-outlet", 5),
    ("exhaust-fan", 5),
    ("outlet-vent", 5),
    ("symmetry", 7),
    ("pressure-far-field", 9),
    ("velocity-inlet", 10),
];

fn type_code(name: &str) -> Option<u32> {
    BOUNDARY_TYPES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, c)| c)
}

/// The shape of a cell, by Fluent's element-type number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Triangle = 1,
    Tetrahedron = 2,
    Quadrilateral = 3,
    Hexahedron = 4,
    Pyramid = 5,
    Prism = 6,
    Polyhedron = 7,
}

/// The shape a cell of this many triangle faces, quadrilateral faces, other
/// faces and distinct points has.
fn classify(triangles: usize, quads: usize, others: usize, points: usize) -> Shape {
    match (triangles, quads, others, points) {
        (4, 0, 0, 4) => Shape::Tetrahedron,
        (0, 6, 0, 8) => Shape::Hexahedron,
        (4, 1, 0, 5) => Shape::Pyramid,
        (2, 3, 0, 6) => Shape::Prism,
        _ => Shape::Polyhedron,
    }
}

/// A zone's name as Fluent and this engine's reader keep it: no space, quote
/// or bracket.
fn word(name: &str) -> String {
    let w: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if w.is_empty() { "zone".into() } else { w }
}

/// A group of faces of the output, by their position in it.
struct FaceGroup {
    name: String,
    kind: &'static str,
    code: u32,
    faces: Vec<OutFace>,
}

/// A face as the file lists it: points (counted from zero), then its cells
/// (counted from one, zero for none).
struct OutFace {
    nodes: Vec<usize>,
    c0: usize,
    c1: usize,
}

/// The cells, divided into zones, and where each one goes.
struct Zones {
    /// Name and old cell numbers, in file order.
    list: Vec<(String, Vec<usize>)>,
    /// The new position (from zero) of each old cell.
    new_of: Vec<usize>,
}

/// A zone name nobody has taken yet, and a note when it is not the one asked for.
fn unique(wanted: &str, used: &mut BTreeSet<String>, warnings: &mut Vec<String>) -> String {
    let base = word(wanted);
    let mut name = base.clone();
    let mut n = 2;
    while !used.insert(name.clone()) {
        name = format!("{base}-{n}");
        n += 1;
    }
    if name != wanted {
        warnings.push(format!("the zone `{wanted}` is written as `{name}`"));
    }
    name
}

fn zones_of(
    mesh: &RawPolyMesh,
    n_cells: usize,
    used: &mut BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Zones {
    let mut seen = vec![false; n_cells];
    let mut split = !mesh.cell_zones.is_empty();
    for zone in &mesh.cell_zones {
        for &c in &zone.cells {
            match seen.get_mut(c) {
                Some(s) if !*s => *s = true,
                _ => split = false,
            }
        }
    }
    if split && seen.iter().any(|&s| !s) {
        split = false;
    }
    let list: Vec<(String, Vec<usize>)> = if split {
        mesh.cell_zones
            .iter()
            .map(|z| (unique(&z.name, used, warnings), z.cells.clone()))
            .collect()
    } else {
        if !mesh.cell_zones.is_empty() {
            warnings.push(
                "the cell zones do not split the cells between them, so all are one zone `fluid`"
                    .into(),
            );
        }
        vec![(unique("fluid", used, warnings), (0..n_cells).collect())]
    };
    let mut new_of = vec![0; n_cells];
    let mut next = 0;
    for (_, cells) in &list {
        for &c in cells {
            if let Some(slot) = new_of.get_mut(c) {
                *slot = next;
            }
            next += 1;
        }
    }
    Zones { list, new_of }
}

/// The faces of each cell, by owner and neighbour.
fn faces_of_cells(mesh: &PolyMesh) -> Vec<Vec<usize>> {
    let mut cells = vec![Vec::new(); mesh.n_cells()];
    for (face, &c) in mesh.owner().iter().enumerate() {
        if let Some(list) = cells.get_mut(c) {
            list.push(face);
        }
    }
    for (face, &c) in mesh.neighbour().iter().enumerate() {
        if let Some(list) = cells.get_mut(c) {
            list.push(face);
        }
    }
    cells
}

/// The Fluent boundary type of a patch, and whether it was a guess.
fn boundary_type(
    name: &str,
    kind: &PatchKind,
    options: &WriteOptions,
) -> Result<(&'static str, bool)> {
    let known = |t: &str| -> Result<&'static str> {
        BOUNDARY_TYPES
            .iter()
            .find(|(n, _)| *n == t)
            .map(|&(n, _)| n)
            .ok_or_else(|| {
                MeshError::Invalid(format!("`{t}` is not a boundary type this writer knows"))
            })
    };
    if let Some((_, t)) = options.boundary_types.iter().find(|(n, _)| n == name) {
        return Ok((known(t)?, false));
    }
    match kind {
        PatchKind::Wall => Ok(("wall", false)),
        PatchKind::Symmetry | PatchKind::SymmetryPlane => Ok(("symmetry", false)),
        PatchKind::Patch => {
            let lower = name.to_lowercase();
            Ok(if lower.contains("inlet") || lower.contains("inflow") {
                ("velocity-inlet", true)
            } else {
                ("pressure-outlet", true)
            })
        }
        PatchKind::Cyclic | PatchKind::Processor => invalid(format!(
            "patch `{name}` is {}: periodic and processor patches are not written yet",
            kind.name()
        )),
        PatchKind::Wedge => invalid(format!(
            "patch `{name}` is a wedge: axisymmetric meshes are not written yet"
        )),
        PatchKind::Empty => invalid(format!("patch `{name}` is empty and has no Fluent type")),
        PatchKind::Other(other) => invalid(format!(
            "patch `{name}` has the type `{other}`, which has no Fluent equivalent here"
        )),
    }
}

/// The z component of the area vector of a face.
fn area_z(points: &[Vec3], face: &[usize]) -> f64 {
    let mut sum = 0.0;
    for (i, &a) in face.iter().enumerate() {
        let b = face.get((i + 1) % face.len()).copied().unwrap_or(a);
        if let (Some(p), Some(q)) = (points.get(a), points.get(b)) {
            sum += p.x * q.y - q.x * p.y;
        }
    }
    sum / 2.0
}

/// The mesh as a Fluent `.msh` file.
pub fn foam_to_fluent(raw: &RawPolyMesh, options: &WriteOptions) -> Result<Written> {
    let mesh = PolyMesh::from_raw(raw.clone())?;
    let mut warnings = Vec::new();
    if !raw.face_zones.is_empty() {
        warnings.push("the face zones are not written".into());
    }
    let two_d = mesh
        .patches()
        .iter()
        .any(|p| p.kind == PatchKind::Empty && p.size > 0);
    let mut used: BTreeSet<String> = BTreeSet::new();
    let zones = zones_of(raw, mesh.n_cells(), &mut used, &mut warnings);
    let cell_faces = faces_of_cells(&mesh);

    // The points and the shape of each cell, and for 2-D the faces too.
    let (points, shapes, side): (Vec<Vec3>, Vec<Shape>, Option<Extruded>) = if two_d {
        let e = extruded(&mesh, &cell_faces)?;
        let points = e
            .front
            .iter()
            .filter_map(|&p| mesh.points().get(p).copied());
        let shapes = e.shapes.clone();
        (points.collect(), shapes, Some(e))
    } else {
        let mut shapes = Vec::with_capacity(cell_faces.len());
        for faces in &cell_faces {
            let (mut tri, mut quad, mut other) = (0, 0, 0);
            let mut nodes: Vec<usize> = Vec::new();
            for &f in faces {
                let face = mesh.face(f);
                match face.len() {
                    3 => tri += 1,
                    4 => quad += 1,
                    _ => other += 1,
                }
                nodes.extend_from_slice(face);
            }
            nodes.sort_unstable();
            nodes.dedup();
            shapes.push(classify(tri, quad, other, nodes.len()));
        }
        (mesh.points().to_vec(), shapes, None)
    };

    // The faces: the interior, then each patch that is not a front or back.
    let node =
        |p: usize| -> Result<usize> {
            match &side {
                Some(e) => e.index.get(&p).copied().ok_or_else(|| {
                    MeshError::Invalid(format!("point {p} is not on the front plane"))
                }),
                None => Ok(p),
            }
        };
    let ends = |f: usize| -> Result<Vec<usize>> {
        match &side {
            Some(e) => {
                let (a, b) = e.edge(&mesh, f)?;
                Ok(vec![node(a)?, node(b)?])
            }
            None => mesh.face(f).iter().map(|&p| node(p)).collect(),
        }
    };
    let new_cell = |c: usize| zones.new_of.get(c).copied().unwrap_or(0) + 1;
    let mut groups: Vec<FaceGroup> = Vec::new();
    let mut interior = Vec::new();
    for f in 0..mesh.n_internal_faces() {
        let (Some(&o), Some(&n)) = (mesh.owner().get(f), mesh.neighbour().get(f)) else {
            return invalid(format!("internal face {f} has no cells"));
        };
        interior.push(OutFace {
            nodes: ends(f)?,
            c0: new_cell(n),
            c1: new_cell(o),
        });
    }
    if !interior.is_empty() {
        let name = unique("interior", &mut used, &mut warnings);
        groups.push(FaceGroup {
            name,
            kind: "interior",
            code: 2,
            faces: interior,
        });
    }
    let mut assumed = Vec::new();
    for patch in mesh.patches() {
        if patch.kind == PatchKind::Empty {
            continue;
        }
        let (kind, guessed) = boundary_type(&patch.name, &patch.kind, options)?;
        if guessed {
            assumed.push(format!("`{}` as {kind}", patch.name));
        }
        let code = type_code(kind).unwrap_or(0);
        let mut faces = Vec::with_capacity(patch.size);
        for f in patch.faces() {
            let Some(&o) = mesh.owner().get(f) else {
                return invalid(format!("face {f} has no owner"));
            };
            let mut nodes = ends(f)?;
            nodes.reverse();
            faces.push(OutFace {
                nodes,
                c0: new_cell(o),
                c1: 0,
            });
        }
        let name = unique(&patch.name, &mut used, &mut warnings);
        groups.push(FaceGroup {
            name,
            kind,
            code,
            faces,
        });
    }
    if !assumed.is_empty() {
        warnings.push(format!(
            "Fluent boundary types were assumed from the names: {}; set them in Fluent or with the options",
            assumed.join(", ")
        ));
    }

    Ok(Written {
        text: render(&points, two_d, &zones, &shapes, &groups),
        warnings,
    })
}

/// A mesh one cell thick, seen from its front plane.
struct Extruded {
    /// The points on the front plane, in file order.
    front: Vec<usize>,
    /// Each of them: its position in the file, counted from zero.
    index: std::collections::HashMap<usize, usize>,
    back: BTreeSet<usize>,
    shapes: Vec<Shape>,
}

impl Extruded {
    /// The two front points of a side face, in the order that keeps the face
    /// pointing out of its owner.
    fn edge(&self, mesh: &PolyMesh, face: usize) -> Result<(usize, usize)> {
        let f = mesh.face(face);
        if f.len() != 4 {
            return invalid(format!(
                "face {face} has {} points, not the four of a side of an extrusion",
                f.len()
            ));
        }
        let at = |i: usize| f.get(i % 4).copied().unwrap_or(0);
        for i in 0..4 {
            let (a, b, c, d) = (at(i), at(i + 1), at(i + 2), at(i + 3));
            if self.index.contains_key(&a)
                && self.index.contains_key(&b)
                && self.back.contains(&c)
                && self.back.contains(&d)
            {
                let (pa, pb, pc, pd) = (
                    mesh.points().get(a),
                    mesh.points().get(b),
                    mesh.points().get(c),
                    mesh.points().get(d),
                );
                if let (Some(pa), Some(pb), Some(pc), Some(pd)) = (pa, pb, pc, pd) {
                    let tol = 1e-9 * (1.0 + pa.x.abs() + pa.y.abs() + pb.x.abs() + pb.y.abs());
                    let same =
                        |p: &Vec3, q: &Vec3| (p.x - q.x).abs() <= tol && (p.y - q.y).abs() <= tol;
                    if !(same(pa, pd) && same(pb, pc)) {
                        return invalid(format!(
                            "face {face} is not straight in z: the mesh is not an extrusion"
                        ));
                    }
                }
                return Ok((a, b));
            }
        }
        invalid(format!("face {face} is not a side of an extrusion in z"))
    }
}

/// What a mesh with `empty` patches needs to be written in 2-D: every cell
/// has one face on the front and one on the back, of three or four points,
/// and the other faces are quadrilaterals joining them.
fn extruded(mesh: &PolyMesh, cell_faces: &[Vec<usize>]) -> Result<Extruded> {
    let mut empty = vec![false; mesh.n_faces()];
    for patch in mesh.patches().iter().filter(|p| p.kind == PatchKind::Empty) {
        for f in patch.faces() {
            if let Some(e) = empty.get_mut(f) {
                *e = true;
            }
        }
    }
    let mut front = BTreeSet::new();
    let mut back = BTreeSet::new();
    let mut shapes = Vec::with_capacity(cell_faces.len());
    for (cell, faces) in cell_faces.iter().enumerate() {
        let mut ends: [Option<usize>; 2] = [None, None];
        for &f in faces.iter().filter(|&&f| empty.get(f) == Some(&true)) {
            let z = area_z(mesh.points(), mesh.face(f));
            if !z.is_finite() || z == 0.0 {
                return invalid(format!(
                    "the empty face {f} has no area in the xy plane: the mesh is not extruded in z"
                ));
            }
            let slot = usize::from(z < 0.0);
            if ends.get(slot).copied().flatten().is_some() {
                return invalid(format!(
                    "cell {cell} has two empty faces on the same side: not a mesh one cell thick"
                ));
            }
            if let Some(s) = ends.get_mut(slot) {
                *s = Some(f);
            }
            let set = if slot == 0 { &mut front } else { &mut back };
            set.extend(mesh.face(f).iter().copied());
        }
        let [Some(a), Some(b)] = ends else {
            return invalid(format!(
                "cell {cell} has no empty face on both its front and its back"
            ));
        };
        let n = mesh.face(a).len();
        if n != mesh.face(b).len() || !(n == 3 || n == 4) {
            return invalid(format!(
                "cell {cell} has a front of {n} points and a back of {}: only triangles and quadrilaterals are 2-D cells",
                mesh.face(b).len()
            ));
        }
        shapes.push(if n == 3 {
            Shape::Triangle
        } else {
            Shape::Quadrilateral
        });
    }
    if let Some(p) = front.intersection(&back).next() {
        return invalid(format!("point {p} is on both the front and the back"));
    }
    let front: Vec<usize> = front.into_iter().collect();
    let index = front.iter().enumerate().map(|(i, &p)| (p, i)).collect();
    Ok(Extruded {
        front,
        index,
        back,
        shapes,
    })
}

/// The text of the file.
fn render(
    points: &[Vec3],
    two_d: bool,
    zones: &Zones,
    shapes: &[Shape],
    groups: &[FaceGroup],
) -> String {
    // Writing to a `String` cannot fail.
    macro_rules! put {
        ($out:expr, $($arg:tt)*) => {
            let _ = writeln!($out, $($arg)*);
        };
    }
    let dim = if two_d { 2 } else { 3 };
    let n_cells = shapes.len();
    let n_faces: usize = groups.iter().map(|g| g.faces.len()).sum();
    let mut out = String::new();
    put!(
        out,
        "(0 \"Vortia mesh: {} points, {} faces, {} cells\")",
        points.len(),
        n_faces,
        n_cells
    );
    put!(out, "(2 {dim})");
    put!(out, "(10 (0 1 {:x} 0 {dim}))", points.len());
    put!(out, "(12 (0 1 {n_cells:x} 0 0))");
    put!(out, "(13 (0 1 {n_faces:x} 0 0))");
    put!(out, "(10 (1 1 {:x} 1 {dim})(", points.len());
    for p in points {
        if two_d {
            put!(out, "{:e} {:e}", p.x, p.y);
        } else {
            put!(out, "{:e} {:e} {:e}", p.x, p.y, p.z);
        }
    }
    put!(out, "))");

    // The cell zones, one after another.
    let mut first = 1;
    for (id, (_, cells)) in (1..).zip(&zones.list) {
        let last = first + cells.len() - 1;
        let kinds: Vec<Shape> = cells
            .iter()
            .filter_map(|&c| shapes.get(c).copied())
            .collect();
        let uniform = kinds.windows(2).all(|w| w.first() == w.get(1));
        match kinds.first() {
            Some(&k) if uniform => {
                put!(out, "(12 ({id:x} {first:x} {last:x} 1 {:x}))", k as u32);
            }
            _ => {
                put!(out, "(12 ({id:x} {first:x} {last:x} 1 0)(");
                for line in kinds.chunks(40) {
                    let words: Vec<String> = line.iter().map(|&k| (k as u32).to_string()).collect();
                    put!(out, "{}", words.join(" "));
                }
                put!(out, "))");
            }
        }
        first = last + 1;
    }

    // The faces, in the order of the groups.
    let mut id = zones.list.len();
    let mut start = 1;
    let mut headers = Vec::new();
    for group in groups {
        id += 1;
        let end = start + group.faces.len() - 1;
        let size = group.faces.first().map_or(0, |f| f.nodes.len());
        let uniform = group.faces.iter().all(|f| f.nodes.len() == size);
        let element = if !uniform {
            0
        } else {
            match size {
                2 => 2,
                3 => 3,
                4 => 4,
                _ => 0,
            }
        };
        put!(
            out,
            "(13 ({id:x} {start:x} {end:x} {:x} {element:x})(",
            group.code
        );
        for f in &group.faces {
            let nodes: Vec<String> = f.nodes.iter().map(|&n| format!("{:x}", n + 1)).collect();
            if element == 0 {
                put!(
                    out,
                    "{:x} {} {:x} {:x}",
                    f.nodes.len(),
                    nodes.join(" "),
                    f.c0,
                    f.c1
                );
            } else {
                put!(out, "{} {:x} {:x}", nodes.join(" "), f.c0, f.c1);
            }
        }
        put!(out, "))");
        headers.push((id, group));
        start = end + 1;
    }

    // The names of the zones.
    for (id, (name, _)) in (1..).zip(&zones.list) {
        put!(out, "(45 ({id:x} fluid {name})())");
    }
    for (id, group) in headers {
        put!(out, "(45 ({id:x} {} {})())", group.kind, group.name);
    }
    out
}
