//! Choosing the edges of a modelled part with the mouse.
//!
//! A part's surface is a mesh; the edges a person means (the rim of a hole, the corner of a
//! block) are the places where the mesh folds. [`feature_edges`] finds them, [`nearest`] says
//! which one is under the pointer, and the viewer draws them and hands back the one clicked.

use std::collections::HashMap;

use vortia_mesh::stl::Stl;

/// The most edges a surface offers for picking; beyond it the surface is too fine to pick on.
pub const MAX_EDGES: usize = 40_000;

/// Two faces fold into an edge when their normals differ by more than this many degrees. A
/// tessellated cylinder's neighbouring facets differ by a few degrees and are not edges.
pub const FOLD_DEGREES: f64 = 25.0;

/// A straight piece of an edge, with the normals of the one or two faces that meet at it.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureEdge {
    pub a: [f64; 3],
    pub b: [f64; 3],
    /// The normals of the faces on each side; the second is zero on an open edge.
    pub normals: [[f64; 3]; 2],
}

impl FeatureEdge {
    /// The middle of the piece: a point the kernel finds on its edge.
    pub fn middle(&self) -> [f64; 3] {
        [
            (self.a[0] + self.b[0]) / 2.0,
            (self.a[1] + self.b[1]) / 2.0,
            (self.a[2] + self.b[2]) / 2.0,
        ]
    }

