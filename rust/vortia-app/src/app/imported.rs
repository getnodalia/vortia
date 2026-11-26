//! Home's "New > Around a CAD file": the flow around a body or a drawing read from a file.
//!
//! An STL, OBJ, PLY, glTF or STEP file is a closed surface in a wind tunnel (`vortia_cases::Body`, meshed by
//! octree refinement); a DXF drawing is 2D outlines, a section of a wing or of a car, in a tunnel
//! drawn around them (`vortia_cad::dxf::Drawing::around`, meshed with triangles).

use std::path::Path;

use eframe::egui::{self, DragValue, Ui};
use vortia_cad::dxf::{self, Drawing, Outline, Tunnel};
use vortia_cases::{Body, BodyModel, Drawn, Geometry};
use vortia_mesh::stl::Summary;

use super::newcase::{row, value};
use crate::i18n::{tr, trf};

/// What was read from the file.
enum Loaded {
    Body {
        body: Box<Body>,
        summary: Summary,
        /// What the last repair found and did, in the app's words.
        repaired: Vec<String>,
    },
    Section {
        drawing: Box<Drawing>,
        drawn: Box<Drawn>,
        tunnel: Tunnel,
        /// The size of the triangles along the bodies; 0 takes a fortieth of their size.
        cell_size: f64,
    },
}

/// The form of a case made around a file.
pub(super) struct Imported {
    /// The file being typed, an `.stl`, `.obj`, `.ply`, `.gltf`/`.glb`, `.step`/`.stp` or `.dxf`.
    pub(super) file: String,
    /// What every length of the file is multiplied by: 0.001 for the millimetres of an STL.
    scale: f64,
    loaded: Option<Loaded>,
    problem: Option<String>,
    /// What the reader had to say about the file (a unit assumed, an entity left out).
    notes: Vec<String>,
}

impl Default for Imported {
    fn default() -> Self {
        Self {
            file: String::new(),
            scale: 1.0,
            loaded: None,
            problem: None,
            notes: Vec::new(),
        }
    }
}

fn is_dxf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dxf"))
}

impl Imported {
    /// The file named, read: the form's fields for it, or why not.
    pub(super) fn load(&mut self) {
        self.loaded = None;
        self.notes.clear();
        self.problem = None;
        let path = Path::new(self.file.trim());
        if self.file.trim().is_empty() {
            self.problem = Some(tr("Say which file to read.").into());
            return;
        }
        let read = if is_dxf(path) {
            dxf::read(path).map(|drawing| {
                self.notes.clone_from(&drawing.notes);
                Loaded::Section {
                    drawing: Box::new(drawing),
                    drawn: Box::new(Drawn::default()),
                    tunnel: Tunnel::default(),
                    cell_size: 0.0,
                }
            })
        } else {
            vortia_cad::load(path).map(|surface| {
                let summary = surface.summary();
                Loaded::Body {
                    body: Box::new(Body::new(surface)),
                    summary,
                    repaired: Vec::new(),
                }
            })
        };
        match read {
            Ok(loaded) => self.loaded = Some(loaded),
            Err(e) => self.problem = Some(e.to_string()),
        }
    }

