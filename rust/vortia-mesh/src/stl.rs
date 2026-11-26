//! STL surfaces: the geometry a mesh is made around, read in both encodings.
//!
//! Format detection: a binary file has exactly `84 + 50 · n` bytes for the `n` triangles its
//! header declares, and then it is binary even when its header starts with `solid` (some
//! exporters write it so); anything else is read as ASCII, whose keywords must come in order.
//! Every coordinate has to be finite. The vertices are merged where they coincide exactly, so
//! the triangles share points and the surface can be asked whether it is closed.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{MeshError, Result};

/// The largest file read; a bigger one is refused before it is loaded.
pub const MAX_FILE_BYTES: u64 = 1 << 30;

/// The most triangles a surface holds.
pub const MAX_TRIANGLES: usize = 40_000_000;

const HEADER: usize = 80;
const FACET: usize = 50;

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// A triangulated surface with its vertices merged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stl {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// The named pieces the file was made of (OBJ objects and groups, the solids of an STL, the
    /// bodies of a STEP file), as runs of consecutive triangles in order. Empty when the file
    /// does not name any.
    pub parts: Vec<SurfacePart>,
}

/// A run of triangles with a name: `triangles[start..end]` of the surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfacePart {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// What a surface is, in numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub triangles: usize,
    pub vertices: usize,
    pub low: [f64; 3],
    pub high: [f64; 3],
    /// The sum of the areas of the triangles.
    pub area: f64,
    /// The volume it encloses, when it is closed; the surface itself when it is open has none.
    pub volume: Option<f64>,
    /// Edges that only one triangle has: the holes.
    pub open_edges: usize,
    /// Edges that more than two triangles share.
    pub non_manifold_edges: usize,
    /// Triangles with no area.
    pub degenerate: usize,
}

impl Summary {
    /// Every edge is shared by exactly two triangles.
    pub fn closed(&self) -> bool {
        self.triangles > 0 && self.open_edges == 0 && self.non_manifold_edges == 0
    }
}

impl Stl {
    /// The part each triangle belongs to, when the parts are a proper cover: every triangle in
    /// exactly one part, the parts in order. `None` otherwise.
    pub fn part_of_each(&self) -> Option<Vec<usize>> {
        let mut of = Vec::with_capacity(self.triangles.len());
        let mut next = 0;
        for (index, part) in self.parts.iter().enumerate() {
            if part.start != next || part.end < part.start {
                return None;
            }
            of.extend(std::iter::repeat_n(index, part.end - part.start));
            next = part.end;
        }
        (next == self.triangles.len()).then_some(of)
    }

