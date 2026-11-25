use std::fmt::Write as _;

use tempfile::TempDir;
use vortia_core::{Aabb, Vec3};
use vortia_mesh::blocks::structured_box_raw;

use crate::{Case, FieldValues, SlicePolygon, StreamlineEnd, line_csv};

/// A 4 × 2 × 1 box of unit cells, written as a case with times `0`.
fn case() -> (TempDir, Case) {
    let dir = TempDir::new().unwrap();
    let bounds = Aabb {
        min: Vec3::new(0.0, 0.0, 0.0),
        max: Vec3::new(4.0, 2.0, 1.0),
    };
    structured_box_raw(bounds, [4, 2, 1])
        .unwrap()
        .write_case(dir.path())
        .unwrap();
    std::fs::create_dir(dir.path().join("0")).unwrap();
    let case = Case::open(dir.path()).unwrap();
    (dir, case)
}

const PATCHES: [&str; 6] = ["xMin", "xMax", "yMin", "yMax", "zMin", "zMax"];

fn write(dir: &TempDir, name: &str, class: &str, internal: &str, patches: &[(&str, &str)]) {
    let mut text = format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class {class};\n    object {name};\n}}\n\
         dimensions [0 0 0 0 0 0 0];\ninternalField {internal};\nboundaryField\n{{\n"
    );
    for p in PATCHES {
        let body = patches
            .iter()
            .find(|(n, _)| *n == p)
            .map_or("type zeroGradient;", |(_, b)| *b);
        let _ = writeln!(text, "    {p} {{ {body} }}");
    }
    text.push_str("}\n");
    std::fs::write(dir.path().join("0").join(name), text).unwrap();
}

/// A scalar field `f(cell centre)`.
fn write_scalar(dir: &TempDir, case: &Case, name: &str, f: impl Fn(Vec3) -> f64) {
    let values: Vec<String> = case
        .geometry()
        .cell_centres
        .iter()
        .map(|&c| f(c).to_string())
        .collect();
    let internal = format!(
        "nonuniform List<scalar> {}({})",
        values.len(),
        values.join(" ")
    );
    write(
        dir,
        name,
        "volScalarField",
        &internal,
        &[("xMax", "type fixedValue; value uniform 5;")],
    );
}

#[test]
fn a_point_is_found_in_the_cell_that_holds_it() {
    let (_dir, case) = case();
    let cell = case.locate(Vec3::new(2.5, 0.5, 0.5)).unwrap();
    let centre = case.geometry().cell_centres[cell];
    assert_eq!((centre.x, centre.y), (2.5, 0.5));
    assert_eq!(case.locate(Vec3::new(4.5, 0.5, 0.5)), None);
    assert_eq!(case.locate(Vec3::new(2.0, 0.5, 2.0)), None);
}

#[test]
fn a_probe_reads_the_cell_around_the_point() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| 10.0 * c.x + c.y);
    let s = case.probe("0", "T", Vec3::new(3.2, 1.7, 0.4)).unwrap();
    assert_eq!(s.value, vec![10.0 * 3.5 + 1.5]);
    let outside = case.probe("0", "T", Vec3::new(9.0, 0.0, 0.0)).unwrap();
    assert!(outside.cell.is_none() && outside.value.is_empty());
}

#[test]
fn a_line_runs_through_the_cells_it_crosses() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| c.x);
    let (values, samples) = case
        .sample_line(
            "0",
            "T",
            Vec3::new(0.25, 1.0, 0.5),
            Vec3::new(3.75, 1.0, 0.5),
            4,
        )
        .unwrap();
    let got: Vec<f64> = samples.iter().map(|s| s.value[0]).collect();
    assert_eq!(got, vec![0.5, 1.5, 2.5, 3.5]);
    assert!((samples[3].distance - 3.5).abs() < 1e-12);
    let csv = line_csv("T", &values, &samples);
    assert!(csv.starts_with("distance,x,y,z,cell,T\n"));
    assert_eq!(csv.lines().count(), 5);
    assert!(
        case.sample_line(
            "0",
            "T",
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            0
        )
        .is_err()
    );
}

#[test]
fn a_patch_is_averaged_by_area() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| c.x);
    // The outlet holds its own value; the inlet has none, so it reads its cells.
    let outlet = case.patch_stats("0", "T", "xMax").unwrap();
    assert_eq!(
        (
            outlet.faces,
            outlet.area,
            outlet.integral[0],
            outlet.average[0]
        ),
        (2, 2.0, 10.0, 5.0)
    );
    let inlet = case.patch_stats("0", "T", "xMin").unwrap();
    assert_eq!(
        (inlet.min[0], inlet.max[0], inlet.average[0]),
        (0.5, 0.5, 0.5)
    );
}

#[test]
fn the_volume_is_averaged_by_cell_volume() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| c.x);
    let v = case.volume_stats("0", "T").unwrap();
    assert_eq!(
        (v.cells, v.volume, v.integral[0], v.average[0]),
        (8, 8.0, 16.0, 2.0)
    );
    assert_eq!((v.min[0], v.max[0]), (0.5, 3.5));
}

#[test]
fn a_vector_field_has_a_value_per_component() {
    let (dir, case) = case();
    write(
        &dir,
        "U",
        "volVectorField",
        "uniform (1 2 3)",
        &[("xMax", "type fixedValue; value uniform (4 5 6);")],
    );
    let v = case.volume_stats("0", "U").unwrap();
    assert_eq!(v.average, vec![1.0, 2.0, 3.0]);
    let p = case.patch_stats("0", "U", "xMax").unwrap();
    assert_eq!(p.integral, vec![8.0, 10.0, 12.0]);
    assert!(matches!(case.field("0", "U").unwrap(), FieldValues::Vector(v) if v.len() == 8));
}

#[test]
fn the_flux_through_a_patch_is_the_sum_of_its_faces() {
    let (dir, case) = case();
    let n = case.mesh().n_internal_faces();
    write(
        &dir,
        "phi",
        "surfaceScalarField",
        &format!("uniform 0 /* {n} */"),
        &[(
            "xMax",
            "type calculated; value nonuniform List<scalar> 2(0.5 0.25);",
        )],
    );
    assert_eq!(case.flux("0", "xMax").unwrap(), 0.75);
}