    /// The case the form describes, or why it cannot be made yet.
    pub(super) fn geometry(&self) -> Result<Geometry, String> {
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            return Err(tr("The scale of the file must be a positive number.").into());
        };
        match &self.loaded {
            None => Err(tr("Read a file first.").into()),
            Some(Loaded::Body { body, .. }) => {
                let mut body = body.as_ref().clone();
                if scale != 1.0 {
                    body.surface.scale(scale).map_err(|e| e.to_string())?;
                }
                // Refuse here, with the mesher's own words, rather than after a folder is made.
                body.spec().map_err(|e| e.to_string())?;
                Ok(Geometry::Body(body))
            }
            Some(Loaded::Section {
                drawing,
                drawn,
                tunnel,
                cell_size,
            }) => {
                let mut drawing = drawing.as_ref().clone();
                if scale != 1.0 {
                    drawing.scale(scale).map_err(|e| e.to_string())?;
                }
                let mut drawn = drawn.as_ref().clone();
                if *cell_size > 0.0 {
                    drawn.cell_size = *cell_size;
                } else if let Some((lo, hi)) = drawing.bounds() {
                    drawn.cell_size = ((hi.0 - lo.0).max(hi.1 - lo.1) / 40.0).max(1.0e-3);
                }
                drawn.far_cell_size = Some(16.0 * drawn.cell_size);
                drawn.sketch = drawing.around(*tunnel).map_err(|e| e.to_string())?;
                Ok(Geometry::Drawn(drawn))
            }
        }
    }

    pub(super) fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.file)
                    .hint_text(tr("A .stl, .obj, .ply, .gltf, .glb, .step or .dxf file"))
                    .desired_width(320.0),
            );
            if ui.button(tr("Read file")).clicked() {
                self.load();
            }
        });
        if let Some(problem) = &self.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
        for note in &self.notes {
            ui.colored_label(ui.visuals().warn_fg_color, note);
        }
        let scale = self.scale;
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        ui.add_space(8.0);
        egui::Grid::new("new_imported")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                let (a, b) = (
                    tr("Scale of the file"),
                    tr(
                        "Multiplies every length of the file: 0.001 turns the millimetres of an STL, OBJ, PLY or glTF into metres. A STEP file carries its own unit.",
                    ),
                );
                ui.label(a).on_hover_text(b);
                ui.add(
                    DragValue::new(&mut self.scale)
                        .speed(0.0001)
                        .range(1.0e-9..=1.0e9)
                        .max_decimals(9),
                );
                ui.end_row();
                match loaded {
                    Loaded::Body { body, summary, .. } => body_fields(ui, body, summary, scale),
                    Loaded::Section {
                        drawing,
                        drawn,
                        tunnel,
                        cell_size,
                    } => section_fields(ui, drawing, drawn, tunnel, cell_size),
                }
            });
        match loaded {
            Loaded::Section { drawing, .. } => {
                ui.add_space(8.0);
                preview(ui, drawing, 220.0);
            }
            Loaded::Body {
                body,
                summary,
                repaired,
            } => repair_ui(ui, body, summary, repaired),
        }
    }
}

/// The "Repair the surface" button, shown while the surface is not closed (or once it was
/// repaired, to say what was done), and what the repair found.
fn repair_ui(ui: &mut Ui, body: &mut Body, summary: &mut Summary, repaired: &mut Vec<String>) {
    if !summary.closed() && ui.button(tr("Repair the surface")).on_hover_text(tr(
        "Welds the corners written twice, takes out triangles with no area and repeated ones, turns triangles to agree, and fills small holes. What it cannot decide is left and counted.",
    )).clicked()
    {
        match vortia_mesh::repair::repair(&body.surface, vortia_mesh::repair::Options::default()) {
            Ok((surface, report)) => {
                body.surface = surface;
                *summary = report.after.clone();
                *repaired = repair_lines(&report);
            }
            Err(e) => *repaired = vec![e.to_string()],
        }
    }
    for line in repaired.iter() {
        ui.label(line);
    }
}

/// What a repair found and did, in the app's words.
fn repair_lines(report: &vortia_mesh::repair::Report) -> Vec<String> {
    let mut lines = Vec::new();
    let mut done = |n: usize, template: &'static str| {
        if n > 0 {
            lines.push(trf(template, &[&n]));
        }
    };
    done(report.welded, "{} corners welded into their neighbours");
    done(report.degenerate, "{} triangles with no area removed");
    done(report.repeated, "{} repeated triangles removed");
    done(
        report.reoriented,
        "{} triangles turned to agree with their neighbours",
    );
    done(report.turned_out, "{} closed pieces turned right way out");
    if report.holes_filled > 0 {
        lines.push(trf(
            "{} holes filled with {} triangles",
            &[&report.holes_filled, &report.triangles_added],
        ));
    }
    if !report.changed() {
        lines.push(tr("Nothing needed repairing.").into());
    }
    let after = &report.after;
    lines.push(if report.watertight() {
        trf(
            "The surface is now closed and consistently wound: {} triangles.",
            &[&after.triangles],
        )
    } else {
        trf(
            "Still not closed: {} open edges, {} shared by more than two triangles, {} edges wound against their neighbour.",
            &[
                &after.open_edges,
                &after.non_manifold_edges,
                &report.inconsistent_after,
            ],
        )
    });
    lines
}

/// How far the tunnel reaches ahead of the body, behind it and at each side, in lengths of it.
fn tunnel_rows(ui: &mut Ui, [upstream, downstream, lateral]: [&mut f64; 3]) {
    row(ui, tr("Tunnel ahead of the body"), |ui| {
        value(ui, upstream, "L")
    });
    row(ui, tr("Tunnel behind the body"), |ui| {
        value(ui, downstream, "L")
    });
    row(ui, tr("Tunnel at each side"), |ui| value(ui, lateral, "L"));
}

