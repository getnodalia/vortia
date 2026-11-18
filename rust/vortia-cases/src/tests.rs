use std::path::Path;
use std::process::Command;

use vortia_mesh::{MeshGeometry, PolyMesh, check};

use super::*;

fn nozzle(kind: NozzleKind) -> Nozzle {
    Nozzle::new(kind)
}

/// A coarse nozzle that a test can run a few iterations of.
fn coarse(kind: NozzleKind) -> Nozzle {
    Nozzle {
        converging_segments: 4,
        diverging_segments: 6,
        axial_cells_per_segment: 2,
        radial_cells: 8,
        ..nozzle(kind)
    }
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap()
}

fn end_at(dir: &Path, iterations: usize) {
    let path = dir.join("system/controlDict");
    let text = read(dir, "system/controlDict");
    let text: Vec<String> = text
        .lines()
        .map(|line| match line.split_whitespace().next() {
            Some("endTime" | "writeInterval") => {
                let key = line.split_whitespace().next().unwrap_or_default();
                format!("{key} {iterations};")
            }
            _ => line.to_owned(),
        })
        .collect();
    std::fs::write(path, text.join("\n")).unwrap();
}

#[test]
fn a_cavity_is_meshed_with_a_lid_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let cavity = Cavity {
        cells: 12,
        ..Cavity::default()
    };
    let made = create(dir.path(), &Geometry::Cavity(cavity)).unwrap();
    assert_eq!(made.cells, 12 * 12);
    assert_eq!(made.application, "simpleFoam");
    let boundary = std::fs::read_to_string(dir.path().join("constant/polyMesh/boundary")).unwrap();
    for patch in ["movingWall", "fixedWalls", "frontAndBack"] {
        assert!(boundary.contains(patch), "no patch {patch}");
    }
    let velocity = std::fs::read_to_string(dir.path().join("0/U")).unwrap();
    assert!(
        velocity.contains("uniform (1 0 0)"),
        "the lid moves at 1 m/s"
    );
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let volume = MeshGeometry::compute(&mesh).total_volume();
    assert!(
        (volume - 0.1).abs() < 1e-12,
        "the box is 1 × 1 × 0.1: {volume}"
    );
    end_at(dir.path(), 5);
    let case = vortia_solvers::Case::open(dir.path()).unwrap();
    vortia_solvers::run(&case, |_| {}).unwrap();
    assert!(dir.path().join("5").is_dir());
}

#[test]
fn a_plate_is_meshed_with_a_slip_wall_ahead_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let plate = Plate {
        cells_x: 10,
        cells_up: 4,
        cells_y: 8,
        ..Plate::default()
    };
    let made = create(dir.path(), &Geometry::Plate(plate)).unwrap();
    assert_eq!(made.cells, (10 + 4) * 8);
    assert_eq!(made.application, "simpleFoam");
    let boundary = std::fs::read_to_string(dir.path().join("constant/polyMesh/boundary")).unwrap();
    for patch in ["inlet", "outlet", "slip", "plate", "top", "frontAndBack"] {
        assert!(boundary.contains(patch), "no patch {patch}");
    }
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let volume = MeshGeometry::compute(&mesh).total_volume();
    let expected = 1.2 * 0.5 * 0.01;
    assert!(
        (volume - expected).abs() < 1e-12,
        "the box is 1.2 × 0.5 × 0.01: {volume}"
    );
    end_at(dir.path(), 5);
    let case = vortia_solvers::Case::open(dir.path()).unwrap();
    vortia_solvers::run(&case, |_| {}).unwrap();
    assert!(dir.path().join("5").is_dir());
}

#[test]
fn a_turbulent_plate_starts_at_the_leading_edge_with_the_fields_of_k_omega_sst() {
    let dir = tempfile::tempdir().unwrap();
    let plate = Plate {
        cells_x: 20,
        cells_y: 12,
        ..Plate::turbulent()
    };
    let made = create(dir.path(), &Geometry::Plate(plate)).unwrap();
    assert_eq!(made.cells, 20 * 12);
    assert_eq!(made.application, "simpleFoam");
    let boundary = std::fs::read_to_string(dir.path().join("constant/polyMesh/boundary")).unwrap();
    for patch in ["inlet", "outlet", "plate", "freestream", "frontAndBack"] {
        assert!(boundary.contains(patch), "no patch {patch}");
    }
    assert!(!boundary.contains("slip"));
    for file in ["0/k", "0/omega", "0/nut"] {
        assert!(dir.path().join(file).is_file(), "{file}");
    }
    let turbulence =
        std::fs::read_to_string(dir.path().join("constant/turbulenceProperties")).unwrap();
    assert!(turbulence.contains("kOmegaSST"));
    let volume = MeshGeometry::compute(&PolyMesh::read_case(dir.path()).unwrap()).total_volume();
    assert!((volume - 10.0 * 2.0 * 0.1).abs() < 1e-9, "{volume}");
    end_at(dir.path(), 3);
    let case = vortia_solvers::Case::open(dir.path()).unwrap();
    vortia_solvers::run(&case, |_| {}).unwrap();
    assert!(dir.path().join("3").is_dir());
    // Numbers that cannot work are refused.
    let mut bad = Plate::turbulent();
    bad.intensity = 2.0;
    let other = tempfile::tempdir().unwrap();
    assert!(create(other.path(), &Geometry::Plate(bad)).is_err());
}

