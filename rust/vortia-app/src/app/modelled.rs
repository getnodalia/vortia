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
use super::newcase::{row, value};
use crate::i18n::{tr, trf};
use crate::surface::Surface;
use crate::viewer::{Source, Viewer};

/// How far a facet of the model may stand off the exact surface, as a share of its size.
const CHORD: f64 = 0.001;

/// The height of the picture of the part.
const VIEW_HEIGHT: f32 = 360.0;

/// The model, as the kernel made it.
struct Built {
    stl: Stl,
    summary: Summary,
    viewer: Viewer,
}

/// What the helpers that fill an outline remember.
struct Helpers {
    /// The four digits of a NACA section.
    naca: u32,
    chord: f64,
    /// The DXF file being typed.
    file: String,
    problem: Option<String>,
}

impl Default for Helpers {
    fn default() -> Self {
        Self {
            naca: 2412,
            chord: 1.0,
            file: String::new(),
            problem: None,
        }
    }
}

/// What the list of features is asked to do after it is drawn.
enum Edit {
    Remove(usize),
    Up(usize),
    Down(usize),
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
        }
    }
}

/// The surface of a part, or why it cannot be made. Runs on a thread: a kernel that stops on a
/// part is an error the person can read, not a crash.
fn build(part: &Part) -> Result<Stl, String> {
    std::panic::catch_unwind(|| part.surface(CHORD))
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

/// The points that name the edges of a round or a chamfer.
fn points_ui(ui: &mut Ui, id: egui::Id, points: &mut Vec<[f64; 3]>) {
    ui.vertical(|ui| {
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
}

/// The wing section and the DXF outline buttons of an extruded outline.
fn outline_helpers(ui: &mut Ui, corners: &mut Vec<[f64; 2]>, helpers: &mut Helpers) {
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
    if let Some(problem) = &helpers.problem {
        ui.colored_label(ui.visuals().error_fg_color, problem);
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

fn shape_fields(ui: &mut Ui, id: egui::Id, shape: &mut Shape, helpers: &mut Helpers) {
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
                        outline_helpers(ui, corners, helpers);
                    });
                });
            }
            Shape::Round { points, size } | Shape::Chamfer { points, size } => {
                row(ui, tr("Size"), |ui| value(ui, size, "m"));
                row(ui, tr("A point on each edge"), |ui| {
                    points_ui(ui, id.with("points"), points)
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
                shape_fields(ui, id, &mut feature.shape, &mut self.helpers);
            });
        }
        match edit {
            Some(Edit::Remove(i)) => {
                self.part.features.remove(i);
            }
            Some(Edit::Up(i)) if i > 0 => self.part.features.swap(i, i - 1),
            Some(Edit::Down(i)) if i < last => self.part.features.swap(i, i + 1),
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
                    .hint_text(tr("File for the STL"))
                    .desired_width(260.0),
            );
            if ui.button(tr("Save as STL")).clicked() {
                let path = std::path::PathBuf::from(self.save_to.trim());
                self.saved = Some(match built.stl.write(&path) {
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
}