pub(super) fn body_fields(ui: &mut Ui, body: &mut Body, summary: &Summary, scale: f64) {
    let extent = |axis: usize| {
        let at = |v: [f64; 3]| v.get(axis).copied().unwrap_or(0.0);
        (at(summary.high) - at(summary.low)) * scale
    };
    row(ui, tr("The body"), |ui| {
        ui.label(trf(
            "{} triangles, {} × {} × {} m",
            &[
                &summary.triangles,
                &format!("{:.4}", extent(0)),
                &format!("{:.4}", extent(1)),
                &format!("{:.4}", extent(2)),
            ],
        ));
    });
    if !summary.closed() {
        row(ui, "", |ui| {
            ui.colored_label(
                ui.visuals().error_fg_color,
                trf(
                    "The surface is not closed: {} open edges, {} shared by more than two triangles.",
                    &[&summary.open_edges, &summary.non_manifold_edges],
                ),
            );
        });
    }
    row(ui, tr("Stream speed"), |ui| {
        value(ui, &mut body.speed, "m/s")
    });
    row(ui, tr("Reynolds number"), |ui| {
        value(ui, &mut body.reynolds, "");
    });
    row(ui, tr("Reference length (0: along the stream)"), |ui| {
        value(ui, &mut body.length, "m");
    });
    tunnel_rows(
        ui,
        [&mut body.upstream, &mut body.downstream, &mut body.lateral],
    );
    row(ui, tr("Base cells along the length"), |ui| {
        ui.add(DragValue::new(&mut body.cells).range(1..=200));
    });
    row(ui, tr("Refinements at the body"), |ui| {
        ui.add(DragValue::new(&mut body.levels).range(0..=8));
    });
    row(ui, tr("Refinements in the wake"), |ui| {
        ui.add(DragValue::new(&mut body.wake_levels).range(0..=8));
    });
    if let Some(parts) = vortia_mesh::bodymesh::part_patches(&body.surface, "body") {
        row(ui, tr("Wall by part"), |ui| {
            ui.checkbox(
                &mut body.by_part,
                trf(
                    "A wall for each of the {} parts, to read their forces apart",
                    &[&parts.len()],
                ),
            );
        });
    }
    row(ui, tr("Prism layers on the wall"), |ui| {
        ui.add(DragValue::new(&mut body.layers).range(0..=20));
    });
    if body.layers > 0 {
        row(ui, tr("Growth of each layer"), |ui| {
            ui.add(
                DragValue::new(&mut body.layer_growth)
                    .range(1.0..=3.0)
                    .speed(0.01),
            );
        });
        row(
            ui,
            tr("First layer for y+ (0: a tenth of the cell)"),
            |ui| {
                ui.add(DragValue::new(&mut body.yplus).range(0.0..=1000.0));
            },
        );
    }
    row(ui, tr("Turbulence"), |ui| {
        ui.horizontal(|ui| {
            for (model, name) in [
                (BodyModel::Laminar, tr("Laminar")),
                (BodyModel::SpalartAllmaras, "Spalart–Allmaras"),
                (BodyModel::KOmegaSst, "k-ω SST"),
            ] {
                ui.selectable_value(&mut body.model, model, name);
            }
        });
    });
}

fn section_fields(
    ui: &mut Ui,
    drawing: &Drawing,
    drawn: &mut Drawn,
    tunnel: &mut Tunnel,
    cell_size: &mut f64,
) {
    let closed = drawing.outlines.len();
    row(ui, tr("The drawing"), |ui| {
        ui.label(trf("{} closed outlines", &[&closed]));
    });
    row(ui, tr("Stream speed"), |ui| {
        value(ui, &mut drawn.speed, "m/s")
    });
    row(ui, tr("Kinematic viscosity"), |ui| {
        value(ui, &mut drawn.viscosity, "m²/s");
    });
    tunnel_rows(
        ui,
        [
            &mut tunnel.upstream,
            &mut tunnel.downstream,
            &mut tunnel.lateral,
        ],
    );
    row(ui, tr("Cell size at the body (0: automatic)"), |ui| {
        value(ui, cell_size, "mm");
    });
    row(ui, tr("Depth of the one cell"), |ui| {
        value(ui, &mut drawn.thickness, "mm");
    });
}

