//! Home's "New > Model a part": the flow around a solid made here, with Nodalia's kernel.
//!
//! The person stacks features (boxes, cylinders, cones, spheres, tori, outlines pushed along an
//! axis or turned about it) and says for each whether it is added, cut away or intersected
//! (`vortia_cad::part`). The part is built on a thread every time it changes and shown in the
//! viewer; a wing section from the NACA family or an outline read from a DXF drawing are one click.
//! The finished surface is the body of a flow case (`vortia_cases::Body`) or an STL to take away.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use eframe::egui::{self, DragValue, Ui};
use nodalia_theme::{Palette, component, for_egui, space};
use vortia_cad::part::{self, Feature, Join, Part, Shape};
use vortia_cases::{Body, Geometry};
use vortia_mesh::stl::{Stl, Summary};

use super::imported::body_fields;
use super::newcase::{region_canvas, region_toolbar, row, value};
use crate::edgepick::{self, FeatureEdge};
use crate::i18n::{tr, trf};
use crate::region_editor::{self, RegionEditor};
use crate::surface::Surface;
use crate::viewer::{Source, Viewer};

/// How far a facet of the model may stand off the exact surface, as a share of its size.
const CHORD: f64 = 0.001;

/// The finer chord of the picture edges are picked on: a point on a piece of a curved edge stands
/// off the true edge by less than the kernel's reach for it.
const PICK_CHORD: f64 = 1.0e-4;

/// The height of the picture of the part.
const VIEW_HEIGHT: f32 = 360.0;

/// The model, as the kernel made it.
struct Built {
    stl: Stl,
    summary: Summary,
    viewer: Viewer,
}

/// A round or a chamfer being filled in by clicking the edges of the part before it.
struct PickView {
    /// The feature the clicks go to.
    feature: usize,
    /// The features before it, which the picture shows.
    of: Option<Part>,
    job: Option<Receiver<Result<Stl, String>>>,
    viewer: Option<Viewer>,
    problem: Option<String>,
}

/// What the helpers that fill an outline remember.
struct Helpers {
    /// The four digits of a NACA section.
    naca: u32,
    chord: f64,
    /// The DXF file being typed.
    file: String,
    /// The outline being drawn, and the saved sketch file typed to open into it.
    region: RegionEditor,
    sketch_file: String,
    problem: Option<String>,
}

impl Default for Helpers {
    fn default() -> Self {
        Self {
            naca: 2412,
            chord: 1.0,
            file: String::new(),
            region: RegionEditor::new(region_editor::Outline::Blank),
            sketch_file: String::new(),
            problem: None,
        }
    }
}

/// What the list of features is asked to do after it is drawn.
enum Edit {
    Remove(usize),
    Up(usize),
    Down(usize),
    Pick(usize),
}

/// The form of a case made around a part modelled here.
pub(super) struct Modelled {
    part: Part,
    /// The flow settings; the surface is put in when the case is made.
    flow: Body,
    built: Option<Built>,
    /// The part the model shown, or the one being built, is of.
    of: Option<Part>,
    job: Option<Receiver<Result<Stl, String>>>,
    /// Why the part cannot be made, when it cannot.
    problem: Option<String>,
    source: Source,
    save_to: String,
    /// The part file being typed.
    part_file: String,
    saved: Option<(bool, String)>,
    helpers: Helpers,
    pick: Option<PickView>,
}

impl Default for Modelled {
    fn default() -> Self {
        Self {
            part: Part::default(),
            flow: Body::new(Stl::default()),
            built: None,
            of: None,
            job: None,
            problem: None,
            source: Source::default(),
            save_to: String::new(),
            part_file: String::new(),
            saved: None,
            helpers: Helpers::default(),
            pick: None,
        }
    }
}

/// The surface of a part, or why it cannot be made. Runs on a thread: a kernel that stops on a
/// part is an error the person can read, not a crash.
fn build(part: &Part) -> Result<Stl, String> {
    build_with(part, CHORD)
}

fn build_with(part: &Part, chord: f64) -> Result<Stl, String> {
    std::panic::catch_unwind(|| part.surface(chord))
        .unwrap_or_else(|_| {
            Err(vortia_mesh::MeshError::Invalid(
                tr("The kernel stopped on this part: change the last feature.").into(),
            ))
        })
        .map_err(|e| e.to_string())
}

fn vec3(ui: &mut Ui, v: &mut [f64; 3]) {
    ui.horizontal(|ui| {
        for (name, c) in ["x", "y", "z"].into_iter().zip(v.iter_mut()) {
            ui.label(name);
            value(ui, c, "m");
        }
    });
}

