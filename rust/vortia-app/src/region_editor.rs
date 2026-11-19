//! The sketch behind "New > Drawn region" (R2/R3 of `docs/architecture/rust-only.md`): a closed
//! outline to dimension, whose edges carry names that say what each one does (`inlet`, `outlet`,
//! `symmetry`, anything else a wall). The state is plain data so everything but the drawing is
//! tested without a window.

use std::path::Path;

use vortia_sketch::{
    Constraint, DEFAULT_PATCH, Entity, MAX_SIDES, Measure, PointId, Sketch, SketchError, templates,
};

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

impl DrawnEdge {
    /// The middle of the edge along its points: where its name and its marks are drawn.
    pub fn middle(&self) -> Option<(f64, f64)> {
        let n = self.points.len();
        let b = self.points.get(n / 2).copied()?;
        if n % 2 == 1 {
            return Some(b);
        }
        let a = self.points.get(n / 2 - 1).copied()?;
        Some(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
    }
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

/// How many edits Undo can take back.
const MAX_HISTORY: usize = 200;

/// What a click on the drawing does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Drag a point; one a dimension holds comes back to where it is held.
    Move,
    /// Click where a line starts and where it ends; the next click goes on from the end.
    Line,
    /// Click the centre, where the arc starts and the direction of its end, counter-clockwise.
    Arc,
    /// Click two opposite corners: four lines held horizontal and vertical.
    Rectangle,
    /// Click the centre and a point on the circle.
    Circle,
    /// Click the two centres of the round ends, then a point at the width.
    Slot,
    /// Click the centre and a corner; the number of sides is set beside the tools.
    Polygon,
    /// Click a corner of two lines to round it with an arc of the radius set beside the tools.
    Fillet,
    /// Click a corner of two lines to cut it at the distance set beside the tools.
    Chamfer,
    /// Click two points to hold them at the measure they have.
    Dimension,
    /// Click what the relation set beside the tools is between, and it is held.
    Constrain,
    /// Click an edge to delete it.
    Delete,
}

impl Tool {
    /// The tools that make shapes.
    pub const DRAW: [Tool; 6] = [
        Tool::Line,
        Tool::Arc,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Slot,
        Tool::Polygon,
    ];
    /// The tools that change what is drawn.
    pub const MODIFY: [Tool; 4] = [Tool::Move, Tool::Fillet, Tool::Chamfer, Tool::Delete];
    /// The tools that hold the drawing to a size or a relation.
    pub const HOLD: [Tool; 2] = [Tool::Dimension, Tool::Constrain];

    pub fn title(self) -> &'static str {
        match self {
            Tool::Move => tr("Move"),
            Tool::Line => tr("Line"),
            Tool::Arc => tr("Arc"),
            Tool::Rectangle => tr("Rectangle"),
            Tool::Circle => tr("Circle"),
            Tool::Slot => tr("Slot"),
            Tool::Polygon => tr("Polygon"),
            Tool::Fillet => tr("Fillet"),
            Tool::Chamfer => tr("Chamfer"),
            Tool::Dimension => tr("Dimension"),
            Tool::Constrain => tr("Constrain"),
            Tool::Delete => tr("Delete"),
        }
    }

