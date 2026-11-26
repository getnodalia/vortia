use super::*;
use crate::{MeshGeometry, PolyMesh, check};

/// The 12 triangles of an axis-aligned box.
fn cube(lo: [f64; 3], hi: [f64; 3]) -> Stl {
    let mut vertices = Vec::new();
    for z in [lo[2], hi[2]] {
        for y in [lo[1], hi[1]] {
            for x in [lo[0], hi[0]] {
                vertices.push([x, y, z]);
            }
        }
    }
    // Corner index = x + 2 y + 4 z.
    let q = |a: u32, b: u32, c: u32, d: u32| [[a, b, c], [a, c, d]];
    let mut triangles = Vec::new();
    for face in [
        q(0, 2, 3, 1),
        q(4, 5, 7, 6),
        q(0, 1, 5, 4),
        q(2, 6, 7, 3),
        q(0, 4, 6, 2),
        q(1, 3, 7, 5),
    ] {
        triangles.extend(face);
    }
    Stl {
        vertices,
        triangles,
        ..Default::default()
    }
}

/// A sphere of triangles by subdividing an octahedron.
fn sphere(centre: [f64; 3], radius: f64, depth: usize) -> Stl {
    let mut points: Vec<[f64; 3]> = vec![
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
    ];
    let mut tris: Vec<[usize; 3]> = vec![
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    for _ in 0..depth {
        let mut mids = std::collections::HashMap::new();
        let mut next = Vec::new();
        let mut mid = |a: usize, b: usize, points: &mut Vec<[f64; 3]>| {
            *mids.entry((a.min(b), a.max(b))).or_insert_with(|| {
                let (p, q) = (points[a], points[b]);
                let m = [p[0] + q[0], p[1] + q[1], p[2] + q[2]];
                let n = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                points.push([m[0] / n, m[1] / n, m[2] / n]);
                points.len() - 1
            })
        };
        for t in &tris {
            let (a, b, c) = (t[0], t[1], t[2]);
            let (ab, bc, ca) = (
                mid(a, b, &mut points),
                mid(b, c, &mut points),
                mid(c, a, &mut points),
            );
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = next;
    }
    Stl {
        vertices: points
            .iter()
            .map(|p| {
                [
                    centre[0] + radius * p[0],
                    centre[1] + radius * p[1],
                    centre[2] + radius * p[2],
                ]
            })
            .collect(),
        triangles: tris
            .iter()
            .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
            .collect(),
        ..Default::default()
    }
}

fn mesh_of(raw: RawPolyMesh) -> (PolyMesh, MeshGeometry) {
    let mesh = PolyMesh::from_raw(raw).expect("a valid mesh");
    let geo = MeshGeometry::compute(&mesh);
    (mesh, geo)
}

#[test]
fn the_cube_the_mesh_lacks_is_exactly_the_cube() {
    let body = cube([-0.5; 3], [0.5; 3]);
    let spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], 1);
    let (raw, stats) = mesh_around(&body, &spec).unwrap();
    let (mesh, geo) = mesh_of(raw);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    assert_eq!(report.open_cells, 0);
    let volume: f64 = geo.cell_volumes.iter().sum();
    assert!((volume - 63.0).abs() < 1e-9, "volume {volume}");
    assert_eq!(stats.cells, mesh.n_cells());
    // The cube's six faces are 1 x 1 each, in cells of 0.25.
    let body_patch = mesh.patch("body").unwrap();
    let area: f64 = body_patch.faces().map(|f| geo.face_areas[f].norm()).sum();
    assert!((area - 6.0).abs() < 1e-9, "area {area}");
}

