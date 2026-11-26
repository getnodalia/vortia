// SPDX-License-Identifier: GPL-3.0-or-later
//! The Surface card of the Results tab: the pressure coefficient `Cp` and the friction
//! coefficient `Cf` on the faces of the walls, plotted along a coordinate. It takes the walls,
//! the speed and the stream direction from the Forces card, so they are asked for once. Worked
//! out by `vortia-post`.

use crate::i18n::{tr, trf};
use std::sync::Arc;

use eframe::egui::{self, Align2, FontId, Pos2, RichText, Sense, Stroke, Vec2};
use nodalia_theme::{Palette, component, font_size, for_egui, space};
use vortia_post::{Case, SurfaceReport};

use super::analysis::write_report;
use super::forces::axis_name;
use super::{Results, Slot, card, plain, spawn, wait};
use crate::app::{accent_button, corner, muted};
use crate::facepaint::FacePaint;

/// The most points a plot paints; a wall with more faces is thinned for the picture (the
/// numbers and the CSV keep every face).
const MAX_DOTS: usize = 6_000;
const HEIGHT: f32 = 180.0;

/// What the card asks, as typed.
#[derive(Clone)]
pub(super) struct SurfaceForm {
    /// The axis the faces are plotted along.
    pub(super) along: usize,
    /// The pressure of the undisturbed stream, taken off the wall pressure.
    pub(super) free_pressure: String,
}

impl Default for SurfaceForm {
    fn default() -> Self {
        Self {
            along: 0,
            free_pressure: "0".into(),
        }
    }
}

impl SurfaceForm {
    pub(super) fn free_pressure(&self) -> Result<f64, String> {
        self.free_pressure
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| trf("{} needs a number.", &[&tr("The free-stream pressure")]))
    }
}

/// `(coordinate, value)` for each face, thinned to [`MAX_DOTS`]; `value` is chosen by `pick`.
pub(super) fn dots(
    report: &SurfaceReport,
    along: usize,
    pick: impl Fn(&vortia_post::SurfaceFace) -> f64,
) -> Vec<(f64, f64)> {
    let step = report.faces.len().div_ceil(MAX_DOTS).max(1);
    report
        .faces
        .iter()
        .step_by(step)
        .map(|f| {
            let at = match along {
                0 => f.centre.x,
                1 => f.centre.y,
                _ => f.centre.z,
            };
            (at, pick(f))
        })
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect()
}

