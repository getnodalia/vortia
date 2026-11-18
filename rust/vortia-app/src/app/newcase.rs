//! Home's "New": the form that starts a case (`vortia_cases`).

use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

use eframe::egui::{self, DragValue, Ui};
use nodalia_theme::engineering::Look;
use nodalia_theme::office;
use nodalia_theme::{Palette, space};
use vortia_cases::{Cavity, Channel, Drawn, Geometry, Jet, Nozzle, NozzleKind, Plate};

use crate::region_editor::{self, DrawnEdge, RegionEditor, Role};
use crate::sketch_editor::{Outline, SketchEditor, Template};

/// Which of the cases Home offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Channel,
    Cavity,
    Plate,
    PlanarNozzle,
    AxisymmetricNozzle,
    Jet,
    Sketch,
    Region,
}

impl Kind {
    const ALL: [Kind; 8] = [
        Kind::Channel,
        Kind::Cavity,
        Kind::Plate,
        Kind::PlanarNozzle,
        Kind::AxisymmetricNozzle,
        Kind::Jet,
        Kind::Sketch,
        Kind::Region,
    ];

    fn title(self) -> &'static str {
        match self {
            Kind::Channel => tr("Channel"),
            Kind::Cavity => tr("Cavity"),
            Kind::Plate => tr("Flat plate"),
            Kind::PlanarNozzle => tr("Planar nozzle"),
            Kind::AxisymmetricNozzle => tr("Axisymmetric nozzle"),
            Kind::Jet => tr("Jet into the atmosphere"),
            Kind::Sketch => tr("From sketch"),
            Kind::Region => tr("Drawn region"),
        }
    }

    fn about(self) -> &'static str {
        match self {
            Kind::Channel => {
                "A straight 2D channel with the fluid coming in on the left: laminar, incompressible, steady (simpleFoam)."
            }
            Kind::Cavity => {
                "A square box closed on every side whose top wall slides: laminar, incompressible, steady (simpleFoam). Reynolds number 100 by default, the case of Ghia et al. (1982)."
            }
            Kind::Plate => {
                "A flat plate in a uniform stream with a slip wall ahead of it: laminar, incompressible, steady (simpleFoam). Reynolds number 10⁵ on the plate by default, the boundary layer Blasius solved."
            }
            Kind::PlanarNozzle => {
                "A converging-diverging slot between two walls: compressible air, k-ω SST, steady (rhoSimpleFoam)."
            }
            Kind::AxisymmetricNozzle => {
                "A converging-diverging nozzle as a 5° wedge of its body of revolution: compressible air, k-ω SST, steady (rhoSimpleFoam)."
            }
            Kind::Jet => {
                "A small orifice in a wall that lets air out into the open atmosphere, as a 5° wedge of the body of revolution: compressible, k-ω SST, steady (rhoSimpleFoam). The practice case of a jet at a pressure ratio of 1.18."
            }
            Kind::Sketch => {
                "A duct whose wall you dimension in a sketch: lines and arcs held by lengths, heights and radii, meshed by Vortia itself and run as a nozzle (rhoSimpleFoam). The sketch is kept in the case."
            }
            Kind::Region => {
                "A closed outline you dimension in a sketch, with a name on every edge: `inlet` is where the fluid comes in, `outlet` where it leaves, `symmetry` a symmetry plane and any other name a wall. Meshed with triangles by Vortia itself and run as laminar, incompressible, steady flow (simpleFoam). The sketch is kept in the case."
            }
        }
    }
}

/// What the form holds.
pub(super) struct NewCase {
    kind: Kind,
    /// The folder the case is written to.
    folder: String,
    channel: Channel,
    cavity: Cavity,
    plate: Plate,
    planar: Nozzle,
    wedge: Nozzle,
    jet: Jet,
    sketch: SketchEditor,
    /// The flow and mesh settings of the sketched duct; its walls come from the sketch.
    sketched: Nozzle,
    sketch_axisymmetric: bool,
    segments: usize,
    region: RegionEditor,
    /// The flow and mesh settings of the drawn region; its edges come from the sketch.
    drawn: Drawn,
}

/// What the form asks for.
pub(super) enum Ask {
    Nothing,
    Create,
}