    /// The NDL icon the tool is drawn with, when the set has one.
    pub fn icon(self) -> Option<&'static str> {
        match self {
            Tool::Move => Some("move"),
            Tool::Line => Some("line"),
            Tool::Arc => Some("arc"),
            Tool::Rectangle => Some("rectangle"),
            Tool::Circle => Some("circle"),
            Tool::Slot => None,
            Tool::Polygon => Some("polygon"),
            Tool::Fillet => Some("fillet"),
            Tool::Chamfer => Some("chamfer"),
            Tool::Dimension => Some("dimension"),
            Tool::Constrain => Some("constraint"),
            Tool::Delete => Some("delete"),
        }
    }

    /// What the next click is for, in words; `picked` is how many the tool has been given.
    pub fn hint(self, picked: usize, relation: Relation) -> &'static str {
        match (self, picked) {
            (Tool::Move, _) => tr("Drag a point."),
            (Tool::Line, 0) => tr("Click where the line starts."),
            (Tool::Line, _) => tr("Click where it ends; Escape stops."),
            (Tool::Arc, 0) => tr("Click the centre of the arc."),
            (Tool::Arc, 1) => tr("Click where the arc starts."),
            (Tool::Arc, _) => tr("Click towards where it ends, counter-clockwise."),
            (Tool::Rectangle, 0) => tr("Click a corner of the rectangle."),
            (Tool::Rectangle, _) => tr("Click the opposite corner."),
            (Tool::Circle, 0) => tr("Click the centre of the circle."),
            (Tool::Circle, _) => tr("Click a point on the circle."),
            (Tool::Slot, 0) => tr("Click the centre of one round end."),
            (Tool::Slot, 1) => tr("Click the centre of the other end."),
            (Tool::Slot, _) => tr("Click a point as far from the axis as the slot is wide."),
            (Tool::Polygon, 0) => tr("Click the centre of the polygon."),
            (Tool::Polygon, _) => tr("Click a corner."),
            (Tool::Fillet, _) => tr("Click the corner to round."),
            (Tool::Chamfer, _) => tr("Click the corner to cut."),
            (Tool::Dimension, 0) => tr("Click the first point."),
            (Tool::Dimension, _) => tr("Click the second point."),
            (Tool::Constrain, _) => relation.hint(picked),
            (Tool::Delete, _) => tr("Click an edge to delete it."),
        }
    }
}

/// What a constraint holds, picked with the Constrain tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// Two points at the same height.
    Horizontal,
    /// Two points at the same `x`.
    Vertical,
    /// Two lines running the same way.
    Parallel,
    /// Two lines at a right angle.
    Perpendicular,
    /// Two edges of the same size.
    Equal,
    /// A line touching an arc or circle.
    Tangent,
    /// Two points that are one.
    Coincident,
    /// A point held where it is.
    Fix,
}

impl Relation {
    pub const ALL: [Relation; 8] = [
        Relation::Horizontal,
        Relation::Vertical,
        Relation::Parallel,
        Relation::Perpendicular,
        Relation::Equal,
        Relation::Tangent,
        Relation::Coincident,
        Relation::Fix,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Relation::Horizontal => tr("Horizontal"),
            Relation::Vertical => tr("Vertical"),
            Relation::Parallel => tr("Parallel"),
            Relation::Perpendicular => tr("Perpendicular"),
            Relation::Equal => tr("Equal"),
            Relation::Tangent => tr("Tangent"),
            Relation::Coincident => tr("Coincident"),
            Relation::Fix => tr("Fix"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Relation::Horizontal => "horizontal",
            Relation::Vertical => "vertical",
            Relation::Parallel => "parallel",
            Relation::Perpendicular => "perpendicular",
            Relation::Equal => "equal",
            Relation::Tangent => "tangent",
            Relation::Coincident => "coincident",
            Relation::Fix => "fix",
        }
    }

    /// What it is picked from: points or edges, and how many.
    fn takes(self) -> (Pick, usize) {
        match self {
            Relation::Horizontal | Relation::Vertical | Relation::Coincident => (Pick::Point, 2),
            Relation::Fix => (Pick::Point, 1),
            Relation::Parallel | Relation::Perpendicular | Relation::Equal | Relation::Tangent => {
                (Pick::Edge, 2)
            }
        }
    }

