//! An ANSYS Workbench project (`.wbpj`): the files it lists, and the Fluent case to open.
//!
//! A `.wbpj` holds no geometry, mesh or results. It is an index: each file of the project is
//! a `FileReference` whose `Location` is relative to the project, such as
//! `$(ProjectName)_files\dp0\FFF\Fluent\FFF-1.cas.h5`, which lives in the `<name>_files`
//! folder Workbench keeps next to the `.wbpj`. This reads that index and picks the Fluent
//! file Vortia can show: the solved case, else the set-up case, else the mesh.
//!
//! The file is untrusted: it is read up to a size, a location may not leave the project's
//! folder, and anything not understood is skipped rather than guessed.

use std::path::{Component, Path, PathBuf};

use crate::error::{MeshError, Result};

/// The largest `.wbpj` read; real ones are tens of kilobytes.
const MAX_BYTES: u64 = 16 << 20;

/// What a file of the project is, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A Fluent case in the common file format (`.cas.h5`).
    Case,
    /// Fluent results in the common file format (`.dat.h5`).
    Data,
    /// A Fluent mesh in the common file format (`.msh.h5`).
    MeshH5,
    /// A Fluent mesh in the traditional format (`.msh`).
    Mesh,
    /// Anything else: geometry, settings, post-processing state.
    Other,
}

impl Kind {
    fn of(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".cas.h5") {
            Kind::Case
        } else if lower.ends_with(".dat.h5") {
            Kind::Data
        } else if lower.ends_with(".msh.h5") {
            Kind::MeshH5
        } else if lower.ends_with(".msh") {
            Kind::Mesh
        } else {
            Kind::Other
        }
    }
}

/// One file the project lists.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFile {
    /// The name Workbench shows, e.g. `FFF-1.cas.h5`.
    pub name: String,
    /// Where it is on this machine.
    pub path: PathBuf,
    pub kind: Kind,
    /// The cells of the schematic it belongs to: `Mesh`, `Setup`, `Solution`, …
    pub cells: Vec<String>,
}

impl ProjectFile {
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    fn in_cell(&self, cell: &str) -> bool {
        self.cells.iter().any(|c| c == cell)
    }
}

/// A Workbench project.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// The project's name: the `.wbpj` file name without its extension.
    pub name: String,
    /// The `<name>_files` folder that holds its data.
    pub files_dir: PathBuf,
    /// Every file the index lists, in its order.
    pub files: Vec<ProjectFile>,
}

