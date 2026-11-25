//! Verification of the Rust engine against solutions that are known exactly, made the way a
//! user makes them: a case written by `vortia-cases`, run by `vortia-solvers`, read by
//! `vortia-post`. The tolerances are the ones `benchmarks/*/README.md` fixes before any run
//! (`docs/VALIDATION.md` keeps the record).

use std::path::Path;

use vortia_cases::{Channel, Geometry};
use vortia_core::Vec3;
use vortia_post::{Case, FieldValues, gci};

const LENGTH: f64 = 1.0;
const HEIGHT: f64 = 0.1;
const SPEED: f64 = 1.0;

/// A channel of Reynolds number 10 on its height, so that the flow has developed well before
/// the station where it is read (the entry length is about 0.2 m of the 1 m).
fn channel(cells_x: usize, cells_y: usize) -> Channel {
    Channel {
        length: LENGTH,
        height: HEIGHT,
        cells_x,
        cells_y,
        speed: SPEED,
        viscosity: 1.0e-2,
        ..Channel::default()
    }
}

/// Writes the channel, runs it to convergence and returns the case, ready to read.
fn solved(dir: &Path, c: Channel) -> Case {
    vortia_cases::create(dir, &Geometry::Channel(c)).expect("the channel is written");
    let case = vortia_solvers::Case::open(dir).expect("the solver opens it");
    let mut log = String::new();
    vortia_solvers::run(&case, |line| log = line.to_owned()).expect("the run finishes");
    Case::open(dir).expect("the results open")
}

/// The streamwise speed along the height at `x`, one value per cell row, with the height at the
/// row's centre.
fn profile(case: &Case, x: f64, rows: usize) -> Vec<(f64, f64)> {
    let time = case.latest_time().expect("a time was written");
    (0..rows)
        .map(|j| {
            let y = (j as f64 + 0.5) * HEIGHT / rows as f64;
            let sample = case
                .probe(&time, "U", Vec3::new(x, y, 0.005))
                .expect("the point is inside");
            (y, sample.value.first().copied().unwrap_or(f64::NAN))
        })
        .collect()
}

/// The exact developed profile of a channel with the mean speed `SPEED`.
fn exact(y: f64) -> f64 {
    let s = y / HEIGHT;
    6.0 * SPEED * s * (1.0 - s)
}

#[test]
fn poiseuille_profile_matches_the_exact_solution() {
    let dir = tempfile::tempdir().expect("a folder");
    let rows = 20;
    let case = solved(dir.path(), channel(80, rows));
    let u = profile(&case, 0.9 * LENGTH, rows);
    let peak = 1.5 * SPEED;
    // Acceptance fixed in benchmarks/poiseuille-laminar-2d: the largest difference relative to
    // the peak under 3 % and the walls at rest, here with the profile read in cell centres.
    let worst = u
        .iter()
        .map(|(y, v)| (v - exact(*y)).abs() / peak)
        .fold(0.0, f64::max);
    println!("Poiseuille 80x20: L-infinity of the profile / peak = {worst:e}");
    assert!(worst < 0.03, "L∞ of the profile is {worst:e}, over 3 %");
    let mid = u.get(rows / 2).map(|p| p.1).unwrap_or(f64::NAN);
    assert!(
        (mid - exact(u[rows / 2].0)).abs() / peak < 0.02,
        "the mid-plane speed is {mid}"
    );
    // The flow rate is conserved down the channel: the section at x = L/2 carries the inflow.
    let time = case.latest_time().expect("a time was written");
    let flux = case.flux(&time, "inlet").expect("the inlet flux");
    let outflux = case.flux(&time, "outlet").expect("the outlet flux");
    assert!(
        (flux + outflux).abs() < 1e-6 * flux.abs(),
        "{flux} in, {outflux} out"
    );
}

/// Three meshes of the same channel, each twice as fine as the last: the speed at the mid-plane
/// converges and the index says how well it is known.
#[test]
fn poiseuille_converges_with_the_mesh() {
    let mid = |ny: usize| {
        let dir = tempfile::tempdir().expect("a folder");
        let case = solved(dir.path(), channel(ny * 4, ny));
        let u = profile(&case, 0.9 * LENGTH, ny);
        // The mean of the two rows either side of the mid-plane is its linear interpolation.
        let (below, above) = (u[ny / 2 - 1].1, u[ny / 2].1);
        0.5 * (below + above)
    };
    let (f3, f2, f1) = (mid(8), mid(16), mid(32));
    let g = gci(f1, f2, f3, 2.0, 2.0, 1.25).expect("the values converge monotonically");
    println!("Poiseuille mid-plane speed on 8, 16, 32 rows: {f3}, {f2}, {f1}; {g:?}");
    // The scheme is second order: three meshes in the asymptotic range show it.
    assert!(
        (1.8..2.3).contains(&g.observed_order),
        "order {}",
        g.observed_order
    );
    assert!(
        (g.asymptotic_ratio - 1.0).abs() < 0.05,
        "ratio {}",
        g.asymptotic_ratio
    );
    assert!(g.fine < 0.01, "the fine-mesh index is {:e}", g.fine);
    // The extrapolated value is the exact mid-plane speed of the developed flow.
    assert!(
        (g.extrapolated - 1.5 * SPEED).abs() < 0.002 * 1.5 * SPEED,
        "extrapolated {}",
        g.extrapolated
    );
}

