// SPDX-License-Identifier: GPL-3.0-or-later
//! Parts modelled in Vortia: a list of solids put together with Nodalia's kernel, meshed as a
//! closed surface the flow cases take as a body.
//!
//! A [`Part`] starts empty. Each [`Feature`] makes a solid and joins it to what is there (adds
//! it, cuts it away or keeps what they share), so a wing, a body shell or a bluff cylinder is a
//! few lines. Lengths are in metres.

use crate::dxf::{Drawing, Outline};
use serde_json::{Value, json};
use solidcore_ops::blend::{Blend, Corners, blend};
use solidcore_ops::body::{Body, make_box, make_cone, make_cylinder, make_sphere, make_torus};
use solidcore_ops::boolean::{BooleanOp, boolean};
use solidcore_ops::pick::{self, Miss};
use solidcore_ops::sweep::{Curve, DraftCorner, extrude, revolve};
use solidcore_tess::{MeshTolerance, plan};
use std::path::Path;
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
    /// Rounds the edges of what the part is so far, each the one nearest a point of the list
    /// (within a millimetre), with a rolling ball of this radius. It needs something to round,
    /// and its join is ignored.
    Round {
        points: Vec<[f64; 3]>,
        size: f64,
    },
    /// Bevels those edges, the same distance back along both faces.
    Chamfer {
        points: Vec<[f64; 3]>,
        size: f64,
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

/// The outline of a NACA 4-digit wing section as a closed polygon `(x, y)`: the leading edge at
/// the origin, the trailing edge at `(chord, 0)`, `points` along each side. Feed it to
/// [`Shape::Extrude`] for a wing of constant section.
pub fn naca4(code: u32, chord: f64, points: usize) -> Result<Vec<[f64; 2]>> {
    if code > 9999 || code % 100 < 1 {
        return Err(MeshError::Invalid(format!(
            "a NACA 4-digit section is 0001 to 9999, not {code}"
        )));
    }
    if !chord.is_finite() || chord <= 0.0 || !(4..=2_000).contains(&points) {
        return Err(MeshError::Invalid(
            "a wing section needs a positive chord and 4 to 2000 points along each side".into(),
        ));
    }
    let m = f64::from(code / 1000) / 100.0;
    let p = f64::from(code / 100 % 10) / 10.0;
    let t = f64::from(code % 100) / 100.0;
    if m > 0.0 && p == 0.0 {
        return Err(MeshError::Invalid(
            "a cambered section needs the position of its camber (the second digit)".into(),
        ));
    }
    let side = |sign: f64| {
        (0..=points).map(move |i| {
            let x = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / points as f64).cos());
            let yt = 5.0
                * t
                * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x.powi(3)
                    - 0.1036 * x.powi(4));
            let (yc, dyc) = if m == 0.0 {
                (0.0, 0.0)
            } else if x < p {
                (
                    m / (p * p) * (2.0 * p * x - x * x),
                    2.0 * m / (p * p) * (p - x),
                )
            } else {
                let q = (1.0 - p) * (1.0 - p);
                (
                    m / q * (1.0 - 2.0 * p + 2.0 * p * x - x * x),
                    2.0 * m / q * (p - x),
                )
            };
            let (sin, cos) = dyc.atan().sin_cos();
            [
                chord * (x - sign * yt * sin),
                chord * (yc + sign * yt * cos),
            ]
        })
    };
    let mut corners: Vec<[f64; 2]> = side(1.0).collect();
    if let Some(last) = corners.last_mut() {
        *last = [chord, 0.0];
    }
    if let Some(first) = corners.first_mut() {
        *first = [0.0, 0.0];
    }
    // Back along the lower side, leaving out the two edges already there.
    let lower: Vec<[f64; 2]> = side(-1.0).collect();
    corners.extend(lower.iter().rev().skip(1).take(points.saturating_sub(1)));
    Ok(corners)
}

