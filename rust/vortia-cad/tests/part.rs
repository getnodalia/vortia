// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use vortia_cad::part::{Feature, Join, Part, Shape};
use vortia_mesh::stl::Stl;

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

fn feature(shape: Shape, join: Join) -> Feature {
    Feature { shape, join }
}

#[test]
fn a_block_with_a_hole_through_it_is_a_closed_surface_of_the_right_volume() {
    let part = Part {
        features: vec![
            feature(
                Shape::Box {
                    origin: [0.0; 3],
                    size: [1.0; 3],
                },
                Join::Add,
            ),
            feature(
                Shape::Cylinder {
                    origin: [0.5, 0.5, -0.5],
                    radius: 0.25,
                    height: 2.0,
                },
                Join::Cut,
            ),
        ],
    };
    let surface = part.surface(0.001).unwrap();
    assert!(surface.summary().closed(), "{:?}", surface.summary());
    let exact = 1.0 - std::f64::consts::PI * 0.0625;
    let v = volume(&surface);
    assert!(
        (v - exact).abs() / exact < 0.01,
        "volume {v} against {exact}"
    );
}

#[test]
fn a_part_that_cannot_be_made_says_why() {
    assert!(Part::default().surface(0.001).is_err());
    let cut_first = Part {
        features: vec![feature(
            Shape::Sphere {
                center: [0.0; 3],
                radius: 1.0,
            },
            Join::Cut,
        )],
    };
    assert!(cut_first.surface(0.001).is_err());
    let flat = Part {
        features: vec![feature(
            Shape::Box {
                origin: [0.0; 3],
                size: [1.0, 0.0, 1.0],
            },
            Join::Add,
        )],
    };
    assert!(flat.surface(0.001).is_err());
    let apart = Part {
        features: vec![
            feature(
                Shape::Sphere {
                    center: [0.0; 3],
                    radius: 1.0,
                },
                Join::Add,
            ),
            feature(
                Shape::Sphere {
                    center: [10.0, 0.0, 0.0],
                    radius: 1.0,
                },
                Join::Keep,
            ),
        ],
    };
    assert!(apart.surface(0.001).is_err(), "nothing is left");
}

#[test]
fn an_outline_pushed_up_and_an_outline_turned_make_solids() {
    // A 2 x 1 rectangle pushed 3 up: volume 6.
    let prism = Part {
        features: vec![feature(
            Shape::Extrude {
                corners: vec![[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]],
                z: 0.0,
                height: 3.0,
            },
            Join::Add,
        )],
    };
    let s = prism.surface(0.001).unwrap();
    assert!(s.summary().closed());
    assert!((volume(&s) - 6.0).abs() < 1.0e-6, "{}", volume(&s));
    // A 1 x 2 rectangle off the axis turned a whole turn: a pipe of outer radius 2 and inner
    // radius 1, 2 long: pi (4 - 1) 2.
    let pipe = Part {
        features: vec![feature(
            Shape::Revolve {
                corners: vec![[1.0, 0.0], [2.0, 0.0], [2.0, 2.0], [1.0, 2.0]],
                center: [0.0; 3],
                angle: 2.0 * std::f64::consts::PI,
            },
            Join::Add,
        )],
    };
    let s = pipe.surface(0.001).unwrap();
    assert!(s.summary().closed());
    let exact = 6.0 * std::f64::consts::PI;
    assert!((volume(&s) - exact).abs() / exact < 0.01, "{}", volume(&s));
    // Outlines that cannot be solids are refused.
    for corners in [
        vec![],
        vec![[0.0, 0.0], [1.0, 0.0]],
        vec![[0.0, 0.0], [f64::NAN, 0.0], [1.0, 1.0]],
    ] {
        let bad = Part {
            features: vec![feature(
                Shape::Extrude {
                    corners,
                    z: 0.0,
                    height: 1.0,
                },
                Join::Add,
            )],
        };
        assert!(bad.surface(0.001).is_err());
    }
}

