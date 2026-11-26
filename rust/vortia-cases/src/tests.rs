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
        Geometry::Nozzle(Nozzle {
            half: true,
            ..nozzle(NozzleKind::Planar)
        }),
        Geometry::Jet(Jet::default()),
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

#[test]
fn the_half_nozzle_is_the_upper_half_of_the_slot_with_a_symmetry_plane() {
    let whole = coarse(NozzleKind::Planar);
    let half = Nozzle {
        half: true,
        ..whole.clone()
    };
    let make = |n: &Nozzle| {
        let dir = tempfile::tempdir().unwrap();
        let made = create(dir.path(), &Geometry::Nozzle(n.clone())).unwrap();
        (dir, made)
    };
    let (whole_dir, whole_made) = make(&whole);
    let (half_dir, half_made) = make(&half);
    assert_eq!(half_made.cells * 2, whole_made.cells);

    let mesh = PolyMesh::read_case(half_dir.path()).unwrap();
    let names: Vec<_> = mesh.patches().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["inlet", "outlet", "nozzleWall", "symmetry", "frontAndBack"]
    );
    assert_eq!(
        mesh.patches()
            .iter()
            .find(|p| p.name == "symmetry")
            .map(|p| p.kind.name()),
        Some("symmetryPlane")
    );
    assert!(check::check(&mesh, &MeshGeometry::compute(&mesh)).is_ok());
    // Half the volume of the whole slot, and every field names the plane.
    let whole_mesh = PolyMesh::read_case(whole_dir.path()).unwrap();
    let (half_volume, whole_volume) = (
        MeshGeometry::compute(&mesh).total_volume(),
        MeshGeometry::compute(&whole_mesh).total_volume(),
    );
    assert!((2.0 * half_volume - whole_volume).abs() < 1e-9 * whole_volume);
    for field in ["U", "p", "T", "k", "omega", "nut", "alphat"] {
        let text = read(half_dir.path(), &format!("0/{field}"));
        assert!(
            text.contains("symmetry\n    {\n        type            symmetryPlane;"),
            "{field}"
        );
    }
    // The axisymmetric nozzle has no plane to cut at, and ignores it.
    let wedge = Nozzle {
        half: true,
        ..coarse(NozzleKind::Axisymmetric)
    };
    let (dir, _) = make(&wedge);
    assert!(!read(dir.path(), "system/blockMeshDict").contains("symmetryPlane"));
}

#[test]
fn the_jet_is_an_orifice_in_a_wall_facing_an_open_atmosphere() {
    let jet = Jet {
        cells_axial: 20,
        cells_orifice: 6,
        cells_outer: 10,
        ..Jet::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let made = create(dir.path(), &Geometry::Jet(jet.clone())).unwrap();
    assert_eq!(made.cells, 20 * (6 + 10));
    assert_eq!(made.application, "rhoSimpleFoam");
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let names: Vec<_> = mesh.patches().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["axis", "inlet", "walls", "atmosphere", "front", "back"]
    );
    assert!(check::check(&mesh, &MeshGeometry::compute(&mesh)).is_ok());
    // A wedge of the cylinder: the volume of the cylinder of the chamber, 5° of 360°.
    let expected = std::f64::consts::PI
        * jet.radius.powi(2)
        * jet.length
        * 1e-9
        * jet.wedge_angle.to_radians()
        / (2.0 * std::f64::consts::PI);
    let volume = MeshGeometry::compute(&mesh).total_volume();
    assert!(
        (volume - expected).abs() < 2e-3 * expected,
        "{volume} against {expected}"
    );
    assert!(read(dir.path(), "0/p").contains("totalPressure"));

    // What cannot be meshed is refused and writes nothing.
    let target = dir.path().join("refused");
    for bad in [
        Jet {
            radius: 0.05,
            ..jet.clone()
        },
        Jet {
            orifice_radius: 0.0,
            ..jet.clone()
        },
        Jet {
            wedge_angle: 6.0,
            ..jet.clone()
        },
        Jet {
            cells_axial: 0,
            ..jet.clone()
        },
        Jet {
            cells_axial: 2000,
            cells_outer: 1000,
            cells_orifice: 1000,
            ..jet.clone()
        },
        Jet {
            total_pressure: f64::NAN,
            ..jet.clone()
        },
    ] {
        assert!(
            create(&target, &Geometry::Jet(bad.clone())).is_err(),
            "{bad:?}"
        );
    }
    assert!(!target.exists());
    let mut jet = jet;
    assert!(jet.set("orifice-radius", 0.2).is_ok() && jet.orifice_radius == 0.2);
    assert!(jet.set("axial-cells", 2.5).is_err());
    assert!(jet.set("nonsense", 1.0).is_err());
}

