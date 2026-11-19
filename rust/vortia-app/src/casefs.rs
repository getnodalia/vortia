//! What is done to a case as a folder: a copy of it to try something else without touching
//! the original. The copy carries what defines the case (`system`, `constant`, the start
//! fields, the sketch) and leaves behind what a run wrote, which the copy will write again.

use std::path::{Path, PathBuf};

use crate::i18n::{tr, trf};

/// Folders deeper than this are not copied; a case is a few levels deep.
const MAX_DEPTH: usize = 12;
/// A case with more entries than this is not a case.
const MAX_ENTRIES: usize = 200_000;

/// Whether a folder of the case's top level is the output of a run, not part of the case.
fn is_output(name: &str) -> bool {
    if matches!(name, "postProcessing" | "VTK" | "dynamicCode" | "pictures") {
        return true;
    }
    if name
        .strip_prefix("processor")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return true;
    }
    // A time directory other than the start: `0.5`, `100`, `1e-05`.
    name != "0" && name.parse::<f64>().is_ok_and(f64::is_finite)
}

/// The folder next to `case` the copy goes to: `name-copy`, then `name-copy-2`…
fn free_name(case: &Path) -> Result<PathBuf, String> {
    let name = case
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| tr("The case has no folder name.").to_owned())?;
    let parent = case.parent().unwrap_or(Path::new("."));
    (1..=1000)
        .map(|n| match n {
            1 => format!("{name}-copy"),
            n => format!("{name}-copy-{n}"),
        })
        .map(|n| parent.join(n))
        .find(|p| !p.exists())
        .ok_or_else(|| tr("There are too many copies of this case.").to_owned())
}

fn copy_dir(
    from: &Path,
    to: &Path,
    depth: usize,
    top: bool,
    entries: &mut usize,
) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(std::io::Error::other("too many files"));
        }
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let kind = entry.file_type()?;
        // A link could lead anywhere, out of the case or round in a circle.
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if top && is_output(&name_text) {
                continue;
            }
            copy_dir(&entry.path(), &to.join(&name), depth + 1, false, entries)?;
        } else if top && (name_text.starts_with("log.") || name_text.ends_with(".log")) {
            continue;
        } else {
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// Copies the case to a new folder next to it and returns that folder.
pub fn duplicate(case: &Path) -> Result<PathBuf, String> {
    if !case.join("system").join("controlDict").is_file() {
        return Err(trf(
            "'{}' is not a case: it has no system/controlDict.",
            &[&case.display()],
        ));
    }
    let to = free_name(case)?;
    let mut entries = 0;
    match copy_dir(case, &to, 0, true, &mut entries) {
        Ok(()) => Ok(to),
        Err(e) => {
            // Half a copy would look like a case and is not one.
            let _ = std::fs::remove_dir_all(&to);
            Err(trf("The case could not be copied: {}", &[&e]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn sample(dir: &Path) -> PathBuf {
        let case = dir.join("nozzle");
        write(&case.join("system/controlDict"), "a");
        write(&case.join("constant/polyMesh/points"), "p");
        write(&case.join("0/p"), "p0");
        write(&case.join("500/p"), "p500");
        write(&case.join("0.5/p"), "p05");
        write(&case.join("postProcessing/line/500/p.csv"), "x");
        write(&case.join("processor0/0/p"), "x");
        write(&case.join("log.rhoSimpleFoam"), "x");
        write(&case.join("sketch.json"), "{}");
        case
    }

    #[test]
    fn a_copy_keeps_what_defines_the_case_and_leaves_the_results() {
        let dir = tempfile::tempdir().unwrap();
        let case = sample(dir.path());
        let copy = duplicate(&case).unwrap();
        assert_eq!(copy, dir.path().join("nozzle-copy"));
        for kept in [
            "system/controlDict",
            "constant/polyMesh/points",
            "0/p",
            "sketch.json",
        ] {
            assert!(copy.join(kept).is_file(), "{kept}");
        }
        for left in [
            "500",
            "0.5",
            "postProcessing",
            "processor0",
            "log.rhoSimpleFoam",
        ] {
            assert!(!copy.join(left).exists(), "{left}");
        }
        // The original is untouched.
        assert!(case.join("500/p").is_file());
        // A second copy does not overwrite the first.
        assert_eq!(duplicate(&case).unwrap(), dir.path().join("nozzle-copy-2"));
    }

    #[test]
    fn what_is_not_a_case_is_not_copied() {
        let dir = tempfile::tempdir().unwrap();
        assert!(duplicate(dir.path()).is_err());
        assert!(duplicate(&dir.path().join("missing")).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let case = sample(dir.path());
        std::os::unix::fs::symlink(dir.path(), case.join("loop")).unwrap();
        let copy = duplicate(&case).unwrap();
        assert!(!copy.join("loop").exists());
    }

    #[test]
    fn output_folders_are_told_from_the_start_fields() {
        for out in [
            "500",
            "0.5",
            "1e-05",
            "processor12",
            "postProcessing",
            "VTK",
        ] {
            assert!(is_output(out), "{out}");
        }
        for case in [
            "0",
            "system",
            "constant",
            "processor",
            "processorX",
            "nan",
            "inf",
        ] {
            assert!(!is_output(case), "{case}");
        }
    }
}