#[test]
fn a_channel_is_meshed_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let made = create(dir.path(), &Geometry::Channel(Channel::default())).unwrap();
    assert_eq!(made.cells, 100 * 20);
    assert_eq!(made.points, 101 * 21 * 2);
    assert_eq!(made.application, "simpleFoam");
    for file in [
        "system/blockMeshDict",
        "system/controlDict",
        "system/fvSchemes",
        "system/fvSolution",
        "constant/polyMesh/points",
        "constant/polyMesh/boundary",
        "constant/transportProperties",
        "0/U",
        "0/p",
    ] {
        assert!(dir.path().join(file).is_file(), "{file} is missing");
    }

    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let names: Vec<&str> = mesh.patches().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["inlet", "outlet", "walls", "frontAndBack"]);
    let geo = MeshGeometry::compute(&mesh);
    assert!(check(&mesh, &geo).is_ok());
    // length × height × depth
    assert!((geo.total_volume() - 1.0 * 0.1 * 0.01).abs() < 1e-12);

    end_at(dir.path(), 30);
    let case = vortia_solvers::Case::open(dir.path()).unwrap();
    vortia_solvers::run(&case, |_| {}).unwrap();
    assert!(dir.path().join("30/U").is_file());
}

#[test]
fn both_nozzles_make_the_meshes_the_qt_application_does() {
    for kind in [NozzleKind::Planar, NozzleKind::Axisymmetric] {
        let dir = tempfile::tempdir().unwrap();
        let made = create(dir.path(), &Geometry::Nozzle(nozzle(kind))).unwrap();
        assert_eq!(made.cells, (18 + 30) * 3 * 40, "{kind:?}");
        assert_eq!(made.application, "rhoSimpleFoam");

        let mesh = PolyMesh::read_case(dir.path()).unwrap();
        let geo = MeshGeometry::compute(&mesh);
        let report = check(&mesh, &geo);
        assert!(
            report.unsolvable().is_empty(),
            "{kind:?}: {:?}",
            report.unsolvable()
        );
        for field in ["U", "p", "T", "k", "omega", "nut", "alphat"] {
            let text = read(dir.path(), &format!("0/{field}"));
            for patch in mesh.patches() {
                assert!(
                    text.contains(&patch.name),
                    "0/{field} has no '{}'",
                    patch.name
                );
            }
        }
    }
}

#[test]
fn the_nozzle_volume_is_that_of_its_profile() {
    // The wall is piecewise linear between stations, so the volume is the sum
    // of truncated cones (axisymmetric, scaled by the wedge's share of the
    // turn) or of trapezoids (planar).
    let n = nozzle(NozzleKind::Planar);
    let stations = n.stations();
    let area: f64 = stations
        .windows(2)
        .map(|w| (w[1].0 - w[0].0) * (w[0].1 + w[1].1) / 2.0)
        .sum();
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), &Geometry::Nozzle(n.clone())).unwrap();
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let volume = MeshGeometry::compute(&mesh).total_volume();
    let expected = 2.0 * area * n.thickness * 1.0e-9;
    assert!(
        (volume - expected).abs() < 1e-6 * expected,
        "{volume} against {expected}"
    );

    let n = nozzle(NozzleKind::Axisymmetric);
    let cones: f64 = n
        .stations()
        .windows(2)
        .map(|w| {
            let (h, a, b) = (w[1].0 - w[0].0, w[0].1, w[1].1);
            std::f64::consts::PI * h * (a * a + a * b + b * b) / 3.0
        })
        .sum();
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), &Geometry::Nozzle(n.clone())).unwrap();
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let volume = MeshGeometry::compute(&mesh).total_volume();
    // The chord of the wedge's wall is a little inside the arc: the section is
    // r·cos(α/2) wide at the wall, so the volume is below the exact wedge by
    // O(α²), a fraction of a percent at 5°.
    let wedge = cones * 1.0e-9 * n.wedge_angle.to_radians() / (2.0 * std::f64::consts::PI);
    assert!(
        volume < wedge && volume > 0.995 * wedge,
        "{volume} against {wedge}"
    );
}

/// A rounded-throat nozzle drawn as a sketch and solved.
fn sketched() -> vortia_sketch::Sketch {
    let mut s = vortia_sketch::templates::rounded_nozzle(50.0, 26.0, 60.0, 158.0, 420.0, 30.0);
    s.solve().unwrap();
    s
}

