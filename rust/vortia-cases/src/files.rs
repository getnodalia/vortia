//! Writing the files of a case.

use std::fs;
use std::path::Path;

use vortia_foamfile::FoamFile;

use crate::error::{CaseError, Result};

/// The header every generated dictionary starts with.
pub(crate) fn header(class: &str, location: &str, object: &str) -> String {
    let location = if location.is_empty() {
        String::new()
    } else {
        format!("    location    \"{location}\";\n")
    };
    format!(
        "FoamFile\n{{\n    version     2.0;\n    format      ascii;\n    class       {class};\n{location}    object      {object};\n}}\n\n"
    )
}

/// One file to write, by its path under the case.
pub(crate) struct File {
    pub path: &'static str,
    pub text: String,
}

/// Everything a geometry writes, before the mesh.
pub(crate) struct FileSet {
    pub files: Vec<File>,
    pub block_mesh_dict: String,
    pub application: &'static str,
}

pub(crate) fn is_empty_dir(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut it| it.next().is_none())
}

pub(crate) fn write_all(dir: &Path, files: &[File]) -> Result<()> {
    for file in files {
        let path = dir.join(file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CaseError::Io(format!("cannot create '{}': {e}", parent.display())))?;
        }
        fs::write(&path, &file.text)
            .map_err(|e| CaseError::Io(format!("cannot write '{}': {e}", path.display())))?;
    }
    Ok(())
}

pub(crate) struct MeshSize {
    pub cells: usize,
    pub points: usize,
}

/// Meshes the `blockMeshDict` the case was written with, into `constant/polyMesh`.
pub(crate) fn mesh(dir: &Path, text: &str) -> Result<MeshSize> {
    let path = dir.join("system").join("blockMeshDict");
    let (_, dict) = FoamFile::from_bytes(path, text.as_bytes().to_vec())
        .dictionary()
        .map_err(|e| CaseError::Mesh(e.to_string()))?;
    let raw =
        vortia_mesh::blockmesh::block_mesh(&dict).map_err(|e| CaseError::Mesh(e.to_string()))?;
    raw.write_case(dir)
        .map_err(|e| CaseError::Io(e.to_string()))?;
    Ok(MeshSize {
        cells: raw.owner.iter().max().map_or(0, |m| m + 1),
        points: raw.points.len(),
    })
}