#[test]
fn the_mesh_around_a_sphere_is_closed_and_the_volume_is_right() {
    let body = sphere([0.0; 3], 1.0, 3);
    let mut spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 3);
    spec.snap = false;
    let (raw, _) = mesh_around(&body, &spec).unwrap();
    let (mesh, geo) = mesh_of(raw);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    // The air is the box less what the cubes of the stepped sphere take: close to the sphere.
    let volume: f64 = geo.cell_volumes.iter().sum();
    let taken = 512.0 - volume;
    let exact = 4.0 / 3.0 * std::f64::consts::PI;
    assert!(
        (taken - exact).abs() / exact < 0.08,
        "the stepped sphere takes {taken}, the sphere {exact}"
    );
    // Every boundary face together encloses the domain: their area vectors cancel.
    let mut sum = vortia_core::Vec3::new(0.0, 0.0, 0.0);
    for area in &geo.face_areas[mesh.n_internal_faces()..mesh.n_faces()] {
        sum = sum + *area;
    }
    assert!(sum.norm() < 1e-9, "the boundary does not close: {sum:?}");
    // The cells step through three sizes: 1, 0.5, 0.25 and 0.125 across the levels.
    let smallest = geo
        .cell_volumes
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    assert!((smallest - 0.125f64.powi(3)).abs() < 1e-12, "{smallest}");
}

#[test]
fn neighbouring_cells_differ_by_one_level_at_most() {
    let body = sphere([0.0; 3], 1.0, 2);
    let spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [4; 3], 3);
    let grid = TriGrid::new(&body);
    let lattice = Lattice {
        low: spec.low,
        base_size: [2.0; 3],
        base: spec.base_cells,
        max_level: 3,
    };
    let tree = Octree::build(
        lattice,
        &Refinement {
            surface: &grid,
            surface_level: 3,
            regions: &[],
            max_cells: 1_000_000,
        },
    )
    .unwrap();
    for &k in &tree.leaves {
        for axis in 0..3 {
            for sign in [-1, 1] {
                for n in tree.across(k, axis, sign) {
                    assert!(
                        k.level.abs_diff(n.level) <= 1,
                        "{k:?} touches {n:?} across {axis}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_wake_box_refines_where_asked() {
    let body = cube([-0.5; 3], [0.5; 3]);
    let plain = BodySpec::tunnel([-3.0; 3], [5.0; 3], [8; 3], 1);
    let mut wake = plain.clone();
    wake.regions.push(Region {
        low: [0.5, -1.0, -1.0],
        high: [4.0, 1.0, 1.0],
        level: 2,
    });
    let a = mesh_around(&body, &plain).unwrap().1.cells;
    let b = mesh_around(&body, &wake).unwrap().1.cells;
    assert!(b > a, "{b} cells with the wake box, {a} without");
    let (raw, _) = mesh_around(&body, &wake).unwrap();
    let (mesh, geo) = mesh_of(raw);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
}

#[test]
fn a_surface_with_a_hole_is_refused() {
    let mut body = cube([-0.5; 3], [0.5; 3]);
    body.triangles.pop();
    let spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], 1);
    let err = mesh_around(&body, &spec).unwrap_err().to_string();
    assert!(err.contains("watertight"), "{err}");
}

#[test]
fn a_body_outside_the_domain_or_a_bad_spec_is_refused() {
    let body = cube([-0.5; 3], [0.5; 3]);
    let mut spec = BodySpec::tunnel([-0.4; 3], [2.0; 3], [8; 3], 1);
    assert!(mesh_around(&body, &spec).is_err(), "body sticks out");
    spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [0, 8, 8], 1);
    assert!(mesh_around(&body, &spec).is_err(), "no base cells");
    spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], MAX_LEVEL + 1);
    assert!(mesh_around(&body, &spec).is_err(), "level too deep");
    spec = BodySpec::tunnel([f64::NAN, -2.0, -2.0], [2.0; 3], [8; 3], 1);
    assert!(mesh_around(&body, &spec).is_err(), "not finite");
    spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], 4);
    spec.max_cells = 700;
    assert!(mesh_around(&body, &spec).is_err(), "over the cell limit");
}