fn corners_ui(
    ui: &mut Ui,
    id: egui::Id,
    corners: &mut Vec<[f64; 2]>,
    names: [&str; 2],
    unit_label: &str,
) {
    egui::CollapsingHeader::new(trf("{} corners", &[&corners.len()]))
        .id_salt(id)
        .show(ui, |ui| {
            ui.label(unit_label);
            egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(180.0)
                .show(ui, |ui| {
                    let mut remove = None;
                    let many = corners.len() > 3;
                    for (i, corner) in corners.iter_mut().enumerate() {
                        let [a, b] = corner;
                        ui.horizontal(|ui| {
                            ui.label(names[0]);
                            value(ui, a, "m");
                            ui.label(names[1]);
                            value(ui, b, "m");
                            if many && ui.small_button(tr("Remove corner")).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        corners.remove(i);
                    }
                });
            if corners.len() < part::MAX_CORNERS && ui.button(tr("Add a corner")).clicked() {
                let last = corners.last().copied().unwrap_or([0.0, 0.0]);
                corners.push(last);
            }
        });
}

/// The points that name the edges of a round or a chamfer. Whether the person asked to pick them
/// in the viewer.
fn points_ui(ui: &mut Ui, id: egui::Id, points: &mut Vec<[f64; 3]>, picking: bool) -> bool {
    let mut pick = false;
    ui.vertical(|ui| {
        if ui
            .add_enabled(!picking, egui::Button::new(tr("Pick in the viewer")))
            .clicked()
        {
            pick = true;
        }
        ui.label(tr(
            "The edge nearest each point, within a millimetre, is rounded: type a point on it, such as the middle of the edge.",
        ));
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt(id.with("scroll"))
            .max_height(140.0)
            .show(ui, |ui| {
                for (i, point) in points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        vec3(ui, point);
                        if ui.small_button(tr("Remove corner")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = remove {
            points.remove(i);
        }
        if points.len() < part::MAX_PICKS && ui.button(tr("Add an edge")).clicked() {
            points.push(points.last().copied().unwrap_or([0.0; 3]));
        }
    });
    pick
}

/// The wing section and the DXF outline buttons of an extruded outline.
fn outline_helpers(ui: &mut Ui, id: egui::Id, corners: &mut Vec<[f64; 2]>, helpers: &mut Helpers) {
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("NACA"));
        ui.add(DragValue::new(&mut helpers.naca).range(1..=9999));
        ui.label(tr("Chord"));
        value(ui, &mut helpers.chord, "m");
        if ui.button(tr("Use this wing section")).clicked() {
            match part::naca4(helpers.naca, helpers.chord, 40) {
                Ok(section) => {
                    *corners = section;
                    helpers.problem = None;
                }
                Err(e) => helpers.problem = Some(e.to_string()),
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut helpers.file)
                .hint_text(tr("A .dxf drawing"))
                .desired_width(240.0),
        );
        if ui.button(tr("Use its largest outline")).clicked() {
            let read = vortia_cad::dxf::read(std::path::Path::new(helpers.file.trim()))
                .and_then(|drawing| part::outline_from_drawing(&drawing));
            match read {
                Ok(outline) => {
                    *corners = outline;
                    helpers.problem = None;
                }
                Err(e) => helpers.problem = Some(e.to_string()),
            }
        }
    });
    egui::CollapsingHeader::new(tr("Draw the outline"))
        .id_salt(id.with("draw"))
        .show(ui, |ui| draw_outline(ui, corners, helpers));
    if let Some(problem) = &helpers.problem {
        ui.colored_label(ui.visuals().error_fg_color, problem);
    }
}

/// The sketch editor of a new case, drawn here for an outline: millimetres on the screen, metres
/// in the part. For a turned outline `x` is the distance from the axis and `y` is along it.
fn draw_outline(ui: &mut Ui, corners: &mut Vec<[f64; 2]>, helpers: &mut Helpers) {
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Start from"));
        for outline in region_editor::Outline::ALL {
            if ui
                .selectable_label(helpers.region.outline == outline, outline.title())
                .clicked()
                && helpers.region.outline != outline
            {
                helpers.region.restart(outline);
            }
        }
        ui.add(
            egui::TextEdit::singleline(&mut helpers.sketch_file)
                .hint_text(tr("A saved sketch"))
                .desired_width(220.0),
        );
        if ui.button(tr("Open sketch")).clicked() {
            // A refusal stays in the editor's `problem`, shown below.
            let _ = helpers
                .region
                .open(std::path::Path::new(helpers.sketch_file.trim()));
        }
    });
    region_toolbar(ui, &mut helpers.region);
    region_canvas(ui, &mut helpers.region);
    if let Some(problem) = &helpers.region.problem {
        ui.colored_label(ui.visuals().error_fg_color, problem);
    } else if let Some(why) = helpers.region.open_because() {
        ui.colored_label(ui.visuals().warn_fg_color, why);
    }
    if ui.button(tr("Use the drawn outline")).clicked() {
        match part::outline_from_sketch(helpers.region.sketch()) {
            Ok(outline) => {
                *corners = outline;
                helpers.problem = None;
            }
            Err(e) => helpers.problem = Some(e.to_string()),
        }
    }
}

