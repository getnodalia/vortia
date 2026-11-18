//! Putting reports of several runs side by side: one number from each, and how far it is from the
//! baseline's. Reports made with different reference values or sections are still compared, with
//! a warning on the row, because the change they show may be the definition's and not the flow's.

use crate::error::{PostError, Result};
use crate::report::csv_text;
use crate::seal::Sealed;

/// One report in a comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub case: String,
    pub time: String,
    pub sha256: String,
    pub value: f64,
    /// `100 (value − baseline) / |baseline|`; `None` when the baseline is zero.
    pub change_percent: Option<f64>,
    /// Set when this report's definitions differ from the baseline's.
    pub warning: Option<&'static str>,
}

/// The warning on a row whose definitions differ from the baseline's.
pub const DEFINITIONS_DIFFER: &str = "Different section or reference definitions: compare the assumptions before interpreting a change.";

/// `metric` of each of `reports` against the one at `baseline`. Every report must be of the same
/// kind and have the metric.
pub fn compare(reports: &[Sealed], metric: &str, baseline: usize) -> Result<Vec<Row>> {
    let first = reports
        .get(baseline)
        .ok_or_else(|| PostError::Invalid(format!("there is no report number {baseline}")))?;
    let reference = first
        .metric(metric)
        .ok_or_else(|| PostError::Invalid(format!("the baseline has no number '{metric}'")))?;
    reports
        .iter()
        .map(|r| {
            if r.schema != first.schema {
                return Err(PostError::Invalid(format!(
                    "{} is not the kind of report the baseline is ({})",
                    r.schema, first.schema
                )));
            }
            let value = r
                .metric(metric)
                .ok_or_else(|| PostError::Invalid(format!("a report has no number '{metric}'")))?;
            Ok(Row {
                label: r.text("label").to_owned(),
                case: r.text("case").to_owned(),
                time: r.text("time").to_owned(),
                sha256: r.sha256.clone(),
                value,
                change_percent: (reference != 0.0)
                    .then(|| 100.0 * (value - reference) / reference.abs()),
                warning: (r.payload.get("options") != first.payload.get("options"))
                    .then_some(DEFINITIONS_DIFFER),
            })
        })
        .collect()
}

/// The comparison as CSV, one row per report.
pub fn comparison_csv(rows: &[Row], metric: &str, baseline_sha256: &str) -> String {
    let mut out = String::from(
        "label,case,time,report_sha256,baseline_sha256,metric,value,change_percent,definition_warning\n",
    );
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            csv_text(&r.label),
            csv_text(&r.case),
            csv_text(&r.time),
            r.sha256,
            baseline_sha256,
            csv_text(metric),
            r.value,
            r.change_percent.map_or_else(String::new, |c| c.to_string()),
            csv_text(r.warning.unwrap_or_default()),
        ));
    }
    out
}
