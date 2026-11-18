//! Object headers: the list of messages that describe a group or a dataset.

use crate::cursor::Cur;
use crate::error::{Result, corrupt, unsupported};
use crate::{File, undefined};

/// Messages read from one object header, over all its continuation blocks.
pub(crate) struct Message {
    pub kind: u16,
    pub data: Vec<u8>,
}

pub(crate) const MSG_DATASPACE: u16 = 0x01;
pub(crate) const MSG_LINK_INFO: u16 = 0x02;
pub(crate) const MSG_DATATYPE: u16 = 0x03;
pub(crate) const MSG_LINK: u16 = 0x06;
pub(crate) const MSG_LAYOUT: u16 = 0x08;
pub(crate) const MSG_FILTERS: u16 = 0x0B;
pub(crate) const MSG_ATTRIBUTE: u16 = 0x0C;
pub(crate) const MSG_CONTINUATION: u16 = 0x10;
pub(crate) const MSG_SYMBOL_TABLE: u16 = 0x11;

const MAX_MESSAGES: usize = 20_000;
const MAX_BLOCKS: usize = 2_000;

pub(crate) fn read_object(file: &File, address: u64) -> Result<Vec<Message>> {
    let head = file.read(address, 16)?;
    if head.get(..4) == Some(b"OHDR") {
        read_v2(file, address)
    } else if head.first() == Some(&1) {
        read_v1(file, address, &head)
    } else {
        corrupt("an object header that is neither version 1 nor 2")
    }
}

fn read_v1(file: &File, address: u64, head: &[u8]) -> Result<Vec<Message>> {
    let mut cur = Cur::new(head);
    cur.skip(8)?;
    let size = cur.u32()? as usize;
    let mut blocks = vec![(address + 16, size)];
    let mut out = Vec::new();
    let mut seen = 0;
    while let Some((at, len)) = blocks.pop() {
        seen += 1;
        if seen > MAX_BLOCKS {
            return corrupt("too many header continuation blocks");
        }
        let bytes = file.read(at, len)?;
        let mut cur = Cur::new(&bytes);
        while cur.remaining() >= 8 {
            let kind = cur.u16()?;
            let size = cur.u16()? as usize;
            cur.skip(4)?; // flags and reserved
            let data = cur.take(size)?.to_vec();
            push(&mut out, &mut blocks, file, kind, data)?;
        }
    }
    Ok(out)
}

fn read_v2(file: &File, address: u64) -> Result<Vec<Message>> {
    let head = file.read(address, 64)?;
    let mut cur = Cur::new(&head);
    cur.skip(4)?;
    if cur.u8()? != 2 {
        return corrupt("an object header version");
    }
    let flags = cur.u8()?;
    if flags & 0x20 != 0 {
        cur.skip(16)?;
    }
    if flags & 0x10 != 0 {
        cur.skip(4)?;
    }
    let size = cur.uint(1usize << (flags & 3))? as usize;
    let first = address + cur.pos() as u64;
    let ordered = flags & 0x04 != 0;
    let mut blocks = vec![(first, size, false)];
    let mut out = Vec::new();
    let mut seen = 0;
    while let Some((at, len, continued)) = blocks.pop() {
        seen += 1;
        if seen > MAX_BLOCKS {
            return corrupt("too many header continuation blocks");
        }
        let bytes = file.read(at, len)?;
        let mut cur = Cur::new(&bytes);
        if continued {
            cur.skip(4)?; // OCHK
        }
        let end = if continued {
            len.saturating_sub(4)
        } else {
            len
        };
        let min = 4 + if ordered { 2 } else { 0 };
        while end.saturating_sub(cur.pos()) >= min {
            let kind = u16::from(cur.u8()?);
            let size = cur.u16()? as usize;
            cur.skip(1)?;
            if ordered {
                cur.skip(2)?;
            }
            let data = cur.take(size)?.to_vec();
            let mut next = Vec::new();
            push(&mut out, &mut next, file, kind, data)?;
            blocks.extend(next.into_iter().map(|(a, l)| (a, l, true)));
        }
    }
    Ok(out)
}

fn push(
    out: &mut Vec<Message>,
    blocks: &mut Vec<(u64, usize)>,
    file: &File,
    kind: u16,
    data: Vec<u8>,
) -> Result<()> {
    if out.len() >= MAX_MESSAGES {
        return corrupt("too many header messages");
    }
    if kind == MSG_CONTINUATION {
        let mut cur = Cur::new(&data);
        let at = cur.uint(file.offset_size())?;
        let len = cur.uint(file.length_size())?;
        let len = usize::try_from(len).or_else(|_| corrupt("a continuation block length"))?;
        if undefined(at, file.offset_size()) {
            return unsupported("a header continuation at an undefined address");
        }
        blocks.push((at, len));
    } else if kind != 0 {
        out.push(Message { kind, data });
    }
    Ok(())
}