fn shape_title(shape: &Shape) -> &'static str {
    match shape {
        Shape::Box { .. } => tr("Box"),
        Shape::Cylinder { .. } => tr("Cylinder"),
        Shape::Cone { .. } => tr("Cone"),
        Shape::Sphere { .. } => tr("Sphere"),
        Shape::Torus { .. } => tr("Torus"),
        Shape::Extrude { .. } => tr("Extruded outline"),
        Shape::Revolve { .. } => tr("Turned outline"),
        Shape::Round { .. } => tr("Round edges"),
        Shape::Chamfer { .. } => tr("Chamfer edges"),
    }
}

fn join_title(join: Join) -> &'static str {
    match join {
        Join::Add => tr("Add"),
        Join::Cut => tr("Cut away"),
        Join::Keep => tr("Keep what both share"),
    }
}

/// The fields of a feature. Returns whether the person asked to pick its edges in the viewer.
fn shape_fields(
    ui: &mut Ui,
    id: egui::Id,
    shape: &mut Shape,
    helpers: &mut Helpers,
    picking: bool,
) -> bool {
    let mut pick = false;
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| match shape {
            Shape::Box { origin, size } => {
                row(ui, tr("Lower corner"), |ui| vec3(ui, origin));
                row(ui, tr("Size"), |ui| vec3(ui, size));
            }
            Shape::Cylinder {
                origin,
                radius,
                height,
            } => {
                row(ui, tr("Centre of the base"), |ui| vec3(ui, origin));
                row(ui, tr("Radius"), |ui| value(ui, radius, "m"));
                row(ui, tr("Height"), |ui| value(ui, height, "m"));
            }
            Shape::Cone {
                origin,
                base_radius,
                top_radius,
                height,
            } => {
                row(ui, tr("Centre of the base"), |ui| vec3(ui, origin));
                row(ui, tr("Radius at the base"), |ui| {
                    value(ui, base_radius, "m")
                });
                row(ui, tr("Radius at the top"), |ui| value(ui, top_radius, "m"));
                row(ui, tr("Height"), |ui| value(ui, height, "m"));
            }
            Shape::Sphere { center, radius } => {
                row(ui, tr("Centre"), |ui| vec3(ui, center));
                row(ui, tr("Radius"), |ui| value(ui, radius, "m"));
            }
            Shape::Torus {
                center,
                major_radius,
                minor_radius,
            } => {
                row(ui, tr("Centre"), |ui| vec3(ui, center));
                row(ui, tr("Radius of the ring"), |ui| {
                    value(ui, major_radius, "m")
                });
                row(ui, tr("Radius of the tube"), |ui| {
                    value(ui, minor_radius, "m")
                });
            }
            Shape::Extrude { corners, z, height } => {
                row(ui, tr("Plane of the outline (z)"), |ui| value(ui, z, "m"));
                row(ui, tr("Length it is pushed along z"), |ui| {
                    value(ui, height, "m")
                });
                row(ui, tr("Outline"), |ui| {
                    ui.vertical(|ui| {
                        corners_ui(
                            ui,
                            id.with("corners"),
                            corners,
                            ["x", "y"],
                            tr("The corners in order, in the plane of the outline."),
                        );
                        outline_helpers(ui, id.with("helpers"), corners, helpers);
                    });
                });
            }
            Shape::Round { points, size } | Shape::Chamfer { points, size } => {
                row(ui, tr("Size"), |ui| value(ui, size, "m"));
                row(ui, tr("A point on each edge"), |ui| {
                    pick = points_ui(ui, id.with("points"), points, picking);
                });
            }
            Shape::Revolve {
                corners,
                center,
                angle,
            } => {
                row(ui, tr("A point of the axis (along z)"), |ui| {
                    vec3(ui, center)
                });
                row(ui, tr("Turned through"), |ui| {
                    let mut degrees = angle.to_degrees();
                    if ui
                        .add(
                            DragValue::new(&mut degrees)
                                .range(1.0..=360.0)
                                .speed(1.0)
                                .suffix(" °"),
                        )
                        .changed()
                    {
                        *angle = degrees.to_radians();
                    }
                });
                row(ui, tr("Profile"), |ui| {
                    corners_ui(
                        ui,
                        id.with("corners"),
                        corners,
                        ["r", "z"],
                        tr("The corners in order: the distance from the axis, then the height."),
                    );
                });
            }
        });
    pick
}

