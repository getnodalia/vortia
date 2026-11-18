//! The Compare card of the Results tab: one number of the reports saved by the Section and Forces
//! cards (or by `vortia section` and `vortia forces`) side by side, against the one chosen as the
//! baseline. The work is `vortia_post::compare`, the same as `vortia compare`.

use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use nodalia_theme::{Palette, font_size, for_egui, space};
use vortia_post::{MAX_REPORT_BYTES, Row, Sealed, compare, comparison_csv};

use super::analysis::write_report;
use super::{Results, card, plain};
use crate::app::{accent_button, muted};

/// The most reports the card lists, so a folder of thousands is not read into memory.
const MAX_FOUND: usize = 200;

/// A report found in the case or added by hand.
#[derive(Clone)]
struct Found {
    /// Where it is, as shown: relative to the case when it is in it.
    shown: String,
    report: Sealed,
    on: bool,
}

/// What the card holds.
#[derive(Default)]
pub(super) struct CompareForm {
    found: Vec<Found>,
    scanned: bool,
    /// Files that were looked at and are not reports this build can read.
    skipped: usize,
    baseline: usize,
    metric: String,
    /// A report file to add, as typed.
    extra: String,
    problem: Option<String>,
    saved: Option<(bool, String)>,
}

/// The reports under `postProcessing/section` and `postProcessing/forces`, and how many files
/// were left out.
fn scan(dir: &Path) -> (Vec<Found>, usize) {
    let mut found = Vec::new();
    let mut skipped = 0;
    for kind in ["section", "forces"] {
        let Ok(times) = std::fs::read_dir(dir.join("postProcessing").join(kind)) else {
            continue;
        };
        let mut times: Vec<PathBuf> = times.flatten().map(|e| e.path()).collect();
        times.sort();
        for time in times {
            let Ok(files) = std::fs::read_dir(&time) else {
                continue;
            };
            let mut files: Vec<PathBuf> = files
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect();
            files.sort();
            for file in files {
                if found.len() >= MAX_FOUND {
                    return (found, skipped);
                }
                match read_report(&file) {
                    Ok(report) => found.push(Found {
                        shown: file
                            .strip_prefix(dir)
                            .unwrap_or(&file)
                            .display()
                            .to_string(),
                        report,
                        on: false,
                    }),
                    Err(_) => skipped += 1,
                }
            }
        }
    }
    (found, skipped)
}

