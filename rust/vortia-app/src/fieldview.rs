//! A field of a case on the cells of its mesh, for the viewer to colour the surface with: read
//! from a time directory as one number per cell (a vector gives its length), with the range of
//! the numbers, and a colour scale to turn them into colours.

use crate::i18n::{tr, trf};
use crate::prefs::ColourScale;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, channel};

use vortia_core::Vec3;
use vortia_fields::{Field, Location};

/// A field as one number per cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellValues {
    pub name: String,
    pub time: String,
    /// `scalar` or `vector` (then the numbers are lengths).
    pub kind: &'static str,
    pub values: Vec<f32>,
    /// The smallest and largest of the finite values; `None` when there are none.
    pub range: Option<(f32, f32)>,
}

impl CellValues {
    fn new(name: &str, time: &str, kind: &'static str, values: Vec<f32>) -> Self {
        let mut range: Option<(f32, f32)> = None;
        for &v in values.iter().filter(|v| v.is_finite()) {
            range = Some(match range {
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
                None => (v, v),
            });
        }
        Self {
            name: name.to_owned(),
            time: time.to_owned(),
            kind,
            values,
            range,
        }
    }

    /// Widens the range to take in `other`'s, so two regions share one colour scale.
    pub fn share_range(&mut self, other: Option<(f32, f32)>) {
        self.range = match (self.range, other) {
            (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
            (one, None) | (None, one) => one,
        };
    }

    /// Where the value of a cell falls in the range, 0 to 1; `None` for a cell with no finite
    /// value.
    pub fn fraction(&self, cell: usize) -> Option<f32> {
        let value = *self.values.get(cell)?;
        let (lo, hi) = self.range?;
        if !value.is_finite() {
            return None;
        }
        Some(if hi > lo {
            ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
        } else {
            0.5
        })
    }
}

/// Reads `<case>/<time>/<name>` for a mesh of `cells` cells.
pub fn read(case: &Path, time: &str, name: &str, cells: usize) -> Result<CellValues, String> {
    read_in(case, None, time, name, cells)
}

/// [`read`] for a region of the case (`<case>/<time>/<region>/<name>`), the main one when `None`.
pub fn read_in(
    case: &Path,
    region: Option<&str>,
    time: &str,
    name: &str,
    cells: usize,
) -> Result<CellValues, String> {
    // The names come from folder listings, but a hostile folder can hold anything.
    let plain =
        |part: &str| !part.is_empty() && !part.contains(['/', '\\']) && part != ".." && part != ".";
    if !plain(time) || !plain(name) {
        return Err(trf("'{}/{}' is not a field of the case.", &[&time, &name]));
    }
    let mut path = case.join(time);
    if let Some(region) = region.filter(|r| plain(r)) {
        path.push(region);
    }
    let path = path.join(name);
    let too_few = |n: usize| {
        trf(
            "{} has {} values, but the mesh has {} cells.",
            &[&name, &n, &cells],
        )
    };
    let faces = || trf("{} has a value on each face, not on each cell.", &[&name]);
    match Field::<f64>::read(&path) {
        Ok(field) if field.location != Location::Cells => Err(faces()),
        Ok(field) => {
            let values = field
                .internal
                .expand(cells)
                .ok_or_else(|| too_few(field.internal.count().unwrap_or(0)))?;
            Ok(CellValues::new(
                name,
                time,
                "scalar",
                values.into_iter().map(|v| v as f32).collect(),
            ))
        }
        Err(scalar_error) => match Field::<Vec3>::read(&path) {
            Ok(field) if field.location != Location::Cells => Err(faces()),
            Ok(field) => {
                let values = field
                    .internal
                    .expand(cells)
                    .ok_or_else(|| too_few(field.internal.count().unwrap_or(0)))?;
                Ok(CellValues::new(
                    name,
                    time,
                    "vector",
                    values
                        .into_iter()
                        .map(|v| (v.x * v.x + v.y * v.y + v.z * v.z).sqrt() as f32)
                        .collect(),
                ))
            }
            // The scalar reading's error says more when the file is not a vector either.
            Err(_) => Err(trf(
                "{} cannot be shown on the mesh: {}",
                &[&name, &scalar_error],
            )),
        },
    }
}

/// Reads a field on a thread; `wake` is called when it is ready.
pub fn spawn(
    case: PathBuf,
    time: String,
    name: String,
    cells: usize,
    wake: impl Fn() + Send + 'static,
) -> Receiver<Result<CellValues, String>> {
    spawn_in(case, None, time, name, cells, wake)
}

/// [`spawn`] for a region of the case.
pub fn spawn_in(
    case: PathBuf,
    region: Option<&'static str>,
    time: String,
    name: String,
    cells: usize,
    wake: impl Fn() + Send + 'static,
) -> Receiver<Result<CellValues, String>> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("vortia-field".into())
        .spawn(move || {
            let read = std::panic::catch_unwind(|| read_in(&case, region, &time, &name, cells))
                .unwrap_or_else(|_| Err(tr("The field could not be read.").to_owned()));
            let _ = tx.send(read);
            wake();
        });
    // No thread, no field: the viewer keeps the colours of the patches.
    drop(spawned);
    rx
}

