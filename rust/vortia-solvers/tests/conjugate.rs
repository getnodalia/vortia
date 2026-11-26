// SPDX-License-Identifier: MIT OR Apache-2.0
//! A fluid and a solid that share a wall, solved together: a fluid at rest and a solid slab in
//! series between a hot end and a cold end conduct a heat flux that is the sum of the two
//! resistances' reciprocal, and the heat that enters the solid is the heat that leaves the fluid.

use std::path::Path;

use vortia_core::{Aabb, Vec3};
use vortia_fields::ScalarField;
use vortia_foamfile::RawPatch;
use vortia_mesh::blocks::structured_box_raw;
use vortia_solvers::{Case, simple};

mod common;

use common::{HEADER, write};

const FLUID_CELLS: usize = 10;
const SOLID_CELLS: usize = 10;
const FLUID_LENGTH: f64 = 0.1;
const SOLID_LENGTH: f64 = 0.2;
const SIDE: f64 = 0.01;
/// `ρ c_p` and `ν / Pr` give the fluid a conductivity of 1 W/mK.
const RHO_CP: f64 = 100.0;
const NU: f64 = 0.01;
const PR: f64 = 1.0;
const K_FLUID: f64 = RHO_CP * NU / PR;
const K_SOLID: f64 = 4.0;
const HOT: f64 = 1.0;
const COLD: f64 = 0.0;
const STEPS: usize = 400;

/// A box from `x0` to `x1` of `cells` cells along x, with its two x faces named `west` and
/// `east` and the four others `sides`, a wall.
fn slab(dir: &Path, x0: f64, x1: f64, cells: usize, west: &str, east: &str) {
    let bounds = Aabb {
        min: Vec3::new(x0, 0.0, 0.0),
        max: Vec3::new(x1, SIDE, SIDE),
    };
    let mut raw = structured_box_raw(bounds, [cells, 1, 1]).unwrap();
    // xMin, xMax, yMin, yMax, zMin, zMax
    let p = raw.patches.clone();
    let named = |name: &str, kind: &str, from: &[RawPatch]| RawPatch {
        name: name.into(),
        kind: kind.into(),
        start_face: from[0].start_face,
        n_faces: from.iter().map(|p| p.n_faces).sum(),
        groups: vec![kind.into()],
        dict: Default::default(),
    };
    raw.patches = vec![
        named(west, "wall", &p[0..1]),
        named(east, "wall", &p[1..2]),
        named("sides", "wall", &p[2..6]),
    ];
    raw.write_dir(dir).unwrap();
}

fn header(class: &str, object: &str) -> String {
    format!("FoamFile {{ version 2.0; format ascii; class {class}; object {object}; }}\n")
}

/// The case: the fluid on `[0, 0.1]` hot at its west end, the solid on `[0.1, 0.3]` cold at its
/// east end, `body` the wall they share.
fn case(dir: &Path) {
    slab(
        &dir.join("constant/polyMesh"),
        0.0,
        FLUID_LENGTH,
        FLUID_CELLS,
        "inlet",
        "body",
    );
    slab(
        &dir.join("constant/solid/polyMesh"),
        FLUID_LENGTH,
        FLUID_LENGTH + SOLID_LENGTH,
        SOLID_CELLS,
        "body",
        "far",
    );
    write(
        &dir.join("0/U"),
        &format!(
            "{}dimensions [0 1 -1 0 0 0 0]; internalField uniform (0 0 0);\n\
             boundaryField {{ inlet {{ type noSlip; }} body {{ type noSlip; }} \
             sides {{ type noSlip; }} }}\n",
            header("volVectorField", "U")
        ),
    );
    write(
        &dir.join("0/p"),
        &format!(
            "{}dimensions [0 2 -2 0 0 0 0]; internalField uniform 0;\n\
             boundaryField {{ inlet {{ type zeroGradient; }} body {{ type zeroGradient; }} \
             sides {{ type zeroGradient; }} }}\n",
            header("volScalarField", "p")
        ),
    );
    write(
        &dir.join("0/T"),
        &format!(
            "{}dimensions [0 0 0 1 0 0 0]; internalField uniform 0.5;\n\
             boundaryField {{ inlet {{ type fixedValue; value uniform {HOT}; }} \
             body {{ type conjugate; }} sides {{ type zeroGradient; }} }}\n",
            header("volScalarField", "T")
        ),
    );
    write(
        &dir.join("0/solid/T"),
        &format!(
            "{}dimensions [0 0 0 1 0 0 0]; internalField uniform 0.5;\n\
             boundaryField {{ body {{ type conjugate; }} \
             far {{ type fixedValue; value uniform {COLD}; }} \
             sides {{ type zeroGradient; }} }}\n",
            header("volScalarField", "T")
        ),
    );
    write(
        &dir.join("constant/solid/solidProperties"),
        &format!("{HEADER}k {K_SOLID}; interface body;"),
    );
    write(
        &dir.join("constant/transportProperties"),
        &format!("{HEADER}transportModel Newtonian; nu {NU}; Pr {PR}; rhoCp {RHO_CP};"),
    );
    write(
        &dir.join("constant/turbulenceProperties"),
        &format!("{HEADER}simulationType laminar;"),
    );
    write(
        &dir.join("system/controlDict"),
        &format!(
            "{HEADER}application simpleFoam; startFrom startTime; startTime 0; stopAt endTime; \
             endTime {STEPS}; deltaT 1; writeControl timeStep; writeInterval {STEPS}; \
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
               T {{ solver PCG; preconditioner DIC; tolerance 1e-14; relTol 0; }} \
             }} \
             SIMPLE {{ nNonOrthogonalCorrectors 0; pRefCell 0; pRefValue 0; }}"
        ),
    );
}

