//! The sketch behind "New > Drawn region" (R2/R3 of `docs/architecture/rust-only.md`): a closed
//! outline to dimension, whose edges carry names that say what each one does (`inlet`, `outlet`,
//! `symmetry`, anything else a wall). The state is plain data so everything but the drawing is
//! tested without a window.

use std::path::Path;

use vortia_sketch::{DEFAULT_PATCH, Entity, Measure, PointId, Sketch, SketchError, templates};

use crate::i18n::tr;
use crate::sketch_editor::{Dimension, dimensions_of, with_dimension};

/// The outlines the editor starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    /// A channel with a square block standing in it.
    ChannelWithBlock,
    /// Nothing: the outline is drawn from scratch.
    Blank,
}

impl Outline {
    pub const ALL: [Outline; 2] = [Outline::ChannelWithBlock, Outline::Blank];

    pub fn title(self) -> &'static str {
        match self {
            Outline::ChannelWithBlock => tr("Channel with a block"),
            Outline::Blank => tr("Blank"),
        }
    }

    fn sketch(self) -> Sketch {
        match self {
            Outline::ChannelWithBlock => {
                templates::channel_with_block(400.0, 100.0, 30.0, 120.0, 35.0)
            }
            Outline::Blank => Sketch::default(),
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

/// What a click on the drawing does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Drag a point; one a dimension holds comes back to where it is held.
    Move,
    /// Click where a line starts and where it ends; the next click goes on from the end.
    Line,
    /// Click the centre, where the arc starts and the direction of its end, counter-clockwise.
    Arc,
    /// Click two points to hold them at the measure they have.
    Dimension,
    /// Click an edge to delete it.
    Delete,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Move,
        Tool::Line,
        Tool::Arc,
        Tool::Dimension,
        Tool::Delete,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tool::Move => tr("Move"),
            Tool::Line => tr("Line"),
            Tool::Arc => tr("Arc"),
            Tool::Dimension => tr("Dimension"),
            Tool::Delete => tr("Delete"),
        }
    }

    /// What the next click is for, in words.
    pub fn hint(self, picked: usize) -> &'static str {
        match (self, picked) {
            (Tool::Move, _) => tr("Drag a point."),
            (Tool::Line, 0) => tr("Click where the line starts."),
            (Tool::Line, _) => tr("Click where it ends; Escape stops."),
            (Tool::Arc, 0) => tr("Click the centre of the arc."),
            (Tool::Arc, 1) => tr("Click where the arc starts."),
            (Tool::Arc, _) => tr("Click towards where it ends, counter-clockwise."),
            (Tool::Dimension, 0) => tr("Click the first point."),
            (Tool::Dimension, _) => tr("Click the second point."),
            (Tool::Delete, _) => tr("Click an edge to delete it."),
        }
    }
}

/// A sketch that is always solved and whose edits are whole or refused. It reads as a region once
/// its outline is closed; until then it can still be drawn and dimensioned.
#[derive(Clone, Debug)]
pub struct RegionEditor {
    pub outline: Outline,
    sketch: Sketch,
    /// Why the last edit was refused, until the next one that works.
    pub problem: Option<String>,
    pub tool: Tool,
    /// How a dimension measures: along the line between the points, or along an axis.
    pub measure: Measure,
    /// Points are put on this grid, in millimetres; `0` for no grid.
    pub grid: f64,
    /// What the drawing tool has been given so far, in millimetres.
    picked: Vec<(f64, f64)>,
    /// The points a dimension was given.
    picked_points: Vec<PointId>,
    dragging: Option<PointId>,
    /// The box the canvas shows, `(x0, x1, y0, y1)` in millimetres: it only grows as the drawing
    /// does, so what is drawn does not shift under the pointer.
    pub view: (f64, f64, f64, f64),
}

impl Default for RegionEditor {
    fn default() -> Self {
        Self::new(Outline::ChannelWithBlock)
    }
}

