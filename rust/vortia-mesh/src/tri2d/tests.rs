use super::*;
use crate::check::check;
use crate::geometry::MeshGeometry;
use crate::polymesh::PolyMesh;

fn seg(points: &[[f64; 2]], patch: &str) -> Segment {
    Segment {
        points: points.to_vec(),
        patch: patch.to_owned(),
        kind: "patch".to_owned(),
        size: None,
    }
}

/// A `w × h` rectangle, counter-clockwise from the lower left: the bottom is the wall, the right
/// the outlet, the top the wall and the left the inlet.
fn rectangle(w: f64, h: f64) -> Region {
    Region {
        loops: vec![vec![
            seg(&[[0.0, 0.0], [w, 0.0]], "bottom"),
            seg(&[[w, 0.0], [w, h]], "outlet"),
            seg(&[[w, h], [0.0, h]], "top"),
            seg(&[[0.0, h], [0.0, 0.0]], "inlet"),
        ]],
    }
}

fn planar(size: f64) -> Options {
    Options::new(size, Extrusion::Planar { thickness: 1.0 })
}

fn built(raw: RawPolyMesh) -> (PolyMesh, MeshGeometry) {
    let mesh = PolyMesh::from_raw(raw).unwrap();
    let geo = MeshGeometry::compute(&mesh);
    (mesh, geo)
}

/// The smallest and largest angle of the triangles of a planar mesh, in degrees, read from the
/// front faces.
fn angles(mesh: &PolyMesh) -> (f64, f64) {
    let front = mesh.patch("frontAndBack").unwrap();
    let (mut lo, mut hi) = (f64::MAX, 0.0_f64);
    for f in front.faces() {
        let ids = mesh.face(f);
        if ids.len() != 3 || mesh.points().get(ids[0]).map_or(0.0, |p| p.z) < 0.5 {
            continue;
        }
        let p: Vec<_> = ids.iter().map(|&i| mesh.points()[i]).collect();
        for k in 0..3 {
            let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
            let (u, v) = ((b.x - a.x, b.y - a.y), (c.x - a.x, c.y - a.y));
            let cos = (u.0 * v.0 + u.1 * v.1) / (u.0.hypot(u.1) * v.0.hypot(v.1));
            let angle = cos.clamp(-1.0, 1.0).acos().to_degrees();
            lo = lo.min(angle);
            hi = hi.max(angle);
        }
    }
    (lo, hi)
}