#[test]
fn names_that_are_not_there_are_refused_with_what_is() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| c.x);
    let e = case.patch_stats("0", "T", "inlet").unwrap_err().to_string();
    assert!(e.contains("no patch 'inlet'") && e.contains("xMax"), "{e}");
    assert!(case.field("0", "p").is_err());
    assert!(case.field("../0", "T").is_err());
    assert!(case.field("0", "../T").is_err());
    assert!(case.field("7", "T").is_err());
    assert_eq!(case.field_names("0").unwrap(), vec!["T".to_owned()]);
}

#[test]
fn a_report_sums_the_domain_and_each_patch_from_one_read() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| c.x);
    let n = case.mesh().n_internal_faces();
    write(
        &dir,
        "phi",
        "surfaceScalarField",
        &format!("uniform 0 /* {n} */"),
        &[(
            "xMax",
            "type calculated; value nonuniform List<scalar> 2(0.5 0.25);",
        )],
    );
    let r = case.report("0", "T").unwrap();
    assert_eq!(r.volume.average, vec![2.0]);
    let names: Vec<&str> = r.patches.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, PATCHES);
    let outlet = r.patches.iter().find(|p| p.name == "xMax").unwrap();
    assert_eq!((outlet.stats.average[0], outlet.flux), (5.0, Some(0.75)));
    // The other patches have no `value` for phi: no flux, but still reported.
    let inlet = r.patches.iter().find(|p| p.name == "xMin").unwrap();
    assert_eq!((inlet.stats.average[0], inlet.flux), (0.5, None));
}

#[test]
fn the_bounds_are_the_box_the_mesh_fills() {
    let (_dir, case) = case();
    let b = case.bounds();
    assert_eq!(
        (b.min, b.max),
        (Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 1.0))
    );
}

fn constant(dir: &TempDir, name: &str, body: &str) {
    let text = format!(
        "FoamFile {{ version 2.0; format ascii; class dictionary; object {name}; }}\n{body}\n"
    );
    std::fs::write(dir.path().join("constant").join(name), text).unwrap();
}

#[test]
fn the_shear_on_a_wall_follows_the_gradient_across_the_first_cell() {
    let (dir, case) = case();
    constant(&dir, "transportProperties", "nu [0 2 -1 0 0 0 0] 0.01;");
    // `noSlip` writes no value, and means a wall at rest.
    write(
        &dir,
        "U",
        "volVectorField",
        "uniform (2 0 0)",
        &[("yMin", "type noSlip;")],
    );
    write(
        &dir,
        "p",
        "volScalarField",
        "uniform 3",
        &[("yMin", "type calculated; value uniform 3;")],
    );
    let w = case.wall("0", "yMin").unwrap();
    assert!(w.kinematic);
    // The first cell is 1 high: its centre is 0.5 from the wall.
    let tau = w.mean_shear();
    assert!((tau.x + 0.04).abs() < 1e-12 && tau.y == 0.0, "{tau:?}");
    // The fluid drags the wall along: +0.04 over 4 of area.
    assert!((w.viscous_force.x - 0.16).abs() < 1e-12);
    let (low, mean, high) = w.y_plus_range().unwrap();
    assert!((low - 10.0).abs() < 1e-9 && mean == low && high == low);
    // Pressure pushes the wall along the face's area vector, out of the fluid.
    let pf = w.pressure_force.unwrap();
    assert!((pf.y + 12.0).abs() < 1e-12 && pf.x == 0.0);
    assert!(w.heat_flux.is_none());
}

#[test]
fn a_gas_wall_has_stress_in_pascals_and_heat_flux() {
    let (dir, case) = case();
    constant(
        &dir,
        "thermophysicalProperties",
        "mixture { specie { molWeight 28.96; } thermodynamics { Cp 1000; Hf 0; } \
         transport { mu 0.01; Pr 1; } }",
    );
    write(
        &dir,
        "U",
        "volVectorField",
        "uniform (2 0 0)",
        &[("yMin", "type fixedValue; value uniform (0 0 0);")],
    );
    write(
        &dir,
        "rho",
        "volScalarField",
        "uniform 2",
        &[("yMin", "type calculated; value uniform 2;")],
    );
    write(
        &dir,
        "T",
        "volScalarField",
        "uniform 300",
        &[("yMin", "type fixedValue; value uniform 310;")],
    );
    let w = case.wall("0", "yMin").unwrap();
    assert!(!w.kinematic);
    assert!(
        (w.mean_shear().x + 0.04).abs() < 1e-12,
        "{:?}",
        w.mean_shear()
    );
    let q = w.heat_flux.as_ref().unwrap();
    assert!(q.iter().all(|q| (q - 200.0).abs() < 1e-9), "{q:?}");
    assert!((w.heat_rate().unwrap() - 800.0).abs() < 1e-9);
    let (low, ..) = w.y_plus_range().unwrap();
    assert!((low - 0.5 * 0.02f64.sqrt() / 0.005).abs() < 1e-9);
}

#[test]
fn an_incompressible_wall_with_a_prandtl_number_has_heat_flux_per_unit_of_rho_cp() {
    let (dir, case) = case();
    constant(
        &dir,
        "transportProperties",
        "nu [0 2 -1 0 0 0 0] 0.01; Pr 0.5; Prt 0.85;",
    );
    write(
        &dir,
        "U",
        "volVectorField",
        "uniform (2 0 0)",
        &[("yMin", "type noSlip;")],
    );
    write(
        &dir,
        "nut",
        "volScalarField",
        "uniform 0.1",
        &[("yMin", "type calculated; value uniform 0.17;")],
    );
    write(
        &dir,
        "T",
        "volScalarField",
        "uniform 300",
        &[("yMin", "type fixedValue; value uniform 310;")],
    );
    let w = case.wall("0", "yMin").unwrap();
    assert!(w.kinematic);
    // (ν/Pr + ν_t/Prt) (T_wall − T_cell) / d = (0.02 + 0.2) × 10 / 0.5, over 4 of area.
    let q = w.heat_flux.as_ref().unwrap();
    assert!(q.iter().all(|q| (q - 4.4).abs() < 1e-9), "{q:?}");
    assert!((w.heat_rate().unwrap() - 17.6).abs() < 1e-9);
}

