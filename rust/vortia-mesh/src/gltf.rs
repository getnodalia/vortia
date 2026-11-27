// SPDX-License-Identifier: GPL-3.0-or-later
//! glTF 2.0 surfaces, as a `.gltf` text file or a `.glb` binary one.
//!
//! Only the geometry is read: the `POSITION` of every triangle primitive of every mesh the scene
//! reaches, with the transforms of the nodes it hangs from (a matrix, or translation, rotation
//! and scale) applied, so an assembly comes out assembled. Triangle lists, strips and fans are
//! read; points and lines are left out. Materials, textures, normals, animations and skins are
//! ignored, and so are the mesh-compression and quantisation extensions, which are refused when
//! the file requires them. Each node's mesh is a named piece of the surface (see
//! [`Stl::parts`](crate::stl::Stl::parts)). Vertices that coincide exactly are merged, so a
//! closed model is recognised as closed. glTF positions are in metres.
//!
//! Buffers come from the `.glb` chunk, a base64 `data:` URI or a file next to the `.gltf`; a
//! buffer file may not climb out of that folder. The file is hostile input: every size, count,
//! index and offset is checked, nodes may nest no deeper than [`MAX_DEPTH`], and an error says
//! what is wrong.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::error::{MeshError, Result};
use crate::stl::{Builder, MAX_FILE_BYTES, Stl};

/// The deepest nodes may be inside nodes.
pub const MAX_DEPTH: usize = 64;
/// The most nodes one scene may visit, an instanced one counted each time.
pub const MAX_VISITS: usize = 200_000;

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(MeshError::Invalid(message.into()))
}

/// Reads a glTF or GLB file, refusing one that is not a file or is too big.
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
    parse(&bytes, path.parent())
}

/// Reads the bytes of a glTF or GLB file; `folder` is where its buffer files are.
pub fn parse(bytes: &[u8], folder: Option<&Path>) -> Result<Stl> {
    let (json, chunk) = if bytes.starts_with(b"glTF") {
        glb(bytes)?
    } else {
        (bytes, None)
    };
    let root: Value = serde_json::from_slice(json)
        .map_err(|e| MeshError::Invalid(format!("not a glTF file: {e}")))?;
    if let Some(Value::Array(required)) = root.get("extensionsRequired") {
        let names: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
        if !names.is_empty() {
            return invalid(format!(
                "the file requires glTF extensions that are not read: {}",
                names.join(", ")
            ));
        }
    }
    let buffers = buffers(&root, chunk, folder)?;
    let file = File {
        root: &root,
        buffers,
    };
    let mut builder = Builder::default();
    let mut visits = 0;
    let identity = IDENTITY;
    let roots = file.scene_roots();
    if file.list("nodes").is_empty() {
        // No scene graph: every mesh as it is.
        for (i, mesh) in file.list("meshes").iter().enumerate() {
            file.mesh(mesh, i, None, &identity, &mut builder)?;
        }
    } else {
        for node in roots {
            file.node(node, &identity, 0, &mut visits, &mut builder)?;
        }
    }
    if builder.stl.triangles.is_empty() {
        return invalid("the glTF file has no triangles");
    }
    Ok(builder.finish())
}

