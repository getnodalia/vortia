// SPDX-License-Identifier: GPL-3.0-or-later
//! Stanford PLY surfaces, ASCII or binary (either byte order): the format scanners and
//! photogrammetry tools export.
//!
//! Only the geometry is read: the `x`, `y` and `z` of the `vertex` element and the vertex list of
//! the `face` element. Colours, normals, texture coordinates, edges, triangle strips and any
//! other element are read past and ignored. A face with more than three corners is split into a
//! fan from its first corner, as the OBJ reader does, and vertices that coincide exactly are
//! merged so a closed model is recognised as closed. PLY has no unit: the numbers are taken as
//! metres (see [`Stl::scale`] for millimetres).
//!
//! The file is hostile input: its size, the counts it declares, the corners of a face and every
//! index and number are checked, and a binary file that ends early is an error, not a panic.

use std::path::Path;

use crate::error::{MeshError, Result};
use crate::stl::{Builder, MAX_FILE_BYTES, MAX_TRIANGLES, Stl};

/// The most vertices a file may declare.
pub const MAX_VERTICES: usize = 40_000_000;
/// The most corners one face may have.
pub const MAX_CORNERS: usize = 1_000;
/// The most elements and properties a header may declare.
const MAX_DECLARED: usize = 64;

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// Reads a PLY file, refusing one that is not a file or is too big.
pub fn read(path: &Path) -> Result<Stl> {
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
    let bytes =
        std::fs::read(path).map_err(|e| MeshError::Invalid(format!("{}: {e}", path.display())))?;
    parse(&bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Ascii,
    Little,
    Big,
}

#[derive(Clone, Copy, Debug)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Ty {
    fn named(word: &str) -> Option<Self> {
        Some(match word {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }
}

enum Kind {
    Scalar(Ty),
    List { count: Ty, item: Ty },
}

struct Property {
    name: String,
    kind: Kind,
}

struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

/// Where the numbers come from: the words of an ASCII body or the bytes of a binary one.
enum Body<'a> {
    Ascii(std::str::SplitAsciiWhitespace<'a>),
    Binary { bytes: &'a [u8], big: bool },
}

impl Body<'_> {
    fn value(&mut self, ty: Ty) -> Result<f64> {
        match self {
            Self::Ascii(words) => {
                let Some(word) = words.next() else {
                    return invalid("the file ends before all its elements are read");
                };
                match word.parse::<f64>() {
                    Ok(v) if v.is_finite() => Ok(v),
                    _ => invalid(format!("'{word}' is not a finite number")),
                }
            }
            Self::Binary { bytes, big } => {
                let size = ty.size();
                let Some((head, rest)) = bytes.split_at_checked(size) else {
                    return invalid("the file ends before all its elements are read");
                };
                *bytes = rest;
                let mut raw = [0u8; 8];
                let Some(slot) = raw.get_mut(..size) else {
                    return invalid("a number is wider than eight bytes");
                };
                slot.copy_from_slice(head);
                if *big {
                    slot.reverse();
                }
                let v = match ty {
                    Ty::I8 => f64::from(i8::from_le_bytes([raw[0]])),
                    Ty::U8 => f64::from(raw[0]),
                    Ty::I16 => f64::from(i16::from_le_bytes([raw[0], raw[1]])),
                    Ty::U16 => f64::from(u16::from_le_bytes([raw[0], raw[1]])),
                    Ty::I32 => f64::from(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    Ty::U32 => f64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    Ty::F32 => f64::from(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    Ty::F64 => f64::from_le_bytes(raw),
                };
                if v.is_finite() {
                    Ok(v)
                } else {
                    invalid("a number is not finite")
                }
            }
        }
    }

    /// A list length or an index: a whole number that is not negative.
    fn whole(&mut self, ty: Ty) -> Result<usize> {
        let v = self.value(ty)?;
        if v >= 0.0 && v.fract() == 0.0 && v < 4.0e9 {
            Ok(v as usize)
        } else {
            invalid(format!("{v} is not a count or an index"))
        }
    }
}

/// What the header says, and where the body begins.
fn header(bytes: &[u8]) -> Result<(Format, Vec<Element>, &[u8])> {
    // The header is text up to the line `end_header`; the body follows it.
    let mut at = 0;
    let mut lines: Vec<&str> = Vec::new();
    loop {
        let Some(rest) = bytes.get(at..).filter(|r| !r.is_empty()) else {
            return invalid("the header has no end_header line");
        };
        let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let line = std::str::from_utf8(rest.get(..end).unwrap_or_default())
            .map_err(|_| MeshError::Invalid("the header is not text".into()))?
            .trim();
        at += end + 1;
        if line == "end_header" {
            break;
        }
        lines.push(line);
        if lines.len() > 4 * MAX_DECLARED {
            return invalid("the header is too long");
        }
    }
    let body = bytes.get(at.min(bytes.len())..).unwrap_or_default();
    let mut lines = lines.into_iter();
    if lines.next() != Some("ply") {
        return invalid("not a PLY file: it does not begin with 'ply'");
    }
    let mut format = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.split_ascii_whitespace().collect();
        match words.as_slice() {
            ["format", name, ..] => {
                format = Some(match *name {
                    "ascii" => Format::Ascii,
                    "binary_little_endian" => Format::Little,
                    "binary_big_endian" => Format::Big,
                    other => return invalid(format!("'{other}' is not a PLY format")),
                });
            }
            ["element", name, count] => {
                let Ok(count) = count.parse::<usize>() else {
                    return invalid(format!("'{count}' is not an element count"));
                };
                if elements.len() >= MAX_DECLARED {
                    return invalid("the header declares too many elements");
                }
                elements.push(Element {
                    name: (*name).to_owned(),
                    count,
                    properties: Vec::new(),
                });
            }
            ["property", "list", count, item, name] => {
                let (Some(count), Some(item)) = (Ty::named(count), Ty::named(item)) else {
                    return invalid(format!("property '{name}' has a type that is not PLY's"));
                };
                let Some(element) = elements.last_mut() else {
                    return invalid("a property comes before any element");
                };
                if element.properties.len() >= MAX_DECLARED {
                    return invalid("an element has too many properties");
                }
                element.properties.push(Property {
                    name: (*name).to_owned(),
                    kind: Kind::List { count, item },
                });
            }
            ["property", ty, name] => {
                let Some(ty) = Ty::named(ty) else {
                    return invalid(format!("property '{name}' has a type that is not PLY's"));
                };
                let Some(element) = elements.last_mut() else {
                    return invalid("a property comes before any element");
                };
                if element.properties.len() >= MAX_DECLARED {
                    return invalid("an element has too many properties");
                }
                element.properties.push(Property {
                    name: (*name).to_owned(),
                    kind: Kind::Scalar(ty),
                });
            }
            _ => {}
        }
    }
    let Some(format) = format else {
        return invalid("the header has no format line");
    };
    Ok((format, elements, body))
}

/// Reads the bytes of a PLY file.
pub fn parse(bytes: &[u8]) -> Result<Stl> {
    let (format, elements, body) = header(bytes)?;
    let mut body = match format {
        Format::Ascii => Body::Ascii(
            std::str::from_utf8(body)
                .map_err(|_| MeshError::Invalid("an ASCII PLY body is not text".into()))?
                .split_ascii_whitespace(),
        ),
        Format::Little | Format::Big => Body::Binary {
            bytes: body,
            big: format == Format::Big,
        },
    };
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut builder = Builder::default();
    for element in &elements {
        match element.name.as_str() {
            "vertex" => vertices(element, &mut body, &mut points)?,
            "face" => faces(element, &mut body, &points, &mut builder)?,
            _ => skip(element, &mut body)?,
        }
    }
    if builder.stl.triangles.is_empty() {
        return invalid("the PLY file has no faces");
    }
    Ok(builder.finish())
}

fn vertices(element: &Element, body: &mut Body<'_>, points: &mut Vec<[f64; 3]>) -> Result<()> {
    if element.count > MAX_VERTICES {
        return invalid(format!("more than {MAX_VERTICES} vertices"));
    }
    let slot = |name: &str| {
        element
            .properties
            .iter()
            .position(|p| p.name == name && matches!(p.kind, Kind::Scalar(_)))
    };
    let (Some(x), Some(y), Some(z)) = (slot("x"), slot("y"), slot("z")) else {
        return invalid("the vertex element has no x, y and z");
    };
    points.reserve(element.count.min(1 << 20));
    for _ in 0..element.count {
        let mut point = [0.0; 3];
        for (i, property) in element.properties.iter().enumerate() {
            let read = row_value(property, body)?;
            if let Some(out) = [x, y, z].iter().position(|&s| s == i)
                && let Some(c) = point.get_mut(out)
            {
                *c = read;
            }
        }
        points.push(point);
    }
    Ok(())
}

/// One property of one row: a scalar's value, or a list read past (its value is 0).
fn row_value(property: &Property, body: &mut Body<'_>) -> Result<f64> {
    match property.kind {
        Kind::Scalar(ty) => body.value(ty),
        Kind::List { count, item } => {
            for _ in 0..body.whole(count)? {
                body.value(item)?;
            }
            Ok(0.0)
        }
    }
}

fn skip(element: &Element, body: &mut Body<'_>) -> Result<()> {
    for _ in 0..element.count {
        for property in &element.properties {
            row_value(property, body)?;
        }
    }
    Ok(())
}

fn faces(
    element: &Element,
    body: &mut Body<'_>,
    points: &[[f64; 3]],
    builder: &mut Builder,
) -> Result<()> {
    if element.count > MAX_TRIANGLES {
        return invalid(format!("more than {MAX_TRIANGLES} faces"));
    }
    let Some(list) = element
        .properties
        .iter()
        .position(|p| matches!(p.name.as_str(), "vertex_indices" | "vertex_index"))
    else {
        return invalid("the face element has no vertex_indices list");
    };
    for face in 0..element.count {
        let mut corners: Vec<[f64; 3]> = Vec::new();
        for (i, property) in element.properties.iter().enumerate() {
            let (Kind::List { count, item }, true) = (&property.kind, i == list) else {
                row_value(property, body)?;
                continue;
            };
            let n = body.whole(*count)?;
            if n > MAX_CORNERS {
                return invalid(format!(
                    "face {face}: a face has more than {MAX_CORNERS} corners"
                ));
            }
            for _ in 0..n {
                let index = body.whole(*item)?;
                let Some(&corner) = points.get(index) else {
                    return invalid(format!(
                        "face {face}: vertex index {index} is outside the {} vertices",
                        points.len()
                    ));
                };
                corners.push(corner);
            }
        }
        let Some((&first, rest)) = corners.split_first() else {
            return invalid(format!("face {face} has no corners"));
        };
        if rest.len() < 2 {
            return invalid(format!("face {face} needs three corners"));
        }
        for pair in rest.windows(2) {
            if let [b, c] = pair {
                builder.triangle([first, *b, *c])?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE_FACES: [[u32; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [1, 2, 6, 5],
        [3, 0, 4, 7],
    ];
    const CUBE_POINTS: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];

    fn ascii_cube() -> String {
        let mut s = String::from(
            "ply\nformat ascii 1.0\ncomment a cube\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nelement face 6\nproperty list uchar int vertex_indices\nend_header\n",
        );
        for p in CUBE_POINTS {
            s += &format!("{} {} {} 200\n", p[0], p[1], p[2]);
        }
        for f in CUBE_FACES {
            s += &format!("4 {} {} {} {}\n", f[0], f[1], f[2], f[3]);
        }
        s
    }

    fn binary_cube(big: bool) -> Vec<u8> {
        let name = if big { "big" } else { "little" };
        let mut bytes = format!(
            "ply\nformat binary_{name}_endian 1.0\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nelement face 6\nproperty list uchar uint vertex_indices\nproperty uchar flag\nend_header\n"
        )
        .into_bytes();
        for p in CUBE_POINTS {
            for c in p {
                bytes.extend(if big {
                    c.to_be_bytes()
                } else {
                    c.to_le_bytes()
                });
            }
        }
        for f in CUBE_FACES {
            bytes.push(4);
            for i in f {
                bytes.extend(if big {
                    i.to_be_bytes()
                } else {
                    i.to_le_bytes()
                });
            }
            bytes.push(1);
        }
        bytes
    }

    fn check_cube(stl: &Stl) {
        assert_eq!(stl.triangles.len(), 12);
        assert_eq!(stl.vertices.len(), 8);
        let s = stl.summary();
        assert!(s.closed());
        assert!((s.volume.unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn an_ascii_cube_with_a_colour_is_a_closed_solid() {
        check_cube(&parse(ascii_cube().as_bytes()).unwrap());
    }

    #[test]
    fn binary_cubes_of_either_byte_order_are_the_same_solid() {
        check_cube(&parse(&binary_cube(false)).unwrap());
        check_cube(&parse(&binary_cube(true)).unwrap());
    }

    #[test]
    fn other_elements_are_read_past() {
        let mut text = ascii_cube().replace(
            "element face 6",
            "element edge 1\nproperty int a\nproperty list uchar int more\nelement face 6",
        );
        // The edge element comes after the vertices in the body.
        let at = text.find("4 0 3 2 1").unwrap();
        text.insert_str(at, "7 2 5 6\n");
        check_cube(&parse(text.as_bytes()).unwrap());
    }

    #[test]
    fn hostile_files_are_refused_not_panicked_on() {
        let cube = ascii_cube();
        for (bad, needle) in [
            (cube.replace("ply", "plx"), "not a PLY"),
            (cube.replace("format ascii 1.0\n", ""), "no format"),
            (
                cube.replace("format ascii", "format utf9"),
                "not a PLY format",
            ),
            (cube.replace("4 0 3 2 1", "4 0 3 2 99"), "outside"),
            (cube.replace("4 0 3 2 1", "2 0 3"), "three corners"),
            (cube.replace("4 0 3 2 1", "0"), "no corners"),
            (cube.replace("4 0 3 2 1", "4 0 3 2 -1"), "not a count"),
            (
                cube.replace("element vertex 8", "element vertex 99999999999"),
                "vertices",
            ),
            (cube.replace("end_header\n", ""), "no end_header"),
            (cube.replace("0 0 0 200", "0 0 nan 200"), "finite"),
            (cube.replace("property float z\n", ""), "no x, y and z"),
            (
                cube.replace(
                    "property list uchar int vertex_indices",
                    "property list uchar foo vertex_indices",
                ),
                "not PLY's",
            ),
        ] {
            let error = parse(bad.as_bytes()).unwrap_err().to_string();
            assert!(error.contains(needle), "{needle}: {error}");
        }
        // A binary file that ends early, at every length.
        let whole = binary_cube(false);
        let body = whole
            .windows(11)
            .position(|w| w == b"end_header\n")
            .unwrap()
            + 11;
        for cut in body..whole.len() {
            assert!(parse(&whole[..cut]).is_err(), "cut at {cut}");
        }
        // A face with too many corners, and bytes that are nothing at all.
        let many = ascii_cube().replace("4 0 3 2 1", &format!("{} {}", 2000, "0 ".repeat(2000)));
        assert!(parse(many.as_bytes()).is_err());
        assert!(parse(&[0xff, 0xfe, 0x00, 0x80]).is_err());
        assert!(parse(b"").is_err());
    }

    #[test]
    fn a_file_is_read_by_its_extension() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Part.PLY");
        std::fs::write(&file, ascii_cube()).unwrap();
        check_cube(&Stl::load(&file).unwrap());
        assert!(Stl::load(&dir.path().join("missing.ply")).is_err());
    }
}
