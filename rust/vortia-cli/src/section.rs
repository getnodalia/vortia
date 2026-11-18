//! `vortia section`: what goes through a plane across the domain, and the report of it.

use std::io::Write as _;
use std::process::ExitCode;

use vortia_post::{Case, Density, SectionOptions, SectionReport};

use crate::{shown, usage_error};

/// Runs `section`; the arguments are those after the word.
pub fn run(args: &[&str]) -> ExitCode {
    match parse(args).and_then(make) {
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

enum Failure {
    Usage(String),
    Failed(String),
}

struct Request {
    case: String,
    time: Option<String>,
    options: SectionOptions,
    report: Option<(String, String)>,
}

fn number(word: Option<&&str>, what: &str) -> Result<f64, Failure> {
    word.and_then(|w| w.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| Failure::Usage(format!("{what} needs a number")))
}

fn parse(args: &[&str]) -> Result<Request, Failure> {
    let [case, axis, position, flags @ ..] = args else {
        return Err(Failure::Usage(
            "section needs a case, an axis (x, y or z) and a position".into(),
        ));
    };
    let axis = match *axis {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        _ => return Err(Failure::Usage("the axis is x, y or z".into())),
    };
    let mut options = SectionOptions {
        axis,
        position: number(Some(position), "the position")?,
        ..SectionOptions::default()
    };
    let mut time = None;
    let mut report = None;
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        match *flag {
            "-reverse" => options.direction = -1.0,
            "-kinematic" => options.kinematic_pressure = true,
            "-time" => {
                time = Some(
                    (*flags
                        .next()
                        .ok_or_else(|| Failure::Usage("-time needs a time".into()))?)
                    .to_owned(),
                );
            }
            "-density" => {
                options.density = Density::Constant(number(flags.next(), "-density")?);
            }
            "-pref" => options.reference_pressure = number(flags.next(), "-pref")?,
            "-rhoref" => options.reference_density = number(flags.next(), "-rhoref")?,
            "-uref" => options.reference_speed = number(flags.next(), "-uref")?,
            "-aref" => options.reference_area = number(flags.next(), "-aref")?,
            "-wedge" => options.wedge_angle = Some(number(flags.next(), "-wedge")?),
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
    Ok(Request {
        case: (*case).to_owned(),
        time,
        options,
        report,
    })
}

fn make(r: Request) -> Result<String, Failure> {
    let failed = |e: String| Failure::Failed(e);
    let case = Case::open(&r.case).map_err(|e| failed(e.to_string()))?;
    let time = match r.time {
        Some(t) => t,
        None => case.latest_time().map_err(|e| failed(e.to_string()))?,
    };
    let analysis = case
        .section(&time, &r.options)
        .map_err(|e| failed(e.to_string()))?;
    let name = std::path::Path::new(&r.case)
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let report = SectionReport {
        label: format!("Section at {}", r.options.position),
        case: name,
        time: time.clone(),
        options: r.options,
        analysis,
    };
    let mut text = format!(
        "Time {}  section {} = {}\n",
        shown(&time),
        ["x", "y", "z"]
            .get(report.options.axis)
            .copied()
            .unwrap_or("x"),
        report.options.position
    );
    for m in report.metrics() {
        text.push_str(&format!("    {:<28} {:e} {}\n", m.title, m.value, m.unit));
    }
    if let Some((format, file)) = r.report {
        let body = match format.as_str() {
            "json" => report.json(),
            "csv" => report.csv(),
            "html" => report.html(),
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