impl NewCase {
    /// A form that proposes `base/channel`, or the first name after it that is free.
    pub(super) fn new(base: &Path) -> Self {
        let mut form = Self {
            kind: Kind::Channel,
            folder: String::new(),
            channel: Channel::default(),
            cavity: Cavity::default(),
            plate: Plate::default(),
            planar: Nozzle::new(NozzleKind::Planar),
            wedge: Nozzle::new(NozzleKind::Axisymmetric),
            jet: Jet::default(),
            sketch: SketchEditor::default(),
            sketched: Nozzle::new(NozzleKind::Axisymmetric),
            sketch_axisymmetric: true,
            segments: 60,
            region: RegionEditor::default(),
            drawn: Drawn::default(),
        };
        form.propose(base);
        form
    }

    /// Where new cases go: the person's `Vortia` folder.
    pub(super) fn default_base() -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join("Vortia")
    }

    fn propose(&mut self, base: &Path) {
        let stem = match self.kind {
            Kind::Channel => "channel",
            Kind::Cavity => "cavity",
            Kind::Plate => "plate",
            Kind::PlanarNozzle => "planar-nozzle",
            Kind::AxisymmetricNozzle => "nozzle",
            Kind::Jet => "jet",
            Kind::Sketch => "sketch",
            Kind::Region => "region",
        };
        let free = (1..1000)
            .map(|n| {
                if n == 1 {
                    stem.to_owned()
                } else {
                    format!("{stem}-{n}")
                }
            })
            .map(|name| base.join(name))
            .find(|path| !path.exists())
            .unwrap_or_else(|| base.join(stem));
        self.folder = free.display().to_string();
    }

    pub(super) fn choose_sketch(&mut self) {
        self.kind = Kind::Sketch;
        self.propose(&Self::default_base());
    }

    pub(super) fn geometry(&self) -> Result<Geometry, String> {
        Ok(match self.kind {
            Kind::Channel => Geometry::Channel(self.channel.clone()),
            Kind::Cavity => Geometry::Cavity(self.cavity.clone()),
            Kind::Plate => Geometry::Plate(self.plate.clone()),
            Kind::PlanarNozzle => Geometry::Nozzle(self.planar.clone()),
            Kind::AxisymmetricNozzle => Geometry::Nozzle(self.wedge.clone()),
            Kind::Jet => Geometry::Jet(self.jet.clone()),
            Kind::Sketch => Geometry::Nozzle(self.sketched_nozzle()?),
            Kind::Region => Geometry::Drawn(self.drawn_region()?),
        })
    }

    /// The nozzle the sketch makes, with the flow and mesh settings of the form.
    fn sketched_nozzle(&self) -> Result<Nozzle, String> {
        let kind = if self.sketch_axisymmetric {
            NozzleKind::Axisymmetric
        } else {
            NozzleKind::Planar
        };
        let mut nozzle = Nozzle::from_sketch(kind, self.sketch.sketch(), self.segments)
            .map_err(|e| e.to_string())?;
        nozzle.inlet_speed = self.sketched.inlet_speed;
        nozzle.inlet_temperature = self.sketched.inlet_temperature;
        nozzle.outlet_pressure = self.sketched.outlet_pressure;
        nozzle.radial_cells = self.sketched.radial_cells;
        nozzle.cell_size = self.sketched.cell_size;
        Ok(nozzle)
    }

    /// The region the sketch makes, with the flow and mesh settings of the form.
    fn drawn_region(&self) -> Result<Drawn, String> {
        if let Some(missing) = self.region.missing() {
            return Err(missing.into());
        }
        let mut drawn = self.drawn.clone();
        drawn.sketch = self.region.sketch().clone();
        Ok(drawn)
    }

    /// Writes the case; the folder it made, or why not.
    pub(super) fn create(&self) -> Result<PathBuf, String> {
        let dir = PathBuf::from(self.folder.trim());
        if self.folder.trim().is_empty() {
            return Err(tr("Say which folder the case goes in.").into());
        }
        vortia_cases::create(&dir, &self.geometry()?).map_err(|e| e.to_string())?;
        // The drawing stays with the case it was meshed into.
        match self.kind {
            Kind::Sketch => self
                .sketch
                .save(&dir.join("sketch.vortia-sketch"))
                .map_err(|e| e.to_string())?,
            Kind::Region => self
                .region
                .save(&dir.join("sketch.vortia-sketch"))
                .map_err(|e| e.to_string())?,
            _ => {}
        }
        Ok(dir)
    }

    pub(super) fn ui(&mut self, ui: &mut Ui, look: &Look, p: &Palette) -> Ask {
        let home = nodalia_theme::engineering::home_look(look);
        let mut ask = Ask::Nothing;
        office::section(ui, &home, tr("New case"), |ui| {
            ui.horizontal(|ui| {
                for kind in Kind::ALL {
                    if ui
                        .selectable_label(self.kind == kind, kind.title())
                        .clicked()
                        && self.kind != kind
                    {
                        let base = Path::new(self.folder.trim())
                            .parent()
                            .map_or_else(Self::default_base, Path::to_path_buf);
                        self.kind = kind;
                        self.propose(&base);
                    }
                }
            });
            ui.add_space(space::SM);
            ui.label(tr(self.kind.about()));
            ui.add_space(space::SM);
            match self.kind {
                Kind::Channel => channel_fields(ui, &mut self.channel),
                Kind::Cavity => cavity_fields(ui, &mut self.cavity),
                Kind::Plate => plate_fields(ui, &mut self.plate),
                Kind::PlanarNozzle => nozzle_fields(ui, &mut self.planar),
                Kind::AxisymmetricNozzle => nozzle_fields(ui, &mut self.wedge),
                Kind::Jet => jet_fields(ui, &mut self.jet),
                Kind::Sketch => self.sketch_fields(ui),
                Kind::Region => self.region_fields(ui),
            }
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 120.0).clamp(160.0, 420.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.folder)
                        .hint_text(tr("Folder for the new case"))
                        .desired_width(width),
                );
                if super::accent_button(ui, p, tr("Create case")).clicked() {
                    ask = Ask::Create;
                }
            });
        });
        ask
    }
}