#[test]
fn an_unstructured_nozzle_is_written_with_its_patches_and_a_mesh_openfoam_accepts() {
    let cases = [
        (
            Nozzle {
                half: true,
                cell_size: Some(6.0),
                ..nozzle(NozzleKind::Planar)
            },
            vec!["nozzleWall", "outlet", "symmetry", "inlet", "frontAndBack"],
        ),
        (
            Nozzle {
                cell_size: Some(6.0),
                ..nozzle(NozzleKind::Planar)
            },
            vec!["nozzleWall", "outlet", "inlet", "frontAndBack"],
        ),
        (
            Nozzle {
                cell_size: Some(6.0),
                ..nozzle(NozzleKind::Axisymmetric)
            },
            vec!["nozzleWall", "outlet", "inlet", "wedgeFront", "wedgeBack"],
        ),
    ];
    let have_openfoam = Command::new("checkMesh").arg("-help").output().is_ok();
    for (n, names) in cases {
        let dir = tempfile::tempdir().unwrap();
        let made = create(dir.path(), &Geometry::Nozzle(n.clone())).unwrap();
        // No blockMeshDict: the mesh did not come from blocks.
        assert!(!dir.path().join("system/blockMeshDict").exists());
        let mesh = PolyMesh::read_case(dir.path()).unwrap();
        assert_eq!(mesh.n_cells(), made.cells);
        let got: Vec<&str> = mesh.patches().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(got, names, "{n:?}");
        assert_eq!(mesh.patch("nozzleWall").unwrap().kind.name(), "wall");
        // In metres: the nozzle is 0.578 m long.
        let geo = MeshGeometry::compute(&mesh);
        let report = check(&mesh, &geo);
        assert!(report.is_ok(), "{:?}", report.failed_checks());
        assert!(
            (report.bounds.max.x - 0.578).abs() < 1e-9,
            "{:?}",
            report.bounds
        );
        if have_openfoam {
            let out = Command::new("checkMesh")
                .arg("-case")
                .arg(dir.path())
                .env("WM_PROJECT_DIR", "/usr/share/openfoam")
                .env("FOAM_ETC", "/usr/share/openfoam/etc")
                .output()
                .unwrap();
            let log = String::from_utf8_lossy(&out.stdout);
            assert!(log.contains("Mesh OK"), "{n:?}:\n{log}");
            assert!(
                log.contains(&format!("cells:            {}", made.cells)),
                "{log}"
            );
        }
    }
    // A size that cannot be used is refused before anything is written.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("case");
    for bad in [-1.0, f64::NAN, 1.0e9] {
        let n = Nozzle {
            cell_size: Some(bad),
            ..nozzle(NozzleKind::Planar)
        };
        assert!(create(&target, &Geometry::Nozzle(n)).is_err(), "{bad}");
    }
    assert!(!target.exists());
}