#[test]
fn a_pocket_inside_a_hollow_body_is_not_meshed() {
    // A box shell: an outer cube and an inner cube with its faces turned inside out.
    let outer = cube([-1.0; 3], [1.0; 3]);
    let mut inner = cube([-0.4; 3], [0.4; 3]);
    for t in &mut inner.triangles {
        t.swap(1, 2);
    }
    let offset = outer.vertices.len() as u32;
    let mut vertices = outer.vertices.clone();
    vertices.extend(inner.vertices.iter().copied());
    let mut triangles = outer.triangles.clone();
    triangles.extend(
        inner
            .triangles
            .iter()
            .map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]),
    );
    let body = Stl {
        vertices,
        triangles,
        ..Default::default()
    };
    let spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], 2);
    let (raw, _) = mesh_around(&body, &spec).unwrap();
    let (mesh, geo) = mesh_of(raw);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    // The air is the domain less the whole outer cube: the cavity is shut in and left out.
    let volume: f64 = geo.cell_volumes.iter().sum();
    assert!((volume - (64.0 - 8.0)).abs() < 1e-9, "volume {volume}");
}

#[test]
fn snapping_puts_the_wall_on_the_sphere_and_keeps_the_mesh_valid() {
    let body = sphere([0.0; 3], 1.0, 4);
    let mut spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 3);
    spec.snap = false;
    let (stepped_raw, _) = mesh_around(&body, &spec).unwrap();
    let (stepped, stepped_geo) = mesh_of(stepped_raw);
    spec.snap = true;
    let (raw, stats) = mesh_around(&body, &spec).unwrap();
    let (mesh, geo) = mesh_of(raw);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    assert_eq!(report.open_cells, 0);
    assert_eq!(mesh.n_cells(), stepped.n_cells());
    assert!(stats.snapping.wall_points > 100, "{:?}", stats.snapping);
    assert!(
        stats.snapping.snapped * 10 >= stats.snapping.wall_points * 5,
        "most wall points reach the surface: {:?}",
        stats.snapping
    );

    // The wall points lie on the sphere (the facets of the surface are inside it by a hair).
    let wall = mesh.patch("body").unwrap();
    let mut worst = 0.0f64;
    for f in wall.faces() {
        for p in mesh.face_points(f) {
            worst = worst.max((p.norm() - 1.0).abs());
        }
    }
    assert!(worst < 0.1, "a wall point is {worst} from the sphere");

    // The wall area is the sphere's much more closely than the steps'.
    let area = |m: &PolyMesh, g: &MeshGeometry| -> f64 {
        m.patch("body")
            .unwrap()
            .faces()
            .map(|f| g.face_areas[f].norm())
            .sum()
    };
    let exact = 4.0 * std::f64::consts::PI;
    let (a_snapped, a_stepped) = (area(&mesh, &geo), area(&stepped, &stepped_geo));
    assert!(
        (a_snapped - exact).abs() < (a_stepped - exact).abs(),
        "snapped {a_snapped}, stepped {a_stepped}, sphere {exact}"
    );
    // The air takes what the sphere leaves, within a few percent.
    let volume: f64 = geo.cell_volumes.iter().sum();
    let sphere_volume = 4.0 / 3.0 * std::f64::consts::PI;
    assert!(
        ((512.0 - volume) - sphere_volume).abs() / sphere_volume < 0.04,
        "the snapped wall takes {}",
        512.0 - volume
    );
}

