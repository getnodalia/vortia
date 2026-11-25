//! The Section card of the Results tab: what goes through a plane across the domain (mass and
//! volume flow, momentum flux, pressure load, power, coefficients), worked out by
//! `vortia-post` and saved as a report in JSON, CSV or HTML.

use crate::i18n::{tr, trf};
use std::path::Path;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use nodalia_theme::{Palette, font_size, for_egui, space};
use vortia_core::Aabb;
use vortia_post::{Case, Density, SectionOptions, SectionReport};

use super::{Results, Slot, card, plain, spawn, wait};
use crate::app::{accent_button, muted};

/// The section as typed.
#[derive(Clone)]
pub(super) struct SectionForm {
    axis: usize,
    position: String,
    reverse: bool,
    /// Density from the `rho` field (compressible), or one value (incompressible).
    from_field: bool,
    density: String,
    kinematic: bool,
    reference_pressure: String,
    reference_density: String,
    reference_speed: String,
    reference_area: String,
    wedge: String,
}

impl Default for SectionForm {
    fn default() -> Self {
        let defaults = SectionOptions::default();
        Self {
            axis: 0,
            position: "0".into(),
            reverse: false,
            from_field: true,
            density: "1".into(),
            kinematic: false,
            reference_pressure: plain(defaults.reference_pressure),
            reference_density: plain(defaults.reference_density),
            reference_speed: plain(defaults.reference_speed),
            reference_area: plain(defaults.reference_area),
            wedge: String::new(),
        }
    }
}

impl SectionForm {
    /// Across the longest side of the mesh, through its middle; the density as the run has it.
    pub(super) fn across(bounds: &Aabb, has_rho: bool) -> Self {
        let size = bounds.size();
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        let centre = bounds.center();
        let position = match axis {
            0 => centre.x,
            1 => centre.y,
            _ => centre.z,
        };
        Self {
            axis,
            position: plain(position),
            from_field: has_rho,
            // No `rho`: the solver is incompressible and its pressure is p/ρ.
            kinematic: !has_rho,
            ..Self::default()
        }
    }

    fn parse(&self) -> Result<SectionOptions, String> {
        let number = |text: &str, what: &str| {
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| trf("{} needs a number.", &[&what]))
        };
        let density = if self.from_field {
            Density::Field
        } else {
            Density::Constant(number(&self.density, tr("The density"))?)
        };
        let wedge = match self.wedge.trim() {
            "" => None,
            text => Some(number(text, tr("The wedge angle"))?),
        };
        Ok(SectionOptions {
            axis: self.axis,
            direction: if self.reverse { -1.0 } else { 1.0 },
            position: number(&self.position, tr("The position"))?,
            density,
            kinematic_pressure: self.kinematic,
            reference_pressure: number(&self.reference_pressure, tr("The reference pressure"))?,
            reference_density: number(&self.reference_density, tr("The reference density"))?,
            reference_speed: number(&self.reference_speed, tr("The reference speed"))?,
            reference_area: number(&self.reference_area, tr("The reference area"))?,
            wedge_angle: wedge,
        })
    }
}

pub(super) fn field(ui: &mut egui::Ui, p: &Palette, name: &str, unit: &str, text: &mut String) {
    ui.label(RichText::new(name).color(muted(p)));
    ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
    ui.label(RichText::new(unit).size(font_size::SMALL).color(muted(p)));
}