/// The JSON chunk and the binary chunk of a GLB container.
fn glb(bytes: &[u8]) -> Result<(&[u8], Option<&[u8]>)> {
    let word = |at: usize| -> Result<usize> {
        let raw = bytes
            .get(at..at + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .ok_or_else(|| MeshError::Invalid("the GLB file ends inside its header".into()))?;
        Ok(u32::from_le_bytes(raw) as usize)
    };
    if word(4)? != 2 {
        return invalid("only glTF version 2 is read");
    }
    let total = word(8)?.min(bytes.len());
    let mut at = 12;
    let (mut json, mut binary) = (None, None);
    while at + 8 <= total {
        let length = word(at)?;
        let kind = word(at + 4)?;
        let start = at + 8;
        let end = start
            .checked_add(length)
            .filter(|&e| e <= total)
            .ok_or_else(|| {
                MeshError::Invalid("a GLB chunk runs past the end of the file".into())
            })?;
        let data = bytes.get(start..end);
        match kind {
            0x4E4F_534A if json.is_none() => json = data,
            0x004E_4942 if binary.is_none() => binary = data,
            _ => {}
        }
        // Chunks are padded to four bytes.
        at = end + (4 - end % 4) % 4;
    }
    let Some(json) = json else {
        return invalid("the GLB file has no JSON chunk");
    };
    Ok((json, binary))
}

/// Every buffer of the file, as bytes.
fn buffers(root: &Value, chunk: Option<&[u8]>, folder: Option<&Path>) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let mut total = 0usize;
    let list = match root.get("buffers") {
        Some(Value::Array(list)) => list.as_slice(),
        _ => &[],
    };
    for (i, buffer) in list.iter().enumerate() {
        let data = match buffer.get("uri").and_then(Value::as_str) {
            Some(uri) => uri_bytes(uri, folder)?,
            None if i == 0 => chunk.map(<[u8]>::to_vec).ok_or_else(|| {
                MeshError::Invalid("buffer 0 has no uri and the file has no binary chunk".into())
            })?,
            None => return invalid(format!("buffer {i} has no uri")),
        };
        total = total.saturating_add(data.len());
        if total as u64 > MAX_FILE_BYTES {
            return invalid(format!(
                "the buffers are larger than {} MiB",
                MAX_FILE_BYTES >> 20
            ));
        }
        out.push(data);
    }
    Ok(out)
}

fn uri_bytes(uri: &str, folder: Option<&Path>) -> Result<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let Some((head, data)) = rest.split_once(',') else {
            return invalid("a data URI has no comma");
        };
        if !head.ends_with(";base64") {
            return invalid("only base64 data URIs are read");
        }
        return base64(data);
    }
    if uri.contains("://") {
        return invalid(format!(
            "'{uri}' is not a file next to the glTF: only local files are read"
        ));
    }
    let decoded = percent_decode(uri)?;
    let relative = PathBuf::from(&decoded);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return invalid(format!(
            "buffer '{uri}' is not inside the folder of the glTF file"
        ));
    }
    let Some(folder) = folder else {
        return invalid(format!(
            "buffer '{uri}' is a file, and these bytes came from none"
        ));
    };
    let path = folder.join(relative);
    let meta = std::fs::metadata(&path)
        .map_err(|e| MeshError::Invalid(format!("{}: {e}", path.display())))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return invalid(format!(
            "{} is not a file of at most {} MiB",
            path.display(),
            MAX_FILE_BYTES >> 20
        ));
    }
    std::fs::read(&path).map_err(|e| MeshError::Invalid(format!("{}: {e}", path.display())))
}

fn percent_decode(text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            let Some(v) = hex else {
                return invalid(format!("'{text}' is not a valid URI"));
            };
            out.push(v);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| MeshError::Invalid(format!("'{text}' is not a valid URI")))
}

