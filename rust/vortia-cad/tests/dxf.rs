// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::fmt::Write as _;

use vortia_cad::dxf::{self, BODY_PATCH, Outline, Tunnel};
use vortia_cases::{Drawn, Geometry, create};
use vortia_mesh::{MeshGeometry, PolyMesh, check};

/// A DXF drawing with these entities (each already written as group code and value lines) and
/// the given `$INSUNITS`.
fn drawing(units: i64, entities: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{units}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(layer: &str, a: (f64, f64), b: (f64, f64)) -> String {
    format!(
        "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
        a.0, a.1, b.0, b.1
    )
}

fn lwpolyline(layer: &str, closed: bool, vertices: &[(f64, f64, f64)]) -> String {
    let mut s = format!(
        "0\nLWPOLYLINE\n8\n{layer}\n90\n{}\n70\n{}\n",
        vertices.len(),
        i32::from(closed)
    );
    for (x, y, bulge) in vertices {
        let _ = write!(s, "10\n{x}\n20\n{y}\n42\n{bulge}\n");
    }
    s
}

fn circle(layer: &str, c: (f64, f64), r: f64) -> String {
    format!("0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n40\n{r}\n", c.0, c.1)
}

fn arc(layer: &str, c: (f64, f64), r: f64, from: f64, to: f64) -> String {
    format!(
        "0\nARC\n8\n{layer}\n10\n{}\n20\n{}\n40\n{r}\n50\n{from}\n51\n{to}\n",
        c.0, c.1
    )
}

fn loop_edges(outline: &Outline) -> &Vec<dxf::Edge> {
    match outline {
        Outline::Loop(edges) => edges,
        Outline::Circle { .. } => panic!("not a loop"),
    }
}

#[test]
fn lines_given_in_any_order_and_direction_join_into_one_closed_loop() {
    let text = drawing(
        4,
        &[
            line("0", (0.0, 0.0), (10.0, 0.0)),
            line("0", (10.0, 10.0), (10.0, 0.0)),
            line("0", (10.0, 10.0), (0.0, 10.0)),
            line("0", (0.0, 10.0), (0.0, 0.0)),
        ]
        .concat(),
    );
    let drawing = dxf::parse(text.as_bytes()).unwrap();
    assert_eq!(drawing.outlines.len(), 1);
    let edges = loop_edges(&drawing.outlines[0]);
    assert_eq!(edges.len(), 4);
    // Every edge starts where the one before ended, and the last closes on the first.
    for i in 0..4 {
        let end = edges[i].points.last().unwrap();
        let start = edges[(i + 1) % 4].points.first().unwrap();
        assert_eq!(end, start);
    }
    assert!(drawing.notes.is_empty(), "{:?}", drawing.notes);
    assert_eq!(drawing.bounds().unwrap(), ((0.0, 0.0), (10.0, 10.0)));
}

#[test]
fn the_file_unit_is_honoured_and_a_missing_one_is_said() {
    let square = [
        line("0", (0.0, 0.0), (1.0, 0.0)),
        line("0", (1.0, 0.0), (1.0, 1.0)),
        line("0", (1.0, 1.0), (0.0, 1.0)),
        line("0", (0.0, 1.0), (0.0, 0.0)),
    ]
    .concat();
    let inches = dxf::parse(drawing(1, &square).as_bytes()).unwrap();
    assert_eq!(inches.bounds().unwrap().1, (25.4, 25.4));
    let metres = dxf::parse(drawing(6, &square).as_bytes()).unwrap();
    assert_eq!(metres.bounds().unwrap().1, (1000.0, 1000.0));
    let none = dxf::parse(drawing(0, &square).as_bytes()).unwrap();
    assert!(none.notes.iter().any(|n| n.contains("no unit")));
    let mut scaled = none;
    scaled.scale(1000.0).unwrap();
    assert_eq!(scaled.bounds().unwrap().1, (1000.0, 1000.0));
    assert!(scaled.scale(0.0).is_err());
}

#[test]
fn a_polyline_bulge_is_an_arc_of_the_right_radius() {
    // A stadium: two straight runs and two half circles of radius 5 (a bulge of 1 is a half circle).
    let text = drawing(
        4,
        &lwpolyline(
            "0",
            true,
            &[
                (0.0, 0.0, 0.0),
                (20.0, 0.0, 1.0),
                (20.0, 10.0, 0.0),
                (0.0, 10.0, 1.0),
            ],
        ),
    );
    let drawing = dxf::parse(text.as_bytes()).unwrap();
    let edges = loop_edges(&drawing.outlines[0]);
    let points: Vec<_> = edges
        .iter()
        .flat_map(|e| e.points.iter().copied())
        .collect();
    for (x, y) in &points {
        let on_arc = |cx: f64, cy: f64| ((x - cx).hypot(y - cy) - 5.0).abs() < 1e-9;
        let on_run = y.abs() < 1e-9 || (y - 10.0).abs() < 1e-9;
        assert!(
            on_run || on_arc(20.0, 5.0) || on_arc(0.0, 5.0),
            "({x}, {y})"
        );
    }
    // The half circle on the right bulges out to x = 25, the other to x = -5.
    let (lo, hi) = drawing.bounds().unwrap();
    assert!((hi.0 - 25.0).abs() < 1e-9 && (lo.0 + 5.0).abs() < 1e-9);
}

#[test]
fn two_half_circles_of_arcs_make_a_loop_and_a_circle_stays_whole() {
    let text = drawing(
        4,
        &[
            arc("0", (0.0, 0.0), 10.0, 0.0, 180.0),
            arc("0", (0.0, 0.0), 10.0, 180.0, 360.0),
            circle("0", (50.0, 0.0), 5.0),
        ]
        .concat(),
    );
    let drawing = dxf::parse(text.as_bytes()).unwrap();
    assert_eq!(drawing.outlines.len(), 2);
    assert!(
        drawing
            .outlines
            .iter()
            .any(|o| matches!(o, Outline::Circle { radius, .. } if (*radius - 5.0).abs() < 1e-12))
    );
    let ring = drawing
        .outlines
        .iter()
        .find_map(|o| match o {
            Outline::Loop(e) => Some(e),
            Outline::Circle { .. } => None,
        })
        .unwrap();
    assert_eq!(ring.len(), 2);
    assert!(
        ring.iter()
            .flat_map(|e| &e.points)
            .all(|(x, y)| (x.hypot(*y) - 10.0).abs() < 1e-9)
    );
}

#[test]
fn an_outline_that_does_not_close_says_where_it_is_open() {
    let text = drawing(
        4,
        &[
            line("0", (0.0, 0.0), (10.0, 0.0)),
            line("0", (10.0, 0.0), (10.0, 10.0)),
            line("0", (10.0, 10.0), (0.0, 10.0)),
        ]
        .concat(),
    );
    let err = dxf::parse(text.as_bytes()).unwrap_err().to_string();
    assert!(err.contains("open near"), "{err}");
    let nothing =
        dxf::parse(drawing(4, "0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n2\n1\nhello\n").as_bytes())
            .unwrap_err()
            .to_string();
    assert!(nothing.contains("closed outline"), "{nothing}");
    assert!(dxf::parse(b"this is not a drawing").is_err());
}

#[test]
fn a_spline_is_sampled_along_its_curve() {
    // A quadratic Bezier from (0,0) over (5,10) to (10,0), as a spline of degree 2: its middle is (5,5).
    let spline = "0\nSPLINE\n8\n0\n70\n8\n71\n2\n72\n6\n73\n3\n\
        40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n\
        10\n0\n20\n0\n10\n5\n20\n10\n10\n10\n20\n0\n";
    let closing = line("0", (10.0, 0.0), (0.0, 0.0));
    let drawing = dxf::parse(drawing(4, &format!("{spline}{closing}")).as_bytes()).unwrap();
    let edges = loop_edges(&drawing.outlines[0]);
    let curve = edges
        .iter()
        .find(|e| e.points.len() > 10)
        .expect("the spline is cut into many pieces");
    let top = curve.points.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    assert!((top - 5.0).abs() < 0.05, "the curve peaks at {top}");
    assert_eq!(*curve.points.first().unwrap(), (0.0, 0.0));
    assert_eq!(*curve.points.last().unwrap(), (10.0, 0.0));
}

#[test]
fn layers_name_the_patches_of_a_drawn_region() {
    let text = drawing(
        4,
        &[
            line("Inlet", (0.0, 0.0), (0.0, 20.0)),
            line("Wall", (0.0, 20.0), (100.0, 20.0)),
            line("OUTLET", (100.0, 20.0), (100.0, 0.0)),
            line("0", (100.0, 0.0), (0.0, 0.0)),
        ]
        .concat(),
    );
    let sketch = dxf::parse(text.as_bytes()).unwrap().sketch().unwrap();
    let names: Vec<&str> = sketch.names.values().map(String::as_str).collect();
    assert_eq!(names, ["inlet", "wall", "outlet", "wall"]);

    let dir = tempfile::tempdir().unwrap();
    let drawn = Drawn {
        sketch,
        cell_size: 4.0,
        ..Drawn::default()
    };
    create(dir.path(), &Geometry::Drawn(drawn)).unwrap();
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    for name in ["inlet", "outlet", "wall"] {
        assert!(mesh.patch(name).is_some(), "no '{name}'");
    }
}

#[test]
fn a_body_is_set_in_a_wind_tunnel_and_the_case_is_meshed_around_it() {
    // A cylinder of 20 mm and a lens-shaped section beside it.
    let text = drawing(
        4,
        &[
            circle("0", (0.0, 0.0), 10.0),
            lwpolyline(
                "0",
                true,
                &[
                    (40.0, 0.0, 0.0),
                    (60.0, -5.0, 0.0),
                    (80.0, 0.0, 0.0),
                    (60.0, 5.0, 0.0),
                ],
            ),
        ]
        .concat(),
    );
    let mut drawing = dxf::parse(text.as_bytes()).unwrap();
    let sketch = drawing
        .around(Tunnel {
            upstream: 2.0,
            downstream: 4.0,
            lateral: 2.0,
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let drawn = Drawn {
        sketch,
        cell_size: 3.0,
        far_cell_size: Some(20.0),
        ..Drawn::default()
    };
    create(dir.path(), &Geometry::Drawn(drawn)).unwrap();
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let geo = MeshGeometry::compute(&mesh);
    assert!(check(&mesh, &geo).is_ok());
    for name in ["inlet", "outlet", "symmetry", BODY_PATCH] {
        assert!(mesh.patch(name).is_some(), "no '{name}'");
    }
    assert_eq!(mesh.patch(BODY_PATCH).unwrap().kind.name(), "wall");
    // The bodies span 90 by 20 mm, so the tunnel is 90 mm · (2 + 1 + 4) long and 20 mm + 2 · 2 · 90
    // mm high, 10 mm deep, less the cylinder and the lens.
    let tunnel = (90.0 * 7.0) * (20.0 + 4.0 * 90.0) * 10.0 * 1e-9;
    let bodies = (std::f64::consts::PI * 100.0 + 0.5 * 40.0 * 10.0) * 10.0 * 1e-9;
    let volume = geo.total_volume();
    assert!(
        (volume - (tunnel - bodies)).abs() < 0.02 * tunnel,
        "the air is {volume} m³, the tunnel less the bodies {}",
        tunnel - bodies
    );
}

#[test]
fn a_drawing_with_more_corners_than_a_sketch_holds_is_thinned_and_says_so() {
    let n = 900;
    let vertices: Vec<(f64, f64, f64)> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / f64::from(n);
            (30.0 * a.cos(), 20.0 * a.sin(), 0.0)
        })
        .collect();
    let mut drawing = dxf::parse(drawing(4, &lwpolyline("0", true, &vertices)).as_bytes()).unwrap();
    let sketch = drawing.around(Tunnel::default()).unwrap();
    assert!(sketch.points.len() <= vortia_sketch::MAX_POINTS);
    assert!(
        drawing.notes.iter().any(|n| n.contains("simplified")),
        "{:?}",
        drawing.notes
    );
    // Within a fraction of a millimetre of the true ellipse.
    let (lo, hi) = drawing.bounds().unwrap();
    assert!((hi.0 - 30.0).abs() < 0.1 && (lo.1 + 20.0).abs() < 0.1);
}