pub(super) fn section_card(
    ui: &mut egui::Ui,
    p: &Palette,
    case: &Arc<Case>,
    results: &mut Results,
) {
    let (mut ask, mut save) = (false, None::<&'static str>);
    card(ui, p, |ui| {
        ui.label(RichText::new(tr("Section")).strong());
        ui.label(
            RichText::new(tr("What goes through a plane across the domain."))
                .size(font_size::SMALL)
                .color(muted(p)),
        );
        ui.add_space(space::SM);
        let form = &mut results.section_form;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Perpendicular to")).color(muted(p)));
            for (i, name) in ["x", "y", "z"].iter().enumerate() {
                ui.selectable_value(&mut form.axis, i, *name);
            }
            ui.add_space(space::MD);
            ui.label(RichText::new("at").color(muted(p)));
            ui.add(egui::TextEdit::singleline(&mut form.position).desired_width(90.0));
            ui.checkbox(&mut form.reverse, tr("positive against the axis"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Density")).color(muted(p)));
            ui.selectable_value(&mut form.from_field, true, tr("from rho"));
            ui.selectable_value(&mut form.from_field, false, "constant");
            if !form.from_field {
                ui.add(egui::TextEdit::singleline(&mut form.density).desired_width(70.0));
                ui.label(
                    RichText::new("kg/m³")
                        .size(font_size::SMALL)
                        .color(muted(p)),
                );
            }
            ui.checkbox(&mut form.kinematic, "pressure is p/ρ");
        });
        egui::CollapsingHeader::new(RichText::new(tr("Reference values")).color(muted(p)))
            .id_salt("section-reference")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    field(ui, p, tr("Pressure"), "Pa", &mut form.reference_pressure);
                    field(ui, p, tr("Density"), "kg/m³", &mut form.reference_density);
                    field(ui, p, tr("Speed"), "m/s", &mut form.reference_speed);
                    field(ui, p, tr("Area"), "m²", &mut form.reference_area);
                    field(ui, p, tr("Wedge angle"), "° (≤ 5, x axis)", &mut form.wedge);
                });
            });
        ui.add_space(space::SM);
        ask = accent_button(ui, p, tr("Analyse")).clicked();
        match &results.section {
            Slot::Idle => {}
            Slot::Working(_) => wait(ui, tr("Cutting the cells…")),
            Slot::Done(Err(message)) => {
                ui.colored_label(for_egui::color(p.status_danger_fg), message.as_str());
            }
            Slot::Done(Ok(report)) => {
                ui.add_space(space::SM);
                egui::Grid::new("section-metrics")
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
                    if let Some((ok, message)) = &results.saved {
                        let tone = if *ok {
                            p.status_success_fg
                        } else {
                            p.status_danger_fg
                        };
                        ui.colored_label(for_egui::color(tone), message.as_str());
                    }
                });
                ui.label(
                    RichText::new(
                        "The resultant is the momentum flux plus the pressure term through the \
                         section, not an engine thrust or a complete force on a body.",
                    )
                    .size(font_size::SMALL)
                    .color(muted(p)),
                );
            }
        }
    });
    if ask {
        let Some(time) = results.time.clone() else {
            return;
        };
        results.saved = None;
        results.section = match results.section_form.parse() {
            Err(message) => Slot::Done(Err(message)),
            Ok(options) => {
                let case = Arc::clone(case);
                let label = trf(
                    "Section at {} {}",
                    &[
                        &["x", "y", "z"].get(options.axis).copied().unwrap_or("x"),
                        &plain(options.position),
                    ],
                );
                spawn(ui.ctx(), move || {
                    let analysis = case.section(&time, &options).map_err(|e| e.to_string())?;
                    let name = case
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    Ok(SectionReport {
                        label,
                        case: name,
                        time,
                        options,
                        analysis,
                    })
                })
            }
        };
    }
    if let Some(format) = save
        && let Slot::Done(Ok(report)) = &results.section
    {
        results.saved = Some(save_report(case.path(), report, format));
    }
}

/// Writes the report to `postProcessing/section/<time>/` in the case, as the other results are.
pub(super) fn save_report(dir: &Path, report: &SectionReport, format: &str) -> (bool, String) {
    let (body, name) = match format {
        "json" => (report.json(), "section.json"),
        "csv" => (report.csv(), "section.csv"),
        "html" => (report.html(), "section.html"),
        other => {
            return (
                false,
                trf("The report is json, csv or html, not '{}'.", &[&other]),
            );
        }
    };
    write_report(dir, "section", &report.time, name, &body)
}