#[test]
fn a_wall_without_a_known_viscosity_or_density_is_refused() {
    let (dir, case) = case();
    write(&dir, "U", "volVectorField", "uniform (2 0 0)", &[]);
    assert!(
        case.wall("0", "yMin")
            .unwrap_err()
            .to_string()
            .contains("viscosity")
    );
    constant(
        &dir,
        "thermophysicalProperties",
        "mixture { thermodynamics { Cp 1000; } transport { mu 0.01; Pr 1; } }",
    );
    assert!(
        case.wall("0", "yMin")
            .unwrap_err()
            .to_string()
            .contains("rho")
    );
    assert!(case.wall("0", "nope").is_err());
}

/// A vector field `f(cell centre)`.
fn write_vector(dir: &TempDir, case: &Case, name: &str, f: impl Fn(Vec3) -> Vec3) {
    let values: Vec<String> = case
        .geometry()
        .cell_centres
        .iter()
        .map(|&c| {
            let v = f(c);
            format!("({} {} {})", v.x, v.y, v.z)
        })
        .collect();
    let internal = format!(
        "nonuniform List<vector> {}({})",
        values.len(),
        values.join(" ")
    );
    write(dir, name, "volVectorField", &internal, &[]);
}

#[test]
fn a_streamline_follows_a_uniform_flow_out_of_the_box() {
    let (dir, case) = case();
    write_vector(&dir, &case, "U", |_| Vec3::new(2.0, 0.0, 0.0));
    let s = case
        .streamline("0", "U", Vec3::new(0.5, 0.5, 0.5), false, 100)
        .unwrap();
    assert_eq!(s.end, StreamlineEnd::Boundary);
    // The seed, then one face of each cell it passes: four of them.
    assert_eq!(s.points.len(), 5);
    let last = s.points.last().unwrap();
    assert!((last.x - 4.0).abs() < 1e-12 && (last.y - 0.5).abs() < 1e-12);
    assert!(s.speeds.iter().all(|&v| v == 2.0));

    let back = case
        .streamline("0", "U", Vec3::new(2.5, 1.5, 0.5), true, 100)
        .unwrap();
    assert_eq!(back.end, StreamlineEnd::Boundary);
    assert!(back.points.last().unwrap().x.abs() < 1e-12);
}

#[test]
fn many_streamlines_run_through_their_seeds_both_ways() {
    let (dir, case) = case();
    write_vector(&dir, &case, "U", |_| Vec3::new(2.0, 0.0, 0.0));
    let seeds = [
        Vec3::new(2.5, 0.5, 0.5),
        Vec3::new(2.5, 1.5, 0.5),
        Vec3::new(40.0, 0.5, 0.5),
    ];
    let lines = case.streamlines("0", "U", &seeds, 100).unwrap();
    // The seed outside the mesh gives no line; the others cross the box.
    assert_eq!(lines.len(), 2);
    for (line, seed) in lines.iter().zip(&seeds) {
        assert!(line.points.first().unwrap().x.abs() < 1e-12);
        assert!((line.points.last().unwrap().x - 4.0).abs() < 1e-12);
        assert_eq!(line.points.len(), line.speeds.len());
        // The seed is in it once.
        let at_seed = line
            .points
            .iter()
            .filter(|p| (**p - *seed).norm() < 1e-12)
            .count();
        assert_eq!(at_seed, 1);
        assert!(line.points.windows(2).all(|w| w[1].x > w[0].x));
    }
    assert!(case.streamlines("0", "U", &seeds, 1).is_err());
    assert!(case.streamlines("0", "nope", &seeds, 10).is_err());
}

#[test]
fn a_streamline_bends_with_the_flow() {
    let (dir, case) = case();
    write_vector(&dir, &case, "U", |_| Vec3::new(1.0, 0.5, 0.0));
    let s = case
        .streamline("0", "U", Vec3::new(0.2, 0.2, 0.5), false, 100)
        .unwrap();
    // Slope 1/2: it reaches the top (y = 2) at x = 0.2 + 3.6.
    let last = s.points.last().unwrap();
    assert_eq!(s.end, StreamlineEnd::Boundary);
    assert!(
        (last.y - 2.0).abs() < 1e-9 && (last.x - 3.8).abs() < 1e-9,
        "{last:?}"
    );
}

#[test]
fn a_streamline_stops_where_the_flow_does_and_at_its_length() {
    let (dir, case) = case();
    write_vector(&dir, &case, "U", |c| {
        if c.x < 2.0 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(-1.0, 0.0, 0.0)
        }
    });
    // The flow from both sides meets at x = 2.
    let s = case
        .streamline("0", "U", Vec3::new(0.5, 0.5, 0.5), false, 100)
        .unwrap();
    assert_eq!(s.end, StreamlineEnd::Stagnation);
    assert!(s.points.last().unwrap().x <= 2.0 + 1e-12);

    write_vector(&dir, &case, "U", |_| Vec3::new(1.0, 0.0, 0.0));
    let short = case
        .streamline("0", "U", Vec3::new(0.5, 0.5, 0.5), false, 2)
        .unwrap();
    assert_eq!((short.points.len(), short.end), (2, StreamlineEnd::Length));

    write_vector(&dir, &case, "U", |_| Vec3::default());
    let still = case
        .streamline("0", "U", Vec3::new(0.5, 0.5, 0.5), false, 10)
        .unwrap();
    assert_eq!(
        (still.points.len(), still.end),
        (1, StreamlineEnd::Stagnation)
    );
}

