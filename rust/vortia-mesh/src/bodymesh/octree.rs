//! The octree the mesh is cut from: a block of equal cells, refined where the body and the
//! requested regions are, then balanced so neighbouring cells differ by at most one level.

use std::collections::HashSet;

use super::grid::{TriGrid, at, put};
use crate::error::{MeshError, Result};

/// One cell: a level (0 is a base cell) and its integer position at that level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct Key {
    pub level: u8,
    pub at: [u32; 3],
}

impl Key {
    fn child(self, octant: usize) -> Self {
        let mut at = [0u32; 3];
        for k in 0..3 {
            put(
                &mut at,
                k,
                at_u(&self.at, k) * 2 + ((octant >> k) & 1) as u32,
            );
        }
        Self {
            level: self.level + 1,
            at,
        }
    }

    fn parent(self) -> Option<Self> {
        (self.level > 0).then(|| {
            let mut at = [0u32; 3];
            for k in 0..3 {
                put(&mut at, k, at_u(&self.at, k) / 2);
            }
            Self {
                level: self.level - 1,
                at,
            }
        })
    }
}

fn at_u(v: &[u32; 3], k: usize) -> u32 {
    at(v, k)
}

/// A box in which cells are refined to at least `level`.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub low: [f64; 3],
    pub high: [f64; 3],
    pub level: u8,
}

/// The block the cells fill and how they map to space.
#[derive(Clone, Debug)]
pub(super) struct Lattice {
    pub low: [f64; 3],
    /// The size of a base cell along each axis.
    pub base_size: [f64; 3],
    pub base: [u32; 3],
    pub max_level: u8,
}

impl Lattice {
    /// The corners of a cell in space.
    pub(super) fn cell_box(&self, k: Key) -> ([f64; 3], [f64; 3]) {
        let scale = 0.5f64.powi(i32::from(k.level));
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for a in 0..3 {
            let size = at(&self.base_size, a) * scale;
            let origin = at(&self.low, a) + f64::from(at_u(&k.at, a)) * size;
            put(&mut lo, a, origin);
            put(&mut hi, a, origin + size);
        }
        (lo, hi)
    }

    /// Whether the integer position is inside the block at this level.
    pub(super) fn in_range(&self, level: u8, p: [i64; 3]) -> bool {
        (0..3).all(|a| {
            let n = i64::from(at(&self.base, a)) << level;
            let x = at(&p, a);
            x >= 0 && x < n
        })
    }
}

pub(super) struct Octree {
    pub lattice: Lattice,
    pub leaves: HashSet<Key>,
}

/// How the cells are refined.
pub(super) struct Refinement<'a> {
    pub surface: &'a TriGrid,
    pub surface_level: u8,
    pub regions: &'a [Region],
    pub max_cells: usize,
}

impl Octree {
    /// Fills the block with base cells and refines it.
    pub(super) fn build(lattice: Lattice, refine: &Refinement<'_>) -> Result<Self> {
        let mut leaves = HashSet::new();
        let mut frontier = Vec::new();
        for z in 0..at(&lattice.base, 2) {
            for y in 0..at(&lattice.base, 1) {
                for x in 0..at(&lattice.base, 0) {
                    let key = Key {
                        level: 0,
                        at: [x, y, z],
                    };
                    leaves.insert(key);
                    frontier.push(key);
                }
            }
        }
        let mut tree = Self { lattice, leaves };
        let mut scratch = Vec::new();
        let target = |tree: &Self, k: Key, scratch: &mut Vec<u32>| {
            let (lo, hi) = tree.lattice.cell_box(k);
            if k.level < refine.surface_level && refine.surface.touches(lo, hi, scratch) {
                return true;
            }
            refine.regions.iter().any(|r| {
                k.level < r.level
                    && (0..3).all(|a| at(&r.low, a) <= at(&hi, a) && at(&r.high, a) >= at(&lo, a))
            })
        };
        for _ in 0..tree.lattice.max_level {
            let mut next = Vec::new();
            for k in frontier {
                if target(&tree, k, &mut scratch) {
                    tree.split(k);
                    next.extend((0..8).map(|o| k.child(o)));
                }
            }
            if tree.leaves.len() > refine.max_cells {
                return Err(MeshError::Invalid(format!(
                    "the mesh would pass {} cells: use a coarser base size or fewer levels",
                    refine.max_cells
                )));
            }
            frontier = next;
        }
        tree.balance(refine.max_cells)?;
        Ok(tree)
    }

    fn split(&mut self, k: Key) {
        if self.leaves.remove(&k) {
            for o in 0..8 {
                self.leaves.insert(k.child(o));
            }
        }
    }

    /// The leaf that covers the cell `level`/`p` (a position at `level`), if one is that large
    /// or larger; `None` means the place is cut into smaller leaves.
    pub(super) fn covering(&self, level: u8, p: [u32; 3]) -> Option<Key> {
        let mut k = Key { level, at: p };
        loop {
            if self.leaves.contains(&k) {
                return Some(k);
            }
            k = k.parent()?;
        }
    }

    /// Splits leaves until no leaf touches one that is two or more levels smaller.
    fn balance(&mut self, max_cells: usize) -> Result<()> {
        loop {
            let mut changed = false;
            let mut fine: Vec<Key> = self
                .leaves
                .iter()
                .copied()
                .filter(|k| k.level >= 2)
                .collect();
            fine.sort_unstable();
            for f in fine {
                if !self.leaves.contains(&f) {
                    continue;
                }
                for axis in 0..3 {
                    for sign in [-1i64, 1] {
                        let mut p = [0i64; 3];
                        for a in 0..3 {
                            put(&mut p, a, i64::from(at(&f.at, a)));
                        }
                        let moved = at(&p, axis) + sign;
                        put(&mut p, axis, moved);
                        if !self.lattice.in_range(f.level, p) {
                            continue;
                        }
                        let q = [p[0] as u32, p[1] as u32, p[2] as u32];
                        if let Some(big) = self.covering(f.level, q)
                            && big.level + 2 <= f.level
                        {
                            self.split(big);
                            changed = true;
                        }
                    }
                }
            }
            if self.leaves.len() > max_cells {
                return Err(MeshError::Invalid(format!(
                    "the mesh would pass {max_cells} cells: use a coarser base size or fewer levels"
                )));
            }
            if !changed {
                return Ok(());
            }
        }
    }

    /// The leaves that share part of the face of `k` in direction `sign` (-1 or +1) along `axis`.
    pub(super) fn across(&self, k: Key, axis: usize, sign: i64) -> Vec<Key> {
        let mut p = [0i64; 3];
        for a in 0..3 {
            put(&mut p, a, i64::from(at(&k.at, a)));
        }
        let moved = at(&p, axis) + sign;
        put(&mut p, axis, moved);
        if !self.lattice.in_range(k.level, p) {
            return Vec::new();
        }
        let q = Key {
            level: k.level,
            at: [p[0] as u32, p[1] as u32, p[2] as u32],
        };
        if let Some(c) = self.covering(q.level, q.at) {
            return vec![c];
        }
        // The place is cut finer: the leaves on the side that touches `k`.
        let touching = usize::from(sign < 0);
        let mut out = Vec::new();
        let mut stack = vec![q];
        while let Some(c) = stack.pop() {
            if self.leaves.contains(&c) {
                out.push(c);
                continue;
            }
            if c.level >= self.lattice.max_level {
                continue;
            }
            for o in 0..8usize {
                if ((o >> axis) & 1) == touching {
                    stack.push(c.child(o));
                }
            }
        }
        out
    }
}
