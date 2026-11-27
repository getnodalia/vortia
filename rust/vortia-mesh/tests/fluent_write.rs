//! A mesh written as a Fluent `.msh` and read back through the engine's own
//! reader is the same mesh: its points, its volume, its patches and the area
//! of each, for every cell shape the writer names and for 2-D and 3-D files.
//!
//! This proves the two halves agree with each other; it does not prove Fluent
//! opens the file, which was not tried.

use vortia_core::{Aabb, Vec3};
use vortia_foamfile::{FaceList, RawCellZone, RawPatch, RawPolyMesh};
use vortia_mesh::fluent::{Options, fluent_to_foam};
use vortia_mesh::fluent_write::{WriteOptions, foam_to_fluent};
use vortia_mesh::tri2d::{self, Extrusion, Region, Segment};
use vortia_mesh::{MeshGeometry, PolyMesh, blocks};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

/// Name, kind, number of faces and total area of each patch.
fn patch_summary(raw: &RawPolyMesh) -> Vec<(String, String, usize, f64)> {
    let mesh = PolyMesh::from_raw(raw.clone()).unwrap();
    let geo = MeshGeometry::compute(&mesh);
    mesh.patches()
        .iter()
        .map(|p| {
            let area: f64 = p.faces().map(|f| geo.face_areas[f].norm()).sum();
            (p.name.clone(), p.kind.name().to_owned(), p.size, area)
        })
        .collect()
}

fn volume(raw: &RawPolyMesh) -> f64 {
    let mesh = PolyMesh::from_raw(raw.clone()).unwrap();
    MeshGeometry::compute(&mesh).total_volume()
}

fn n_cells(raw: &RawPolyMesh) -> usize {
    raw.owner.iter().max().map_or(0, |m| m + 1)
}

fn written(raw: &RawPolyMesh) -> String {
    foam_to_fluent(raw, &WriteOptions::default()).unwrap().text
}

fn same_patches(a: &RawPolyMesh, b: &RawPolyMesh, skip: &[&str]) {
    let keep = |v: Vec<(String, String, usize, f64)>| {
        v.into_iter()
            .filter(|p| !skip.contains(&p.0.as_str()))
            .collect::<Vec<_>>()
    };
    let (a, b) = (keep(patch_summary(a)), keep(patch_summary(b)));
    assert_eq!(a.len(), b.len(), "{a:?} against {b:?}");
    for (x, y) in a.iter().zip(&b) {
        assert_eq!((&x.0, &x.1, x.2), (&y.0, &y.1, y.2), "{a:?} against {b:?}");
        assert!(close(x.3, y.3), "area of {}: {} against {}", x.0, x.3, y.3);
    }
}

fn the_box(cells: [usize; 3]) -> RawPolyMesh {
    blocks::structured_box_raw(
        Aabb {
            min: Vec3::new(-1.0, 0.0, 2.0),
            max: Vec3::new(2.0, 1.0, 4.0),
        },
        cells,
    )
    .unwrap()
}