#[test]
fn a_nozzle_drawn_in_a_sketch_is_meshed_with_the_volume_of_its_wall() {
    let sketch = sketched();
    let wall = sketch.profile(0.25).unwrap();
    // The fine wall, as the area under it: trapezoids between its points.
    let exact: f64 = wall
        .windows(2)
        .map(|w| (w[1].0 - w[0].0) * (w[0].1 + w[1].1) / 2.0)
        .sum();
    let n = Nozzle::from_sketch(NozzleKind::Planar, &sketch, 60).unwrap();
    assert_eq!(n.stations().len(), 61);
    assert!((n.inlet_diameter - 100.0).abs() < 1e-9);
    assert!((n.outlet_diameter - 120.0).abs() < 1e-9);
    assert!(
        (n.throat_diameter - 52.0).abs() < 0.05,
        "{}",
        n.throat_diameter
    );
    let dir = tempfile::tempdir().unwrap();
    let made = create(dir.path(), &Geometry::Nozzle(n.clone())).unwrap();
    assert_eq!(made.cells, 60 * n.axial_cells_per_segment * n.radial_cells);
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let volume = MeshGeometry::compute(&mesh).total_volume();
    // Sixty chords against a quarter-degree arc: the area differs by a fraction of a percent.
    let expected = 2.0 * exact * n.thickness * 1.0e-9;
    assert!(
        (volume - expected).abs() < 3.0e-3 * expected,
        "{volume} against {expected}"
    );
}

#[test]
fn a_sketched_nozzle_runs_both_ways_and_a_bad_wall_is_refused() {
    for kind in [NozzleKind::Planar, NozzleKind::Axisymmetric] {
        let n = Nozzle {
            axial_cells_per_segment: 2,
            radial_cells: 8,
            ..Nozzle::from_sketch(kind, &sketched(), 12).unwrap()
        };
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), &Geometry::Nozzle(n)).unwrap();
        end_at(dir.path(), 2);
        let case = vortia_solvers::Case::open(dir.path()).unwrap();
        vortia_solvers::run(&case, |_| {}).unwrap();
    }
    // A wall that is not a chain from the inlet to the outlet makes no mesh.
    let mut s = vortia_sketch::Sketch::default();
    let (a, b, c) = (s.point(0.0, 1.0), s.point(2.0, 1.0), s.point(1.0, 1.0));
    s.line(a, b);
    s.line(b, c);
    assert!(Nozzle::from_sketch(NozzleKind::Planar, &s, 10).is_err());
    assert!(Nozzle::from_sketch(NozzleKind::Planar, &sketched(), 1).is_err());
}

#[test]
fn a_coarse_nozzle_runs() {
    for kind in [NozzleKind::Planar, NozzleKind::Axisymmetric] {
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), &Geometry::Nozzle(coarse(kind))).unwrap();
        end_at(dir.path(), 3);
        let case = vortia_solvers::Case::open(dir.path()).unwrap();
        vortia_solvers::run(&case, |_| {}).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        assert!(dir.path().join("3/p").is_file(), "{kind:?}");
    }
}