#[test]
fn a_streamline_refuses_what_it_cannot_follow() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "T", |c| c.x);
    let seed = Vec3::new(0.5, 0.5, 0.5);
    assert!(case.streamline("0", "T", seed, false, 10).is_err());
    write_vector(&dir, &case, "U", |_| Vec3::new(1.0, 0.0, 0.0));
    assert!(
        case.streamline("0", "U", Vec3::new(9.0, 0.0, 0.0), false, 10)
            .is_err()
    );
    assert!(
        case.streamline("0", "U", Vec3::new(f64::NAN, 0.0, 0.0), false, 10)
            .is_err()
    );
    assert!(case.streamline("0", "U", seed, false, 0).is_err());
    assert!(case.streamline("0", "U", seed, false, usize::MAX).is_err());
}

fn area(polygon: &SlicePolygon) -> f64 {
    let p = &polygon.points;
    let mut sum = Vec3::new(0.0, 0.0, 0.0);
    for pair in p.windows(2) {
        let (a, b) = (pair[0] - p[0], pair[1] - p[0]);
        sum = sum
            + Vec3::new(
                a.y * b.z - a.z * b.y,
                a.z * b.x - a.x * b.z,
                a.x * b.y - a.y * b.x,
            );
    }
    sum.norm() / 2.0
}

#[test]
fn a_slice_cuts_the_cells_it_crosses() {
    let (_dir, case) = case();
    // x = 1.5 crosses the column of two cells at x in [1, 2]: two unit squares.
    let cut = case
        .slice(Vec3::new(1.5, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0))
        .unwrap();
    assert_eq!(cut.len(), 2);
    for polygon in &cut {
        assert_eq!(polygon.points.len(), 4);
        assert!(polygon.points.iter().all(|p| (p.x - 1.5).abs() < 1e-12));
        assert!((area(polygon) - 1.0).abs() < 1e-12);
    }
    // Along the faces between cells, the cells on both sides are cut: two columns.
    let along = case
        .slice(Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0))
        .unwrap();
    assert_eq!(along.len(), 4);
    // Outside the box there is nothing.
    assert!(
        case.slice(Vec3::new(9.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_slanted_slice_lies_in_its_plane() {
    let (_dir, case) = case();
    let origin = Vec3::new(2.0, 1.0, 0.5);
    let normal = Vec3::new(1.0, 1.0, 0.0);
    let cut = case.slice(origin, normal).unwrap();
    assert!(!cut.is_empty());
    for polygon in &cut {
        assert!(polygon.points.len() >= 3);
        for p in &polygon.points {
            assert!((*p - origin).dot(normal).abs() < 1e-9);
        }
        assert!(area(polygon) > 0.0);
    }
    // x + y = 3 cuts the whole box diagonal: the sections add up to the slanted rectangle.
    let total: f64 = cut.iter().map(area).sum();
    assert!(total > 1.0);
}

#[test]
fn a_slice_refuses_a_plane_that_is_not_one() {
    let (_dir, case) = case();
    let zero = Vec3::new(0.0, 0.0, 0.0);
    assert!(case.slice(zero, zero).is_err());
    assert!(
        case.slice(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0))
            .is_err()
    );
    assert!(
        case.slice(zero, Vec3::new(f64::INFINITY, 0.0, 0.0))
            .is_err()
    );
}

/// The text between `<DataArray ... Name="name" ...>` and its closing tag.
fn array<'a>(vtu: &'a str, name: &str) -> &'a str {
    let at = vtu.find(&format!("Name=\"{name}\"")).unwrap();
    let start = vtu[at..].find('>').unwrap() + at + 1;
    let end = vtu[start..].find("</DataArray>").unwrap() + start;
    &vtu[start..end]
}

#[test]
fn the_cells_go_out_as_a_table() {
    let (dir, case) = case();
    write_vector(&dir, &case, "U", |c| Vec3::new(c.x, 0.0, 1.0));
    write_scalar(&dir, &case, "p", |c| c.y);
    let csv = case.cells_csv("0", &[]).unwrap();
    let mut lines = csv.lines();
    assert_eq!(lines.next(), Some("cell,x,y,z,volume,U_x,U_y,U_z,p"));
    assert_eq!(lines.count(), 8);
    let only = case.cells_csv("0", &["p".to_owned()]).unwrap();
    assert_eq!(only.lines().next(), Some("cell,x,y,z,volume,p"));
    // A cell's row: its centre, unit volume, and the value there.
    let first: Vec<f64> = only
        .lines()
        .nth(1)
        .unwrap()
        .split(',')
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(first, [0.5, 0.5, 0.5, 1.0, 0.5]);
    assert!(case.cells_csv("0", &["nope".to_owned()]).is_err());
}

#[test]
fn the_mesh_goes_out_as_a_vtk_grid_of_polyhedra() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "p", |c| c.x);
    let vtu = case.vtu("0", &[]).unwrap();
    assert!(vtu.contains("NumberOfPoints=\"30\" NumberOfCells=\"8\""));
    let types: Vec<&str> = array(&vtu, "types").split_whitespace().collect();
    assert_eq!(types, ["42"; 8]);
    // Each cell is a box of eight points and six quadrilaterals.
    let offsets: Vec<usize> = array(&vtu, "offsets")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(offsets, [8, 16, 24, 32, 40, 48, 56, 64]);
    let face_offsets: Vec<usize> = array(&vtu, "faceoffsets")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    // 1 face count + 6 * (1 length + 4 points) = 31 numbers a cell.
    assert_eq!(face_offsets, [31, 62, 93, 124, 155, 186, 217, 248]);
    let faces: Vec<usize> = array(&vtu, "faces")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(faces.len(), 248);
    assert!(faces.chunks(31).all(|cell| cell[0] == 6));
    // Every point number is a point of the mesh.
    let connectivity: Vec<usize> = array(&vtu, "connectivity")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    assert!(connectivity.iter().all(|&i| i < 30));
    let p: Vec<f64> = array(&vtu, "p")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(p.len(), 8);
    assert_eq!(p[0], 0.5);
}

