// SPDX-License-Identifier: GPL-3.0-or-later
//! `vortia repair FILE OUT.stl [weld=F] [max-hole=N]`: makes a surface watertight where that can
//! be done without guessing, and says what it did and what is left.

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use vortia_mesh::repair::{self, Options};

/// Reads the surface, repairs it, prints the report and writes the repaired surface as a binary
/// STL. The exit code is failure when the surface is still not watertight.
pub fn run(args: &[&str]) -> ExitCode {
    let go = || -> Result<bool, String> {
        let [file, out, settings @ ..] = args else {
            return Err("usage: vortia repair FILE OUT.stl [weld=F] [max-hole=N]".into());
        };
        let mut options = Options::default();
        for setting in settings {
            let (name, value) = setting
                .split_once('=')
                .ok_or_else(|| format!("'{setting}' is not NAME=VALUE"))?;
            let number: f64 = value
                .parse()
                .map_err(|_| format!("'{value}' in '{setting}' is not a number"))?;
            match name {
                "weld" if number >= 0.0 => options.weld = number,
                "max-hole" if number >= 0.0 && number.fract() == 0.0 => {
                    options.max_hole = number as usize;
                }
                _ => return Err(format!("'{setting}' is not weld=F or max-hole=N")),
            }
        }
        let surface = vortia_cad::load(Path::new(file)).map_err(|e| e.to_string())?;
        let (repaired, report) = repair::repair(&surface, options).map_err(|e| e.to_string())?;
        for line in report.describe() {
            out!("{line}");
        }
        repaired.write(Path::new(out)).map_err(|e| e.to_string())?;
        out!("written to {out}");
        Ok(report.watertight())
    };
    match go() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("vortia: {e}");
            ExitCode::FAILURE
        }
    }
}