/// The outlines of the drawing, fitted to a box `height` high.
pub(super) fn preview(ui: &mut Ui, drawing: &Drawing, height: f32) {
    let width = ui.available_width().clamp(200.0, 1280.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 6.0, visuals.extreme_bg_color);
    let Some((lo, hi)) = drawing.bounds() else {
        return;
    };
    let inner = rect.shrink(16.0);
    let scale = (f64::from(inner.width()) / (hi.0 - lo.0).max(1.0e-12))
        .min(f64::from(inner.height()) / (hi.1 - lo.1).max(1.0e-12));
    let at = |(x, y): (f64, f64)| {
        egui::pos2(
            inner.center().x + ((x - 0.5 * (lo.0 + hi.0)) * scale) as f32,
            inner.center().y - ((y - 0.5 * (lo.1 + hi.1)) * scale) as f32,
        )
    };
    let stroke = egui::Stroke::new(1.5, visuals.text_color());
    for outline in &drawing.outlines {
        match outline {
            Outline::Circle { center, radius, .. } => {
                painter.circle_stroke(at(*center), (radius * scale) as f32, stroke);
            }
            Outline::Loop(edges) => {
                for edge in edges {
                    let line: Vec<_> = edge.points.iter().map(|&p| at(p)).collect();
                    painter.add(egui::Shape::line(line, stroke));
                }
            }
        }
    }
}

/// A circle of radius 10 mm drawn in millimetres.
#[cfg(test)]
pub(super) const CIRCLE_DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n10\n0\nENDSEC\n0\nEOF\n";

#[cfg(test)]
mod tests {
    use super::*;

    /// An axis-aligned cube of side 1, as an ASCII STL.
    fn cube() -> String {
        let v = |x: u8, y: u8, z: u8| format!("vertex {x} {y} {z}\n");
        let quad = |a: [u8; 3], b: [u8; 3], c: [u8; 3], d: [u8; 3]| {
            let mut t = String::new();
            for tri in [[a, b, c], [a, c, d]] {
                t += "facet normal 0 0 0\nouter loop\n";
                for p in tri {
                    t += &v(p[0], p[1], p[2]);
                }
                t += "endloop\nendfacet\n";
            }
            t
        };
        let mut s = String::from("solid cube\n");
        s += &quad([0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]);
        s += &quad([0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]);
        s += &quad([0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]);
        s += &quad([0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]);
        s += &quad([0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]);
        s += &quad([1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]);
        s + "endsolid cube\n"
    }

    #[test]
    fn a_cube_missing_a_triangle_is_repaired_and_the_report_says_so() {
        let dir = tempfile::tempdir().unwrap();
        // Drop the last facet: a triangular hole in the cube.
        let text = cube();
        let cut = text.rfind("facet normal").unwrap();
        let cut = text[..cut].rfind("facet normal").unwrap();
        let open = format!("{}endsolid cube\n", &text[..cut]);
        let path = written(&dir, "open.stl", &open);
        let mut form = Imported {
            file: path.display().to_string(),
            ..Imported::default()
        };
        form.load();
        let Some(Loaded::Body { body, summary, .. }) = &mut form.loaded else {
            panic!("the file reads: {:?}", form.problem);
        };
        assert!(!summary.closed());
        let (surface, report) =
            vortia_mesh::repair::repair(&body.surface, vortia_mesh::repair::Options::default())
                .unwrap();
        assert!(report.watertight(), "{report:?}");
        let lines = repair_lines(&report);
        assert!(
            lines.iter().any(|l| l.contains("holes filled")),
            "{lines:?}"
        );
        assert!(lines.last().unwrap().contains("closed"), "{lines:?}");
        assert!(surface.summary().closed());
    }

