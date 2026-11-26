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
