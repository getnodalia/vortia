//! The mesh viewer: the surface of a case drawn by egui, turned with the mouse. The geometry is
//! in [`crate::surface`]; this draws it, on the CPU, so it runs wherever egui does.

use crate::i18n::{tr, trf};
use eframe::egui::{
    self, Color32, CornerRadius, Mesh, PointerButton, Pos2, Sense, Stroke, Vec2, vec2,
};

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use vortia_post::Case;

use crate::edgepick::{self, FeatureEdge};
use crate::facepaint::FacePaint;
use crate::fieldview::{self, CellValues};
use crate::flowlines::{self, FlowLines};
use crate::section::{self, Section};
use crate::surface::{Camera, Cut, Surface};

/// Outlines are drawn for surfaces up to this many faces; beyond it they would cost more than
/// they show.
const MAX_OUTLINED_FACES: usize = 8_000;
/// Whether a viewer starts with the cell edges drawn, set from the Preferences.
static DEFAULT_CELL_EDGES: AtomicBool = AtomicBool::new(true);

/// Makes the viewers opened from now on start with the cell edges drawn, or not.
pub fn set_default_cell_edges(on: bool) {
    DEFAULT_CELL_EDGES.store(on, Ordering::Relaxed);
}

/// The width of the list of patches beside the picture.
const LEGEND: f32 = 220.0;

/// The colours of the patches, in turn; the ones that close the domain in 2-D are grey.
const PALETTE: [Color32; 7] = [
    Color32::from_rgb(0x4c, 0x8b, 0xf5),
    Color32::from_rgb(0xf0, 0x8c, 0x3a),
    Color32::from_rgb(0x2f, 0xb3, 0x8c),
    Color32::from_rgb(0x9b, 0x6b, 0xe0),
    Color32::from_rgb(0xe0, 0x5b, 0x8a),
    Color32::from_rgb(0xd0, 0xb0, 0x2c),
    Color32::from_rgb(0x4f, 0xb8, 0xd8),
];

/// The colour of a patch of `kind`, the `index`-th of its mesh.
pub fn patch_color(kind: &str, index: usize) -> Color32 {
    if is_closing(kind) {
        Color32::from_rgb(0x8a, 0x92, 0x9c)
    } else {
        PALETTE
            .get(index % PALETTE.len())
            .copied()
            .unwrap_or(Color32::GRAY)
    }
}

/// Whether patches of this kind close the domain (the sides of a 2-D or wedge case) rather
/// than bound the flow.
fn is_closing(kind: &str) -> bool {
    matches!(
        kind,
        "empty" | "wedge" | "symmetry" | "symmetryPlane" | "cyclic"
    )
}

/// Where the viewer finds the fields of the case: its folder, the times written and the field
/// files of the latest of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub dir: PathBuf,
    pub times: Vec<String>,
    pub fields: Vec<String>,
}

pub struct Viewer {
    surface: Surface,
    camera: Camera,
    visible: Vec<bool>,
    outlines: bool,
    /// The field the surface is coloured with, or none for the colours of the patches.
    field: Option<String>,
    /// The time it is read at; the latest written when none is chosen.
    time: Option<String>,
    values: Option<Result<CellValues, String>>,
    values_rx: Option<Receiver<Result<CellValues, String>>>,
    /// What `values` (or the read in flight) is of.
    requested: Option<(String, String)>,
    /// Whether streamlines and arrows are drawn over a vector field.
    lines: bool,
    arrows: bool,
    flow: Option<Result<FlowLines, String>>,
    flow_rx: Option<Receiver<Result<FlowLines, String>>>,
    flow_requested: Option<(String, String)>,
    /// A plane through the volume: the half in front of it is cut away and the cells it crosses
    /// are coloured with the field.
    slice: bool,
    slice_axis: usize,
    /// Where the plane is, 0 to 1 of the way through the mesh.
    slice_pos: f32,
    /// The case, read once for the sections.
    case: Option<Arc<Case>>,
    case_rx: Option<Receiver<Result<Arc<Case>, String>>>,
    section: Option<Result<Section, String>>,
    section_rx: Option<Receiver<Result<Section, String>>>,
    /// The axis and position (in thousandths) the section shown, or the one in flight, is of.
    section_asked: Option<(usize, i32)>,
    /// Where the picture was drawn on the last frame, in points.
    picture_rect: egui::Rect,
    /// A saved picture in flight, and what came of the last one.
    picture_rx: Option<Receiver<(bool, String)>>,
    picture_note: Option<(bool, String)>,
    /// The edges of a modelled part that can be picked with the mouse, when it is being done.
    picking: Option<Picking>,
    /// A number for each face of some walls, painted in place of the field.
    paint: Option<FacePaint>,
    /// Whether the choices over the picture are open.
    legend_open: bool,
}

