use crate::{Constraint, Sketch, SketchError, stations, templates};

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn a_duct_is_one_straight_wall() {
    let mut s = templates::duct(500.0, 25.0);
    s.solve().unwrap();
    let p = s.profile(5.0).unwrap();
    assert_eq!(p, vec![(0.0, 25.0), (500.0, 25.0)]);
}

#[test]
fn the_dimensions_of_a_nozzle_pull_scattered_points_into_place() {
    let mut s = templates::nozzle(50.0, 26.0, 60.0, 158.0, 420.0);
    // Start somewhere else entirely.
    for (i, p) in s.points.iter_mut().enumerate() {
        if !p.fixed {
            p.x = 17.0 * (i as f64 + 1.0);
            p.y = 3.0 + i as f64;
        }
    }
    let solution = s.solve().unwrap();
    assert!(solution.residual < 1.0e-9, "{solution:?}");
    let p = s.profile(5.0).unwrap();
    assert_eq!(p.len(), 3);
    assert!(near(p[1].0, 158.0, 1e-6) && near(p[1].1, 26.0, 1e-6));
    assert!(near(p[2].0, 578.0, 1e-6) && near(p[2].1, 60.0, 1e-6));
}

#[test]
fn a_rounded_throat_has_the_radius_and_the_height_the_dimensions_give() {
    let mut s = templates::rounded_nozzle(50.0, 26.0, 60.0, 158.0, 420.0, 30.0);
    s.solve().unwrap();
    let p = s.profile(0.5).unwrap();
    assert!(p.len() > 20);
    let throat = p.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    assert!(near(throat, 26.0, 1.0e-3), "{throat}");
    assert!(p.windows(2).all(|w| w[1].0 > w[0].0));
}

#[test]
fn dimensions_that_contradict_each_other_are_said_so_and_leave_the_sketch_alone() {
    let mut s = templates::duct(500.0, 25.0);
    let before = s.clone();
    s.constrain(Constraint::DeltaX {
        a: 1,
        b: 2,
        value: 400.0,
    });
    let err = s.solve().unwrap_err();
    assert!(matches!(err, SketchError::Unsolved(_)), "{err}");
    assert_eq!(s, before_with_extra(before));
}

fn before_with_extra(mut s: Sketch) -> Sketch {
    s.constrain(Constraint::DeltaX {
        a: 1,
        b: 2,
        value: 400.0,
    });
    s
}

