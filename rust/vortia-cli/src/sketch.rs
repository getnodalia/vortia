//! `vortia sketch`: a dimensioned wall profile in a file, and `vortia new sketch`, which meshes
//! it into a case.

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use vortia_sketch::{Sketch, templates};

use crate::usage_error;

/// `vortia sketch template NAME FILE [NAME=VALUE...]` and `vortia sketch info FILE`.
pub fn run(args: &[&str]) -> ExitCode {
    match args {
        ["template", name, file, settings @ ..] => finish(template(name, file, settings)),
        ["info", file] => finish(info(file)),
        _ => usage_error("sketch needs `template NAME FILE [NAME=VALUE...]` or `info FILE`"),
    }
}

fn finish(result: Result<String, String>) -> ExitCode {
    match result {
        Ok(text) => {
            let _ = std::io::stdout().write_all(text.as_bytes());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("vortia: {message}");
            ExitCode::FAILURE
        }
    }
}

fn numbers(settings: &[&str], allowed: &[&str]) -> Result<Vec<(String, f64)>, String> {
    settings
        .iter()
        .map(|setting| {
            let (name, value) = setting
                .split_once('=')
                .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
            if !allowed.contains(&name) {
                return Err(format!("'{name}' is not one of {}", allowed.join(", ")));
            }
            let number: f64 = value
                .parse()
                .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
            Ok((name.to_owned(), number))
        })
        .collect()
}

fn template(name: &str, file: &str, settings: &[&str]) -> Result<String, String> {
    let get = |given: &[(String, f64)], key: &str, default: f64| {
        given
            .iter()
            .find(|(k, _)| k == key)
            .map_or(default, |(_, v)| *v)
    };
    let mut sketch = match name {
        "duct" => {
            let given = numbers(settings, &["length", "radius"])?;
            templates::duct(get(&given, "length", 500.0), get(&given, "radius", 25.0))
        }
        "nozzle" => {
            let given = numbers(
                settings,
                &[
                    "inlet-radius",
                    "throat-radius",
                    "outlet-radius",
                    "converging-length",
                    "diverging-length",
                ],
            )?;
            templates::nozzle(
                get(&given, "inlet-radius", 50.0),
                get(&given, "throat-radius", 26.0),
                get(&given, "outlet-radius", 60.0),
                get(&given, "converging-length", 158.0),
                get(&given, "diverging-length", 420.0),
            )
        }
        "rounded-nozzle" => {
            let given = numbers(
                settings,
                &[
                    "inlet-radius",
                    "throat-radius",
                    "outlet-radius",
                    "converging-length",
                    "diverging-length",
                    "throat-arc-radius",
                ],
            )?;
            templates::rounded_nozzle(
                get(&given, "inlet-radius", 50.0),
                get(&given, "throat-radius", 26.0),
                get(&given, "outlet-radius", 60.0),
                get(&given, "converging-length", 158.0),
                get(&given, "diverging-length", 420.0),
                get(&given, "throat-arc-radius", 30.0),
            )
        }
        "channel-with-block" => {
            let given = numbers(
                settings,
                &["length", "height", "block", "block-x", "block-y"],
            )?;
            templates::channel_with_block(
                get(&given, "length", 200.0),
                get(&given, "height", 50.0),
                get(&given, "block", 10.0),
                get(&given, "block-x", 60.0),
                get(&given, "block-y", 20.0),
            )
        }
        other => {
            return Err(format!(
                "'{other}' is not a template: duct, nozzle, rounded-nozzle or channel-with-block"
            ));
        }
    };
    sketch.solve().map_err(|e| e.to_string())?;
    sketch.save(Path::new(file)).map_err(|e| e.to_string())?;
    Ok(format!("Wrote the {name} sketch to {file}\n"))
}

fn info(file: &str) -> Result<String, String> {
    let mut sketch = Sketch::open(Path::new(file)).map_err(|e| e.to_string())?;
    let solution = sketch.solve().map_err(|e| e.to_string())?;
    let wall = sketch.profile(5.0).map_err(|e| e.to_string())?;
    let (first, last) = (wall.first(), wall.last());
    let (Some(first), Some(last)) = (first, last) else {
        return Err("the sketch has no wall".into());
    };
    let narrowest = wall.iter().fold(first.1, |m, p| m.min(p.1));
    Ok(format!(
        "{} points, {} lines and arcs, {} dimensions ({} equations for {} unknowns, solved to {:.1e} mm)\n\
         wall from x = {} to x = {} mm; radius {} at the inlet, {} at the outlet, {} at the narrowest\n",
        sketch.points.len(),
        sketch.entities.len(),
        sketch.constraints.len(),
        solution.equations,
        solution.unknowns,
        solution.residual,
        first.0,
        last.0,
        first.1,
        last.1,
        narrowest
    ))
}