/// The edges a person is choosing among, and what they did with them.
struct Picking {
    edges: Vec<FeatureEdge>,
    /// The points already chosen, drawn as chosen.
    chosen: Vec<[f64; 3]>,
    /// The edge under the pointer.
    hover: Option<usize>,
    /// The edge clicked on the last frame, for whoever asked to pick.
    clicked: Option<FeatureEdge>,
}

/// How near the pointer must be to an edge, in points, to mean it.
const PICK_REACH: f32 = 8.0;

impl Viewer {
    pub fn new(surface: Surface) -> Self {
        let camera = Camera::fit(&surface);
        let visible = vec![true; surface.patches.len()];
        Self {
            surface,
            camera,
            visible,
            outlines: DEFAULT_CELL_EDGES.load(Ordering::Relaxed),
            field: None,
            time: None,
            values: None,
            values_rx: None,
            paint: None,
            legend_open: true,
            requested: None,
            lines: false,
            arrows: false,
            flow: None,
            flow_rx: None,
            flow_requested: None,
            slice: false,
            slice_axis: 0,
            slice_pos: 0.5,
            case: None,
            case_rx: None,
            section: None,
            section_rx: None,
            section_asked: None,
            picture_rect: egui::Rect::NOTHING,
            picture_rx: None,
            picture_note: None,
            picking: None,
        }
    }

    /// Lets the person click the edges of the part shown (`None`: back to just looking).
    pub fn pick_among(&mut self, edges: Option<Vec<FeatureEdge>>) {
        self.picking = edges.map(|edges| Picking {
            edges,
            chosen: Vec::new(),
            hover: None,
            clicked: None,
        });
    }

    /// The points to draw as chosen.
    pub fn set_chosen(&mut self, points: &[[f64; 3]]) {
        if let Some(picking) = &mut self.picking {
            picking.chosen = points.to_vec();
        }
    }

    /// The edge clicked since this was last asked.
    pub fn take_click(&mut self) -> Option<FeatureEdge> {
        self.picking.as_mut()?.clicked.take()
    }

    /// Puts another surface in place of this one and keeps the camera where it was, so a part
    /// being modelled does not jump back to its first view at every change.
    pub fn replace_surface(&mut self, surface: Surface) {
        self.visible = vec![true; surface.patches.len()];
        self.surface = surface;
    }

    /// Colours the surface with a field of the case (`None`: the colours of the patches), at a
    /// time (`None`: the latest written).
    pub fn colour_by(&mut self, field: Option<&str>, time: Option<&str>) {
        self.field = field.map(str::to_owned);
        self.time = time.map(str::to_owned);
    }

    /// Paints a number for each face of some walls on the surface (`None`: back to the field).
    pub fn paint(&mut self, paint: Option<FacePaint>) {
        self.paint = paint;
    }

    pub fn painted(&self) -> Option<&FacePaint> {
        self.paint.as_ref()
    }

    pub fn values(&self) -> Option<&Result<CellValues, String>> {
        self.values.as_ref()
    }

    /// Takes a finished read, and starts the one the choices ask for if it is not the one held.
    fn sync_field(&mut self, ctx: &egui::Context, source: &Source) {
        if let Some(rx) = &self.values_rx
            && let Ok(read) = rx.try_recv()
        {
            self.values = Some(read);
            self.values_rx = None;
        }
        let Some(name) = self.field.clone() else {
            self.values = None;
            self.values_rx = None;
            self.requested = None;
            return;
        };
        let time = self
            .time
            .clone()
            .or_else(|| source.times.last().cloned())
            .unwrap_or_else(|| "0".to_owned());
        let wanted = (name.clone(), time.clone());
        if self.requested.as_ref() != Some(&wanted) {
            let repaint = ctx.clone();
            self.values = None;
            self.values_rx = Some(fieldview::spawn(
                source.dir.clone(),
                time,
                name,
                self.surface.cells,
                move || repaint.request_repaint(),
            ));
            self.requested = Some(wanted);
        }
        self.sync_flow(ctx, source);
        self.sync_section(ctx, source);
    }