#[test]
fn prism_layers_on_the_sphere_add_cells_and_keep_the_mesh_valid() {
    let body = sphere([0.0; 3], 1.0, 4);
    let mut spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 3);
    let (plain_raw, plain_stats) = mesh_around(&body, &spec).unwrap();
    let (plain, plain_geo) = mesh_of(plain_raw);
    spec.layers = Some(LayerSpec {
        count: 3,
        first: 0.01,
        ratio: 1.3,
        reach: 0.5,
    });
    let (raw, stats) = mesh_around(&body, &spec).unwrap();
    let (mesh, geo) = mesh_of(raw);
    // Thin cells on a wall of skewed faces lean on each other, so some faces are more
    // non-orthogonal than on the plain mesh; the mesh is still closed, valid and solvable.
    let report = check(&mesh, &geo);
    assert_eq!(report.non_positive_volumes, 0);
    assert_eq!(report.open_cells, 0);
    assert_eq!(report.wrongly_oriented, 0);
    assert!(report.upper_triangular);
    let worst = report.non_orthogonality.map_or(0.0, |s| s.max);
    assert!(worst < 85.0, "worst non-orthogonality {worst} degrees");
    assert_eq!(stats.layering.cells, 3 * plain_stats.body_faces);
    assert_eq!(mesh.n_cells(), plain.n_cells() + stats.layering.cells);
    assert_eq!(stats.cells, mesh.n_cells());
    assert_eq!(stats.body_faces, plain_stats.body_faces);
    assert!(stats.layering.mean > 0.0 && stats.layering.mean <= stats.layering.wanted + 1e-12);

    // The wall is still the same size, the first layer is as thick as asked where it fits, and
    // the layers take only the air next to the wall: the volume the body leaves is the same.
    let wall_area = |m: &PolyMesh, g: &MeshGeometry| -> f64 {
        m.patch("body")
            .unwrap()
            .faces()
            .map(|f| g.face_areas[f].norm())
            .sum()
    };
    let (a0, a1) = (wall_area(&plain, &plain_geo), wall_area(&mesh, &geo));
    assert!((a0 - a1).abs() / a0 < 1e-9, "{a0} vs {a1}");
    let (v0, v1): (f64, f64) = (
        plain_geo.cell_volumes.iter().sum(),
        geo.cell_volumes.iter().sum(),
    );
    assert!((v0 - v1).abs() / v0 < 1e-9, "{v0} vs {v1}");
}

#[test]
fn layers_that_cannot_be_grown_are_refused_and_leave_the_mesh_alone() {
    let body = sphere([0.0; 3], 1.0, 3);
    let spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 3);
    let (mut raw, _) = mesh_around(&body, &spec).unwrap();
    let before = raw.clone();
    let good = LayerSpec {
        count: 2,
        first: 0.01,
        ratio: 1.2,
        reach: 0.5,
    };
    for bad in [
        LayerSpec {
            count: 0,
            ..good.clone()
        },
        LayerSpec {
            count: MAX_LAYERS + 1,
            ..good.clone()
        },
        LayerSpec {
            first: 0.0,
            ..good.clone()
        },
        LayerSpec {
            first: f64::NAN,
            ..good.clone()
        },
        LayerSpec {
            ratio: 0.5,
            ..good.clone()
        },
        LayerSpec {
            reach: 0.0,
            ..good.clone()
        },
    ] {
        assert!(add_layers(&mut raw, "body", &bad).is_err(), "{bad:?}");
    }
    assert!(add_layers(&mut raw, "no-such-patch", &good).is_err());
    assert_eq!(raw, before);
}

/// Two separate cubes in one surface, named "left" and "right".
fn two_cubes() -> Stl {
    let left = cube([-1.5, -0.5, -0.5], [-0.5, 0.5, 0.5]);
    let right = cube([0.5, -0.5, -0.5], [1.5, 0.5, 0.5]);
    let offset = left.vertices.len() as u32;
    let mut vertices = left.vertices.clone();
    vertices.extend(right.vertices.iter().copied());
    let mut triangles = left.triangles.clone();
    triangles.extend(right.triangles.iter().map(|t| t.map(|i| i + offset)));
    let split = left.triangles.len();
    let total = triangles.len();
    let part = |name: &str, start, end| crate::stl::SurfacePart {
        name: name.into(),
        start,
        end,
    };
    Stl {
        vertices,
        triangles,
        parts: vec![part("left wing", 0, split), part("right", split, total)],
    }
}

