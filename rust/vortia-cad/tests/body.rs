// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use vortia_cad::step;
use vortia_cases::Body;
use vortia_mesh::bodymesh::mesh_around;
use vortia_mesh::{MeshGeometry, PolyMesh, check};

const CYLINDER_MM: &str = include_str!("data/cylinder-mm.step");

/// A STEP part (a cylinder of 1 m by 2 m, the file said in metres) goes through the whole chain
/// to a mesh that the checks accept, with the air taking the tunnel less the cylinder.
#[test]
fn a_step_part_is_meshed_around_in_a_tunnel() {
    let metres = CYLINDER_MM.replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT($,.METRE.)");
    let read = step::parse(metres.as_bytes(), step::DEFAULT_CHORD).unwrap();
    let mut body = Body::new(read.surface);
    for (name, value) in [("levels", 3.0), ("wake-levels", 2.0), ("cells", 1.0)] {
        body.set(name, value).unwrap();
    }
    let spec = body.spec().unwrap();
    let (raw, stats) = mesh_around(&body.surface, &spec).unwrap();
    let mesh = PolyMesh::from_raw(raw).unwrap();
    let geo = MeshGeometry::compute(&mesh);
    let report = check(&mesh, &geo);
    assert!(report.is_ok(), "{:?}", report.failed_checks());
    assert!(stats.body_faces > 100);
    let tunnel: f64 = (0..3).map(|i| spec.high[i] - spec.low[i]).product();
    let cylinder = 2.0 * std::f64::consts::PI;
    let air = geo.total_volume();
    assert!(
        ((tunnel - air) - cylinder).abs() < 0.1 * cylinder,
        "the cylinder takes {} of {cylinder}",
        tunnel - air
    );
}
