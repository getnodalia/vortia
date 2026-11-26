// SPDX-License-Identifier: GPL-3.0-or-later
//! `vortia new body ... solid=MATERIAL heat=W`, `vortia solve` and `vortia solid`, run as the
//! binary a user runs: a heated body gives its heat to the air.

use std::path::Path;
use std::process::Command;

fn vortia(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vortia"))
        .args(args)
        .output()
        .expect("the vortia binary runs")
}

/// An ASCII STL of the unit cube centred on the origin, outward facing.
fn cube(path: &Path) {
    let v = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    let quads = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [1, 2, 6, 5],
        [3, 0, 4, 7],
    ];
    let mut text = String::from("solid cube\n");
    for q in quads {
        for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            text.push_str("facet normal 0 0 0\nouter loop\n");
            for i in tri {
                let p = v[i];
                text.push_str(&format!("vertex {} {} {}\n", p[0], p[1], p[2]));
            }
            text.push_str("endloop\nendfacet\n");
        }
    }
    text.push_str("endsolid cube\n");
    std::fs::write(path, text).expect("the cube is written");
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_heated_cube_gives_its_heat_to_the_air_and_the_report_says_so() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let file = dir.path().join("cube.stl");
    cube(&file);
    let case = dir.path().join("case");
    let (file, case) = (file.to_str().expect("utf-8"), case.to_str().expect("utf-8"));
    let new = vortia(&[
        "new",
        "body",
        case,
        file,
        "solid=aluminium",
        "heat=5",
        "model=0",
        "reynolds=100",
        "cells=1",
        "levels=2",
        "wake-levels=1",
        "upstream=1.5",
        "downstream=3",
        "lateral=1.5",
    ]);
    assert!(new.status.success(), "{}", stderr(&new));
    assert!(stdout(&new).contains("solid body"), "{}", stdout(&new));
    assert!(Path::new(case).join("constant/solid/polyMesh").is_dir());

    let control = Path::new(case).join("system/controlDict");
    let text = std::fs::read_to_string(&control).expect("controlDict");
    let text: Vec<String> = text
        .lines()
        .map(|l| match l.split_whitespace().next() {
            Some(k @ ("endTime" | "writeInterval")) => format!("{k} 400;"),
            _ => l.to_owned(),
        })
        .collect();
    std::fs::write(&control, text.join("\n")).expect("controlDict");

    let solved = vortia(&["solve", case]);
    assert!(solved.status.success(), "{}", stderr(&solved));
    let report = vortia(&["solid", case]);
    assert!(report.status.success(), "{}", stderr(&report));
    let report = stdout(&report);
    let number = |label: &str| -> f64 {
        report
            .lines()
            .find(|l| l.trim_start().starts_with(label))
            .and_then(|l| l.split_whitespace().find_map(|w| w.parse().ok()))
            .unwrap_or_else(|| panic!("no '{label}' in\n{report}"))
    };
    assert!((number("heat generated") - 5.0).abs() < 1e-9, "{report}");
    assert!((number("heat out") - 5.0).abs() < 0.1, "{report}");
}

#[test]
fn an_unknown_material_is_refused_with_the_ones_there_are() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let file = dir.path().join("cube.stl");
    cube(&file);
    let case = dir.path().join("case");
    let new = vortia(&[
        "new",
        "body",
        case.to_str().expect("utf-8"),
        file.to_str().expect("utf-8"),
        "solid=unobtainium",
    ]);
    assert!(!new.status.success());
    let message = stderr(&new);
    assert!(
        message.contains("unobtainium") && message.contains("aluminium"),
        "{message}"
    );
}

