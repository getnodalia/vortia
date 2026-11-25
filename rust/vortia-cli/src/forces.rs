//! `vortia forces` and `vortia compare`: the force and moment on walls with their coefficients,
//! and one number of several saved reports side by side.

use std::io::Write as _;
use std::process::ExitCode;

use vortia_core::Vec3;
use vortia_post::{Case, ForceReference, Sealed, compare, comparison_csv};

use crate::{shown, usage_error};

/// What went wrong: a mistake in the command line, or a run that failed.
enum Failure {
    Usage(String),
    Failed(String),
}

fn finish(result: Result<String, Failure>) -> ExitCode {
    match result {
        Ok(text) => {
            out!("{text}");
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(message)) => usage_error(&message),
        Err(Failure::Failed(message)) => {
            eprintln!("vortia: {}", shown(&message));
            ExitCode::FAILURE
        }
    }
}

fn number(word: Option<&&str>, what: &str) -> Result<f64, Failure> {
    word.and_then(|w| w.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| Failure::Usage(format!("{what} needs a number")))
}

fn triple(flags: &mut std::slice::Iter<'_, &str>, what: &str) -> Result<Vec3, Failure> {
    Ok(Vec3::new(
        number(flags.next(), what)?,
        number(flags.next(), what)?,
        number(flags.next(), what)?,
    ))
}

/// Runs `forces`; the arguments are those after the word.
pub fn forces(args: &[&str]) -> ExitCode {
    finish(make_forces(args))
}

fn make_forces(args: &[&str]) -> Result<String, Failure> {
    let [case, walls, flags @ ..] = args else {
        return Err(Failure::Usage(
            "forces needs a case and the walls, separated by commas".into(),
        ));
    };
    let patches: Vec<String> = walls.split(',').map(str::to_owned).collect();
    let mut reference = ForceReference::default();
    let (mut time, mut report) = (None, None);
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        match *flag {
            "-time" => {
                time = Some(
                    (*flags
                        .next()
                        .ok_or_else(|| Failure::Usage("-time needs a time".into()))?)
                    .to_owned(),
                );
            }
            "-rho" => reference.density = Some(number(flags.next(), "-rho")?),
            "-uref" => reference.speed = number(flags.next(), "-uref")?,
            "-aref" => reference.area = number(flags.next(), "-aref")?,
            "-lref" => reference.length = number(flags.next(), "-lref")?,
            "-drag" => reference.drag = triple(&mut flags, "-drag")?,
            "-lift" => reference.lift = triple(&mut flags, "-lift")?,
            "-cofr" => reference.centre = triple(&mut flags, "-cofr")?,
            "-report" => {
                let (Some(format), Some(file)) = (flags.next(), flags.next()) else {
                    return Err(Failure::Usage("-report needs a format and a file".into()));
                };
                report = Some(((*format).to_owned(), (*file).to_owned()));
            }
            other => {
                return Err(Failure::Usage(format!("unknown option '{}'", shown(other))));
            }
        }
    }
    let failed = |e: String| Failure::Failed(e);
    let opened = Case::open(case).map_err(|e| failed(e.to_string()))?;
    let time = match time {
        Some(t) => t,
        None => opened.latest_time().map_err(|e| failed(e.to_string()))?,
    };
    let mut result = opened
        .forces(&time, &patches, &reference)
        .map_err(|e| failed(e.to_string()))?;
    result.case = std::path::Path::new(case)
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    result.label = format!("Forces on {}", patches.join(", "));
    let mut text = format!(
        "Time {}  walls {}\n",
        shown(&time),
        shown(&patches.join(", "))
    );
    for m in result.metrics() {
        text.push_str(&format!("    {:<28} {:e} {}\n", m.title, m.value, m.unit));
    }
    if let Some((format, file)) = report {
        let body = match format.as_str() {
            "json" => result.json(),
            "csv" => result.csv(),
            "html" => result.html(),
            other => {
                return Err(Failure::Usage(format!(
                    "the report is json, csv or html, not '{}'",
                    shown(other)
                )));
            }
        };
        std::fs::write(&file, &body)
            .map_err(|e| failed(format!("could not write {}: {e}", shown(&file))))?;
        text.push_str(&format!("Wrote {} ({} bytes)\n", shown(&file), body.len()));
    }
    Ok(text)
}

/// Runs `airfoil`; the arguments are those after the word.
pub fn airfoil(args: &[&str]) -> ExitCode {
    finish(make_airfoil(args))
}