/// Writes `body` to `postProcessing/<kind>/<time>/<name>` in the case. A time that is not a
/// folder name is refused, so a report cannot be written outside the case.
pub(super) fn write_report(
    dir: &Path,
    kind: &str,
    time: &str,
    name: &str,
    body: &str,
) -> (bool, String) {
    if time.contains(['/', '\\']) || time == ".." || time.is_empty() {
        return (false, tr("The time is not a folder name.").into());
    }
    let relative = Path::new("postProcessing").join(kind).join(time).join(name);
    match vortia_foamfile::write::atomic(&dir.join(&relative), body.as_bytes()) {
        Ok(()) => (true, trf("Saved to {}.", &[&relative.display()])),
        Err(e) => (false, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortia_core::Vec3;

    fn bounds() -> Aabb {
        Aabb {
            min: Vec3::default(),
            max: Vec3::new(4.0, 2.0, 1.0),
        }
    }

    #[test]
    fn the_form_starts_across_the_longest_side_with_the_density_the_run_has() {
        let compressible = SectionForm::across(&bounds(), true).parse().unwrap();
        assert_eq!((compressible.axis, compressible.position), (0, 2.0));
        assert_eq!(compressible.density, Density::Field);
        assert!(!compressible.kinematic_pressure);
        // A solver that writes no rho is incompressible: p is p/ρ and the density is a number.
        let mut form = SectionForm::across(&bounds(), false);
        assert!(form.kinematic && !form.from_field);
        assert_eq!(form.parse().unwrap().density, Density::Constant(1.0));
        form.reverse = true;
        form.wedge = "2.5".into();
        let o = form.parse().unwrap();
        assert_eq!((o.direction, o.wedge_angle), (-1.0, Some(2.5)));
    }

    #[test]
    fn what_is_typed_that_is_not_a_number_is_said() {
        let form = SectionForm {
            position: "here".into(),
            ..SectionForm::default()
        };
        assert_eq!(form.parse().unwrap_err(), "The position needs a number.");
        let form = SectionForm {
            position: "1".into(),
            reference_speed: "inf".into(),
            ..SectionForm::default()
        };
        assert!(form.parse().unwrap_err().contains("reference speed"));
    }

    #[test]
    fn a_report_is_saved_where_the_case_keeps_such_things() {
        let dir = tempfile::tempdir().unwrap();
        let report = SectionReport {
            label: "Throat".into(),
            case: "box".into(),
            time: "12".into(),
            options: SectionOptions::default(),
            analysis: vortia_post::SectionAnalysis {
                area: 2.0,
                mass_flow: 6.0,
                forward_mass_flow: 6.0,
                reverse_mass_flow: 0.0,
                volume_flow: 4.0,
                mean_pressure: 1.0,
                mean_density: 1.5,
                mean_normal_velocity: 2.0,
                momentum_flux: 12.0,
                pressure_load: 0.0,
                resultant: 12.0,
                kinetic_power: 12.0,
                coefficient: 1.0,
                mean_cp: 1.0,
                cells: 2,
                shared_faces: 0,
                area_scale: 1.0,
            },
        };
        for (format, name) in [
            ("json", "section.json"),
            ("csv", "section.csv"),
            ("html", "section.html"),
        ] {
            let (ok, message) = save_report(dir.path(), &report, format);
            assert!(ok, "{message}");
            assert_eq!(
                message,
                format!("Saved to postProcessing/section/12/{name}.")
            );
            assert!(
                dir.path()
                    .join("postProcessing/section/12")
                    .join(name)
                    .is_file()
            );
        }
        let (ok, message) = save_report(dir.path(), &report, "pdf");
        assert!(!ok && message.contains("json, csv or html"));
        // A time that tries to leave the case is not a place to write.
        let escaping = SectionReport {
            time: "../../outside".into(),
            ..report
        };
        let (ok, message) = save_report(dir.path(), &escaping, "json");
        assert!(!ok && message.contains("folder name"), "{message}");
        assert!(!dir.path().join("outside").exists());
    }
}