/// The outline of a DXF drawing, in metres, for [`Shape::Extrude`]: the largest closed outline of
/// the drawing, whose millimetres become metres. A circle is cut into 96 straight pieces.
pub fn outline_from_drawing(drawing: &Drawing) -> Result<Vec<[f64; 2]>> {
    let bounds = |points: &[[f64; 2]]| {
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in points {
            for k in 0..2 {
                if let (Some(&c), Some(l), Some(h)) = (p.get(k), low.get_mut(k), high.get_mut(k)) {
                    *l = l.min(c);
                    *h = h.max(c);
                }
            }
        }
        (high[0] - low[0]) * (high[1] - low[1])
    };
    let polygons = drawing.outlines.iter().map(|outline| match outline {
        Outline::Circle { center, radius, .. } => (0..96)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 96.0;
                [center.0 + radius * a.cos(), center.1 + radius * a.sin()]
            })
            .collect::<Vec<_>>(),
        Outline::Loop(edges) => {
            let mut corners: Vec<[f64; 2]> = Vec::new();
            for &(x, y) in edges.iter().flat_map(|e| &e.points) {
                if corners.last() != Some(&[x, y]) {
                    corners.push([x, y]);
                }
            }
            if corners.len() > 1 && corners.first() == corners.last() {
                corners.pop();
            }
            corners
        }
    });
    let mut best: Option<Vec<[f64; 2]>> = None;
    for polygon in polygons {
        if best.as_ref().is_none_or(|b| bounds(&polygon) > bounds(b)) {
            best = Some(polygon);
        }
    }
    let corners = best.ok_or_else(|| {
        MeshError::Invalid("the drawing has no closed outline to push into a solid".into())
    })?;
    Ok(corners
        .iter()
        .map(|&[x, y]| [x * 1.0e-3, y * 1.0e-3])
        .collect())
}

/// The outer outline of a sketch, in metres, for [`Shape::Extrude`] or [`Shape::Revolve`]: the
/// sketch is read as a region (every line and arc in a closed loop), its boundary is the largest
/// loop, arcs are cut into steps of at most 3 degrees, and its millimetres become metres. The
/// corners come out counter-clockwise.
pub fn outline_from_sketch(sketch: &vortia_sketch::Sketch) -> Result<Vec<[f64; 2]>> {
    let loops = sketch
        .loops(3.0)
        .map_err(|e| MeshError::Invalid(format!("the sketch is not a closed outline: {e}")))?;
    let outer = loops
        .first()
        .ok_or_else(|| MeshError::Invalid("the sketch has no closed outline".into()))?;
    let mut corners: Vec<[f64; 2]> = Vec::new();
    for &(x, y) in outer.iter().flat_map(|e| &e.points) {
        if corners.last() != Some(&[x, y]) {
            corners.push([x, y]);
        }
    }
    if corners.len() > 1 && corners.first() == corners.last() {
        corners.pop();
    }
    if corners.len() < 3 {
        return Err(MeshError::Invalid(
            "the sketch outline has fewer than three corners".into(),
        ));
    }
    if corners.len() > MAX_CORNERS {
        return Err(MeshError::Invalid(format!(
            "the sketch outline has more than {MAX_CORNERS} corners"
        )));
    }
    let twice_area: f64 = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(p, q)| p[0] * q[1] - q[0] * p[1])
        .sum();
    if twice_area < 0.0 {
        corners.reverse();
    }
    Ok(corners
        .iter()
        .map(|&[x, y]| [x * 1.0e-3, y * 1.0e-3])
        .collect())
}

/// The most corners an outline may have.
pub const MAX_CORNERS: usize = 10_000;

/// The most features a part file holds.
pub const MAX_FEATURES: usize = 256;

/// The largest part file read.
const MAX_FILE_BYTES: u64 = 16 << 20;

const FORMAT: &str = "vortia-part";