pub(super) fn surface_card(
    ui: &mut egui::Ui,
    p: &Palette,
    case: &Arc<Case>,
    results: &mut Results,
) {
    let (mut ask, mut save) = (false, false);
    let mut paint: Option<FacePaint> = None;
    card(ui, p, |ui| {
        ui.label(RichText::new(tr("Pressure and friction on the surface")).strong());
        ui.label(
            RichText::new(tr("Cp and Cf on the faces of the walls chosen in Forces."))
                .size(font_size::SMALL)
                .color(muted(p)),
        );
        ui.add_space(space::SM);
        if results.forces_form.is_empty() {
            ui.label(RichText::new(tr("The case has no wall with results.")).color(muted(p)));
            return;
        }
        let form = &mut results.surface_form;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Plot along")).color(muted(p)));
            for i in 0..3 {
                ui.selectable_value(&mut form.along, i, axis_name(i));
            }
            ui.add_space(space::MD);
            ui.label(RichText::new(tr("Free-stream pressure")).color(muted(p)));
            ui.add(egui::TextEdit::singleline(&mut form.free_pressure).desired_width(70.0));
        });
        ui.add_space(space::SM);
        ask = accent_button(ui, p, tr("Show the coefficients")).clicked();
        match &results.surface {
            Slot::Idle => {}
            Slot::Working(_) => wait(ui, tr("Reading the faces…")),
            Slot::Done(Err(message)) => {
                ui.colored_label(for_egui::color(p.status_danger_fg), message.as_str());
            }
            Slot::Done(Ok(report)) => {
                ui.add_space(space::SM);
                summary(ui, report);
                let along = results.surface_form.along;
                ui.add_space(space::SM);
                ui.label(RichText::new(tr("Pressure coefficient (suction up)")).strong());
                scatter(
                    ui,
                    p,
                    &dots(report, along, |f| -f.cp),
                    axis_name(along),
                    "−Cp",
                );
                ui.add_space(space::SM);
                ui.label(RichText::new(tr("Friction along the stream")).strong());
                scatter(
                    ui,
                    p,
                    &dots(report, along, |f| f.cf_stream),
                    axis_name(along),
                    "Cf",
                );
                ui.add_space(space::SM);
                ui.horizontal(|ui| {
                    if ui.button(trf("Save {}", &[&"CSV"])).clicked() {
                        save = true;
                    }
                    if ui.button(tr("Paint Cp on the body")).clicked() {
                        paint = Some(FacePaint::from_report("Cp", report, |f| f.cp));
                    }
                    if ui.button(tr("Paint Cf on the body")).clicked() {
                        paint = Some(FacePaint::from_report("Cf", report, |f| f.cf_stream));
                    }
                    if let Some((ok, message)) = &results.saved_surface {
                        let tone = if *ok {
                            p.status_success_fg
                        } else {
                            p.status_danger_fg
                        };
                        ui.colored_label(for_egui::color(tone), message.as_str());
                    }
                });
            }
        }
    });
    if ask {
        let Some(time) = results.time.clone() else {
            return;
        };
        results.saved_surface = None;
        let asked = results
            .forces_form
            .parse()
            .and_then(|(patches, reference)| {
                results
                    .surface_form
                    .free_pressure()
                    .map(|p| (patches, reference, p))
            });
        results.surface = match asked {
            Err(message) => Slot::Done(Err(message)),
            Ok((patches, reference, free)) => {
                let case = Arc::clone(case);
                spawn(ui.ctx(), move || {
                    case.surface_coefficients(&time, &patches, &reference, free)
                        .map_err(|e| e.to_string())
                })
            }
        };
    }
    if paint.is_some() {
        results.paint = paint;
    }
    if save && let Slot::Done(Ok(report)) = &results.surface {
        results.saved_surface = Some(write_report(
            case.path(),
            "surface",
            &report.time,
            "surface.csv",
            &report.csv(),
        ));
    }
}

fn summary(ui: &mut egui::Ui, report: &SurfaceReport) {
    egui::Grid::new("surface-summary")
        .num_columns(2)
        .spacing([space::XL, space::XS])
        .striped(true)
        .show(ui, |ui| {
            let mut row = |name: &str, value: String| {
                ui.label(name);
                ui.label(RichText::new(value).monospace());
                ui.end_row();
            };
            row(tr("Faces"), report.faces.len().to_string());
            row(tr("Dynamic pressure"), plain(report.dynamic_pressure));
            if let Some((low, high)) = report.cp_range() {
                row(
                    tr("Cp, lowest and highest"),
                    format!("{} · {}", plain(low), plain(high)),
                );
            }
            if let Some((low, high)) = report.cf_range() {
                row(
                    tr("Cf, lowest and highest"),
                    format!("{} · {}", plain(low), plain(high)),
                );
            }
            let total: f64 = report.faces.iter().map(|f| f.area).sum();
            if total > 0.0 {
                row(
                    tr("Share of the area with reversed flow"),
                    format!("{} %", plain(100.0 * report.reversed_area() / total)),
                );
            }
        });
}

