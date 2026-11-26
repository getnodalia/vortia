// SPDX-License-Identifier: GPL-3.0-or-later
//! A number for each face of some walls, painted on the body in the viewer in place of a field of
//! the case: the pressure and friction coefficients, which belong to the faces of the wall and
//! not to the cells behind them.

use vortia_post::{SurfaceFace, SurfaceReport};

/// The values of one quantity on the faces of some walls, in the order the mesh numbers them.
#[derive(Clone, Debug, PartialEq)]
pub struct FacePaint {
    /// What the numbers are, for the colour bar.
    pub name: String,
    walls: Vec<(String, Vec<f32>)>,
    range: Option<(f32, f32)>,
}

impl FacePaint {
    pub fn new(name: &str, walls: Vec<(String, Vec<f32>)>) -> Self {
        let mut range: Option<(f32, f32)> = None;
        for &v in walls
            .iter()
            .flat_map(|(_, values)| values)
            .filter(|v| v.is_finite())
        {
            range = Some(match range {
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
                None => (v, v),
            });
        }
        Self {
            name: name.to_owned(),
            walls,
            range,
        }
    }

    /// One quantity of a surface report (`pick` takes it from a face), wall by wall.
    pub fn from_report(
        name: &str,
        report: &SurfaceReport,
        pick: impl Fn(&SurfaceFace) -> f64,
    ) -> Self {
        let mut faces = report.faces.iter();
        let walls = report
            .patches
            .iter()
            .map(|(patch, count)| {
                let values = faces
                    .by_ref()
                    .take(*count)
                    .map(|f| pick(f) as f32)
                    .collect();
                (patch.clone(), values)
            })
            .collect();
        Self::new(name, walls)
    }

    /// The smallest and largest finite value.
    pub fn range(&self) -> Option<(f32, f32)> {
        self.range
    }

    /// The index of the wall with this patch name.
    pub fn wall_index(&self, patch: &str) -> Option<usize> {
        self.walls.iter().position(|(name, _)| name == patch)
    }

    /// Where the value on a face of a wall falls in the range, 0 to 1; `None` where it has no
    /// finite value.
    pub fn fraction(&self, wall: usize, face: usize) -> Option<f32> {
        let value = *self.walls.get(wall)?.1.get(face)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_fall_in_the_range_wall_by_wall_and_a_missing_one_has_no_colour() {
        let paint = FacePaint::new(
            "Cp",
            vec![
                ("a".into(), vec![-1.0, 0.0, f32::NAN]),
                ("b".into(), vec![1.0]),
            ],
        );
        assert_eq!(paint.range(), Some((-1.0, 1.0)));
        assert_eq!(paint.wall_index("b"), Some(1));
        assert_eq!(paint.wall_index("c"), None);
        assert_eq!(paint.fraction(0, 0), Some(0.0));
        assert_eq!(paint.fraction(0, 1), Some(0.5));
        assert_eq!(paint.fraction(1, 0), Some(1.0));
        assert_eq!(paint.fraction(0, 2), None, "not a finite number");
        assert_eq!(paint.fraction(0, 3), None, "past the wall");
        assert_eq!(paint.fraction(2, 0), None, "no such wall");
        // One value everywhere is the middle of the scale, and no values give no range.
        let flat = FacePaint::new("Cp", vec![("a".into(), vec![2.0, 2.0])]);
        assert_eq!(flat.fraction(0, 1), Some(0.5));
        assert_eq!(FacePaint::new("Cp", vec![]).range(), None);
    }

    #[test]
    fn a_report_is_painted_wall_by_wall_in_the_order_of_its_faces() {
        use vortia_core::Vec3;
        let face = |cp: f64| SurfaceFace {
            centre: Vec3::new(0.0, 0.0, 0.0),
            normal: Vec3::new(0.0, 0.0, 1.0),
            area: 1.0,
            cp,
            cf: 0.0,
            cf_stream: -cp,
        };
        let report = SurfaceReport {
            time: "1".into(),
            patches: vec![("front".into(), 2), ("back".into(), 1)],
            dynamic_pressure: 0.5,
            faces: vec![face(1.0), face(0.0), face(-1.0)],
        };
        let cp = FacePaint::from_report("Cp", &report, |f| f.cp);
        assert_eq!(cp.range(), Some((-1.0, 1.0)));
        assert_eq!(cp.fraction(0, 0), Some(1.0));
        assert_eq!(cp.fraction(0, 1), Some(0.5));
        assert_eq!(cp.fraction(cp.wall_index("back").unwrap(), 0), Some(0.0));
        let cf = FacePaint::from_report("Cf", &report, |f| f.cf_stream);
        assert_eq!(
            cf.fraction(0, 0),
            Some(0.0),
            "the sign follows the quantity"
        );
    }
}