impl NewCase {
    fn region_fields(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("Start from"));
            for outline in region_editor::Outline::ALL {
                if ui
                    .selectable_label(self.region.outline == outline, outline.title())
                    .clicked()
                    && self.region.outline != outline
                {
                    self.region.restart(outline);
                }
            }
        });
        ui.add_space(space::SM);
        draw_region(ui, &self.region.drawing());
        if let Some(problem) = &self.region.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        } else if let Some(missing) = self.region.missing() {
            ui.colored_label(ui.visuals().warn_fg_color, missing);
        }
        ui.add_space(space::SM);
        let mut edit = None;
        let mut rename = None;
        egui::Grid::new("new_region_dimensions")
            .num_columns(2)
            .spacing([space::MD, space::SM])
            .show(ui, |ui| {
                for mut d in self.region.dimensions() {
                    let speed = 0.01 * d.value.abs().max(1.0);
                    row(ui, &d.label, |ui| {
                        if ui
                            .add(
                                DragValue::new(&mut d.value)
                                    .speed(speed)
                                    .suffix(format!(" {}", d.unit))
                                    .max_decimals(4),
                            )
                            .changed()
                        {
                            edit = Some((d.index, d.value));
                        }
                    });
                }
                for (entity, mut name) in self.region.names() {
                    row(ui, &trf("Name of edge {}", &[&entity]), |ui| {
                        let answer = ui.add(
                            egui::TextEdit::singleline(&mut name)
                                .id_salt(("region_edge", entity))
                                .desired_width(140.0),
                        );
                        if answer.changed() {
                            rename = Some((entity, name.clone()));
                        }
                    });
                }
                row(ui, tr("Size of the triangles"), |ui| {
                    value(ui, &mut self.drawn.cell_size, "mm")
                });
                row(ui, tr("Inlet speed"), |ui| {
                    value(ui, &mut self.drawn.speed, "m/s")
                });
                row(ui, tr("Viscosity"), |ui| {
                    value(ui, &mut self.drawn.viscosity, "m²/s")
                });
            });
        // A refused edit keeps the sketch as it was and says why above the dimensions.
        if let Some((index, v)) = edit {
            let _ = self.region.set_dimension(index, v);
        }
        if let Some((entity, name)) = rename {
            let _ = self.region.rename(entity, &name);
        }
    }

    fn sketch_fields(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("Start from"));
            for template in Template::ALL {
                if ui
                    .selectable_label(self.sketch.template == template, template.title())
                    .clicked()
                    && self.sketch.template != template
                {
                    self.sketch.restart(template);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("Body"));
            ui.selectable_value(
                &mut self.sketch_axisymmetric,
                true,
                tr("Revolved (5° wedge)"),
            );
            ui.selectable_value(&mut self.sketch_axisymmetric, false, tr("Planar slot"));
        });
        ui.add_space(space::SM);
        draw(ui, &self.sketch.outline());
        if let Some(problem) = &self.sketch.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
        ui.add_space(space::SM);
        let mut edit = None;
        egui::Grid::new("new_sketch_dimensions")
            .num_columns(2)
            .spacing([space::MD, space::SM])
            .show(ui, |ui| {
                for mut d in self.sketch.dimensions() {
                    let speed = 0.01 * d.value.abs().max(1.0);
                    row(ui, &d.label, |ui| {
                        if ui
                            .add(
                                DragValue::new(&mut d.value)
                                    .speed(speed)
                                    .suffix(format!(" {}", d.unit))
                                    .max_decimals(4),
                            )
                            .changed()
                        {
                            edit = Some((d.index, d.value));
                        }
                    });
                }
                row(ui, tr("Blocks along the wall"), |ui| {
                    ui.add(DragValue::new(&mut self.segments).speed(0.5).range(2..=400));
                });
                row(ui, tr("Cells across"), |ui| {
                    count(ui, &mut self.sketched.radial_cells)
                });
                mesh_rows(ui, &mut self.sketched);
                row(ui, tr("Inlet speed"), |ui| {
                    value(ui, &mut self.sketched.inlet_speed, "m/s")
                });
                row(ui, tr("Inlet temperature"), |ui| {
                    value(ui, &mut self.sketched.inlet_temperature, "K")
                });
                row(ui, tr("Outlet pressure"), |ui| {
                    value(ui, &mut self.sketched.outlet_pressure, "Pa")
                });
            });
        if let Some((index, v)) = edit {
            // A refused edit keeps the sketch as it was and says why above the dimensions.
            let _ = self.sketch.set_dimension(index, v);
        }
    }
}