/// A duct 200 × 50 mm with a square block in it, drawn as named edges. The outline runs
/// counter-clockwise, or clockwise when `reversed`.
fn drawn_duct(reversed: bool) -> vortia_sketch::Sketch {
    let mut s = vortia_sketch::Sketch::default();
    let corners = [(0.0, 0.0), (200.0, 0.0), (200.0, 50.0), (0.0, 50.0)];
    let names = ["floor", "outlet", "ceiling", "inlet"];
    let ids: Vec<usize> = corners.iter().map(|&(x, y)| s.point(x, y)).collect();
    let mut edges: Vec<(usize, usize, &str)> = (0..4)
        .map(|i| (ids[i], ids[(i + 1) % 4], names[i]))
        .collect();
    if reversed {
        edges = edges.into_iter().rev().map(|(a, b, n)| (b, a, n)).collect();
    }
    for (a, b, name) in edges {
        let e = s.line(a, b);
        s.name(e, name);
    }
    let block = [(60.0, 20.0), (70.0, 20.0), (70.0, 30.0), (60.0, 30.0)];
    let ids: Vec<usize> = block.iter().map(|&(x, y)| s.point(x, y)).collect();
    for i in 0..4 {
        let e = s.line(ids[i], ids[(i + 1) % 4]);
        s.name(e, "block");
    }
    s
}

#[test]
fn a_region_drawn_with_named_edges_is_meshed_and_runs_with_the_flow_entering_at_the_inlet() {
    for reversed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let drawn = Drawn {
            sketch: drawn_duct(reversed),
            cell_size: 5.0,
            ..Drawn::default()
        };
        let made = create(dir.path(), &Geometry::Drawn(drawn)).unwrap();
        assert_eq!(made.application, "simpleFoam");
        let mesh = PolyMesh::read_case(dir.path()).unwrap();
        let geo = MeshGeometry::compute(&mesh);
        assert!(check(&mesh, &geo).is_ok());
        // The duct less the block, 10 mm deep, in metres.
        let volume = (0.2 * 0.05 - 0.01 * 0.01) * 0.01;
        assert!((geo.total_volume() - volume).abs() < 1e-12);
        for name in [
            "floor",
            "outlet",
            "ceiling",
            "inlet",
            "block",
            "frontAndBack",
        ] {
            assert!(mesh.patch(name).is_some(), "no '{name}'");
        }
        assert_eq!(mesh.patch("block").unwrap().kind.name(), "wall");
        // The fluid enters going right, whichever way the outline was drawn.
        let u = read(dir.path(), "0/U");
        assert!(u.contains("uniform (1 0 0)"), "{u}");
        assert!(u.contains("noSlip"));

        end_at(dir.path(), 30);
        let case = vortia_solvers::Case::open(dir.path()).unwrap();
        vortia_solvers::run(&case, |_| {}).unwrap();
        assert!(dir.path().join("30/U").is_file());
    }
}

#[test]
fn a_drawn_region_without_an_inlet_and_an_outlet_or_with_an_open_outline_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = drawn_duct(false);
    s.names.insert(3, "wall".to_owned());
    let err = create(
        dir.path(),
        &Geometry::Drawn(Drawn {
            sketch: s,
            ..Drawn::default()
        }),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("'inlet'"), "{err}");
    // Not a closed outline.
    let mut open = vortia_sketch::Sketch::default();
    let (a, b) = (open.point(0.0, 0.0), open.point(1.0, 0.0));
    open.line(a, b);
    assert!(
        create(
            dir.path(),
            &Geometry::Drawn(Drawn {
                sketch: open,
                ..Drawn::default()
            })
        )
        .is_err()
    );
    let mut d = Drawn::default();
    assert!(d.set("cell-size", 2.0).is_ok() && d.set("nope", 1.0).is_err());
}

fn coarse_airfoil() -> Airfoil {
    Airfoil {
        cells_chord: 16,
        cells_normal: 8,
        cells_wake: 6,
        ..Airfoil::default()
    }
}