    /// Takes a finished section, and starts the one the plane asks for. One is computed at a
    /// time, so dragging the plane asks for the latest place and not for every one on the way.
    fn sync_section(&mut self, ctx: &egui::Context, source: &Source) {
        if let Some(rx) = &self.case_rx
            && let Ok(read) = rx.try_recv()
        {
            match read {
                Ok(case) => self.case = Some(case),
                Err(message) => self.section = Some(Err(message)),
            }
            self.case_rx = None;
        }
        if let Some(rx) = &self.section_rx
            && let Ok(done) = rx.try_recv()
        {
            self.section = Some(done);
            self.section_rx = None;
        }
        if !(self.slice && self.field.is_some()) {
            self.camera.cut = None;
            self.section_asked = None;
            return;
        }
        if self.case.is_none() && self.case_rx.is_none() && !matches!(self.section, Some(Err(_))) {
            let repaint = ctx.clone();
            self.case_rx = Some(section::load(source.dir.clone(), move || {
                repaint.request_repaint()
            }));
        }
        let wanted = (self.slice_axis, (self.slice_pos * 1000.0).round() as i32);
        if let Some(case) = &self.case
            && self.section_rx.is_none()
            && self.section_asked != Some(wanted)
        {
            let repaint = ctx.clone();
            self.section_rx = Some(section::spawn(
                case.clone(),
                wanted.0,
                f64::from(wanted.1) / 1000.0,
                move || repaint.request_repaint(),
            ));
            self.section_asked = Some(wanted);
        }
        // The cut follows the section on show, so the plane and the half taken away agree.
        self.camera.cut = match &self.section {
            Some(Ok(s)) => Some(Cut {
                axis: s.axis,
                offset: s.offset,
            }),
            _ => None,
        };
    }