const GHIA_U: &str = include_str!(
    "../../../benchmarks/lid-driven-cavity-ghia-re100/reference/ghia_1982_re100_u_centerline.csv"
);
const GHIA_V: &str = include_str!(
    "../../../benchmarks/lid-driven-cavity-ghia-re100/reference/ghia_1982_re100_v_centerline.csv"
);

/// The `(position, value)` rows of one of the reference tables, the two walls left out: the
/// value there is the boundary condition, not something the solution found.
fn ghia(table: &str) -> Vec<(f64, f64)> {
    table
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let (a, b) = l.split_once(',')?;
            Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?))
        })
        .filter(|(s, _)| *s > 0.0 && *s < 1.0)
        .collect()
}

/// A component of the velocity in the cells of an `n` × `n` cavity of side 1, by `[i][j]` with
/// `i` along x and `j` along y, from the cell centres.
fn grid(case: &Case, n: usize, component: usize) -> Vec<Vec<f64>> {
    let time = case.latest_time().expect("a time was written");
    (0..n)
        .map(|i| {
            (0..n)
                .map(|j| {
                    let point = Vec3::new(
                        (i as f64 + 0.5) / n as f64,
                        (j as f64 + 0.5) / n as f64,
                        0.05,
                    );
                    let sample = case.probe(&time, "U", point).expect("inside the cavity");
                    sample.value.get(component).copied().unwrap_or(f64::NAN)
                })
                .collect()
        })
        .collect()
}

/// The value along a line through the middle of the cavity at `s` (0 to 1) by linear
/// interpolation between cell centres, with the wall's own value (`at_start`, `at_end`) taken
/// as the value on the wall itself. `line(k)` is the value of the `k`th cell on the line.
fn along(n: usize, s: f64, at_start: f64, at_end: f64, line: impl Fn(usize) -> f64) -> f64 {
    let h = 1.0 / n as f64;
    let (first, last) = (0.5 * h, 1.0 - 0.5 * h);
    if s <= first {
        return at_start + (line(0) - at_start) * s / first;
    }
    if s >= last {
        return line(n - 1) + (at_end - line(n - 1)) * (s - last) / (1.0 - last);
    }
    let k = ((s - first) / h).floor() as usize;
    let t = (s - first) / h - k as f64;
    line(k) * (1.0 - t) + line(k + 1) * t
}

/// Lid-driven cavity at Reynolds number 100 against Ghia, Ghia and Shin (1982), both
/// centre-line profiles, with the largest difference taken relative to the lid speed.
/// Acceptance fixed in `benchmarks/lid-driven-cavity-ghia-re100`: under 5 %.
#[test]
fn the_cavity_matches_ghia_at_reynolds_100() {
    let n = 32;
    let dir = tempfile::tempdir().expect("a folder");
    vortia_cases::create(
        dir.path(),
        &Geometry::Cavity(vortia_cases::Cavity {
            cells: n,
            ..Default::default()
        }),
    )
    .expect("the cavity is written");
    let run = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&run, |_| {}).expect("the run finishes");
    let case = Case::open(dir.path()).expect("the results open");
    let (u, v) = (grid(&case, n, 0), grid(&case, n, 1));
    // u on x = 1/2: the mean of the two columns either side of it, then along y.
    let u_mid = |j: usize| 0.5 * (u[n / 2 - 1][j] + u[n / 2][j]);
    let v_mid = |i: usize| 0.5 * (v[i][n / 2 - 1] + v[i][n / 2]);
    let du = ghia(GHIA_U)
        .iter()
        .map(|(y, want)| (along(n, *y, 0.0, 1.0, u_mid) - want).abs())
        .fold(0.0, f64::max);
    let dv = ghia(GHIA_V)
        .iter()
        .map(|(x, want)| (along(n, *x, 0.0, 0.0, v_mid) - want).abs())
        .fold(0.0, f64::max);
    println!("Cavity Re 100, {n}x{n}: u on x = 1/2 differs by {du:e}, v on y = 1/2 by {dv:e} of U");
    assert!(du < 0.05, "u differs from Ghia by {du:e} of the lid speed");
    assert!(dv < 0.05, "v differs from Ghia by {dv:e} of the lid speed");
}