/// An ASCII STL of a sphere of radius `r` about the origin: an icosahedron split `splits` times.
fn sphere(path: &Path, r: f64, splits: u32) {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let unit = |p: [f64; 3]| {
        let n = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        [p[0] / n, p[1] / n, p[2] / n]
    };
    let mut v: Vec<[f64; 3]> = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ]
    .into_iter()
    .map(unit)
    .collect();
    let mut f: Vec<[usize; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..splits {
        let mut middle = std::collections::HashMap::new();
        let mut split = Vec::new();
        for [a, b, c] in f {
            let mut mid = |i: usize, j: usize| {
                *middle.entry((i.min(j), i.max(j))).or_insert_with(|| {
                    let (p, q) = (v[i], v[j]);
                    v.push(unit([
                        (p[0] + q[0]) / 2.0,
                        (p[1] + q[1]) / 2.0,
                        (p[2] + q[2]) / 2.0,
                    ]));
                    v.len() - 1
                })
            };
            let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
            split.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        f = split;
    }
    let mut text = String::from("solid sphere\n");
    for tri in f {
        text.push_str("facet normal 0 0 0\nouter loop\n");
        for i in tri {
            let p = v[i];
            text.push_str(&format!("vertex {} {} {}\n", r * p[0], r * p[1], r * p[2]));
        }
        text.push_str("endloop\nendfacet\n");
    }
    text.push_str("endsolid sphere\n");
    std::fs::write(path, text).expect("the sphere is written");
}

/// The number after `key` on the line of `report` that starts with `label`.
fn after(report: &str, label: &str, key: &str) -> f64 {
    let line = report
        .lines()
        .find(|l| l.trim_start().starts_with(label))
        .unwrap_or_else(|| panic!("no '{label}' in\n{report}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    words
        .windows(2)
        .find(|w| w[0] == key)
        .and_then(|w| w[1].parse().ok())
        .unwrap_or_else(|| panic!("no '{key}' on '{line}'"))
}

/// A heated sphere in a stream, against what can be worked out by hand: the temperature inside
/// a sphere that generates heat evenly, and the heat the air takes from a sphere.
#[test]
fn a_heated_sphere_has_the_temperature_inside_and_the_film_outside_that_theory_gives() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let file = dir.path().join("sphere.stl");
    let (radius, heat, k, re, pr, rho_cp, nu): (f64, f64, f64, f64, f64, f64, f64) =
        (0.5, 300.0, 0.2, 200.0, 0.71, 1206.0, 0.005);
    sphere(&file, radius, 3);
    let case = dir.path().join("case");
    let (file, case) = (file.to_str().expect("utf-8"), case.to_str().expect("utf-8"));
    let new = vortia(&[
        "new",
        "body",
        case,
        file,
        "solid=plastic",
        "heat=300",
        "model=0",
        "reynolds=200",
        "cells=1",
        "levels=4",
        "wake-levels=2",
        "upstream=2",
        "downstream=4",
        "lateral=2",
    ]);
    assert!(new.status.success(), "{}", stderr(&new));
    let solved = vortia(&["solve", case]);
    assert!(solved.status.success(), "{}", stderr(&solved));
    let report = vortia(&["solid", case]);
    assert!(report.status.success(), "{}", stderr(&report));
    let report = stdout(&report);

    // All the heat the sphere makes leaves through its wall.
    let heat_out = after(&report, "heat out", "out");
    assert!((heat_out - heat).abs() < 1e-3 * heat, "{report}");

    // Inside: T(r) = T_wall + q (R² − r²) / 6k, so the centre is q R² / 6k = Q / (8 π R k)
    // above the wall. The mesh follows the sphere in steps, hence the band.
    let rise = heat / (8.0 * std::f64::consts::PI * radius * k);
    let t_max = after(&report, "temperature", "max");
    let wall = after(&report, "wall", "temperature");
    assert!(
        (t_max - wall - rise).abs() < 0.05 * rise,
        "centre {} K above the wall, theory {rise} K\n{report}",
        t_max - wall
    );

    // Outside: the Nusselt number of the sphere from the film, Ranz and Marshall's
    // Nu = 2 + 0.6 Re^½ Pr^⅓, a correlation that is good to some tens of per cent.
    let area = after(&report, "wall", "area");
    let k_air = rho_cp * nu / pr;
    let nu_sphere = heat / (area * (wall - 293.15)) * (2.0 * radius) / k_air;
    let ranz_marshall = 2.0 + 0.6 * re.sqrt() * pr.cbrt();
    assert!(
        (nu_sphere / ranz_marshall - 1.0).abs() < 0.25,
        "Nu {nu_sphere}, correlation {ranz_marshall}\n{report}"
    );
}