#[test]
fn a_wall_must_be_one_open_chain_above_the_axis_with_x_rising() {
    let chain = |points: &[(f64, f64)], lines: &[(usize, usize)]| {
        let mut s = Sketch::default();
        for &(x, y) in points {
            s.point(x, y);
        }
        for &(a, b) in lines {
            s.line(a, b);
        }
        s.profile(5.0)
    };
    assert!(chain(&[(0.0, 1.0), (1.0, 1.0)], &[(0, 1)]).is_ok());
    // A loop.
    let err = chain(
        &[(0.0, 1.0), (1.0, 1.0), (1.0, 2.0)],
        &[(0, 1), (1, 2), (2, 0)],
    );
    assert!(matches!(err, Err(SketchError::Profile(_))));
    // A branch.
    let err = chain(
        &[(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (1.0, 2.0)],
        &[(0, 1), (1, 2), (1, 3)],
    );
    assert!(matches!(err, Err(SketchError::Profile(_))));
    // Two separate pieces.
    let err = chain(
        &[(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 1.0)],
        &[(0, 1), (2, 3)],
    );
    assert!(matches!(err, Err(SketchError::Profile(_))));
    // x turns back.
    let err = chain(&[(0.0, 1.0), (2.0, 1.0), (1.0, 1.0)], &[(0, 1), (1, 2)]);
    assert!(err.unwrap_err().to_string().contains("x always rising"));
    // Below the axis.
    let err = chain(&[(0.0, 1.0), (1.0, -1.0)], &[(0, 1)]);
    assert!(err.unwrap_err().to_string().contains("above the axis"));
    // Nothing drawn.
    assert!(Sketch::default().profile(5.0).is_err());
}

#[test]
fn the_stations_are_even_along_the_wall_and_keep_both_ends() {
    let p = vec![(0.0, 1.0), (3.0, 1.0), (3.0, 5.0)];
    let s = stations(&p, 7).unwrap();
    assert_eq!(s.len(), 8);
    assert_eq!(s[0], (0.0, 1.0));
    assert!(near(s[7].0, 3.0, 1e-12) && near(s[7].1, 5.0, 1e-12));
    let gaps: Vec<f64> = s
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .collect();
    // Seven stations across 7 units of length: every gap one unit except where the corner cuts.
    assert!(
        gaps.iter().all(|g| *g <= 1.0 + 1e-9 && *g > 0.7),
        "{gaps:?}"
    );
    assert!(stations(&p, 1).is_err());
    assert!(stations(&vec![(0.0, 1.0)], 4).is_err());
    assert!(stations(&vec![(0.0, 1.0), (0.0, 1.0)], 4).is_err());
}

#[test]
fn a_sketch_is_saved_and_read_back_as_it_was() {
    let mut s = templates::rounded_nozzle(50.0, 26.0, 60.0, 158.0, 420.0, 30.0);
    s.solve().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nozzle.vortia-sketch");
    s.save(&path).unwrap();
    assert_eq!(Sketch::open(&path).unwrap(), s);
}

#[test]
fn a_hostile_file_is_an_error_and_never_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.json");
    let ok = templates::duct(10.0, 1.0).to_json();
    for (what, edit) in [
        ("wrong format", r#"{"format":"x"}"#.to_owned()),
        ("not json", "{".to_owned()),
        (
            "newer",
            ok.to_string().replace("\"version\":2", "\"version\":9"),
        ),
        (
            "unknown entity",
            ok.to_string().replace("\"line\"", "\"spline\""),
        ),
        (
            "point out of range",
            ok.to_string().replace("\"b\":2", "\"b\":77"),
        ),
        (
            "huge coordinate",
            ok.to_string().replace("\"x\":10.0", "\"x\":1e300"),
        ),
        ("inches", ok.to_string().replace("\"mm\"", "\"in\"")),
    ] {
        std::fs::write(&path, edit).unwrap();
        assert!(Sketch::open(&path).is_err(), "{what}");
    }
    std::fs::write(&path, vec![b' '; 3 * 1024 * 1024]).unwrap();
    assert!(
        Sketch::open(&path)
            .unwrap_err()
            .to_string()
            .contains("too large")
    );
    assert!(Sketch::open(&dir.path().join("missing")).is_err());
}

#[test]
fn numbers_that_cannot_be_dimensions_are_refused() {
    let mut s = templates::duct(10.0, 1.0);
    s.constrain(Constraint::Distance {
        a: 1,
        b: 2,
        value: f64::NAN,
    });
    assert!(s.validate().is_err());
    let mut s = templates::duct(10.0, 1.0);
    s.constrain(Constraint::Radius { arc: 0, value: 5.0 });
    assert!(s.validate().unwrap_err().to_string().contains("not an arc"));
    let mut s = templates::duct(10.0, 1.0);
    s.constrain(Constraint::Distance {
        a: 1,
        b: 2,
        value: -1.0,
    });
    assert!(s.validate().is_err());
    let mut s = Sketch::default();
    for _ in 0..=crate::MAX_POINTS {
        s.point(0.0, 1.0);
    }
    assert!(s.validate().is_err());
}

#[test]
fn an_angle_and_a_distance_are_dimensions_too() {
    let mut s = Sketch::default();
    let a = s.fixed_point(0.0, 5.0);
    let b = s.point(3.0, 6.0);
    s.line(a, b);
    s.constrain(Constraint::Distance { a, b, value: 10.0 });
    s.constrain(Constraint::Angle {
        a,
        b,
        degrees: 30.0,
    });
    s.solve().unwrap();
    let p = s.points[b];
    assert!(near(p.x, 10.0 * 30f64.to_radians().cos(), 1e-6));
    assert!(near(p.y, 5.0 + 5.0, 1e-6));
}

/// A solved sketch is a list of awkward floats, and the file has to give back the very same
/// bits: without serde_json's `float_roundtrip`, its parser is off by one in the last place for
/// some digits (found on macOS, where the nozzle template did not read back equal).
#[test]
fn a_saved_sketch_reads_back_bit_for_bit() {
    let mut s = Sketch::default();
    let awkward = [
        27.378058810633842,
        149.01198116639372,
        166.98801883357694,
        0.1 + 0.2,
    ];
    for (i, x) in awkward.into_iter().enumerate() {
        s.point(x, awkward[(i + 1) % awkward.len()]);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("awkward.vortia-sketch");
    s.save(&path).unwrap();
    let back = Sketch::open(&path).unwrap();
    assert_eq!(back, s);
}

fn square(s: &mut Sketch, x: f64, y: f64, side: f64) -> [usize; 4] {
    let p: Vec<usize> = [(x, y), (x + side, y), (x + side, y + side), (x, y + side)]
        .iter()
        .map(|&(a, b)| s.point(a, b))
        .collect();
    [
        s.line(p[0], p[1]),
        s.line(p[1], p[2]),
        s.line(p[2], p[3]),
        s.line(p[3], p[0]),
    ]
}

#[test]
fn a_closed_sketch_reads_as_loops_with_the_names_of_its_edges() {
    let mut s = Sketch::default();
    let outer = square(&mut s, 0.0, 0.0, 10.0);
    let hole = square(&mut s, 4.0, 4.0, 2.0);
    s.name(outer[0], "bottom");
    s.name(outer[3], "inlet");
    s.name(hole[0], "cylinder");
    let loops = s.loops(5.0).unwrap();
    assert_eq!(loops.len(), 2);
    assert_eq!(loops[0].len(), 4);
    assert_eq!(loops[1].len(), 4);
    let names: Vec<&str> = loops[0].iter().map(|e| e.patch.as_str()).collect();
    assert_eq!(names, ["bottom", "wall", "wall", "inlet"]);
    // Each edge starts where the one before it ended.
    for l in &loops {
        for (i, e) in l.iter().enumerate() {
            let before = &l[(i + l.len() - 1) % l.len()];
            assert_eq!(before.points.last(), e.points.first());
        }
    }
    // A name survives the file.
    let back = Sketch::from_json(&s.to_json()).unwrap();
    assert_eq!(back.names, s.names);
}

#[test]
fn an_outline_with_an_open_end_a_stray_hole_or_a_bad_name_is_refused() {
    let mut open = Sketch::default();
    let a = open.point(0.0, 0.0);
    let b = open.point(1.0, 0.0);
    open.line(a, b);
    assert!(matches!(open.loops(5.0), Err(SketchError::Profile(_))));

    let mut apart = Sketch::default();
    square(&mut apart, 0.0, 0.0, 1.0);
    square(&mut apart, 5.0, 5.0, 1.0);
    let err = apart.loops(5.0).unwrap_err().to_string();
    assert!(err.contains("outside"), "{err}");

    let mut named = Sketch::default();
    let e = square(&mut named, 0.0, 0.0, 1.0);
    named.name(e[0], "has space");
    assert!(named.loops(5.0).is_err());
    named.names.clear();
    named.name(99, "x");
    assert!(named.validate().is_err());
}

mod editing {
    use crate::{Constraint, Measure, Sketch, templates};

    fn square() -> Sketch {
        let mut s = Sketch::default();
        for (a, b) in [
            ((0.0, 0.0), (100.0, 0.0)),
            ((100.0, 0.0), (100.0, 100.0)),
            ((100.0, 100.0), (0.0, 100.0)),
            ((0.0, 100.0), (0.0, 0.0)),
        ] {
            s.draw_line(a, b, 1.0).unwrap();
        }
        s
    }

    #[test]
    fn lines_drawn_end_to_end_share_their_points_and_close_into_a_region() {
        let s = square();
        assert_eq!((s.points.len(), s.entities.len()), (4, 4));
        let loops = s.loops(5.0).unwrap();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 4);
    }

    #[test]
    fn a_refused_edit_leaves_the_sketch_as_it_was() {
        let mut s = square();
        let before = s.clone();
        assert!(
            s.draw_line((0.0, 0.0), (0.2, 0.2), 1.0).is_err(),
            "one point twice"
        );
        assert!(s.draw_line((f64::NAN, 0.0), (1.0, 1.0), 1.0).is_err());
        assert!(s.move_point(99, 0.0, 0.0).is_err());
        assert!(s.remove_edge(99).is_err());
        assert!(s.dimension_points(0, 0, Measure::Distance).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn an_arc_ends_on_its_circle_towards_the_pointer() {
        let mut s = Sketch::default();
        let arc = s
            .draw_arc((0.0, 0.0), (10.0, 0.0), (0.0, 50.0), 0.5)
            .unwrap();
        let crate::Entity::Arc { end, .. } = s.entities[arc] else {
            panic!("not an arc");
        };
        let e = s.points[end];
        assert!((e.x).abs() < 1e-9 && (e.y - 10.0).abs() < 1e-9, "{e:?}");
        assert!(
            s.draw_arc((0.0, 0.0), (10.0, 0.0), (0.0, 0.0), 0.5)
                .is_err()
        );
        assert!(s.draw_arc((0.0, 0.0), (0.0, 0.0), (5.0, 5.0), 0.5).is_err());
    }

    #[test]
    fn a_dragged_point_goes_where_it_is_taken_unless_a_dimension_holds_it() {
        let mut s = square();
        // Free: it follows the pointer (the corner at (100, 100)).
        let corner = s.point_near(100.0, 100.0, 1.0).unwrap();
        assert_eq!(s.move_point(corner, 120.0, 90.0).unwrap(), (120.0, 90.0));
        // Held at a height of 90 above the corner at (0, 0), which stays put, it comes back to it.
        let origin = s.point_near(0.0, 0.0, 1.0).unwrap();
        s.points[origin].fixed = true;
        s.dimension_points(origin, corner, Measure::Height).unwrap();
        let (x, y) = s.move_point(corner, 130.0, 40.0).unwrap();
        assert!((y - 90.0).abs() < 1e-6 && x > 100.0, "({x}, {y})");
        // A fixed point does not move at all.
        assert!(s.move_point(origin, 5.0, 5.0).is_err());
    }

    #[test]
    fn deleting_an_edge_renumbers_names_and_dimensions_and_drops_orphans() {
        let mut s = templates::channel_with_block(400.0, 100.0, 30.0, 120.0, 35.0);
        let before = s.entities.len();
        // The block's four edges are the last ones, all called `block`.
        let last = s.entities.len() - 1;
        s.remove_edge(0).unwrap();
        assert_eq!(s.entities.len(), before - 1);
        assert_eq!(s.names.get(&(last - 1)).map(String::as_str), Some("block"));
        assert!(!s.names.contains_key(&last));
        assert!(s.validate().is_ok());
        // Remove the rest of the block: its points and dimensions go with it.
        for _ in 0..4 {
            let i = s.entities.len() - 1;
            s.remove_edge(i).unwrap();
        }
        assert!(s.points.iter().all(|p| p.x <= 400.0));
        assert!(s.constraints.iter().all(|c| {
            c.map_points(|p| (p < s.points.len()).then_some(p))
                .is_some()
                && c.map_edges(|e| (e < s.entities.len()).then_some(e))
                    .is_some()
        }));
    }

    #[test]
    fn the_edge_under_the_pointer_is_found_and_a_dimension_takes_the_measure_it_has() {
        let mut s = square();
        assert_eq!(s.entity_near(50.0, 1.5, 3.0), Some(0));
        assert_eq!(s.entity_near(50.0, 50.0, 3.0), None);
        let (a, b) = (
            s.point_near(0.0, 0.0, 1.0).unwrap(),
            s.point_near(100.0, 0.0, 1.0).unwrap(),
        );
        let i = s.dimension_points(a, b, Measure::Distance).unwrap();
        let Constraint::Distance { value, .. } = s.constraints[i] else {
            panic!("not a distance");
        };
        assert!((value - 100.0).abs() < 1e-9);
        let arc = {
            let mut t = Sketch::default();
            let e = t
                .draw_arc((0.0, 0.0), (10.0, 0.0), (0.0, 10.0), 0.5)
                .unwrap();
            assert_eq!(t.entity_near(7.07, 7.07, 1.0), Some(e));
            t
        };
        assert_eq!(arc.entities.len(), 1);
    }

    #[test]
    fn an_unfinished_outline_can_still_be_drawn() {
        let mut s = Sketch::default();
        s.draw_line((0.0, 0.0), (10.0, 0.0), 1.0).unwrap();
        s.draw_arc((10.0, 5.0), (10.0, 0.0), (15.0, 5.0), 0.5)
            .unwrap();
        s.name(0, "inlet");
        let edges = s.edges(10.0).unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].patch, "inlet");
        assert_eq!(edges[1].patch, "wall");
        assert!(edges[1].points.len() > 2, "the arc is cut in steps");
        assert!(s.loops(5.0).is_err(), "but it is not a region yet");
    }
}

mod shapes {
    use crate::{Constraint, Entity, Measure, Sketch, templates};

    fn centre_to_line(s: &Sketch, line: usize, round: usize) -> (f64, f64) {
        let Entity::Line { a, b } = s.entities[line] else {
            panic!("not a line")
        };
        let (c, r) = match s.entities[round] {
            Entity::Arc { start, center, .. } => (center, start),
            Entity::Circle { center, rim } => (center, rim),
            Entity::Line { .. } => panic!("not round"),
        };
        let p = |i: usize| (s.points[i].x, s.points[i].y);
        let (pa, pb, pc, pr) = (p(a), p(b), p(c), p(r));
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let across = (dx * (pc.1 - pa.1) - dy * (pc.0 - pa.0)).abs() / dx.hypot(dy);
        (across, (pc.0 - pr.0).hypot(pc.1 - pr.1))
    }

    #[test]
    fn a_circle_in_a_channel_is_a_hole_and_has_a_radius_to_dimension() {
        let mut s = Sketch::default();
        s.draw_rectangle((0.0, 0.0), (400.0, 100.0), 1.0).unwrap();
        let circle = s.draw_circle((100.0, 50.0), (120.0, 50.0), 1.0).unwrap();
        s.name(circle, "cylinder");
        let loops = s.loops(10.0).unwrap();
        assert_eq!(loops.len(), 2);
        assert_eq!(loops[1].len(), 1);
        assert_eq!(loops[1][0].patch, "cylinder");
        let ring = &loops[1][0].points;
        assert_eq!(ring.first(), ring.last(), "a circle's outline closes");
        // Its radius is a dimension the solver holds.
        s.add_constraint(Constraint::Radius {
            arc: circle,
            value: 12.0,
        })
        .unwrap();
        let (cx, rim) = (
            s.points[s.entities[circle].points()[0]],
            s.points[s.entities[circle].points()[1]],
        );
        assert!(((rim.x - cx.x).hypot(rim.y - cx.y) - 12.0).abs() < 1e-6);
        // A circle is not part of an axisymmetric wall.
        assert!(s.profile(5.0).is_err());
    }

    #[test]
    fn a_rectangle_stays_a_rectangle_when_a_corner_is_dragged() {
        let mut s = Sketch::default();
        let edges = s.draw_rectangle((10.0, 20.0), (110.0, 70.0), 1.0).unwrap();
        assert_eq!(edges.len(), 4);
        assert_eq!(s.loops(10.0).unwrap().len(), 1);
        let corner = s.point_near(110.0, 70.0, 1.0).unwrap();
        s.move_point(corner, 150.0, 90.0).unwrap();
        let (x0, y0) = (s.points[0].x, s.points[0].y);
        assert!((x0 - 10.0).abs() < 1e-6 && (y0 - 20.0).abs() < 1e-6);
        assert!(
            (s.points[1].y - s.points[0].y).abs() < 1e-6,
            "the bottom stays level"
        );
        assert!(
            (s.points[2].x - s.points[1].x).abs() < 1e-6,
            "the right side stays upright"
        );
        assert!(s.draw_rectangle((5.0, 5.0), (5.0, 50.0), 1.0).is_err());
    }

    #[test]
    fn a_slot_keeps_its_ends_round_and_its_sides_tangent_as_it_is_dragged() {
        let mut s = Sketch::default();
        let slot = s.draw_slot((0.0, 0.0), (100.0, 0.0), 10.0, 1.0).unwrap();
        assert_eq!(slot.len(), 4);
        assert_eq!(s.loops(10.0).unwrap().len(), 1);
        let end = s.point_near(100.0, 0.0, 1.0).unwrap();
        s.move_point(end, 80.0, 40.0).unwrap();
        for (line, arc) in [(0, 1), (0, 2), (3, 1), (3, 2)] {
            let (across, radius) = centre_to_line(&s, line, arc);
            assert!(
                (across - radius).abs() < 1e-5,
                "{line} {arc}: {across} {radius}"
            );
        }
        let radii: Vec<f64> = [1, 2].iter().map(|&a| centre_to_line(&s, 0, a).1).collect();
        assert!((radii[0] - radii[1]).abs() < 1e-5, "{radii:?}");
        assert!(s.draw_slot((0.0, 0.0), (0.0, 0.0), 5.0, 1.0).is_err());
    }

    #[test]
    fn a_polygon_has_as_many_sides_as_asked_and_no_more_than_it_may() {
        let mut s = Sketch::default();
        let edges = s.draw_polygon((0.0, 0.0), (10.0, 0.0), 6, 0.5).unwrap();
        assert_eq!(edges.len(), 6);
        assert_eq!(s.points.len(), 6);
        assert_eq!(s.loops(10.0).unwrap().len(), 1);
        for bad in [0, 2, 65, usize::MAX] {
            assert!(
                s.draw_polygon((0.0, 0.0), (10.0, 0.0), bad, 0.5).is_err(),
                "{bad}"
            );
        }
        assert!(s.draw_polygon((0.0, 0.0), (f64::NAN, 0.0), 5, 0.5).is_err());
    }

    #[test]
    fn a_corner_is_rounded_with_an_arc_that_touches_both_lines() {
        let mut s = Sketch::default();
        s.draw_rectangle((0.0, 0.0), (100.0, 60.0), 1.0).unwrap();
        let corner = s.point_near(100.0, 60.0, 1.0).unwrap();
        let arc = s.fillet(corner, 8.0).unwrap();
        assert!(matches!(s.entities[arc], Entity::Arc { .. }));
        assert_eq!(s.entities.len(), 5);
        assert_eq!(s.loops(10.0).unwrap().len(), 1, "still one closed outline");
        let tangents: Vec<(usize, usize)> = s
            .constraints
            .iter()
            .filter_map(|c| match *c {
                Constraint::Tangent { line, round } => Some((line, round)),
                _ => None,
            })
            .collect();
        assert_eq!(tangents.len(), 2);
        for (line, round) in tangents {
            let (across, radius) = centre_to_line(&s, line, round);
            assert!((across - 8.0).abs() < 1e-5 && (radius - 8.0).abs() < 1e-5);
        }
        // The radius is a dimension that can be edited afterwards.
        let at = s
            .constraints
            .iter()
            .position(|c| matches!(c, Constraint::Radius { .. }))
            .unwrap();
        let Constraint::Radius { value, .. } = &mut s.constraints[at] else {
            panic!()
        };
        *value = 12.0;
        s.solve().unwrap();
        let (_, radius) = centre_to_line(&s, 1, arc);
        assert!((radius - 12.0).abs() < 1e-5, "{radius}");
    }

    #[test]
    fn a_corner_is_chamfered_and_what_cannot_be_is_refused() {
        let mut s = Sketch::default();
        s.draw_rectangle((0.0, 0.0), (100.0, 60.0), 1.0).unwrap();
        let corner = s.point_near(0.0, 0.0, 1.0).unwrap();
        let line = s.chamfer(corner, 10.0).unwrap();
        let Entity::Line { a, b } = s.entities[line] else {
            panic!("not a line")
        };
        let (pa, pb) = (s.points[a], s.points[b]);
        assert!(((pa.x - pb.x).hypot(pa.y - pb.y) - 10.0 * 2f64.sqrt()).abs() < 1e-6);
        assert_eq!(s.loops(10.0).unwrap().len(), 1);
        // More than the lines are long, a negative, a NaN, a point that is no corner.
        let before = s.clone();
        let other = s.point_near(100.0, 60.0, 1.0).unwrap();
        assert!(s.chamfer(other, 80.0).is_err());
        assert!(s.fillet(other, -1.0).is_err());
        assert!(s.fillet(other, f64::NAN).is_err());
        assert!(s.fillet(999, 1.0).is_err());
        assert_eq!(s, before, "a refused edit leaves the sketch alone");
    }

    #[test]
    fn constraints_are_held_and_one_that_cannot_hold_is_refused() {
        let mut s = Sketch::default();
        let l0 = s.draw_line((0.0, 0.0), (100.0, 10.0), 1.0).unwrap();
        let l1 = s.draw_line((0.0, 50.0), (80.0, 70.0), 1.0).unwrap();
        s.add_constraint(Constraint::Parallel { a: l0, b: l1 })
            .unwrap();
        let dir = |s: &Sketch, e: usize| {
            let Entity::Line { a, b } = s.entities[e] else {
                panic!()
            };
            (s.points[b].x - s.points[a].x, s.points[b].y - s.points[a].y)
        };
        let (u, v) = (dir(&s, l0), dir(&s, l1));
        assert!((u.0 * v.1 - u.1 * v.0).abs() / (u.0.hypot(u.1) * v.0.hypot(v.1)) < 1e-6);
        // Perpendicular to something already parallel to it cannot be.
        let before = s.clone();
        assert!(
            s.add_constraint(Constraint::Perpendicular { a: l0, b: l1 })
                .is_err()
        );
        assert_eq!(s, before);
        // Constraints on what is not there are refused, not crashed on.
        assert!(
            s.add_constraint(Constraint::Parallel { a: 0, b: 99 })
                .is_err()
        );
        assert!(
            s.add_constraint(Constraint::Tangent { line: 0, round: 1 })
                .is_err()
        );
        assert!(s.add_constraint(Constraint::Equal { a: 0, b: 0 }).is_err());
    }

    #[test]
    fn two_lines_made_equal_and_two_points_made_one() {
        let mut s = Sketch::default();
        let l0 = s.draw_line((0.0, 0.0), (100.0, 0.0), 1.0).unwrap();
        let l1 = s.draw_line((0.0, 20.0), (60.0, 20.0), 1.0).unwrap();
        s.add_constraint(Constraint::Equal { a: l0, b: l1 })
            .unwrap();
        let len = |s: &Sketch, e: usize| {
            let Entity::Line { a, b } = s.entities[e] else {
                panic!()
            };
            (s.points[b].x - s.points[a].x).hypot(s.points[b].y - s.points[a].y)
        };
        assert!((len(&s, l0) - len(&s, l1)).abs() < 1e-6);
        let (a, b) = (s.entities[l0].points()[0], s.entities[l1].points()[0]);
        s.add_constraint(Constraint::Coincident { a, b }).unwrap();
        assert!((s.points[a].x - s.points[b].x).abs() < 1e-6);
        assert!((s.points[a].y - s.points[b].y).abs() < 1e-6);
    }

    #[test]
    fn the_degrees_of_freedom_fall_as_the_sketch_is_held() {
        let mut s = Sketch::default();
        s.draw_rectangle((0.0, 0.0), (100.0, 50.0), 1.0).unwrap();
        // Four free corners, eight numbers, four of them held by horizontal and vertical.
        assert_eq!(s.degrees_of_freedom().unwrap(), 4);
        let (a, c) = (0, 2);
        s.dimension_points(a, c, Measure::Along).unwrap();
        s.dimension_points(a, c, Measure::Height).unwrap();
        assert_eq!(
            s.degrees_of_freedom().unwrap(),
            2,
            "the size is held, the place is not"
        );
        s.set_fixed(a, true).unwrap();
        assert_eq!(s.degrees_of_freedom().unwrap(), 0);
        // A hostile sketch has no degrees, only an error.
        let mut bad = s.clone();
        bad.points[0].x = f64::NAN;
        assert!(bad.degrees_of_freedom().is_err());
    }

    #[test]
    fn the_new_things_survive_a_file_and_removing_an_edge() {
        let mut s = templates::channel_with_block(400.0, 100.0, 30.0, 120.0, 35.0);
        let circle = s.draw_circle((300.0, 50.0), (310.0, 50.0), 1.0).unwrap();
        let slot = s.draw_slot((50.0, 20.0), (90.0, 20.0), 5.0, 1.0).unwrap();
        s.add_constraint(Constraint::Radius {
            arc: circle,
            value: 9.0,
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.vortia-sketch");
        s.save(&path).unwrap();
        assert_eq!(Sketch::open(&path).unwrap(), s);
        // Taking an edge out renumbers what refers to the ones after it, and drops what
        // referred to it.
        s.remove_edge(slot[0]).unwrap();
        assert!(s.validate().is_ok());
        // The two tangents that touched the removed side are gone; the other side's stay.
        let tangents = s
            .constraints
            .iter()
            .filter(|c| matches!(c, Constraint::Tangent { .. }))
            .count();
        assert_eq!(tangents, 2);
        s.remove_edge(s.entities.len() - 1 - 2).ok();
        assert!(s.validate().is_ok());
    }

    #[test]
    fn a_hostile_file_cannot_name_a_circle_that_is_not_there() {
        let s = templates::duct(10.0, 1.0);
        let mut json = s.to_json();
        json["entities"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "circle", "center": 0, "rim": 0}));
        assert!(Sketch::from_json(&json).is_err());
        let mut json = s.to_json();
        json["constraints"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "tangent", "line": 0, "round": 7}));
        assert!(Sketch::from_json(&json).is_err());
    }
}
