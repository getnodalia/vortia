//! `vortia new`: starts a case, as the app's Home "New" does.

use std::path::Path;
use std::process::ExitCode;

use vortia_cases::{Airfoil, Cavity, Channel, Geometry, Jet, Nozzle, NozzleKind, Plate};

/// `vortia new KIND DIR [NAME=VALUE...]`.
pub fn run(args: &[&str]) -> ExitCode {
    if let ["sketch", dir, file, settings @ ..] = args {
        return from_sketch(dir, file, settings);
    }
    if let ["region", dir, file, settings @ ..] = args {
        return from_region(dir, file, settings);
    }
    if let ["body", dir, file, settings @ ..] = args {
        return from_body(dir, file, settings);
    }
    let [kind, dir, settings @ ..] = args else {
        eprintln!(
            "vortia: new needs a kind (channel, cavity, plate, plate-turbulent, airfoil, nozzle, nozzle-wedge or jet) and a folder"
        );
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
        "cavity" => Geometry::Cavity(Cavity::default()),
        "plate" => Geometry::Plate(Plate::default()),
        "plate-turbulent" => Geometry::Plate(Plate::turbulent()),
        "nozzle" => Geometry::Nozzle(Nozzle::new(NozzleKind::Planar)),
        "nozzle-wedge" => Geometry::Nozzle(Nozzle::new(NozzleKind::Axisymmetric)),
        "jet" => Geometry::Jet(Jet::default()),
        "airfoil" => Geometry::Airfoil(Airfoil::default()),
        other => {
            return Err(format!(
                "'{other}' is not a kind of case: channel, cavity, plate, plate-turbulent, airfoil, nozzle, nozzle-wedge or jet"
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
            Geometry::Cavity(c) => c.set(name, number),
            Geometry::Plate(p) => p.set(name, number),
            Geometry::Nozzle(n) => n.set(name, number),
            Geometry::Jet(j) => j.set(name, number),
            Geometry::Drawn(d) => d.set(name, number),
            Geometry::Airfoil(a) => a.set(name, number),
            Geometry::Body(b) => b.set(name, number),
        }
        .map_err(|e| e.to_string())?;
    }
    Ok(geometry)
}

/// `vortia new sketch DIR FILE [planar|axisymmetric] [segments=N] [NAME=VALUE...]`: a case whose
/// wall is the profile of a sketch file.
fn from_sketch(dir: &str, file: &str, settings: &[&str]) -> ExitCode {
    let build = || -> Result<Nozzle, String> {
        let mut sketch = vortia_sketch::Sketch::open(Path::new(file)).map_err(|e| e.to_string())?;
        sketch.solve().map_err(|e| e.to_string())?;
        let mut kind = NozzleKind::Planar;
        let mut segments = 60;
        let mut rest = Vec::new();
        for setting in settings {
            match *setting {
                "planar" => kind = NozzleKind::Planar,
                "axisymmetric" => kind = NozzleKind::Axisymmetric,
                other => match other.strip_prefix("segments=") {
                    Some(n) => {
                        segments = n
                            .parse()
                            .map_err(|_| format!("'{n}' in '{other}' is not a whole number"))?;
                    }
                    None => rest.push(other),
                },
            }
        }
        let mut nozzle = Nozzle::from_sketch(kind, &sketch, segments).map_err(|e| e.to_string())?;
        for setting in rest {
            let (name, value) = setting
                .split_once('=')
                .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
            let number: f64 = value
                .parse()
                .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
            nozzle.set(name, number).map_err(|e| e.to_string())?;
        }
        Ok(nozzle)
    };
    let nozzle = match build() {
        Ok(n) => n,
        Err(message) => {
            eprintln!("vortia: {message}");
            return ExitCode::from(2);
        }
    };
    match vortia_cases::create(Path::new(dir), &Geometry::Nozzle(nozzle)) {
        Ok(made) => {
            let _ = std::io::Write::write_fmt(
                &mut std::io::stdout(),
                format_args!(
                    "Wrote the sketched duct ({} cells, {} points) to {dir}; run it with `vortia solve {dir}` ({})\n",
                    made.cells, made.points, made.application
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

/// `vortia new region DIR FILE [NAME=VALUE...]`: a case whose region is the closed outline of a
/// sketch file, its edges named `inlet`, `outlet`, `symmetry` or anything else for a wall.
fn from_region(dir: &str, file: &str, settings: &[&str]) -> ExitCode {
    let build = || -> Result<vortia_cases::Drawn, String> {
        let mut sketch = vortia_sketch::Sketch::open(Path::new(file)).map_err(|e| e.to_string())?;
        sketch.solve().map_err(|e| e.to_string())?;
        let mut drawn = vortia_cases::Drawn {
            sketch,
            ..vortia_cases::Drawn::default()
        };
        for setting in settings {
            let (name, value) = setting
                .split_once('=')
                .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
            let number: f64 = value
                .parse()
                .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
            drawn.set(name, number).map_err(|e| e.to_string())?;
        }
        Ok(drawn)
    };
    let drawn = match build() {
        Ok(d) => d,
        Err(message) => {
            eprintln!("vortia: {message}");
            return ExitCode::from(2);
        }
    };
    match vortia_cases::create(Path::new(dir), &Geometry::Drawn(drawn)) {
        Ok(made) => {
            let _ = std::io::Write::write_fmt(
                &mut std::io::stdout(),
                format_args!(
                    "Wrote the drawn region ({} cells, {} points) to {dir}; run it with `vortia solve {dir}` ({})\n",
                    made.cells, made.points, made.application
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

/// `vortia new body DIR FILE.stl|FILE.obj [scale=F] [NAME=VALUE...]`: the flow around a closed
/// surface in a wind tunnel, meshed by octree refinement.
fn from_body(dir: &str, file: &str, settings: &[&str]) -> ExitCode {
    let build = || -> Result<vortia_cases::Body, String> {
        let mut surface =
            vortia_mesh::stl::Stl::load(Path::new(file)).map_err(|e| e.to_string())?;
        let mut rest = Vec::new();
        for setting in settings {
            let (name, value) = setting
                .split_once('=')
                .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
            let number: f64 = value
                .parse()
                .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
            // The unit of the file: 0.001 for millimetres.
            if name == "scale" {
                surface.scale(number).map_err(|e| e.to_string())?;
            } else {
                rest.push((name, number));
            }
        }
        let mut body = vortia_cases::Body::new(surface);
        for (name, number) in rest {
            body.set(name, number).map_err(|e| e.to_string())?;
        }
        Ok(body)
    };
    let body = match build() {
        Ok(b) => b,
        Err(message) => {
            eprintln!("vortia: {message}");
            return ExitCode::from(2);
        }
    };
    match vortia_cases::create(Path::new(dir), &Geometry::Body(body)) {
        Ok(made) => {
            let _ = std::io::Write::write_fmt(
                &mut std::io::stdout(),
                format_args!(
                    "Wrote the flow around the body ({} cells, {} points) to {dir}; run it with `vortia solve {dir}` ({}), then `vortia forces {dir} {}`\n",
                    made.cells,
                    made.points,
                    made.application,
                    vortia_cases::BODY_PATCH
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
