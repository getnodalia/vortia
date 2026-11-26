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
    if let ["section", dir, file, settings @ ..] = args {
        return from_section(dir, file, settings);
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

fn is_dxf(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dxf"))
}

fn number_setting(setting: &str) -> Result<(&str, f64), String> {
    let (name, value) = setting
        .split_once('=')
        .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
    let number: f64 = value
        .parse()
        .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
    Ok((name, number))
}

/// `vortia new region DIR FILE.sketch|.dxf [NAME=VALUE...]`: a case whose region is the closed
/// outline of a sketch file or a DXF drawing, its edges (a DXF's layers) named `inlet`, `outlet`,
/// `symmetry` or anything else for a wall. A DXF without a unit is read as millimetres; `scale=F`
/// multiplies its lengths.
fn from_region(dir: &str, file: &str, settings: &[&str]) -> ExitCode {
    let build = || -> Result<vortia_cases::Drawn, String> {
        let mut drawn = vortia_cases::Drawn::default();
        if is_dxf(file) {
            let mut drawing = vortia_cad::dxf::read(Path::new(file)).map_err(|e| e.to_string())?;
            for setting in settings {
                let (name, number) = number_setting(setting)?;
                if name == "scale" {
                    drawing.scale(number).map_err(|e| e.to_string())?;
                } else {
                    drawn.set(name, number).map_err(|e| e.to_string())?;
                }
            }
            drawn.sketch = drawing.sketch().map_err(|e| e.to_string())?;
            for note in &drawing.notes {
                eprintln!("vortia: {note}");
            }
        } else {
            let mut sketch =
                vortia_sketch::Sketch::open(Path::new(file)).map_err(|e| e.to_string())?;
            sketch.solve().map_err(|e| e.to_string())?;
            drawn.sketch = sketch;
            for setting in settings {
                let (name, number) = number_setting(setting)?;
                drawn.set(name, number).map_err(|e| e.to_string())?;
            }
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

/// `vortia new section DIR FILE.dxf [upstream=F] [downstream=F] [lateral=F] [scale=F] [NAME=VALUE...]`:
/// the 2D flow around the closed outlines of a DXF drawing (an airfoil section, a car's side
/// profile, a cylinder) in a wind tunnel drawn around them. The tunnel's sizes are multiples of
/// the bodies' largest size; the flow comes in from the left, the top and bottom are symmetry planes
/// and the bodies are the patch `body`.
fn from_section(dir: &str, file: &str, settings: &[&str]) -> ExitCode {
    let build = || -> Result<vortia_cases::Drawn, String> {
        let mut drawing = vortia_cad::dxf::read(Path::new(file)).map_err(|e| e.to_string())?;
        let mut tunnel = vortia_cad::dxf::Tunnel::default();
        let mut drawn = vortia_cases::Drawn::default();
        let mut sized = false;
        for setting in settings {
            let (name, number) = number_setting(setting)?;
            match name {
                "upstream" => tunnel.upstream = number,
                "downstream" => tunnel.downstream = number,
                "lateral" => tunnel.lateral = number,
                "scale" => drawing.scale(number).map_err(|e| e.to_string())?,
                _ => {
                    sized |= name == "cell-size";
                    drawn.set(name, number).map_err(|e| e.to_string())?;
                }
            }
        }
        // The cells along the bodies are a fortieth of their size unless said, and grow out to
        // sixteen times that at the tunnel's walls.
        if let Some((lo, hi)) = drawing.bounds()
            && !sized
        {
            drawn.cell_size = ((hi.0 - lo.0).max(hi.1 - lo.1) / 40.0).max(1.0e-3);
        }
        if drawn.far_cell_size.is_none() {
            drawn.far_cell_size = Some(16.0 * drawn.cell_size);
        }
        drawn.sketch = drawing.around(tunnel).map_err(|e| e.to_string())?;
        for note in &drawing.notes {
            eprintln!("vortia: {note}");
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
                    "Wrote the flow around the section ({} cells, {} points) to {dir}; run it with `vortia solve {dir}` ({}), then `vortia forces {dir} {}`\n",
                    made.cells,
                    made.points,
                    made.application,
                    vortia_cad::dxf::BODY_PATCH
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

/// `vortia new body DIR FILE.stl|.obj|.step [scale=F] [NAME=VALUE...]`: the flow around a closed
/// surface in a wind tunnel, meshed by octree refinement. A STEP file carries its own unit; an
/// STL or OBJ is in metres unless `scale` says otherwise (0.001 for millimetres).
fn from_body(dir: &str, file: &str, settings: &[&str]) -> ExitCode {
    let build = || -> Result<vortia_cases::Body, String> {
        let mut surface = vortia_cad::load(Path::new(file)).map_err(|e| e.to_string())?;
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
    // The wall is one patch, or one for each named part of the surface.
    let walls = if body.by_part {
        vortia_mesh::bodymesh::part_patches(&body.surface, vortia_cases::BODY_PATCH)
    } else {
        None
    };
    match vortia_cases::create(Path::new(dir), &Geometry::Body(body)) {
        Ok(made) => {
            let forces = walls.map_or_else(
                || format!("`vortia forces {dir} {}`", vortia_cases::BODY_PATCH),
                |names| {
                    format!(
                        "`vortia forces {dir} PATCH` for each of {}",
                        names.join(", ")
                    )
                },
            );
            let _ = std::io::Write::write_fmt(
                &mut std::io::stdout(),
                format_args!(
                    "Wrote the flow around the body ({} cells, {} points) to {dir}; run it with `vortia solve {dir}` ({}), then {forces}\n",
                    made.cells, made.points, made.application,
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