/// The rainbow, from low to high: deep blue, blue, teal, green, yellow, orange, red.
const RAINBOW: [[u8; 3]; 7] = [
    [0x26, 0x2a, 0x7a],
    [0x2c, 0x6b, 0xc9],
    [0x22, 0xa7, 0xa8],
    [0x5f, 0xc2, 0x5d],
    [0xe6, 0xd8, 0x3c],
    [0xf2, 0x8e, 0x2b],
    [0xc9, 0x2e, 0x2e],
];

/// Viridis (CC0), which reads in grey scale and for the common colour-vision deficiencies.
const VIRIDIS: [[u8; 3]; 7] = [
    [0x44, 0x01, 0x54],
    [0x41, 0x44, 0x87],
    [0x2a, 0x78, 0x8e],
    [0x22, 0xa8, 0x84],
    [0x7a, 0xd1, 0x51],
    [0xbd, 0xdf, 0x26],
    [0xfd, 0xe7, 0x25],
];

/// Dark to light, for a picture that is printed in black and white.
const GREYS: [[u8; 3]; 2] = [[0x1c, 0x1f, 0x26], [0xf0, 0xf2, 0xf5]];

/// The scale and the digits the pictures are drawn with, set from the Preferences.
static SCALE: AtomicU8 = AtomicU8::new(0);
static DIGITS: AtomicU8 = AtomicU8::new(DEFAULT_DIGITS);

/// Digits of a number on the scale's labels when the user chose none.
pub const DEFAULT_DIGITS: u8 = 4;

/// Makes `scale` and `digits` what every picture of fields uses from now on.
pub fn configure(scale: ColourScale, digits: u8) {
    SCALE.store(scale.index(), Ordering::Relaxed);
    DIGITS.store(digits.clamp(2, 9), Ordering::Relaxed);
}

/// The colour for a place on the scale, 0 to 1; a place outside it is the nearest end.
pub fn scale(t: f32) -> [u8; 3] {
    scale_of(ColourScale::from_index(SCALE.load(Ordering::Relaxed)), t)
}

/// [`scale`] for a chosen colour scale.
pub fn scale_of(kind: ColourScale, t: f32) -> [u8; 3] {
    match kind {
        ColourScale::Rainbow => along(&RAINBOW, t),
        ColourScale::Viridis => along(&VIRIDIS, t),
        ColourScale::Greys => along(&GREYS, t),
    }
}