    /// Reads a file, refusing one that is not a file, is too big or is not an STL.
    pub fn read(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path)
            .map_err(|e| MeshError::Invalid(format!("{}: {e}", path.display())))?;
        if !meta.is_file() {
            return invalid(format!("{} is not a file", path.display()));
        }
        if meta.len() > MAX_FILE_BYTES {
            return invalid(format!(
                "{} is larger than {} MiB",
                path.display(),
                MAX_FILE_BYTES >> 20
            ));
        }
        let bytes = std::fs::read(path)
            .map_err(|e| MeshError::Invalid(format!("{}: {e}", path.display())))?;
        Self::parse(&bytes)
    }

    /// Reads the bytes of an STL file in either encoding.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if let Some(count) = binary_count(bytes) {
            return parse_binary(bytes, count);
        }
        let text = std::str::from_utf8(bytes).or_else(|e| {
            // A solid name in Latin-1 is legal enough; only the keywords and numbers matter.
            bytes
                .get(..e.valid_up_to())
                .map_or(Err(()), |ok| std::str::from_utf8(ok).map_err(|_| ()))
        });
        match text {
            Ok(text) if text.trim_start().starts_with("solid") => parse_ascii(text),
            _ => invalid("not an STL file: neither a binary of the right size nor an ASCII solid"),
        }
    }

    /// Reads a surface file by its extension: `.stl`, `.obj`, `.ply`, `.gltf` or `.glb`, upper or lower case.
    pub fn load(path: &Path) -> Result<Self> {
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        match extension.as_deref() {
            Some("stl") => Self::read(path),
            Some("obj") => crate::obj::read(path),
            Some("ply") => crate::ply::read(path),
            Some("gltf" | "glb") => crate::gltf::read(path),
            // A file with another name may still be an STL.
            _ => Self::read(path).map_err(|e| match e {
                MeshError::Invalid(message) => MeshError::Invalid(format!(
                    "{message}; surfaces are read from STL, OBJ, PLY, glTF and STEP files (IGES is not read)"
                )),
                other => other,
            }),
        }
    }

    /// The surface as a binary STL: the triangles with their normals, coordinates in single precision.
    pub fn to_binary(&self) -> Vec<u8> {
        let mut out = vec![0u8; HEADER];
        let label = b"Vortia surface";
        if let Some(head) = out.get_mut(..label.len()) {
            head.copy_from_slice(label);
        }
        out.extend_from_slice(
            &u32::try_from(self.triangles.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for tri in &self.triangles {
            let [a, b, c] = tri.map(|i| self.corner(i));
            let n = cross(sub(b, a), sub(c, a));
            let length = dot(n, n).sqrt();
            let unit = if length > 0.0 {
                n.map(|v| v / length)
            } else {
                [0.0; 3]
            };
            for v in [unit, a, b, c].into_iter().flatten() {
                out.extend_from_slice(&(v as f32).to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
        }
        out
    }

    /// Writes the surface to `path` as a binary STL.
    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_binary())
            .map_err(|e| MeshError::Invalid(format!("{}: {e}", path.display())))
    }

    /// Multiplies every coordinate by `factor` (a unit change, such as 0.001 for millimetres).
    pub fn scale(&mut self, factor: f64) -> Result<()> {
        if !factor.is_finite() || factor <= 0.0 {
            return invalid(format!("the scale must be a positive number, not {factor}"));
        }
        for v in &mut self.vertices {
            for c in v {
                *c *= factor;
            }
        }
        Ok(())
    }

    fn corner(&self, index: u32) -> [f64; 3] {
        self.vertices
            .get(index as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Measures the surface.
    pub fn summary(&self) -> Summary {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for v in &self.vertices {
            for ((l, h), c) in low.iter_mut().zip(high.iter_mut()).zip(v) {
                *l = l.min(*c);
                *h = h.max(*c);
            }
        }
        if self.vertices.is_empty() {
            low = [0.0; 3];
            high = [0.0; 3];
        }
        let mut area = 0.0;
        let mut signed_volume = 0.0;
        let mut degenerate = 0;
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for &[a, b, c] in &self.triangles {
            let (pa, pb, pc) = (self.corner(a), self.corner(b), self.corner(c));
            let normal = cross(sub(pb, pa), sub(pc, pa));
            let twice = dot(normal, normal).sqrt();
            if twice == 0.0 || a == b || b == c || a == c {
                degenerate += 1;
            }
            area += twice / 2.0;
            signed_volume += dot(pa, cross(pb, pc)) / 6.0;
            for (from, to) in [(a, b), (b, c), (c, a)] {
                if from != to {
                    *edges.entry((from.min(to), from.max(to))).or_insert(0) += 1;
                }
            }
        }
        let open_edges = edges.values().filter(|&&n| n == 1).count();
        let non_manifold_edges = edges.values().filter(|&&n| n > 2).count();
        let closed = !self.triangles.is_empty() && open_edges == 0 && non_manifold_edges == 0;
        Summary {
            triangles: self.triangles.len(),
            vertices: self.vertices.len(),
            low,
            high,
            area,
            volume: closed.then_some(signed_volume.abs()),
            open_edges,
            non_manifold_edges,
            degenerate,
        }
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

/// Merges coinciding vertices as the triangles are added.
#[derive(Default)]
pub(crate) struct Builder {
    pub(crate) stl: Stl,
    index: HashMap<[u64; 3], u32>,
    /// Where each named piece starts: its name and the triangle it begins at.
    marks: Vec<(String, usize)>,
}

impl Builder {
    /// A new named piece begins with the next triangle.
    pub(crate) fn part(&mut self, name: &str) {
        self.marks.push((name.to_owned(), self.stl.triangles.len()));
    }

    /// The surface, with the named pieces as runs of triangles (empty pieces dropped).
    pub(crate) fn finish(mut self) -> Stl {
        let total = self.stl.triangles.len();
        let mut parts = Vec::new();
        if let Some((_, first)) = self.marks.first()
            && *first > 0
        {
            parts.push(SurfacePart {
                name: "default".into(),
                start: 0,
                end: *first,
            });
        }
        for (k, (name, start)) in self.marks.iter().enumerate() {
            let end = self.marks.get(k + 1).map_or(total, |(_, next)| *next);
            if end > *start {
                parts.push(SurfacePart {
                    name: name.clone(),
                    start: *start,
                    end,
                });
            }
        }
        self.stl.parts = parts;
        self.stl
    }

    pub(crate) fn vertex(&mut self, v: [f64; 3]) -> Result<u32> {
        // -0.0 and 0.0 are one point.
        let key = v.map(|c| if c == 0.0 { 0 } else { c.to_bits() });
        if let Some(&i) = self.index.get(&key) {
            return Ok(i);
        }
        let Ok(i) = u32::try_from(self.stl.vertices.len()) else {
            return invalid("too many vertices");
        };
        self.stl.vertices.push(v);
        self.index.insert(key, i);
        Ok(i)
    }

    pub(crate) fn triangle(&mut self, corners: [[f64; 3]; 3]) -> Result<()> {
        if self.stl.triangles.len() >= MAX_TRIANGLES {
            return invalid(format!("more than {MAX_TRIANGLES} triangles"));
        }
        let [a, b, c] = corners;
        let ids = [self.vertex(a)?, self.vertex(b)?, self.vertex(c)?];
        self.stl.triangles.push(ids);
        Ok(())
    }
}

/// The triangle count of a binary file, when the size says it is one.
fn binary_count(bytes: &[u8]) -> Option<usize> {
    let raw = bytes.get(HEADER..HEADER + 4)?;
    let count = u32::from_le_bytes(raw.try_into().ok()?) as u64;
    let expected = (HEADER as u64 + 4).checked_add(count.checked_mul(FACET as u64)?)?;
    (expected == bytes.len() as u64).then_some(count as usize)
}

fn parse_binary(bytes: &[u8], count: usize) -> Result<Stl> {
    if count > MAX_TRIANGLES {
        return invalid(format!("{count} triangles is more than {MAX_TRIANGLES}"));
    }
    let body = bytes.get(HEADER + 4..).unwrap_or_default();
    let mut builder = Builder::default();
    for (n, facet) in body.as_chunks::<FACET>().0.iter().enumerate() {
        let mut corners = [[0.0; 3]; 3];
        for (corner, out) in corners.iter_mut().enumerate() {
            for (axis, slot) in out.iter_mut().enumerate() {
                // After the normal: three corners of three floats.
                let at = 12 + (corner * 3 + axis) * 4;
                let raw: [u8; 4] = facet
                    .get(at..at + 4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or_default();
                let value = f64::from(f32::from_le_bytes(raw));
                if !value.is_finite() {
                    return invalid(format!("non-finite vertex in triangle {n}"));
                }
                *slot = value;
            }
        }
        builder.triangle(corners)?;
    }
    Ok(builder.finish())
}

/// The words of an ASCII solid, one at a time.
struct Words<'a>(std::str::SplitAsciiWhitespace<'a>);

impl Words<'_> {
    fn next(&mut self) -> Result<&str> {
        self.0
            .next()
            .map_or_else(|| invalid("the ASCII STL ends too soon"), Ok)
    }

    fn expect(&mut self, keyword: &str) -> Result<()> {
        let word = self.next()?;
        if word == keyword {
            Ok(())
        } else {
            invalid(format!("expected '{keyword}', found '{word}'"))
        }
    }

    fn number(&mut self) -> Result<f64> {
        let word = self.next()?;
        match word.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(v),
            _ => invalid(format!("'{word}' is not a finite number")),
        }
    }

    fn point(&mut self) -> Result<[f64; 3]> {
        Ok([self.number()?, self.number()?, self.number()?])
    }
}

fn parse_ascii(text: &str) -> Result<Stl> {
    let mut words = Words(text.split_ascii_whitespace());
    words.expect("solid")?;
    let mut builder = Builder::default();
    // The solid's name is free text up to the first facet.
    let mut name: Vec<String> = Vec::new();
    let mut named = false;
    let mut solids = 0usize;
    loop {
        match words.next()? {
            "endsolid" => {
                // More solids may follow in the same file; the words in between are the name
                // that closes this one.
                loop {
                    match words.0.next() {
                        None => return Ok(builder.finish()),
                        Some("solid") => break,
                        Some(_) => {}
                    }
                }
                name.clear();
                named = false;
            }
            "facet" => {
                if !named {
                    solids += 1;
                    let label = if name.is_empty() {
                        format!("solid{solids}")
                    } else {
                        name.join(" ")
                    };
                    builder.part(&label);
                    named = true;
                }
                let n = builder.stl.triangles.len();
                let at = |e: MeshError| MeshError::Invalid(format!("triangle {n}: {e}"));
                words.expect("normal").map_err(at)?;
                words.point().map_err(at)?;
                words.expect("outer").map_err(at)?;
                words.expect("loop").map_err(at)?;
                let mut corners = [[0.0; 3]; 3];
                for corner in &mut corners {
                    words.expect("vertex").map_err(at)?;
                    *corner = words.point().map_err(at)?;
                }
                builder.triangle(corners)?;
                words.expect("endloop").map_err(at)?;
                words.expect("endfacet").map_err(at)?;
            }
            word => {
                if !named && name.len() < 16 {
                    name.push(word.to_owned());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube, 12 triangles, outward normals, as ASCII.
    fn cube() -> Vec<[[f64; 3]; 3]> {
        let p = |x: f64, y: f64, z: f64| [x, y, z];
        let (a, b, c, d) = (p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.));
        let (e, f, g, h) = (p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.));
        vec![
            [a, d, c],
            [a, c, b],
            [e, f, g],
            [e, g, h],
            [a, b, f],
            [a, f, e],
            [b, c, g],
            [b, g, f],
            [c, d, h],
            [c, h, g],
            [d, a, e],
            [d, e, h],
        ]
    }

    #[test]
    fn a_surface_written_as_binary_reads_back_the_same() {
        let stl = Stl::parse(ascii(&cube()).as_bytes()).unwrap();
        let bytes = stl.to_binary();
        assert_eq!(bytes.len(), HEADER + 4 + FACET * 12);
        let back = Stl::parse(&bytes).unwrap();
        assert_eq!(back.summary(), stl.summary());
        assert!(back.summary().closed());
    }

    fn ascii(tris: &[[[f64; 3]; 3]]) -> String {
        let mut s = String::from("solid cube\n");
        for t in tris {
            s.push_str("facet normal 0 0 0\nouter loop\n");
            for v in t {
                s.push_str(&format!("vertex {} {} {}\n", v[0], v[1], v[2]));
            }
            s.push_str("endloop\nendfacet\n");
        }
        s.push_str("endsolid cube\n");
        s
    }

    fn binary(tris: &[[[f64; 3]; 3]], header: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER];
        let n = header.len().min(HEADER);
        out[..n].copy_from_slice(&header[..n]);
        out.extend((tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend([0u8; 12]);
            for v in t {
                for c in v {
                    out.extend((*c as f32).to_le_bytes());
                }
            }
            out.extend([0u8; 2]);
        }
        out
    }

    #[test]
    fn a_cube_in_both_encodings_is_the_same_closed_solid() {
        let tris = cube();
        for stl in [
            Stl::parse(ascii(&tris).as_bytes()).unwrap(),
            Stl::parse(&binary(&tris, b"a cube")).unwrap(),
        ] {
            assert_eq!(stl.vertices.len(), 8);
            let s = stl.summary();
            assert_eq!(s.triangles, 12);
            assert!(s.closed());
            assert!((s.area - 6.0).abs() < 1e-12);
            assert!((s.volume.unwrap() - 1.0).abs() < 1e-12);
            assert_eq!(s.low, [0.0; 3]);
            assert_eq!(s.high, [1.0; 3]);
            assert_eq!(s.degenerate, 0);
        }
    }

    #[test]
    fn a_binary_file_whose_header_says_solid_is_still_binary() {
        let stl = Stl::parse(&binary(&cube(), b"solid exported by a tool")).unwrap();
        assert_eq!(stl.triangles.len(), 12);
    }

    #[test]
    fn a_missing_face_leaves_holes_and_no_volume() {
        let mut tris = cube();
        tris.pop();
        let s = Stl::parse(ascii(&tris).as_bytes()).unwrap().summary();
        assert!(!s.closed());
        assert_eq!(s.open_edges, 3);
        assert!(s.volume.is_none());
    }

    #[test]
    fn a_hostile_file_is_an_error_and_never_a_panic() {
        // Wrong size for the declared count, truncated, empty, a count that overflows.
        let mut lying = binary(&cube(), b"");
        lying.truncate(lying.len() - 3);
        let mut huge = vec![0u8; HEADER];
        huge.extend(u32::MAX.to_le_bytes());
        for bytes in [
            lying,
            huge,
            Vec::new(),
            b"solid".to_vec(),
            b"solid x\nfacet normal 0 0\n".to_vec(),
            b"solid x facet normal 0 0 0 outer loop vertex 0 0 nan vertex 0 0 0 vertex 1 0 0 endloop endfacet endsolid"
                .to_vec(),
            b"solid x facet normal 0 0 0 outer loop vertex 0 0 1e999 vertex 0 0 0 vertex 1 0 0"
                .to_vec(),
            vec![0xff; 200],
            b"hello".to_vec(),
        ] {
            assert!(Stl::parse(&bytes).is_err(), "{:?}", &bytes[..bytes.len().min(30)]);
        }
        // A non-finite float in a binary file.
        let mut bin = binary(&cube(), b"");
        bin[HEADER + 4 + 12..HEADER + 4 + 16].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Stl::parse(&bin).is_err());
    }

    #[test]
    fn an_empty_solid_is_an_empty_surface() {
        let stl = Stl::parse(b"solid nothing\nendsolid nothing\n").unwrap();
        let s = stl.summary();
        assert_eq!((s.triangles, s.vertices), (0, 0));
        assert!(!s.closed());
        assert_eq!(s.low, [0.0; 3]);
    }

    #[test]
    fn a_name_with_a_comma_for_decimals_is_not_a_number() {
        // A locale that writes 1,5 does not make a valid STL: reported, not misread.
        let text = ascii(&cube()).replacen("vertex 0 0 0", "vertex 0,5 0 0", 1);
        assert!(Stl::parse(text.as_bytes()).is_err());
    }

    #[test]
    fn the_solids_of_an_ascii_file_are_its_parts() {
        let facet = |z: f64| {
            format!(
                "facet normal 0 0 1\nouter loop\nvertex 0 0 {z}\nvertex 1 0 {z}\nvertex 0 1 {z}\n\
                 endloop\nendfacet\n"
            )
        };
        let text = format!(
            "solid left wing\n{}{}endsolid left wing\nsolid\n{}endsolid\n",
            facet(0.0),
            facet(1.0),
            facet(2.0)
        );
        let stl = Stl::parse(text.as_bytes()).unwrap();
        assert_eq!(stl.triangles.len(), 3);
        let parts: Vec<_> = stl
            .parts
            .iter()
            .map(|p| (p.name.as_str(), p.start, p.end))
            .collect();
        assert_eq!(parts, [("left wing", 0, 2), ("solid2", 2, 3)]);
        // One solid is one part, and a binary file names nothing.
        let one = Stl::parse(format!("solid a\n{}endsolid a\n", facet(0.0)).as_bytes()).unwrap();
        assert_eq!(one.parts.len(), 1);
        assert!(Stl::parse(&one.to_binary()).unwrap().parts.is_empty());
    }
}