#[test]
fn the_wall_is_split_into_a_patch_for_each_part() {
    let body = two_cubes();
    let mut spec = BodySpec::tunnel([-3.0; 3], [3.0; 3], [12; 3], 1);
    spec.snap = false;
    let (single, _) = mesh_around(&body, &spec).unwrap();
    spec.by_part = true;
    let (raw, stats) = mesh_around(&body, &spec).unwrap();
    // Nothing but the wall changed: the same cells and points, the faces the same set.
    assert_eq!(raw.points, single.points);
    assert_eq!(raw.faces.len(), single.faces.len());
    assert_eq!(raw.owner.len(), single.owner.len());
    let (mesh, geo) = mesh_of(raw);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    assert!(mesh.patch("body").is_none());
    let names: Vec<_> = stats.parts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["body_left_wing", "body_right"]);
    for (name, side) in [("body_left_wing", -1.0), ("body_right", 1.0)] {
        let patch = mesh.patch(name).unwrap();
        assert_eq!(patch.kind.name(), "wall");
        let area: f64 = patch.faces().map(|f| geo.face_areas[f].norm()).sum();
        assert!((area - 6.0).abs() < 1e-9, "{name}: area {area}");
        for f in patch.faces() {
            let x = mesh.face_points(f).map(|p| p.x).sum::<f64>() / 4.0;
            assert!(x * side > 0.0, "{name} has a face at x = {x}");
        }
    }
    assert_eq!(
        stats.parts.iter().map(|(_, n)| n).sum::<usize>(),
        stats.body_faces
    );
}

#[test]
fn a_surface_without_parts_keeps_one_wall_and_bad_parts_are_refused() {
    let body = cube([-0.5; 3], [0.5; 3]);
    let mut spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], 1);
    spec.by_part = true;
    let (raw, stats) = mesh_around(&body, &spec).unwrap();
    assert!(stats.parts.is_empty());
    assert!(raw.patches.iter().any(|p| p.name == "body"));
    // Parts that leave triangles out are not a cover: no split, one wall.
    let mut two = two_cubes();
    two.parts.pop();
    assert!(part_patches(&two, "body").is_none());
    // Names that clean up to the same patch are one patch.
    let mut same = two_cubes();
    same.parts[0].name = "Wing 1".into();
    same.parts[1].name = "Wing-1".into();
    assert!(part_patches(&same, "body").is_none());
    assert_eq!(patch_name("body", "  "), "body_part");
    assert_eq!(patch_name("body", "front wheel.L"), "body_front_wheel_L");
}

fn solid_spec(snap: bool) -> BodySpec {
    let mut spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 3);
    spec.snap = snap;
    spec.solid = true;
    spec
}

fn area_of(mesh: &PolyMesh, geo: &MeshGeometry, patch: &str) -> f64 {
    mesh.patch(patch)
        .unwrap()
        .faces()
        .map(|f| geo.face_areas[f].norm())
        .sum()
}

