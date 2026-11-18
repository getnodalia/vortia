//! Datasets and attributes: shape, type, storage, and the numbers themselves.

use crate::cursor::Cur;
use crate::error::{H5Error, Result, corrupt, unsupported};
use crate::object::{
    MSG_ATTRIBUTE, MSG_DATASPACE, MSG_DATATYPE, MSG_FILTERS, MSG_LAYOUT, read_object,
};
use crate::{File, MAX_ELEMENTS, MAX_READ, undefined};
use std::io::Read;

const MAX_CHUNKS: usize = 5_000_000;
const MAX_NODES: usize = 1_000_000;

/// The type of the elements of a dataset or an attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    Int {
        size: usize,
        signed: bool,
        big: bool,
    },
    Float {
        size: usize,
        big: bool,
    },
    /// A fixed-length string of `size` bytes.
    Str {
        size: usize,
    },
    /// A type this reader does not decode; its elements are `size` bytes.
    Other {
        class: u8,
        size: usize,
    },
}

impl DType {
    pub fn size(self) -> usize {
        match self {
            DType::Int { size, .. }
            | DType::Float { size, .. }
            | DType::Str { size }
            | DType::Other { size, .. } => size,
        }
    }

    fn parse(bytes: &[u8]) -> Result<Self> {
        let mut cur = Cur::new(bytes);
        let class = cur.u8()? & 0x0f;
        let bits = cur.take(3)?;
        let first = bits.first().copied().unwrap_or(0);
        let size = cur.u32()? as usize;
        Ok(match class {
            0 => DType::Int {
                size,
                signed: first & 0x08 != 0,
                big: first & 0x01 != 0,
            },
            1 => DType::Float {
                size,
                big: first & 0x01 != 0,
            },
            3 => DType::Str { size },
            // An enumeration is stored as its base integer type.
            8 => DType::parse(bytes.get(8..).unwrap_or_default())?,
            class => DType::Other { class, size },
        })
    }
}

/// The dimensions of a dataset; empty for a single value.
struct Space {
    dims: Vec<u64>,
    null: bool,
}

impl Space {
    fn parse(bytes: &[u8], length_size: usize) -> Result<Self> {
        let mut cur = Cur::new(bytes);
        let version = cur.u8()?;
        let rank = cur.u8()? as usize;
        let flags = cur.u8()?;
        let null = match version {
            1 => {
                cur.skip(5)?;
                false
            }
            2 => cur.u8()? == 2,
            v => return unsupported(format!("a dataspace of version {v}")),
        };
        if rank > 32 {
            return corrupt("a dataspace of more than 32 dimensions");
        }
        let _ = flags;
        let dims = (0..rank)
            .map(|_| cur.uint(length_size))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { dims, null })
    }

    fn count(&self) -> Result<u64> {
        if self.null {
            return Ok(0);
        }
        self.dims.iter().try_fold(1u64, |n, &d| {
            n.checked_mul(d)
                .filter(|&n| n <= MAX_ELEMENTS)
                .ok_or_else(|| H5Error::TooLarge("a dataset with too many elements".into()))
        })
    }
}

enum Layout {
    Compact(Vec<u8>),
    Contiguous {
        address: u64,
    },
    /// The B-tree of the chunks and the chunk's dimensions, without the element size.
    Chunked {
        tree: u64,
        chunk: Vec<u64>,
    },
}

struct Filter {
    id: u16,
    values: Vec<u32>,
}

/// A dataset in an open file.
pub struct Dataset<'f> {
    file: &'f File,
    pub shape: Vec<u64>,
    pub dtype: DType,
    count: u64,
    layout: Layout,
    filters: Vec<Filter>,
}