#[test]
fn a_box_of_hexahedra_reads_back_as_the_same_mesh() {
    let mut raw = the_box([3, 2, 4]);
    // Named and typed the way a case is: an inlet, an outlet, walls, a symmetry plane.
    let names = [
        ("inlet", "patch"),
        ("outlet", "patch"),
        ("floor", "wall"),
        ("lid", "wall"),
        ("side one", "symmetryPlane"),
        ("side two", "symmetryPlane"),
    ];
    for (p, (name, kind)) in raw.patches.iter_mut().zip(names) {
        p.name = name.to_owned();
        p.kind = kind.to_owned();
        p.groups.clear();
    }
    let out = foam_to_fluent(&raw, &WriteOptions::default()).unwrap();
    // The patches of kind `patch` were typed from their names, and it says so.
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("`inlet` as velocity-inlet")
                && w.contains("`outlet` as pressure-outlet")),
        "{:?}",
        out.warnings
    );
    // A name with a space cannot be a zone name: it is written changed, and says so.
    assert!(out.text.contains("(45 (7 symmetry side_one)())"));
    assert!(out.warnings.iter().any(|w| w.contains("`side one`")));

    let back = fluent_to_foam(&out.text, Options::default()).unwrap();
    assert_eq!(back.points, raw.points);
    assert_eq!(n_cells(&back), 24);
    assert_eq!(back.faces.len(), raw.faces.len());
    assert_eq!(back.neighbour.len(), raw.neighbour.len());
    assert!(close(volume(&back), volume(&raw)));
    assert!(close(volume(&back), 3.0 * 1.0 * 2.0));
    for (a, b) in patch_summary(&raw).iter().zip(patch_summary(&back)) {
        assert_eq!(a.0.replace(' ', "_"), b.0);
        assert_eq!(a.2, b.2);
        assert!(close(a.3, b.3));
    }
    let kinds: Vec<String> = patch_summary(&back).into_iter().map(|p| p.1).collect();
    assert_eq!(
        kinds,
        ["patch", "patch", "wall", "wall", "symmetry", "symmetry"]
    );
}

#[test]
fn the_file_has_the_sections_fluent_reads() {
    let text = written(&the_box([2, 1, 1]));
    for line in [
        "(2 3)",
        "(10 (0 1 c 0 3))",
        "(12 (0 1 2 0 0))",
        "(10 (1 1 c 1 3)(",
        "(12 (1 1 2 1 4))",
        "(45 (1 fluid fluid)())",
        "(45 (2 interior interior)())",
    ] {
        assert!(text.lines().any(|l| l == line), "no `{line}` in\n{text}");
    }
    // One interior face between the two cells, listing its neighbour first.
    let faces = text.lines().find(|l| l.starts_with("(13 (2 ")).unwrap();
    assert_eq!(faces, "(13 (2 1 1 2 4)(");
}

#[test]
fn an_overridden_type_is_used_and_an_unknown_one_is_refused() {
    let raw = the_box([1, 1, 1]);
    let options = WriteOptions {
        boundary_types: vec![("xMin".into(), "mass-flow-inlet".into())],
    };
    assert!(foam_to_fluent(&raw, &options).is_err());
    let options = WriteOptions {
        boundary_types: vec![("xMin".into(), "velocity-inlet".into())],
    };
    let out = foam_to_fluent(&raw, &options).unwrap();
    assert!(out.text.contains("velocity-inlet xMin"));
    assert!(!out.warnings.iter().any(|w| w.contains("`xMin`")));
}