#[test]
fn the_faces_of_the_grid_point_out_of_their_cell() {
    let (dir, case) = case();
    write_scalar(&dir, &case, "p", |c| c.x);
    let vtu = case.vtu("0", &[]).unwrap();
    let faces: Vec<usize> = array(&vtu, "faces")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let mut at = 0;
    for cell in 0..8 {
        let centre = case.geometry().cell_centres[cell];
        let count = faces[at];
        at += 1;
        for _ in 0..count {
            let n = faces[at];
            let ring: Vec<Vec3> = faces[at + 1..at + 1 + n]
                .iter()
                .map(|&i| case.mesh().points()[i])
                .collect();
            at += 1 + n;
            // Newell's normal of the ring, against the way from the cell's middle to the face.
            let mut normal = Vec3::new(0.0, 0.0, 0.0);
            for (k, a) in ring.iter().enumerate() {
                let b = ring[(k + 1) % n];
                normal = normal
                    + Vec3::new(
                        (a.y - b.y) * (a.z + b.z),
                        (a.z - b.z) * (a.x + b.x),
                        (a.x - b.x) * (a.y + b.y),
                    );
            }
            let middle = ring.iter().fold(Vec3::new(0.0, 0.0, 0.0), |s, &p| s + p) * 0.25;
            assert!(
                normal.dot(middle - centre) > 0.0,
                "cell {cell} has a face pointing in"
            );
        }
    }
}

/// A `phi` at `time` giving every patch of the box (sizes 2, 2, 4, 4, 8, 8) one
/// uniform value `v`: the flux through the patch is `v` times its face count.
fn write_phi(dir: &TempDir, case: &Case, time: &str, uniform: [f64; 6]) {
    let n = case.mesh().n_internal_faces();
    let mut text = format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class surfaceScalarField;\n    object phi;\n}}\n\
         dimensions [0 3 -1 0 0 0 0];\ninternalField uniform 0 /* {n} */;\nboundaryField\n{{\n"
    );
    for (p, v) in PATCHES.iter().zip(uniform) {
        let _ = writeln!(text, "    {p} {{ type calculated; value uniform {v}; }}");
    }
    text.push_str("}\n");
    let folder = dir.path().join(time);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("phi"), text).unwrap();
}

#[test]
fn the_balance_compares_what_enters_with_what_leaves() {
    let (dir, case) = case();
    // In: 2 faces × 1.5 = 3. Out: 2 faces × 0.5 = 1 and 4 × 0.5 = 2 through yMax.
    write_phi(&dir, &case, "10", [-1.5, 0.5, 0.0, 0.5, 0.0, 0.0]);
    let b = case.mass_balance("10").unwrap();
    assert_eq!((b.inflow(), b.outflow(), b.net()), (3.0, 3.0, 0.0));
    assert_eq!(b.percent(), 0.0);
    // The leak is a percentage of the larger side.
    write_phi(&dir, &case, "20", [-1.5, 0.25, 0.0, 0.5, 0.0, 0.0]);
    let b = case.mass_balance("20").unwrap();
    assert_eq!((b.inflow(), b.outflow(), b.net()), (3.0, 2.5, -0.5));
    assert!((b.percent() + 100.0 * 0.5 / 3.0).abs() < 1e-12);
    // Nothing flowing is balanced, not a division by zero.
    write_phi(&dir, &case, "30", [0.0; 6]);
    assert_eq!(case.mass_balance("30").unwrap().percent(), 0.0);
}