#[test]
fn a_wing_is_a_section_pushed_along_its_span() {
    let section = vortia_cad::part::naca4(12, 1.0, 40).unwrap();
    assert_eq!(section.first(), Some(&[0.0, 0.0]));
    assert!(section.contains(&[1.0, 0.0]));
    let wing = Part {
        features: vec![feature(
            Shape::Extrude {
                corners: section,
                z: 0.0,
                height: 2.0,
            },
            Join::Add,
        )],
    };
    let surface = wing.surface(0.001).unwrap();
    assert!(surface.summary().closed(), "{:?}", surface.summary());
    // A NACA 0012 covers 8.2 % of its chord squared.
    let v = volume(&surface).abs();
    assert!((v - 2.0 * 0.0822).abs() / (2.0 * 0.0822) < 0.02, "{v}");
}

#[test]
fn a_wing_section_that_is_not_a_naca_4_digit_is_refused() {
    use vortia_cad::part::naca4;
    assert!(naca4(10_000, 1.0, 40).is_err());
    assert!(naca4(0, 1.0, 40).is_err());
    assert!(naca4(2_400, 1.0, 40).is_err()); // 2412 is fine, 2400 has no thickness
    assert!(naca4(2_012, 1.0, 40).is_err()); // camber without a position
    assert!(naca4(12, 0.0, 40).is_err());
    assert!(naca4(12, 1.0, 3).is_err());
    assert!(naca4(2_412, 2.0, 40).is_ok());
}

#[test]
fn the_largest_outline_of_a_drawing_becomes_a_solid_in_metres() {
    // A circle of radius 10 mm drawn in millimetres, and a smaller one inside it.
    let text = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
                0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n10\n\
                0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n2\n0\nENDSEC\n0\nEOF\n";
    let drawing = vortia_cad::dxf::parse(text.as_bytes()).unwrap();
    let corners = vortia_cad::part::outline_from_drawing(&drawing).unwrap();
    assert_eq!(corners.len(), 96);
    assert!(
        corners
            .iter()
            .all(|c| ((c[0] * c[0] + c[1] * c[1]).sqrt() - 0.01).abs() < 1.0e-9)
    );
    let part = Part {
        features: vec![feature(
            Shape::Extrude {
                corners,
                z: 0.0,
                height: 0.1,
            },
            Join::Add,
        )],
    };
    let surface = part.surface(0.001).unwrap();
    let v = volume(&surface).abs();
    let exact = std::f64::consts::PI * 1.0e-4 * 0.1;
    assert!((v - exact).abs() / exact < 0.01, "{v} against {exact}");
    assert!(vortia_cad::part::outline_from_drawing(&vortia_cad::dxf::Drawing::default()).is_err());
}