    /// The cells the plane crosses, coloured with the field.
    fn draw_section(&self, painter: &egui::Painter, rect: egui::Rect, screen: [f32; 2]) {
        let (Some(Ok(section)), Some(Ok(values))) = (&self.section, &self.values) else {
            return;
        };
        if !self.slice {
            return;
        }
        let to_pixel = self.camera.pixel_projection(&self.surface, screen);
        let at = |p: [f32; 2]| Pos2::new(rect.min.x + p[0], rect.min.y + p[1]);
        let mut mesh = Mesh::default();
        for polygon in &section.polygons {
            let color = match values.fraction(polygon.cell) {
                Some(t) => {
                    let [r, g, b] = fieldview::scale(t);
                    Color32::from_rgb(r, g, b)
                }
                None => Color32::from_rgb(0x55, 0x5b, 0x63),
            };
            let first = mesh.vertices.len() as u32;
            for p in &polygon.points {
                mesh.colored_vertex(at(to_pixel(p)), color);
            }
            for k in 1..polygon.points.len().saturating_sub(1) as u32 {
                mesh.add_triangle(first, first + k, first + k + 1);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        if self.outlines && section.polygons.len() <= MAX_OUTLINED_FACES {
            let ink = Stroke::new(0.75, Color32::from_black_alpha(110));
            for polygon in &section.polygons {
                let points: Vec<Pos2> = polygon.points.iter().map(|p| at(to_pixel(p))).collect();
                if points.len() >= 2 {
                    painter.add(egui::Shape::closed_line(points, ink));
                }
            }
        }
    }

    /// Takes a finished flow, and starts the one the choices ask for: lines or arrows wanted
    /// and a vector field on the surface.
    fn sync_flow(&mut self, ctx: &egui::Context, source: &Source) {
        if let Some(rx) = &self.flow_rx
            && let Ok(done) = rx.try_recv()
        {
            self.flow = Some(done);
            self.flow_rx = None;
        }
        let vector = matches!(&self.values, Some(Ok(v)) if v.kind == "vector");
        let wanted = match (&self.field, vector && (self.lines || self.arrows)) {
            (Some(name), true) => {
                let time = self
                    .time
                    .clone()
                    .or_else(|| source.times.last().cloned())
                    .unwrap_or_else(|| "0".to_owned());
                Some((name.clone(), time))
            }
            _ => None,
        };
        let Some(wanted) = wanted else {
            self.flow = None;
            self.flow_rx = None;
            self.flow_requested = None;
            return;
        };
        if self.flow_requested.as_ref() != Some(&wanted) {
            let repaint = ctx.clone();
            self.flow = None;
            self.flow_rx = Some(flowlines::spawn(
                source.dir.clone(),
                wanted.1.clone(),
                wanted.0.clone(),
                move || repaint.request_repaint(),
            ));
            self.flow_requested = Some(wanted);
        }
    }

    /// Streamlines and arrows over the picture.
    fn draw_flow(&self, painter: &egui::Painter, rect: egui::Rect, screen: [f32; 2]) {
        let Some(Ok(flow)) = &self.flow else {
            return;
        };
        let to_pixel = self.camera.pixel_projection(&self.surface, screen);
        let at = |p: [f32; 2]| Pos2::new(rect.min.x + p[0], rect.min.y + p[1]);
        if self.lines {
            let halo = Stroke::new(2.6, Color32::from_black_alpha(90));
            let ink = Stroke::new(1.2, Color32::from_white_alpha(235));
            for line in &flow.lines {
                let points: Vec<Pos2> = line.points.iter().map(|p| at(to_pixel(p))).collect();
                painter.add(egui::Shape::line(points.clone(), halo));
                painter.add(egui::Shape::line(points, ink));
            }
        }
        if self.arrows {
            let reach = self.surface.radius * 0.01;
            // About one seed spacing, so neighbours do not run into each other.
            let arrow_size =
                (screen[0].min(screen[1]) * 0.9 * self.camera.zoom / 18.0 * 0.85).clamp(5.0, 24.0);
            for arrow in &flow.arrows {
                let tip = [
                    arrow.at[0] + arrow.direction[0] * reach,
                    arrow.at[1] + arrow.direction[1] * reach,
                    arrow.at[2] + arrow.direction[2] * reach,
                ];
                let (from, to) = (to_pixel(&arrow.at), to_pixel(&tip));
                let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
                let length = (dx * dx + dy * dy).sqrt();
                // An arrow pointing at the viewer is a dot, not a direction.
                if !length.is_finite() || length < 1e-6 {
                    continue;
                }
                let (ux, uy) = (dx / length, dy / length);
                let size = arrow_size;
                let head = [from[0] + ux * size, from[1] + uy * size];
                let [r, g, b] = fieldview::scale(flow.fraction(arrow.speed));
                let colour = Color32::from_rgb(r, g, b);
                let halo = Stroke::new(3.4, Color32::from_black_alpha(140));
                let ink = Stroke::new(1.8, colour);
                let wing = |sign: f32| {
                    at([
                        head[0] - ux * arrow_size * 0.35 - sign * uy * arrow_size * 0.22,
                        head[1] - uy * arrow_size * 0.35 + sign * ux * arrow_size * 0.22,
                    ])
                };
                for stroke in [halo, ink] {
                    painter.line_segment([at(from), at(head)], stroke);
                    painter.line_segment([at(head), wing(1.0)], stroke);
                    painter.line_segment([at(head), wing(-1.0)], stroke);
                }
            }
        }
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Hides or shows a patch of the surface.
    pub fn set_visible(&mut self, patch: usize, visible: bool) {
        if let Some(slot) = self.visible.get_mut(patch) {
            *slot = visible;
        }
    }

    fn visible_faces(&self) -> usize {
        self.surface
            .patches
            .iter()
            .zip(&self.visible)
            .filter(|(_, shown)| **shown)
            .map(|(patch, _)| patch.faces)
            .sum()
    }

    /// The viewer and its list of patches, filling the room the ui has left.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        background: Color32,
        muted: Color32,
        radius: u8,
        source: &Source,
    ) {
        self.sync_field(&ui.ctx().clone(), source);
        // The picture takes all the room; the choices float over its corner.
        let room = ui.available_size();
        self.picture(ui, room, background, muted, radius);
        self.overlay(ui, muted, source);
    }

    /// The choices over the picture's top right corner: a button that opens them, and the
    /// colour scale along the bottom while they are shut.
    fn overlay(&mut self, ui: &mut egui::Ui, muted: Color32, source: &Source) {
        let rect = self.picture_rect;
        let ctx = ui.ctx().clone();
        let open = self.legend_open;
        let mut toggle = false;
        egui::Area::new(egui::Id::unique("viewer-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.right_top() + vec2(-8.0, 8.0))
            .pivot(egui::Align2::RIGHT_TOP)
            .show(&ctx, |ui| {
                ui.set_max_width(LEGEND + 24.0);
                // An area's room is what it took last frame, so ask for the picture's height
                // or the scroll area inside would never grow past its first size.
                ui.set_max_height((rect.height() - 16.0).max(80.0));
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(if open { LEGEND + 6.0 } else { 90.0 });
                    ui.horizontal(|ui| {
                        let label = if open { tr("Hide") } else { tr("Display") };
                        toggle = ui.small_button(label).clicked();
                    });
                    if open {
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .id_salt("viewer-legend")
                            .max_height((rect.height() - 70.0).max(80.0))
                            .show(ui, |ui| self.legend(ui, muted, source));
                    }
                });
            });
        if toggle {
            self.legend_open = !self.legend_open;
        }
        // With the choices shut the colour scale still says what the colours mean.
        let scale = match (&self.paint, &self.values) {
            (Some(paint), _) => Some((paint.name.clone(), paint.range())),
            (None, Some(Ok(values))) => Some((
                if values.kind == "vector" {
                    format!("{}, length", values.name)
                } else {
                    values.name.clone()
                },
                values.range,
            )),
            _ => None,
        };
        if !self.legend_open
            && let Some((what, range)) = scale
        {
            egui::Area::new(egui::Id::unique("viewer-scale"))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.left_bottom() + vec2(8.0, -8.0))
                .pivot(egui::Align2::LEFT_BOTTOM)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(LEGEND - 12.0);
                        scale_bar(ui, &what, range, muted);
                    });
                });
        }
    }

    fn picture(
        &mut self,
        ui: &mut egui::Ui,
        size: Vec2,
        background: Color32,
        muted: Color32,
        radius: u8,
    ) {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.picture_rect = rect;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(radius), background);

        // The mouse: the primary button turns the surface, the others slide it, the wheel zooms.
        let drag = response.drag_delta();
        let pixels_per_radius =
            (rect.width().min(rect.height()) / 2.0 * self.camera.zoom * 0.9).max(1.0);
        if response.dragged_by(PointerButton::Primary) {
            self.camera.orbit(drag.x * 0.01, drag.y * 0.01);
        } else if response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
        {
            self.camera.pan[0] += drag.x / pixels_per_radius;
            self.camera.pan[1] -= drag.y / pixels_per_radius;
        }
        if response.hovered() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                self.camera.zoom_by((wheel * 0.002).exp());
            }
        }
        if response.double_clicked() {
            self.camera = Camera::fit(&self.surface);
        }

        let screen = [rect.width(), rect.height()];
        let triangles = self.camera.project(&self.surface, screen, &self.visible);
        let mut mesh = Mesh::default();
        mesh.reserve_triangles(triangles.len());
        mesh.reserve_vertices(triangles.len().saturating_mul(3));
        // The wall of the paint each patch is, when it is one.
        let wall_of: Vec<Option<usize>> = self
            .surface
            .patches
            .iter()
            .map(|patch| self.paint.as_ref().and_then(|p| p.wall_index(&patch.name)))
            .collect();
        for triangle in &triangles {
            let color = if let Some(paint) = &self.paint {
                let fraction = wall_of
                    .get(triangle.patch)
                    .copied()
                    .flatten()
                    .and_then(|wall| paint.fraction(wall, triangle.face));
                match fraction {
                    Some(t) => {
                        let [r, g, b] = fieldview::scale(t);
                        shade(Color32::from_rgb(r, g, b), 0.85 + 0.15 * triangle.light)
                    }
                    None => shade(Color32::from_rgb(0x55, 0x5b, 0x63), triangle.light),
                }
            } else {
                match &self.values {
                    Some(Ok(values)) => match values.fraction(triangle.cell) {
                        Some(t) => {
                            let [r, g, b] = fieldview::scale(t);
                            // A light touch of shading: the colour is the information.
                            shade(Color32::from_rgb(r, g, b), 0.85 + 0.15 * triangle.light)
                        }
                        None => Color32::from_rgb(0x55, 0x5b, 0x63),
                    },
                    _ => {
                        let kind = self
                            .surface
                            .patches
                            .get(triangle.patch)
                            .map_or("", |p| p.kind.as_str());
                        shade(patch_color(kind, triangle.patch), triangle.light)
                    }
                }
            };
            let first = mesh.vertices.len() as u32;
            for [x, y] in triangle.points {
                mesh.colored_vertex(Pos2::new(rect.min.x + x, rect.min.y + y), color);
            }
            mesh.add_triangle(first, first + 1, first + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
        self.draw_section(&painter, rect, screen);

        // The borders of the patches, in the colour of the patch: a wall seen edge on is a line.
        // The patches that close the domain come first, so they never cover the others.
        let borders = self.camera.borders(&self.surface, screen, &self.visible);
        for closing in [true, false] {
            for ([a, b], patch) in &borders {
                let kind = self
                    .surface
                    .patches
                    .get(*patch)
                    .map_or("", |p| p.kind.as_str());
                if is_closing(kind) != closing {
                    continue;
                }
                let width = if closing { 1.0 } else { 2.5 };
                painter.line_segment(
                    [
                        Pos2::new(rect.min.x + a[0], rect.min.y + a[1]),
                        Pos2::new(rect.min.x + b[0], rect.min.y + b[1]),
                    ],
                    Stroke::new(width, patch_color(kind, *patch)),
                );
            }
        }
        if self.outlines && self.visible_faces() <= MAX_OUTLINED_FACES {
            let ink = Stroke::new(0.75, Color32::from_black_alpha(110));
            for [a, b] in self.camera.outlines(&self.surface, screen, &self.visible) {
                painter.line_segment(
                    [
                        Pos2::new(rect.min.x + a[0], rect.min.y + a[1]),
                        Pos2::new(rect.min.x + b[0], rect.min.y + b[1]),
                    ],
                    ink,
                );
            }
        }

        self.draw_flow(&painter, rect, screen);
        self.pick_edges(ui, &response, &painter, rect, screen);

        let mut note = trf(
            "{} faces  ·  drag to turn, right-drag to slide, wheel to zoom, double-click to reset",
            &[&self.surface.face_count()],
        );
        if self.surface.truncated {
            note.push_str(tr("  ·  the mesh is larger than the viewer shows"));
        }
        // The hint is for the person at the screen, not for the saved picture.
        if self.picture_rx.is_none() {
            painter.text(
                Pos2::new(rect.min.x + 12.0, rect.max.y - 10.0),
                egui::Align2::LEFT_BOTTOM,
                note,
                egui::FontId::proportional(11.0),
                muted,
            );
        }
    }

    /// While the person picks edges: draws the ones that can be picked, finds the one under the
    /// pointer and keeps the one clicked.
    fn pick_edges(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        rect: egui::Rect,
        screen: [f32; 2],
    ) {
        let Some(picking) = &mut self.picking else {
            return;
        };
        let project = self.camera.pixel_projection(&self.surface, screen);
        let to_screen = |p: &[f64; 3]| project(&[p[0] as f32, p[1] as f32, p[2] as f32]);
        let towards = self.camera.towards().map(f64::from);
        let on = |p: [f32; 2]| Pos2::new(rect.min.x + p[0], rect.min.y + p[1]);
        picking.hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|_| response.hovered())
            .and_then(|at| {
                edgepick::nearest(
                    &picking.edges,
                    to_screen,
                    &towards,
                    [at.x - rect.min.x, at.y - rect.min.y],
                    PICK_REACH,
                )
            });
        if response.clicked()
            && let Some(edge) = picking.hover.and_then(|n| picking.edges.get(n))
        {
            picking.clicked = Some(edge.clone());
        }
        let loose = Stroke::new(1.5, Color32::from_rgb(0x8F, 0xB8, 0xFF));
        let chosen = Stroke::new(3.5, Color32::from_rgb(0xF2, 0xA6, 0x3B));
        let hover = Stroke::new(3.5, Color32::from_rgb(0xFF, 0xFF, 0xFF));
        for (n, edge) in picking.edges.iter().enumerate() {
            if !edge.faces(&towards) {
                continue;
            }
            let is_chosen = picking.chosen.iter().any(|p| edge.distance(p) < 1.0e-4);
            let stroke = if picking.hover == Some(n) {
                hover
            } else if is_chosen {
                chosen
            } else {
                loose
            };
            painter.line_segment([on(to_screen(&edge.a)), on(to_screen(&edge.b))], stroke);
        }
    }

    /// The choice of field and time, and the colour scale of the field read.
    fn field_choices(&mut self, ui: &mut egui::Ui, muted: Color32, source: &Source) {
        if let Some(paint) = &self.paint {
            scale_bar(ui, &paint.name, paint.range(), muted);
            ui.add_space(4.0);
            if ui.button(tr("Back to the field")).clicked() {
                self.paint = None;
            }
            ui.add_space(10.0);
        }
        ui.label(egui::RichText::new(tr("Colour by")).color(muted));
        let current = self.field.clone();
        egui::ComboBox::from_id_salt("viewer-field")
            .width(LEGEND - 12.0)
            .selected_text(current.as_deref().unwrap_or(tr("Patches")))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.field, None, tr("Patches"));
                // A file with a space in its name (a copy of another) is not a field.
                for name in source.fields.iter().filter(|n| !n.contains(' ')) {
                    ui.selectable_value(&mut self.field, Some(name.clone()), name);
                }
            });
        if self.field.is_some() {
            let shown = self
                .time
                .clone()
                .or_else(|| source.times.last().cloned())
                .unwrap_or_else(|| "0".to_owned());
            ui.add_space(4.0);
            egui::ComboBox::from_id_salt("viewer-time")
                .width(LEGEND - 12.0)
                .selected_text(trf("Time {}", &[&shown]))
                .show_ui(ui, |ui| {
                    for time in source.times.iter().rev() {
                        ui.selectable_value(&mut self.time, Some(time.clone()), time);
                    }
                });
            ui.add_space(6.0);
            match &self.values {
                Some(Ok(values)) => {
                    let what = if values.kind == "vector" {
                        format!("{}, length", values.name)
                    } else {
                        values.name.clone()
                    };
                    scale_bar(ui, &what, values.range, muted);
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.slice, tr("Slice"));
                    if self.slice {
                        ui.horizontal(|ui| {
                            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                                ui.selectable_value(&mut self.slice_axis, axis, name);
                            }
                        });
                        ui.spacing_mut().slider_width = LEGEND - 12.0;
                        ui.add(egui::Slider::new(&mut self.slice_pos, 0.0..=1.0).show_value(false));
                        match &self.section {
                            Some(Ok(s)) => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} = {}",
                                        ["x", "y", "z"].get(s.axis).copied().unwrap_or("?"),
                                        fieldview::label(s.offset)
                                    ))
                                    .size(11.0)
                                    .color(muted),
                                );
                            }
                            Some(Err(message)) => {
                                ui.label(egui::RichText::new(message).size(11.0).color(muted));
                            }
                            None => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(egui::RichText::new(tr("Cutting…")).color(muted));
                                });
                            }
                        }
                    }
                    if values.kind == "vector" {
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.lines, tr("Streamlines"));
                        ui.checkbox(&mut self.arrows, tr("Arrows"));
                        match &self.flow {
                            Some(Err(message)) => {
                                ui.label(egui::RichText::new(message).size(11.0).color(muted));
                            }
                            None if self.lines || self.arrows => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(egui::RichText::new(tr("Following…")).color(muted));
                                });
                            }
                            _ => {}
                        }
                    }
                }
                Some(Err(message)) => {
                    ui.label(egui::RichText::new(message).size(11.0).color(muted));
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new(tr("Reading…")).color(muted));
                    });
                }
            }
        }
        ui.add_space(10.0);
    }

    /// "Save picture" and what came of the last one.
    fn picture_button(&mut self, ui: &mut egui::Ui, muted: Color32, source: &Source) {
        if let Some(rx) = &self.picture_rx
            && let Ok(done) = rx.try_recv()
        {
            self.picture_note = Some(done);
            self.picture_rx = None;
        }
        let waiting = self.picture_rx.is_some();
        let ready = !waiting && !source.dir.as_os_str().is_empty();
        if ui
            .add_enabled(ready, egui::Button::new(tr("Save picture")))
            .clicked()
        {
            let time = self
                .time
                .clone()
                .or_else(|| source.times.last().cloned())
                .unwrap_or_else(|| "mesh".to_owned());
            let what = match (&self.paint, &self.field) {
                (Some(paint), _) => paint.name.replace(' ', "-").to_lowercase(),
                (None, field) => field.clone().unwrap_or_else(|| "patches".to_owned()),
            };
            let path = crate::picture::path_for(&source.dir, &what, &time);
            self.picture_note = None;
            // Repaint first: the picture is taken from the frame after this one, without the hint.
            self.picture_rx = Some(crate::picture::request(
                ui.ctx(),
                self.picture_rect,
                path,
                source.dir.clone(),
            ));
        }
        if let Some((ok, message)) = &self.picture_note {
            let color = if *ok {
                muted
            } else {
                Color32::from_rgb(0xe0, 0x5b, 0x5b)
            };
            ui.label(
                egui::RichText::new(message.as_str())
                    .size(11.0)
                    .color(color),
            );
        }
        ui.add_space(6.0);
    }

    fn legend(&mut self, ui: &mut egui::Ui, muted: Color32, source: &Source) {
        self.field_choices(ui, muted, source);
        if self.surface.face_count() <= MAX_OUTLINED_FACES {
            ui.checkbox(&mut self.outlines, tr("Cell edges"));
            ui.add_space(6.0);
        }
        self.picture_button(ui, muted, source);
        ui.label(egui::RichText::new(tr("Patches")).color(muted));
        {
            {
                for (index, (patch, shown)) in self
                    .surface
                    .patches
                    .iter()
                    .zip(self.visible.iter_mut())
                    .enumerate()
                {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let (swatch, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        ui.painter().rect_filled(
                            swatch,
                            CornerRadius::same(3),
                            patch_color(&patch.kind, index),
                        );
                        ui.checkbox(shown, &patch.name);
                    });
                    ui.add_space(-4.0);
                    ui.label(
                        egui::RichText::new(format!("{}  ·  {} faces", patch.kind, patch.faces))
                            .size(11.0)
                            .color(muted),
                    );
                }
            }
        }
    }
}

