//! The `vortia sketch` and `vortia new sketch` commands, run as the binary a user runs.

use std::process::Command;

fn vortia(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vortia"))
        .args(args)
        .output()
        .expect("the vortia binary runs")
}

#[test]
fn a_sketch_template_is_inspected_and_meshed_into_a_case() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let file = dir.path().join("nozzle.vortia-sketch");
    let file = file.to_str().expect("a utf-8 path");
    let case = dir.path().join("case");
    let case = case.to_str().expect("a utf-8 path");

    let made = vortia(&["sketch", "template", "rounded-nozzle", file]);
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );

    let info = vortia(&["sketch", "info", file]);
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    assert!(!info.stdout.is_empty());

    let new = vortia(&[
        "new",
        "sketch",
        case,
        file,
        "axisymmetric",
        "segments=12",
        "radial-cells=8",
        "cells-per-segment=2",
    ]);
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    assert!(
        std::path::Path::new(case)
            .join("system/controlDict")
            .is_file()
    );
}

#[test]
fn a_bad_sketch_command_is_refused_with_a_message() {
    let unknown = vortia(&["sketch", "template", "teapot", "/nonexistent/x.json"]);
    assert!(!unknown.status.success());
    assert!(!unknown.stderr.is_empty());

    let missing = vortia(&["sketch", "info", "/nonexistent/x.json"]);
    assert!(!missing.status.success());
}
