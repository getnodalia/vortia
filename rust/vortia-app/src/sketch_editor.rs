//! The sketch behind "New > From sketch" (R2 of `docs/architecture/rust-only.md`): a template to
//! start from, its dimensions to edit, and the wall they make, kept solved. The state is plain data
//! so everything but the drawing is tested without a window.

use crate::i18n::{tr, trf};
use std::path::Path;

use vortia_sketch::{Constraint, Entity, Sketch, SketchError, templates};

/// The sketches the editor starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Duct,
    Nozzle,
    RoundedNozzle,
}

impl Template {
    pub const ALL: [Template; 3] = [Template::Duct, Template::Nozzle, Template::RoundedNozzle];

    pub fn title(self) -> &'static str {
        match self {
            Template::Duct => tr("Straight duct"),
            Template::Nozzle => tr("Conical nozzle"),
            Template::RoundedNozzle => tr("Rounded throat"),
        }
    }

    fn sketch(self) -> Sketch {
        match self {
            Template::Duct => templates::duct(500.0, 25.0),
            Template::Nozzle => templates::nozzle(50.0, 26.0, 60.0, 158.0, 420.0),
            Template::RoundedNozzle => {
                templates::rounded_nozzle(50.0, 26.0, 60.0, 158.0, 420.0, 30.0)
            }
        }
    }
}

/// One dimension of the sketch, as the editor lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Dimension {
    /// Which of the sketch's constraints it is.
    pub index: usize,
    pub label: String,
    pub unit: &'static str,
    pub value: f64,
}

/// A sketch that is always solved: an edit that cannot be solved is refused and leaves it as it was.
#[derive(Clone, Debug)]
pub struct SketchEditor {
    pub template: Template,
    sketch: Sketch,
    /// Why the last edit was refused, until the next one that works.
    pub problem: Option<String>,
}

impl Default for SketchEditor {
    fn default() -> Self {
        Self::new(Template::RoundedNozzle)
    }
}

impl SketchEditor {
    pub fn new(template: Template) -> Self {
        let mut sketch = template.sketch();
        let problem = sketch.solve().err().map(|e| e.to_string());
        Self {
            template,
            sketch,
            problem,
        }
    }

    pub fn sketch(&self) -> &Sketch {
        &self.sketch
    }

    pub fn restart(&mut self, template: Template) {
        *self = Self::new(template);
    }

