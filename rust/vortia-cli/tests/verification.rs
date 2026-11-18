//! Verification of the Rust engine against solutions that are known exactly, made the way a
//! user makes them: a case written by `vortia-cases`, run by `vortia-solvers`, read by
//! `vortia-post`. The tolerances are the ones `benchmarks/*/README.md` fixes before any run
//! (`docs/VALIDATION.md` keeps the record).

use std::path::Path;

use vortia_cases::{Channel, Geometry};
use vortia_core::Vec3;
use vortia_post::{Case, gci};

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