/// The wall of the sketch and its points, mirrored about the axis, in a box that fits the form.
fn draw(ui: &mut Ui, outline: &Outline) {
    let width = ui.available_width().clamp(200.0, 640.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 200.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 6.0, visuals.extreme_bg_color);
    let Some((x0, x1, h)) = outline.bounds() else {
        return;
    };
    let margin = 16.0;
    let inner = rect.shrink(margin);
    let scale = (inner.width() as f64 / (x1 - x0).max(1.0e-9))
        .min(inner.height() as f64 / (2.0 * h).max(1.0e-9));
    let at = |x: f64, y: f64| {
        egui::pos2(
            inner.left() + ((x - x0) * scale) as f32,
            inner.center().y - (y * scale) as f32,
        )
    };
    let axis = egui::Stroke::new(1.0, visuals.weak_text_color());
    painter.line_segment([at(x0, 0.0), at(x1, 0.0)], axis);
    let wall = egui::Stroke::new(2.0, visuals.text_color());
    let ghost = egui::Stroke::new(1.0, visuals.weak_text_color());
    let top: Vec<_> = outline.wall.iter().map(|&(x, y)| at(x, y)).collect();
    let bottom: Vec<_> = outline.wall.iter().map(|&(x, y)| at(x, -y)).collect();
    painter.add(egui::Shape::line(top, wall));
    painter.add(egui::Shape::line(bottom, ghost));
    for &(x, y, fixed) in &outline.points {
        let c = at(x, y);
        if fixed {
            painter.rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0)),
                1.0,
                visuals.selection.stroke.color,
            );
        } else {
            painter.circle_filled(c, 3.5, visuals.selection.stroke.color);
        }
    }
}