    /// How far `p` is from this piece, in metres.
    pub fn distance(&self, p: &[f64; 3]) -> f64 {
        let ab = sub(&self.b, &self.a);
        let ap = sub(p, &self.a);
        let length2 = dot(&ab, &ab);
        let t = if length2 > 0.0 {
            (dot(&ap, &ab) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let nearest = [
            self.a[0] + ab[0] * t,
            self.a[1] + ab[1] * t,
            self.a[2] + ab[2] * t,
        ];
        let d = sub(p, &nearest);
        dot(&d, &d).sqrt()
    }

    /// Whether some face of the edge looks towards `viewer` (a direction): an edge on the far
    /// side of the part is not one the person can point at.
    pub fn faces(&self, viewer: &[f64; 3]) -> bool {
        let open = self.normals.get(1).is_none_or(|n| dot(n, n) == 0.0);
        open || self.normals.iter().any(|n| dot(n, viewer) > -1.0e-9)
    }
}

fn sub(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = dot(&v, &v).sqrt();
    if length > 0.0 && length.is_finite() {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        [0.0; 3]
    }
}

/// The folds and open borders of a surface, up to [`MAX_EDGES`] of them.
pub fn feature_edges(stl: &Stl) -> Vec<FeatureEdge> {
    // A tessellated seam repeats its vertices; welding by position makes both sides of it one
    // edge, as the part really has.
    let mut welded: HashMap<[i64; 3], u32> = HashMap::new();
    let mut points: Vec<[f64; 3]> = Vec::new();
    let ids: Vec<u32> = stl
        .vertices
        .iter()
        .map(|v| {
            let key = v.map(|x| (x * 1.0e6).round() as i64);
            *welded.entry(key).or_insert_with(|| {
                points.push(*v);
                (points.len() - 1) as u32
            })
        })
        .collect();
    let point = |i: u32| points.get(i as usize).copied();
    let mut sides: HashMap<(u32, u32), Vec<[f64; 3]>> = HashMap::new();
    for triangle in &stl.triangles {
        let [i, j, k] = *triangle;
        let id = |n: u32| ids.get(n as usize).copied();
        let (Some(i), Some(j), Some(k)) = (id(i), id(j), id(k)) else {
            continue;
        };
        let (Some(a), Some(b), Some(c)) = (point(i), point(j), point(k)) else {
            continue;
        };
        if i == j || j == k || k == i {
            continue;
        }
        let normal = unit(cross(&sub(&b, &a), &sub(&c, &a)));
        for (p, q) in [(i, j), (j, k), (k, i)] {
            sides.entry((p.min(q), p.max(q))).or_default().push(normal);
        }
    }
    let cos_fold = FOLD_DEGREES.to_radians().cos();
    let mut edges: Vec<FeatureEdge> = sides
        .into_iter()
        .filter_map(|((p, q), faces)| {
            let folds = match faces.as_slice() {
                [n, m] => dot(n, m) < cos_fold,
                _ => true,
            };
            if !folds {
                return None;
            }
            let normals = [
                faces.first().copied().unwrap_or([0.0; 3]),
                faces.get(1).copied().unwrap_or([0.0; 3]),
            ];
            Some(FeatureEdge {
                a: point(p)?,
                b: point(q)?,
                normals,
            })
        })
        .collect();
    // A hash map has no order; the same part always gives the same list.
    edges.sort_by(|x, y| {
        x.a.partial_cmp(&y.a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(x.b.partial_cmp(&y.b).unwrap_or(std::cmp::Ordering::Equal))
    });
    edges.truncate(MAX_EDGES);
    edges
}

/// The index of the edge nearest `pointer` within `reach` pixels, among those that face the
/// viewer. `screen` puts a point of the part on the screen.
pub fn nearest(
    edges: &[FeatureEdge],
    screen: impl Fn(&[f64; 3]) -> [f32; 2],
    viewer: &[f64; 3],
    pointer: [f32; 2],
    reach: f32,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (n, edge) in edges.iter().enumerate() {
        if !edge.faces(viewer) {
            continue;
        }
        let a = screen(&edge.a);
        let b = screen(&edge.b);
        let d = distance_to_segment(pointer, a, b);
        if d <= reach && best.is_none_or(|(_, nearest)| d < nearest) {
            best = Some((n, d));
        }
    }
    best.map(|(n, _)| n)
}

fn distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let length2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if length2 > 0.0 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = [ap[0] - ab[0] * t, ap[1] - ab[1] * t];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortia_cad::part::{Feature, Join, Part, Shape};

    fn part(shape: Shape) -> Part {
        Part {
            features: vec![Feature {
                shape,
                join: Join::Add,
            }],
        }
    }

    #[test]
    fn a_block_has_twelve_edges_and_a_cylinder_two_rims() {
        let block = part(Shape::Box {
            origin: [0.0; 3],
            size: [1.0, 2.0, 3.0],
        })
        .surface(0.001)
        .unwrap();
        let edges = feature_edges(&block);
        // Each side of a block is two triangles, so the diagonal of every side is flat and the
        // twelve folds are all there is, each in one piece.
        assert_eq!(edges.len(), 12);

        let can = part(Shape::Cylinder {
            origin: [0.0; 3],
            radius: 0.5,
            height: 1.0,
        })
        .surface(0.001)
        .unwrap();
        let edges = feature_edges(&can);
        assert!(edges.len() > 20, "{} pieces", edges.len());
        // Every piece lies on one of the two rims.
        for edge in &edges {
            let z = edge.a[2];
            assert!(z.abs() < 1e-9 || (z - 1.0).abs() < 1e-9, "{edge:?}");
            assert!((edge.a[2] - edge.b[2]).abs() < 1e-9, "{edge:?}");
        }
    }

    #[test]
    fn the_middle_of_a_piece_is_found_on_its_edge_and_rounds_it() {
        // Every one of the twelve edges of a block, and the rim of a cylinder, taken by the
        // middle of a piece of the mesh as the viewer hands it over.
        let block = Shape::Box {
            origin: [0.0; 3],
            size: [1.0, 2.0, 3.0],
        };
        for shape in [
            block,
            Shape::Cylinder {
                origin: [0.0; 3],
                radius: 0.5,
                height: 1.0,
            },
        ] {
            let base = part(shape);
            let surface = base.surface(1.0e-3).unwrap();
            let edges = feature_edges(&surface);
            assert!(!edges.is_empty());
            for edge in edges.iter().step_by(edges.len() / 3 + 1) {
                let mut rounded = base.clone();
                rounded.features.push(Feature {
                    shape: Shape::Chamfer {
                        points: vec![edge.middle()],
                        size: 0.02,
                    },
                    join: Join::Add,
                });
                let made = rounded.build();
                assert!(made.is_ok(), "{:?}: {:?}", edge.middle(), made.err());
            }
        }
    }

    #[test]
    fn the_pointer_finds_the_edge_it_is_on_and_not_one_behind_the_part() {
        let edges = vec![
            FeatureEdge {
                a: [0.0, 0.0, 0.0],
                b: [1.0, 0.0, 0.0],
                normals: [[0.0, 0.0, 1.0], [0.0, -0.7, 0.7]],
            },
            FeatureEdge {
                a: [0.0, 0.0, 0.0],
                b: [1.0, 0.0, 0.0],
                normals: [[0.0, 0.0, -1.0], [0.0, -0.7, -0.7]],
            },
        ];
        // A view along -z: x to the right, y down the screen, 100 px a metre.
        let screen = |p: &[f64; 3]| [p[0] as f32 * 100.0, p[1] as f32 * 100.0];
        let towards = [0.0, 0.0, 1.0];
        assert_eq!(nearest(&edges, screen, &towards, [50.0, 3.0], 8.0), Some(0));
        assert_eq!(nearest(&edges, screen, &towards, [50.0, 30.0], 8.0), None);
        // Seen from behind, the first edge is the hidden one.
        let behind = [0.0, 0.0, -1.0];
        assert_eq!(nearest(&edges, screen, &behind, [50.0, 3.0], 8.0), Some(1));
    }
}