impl Project {
    /// Reads the index of a `.wbpj`.
    pub fn read(path: &Path) -> Result<Self> {
        let invalid = |why: String| MeshError::Invalid(format!("{}: {why}", path.display()));
        let size = std::fs::metadata(path)
            .map_err(|e| invalid(e.to_string()))?
            .len();
        if size > MAX_BYTES {
            return Err(invalid("too large for a Workbench project".into()));
        }
        let bytes = std::fs::read(path).map_err(|e| invalid(e.to_string()))?;
        let text = String::from_utf8_lossy(&bytes);
        if !text.contains("<Storage>") || !text.contains("FileReference") {
            return Err(invalid("not an ANSYS Workbench project (.wbpj)".into()));
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = path.parent().unwrap_or(Path::new("."));
        Ok(Self::parse(&text, &name, dir))
    }

    /// The index in `text`, of a project called `name` whose `.wbpj` is in `dir`.
    pub fn parse(text: &str, name: &str, dir: &Path) -> Self {
        let mut files = Vec::new();
        for block in text
            .split("<Object Name=\"/FileContainer/FileReference:")
            .skip(1)
        {
            let Some(data) = between(block, "<member-data valType=\"String\">", "</member-data>")
            else {
                continue;
            };
            let data = unescape(data);
            let (Some(shown), Some(location)) = (
                string_field(&data, "DisplayText"),
                string_field(&data, "Location"),
            ) else {
                continue;
            };
            let Some(path) = resolve(&location, name, dir) else {
                continue;
            };
            files.push(ProjectFile {
                kind: Kind::of(&shown),
                name: shown,
                path,
                cells: list_field(&data, "ContainerAssociations"),
            });
        }
        Self {
            name: name.to_owned(),
            files_dir: dir.join(format!("{name}_files")),
            files,
        }
    }

    /// The Fluent file to show, of those on disk: the solved case, the set-up case, any
    /// other case, the mesh in the common format, the traditional mesh.
    pub fn fluent_file(&self) -> Option<&ProjectFile> {
        let present = || self.files.iter().filter(|f| f.exists());
        present()
            .find(|f| f.kind == Kind::Case && f.in_cell("Solution"))
            .or_else(|| present().find(|f| f.kind == Kind::Case && f.in_cell("Setup")))
            .or_else(|| present().find(|f| f.kind == Kind::Case))
            .or_else(|| present().find(|f| f.kind == Kind::MeshH5))
            .or_else(|| present().find(|f| f.kind == Kind::Mesh))
    }

    /// The results that go with the project's solved case, if they are on disk.
    pub fn results(&self) -> Option<&ProjectFile> {
        self.files
            .iter()
            .find(|f| f.kind == Kind::Data && f.exists())
    }

    /// The Fluent files the project lists, present or not.
    pub fn fluent_files(&self) -> impl Iterator<Item = &ProjectFile> {
        self.files.iter().filter(|f| f.kind != Kind::Other)
    }

    /// Why nothing can be opened: a sentence that names the folder and the files expected.
    pub fn missing(&self) -> String {
        let expected: Vec<&str> = self.fluent_files().map(|f| f.name.as_str()).collect();
        let folder = self
            .files_dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if expected.is_empty() {
            return format!(
                "the Workbench project '{}' lists no Fluent case or mesh",
                self.name
            );
        }
        if self.files_dir.is_dir() {
            format!(
                "none of the Fluent files the Workbench project '{}' lists is in its folder '{folder}': {}",
                self.name,
                expected.join(", ")
            )
        } else {
            format!(
                "a .wbpj holds no mesh or case: they are in the folder '{folder}', which must be next to it \
                 (copy the whole project, or archive it from Workbench as a .wbpz). It lists {}",
                expected.join(", ")
            )
        }
    }
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = text.get(from..)?.find(end)? + from;
    text.get(from..to)
}

/// The XML entities a member-data string uses.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `"key": "value"` or `"key": r"value"` in a member-data dictionary.
fn string_field(data: &str, key: &str) -> Option<String> {
    let after = data.split(&format!("\"{key}\":")).nth(1)?.trim_start();
    let after = after.strip_prefix('r').unwrap_or(after);
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = after.get(1..)?;
    let end = rest.find(quote)?;
    rest.get(..end).map(str::to_owned)
}

/// `"key": ["a", "b"]` in a member-data dictionary.
fn list_field(data: &str, key: &str) -> Vec<String> {
    let Some(after) = data.split(&format!("\"{key}\":")).nth(1) else {
        return Vec::new();
    };
    let Some(inner) = between(after, "[", "]") else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A `Location` on this machine: `$(ProjectName)` is the project's name, both separators
/// work, and a location that is absolute or climbs out of the project is not followed.
fn resolve(location: &str, name: &str, dir: &Path) -> Option<PathBuf> {
    let relative = location.replace("$(ProjectName)", name).replace('\\', "/");
    if relative.starts_with('/') || relative.contains(':') {
        return None;
    }
    let mut path = dir.to_path_buf();
    for part in Path::new(&relative).components() {
        match part {
            Component::Normal(p) => path.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(n: u32, shown: &str, location: &str, cells: &str) -> String {
        format!(
            "<Object Name=\"/FileContainer/FileReference:F{n}\" Version=\"1.0\">\n\
             <member-data valType=\"String\">{{\"DisplayText\": \"{shown}\", \"FileType\": $$x, \
             \"ContainerAssociations\": [{cells}], \"Location\": r\"{location}\", \"ReferenceCount\": 1}}</member-data>\n\
             </Object>\n"
        )
    }

    fn project() -> String {
        format!(
            "<?xml version=\"1.0\"?>\n<Storage><Containers>\n{}{}{}{}{}{}</Containers></Storage>",
            reference(
                1,
                "FFF.scdocx",
                r"$(ProjectName)_files\dp0\FFF\DM\FFF.scdocx",
                "\"Geometry\""
            ),
            reference(
                5,
                "FFF.msh",
                r"$(ProjectName)_files\dp0\FFF\MECH\FFF.msh",
                "\"Mesh\", \"Setup\""
            ),
            reference(
                7,
                "FFF-Setup-Output.cas.h5",
                r"$(ProjectName)_files\dp0\FFF\Fluent\FFF-Setup-Output.cas.h5",
                "\"Setup\""
            ),
            reference(
                10,
                "FFF-1.cas.h5",
                r"$(ProjectName)_files\dp0\FFF\Fluent\FFF-1.cas.h5",
                "\"Solution\""
            ),
            reference(
                11,
                "FFF-1-08120.dat.h5",
                r"$(ProjectName)_files\dp0\FFF\Fluent\FFF-1-08120.dat.h5",
                "\"Solution\""
            ),
            reference(66, "evil", r"..\..\etc\passwd", "\"Setup\""),
        )
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn reads_the_index_as_workbench_writes_it() {
        let dir = Path::new("/projects");
        let p = Project::parse(&project(), "ANSYS COMPLETO", dir);
        let names: Vec<&str> = p.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "FFF.scdocx",
                "FFF.msh",
                "FFF-Setup-Output.cas.h5",
                "FFF-1.cas.h5",
                "FFF-1-08120.dat.h5"
            ],
            "a location that climbs out of the project is dropped"
        );
        let case = &p.files[3];
        assert_eq!(case.kind, Kind::Case);
        assert_eq!(case.cells, ["Solution"]);
        assert_eq!(
            case.path,
            Path::new("/projects/ANSYS COMPLETO_files/dp0/FFF/Fluent/FFF-1.cas.h5")
        );
        assert_eq!(p.files[1].cells, ["Mesh", "Setup"]);
        assert_eq!(p.files_dir, Path::new("/projects/ANSYS COMPLETO_files"));
    }

    #[test]
    fn opens_the_solved_case_then_the_setup_then_the_mesh() {
        let tmp = tempfile::tempdir().unwrap();
        let wbpj = tmp.path().join("P.wbpj");
        std::fs::write(&wbpj, project()).unwrap();
        let fluent = tmp.path().join("P_files/dp0/FFF/Fluent");
        let read = || Project::read(&wbpj).unwrap();

        // Nothing but the index: a sentence that names the folder to copy.
        let p = read();
        assert!(p.fluent_file().is_none());
        let why = p.missing();
        assert!(
            why.contains("P_files") && why.contains("FFF-1.cas.h5") && why.contains(".wbpz"),
            "{why}"
        );

        touch(&tmp.path().join("P_files/dp0/FFF/MECH/FFF.msh"));
        assert_eq!(read().fluent_file().unwrap().name, "FFF.msh");
        touch(&fluent.join("FFF-Setup-Output.cas.h5"));
        assert_eq!(
            read().fluent_file().unwrap().name,
            "FFF-Setup-Output.cas.h5"
        );
        touch(&fluent.join("FFF-1.cas.h5"));
        assert_eq!(read().fluent_file().unwrap().name, "FFF-1.cas.h5");
        assert!(read().results().is_none());
        touch(&fluent.join("FFF-1-08120.dat.h5"));
        assert_eq!(read().results().unwrap().name, "FFF-1-08120.dat.h5");
    }

    #[test]
    fn what_is_not_a_project_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.wbpj");
        std::fs::write(&path, "hello").unwrap();
        assert!(Project::read(&path).is_err());
        assert!(Project::read(&tmp.path().join("none.wbpj")).is_err());
    }

    #[test]
    fn every_truncation_of_an_index_is_survived() {
        let text = project();
        for cut in 0..text.len() {
            if let Some(part) = text.get(..cut) {
                let _ = Project::parse(part, "P", Path::new("/p"));
            }
        }
    }

    #[test]
    fn locations_never_leave_the_project() {
        let dir = Path::new("/p");
        assert_eq!(
            resolve(r"a\b.msh", "P", dir),
            Some(PathBuf::from("/p/a/b.msh"))
        );
        assert_eq!(resolve(r"..\x", "P", dir), None);
        assert_eq!(resolve(r"C:\Users\x.msh", "P", dir), None);
        assert_eq!(resolve("/etc/passwd", "P", dir), None);
    }
}