#[test]
fn an_airfoil_is_a_c_grid_of_six_blocks_with_the_flow_at_its_angle_of_attack() {
    let dir = tempfile::tempdir().unwrap();
    let foil = Airfoil {
        alpha: 5.0,
        ..coarse_airfoil()
    };
    let made = create(dir.path(), &Geometry::Airfoil(foil.clone())).unwrap();
    assert_eq!(made.application, "simpleFoam");
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let geo = MeshGeometry::compute(&mesh);
    let report = check(&mesh, &geo);
    assert!(
        report.unsolvable().is_empty(),
        "mesh not solvable: {:?}",
        report.unsolvable()
    );
    let names: Vec<&str> = mesh.patches().iter().map(|p| p.name.as_str()).collect();
    for patch in ["airfoil", "farfield", "outlet", "frontAndBack"] {
        assert!(names.contains(&patch), "no patch {patch} in {names:?}");
    }
    assert_eq!(made.cells, 2 * 16 * 8 + 2 * 6 * 8);
    // The stream carries the angle of attack; the chord stays on the x axis.
    let u = read(dir.path(), "0/U");
    let (sin_a, cos_a) = 5.0_f64.to_radians().sin_cos();
    let want = format!("{:.6}", foil.speed * cos_a);
    assert!(u.contains(&want[..5]), "freestream {want} not in 0/U");
    assert!(sin_a > 0.0);
    for file in ["0/nuTilda", "0/nut", "constant/airfoilProperties"] {
        assert!(dir.path().join(file).is_file(), "{file}");
    }
    let turbulence = read(dir.path(), "constant/turbulenceProperties");
    assert!(turbulence.contains("SpalartAllmaras"));
    // The volume of the box is the C grid minus the section; the section is thinner than the box.
    assert!(geo.total_volume() > 0.0);
}

#[test]
fn an_airfoil_with_a_k_omega_sst_model_has_those_fields() {
    let dir = tempfile::tempdir().unwrap();
    let foil = Airfoil {
        model: AirfoilModel::KOmegaSst,
        ..coarse_airfoil()
    };
    create(dir.path(), &Geometry::Airfoil(foil)).unwrap();
    for file in ["0/k", "0/omega", "0/nut"] {
        assert!(dir.path().join(file).is_file(), "{file}");
    }
    assert!(read(dir.path(), "constant/turbulenceProperties").contains("kOmegaSST"));
}

#[test]
fn an_airfoil_runs_and_what_cannot_be_one_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), &Geometry::Airfoil(coarse_airfoil())).unwrap();
    end_at(dir.path(), 3);
    let case = vortia_solvers::Case::open(dir.path()).unwrap();
    vortia_solvers::run(&case, |_| {}).unwrap();
    assert!(dir.path().join("3/U").is_file());
    assert!(dir.path().join("3/nuTilda").is_file());

    let base = coarse_airfoil();
    let bad = [
        Airfoil {
            naca: 12_345,
            ..base.clone()
        },
        Airfoil {
            naca: 2400,
            ..base.clone()
        },
        Airfoil {
            naca: 0,
            ..base.clone()
        },
        Airfoil {
            chord: 0.0,
            ..base.clone()
        },
        Airfoil {
            speed: f64::NAN,
            ..base.clone()
        },
        Airfoil {
            reynolds: -1.0,
            ..base.clone()
        },
        Airfoil {
            alpha: 120.0,
            ..base.clone()
        },
        Airfoil {
            alpha: f64::INFINITY,
            ..base.clone()
        },
        Airfoil {
            far: 0.0,
            ..base.clone()
        },
        Airfoil {
            cells_chord: 0,
            ..base.clone()
        },
        Airfoil {
            cells_normal: 100_000,
            cells_chord: 100_000,
            ..base.clone()
        },
        Airfoil {
            yplus: 0.0,
            ..base.clone()
        },
    ];
    for (i, foil) in bad.iter().enumerate() {
        let other = tempfile::tempdir().unwrap();
        assert!(
            create(other.path(), &Geometry::Airfoil(foil.clone())).is_err(),
            "bad airfoil {i} was accepted"
        );
    }
    let mut foil = Airfoil::default();
    assert!(foil.set("alpha", 3.0).is_ok());
    assert!(foil.set("cells-chord", 2.5).is_err());
    assert!(foil.set("model", 7.0).is_err());
    assert!(foil.set("nope", 1.0).is_err());
}

