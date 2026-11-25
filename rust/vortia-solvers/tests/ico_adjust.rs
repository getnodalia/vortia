// SPDX-License-Identifier: MIT OR Apache-2.0
//! `icoFoam` with `adjustTimeStep yes` on the lid-driven cavity (the
//! tutorial's 20 × 20 cells, ν = 0.01, lid at 1 m/s, to t = 0.5).
//!
//! The clock follows the Courant number, lands on the write times and on the
//! end of the run, and the answer is the one a step several times shorter
//! gives.

use std::path::Path;

use vortia_fields::VectorField;
use vortia_solvers::{Case, ico};

mod common;

use common::{HEADER, cavity_mesh_and_fields, write};

const CELLS: usize = 20;

fn cavity(dir: &Path, control: &str) {
    cavity_mesh_and_fields(dir, CELLS);
    write(
        &dir.join("system/controlDict"),
        &format!(
            "{HEADER}application icoFoam; startFrom startTime; startTime 0; stopAt endTime; \
             {control} writeFormat ascii; writePrecision 12; timePrecision 6;"
        ),
    );
    write(
        &dir.join("system/fvSchemes"),
        &format!(
            "{HEADER}ddtSchemes {{ default Euler; }} gradSchemes {{ default Gauss linear; }} \
             divSchemes {{ default none; div(phi,U) Gauss linear; }} \
             laplacianSchemes {{ default Gauss linear orthogonal; }} \
             interpolationSchemes {{ default linear; }} snGradSchemes {{ default orthogonal; }}"
        ),
    );
    write(
        &dir.join("system/fvSolution"),
        &format!(
            "{HEADER}solvers {{ \
               p {{ solver PCG; preconditioner DIC; tolerance 1e-10; relTol 0; }} \
               pFinal {{ $p; }} \
               U {{ solver smoothSolver; smoother symGaussSeidel; tolerance 1e-10; relTol 0; }} \
             }} \
             PISO {{ nCorrectors 2; nNonOrthogonalCorrectors 0; pRefCell 0; pRefValue 0; }}"
        ),
    );
    write(
        &dir.join("constant/transportProperties"),
        &format!("{HEADER}nu 0.01;"),
    );
}

/// The velocity written at `time`.
fn velocity(dir: &Path, time: &str) -> Vec<f64> {
    let u = VectorField::read(dir.join(time).join("U")).unwrap();
    let u = u.internal.expand(CELLS * CELLS).unwrap();
    u.iter().flat_map(|v| [v.x, v.y]).collect()
}

/// A run that follows the Courant number lands on the write times and on the
/// end of the run, never passes `maxCo` by more than the flux changing
/// within a step allows, `maxDeltaT`, or a 20 % growth, and gives the
/// answer of a step five times shorter than the tutorial's, to 1 % of the
/// lid speed (the tolerance was fixed before the first run).
#[test]
fn an_adjusted_step_follows_the_courant_number_and_the_answer_holds() {
    let reference_dir = tempfile::tempdir().unwrap();
    cavity(
        reference_dir.path(),
        "endTime 0.5; deltaT 0.001; writeControl runTime; writeInterval 0.25;",
    );
    let reference = ico::run(&Case::open(reference_dir.path()).unwrap(), |_| {}).unwrap();
    assert_eq!(reference.len(), 500);

    let dir = tempfile::tempdir().unwrap();
    cavity(
        dir.path(),
        "endTime 0.5; deltaT 0.0001; adjustTimeStep yes; maxCo 0.5; maxDeltaT 0.02; \
         writeControl adjustableRunTime; writeInterval 0.25;",
    );
    let mut log = String::new();
    let steps = ico::run(&Case::open(dir.path()).unwrap(), |l| log.push_str(l)).unwrap();

    assert_eq!(steps.last().map(|s| s.time.as_str()), Some("0.5"));
    let written: Vec<&str> = steps
        .iter()
        .filter(|s| s.written)
        .map(|s| s.time.as_str())
        .collect();
    assert_eq!(written, ["0.25", "0.5"], "{written:?}");

    let number = |prefix: &str| -> Vec<f64> {
        log.lines()
            .filter_map(|l| l.strip_prefix(prefix))
            .map(|v| v.trim().parse().unwrap())
            .collect()
    };
    let dts = number("deltaT = ");
    let courants = number("Courant Number max: ");
    assert_eq!(dts.len(), steps.len());
    assert_eq!(courants.len(), steps.len());
    assert!(dts.iter().all(|&dt| dt > 0.0 && dt <= 0.02 + 1e-12));
    // The first step has no flux to follow, so it has no number to bound.
    let worst = courants.iter().skip(1).fold(0.0_f64, |a, &b| a.max(b));
    eprintln!("{} steps, largest Courant number {worst:.3}", steps.len());
    assert!(worst <= 0.5 * 1.2, "Courant number {worst}");
    // It grows by at most 20 % a step (landing on a write time may shorten it).
    assert!(dts.windows(2).all(|w| w[1] <= 1.2 * w[0] + 1e-12));
    // The run is not left taking the step it started with.
    assert!(
        dts.iter().any(|&dt| dt > 0.002),
        "the step stayed at {:?}",
        dts.iter().fold(0.0_f64, |a, &b| a.max(b))
    );

    for time in ["0.25", "0.5"] {
        let (a, b) = (
            velocity(dir.path(), time),
            velocity(reference_dir.path(), time),
        );
        let worst = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max);
        eprintln!("t = {time}: largest difference from the 0.001 step {worst:.5}");
        assert!(worst < 0.01, "t = {time}: {worst}");
    }
}

/// The limits are checked and the run ends rather than looping on a flow
/// that gives the clock nothing to follow.
#[test]
fn adjust_time_step_limits_are_refused_when_they_make_no_sense() {
    for control in [
        "endTime 0.1; deltaT 0.001; adjustTimeStep yes; maxCo 0;",
        "endTime 0.1; deltaT 0.001; adjustTimeStep yes; maxCo -1;",
        "endTime 0.1; deltaT 0.001; adjustTimeStep yes; maxCo 0.5; maxDeltaT 0;",
    ] {
        let dir = tempfile::tempdir().unwrap();
        cavity(dir.path(), control);
        let err = ico::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap_err();
        assert!(
            err.to_string().contains("must be positive"),
            "{control}: {err}"
        );
    }
}