/// The loops of the region, each edge in the colour of what its name makes it, with the name
/// beside it.
fn draw_region(ui: &mut Ui, drawing: &[Vec<DrawnEdge>]) {
    let width = ui.available_width().clamp(200.0, 640.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 220.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 6.0, visuals.extreme_bg_color);
    let Some((x0, x1, y0, y1)) = region_editor::bounds(drawing) else {
        return;
    };
    let inner = rect.shrink(24.0);
    let scale = (inner.width() as f64 / (x1 - x0).max(1.0e-9))
        .min(inner.height() as f64 / (y1 - y0).max(1.0e-9));
    let at = |(x, y): (f64, f64)| {
        egui::pos2(
            inner.center().x + ((x - (x0 + x1) / 2.0) * scale) as f32,
            inner.center().y - ((y - (y0 + y1) / 2.0) * scale) as f32,
        )
    };
    for edge in drawing.iter().flatten() {
        let color = match Role::of(&edge.patch) {
            Role::Inlet => visuals.selection.stroke.color,
            Role::Outlet => visuals.warn_fg_color,
            Role::Symmetry => visuals.weak_text_color(),
            Role::Wall => visuals.text_color(),
        };
        let line: Vec<_> = edge.points.iter().copied().map(at).collect();
        painter.add(egui::Shape::line(
            line.clone(),
            egui::Stroke::new(2.0, color),
        ));
        if let Some(&mid) = line.get(line.len() / 2) {
            painter.text(
                mid,
                egui::Align2::CENTER_CENTER,
                &edge.patch,
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(label);
    add(ui);
    ui.end_row();
}

fn value(ui: &mut Ui, v: &mut f64, unit: &str) {
    let speed = 0.01 * v.abs().max(0.001);
    ui.add(
        DragValue::new(v)
            .speed(speed)
            .suffix(format!(" {unit}"))
            .max_decimals(6),
    );
}

fn count(ui: &mut Ui, v: &mut usize) {
    ui.add(DragValue::new(v).speed(0.2).range(1..=10_000));
}

fn channel_fields(ui: &mut Ui, c: &mut Channel) {
    egui::Grid::new("new_channel")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, tr("Length"), |ui| value(ui, &mut c.length, "m"));
            row(ui, tr("Height"), |ui| value(ui, &mut c.height, "m"));
            row(ui, tr("Cells along"), |ui| count(ui, &mut c.cells_x));
            row(ui, tr("Cells across"), |ui| count(ui, &mut c.cells_y));
            row(ui, tr("Inlet speed"), |ui| value(ui, &mut c.speed, "m/s"));
            row(ui, tr("Viscosity"), |ui| {
                value(ui, &mut c.viscosity, "m²/s")
            });
        });
}

fn cavity_fields(ui: &mut Ui, c: &mut Cavity) {
    egui::Grid::new("new_cavity")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, tr("Side"), |ui| value(ui, &mut c.side, "m"));
            row(ui, tr("Cells along a side"), |ui| count(ui, &mut c.cells));
            row(ui, tr("Lid speed"), |ui| value(ui, &mut c.lid_speed, "m/s"));
            row(ui, tr("Viscosity"), |ui| {
                value(ui, &mut c.viscosity, "m²/s")
            });
        });
}

fn plate_fields(ui: &mut Ui, p: &mut Plate) {
    let mut turbulent = p.turbulent;
    if ui
        .checkbox(&mut turbulent, tr("Turbulent (k-ω SST, wall functions)"))
        .changed()
    {
        // The two plates have different sizes, so each starts from its own numbers.
        *p = if turbulent {
            Plate::turbulent()
        } else {
            Plate::default()
        };
    }
    egui::Grid::new("new_plate")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, tr("Plate length"), |ui| value(ui, &mut p.length, "m"));
            if !p.turbulent {
                row(ui, tr("Slip wall ahead"), |ui| {
                    value(ui, &mut p.upstream, "m")
                });
            }
            row(ui, tr("Height"), |ui| value(ui, &mut p.height, "m"));
            row(ui, tr("Cells along the plate"), |ui| {
                count(ui, &mut p.cells_x)
            });
            if !p.turbulent {
                row(ui, tr("Cells ahead"), |ui| count(ui, &mut p.cells_up));
            }
            row(ui, tr("Cells across"), |ui| count(ui, &mut p.cells_y));
            row(ui, tr("Stream speed"), |ui| value(ui, &mut p.speed, "m/s"));
            row(ui, tr("Viscosity"), |ui| {
                value(ui, &mut p.viscosity, "m²/s")
            });
            if p.turbulent {
                row(ui, tr("Turbulence intensity"), |ui| {
                    value(ui, &mut p.intensity, "")
                });
                row(ui, tr("Mixing length"), |ui| {
                    value(ui, &mut p.length_scale, "m")
                });
            }
        });
}