#[test]
fn a_rectangle_is_meshed_exactly_and_with_good_triangles() {
    let raw = mesh(&rectangle(2.0, 1.0), &planar(0.1)).unwrap();
    let (m, geo) = built(raw);
    let report = check(&m, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    // The prisms fill the rectangle, one unit thick.
    assert!(
        (geo.total_volume() - 2.0).abs() < 1e-9,
        "{}",
        geo.total_volume()
    );
    assert!(m.n_cells() > 300 && m.n_cells() < 1200, "{}", m.n_cells());
    let (lo, _) = angles(&m);
    assert!(lo > 19.0, "the thinnest triangle has a {lo}° angle");
    // Each side is its own patch, and the ends are empty.
    for name in ["bottom", "outlet", "top", "inlet"] {
        let p = m.patch(name).unwrap_or_else(|| panic!("no '{name}'"));
        assert!(p.size >= 10, "{name} has {} faces", p.size);
    }
    assert_eq!(m.patches().last().unwrap().name, "frontAndBack");
    assert_eq!(m.patches().last().unwrap().kind.name(), "empty");
    // The outlet has the faces of a length-1 side cut in tenths.
    let outlet = m.patch("outlet").unwrap();
    let mut area = 0.0_f64;
    for f in outlet.faces() {
        area += geo.face_areas[f].norm();
    }
    assert!((area - 1.0).abs() < 1e-9, "{area}");
}

#[test]
fn a_hole_is_left_out_and_its_wall_is_a_patch() {
    let mut region = rectangle(2.0, 1.0);
    // A square hole in the middle, 0.4 wide.
    region.loops.push(vec![
        seg(&[[0.8, 0.3], [0.8, 0.7]], "cylinder"),
        seg(&[[0.8, 0.7], [1.2, 0.7]], "cylinder"),
        seg(&[[1.2, 0.7], [1.2, 0.3]], "cylinder"),
        seg(&[[1.2, 0.3], [0.8, 0.3]], "cylinder"),
    ]);
    let (m, geo) = built(mesh(&region, &planar(0.08)).unwrap());
    assert!(check(&m, &geo).is_ok());
    assert!((geo.total_volume() - (2.0 - 0.16)).abs() < 1e-9);
    let wall = m.patch("cylinder").unwrap();
    assert!(wall.size >= 20);
    let mut length = 0.0_f64;
    for f in wall.faces() {
        length += geo.face_areas[f].norm();
    }
    assert!((length - 1.6).abs() < 1e-9, "{length}");
}

#[test]
fn a_finer_segment_gets_finer_cells_and_the_mesh_grades_away_from_it() {
    let uniform = mesh(&rectangle(2.0, 1.0), &planar(0.1)).unwrap();
    let mut graded = rectangle(2.0, 1.0);
    if let Some(bottom) = graded.loops.first_mut().and_then(|l| l.first_mut()) {
        bottom.size = Some(0.02);
    }
    let raw = mesh(&graded, &planar(0.1)).unwrap();
    let (m, geo) = built(raw);
    assert!(check(&m, &geo).is_ok());
    assert!((geo.total_volume() - 2.0).abs() < 1e-9);
    let n_uniform = built(uniform).0.n_cells();
    assert!(
        m.n_cells() > n_uniform,
        "{} against {n_uniform}",
        m.n_cells()
    );
    // The cells at the bottom are smaller than the ones at the top.
    let (mut near, mut far) = ((0.0, 0), (0.0, 0));
    for c in 0..m.n_cells() {
        let y = geo.cell_centres[c].y;
        let v = geo.cell_volumes[c];
        if y < 0.15 {
            near = (near.0 + v, near.1 + 1);
        } else if y > 0.85 {
            far = (far.0 + v, far.1 + 1);
        }
    }
    let (near, far) = (near.0 / near.1 as f64, far.0 / far.1 as f64);
    assert!(near * 4.0 < far, "{near} against {far}");
    let (lo, _) = angles(&m);
    assert!(lo > 19.0, "{lo}");
}

#[test]
fn an_arc_boundary_is_followed_by_its_chords() {
    // A half disc of radius 1 on the axis: the chords of the arc are the curved wall.
    let n = 40;
    let arc: Vec<[f64; 2]> = (0..=n)
        .map(|i| {
            let a = std::f64::consts::PI * i as f64 / n as f64;
            [a.cos(), a.sin()]
        })
        .collect();
    let region = Region {
        loops: vec![vec![
            seg(&arc, "wall"),
            seg(&[[-1.0, 0.0], [1.0, 0.0]], "floor"),
        ]],
    };
    let (m, geo) = built(mesh(&region, &planar(0.1)).unwrap());
    assert!(check(&m, &geo).is_ok());
    // The polygon's area, from the chords: (n/2) sin(π/n).
    let polygon = (n as f64 / 2.0) * (std::f64::consts::PI / n as f64).sin();
    assert!(
        (geo.total_volume() - polygon).abs() < 1e-9,
        "{}",
        geo.total_volume()
    );
}

#[test]
fn the_wedge_of_a_duct_has_its_axis_collapsed() {
    // A duct 1 long and 0.2 in radius about the x axis.
    let region = Region {
        loops: vec![vec![
            seg(&[[0.0, 0.0], [1.0, 0.0]], "axis"),
            seg(&[[1.0, 0.0], [1.0, 0.2]], "outlet"),
            seg(&[[1.0, 0.2], [0.0, 0.2]], "wall"),
            seg(&[[0.0, 0.2], [0.0, 0.0]], "inlet"),
        ]],
    };
    let options = Options::new(0.02, Extrusion::Wedge { degrees: 5.0 });
    let (m, geo) = built(mesh(&region, &options).unwrap());
    let report = check(&m, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    // The axis has no faces, so there is no patch for it.
    assert!(m.patch("axis").is_none());
    assert_eq!(m.patch("wedgeFront").unwrap().kind.name(), "wedge");
    assert_eq!(m.patch("wedgeBack").unwrap().kind.name(), "wedge");
    // The volume of the 5° wedge of a cylinder: (5/360) π r² L.
    let exact = 5.0 / 360.0 * std::f64::consts::PI * 0.2 * 0.2;
    assert!(
        (geo.total_volume() - exact).abs() < 0.01 * exact,
        "{}",
        geo.total_volume()
    );
}

#[test]
fn a_region_that_cannot_be_meshed_is_refused_not_panicked_on() {
    let ok = rectangle(1.0, 1.0);
    // Bad numbers.
    for size in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(mesh(&ok, &planar(size)).is_err(), "{size}");
    }
    assert!(
        mesh(
            &ok,
            &Options::new(0.1, Extrusion::Planar { thickness: 0.0 })
        )
        .is_err()
    );
    assert!(mesh(&ok, &Options::new(0.1, Extrusion::Wedge { degrees: 9.0 })).is_err());
    let mut o = planar(0.1);
    o.min_angle = 40.0;
    assert!(mesh(&ok, &o).is_err());
    // No loops, no segments, one point, not a number, a gap in the loop.
    assert!(mesh(&Region::default(), &planar(0.1)).is_err());
    assert!(
        mesh(
            &Region {
                loops: vec![vec![]]
            },
            &planar(0.1)
        )
        .is_err()
    );
    assert!(
        mesh(
            &Region {
                loops: vec![vec![seg(&[[0.0, 0.0]], "x")]]
            },
            &planar(0.1)
        )
        .is_err()
    );
    let mut nan = rectangle(1.0, 1.0);
    nan.loops[0][0].points[1] = [f64::NAN, 0.0];
    assert!(mesh(&nan, &planar(0.1)).is_err());
    let mut gap = rectangle(1.0, 1.0);
    gap.loops[0][1].points[0] = [1.0, 0.3];
    let err = mesh(&gap, &planar(0.1)).unwrap_err().to_string();
    assert!(err.contains("meet end to end"), "{err}");
    // A boundary that crosses itself (a bow tie).
    let bow = Region {
        loops: vec![vec![
            seg(&[[0.0, 0.0], [1.0, 1.0]], "a"),
            seg(&[[1.0, 1.0], [1.0, 0.0]], "b"),
            seg(&[[1.0, 0.0], [0.0, 1.0]], "c"),
            seg(&[[0.0, 1.0], [0.0, 0.0]], "d"),
        ]],
    };
    let err = mesh(&bow, &planar(0.1)).unwrap_err().to_string();
    assert!(err.contains("crosses itself"), "{err}");
    // So many cells that the limit stops it.
    let mut big = planar(0.001);
    big.max_cells = 5_000;
    assert!(mesh(&rectangle(1.0, 1.0), &big).is_err());
    // Two points on top of each other.
    let twice = Region {
        loops: vec![vec![
            seg(&[[0.0, 0.0], [1.0, 0.0], [1.0, 0.0]], "a"),
            seg(&[[1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]], "b"),
        ]],
    };
    let _ = mesh(&twice, &planar(0.2));
}

#[test]
fn a_long_thin_duct_is_meshed() {
    let (m, geo) = built(mesh(&rectangle(578.0, 50.0), &planar(6.0)).unwrap());
    assert!(check(&m, &geo).is_ok());
    assert!(m.n_cells() < 5000, "{}", m.n_cells());
}