fn bad(what: impl Into<String>) -> MeshError {
    MeshError::Invalid(format!("the part file is not valid: {}", what.into()))
}

fn number(v: &Value, what: &str) -> Result<f64> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| bad(format!("{what} is not a number")))
}

fn field<'a>(o: &'a Value, name: &str) -> Result<&'a Value> {
    o.get(name)
        .ok_or_else(|| bad(format!("`{name}` is missing")))
}

fn num(o: &Value, name: &str) -> Result<f64> {
    number(field(o, name)?, name)
}

fn triple(o: &Value, name: &str) -> Result<[f64; 3]> {
    match field(o, name)?.as_array().map(Vec::as_slice) {
        Some([x, y, z]) => Ok([number(x, name)?, number(y, name)?, number(z, name)?]),
        _ => Err(bad(format!("`{name}` is not three numbers"))),
    }
}

fn points(o: &Value) -> Result<Vec<[f64; 3]>> {
    let list = field(o, "points")?
        .as_array()
        .ok_or_else(|| bad("`points` is not a list"))?;
    if list.len() > MAX_PICKS {
        return Err(bad(format!("more than {MAX_PICKS} points")));
    }
    list.iter()
        .map(|c| match c.as_array().map(Vec::as_slice) {
            Some([x, y, z]) => Ok([
                number(x, "a point")?,
                number(y, "a point")?,
                number(z, "a point")?,
            ]),
            _ => Err(bad("a point is not three numbers")),
        })
        .collect()
}

fn corners(o: &Value) -> Result<Vec<[f64; 2]>> {
    let list = field(o, "corners")?
        .as_array()
        .ok_or_else(|| bad("`corners` is not a list"))?;
    if list.len() > MAX_CORNERS {
        return Err(bad(format!("more than {MAX_CORNERS} corners")));
    }
    list.iter()
        .map(|c| match c.as_array().map(Vec::as_slice) {
            Some([a, b]) => Ok([number(a, "a corner")?, number(b, "a corner")?]),
            _ => Err(bad("a corner is not two numbers")),
        })
        .collect()
}

impl Shape {
    fn to_json(&self) -> Value {
        match self {
            Shape::Box { origin, size } => {
                json!({"shape": "box", "origin": origin, "size": size})
            }
            Shape::Cylinder {
                origin,
                radius,
                height,
            } => json!({"shape": "cylinder", "origin": origin, "radius": radius, "height": height}),
            Shape::Cone {
                origin,
                base_radius,
                top_radius,
                height,
            } => json!({"shape": "cone", "origin": origin, "base_radius": base_radius,
                "top_radius": top_radius, "height": height}),
            Shape::Sphere { center, radius } => {
                json!({"shape": "sphere", "center": center, "radius": radius})
            }
            Shape::Torus {
                center,
                major_radius,
                minor_radius,
            } => json!({"shape": "torus", "center": center, "major_radius": major_radius,
                "minor_radius": minor_radius}),
            Shape::Round { points, size } => {
                json!({"shape": "round", "points": points, "size": size})
            }
            Shape::Chamfer { points, size } => {
                json!({"shape": "chamfer", "points": points, "size": size})
            }
            Shape::Extrude { corners, z, height } => {
                json!({"shape": "extrude", "corners": corners, "z": z, "height": height})
            }
            Shape::Revolve {
                corners,
                center,
                angle,
            } => json!({"shape": "revolve", "corners": corners, "center": center, "angle": angle}),
        }
    }

