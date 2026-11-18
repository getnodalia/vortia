//! A geometry file on Home: an STL read on a thread, its size and whether it is closed, and
//! the surface to turn around with the mouse.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use vortia_mesh::stl::{Stl, Summary};

use crate::i18n::tr;
use crate::surface::Surface;
use crate::viewer::{Source, Viewer};

/// What a successful read leaves.
pub struct Loaded {
    pub summary: Summary,
    pub viewer: Viewer,
}

enum State {
    Reading(Receiver<Result<(Summary, Surface), String>>),
    Ready(Box<Loaded>),
    Failed(String),
}

/// One geometry file, from the first byte read to the picture.
pub struct StlView {
    pub path: PathBuf,
    state: State,
    source: Source,
}

/// Reads and measures a file; the error is a sentence for the user.
pub fn load(path: &Path) -> Result<(Summary, Surface), String> {
    let stl = Stl::read(path).map_err(|e| e.to_string())?;
    let name = path
        .file_stem()
        .map_or_else(|| "STL".to_owned(), |n| n.to_string_lossy().into_owned());
    Ok((stl.summary(), Surface::from_stl(&name, &stl)))
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
            Ok(Ok((summary, surface))) => {
                self.state = State::Ready(Box::new(Loaded {
                    summary,
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

    pub fn loaded(&mut self) -> Option<(&Summary, &mut Viewer, &Source)> {
        match &mut self.state {
            State::Ready(loaded) => Some((&loaded.summary, &mut loaded.viewer, &self.source)),
            _ => None,
        }
    }

    pub fn summary(&self) -> Option<&Summary> {
        match &self.state {
            State::Ready(loaded) => Some(&loaded.summary),
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
        let (summary, surface) = load(&file).unwrap();
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
    fn a_missing_file_is_an_error_not_a_panic() {
        assert!(load(Path::new("/no/such/file.stl")).is_err());
    }
}
