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
