// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use vortia_cad::step;
use vortia_mesh::stl::Stl;

const CYLINDER_MM: &str = include_str!("data/cylinder-mm.step");

/// The volume a closed surface encloses.
fn volume(s: &Stl) -> f64 {
    s.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| s.vertices[i as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn a_step_cylinder_is_a_closed_surface_in_metres() {
    let read = step::parse(CYLINDER_MM.as_bytes(), step::DEFAULT_CHORD).unwrap();
    assert_eq!(read.solids, 1);
    assert!(
        read.surface.summary().closed(),
        "{:?}",
        read.surface.summary()
    );
    // A cylinder of radius 1 mm and height 2 mm: 2 pi mm^3 = 2 pi 1e-9 m^3.
    let exact = 2.0 * std::f64::consts::PI * 1.0e-9;
    let v = volume(&read.surface);
    assert!(
        (v - exact).abs() / exact < 0.01,
        "volume {v} against {exact}"
    );
}

#[test]
fn the_length_unit_of_the_file_is_honoured() {
    let metres = CYLINDER_MM.replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT($,.METRE.)");
    assert_ne!(metres, CYLINDER_MM);
    let read = step::parse(metres.as_bytes(), step::DEFAULT_CHORD).unwrap();
    let exact = 2.0 * std::f64::consts::PI;
    let v = volume(&read.surface);
    assert!(
        (v - exact).abs() / exact < 0.01,
        "volume {v} against {exact}"
    );
}

#[test]
fn what_is_not_a_step_file_is_refused() {
    for text in [
        "",
        "hello",
        "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n",
    ] {
        assert!(
            step::parse(text.as_bytes(), step::DEFAULT_CHORD).is_err(),
            "{text:?}"
        );
    }
    assert!(step::parse(&[0xff, 0xfe, 0x00], step::DEFAULT_CHORD).is_err());
    for chord in [0.0, -1.0, f64::NAN, 2.0] {
        assert!(step::parse(CYLINDER_MM.as_bytes(), chord).is_err());
    }
}

#[test]
fn load_picks_the_reader_by_extension() {
    let dir = std::env::temp_dir().join(format!("vortia-cad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.STP");
    std::fs::write(&path, CYLINDER_MM).unwrap();
    let surface = vortia_cad::load(&path).unwrap();
    assert!(surface.summary().closed());
    assert!(vortia_cad::load(&dir.join("missing.step")).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
