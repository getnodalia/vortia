// SPDX-License-Identifier: GPL-3.0-or-later
//! Parts modelled in Vortia: a list of solids put together with Nodalia's kernel, meshed as a
//! closed surface the flow cases take as a body.
//!
//! A [`Part`] starts empty. Each [`Feature`] makes a solid and joins it to what is there (adds
//! it, cuts it away or keeps what they share), so a wing, a body shell or a bluff cylinder is a
//! few lines. Lengths are in metres.

use solidcore_ops::body::{Body, make_box, make_cone, make_cylinder, make_sphere, make_torus};
use solidcore_ops::boolean::{BooleanOp, boolean};
use solidcore_ops::sweep::{Curve, DraftCorner, extrude, revolve};
use solidcore_tess::{MeshTolerance, plan};
use vortia_mesh::stl::Stl;
use vortia_mesh::{MeshError, Result};

/// What a feature makes.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// An axis-aligned box from its lower corner.
    Box {
        origin: [f64; 3],
        size: [f64; 3],
    },
    /// A cylinder about +Z from the centre of its base.
    Cylinder {
        origin: [f64; 3],
        radius: f64,
        height: f64,
    },
    /// A cone or frustum about +Z.
    Cone {
        origin: [f64; 3],
        base_radius: f64,
        top_radius: f64,
        height: f64,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    /// A closed polygon in the plane `z`, its corners `(x, y)` in order, pushed up `height`
    /// along +Z: a wing section, the side of a car, any outline drawn or read from a DXF.
    Extrude {
        corners: Vec<[f64; 2]>,
        z: f64,
        height: f64,
    },
    /// A closed polygon `(r, z)` on one side of the axis, turned `angle` radians (at most a
    /// whole turn) about the line through `center` along +Z: a nose cone, a nozzle, a bottle.
    Revolve {
        corners: Vec<[f64; 2]>,
        center: [f64; 3],
        angle: f64,
    },
    Torus {
        center: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
    },
}

/// How a feature joins what is already there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// Everything of both.
    Add,
    /// What was there less the feature.
    Cut,
    /// Only what both share.
    Keep,
}

/// One step of a part.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub shape: Shape,
    pub join: Join,
}

/// A part: its features in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    pub features: Vec<Feature>,
}

fn refused(what: &str, e: impl std::fmt::Debug) -> MeshError {
    MeshError::Invalid(format!("{what}: {e:?}"))
}

/// The straight edges of a closed polygon, its corners placed by `place`.
fn outline(corners: &[[f64; 2]], place: impl Fn(&[f64; 2]) -> [f64; 3]) -> Result<Vec<Curve>> {
    if corners.len() < 3 || corners.len() > MAX_CORNERS {
        return Err(MeshError::Invalid(format!(
            "an outline has 3 to {MAX_CORNERS} corners, not {}",
            corners.len()
        )));
    }
    if corners.iter().any(|c| c.iter().any(|v| !v.is_finite())) {
        return Err(MeshError::Invalid(
            "a corner of an outline is not a number".into(),
        ));
    }
    let n = corners.len();
    Ok((0..n)
        .filter_map(|i| {
            let a = corners.get(i)?;
            let b = corners.get((i + 1) % n)?;
            Some(Curve::Line {
                start: place(a),
                end: place(b),
            })
        })
        .collect())
}

/// The most corners an outline may have.
pub const MAX_CORNERS: usize = 10_000;

impl Shape {
    fn solid(&self) -> Result<Body> {
        match *self {
            Shape::Box { origin, size } => make_box(origin, size),
            Shape::Cylinder {
                origin,
                radius,
                height,
            } => make_cylinder(origin, radius, height),
            Shape::Cone {
                origin,
                base_radius,
                top_radius,
                height,
            } => make_cone(origin, base_radius, top_radius, height),
            Shape::Sphere { center, radius } => make_sphere(center, radius),
            Shape::Torus {
                center,
                major_radius,
                minor_radius,
            } => make_torus(center, major_radius, minor_radius),
            Shape::Extrude {
                ref corners,
                z,
                height,
            } => extrude(
                &outline(corners, |&[x, y]| [x, y, z])?,
                [0.0, 0.0, 1.0],
                height,
                0.0,
                DraftCorner::Curvature,
                true,
            ),
            Shape::Revolve {
                ref corners,
                center,
                angle,
            } => revolve(
                &outline(corners, |&[r, h]| [center[0] + r, center[1], center[2] + h])?,
                center,
                [0.0, 0.0, 1.0],
                angle,
                true,
            ),
        }
        .map_err(|e| refused("a shape of the part is out of range", e))
    }
}

impl Part {
    /// The solid the features make, or why they cannot.
    pub fn build(&self) -> Result<Body> {
        let mut made: Option<Body> = None;
        for (n, feature) in self.features.iter().enumerate() {
            let solid = feature.shape.solid()?;
            made = Some(match (made, feature.join) {
                (None, Join::Add) => solid,
                (None, _) => {
                    return Err(MeshError::Invalid(
                        "the first feature of a part adds: there is nothing yet to cut or keep"
                            .into(),
                    ));
                }
                (Some(body), join) => {
                    let op = match join {
                        Join::Add => BooleanOp::Union,
                        Join::Cut => BooleanOp::Subtract,
                        Join::Keep => BooleanOp::Intersect,
                    };
                    boolean(body, solid, op)
                        .map_err(|e| refused(&format!("feature {} could not be joined", n + 1), e))?
                        .ok_or_else(|| {
                            MeshError::Invalid(format!("feature {} leaves nothing", n + 1))
                        })?
                }
            });
        }
        made.ok_or_else(|| MeshError::Invalid("the part has no features".into()))
    }

    /// The part as a closed triangle surface. `chord` is the farthest a facet may stand off the
    /// exact surface, as a share of the part's box diagonal.
    pub fn surface(&self, chord: f64) -> Result<Stl> {
        if !chord.is_finite() || chord <= 0.0 || chord > 0.5 {
            return Err(MeshError::Invalid(
                "the chord tolerance is a share of the part's size, above 0 and at most 0.5".into(),
            ));
        }
        let body = self.build()?;
        let (record, _) = body
            .to_record()
            .map_err(|e| refused("the part could not be read back", e))?;
        let size = |plan_chord: f64, diagonal: f64| {
            plan(&record, &MeshTolerance::chord(plan_chord), diagonal)
                .map_err(|e| refused("could not mesh the part", e))?
                .0
                .welded()
                .map(|(w, _)| w)
                .map_err(|e| refused("could not join the part's faces", e))
        };
        let coarse = size(1.0e6, 1.0)?;
        let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in &coarse.positions {
            for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                if let (Some(l), Some(h)) = (low.get_mut(k), high.get_mut(k)) {
                    *l = l.min(c);
                    *h = h.max(c);
                }
            }
        }
        let diagonal = low
            .iter()
            .zip(&high)
            .map(|(l, h)| (h - l) * (h - l))
            .sum::<f64>()
            .sqrt();
        if !diagonal.is_finite() || diagonal <= 0.0 {
            return Err(MeshError::Invalid("the part has no size".into()));
        }
        let welded = size(chord * diagonal, diagonal)?;
        Ok(Stl {
            vertices: welded.positions.iter().map(|p| [p.x, p.y, p.z]).collect(),
            triangles: welded.triangles.clone(),
        })
    }
}
