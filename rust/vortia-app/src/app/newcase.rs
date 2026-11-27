//! Home's "New": the form that starts a case (`vortia_cases`).

use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

use eframe::egui::{self, DragValue, Frame, Margin, RichText, Ui};
use nodalia_theme::engineering::{self as eng, Look, layout};
use nodalia_theme::office;
use nodalia_theme::{Palette, for_egui, icons, space};
use vortia_cases::{
    Airfoil, AirfoilModel, Cavity, Channel, Drawn, Geometry, Jet, Nozzle, NozzleKind, Plate,
};
use vortia_sketch::Measure;

mod studio;

use super::imported::Imported;
use super::modelled::Modelled;
use crate::region_editor::{self, Canvas, RegionEditor, Role};
use crate::sketch_editor::{Outline, SketchEditor, Template};

/// Which of the cases Home offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Channel,
    Cavity,
    Plate,
    Airfoil,
    PlanarNozzle,
    AxisymmetricNozzle,
    Jet,
    Sketch,
    Region,
    Imported,
    Modelled,
}

impl Kind {
    #[cfg(test)]
    const ALL: [Kind; 11] = [
        Kind::Channel,
        Kind::Cavity,
        Kind::Plate,
        Kind::Airfoil,
        Kind::PlanarNozzle,
        Kind::AxisymmetricNozzle,
        Kind::Jet,
        Kind::Sketch,
        Kind::Region,
        Kind::Imported,
        Kind::Modelled,
    ];