impl<'f> Dataset<'f> {
    pub(crate) fn open(file: &'f File, address: u64, path: &str) -> Result<Self> {
        let (o, l) = (file.offset_size(), file.length_size());
        let (mut space, mut dtype, mut layout) = (None, None, None);
        let mut filters = Vec::new();
        for message in read_object(file, address)? {
            match message.kind {
                MSG_DATASPACE => space = Some(Space::parse(&message.data, l)?),
                MSG_DATATYPE => dtype = Some(DType::parse(&message.data)?),
                MSG_LAYOUT => layout = Some(parse_layout(&message.data, o, l)?),
                MSG_FILTERS => filters = parse_filters(&message.data)?,
                _ => {}
            }
        }
        match (space, dtype, layout) {
            (Some(space), Some(dtype), Some(layout)) => Ok(Self {
                file,
                count: space.count()?,
                shape: space.dims,
                dtype,
                layout,
                filters,
            }),
            _ => Err(H5Error::NotFound(format!("{path} is not a dataset"))),
        }
    }

    /// The number of elements.
    pub fn len(&self) -> u64 {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The elements as stored, `len() * dtype.size()` bytes in row-major order.
    pub fn raw(&self) -> Result<Vec<u8>> {
        let size = self.dtype.size();
        let total = usize::try_from(self.count)
            .ok()
            .and_then(|n| n.checked_mul(size))
            .filter(|&t| t <= MAX_READ)
            .ok_or_else(|| H5Error::TooLarge("a dataset of more than 1 GiB".into()))?;
        match &self.layout {
            Layout::Compact(data) => {
                let mut data = data.clone();
                data.resize(total, 0);
                Ok(data)
            }
            Layout::Contiguous { address } => {
                if undefined(*address, self.file.offset_size()) {
                    Ok(vec![0; total])
                } else {
                    self.file.read(*address, total)
                }
            }
            Layout::Chunked { tree, chunk } => self.chunked(*tree, chunk, total),
        }
    }

    fn chunked(&self, tree: u64, chunk: &[u64], total: usize) -> Result<Vec<u8>> {
        let rank = self.shape.len();
        if rank == 0 || chunk.len() != rank {
            return corrupt("the chunk shape does not match the dataset");
        }
        let elt = self.dtype.size();
        let chunk_bytes = chunk
            .iter()
            .try_fold(elt as u64, |n, &d| n.checked_mul(d))
            .filter(|&n| n > 0 && n <= MAX_READ as u64)
            .ok_or_else(|| H5Error::TooLarge("a chunk of more than 1 GiB".into()))?
            as usize;
        let mut out = vec![0u8; total];
        if undefined(tree, self.file.offset_size()) {
            // Nothing was ever written: the dataset reads as its fill, zeros.
            return Ok(out);
        }
        for found in self.chunks(tree, rank)? {
            let data = self.file.read(found.address, found.size)?;
            let data = unfilter(data, found.mask, &self.filters, elt, chunk_bytes)?;
            place(&mut out, &self.shape, chunk, &found.offset, &data, elt);
        }
        Ok(out)
    }

    /// Every chunk the B-tree of a chunked dataset lists.
    fn chunks(&self, tree: u64, rank: usize) -> Result<Vec<Chunk>> {
        let (o, l) = (self.file.offset_size(), self.file.length_size());
        let key_len = 8 + 8 * (rank + 1);
        let mut out = Vec::new();
        let mut nodes = vec![tree];
        let mut seen = 0;
        while let Some(node) = nodes.pop() {
            seen += 1;
            if seen > MAX_NODES {
                return corrupt("too many nodes in a chunk index");
            }
            let head = self.file.read(node, 8 + 2 * o)?;
            let mut cur = Cur::new(&head);
            if cur.take(4)? != b"TREE" {
                return corrupt("a chunk B-tree node");
            }
            if cur.u8()? != 1 {
                return corrupt("a group node inside a chunk index");
            }
            let level = cur.u8()?;
            let used = cur.u16()? as usize;
            let body = self
                .file
                .read(node + 8 + 2 * o as u64, used * (key_len + o) + key_len)?;
            let mut cur = Cur::new(&body);
            for _ in 0..used {
                let size = cur.u32()? as usize;
                let mask = cur.u32()?;
                let offset = (0..rank).map(|_| cur.uint(8)).collect::<Result<Vec<_>>>()?;
                cur.skip(8)?; // the offset along the element size, always 0
                let address = cur.uint(o)?;
                if level > 0 {
                    nodes.push(address);
                } else {
                    if out.len() >= MAX_CHUNKS {
                        return corrupt("too many chunks");
                    }
                    out.push(Chunk {
                        size,
                        mask,
                        offset,
                        address,
                    });
                }
            }
        }
        let _ = l;
        Ok(out)
    }

    /// The numbers as `f64`, whatever integer or floating-point type they are stored as.
    pub fn read_f64(&self) -> Result<Vec<f64>> {
        decode_f64(self.dtype, &self.raw()?)
    }

    /// The integers as `i64`.
    pub fn read_i64(&self) -> Result<Vec<i64>> {
        decode_i64(self.dtype, &self.raw()?)
    }

    /// Fixed-length strings, cut at their first NUL.
    pub fn read_strings(&self) -> Result<Vec<String>> {
        decode_strings(self.dtype, &self.raw()?)
    }
}

struct Chunk {
    size: usize,
    mask: u32,
    offset: Vec<u64>,
    address: u64,
}

fn parse_layout(bytes: &[u8], o: usize, l: usize) -> Result<Layout> {
    let mut cur = Cur::new(bytes);
    let version = cur.u8()?;
    if version != 3 && version != 4 {
        return unsupported(format!("a storage layout of version {version}"));
    }
    match cur.u8()? {
        0 => {
            let size = cur.u16()? as usize;
            Ok(Layout::Compact(cur.take(size)?.to_vec()))
        }
        1 => {
            let address = cur.uint(o)?;
            cur.skip(l)?; // the size, which the dataspace and type already give
            Ok(Layout::Contiguous { address })
        }
        2 if version == 3 => {
            let dimensionality = cur.u8()? as usize;
            let tree = cur.uint(o)?;
            let mut dims = (0..dimensionality)
                .map(|_| cur.u32().map(u64::from))
                .collect::<Result<Vec<_>>>()?;
            dims.pop(); // the element size
            Ok(Layout::Chunked { tree, chunk: dims })
        }
        2 => unsupported("the chunk index of storage layout version 4"),
        c => unsupported(format!("a storage layout of class {c}")),
    }
}

fn parse_filters(bytes: &[u8]) -> Result<Vec<Filter>> {
    let mut cur = Cur::new(bytes);
    let version = cur.u8()?;
    let count = cur.u8()? as usize;
    if version == 1 {
        cur.skip(6)?;
    } else if version != 2 {
        return unsupported(format!("a filter pipeline of version {version}"));
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let id = cur.u16()?;
        let name_len = if version == 1 || id >= 256 {
            cur.u16()? as usize
        } else {
            0
        };
        cur.skip(2)?; // flags
        let n = cur.u16()? as usize;
        cur.skip(if version == 1 {
            name_len.div_ceil(8) * 8
        } else {
            name_len
        })?;
        let values = (0..n).map(|_| cur.u32()).collect::<Result<Vec<_>>>()?;
        if version == 1 && n % 2 == 1 {
            cur.skip(4)?;
        }
        out.push(Filter { id, values });
    }
    Ok(out)
}

/// Undoes the filters of a chunk, last first, except the ones its mask says were skipped.
fn unfilter(
    mut data: Vec<u8>,
    mask: u32,
    filters: &[Filter],
    elt: usize,
    limit: usize,
) -> Result<Vec<u8>> {
    for (i, filter) in filters.iter().enumerate().rev() {
        if i < 32 && mask & (1 << i) != 0 {
            continue;
        }
        data = match filter.id {
            1 => {
                let mut out = Vec::new();
                flate2::read::ZlibDecoder::new(data.as_slice())
                    .take(limit as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| H5Error::Corrupt(format!("a deflated chunk: {e}")))?;
                if out.len() > limit {
                    return corrupt("a chunk that inflates past its declared size");
                }
                out
            }
            2 => {
                let size = filter.values.first().map_or(elt, |&v| v as usize).max(1);
                unshuffle(&data, size)
            }
            id => return unsupported(format!("the filter {id}")),
        };
    }
    Ok(data)
}

/// Undoes the shuffle filter: byte `k` of every element was stored together.
fn unshuffle(data: &[u8], size: usize) -> Vec<u8> {
    let n = data.len() / size;
    let mut out = vec![0u8; data.len()];
    for i in 0..n {
        for k in 0..size {
            if let (Some(slot), Some(&b)) = (out.get_mut(i * size + k), data.get(k * n + i)) {
                *slot = b;
            }
        }
    }
    let done = n * size;
    if let (Some(dst), Some(src)) = (out.get_mut(done..), data.get(done..)) {
        dst.copy_from_slice(src);
    }
    out
}

/// Copies one decoded chunk into the dataset's bytes, cutting what falls outside the shape.
fn place(out: &mut [u8], shape: &[u64], chunk: &[u64], offset: &[u64], data: &[u8], elt: usize) {
    let rank = shape.len();
    let (Some(&last_chunk), Some(&last_shape), Some(&last_off)) =
        (chunk.last(), shape.last(), offset.last())
    else {
        return;
    };
    if last_off >= last_shape {
        return;
    }
    let run = last_chunk.min(last_shape - last_off) as usize * elt;
    let mut strides = vec![1u64; rank];
    for d in (0..rank.saturating_sub(1)).rev() {
        let next = strides.get(d + 1).copied().unwrap_or(1);
        let extent = shape.get(d + 1).copied().unwrap_or(1);
        if let Some(s) = strides.get_mut(d) {
            *s = next.saturating_mul(extent);
        }
    }
    let outer: u64 = chunk.iter().take(rank - 1).product();
    for row in 0..outer {
        let mut rest = row;
        let mut at = last_off;
        let mut inside = true;
        for d in (0..rank - 1).rev() {
            let extent = chunk.get(d).copied().unwrap_or(1).max(1);
            let coord = offset.get(d).copied().unwrap_or(0) + rest % extent;
            rest /= extent;
            if coord >= shape.get(d).copied().unwrap_or(0) {
                inside = false;
                break;
            }
            at = at.saturating_add(coord.saturating_mul(strides.get(d).copied().unwrap_or(1)));
        }
        if !inside {
            continue;
        }
        let dst = at as usize * elt;
        let src = (row * last_chunk) as usize * elt;
        if let (Some(to), Some(from)) = (out.get_mut(dst..dst + run), data.get(src..src + run)) {
            to.copy_from_slice(from);
        }
    }
}

fn uint(bytes: &[u8], big: bool) -> u64 {
    let fold = |acc: u64, &b: &u8| (acc << 8) | u64::from(b);
    if big {
        bytes.iter().fold(0, fold)
    } else {
        bytes.iter().rev().fold(0, fold)
    }
}

fn decode_f64(dtype: DType, raw: &[u8]) -> Result<Vec<f64>> {
    match dtype {
        DType::Float { size, big } if size == 8 || size == 4 => Ok(raw
            .chunks_exact(size)
            .map(|c| {
                let bits = uint(c, big);
                if size == 8 {
                    f64::from_bits(bits)
                } else {
                    f64::from(f32::from_bits(bits as u32))
                }
            })
            .collect()),
        DType::Int { size, signed, big } if (1..=8).contains(&size) => Ok(raw
            .chunks_exact(size)
            .map(|c| {
                let v = uint(c, big);
                if signed {
                    sign_extend(v, size) as f64
                } else {
                    v as f64
                }
            })
            .collect()),
        other => unsupported(format!("reading {other:?} as numbers")),
    }
}

fn sign_extend(v: u64, size: usize) -> i64 {
    let shift = 64 - 8 * size as u32;
    ((v << shift) as i64) >> shift
}

fn decode_i64(dtype: DType, raw: &[u8]) -> Result<Vec<i64>> {
    match dtype {
        DType::Int { size, signed, big } if (1..=8).contains(&size) => raw
            .chunks_exact(size)
            .map(|c| {
                let v = uint(c, big);
                if signed {
                    Ok(sign_extend(v, size))
                } else {
                    i64::try_from(v)
                        .map_err(|_| H5Error::Corrupt("an integer that does not fit i64".into()))
                }
            })
            .collect(),
        other => unsupported(format!("reading {other:?} as integers")),
    }
}

fn decode_strings(dtype: DType, raw: &[u8]) -> Result<Vec<String>> {
    match dtype {
        DType::Str { size } if size > 0 => Ok(raw
            .chunks_exact(size)
            .map(|c| {
                let end = c.iter().position(|&b| b == 0).unwrap_or(c.len());
                String::from_utf8_lossy(c.get(..end).unwrap_or_default())
                    .trim_end()
                    .to_owned()
            })
            .collect()),
        other => unsupported(format!("reading {other:?} as text")),
    }
}

/// A named value attached to a group or a dataset.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub dtype: DType,
    pub shape: Vec<u64>,
    data: Vec<u8>,
}