    fn from_json(o: &Value) -> Result<Self> {
        let kind = field(o, "shape")?
            .as_str()
            .ok_or_else(|| bad("`shape` is not a name"))?;
        Ok(match kind {
            "box" => Shape::Box {
                origin: triple(o, "origin")?,
                size: triple(o, "size")?,
            },
            "cylinder" => Shape::Cylinder {
                origin: triple(o, "origin")?,
                radius: num(o, "radius")?,
                height: num(o, "height")?,
            },
            "cone" => Shape::Cone {
                origin: triple(o, "origin")?,
                base_radius: num(o, "base_radius")?,
                top_radius: num(o, "top_radius")?,
                height: num(o, "height")?,
            },
            "sphere" => Shape::Sphere {
                center: triple(o, "center")?,
                radius: num(o, "radius")?,
            },
            "torus" => Shape::Torus {
                center: triple(o, "center")?,
                major_radius: num(o, "major_radius")?,
                minor_radius: num(o, "minor_radius")?,
            },
            "round" => Shape::Round {
                points: points(o)?,
                size: num(o, "size")?,
            },
            "chamfer" => Shape::Chamfer {
                points: points(o)?,
                size: num(o, "size")?,
            },
            "extrude" => Shape::Extrude {
                corners: corners(o)?,
                z: num(o, "z")?,
                height: num(o, "height")?,
            },
            "revolve" => Shape::Revolve {
                corners: corners(o)?,
                center: triple(o, "center")?,
                angle: num(o, "angle")?,
            },
            other => return Err(bad(format!("`{other}` is not a shape"))),
        })
    }
}

impl Join {
    fn name(self) -> &'static str {
        match self {
            Join::Add => "add",
            Join::Cut => "cut",
            Join::Keep => "keep",
        }
    }
}

impl Part {
    /// The part as the JSON a file holds.
    pub fn to_json(&self) -> Value {
        let features: Vec<Value> = self
            .features
            .iter()
            .map(|f| {
                let mut o = f.shape.to_json();
                if let Some(map) = o.as_object_mut() {
                    map.insert("join".into(), f.join.name().into());
                }
                o
            })
            .collect();
        json!({"format": FORMAT, "version": 1, "features": features})
    }

