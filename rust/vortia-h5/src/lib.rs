//! A reader for HDF5 files, in plain Rust (R8 of `docs/architecture/rust-only.md`): the format
//! Fluent's `.cas.h5` and `.dat.h5` files are written in.
//!
//! What is read: superblocks of versions 0 to 3; version 1 and 2 object headers; groups kept as a
//! symbol table (B-tree version 1 and a local heap) or as link messages; datasets stored compact,
//! contiguous or chunked (layout version 3), with the deflate and shuffle filters; integer and
//! floating-point numbers of any size and byte order; fixed-length strings; and attributes. What is
//! not read yet comes back as [`H5Error::Unsupported`] naming it: groups with many links kept in a
//! fractal heap, the chunk indexes of layout version 4, compound and variable-length types.
//!
//! The file is read in pieces as it is asked for, never all at once, and every input is treated as
//! hostile: no panics, sizes read from the file never decide how much memory is reserved, and
//! nesting is bounded.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod cursor;
mod dataset;
mod error;
mod group;
mod object;

pub use dataset::{Attribute, DType, Dataset};
pub use error::{H5Error, Result};

use cursor::Cur;
use error::{corrupt, unsupported};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

/// The most bytes one read from the file may ask for.
pub(crate) const MAX_READ: usize = 1 << 30;
/// The most elements a dataset may be read into memory with.
pub const MAX_ELEMENTS: u64 = 1 << 28;

enum Source {
    Memory(Vec<u8>),
    Disk(Mutex<std::fs::File>, u64),
}

/// An open HDF5 file.
pub struct File {
    source: Source,
    /// Where the superblock starts; addresses in the file count from here.
    origin: u64,
    offset_size: usize,
    length_size: usize,
    root: u64,
}

const SIGNATURE: [u8; 8] = [0x89, b'H', b'D', b'F', 0x0d, 0x0a, 0x1a, 0x0a];

/// Whether `address` is the all-ones value HDF5 uses for "nowhere".
pub(crate) fn undefined(address: u64, size: usize) -> bool {
    match size {
        1..=7 => address == (1u64 << (8 * size)) - 1,
        _ => address == u64::MAX,
    }
}

impl File {
    pub fn open(path: &Path) -> Result<Self> {
        let io = |e: std::io::Error| H5Error::Io(format!("{}: {e}", path.display()));
        let file = std::fs::File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        Self::start(Source::Disk(Mutex::new(file), len))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::start(Source::Memory(bytes))
    }

    fn start(source: Source) -> Result<Self> {
        let mut file = Self {
            source,
            origin: 0,
            offset_size: 8,
            length_size: 8,
            root: 0,
        };
        let mut found = None;
        for origin in [0u64, 512, 1024, 2048, 4096] {
            if let Ok(head) = file.read_raw(origin, 8)
                && head == SIGNATURE
            {
                found = Some(origin);
                break;
            }
        }
        let Some(origin) = found else {
            return Err(H5Error::NotHdf5);
        };
        file.origin = origin;
        let head = file.read_raw(origin, 96)?;
        let mut cur = Cur::new(&head);
        cur.skip(8)?;
        let version = cur.u8()?;
        match version {
            0 | 1 => {
                cur.skip(3)?;
                cur.skip(1)?; // shared header message format
                file.offset_size = cur.u8()? as usize;
                file.length_size = cur.u8()? as usize;
                cur.skip(1 + 2 + 2 + 4)?;
                if version == 1 {
                    cur.skip(4)?;
                }
                file.check_sizes()?;
                cur.skip(3 * file.offset_size + file.offset_size)?; // base, free space, end, driver
                cur.skip(file.offset_size)?; // the root's link name
                file.root = cur.uint(file.offset_size)?;
            }
            2 | 3 => {
                file.offset_size = cur.u8()? as usize;
                file.length_size = cur.u8()? as usize;
                cur.skip(1)?;
                file.check_sizes()?;
                cur.skip(file.offset_size * 3)?; // base, extension, end
                file.root = cur.uint(file.offset_size)?;
            }
            v => return unsupported(format!("a superblock of version {v}")),
        }
        Ok(file)
    }

    fn check_sizes(&self) -> Result<()> {
        let ok = |n: usize| matches!(n, 2 | 4 | 8);
        if ok(self.offset_size) && ok(self.length_size) {
            Ok(())
        } else {
            corrupt("the sizes of offsets and lengths")
        }
    }

    pub(crate) fn offset_size(&self) -> usize {
        self.offset_size
    }

    pub(crate) fn length_size(&self) -> usize {
        self.length_size
    }

    fn read_raw(&self, at: u64, len: usize) -> Result<Vec<u8>> {
        if len > MAX_READ {
            return Err(H5Error::TooLarge(format!("a read of {len} bytes")));
        }
        match &self.source {
            Source::Memory(bytes) => {
                let start = usize::try_from(at).or_else(|_| corrupt("an address"))?;
                let end = start.checked_add(len);
                end.and_then(|e| bytes.get(start..e))
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| H5Error::Corrupt("a read past the end of the file".into()))
            }
            Source::Disk(file, size) => {
                if at.checked_add(len as u64).is_none_or(|end| end > *size) {
                    return corrupt("a read past the end of the file");
                }
                let mut file = file
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut buf = vec![0u8; len];
                file.seek(SeekFrom::Start(at))
                    .and_then(|_| file.read_exact(&mut buf))
                    .map_err(|e| H5Error::Io(e.to_string()))?;
                Ok(buf)
            }
        }
    }

    /// `len` bytes at `address`, counted from the superblock.
    pub(crate) fn read(&self, address: u64, len: usize) -> Result<Vec<u8>> {
        match self.origin.checked_add(address) {
            Some(at) => self.read_raw(at, len),
            None => corrupt("an address"),
        }
    }

    /// The names in the group at `path` (`"/"` is the root), sorted.
    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let address = self.find(path)?;
        let mut names: Vec<String> = group::children(self, address)?
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        names.sort();
        Ok(names)
    }

    /// Whether there is an object at `path`.
    pub fn exists(&self, path: &str) -> bool {
        self.find(path).is_ok()
    }

    /// The dataset at `path`.
    pub fn dataset(&self, path: &str) -> Result<Dataset<'_>> {
        Dataset::open(self, self.find(path)?, path)
    }

    /// The attribute `name` of the group or dataset at `path`.
    pub fn attribute(&self, path: &str, name: &str) -> Result<Attribute> {
        let address = self.find(path)?;
        dataset::attribute(self, address, name)
    }

    /// The attributes of the group or dataset at `path`.
    pub fn attributes(&self, path: &str) -> Result<Vec<Attribute>> {
        dataset::attributes(self, self.find(path)?)
    }

    /// Follows the names of `path` from the root.
    fn find(&self, path: &str) -> Result<u64> {
        let mut at = self.root;
        let mut depth = 0;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            depth += 1;
            if depth > 64 {
                return corrupt("a path that nests too deep");
            }
            let children = group::children(self, at)?;
            at = children
                .into_iter()
                .find(|(name, _)| name == part)
                .map(|(_, a)| a)
                .ok_or_else(|| H5Error::NotFound(path.to_owned()))?;
        }
        Ok(at)
    }
}

#[cfg(test)]
mod tests;