impl Attribute {
    pub fn read_f64(&self) -> Result<Vec<f64>> {
        decode_f64(self.dtype, &self.data)
    }

    pub fn read_i64(&self) -> Result<Vec<i64>> {
        decode_i64(self.dtype, &self.data)
    }

    pub fn read_strings(&self) -> Result<Vec<String>> {
        decode_strings(self.dtype, &self.data)
    }
}

pub(crate) fn attributes(file: &File, address: u64) -> Result<Vec<Attribute>> {
    let l = file.length_size();
    read_object(file, address)?
        .iter()
        .filter(|m| m.kind == MSG_ATTRIBUTE)
        .map(|m| parse_attribute(&m.data, l))
        .collect()
}

pub(crate) fn attribute(file: &File, address: u64, name: &str) -> Result<Attribute> {
    attributes(file, address)?
        .into_iter()
        .find(|a| a.name == name)
        .ok_or_else(|| H5Error::NotFound(format!("attribute {name}")))
}

fn parse_attribute(bytes: &[u8], l: usize) -> Result<Attribute> {
    let mut cur = Cur::new(bytes);
    let version = cur.u8()?;
    if !(1..=3).contains(&version) {
        return unsupported(format!("an attribute of version {version}"));
    }
    cur.skip(1)?; // reserved, or flags
    let name_len = cur.u16()? as usize;
    let type_len = cur.u16()? as usize;
    let space_len = cur.u16()? as usize;
    if version == 3 {
        cur.skip(1)?; // character set
    }
    let pad = |n: usize| if version == 1 { n.div_ceil(8) * 8 } else { n };
    let name_bytes = cur.take(pad(name_len))?;
    let name = name_bytes.get(..name_len).unwrap_or_default();
    let name_end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    let name = String::from_utf8_lossy(name.get(..name_end).unwrap_or_default()).into_owned();
    let dtype = DType::parse(cur.take(pad(type_len))?.get(..type_len).unwrap_or_default())?;
    let space = Space::parse(
        cur.take(pad(space_len))?
            .get(..space_len)
            .unwrap_or_default(),
        l,
    )?;
    let count = space.count()? as usize;
    let want = count.saturating_mul(dtype.size());
    let rest = cur.take(cur.remaining())?;
    Ok(Attribute {
        name,
        dtype,
        shape: space.dims,
        data: rest
            .get(..want.min(rest.len()))
            .unwrap_or_default()
            .to_vec(),
    })
}