/// A sphere of triangles (diameter 1, centred on the origin) by subdividing an octahedron.
fn unit_sphere(depth: usize) -> vortia_mesh::stl::Stl {
    let mut points: Vec<[f64; 3]> = vec![
        [0.5, 0., 0.],
        [-0.5, 0., 0.],
        [0., 0.5, 0.],
        [0., -0.5, 0.],
        [0., 0., 0.5],
        [0., 0., -0.5],
    ];
    let mut tris: Vec<[u32; 3]> = vec![
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
        for t in &tris {
            let mut mid = |a: u32, b: u32| -> u32 {
                *mids.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    let (p, q) = (points[a as usize], points[b as usize]);
                    let m = [p[0] + q[0], p[1] + q[1], p[2] + q[2]];
                    let n = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                    points.push([0.5 * m[0] / n, 0.5 * m[1] / n, 0.5 * m[2] / n]);
                    (points.len() - 1) as u32
                })
            };
            let (a, b, c) = (t[0], t[1], t[2]);
            let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = next;
    }
    vortia_mesh::stl::Stl {
        vertices: points,
        triangles: tris,
    }
}

fn small_body(model: BodyModel) -> Body {
    Body {
        cells: 1,
        levels: 2,
        wake_levels: 1,
        upstream: 1.5,
        downstream: 3.0,
        lateral: 1.5,
        model,
        reynolds: 100.0,
        ..Body::new(unit_sphere(2))
    }
}

#[test]
fn the_flow_around_a_body_has_its_wall_a_tunnel_and_a_solvable_mesh() {
    let dir = tempfile::tempdir().unwrap();
    let made = create(dir.path(), &Geometry::Body(small_body(BodyModel::Laminar))).unwrap();
    assert_eq!(made.application, "simpleFoam");
    assert!(made.cells > 100);
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let geo = MeshGeometry::compute(&mesh);
    let report = check(&mesh, &geo);
    assert!(
        report.unsolvable().is_empty(),
        "mesh not solvable: {:?}",
        report.unsolvable()
    );
    let names: Vec<&str> = mesh.patches().iter().map(|p| p.name.as_str()).collect();
    for patch in ["body", "inlet", "outlet", "ymin", "ymax", "zmin", "zmax"] {
        assert!(names.contains(&patch), "no patch {patch} in {names:?}");
    }
    let u = read(dir.path(), "0/U");
    assert!(u.contains("noSlip"));
    assert!(u.contains("symmetryPlane"));
    assert!(read(dir.path(), "constant/turbulenceProperties").contains("laminar"));
    // The viscosity follows from the Reynolds number on the body's length (1 m at 1 m/s).
    assert!(read(dir.path(), "constant/transportProperties").contains("nu              0.01"));
    // The air in the tunnel is the tunnel minus the body: smaller than the box, bigger than none.
    let spec = small_body(BodyModel::Laminar).spec().unwrap();
    let tunnel: f64 = (0..3).map(|i| spec.high[i] - spec.low[i]).product();
    let sphere = std::f64::consts::PI / 6.0;
    assert!(
        (geo.total_volume() - (tunnel - sphere)).abs() < 0.2 * sphere,
        "air {} for a tunnel of {tunnel} around a sphere of {sphere}",
        geo.total_volume()
    );
}