/// The colour scale as a bar, with the range under it.
fn scale_bar(ui: &mut egui::Ui, what: &str, range: Option<(f32, f32)>, muted: Color32) {
    let Some((low, high)) = range else {
        ui.label(
            egui::RichText::new(tr("No finite values."))
                .size(11.0)
                .color(muted),
        );
        return;
    };
    let (rect, _) = ui.allocate_exact_size(vec2(LEGEND - 12.0, 12.0), Sense::hover());
    let mut mesh = Mesh::default();
    const STEPS: usize = 24;
    for step in 0..=STEPS {
        let t = step as f32 / STEPS as f32;
        let x = rect.min.x + rect.width() * t;
        let [r, g, b] = fieldview::scale(t);
        let color = Color32::from_rgb(r, g, b);
        mesh.colored_vertex(Pos2::new(x, rect.min.y), color);
        mesh.colored_vertex(Pos2::new(x, rect.max.y), color);
    }
    for step in 0..STEPS as u32 {
        let i = step * 2;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(fieldview::label(low))
                .monospace()
                .size(11.0)
                .color(muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(fieldview::label(high))
                    .monospace()
                    .size(11.0)
                    .color(muted),
            );
        });
    });
    ui.label(egui::RichText::new(what).size(11.0).color(muted));
}

fn shade(color: Color32, light: f32) -> Color32 {
    let light = light.clamp(0.0, 1.0);
    let scale = |channel: u8| (f32::from(channel) * light).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(scale(color.r()), scale(color.g()), scale(color.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shading_darkens_and_never_overflows() {
        let c = Color32::from_rgb(200, 100, 50);
        assert_eq!(shade(c, 1.0), c);
        assert_eq!(shade(c, 0.5), Color32::from_rgb(100, 50, 25));
        assert_eq!(shade(c, 7.0), c);
        assert_eq!(shade(c, f32::NAN.max(-1.0)), Color32::BLACK);
    }

    #[test]
    fn closing_patches_are_grey_and_the_others_take_turns() {
        assert_eq!(patch_color("empty", 0), patch_color("wedge", 5));
        assert_ne!(patch_color("patch", 0), patch_color("patch", 1));
        assert_eq!(patch_color("wall", 0), patch_color("wall", PALETTE.len()));
    }
}