    fn title(self) -> &'static str {
        match self {
            Kind::Channel => tr("Channel"),
            Kind::Cavity => tr("Cavity"),
            Kind::Plate => tr("Flat plate"),
            Kind::Airfoil => tr("Airfoil"),
            Kind::PlanarNozzle => tr("Planar nozzle"),
            Kind::AxisymmetricNozzle => tr("Axisymmetric nozzle"),
            Kind::Jet => tr("Jet into the atmosphere"),
            Kind::Sketch => tr("From sketch"),
            Kind::Region => tr("Drawn region"),
            Kind::Imported => tr("Around a CAD file"),
            Kind::Modelled => tr("Model a part"),
        }
    }

    /// The CAD icon of the shared set the card and the page wear.
    fn icon(self) -> &'static str {
        match self {
            Kind::Channel => "arrows-horizontal",
            Kind::Cavity => "square",
            Kind::Plate => "line",
            Kind::Airfoil => "curvature",
            Kind::PlanarNozzle => "nozzle",
            Kind::AxisymmetricNozzle => "revolve",
            Kind::Jet => "flare",
            Kind::Sketch => "sketch",
            Kind::Region => "region",
            Kind::Imported => "import",
            Kind::Modelled => "cube",
        }
    }

    /// The line under the name on its card.
    fn tagline(self) -> &'static str {
        match self {
            Kind::Channel => tr("Laminar flow in a duct"),
            Kind::Cavity => tr("Lid-driven square box"),
            Kind::Plate => tr("Boundary layer on a plate"),
            Kind::Airfoil => tr("NACA 4-digit, any angle"),
            Kind::PlanarNozzle => tr("Compressible slot nozzle"),
            Kind::AxisymmetricNozzle => tr("Compressible, as a wedge"),
            Kind::Jet => tr("Orifice into the open air"),
            Kind::Sketch => tr("A duct wall you dimension"),
            Kind::Region => tr("Outline with named edges"),
            Kind::Imported => tr("STL, OBJ, STEP, DXF…"),
            Kind::Modelled => tr("With Nodalia's own kernel"),
        }
    }

    /// Whether the case is drawn in the studio, with the ribbon, the Browser and the canvas of a
    /// CAD, rather than filled in on a page.
    pub(super) fn studio(self) -> bool {
        matches!(self, Kind::Region | Kind::Modelled)
    }

    /// The groups the gallery lists, with what each holds.
    const GROUPS: [(&'static str, &'static [Kind]); 3] = [
        (
            "Ready-made cases",
            &[
                Kind::Channel,
                Kind::Cavity,
                Kind::Plate,
                Kind::Airfoil,
                Kind::PlanarNozzle,
                Kind::AxisymmetricNozzle,
                Kind::Jet,
            ],
        ),
        ("Draw the region", &[Kind::Sketch, Kind::Region]),
        ("Bring or model a part", &[Kind::Imported, Kind::Modelled]),
    ];

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
            Kind::Airfoil => {
                "A NACA 4-digit airfoil in a stream at any angle of attack, on a C-grid Vortia meshes itself: turbulent, incompressible, steady (simpleFoam) with Spalart–Allmaras or k-ω SST. Lift, drag and moment coefficients and the Cp curve come out of `vortia airfoil`. The NACA 0012 at Reynolds 6·10⁶ by default."
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
            Kind::Imported => {
                "A body read from a file in a wind tunnel built around it: an STL, OBJ, PLY, glTF or STEP surface is meshed in 3D by octree refinement (Spalart–Allmaras or k-ω SST), a DXF drawing, say the side of a car or a wing section, in 2D with triangles (laminar, incompressible). The drag is the force on the patch `body`."
            }
            Kind::Modelled => {
                "A part you model here with Nodalia's own kernel, like the ones of Torno and Forja: boxes, cylinders, cones, spheres, tori, a wing section or a DXF outline pushed along an axis, an outline turned about it, each added to the part, cut out of it or intersected with it. The flow around it is meshed in 3D by octree refinement (Spalart–Allmaras or k-ω SST); the part can also be saved as an STL. The drag is the force on the patch `body`."
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
    airfoil: Airfoil,
    planar: Nozzle,
    wedge: Nozzle,
    jet: Jet,
    sketch: SketchEditor,
    /// The flow and mesh settings of the sketched duct; its walls come from the sketch.
    sketched: Nozzle,
    sketch_axisymmetric: bool,
    segments: usize,
    region: RegionEditor,
    /// The saved sketch being typed for "Open sketch".
    region_file: String,
    /// The flow and mesh settings of the drawn region; its edges come from the sketch.
    drawn: Drawn,
    /// The body or drawing read from a file.
    imported: Imported,
    /// The part modelled here.
    modelled: Modelled,
    /// Whether a kind was picked: until then the form is the gallery of kinds.
    chosen: bool,
    /// Where the pointer is over the studio's canvas, in millimetres, as of the last frame.
    cursor: Option<(f64, f64)>,
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
            airfoil: Airfoil::default(),
            planar: Nozzle::new(NozzleKind::Planar),
            wedge: Nozzle::new(NozzleKind::Axisymmetric),
            jet: Jet::default(),
            sketch: SketchEditor::default(),
            sketched: Nozzle::new(NozzleKind::Axisymmetric),
            sketch_axisymmetric: true,
            segments: 60,
            region: RegionEditor::default(),
            region_file: String::new(),
            drawn: Drawn::default(),
            imported: Imported::default(),
            modelled: Modelled::default(),
            chosen: false,
            cursor: None,
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
            Kind::Airfoil => "airfoil",
            Kind::PlanarNozzle => "planar-nozzle",
            Kind::AxisymmetricNozzle => "nozzle",
            Kind::Jet => "jet",
            Kind::Sketch => "sketch",
            Kind::Region => "region",
            Kind::Imported | Kind::Modelled => "body",
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
        self.choose(Kind::Sketch);
    }

    /// Starts the case of `kind`: the form leaves the gallery for it.
    pub(super) fn choose(&mut self, kind: Kind) {
        let base = Path::new(self.folder.trim())
            .parent()
            .map_or_else(Self::default_base, Path::to_path_buf);
        self.kind = kind;
        self.chosen = true;
        self.propose(&base);
    }

    /// Starts the kind a command line names (`channel`, `region`, `model`…); false if it names
    /// none, which leaves the gallery.
    pub(super) fn choose_named(&mut self, key: &str) -> bool {
        let kind = match key {
            "channel" => Kind::Channel,
            "cavity" => Kind::Cavity,
            "plate" => Kind::Plate,
            "airfoil" => Kind::Airfoil,
            "planar" => Kind::PlanarNozzle,
            "nozzle" => Kind::AxisymmetricNozzle,
            "jet" => Kind::Jet,
            "sketch" => Kind::Sketch,
            "region" => Kind::Region,
            "body" => Kind::Imported,
            "model" => Kind::Modelled,
            _ => return false,
        };
        self.choose(kind);
        true
    }

    /// Back to the gallery of kinds.
    fn leave(&mut self) {
        self.chosen = false;
    }

    /// Whether the gallery is showing, rather than the form of a kind.
    pub(super) fn in_gallery(&self) -> bool {
        !self.chosen
    }

    /// Whether the case is being drawn in the studio, which takes the whole window.
    pub(super) fn in_studio(&self) -> bool {
        self.chosen && self.kind.studio()
    }

    pub(super) fn geometry(&self) -> Result<Geometry, String> {
        Ok(match self.kind {
            Kind::Channel => Geometry::Channel(self.channel.clone()),
            Kind::Cavity => Geometry::Cavity(self.cavity.clone()),
            Kind::Plate => Geometry::Plate(self.plate.clone()),
            Kind::Airfoil => Geometry::Airfoil(self.airfoil.clone()),
            Kind::PlanarNozzle => Geometry::Nozzle(self.planar.clone()),
            Kind::AxisymmetricNozzle => Geometry::Nozzle(self.wedge.clone()),
            Kind::Jet => Geometry::Jet(self.jet.clone()),
            Kind::Sketch => Geometry::Nozzle(self.sketched_nozzle()?),
            Kind::Region => Geometry::Drawn(self.drawn_region()?),
            Kind::Imported => self.imported.geometry()?,
            Kind::Modelled => self.modelled.geometry()?,
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

    /// The page of New: the gallery of kinds, then the form of the one picked. The studio kinds
    /// are drawn by [`NewCase::studio`] instead, which takes the whole window.
    pub(super) fn ui(&mut self, ui: &mut Ui, look: &Look, p: &Palette) -> Ask {
        if self.chosen {
            self.detail(ui, look, p)
        } else {
            self.gallery(ui, look);
            Ask::Nothing
        }
    }

    /// Every kind of case on a card, in the groups they belong to.
    fn gallery(&mut self, ui: &mut Ui, look: &Look) {
        let home = eng::home_look(look);
        let mut picked = None;
        for (heading, kinds) in Kind::GROUPS {
            office::section(ui, &home, tr(heading), |ui| {
                let cards: Vec<eng::StartCard> = kinds
                    .iter()
                    .map(|kind| eng::StartCard {
                        icon: kind.icon(),
                        name: kind.title(),
                        detail: kind.tagline(),
                    })
                    .collect();
                if let Some(i) = eng::start_cards(ui, look, &cards) {
                    picked = kinds.get(i).copied();
                }
            });
        }
        if let Some(kind) = picked {
            self.choose(kind);
        }
    }

    /// The form of the kind picked: its numbers beside what the case is, and where it goes.
    fn detail(&mut self, ui: &mut Ui, look: &Look, p: &Palette) -> Ask {
        let mut ask = Ask::Nothing;
        let palette = &look.palette;
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("‹  {}", tr("All cases")))
                            .color(for_egui::color(palette.accent_text)),
                    )
                    .frame(false),
                )
                .clicked()
            {
                self.leave();
            }
        });
        ui.add_space(space::SM);
        ui.label(
            RichText::new(self.kind.title())
                .font(eng::page_title())
                .color(for_egui::color(palette.text_primary)),
        );
        ui.add_space(space::MD);
        let total = ui.available_width();
        let side = (total * 0.36).clamp(240.0, 340.0);
        let main = (total - side - 16.0).max(240.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.allocate_ui_with_layout(
                egui::vec2(main, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    card(ui, look, tr("Settings"), |ui| self.fields(ui, p));
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(side, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    card(ui, look, tr("About this case"), |ui| {
                        self.about_card(ui, look)
                    });
                },
            );
        });
        ui.add_space(space::MD);
        card(ui, look, tr("Where it is saved"), |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 140.0).max(160.0);
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

    /// The numbers of the kind picked.
    fn fields(&mut self, ui: &mut Ui, p: &Palette) {
        match self.kind {
            Kind::Channel => channel_fields(ui, &mut self.channel),
            Kind::Cavity => cavity_fields(ui, &mut self.cavity),
            Kind::Plate => plate_fields(ui, &mut self.plate),
            Kind::Airfoil => airfoil_fields(ui, &mut self.airfoil),
            Kind::PlanarNozzle => nozzle_fields(ui, &mut self.planar),
            Kind::AxisymmetricNozzle => nozzle_fields(ui, &mut self.wedge),
            Kind::Jet => jet_fields(ui, &mut self.jet),
            Kind::Sketch => self.sketch_fields(ui),
            Kind::Region => self.region_fields(ui),
            Kind::Imported => self.imported.ui(ui),
            Kind::Modelled => self.modelled.ui(ui, p),
        }
    }

    /// What the case is: the icon of its kind, the words and, for a sketch, its wall.
    fn about_card(&self, ui: &mut Ui, look: &Look) {
        let palette = &look.palette;
        let (badge, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(badge, 12.0, for_egui::color(palette.accent_subtle));
        if let Some(glyph) = icons::get(self.kind.icon()) {
            icons::paint(
                ui.painter(),
                badge.shrink(10.0),
                glyph,
                icons::Style {
                    accent: Some(for_egui::color(palette.accent_default)),
                    ..icons::Style::regular(for_egui::color(palette.text_primary))
                },
            );
        }
        ui.add_space(space::SM);
        ui.label(
            RichText::new(tr(self.kind.about())).color(for_egui::color(palette.text_secondary)),
        );
    }
}

