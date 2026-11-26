// SPDX-License-Identifier: GPL-3.0-or-later
//! Wavefront OBJ surfaces: the geometry most modelling and CAD tools export.
//!
//! Only the geometry is read: `v` vertices and `f` faces. Colours, normals, texture
//! coordinates, materials and lines are ignored. Objects (`o`) and groups (`g`) name the pieces of the
//! surface (see [`Stl::parts`](crate::stl::Stl::parts)). A face with more than three corners is
//! split into a fan from its first corner, which is right for the convex polygons exporters
//! write. Indices are 1-based, and a negative one counts back from the last vertex read so far.
//! Vertices are merged where they coincide exactly, as the STL reader does, so a closed model is
//! recognised as closed. OBJ has no unit: the numbers are taken as metres (see
//! [`Stl::scale`](crate::stl::Stl::scale) for millimetres).
//!
//! The file is hostile input: its size, the vertex and face counts, the corners of one face and
//! every index and number are checked, and an error names the line.

use std::path::Path;

use crate::error::{MeshError, Result};
use crate::stl::{Builder, MAX_FILE_BYTES, MAX_TRIANGLES, Stl};

/// The most vertices a file may declare.
pub const MAX_VERTICES: usize = 40_000_000;

/// The most corners one face may have.
pub const MAX_CORNERS: usize = 1_000;

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// Reads an OBJ file, refusing one that is not a file or is too big.
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