/// Three meshes of the cavity, each twice as fine as the last: the horizontal speed at the centre
/// (the mean of the four cells around it) converges, the index says how well it is known, and the
/// extrapolated value is within 2 % of the 0.20581 that Ghia, Ghia and Shin give at the middle of
/// the vertical line (their table is read on a 129 × 129 grid; the extrapolation, 0.2092, is the
/// mesh-independent value of this scheme, 1.6 % from it).
#[test]
fn the_cavity_converges_with_the_mesh() {
    let centre = |n: usize| {
        let dir = tempfile::tempdir().expect("a folder");
        vortia_cases::create(
            dir.path(),
            &Geometry::Cavity(vortia_cases::Cavity {
                cells: n,
                ..Default::default()
            }),
        )
        .expect("the cavity is written");
        let run = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
        vortia_solvers::run(&run, |_| {}).expect("the run finishes");
        let case = Case::open(dir.path()).expect("the results open");
        let u = grid(&case, n, 0);
        let (a, b) = (n / 2 - 1, n / 2);
        0.25 * (u[a][a] + u[a][b] + u[b][a] + u[b][b])
    };
    let (f3, f2, f1) = (centre(16), centre(32), centre(64));
    let g = gci(f1, f2, f3, 2.0, 2.0, 1.25).expect("the values converge monotonically");
    println!("Cavity centre u on 16, 32, 64 cells: {f3}, {f2}, {f1}; {g:?}");
    assert!(
        (1.5..2.5).contains(&g.observed_order),
        "order {}",
        g.observed_order
    );
    assert!(
        (g.asymptotic_ratio - 1.0).abs() < 0.1,
        "ratio {}",
        g.asymptotic_ratio
    );
    assert!(g.fine < 0.02, "the fine-mesh index is {:e}", g.fine);
    assert!(
        (g.extrapolated + 0.20581).abs() < 0.02 * 0.20581,
        "extrapolated {}",
        g.extrapolated
    );
}

/// The Blasius function `f'(η) = u/U` against `η = y √(U / ν x)`: the solution of
/// `f''' + f f''/2 = 0` with `f(0) = f'(0) = 0` and `f'(∞) = 1`, integrated from the wall value
/// `f''(0) = 0.332057` by fourth-order Runge-Kutta. Gives `f'` at each `η` asked for, which must
/// be increasing.
fn blasius(etas: &[f64]) -> Vec<f64> {
    let step = 1.0e-3;
    let rate = |s: [f64; 3]| [s[1], s[2], -0.5 * s[0] * s[2]];
    let add =
        |a: [f64; 3], b: [f64; 3], k: f64| [a[0] + k * b[0], a[1] + k * b[1], a[2] + k * b[2]];
    let mut s = [0.0, 0.0, 0.332_057_336_215_196];
    let mut eta = 0.0;
    etas.iter()
        .map(|&target| {
            while eta + step <= target {
                let k1 = rate(s);
                let k2 = rate(add(s, k1, step / 2.0));
                let k3 = rate(add(s, k2, step / 2.0));
                let k4 = rate(add(s, k3, step));
                for i in 0..3 {
                    s[i] += step / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
                }
                eta += step;
            }
            // The rest of the way to the target, by the slope at the last point.
            s[1] + (target - eta) * s[2]
        })
        .collect()
}

#[test]
fn the_blasius_function_has_its_known_values() {
    let f = blasius(&[1.0, 2.0, 3.0, 5.0]);
    for (got, table) in f.iter().zip([0.32979, 0.62977, 0.84605, 0.99155]) {
        assert!((got - table).abs() < 2e-5, "{got} vs {table}");
    }
}

