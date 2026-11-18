//! `vortia new`: starts a case, as the app's Home "New" does.

use std::path::Path;
use std::process::ExitCode;

use vortia_cases::{Channel, Geometry, Nozzle, NozzleKind};

/// `vortia new KIND DIR [NAME=VALUE...]`.
pub fn run(args: &[&str]) -> ExitCode {
    let [kind, dir, settings @ ..] = args else {
        eprintln!("vortia: new needs a kind (channel, nozzle or nozzle-wedge) and a folder");
        return ExitCode::from(2);
    };
    let geometry = match geometry(kind, settings) {
        Ok(g) => g,
        Err(message) => {
            eprintln!("vortia: {message}");
            return ExitCode::from(2);
        }
    };
    match vortia_cases::create(Path::new(dir), &geometry) {
        Ok(made) => {
            let _ = std::io::Write::write_fmt(
                &mut std::io::stdout(),
                format_args!(
                    "Wrote {} ({} cells, {} points) to {dir}; run it with `vortia solve {dir}` ({})\n",
                    geometry.title().to_lowercase(),
                    made.cells,
                    made.points,
                    made.application
                ),
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("vortia: {e}");
            ExitCode::FAILURE
        }
    }
}

fn geometry(kind: &str, settings: &[&str]) -> Result<Geometry, String> {
    let mut geometry = match kind {
        "channel" => Geometry::Channel(Channel::default()),
        "nozzle" => Geometry::Nozzle(Nozzle::new(NozzleKind::Planar)),
        "nozzle-wedge" => Geometry::Nozzle(Nozzle::new(NozzleKind::Axisymmetric)),
        other => {
            return Err(format!(
                "'{other}' is not a kind of case: channel, nozzle or nozzle-wedge"
            ));
        }
    };
    for setting in settings {
        let (name, value) = setting
            .split_once('=')
            .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
        let number: f64 = value
            .parse()
            .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
        match &mut geometry {
            Geometry::Channel(c) => c.set(name, number),
            Geometry::Nozzle(n) => n.set(name, number),
        }
        .map_err(|e| e.to_string())?;
    }
    Ok(geometry)
}