#[test]
fn what_cannot_make_a_case_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("case");
    let bad_channels = [
        Channel {
            length: 0.0,
            ..Channel::default()
        },
        Channel {
            height: f64::NAN,
            ..Channel::default()
        },
        Channel {
            cells_x: 0,
            ..Channel::default()
        },
        Channel {
            cells_x: 10_000,
            cells_y: 10_000,
            ..Channel::default()
        },
        Channel {
            speed: -1.0,
            ..Channel::default()
        },
        Channel {
            viscosity: f64::INFINITY,
            ..Channel::default()
        },
    ];
    for c in bad_channels {
        assert!(create(&target, &Geometry::Channel(c)).is_err());
    }
    let bad_cavities = [
        Cavity {
            cells: 0,
            ..Cavity::default()
        },
        Cavity {
            cells: 10_000,
            ..Cavity::default()
        },
        Cavity {
            lid_speed: f64::NAN,
            ..Cavity::default()
        },
        Cavity {
            side: -1.0,
            ..Cavity::default()
        },
        Cavity {
            viscosity: 0.0,
            ..Cavity::default()
        },
    ];
    for c in bad_cavities {
        assert!(create(&target, &Geometry::Cavity(c)).is_err());
    }
    let bad_plates = [
        Plate {
            length: 0.0,
            ..Plate::default()
        },
        Plate {
            upstream: -1.0,
            ..Plate::default()
        },
        Plate {
            cells_up: 0,
            ..Plate::default()
        },
        Plate {
            cells_x: 10_000,
            cells_y: 10_000,
            ..Plate::default()
        },
        Plate {
            speed: f64::NAN,
            ..Plate::default()
        },
        Plate {
            viscosity: f64::INFINITY,
            ..Plate::default()
        },
    ];
    for p in bad_plates {
        assert!(create(&target, &Geometry::Plate(p)).is_err());
    }
    for kind in [NozzleKind::Planar, NozzleKind::Axisymmetric] {
        let base = nozzle(kind);
        let mut bad = vec![
            Nozzle {
                throat_diameter: 200.0,
                ..base.clone()
            },
            Nozzle {
                inlet_diameter: 0.0,
                ..base.clone()
            },
            Nozzle {
                converging_segments: 1,
                ..base.clone()
            },
            Nozzle {
                radial_cells: 1000,
                diverging_segments: 200,
                converging_segments: 200,
                axial_cells_per_segment: 200,
                ..base.clone()
            },
            Nozzle {
                radial_grading: f64::NAN,
                ..base.clone()
            },
            Nozzle {
                outlet_pressure: -5.0,
                ..base.clone()
            },
        ];
        match kind {
            NozzleKind::Planar => {
                bad.push(Nozzle {
                    thickness: 0.0,
                    ..base.clone()
                });
                bad.push(Nozzle {
                    radial_cells: 41,
                    ..base.clone()
                });
            }
            NozzleKind::Axisymmetric => bad.push(Nozzle {
                wedge_angle: 6.0,
                ..base.clone()
            }),
        }
        for n in bad {
            assert!(
                create(&target, &Geometry::Nozzle(n.clone())).is_err(),
                "{n:?}"
            );
        }
    }
    // Nothing was written by any of them.
    assert!(!target.exists());
}

#[test]
fn a_folder_with_something_in_it_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "keep").unwrap();
    let err = create(dir.path(), &Geometry::Channel(Channel::default())).unwrap_err();
    assert!(err.to_string().contains("not empty"));
    assert_eq!(read(dir.path(), "notes.txt"), "keep");
    assert!(!dir.path().join("system").exists());
}

/// OpenFOAM's `blockMesh` and `checkMesh` on the same dictionary: the same
/// points, faces and cells. Skipped where OpenFOAM is not installed.
#[test]
fn openfoam_makes_the_same_meshes() {
    if Command::new("blockMesh").arg("-help").output().is_err() {
        eprintln!("skipped: blockMesh is not installed");
        return;
    }
    let geometries = [
        Geometry::Channel(Channel::default()),
        Geometry::Cavity(Cavity::default()),
        Geometry::Plate(Plate::default()),
        Geometry::Nozzle(nozzle(NozzleKind::Planar)),
        Geometry::Nozzle(nozzle(NozzleKind::Axisymmetric)),
    ];
    for geometry in geometries {
        let dir = tempfile::tempdir().unwrap();
        let made = create(dir.path(), &geometry).unwrap();
        let mesh = PolyMesh::read_case(dir.path()).unwrap();
        std::fs::remove_dir_all(dir.path().join("constant/polyMesh")).unwrap();
        let run = |tool: &str| {
            let out = Command::new(tool)
                .arg("-case")
                .arg(dir.path())
                .env("WM_PROJECT_DIR", "/usr/share/openfoam")
                .env("FOAM_ETC", "/usr/share/openfoam/etc")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{tool}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        run("blockMesh");
        let log = run("checkMesh");
        let number = |label: &str| -> usize {
            log.lines()
                .find_map(|l| l.trim().strip_prefix(label))
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("no '{label}' in:\n{log}"))
        };
        assert_eq!(number("cells:"), made.cells, "{}", geometry.title());
        assert_eq!(number("points:"), made.points, "{}", geometry.title());
        assert_eq!(number("faces:"), mesh.n_faces(), "{}", geometry.title());
        assert_eq!(
            number("internal faces:"),
            mesh.n_internal_faces(),
            "{}",
            geometry.title()
        );
        assert!(log.contains("Mesh OK"), "{}:\n{log}", geometry.title());
    }
}

#[test]
fn settings_are_set_by_name_and_only_whole_counts_are_counts() {
    let mut c = Channel::default();
    c.set("height", 0.2).unwrap();
    c.set("cells-y", 40.0).unwrap();
    assert_eq!((c.height, c.cells_y), (0.2, 40));
    assert!(c.set("cells-y", 2.5).is_err());
    assert!(c.set("cells-y", -1.0).is_err());
    assert!(c.set("inlet-speed", 1.0).is_err());

    let mut n = nozzle(NozzleKind::Axisymmetric);
    n.set("throat-diameter", 40.0).unwrap();
    n.set("radial-cells", 30.0).unwrap();
    assert_eq!((n.throat_diameter, n.radial_cells), (40.0, 30));
    assert!(n.set("radial-cells", f64::NAN).is_err());
    assert!(n.set("height", 1.0).is_err());
}
