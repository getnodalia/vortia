//! The case report of the Results tab: what the Results tab shows about one time and field, the
//! patches, the walls and the forces when they were worked out, written as one Markdown document
//! that can be kept with the case or pasted into a report.

use crate::i18n::tr;
use vortia_post::{FieldReport, ForceReport, WallReport};

use super::{plain, shown};

/// A table cell: a name from a file can hold a bar or a line break that would break the table.
fn cell(text: &str) -> String {
    text.replace(['\n', '\r'], " ").replace('|', "\\|")
}

fn vector(v: vortia_core::Vec3) -> String {
    shown(&[v.x, v.y, v.z])
}

fn row(cells: &[String]) -> String {
    format!("| {} |\n", cells.join(" | "))
}

fn rule(columns: usize) -> String {
    row(&vec!["---".to_owned(); columns])
}

/// The report of `case` (the folder's name) as Markdown.
pub(super) fn markdown(
    case: &str,
    report: &FieldReport,
    walls: &[WallReport],
    forces: Option<&ForceReport>,
) -> String {
    let mut out = format!("# {}: {}\n\n", tr("Case report"), cell(case));
    out.push_str(&format!(
        "{}: {} · {}: {}\n\n",
        tr("Time"),
        cell(&report.time),
        tr("Field"),
        cell(&report.field)
    ));

    let v = &report.volume;
    out.push_str(&format!("## {}\n\n", report.field));
    for (name, value) in [
        (tr("Cells"), v.cells.to_string()),
        (tr("Volume"), plain(v.volume)),
        (tr("Average"), shown(&v.average)),
        (tr("Minimum"), shown(&v.min)),
        (tr("Maximum"), shown(&v.max)),
        (tr("Integral"), shown(&v.integral)),
    ] {
        out.push_str(&format!("- {name}: {value}\n"));
    }

    out.push_str(&format!("\n## {}\n\n", tr("Patches")));
    let heads: Vec<String> = [
        tr("Patch"),
        tr("Type"),
        tr("Area"),
        tr("Average"),
        tr("Minimum"),
        tr("Maximum"),
        tr("Flux out"),
    ]
    .iter()
    .map(|h| (*h).to_owned())
    .collect();
    out.push_str(&row(&heads));
    out.push_str(&rule(heads.len()));
    for patch in &report.patches {
        let s = &patch.stats;
        out.push_str(&row(&[
            cell(&patch.name),
            cell(&patch.kind),
            plain(s.area),
            shown(&s.average),
            shown(&s.min),
            shown(&s.max),
            patch.flux.map_or_else(|| "–".to_owned(), plain),
        ]));
    }
    let fluxes: Vec<f64> = report.patches.iter().filter_map(|p| p.flux).collect();
    if !fluxes.is_empty() {
        out.push_str(&format!(
            "\n{}: {}\n",
            tr("Flux out of all patches"),
            plain(fluxes.iter().sum())
        ));
    }

    if !walls.is_empty() {
        out.push_str(&format!("\n## {}\n\n", tr("Walls")));
        let heads: Vec<String> = [
            tr("Patch"),
            tr("Mean shear"),
            tr("Viscous"),
            tr("Pressure"),
            tr("y+ (min, mean, max)"),
            tr("Heat in"),
        ]
        .iter()
        .map(|h| (*h).to_owned())
        .collect();
        out.push_str(&row(&heads));
        out.push_str(&rule(heads.len()));
        for w in walls {
            out.push_str(&row(&[
                cell(&w.patch),
                vector(w.mean_shear()),
                vector(w.viscous_force),
                w.pressure_force.map_or_else(|| "–".to_owned(), vector),
                w.y_plus_range().map_or_else(
                    || "–".to_owned(),
                    |(lo, mean, hi)| format!("{} / {} / {}", plain(lo), plain(mean), plain(hi)),
                ),
                w.heat_rate().map_or_else(
                    || "–".to_owned(),
                    |q| format!("{} {}", plain(q), if w.kinematic { "K m³/s" } else { "W" }),
                ),
            ]));
        }
        let note = if walls.iter().any(|w| w.kinematic) {
            tr("Per unit density (m²/s², m³/s²): the solver is incompressible.")
        } else {
            tr(tr(
                "Stress in Pa, force in N. Shear is the wall's pull on the fluid, so it is negative along the flow.",
            ))
        };
        out.push_str(&format!("\n{note}\n"));
    }

    if let Some(f) = forces {
        out.push_str(&format!("\n## {}\n\n", tr("Forces")));
        out.push_str(&format!(
            "- {}: {}\n",
            tr("Walls"),
            cell(&f.patches.join(", "))
        ));
        out.push_str(&format!("- {}: {}\n", tr("Area"), plain(f.area)));
        for (name, value) in [
            (tr("Pressure"), f.pressure),
            (tr("Viscous"), f.viscous),
            (tr("Total"), f.total()),
        ] {
            out.push_str(&format!("- {name}: {}\n", vector(value)));
        }
        if f.per_unit_density {
            out.push_str(&format!(
                "\n{}\n",
                tr("Per unit density: give a density to have them in newtons.")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortia_post::{PatchReport, PatchStats, VolumeStats};

    fn report(patch: &str) -> FieldReport {
        FieldReport {
            time: "100".into(),
            field: "p".into(),
            volume: VolumeStats {
                cells: 40,
                volume: 2.0,
                integral: vec![8.0],
                average: vec![4.0],
                min: vec![1.0],
                max: vec![7.0],
            },
            patches: vec![PatchReport {
                name: patch.into(),
                kind: "wall".into(),
                stats: PatchStats {
                    faces: 4,
                    area: 0.5,
                    integral: vec![1.0],
                    average: vec![2.0],
                    min: vec![1.5],
                    max: vec![2.5],
                },
                flux: Some(0.0),
            }],
        }
    }

    #[test]
    fn the_report_names_the_case_the_time_and_the_patches() {
        let text = markdown("nozzle", &report("inlet"), &[], None);
        assert!(text.starts_with("# "), "{text}");
        assert!(text.contains("nozzle"));
        assert!(text.contains("100"));
        assert!(text.contains("| inlet | wall |"), "{text}");
        assert!(!text.contains("Walls"), "no walls were worked out: {text}");
    }

    #[test]
    fn a_name_cannot_break_the_table() {
        let text = markdown("a\nb", &report("in|let\nx"), &[], None);
        assert!(text.contains("in\\|let x"), "{text}");
        assert!(!text.contains("a\nb"));
    }

    #[test]
    fn every_table_row_has_as_many_columns_as_its_head() {
        let text = markdown("c", &report("out"), &[], None);
        let rows: Vec<&str> = text.lines().filter(|l| l.starts_with('|')).collect();
        assert_eq!(rows.len(), 3);
        let columns = |l: &str| l.matches(" | ").count();
        assert!(
            rows.iter().all(|r| columns(r) == columns(rows[0])),
            "{rows:?}"
        );
    }
}
