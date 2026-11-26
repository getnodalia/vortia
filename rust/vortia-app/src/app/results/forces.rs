//! The Forces card of the Results tab: the force and moment the flow puts on the walls, split
//! into pressure and viscous, along a drag and a lift direction, with the coefficients and a
//! report in JSON, CSV or HTML. Worked out by `vortia-post`.

use crate::i18n::{tr, trf};
use std::sync::Arc;

use eframe::egui::{self, RichText};
use nodalia_theme::{Palette, font_size, for_egui, space};
use vortia_core::Vec3;
use vortia_post::{Case, ForceReference, ForceReport, WallReport};

use super::analysis::{field, write_report};
use super::{Results, Slot, card, plain, spawn, wait};
use crate::app::{accent_button, muted};

/// What the card asks, as typed.
#[derive(Clone)]
pub(super) struct ForcesForm {
    /// Each wall of the case, and whether its forces are counted.
    walls: Vec<(String, bool)>,
    /// The axis the stream goes along (the drag), and the one the lift is along.
    drag: usize,
    lift: usize,
    speed: String,
    area: String,
    length: String,
    /// Forces in newtons: the density of an incompressible case; a gas must give one.
    give_density: bool,
    density: String,
    centre: [String; 3],
}

impl Default for ForcesForm {
    fn default() -> Self {
        Self {
            walls: Vec::new(),
            drag: 0,
            lift: 1,
            speed: "1".into(),
            area: "1".into(),
            length: "1".into(),
            give_density: false,
            density: "1.225".into(),
            centre: ["0".into(), "0".into(), "0".into()],
        }
    }
}

fn axis(i: usize) -> Vec3 {
    match i {
        0 => Vec3::new(1.0, 0.0, 0.0),
        1 => Vec3::new(0.0, 1.0, 0.0),
        _ => Vec3::new(0.0, 0.0, 1.0),
    }
}

/// The name of an axis.
pub(super) fn axis_name(i: usize) -> &'static str {
    match i {
        0 => "x",
        1 => "y",
        _ => "z",
    }
}

impl ForcesForm {
    /// Whether no wall has been given yet.
    pub(super) fn is_empty(&self) -> bool {
        self.walls.is_empty()
    }

    /// The walls the run has, all counted; a gas (which has `rho`) must give a density.
    pub(super) fn with_walls(&mut self, walls: &[WallReport], compressible: bool) {
        self.walls = walls.iter().map(|w| (w.patch.clone(), true)).collect();
        self.give_density = compressible;
    }

    pub(super) fn parse(&self) -> Result<(Vec<String>, ForceReference), String> {
        let number = |text: &str, what: &str| {
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| trf("{} needs a number.", &[&what]))
        };
        let patches: Vec<String> = self
            .walls
            .iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| name.clone())
            .collect();
        if patches.is_empty() {
            return Err(tr("Choose at least one wall.").into());
        }
        if self.drag == self.lift {
            return Err(tr("The stream and the lift cannot be along the same axis.").into());
        }
        let density = if self.give_density {
            Some(number(&self.density, tr("The density"))?)
        } else {
            None
        };
        let [cx, cy, cz] = &self.centre;
        Ok((
            patches,
            ForceReference {
                density,
                speed: number(&self.speed, tr("The reference speed"))?,
                area: number(&self.area, tr("The reference area"))?,
                length: number(&self.length, tr("The reference length"))?,
                centre: Vec3::new(
                    number(cx, tr("The centre of rotation"))?,
                    number(cy, tr("The centre of rotation"))?,
                    number(cz, tr("The centre of rotation"))?,
                ),
                drag: axis(self.drag),
                lift: axis(self.lift),
            },
        ))
    }
}

