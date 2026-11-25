// SPDX-License-Identifier: MIT OR Apache-2.0
//! The temperature `simpleFoam` carries when the case has a `T` and a `Pr`:
//! in a fluid at rest it is conduction, which the scheme gets exactly, and a
//! case without a `Pr` is left alone.

use std::path::Path;

use vortia_fields::ScalarField;
use vortia_solvers::{Case, simple};

mod common;

use common::{HEADER, cavity_mesh_and_fields, write};

const CELLS: usize = 10;

/// The cavity with its lid at rest and a temperature of 1 on the lid and 0 on the other walls
/// (when `with_pr`, the Prandtl number is in `transportProperties`).
fn at_rest(dir: &Path, pr: Option<&str>) {
    cavity_mesh_and_fields(dir, CELLS);
    write(
        &dir.join("0/U"),
        "FoamFile { version 2.0; format ascii; class volVectorField; object U; }\n\
         dimensions [0 1 -1 0 0 0 0]; internalField uniform (0 0 0);\n\
         boundaryField { movingWall { type noSlip; } fixedWalls { type noSlip; } \
         frontAndBack { type empty; } }\n",
    );
    write(
        &dir.join("0/T"),
        "FoamFile { version 2.0; format ascii; class volScalarField; object T; }\n\
         dimensions [0 0 0 1 0 0 0]; internalField uniform 0;\n\
         boundaryField { movingWall { type fixedValue; value uniform 1; } \
         fixedWalls { type zeroGradient; } frontAndBack { type empty; } }\n",
    );
    write(
        &dir.join("system/controlDict"),
        &format!(
            "{HEADER}application simpleFoam; startFrom startTime; startTime 0; stopAt endTime; \
             endTime 3; deltaT 1; writeControl timeStep; writeInterval 3; \
             writeFormat ascii; writePrecision 14; timePrecision 6;"
        ),
    );
    write(
        &dir.join("system/fvSchemes"),
        &format!(
            "{HEADER}ddtSchemes {{ default steadyState; }} gradSchemes {{ default Gauss linear; }} \
             divSchemes {{ default none; div(phi,U) bounded Gauss upwind; \
             div(phi,T) bounded Gauss upwind; div((nuEff*dev2(T(grad(U))))) Gauss linear; }} \
             laplacianSchemes {{ default Gauss linear orthogonal; }} \
             interpolationSchemes {{ default linear; }} snGradSchemes {{ default orthogonal; }}"
        ),
    );
    write(
        &dir.join("system/fvSolution"),
        &format!(
            "{HEADER}solvers {{ \
               p {{ solver PCG; preconditioner DIC; tolerance 1e-10; relTol 0; }} \
               U {{ solver smoothSolver; smoother symGaussSeidel; tolerance 1e-10; relTol 0; }} \
               T {{ solver PCG; preconditioner DIC; tolerance 1e-13; relTol 0; }} \
             }} \
             SIMPLE {{ nNonOrthogonalCorrectors 0; pRefCell 0; pRefValue 0; }} \
             relaxationFactors {{ fields {{ p 0.3; }} equations {{ U 0.7; }} }}"
        ),
    );
    let pr = pr.map_or(String::new(), |p| format!("Pr {p};"));
    write(
        &dir.join("constant/transportProperties"),
        &format!("{HEADER}transportModel Newtonian; nu 0.01; {pr}"),
    );
    write(
        &dir.join("constant/turbulenceProperties"),
        &format!("{HEADER}simulationType laminar;"),
    );
}

fn temperature(dir: &Path) -> Vec<f64> {
    ScalarField::read(dir.join("3").join("T"))
        .unwrap()
        .internal
        .expand(CELLS * CELLS)
        .unwrap()
}

/// A fluid at rest between a hot lid and cold walls conducts: the temperature is the cavity's
/// own symmetric one, highest under the lid and between 0 and 1 everywhere.
#[test]
fn a_fluid_at_rest_conducts_symmetrically_between_a_hot_lid_and_cold_walls() {
    let dir = tempfile::tempdir().unwrap();
    at_rest(dir.path(), Some("0.71"));
    let path = dir.path().join("0/T");
    let t = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        t.replace(
            "fixedWalls { type zeroGradient; }",
            "fixedWalls { type fixedValue; value uniform 0; }",
        ),
    )
    .unwrap();
    simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap();
    let t = temperature(dir.path());
    let at = |i: usize, j: usize| t[j * CELLS + i];
    for j in 0..CELLS {
        for i in 0..CELLS / 2 {
            assert!(
                (at(i, j) - at(CELLS - 1 - i, j)).abs() < 1e-9,
                "T is not symmetric at ({i}, {j})"
            );
        }
    }
    assert!(at(CELLS / 2, CELLS - 1) > at(CELLS / 2, CELLS / 2));
    assert!(t.iter().all(|&v| (0.0..=1.0).contains(&v)));
}

/// With every other wall insulated the heat has nowhere to go: the fluid ends at the lid's
/// temperature, all through.
#[test]
fn a_fluid_with_insulated_walls_takes_the_temperature_of_the_lid() {
    let dir = tempfile::tempdir().unwrap();
    at_rest(dir.path(), Some("0.71"));
    simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap();
    let t = temperature(dir.path());
    assert!(
        t.iter().all(|&v| (v - 1.0).abs() < 1e-9),
        "T ranges over {:?}",
        t.iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
    );
}

/// Without a Prandtl number the temperature is not solved and not written.
#[test]
fn without_a_prandtl_number_the_temperature_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    at_rest(dir.path(), None);
    simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap();
    assert!(!dir.path().join("3/T").exists());
}

/// A Prandtl number that is not positive is refused.
#[test]
fn a_prandtl_number_that_is_not_positive_is_refused() {
    for pr in ["0", "-1"] {
        let dir = tempfile::tempdir().unwrap();
        at_rest(dir.path(), Some(pr));
        let err = simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap_err();
        assert!(
            err.to_string().contains("Pr must be positive"),
            "{pr}: {err}"
        );
    }
}