    /// A part from the JSON a file holds, every number checked.
    pub fn from_json(value: &Value) -> Result<Self> {
        if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(bad("it is not a Vortia part"));
        }
        let list = field(value, "features")?
            .as_array()
            .ok_or_else(|| bad("`features` is not a list"))?;
        if list.len() > MAX_FEATURES {
            return Err(bad(format!("more than {MAX_FEATURES} features")));
        }
        let features = list
            .iter()
            .map(|o| {
                let join = match field(o, "join")?.as_str() {
                    Some("add") => Join::Add,
                    Some("cut") => Join::Cut,
                    Some("keep") => Join::Keep,
                    _ => return Err(bad("`join` is not add, cut or keep")),
                };
                Ok(Feature {
                    shape: Shape::from_json(o)?,
                    join,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { features })
    }

    /// Reads a part file.
    pub fn open(path: &Path) -> Result<Self> {
        let io = |e: std::io::Error| MeshError::Invalid(format!("{}: {e}", path.display()));
        if std::fs::metadata(path).map_err(io)?.len() > MAX_FILE_BYTES {
            return Err(bad("the file is too large"));
        }
        let text = std::fs::read_to_string(path).map_err(io)?;
        let value: Value =
            serde_json::from_str(&text).map_err(|e| bad(format!("not valid JSON: {e}")))?;
        Self::from_json(&value)
    }

    /// The part as STEP (ISO 10303-21) text: the exact solid the kernel built, not its mesh.
    ///
    /// Parts are in metres and the STEP writer declares millimetres, so the body is scaled by
    /// 1000 about the origin first; another program opens it at the size it has here.
    pub fn to_step(&self, name: &str) -> Result<String> {
        let body = self.build()?;
        let scale = solidcore_ops::affine::scaling([0.0; 3], 1000.0)
            .map_err(|e| refused("the part could not be scaled to millimetres", e))?;
        body.transform(&scale)
            .map_err(|e| refused("the part could not be scaled to millimetres", e))?;
        let name = if name.trim().is_empty() {
            "Part"
        } else {
            name.trim()
        };
        let header = solidcore_step::write::product::Header {
            file_name: format!("{name}.step"),
            description: "A part modelled in Vortia".into(),
            originating_system: "Vortia".into(),
            ..Default::default()
        };
        solidcore_step::write::product::solidcore_bodies(&[(name, &body)], &header)
            .map_err(|e| refused("the part could not be written as STEP", e))
    }

    /// Writes the part to `path` as a STEP file, through a temporary file.
    pub fn save_step(&self, path: &Path) -> Result<()> {
        let io = |e: std::io::Error| MeshError::Invalid(format!("{}: {e}", path.display()));
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Part");
        let text = self.to_step(name)?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp);
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    /// Writes the part to `path`, through a temporary file so a failure leaves the old one.
    pub fn save(&self, path: &Path) -> Result<()> {
        let io = |e: std::io::Error| MeshError::Invalid(format!("{}: {e}", path.display()));
        let text = serde_json::to_string_pretty(&self.to_json())
            .map_err(|e| MeshError::Invalid(e.to_string()))?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp);
        std::fs::write(&tmp, format!("{text}\n")).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }
}

/// How far from an edge a point may be and still mean it, in metres.
const PICK_TOLERANCE: f64 = 1.0e-3;

/// The most edges one round or chamfer takes.
pub const MAX_PICKS: usize = 64;

impl Shape {
    /// The kind, the pick points and the size of a round or a chamfer.
    fn finish(&self) -> Option<(Blend, &[[f64; 3]], f64)> {
        match self {
            Shape::Round { points, size } => Some((Blend::Round, points, *size)),
            Shape::Chamfer { points, size } => Some((Blend::Chamfer, points, *size)),
            _ => None,
        }
    }
}

/// Rounds or bevels, in place, the edges of `body` nearest `points`.
fn round_edges(
    body: &Body,
    kind: Blend,
    points: &[[f64; 3]],
    size: f64,
    feature: usize,
) -> Result<()> {
    if points.is_empty() || points.len() > MAX_PICKS {
        return Err(MeshError::Invalid(format!(
            "feature {feature} takes 1 to {MAX_PICKS} edges, not {}",
            points.len()
        )));
    }
    if !size.is_finite() || size <= 0.0 {
        return Err(MeshError::Invalid(format!(
            "feature {feature}: the size of a round or chamfer is a positive length"
        )));
    }
    let edges = match pick::edges(body, points, PICK_TOLERANCE)
        .map_err(|e| refused(&format!("feature {feature} could not find its edges"), e))?
    {
        Ok(edges) => edges,
        Err(Miss::None(i)) => {
            return Err(MeshError::Invalid(format!(
                "feature {feature}: point {} is not within a millimetre of an edge of the part",
                i + 1
            )));
        }
        Err(Miss::Ambiguous(i)) => {
            return Err(MeshError::Invalid(format!(
                "feature {feature}: point {} is as near to two edges: move it along its edge",
                i + 1
            )));
        }
    };
    let sizes = vec![size; edges.len()];
    blend(body, kind, &edges, &sizes, &[], Corners::Filled)
        .map(|_| ())
        .map_err(|e| refused(&format!("feature {feature} could not round its edges"), e))
}

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
            Shape::Round { .. } | Shape::Chamfer { .. } => {
                return Err(MeshError::Invalid(
                    "a round or a chamfer works on the part so far, not on a solid of its own"
                        .into(),
                ));
            }
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
            if let Some((kind, points, size)) = feature.shape.finish() {
                let body = made.take().ok_or_else(|| {
                    MeshError::Invalid(format!(
                        "feature {} rounds or bevels edges, and there is nothing yet",
                        n + 1
                    ))
                })?;
                round_edges(&body, kind, points, size, n + 1)?;
                made = Some(body);
                continue;
            }
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
            parts: Vec::new(),
        })
    }
}