#[test]
fn cell_zones_are_kept_and_the_cells_are_moved_to_sit_together() {
    let mut raw = the_box([4, 1, 1]);
    // Cells 0 and 2 in one zone, 1 and 3 in another: Fluent's zones are
    // ranges, so the cells are renumbered 0 2 | 1 3.
    raw.cell_zones = vec![
        RawCellZone {
            name: "even".into(),
            cells: vec![0, 2],
        },
        RawCellZone {
            name: "odd".into(),
            cells: vec![1, 3],
        },
    ];
    let out = foam_to_fluent(&raw, &WriteOptions::default()).unwrap();
    assert!(!out.warnings.iter().any(|w| w.contains("cell zones")));
    assert!(out.text.contains("(45 (1 fluid even)())"));
    assert!(out.text.contains("(45 (2 fluid odd)())"));
    let back = fluent_to_foam(
        &out.text,
        Options {
            write_zones: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(back.cell_zones.len(), 2);
    assert_eq!(back.cell_zones[0].name, "even");
    assert_eq!(back.cell_zones[0].cells, [0, 1]);
    assert_eq!(back.cell_zones[1].cells, [2, 3]);
    assert!(close(volume(&back), volume(&raw)));

    // Zones that leave a cell out are not trusted: one zone, and a note.
    raw.cell_zones.pop();
    let out = foam_to_fluent(&raw, &WriteOptions::default()).unwrap();
    assert!(out.text.contains("(45 (1 fluid fluid)())"));
    assert!(out.warnings.iter().any(|w| w.contains("cell zones")));
}

fn tetrahedron() -> RawPolyMesh {
    let p = |x, y, z| Vec3::new(x, y, z);
    RawPolyMesh {
        points: vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(0.0, 1.0, 0.0),
            p(0.0, 0.0, 1.0),
        ],
        faces: FaceList {
            offsets: vec![0, 3, 6, 9, 12],
            points: vec![0, 2, 1, 0, 3, 2, 0, 1, 3, 1, 2, 3],
        },
        owner: vec![0; 4],
        neighbour: vec![],
        patches: vec![RawPatch {
            name: "all".into(),
            kind: "wall".into(),
            start_face: 0,
            n_faces: 4,
            groups: vec![],
            dict: Default::default(),
        }],
        ..Default::default()
    }
}

fn pyramid() -> RawPolyMesh {
    let p = |x, y, z| Vec3::new(x, y, z);
    let patch = |name: &str, start, n| RawPatch {
        name: name.into(),
        kind: "wall".into(),
        start_face: start,
        n_faces: n,
        groups: vec![],
        dict: Default::default(),
    };
    RawPolyMesh {
        points: vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
            p(0.0, 1.0, 0.0),
            p(0.5, 0.5, 1.0),
        ],
        faces: FaceList {
            offsets: vec![0, 4, 7, 10, 13, 16],
            points: vec![0, 3, 2, 1, 0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4],
        },
        owner: vec![0; 5],
        neighbour: vec![],
        patches: vec![patch("base", 0, 1), patch("sides", 1, 4)],
        ..Default::default()
    }
}

#[test]
fn a_tetrahedron_and_a_pyramid_keep_their_shape_and_volume() {
    let tet = tetrahedron();
    let text = written(&tet);
    assert!(text.contains("(12 (1 1 1 1 2))"), "{text}");
    let back = fluent_to_foam(&text, Options::default()).unwrap();
    assert!(close(volume(&back), 1.0 / 6.0));
    same_patches(&tet, &back, &[]);

    let pyr = pyramid();
    let text = written(&pyr);
    assert!(text.contains("(12 (1 1 1 1 5))"), "{text}");
    // The base is a quadrilateral zone, the sides triangles.
    assert!(text.contains("(13 (2 1 1 3 4)("));
    assert!(text.contains("(13 (3 2 5 3 3)("));
    let back = fluent_to_foam(&text, Options::default()).unwrap();
    assert!(close(volume(&back), 1.0 / 3.0));
    same_patches(&pyr, &back, &[]);
}

fn seg(points: &[[f64; 2]], patch: &str) -> Segment {
    Segment {
        points: points.to_vec(),
        patch: patch.to_owned(),
        kind: "patch".to_owned(),
        size: None,
    }
}

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