/// A titled card of the page: a raised surface with a hairline edge.
fn card<R>(ui: &mut Ui, look: &Look, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    let p = &look.palette;
    Frame::NONE
        .fill(for_egui::color(p.surface_raised))
        .stroke(egui::Stroke::new(1.0, for_egui::color(p.border_subtle)))
        .corner_radius(layout::ISLAND_RADIUS)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .font(eng::panel_title())
                    .color(for_egui::color(p.text_primary)),
            );
            ui.add_space(space::SM);
            body(ui)
        })
        .inner
}

impl NewCase {
    fn region_fields(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
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
            ui.add_space(space::MD);
            ui.add(
                egui::TextEdit::singleline(&mut self.region_file)
                    .hint_text(tr("A saved sketch"))
                    .desired_width(220.0),
            );
            if ui.button(tr("Open sketch")).clicked() {
                // A refusal stays in the editor's `problem`, which the form shows below.
                let _ = self
                    .region
                    .open(std::path::Path::new(self.region_file.trim()));
            }
        });
        region_toolbar(ui, &mut self.region);
        region_canvas(ui, &mut self.region);
        region_status(ui, &self.region);
        ui.add_space(space::SM);
        let mut edit = None;
        let mut remove = None;
        let mut rename = None;
        grid(ui, "new_region_dimensions", |ui| {
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
                    if ui.small_button("✕").clicked() {
                        remove = Some(d.index);
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
        if let Some(index) = remove {
            let _ = self.region.remove_dimension(index);
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
        grid(ui, "new_sketch_dimensions", |ui| {
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

/// The tools, in the three groups a CAD sketch has: draw, modify and hold. What a tool needs
/// beside the clicks (sides, radius, distance, a relation) is set on the line under them.
pub(super) fn region_toolbar(ui: &mut Ui, editor: &mut RegionEditor) {
    use region_editor::Tool;
    ui.horizontal_wrapped(|ui| {
        for (title, tools) in [
            (tr("Draw"), &Tool::DRAW[..]),
            (tr("Modify"), &Tool::MODIFY[..]),
            (tr("Hold"), &Tool::HOLD[..]),
        ] {
            ui.label(egui::RichText::new(title).weak());
            for &tool in tools {
                if tool_button(ui, tool.icon(), tool.title(), editor.tool == tool).clicked()
                    && editor.tool != tool
                {
                    editor.set_tool(tool);
                }
            }
            ui.add_space(space::MD);
        }
    });
    ui.horizontal_wrapped(|ui| {
        match editor.tool {
            Tool::Polygon => {
                ui.label(tr("Sides"));
                ui.add(DragValue::new(&mut editor.sides).range(3..=vortia_sketch::MAX_SIDES));
            }
            Tool::Fillet => {
                ui.label(tr("Radius"));
                ui.add(
                    DragValue::new(&mut editor.radius)
                        .speed(0.5)
                        .range(0.1..=10000.0)
                        .suffix(" mm"),
                );
            }
            Tool::Chamfer => {
                ui.label(tr("Distance"));
                ui.add(
                    DragValue::new(&mut editor.distance)
                        .speed(0.5)
                        .range(0.1..=10000.0)
                        .suffix(" mm"),
                );
            }
            Tool::Dimension => {
                for (measure, label) in [
                    (Measure::Distance, tr("Distance")),
                    (Measure::Along, tr("Along x")),
                    (Measure::Height, tr("Height")),
                ] {
                    ui.selectable_value(&mut editor.measure, measure, label);
                }
            }
            Tool::Constrain => {
                for relation in region_editor::Relation::ALL {
                    if tool_button(
                        ui,
                        Some(relation.icon()),
                        relation.title(),
                        editor.relation == relation,
                    )
                    .clicked()
                        && editor.relation != relation
                    {
                        editor.relation = relation;
                        editor.cancel();
                    }
                }
            }
            _ => {}
        }
        ui.add_space(space::MD);
        ui.label(tr("Grid"));
        ui.add(
            DragValue::new(&mut editor.grid)
                .speed(0.5)
                .range(0.0..=1000.0)
                .suffix(" mm"),
        );
        if ui.button(tr("Fit view")).clicked() {
            editor.fit();
        }
        if ui
            .add_enabled(editor.can_undo(), egui::Button::new(tr("Undo")))
            .clicked()
        {
            editor.undo();
        }
        if ui
            .add_enabled(editor.can_redo(), egui::Button::new(tr("Redo")))
            .clicked()
        {
            editor.redo();
        }
    });
    ui.label(egui::RichText::new(editor.tool.hint(editor.progress(), editor.relation)).weak());
}

/// What is wrong or missing in the drawing, and how much of it is still free to move.
pub(super) fn region_status(ui: &mut Ui, editor: &RegionEditor) {
    if let Some(problem) = &editor.problem {
        ui.colored_label(ui.visuals().error_fg_color, problem);
    } else if let Some(why) = editor.open_because() {
        ui.colored_label(ui.visuals().warn_fg_color, why);
    } else if let Some(missing) = editor.missing() {
        ui.colored_label(ui.visuals().warn_fg_color, missing);
    }
    if let Some(free) = editor.freedom() {
        let text = if free == 0 {
            tr("Fully defined: nothing can move.").to_owned()
        } else {
            trf(
                "{} degrees of freedom: dimension or constrain more.",
                &[&free],
            )
        };
        ui.label(egui::RichText::new(text).weak());
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

/// A tool or a relation: its icon (when the set has one) and name, lit when it is the one in use.
fn tool_button(ui: &mut Ui, icon: Option<&str>, text: &str, active: bool) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, ui.visuals().text_color());
    let icon_width = if icon.is_some() { 20.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + icon_width + 16.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let visuals = ui.visuals();
    let fill = if active {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let ink = if active {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, fill);
    let mut x = rect.left() + 8.0;
    if let Some(icon) = icon {
        let square =
            egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 8.0), egui::vec2(16.0, 16.0));
        super::paint_icon(painter, square, icon, ink);
        x += icon_width;
    }
    painter.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// The drawing of the region, which the tool in use acts on: each edge is in the colour of what
/// its name makes it, with the name beside it.
pub(super) fn region_canvas(ui: &mut Ui, editor: &mut RegionEditor) {
    let width = ui.available_width().clamp(240.0, 1100.0);
    region_canvas_in(ui, editor, egui::vec2(width, 440.0));
}

/// The drawing, in a box of `size`; where the pointer is over it, in millimetres.
pub(super) fn region_canvas_in(
    ui: &mut Ui,
    editor: &mut RegionEditor,
    size: egui::Vec2,
) -> Option<(f64, f64)> {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let canvas = Canvas::new(
        f64::from(rect.left()),
        f64::from(rect.top()),
        f64::from(rect.width()),
        f64::from(rect.height()),
        editor.view,
    );
    let tolerance = canvas.millimetres(8.0);
    let world = |p: egui::Pos2| canvas.to_world(f64::from(p.x), f64::from(p.y));
    let screen = |x: f64, y: f64| {
        let (px, py) = canvas.to_screen(x, y);
        egui::pos2(px as f32, py as f32)
    };
    if let Some(p) = response.interact_pointer_pos() {
        let (x, y) = world(p);
        if response.drag_started() {
            editor.begin_drag(x, y, tolerance);
        }
        if response.dragged() {
            editor.drag_to(x, y);
        }
        if response.clicked() {
            editor.click(x, y, tolerance);
        }
    }
    if response.drag_stopped() {
        editor.end_drag();
    }
    if response.secondary_clicked()
        || (response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)))
    {
        editor.cancel();
    }
    if response.hovered() {
        let redo = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        let undo = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
        // Redo first: Ctrl+Shift+Z also satisfies a Ctrl+Z that does not mind extra modifiers.
        if ui.input_mut(|i| i.consume_shortcut(&redo)) {
            editor.redo();
        } else if ui.input_mut(|i| i.consume_shortcut(&undo)) {
            editor.undo();
        }
    }
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    painter.rect_filled(rect, 12.0, visuals.extreme_bg_color);
    painter.rect_stroke(
        rect,
        12.0,
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    if editor.grid > 0.0 && canvas.millimetres(1.0) * editor.grid >= 8.0 {
        let (x0, y1) = canvas.to_world(f64::from(rect.left()), f64::from(rect.top()));
        let (x1, y0) = canvas.to_world(f64::from(rect.right()), f64::from(rect.bottom()));
        let step = editor.grid;
        let (mut gx, count) = ((x0 / step).ceil() * step, 0);
        let dots = visuals.weak_text_color().gamma_multiply(0.35);
        let mut count: usize = count;
        while gx <= x1 && count < 400 {
            let mut gy = (y0 / step).ceil() * step;
            while gy <= y1 {
                painter.circle_filled(screen(gx, gy), 1.0, dots);
                gy += step;
            }
            gx += step;
            count += 1;
        }
    }
    // The axes through the origin, faint, so the drawing has a place to be measured from.
    let axis = egui::Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.3));
    let origin = screen(0.0, 0.0);
    if rect.x_range().contains(origin.x) {
        painter.line_segment(
            [
                egui::pos2(origin.x, rect.top()),
                egui::pos2(origin.x, rect.bottom()),
            ],
            axis,
        );
    }
    if rect.y_range().contains(origin.y) {
        painter.line_segment(
            [
                egui::pos2(rect.left(), origin.y),
                egui::pos2(rect.right(), origin.y),
            ],
            axis,
        );
    }
    let edges = editor.drawing();
    let lines: Vec<Vec<egui::Pos2>> = edges
        .iter()
        .map(|e| e.points.iter().map(|&(x, y)| screen(x, y)).collect())
        .collect();
    let colour_of = |patch: &str| match Role::of(patch) {
        Role::Inlet => visuals.selection.stroke.color,
        Role::Outlet => visuals.warn_fg_color,
        Role::Symmetry => visuals.weak_text_color(),
        Role::Wall => visuals.text_color(),
    };
    for (edge, line) in edges.iter().zip(&lines) {
        painter.add(egui::Shape::line(
            line.clone(),
            egui::Stroke::new(2.0, colour_of(&edge.patch)),
        ));
    }
    // The middle of the drawing, which what is written beside an edge keeps away from.
    let points_on_screen: Vec<egui::Pos2> = lines.iter().flatten().copied().collect();
    let centre = if points_on_screen.is_empty() {
        rect.center()
    } else {
        let n = points_on_screen.len() as f32;
        egui::pos2(
            points_on_screen.iter().map(|p| p.x).sum::<f32>() / n,
            points_on_screen.iter().map(|p| p.y).sum::<f32>() / n,
        )
    };
    for (x, y, fixed) in editor.points() {
        let c = screen(x, y);
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
    // What each constraint holds, marked where it holds: a chip on the inner side of the nearest
    // edge, and the next one along when two hold the same place.
    let mut chips: Vec<egui::Rect> = Vec::new();
    for marker in editor.markers() {
        let at = screen(marker.at.0, marker.at.1);
        let (along, inward) = side_of(&lines, at, centre);
        let mut chip = egui::Rect::from_center_size(at + inward * 13.0, egui::vec2(18.0, 18.0));
        for _ in 0..6 {
            if !chips.iter().any(|c| c.expand(1.0).intersects(chip)) {
                break;
            }
            chip = chip.translate(along * 19.0);
        }
        chips.push(chip);
        painter.rect_filled(chip, 4.0, visuals.extreme_bg_color);
        painter.rect_stroke(
            chip,
            4.0,
            egui::Stroke::new(1.0, visuals.warn_fg_color),
            egui::StrokeKind::Inside,
        );
        super::paint_icon(
            &painter,
            chip.shrink(2.5),
            marker.relation.icon(),
            visuals.warn_fg_color,
        );
    }
    // Only an edge with a name of its own is labelled: every other one is a plain wall. The
    // name sits in a pill on the outer side of its edge, pushed further out where it would
    // cover another name or a chip.
    let mut taken = chips;
    let mut named: Vec<&str> = Vec::new();
    for (edge, line) in edges.iter().zip(&lines) {
        // One name for each patch is enough: the edges of a block are all `block`.
        if edge.patch == vortia_sketch::DEFAULT_PATCH || named.contains(&edge.patch.as_str()) {
            continue;
        }
        named.push(&edge.patch);
        let Some((x, y)) = edge.middle() else {
            continue;
        };
        let colour = colour_of(&edge.patch);
        let galley =
            painter.layout_no_wrap(edge.patch.clone(), egui::FontId::proportional(11.5), colour);
        let pill = galley.size() + egui::vec2(12.0, 6.0);
        let mid = screen(x, y);
        let outward = outward_of(line, mid, centre);
        let reach = outward.x.abs() * pill.x / 2.0 + outward.y.abs() * pill.y / 2.0 + 7.0;
        let mut at = mid + outward * reach;
        let mut label = egui::Rect::from_center_size(at, pill);
        for _ in 0..6 {
            if !taken.iter().any(|t| t.expand(2.0).intersects(label)) {
                break;
            }
            at += outward * (pill.y + 4.0);
            label = egui::Rect::from_center_size(at, pill);
        }
        taken.push(label);
        painter.rect_filled(label, 5.0, visuals.extreme_bg_color.gamma_multiply(0.92));
        painter.rect_stroke(
            label,
            5.0,
            egui::Stroke::new(1.0, colour.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
        painter.galley(label.center() - galley.size() / 2.0, galley, colour);
    }
    // What the tool has been given, and the shape it would make where the pointer is.
    for &picked in editor.picked() {
        painter.circle_stroke(
            screen(picked.0, picked.1),
            5.0,
            egui::Stroke::new(1.5, visuals.warn_fg_color),
        );
    }
    if let Some(p) = response.hover_pos() {
        let (hx, hy) = editor.landing(world(p).0, world(p).1, tolerance);
        let stroke = egui::Stroke::new(1.5, visuals.warn_fg_color.gamma_multiply(0.8));
        for shape in editor.preview((hx, hy)) {
            let line: Vec<_> = shape.iter().map(|&(x, y)| screen(x, y)).collect();
            painter.add(egui::Shape::line(line, stroke));
        }
        if editor.tool != region_editor::Tool::Move {
            painter.circle_stroke(screen(hx, hy), 4.0, stroke);
        }
    }
    response.hover_pos().map(world)
}

/// The unit direction from `from` to `to`; none when they are the same point.
fn unit(from: egui::Pos2, to: egui::Pos2) -> Option<egui::Vec2> {
    let d = to - from;
    (d.length() > 1.0e-3).then(|| d.normalized())
}

/// Which way is out of the drawing at `mid` on `line`: the normal of the edge, turned to face
/// away from the middle of the drawing.
fn outward_of(line: &[egui::Pos2], mid: egui::Pos2, centre: egui::Pos2) -> egui::Vec2 {
    let chord = line
        .first()
        .zip(line.last())
        .and_then(|(a, b)| unit(*a, *b));
    let away = unit(centre, mid);
    match (chord, away) {
        (Some(d), Some(away)) => {
            let n = egui::vec2(-d.y, d.x);
            if n.dot(away) < 0.0 { -n } else { n }
        }
        (Some(d), None) => egui::vec2(-d.y, d.x),
        (None, Some(away)) => away,
        (None, None) => egui::vec2(0.0, -1.0),
    }
}

/// The direction of the edge nearest `at` and the normal of it that faces the middle of the
/// drawing, for what is set on the inner side of an edge.
fn side_of(
    lines: &[Vec<egui::Pos2>],
    at: egui::Pos2,
    centre: egui::Pos2,
) -> (egui::Vec2, egui::Vec2) {
    let mut best: Option<(f32, egui::Vec2)> = None;
    for line in lines {
        for pair in line.windows(2) {
            let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let ab = b - a;
            let len2 = ab.length_sq();
            let t = if len2 > 0.0 {
                ((at - a).dot(ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = (at - (a + ab * t)).length();
            if best.is_none_or(|(near, _)| d < near)
                && let Some(dir) = unit(a, b)
            {
                best = Some((d, dir));
            }
        }
    }
    let along = best.map_or(egui::vec2(1.0, 0.0), |(_, d)| d);
    let n = egui::vec2(-along.y, along.x);
    let inward = unit(at, centre).map_or(n, |to| if n.dot(to) < 0.0 { -n } else { n });
    (along, inward)
}

/// A grid of settings in the engineering suite's measure: the labels in a column of their own,
/// wrapping when long, and the controls starting at one x.
pub(super) fn grid<R>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rows: impl FnOnce(&mut Ui) -> R,
) -> R {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(layout::PROPERTY_LABEL)
        .max_col_width(176.0)
        .min_row_height(layout::PROPERTY_ROW)
        .spacing([10.0, 2.0])
        .show(ui, rows)
        .inner
}

/// A setting: its label (secondary ink) and its control.
pub(super) fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(label).weak());
    add(ui);
    ui.end_row();
}

pub(super) fn value(ui: &mut Ui, v: &mut f64, unit: &str) {
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
    grid(ui, "new_channel", |ui| {
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
    grid(ui, "new_cavity", |ui| {
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
    grid(ui, "new_plate", |ui| {
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

fn airfoil_fields(ui: &mut Ui, a: &mut Airfoil) {
    ui.horizontal(|ui| {
        ui.label(tr("Turbulence model"));
        for (model, title) in [
            (AirfoilModel::SpalartAllmaras, "Spalart–Allmaras"),
            (AirfoilModel::KOmegaSst, "k-ω SST"),
        ] {
            if ui.selectable_label(a.model == model, title).clicked() {
                a.model = model;
            }
        }
    });
    grid(ui, "new_airfoil", |ui| {
        row(ui, tr("NACA profile"), |ui| {
            ui.add(egui::DragValue::new(&mut a.naca).speed(0.2).range(1..=9999));
        });
        row(ui, tr("Angle of attack"), |ui| {
            ui.add(
                egui::DragValue::new(&mut a.alpha)
                    .speed(0.1)
                    .range(-45.0..=45.0)
                    .suffix(" °"),
            );
        });
        row(ui, tr("Chord"), |ui| value(ui, &mut a.chord, "m"));
        row(ui, tr("Stream speed"), |ui| value(ui, &mut a.speed, "m/s"));
        row(ui, tr("Reynolds number"), |ui| {
            value(ui, &mut a.reynolds, "")
        });
        row(ui, tr("Distance to the far field"), |ui| {
            value(ui, &mut a.far, "chords")
        });
        row(ui, tr("Wake length"), |ui| value(ui, &mut a.wake, "chords"));
        row(ui, tr("Cells along each surface"), |ui| {
            count(ui, &mut a.cells_chord)
        });
        row(ui, tr("Cells to the far field"), |ui| {
            count(ui, &mut a.cells_normal)
        });
        row(ui, tr("Cells along the wake"), |ui| {
            count(ui, &mut a.cells_wake)
        });
        row(ui, tr("First cell y+"), |ui| value(ui, &mut a.yplus, ""));
        if a.model == AirfoilModel::KOmegaSst {
            row(ui, tr("Turbulence intensity"), |ui| {
                value(ui, &mut a.intensity, "")
            });
        }
    });
}

fn jet_fields(ui: &mut Ui, j: &mut Jet) {
    grid(ui, "new_jet", |ui| {
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
    grid(ui, "new_nozzle", |ui| {
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
    fn the_gallery_lists_every_kind_once_and_each_has_an_icon_of_the_shared_set() {
        let listed: Vec<Kind> = Kind::GROUPS
            .iter()
            .flat_map(|(_, kinds)| kinds.iter().copied())
            .collect();
        assert_eq!(listed.len(), Kind::ALL.len());
        for kind in Kind::ALL {
            assert_eq!(listed.iter().filter(|k| **k == kind).count(), 1, "{kind:?}");
            assert!(icons::get(kind.icon()).is_some(), "{kind:?}");
            assert!(!kind.tagline().is_empty());
        }
    }

    #[test]
    fn a_kind_named_on_the_command_line_opens_and_a_stranger_leaves_the_gallery() {
        let base = tempfile::tempdir().unwrap();
        let mut form = NewCase::new(base.path());
        assert!(form.in_gallery());
        assert!(!form.choose_named("nonsense"));
        assert!(form.in_gallery());
        assert!(form.choose_named("region"));
        assert!(form.in_studio() && !form.in_gallery());
        assert!(form.choose_named("channel"));
        assert!(!form.in_studio() && !form.in_gallery());
        form.leave();
        assert!(form.in_gallery());
    }

    #[test]
    fn every_kind_makes_a_case_the_app_opens() {
        for kind in Kind::ALL {
            let base = tempfile::tempdir().unwrap();
            let mut form = NewCase::new(base.path());
            form.kind = kind;
            form.propose(base.path());
            if kind == Kind::Imported {
                // The one kind that needs a file: a circle drawn in millimetres.
                let file = base.path().join("circle.dxf");
                std::fs::write(&file, crate::app::imported::CIRCLE_DXF).unwrap();
                form.imported.file = file.display().to_string();
                form.imported.load();
                form.propose(base.path());
            }
            if kind == Kind::Modelled {
                // A part built, and a mesh as coarse as the form allows.
                form.modelled.prepare_for_test();
                form.propose(base.path());
            }
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