fn along(stops: &[[u8; 3]], t: f32) -> [u8; 3] {
    // An infinite place is the nearest end; one that is not a number is the low end.
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let position = t * stops.len().saturating_sub(1) as f32;
    let index = (position.floor() as usize).min(stops.len().saturating_sub(2));
    let along = position - index as f32;
    let (Some(a), Some(b)) = (stops.get(index), stops.get(index + 1)) else {
        return [0; 3];
    };
    let mix = |i: usize| -> u8 {
        let (x, y) = (
            f32::from(a.get(i).copied().unwrap_or(0)),
            f32::from(b.get(i).copied().unwrap_or(0)),
        );
        (x + (y - x) * along).round().clamp(0.0, 255.0) as u8
    };
    [mix(0), mix(1), mix(2)]
}

/// A number for a label of the scale: few digits, scientific when it is very small or large.
pub fn label(value: f32) -> String {
    label_with(DIGITS.load(Ordering::Relaxed), value)
}

/// [`label`] with `digits` significant digits (decimals in the plain form).
pub fn label_with(digits: u8, value: f32) -> String {
    let digits = usize::from(digits.clamp(2, 9));
    let magnitude = value.abs();
    if value == 0.0 || (1e-3..1e5).contains(&magnitude) {
        let text = format!("{value:.digits$}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        text.to_owned()
    } else {
        format!("{value:.prec$e}", prec = digits - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, time: &str, name: &str, class: &str, internal: &str) {
        let path = dir.join(time).join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            format!(
                "FoamFile {{ version 2.0; format ascii; class {class}; object {name}; }}\n\
                 dimensions [0 0 0 0 0 0 0];\ninternalField {internal};\n\
                 boundaryField {{ wall {{ type zeroGradient; }} }}\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_scalar_field_gives_its_values_and_range() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "0.5",
            "p",
            "volScalarField",
            "nonuniform List<scalar> 3(1 5 3)",
        );
        let field = read(dir.path(), "0.5", "p", 3).unwrap();
        assert_eq!(field.kind, "scalar");
        assert_eq!(field.values, [1.0, 5.0, 3.0]);
        assert_eq!(field.range, Some((1.0, 5.0)));
        assert_eq!(field.fraction(0), Some(0.0));
        assert_eq!(field.fraction(1), Some(1.0));
        assert_eq!(field.fraction(2), Some(0.5));
        assert_eq!(field.fraction(9), None);
    }

    #[test]
    fn a_region_has_its_fields_in_its_own_folder_and_shares_the_scale() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "1",
            "T",
            "volScalarField",
            "nonuniform List<scalar> 2(300 310)",
        );
        write(
            dir.path(),
            "1/solid",
            "T",
            "volScalarField",
            "nonuniform List<scalar> 2(320 360)",
        );
        let mut fluid = read(dir.path(), "1", "T", 2).unwrap();
        let mut solid = read_in(dir.path(), Some("solid"), "1", "T", 2).unwrap();
        assert_eq!(solid.values, [320.0, 360.0]);
        fluid.share_range(solid.range);
        solid.share_range(fluid.range);
        assert_eq!(fluid.range, Some((300.0, 360.0)));
        assert_eq!(solid.range, fluid.range);
        // Hotter in the solid than in the air, on one scale.
        assert!(solid.fraction(1) > fluid.fraction(1));
        let mut lone = read(dir.path(), "1", "T", 2).unwrap();
        lone.share_range(None);
        assert_eq!(lone.range, Some((300.0, 310.0)));
    }

    #[test]
    fn a_uniform_field_fills_every_cell_and_sits_in_the_middle_of_the_scale() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "0", "T", "volScalarField", "uniform 300");
        let field = read(dir.path(), "0", "T", 4).unwrap();
        assert_eq!(field.values, [300.0; 4]);
        assert_eq!(field.fraction(2), Some(0.5));
    }

    #[test]
    fn a_vector_field_is_shown_by_its_length() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "0",
            "U",
            "volVectorField",
            "nonuniform List<vector> 2((3 4 0)(0 0 2))",
        );
        let field = read(dir.path(), "0", "U", 2).unwrap();
        assert_eq!(field.kind, "vector");
        assert_eq!(field.values, [5.0, 2.0]);
    }

    #[test]
    fn a_field_of_another_size_or_place_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "0",
            "p",
            "volScalarField",
            "nonuniform List<scalar> 2(1 2)",
        );
        write(dir.path(), "0", "phi", "surfaceScalarField", "uniform 0");
        assert!(
            read(dir.path(), "0", "p", 3)
                .unwrap_err()
                .contains("2 values")
        );
        assert!(read(dir.path(), "0", "phi", 3).is_err());
        assert!(read(dir.path(), "0", "missing", 3).is_err());
        for hostile in ["../p", "a/b", "..", ""] {
            assert!(read(dir.path(), "0", hostile, 2).is_err());
            assert!(read(dir.path(), hostile, "p", 2).is_err());
        }
    }

    #[test]
    fn numbers_that_are_not_finite_have_no_colour_and_do_not_set_the_range() {
        let field = CellValues::new("x", "0", "scalar", vec![f32::NAN, 2.0, f32::INFINITY, 4.0]);
        assert_eq!(field.range, Some((2.0, 4.0)));
        assert_eq!(field.fraction(0), None);
        assert_eq!(field.fraction(2), None);
        assert_eq!(
            CellValues::new("x", "0", "scalar", vec![f32::NAN]).range,
            None
        );
    }

    #[test]
    fn the_scale_runs_from_the_first_stop_to_the_last_and_never_leaves_them() {
        assert_eq!(scale_of(ColourScale::Rainbow, 0.0), RAINBOW[0]);
        assert_eq!(scale_of(ColourScale::Rainbow, 1.0), RAINBOW[6]);
        assert_eq!(scale_of(ColourScale::Rainbow, -5.0), RAINBOW[0]);
        assert_eq!(scale_of(ColourScale::Rainbow, 5.0), RAINBOW[6]);
        assert_eq!(scale_of(ColourScale::Rainbow, f32::NAN), RAINBOW[0]);
        assert_eq!(scale_of(ColourScale::Rainbow, 0.5), RAINBOW[3]);
    }

    #[test]
    fn labels_are_short() {
        assert_eq!(label_with(DEFAULT_DIGITS, 0.0), "0");
        assert_eq!(label_with(DEFAULT_DIGITS, 101325.0), "1.013e5");
        assert_eq!(label_with(DEFAULT_DIGITS, 300.5), "300.5");
        assert_eq!(label_with(DEFAULT_DIGITS, -0.25), "-0.25");
        assert_eq!(label_with(DEFAULT_DIGITS, 2.5e-6), "2.500e-6");
    }

    #[test]
    fn every_colour_scale_has_its_own_ends_and_survives_hostile_places() {
        for kind in ColourScale::ALL {
            let (low, high) = (scale_of(kind, 0.0), scale_of(kind, 1.0));
            assert_ne!(low, high, "{kind:?}");
            assert_eq!(scale_of(kind, f32::INFINITY), high);
            assert_eq!(scale_of(kind, f32::NEG_INFINITY), low);
        }
        assert_eq!(scale_of(ColourScale::Viridis, 0.0), VIRIDIS[0]);
        assert_eq!(scale_of(ColourScale::Greys, 1.0), GREYS[1]);
    }

    #[test]
    fn the_digits_of_a_label_follow_the_choice_and_stay_in_range() {
        assert_eq!(label_with(2, 300.567), "300.57");
        assert_eq!(label_with(6, 300.5), "300.5");
        assert_eq!(label_with(6, 101325.0), "1.01325e5");
        // Nonsense digits are clamped rather than panicking in the formatter.
        assert!(!label_with(0, 1.234_567).is_empty());
        assert!(!label_with(200, 1.234_567).is_empty());
    }
}