/// A saved report, whole or why not.
fn read_report(path: &Path) -> Result<Sealed, String> {
    let shown = path.display();
    let meta = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if !meta.is_file() || meta.len() > MAX_REPORT_BYTES as u64 {
        return Err(format!("{shown} is not a report this build can read."));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    Sealed::open(&text).map_err(|e| format!("{shown}: {e}"))
}

impl CompareForm {
    /// Lists the reports of the case, keeping the choices already made for those still there.
    fn rescan(&mut self, dir: &Path) {
        let (mut found, skipped) = scan(dir);
        for f in &mut found {
            f.on = self
                .found
                .iter()
                .any(|old| old.on && old.report.sha256 == f.report.sha256);
        }
        // Reports added by hand from elsewhere stay.
        for old in &self.found {
            if !found.iter().any(|f| f.report.sha256 == old.report.sha256)
                && Path::new(&old.shown).is_absolute()
            {
                found.push(old.clone());
            }
        }
        let baseline = self
            .found
            .get(self.baseline)
            .map(|f| f.report.sha256.clone());
        self.baseline = baseline
            .and_then(|sha| found.iter().position(|f| f.report.sha256 == sha))
            .unwrap_or(0);
        self.found = found;
        self.skipped = skipped;
        self.scanned = true;
        self.fit_metric();
    }

    /// Adds the report file typed, whole or with the reason it was refused.
    fn add(&mut self) {
        let path = PathBuf::from(self.extra.trim());
        match read_report(&path) {
            Ok(report) => {
                if self.found.iter().any(|f| f.report.sha256 == report.sha256) {
                    self.problem = Some(tr("That report is already in the list.").into());
                } else if self.found.len() >= MAX_FOUND {
                    self.problem = Some(tr("There are too many reports in the list.").into());
                } else {
                    self.found.push(Found {
                        shown: path.display().to_string(),
                        report,
                        on: true,
                    });
                    self.extra.clear();
                    self.problem = None;
                }
            }
            Err(e) => self.problem = Some(e),
        }
        self.fit_metric();
    }

    /// The number compared must be one the baseline has.
    fn fit_metric(&mut self) {
        let names = self.metrics();
        if !names.contains(&self.metric) {
            self.metric = names.into_iter().next().unwrap_or_default();
        }
    }

    fn metrics(&self) -> Vec<String> {
        self.found
            .get(self.baseline)
            .map(|f| f.report.metric_names())
            .unwrap_or_default()
    }

    /// Whether the report at `i` can be set against the baseline: it is the same kind.
    fn comparable(&self, i: usize) -> bool {
        match (self.found.get(i), self.found.get(self.baseline)) {
            (Some(f), Some(b)) => f.report.schema == b.report.schema,
            _ => false,
        }
    }

    /// The baseline and the reports ticked that are of its kind, compared on the number chosen.
    fn rows(&self) -> Result<Vec<Row>, String> {
        let baseline = self
            .found
            .get(self.baseline)
            .ok_or_else(|| tr("There are no reports to compare.").to_owned())?;
        let mut reports = vec![baseline.report.clone()];
        reports.extend(
            self.found
                .iter()
                .enumerate()
                .filter(|(i, f)| f.on && *i != self.baseline && self.comparable(*i))
                .map(|(_, f)| f.report.clone()),
        );
        if reports.len() < 2 {
            return Err(tr("Tick at least one more report of the same kind.").into());
        }
        compare(&reports, &self.metric, 0).map_err(|e| e.to_string())
    }

    fn csv(&self, rows: &[Row]) -> String {
        let sha = self
            .found
            .get(self.baseline)
            .map(|f| f.report.sha256.as_str())
            .unwrap_or_default();
        comparison_csv(rows, &self.metric, sha)
    }
}

pub(super) fn compare_card(ui: &mut egui::Ui, p: &Palette, dir: &Path, results: &mut Results) {
    let form = &mut results.compare_form;
    if !form.scanned {
        form.rescan(dir);
    }
    let (mut rescan, mut add, mut save) = (false, false, false);
    card(ui, p, |ui| {
        ui.label(RichText::new(tr("Compare")).strong());
        ui.label(
            RichText::new(tr(
                "One number of saved reports side by side, against the baseline.",
            ))
            .size(font_size::SMALL)
            .color(muted(p)),
        );
        ui.add_space(space::SM);
        if form.found.is_empty() {
            ui.label(
                RichText::new(tr(
                    "Save a Section or Forces report, or add a report file, to compare runs.",
                ))
                .color(muted(p)),
            );
        }
        let mut baseline = form.baseline;
        egui::Grid::new("compare-reports")
            .num_columns(4)
            .spacing([space::MD, space::XS])
            .striped(true)
            .show(ui, |ui| {
                if !form.found.is_empty() {
                    ui.label(RichText::new(tr("Baseline")).color(muted(p)));
                    ui.label(RichText::new(tr("Compare")).color(muted(p)));
                    ui.label(RichText::new(tr("Report")).color(muted(p)));
                    ui.label(RichText::new(tr("File")).color(muted(p)));
                    ui.end_row();
                }
                let kinds: Vec<bool> = (0..form.found.len()).map(|i| form.comparable(i)).collect();
                for (i, f) in form.found.iter_mut().enumerate() {
                    ui.radio_value(&mut baseline, i, "");
                    let same = kinds.get(i).copied().unwrap_or(false);
                    ui.add_enabled(
                        same && i != form.baseline,
                        egui::Checkbox::without_text(&mut f.on),
                    );
                    let label = f.report.text("label");
                    ui.label(trf(
                        "{} at {}",
                        &[
                            &if label.is_empty() {
                                f.report.schema.as_str()
                            } else {
                                label
                            },
                            &f.report.text("time"),
                        ],
                    ));
                    ui.label(
                        RichText::new(f.shown.as_str())
                            .size(font_size::SMALL)
                            .color(muted(p)),
                    );
                    ui.end_row();
                }
            });
        if baseline != form.baseline {
            form.baseline = baseline;
            form.fit_metric();
        }
        ui.add_space(space::SM);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Number")).color(muted(p)));
            let names = form.metrics();
            egui::ComboBox::from_id_salt("compare-metric")
                .selected_text(form.metric.as_str())
                .show_ui(ui, |ui| {
                    for name in names {
                        ui.selectable_value(&mut form.metric, name.clone(), name);
                    }
                });
            if ui.button(tr("Look again")).clicked() {
                rescan = true;
            }
            if form.skipped > 0 {
                ui.label(
                    RichText::new(trf("{} files were not reports.", &[&form.skipped]))
                        .size(font_size::SMALL)
                        .color(muted(p)),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut form.extra)
                    .hint_text(tr("A report file from another run"))
                    .desired_width(320.0),
            );
            if ui.button(tr("Add")).clicked() {
                add = true;
            }
        });
        if let Some(problem) = &form.problem {
            ui.colored_label(for_egui::color(p.status_danger_fg), problem.as_str());
        }
        if form.found.is_empty() {
            return;
        }
        ui.add_space(space::SM);
        match form.rows() {
            Err(message) => {
                ui.label(RichText::new(message).color(muted(p)));
            }
            Ok(rows) => {
                egui::Grid::new("compare-rows")
                    .num_columns(3)
                    .spacing([space::XL, space::XS])
                    .striped(true)
                    .show(ui, |ui| {
                        for r in &rows {
                            ui.label(trf("{} at {}", &[&r.label, &r.time]));
                            ui.label(RichText::new(plain(r.value)).monospace());
                            let change = r
                                .change_percent
                                .map_or_else(|| "—".to_owned(), |c| format!("{c:+.3} %"));
                            ui.label(RichText::new(change).monospace());
                            ui.end_row();
                            if r.warning.is_some() {
                                ui.label("");
                                ui.colored_label(
                                    for_egui::color(p.status_warning_fg),
                                    tr(
                                        "Different section or reference definitions: compare the assumptions before interpreting a change.",
                                    ),
                                );
                                ui.end_row();
                            }
                        }
                    });
                ui.add_space(space::SM);
                ui.horizontal(|ui| {
                    if accent_button(ui, p, tr("Save CSV")).clicked() {
                        save = true;
                    }
                    if let Some((ok, message)) = &form.saved {
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
    let form = &mut results.compare_form;
    if rescan {
        form.rescan(dir);
    }
    if add {
        form.add();
    }
    if save {
        form.saved = Some(match form.rows() {
            Ok(rows) => write_report(
                dir,
                "comparison",
                &form.metric,
                "comparison.csv",
                &form.csv(&rows),
            ),
            Err(message) => (false, message),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortia_post::seal;

    fn report(dir: &Path, kind: &str, time: &str, drag: f64, options: &str) {
        let payload = serde_json::json!({
            "label": format!("Forces at {time}"),
            "case": "plate",
            "time": time,
            "options": options,
            "metrics": { "drag": drag, "lift": 0.0 },
        });
        let text = seal(&format!("vortia.{kind}"), payload);
        let folder = dir.join("postProcessing").join(kind).join(time);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{kind}.json")), text).unwrap();
    }

    #[test]
    fn the_reports_of_the_case_are_listed_and_what_is_not_one_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        report(dir.path(), "forces", "100", 2.0, "a");
        report(dir.path(), "forces", "200", 2.2, "a");
        let broken = dir.path().join("postProcessing/forces/300");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("forces.json"), "{ not a report").unwrap();
        let (found, skipped) = scan(dir.path());
        assert_eq!(found.len(), 2);
        assert_eq!(skipped, 1);
        assert_eq!(found[0].shown, "postProcessing/forces/100/forces.json");
        assert_eq!(found[0].report.metric_names(), ["drag", "lift"]);
    }

    #[test]
    fn a_report_is_compared_with_the_baseline_and_a_changed_definition_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        report(dir.path(), "forces", "100", 2.0, "a");
        report(dir.path(), "forces", "200", 2.2, "a");
        report(dir.path(), "forces", "300", 1.0, "b");
        let mut form = CompareForm::default();
        form.rescan(dir.path());
        assert_eq!(form.metric, "drag");
        // Nothing is ticked yet.
        assert!(form.rows().is_err());
        for f in &mut form.found {
            f.on = true;
        }
        let rows = form.rows().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].change_percent, Some(0.0));
        assert!((rows[1].change_percent.unwrap() - 10.0).abs() < 1e-9);
        assert!(rows[1].warning.is_none() && rows[2].warning.is_some());
        let csv = form.csv(&rows);
        assert_eq!(csv.lines().count(), 4);
        // A choice survives looking again.
        form.rescan(dir.path());
        assert!(form.found.iter().all(|f| f.on));
    }

    #[test]
    fn a_report_of_another_kind_is_left_out_and_a_file_that_is_not_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        report(dir.path(), "forces", "100", 2.0, "a");
        report(dir.path(), "section", "100", 5.0, "a");
        let mut form = CompareForm::default();
        form.rescan(dir.path());
        let forces = form
            .found
            .iter()
            .position(|f| f.report.schema == "vortia.forces")
            .unwrap();
        form.baseline = forces;
        form.found.iter_mut().for_each(|f| f.on = true);
        let other = 1 - forces;
        assert!(!form.comparable(other));
        assert!(form.rows().is_err(), "the other kind is not counted");
        form.extra = dir.path().join("nothing.json").display().to_string();
        form.add();
        assert!(form.problem.is_some());
        let before = form.found.len();
        form.extra = dir
            .path()
            .join("postProcessing/forces/100/forces.json")
            .display()
            .to_string();
        form.add();
        assert_eq!(form.found.len(), before, "it was in the list already");
    }

    #[test]
    fn the_comparison_is_saved_in_the_case() {
        let dir = tempfile::tempdir().unwrap();
        report(dir.path(), "forces", "100", 2.0, "a");
        report(dir.path(), "forces", "200", 3.0, "a");
        let mut form = CompareForm::default();
        form.rescan(dir.path());
        form.found.iter_mut().for_each(|f| f.on = true);
        let rows = form.rows().unwrap();
        let (ok, message) = write_report(
            dir.path(),
            "comparison",
            &form.metric,
            "comparison.csv",
            &form.csv(&rows),
        );
        assert!(ok, "{message}");
        assert_eq!(
            message,
            "Saved to postProcessing/comparison/drag/comparison.csv."
        );
        assert!(!write_report(dir.path(), "comparison", "../x", "c.csv", "").0);
    }
}