    /// The dimensions that have a number to edit, in the order the sketch holds them.
    pub fn dimensions(&self) -> Vec<Dimension> {
        let item = |index: usize, label: String, unit, value| Dimension {
            index,
            label,
            unit,
            value,
        };
        self.sketch
            .constraints
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match *c {
                Constraint::Distance { a, b, value } => Some(item(
                    i,
                    trf("Distance, point {} to {}", &[&a, &b]),
                    "mm",
                    value,
                )),
                Constraint::DeltaX { a, b, value } => Some(item(
                    i,
                    trf("Length along, point {} to {}", &[&a, &b]),
                    "mm",
                    value,
                )),
                Constraint::DeltaY { a, b, value } => Some(item(
                    i,
                    trf("Height, point {} to {}", &[&a, &b]),
                    "mm",
                    value,
                )),
                Constraint::Radius { arc, value } => {
                    Some(item(i, trf("Radius of arc {}", &[&arc]), "mm", value))
                }
                Constraint::Angle { a, b, degrees } => Some(item(
                    i,
                    trf("Angle, point {} to {}", &[&a, &b]),
                    "°",
                    degrees,
                )),
                Constraint::Horizontal { .. } | Constraint::Vertical { .. } => None,
            })
            .collect()
    }

    /// Gives dimension `index` a new value and solves again. On an error the sketch is as it was,
    /// and the reason is kept in [`Self::problem`].
    pub fn set_dimension(&mut self, index: usize, value: f64) -> Result<(), SketchError> {
        let mut next = self.sketch.clone();
        match next.constraints.get_mut(index) {
            Some(
                Constraint::Distance { value: v, .. }
                | Constraint::DeltaX { value: v, .. }
                | Constraint::DeltaY { value: v, .. }
                | Constraint::Radius { value: v, .. }
                | Constraint::Angle { degrees: v, .. },
            ) => *v = value,
            _ => {
                return Err(SketchError::Invalid(format!(
                    "dimension {index} has no number to change"
                )));
            }
        }
        let solved = next.solve().and_then(|_| next.profile(5.0));
        match solved {
            Ok(_) => {
                self.sketch = next;
                self.problem = None;
                Ok(())
            }
            Err(e) => {
                self.problem = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// What to draw: the wall as a polyline (arcs cut into steps), and the points. When the wall
    /// cannot be read as a chain, the lines between the points stand in for it.
    pub fn outline(&self) -> Outline {
        let wall = self.sketch.profile(5.0).ok();
        let lines = || {
            self.sketch
                .entities
                .iter()
                .filter_map(|e| match *e {
                    Entity::Line { a, b } => Some((
                        self.sketch.points.get(a).copied()?,
                        self.sketch.points.get(b).copied()?,
                    )),
                    Entity::Arc { start, end, .. } => Some((
                        self.sketch.points.get(start).copied()?,
                        self.sketch.points.get(end).copied()?,
                    )),
                })
                .flat_map(|(a, b)| [(a.x, a.y), (b.x, b.y)])
                .collect::<Vec<_>>()
        };
        Outline {
            wall: wall.unwrap_or_else(lines),
            points: self
                .sketch
                .points
                .iter()
                .map(|p| (p.x, p.y, p.fixed))
                .collect(),
        }
    }

    /// Writes the sketch to `path`, so a case keeps the drawing it was meshed from.
    pub fn save(&self, path: &Path) -> Result<(), SketchError> {
        self.sketch.save(path)
    }
}

/// The drawing of a sketch, in millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub wall: Vec<(f64, f64)>,
    /// Each point with whether it is fixed.
    pub points: Vec<(f64, f64, bool)>,
}

impl Outline {
    /// The box that holds the wall and the points, mirrored about the axis: `(x0, x1, half-height)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64)> {
        let xs = self
            .wall
            .iter()
            .copied()
            .chain(self.points.iter().map(|&(x, y, _)| (x, y)));
        let (mut x0, mut x1, mut h) = (f64::MAX, f64::MIN, 0.0_f64);
        let mut any = false;
        for (x, y) in xs {
            any = true;
            x0 = x0.min(x);
            x1 = x1.max(x);
            h = h.max(y.abs());
        }
        any.then_some((x0, x1, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn throat_radius(editor: &SketchEditor) -> f64 {
        editor
            .sketch()
            .profile(2.0)
            .unwrap()
            .iter()
            .fold(f64::MAX, |m, p| m.min(p.1))
    }

    #[test]
    fn every_template_starts_solved_with_dimensions_to_edit() {
        for template in Template::ALL {
            let editor = SketchEditor::new(template);
            assert!(
                editor.problem.is_none(),
                "{template:?}: {:?}",
                editor.problem
            );
            assert!(!editor.dimensions().is_empty(), "{template:?}");
            assert!(editor.outline().wall.len() >= 2, "{template:?}");
        }
    }

    #[test]
    fn a_dimension_changed_moves_the_wall() {
        let mut editor = SketchEditor::new(Template::Nozzle);
        let before = throat_radius(&editor);
        let throat = editor
            .dimensions()
            .into_iter()
            .find(|d| d.label.starts_with("Height") && (d.value - 26.0).abs() < 1e-9)
            .unwrap();
        editor.set_dimension(throat.index, 20.0).unwrap();
        assert!((throat_radius(&editor) - 20.0).abs() < 1e-3);
        assert!(before > 25.0);
    }

    #[test]
    fn an_edit_that_cannot_work_is_refused_and_leaves_the_sketch_alone() {
        let mut editor = SketchEditor::new(Template::Duct);
        let before = editor.sketch().clone();
        let first = editor.dimensions().first().unwrap().index;
        assert!(editor.set_dimension(first, f64::NAN).is_err());
        assert!(editor.problem.is_some());
        assert_eq!(editor.sketch(), &before);
        assert!(editor.set_dimension(999, 1.0).is_err());
        // A good edit afterwards clears the reason.
        editor.set_dimension(first, 400.0).unwrap();
        assert!(editor.problem.is_none());
    }

    #[test]
    fn the_outline_is_bounded_and_the_sketch_is_saved_and_read_back() {
        let editor = SketchEditor::default();
        let (x0, x1, h) = editor.outline().bounds().unwrap();
        assert!(x0 < x1 && h > 0.0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wall.vortia-sketch");
        editor.save(&path).unwrap();
        assert_eq!(&Sketch::open(&path).unwrap(), editor.sketch());
    }
}