fn jet_fields(ui: &mut Ui, j: &mut Jet) {
    egui::Grid::new("new_jet")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, tr("Orifice radius"), |ui| {
                value(ui, &mut j.orifice_radius, "mm")
            });
            row(ui, tr("Atmosphere length"), |ui| {
                value(ui, &mut j.length, "mm")
            });
            row(ui, tr("Atmosphere radius"), |ui| {
                value(ui, &mut j.radius, "mm")
            });
            row(ui, tr("Total pressure"), |ui| {
                value(ui, &mut j.total_pressure, "Pa")
            });
            row(ui, tr("Total temperature"), |ui| {
                value(ui, &mut j.total_temperature, "K")
            });
            row(ui, tr("Ambient pressure"), |ui| {
                value(ui, &mut j.ambient_pressure, "Pa")
            });
            row(ui, tr("Cells along the axis"), |ui| {
                count(ui, &mut j.cells_axial)
            });
            row(ui, tr("Cells across the orifice"), |ui| {
                count(ui, &mut j.cells_orifice)
            });
            row(ui, tr("Cells outside the orifice"), |ui| {
                count(ui, &mut j.cells_outer)
            });
        });
}

/// The choice of mesh: blocks, or triangles made by the engine's own mesher at a cell size.
fn mesh_rows(ui: &mut Ui, n: &mut Nozzle) {
    row(ui, tr("Mesh"), |ui| {
        let mut triangles = n.cell_size.is_some();
        if ui
            .checkbox(&mut triangles, tr("Triangles (unstructured)"))
            .changed()
        {
            n.cell_size = triangles.then(|| (n.throat_diameter / 10.0).max(0.5));
        }
    });
    if let Some(size) = n.cell_size.as_mut() {
        row(ui, tr("Cell size"), |ui| value(ui, size, "mm"));
    }
}