pub(super) fn forces_card(ui: &mut egui::Ui, p: &Palette, case: &Arc<Case>, results: &mut Results) {
    let (mut ask, mut save) = (false, None::<&'static str>);
    card(ui, p, |ui| {
        ui.label(RichText::new(tr("Forces")).strong());
        ui.label(
            RichText::new(tr(tr(
                "Force and moment on the walls, with their coefficients.",
            )))
            .size(font_size::SMALL)
            .color(muted(p)),
        );
        ui.add_space(space::SM);
        let form = &mut results.forces_form;
        if form.walls.is_empty() {
            ui.label(RichText::new(tr("The case has no wall with results.")).color(muted(p)));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Walls")).color(muted(p)));
            for (name, on) in &mut form.walls {
                ui.checkbox(on, name.as_str());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Stream along")).color(muted(p)));
            for (i, name) in ["x", "y", "z"].iter().enumerate() {
                ui.selectable_value(&mut form.drag, i, *name);
            }
            ui.add_space(space::MD);
            ui.label(RichText::new(tr("lift along")).color(muted(p)));
            for (i, name) in ["x", "y", "z"].iter().enumerate() {
                ui.selectable_value(&mut form.lift, i, *name);
            }
        });
        ui.horizontal_wrapped(|ui| {
            field(ui, p, tr("Speed"), "m/s", &mut form.speed);
            field(ui, p, tr("Area"), "m²", &mut form.area);
            field(ui, p, tr("Length"), "m", &mut form.length);
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut form.give_density, tr("Density"));
            if form.give_density {
                ui.add(egui::TextEdit::singleline(&mut form.density).desired_width(70.0));
                ui.label(
                    RichText::new("kg/m³")
                        .size(font_size::SMALL)
                        .color(muted(p)),
                );
            }
            ui.add_space(space::MD);
            ui.label(RichText::new(tr("Moments about")).color(muted(p)));
            for c in &mut form.centre {
                ui.add(egui::TextEdit::singleline(c).desired_width(50.0));
            }
        });
        ui.add_space(space::SM);
        ask = accent_button(ui, p, tr("Calculate")).clicked();
        match &results.forces {
            Slot::Idle => {}
            Slot::Working(_) => wait(ui, tr("Adding up the faces…")),
            Slot::Done(Err(message)) => {
                ui.colored_label(for_egui::color(p.status_danger_fg), message.as_str());
            }
            Slot::Done(Ok(report)) => {
                ui.add_space(space::SM);
                egui::Grid::new("forces-metrics")
                    .num_columns(3)
                    .spacing([space::XL, space::XS])
                    .striped(true)
                    .show(ui, |ui| {
                        for m in report.metrics() {
                            ui.label(m.title);
                            ui.label(RichText::new(plain(m.value)).monospace());
                            ui.label(RichText::new(m.unit).color(muted(p)));
                            ui.end_row();
                        }
                    });
                ui.add_space(space::SM);
                ui.horizontal(|ui| {
                    for (format, label) in [("json", "JSON"), ("csv", "CSV"), ("html", "HTML")] {
                        if ui.button(trf("Save {}", &[&label])).clicked() {
                            save = Some(format);
                        }
                    }
                    if let Some((ok, message)) = &results.saved_forces {
                        let tone = if *ok {
                            p.status_success_fg
                        } else {
                            p.status_danger_fg
                        };
                        ui.colored_label(for_egui::color(tone), message.as_str());
                    }
                });
                if report.per_unit_density {
                    ui.label(
                        RichText::new(
                            "An incompressible solver's forces are per unit density: give a \
                             density to have them in newtons.",
                        )
                        .size(font_size::SMALL)
                        .color(muted(p)),
                    );
                }
            }
        }
    });
    if ask {
        let Some(time) = results.time.clone() else {
            return;
        };
        results.saved_forces = None;
        results.forces = match results.forces_form.parse() {
            Err(message) => Slot::Done(Err(message)),
            Ok((patches, reference)) => {
                let case = Arc::clone(case);
                spawn(ui.ctx(), move || {
                    let mut report = case
                        .forces(&time, &patches, &reference)
                        .map_err(|e| e.to_string())?;
                    report.case = case
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    report.label = trf("Forces on {}", &[&patches.join(", ")]);
                    Ok(report)
                })
            }
        };
    }
    if let Some(format) = save
        && let Slot::Done(Ok(report)) = &results.forces
    {
        results.saved_forces = Some(save_forces(case.path(), report, format));
    }
}

/// Writes the report to `postProcessing/forces/<time>/` in the case.
pub(super) fn save_forces(
    dir: &std::path::Path,
    report: &ForceReport,
    format: &str,
) -> (bool, String) {
    let (body, name) = match format {
        "json" => (report.json(), "forces.json"),
        "csv" => (report.csv(), "forces.csv"),
        "html" => (report.html(), "forces.html"),
        other => {
            return (
                false,
                format!("The report is json, csv or html, not '{other}'."),
            );
        }
    };
    write_report(dir, "forces", &report.time, name, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walls() -> Vec<WallReport> {
        ["plate", "top"]
            .iter()
            .map(|name| WallReport {
                patch: (*name).to_owned(),
                kinematic: true,
                area: 1.0,
                shear: Vec::new(),
                y_plus: Vec::new(),
                heat_flux: None,
                areas: Vec::new(),
                viscous_force: Vec3::default(),
                pressure_force: None,
            })
            .collect()
    }

    #[test]
    fn every_wall_is_counted_to_begin_with_and_a_gas_must_give_a_density() {
        let mut form = ForcesForm::default();
        form.with_walls(&walls(), false);
        let (patches, reference) = form.parse().unwrap();
        assert_eq!(patches, ["plate", "top"]);
        assert_eq!(reference.density, None);
        assert_eq!((reference.drag, reference.lift), (axis(0), axis(1)));
        form.with_walls(&walls(), true);
        assert_eq!(form.parse().unwrap().1.density, Some(1.225));
    }

    #[test]
    fn what_cannot_be_asked_is_said_in_words() {
        let mut form = ForcesForm::default();
        form.with_walls(&walls(), false);
        form.walls.iter_mut().for_each(|(_, on)| *on = false);
        assert_eq!(form.parse().unwrap_err(), "Choose at least one wall.");
        form.walls.iter_mut().for_each(|(_, on)| *on = true);
        form.lift = form.drag;
        assert!(form.parse().unwrap_err().contains("same axis"));
        form.lift = 2;
        form.speed = "fast".into();
        assert_eq!(
            form.parse().unwrap_err(),
            "The reference speed needs a number."
        );
    }

    #[test]
    fn a_report_is_saved_beside_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let report = ForceReport {
            label: "Forces on plate".into(),
            case: "plate".into(),
            time: "100".into(),
            patches: vec!["plate".into()],
            reference: ForceReference::default(),
            per_unit_density: true,
            area: 1.0,
            pressure: Vec3::default(),
            viscous: Vec3::new(1.0, 0.0, 0.0),
            pressure_moment: Vec3::default(),
            viscous_moment: Vec3::default(),
        };
        for (format, name) in [
            ("json", "forces.json"),
            ("csv", "forces.csv"),
            ("html", "forces.html"),
        ] {
            let (ok, message) = save_forces(dir.path(), &report, format);
            assert!(ok, "{message}");
            assert_eq!(
                message,
                format!("Saved to postProcessing/forces/100/{name}.")
            );
        }
        let (ok, message) = save_forces(dir.path(), &report, "pdf");
        assert!(!ok && message.contains("json, csv or html"));
        let escaping = ForceReport {
            time: "../x".into(),
            ..report
        };
        assert!(!save_forces(dir.path(), &escaping, "json").0);
    }
}