fn base64(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return invalid("a data URI is not valid base64"),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// A 4 × 4 matrix, column by column as glTF writes it.
type Mat = [f64; 16];

const IDENTITY: Mat = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn cell(m: &Mat, col: usize, row: usize) -> f64 {
    m.get(col * 4 + row).copied().unwrap_or(0.0)
}

/// `a` after `b`: the point goes through `b` first.
fn multiply(a: &Mat, b: &Mat) -> Mat {
    let mut out = IDENTITY;
    for (i, slot) in out.iter_mut().enumerate() {
        let (col, row) = (i / 4, i % 4);
        *slot = (0..4).map(|k| cell(a, k, row) * cell(b, col, k)).sum();
    }
    out
}

fn transform(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = p;
    [0, 1, 2].map(|row| {
        cell(m, 0, row) * x + cell(m, 1, row) * y + cell(m, 2, row) * z + cell(m, 3, row)
    })
}

/// Whether the transform turns the surface inside out (a mirror), so the winding must flip.
fn mirrors(m: &Mat) -> bool {
    let c = |col, row| cell(m, col, row);
    let det = c(0, 0) * (c(1, 1) * c(2, 2) - c(2, 1) * c(1, 2))
        - c(1, 0) * (c(0, 1) * c(2, 2) - c(2, 1) * c(0, 2))
        + c(2, 0) * (c(0, 1) * c(1, 2) - c(1, 1) * c(0, 2));
    det < 0.0
}

struct File<'a> {
    root: &'a Value,
    buffers: Vec<Vec<u8>>,
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// A node's vector of `N` numbers, or `default` when it has none.
fn fixed<const N: usize>(node: &Value, key: &str, default: [f64; N]) -> Result<[f64; N]> {
    match node.get(key) {
        None => Ok(default),
        Some(Value::Array(values)) => {
            let numbers: Option<Vec<f64>> = values.iter().map(number).collect();
            numbers
                .and_then(|n| <[f64; N]>::try_from(n).ok())
                .ok_or_else(|| MeshError::Invalid(format!("a node's {key} needs {N} numbers")))
        }
        Some(_) => invalid(format!("a node's {key} needs {N} numbers")),
    }
}

impl File<'_> {
    fn list(&self, key: &str) -> &[Value] {
        match self.root.get(key) {
            Some(Value::Array(list)) => list,
            _ => &[],
        }
    }

    /// The nodes the scene starts from: the default scene's, the first scene's, or every node
    /// that is nobody's child.
    fn scene_roots(&self) -> Vec<usize> {
        let scenes = self.list("scenes");
        let chosen = self
            .root
            .get("scene")
            .and_then(Value::as_u64)
            .and_then(|i| scenes.get(usize::try_from(i).ok()?))
            .or_else(|| scenes.first());
        if let Some(Value::Array(nodes)) = chosen.and_then(|s| s.get("nodes")) {
            return nodes
                .iter()
                .filter_map(|n| usize::try_from(n.as_u64()?).ok())
                .collect();
        }
        let nodes = self.list("nodes");
        let mut child = vec![false; nodes.len()];
        for node in nodes {
            if let Some(Value::Array(kids)) = node.get("children") {
                for k in kids.iter().filter_map(Value::as_u64) {
                    if let Some(flag) = usize::try_from(k).ok().and_then(|k| child.get_mut(k)) {
                        *flag = true;
                    }
                }
            }
        }
        child
            .iter()
            .enumerate()
            .filter(|(_, is_child)| !**is_child)
            .map(|(i, _)| i)
            .collect()
    }

    fn local(node: &Value) -> Result<Mat> {
        if let Some(Value::Array(values)) = node.get("matrix") {
            let numbers: Vec<f64> = values.iter().filter_map(number).collect();
            return <Mat>::try_from(numbers)
                .map_err(|_| MeshError::Invalid("a node matrix needs sixteen numbers".into()));
        }
        let t = fixed(node, "translation", [0.0; 3])?;
        let q = fixed(node, "rotation", [0.0, 0.0, 0.0, 1.0])?;
        let s = fixed(node, "scale", [1.0; 3])?;
        let norm = q.iter().map(|c| c * c).sum::<f64>().sqrt();
        if norm == 0.0 {
            return invalid("a node's rotation is not a unit quaternion");
        }
        let [x, y, z, w] = q.map(|c| c / norm);
        let [sx, sy, sz] = s;
        let [tx, ty, tz] = t;
        // Rotation matrix columns, each scaled by the node's scale.
        Ok([
            (1.0 - 2.0 * (y * y + z * z)) * sx,
            (2.0 * (x * y + z * w)) * sx,
            (2.0 * (x * z - y * w)) * sx,
            0.0,
            (2.0 * (x * y - z * w)) * sy,
            (1.0 - 2.0 * (x * x + z * z)) * sy,
            (2.0 * (y * z + x * w)) * sy,
            0.0,
            (2.0 * (x * z + y * w)) * sz,
            (2.0 * (y * z - x * w)) * sz,
            (1.0 - 2.0 * (x * x + y * y)) * sz,
            0.0,
            tx,
            ty,
            tz,
            1.0,
        ])
    }

    fn node(
        &self,
        index: usize,
        parent: &Mat,
        depth: usize,
        visits: &mut usize,
        builder: &mut Builder,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return invalid(format!("nodes are nested more than {MAX_DEPTH} deep"));
        }
        *visits += 1;
        if *visits > MAX_VISITS {
            return invalid(format!("the scene visits more than {MAX_VISITS} nodes"));
        }
        let Some(node) = self.list("nodes").get(index) else {
            return invalid(format!("node {index} does not exist"));
        };
        let world = multiply(parent, &Self::local(node)?);
        if let Some(mesh) = node.get("mesh").and_then(Value::as_u64) {
            let Some(mesh_value) = usize::try_from(mesh)
                .ok()
                .and_then(|m| self.list("meshes").get(m).map(|v| (m, v)))
            else {
                return invalid(format!("mesh {mesh} does not exist"));
            };
            let name = node.get("name").and_then(Value::as_str);
            self.mesh(mesh_value.1, mesh_value.0, name, &world, builder)?;
        }
        if let Some(Value::Array(kids)) = node.get("children") {
            for kid in kids {
                let Some(kid) = kid.as_u64().and_then(|k| usize::try_from(k).ok()) else {
                    return invalid("a child node is not an index");
                };
                self.node(kid, &world, depth + 1, visits, builder)?;
            }
        }
        Ok(())
    }

    fn mesh(
        &self,
        mesh: &Value,
        index: usize,
        node_name: Option<&str>,
        world: &Mat,
        builder: &mut Builder,
    ) -> Result<()> {
        let name = node_name
            .or_else(|| mesh.get("name").and_then(Value::as_str))
            .map_or_else(|| format!("mesh {index}"), str::to_owned);
        builder.part(&name);
        let flip = mirrors(world);
        let primitives = match mesh.get("primitives") {
            Some(Value::Array(list)) => list.as_slice(),
            _ => &[],
        };
        for primitive in primitives {
            let mode = primitive.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if !(4..=6).contains(&mode) {
                continue;
            }
            let Some(position) = primitive
                .get("attributes")
                .and_then(|a| a.get("POSITION"))
                .and_then(Value::as_u64)
            else {
                continue;
            };
            let points = self.positions(position)?;
            let order: Vec<u32> = match primitive.get("indices").and_then(Value::as_u64) {
                Some(accessor) => self.indices(accessor)?,
                None => (0..u32::try_from(points.len()).unwrap_or(u32::MAX)).collect(),
            };
            let mut corner = |a: u32, b: u32, c: u32| -> Result<()> {
                let get = |i: u32| {
                    points
                        .get(i as usize)
                        .map(|&p| transform(world, p))
                        .ok_or_else(|| {
                            MeshError::Invalid(format!(
                                "vertex index {i} is outside the {} positions",
                                points.len()
                            ))
                        })
                };
                let (a, b, c) = (get(a)?, get(b)?, get(c)?);
                builder.triangle(if flip { [a, c, b] } else { [a, b, c] })
            };
            match mode {
                4 => {
                    for &[a, b, c] in order.as_chunks::<3>().0 {
                        corner(a, b, c)?;
                    }
                }
                5 => {
                    for (i, w) in order.windows(3).enumerate() {
                        if let [a, b, c] = *w {
                            // Every other triangle of a strip is turned the other way.
                            if i % 2 == 0 {
                                corner(a, b, c)?;
                            } else {
                                corner(b, a, c)?;
                            }
                        }
                    }
                }
                _ => {
                    if let Some((&first, rest)) = order.split_first() {
                        for w in rest.windows(2) {
                            if let [b, c] = *w {
                                corner(first, b, c)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The bytes an accessor covers, its component size, its component count and its stride.
    fn accessor(&self, index: u64) -> Result<(&[u8], usize, usize, usize, usize)> {
        let Some(accessor) = usize::try_from(index)
            .ok()
            .and_then(|i| self.list("accessors").get(i))
        else {
            return invalid(format!("accessor {index} does not exist"));
        };
        if accessor.get("sparse").is_some() {
            return invalid("sparse accessors are not read");
        }
        let count = accessor
            .get("count")
            .and_then(Value::as_u64)
            .and_then(|c| usize::try_from(c).ok())
            .ok_or_else(|| MeshError::Invalid("an accessor has no count".into()))?;
        let kind = accessor.get("componentType").and_then(Value::as_u64);
        let size = match kind {
            Some(5120 | 5121) => 1,
            Some(5122 | 5123) => 2,
            Some(5125 | 5126) => 4,
            _ => return invalid("an accessor has a component type that is not glTF's"),
        };
        let components = match accessor.get("type").and_then(Value::as_str) {
            Some("SCALAR") => 1,
            Some("VEC3") => 3,
            _ => 0,
        };
        let Some(view) = accessor
            .get("bufferView")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .and_then(|v| self.list("bufferViews").get(v))
        else {
            return invalid("an accessor has no buffer view");
        };
        let field = |v: &Value, key: &str| {
            v.get(key)
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
        };
        let buffer = field(view, "buffer")
            .and_then(|b| self.buffers.get(b))
            .ok_or_else(|| {
                MeshError::Invalid("a buffer view names a buffer that is not there".into())
            })?;
        let start = field(view, "byteOffset")
            .unwrap_or(0)
            .checked_add(field(accessor, "byteOffset").unwrap_or(0))
            .ok_or_else(|| MeshError::Invalid("an accessor offset overflows".into()))?;
        let stride = field(view, "byteStride").unwrap_or(0);
        Ok((
            buffer.get(start..).unwrap_or_default(),
            size,
            components,
            stride,
            count,
        ))
    }

    fn positions(&self, accessor: u64) -> Result<Vec<[f64; 3]>> {
        let (bytes, size, components, stride, count) = self.accessor(accessor)?;
        let kind = self
            .list("accessors")
            .get(usize::try_from(accessor).unwrap_or(usize::MAX))
            .and_then(|a| a.get("componentType"))
            .and_then(Value::as_u64);
        if kind != Some(5126) || components != 3 {
            return invalid("positions must be a VEC3 of floats");
        }
        let step = if stride == 0 { 12 } else { stride };
        let mut out = Vec::with_capacity(count.min(1 << 20));
        for i in 0..count {
            let at = i
                .checked_mul(step)
                .ok_or_else(|| MeshError::Invalid("a position offset overflows".into()))?;
            let raw = bytes.get(at..at.saturating_add(size * 3)).ok_or_else(|| {
                MeshError::Invalid("the positions run past the end of their buffer".into())
            })?;
            let mut p = [0.0; 3];
            for (c, slot) in p.iter_mut().enumerate() {
                let b: [u8; 4] = raw
                    .get(c * 4..c * 4 + 4)
                    .and_then(|b| b.try_into().ok())
                    .unwrap_or_default();
                let v = f64::from(f32::from_le_bytes(b));
                if !v.is_finite() {
                    return invalid("a position is not a finite number");
                }
                *slot = v;
            }
            out.push(p);
        }
        Ok(out)
    }

    fn indices(&self, accessor: u64) -> Result<Vec<u32>> {
        let (bytes, size, components, stride, count) = self.accessor(accessor)?;
        if components != 1 {
            return invalid("indices must be a SCALAR");
        }
        let kind = self
            .list("accessors")
            .get(usize::try_from(accessor).unwrap_or(usize::MAX))
            .and_then(|a| a.get("componentType"))
            .and_then(Value::as_u64);
        if !matches!(kind, Some(5121 | 5123 | 5125)) {
            return invalid("indices must be unsigned bytes, shorts or integers");
        }
        let step = if stride == 0 { size } else { stride };
        let mut out = Vec::with_capacity(count.min(1 << 22));
        for i in 0..count {
            let at = i
                .checked_mul(step)
                .ok_or_else(|| MeshError::Invalid("an index offset overflows".into()))?;
            let raw = bytes.get(at..at.saturating_add(size)).ok_or_else(|| {
                MeshError::Invalid("the indices run past the end of their buffer".into())
            })?;
            out.push(match *raw {
                [a] => u32::from(a),
                [a, b] => u32::from(u16::from_le_bytes([a, b])),
                [a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
                _ => return invalid("an index has an odd width"),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eight corners and twelve triangles of a unit cube, as a glTF buffer: floats, then
    /// unsigned-short indices.
    fn cube_buffer() -> Vec<u8> {
        let points: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let faces: [[u16; 4]; 6] = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [1, 2, 6, 5],
            [3, 0, 4, 7],
        ];
        let mut bytes = Vec::new();
        for p in points {
            for c in p {
                bytes.extend(c.to_le_bytes());
            }
        }
        for f in faces {
            for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
                for i in tri {
                    bytes.extend(i.to_le_bytes());
                }
            }
        }
        bytes
    }

    fn encode(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
            for k in 0..4 {
                if k <= chunk.len() {
                    out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// A glTF text with the cube, `nodes` as its node list JSON and `scene` roots.
    fn gltf(uri: &str, nodes: &str, scene: &str) -> String {
        format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":{scene}}}],
            "nodes":{nodes},
            "meshes":[{{"name":"cube","primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":8,"type":"VEC3"}},
              {{"bufferView":1,"componentType":5123,"count":36,"type":"SCALAR"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":96}},
              {{"buffer":0,"byteOffset":96,"byteLength":72}}],
            "buffers":[{{"byteLength":168{uri}}}]}}"#
        )
    }

    fn with_data(nodes: &str, scene: &str) -> String {
        gltf(
            &format!(
                r#","uri":"data:application/octet-stream;base64,{}""#,
                encode(&cube_buffer())
            ),
            nodes,
            scene,
        )
    }

    #[test]
    fn a_cube_in_a_data_uri_is_a_closed_solid() {
        let text = with_data(r#"[{"mesh":0,"name":"box"}]"#, "[0]");
        let stl = parse(text.as_bytes(), None).unwrap();
        assert_eq!(stl.triangles.len(), 12);
        let s = stl.summary();
        assert!(s.closed());
        assert!((s.volume.unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(stl.parts[0].name, "box");
    }

    #[test]
    fn node_transforms_are_applied_down_the_tree() {
        // A parent moved 10 along x, holding a child scaled by two and moved 1 along y.
        let nodes = r#"[{"translation":[10,0,0],"children":[1]},{"mesh":0,"scale":[2,2,2],"translation":[0,1,0]}]"#;
        let stl = parse(with_data(nodes, "[0]").as_bytes(), None).unwrap();
        let s = stl.summary();
        assert!(
            (s.low[0] - 10.0).abs() < 1e-9 && (s.high[0] - 12.0).abs() < 1e-9,
            "{s:?}"
        );
        assert!(
            (s.low[1] - 1.0).abs() < 1e-9 && (s.high[1] - 3.0).abs() < 1e-9,
            "{s:?}"
        );
        assert!((s.volume.unwrap() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn a_turn_and_a_mirror_keep_the_solid_outward() {
        // A quarter turn about z: the cube now lies to the left of the origin.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let nodes = format!(r#"[{{"mesh":0,"rotation":[0,0,{h},{h}]}}]"#);
        let s = parse(with_data(&nodes, "[0]").as_bytes(), None)
            .unwrap()
            .summary();
        assert!(
            (s.low[0] + 1.0).abs() < 1e-9 && s.high[0].abs() < 1e-9,
            "{s:?}"
        );
        assert!((s.volume.unwrap() - 1.0).abs() < 1e-9);
        // Mirrored in x, the winding flips so the volume is still positive.
        let mirrored = parse(
            with_data(r#"[{"mesh":0,"scale":[-1,1,1]}]"#, "[0]").as_bytes(),
            None,
        )
        .unwrap()
        .summary();
        assert!(
            (mirrored.volume.unwrap() - 1.0).abs() < 1e-9,
            "{mirrored:?}"
        );
    }

    #[test]
    fn a_glb_container_and_a_buffer_file_are_read() {
        let json = gltf("", r#"[{"mesh":0}]"#, "[0]");
        let mut json = json.into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let bin = cube_buffer();
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend((total as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(0x4E4F_534Au32.to_le_bytes());
        glb.extend(&json);
        glb.extend((bin.len() as u32).to_le_bytes());
        glb.extend(0x004E_4942u32.to_le_bytes());
        glb.extend(&bin);
        assert!(parse(&glb, None).unwrap().summary().closed());
        // Cut anywhere inside, it is an error and not a panic.
        for cut in 0..glb.len() {
            let _ = parse(&glb[..cut], None);
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cube.bin"), &bin).unwrap();
        let text = gltf(r#","uri":"cube.bin""#, r#"[{"mesh":0}]"#, "[0]");
        let file = dir.path().join("Cube.GLTF");
        std::fs::write(&file, text).unwrap();
        assert!(Stl::load(&file).unwrap().summary().closed());
        let glb_file = dir.path().join("cube.glb");
        std::fs::write(&glb_file, &glb).unwrap();
        assert!(Stl::load(&glb_file).unwrap().summary().closed());
    }

    #[test]
    fn hostile_files_are_refused_not_panicked_on() {
        let good = with_data(r#"[{"mesh":0}]"#, "[0]");
        for (bad, needle) in [
            (good.replace(r#""mesh":0"#, r#""mesh":7"#), "mesh 7"),
            (good.replace(r#""count":36"#, r#""count":3600"#), "past the end"),
            (good.replace(r#""count":8"#, r#""count":800"#), "past the end"),
            (good.replace("5126", "5121"), "floats"),
            (good.replace(r#""POSITION":0"#, r#""POSITION":9"#), "accessor 9"),
            (good.replace("data:application/octet-stream;base64,", "data:text/plain,"), "base64"),
            (good.replace("data:application/octet-stream;base64,", "https://example.org/x.bin#"), "only local files"),
            (good.replace(r#""buffers":[{"byteLength":168"#, r#""extensionsRequired":["KHR_draco_mesh_compression"],"buffers":[{"byteLength":168"#), "extensions"),
            (good.replace(r#""type":"SCALAR""#, r#""type":"VEC3""#), "SCALAR"),
            ("not json".to_owned(), "not a glTF"),
        ] {
            let error = parse(bad.as_bytes(), None).unwrap_err().to_string();
            assert!(error.contains(needle), "{needle}: {error}");
        }
        // Buffer files may not climb out of their folder or be absolute.
        let dir = tempfile::tempdir().unwrap();
        for uri in ["../x.bin", "/etc/passwd", "a/../../b.bin", "missing.bin"] {
            let text = gltf(&format!(r#","uri":"{uri}""#), r#"[{"mesh":0}]"#, "[0]");
            assert!(parse(text.as_bytes(), Some(dir.path())).is_err(), "{uri}");
        }
        // A node that holds itself is cut off, not followed for ever.
        let cycle = with_data(r#"[{"mesh":0,"children":[0]}]"#, "[0]");
        assert!(parse(cycle.as_bytes(), None).is_err());
        // A rotation that is not a rotation.
        let zero = with_data(r#"[{"mesh":0,"rotation":[0,0,0,0]}]"#, "[0]");
        assert!(parse(zero.as_bytes(), None).is_err());
    }
}