impl RegionEditor {
    pub fn new(outline: Outline) -> Self {
        let mut sketch = outline.sketch();
        let problem = sketch.solve().err().map(|e| e.to_string());
        let mut editor = Self {
            outline,
            sketch,
            problem,
            tool: Tool::Move,
            measure: Measure::Distance,
            grid: 5.0,
            picked: Vec::new(),
            picked_points: Vec::new(),
            dragging: None,
            view: (0.0, 400.0, 0.0, 200.0),
        };
        editor.fit();
        editor
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

    /// Every edge as it is drawn, closed outline or not.
    pub fn drawing(&self) -> Vec<DrawnEdge> {
        self.sketch
            .edges(5.0)
            .map(|edges| {
                edges
                    .into_iter()
                    .map(|e| DrawnEdge {
                        patch: e.patch,
                        points: e.points,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Why the sketch is not a region yet, or `None` when it is.
    pub fn open_because(&self) -> Option<String> {
        self.sketch.loops(5.0).err().map(|e| e.to_string())
    }

    /// The points of the sketch, with whether each is fixed.
    pub fn points(&self) -> Vec<(f64, f64, bool)> {
        self.sketch
            .points
            .iter()
            .map(|p| (p.x, p.y, p.fixed))
            .collect()
    }

    /// What the drawing tool has been given: where to draw a rubber band from.
    pub fn picked(&self) -> &[(f64, f64)] {
        &self.picked
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.cancel();
    }

    /// Forgets what the tool was given.
    pub fn cancel(&mut self) {
        self.picked.clear();
        self.picked_points.clear();
        self.dragging = None;
    }

    /// Shows the whole drawing, with room around it to draw more.
    pub fn fit(&mut self) {
        let edges = self.drawing();
        let (x0, x1, y0, y1) = bounds(&edges).unwrap_or((0.0, 400.0, 0.0, 200.0));
        let (w, h) = ((x1 - x0).max(50.0), (y1 - y0).max(50.0));
        self.view = (x0 - 0.15 * w, x1 + 0.15 * w, y0 - 0.15 * h, y1 + 0.15 * h);
    }

    /// Where a click at `(x, y)` lands: on the point of the sketch within `tolerance`, else on
    /// the grid.
    pub fn landing(&self, x: f64, y: f64, tolerance: f64) -> (f64, f64) {
        self.snap(x, y, tolerance)
    }

    fn snap(&self, x: f64, y: f64, tolerance: f64) -> (f64, f64) {
        if let Some(p) = self
            .sketch
            .point_near(x, y, tolerance)
            .and_then(|id| self.sketch.points.get(id))
        {
            return (p.x, p.y);
        }
        if self.grid > 0.0 {
            (
                (x / self.grid).round() * self.grid,
                (y / self.grid).round() * self.grid,
            )
        } else {
            (x, y)
        }
    }

    fn keep_edit<T>(&mut self, done: Result<T, SketchError>) -> Option<T> {
        match done {
            Ok(v) => {
                self.problem = None;
                Some(v)
            }
            Err(e) => {
                self.problem = Some(e.to_string());
                None
            }
        }
    }

    /// A click on the drawing at `(x, y)`; `tolerance` is how near, in millimetres, counts as on a
    /// point or an edge.
    pub fn click(&mut self, x: f64, y: f64, tolerance: f64) {
        self.act(x, y, tolerance);
        self.grow();
    }

    /// Makes the canvas show everything that is drawn.
    fn grow(&mut self) {
        if let Some((x0, x1, y0, y1)) = bounds(&self.drawing()) {
            let v = &mut self.view;
            *v = (v.0.min(x0), v.1.max(x1), v.2.min(y0), v.3.max(y1));
        }
    }

    fn act(&mut self, x: f64, y: f64, tolerance: f64) {
        let (sx, sy) = self.snap(x, y, tolerance);
        match self.tool {
            Tool::Move => {}
            Tool::Line => match self.picked.first().copied() {
                None => self.picked.push((sx, sy)),
                Some(from) => {
                    let result = self.sketch.draw_line(from, (sx, sy), tolerance);
                    if self.keep_edit(result).is_some() {
                        // Go on from the end, unless it joined a corner that is complete.
                        let closed = self
                            .sketch
                            .point_near(sx, sy, tolerance)
                            .is_some_and(|id| self.degree(id) >= 2);
                        self.picked.clear();
                        if !closed {
                            self.picked.push((sx, sy));
                        }
                    }
                }
            },
            Tool::Arc => {
                self.picked.push((sx, sy));
                if let &[centre, start, toward] = self.picked.as_slice() {
                    let result = self.sketch.draw_arc(centre, start, toward, tolerance);
                    self.keep_edit(result);
                    self.picked.clear();
                }
            }
            Tool::Dimension => {
                let Some(id) = self.sketch.point_near(x, y, tolerance) else {
                    self.problem = Some(tr("Click on a point.").into());
                    return;
                };
                match self.picked_points.first().copied() {
                    None => {
                        self.picked_points.push(id);
                        self.picked.push((sx, sy));
                        self.problem = None;
                    }
                    Some(first) => {
                        let result = self.sketch.dimension_points(first, id, self.measure);
                        self.keep_edit(result);
                        self.cancel();
                    }
                }
            }
            Tool::Delete => match self.sketch.entity_near(x, y, tolerance) {
                Some(entity) => {
                    let result = self.sketch.remove_edge(entity);
                    self.keep_edit(result);
                }
                None => self.problem = Some(tr("Click on an edge.").into()),
            },
        }
    }

    /// How many points a dimension has been given.
    pub fn pending_points(&self) -> usize {
        self.picked_points.len()
    }

    /// How many edges end at point `id`.
    fn degree(&self, id: PointId) -> usize {
        self.sketch
            .entities
            .iter()
            .filter_map(Entity::ends)
            .map(|(a, b)| [a, b].iter().filter(|p| **p == id).count())
            .sum()
    }

    /// The pointer went down at `(x, y)` with the Move tool: which point it takes, if any.
    pub fn begin_drag(&mut self, x: f64, y: f64, tolerance: f64) -> bool {
        self.dragging = if self.tool == Tool::Move {
            self.sketch.point_near(x, y, tolerance)
        } else {
            None
        };
        self.dragging.is_some()
    }

    /// The pointer moved to `(x, y)` with a point taken: the point goes there, or as near as
    /// its dimensions let it.
    pub fn drag_to(&mut self, x: f64, y: f64) {
        let Some(id) = self.dragging else {
            return;
        };
        let (x, y) = if self.grid > 0.0 {
            (
                (x / self.grid).round() * self.grid,
                (y / self.grid).round() * self.grid,
            )
        } else {
            (x, y)
        };
        let result = self.sketch.move_point(id, x, y);
        self.keep_edit(result);
        self.grow();
    }

    pub fn end_drag(&mut self) {
        self.dragging = None;
    }

    /// Takes dimension `index` off the sketch.
    pub fn remove_dimension(&mut self, index: usize) -> Result<(), SketchError> {
        let mut next = self.sketch.clone();
        if index >= next.constraints.len() {
            let e = SketchError::Invalid(format!("dimension {index} is not in the sketch"));
            self.problem = Some(e.to_string());
            return Err(e);
        }
        next.constraints.remove(index);
        let done = next.validate().map(|()| next);
        self.keep(done)
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

    /// Replaces the drawing with the sketch saved at `path` (one a case kept as
    /// `sketch.vortia-sketch`, or any sketch file). A file that cannot be read, or whose sketch
    /// cannot be solved, is refused and leaves the drawing as it was.
    pub fn open(&mut self, path: &Path) -> Result<(), SketchError> {
        let sketch = Sketch::open(path).and_then(|mut s| {
            s.solve()?;
            s.edges(5.0)?;
            Ok(s)
        });
        self.keep(sketch)?;
        self.picked.clear();
        self.picked_points.clear();
        self.dragging = None;
        self.fit();
        Ok(())
    }
}

/// Where the drawing is on the screen: the view fitted into a rectangle, centred, the same scale
/// along both axes, `y` up in the drawing and down on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    left: f64,
    top: f64,
    centre: (f64, f64),
    middle: (f64, f64),
    scale: f64,
}

impl Canvas {
    /// `view` is `(x0, x1, y0, y1)` in millimetres; the rectangle is in screen points.
    pub fn new(left: f64, top: f64, width: f64, height: f64, view: (f64, f64, f64, f64)) -> Self {
        let (x0, x1, y0, y1) = view;
        let scale = (width / (x1 - x0).max(1.0e-9)).min(height / (y1 - y0).max(1.0e-9));
        Self {
            left,
            top,
            centre: ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
            middle: (width / 2.0, height / 2.0),
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
        }
    }

    /// Screen points for a place in the drawing.
    pub fn to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.left + self.middle.0 + (x - self.centre.0) * self.scale,
            self.top + self.middle.1 - (y - self.centre.1) * self.scale,
        )
    }

    /// The place in the drawing under a screen point.
    pub fn to_world(&self, px: f64, py: f64) -> (f64, f64) {
        (
            self.centre.0 + (px - self.left - self.middle.0) / self.scale,
            self.centre.1 - (py - self.top - self.middle.1) / self.scale,
        )
    }

    /// Millimetres in a distance of `points` on the screen.
    pub fn millimetres(&self, points: f64) -> f64 {
        points / self.scale
    }
}

/// The box that holds the drawing: `(x0, x1, y0, y1)`.
pub fn bounds(drawing: &[DrawnEdge]) -> Option<(f64, f64, f64, f64)> {
    let mut it = drawing.iter().flat_map(|e| e.points.iter().copied());
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
        assert_eq!(
            drawing.len(),
            8,
            "four edges of the channel and four of the block"
        );
        assert_eq!(editor.open_because(), None);
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

    #[test]
    fn a_saved_sketch_is_opened_and_a_bad_file_leaves_the_drawing_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("region.vortia-sketch");
        let mut saved = RegionEditor::default();
        saved.rename(0, "inlet").unwrap();
        saved.save(&path).unwrap();

        let mut e = RegionEditor::new(Outline::Blank);
        e.open(&path).unwrap();
        assert_eq!(e.sketch(), saved.sketch());
        assert!(e.problem.is_none());
        assert_eq!(e.names().first().map(|(_, n)| n.as_str()), Some("inlet"));

        let before = e.sketch().clone();
        assert!(e.open(&dir.path().join("missing")).is_err());
        let junk = dir.path().join("junk.vortia-sketch");
        std::fs::write(&junk, "not json").unwrap();
        assert!(e.open(&junk).is_err());
        assert!(e.problem.is_some());
        assert_eq!(e.sketch(), &before);
    }

    const TOL: f64 = 3.0;

    #[test]
    fn a_region_is_drawn_from_nothing_with_the_line_tool_and_closed_by_its_first_point() {
        let mut e = RegionEditor::new(Outline::Blank);
        assert!(e.open_because().is_some(), "nothing is not a region");
        e.set_tool(Tool::Line);
        for (x, y) in [
            (0.0, 0.0),
            (200.0, 0.0),
            (200.0, 100.0),
            (0.0, 100.0),
            (0.0, 0.0),
        ] {
            e.click(x, y, TOL);
            assert_eq!(e.problem, None);
        }
        assert!(
            e.picked().is_empty(),
            "closing the outline ends the drawing"
        );
        assert_eq!((e.points().len(), e.drawing().len()), (4, 4));
        assert_eq!(e.open_because(), None);
        // Name an inlet and an outlet and the case can be made.
        assert!(e.missing().unwrap().contains("inlet"));
        e.rename(3, "inlet").unwrap();
        e.rename(1, "outlet").unwrap();
        assert_eq!(e.missing(), None);
        // Clicks are put on the 5 mm grid.
        let mut g = RegionEditor::new(Outline::Blank);
        g.set_tool(Tool::Line);
        g.click(1.4, 0.9, 0.1);
        g.click(52.6, 3.0, 0.1);
        assert_eq!(g.points()[0].0, 0.0);
        assert_eq!((g.points()[1].0, g.points()[1].1), (55.0, 5.0));
    }

    #[test]
    fn the_arc_tool_takes_three_clicks_and_a_dimension_two() {
        let mut e = RegionEditor::new(Outline::Blank);
        e.set_tool(Tool::Arc);
        e.click(0.0, 0.0, TOL);
        e.click(50.0, 0.0, TOL);
        assert_eq!(e.drawing().len(), 0, "an arc is not drawn before its end");
        e.click(0.0, 80.0, TOL);
        assert_eq!(e.drawing().len(), 1);
        assert!(e.picked().is_empty());
        // Two clicks on points hold them at the measure they have.
        e.set_tool(Tool::Dimension);
        e.measure = Measure::Distance;
        e.click(0.0, 0.0, TOL);
        e.click(50.0, 0.0, TOL);
        assert_eq!(e.problem, None);
        let held: Vec<_> = e.dimensions().into_iter().map(|d| d.value).collect();
        assert_eq!(held.len(), 1);
        assert!((held[0] - 50.0).abs() < 1e-9);
        // Clicking where there is no point says so.
        e.click(300.0, 300.0, TOL);
        assert!(e.problem.is_some());
        // The dimension comes off again.
        e.remove_dimension(0).unwrap();
        assert!(e.dimensions().is_empty());
        assert!(e.remove_dimension(5).is_err());
    }

    #[test]
    fn the_delete_tool_takes_the_edge_under_the_pointer_and_the_move_tool_drags_a_point() {
        let mut e = RegionEditor::default();
        let before = e.drawing().len();
        e.set_tool(Tool::Delete);
        // The middle of the channel's floor, away from the block.
        e.click(350.0, 0.0, TOL);
        assert_eq!(e.problem, None);
        assert_eq!(e.drawing().len(), before - 1);
        assert!(e.open_because().is_some());
        e.click(300.0, 50.0, TOL);
        assert!(e.problem.is_some(), "nothing there");
        // Drag the block's corner: it is held by dimensions, so it ends up where they hold it.
        e.set_tool(Tool::Move);
        assert!(e.begin_drag(120.0, 35.0, TOL));
        e.drag_to(300.0, 80.0);
        e.end_drag();
        assert!(!e.begin_drag(10.0, 90.0, TOL) || e.begin_drag(0.0, 100.0, TOL));
        // The view has grown to hold everything that was drawn.
        let (x0, x1, y0, y1) = e.view;
        assert!(x0 <= 0.0 && x1 >= 400.0 && y0 <= 0.0 && y1 >= 100.0);
    }

    #[test]
    fn the_canvas_maps_both_ways_with_the_same_scale_on_both_axes() {
        let c = Canvas::new(10.0, 20.0, 400.0, 200.0, (0.0, 400.0, 0.0, 100.0));
        // 400 mm across 400 points would be 1; 100 mm up 200 points would be 2: the smaller wins.
        assert!((c.millimetres(1.0) - 1.0).abs() < 1e-12);
        let (px, py) = c.to_screen(100.0, 25.0);
        let (x, y) = c.to_world(px, py);
        assert!((x - 100.0).abs() < 1e-9 && (y - 25.0).abs() < 1e-9);
        // Up in the drawing is up on the screen (smaller y), and the picture is centred.
        assert!(c.to_screen(0.0, 100.0).1 < c.to_screen(0.0, 0.0).1);
        assert!((c.to_screen(200.0, 50.0).0 - 210.0).abs() < 1e-9);
        assert!((c.to_screen(200.0, 50.0).1 - 120.0).abs() < 1e-9);
        // A view with no size does not divide by zero.
        let flat = Canvas::new(0.0, 0.0, 100.0, 100.0, (5.0, 5.0, 5.0, 5.0));
        assert!(flat.to_screen(5.0, 5.0).0.is_finite());
    }
}
