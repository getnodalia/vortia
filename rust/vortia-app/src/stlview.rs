//! A geometry file on Home: an STL, or a Fluent case in the common file format, read on a
//! thread, what it is, and the surface to turn around with the mouse.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use vortia_mesh::stl::{Stl, Summary};

use crate::app::grouped;
use crate::i18n::{tr, trf};
use crate::surface::Surface;
use crate::viewer::{Source, Viewer};

/// What is said about a file.
pub enum About {
    /// An STL: its size and whether it is closed.
    Stl(Summary),
    /// A Fluent case: its mesh, as sentences.
    Case(Vec<String>),
}

/// What a successful read leaves.
pub struct Loaded {
    pub about: About,
    pub viewer: Viewer,
}

enum State {
    Reading(Receiver<Result<(About, Surface), String>>),
    Ready(Box<Loaded>),
    Failed(String),
}

/// One geometry file, from the first byte read to the picture.
pub struct StlView {
    pub path: PathBuf,
    state: State,
    source: Source,
}

/// Whether a path is a Fluent case or mesh in the common file format.
fn is_case(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("h5"))
}

/// What a Fluent case holds, as sentences.
fn case_lines(mesh: &vortia_foamfile::RawPolyMesh, surface: &Surface) -> Vec<String> {
    let mut lines = vec![trf(
        "{} cells, {} faces, {} points",
        &[
            &grouped(surface.cells),
            &grouped(mesh.faces.len()),
            &grouped(mesh.points.len()),
        ],
    )];
    let patches: Vec<String> = mesh
        .patches
        .iter()
        .map(|p| format!("{} ({})", p.name, grouped(p.n_faces)))
        .collect();
    if !patches.is_empty() {
        lines.push(trf("Boundaries: {}", &[&patches.join(", ")]));
    }
    lines
}

/// Reads and measures a file; the error is a sentence for the user.
pub fn load(path: &Path) -> Result<(About, Surface), String> {
    if is_case(path) {
        let mesh = vortia_mesh::cff::cff_to_foam(path, vortia_mesh::fluent::Options::default())
            .map_err(|e| e.to_string())?;
        let surface = Surface::from_mesh(&mesh);
        let lines = case_lines(&mesh, &surface);
        return Ok((About::Case(lines), surface));
    }
    let stl = Stl::read(path).map_err(|e| e.to_string())?;
    let name = path
        .file_stem()
        .map_or_else(|| "STL".to_owned(), |n| n.to_string_lossy().into_owned());
    Ok((About::Stl(stl.summary()), Surface::from_stl(&name, &stl)))
}

impl StlView {
    /// Starts reading `path`; `wake` is called when it is done.
    pub fn open(path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let file = path.clone();
        let spawned = std::thread::Builder::new()
            .name("vortia-stl".into())
            .spawn(move || {
                let read = std::panic::catch_unwind(|| load(&file))
                    .unwrap_or_else(|_| Err(tr("The file could not be read.").to_owned()));
                let _ = tx.send(read);
                wake();
            });
        let state = match spawned {
            Ok(_) => State::Reading(rx),
            Err(_) => State::Failed(tr("The file could not be read.").to_owned()),
        };
        Self {
            path,
            state,
            source: Source::default(),
        }
    }

    /// Takes in a finished read.
    pub fn poll(&mut self) {
        let State::Reading(rx) = &self.state else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok((about, surface))) => {
                self.state = State::Ready(Box::new(Loaded {
                    about,
                    viewer: Viewer::new(surface),
                }));
            }
            Ok(Err(message)) => self.state = State::Failed(message),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.state = State::Failed(tr("The file could not be read.").to_owned());
            }
        }
    }

    pub fn reading(&self) -> bool {
        matches!(self.state, State::Reading(_))
    }

    pub fn failure(&self) -> Option<&str> {
        match &self.state {
            State::Failed(message) => Some(message),
            _ => None,
        }
    }

    pub fn loaded(&mut self) -> Option<(&About, &mut Viewer, &Source)> {
        match &mut self.state {
            State::Ready(loaded) => Some((&loaded.about, &mut loaded.viewer, &self.source)),
            _ => None,
        }
    }

    pub fn about(&self) -> Option<&About> {
        match &self.state {
            State::Ready(loaded) => Some(&loaded.about),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TETRA: &str = "solid t
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 0 1 0
vertex 1 0 0
endloop
endfacet
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 0 1
endloop
endfacet
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 0 0 1
vertex 0 1 0
endloop
endfacet
facet normal 0 0 0
outer loop
vertex 1 0 0
vertex 0 1 0
vertex 0 0 1
endloop
endfacet
endsolid t
";

    #[test]
    fn a_file_is_read_measured_and_turned_into_a_surface() {
        let dir = std::env::temp_dir().join(format!("vortia-stlview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tetra.stl");
        std::fs::write(&file, TETRA).unwrap();
        let (about, surface) = load(&file).unwrap();
        let About::Stl(summary) = about else {
            panic!("an STL is described as an STL");
        };
        assert!(summary.closed());
        assert!((summary.volume.unwrap() - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(surface.triangle_count(), 4);
        assert_eq!(surface.patches.len(), 1);
        assert_eq!(surface.patches[0].name, "tetra");
        assert_eq!(surface.extent, [1.0; 3]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_view_reads_on_a_thread_and_reports_a_bad_file() {
        let dir = std::env::temp_dir().join(format!("vortia-stlview-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("nope.stl");
        std::fs::write(&file, b"not an stl").unwrap();
        let mut view = StlView::open(file, || {});
        for _ in 0..200 {
            view.poll();
            if !view.reading() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(view.failure().is_some());
        assert!(view.loaded().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fluent_case_is_read_as_a_mesh_with_its_boundaries() {
        let file =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../vortia-mesh/tests/cff/hex.cas.h5");
        let (about, surface) = load(&file).unwrap();
        let About::Case(lines) = about else {
            panic!("a case is described as a case");
        };
        assert_eq!(surface.cells, 1);
        assert_eq!(surface.patches.len(), 3);
        assert!(lines[0].contains('1') && lines[0].contains('6') && lines[0].contains('8'));
        assert!(
            lines[1].contains("inlet (1)") && lines[1].contains("wall (4)"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_broken_case_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("vortia-case-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bad.cas.h5");
        std::fs::write(&file, b"not hdf5 at all").unwrap();
        assert!(load(&file).is_err());
        assert!(load(Path::new("/no/such/file.cas.h5")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        assert!(load(Path::new("/no/such/file.stl")).is_err());
    }
}