#[test]
fn a_triangular_2d_mesh_is_written_as_a_2d_file_and_reads_back() {
    let mut region = rectangle(2.0, 1.0);
    region.loops.push(vec![
        seg(&[[0.8, 0.3], [0.8, 0.7]], "cylinder"),
        seg(&[[0.8, 0.7], [1.2, 0.7]], "cylinder"),
        seg(&[[1.2, 0.7], [1.2, 0.3]], "cylinder"),
        seg(&[[1.2, 0.3], [0.8, 0.3]], "cylinder"),
    ]);
    let options = tri2d::Options::new(0.1, Extrusion::Planar { thickness: 1.0 });
    let mut raw = tri2d::mesh(&region, &options).unwrap();
    for p in &mut raw.patches {
        if p.name == "cylinder" || p.name == "top" || p.name == "bottom" {
            p.kind = "wall".into();
        }
    }
    let out = foam_to_fluent(&raw, &WriteOptions::default()).unwrap();
    let text = &out.text;
    assert!(text.lines().any(|l| l == "(2 2)"));
    // Triangles, and lines for faces; no front or back patch.
    let cells = n_cells(&raw);
    assert!(text.contains(&format!("(12 (1 1 {cells:x} 1 1))")));
    assert!(!text.contains("frontAndBack"));
    assert!(
        text.lines()
            .any(|l| l.ends_with(" 2)(") && l.starts_with("(13 ("))
    );

    let back = fluent_to_foam(
        text,
        Options {
            thickness: Some(1.0),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(n_cells(&back), cells);
    assert_eq!(back.points.len(), raw.points.len());
    assert!(close(volume(&back), 2.0 - 0.16));
    assert!(close(volume(&back), volume(&raw)));
    same_patches(&raw, &back, &["frontAndBack", "frontAndBackPlanes"]);
    let kinds: Vec<_> = patch_summary(&back)
        .into_iter()
        .map(|p| (p.0, p.1))
        .collect();
    assert!(kinds.contains(&("cylinder".into(), "wall".into())));
    assert!(kinds.contains(&("inlet".into(), "patch".into())));
    assert_eq!(kinds.last().unwrap().1, "empty");
}

#[test]
fn a_quadrilateral_2d_mesh_reads_back_too() {
    let mut raw = blocks::structured_box_raw(
        Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(3.0, 2.0, 0.5),
        },
        [6, 4, 1],
    )
    .unwrap();
    for p in &mut raw.patches {
        if p.name == "zMin" || p.name == "zMax" {
            p.kind = "empty".into();
        }
    }
    // The empty patches are not the last ones: the writer does not mind.
    let text = written(&raw);
    assert!(text.lines().any(|l| l == "(2 2)"));
    assert!(text.contains("(12 (1 1 18 1 3))"), "{text}");
    let back = fluent_to_foam(
        &text,
        Options {
            thickness: Some(0.5),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(n_cells(&back), 24);
    assert!(close(volume(&back), 3.0));
    same_patches(&raw, &back, &["zMin", "zMax", "frontAndBackPlanes"]);
}

#[test]
fn what_cannot_be_written_is_refused_with_the_reason() {
    let refused = |raw: &RawPolyMesh| {
        foam_to_fluent(raw, &WriteOptions::default())
            .unwrap_err()
            .to_string()
    };
    let mut raw = the_box([2, 2, 2]);
    raw.patches[0].kind = "cyclic".into();
    assert!(refused(&raw).contains("periodic"), "{}", refused(&raw));

    let mut raw = the_box([2, 2, 2]);
    raw.patches[0].kind = "wedge".into();
    assert!(refused(&raw).contains("wedge"));

    let mut raw = the_box([2, 2, 2]);
    raw.patches[0].kind = "turbo".into();
    assert!(refused(&raw).contains("turbo"));

    // Empty faces on one side only: not a mesh one cell thick.
    let mut raw = the_box([2, 2, 2]);
    raw.patches[4].kind = "empty".into();
    assert!(refused(&raw).contains("empty face"), "{}", refused(&raw));

    // One cell thick but sheared: the back is not under the front.
    let mut raw = blocks::structured_box_raw(
        Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(1.0, 1.0, 1.0),
        },
        [2, 2, 1],
    )
    .unwrap();
    raw.patches[4].kind = "empty".into();
    raw.patches[5].kind = "empty".into();
    for p in &mut raw.points {
        if p.z > 0.5 {
            p.x += 0.25;
        }
    }
    assert!(refused(&raw).contains("not straight"), "{}", refused(&raw));

    // A mesh that is not a mesh is the mesh reader's error.
    let mut raw = the_box([1, 1, 1]);
    raw.owner.pop();
    assert!(foam_to_fluent(&raw, &WriteOptions::default()).is_err());
}

#[test]
fn a_mesh_written_twice_gives_the_same_file() {
    let raw = the_box([3, 3, 2]);
    assert_eq!(written(&raw), written(&raw));
}
