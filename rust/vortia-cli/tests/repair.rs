// SPDX-License-Identifier: GPL-3.0-or-later
//! `vortia repair` and `vortia new body ... repair=1`, run as the binary a user runs.

use std::path::Path;
use std::process::Command;

use vortia_mesh::stl::Stl;

fn vortia(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vortia"))
        .args(args)
        .output()
        .expect("the vortia binary runs")
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A cube of side 1, one triangle short of closed, the last quad turned the wrong way round, and
/// its corners nudged a billionth so no two triangles quite agree about them.
fn broken_cube(path: &Path) {
    let v = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
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
    let mut n = 0.0;
    for (k, q) in quads.iter().enumerate() {
        for (j, tri) in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
            .into_iter()
            .enumerate()
        {
            if k == 5 && j == 1 {
                continue;
            }
            let mut tri = tri;
            if k == 3 {
                tri.swap(1, 2);
            }
            text.push_str("facet normal 0 0 0\nouter loop\n");
            for i in tri {
                let p = v[i];
                n += 1.0;
                text.push_str(&format!("vertex {} {} {}\n", p[0] + 1.0e-9 * n, p[1], p[2]));
            }
            text.push_str("endloop\nendfacet\n");
        }
    }
    text.push_str("endsolid cube\n");
    std::fs::write(path, text).expect("the cube is written");
}

#[test]
fn a_cube_with_a_hole_a_turned_face_and_loose_corners_is_made_watertight() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (file, mended) = (dir.path().join("cube.stl"), dir.path().join("mended.stl"));
    broken_cube(&file);
    assert!(!Stl::read(&file).unwrap().summary().closed());
    let out = vortia(&[
        "repair",
        file.to_str().expect("utf-8"),
        mended.to_str().expect("utf-8"),
    ]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let said = stdout(&out);
    for needle in [
        "vertices welded",
        "turned to agree",
        "holes filled",
        "closed and consistently wound",
    ] {
        assert!(said.contains(needle), "'{needle}' in\n{said}");
    }
    let summary = Stl::read(&mended).unwrap().summary();
    assert!(summary.closed());
    assert!((summary.volume.unwrap() - 1.0).abs() < 1e-3, "{summary:?}");
}

#[test]
fn what_cannot_be_mended_fails_and_says_what_is_left() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (file, mended) = (dir.path().join("cube.stl"), dir.path().join("mended.stl"));
    broken_cube(&file);
    let out = vortia(&[
        "repair",
        file.to_str().expect("utf-8"),
        mended.to_str().expect("utf-8"),
        "max-hole=2",
    ]);
    assert!(!out.status.success());
    assert!(
        stdout(&out).contains("still not watertight: 3 open edges"),
        "{}",
        stdout(&out)
    );
    let bad = vortia(&["repair", file.to_str().expect("utf-8")]);
    assert!(!bad.status.success());
    assert!(stderr(&bad).contains("usage"), "{}", stderr(&bad));
}

#[test]
fn a_case_is_made_from_a_broken_surface_only_when_it_is_repaired() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let file = dir.path().join("cube.stl");
    broken_cube(&file);
    let file = file.to_str().expect("utf-8");
    let plain = dir.path().join("plain");
    let refused = vortia(&["new", "body", plain.to_str().expect("utf-8"), file]);
    assert!(!refused.status.success(), "{}", stdout(&refused));
    let mended = dir.path().join("mended");
    let made = vortia(&[
        "new",
        "body",
        mended.to_str().expect("utf-8"),
        file,
        "repair=1",
        "model=0",
        "cells=1",
        "levels=1",
        "wake-levels=0",
    ]);
    assert!(made.status.success(), "{}{}", stdout(&made), stderr(&made));
    assert!(stdout(&made).contains("repair: "), "{}", stdout(&made));
    assert!(mended.join("constant/polyMesh").is_dir());
}