#[test]
fn a_cube_gives_a_solid_that_is_exactly_the_cube_and_shares_its_faces_with_the_air() {
    let body = cube([-0.5; 3], [0.5; 3]);
    let mut spec = BodySpec::tunnel([-2.0; 3], [2.0; 3], [8; 3], 1);
    spec.solid = true;
    let BodyMeshes {
        fluid,
        solid,
        stats,
    } = mesh_body(&body, &spec).unwrap();
    let solid = solid.expect("the solid was asked for");
    let (fluid_mesh, fluid_geo) = mesh_of(fluid.clone());
    let (solid_mesh, solid_geo) = mesh_of(solid.clone());
    for (name, mesh, geo) in [
        ("fluid", &fluid_mesh, &fluid_geo),
        ("solid", &solid_mesh, &solid_geo),
    ] {
        let report = check(mesh, geo);
        assert!(report.is_ok(), "{name}: {:?}", report.failed_checks());
        assert_eq!(report.open_cells, 0, "{name}");
    }
    // The air is the box less the cube, the solid is the cube.
    let air: f64 = fluid_geo.cell_volumes.iter().sum();
    let body_volume: f64 = solid_geo.cell_volumes.iter().sum();
    assert!((air - 63.0).abs() < 1e-9, "air {air}");
    assert!((body_volume - 1.0).abs() < 1e-9, "solid {body_volume}");
    assert_eq!(stats.cells, fluid_mesh.n_cells());
    assert_eq!(stats.solid_cells, solid_mesh.n_cells());
    // The solid is all wall: one patch of six 1 x 1 faces in cells of 0.25.
    assert_eq!(solid_mesh.patches().len(), 1);
    assert!((area_of(&solid_mesh, &solid_geo, "body") - 6.0).abs() < 1e-9);
    assert!((area_of(&fluid_mesh, &fluid_geo, "body") - 6.0).abs() < 1e-9);
    // The same faces, with their order: face i of one is face i of the other, the normals
    // opposite.
    let (fw, sw) = (
        fluid_mesh.patch("body").unwrap(),
        solid_mesh.patch("body").unwrap(),
    );
    assert_eq!(fw.size, sw.size);
    for (f, s) in fw.faces().zip(sw.faces()) {
        let (a, b) = (fluid_geo.face_areas[f], solid_geo.face_areas[s]);
        assert!((a + b).norm() < 1e-12, "normals {a:?} and {b:?}");
        let (c, d) = (fluid_geo.face_centres[f], solid_geo.face_centres[s]);
        assert!((c - d).norm() < 1e-12, "centres {c:?} and {d:?}");
    }
    let pairs = match_interface(&fluid, &["body"], &solid, "body").unwrap();
    assert_eq!(pairs.solid_face, (0..fw.size).collect::<Vec<_>>());
}

#[test]
fn the_air_and_the_solid_of_a_snapped_sphere_fit_together() {
    let body = sphere([0.0; 3], 1.0, 3);
    let BodyMeshes {
        fluid,
        solid,
        stats,
    } = mesh_body(&body, &solid_spec(true)).unwrap();
    let solid = solid.unwrap();
    assert!(stats.snapping.snapped > 0, "{:?}", stats.snapping);
    let (fluid_mesh, fluid_geo) = mesh_of(fluid.clone());
    let (solid_mesh, solid_geo) = mesh_of(solid.clone());
    for (name, mesh, geo) in [
        ("fluid", &fluid_mesh, &fluid_geo),
        ("solid", &solid_mesh, &solid_geo),
    ] {
        let report = check(mesh, geo);
        assert!(report.is_ok(), "{name}: {:?}", report.failed_checks());
        assert_eq!(report.open_cells, 0, "{name}");
    }
    // The solid is the sphere (to the mesh): its volume within a few per cent, and the box is
    // air and solid together.
    let exact = 4.0 / 3.0 * std::f64::consts::PI;
    let body_volume: f64 = solid_geo.cell_volumes.iter().sum();
    let air: f64 = fluid_geo.cell_volumes.iter().sum();
    assert!(
        (body_volume - exact).abs() / exact < 0.04,
        "solid {body_volume}, sphere {exact}"
    );
    assert!(
        (air + body_volume - 512.0).abs() < 1e-6,
        "{air} + {body_volume}"
    );
    // The interface is one surface seen from both sides.
    let (fa, sa) = (
        area_of(&fluid_mesh, &fluid_geo, "body"),
        area_of(&solid_mesh, &solid_geo, "body"),
    );
    assert!((fa - sa).abs() < 1e-9 * fa, "{fa} and {sa}");
    // Snapped together the wall also has to keep the solid's cells valid, so fewer of its points
    // reach the surface than for the air alone (446 of 1250 against 602): its area is a little
    // more, 12 % at most, and far less than the stepped wall's.
    let (alone, alone_stats) = mesh_around(&body, &solid_spec(true)).unwrap();
    let (alone_mesh, alone_geo) = mesh_of(alone);
    let alone_area = area_of(&alone_mesh, &alone_geo, "body");
    assert!(
        fa >= alone_area * 0.99 && fa < alone_area * 1.12,
        "{fa} with the solid, {alone_area} without"
    );
    let (stepped, _) = mesh_body(&body, &{
        let mut s = solid_spec(false);
        s.solid = true;
        s
    })
    .map(|m| (m.solid.unwrap(), ()))
    .unwrap();
    let (stepped_mesh, stepped_geo) = mesh_of(stepped);
    assert!(area_of(&stepped_mesh, &stepped_geo, "body") > 1.2 * fa);
    assert_eq!(alone_stats.solid_cells, 0);
    let pairs = match_interface(&fluid, &["body"], &solid, "body").unwrap();
    assert_eq!(
        pairs.solid_face.len(),
        fluid_mesh.patch("body").unwrap().size
    );
    let mut seen = pairs.solid_face.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), pairs.solid_face.len(), "a face is taken twice");
}