#[test]
fn the_balance_lists_the_flux_through_each_patch() {
    let (dir, case) = case();
    write_phi(&dir, &case, "10", [-1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let b = case.mass_balance("10").unwrap();
    assert_eq!(b.patches.len(), 6);
    assert_eq!(b.patches[0], ("xMin".to_owned(), -2.0));
}

#[test]
fn the_history_covers_the_last_times_with_a_flux() {
    let (dir, case) = case();
    write_phi(&dir, &case, "10", [-1.0, 0.5, 0.0, 0.0, 0.0, 0.0]); // 50 % short
    write_phi(&dir, &case, "20", [-1.0, 0.5, 0.0, 0.0, 0.0, 0.0]);
    write_phi(&dir, &case, "30", [-1.0, 0.5, 0.0, 0.0, 0.0, 0.0]);
    write_phi(&dir, &case, "40", [-1.0, 1.0, 0.0, 0.0, 0.0, 0.0]); // closed
    write_phi(&dir, &case, "50", [-1.0, 0.75, 0.0, 0.0, 0.0, 0.0]); // 25 %
    let h = case.mass_balance_history(2).unwrap();
    assert_eq!(
        (h.first.as_str(), h.last.as_str(), h.samples),
        ("40", "50", 2)
    );
    assert!((h.mean_abs_percent - 12.5).abs() < 1e-12);
    assert!((h.max_abs_percent - 25.0).abs() < 1e-12);
    assert_eq!(h.latest.time, "50");
    // A window longer than what exists takes what there is, never time 0.
    let all = case.mass_balance_history(100).unwrap();
    assert_eq!((all.first.as_str(), all.samples), ("10", 5));
}

#[test]
fn the_history_of_a_case_that_never_ran_is_an_error() {
    let (_dir, case) = case();
    assert!(case.mass_balance_history(5).is_err());
    assert!(case.mass_balance_history(0).is_err());
    assert!(case.mass_balance("0").is_err());
}

fn write_uniform(dir: &TempDir, name: &str, class: &str, value: &str) {
    write(dir, name, class, &format!("uniform {value}"), &[]);
}

use crate::{Density, SectionOptions};

fn flow(dir: &TempDir, u: &str, p: &str, rho: Option<&str>) {
    write_uniform(dir, "U", "volVectorField", u);
    write_uniform(dir, "p", "volScalarField", p);
    if let Some(r) = rho {
        write_uniform(dir, "rho", "volScalarField", r);
    }
}

#[test]
fn a_uniform_flow_through_a_section_gives_the_textbook_numbers() {
    let (dir, case) = case();
    flow(&dir, "(2 0 0)", "101425", Some("1.5"));
    let o = SectionOptions {
        position: 2.5,
        reference_pressure: 101_325.0,
        reference_speed: 10.0,
        reference_density: 1.0,
        reference_area: 2.0,
        ..SectionOptions::default()
    };
    let s = case.section("0", &o).unwrap();
    // The plane x = 2.5 cuts the 2 × 1 cross-section of the box.
    assert!((s.area - 2.0).abs() < 1e-12, "{}", s.area);
    assert!((s.mass_flow - 1.5 * 2.0 * 2.0).abs() < 1e-12);
    assert_eq!(
        (s.forward_mass_flow, s.reverse_mass_flow),
        (s.mass_flow, 0.0)
    );
    assert!((s.volume_flow - 4.0).abs() < 1e-12);
    assert!((s.momentum_flux - 1.5 * 4.0 * 2.0).abs() < 1e-12);
    assert!((s.pressure_load - 100.0 * 2.0).abs() < 1e-9);
    assert!((s.resultant - (12.0 + 200.0)).abs() < 1e-9);
    assert!((s.kinetic_power - 0.5 * 1.5 * 4.0 * 2.0 * 2.0).abs() < 1e-12);
    assert!((s.mean_pressure - 101_425.0).abs() < 1e-9);
    assert_eq!((s.mean_density, s.mean_normal_velocity), (1.5, 2.0));
    // q = ½ · 1 · 10² = 50 and A_ref = 2.
    assert!((s.coefficient - 212.0 / 100.0).abs() < 1e-9);
    assert!((s.mean_cp - 100.0 / 50.0).abs() < 1e-9);
    assert_eq!((s.cells, s.shared_faces, s.area_scale), (2, 0, 1.0));
}

#[test]
fn flow_the_other_way_is_reverse_flow_and_the_direction_flips_it() {
    let (dir, case) = case();
    flow(&dir, "(-2 0 0)", "0", Some("1"));
    let o = SectionOptions {
        position: 1.5,
        ..SectionOptions::default()
    };
    let s = case.section("0", &o).unwrap();
    assert_eq!((s.forward_mass_flow, s.reverse_mass_flow), (0.0, 4.0));
    assert!((s.mass_flow + 4.0).abs() < 1e-12);
    let back = SectionOptions {
        direction: -1.0,
        ..o
    };
    let s = case.section("0", &back).unwrap();
    assert!((s.mass_flow - 4.0).abs() < 1e-12);
}

#[test]
fn a_plane_on_the_faces_between_cells_counts_each_face_once() {
    let (dir, case) = case();
    flow(&dir, "(1 0 0)", "5", Some("2"));
    let o = SectionOptions {
        position: 2.0, // the face between the second and third cell
        ..SectionOptions::default()
    };
    let s = case.section("0", &o).unwrap();
    assert!((s.area - 2.0).abs() < 1e-9, "{}", s.area);
    assert_eq!(s.shared_faces, 2);
    assert!((s.mass_flow - 4.0).abs() < 1e-9);
}

#[test]
fn an_incompressible_run_takes_a_constant_density_and_kinematic_pressure() {
    let (dir, case) = case();
    flow(&dir, "(1 0 0)", "3", None);
    let o = SectionOptions {
        position: 1.5,
        density: Density::Constant(1000.0),
        kinematic_pressure: true,
        reference_pressure: 0.0,
        ..SectionOptions::default()
    };
    let s = case.section("0", &o).unwrap();
    assert!((s.mass_flow - 2000.0).abs() < 1e-9);
    assert!((s.mean_pressure - 3000.0).abs() < 1e-9);
    // Without a density it says so instead of guessing.
    let o = SectionOptions {
        position: 1.5,
        ..SectionOptions::default()
    };
    assert!(case.section("0", &o).is_err());
}

#[test]
fn a_wedge_is_scaled_to_the_full_revolution() {
    let (dir, case) = case();
    flow(&dir, "(1 0 0)", "0", Some("1"));
    let o = SectionOptions {
        position: 1.5,
        wedge_angle: Some(5.0),
        ..SectionOptions::default()
    };
    let s = case.section("0", &o).unwrap();
    assert!((s.area_scale - 72.0).abs() < 1e-12);
    assert!((s.area - 2.0 * 72.0).abs() < 1e-9);
    assert!((s.mass_flow - 144.0).abs() < 1e-9);
}

#[test]
fn a_section_refuses_what_it_cannot_work_out() {
    let (dir, case) = case();
    flow(&dir, "(1 0 0)", "0", Some("1"));
    let base = SectionOptions {
        position: 1.5,
        ..SectionOptions::default()
    };
    let bad = [
        SectionOptions {
            axis: 3,
            ..base.clone()
        },
        SectionOptions {
            direction: 0.0,
            ..base.clone()
        },
        SectionOptions {
            position: f64::NAN,
            ..base.clone()
        },
        SectionOptions {
            position: 40.0,
            ..base.clone()
        },
        SectionOptions {
            reference_speed: 0.0,
            ..base.clone()
        },
        SectionOptions {
            reference_area: -1.0,
            ..base.clone()
        },
        SectionOptions {
            density: Density::Constant(0.0),
            ..base.clone()
        },
        SectionOptions {
            wedge_angle: Some(10.0),
            ..base.clone()
        },
        SectionOptions {
            axis: 1,
            wedge_angle: Some(5.0),
            position: 1.0,
            ..base.clone()
        },
    ];
    for (i, o) in bad.iter().enumerate() {
        assert!(case.section("0", o).is_err(), "case {i}");
    }
    // A density that is not positive in a cell the plane cuts.
    write_uniform(&dir, "rho", "volScalarField", "-1");
    assert!(case.section("0", &base).is_err());
    // No fields at that time.
    assert!(case.section("5", &base).is_err());
}

fn report_of(label: &str) -> crate::SectionReport {
    let (dir, case) = case();
    flow(&dir, "(2 0 0)", "101425", Some("1.5"));
    let options = SectionOptions {
        position: 2.5,
        ..SectionOptions::default()
    };
    let analysis = case.section("0", &options).unwrap();
    crate::SectionReport {
        label: label.to_owned(),
        case: "box".to_owned(),
        time: "0".to_owned(),
        options,
        analysis,
    }
}

#[test]
fn the_report_says_the_same_numbers_in_json_csv_and_html() {
    let report = report_of("Throat");
    let json: serde_json::Value = serde_json::from_str(&report.json()).unwrap();
    assert_eq!(json["schema"], "vortia.section-report");
    assert_eq!(json["payload"]["metrics"]["massFlow"], 6.0);
    assert_eq!(json["payload"]["options"]["axis"], "x");
    assert_eq!(json["payload"]["options"]["density"], "field");
    assert_eq!(json["sha256"].as_str().unwrap().len(), 64);
    let csv = report.csv();
    assert_eq!(csv.lines().count(), 1 + report.metrics().len());
    assert!(
        csv.contains("\"Throat\",\"box\",\"0\",massFlow,6,\"kg/s\""),
        "{csv}"
    );
    let html = report.html();
    assert!(html.contains("<h1>Throat</h1>") && html.contains("Signed mass flow"));
}

#[test]
fn a_label_cannot_run_in_a_spreadsheet_or_a_page() {
    let report = report_of("=HYPERLINK(\"x\")<script>");
    assert!(
        report.csv().contains("\"'=HYPERLINK(\"\"x\"\")<script>\""),
        "{}",
        report.csv()
    );
    let html = report.html();
    assert!(!html.contains("<script>") && html.contains("&lt;script&gt;"));
    let long = report_of(&"a".repeat(1000));
    let json: serde_json::Value = serde_json::from_str(&long.json()).unwrap();
    assert_eq!(json["payload"]["label"].as_str().unwrap().len(), 160);
}

#[test]
fn a_saved_report_opens_and_one_that_was_changed_does_not() {
    let report = report_of("Throat");
    let text = report.json();
    let opened = crate::Sealed::open(&text).unwrap();
    assert_eq!(opened.schema, crate::SECTION_SCHEMA);
    assert_eq!(opened.metric("massFlow"), Some(6.0));
    assert_eq!(opened.text("label"), "Throat");
    // The number was edited after the report was saved.
    let edited = text.replace("\"massFlow\": 6.0", "\"massFlow\": 7.0");
    assert_ne!(edited, text);
    let err = crate::Sealed::open(&edited).unwrap_err().to_string();
    assert!(err.contains("digest"), "{err}");
    // Not a report, not JSON, too large, another version.
    assert!(crate::Sealed::open("{}").is_err());
    assert!(crate::Sealed::open("not json").is_err());
    assert!(crate::Sealed::open(&" ".repeat(crate::MAX_REPORT_BYTES + 1)).is_err());
    assert!(crate::Sealed::open(&text.replace("\"version\": 1", "\"version\": 2")).is_err());
}

#[test]
fn the_digest_does_not_depend_on_the_order_the_payload_was_built_in() {
    let a = serde_json::json!({"b": 1, "a": {"y": [1, 2], "x": "t"}});
    let b: serde_json::Value = serde_json::from_str(r#"{"a":{"x":"t","y":[1,2]},"b":1}"#).unwrap();
    assert_eq!(crate::digest(&a), crate::digest(&b));
    let c = serde_json::json!({"b": 2, "a": {"y": [1, 2], "x": "t"}});
    assert_ne!(crate::digest(&a), crate::digest(&c));
}

#[test]
fn runs_are_compared_to_a_baseline_and_a_different_definition_is_flagged() {
    let one = crate::Sealed::open(&report_of("Run A").json()).unwrap();
    let same = crate::Sealed::open(&report_of("Run B").json()).unwrap();
    let rows = crate::compare(&[one.clone(), same.clone()], "massFlow", 0).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].change_percent, Some(0.0));
    assert_eq!(rows[1].change_percent, Some(0.0));
    assert!(rows.iter().all(|r| r.warning.is_none()));

    // The same flow read at another position: the number is the same, the definition is not.
    let (dir, case) = case();
    flow(&dir, "(2 0 0)", "101425", Some("1.5"));
    let options = SectionOptions {
        position: 1.5,
        ..SectionOptions::default()
    };
    let moved = crate::SectionReport {
        label: "Run C".into(),
        case: "box".into(),
        time: "0".into(),
        analysis: case.section("0", &options).unwrap(),
        options,
    };
    let moved = crate::Sealed::open(&moved.json()).unwrap();
    let rows = crate::compare(&[one.clone(), moved], "massFlow", 0).unwrap();
    assert_eq!(rows[1].warning, Some(crate::DEFINITIONS_DIFFER));
    let csv = crate::comparison_csv(&rows, "massFlow", &one.sha256);
    assert_eq!(csv.lines().count(), 3);
    assert!(csv.contains("Different section"), "{csv}");

    assert!(crate::compare(std::slice::from_ref(&one), "massFlow", 3).is_err());
    assert!(crate::compare(&[one], "nothing", 0).is_err());
}

/// The box of `the_shear_on_a_wall...`: the bottom wall, 4 m² of it, pulled along x by the
/// stress 0.04 and pushed down by p = 3.
fn force_case() -> (TempDir, Case) {
    let (dir, case) = case();
    constant(&dir, "transportProperties", "nu [0 2 -1 0 0 0 0] 0.01;");
    write(
        &dir,
        "U",
        "volVectorField",
        "uniform (2 0 0)",
        &[("yMin", "type noSlip;")],
    );
    write(
        &dir,
        "p",
        "volScalarField",
        "uniform 3",
        &[("yMin", "type calculated; value uniform 3;")],
    );
    (dir, case)
}

#[test]
fn forces_are_split_along_drag_and_lift_and_made_into_coefficients() {
    let (_dir, case) = force_case();
    let reference = crate::ForceReference {
        speed: 2.0,
        area: 4.0,
        length: 2.0,
        ..crate::ForceReference::default()
    };
    let r = case.forces("0", &["yMin".to_owned()], &reference).unwrap();
    assert!(r.per_unit_density);
    let m = |key: &str| {
        r.metrics()
            .into_iter()
            .find(|m| m.key == key)
            .unwrap()
            .value
    };
    assert!((m("area") - 4.0).abs() < 1e-12);
    assert!((m("viscousDrag") - 0.16).abs() < 1e-12);
    assert_eq!(m("pressureDrag"), 0.0);
    assert!((m("drag") - 0.16).abs() < 1e-12);
    // Pressure pushes the bottom wall down: out of the fluid.
    assert!((m("pressureLift") + 12.0).abs() < 1e-12 && m("viscousLift") == 0.0);
    assert!((m("lift") + 12.0).abs() < 1e-12 && m("side") == 0.0);
    // ½ U² A = 8.
    assert!((m("cd") - 0.02).abs() < 1e-12 && (m("cl") + 1.5).abs() < 1e-12);
    // The four faces are at x = 0.5, 1.5, 2.5 and 3.5, each pushed down by 3: about the
    // origin the moment about z is Σ x · (−3) = −24.
    assert!((m("moment") + 24.0).abs() < 1e-9, "{}", m("moment"));
    assert!((m("cm") + 24.0 / (8.0 * 2.0)).abs() < 1e-9);

    // The same flow with a density gives newtons and the same coefficients.
    let with_density = crate::ForceReference {
        density: Some(1.2),
        ..reference.clone()
    };
    let n = case
        .forces("0", &["yMin".to_owned()], &with_density)
        .unwrap();
    assert!(!n.per_unit_density);
    let k = |key: &str| {
        n.metrics()
            .into_iter()
            .find(|m| m.key == key)
            .unwrap()
            .value
    };
    assert!((k("drag") - 1.2 * 0.16).abs() < 1e-12);
    assert!((k("cd") - 0.02).abs() < 1e-12 && (k("cl") + 1.5).abs() < 1e-12);

    // Another direction for the stream: the drag is then the force along y.
    let up = crate::ForceReference {
        drag: Vec3::new(0.0, 1.0, 0.0),
        lift: Vec3::new(1.0, 0.0, 0.0),
        ..reference
    };
    let u = case.forces("0", &["yMin".to_owned()], &up).unwrap();
    let d = u
        .metrics()
        .into_iter()
        .find(|m| m.key == "drag")
        .unwrap()
        .value;
    assert!((d + 12.0).abs() < 1e-12);

    // The report in its three forms, and the digest.
    let mut report = r.clone();
    report.label = "Floor".into();
    report.case = "box".into();
    let sealed = crate::Sealed::open(&report.json()).unwrap();
    assert_eq!(sealed.schema, crate::FORCE_SCHEMA);
    assert_eq!(sealed.metric("cd"), Some(m("cd")));
    assert!(report.csv().contains("\"Floor\",\"box\",\"0\",cd,0.02"));
    assert!(report.html().contains("<h1>Floor</h1>"));
}

#[test]
fn forces_refuse_what_cannot_be_computed() {
    let (dir, case) = force_case();
    let ok = crate::ForceReference::default();
    let wall = ["yMin".to_owned()];
    let bad = |reference: crate::ForceReference| case.forces("0", &wall, &reference).is_err();
    assert!(bad(crate::ForceReference {
        speed: 0.0,
        ..ok.clone()
    }));
    assert!(bad(crate::ForceReference {
        area: f64::NAN,
        ..ok.clone()
    }));
    assert!(bad(crate::ForceReference {
        density: Some(-1.0),
        ..ok.clone()
    }));
    assert!(bad(crate::ForceReference {
        lift: Vec3::new(1.0, 0.1, 0.0),
        ..ok.clone()
    }));
    assert!(bad(crate::ForceReference {
        drag: Vec3::default(),
        ..ok.clone()
    }));
    assert!(bad(crate::ForceReference {
        centre: Vec3::new(f64::INFINITY, 0.0, 0.0),
        ..ok.clone()
    }));
    assert!(case.forces("0", &[], &ok).is_err());
    assert!(case.forces("0", &["nope".to_owned()], &ok).is_err());
    assert!(
        case.forces("0", &["yMin".to_owned(), "yMin".to_owned()], &ok)
            .is_err()
    );
    // No pressure: no forces.
    std::fs::remove_file(dir.path().join("0/p")).unwrap();
    let err = case.forces("0", &wall, &ok).unwrap_err().to_string();
    assert!(err.contains("pressure"), "{err}");
}

#[test]
fn a_gas_needs_its_reference_density_for_the_coefficients() {
    let (dir, case) = case();
    constant(
        &dir,
        "thermophysicalProperties",
        "mixture { specie { molWeight 28.96; } thermodynamics { Cp 1000; Hf 0; } \
         transport { mu 0.01; Pr 1; } }",
    );
    write(
        &dir,
        "U",
        "volVectorField",
        "uniform (2 0 0)",
        &[("yMin", "type fixedValue; value uniform (0 0 0);")],
    );
    write(
        &dir,
        "rho",
        "volScalarField",
        "uniform 2",
        &[("yMin", "type calculated; value uniform 2;")],
    );
    write(
        &dir,
        "p",
        "volScalarField",
        "uniform 100",
        &[("yMin", "type calculated; value uniform 100;")],
    );
    let wall = ["yMin".to_owned()];
    assert!(
        case.forces("0", &wall, &crate::ForceReference::default())
            .is_err()
    );
    let reference = crate::ForceReference {
        density: Some(2.0),
        speed: 2.0,
        area: 4.0,
        ..crate::ForceReference::default()
    };
    let r = case.forces("0", &wall, &reference).unwrap();
    assert!(!r.per_unit_density);
    // Stress 0.01 · 2 / 0.5 = 0.04 Pa over 4 m²; ½ρU²A = 16.
    let value = |key: &str| {
        r.metrics()
            .into_iter()
            .find(|m| m.key == key)
            .unwrap()
            .value
    };
    assert!(
        (value("viscousDrag") - 0.16).abs() < 1e-12,
        "{}",
        value("viscousDrag")
    );
    assert!((value("pressureLift") + 400.0).abs() < 1e-9);
    assert!((value("cd") - 0.16 / 16.0).abs() < 1e-12);
}
