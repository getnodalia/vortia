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
    let spec = BodySpec::tunnel([-4.0; 3], [4.0; 3], [8; 3], 3);
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
    for f in mesh.n_internal_faces()..mesh.n_faces() {
        sum = sum + geo.face_areas[f];
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