/// Reads the bytes of an OBJ file.
pub fn parse(bytes: &[u8]) -> Result<Stl> {
    // Names and comments may be in any encoding; only the keywords and numbers matter.
    let text = String::from_utf8_lossy(bytes);
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut builder = Builder::default();
    let mut pending = String::new();
    for (n, raw) in text.lines().enumerate() {
        let line_number = n + 1;
        // A trailing backslash continues the line.
        let line = raw.trim_end();
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        let line = std::mem::take(&mut pending);
        let line = line.split('#').next().unwrap_or_default();
        let mut words = line.split_ascii_whitespace();
        let at = |e: MeshError| MeshError::Invalid(format!("line {line_number}: {e}"));
        match words.next() {
            Some("v") => {
                if points.len() >= MAX_VERTICES {
                    return invalid(format!("more than {MAX_VERTICES} vertices"));
                }
                let mut point = [0.0; 3];
                for slot in &mut point {
                    let word = words
                        .next()
                        .map_or_else(|| invalid("a vertex needs three numbers"), Ok);
                    let word = word.map_err(at)?;
                    *slot = match word.parse::<f64>() {
                        Ok(v) if v.is_finite() => v,
                        _ => {
                            return invalid(format!(
                                "line {line_number}: '{word}' is not a finite number"
                            ));
                        }
                    };
                }
                points.push(point);
            }
            Some("f") => {
                let mut corners: Vec<[f64; 3]> = Vec::new();
                for word in words {
                    if corners.len() >= MAX_CORNERS {
                        return invalid(format!(
                            "line {line_number}: a face has more than {MAX_CORNERS} corners"
                        ));
                    }
                    // `v`, `v/t`, `v//n` or `v/t/n`: only the first number is a vertex.
                    let index = word.split('/').next().unwrap_or_default();
                    let Ok(index) = index.parse::<i64>() else {
                        return invalid(format!(
                            "line {line_number}: '{word}' is not a vertex index"
                        ));
                    };
                    let position = if index > 0 {
                        usize::try_from(index - 1).ok()
                    } else if index < 0 {
                        points
                            .len()
                            .checked_sub(usize::try_from(-index).unwrap_or(usize::MAX))
                    } else {
                        None
                    };
                    let Some(&corner) = position.and_then(|p| points.get(p)) else {
                        return invalid(format!(
                            "line {line_number}: vertex index {index} is outside the {} vertices read so far",
                            points.len()
                        ));
                    };
                    corners.push(corner);
                }
                let Some((&first, rest)) = corners.split_first() else {
                    return invalid(format!("line {line_number}: a face has no corners"));
                };
                if rest.len() < 2 {
                    return invalid(format!("line {line_number}: a face needs three corners"));
                }
                for pair in rest.windows(2) {
                    if let [b, c] = pair {
                        if builder.stl.triangles.len() >= MAX_TRIANGLES {
                            return invalid(format!("more than {MAX_TRIANGLES} triangles"));
                        }
                        builder.triangle([first, *b, *c])?;
                    }
                }
            }
            // An object or a group starts a named piece; the faces after it belong to it.
            Some("o" | "g") => {
                let name: Vec<&str> = words.take(4).collect();
                builder.part(&if name.is_empty() {
                    "default".to_owned()
                } else {
                    name.join(" ")
                });
            }
            _ => {}
        }
    }
    if builder.stl.triangles.is_empty() {
        return invalid("the OBJ file has no faces");
    }
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube of six quads, outward normals, with the usual extras.
    const CUBE: &str = "# a cube\n\
        mtllib none.mtl\no cube\n\
        v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
        vn 0 0 1\nvt 0 0\ns off\nusemtl x\n\
        f 1//1 4//1 3//1 2//1\n\
        f 5/1/1 6/2/1 7/3/1 8/4/1\n\
        f 1 2 6 5\n\
        f 2 3 7 6\n\
        f 3 4 8 7\n\
        f 4 1 5 8\n";

    #[test]
    fn quads_with_extras_make_a_closed_cube() {
        let stl = parse(CUBE.as_bytes()).unwrap();
        assert_eq!(stl.triangles.len(), 12);
        assert_eq!(stl.vertices.len(), 8);
        let s = stl.summary();
        assert!(s.closed());
        assert!((s.volume.unwrap() - 1.0).abs() < 1e-12);
        assert!((s.area - 6.0).abs() < 1e-12);
    }

    #[test]
    fn negative_indices_count_back_and_a_continued_line_joins() {
        let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\n\
            f -4 -3 -2\nf 1 2 \\\n 4\nf 1 3 4 # comment\nf 2 3 4\n";
        let stl = parse(text.as_bytes()).unwrap();
        assert_eq!(stl.triangles.len(), 4);
        assert!(stl.summary().closed());
    }

    #[test]
    fn hostile_files_are_refused_with_the_line() {
        for (text, needle) in [
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 0\n", "outside"),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 4\n", "outside"),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 -9\n", "outside"),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2\n", "three corners"),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf\n", "no corners"),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 x 3\n", "not a vertex index"),
            ("v 0 0 0\nv 1 0\nf 1 1 1\n", "line 2"),
            ("v 0 0 nan\n", "finite"),
            ("v 0 0 1e999\n", "finite"),
            ("v 0 0 0\n", "no faces"),
            ("", "no faces"),
        ] {
            let error = parse(text.as_bytes()).unwrap_err().to_string();
            assert!(error.contains(needle), "{text:?}: {error}");
        }
        // A face with too many corners, and one index no integer can hold.
        let mut huge = String::from("v 0 0 0\nf");
        for _ in 0..=MAX_CORNERS {
            huge.push_str(" 1");
        }
        assert!(parse(huge.as_bytes()).is_err());
        assert!(parse(b"v 0 0 0\nf 1 1 99999999999999999999999\n").is_err());
        // Bytes that are not text at all do not panic.
        assert!(parse(&[0xff, 0xfe, 0x00, 0x80, b'f', b' ', 0xc3]).is_err());
    }

    #[test]
    fn a_file_is_read_by_its_extension_and_scaled() {
        let dir = tempfile::tempdir().unwrap();
        let obj = dir.path().join("Part.OBJ");
        std::fs::write(&obj, CUBE).unwrap();
        let mut stl = Stl::load(&obj).unwrap();
        stl.scale(0.001).unwrap();
        assert!((stl.summary().volume.unwrap() - 1.0e-9).abs() < 1e-18);
        assert!(stl.scale(0.0).is_err());
        assert!(stl.scale(f64::NAN).is_err());
        let other = dir.path().join("part.step");
        std::fs::write(&other, "ISO-10303-21;").unwrap();
        let error = Stl::load(&other).unwrap_err().to_string();
        assert!(error.contains("STL, OBJ and STEP"), "{error}");
        assert!(Stl::load(&dir.path().join("missing.obj")).is_err());
    }

    #[test]
    fn objects_and_groups_become_the_parts_of_the_surface() {
        let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\n\
            o first\nf 1 3 2\ng second part\nf 1 2 4\nf 1 4 3\no\nf 2 3 4\n";
        let stl = parse(text.as_bytes()).unwrap();
        let parts: Vec<_> = stl
            .parts
            .iter()
            .map(|p| (p.name.as_str(), p.start, p.end))
            .collect();
        assert_eq!(
            parts,
            [("first", 0, 1), ("second part", 1, 3), ("default", 3, 4)]
        );
        assert_eq!(stl.part_of_each().unwrap(), [0, 1, 1, 2]);
        // Faces before any name are a part of their own, and a file with no names has none.
        let early = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\no late\nf 1 3 2\n").unwrap();
        assert_eq!(early.parts[0].name, "default");
        assert_eq!(early.parts.len(), 2);
        let none = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        assert!(none.parts.is_empty());
    }
}