#[test]
fn a_part_survives_its_file_and_a_damaged_file_is_refused() {
    let part = Part {
        features: vec![
            feature(
                Shape::Extrude {
                    corners: vortia_cad::part::naca4(2412, 1.0, 20).unwrap(),
                    z: 0.0,
                    height: 2.0,
                },
                Join::Add,
            ),
            feature(
                Shape::Cylinder {
                    origin: [0.5, 0.0, -0.1],
                    radius: 0.05,
                    height: 3.0,
                },
                Join::Cut,
            ),
            feature(
                Shape::Revolve {
                    corners: vec![[0.1, 0.0], [0.2, 0.0], [0.2, 0.5]],
                    center: [0.0, 0.0, 1.0],
                    angle: 1.0 / 3.0,
                },
                Join::Keep,
            ),
        ],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wing.vortia-part");
    part.save(&path).unwrap();
    assert_eq!(Part::open(&path).unwrap(), part);
    for damaged in [
        "{}",
        r#"{"format":"vortia-part","features":[{"shape":"box","join":"add","origin":[0,0],"size":[1,1,1]}]}"#,
        r#"{"format":"vortia-part","features":[{"shape":"blob","join":"add"}]}"#,
        r#"{"format":"vortia-part","features":[{"shape":"sphere","join":"melt","center":[0,0,0],"radius":1}]}"#,
        r#"{"format":"vortia-part","features":[{"shape":"sphere","join":"add","center":[0,0,0],"radius":"big"}]}"#,
        "not json",
    ] {
        std::fs::write(&path, damaged).unwrap();
        assert!(Part::open(&path).is_err(), "{damaged}");
    }
    assert!(Part::open(&dir.path().join("missing")).is_err());
}

fn unit_box() -> Feature {
    feature(
        Shape::Box {
            origin: [0.0; 3],
            size: [1.0; 3],
        },
        Join::Add,
    )
}

#[test]
fn a_rounded_and_a_bevelled_edge_take_off_the_closed_form_volume() {
    // The top front edge of a unit cube, along x at y = 0, z = 1.
    let round = Part {
        features: vec![
            unit_box(),
            feature(
                Shape::Round {
                    points: vec![[0.5, 0.0, 1.0]],
                    size: 0.1,
                },
                Join::Add,
            ),
        ],
    };
    let s = round.surface(0.0005).unwrap();
    assert!(s.summary().closed(), "{:?}", s.summary());
    let exact = 1.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 0.01;
    assert!(
        (volume(&s) - exact).abs() < 2.0e-4,
        "{} against {exact}",
        volume(&s)
    );

    let chamfer = Part {
        features: vec![
            unit_box(),
            feature(
                Shape::Chamfer {
                    points: vec![[0.5, 0.0, 1.0]],
                    size: 0.1,
                },
                Join::Add,
            ),
        ],
    };
    let s = chamfer.surface(0.0005).unwrap();
    assert!(s.summary().closed());
    assert!(
        (volume(&s) - (1.0 - 0.005)).abs() < 1.0e-6,
        "{}",
        volume(&s)
    );
}

#[test]
fn a_round_with_nothing_to_round_or_no_edge_there_says_so() {
    let at = |points: Vec<[f64; 3]>, size: f64| {
        Part {
            features: vec![
                unit_box(),
                feature(Shape::Round { points, size }, Join::Add),
            ],
        }
        .surface(0.001)
    };
    // Far from every edge, none given, and a size that is not a length.
    let e = at(vec![[0.5, 0.5, 0.5]], 0.1).unwrap_err().to_string();
    assert!(e.contains("not within a millimetre"), "{e}");
    assert!(at(vec![], 0.1).is_err());
    assert!(at(vec![[0.5, 0.0, 1.0]], -0.1).is_err());
    assert!(at(vec![[0.5, 0.0, 1.0]], f64::NAN).is_err());
    // A round larger than the cube allows is refused by the kernel, not a crash.
    assert!(at(vec![[0.5, 0.0, 1.0]], 5.0).is_err());
    // Rounding before there is anything.
    let first = Part {
        features: vec![feature(
            Shape::Round {
                points: vec![[0.0; 3]],
                size: 0.1,
            },
            Join::Add,
        )],
    };
    assert!(first.surface(0.001).is_err());
}

#[test]
fn rounds_are_kept_in_a_part_file() {
    let part = Part {
        features: vec![
            unit_box(),
            feature(
                Shape::Round {
                    points: vec![[0.5, 0.0, 1.0], [0.5, 1.0, 1.0]],
                    size: 0.05,
                },
                Join::Add,
            ),
            feature(
                Shape::Chamfer {
                    points: vec![[1.0, 0.5, 1.0]],
                    size: 0.02,
                },
                Join::Add,
            ),
        ],
    };
    assert_eq!(Part::from_json(&part.to_json()).unwrap(), part);
}

#[test]
fn a_part_written_as_step_reads_back_the_same_size_in_metres() {
    // A 2 x 1 x 0.5 m block with a 0.2 m round hole through it.
    let part = Part {
        features: vec![
            feature(
                Shape::Box {
                    origin: [0.0; 3],
                    size: [2.0, 1.0, 0.5],
                },
                Join::Add,
            ),
            feature(
                Shape::Cylinder {
                    origin: [1.0, 0.5, -0.1],
                    radius: 0.1,
                    height: 0.7,
                },
                Join::Cut,
            ),
        ],
    };
    let text = part.to_step("block").unwrap();
    assert!(text.starts_with("ISO-10303-21;"));
    assert!(
        text.contains("MILLI"),
        "STEP says its unit is the millimetre"
    );

    let read = vortia_cad::step::parse(text.as_bytes(), 0.001).unwrap();
    assert_eq!(read.solids, 1);
    assert!(read.surface.summary().closed());
    let expected = 2.0 * 1.0 * 0.5 - std::f64::consts::PI * 0.1 * 0.1 * 0.5;
    let got = volume(&read.surface);
    assert!(
        (got - expected).abs() / expected < 0.01,
        "volume {got} against {expected}"
    );
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for v in &read.surface.vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    for (k, size) in [2.0, 1.0, 0.5].into_iter().enumerate() {
        assert!(
            (hi[k] - lo[k] - size).abs() < 1e-6,
            "axis {k}: {lo:?} {hi:?}"
        );
    }
}

#[test]
fn a_step_file_is_written_to_disk_and_an_empty_part_is_refused() {
    let dir = std::env::temp_dir().join(format!("vortia-step-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cube.step");
    Part {
        features: vec![unit_box()],
    }
    .save_step(&path)
    .unwrap();
    let read = vortia_cad::load(&path).unwrap();
    assert!((volume(&read) - 1.0).abs() < 1e-6);
    assert!(!dir.join("cube.step.tmp").exists());
    std::fs::remove_dir_all(&dir).unwrap();

    assert!(Part { features: vec![] }.to_step("nothing").is_err());
}

#[test]
fn a_sketched_outline_becomes_a_solid_in_metres_and_an_open_one_is_refused() {
    // 40 x 20 mm with one corner rounded to 5 mm, drawn clockwise-agnostic and in millimetres.
    let mut sketch = vortia_sketch::Sketch::default();
    sketch
        .draw_rectangle((0.0, 0.0), (40.0, 20.0), 1.0e-6)
        .unwrap();
    let corner = sketch.point_near(40.0, 20.0, 1.0e-6).unwrap();
    sketch.fillet(corner, 5.0).unwrap();
    let corners = vortia_cad::part::outline_from_sketch(&sketch).unwrap();
    // Counter-clockwise, in metres.
    let twice: f64 = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(p, q)| p[0] * q[1] - q[0] * p[1])
        .sum();
    assert!(twice > 0.0);
    assert!(
        corners
            .iter()
            .all(|c| c[0] <= 0.040 + 1e-9 && c[1] <= 0.020 + 1e-9)
    );
    let part = Part {
        features: vec![feature(
            Shape::Extrude {
                corners,
                z: 0.0,
                height: 0.1,
            },
            Join::Add,
        )],
    };
    let surface = part.surface(0.0005).unwrap();
    let v = volume(&surface).abs();
    // The rectangle less what the round takes off the corner: r^2 (1 - pi/4).
    let area = 0.040 * 0.020 - 0.005 * 0.005 * (1.0 - std::f64::consts::FRAC_PI_4);
    assert!(
        (v - area * 0.1).abs() / (area * 0.1) < 0.01,
        "{v} against {}",
        area * 0.1
    );

    // A line that does not close is not an outline.
    let mut open = vortia_sketch::Sketch::default();
    open.draw_line((0.0, 0.0), (10.0, 0.0), 1.0e-6).unwrap();
    assert!(vortia_cad::part::outline_from_sketch(&open).is_err());
    assert!(vortia_cad::part::outline_from_sketch(&vortia_sketch::Sketch::default()).is_err());
}