    fn hint(self, picked: usize) -> &'static str {
        match (self.takes().0, picked) {
            (Pick::Point, 0) if self == Relation::Fix => tr("Click the point to hold in place."),
            (Pick::Point, 0) => tr("Click the first point."),
            (Pick::Point, _) => tr("Click the second point."),
            (Pick::Edge, 0) => tr("Click the first edge."),
            (Pick::Edge, _) => tr("Click the second edge."),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Point,
    Edge,
}

/// A mark the canvas draws where a constraint holds, in millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub at: (f64, f64),
    pub relation: Relation,
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
    /// What the Constrain tool holds.
    pub relation: Relation,
    /// The sides of a polygon the Polygon tool draws.
    pub sides: usize,
    /// The radius, in millimetres, the Fillet tool rounds a corner with.
    pub radius: f64,
    /// How far back along each line, in millimetres, the Chamfer tool cuts.
    pub distance: f64,
    /// How a dimension measures: along the line between the points, or along an axis.
    pub measure: Measure,
    /// Points are put on this grid, in millimetres; `0` for no grid.
    pub grid: f64,
    /// What the drawing tool has been given so far, in millimetres.
    picked: Vec<(f64, f64)>,
    /// The points a dimension was given.
    picked_points: Vec<PointId>,
    /// The edges (entities) the Constrain tool was given.
    picked_edges: Vec<usize>,
    dragging: Option<PointId>,
    /// The sketch when a drag began, to be kept for Undo if the drag changed it.
    drag_origin: Option<Sketch>,
    /// The sketches before each edit, latest last, and the ones Undo took back.
    history: Vec<Sketch>,
    future: Vec<Sketch>,
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
            relation: Relation::Horizontal,
            sides: 6,
            radius: 10.0,
            distance: 10.0,
            measure: Measure::Distance,
            grid: 5.0,
            picked: Vec::new(),
            picked_points: Vec::new(),
            picked_edges: Vec::new(),
            dragging: None,
            drag_origin: None,
            history: Vec::new(),
            future: Vec::new(),
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
                let before = std::mem::replace(&mut self.sketch, next);
                self.problem = None;
                self.remember(before);
                Ok(())
            }
            Err(e) => {
                self.problem = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// Keeps `before` for Undo when the sketch is no longer it.
    fn remember(&mut self, before: Sketch) {
        if before != self.sketch {
            self.history.push(before);
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
            self.future.clear();
        }
    }

    /// Runs `change` on the sketch: it is kept, and Undo can take it back, or it is refused and
    /// the sketch is as it was.
    fn edit<T>(&mut self, change: impl FnOnce(&mut Sketch) -> Result<T, SketchError>) -> Option<T> {
        let before = self.sketch.clone();
        match change(&mut self.sketch) {
            Ok(v) => {
                self.problem = None;
                self.remember(before);
                Some(v)
            }
            Err(e) => {
                self.sketch = before;
                self.problem = Some(e.to_string());
                None
            }
        }
    }

    /// Takes back the last edit; whether there was one.
    pub fn undo(&mut self) -> bool {
        self.cancel();
        let Some(before) = self.history.pop() else {
            return false;
        };
        self.future
            .push(std::mem::replace(&mut self.sketch, before));
        self.problem = None;
        self.grow();
        true
    }

    /// Does again what Undo took back; whether there was something.
    pub fn redo(&mut self) -> bool {
        self.cancel();
        let Some(after) = self.future.pop() else {
            return false;
        };
        self.history
            .push(std::mem::replace(&mut self.sketch, after));
        self.problem = None;
        self.grow();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// How many ways the drawing can still move: `0` when it is held in every direction. `None`
    /// when it cannot be worked out.
    pub fn freedom(&self) -> Option<usize> {
        self.sketch.degrees_of_freedom().ok()
    }

    /// A mark for every constraint that is not a measure: where the canvas shows it holds.
    pub fn markers(&self) -> Vec<Marker> {
        let drawing = self.drawing();
        let point = |id: PointId| self.sketch.points.get(id).map(|p| (p.x, p.y));
        let middle = |e: usize| drawing.get(e).and_then(DrawnEdge::middle);
        let mut out = Vec::new();
        let mut mark = |at: Option<(f64, f64)>, relation| {
            if let Some(at) = at {
                out.push(Marker { at, relation });
            }
        };
        for c in &self.sketch.constraints {
            match *c {
                Constraint::Horizontal { a, b } => {
                    mark(midway(point(a), point(b)), Relation::Horizontal);
                }
                Constraint::Vertical { a, b } => {
                    mark(midway(point(a), point(b)), Relation::Vertical);
                }
                Constraint::Coincident { a, .. } => mark(point(a), Relation::Coincident),
                Constraint::Parallel { a, b } => {
                    mark(middle(a), Relation::Parallel);
                    mark(middle(b), Relation::Parallel);
                }
                Constraint::Perpendicular { a, b } => {
                    mark(middle(a), Relation::Perpendicular);
                    mark(middle(b), Relation::Perpendicular);
                }
                Constraint::Equal { a, b } => {
                    mark(middle(a), Relation::Equal);
                    mark(middle(b), Relation::Equal);
                }
                Constraint::Tangent { line, round } => {
                    mark(
                        self.touch(line, round).or_else(|| middle(line)),
                        Relation::Tangent,
                    );
                }
                _ => {}
            }
        }
        out
    }

    /// The point a line and an arc or circle share, where they touch.
    fn touch(&self, line: usize, round: usize) -> Option<(f64, f64)> {
        let (a, b) = self.sketch.entities.get(line)?.ends()?;
        let round = self.sketch.entities.get(round)?;
        round
            .points()
            .into_iter()
            .find(|p| *p == a || *p == b)
            .and_then(|id| self.sketch.points.get(id))
            .map(|p| (p.x, p.y))
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
        self.picked_edges.clear();
        self.dragging = None;
        self.drag_origin = None;
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
                    if self
                        .edit(|s| s.draw_line(from, (sx, sy), tolerance))
                        .is_some()
                    {
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
                    self.edit(|s| s.draw_arc(centre, start, toward, tolerance));
                    self.picked.clear();
                }
            }
            Tool::Rectangle => {
                self.picked.push((sx, sy));
                if let &[a, b] = self.picked.as_slice() {
                    self.edit(|s| s.draw_rectangle(a, b, tolerance));
                    self.picked.clear();
                }
            }
            Tool::Circle => {
                self.picked.push((sx, sy));
                if let &[centre, through] = self.picked.as_slice() {
                    self.edit(|s| s.draw_circle(centre, through, tolerance));
                    self.picked.clear();
                }
            }
            Tool::Slot => {
                self.picked.push((sx, sy));
                if let &[a, b, third] = self.picked.as_slice() {
                    let half = distance_to_line(third, a, b);
                    self.edit(|s| s.draw_slot(a, b, half, tolerance));
                    self.picked.clear();
                }
            }
            Tool::Polygon => {
                self.picked.push((sx, sy));
                if let &[centre, vertex] = self.picked.as_slice() {
                    let sides = self.sides;
                    self.edit(|s| s.draw_polygon(centre, vertex, sides, tolerance));
                    self.picked.clear();
                }
            }
            Tool::Fillet | Tool::Chamfer => {
                let Some(corner) = self.sketch.point_near(x, y, tolerance) else {
                    self.problem = Some(tr("Click on a corner.").into());
                    return;
                };
                if self.tool == Tool::Fillet {
                    let radius = self.radius;
                    self.edit(|s| s.fillet(corner, radius));
                } else {
                    let distance = self.distance;
                    self.edit(|s| s.chamfer(corner, distance));
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
                        let measure = self.measure;
                        self.edit(|s| s.dimension_points(first, id, measure));
                        self.cancel();
                    }
                }
            }
            Tool::Constrain => self.constrain(x, y, tolerance),
            Tool::Delete => match self.sketch.entity_near(x, y, tolerance) {
                Some(entity) => {
                    self.edit(|s| s.remove_edge(entity));
                }
                None => self.problem = Some(tr("Click on an edge.").into()),
            },
        }
    }

    /// A click for the Constrain tool: it collects what the relation is between, then holds it.
    fn constrain(&mut self, x: f64, y: f64, tolerance: f64) {
        let relation = self.relation;
        let (kind, wanted) = relation.takes();
        match kind {
            Pick::Point => {
                let Some(id) = self.sketch.point_near(x, y, tolerance) else {
                    self.problem = Some(tr("Click on a point.").into());
                    return;
                };
                self.picked_points.push(id);
                if let Some(p) = self.sketch.points.get(id) {
                    self.picked.push((p.x, p.y));
                }
            }
            Pick::Edge => {
                let Some(edge) = self.sketch.entity_near(x, y, tolerance) else {
                    self.problem = Some(tr("Click on an edge.").into());
                    return;
                };
                self.picked_edges.push(edge);
            }
        }
        self.problem = None;
        if self.picked_points.len().max(self.picked_edges.len()) < wanted {
            return;
        }
        let (points, edges) = (self.picked_points.clone(), self.picked_edges.clone());
        self.cancel();
        match (relation, points.as_slice(), edges.as_slice()) {
            (Relation::Fix, &[id], _) => {
                let fixed = self.sketch.points.get(id).is_some_and(|p| p.fixed);
                self.edit(|s| s.set_fixed(id, !fixed));
            }
            (Relation::Horizontal | Relation::Vertical | Relation::Coincident, &[a, b], _) => {
                if a == b {
                    self.problem = Some(tr("Pick two different points.").into());
                    return;
                }
                let constraint = match relation {
                    Relation::Horizontal => Constraint::Horizontal { a, b },
                    Relation::Vertical => Constraint::Vertical { a, b },
                    _ => Constraint::Coincident { a, b },
                };
                self.edit(|s| s.add_constraint(constraint));
            }
            (_, _, &[a, b]) => {
                if a == b {
                    self.problem = Some(tr("Pick two different edges.").into());
                    return;
                }
                match self.between_edges(relation, a, b) {
                    Some(constraint) => {
                        self.edit(|s| s.add_constraint(constraint));
                    }
                    None => {
                        self.problem = Some(
                            match relation {
                                Relation::Tangent => tr("Pick a line and an arc or circle."),
                                Relation::Equal => tr("Pick two lines, or two arcs or circles."),
                                _ => tr("Pick two lines."),
                            }
                            .into(),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// The constraint of `relation` between edges `a` and `b`, when they are the kinds it needs.
    fn between_edges(&self, relation: Relation, a: usize, b: usize) -> Option<Constraint> {
        let line = |e: usize| matches!(self.sketch.entities.get(e), Some(Entity::Line { .. }));
        let round = |e: usize| {
            matches!(
                self.sketch.entities.get(e),
                Some(Entity::Arc { .. } | Entity::Circle { .. })
            )
        };
        match relation {
            Relation::Parallel if line(a) && line(b) => Some(Constraint::Parallel { a, b }),
            Relation::Perpendicular if line(a) && line(b) => {
                Some(Constraint::Perpendicular { a, b })
            }
            Relation::Equal if (line(a) && line(b)) || (round(a) && round(b)) => {
                Some(Constraint::Equal { a, b })
            }
            Relation::Tangent if line(a) && round(b) => {
                Some(Constraint::Tangent { line: a, round: b })
            }
            Relation::Tangent if line(b) && round(a) => {
                Some(Constraint::Tangent { line: b, round: a })
            }
            _ => None,
        }
    }

    /// How far into the current pick the tool is: how many clicks it has been given.
    pub fn progress(&self) -> usize {
        self.picked
            .len()
            .max(self.picked_points.len())
            .max(self.picked_edges.len())
    }

    /// The shape the tool would make if the next click were at `to`, as lines of points in
    /// millimetres: what the canvas draws under the pointer.
    pub fn preview(&self, to: (f64, f64)) -> Vec<Vec<(f64, f64)>> {
        let tau = std::f64::consts::TAU;
        let circle = |c: (f64, f64), r: f64| -> Vec<(f64, f64)> {
            (0..=48)
                .map(|i| {
                    let a = tau * f64::from(i) / 48.0;
                    (c.0 + r * a.cos(), c.1 + r * a.sin())
                })
                .collect()
        };
        match (self.tool, self.picked.as_slice()) {
            (Tool::Line, &[from]) => vec![vec![from, to]],
            (Tool::Arc, &[c]) => vec![vec![c, to]],
            (Tool::Arc, &[c, start]) => {
                let r = (start.0 - c.0).hypot(start.1 - c.1);
                let a0 = (start.1 - c.1).atan2(start.0 - c.0);
                let a1 = (to.1 - c.1).atan2(to.0 - c.0);
                let sweep = (a1 - a0).rem_euclid(tau);
                let arc = (0..=32)
                    .map(|i| {
                        let a = a0 + sweep * f64::from(i) / 32.0;
                        (c.0 + r * a.cos(), c.1 + r * a.sin())
                    })
                    .collect();
                vec![arc]
            }
            (Tool::Rectangle, &[a]) => vec![vec![a, (to.0, a.1), to, (a.0, to.1), a]],
            (Tool::Circle, &[c]) => vec![circle(c, (to.0 - c.0).hypot(to.1 - c.1))],
            (Tool::Slot, &[a]) => vec![vec![a, to]],
            (Tool::Slot, &[a, b]) => {
                let half = distance_to_line(to, a, b);
                let length = (b.0 - a.0).hypot(b.1 - a.1);
                if length <= 0.0 || half <= 0.0 {
                    return vec![vec![a, b]];
                }
                let theta = (b.1 - a.1).atan2(b.0 - a.0);
                let end = |c: (f64, f64), from: f64| -> Vec<(f64, f64)> {
                    (0..=16)
                        .map(|i| {
                            let t = from + std::f64::consts::PI * f64::from(i) / 16.0;
                            (c.0 + half * t.cos(), c.1 + half * t.sin())
                        })
                        .collect()
                };
                let mut outline = end(b, theta - std::f64::consts::FRAC_PI_2);
                outline.extend(end(a, theta + std::f64::consts::FRAC_PI_2));
                if let Some(&first) = outline.first() {
                    outline.push(first);
                }
                vec![outline]
            }
            (Tool::Polygon, &[c]) => {
                let sides = self.sides.clamp(3, MAX_SIDES);
                let r = (to.0 - c.0).hypot(to.1 - c.1);
                let a0 = (to.1 - c.1).atan2(to.0 - c.0);
                let corners = (0..=sides)
                    .map(|i| {
                        let a = a0 + tau * i as f64 / sides as f64;
                        (c.0 + r * a.cos(), c.1 + r * a.sin())
                    })
                    .collect();
                vec![corners]
            }
            (Tool::Dimension | Tool::Constrain, &[from])
                if self.relation.takes().0 == Pick::Point || self.tool == Tool::Dimension =>
            {
                vec![vec![from, to]]
            }
            _ => Vec::new(),
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
        self.drag_origin = self.dragging.map(|_| self.sketch.clone());
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
        if let Some(before) = self.drag_origin.take() {
            self.remember(before);
        }
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
        // Undo can still go back to what was drawn before the file was opened.
        self.keep(sketch)?;
        self.cancel();
        self.fit();
        Ok(())
    }
}

/// The middle of two places, when both are known.
fn midway(a: Option<(f64, f64)>, b: Option<(f64, f64)>) -> Option<(f64, f64)> {
    let (a, b) = (a?, b?);
    Some(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
}

/// How far `p` is from the infinite line through `a` and `b`; from `a` when they are one place.
fn distance_to_line(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    if length <= 0.0 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / length
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

    fn clicks(e: &mut RegionEditor, points: &[(f64, f64)]) {
        for &(x, y) in points {
            e.click(x, y, TOL);
        }
    }

    #[test]
    fn the_shape_tools_draw_whole_shapes_from_their_clicks() {
        let mut e = RegionEditor::new(Outline::Blank);
        e.set_tool(Tool::Rectangle);
        clicks(&mut e, &[(0.0, 0.0), (100.0, 50.0)]);
        assert_eq!((e.problem.clone(), e.drawing().len()), (None, 4));
        assert_eq!(e.open_because(), None, "a rectangle is a closed outline");
        assert!(e.picked().is_empty());

        e.set_tool(Tool::Circle);
        clicks(&mut e, &[(50.0, 25.0), (60.0, 25.0)]);
        assert_eq!((e.problem.clone(), e.drawing().len()), (None, 5));

        e.set_tool(Tool::Polygon);
        e.sides = 5;
        clicks(&mut e, &[(200.0, 25.0), (230.0, 25.0)]);
        assert_eq!((e.problem.clone(), e.drawing().len()), (None, 10));

        e.set_tool(Tool::Slot);
        clicks(&mut e, &[(300.0, 0.0), (400.0, 0.0), (400.0, 10.0)]);
        assert_eq!((e.problem.clone(), e.drawing().len()), (None, 14));
        // A slot with no width, and a polygon with two sides, are refused whole.
        let kept = e.sketch().clone();
        clicks(&mut e, &[(300.0, 50.0), (400.0, 50.0), (400.0, 50.0)]);
        assert!(e.problem.is_some());
        e.set_tool(Tool::Polygon);
        e.sides = 2;
        clicks(&mut e, &[(0.0, 100.0), (20.0, 100.0)]);
        assert!(e.problem.is_some());
        assert_eq!(e.sketch(), &kept);
    }

    #[test]
    fn undo_and_redo_walk_back_and_forward_over_every_kind_of_edit() {
        let mut e = RegionEditor::new(Outline::Blank);
        assert!(!e.can_undo() && !e.undo() && !e.redo());
        e.set_tool(Tool::Rectangle);
        clicks(&mut e, &[(0.0, 0.0), (100.0, 50.0)]);
        let drawn = e.sketch().clone();
        // A drag is one edit.
        e.set_tool(Tool::Move);
        assert!(e.begin_drag(100.0, 50.0, TOL));
        e.drag_to(150.0, 80.0);
        e.drag_to(160.0, 90.0);
        e.end_drag();
        let dragged = e.sketch().clone();
        assert_ne!(dragged, drawn);
        // A drag that moved nothing is not.
        assert!(e.begin_drag(0.0, 0.0, TOL));
        e.end_drag();
        assert!(e.undo());
        assert_eq!(e.sketch(), &drawn);
        assert!(e.can_redo() && e.redo());
        assert_eq!(e.sketch(), &dragged);
        assert!(e.undo() && e.undo());
        assert_eq!(e.drawing().len(), 0);
        assert!(!e.can_undo());
        // A new edit forgets what could have been redone.
        e.set_tool(Tool::Line);
        clicks(&mut e, &[(0.0, 0.0), (10.0, 0.0)]);
        assert!(!e.can_redo());
        // A refused edit is not an edit.
        let before = e.sketch().clone();
        e.set_tool(Tool::Delete);
        e.click(500.0, 500.0, TOL);
        assert!(e.problem.is_some());
        assert_eq!(e.sketch(), &before);
        assert!(e.undo());
        assert_eq!(e.drawing().len(), 0);
    }

    #[test]
    fn a_fillet_and_a_chamfer_cut_the_corner_they_are_clicked_on() {
        let mut e = RegionEditor::new(Outline::Blank);
        e.set_tool(Tool::Rectangle);
        clicks(&mut e, &[(0.0, 0.0), (100.0, 50.0)]);
        e.set_tool(Tool::Fillet);
        e.radius = 8.0;
        e.click(100.0, 50.0, TOL);
        assert_eq!(e.problem, None);
        assert_eq!(e.drawing().len(), 5, "four sides and the round corner");
        assert_eq!(e.open_because(), None);
        e.set_tool(Tool::Chamfer);
        e.distance = 6.0;
        e.click(0.0, 0.0, TOL);
        assert_eq!(e.problem, None);
        assert_eq!(e.drawing().len(), 6);
        // Where no corner is, or too far back, the click is refused.
        let kept = e.sketch().clone();
        e.click(50.0, 25.0, TOL);
        assert!(e.problem.is_some());
        e.set_tool(Tool::Fillet);
        e.radius = 1.0e6;
        e.click(100.0, 0.0, TOL);
        assert!(e.problem.is_some());
        assert_eq!(e.sketch(), &kept);
        assert!(e.undo() && e.undo());
        assert_eq!(e.drawing().len(), 4);
    }

    #[test]
    fn constraints_are_picked_from_the_right_kinds_and_marked_where_they_hold() {
        let mut e = RegionEditor::new(Outline::Blank);
        e.set_tool(Tool::Line);
        clicks(&mut e, &[(0.0, 0.0), (100.0, 20.0)]);
        e.cancel();
        clicks(&mut e, &[(0.0, 50.0), (100.0, 80.0)]);
        e.cancel();
        assert!(e.markers().is_empty());
        e.set_tool(Tool::Constrain);
        // Horizontal takes points; the line becomes level.
        e.relation = Relation::Horizontal;
        clicks(&mut e, &[(0.0, 0.0), (100.0, 20.0)]);
        assert_eq!(e.problem, None);
        let m = e.markers();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].relation, Relation::Horizontal);
        let level: Vec<f64> = e.points().iter().take(2).map(|p| p.1).collect();
        assert!((level[0] - level[1]).abs() < 1e-6, "{level:?}");
        // The same point twice is refused, as is a click on nothing.
        clicks(&mut e, &[(0.0, 0.0), (0.0, 0.0)]);
        assert!(e.problem.is_some());
        e.cancel();
        e.click(300.0, 300.0, TOL);
        assert!(e.problem.is_some());
        // Parallel takes two lines and marks both.
        e.relation = Relation::Parallel;
        clicks(&mut e, &[(50.0, 10.0), (50.0, 65.0)]);
        assert_eq!(e.problem, None, "{:?}", e.problem);
        assert_eq!(
            e.markers()
                .iter()
                .filter(|m| m.relation == Relation::Parallel)
                .count(),
            2
        );
        // Tangent needs a line and a round one: two lines are refused and nothing changes.
        let kept = e.sketch().clone();
        e.relation = Relation::Tangent;
        clicks(&mut e, &[(50.0, 10.0), (50.0, 65.0)]);
        assert!(e.problem.as_deref().is_some_and(|p| p.contains("arc")));
        assert_eq!(e.sketch(), &kept);
        // Fix toggles a point.
        e.relation = Relation::Fix;
        let (x, y, _) = e.points()[0];
        e.click(x, y, TOL);
        assert!(e.points().iter().any(|p| p.2));
        e.click(x, y, TOL);
        assert!(e.points().iter().all(|p| !p.2));
        // Every one of those can be taken back.
        while e.undo() {}
        assert_eq!(e.drawing().len(), 0);
    }

    #[test]
    fn the_degrees_of_freedom_fall_as_the_drawing_is_held() {
        let mut e = RegionEditor::new(Outline::Blank);
        e.set_tool(Tool::Rectangle);
        clicks(&mut e, &[(0.0, 0.0), (100.0, 50.0)]);
        // 8 coordinates less 4 level/plumb constraints.
        assert_eq!(e.freedom(), Some(4));
        e.set_tool(Tool::Constrain);
        e.relation = Relation::Fix;
        e.click(0.0, 0.0, TOL);
        assert_eq!(e.freedom(), Some(2));
        e.set_tool(Tool::Dimension);
        e.measure = Measure::Along;
        clicks(&mut e, &[(0.0, 0.0), (100.0, 0.0)]);
        e.measure = Measure::Height;
        clicks(&mut e, &[(0.0, 0.0), (0.0, 50.0)]);
        assert_eq!(e.freedom(), Some(0), "{:?}", e.problem);
    }

    #[test]
    fn the_preview_shows_what_the_next_click_would_make() {
        let mut e = RegionEditor::new(Outline::Blank);
        assert!(e.preview((5.0, 5.0)).is_empty());
        e.set_tool(Tool::Rectangle);
        e.click(0.0, 0.0, TOL);
        let p = e.preview((40.0, 20.0));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].first(), p[0].last(), "a rectangle is closed");
        assert_eq!(p[0].len(), 5);
        e.set_tool(Tool::Polygon);
        e.sides = 6;
        e.click(0.0, 0.0, TOL);
        assert_eq!(e.preview((10.0, 0.0))[0].len(), 7);
        e.set_tool(Tool::Slot);
        clicks(&mut e, &[(0.0, 0.0), (50.0, 0.0)]);
        let slot = e.preview((50.0, 8.0));
        assert!(slot[0].iter().all(|(x, y)| x.is_finite() && y.is_finite()));
        // No width yet: only the axis.
        assert_eq!(e.preview((20.0, 0.0))[0].len(), 2);
        // Hostile numbers never come out as a panic.
        e.set_tool(Tool::Circle);
        e.click(0.0, 0.0, TOL);
        let _ = e.preview((f64::NAN, f64::INFINITY));
    }

    #[test]
    fn an_edge_is_named_and_marked_at_its_middle() {
        let edge = |points: &[(f64, f64)]| DrawnEdge {
            patch: "wall".into(),
            points: points.to_vec(),
        };
        assert_eq!(edge(&[(0.0, 0.0), (10.0, 4.0)]).middle(), Some((5.0, 2.0)));
        assert_eq!(
            edge(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]).middle(),
            Some((1.0, 1.0))
        );
        assert_eq!(edge(&[]).middle(), None);
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