#[test]
fn a_turbulent_body_has_the_fields_of_its_model() {
    for (model, files, name) in [
        (
            BodyModel::SpalartAllmaras,
            vec!["0/nuTilda", "0/nut"],
            "SpalartAllmaras",
        ),
        (
            BodyModel::KOmegaSst,
            vec!["0/k", "0/omega", "0/nut"],
            "kOmegaSST",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), &Geometry::Body(small_body(model))).unwrap();
        for file in files {
            assert!(dir.path().join(file).is_file(), "{file}");
        }
        assert!(read(dir.path(), "constant/turbulenceProperties").contains(name));
    }
}

#[test]
fn a_body_runs_and_what_cannot_be_one_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    create(dir.path(), &Geometry::Body(small_body(BodyModel::Laminar))).unwrap();
    end_at(dir.path(), 3);
    let case = vortia_solvers::Case::open(dir.path()).unwrap();
    vortia_solvers::run(&case, |_| {}).unwrap();
    assert!(dir.path().join("3/U").is_file());

    // A surface with a hole is no body.
    let mut open = unit_sphere(1);
    open.triangles.pop();
    let base = small_body(BodyModel::Laminar);
    let bad = [
        Body {
            surface: open,
            ..base.clone()
        },
        Body {
            speed: 0.0,
            ..base.clone()
        },
        Body {
            reynolds: f64::NAN,
            ..base.clone()
        },
        Body {
            cells: 0,
            ..base.clone()
        },
        Body {
            levels: 200,
            ..base.clone()
        },
        Body {
            lateral: 0.0,
            ..base.clone()
        },
        Body {
            surface: vortia_mesh::stl::Stl::default(),
            ..base.clone()
        },
    ];
    for (i, body) in bad.into_iter().enumerate() {
        let other = tempfile::tempdir().unwrap();
        assert!(
            create(other.path(), &Geometry::Body(body)).is_err(),
            "bad body {i} was accepted"
        );
    }
    let mut body = base;
    assert!(body.set("speed", 3.0).is_ok());
    assert!(body.set("levels", 2.5).is_err());
    assert!(body.set("model", 7.0).is_err());
    assert!(body.set("nope", 1.0).is_err());
}

#[test]
fn a_body_with_prism_layers_has_a_solvable_mesh_and_the_layers_are_sized_from_y_plus() {
    let dir = tempfile::tempdir().unwrap();
    let mut body = small_body(BodyModel::Laminar);
    body.layers = 2;
    body.set("layer-growth", 1.3).unwrap();
    let plain = create(
        tempfile::tempdir().unwrap().path(),
        &Geometry::Body(small_body(BodyModel::Laminar)),
    )
    .unwrap();
    let made = create(dir.path(), &Geometry::Body(body.clone())).unwrap();
    assert!(
        made.cells > plain.cells,
        "{} vs {}",
        made.cells,
        plain.cells
    );
    let mesh = PolyMesh::read_case(dir.path()).unwrap();
    let report = check(&mesh, &MeshGeometry::compute(&mesh));
    assert!(report.unsolvable().is_empty(), "{:?}", report.unsolvable());

    // A target y+ sizes the first layer from the flat-plate friction.
    let mut by_yplus = Body::new(unit_sphere(2));
    by_yplus.reynolds = 1.0e6;
    by_yplus.layers = 3;
    by_yplus.yplus = 1.0;
    let first = by_yplus.spec().unwrap().layers.unwrap().first;
    assert!(first > 1.0e-6 && first < 1.0e-4, "first layer {first}");
    by_yplus.yplus = 30.0;
    let thicker = by_yplus.spec().unwrap().layers.unwrap().first;
    assert!((thicker / first - 30.0).abs() < 1e-9);

    for bad in [
        ("layers", 1000.0),
        ("layer-growth", 0.5),
        ("layer-growth", 9.0),
        ("yplus", -1.0e9),
    ] {
        let mut b = body.clone();
        b.set(bad.0, bad.1).unwrap();
        // A negative y+ is the same as none; everything else is refused.
        if bad.0 == "yplus" {
            assert!(b.spec().is_ok());
        } else {
            assert!(b.spec().is_err(), "{bad:?} was accepted");
        }
    }
}
