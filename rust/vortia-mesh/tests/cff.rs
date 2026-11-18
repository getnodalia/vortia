//! A Fluent case in the common file format gives the mesh its `.msh` twin gives.
//!
//! `tests/cff/hex.cas.h5` was written for these tests (a unit cube with three boundary zones,
//! the same mesh as `tests/fluent/hex.msh`); it is laid out as the format's description says,
//! not copied from a file Fluent wrote.

use std::path::PathBuf;

use vortia_mesh::cff::cff_to_foam;
use vortia_mesh::fluent::{Options, fluent_to_foam};

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cff/hex.cas.h5")
}

#[test]
fn a_case_gives_the_mesh_its_msh_twin_gives() {
    let from_case = cff_to_foam(&path(), Options::default()).unwrap();
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fluent/hex.msh"),
    )
    .unwrap();
    let from_msh = fluent_to_foam(&text, Options::default()).unwrap();
    assert_eq!(from_case.points, from_msh.points);
    assert_eq!(from_case.faces, from_msh.faces);
    assert_eq!(from_case.owner, from_msh.owner);
    let patches = |m: &vortia_foamfile::RawPolyMesh| {
        m.patches
            .iter()
            .map(|p| (p.name.clone(), p.kind.clone(), p.start_face, p.n_faces))
            .collect::<Vec<_>>()
    };
    assert_eq!(patches(&from_case), patches(&from_msh));
    assert_eq!(
        patches(&from_case),
        [
            ("inlet".to_owned(), "patch".to_owned(), 0, 1),
            ("outlet".to_owned(), "patch".to_owned(), 1, 1),
            ("wall".to_owned(), "patch".to_owned(), 2, 4),
        ]
    );
}

#[test]
fn what_is_missing_is_named() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cff");
    let error = cff_to_foam(&dir.join("no-c0.cas.h5"), Options::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("faces/c0/10"), "{error}");
    let error = cff_to_foam(&dir.join("nothing.h5"), Options::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("HDF5"), "{error}");
}

#[test]
fn a_damaged_case_never_panics() {
    let bytes = std::fs::read(path()).unwrap();
    let dir = std::env::temp_dir().join(format!("vortia-cff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("damaged.cas.h5");
    let mut cuts: Vec<usize> = (0..bytes.len()).step_by(997).collect();
    cuts.push(bytes.len() - 1);
    for cut in cuts {
        std::fs::write(&file, &bytes[..cut]).unwrap();
        let _ = cff_to_foam(&file, Options::default());
    }
    for at in (0..bytes.len()).step_by(61) {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0xA5;
        std::fs::write(&file, &flipped).unwrap();
        let _ = cff_to_foam(&file, Options::default());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
