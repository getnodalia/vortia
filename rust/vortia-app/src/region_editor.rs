//! The sketch behind "New > Drawn region" (R2/R3 of `docs/architecture/rust-only.md`): a closed
//! outline to dimension, whose edges carry names that say what each one does (`inlet`, `outlet`,
//! `symmetry`, anything else a wall). The state is plain data so everything but the drawing is
//! tested without a window.

use std::path::Path;

use vortia_sketch::{DEFAULT_PATCH, Sketch, SketchError, templates};

use crate::i18n::tr;
use crate::sketch_editor::{Dimension, dimensions_of, with_dimension};

/// The outlines the editor starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    /// A channel with a square block standing in it.
    ChannelWithBlock,
}

impl Outline {
    pub const ALL: [Outline; 1] = [Outline::ChannelWithBlock];

    pub fn title(self) -> &'static str {
        match self {
            Outline::ChannelWithBlock => tr("Channel with a block"),
        }
    }

    fn sketch(self) -> Sketch {
        match self {
            Outline::ChannelWithBlock => {
                templates::channel_with_block(400.0, 100.0, 30.0, 120.0, 35.0)
            }
        }
    }
}

/// One edge of the region, drawn: its points in millimetres and the patch it belongs to.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnEdge {
    pub patch: String,
    pub points: Vec<(f64, f64)>,
}

/// What an edge does, which its name decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Inlet,
    Outlet,
    Symmetry,
    Wall,
}

impl Role {
    pub fn of(patch: &str) -> Self {
        match patch {
            "inlet" => Role::Inlet,
            "outlet" => Role::Outlet,
            "symmetry" => Role::Symmetry,
            _ => Role::Wall,
        }
    }
}

/// A closed sketch that is always solved and always reads as a region: an edit that cannot be
/// is refused and leaves it as it was.
#[derive(Clone, Debug)]
pub struct RegionEditor {
    pub outline: Outline,
    sketch: Sketch,
    /// Why the last edit was refused, until the next one that works.
    pub problem: Option<String>,
}

impl Default for RegionEditor {
    fn default() -> Self {
        Self::new(Outline::ChannelWithBlock)
    }
}

impl RegionEditor {
    pub fn new(outline: Outline) -> Self {
        let mut sketch = outline.sketch();
        let problem = sketch
            .solve()
            .and_then(|_| sketch.loops(5.0).map(|_| ()))
            .err()
            .map(|e| e.to_string());
        Self {
            outline,
            sketch,
            problem,
        }
    }

    pub fn sketch(&self) -> &Sketch {
        &self.sketch
    }

    pub fn restart(&mut self, outline: Outline) {
        *self = Self::new(outline);
    }

    pub fn dimensions(&self) -> Vec<Dimension> {
        dimensions_of(&self.sketch)
    }

    /// Gives dimension `index` a new value and solves again.
    pub fn set_dimension(&mut self, index: usize, value: f64) -> Result<(), SketchError> {
        let done = with_dimension(&self.sketch, index, value).and_then(|mut next| {
            next.solve()?;
            next.loops(5.0)?;
            Ok(next)
        });
        self.keep(done)
    }

    /// The patch of every edge, by the entity it belongs to.
    pub fn names(&self) -> Vec<(usize, String)> {
        (0..self.sketch.entities.len())
            .map(|i| {
                let name = self
                    .sketch
                    .names
                    .get(&i)
                    .map_or(DEFAULT_PATCH, String::as_str);
                (i, name.to_owned())
            })
            .collect()
    }

    /// Gives the edge of `entity` the patch `name`; an empty name puts it back in `wall`.
    pub fn rename(&mut self, entity: usize, name: &str) -> Result<(), SketchError> {
        let name = name.trim();
        let mut next = self.sketch.clone();
        if entity >= next.entities.len() {
            let e = SketchError::Invalid(format!("edge {entity} is not in the sketch"));
            self.problem = Some(e.to_string());
            return Err(e);
        }
        if name.is_empty() || name == DEFAULT_PATCH {
            next.names.remove(&entity);
        } else {
            next.name(entity, name);
        }
        let done = next.validate().map(|()| next);
        self.keep(done)
    }