/// A new feature of each kind, in a size that shows on the screen.
fn new_shape(which: usize) -> Option<Shape> {
    let tau = std::f64::consts::TAU;
    Some(match which {
        0 => Shape::Box {
            origin: [0.0; 3],
            size: [1.0, 0.5, 0.5],
        },
        1 => Shape::Cylinder {
            origin: [0.0; 3],
            radius: 0.25,
            height: 1.0,
        },
        2 => Shape::Cone {
            origin: [0.0; 3],
            base_radius: 0.3,
            top_radius: 0.1,
            height: 1.0,
        },
        3 => Shape::Sphere {
            center: [0.0; 3],
            radius: 0.5,
        },
        4 => Shape::Torus {
            center: [0.0; 3],
            major_radius: 0.5,
            minor_radius: 0.1,
        },
        5 => Shape::Extrude {
            corners: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.4], [0.0, 0.4]],
            z: 0.0,
            height: 1.0,
        },
        6 => Shape::Revolve {
            corners: vec![[0.0, 0.0], [0.3, 0.0], [0.3, 1.0], [0.0, 1.0]],
            center: [0.0; 3],
            angle: tau,
        },
        7 => Shape::Round {
            points: vec![[0.0; 3]],
            size: 0.05,
        },
        8 => Shape::Chamfer {
            points: vec![[0.0; 3]],
            size: 0.05,
        },
        _ => return None,
    })
}

/// How many kinds of shape [`new_shape`] makes, the last two rounding and bevelling what is there.
#[cfg(test)]
const KINDS: usize = 9;

/// A wing of constant NACA 2412 section, a metre of chord and two of span.
fn wing() -> Option<Feature> {
    Some(Feature {
        shape: Shape::Extrude {
            corners: part::naca4(2412, 1.0, 40).ok()?,
            z: 0.0,
            height: 2.0,
        },
        join: Join::Add,
    })
}

/// Adds the middle of `edge` to the points, or takes away the points already on it.
fn toggle(points: &mut Vec<[f64; 3]>, edge: &FeatureEdge) {
    let before = points.len();
    points.retain(|p| edge.distance(p) >= 1.0e-4);
    if points.len() == before && points.len() < part::MAX_PICKS {
        points.push(edge.middle());
    }
}

impl Modelled {
    /// What the kernel made, or what went wrong.
    fn accept(&mut self, made: Result<Stl, String>) {
        match made {
            Ok(stl) => {
                let summary = stl.summary();
                let surface = Surface::from_stl(tr("Part"), &stl);
                let viewer = match self.built.take() {
                    Some(mut old) => {
                        old.viewer.replace_surface(surface);
                        old.viewer
                    }
                    None => Viewer::new(surface),
                };
                self.built = Some(Built {
                    stl,
                    summary,
                    viewer,
                });
                self.problem = None;
            }
            Err(problem) => self.problem = Some(problem),
        }
    }

