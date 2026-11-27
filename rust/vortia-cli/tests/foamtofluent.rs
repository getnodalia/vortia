// SPDX-License-Identifier: GPL-3.0-or-later
//! `vortia foamtofluent`, run as the binary a user runs, and read back by `fluenttofoam`.

use std::process::Command;

use vortia_core::{Aabb, Vec3};
use vortia_foamfile::RawPolyMesh;
use vortia_mesh::blocks;

fn vortia(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vortia"))
        .args(args)
        .output()
        .expect("the vortia binary runs")
}

#[test]
fn a_case_goes_to_fluent_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let case = dir.path().join("case");
    let back = dir.path().join("back");
    let msh = dir.path().join("box.msh");
    let raw = blocks::structured_box_raw(
        Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(2.0, 1.0, 1.0),
        },
        [4, 2, 2],
    )
    .unwrap();
    raw.write_case(&case).unwrap();

    let (case_s, msh_s, back_s) = (
        case.to_str().unwrap(),
        msh.to_str().unwrap(),
        back.to_str().unwrap(),
    );
    let out = vortia(&[
        "foamtofluent",
        "-type",
        "xMin=velocity-inlet",
        msh_s,
        case_s,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The five patches it had to guess are named; the one the person typed is not.
    let note = String::from_utf8_lossy(&out.stderr);
    assert!(note.contains("`xMax` as pressure-outlet"), "{note}");
    assert!(!note.contains("`xMin`"), "{note}");

    let out = vortia(&["fluenttofoam", msh_s, back_s]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let again = RawPolyMesh::read_case(&back).unwrap();
    assert_eq!(again.points, raw.points);
    assert_eq!(again.faces.len(), raw.faces.len());
    assert_eq!(again.patches.len(), raw.patches.len());
}

#[test]
fn a_bad_call_or_a_bad_type_fails_with_a_message() {
    let out = vortia(&["foamtofluent"]);
    assert_eq!(out.status.code(), Some(2));
    let out = vortia(&["foamtofluent", "-type", "noequals", "x.msh"]);
    assert_eq!(out.status.code(), Some(2));
    let out = vortia(&["foamtofluent", "x.msh", "/no/such/case"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.is_empty());
}
