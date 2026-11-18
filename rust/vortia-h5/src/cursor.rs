use crate::error::{Result, corrupt};

/// A bounds-checked reader over a byte slice; every read can fail, none can panic.
pub(crate) struct Cur<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn pos(&self) -> usize {
        self.at
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n);
        match end.and_then(|e| self.bytes.get(self.at..e)) {
            Some(s) => {
                self.at += n;
                Ok(s)
            }
            None => corrupt("a structure runs past the end of its block"),
        }
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(self.uint(2)? as u16)
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(self.uint(4)? as u32)
    }

    /// A little-endian unsigned integer of `n` bytes (1 to 8).
    pub fn uint(&mut self, n: usize) -> Result<u64> {
        if !(1..=8).contains(&n) {
            return corrupt(format!("an integer of {n} bytes"));
        }
        let bytes = self.take(n)?;
        Ok(bytes
            .iter()
            .rev()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
    }
}