fn make_airfoil(args: &[&str]) -> Result<String, Failure> {
    let [case, flags @ ..] = args else {
        return Err(Failure::Usage("airfoil needs a case".into()));
    };
    let (mut time, mut csv) = (None, None);
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let mut value = |what: &str| {
            flags
                .next()
                .map(|v| (*v).to_owned())
                .ok_or_else(|| Failure::Usage(format!("{what} needs a value")))
        };
        match *flag {
            "-time" => time = Some(value("-time")?),
            "-csv" => csv = Some(value("-csv")?),
            other => {
                return Err(Failure::Usage(format!("unknown option '{}'", shown(other))));
            }
        }
    }
    let failed = |e: String| Failure::Failed(e);
    let opened = Case::open(case).map_err(|e| failed(e.to_string()))?;
    let time = match time {
        Some(t) => t,
        None => opened.latest_time().map_err(|e| failed(e.to_string()))?,
    };
    let r = opened.airfoil(&time).map_err(|e| failed(e.to_string()))?;
    let s = &r.settings;
    let mut text = format!(
        "Time {}  NACA {:04}  alpha {}°  Re {:e}\n",
        shown(&time),
        s.naca,
        s.alpha,
        s.reynolds
    );
    text.push_str(&format!("    Lift coefficient Cl          {:.5}\n", r.cl));
    text.push_str(&format!(
        "    Drag coefficient Cd          {:.5}  (pressure {:.5}, viscous {:.5})\n",
        r.cd, r.cd_pressure, r.cd_viscous
    ));
    text.push_str(&format!("    Moment coefficient Cm (c/4)  {:.5}\n", r.cm));
    if let Some(peak) = r.suction_peak() {
        text.push_str(&format!("    Suction peak Cp              {peak:.4}\n"));
    }
    if let Some(file) = csv {
        let body = r.surface_csv();
        std::fs::write(&file, &body)
            .map_err(|e| failed(format!("could not write {}: {e}", shown(&file))))?;
        text.push_str(&format!(
            "Wrote {} ({} points)\n",
            shown(&file),
            r.surface.len()
        ));
    }
    Ok(text)
}

/// Runs `compare`; the arguments are those after the word.
pub fn compare_reports(args: &[&str]) -> ExitCode {
    finish(make_comparison(args))
}

fn make_comparison(args: &[&str]) -> Result<String, Failure> {
    let [metric, baseline, files @ ..] = args else {
        return Err(Failure::Usage(
            "compare needs a number's name, the baseline's position and the reports".into(),
        ));
    };
    let baseline: usize = baseline
        .parse()
        .map_err(|_| Failure::Usage("the baseline is the position of a report, from 1".into()))?;
    let mut files = files.to_vec();
    let mut csv_file = None;
    if let Some(at) = files.iter().position(|f| *f == "-csv") {
        csv_file = files.get(at + 1).map(|f| (*f).to_owned());
        if csv_file.is_none() {
            return Err(Failure::Usage("-csv needs a file".into()));
        }
        files.drain(at..=at + 1);
    }
    if files.is_empty() {
        return Err(Failure::Usage("compare needs at least one report".into()));
    }
    let failed = |e: String| Failure::Failed(e);
    let mut reports = Vec::with_capacity(files.len());
    for file in &files {
        let meta = std::fs::metadata(file)
            .map_err(|e| failed(format!("could not read {}: {e}", shown(file))))?;
        if meta.len() > vortia_post::MAX_REPORT_BYTES as u64 {
            return Err(failed(format!("{} is too large for a report", shown(file))));
        }
        let text = std::fs::read_to_string(file)
            .map_err(|e| failed(format!("could not read {}: {e}", shown(file))))?;
        reports.push(Sealed::open(&text).map_err(|e| failed(format!("{}: {e}", shown(file))))?);
    }
    let index = baseline
        .checked_sub(1)
        .ok_or_else(|| Failure::Usage("the baseline is the position of a report, from 1".into()))?;
    let rows = compare(&reports, metric, index).map_err(|e| failed(e.to_string()))?;
    let base_sha = reports
        .get(index)
        .map(|r| r.sha256.clone())
        .unwrap_or_default();
    let mut text = format!("{}, against report {baseline}\n", shown(metric));
    for r in &rows {
        let change = r
            .change_percent
            .map_or_else(|| "      n/a".to_owned(), |c| format!("{c:+8.3} %"));
        text.push_str(&format!(
            "    {:<32} {:e}  {change}{}\n",
            shown(&r.label),
            r.value,
            if r.warning.is_some() {
                "  (different definitions)"
            } else {
                ""
            }
        ));
    }
    if let Some(file) = csv_file {
        let body = comparison_csv(&rows, metric, &base_sha);
        std::fs::write(&file, &body)
            .map_err(|e| failed(format!("could not write {}: {e}", shown(&file))))?;
        text.push_str(&format!("Wrote {} ({} bytes)\n", shown(&file), body.len()));
    }
    Ok(text)
}
