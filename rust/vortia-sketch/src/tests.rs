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
            ok.to_string().replace("\"version\":1", "\"version\":9"),
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