#[test]
fn the_interface_survives_prism_layers_and_a_wall_for_each_part() {
    // Two cubes apart: two parts, and layers over the wall of the air.
    let mut surface = cube([-1.5, -0.5, -0.5], [-0.5, 0.5, 0.5]);
    let other = cube([0.5, -0.5, -0.5], [1.5, 0.5, 0.5]);
    let shift = surface.vertices.len() as u32;
    surface.vertices.extend(other.vertices);
    surface
        .triangles
        .extend(other.triangles.iter().map(|t| t.map(|i| i + shift)));
    let mut spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 2);
    spec.solid = true;
    spec.layers = Some(LayerSpec {
        count: 2,
        first: 0.02,
        ratio: 1.3,
        reach: 0.5,
    });
    let BodyMeshes { fluid, solid, .. } = mesh_body(&surface, &spec).unwrap();
    let solid = solid.unwrap();
    let (fluid_mesh, fluid_geo) = mesh_of(fluid.clone());
    let (solid_mesh, solid_geo) = mesh_of(solid.clone());
    assert!(check(&fluid_mesh, &fluid_geo).is_ok());
    assert!(check(&solid_mesh, &solid_geo).is_ok());
    // Two cubes of 1 m³, 12 m² of wall in all.
    let body_volume: f64 = solid_geo.cell_volumes.iter().sum();
    assert!((body_volume - 2.0).abs() < 1e-9, "{body_volume}");
    let pairs = match_interface(&fluid, &["body"], &solid, "body").unwrap();
    assert_eq!(
        pairs.solid_face.len(),
        fluid_mesh.patch("body").unwrap().size
    );
}

#[test]
fn a_body_with_a_cavity_is_refused_and_the_air_alone_is_unchanged_by_asking_for_nothing() {
    // A box with a box-shaped hole shut inside: two nested closed surfaces, the inner one inverted.
    let mut outer = cube([-1.0; 3], [1.0; 3]);
    let mut hole = cube([-0.4; 3], [0.4; 3]);
    for t in &mut hole.triangles {
        t.swap(1, 2);
    }
    let shift = outer.vertices.len() as u32;
    outer.vertices.extend(hole.vertices);
    outer
        .triangles
        .extend(hole.triangles.iter().map(|t| t.map(|i| i + shift)));
    let mut spec = BodySpec::tunnel([-3.0; 3], [3.0; 3], [12; 3], 2);
    spec.solid = true;
    let err = mesh_body(&outer, &spec).unwrap_err().to_string();
    assert!(err.contains("cavity"), "{err}");
    // Without a solid it is meshed as it always was.
    spec.solid = false;
    assert!(mesh_body(&outer, &spec).unwrap().solid.is_none());
}