/// The heat flux through the two slabs in series, W/m², and the temperature of the wall.
fn series() -> (f64, f64) {
    let resistance = FLUID_LENGTH / K_FLUID + SOLID_LENGTH / K_SOLID;
    let q = (HOT - COLD) / resistance;
    (q, HOT - q * FLUID_LENGTH / K_FLUID)
}

fn read(path: impl AsRef<Path>, cells: usize) -> Vec<f64> {
    ScalarField::read(path)
        .unwrap()
        .internal
        .expand(cells)
        .unwrap()
}

fn run(dir: &Path) -> Vec<String> {
    let mut log = Vec::new();
    simple::run(&Case::open(dir).unwrap(), |line| log.push(line.to_owned())).unwrap();
    log
}

/// The fluid's temperature and the solid's are the straight lines of two conductors in series,
/// meeting at the temperature the resistances fix at their common wall.
#[test]
fn a_fluid_and_a_solid_in_series_conduct_the_analytic_flux() {
    let dir = tempfile::tempdir().unwrap();
    case(dir.path());
    run(dir.path());
    let (q, wall) = series();
    let end = dir.path().join(STEPS.to_string());
    let fluid = read(end.join("T"), FLUID_CELLS);
    let solid = read(end.join("solid/T"), SOLID_CELLS);
    for (i, t) in fluid.iter().enumerate() {
        let x = (i as f64 + 0.5) * FLUID_LENGTH / FLUID_CELLS as f64;
        let exact = HOT - q * x / K_FLUID;
        assert!(
            (t - exact).abs() < 1e-6,
            "fluid cell {i}: {t} against {exact}"
        );
    }
    for (j, t) in solid.iter().enumerate() {
        let x = (j as f64 + 0.5) * SOLID_LENGTH / SOLID_CELLS as f64;
        let exact = wall - q * x / K_SOLID;
        assert!(
            (t - exact).abs() < 1e-6,
            "solid cell {j}: {t} against {exact}"
        );
    }
}

/// When the coupling has settled, the heat that leaves the fluid through the wall is the heat
/// that enters the solid, and is the analytic flux over the wall's area.
#[test]
fn the_heat_that_leaves_the_fluid_enters_the_solid() {
    let dir = tempfile::tempdir().unwrap();
    case(dir.path());
    let log = run(dir.path());
    let line = log
        .iter()
        .rev()
        .find(|l| l.starts_with("conjugate:"))
        .expect("the run logs the balance");
    let numbers: Vec<f64> = line
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect();
    let (from_fluid, into_solid) = (numbers[0], numbers[1]);
    let expected = series().0 * SIDE * SIDE;
    assert!(
        (from_fluid - expected).abs() < 1e-6 * expected,
        "{from_fluid} W leave the fluid, {expected} W expected"
    );
    assert!(
        (into_solid - expected).abs() < 1e-6 * expected,
        "{into_solid} W enter the solid, {expected} W expected"
    );
}

/// A wall that is `conjugate` with no solid beside the fluid is refused, and so is a solid
/// whose coupling the fluid cannot size.
#[test]
fn a_conjugate_wall_needs_its_solid_and_the_fluids_rho_cp() {
    let dir = tempfile::tempdir().unwrap();
    case(dir.path());
    std::fs::remove_dir_all(dir.path().join("constant/solid")).unwrap();
    let err = simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap_err();
    assert!(err.to_string().contains("no constant/solid"), "{err}");

    let dir = tempfile::tempdir().unwrap();
    case(dir.path());
    let path = dir.path().join("constant/transportProperties");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace(&format!("rhoCp {RHO_CP};"), "")).unwrap();
    let err = simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap_err();
    assert!(err.to_string().contains("rhoCp"), "{err}");
}

/// A condition on the solid's walls that it does not know is refused by name, not read as an
/// insulated wall.
#[test]
fn a_solid_wall_condition_it_does_not_know_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    case(dir.path());
    let path = dir.path().join("0/solid/T");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("sides { type zeroGradient; }", "sides { type magic; }"),
    )
    .unwrap();
    let err = simple::run(&Case::open(dir.path()).unwrap(), |_| {}).unwrap_err();
    assert!(err.to_string().contains("magic"), "{err}");
}
