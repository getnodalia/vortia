//! Groups: the names an object holds and where each one leads.

use crate::cursor::Cur;
use crate::error::{Result, corrupt, unsupported};
use crate::object::{MSG_LINK, MSG_LINK_INFO, MSG_SYMBOL_TABLE, read_object};
use crate::{File, undefined};

const MAX_NODES: usize = 200_000;

/// The hard links of the group whose header is at `address`: name and object header address.
pub(crate) fn children(file: &File, address: u64) -> Result<Vec<(String, u64)>> {
    let messages = read_object(file, address)?;
    let o = file.offset_size();
    let mut out = Vec::new();
    for message in &messages {
        let mut cur = Cur::new(&message.data);
        match message.kind {
            MSG_SYMBOL_TABLE => {
                let tree = cur.uint(o)?;
                let heap = cur.uint(o)?;
                symbol_table(file, tree, heap, &mut out)?;
            }
            MSG_LINK => {
                if let Some(link) = link(file, &mut cur)? {
                    out.push(link);
                }
            }
            MSG_LINK_INFO => {
                cur.skip(1)?;
                let flags = cur.u8()?;
                if flags & 1 != 0 {
                    cur.skip(8)?;
                }
                let heap = cur.uint(o)?;
                if !undefined(heap, o) {
                    return unsupported("a group with its links in a fractal heap");
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A link message; only hard links lead anywhere this reader follows.
fn link(file: &File, cur: &mut Cur) -> Result<Option<(String, u64)>> {
    if cur.u8()? != 1 {
        return corrupt("a link message version");
    }
    let flags = cur.u8()?;
    let kind = if flags & 0x08 != 0 { cur.u8()? } else { 0 };
    if flags & 0x04 != 0 {
        cur.skip(8)?;
    }
    if flags & 0x10 != 0 {
        cur.skip(1)?;
    }
    let len = cur.uint(1usize << (flags & 3))? as usize;
    let name = String::from_utf8_lossy(cur.take(len)?).into_owned();
    if kind == 0 {
        Ok(Some((name, cur.uint(file.offset_size())?)))
    } else {
        Ok(None)
    }
}

fn symbol_table(file: &File, tree: u64, heap: u64, out: &mut Vec<(String, u64)>) -> Result<()> {
    let (o, l) = (file.offset_size(), file.length_size());
    let head = file.read(heap, 8 + 2 * l + o)?;
    let mut cur = Cur::new(&head);
    if cur.take(4)? != b"HEAP" {
        return corrupt("a local heap");
    }
    cur.skip(4)?;
    let size = usize::try_from(cur.uint(l)?).or_else(|_| corrupt("a heap size"))?;
    cur.skip(l)?;
    let data = cur.uint(o)?;
    let names = file.read(data, size)?;

    let mut nodes = vec![tree];
    let mut seen = 0;
    while let Some(node) = nodes.pop() {
        seen += 1;
        if seen > MAX_NODES {
            return corrupt("too many nodes in a group");
        }
        let head = file.read(node, 8 + 2 * o)?;
        let mut cur = Cur::new(&head);
        if cur.take(4)? != b"TREE" {
            return corrupt("a group B-tree node");
        }
        let kind = cur.u8()?;
        let level = cur.u8()?;
        let used = cur.u16()? as usize;
        if kind != 0 {
            return corrupt("a chunk node inside a group");
        }
        // Keys and children alternate: key, child, key, child, ..., key.
        let body = file.read(node + 8 + 2 * o as u64, l + used * (l + o))?;
        let mut cur = Cur::new(&body);
        cur.skip(l)?;
        for _ in 0..used {
            let child = cur.uint(o)?;
            cur.skip(l)?;
            if level > 0 {
                nodes.push(child);
            } else {
                entries(file, child, &names, out)?;
            }
        }
    }
    Ok(())
}

/// The symbol table entries of one leaf node.
fn entries(file: &File, node: u64, names: &[u8], out: &mut Vec<(String, u64)>) -> Result<()> {
    let o = file.offset_size();
    let head = file.read(node, 8)?;
    let mut cur = Cur::new(&head);
    if cur.take(4)? != b"SNOD" {
        return corrupt("a symbol table node");
    }
    cur.skip(2)?;
    let count = cur.u16()? as usize;
    let each = 2 * o + 24;
    let body = file.read(node + 8, count.saturating_mul(each))?;
    let mut cur = Cur::new(&body);
    for _ in 0..count {
        let name_at = cur.uint(o)? as usize;
        let object = cur.uint(o)?;
        cur.skip(24)?;
        let rest = names.get(name_at..).unwrap_or_default();
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let name = String::from_utf8_lossy(rest.get(..end).unwrap_or_default()).into_owned();
        out.push((name, object));
    }
    Ok(())
}