    /// Takes in a finished build and starts the next when the part changed meanwhile.
    fn refresh(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok(done) => {
                    self.job = None;
                    self.accept(done);
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.problem = Some(tr("The part could not be built.").into());
                }
            }
        }
        if self.job.is_some() || self.of.as_ref() == Some(&self.part) {
            return;
        }
        self.of = Some(self.part.clone());
        if self.part.features.is_empty() {
            self.built = None;
            self.problem = None;
            return;
        }
        let (tx, rx) = channel();
        let part = self.part.clone();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("vortia-part".into())
            .spawn(move || {
                let _ = tx.send(build(&part));
                wake.request_repaint();
            });
        match spawned {
            Ok(_) => self.job = Some(rx),
            Err(_) => self.problem = Some(tr("The part could not be built.").into()),
        }
    }

    /// Starts choosing the edges of feature `i` (a round or a chamfer) by clicking them on the
    /// part as it is before that feature.
    fn start_pick(&mut self, i: usize) {
        if i == 0 {
            return;
        }
        if let Some(Feature {
            shape: Shape::Round { points, .. } | Shape::Chamfer { points, .. },
            ..
        }) = self.part.features.get_mut(i)
        {
            // The placeholder a new round starts with is not an edge anyone chose.
            if points.as_slice() == [[0.0; 3]] {
                points.clear();
            }
            self.pick = Some(PickView {
                feature: i,
                of: None,
                job: None,
                viewer: None,
                problem: None,
            });
        }
    }

    /// Builds the part before the feature being picked, and takes it in when it is done.
    fn refresh_pick(&mut self, ctx: &egui::Context) {
        let Some(pick) = &mut self.pick else {
            return;
        };
        let before = self.part.features.get(..pick.feature).map(|features| Part {
            features: features.to_vec(),
        });
        let still_rounding = matches!(
            self.part.features.get(pick.feature).map(|f| &f.shape),
            Some(Shape::Round { .. } | Shape::Chamfer { .. })
        );
        let Some(before) = before.filter(|_| still_rounding) else {
            self.pick = None;
            return;
        };
        if let Some(rx) = &pick.job {
            match rx.try_recv() {
                Ok(Ok(stl)) => {
                    pick.job = None;
                    pick.problem = None;
                    let edges = edgepick::feature_edges(&stl);
                    let surface = Surface::from_stl(tr("Part"), &stl);
                    let mut viewer = match pick.viewer.take() {
                        Some(mut old) => {
                            old.replace_surface(surface);
                            old
                        }
                        None => Viewer::new(surface),
                    };
                    viewer.pick_among(Some(edges));
                    pick.viewer = Some(viewer);
                }
                Ok(Err(problem)) => {
                    pick.job = None;
                    pick.viewer = None;
                    pick.problem = Some(problem);
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => {
                    pick.job = None;
                    pick.problem = Some(tr("The part could not be built.").into());
                }
            }
        }
        if pick.job.is_some() || pick.of.as_ref() == Some(&before) {
            return;
        }
        pick.of = Some(before.clone());
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("vortia-pick".into())
            .spawn(move || {
                let _ = tx.send(build_with(&before, PICK_CHORD));
                wake.request_repaint();
            });
        match spawned {
            Ok(_) => pick.job = Some(rx),
            Err(_) => pick.problem = Some(tr("The part could not be built.").into()),
        }
    }

    /// The case the form describes, or why it cannot be made yet.
    /// A wing built, with the coarsest mesh around it.
    #[cfg(test)]
    pub(super) fn prepare_for_test(&mut self) {
        self.part.features.extend(wing());
        self.of = Some(self.part.clone());
        self.accept(build(&self.part));
        self.flow.cells = 1;
        self.flow.levels = 0;
        self.flow.wake_levels = 0;
    }

    pub(super) fn geometry(&self) -> Result<Geometry, String> {
        if let Some(problem) = &self.problem {
            return Err(problem.clone());
        }
        let Some(built) = &self.built else {
            return Err(tr("Model a part first: add a feature.").into());
        };
        if self.job.is_some() || self.of.as_ref() != Some(&self.part) {
            return Err(tr("The part is still being built.").into());
        }
        let mut body = self.flow.clone();
        body.surface = built.stl.clone();
        // Refuse here, with the mesher's own words, rather than after a folder is made.
        body.spec().map_err(|e| e.to_string())?;
        Ok(Geometry::Body(body))
    }

    fn add_buttons(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Add"));
            let full = self.part.features.len() >= part::MAX_FEATURES;
            let names = [
                tr("Box"),
                tr("Cylinder"),
                tr("Cone"),
                tr("Sphere"),
                tr("Torus"),
                tr("Extruded outline"),
                tr("Turned outline"),
                tr("Round edges"),
                tr("Chamfer edges"),
            ];
            for (which, name) in names.into_iter().enumerate() {
                if ui.add_enabled(!full, egui::Button::new(name)).clicked()
                    && let Some(shape) = new_shape(which)
                {
                    self.part.features.push(Feature {
                        shape,
                        join: Join::Add,
                    });
                }
            }
            ui.add_space(space::MD);
            if ui
                .add_enabled(!full, egui::Button::new(tr("A wing")))
                .on_hover_text(tr(
                    "A wing of constant NACA 2412 section: a metre of chord, two of span.",
                ))
                .clicked()
                && let Some(feature) = wing()
            {
                self.part.features.push(feature);
            }
        });
    }

    fn feature_list(&mut self, ui: &mut Ui) {
        let mut edit = None;
        let last = self.part.features.len().saturating_sub(1);
        let picking = self.pick.as_ref().map(|p| p.feature);
        for (i, feature) in self.part.features.iter_mut().enumerate() {
            let id = ui.make_persistent_id(("modelled_feature", i));
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{}  {}", i + 1, shape_title(&feature.shape)))
                            .strong(),
                    );
                    if i == 0
                        || matches!(feature.shape, Shape::Round { .. } | Shape::Chamfer { .. })
                    {
                        // The first has nothing to cut or keep: it starts the part. A round or
                        // a chamfer works on what is there.
                        feature.join = Join::Add;
                    } else {
                        egui::ComboBox::from_id_salt(id.with("join"))
                            .selected_text(join_title(feature.join))
                            .show_ui(ui, |ui| {
                                for join in [Join::Add, Join::Cut, Join::Keep] {
                                    ui.selectable_value(&mut feature.join, join, join_title(join));
                                }
                            });
                    }
                    if i > 0 && ui.small_button(tr("Move up")).clicked() {
                        edit = Some(Edit::Up(i));
                    }
                    if i < last && ui.small_button(tr("Move down")).clicked() {
                        edit = Some(Edit::Down(i));
                    }
                    if ui.small_button(tr("Remove")).clicked() {
                        edit = Some(Edit::Remove(i));
                    }
                });
                if shape_fields(
                    ui,
                    id,
                    &mut feature.shape,
                    &mut self.helpers,
                    picking == Some(i),
                ) && i > 0
                {
                    edit = Some(Edit::Pick(i));
                }
            });
        }
        match edit {
            Some(Edit::Remove(i)) => {
                self.pick = None;
                self.part.features.remove(i);
            }
            Some(Edit::Up(i)) if i > 0 => {
                self.pick = None;
                self.part.features.swap(i, i - 1);
            }
            Some(Edit::Down(i)) if i < last => {
                self.pick = None;
                self.part.features.swap(i, i + 1);
            }
            Some(Edit::Pick(i)) => self.start_pick(i),
            _ => {}
        }
    }

    fn picture(&mut self, ui: &mut Ui, p: &Palette) {
        let working = self.job.is_some();
        if working {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Building the part…"));
            });
        }
        if let Some(problem) = &self.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
        if self.pick.is_some() {
            self.pick_picture(ui, p);
            return;
        }
        let Some(built) = &mut self.built else {
            return;
        };
        if !built.summary.closed() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                trf(
                    "The surface is not closed: {} open edges, {} shared by more than two triangles.",
                    &[&built.summary.open_edges, &built.summary.non_manifold_edges],
                ),
            );
        }
        if let Some(volume) = built.summary.volume {
            ui.label(trf(
                "Volume {} m³, surface {} m²",
                &[
                    &format!("{volume:.6}"),
                    &format!("{:.4}", built.summary.area),
                ],
            ));
        }
        let width = ui.available_width();
        ui.allocate_ui(egui::vec2(width, VIEW_HEIGHT), |ui| {
            built.viewer.show(
                ui,
                for_egui::color(p.surface_sunken),
                for_egui::color(p.text_secondary),
                component::CONTROL_RADIUS.round().clamp(0.0, 255.0) as u8,
                &self.source,
            );
        });
    }

    /// The part as it is before the round or chamfer being filled in, with its edges to click.
    fn pick_picture(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(pick) = &mut self.pick else {
            return;
        };
        let mut done = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(tr(
                "Click the edges to round or bevel; click a chosen edge again to let it go. The picture shows the part as it is before this step.",
            ));
            if ui.button(tr("Finish picking")).clicked() {
                done = true;
            }
        });
        if let Some(problem) = &pick.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
        if pick.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Building the part…"));
            });
        }
        let feature = pick.feature;
        let chosen = match self.part.features.get(feature).map(|f| &f.shape) {
            Some(Shape::Round { points, .. } | Shape::Chamfer { points, .. }) => points.clone(),
            _ => Vec::new(),
        };
        let mut clicked = None;
        if let Some(viewer) = &mut pick.viewer {
            viewer.set_chosen(&chosen);
            let width = ui.available_width();
            ui.allocate_ui(egui::vec2(width, VIEW_HEIGHT), |ui| {
                viewer.show(
                    ui,
                    for_egui::color(p.surface_sunken),
                    for_egui::color(p.text_secondary),
                    component::CONTROL_RADIUS.round().clamp(0.0, 255.0) as u8,
                    &self.source,
                );
            });
            clicked = viewer.take_click();
        }
        if let Some(edge) = clicked
            && let Some(Feature {
                shape: Shape::Round { points, .. } | Shape::Chamfer { points, .. },
                ..
            }) = self.part.features.get_mut(feature)
        {
            toggle(points, &edge);
        }
        if done {
            self.pick = None;
        }
    }

    /// The part file: saving and opening it.
    fn file_row(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.part_file)
                    .hint_text(tr("A part file (.vortia-part)"))
                    .desired_width(260.0),
            );
            let path = std::path::PathBuf::from(self.part_file.trim());
            if ui.button(tr("Open part")).clicked() {
                self.saved = Some(match Part::open(&path) {
                    Ok(part) => {
                        self.part = part;
                        (true, trf("Opened {}", &[&path.display()]))
                    }
                    Err(e) => (false, e.to_string()),
                });
            }
            if ui
                .add_enabled(
                    !self.part.features.is_empty(),
                    egui::Button::new(tr("Save part")),
                )
                .clicked()
            {
                self.saved = Some(match self.part.save(&path) {
                    Ok(()) => (true, trf("Saved {}", &[&path.display()])),
                    Err(e) => (false, e.to_string()),
                });
            }
        });
    }

    fn export(&mut self, ui: &mut Ui) {
        let Some(built) = &self.built else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.save_to)
                    .hint_text(tr("File to export to"))
                    .desired_width(260.0),
            );
            if ui.button(tr("Save as STL")).clicked() {
                let path = std::path::PathBuf::from(self.save_to.trim());
                self.saved = Some(match built.stl.write(&path) {
                    Ok(()) => (true, trf("Saved {}", &[&path.display()])),
                    Err(e) => (false, e.to_string()),
                });
            }
            if ui.button(tr("Save as STEP")).clicked() {
                let path = std::path::PathBuf::from(self.save_to.trim());
                self.saved = Some(match self.part.save_step(&path) {
                    Ok(()) => (true, trf("Saved {}", &[&path.display()])),
                    Err(e) => (false, e.to_string()),
                });
            }
        });
    }

    fn note(&self, ui: &mut Ui) {
        if let Some((ok, message)) = &self.saved {
            let colour = if *ok {
                ui.visuals().text_color()
            } else {
                ui.visuals().error_fg_color
            };
            ui.colored_label(colour, message);
        }
    }

    pub(super) fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        self.refresh(&ui.ctx().clone());
        self.refresh_pick(&ui.ctx().clone());
        self.add_buttons(ui);
        self.file_row(ui);
        ui.add_space(space::SM);
        if self.part.features.is_empty() {
            ui.label(tr(
                "Nothing yet. Add a shape, or a wing to start from; each next shape can be added to the part, cut out of it or intersected with it.",
            ));
        }
        self.feature_list(ui);
        ui.add_space(space::SM);
        self.picture(ui, p);
        self.export(ui);
        self.note(ui);
        if let Some(built) = &self.built {
            ui.add_space(space::SM);
            ui.label(tr("The flow comes along x."));
            egui::Grid::new("new_modelled_flow")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    body_fields(ui, &mut self.flow, &built.summary, 1.0)
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form after the wing was built, as the thread would have left it.
    fn built_wing() -> Modelled {
        let mut form = Modelled::default();
        form.part.features.extend(wing());
        form.of = Some(form.part.clone());
        form.accept(build(&form.part));
        form
    }

    #[test]
    fn nothing_is_made_before_a_part_is() {
        let form = Modelled::default();
        assert!(form.geometry().is_err());
    }

    #[test]
    fn a_wing_becomes_a_body_in_a_wind_tunnel_and_a_changed_part_is_not_taken_yet() {
        let mut form = built_wing();
        assert!(form.problem.is_none(), "{:?}", form.problem);
        let Geometry::Body(body) = form.geometry().unwrap() else {
            panic!("a part is a body");
        };
        assert!(body.surface.summary().closed());
        assert!(body.surface.summary().volume.is_some());
        // The picture is of the part as it was: the next one is not the flow's.
        if let Some(Feature {
            shape: Shape::Extrude { height, .. },
            ..
        }) = form.part.features.first_mut()
        {
            *height = 3.0;
        }
        assert!(form.geometry().is_err());
    }

    #[test]
    fn a_part_the_kernel_refuses_says_why_and_makes_no_case() {
        let mut form = Modelled::default();
        form.part.features.push(Feature {
            shape: Shape::Sphere {
                center: [0.0; 3],
                radius: -1.0,
            },
            join: Join::Add,
        });
        form.of = Some(form.part.clone());
        form.accept(build(&form.part));
        assert!(form.problem.is_some());
        assert!(form.geometry().is_err());
    }

    #[test]
    fn every_kind_of_shape_is_offered_and_none_more() {
        assert!((0..KINDS).all(|i| new_shape(i).is_some()));
        assert!(new_shape(KINDS).is_none());
        // Each solid builds on its own.
        for i in 0..7 {
            let part = Part {
                features: vec![Feature {
                    shape: new_shape(i).unwrap(),
                    join: Join::Add,
                }],
            };
            assert!(build(&part).is_ok(), "shape {i}");
        }
    }

    #[test]
    fn a_rounded_edge_is_a_feature_after_the_solid_it_rounds() {
        let mut form = Modelled::default();
        form.part.features.push(Feature {
            shape: new_shape(0).unwrap(),
            join: Join::Add,
        });
        // The top front edge of the box, 1 × 0.5 × 0.5 m.
        form.part.features.push(Feature {
            shape: Shape::Round {
                points: vec![[0.5, 0.0, 0.5]],
                size: 0.05,
            },
            join: Join::Add,
        });
        form.of = Some(form.part.clone());
        form.accept(build(&form.part));
        assert!(form.problem.is_none(), "{:?}", form.problem);
        let volume = form.built.as_ref().unwrap().summary.volume.unwrap();
        let exact = 0.25 - (1.0 - std::f64::consts::FRAC_PI_4) * 0.0025;
        assert!((volume - exact).abs() < 2.0e-4, "{volume} against {exact}");
        // Moved before the solid, it has nothing to round and says so.
        form.part.features.swap(0, 1);
        form.of = Some(form.part.clone());
        form.accept(build(&form.part));
        assert!(form.problem.is_some());
    }

    #[test]
    fn the_form_draws_without_panicking_and_builds_what_was_added() {
        let ctx = egui::Context::default();
        let palette = nodalia_theme::Mode::Dark.palette_with(crate::app::ACCENT);
        let mut form = Modelled::default();
        form.part.features.extend(wing());
        let run = |form: &mut Modelled| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| form.ui(ui, &palette));
            // Nobody paints these frames.
            output.textures_delta.clear();
        };
        run(&mut form);
        assert!(form.job.is_some(), "the part starts to build");
        for _ in 0..600 {
            std::thread::sleep(Duration::from_millis(50));
            run(&mut form);
            if form.job.is_none() {
                break;
            }
        }
        assert!(form.built.is_some(), "{:?}", form.problem);
        assert!(form.geometry().is_ok());
    }

    #[test]
    fn edges_picked_on_the_part_before_a_round_are_the_ones_it_rounds() {
        let ctx = egui::Context::default();
        let palette = nodalia_theme::Mode::Dark.palette_with(crate::app::ACCENT);
        let mut form = Modelled::default();
        form.part.features.push(Feature {
            shape: Shape::Box {
                origin: [0.0; 3],
                size: [1.0; 3],
            },
            join: Join::Add,
        });
        form.part.features.push(Feature {
            shape: new_shape(7).unwrap(),
            join: Join::Add,
        });
        form.start_pick(1);
        // The placeholder point a new round starts with is not an edge.
        assert!(matches!(
            form.part.features.get(1).map(|f| &f.shape),
            Some(Shape::Round { points, .. }) if points.is_empty()
        ));
        let run = |form: &mut Modelled| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| form.ui(ui, &palette));
            output.textures_delta.clear();
        };
        for _ in 0..600 {
            run(&mut form);
            let ready = form.pick.as_ref().is_some_and(|p| p.viewer.is_some());
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            form.pick.as_ref().is_some_and(|p| p.viewer.is_some()),
            "{:?}",
            form.pick.as_ref().map(|p| &p.problem)
        );
        // What the viewer would hand over for a click on an edge of the block.
        let block = build_with(
            &Part {
                features: form.part.features.get(..1).unwrap_or(&[]).to_vec(),
            },
            PICK_CHORD,
        )
        .unwrap();
        let edges = edgepick::feature_edges(&block);
        assert_eq!(edges.len(), 12);
        let edge = edges.first().unwrap().clone();
        for shape in form.part.features.iter_mut().skip(1).map(|f| &mut f.shape) {
            if let Shape::Round { points, .. } = shape {
                toggle(points, &edge);
                assert_eq!(points.len(), 1);
            }
        }
        for _ in 0..600 {
            run(&mut form);
            if form.job.is_none() && form.of.as_ref() == Some(&form.part) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(form.problem.is_none(), "{:?}", form.problem);
        let volume = form.built.as_ref().and_then(|b| b.summary.volume).unwrap();
        let rounded = 1.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 0.05 * 0.05;
        assert!(
            (volume - rounded).abs() < 1.0e-3,
            "{volume} against {rounded}"
        );
        // Clicking the same edge again lets it go.
        for shape in form.part.features.iter_mut().skip(1).map(|f| &mut f.shape) {
            if let Shape::Round { points, .. } = shape {
                toggle(points, &edge);
                assert!(points.is_empty());
            }
        }
    }

    #[test]
    fn a_saved_sketch_opened_in_the_outline_editor_becomes_the_outline() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plate.vortia-sketch");
        let mut sketch = vortia_sketch::Sketch::default();
        sketch
            .draw_rectangle((0.0, 0.0), (60.0, 30.0), 1.0e-6)
            .unwrap();
        sketch.save(&file).unwrap();

        let mut helpers = Helpers::default();
        helpers.region.open(&file).unwrap();
        assert!(helpers.region.problem.is_none());
        // The editor is drawn (toolbar, canvas, status) without panicking.
        let ctx = egui::Context::default();
        let mut corners: Vec<[f64; 2]> = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_outline(ui, &mut corners, &mut helpers)
        });
        output.textures_delta.clear();
        assert!(corners.is_empty(), "nothing is taken until the button is");

        let outline = part::outline_from_sketch(helpers.region.sketch()).unwrap();
        let mut part = Part::default();
        part.features.push(Feature {
            shape: Shape::Extrude {
                corners: outline,
                z: 0.0,
                height: 0.01,
            },
            join: Join::Add,
        });
        let surface = part.surface(0.001).unwrap();
        assert!(surface.summary().closed());
        let (low, high) = (surface.summary().low, surface.summary().high);
        assert!((high[0] - low[0] - 0.06).abs() < 1e-6);
        assert!((high[1] - low[1] - 0.03).abs() < 1e-6);
    }
}