    fn keep(&mut self, next: Result<Sketch, SketchError>) -> Result<(), SketchError> {
        match next {
            Ok(next) => {
                self.sketch = next;
                self.problem = None;
                Ok(())
            }
            Err(e) => {
                self.problem = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// The edges to draw, loop after loop; the first loop is the outer boundary.
    pub fn drawing(&self) -> Vec<Vec<DrawnEdge>> {
        self.sketch
            .loops(5.0)
            .map(|loops| {
                loops
                    .into_iter()
                    .map(|l| {
                        l.into_iter()
                            .map(|e| DrawnEdge {
                                patch: e.patch,
                                points: e.points,
                            })
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the region has what a case needs: somewhere the fluid comes in and somewhere it
    /// leaves.
    pub fn missing(&self) -> Option<&'static str> {
        let has = |role| self.names().iter().any(|(_, n)| Role::of(n) == role);
        if !has(Role::Inlet) {
            Some(tr("Name one edge inlet: the fluid comes in there."))
        } else if !has(Role::Outlet) {
            Some(tr("Name one edge outlet: the fluid leaves there."))
        } else {
            None
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), SketchError> {
        self.sketch.save(path)
    }
}

/// The box that holds the drawing: `(x0, x1, y0, y1)`.
pub fn bounds(drawing: &[Vec<DrawnEdge>]) -> Option<(f64, f64, f64, f64)> {
    let mut it = drawing
        .iter()
        .flatten()
        .flat_map(|e| e.points.iter().copied());
    let (x, y) = it.next()?;
    Some(it.fold((x, x, y, y), |(x0, x1, y0, y1), (x, y)| {
        (x0.min(x), x1.max(x), y0.min(y), y1.max(y))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_is_solved_and_reads_as_a_region_with_an_inlet_and_an_outlet() {
        let editor = RegionEditor::default();
        assert!(editor.problem.is_none(), "{:?}", editor.problem);
        assert!(!editor.dimensions().is_empty());
        let drawing = editor.drawing();
        assert_eq!(drawing.len(), 2, "the channel and the block");
        assert_eq!(editor.missing(), None);
        let (x0, x1, y0, y1) = bounds(&drawing).unwrap();
        assert!(x1 - x0 > 300.0 && y1 - y0 > 50.0);
    }

    #[test]
    fn a_dimension_changed_moves_the_block_and_a_bad_one_is_refused() {
        let mut editor = RegionEditor::default();
        let before = editor.sketch().clone();
        let block_x = editor
            .dimensions()
            .into_iter()
            .find(|d| (d.value - 120.0).abs() < 1e-9)
            .unwrap();
        editor.set_dimension(block_x.index, 200.0).unwrap();
        let moved = editor
            .sketch()
            .points
            .iter()
            .any(|p| (p.x - 200.0).abs() < 1e-6);
        assert!(moved);
        let kept = editor.sketch().clone();
        assert!(editor.set_dimension(block_x.index, f64::NAN).is_err());
        assert!(editor.problem.is_some());
        assert_eq!(editor.sketch(), &kept);
        assert_ne!(&kept, &before);
        assert!(editor.set_dimension(999, 1.0).is_err());
    }

    #[test]
    fn renaming_an_edge_changes_what_it_does_and_missing_says_what_is_left() {
        let mut editor = RegionEditor::default();
        let (inlet, _) = editor
            .names()
            .into_iter()
            .find(|(_, n)| n == "inlet")
            .unwrap();
        editor.rename(inlet, "wall").unwrap();
        assert!(editor.missing().unwrap().contains("inlet"));
        editor.rename(inlet, "  inlet ").unwrap();
        assert_eq!(editor.missing(), None);
        // A name OpenFOAM could not use is refused and the sketch stays.
        let kept = editor.sketch().clone();
        assert!(editor.rename(inlet, "a b/c").is_err());
        assert!(editor.problem.is_some());
        assert_eq!(editor.sketch(), &kept);
        assert!(editor.rename(999, "x").is_err());
    }

    #[test]
    fn the_drawing_is_saved_and_read_back() {
        let editor = RegionEditor::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("region.vortia-sketch");
        editor.save(&path).unwrap();
        assert_eq!(&Sketch::open(&path).unwrap(), editor.sketch());
    }
}