/// Dots of `(x, y)`, with the extremes written on the frame.
fn scatter(ui: &mut egui::Ui, p: &Palette, points: &[(f64, f64)], x_name: &str, y_name: &str) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        corner(component::CONTROL_RADIUS),
        for_egui::color(p.surface_panel),
    );
    let small = FontId::proportional(font_size::SMALL);
    let (Some(x0), Some(x1), Some(low), Some(high)) = (
        points.iter().map(|d| d.0).reduce(f64::min),
        points.iter().map(|d| d.0).reduce(f64::max),
        points.iter().map(|d| d.1).reduce(f64::min),
        points.iter().map(|d| d.1).reduce(f64::max),
    ) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr("There are no faces to plot"),
            small,
            muted(p),
        );
        return;
    };
    let (mut y0, mut y1) = (low, high);
    if y1 - y0 <= f64::EPSILON * y0.abs().max(1.0) {
        y0 -= 1.0;
        y1 += 1.0;
    }
    let pad = (y1 - y0) * 0.06;
    let (y0, y1) = (y0 - pad, y1 + pad);
    let inner = rect.shrink2(Vec2::new(10.0, 22.0));
    let span_x = (x1 - x0).max(f64::EPSILON);
    let to_screen = |(x, y): (f64, f64)| {
        Pos2::new(
            inner.left() + ((x - x0) / span_x) as f32 * inner.width(),
            inner.bottom() - ((y - y0) / (y1 - y0)) as f32 * inner.height(),
        )
    };
    let grid = Stroke::new(1.0, for_egui::color(p.border_subtle));
    if y0 < 0.0 && y1 > 0.0 {
        let at = to_screen((x0, 0.0)).y;
        painter.line_segment(
            [Pos2::new(inner.left(), at), Pos2::new(inner.right(), at)],
            grid,
        );
    }
    let ink = for_egui::color(p.accent_default).gamma_multiply(0.7);
    for &d in points {
        painter.circle_filled(to_screen(d), 1.6, ink);
    }
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 4.0),
        Align2::LEFT_TOP,
        format!("{y_name} {}", plain(high)),
        small.clone(),
        muted(p),
    );
    painter.text(
        Pos2::new(rect.left() + 8.0, inner.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        plain(low),
        small.clone(),
        muted(p),
    );
    painter.text(
        Pos2::new(inner.left(), rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        plain(x0),
        small.clone(),
        muted(p),
    );
    painter.text(
        Pos2::new(inner.right(), rect.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        format!("{x_name} {}", plain(x1)),
        small.clone(),
        muted(p),
    );
    if let Some(at) = response
        .hover_pos()
        .filter(|at| inner.expand2(Vec2::new(6.0, 0.0)).contains(*at))
        && let Some(&(x, y)) = points.iter().min_by(|a, b| {
            let (da, db) = (
                (to_screen(**a).x - at.x).abs(),
                (to_screen(**b).x - at.x).abs(),
            );
            da.total_cmp(&db)
        })
    {
        let spot = to_screen((x, y));
        painter.circle_filled(spot, 3.5, for_egui::color(p.accent_default));
        painter.text(
            Pos2::new(inner.right(), rect.top() + 4.0),
            Align2::RIGHT_TOP,
            format!("{y_name} {}  at {x_name} {}", plain(y), plain(x)),
            small,
            muted(p),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortia_core::Vec3;
    use vortia_post::SurfaceFace;

    fn report(n: usize) -> SurfaceReport {
        SurfaceReport {
            time: "1".into(),
            patches: vec![("body".into(), n)],
            dynamic_pressure: 2.0,
            faces: (0..n)
                .map(|k| SurfaceFace {
                    centre: Vec3::new(k as f64, 2.0 * k as f64, 0.0),
                    normal: Vec3::new(0.0, 1.0, 0.0),
                    area: 1.0,
                    cp: k as f64,
                    cf: 0.1,
                    cf_stream: -0.1,
                })
                .collect(),
        }
    }

    #[test]
    fn the_dots_follow_the_chosen_axis_and_a_large_wall_is_thinned() {
        let small = report(10);
        let along_y = dots(&small, 1, |f| f.cp);
        assert_eq!(along_y.len(), 10);
        assert_eq!(along_y.get(3), Some(&(6.0, 3.0)));
        assert_eq!(dots(&small, 0, |f| f.cp).get(3), Some(&(3.0, 3.0)));
        let big = report(MAX_DOTS * 3 + 5);
        let thinned = dots(&big, 0, |f| f.cp);
        assert!(thinned.len() <= MAX_DOTS && thinned.len() > MAX_DOTS / 2);
    }

    #[test]
    fn the_free_stream_pressure_must_be_a_number() {
        let mut form = SurfaceForm::default();
        assert_eq!(form.free_pressure().unwrap(), 0.0);
        form.free_pressure = " 101325 ".into();
        assert_eq!(form.free_pressure().unwrap(), 101_325.0);
        form.free_pressure = "high".into();
        assert!(form.free_pressure().is_err());
        form.free_pressure = "inf".into();
        assert!(form.free_pressure().is_err());
    }

    /// A 4 × 2 × 1 box of unit cells with a wall at rest on the bottom and a pressure on it.
    fn case_with_a_wall() -> tempfile::TempDir {
        use vortia_core::Aabb;
        use vortia_mesh::blocks::structured_box_raw;
        let dir = tempfile::tempdir().unwrap();
        structured_box_raw(
            Aabb {
                min: Vec3::default(),
                max: Vec3::new(4.0, 2.0, 1.0),
            },
            [4, 2, 1],
        )
        .unwrap()
        .write_case(dir.path())
        .unwrap();
        std::fs::create_dir(dir.path().join("0")).unwrap();
        std::fs::write(
            dir.path().join("constant/transportProperties"),
            "FoamFile { version 2.0; format ascii; class dictionary; object transportProperties; }\n\
             nu [0 2 -1 0 0 0 0] 0.01;\n",
        )
        .unwrap();
        for (name, class, value, wall) in [
            ("U", "volVectorField", "uniform (2 0 0)", "type noSlip;"),
            (
                "p",
                "volScalarField",
                "uniform 3",
                "type calculated; value uniform 3;",
            ),
        ] {
            let patches: String = ["xMin", "xMax", "yMin", "yMax", "zMin", "zMax"]
                .iter()
                .map(|p| {
                    let body = if *p == "yMin" {
                        wall
                    } else {
                        "type zeroGradient;"
                    };
                    format!("    {p} {{ {body} }}\n")
                })
                .collect();
            std::fs::write(
                dir.path().join("0").join(name),
                format!(
                    "FoamFile {{ version 2.0; format ascii; class {class}; object {name}; }}\n\
                     dimensions [0 0 0 0 0 0 0];\ninternalField {value};\n\
                     boundaryField\n{{\n{patches}}}\n"
                ),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn the_card_draws_the_coefficients_of_a_wall_and_saves_them() {
        let dir = case_with_a_wall();
        let ctx = egui::Context::default();
        let mut results = Results::default();
        let draw = |results: &mut Results| {
            let p = crate::app::palette(&ctx);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                super::super::results_tab(ui, &p, dir.path(), results);
            });
            out.textures_delta.clear();
        };
        for _ in 0..500 {
            draw(&mut results);
            if matches!(results.case, Slot::Done(_)) && results.time.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let Slot::Done(Ok(case)) = &results.case else {
            panic!("the case was not read");
        };
        let case = Arc::clone(case);
        let wall = vortia_post::WallReport {
            patch: "yMin".into(),
            kinematic: true,
            area: 4.0,
            shear: Vec::new(),
            y_plus: Vec::new(),
            heat_flux: None,
            areas: Vec::new(),
            viscous_force: Vec3::default(),
            pressure_force: None,
        };
        results.forces_form.with_walls(&[wall], false);
        let (patches, mut reference) = results.forces_form.parse().unwrap();
        reference.speed = 2.0;
        let time = results.time.clone().unwrap();
        let report = case
            .surface_coefficients(&time, &patches, &reference, 0.0)
            .unwrap();
        assert!(!report.faces.is_empty());
        results.surface = Slot::Done(Ok(report));
        draw(&mut results);
        // Saved where the case keeps its reports.
        let Slot::Done(Ok(report)) = &results.surface else {
            panic!("the report went missing");
        };
        let (ok, message) = write_report(
            dir.path(),
            "surface",
            &report.time,
            "surface.csv",
            &report.csv(),
        );
        assert!(ok, "{message}");
        assert!(
            dir.path()
                .join("postProcessing/surface/0/surface.csv")
                .is_file()
        );
        results.surface = Slot::Done(Err("no".into()));
        draw(&mut results);
    }
}