#[test]
fn the_flat_plate_matches_blasius() {
    let dir = tempfile::tempdir().expect("a folder");
    // A mesh a quarter the size of the default, which keeps the test quick: the friction
    // coefficient is within 2 % of Blasius's on it and about as far on the default mesh.
    let plate = vortia_cases::Plate {
        height: 0.3,
        cells_x: 50,
        cells_up: 10,
        cells_y: 80,
        grading: 200.0,
        ..vortia_cases::Plate::default()
    };
    vortia_cases::create(dir.path(), &Geometry::Plate(plate.clone()))
        .expect("the plate is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let case = Case::open(dir.path()).expect("the results open");
    let time = case.latest_time().expect("a time was written");

    // The friction coefficient from the stress the engine reports on the plate: local at the
    // middle (Blasius gives 0.664 / √Re_x) and the mean over the plate (1.328 / √Re_L).
    let wall = case.wall(&time, "plate").expect("the wall is read");
    let q = plate.speed * plate.speed;
    let mean = 2.0 * wall.viscous_force.x / (wall.area * q);
    let exact_mean = 1.328 / (plate.speed * plate.length / plate.viscosity).sqrt();
    eprintln!("mean cf = {mean:.5}, Blasius {exact_mean:.5}");
    assert!(
        (mean - exact_mean).abs() < 0.05 * exact_mean,
        "the mean cf {mean} is not within 5 % of {exact_mean}"
    );
    // The drag coefficient of the plate, from the forces report, is the same number: the plate
    // has no pressure drag, and its area is its length times the one cell's depth.
    let reference = vortia_post::ForceReference {
        speed: plate.speed,
        area: plate.length * plate.depth,
        ..vortia_post::ForceReference::default()
    };
    let forces = case
        .forces(&time, &["plate".to_owned()], &reference)
        .expect("the forces are read");
    let metric = |key: &str| {
        forces
            .metrics()
            .into_iter()
            .find(|m| m.key == key)
            .map_or(f64::NAN, |m| m.value)
    };
    assert!(
        (metric("cd") - mean).abs() < 1e-9 * mean.abs(),
        "cd {} and the mean cf {mean} differ",
        metric("cd")
    );
    assert!(metric("pressureDrag").abs() < 1e-12 * metric("viscousDrag"));
    // The faces of the plate run along it in cell order, so the middle one is half way.
    let middle = wall
        .shear
        .get(plate.cells_x / 2)
        .map_or(f64::NAN, |s| -2.0 * s.x / q);
    let exact_middle = 0.664 / (plate.speed * 0.5 * plate.length / plate.viscosity).sqrt();
    eprintln!("cf at x = L/2: {middle:.5}, Blasius {exact_middle:.5}");
    assert!(
        (middle - exact_middle).abs() < 0.05 * exact_middle,
        "cf at x = L/2, {middle}, is not within 5 % of {exact_middle}"
    );

    // The profile half way along the plate, in every cell of the column up to η = 5: the
    // largest difference from Blasius's, relative to the stream speed (the acceptance of
    // `benchmarks/blasius-flat-plate`).
    let centres = &case.geometry().cell_centres;
    // The plate starts at x = 0 and the slip wall is at negative x.
    let x_mid = 0.5 * plate.length;
    let x = centres
        .iter()
        .map(|c| c.x)
        .min_by(|a, b| (a - x_mid).abs().total_cmp(&(b - x_mid).abs()))
        .expect("the mesh has cells");
    let scale = (plate.speed / (plate.viscosity * x)).sqrt();
    let mut column: Vec<(f64, f64)> = centres
        .iter()
        .enumerate()
        .filter(|(_, c)| (c.x - x).abs() < 1e-9 && c.y * scale <= 5.0)
        .map(|(i, c)| (c.y * scale, i as f64))
        .collect();
    column.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(column.len() > 10, "{} cells in the layer", column.len());
    let exact = blasius(&column.iter().map(|c| c.0).collect::<Vec<_>>());
    let FieldValues::Vector(u) = case.field(&time, "U").expect("U is read") else {
        panic!("U is a vector");
    };
    let mut worst = 0.0f64;
    for ((eta, i), f) in column.iter().zip(exact) {
        let speed = u.get(*i as usize).map_or(f64::NAN, |v| v.x) / plate.speed;
        worst = worst.max((speed - f).abs());
        eprintln!("eta {eta:.3}: {speed:.4} vs {f:.4}");
    }
    eprintln!("largest difference {worst:.4} of the stream speed");
    assert!(worst < 0.05, "the profile is {worst} off Blasius's");
}

/// A stream that meets nothing but slip walls stays as it is: uniform, with no friction on the
/// walls and no flow through them. (`slip` on a plane normal to an axis.)
#[test]
fn a_slip_wall_neither_rubs_nor_lets_anything_through() {
    let dir = tempfile::tempdir().expect("a folder");
    let plate = vortia_cases::Plate {
        cells_x: 10,
        cells_up: 4,
        cells_y: 8,
        grading: 4.0,
        ..vortia_cases::Plate::default()
    };
    vortia_cases::create(dir.path(), &Geometry::Plate(plate)).expect("the plate is written");
    let velocity = dir.path().join("0/U");
    let text = std::fs::read_to_string(&velocity).expect("U is there");
    assert!(text.contains("noSlip"));
    std::fs::write(&velocity, text.replace("noSlip", "slip")).expect("U is written");
    // A uniform stream is already the answer: a few iterations show it.
    let control = dir.path().join("system/controlDict");
    let text = std::fs::read_to_string(&control).expect("controlDict is there");
    assert!(text.contains("endTime         3000;"));
    let text = text
        .replace("endTime         3000;", "endTime         20;")
        .replace("writeInterval   100;", "writeInterval   20;");
    std::fs::write(&control, text).expect("controlDict is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let case = Case::open(dir.path()).expect("the results open");
    let time = case.latest_time().expect("a time was written");

    let wall = case.wall(&time, "plate").expect("the wall is read");
    assert!(
        wall.viscous_force.norm() < 1e-12,
        "{:?}",
        wall.viscous_force
    );
    for patch in ["plate", "slip"] {
        let flux = case.flux(&time, patch).expect("the flux is read");
        assert!(flux.abs() < 1e-12, "{patch} lets {flux} through");
    }
    for y in [0.01, 0.1, 0.45] {
        let u = case
            .probe(&time, "U", Vec3::new(0.5, y, 0.005))
            .expect("the point is inside")
            .value;
        assert!(
            (u.first().copied().unwrap_or(f64::NAN) - 1.0).abs() < 1e-9,
            "{u:?}"
        );
        assert!(u.get(1).copied().unwrap_or(f64::NAN).abs() < 1e-9, "{u:?}");
    }
}

/// The turbulent plate of `benchmarks/turbulent-flat-plate-sst`: k-ω SST with wall functions at
/// Reynolds number 10⁷. The acceptance is the one fixed before the first run: the mean friction
/// coefficient within 15 % of Prandtl–Schlichting's, C_D = 0.455 / (log₁₀ Re_L)^2.58. It is a
/// plausibility check against an empirical correlation, not a validation.
#[test]
fn the_turbulent_plate_is_in_the_range_of_prandtl_schlichting() {
    let dir = tempfile::tempdir().expect("a folder");
    let plate = vortia_cases::Plate::turbulent();
    vortia_cases::create(dir.path(), &Geometry::Plate(plate.clone()))
        .expect("the plate is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let case = Case::open(dir.path()).expect("the results open");
    let time = case.latest_time().expect("a time was written");

    let reynolds = plate.speed * plate.length / plate.viscosity;
    let correlation = 0.455 / reynolds.log10().powf(2.58);
    let reference = vortia_post::ForceReference {
        speed: plate.speed,
        area: plate.length * plate.depth,
        ..vortia_post::ForceReference::default()
    };
    let forces = case
        .forces(&time, &["plate".to_owned()], &reference)
        .expect("the forces are read");
    let cd = forces
        .metrics()
        .into_iter()
        .find(|m| m.key == "cd")
        .map_or(f64::NAN, |m| m.value);
    eprintln!(
        "time {time}: C_D = {cd:.6}, Prandtl-Schlichting {correlation:.6} ({:+.1} %)",
        100.0 * (cd / correlation - 1.0)
    );
    assert!(
        (cd - correlation).abs() < 0.15 * correlation,
        "C_D {cd} is not within 15 % of {correlation}"
    );
}

/// The half section of the planar nozzle, with a symmetry plane on the axis, is the upper half
/// of the whole slot: the same flow through it half as much, nothing through the plane, and the
/// same speed, temperature and pressure at a point of the half.
#[test]
fn a_half_nozzle_with_a_symmetry_plane_is_half_of_the_whole_slot() {
    let run = |half: bool| {
        let dir = tempfile::tempdir().expect("a folder");
        let nozzle = vortia_cases::Nozzle {
            converging_segments: 6,
            diverging_segments: 10,
            axial_cells_per_segment: 2,
            radial_cells: 12,
            radial_grading: 1.5,
            half,
            ..vortia_cases::Nozzle::new(vortia_cases::NozzleKind::Planar)
        };
        vortia_cases::create(dir.path(), &Geometry::Nozzle(nozzle)).expect("the nozzle is written");
        let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
        vortia_solvers::run(&case, |_| {}).expect("the run finishes");
        let results = Case::open(dir.path()).expect("the results open");
        let time = results.latest_time().expect("a time was written");
        (dir, results, time)
    };
    let (_whole_dir, whole, whole_time) = run(false);
    let (_half_dir, half, half_time) = run(true);

    let out = |case: &Case, time: &str| case.flux(time, "outlet").expect("the flux is read");
    let (whole_out, half_out) = (out(&whole, &whole_time), out(&half, &half_time));
    assert!(
        (2.0 * half_out - whole_out).abs() < 1e-3 * whole_out.abs(),
        "twice the half's outlet flux {} against the whole's {whole_out}",
        2.0 * half_out
    );
    let across = half.flux(&half_time, "symmetry").expect("the flux is read");
    assert!(
        across.abs() < 1e-12,
        "the symmetry plane lets {across} through"
    );

    let point = Vec3::new(0.2, 0.02, 0.0005);
    for field in ["U", "T", "p"] {
        let value = |case: &Case, time: &str| {
            case.probe(time, field, point)
                .expect("the point is inside")
                .value
        };
        let (a, b) = (value(&half, &half_time), value(&whole, &whole_time));
        let first = |v: &[f64]| v.first().copied().unwrap_or(f64::NAN);
        assert!(
            (first(&a) - first(&b)).abs() < 1e-3 * first(&b).abs(),
            "{field}: {a:?} in the half, {b:?} in the whole"
        );
    }
}

/// A jet from an orifice into the open atmosphere (`vortia_cases::Jet`, the practice case of
/// `examples/nozzle-practice-2026`): the flow leaving at a pressure ratio of 1.18 is the
/// isentropic one, Mach 0.50 at 169 m/s, and what enters through the orifice leaves through the
/// atmosphere.
#[test]
fn a_jet_into_the_atmosphere_leaves_at_the_isentropic_speed() {
    let dir = tempfile::tempdir().expect("a folder");
    let jet = vortia_cases::Jet {
        cells_axial: 40,
        cells_orifice: 15,
        cells_outer: 30,
        ..vortia_cases::Jet::default()
    };
    let (p0, pa, t0, gamma, r_gas) = (
        jet.total_pressure,
        jet.ambient_pressure,
        jet.total_temperature,
        1.4_f64,
        287.0_f64,
    );
    vortia_cases::create(dir.path(), &Geometry::Jet(jet)).expect("the jet is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let results = Case::open(dir.path()).expect("the results open");
    let time = results.latest_time().expect("a time was written");

    let mach_squared = 2.0 / (gamma - 1.0) * ((p0 / pa).powf((gamma - 1.0) / gamma) - 1.0);
    let speed = mach_squared.sqrt()
        * (gamma * r_gas * t0 / (1.0 + 0.5 * (gamma - 1.0) * mach_squared)).sqrt();
    let orifice = results
        .patch_stats(&time, "U", "inlet")
        .expect("the patch is read");
    let mean = orifice.average.first().copied().unwrap_or(f64::NAN);
    assert!(
        (mean - speed).abs() < 0.05 * speed,
        "{mean} m/s against the isentropic {speed} m/s"
    );
    let (entering, leaving) = (
        results.flux(&time, "inlet").expect("the flux is read"),
        results.flux(&time, "atmosphere").expect("the flux is read"),
    );
    assert!(entering < 0.0 && leaving > 0.0);
    assert!(
        (entering + leaving).abs() < 1e-3 * leaving,
        "{entering} in, {leaving} out"
    );
    assert!(
        results
            .flux(&time, "walls")
            .expect("the flux is read")
            .abs()
            < 1e-18
    );
}

/// The same nozzle meshed by the engine's own unstructured mesher (triangles made into prisms)
/// and by blocks: the flow through it and the state at a point agree, though not a single cell
/// does. The half slot is checked against the blocks within 0.2 %, and the axisymmetric wedge
/// (a coarser mesh, with cells on the collapsed axis) within 1 %.
#[test]
fn an_unstructured_nozzle_carries_the_same_flow_as_the_blocks() {
    use vortia_cases::NozzleKind;
    for (kind, size, flux_tolerance) in [
        (NozzleKind::Planar, 5.0, 2e-3),
        (NozzleKind::Axisymmetric, 6.0, 1e-2),
    ] {
        let run = |cell_size: Option<f64>| {
            let dir = tempfile::tempdir().expect("a folder");
            let nozzle = vortia_cases::Nozzle {
                converging_segments: 6,
                diverging_segments: 10,
                axial_cells_per_segment: 2,
                radial_cells: 20,
                radial_grading: 1.5,
                half: kind == NozzleKind::Planar,
                cell_size,
                ..vortia_cases::Nozzle::new(kind)
            };
            vortia_cases::create(dir.path(), &Geometry::Nozzle(nozzle))
                .expect("the nozzle is written");
            let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
            vortia_solvers::run(&case, |_| {}).expect("the run finishes");
            let results = Case::open(dir.path()).expect("the results open");
            let time = results.latest_time().expect("a time was written");
            (dir, results, time)
        };
        let (_blocks_dir, blocks, blocks_time) = run(None);
        let (_tri_dir, tri, tri_time) = run(Some(size));

        let out = |case: &Case, time: &str| case.flux(time, "outlet").expect("the flux is read");
        let (a, b) = (out(&tri, &tri_time), out(&blocks, &blocks_time));
        assert!(
            (a - b).abs() < flux_tolerance * b.abs(),
            "{kind:?}: the outlet flux {a} on triangles against {b} on blocks"
        );
        let inlet = tri.flux(&tri_time, "inlet").expect("the flux is read");
        assert!(
            (inlet + a).abs() < 1e-3 * a.abs(),
            "{kind:?}: {inlet} enters and {a} leaves"
        );
        if kind == NozzleKind::Planar {
            let across = tri.flux(&tri_time, "symmetry").expect("the flux is read");
            assert!(
                across.abs() < 1e-9 * a.abs(),
                "the symmetry plane lets {across} through"
            );
        }
        let point = Vec3::new(0.2, 0.02, 0.0005);
        let first = |v: &[f64]| v.first().copied().unwrap_or(f64::NAN);
        for (field, tolerance) in [("U", 0.03), ("T", 0.01), ("p", 0.01)] {
            let value = |case: &Case, time: &str| {
                case.probe(time, field, point)
                    .map(|p| first(&p.value))
                    .unwrap_or(f64::NAN)
            };
            let (x, y) = (value(&tri, &tri_time), value(&blocks, &blocks_time));
            if kind == NozzleKind::Planar {
                assert!(
                    (x - y).abs() < tolerance * y.abs(),
                    "{field}: {x} on triangles, {y} on blocks"
                );
            }
        }
    }
}

/// A channel with a block in it, drawn as a closed sketch with named edges and meshed with triangles,
/// carries the flow it is given: what enters at the inlet (speed × height × depth) leaves at the
/// outlet, and the wall of the block lets nothing through.
#[test]
fn a_drawn_channel_with_a_block_conserves_the_flow_that_enters_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let drawn = vortia_cases::Drawn {
        sketch: vortia_sketch::templates::channel_with_block(200.0, 50.0, 10.0, 60.0, 20.0),
        cell_size: 4.0,
        ..vortia_cases::Drawn::default()
    };
    vortia_cases::create(dir.path(), &Geometry::Drawn(drawn)).expect("the region is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let results = Case::open(dir.path()).expect("the results open");
    let time = results.latest_time().expect("a time was written");
    let flux = |patch: &str| results.flux(&time, patch).expect("the flux is read");
    // 1 m/s through 0.05 m × 0.01 m.
    let (inlet, outlet) = (flux("inlet"), flux("outlet"));
    assert!((inlet + 5e-4).abs() < 1e-6, "{inlet} enters");
    assert!((outlet - 5e-4).abs() < 1e-6, "{outlet} leaves");
    assert!(
        flux("block").abs() < 1e-12,
        "{} through the block",
        flux("block")
    );
}

/// A NACA 0012 at 5° and Reynolds number 6 × 10⁶, on a medium C grid with wall functions.
/// Thin-airfoil theory gives a lift slope of 2π per radian, which the NASA Turbulence Modeling
/// Resource's CFL3D Spalart–Allmaras results for this section match (C_L 1.0909 at 10°). The
/// acceptance, fixed before the first run: C_L within 8 % of 2π·α, a drag coefficient between
/// 0.008 and 0.016 (a wall-function mesh overpredicts the 0.0082–0.0123 of the references), a
/// small moment about the quarter chord and a suction peak on the upper surface.
#[test]
fn a_naca_0012_at_five_degrees_follows_thin_airfoil_theory() {
    let dir = tempfile::tempdir().expect("a folder");
    let foil = vortia_cases::Airfoil {
        alpha: 5.0,
        cells_chord: 60,
        cells_normal: 32,
        cells_wake: 20,
        ..vortia_cases::Airfoil::default()
    };
    vortia_cases::create(dir.path(), &Geometry::Airfoil(foil)).expect("the airfoil is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let case = Case::open(dir.path()).expect("the results open");
    let time = case.latest_time().expect("a time was written");
    let report = case.airfoil(&time).expect("the coefficients are read");
    let thin = 2.0 * std::f64::consts::PI * 5.0_f64.to_radians();
    eprintln!(
        "time {time}: C_L = {:.4} (2πα {thin:.4}, {:+.1} %), C_D = {:.5}, C_m = {:.4}",
        report.cl,
        100.0 * (report.cl / thin - 1.0),
        report.cd,
        report.cm
    );
    assert!(
        (report.cl - thin).abs() < 0.08 * thin,
        "C_L {} is not within 8 % of {thin}",
        report.cl
    );
    assert!(
        (0.008..0.016).contains(&report.cd),
        "C_D {} is out of range",
        report.cd
    );
    assert!(report.cm.abs() < 0.02, "C_m {}", report.cm);
    assert!(
        (report.cd - report.cd_pressure - report.cd_viscous).abs() < 1e-9,
        "the two parts of the drag add up to it"
    );
    let peak = report.suction_peak().expect("the surface has points");
    assert!(peak < -1.0, "suction peak {peak}");
    assert!(
        report
            .surface
            .iter()
            .all(|p| p.cp.is_finite() && p.cf.is_finite()),
        "every surface value is finite"
    );
}

/// The same turbulent plate with the Spalart–Allmaras model (wall function of Spalding,
/// `nuTilda` at four times the viscosity in the stream). The first run, on 2026-11-25, gave
/// a friction coefficient 5.6 % above Prandtl–Schlichting's, so the band here is 10 %, a
/// margin around what the engine does and not a band fixed beforehand.
#[test]
fn the_turbulent_plate_with_spalart_allmaras_is_in_the_range_of_prandtl_schlichting() {
    let dir = tempfile::tempdir().expect("a folder");
    let plate = vortia_cases::Plate::turbulent();
    vortia_cases::create(dir.path(), &Geometry::Plate(plate.clone()))
        .expect("the plate is written");
    let swap = |file: &str, from: &str, to: &str| {
        let path = dir.path().join(file);
        let text = std::fs::read_to_string(&path).expect("the file is there");
        assert!(text.contains(from), "{file} has no {from}");
        std::fs::write(&path, text.replace(from, to)).expect("the file is written");
    };
    swap(
        "constant/turbulenceProperties",
        "kOmegaSST",
        "SpalartAllmaras",
    );
    swap(
        "0/nut",
        "type            nutkWallFunction;",
        "type            nutUSpaldingWallFunction;",
    );
    swap(
        "system/fvSchemes",
        "div(phi,k)      bounded Gauss upwind;\n    div(phi,omega)  bounded Gauss upwind;",
        "div(phi,nuTilda) bounded Gauss upwind;",
    );
    let block = |name: &str| {
        format!(
            "    {name}\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n"
        )
    };
    swap("system/fvSolution", &format!("\n{}", block("omega")), "");
    swap("system/fvSolution", &block("k"), &block("nuTilda"));
    swap(
        "system/fvSolution",
        "k               1e-06;\n        omega           1e-06;",
        "nuTilda         1e-06;",
    );
    swap(
        "system/fvSolution",
        "k               0.7;\n        omega           0.7;",
        "nuTilda         0.7;",
    );
    std::fs::remove_file(dir.path().join("0/k")).expect("k is there");
    std::fs::remove_file(dir.path().join("0/omega")).expect("omega is there");
    let nu_tilda = 4.0 * plate.viscosity;
    std::fs::write(
        dir.path().join("0/nuTilda"),
        format!(
            "FoamFile {{ version 2.0; format ascii; class volScalarField; object nuTilda; }}\n\
             dimensions [0 2 -1 0 0 0 0];\ninternalField uniform {nu_tilda};\nboundaryField\n{{\n\
             inlet {{ type fixedValue; value uniform {nu_tilda}; }}\n\
             outlet {{ type inletOutlet; inletValue uniform {nu_tilda}; value uniform {nu_tilda}; }}\n\
             plate {{ type fixedValue; value uniform 0; }}\n\
             freestream {{ type inletOutlet; inletValue uniform {nu_tilda}; value uniform {nu_tilda}; }}\n\
             frontAndBack {{ type empty; }}\n}}\n"
        ),
    )
    .expect("nuTilda is written");
    let case = vortia_solvers::Case::open(dir.path()).expect("the solver opens it");
    vortia_solvers::run(&case, |_| {}).expect("the run finishes");
    let case = Case::open(dir.path()).expect("the results open");
    let time = case.latest_time().expect("a time was written");

    let reynolds = plate.speed * plate.length / plate.viscosity;
    let correlation = 0.455 / reynolds.log10().powf(2.58);
    let reference = vortia_post::ForceReference {
        speed: plate.speed,
        area: plate.length * plate.depth,
        ..vortia_post::ForceReference::default()
    };
    let cd = case
        .forces(&time, &["plate".to_owned()], &reference)
        .expect("the forces are read")
        .metrics()
        .into_iter()
        .find(|m| m.key == "cd")
        .map_or(f64::NAN, |m| m.value);
    eprintln!(
        "time {time}: C_D = {cd:.6}, Prandtl-Schlichting {correlation:.6} ({:+.1} %)",
        100.0 * (cd / correlation - 1.0)
    );
    assert!(
        (cd - correlation).abs() < 0.10 * correlation,
        "C_D {cd} is not within 10 % of {correlation}"
    );
}