    fn written(dir: &tempfile::TempDir, name: &str, text: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// The form after reading the file.
    fn read(path: &Path) -> Imported {
        let mut form = Imported {
            file: path.display().to_string(),
            ..Imported::default()
        };
        form.load();
        form
    }

    #[test]
    fn nothing_is_made_before_a_file_is_read() {
        let form = Imported::default();
        assert!(form.geometry().is_err());
        let mut form = Imported::default();
        form.load();
        assert!(form.problem.is_some());
        form.file = "/no/such/file.stl".into();
        form.load();
        assert!(form.problem.is_some() && form.geometry().is_err());
    }

    #[test]
    fn a_drawing_becomes_a_flow_around_its_outlines() {
        let dir = tempfile::tempdir().unwrap();
        let path = written(&dir, "circle.dxf", CIRCLE_DXF);
        let form = read(&path);
        assert!(form.problem.is_none(), "{:?}", form.problem);
        let Geometry::Drawn(drawn) = form.geometry().unwrap() else {
            panic!("a drawing is a drawn case");
        };
        // The cells along the circle are a fortieth of its 20 mm, 0.5 mm, and grow out to sixteen times.
        assert!(
            (drawn.cell_size - 0.5).abs() < 1.0e-9,
            "{}",
            drawn.cell_size
        );
        assert_eq!(drawn.far_cell_size, Some(16.0 * drawn.cell_size));
        assert!(drawn.sketch.points.len() > 4);
    }

    #[test]
    fn a_surface_becomes_a_flow_around_a_body_and_an_open_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = written(&dir, "cube.stl", &cube());
        let mut form = read(&path);
        assert!(form.problem.is_none(), "{:?}", form.problem);
        let Geometry::Body(body) = form.geometry().unwrap() else {
            panic!("a surface is a body");
        };
        assert_eq!(body.surface.summary().triangles, 12);
        // The unit of the file reaches the surface.
        form.scale = 0.001;
        let Geometry::Body(small) = form.geometry().unwrap() else {
            panic!("a surface is a body");
        };
        assert!((small.surface.summary().high[0] - 0.001).abs() < 1.0e-12);
        form.scale = 0.0;
        assert!(form.geometry().is_err());

        let open = cube();
        let open: String = open
            .lines()
            .take(open.lines().count() - 8)
            .chain(["endsolid cube"])
            .map(|l| format!("{l}\n"))
            .collect();
        let hole = written(&dir, "open.stl", &open);
        let form = read(&hole);
        assert!(form.geometry().is_err());
    }

    #[test]
    fn the_objects_of_an_obj_file_reach_the_body_as_its_parts() {
        // Two unit cubes a unit apart, one object each (quads, outward).
        let mut text = String::new();
        let mut base = 0;
        for (name, x) in [("hull", 0.0), ("keel", 2.0)] {
            text += &format!("o {name}\n");
            for (dx, dy, dz) in [
                (0, 0, 0),
                (1, 0, 0),
                (1, 1, 0),
                (0, 1, 0),
                (0, 0, 1),
                (1, 0, 1),
                (1, 1, 1),
                (0, 1, 1),
            ] {
                text += &format!("v {} {dy} {dz}\n", x + f64::from(dx));
            }
            for quad in [
                [1, 4, 3, 2],
                [5, 6, 7, 8],
                [1, 2, 6, 5],
                [2, 3, 7, 6],
                [3, 4, 8, 7],
                [4, 1, 5, 8],
            ] {
                let [a, b, c, d] = quad.map(|i| i + base);
                text += &format!("f {a} {b} {c} {d}\n");
            }
            base += 8;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = written(&dir, "boat.obj", &text);
        let form = read(&path);
        assert!(form.problem.is_none(), "{:?}", form.problem);
        let Geometry::Body(body) = form.geometry().unwrap() else {
            panic!("a surface is a body");
        };
        assert_eq!(body.surface.parts.len(), 2);
        assert!(body.by_part);
        assert_eq!(
            vortia_mesh::bodymesh::part_patches(&body.surface, "body").unwrap(),
            ["body_hull", "body_keel"]
        );
    }

    #[test]
    fn a_ply_and_a_glb_are_read_like_any_other_surface() {
        let dir = tempfile::tempdir().unwrap();
        let ply = "ply\nformat ascii 1.0\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nelement face 6\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n0 0 1\n1 0 1\n1 1 1\n0 1 1\n4 0 3 2 1\n4 4 5 6 7\n4 0 1 5 4\n4 2 3 7 6\n4 1 2 6 5\n4 3 0 4 7\n";
        let path = written(&dir, "box.ply", ply);
        let form = read(&path);
        assert!(form.problem.is_none(), "{:?}", form.problem);
        assert!(matches!(form.geometry(), Ok(Geometry::Body(_))));
        let broken = written(&dir, "broken.gltf", "{\"asset\":");
        assert!(read(&broken).problem.is_some());
    }
}