fn nozzle_fields(ui: &mut Ui, n: &mut Nozzle) {
    egui::Grid::new("new_nozzle")
        .num_columns(2)
        .spacing([space::MD, space::SM])
        .show(ui, |ui| {
            row(ui, tr("Inlet diameter"), |ui| {
                value(ui, &mut n.inlet_diameter, "mm")
            });
            row(ui, tr("Throat diameter"), |ui| {
                value(ui, &mut n.throat_diameter, "mm")
            });
            row(ui, tr("Outlet diameter"), |ui| {
                value(ui, &mut n.outlet_diameter, "mm")
            });
            row(ui, tr("Converging length"), |ui| {
                value(ui, &mut n.converging_length, "mm")
            });
            row(ui, tr("Diverging length"), |ui| {
                value(ui, &mut n.diverging_length, "mm")
            });
            row(ui, tr("Inlet speed"), |ui| {
                value(ui, &mut n.inlet_speed, "m/s")
            });
            row(ui, tr("Inlet temperature"), |ui| {
                value(ui, &mut n.inlet_temperature, "K")
            });
            row(ui, tr("Outlet pressure"), |ui| {
                value(ui, &mut n.outlet_pressure, "Pa")
            });
            row(ui, tr("Cells across"), |ui| count(ui, &mut n.radial_cells));
            mesh_rows(ui, n);
            if n.kind == vortia_cases::NozzleKind::Planar {
                row(ui, tr("Half section"), |ui| {
                    ui.checkbox(&mut n.half, tr("Symmetry plane on the axis"));
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_makes_a_case_the_app_opens() {
        for kind in Kind::ALL {
            let base = tempfile::tempdir().unwrap();
            let mut form = NewCase::new(base.path());
            form.kind = kind;
            form.propose(base.path());
            let dir = form.create().unwrap();
            assert!(dir.starts_with(base.path()));
            assert!(dir.join("system/controlDict").is_file(), "{kind:?}");
            assert!(dir.join("constant/polyMesh/points").is_file(), "{kind:?}");
        }
    }

    #[test]
    fn a_sketched_case_keeps_its_sketch_and_follows_its_dimensions() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        form.kind = Kind::Sketch;
        form.propose(base.path());
        let height = form
            .sketch
            .dimensions()
            .into_iter()
            .find(|d| d.label.starts_with("Height") && d.value > 55.0)
            .unwrap();
        form.sketch.set_dimension(height.index, 80.0).unwrap();
        let dir = form.create().unwrap();
        let kept = dir.join("sketch.vortia-sketch");
        assert!(kept.is_file());
        assert_eq!(
            &vortia_sketch::Sketch::open(&kept).unwrap(),
            form.sketch.sketch()
        );
        // A wall the mesher cannot read stops the case before anything is written.
        let mut broken = NewCase::new(base.path());
        broken.kind = Kind::Sketch;
        broken.propose(base.path());
        broken.segments = 1;
        let folder = broken.folder.clone();
        assert!(broken.create().is_err());
        assert!(!Path::new(&folder).exists());
    }

    #[test]
    fn a_drawn_region_keeps_its_sketch_and_needs_an_inlet_and_an_outlet() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        form.kind = Kind::Region;
        form.propose(base.path());
        form.drawn.cell_size = 10.0;
        let (outlet, _) = form
            .region
            .names()
            .into_iter()
            .find(|(_, n)| n == "outlet")
            .unwrap();
        // Without an outlet nothing is written.
        form.region.rename(outlet, "wall").unwrap();
        let folder = form.folder.clone();
        assert!(form.create().unwrap_err().contains("outlet"));
        assert!(!Path::new(&folder).exists());
        form.region.rename(outlet, "outlet").unwrap();
        let dir = form.create().unwrap();
        assert!(dir.join("0/U").is_file());
        let kept = dir.join("sketch.vortia-sketch");
        assert_eq!(
            &vortia_sketch::Sketch::open(&kept).unwrap(),
            form.region.sketch()
        );
        let boundary = std::fs::read_to_string(dir.join("constant/polyMesh/boundary")).unwrap();
        assert!(boundary.contains("block") && boundary.contains("inlet"));
    }

    #[test]
    fn the_folder_proposed_is_never_one_that_exists() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        assert!(form.folder.ends_with("channel"));
        let first = form.create().unwrap();
        form.propose(base.path());
        assert_ne!(PathBuf::from(&form.folder), first);
        assert!(form.folder.ends_with("channel-2"));
        // Creating over a case that is there is refused and leaves it alone.
        form.folder = first.display().to_string();
        let err = form.create().unwrap_err();
        assert!(err.contains("not empty"), "{err}");
        assert!(first.join("system/controlDict").is_file());
    }

    #[test]
    fn nothing_is_made_without_a_folder_or_with_numbers_that_cannot_work() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        form.folder = "  ".into();
        assert!(form.create().is_err());
        form.propose(base.path());
        form.channel.length = -1.0;
        let err = form.create().unwrap_err();
        assert!(err.contains("length"), "{err}");
        assert!(!Path::new(form.folder.trim()).exists());
    }

    #[test]
    fn triangles_can_be_chosen_for_a_nozzle_and_for_a_sketch() {
        for kind in [Kind::PlanarNozzle, Kind::AxisymmetricNozzle, Kind::Sketch] {
            let base = tempfile::tempdir().unwrap();
            let mut form = NewCase::new(base.path());
            form.kind = kind;
            form.planar.cell_size = Some(8.0);
            form.wedge.cell_size = Some(8.0);
            form.sketched.cell_size = Some(8.0);
            form.propose(base.path());
            let dir = form.create().unwrap();
            // The mesh came from the unstructured mesher, not from a blockMeshDict.
            assert!(!dir.join("system/blockMeshDict").exists(), "{kind:?}");
            assert!(dir.join("constant/polyMesh/points").exists(), "{kind:?}");
        }
    }
}
