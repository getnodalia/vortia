// SPDX-License-Identifier: GPL-3.0-or-later
//! A NACA 4-digit airfoil in a uniform stream: a C-grid of six blocks around the profile and its
//! wake, made by the engine's own `blockMesh`, steady incompressible RANS with Spalart–Allmaras
//! or k-ω SST, and the stream at any angle of attack.
//!
//! The mesh stays with the profile, not with the stream: the chord lies on the x axis and the
//! far-field velocity carries the angle, so one mesh serves every angle of attack.

use crate::error::{CaseError, Result};
use crate::files::{File, FileSet, header};
use crate::{MAX_CELLS, count, size};

/// Which turbulence model the airfoil runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirfoilModel {
    /// Spalart–Allmaras with Spalding's wall function: the aerospace default, good for attached
    /// and mildly separated flow at any first-cell height.
    SpalartAllmaras,
    /// k-ω SST with wall functions (needs the first cell in the log layer, `yplus` of 30 or more).
    KOmegaSst,
}

/// A NACA 4-digit airfoil, meshed with a C-grid, in a stream at an angle of attack.
#[derive(Debug, Clone, PartialEq)]
pub struct Airfoil {
    /// The four digits as a number: `12` is the 0012, `2412` the 2412. The first is the camber
    /// in percent of the chord, the second the position of the camber in tenths of it, the last
    /// two the thickness in percent.
    pub naca: u32,
    /// Angle of attack, in degrees.
    pub alpha: f64,
    /// Chord length, in metres.
    pub chord: f64,
    /// Stream speed, in m/s.
    pub speed: f64,
    /// Reynolds number on the chord. The viscosity follows from it.
    pub reynolds: f64,
    /// Distance from the profile to the far field, in chords.
    pub far: f64,
    /// Length of the wake behind the trailing edge, in chords.
    pub wake: f64,
    /// Cells along each surface.
    pub cells_chord: usize,
    /// Cells from the profile to the far field.
    pub cells_normal: usize,
    /// Cells along the wake.
    pub cells_wake: usize,
    /// The first cell's height, as a `y+` for a flat plate at this Reynolds number.
    pub yplus: f64,
    pub model: AirfoilModel,
    /// Free-stream turbulence intensity of the k-ω SST run (a fraction of the speed).
    pub intensity: f64,
}

impl Default for Airfoil {
    /// The NACA 0012 at 5° and Reynolds 6·10⁶, the case the NASA turbulence modelling resource
    /// publishes numbers for.
    fn default() -> Self {
        Self {
            naca: 12,
            alpha: 5.0,
            chord: 1.0,
            speed: 50.0,
            reynolds: 6.0e6,
            far: 20.0,
            wake: 20.0,
            cells_chord: 120,
            cells_normal: 64,
            cells_wake: 40,
            yplus: 30.0,
            model: AirfoilModel::SpalartAllmaras,
            intensity: 0.001,
        }
    }
}

/// The depth of the one cell the 2D flow is solved in, as a fraction of the chord.
const DEPTH: f64 = 0.1;
/// Points sampled on each surface for the spline the block edge follows.
const SURFACE_POINTS: usize = 160;
/// Points on the quarter circle of the far field.
const ARC_POINTS: usize = 32;
/// Sections of the grading along a surface, which approximate a cosine spacing.
const SECTIONS: usize = 6;
/// The surface sample that ends the nose: the profile's normal has turned by some 65° there.
const NOSE_SAMPLE: usize = 16;

/// The upper and the lower surface, each a list of `(x, y)` in chords.
pub type Surfaces = (Vec<(f64, f64)>, Vec<(f64, f64)>);

impl Airfoil {
    /// Sets one number by its name, for a command line or a script: `naca`, `alpha`, `chord`,
    /// `speed`, `reynolds`, `far`, `wake`, `cells-chord`, `cells-normal`, `cells-wake`, `yplus`,
    /// `model` (0 Spalart–Allmaras, 1 k-ω SST) and `intensity`.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "naca" => self.naca = u32::try_from(crate::whole(name, value)?).unwrap_or(u32::MAX),
            "alpha" => self.alpha = value,
            "chord" => self.chord = value,
            "speed" => self.speed = value,
            "reynolds" => self.reynolds = value,
            "far" => self.far = value,
            "wake" => self.wake = value,
            "yplus" => self.yplus = value,
            "intensity" => self.intensity = value,
            "model" => {
                self.model = if value == 0.0 {
                    AirfoilModel::SpalartAllmaras
                } else if value == 1.0 {
                    AirfoilModel::KOmegaSst
                } else {
                    return Err(CaseError::Invalid(
                        "model is 0 (Spalart–Allmaras) or 1 (k-ω SST)".into(),
                    ));
                }
            }
            "cells-chord" => self.cells_chord = crate::whole(name, value)?,
            "cells-normal" => self.cells_normal = crate::whole(name, value)?,
            "cells-wake" => self.cells_wake = crate::whole(name, value)?,
            _ => return Err(crate::unknown(name, "an airfoil")),
        }
        Ok(())
    }

    /// The camber, its position and the thickness, as fractions of the chord.
    fn digits(&self) -> Result<(f64, f64, f64)> {
        if self.naca > 9999 {
            return Err(CaseError::Invalid(format!(
                "naca must be a 4-digit profile (up to 9999), not {}",
                self.naca
            )));
        }
        let m = f64::from(self.naca / 1000) / 100.0;
        let p = f64::from(self.naca / 100 % 10) / 10.0;
        let t = f64::from(self.naca % 100) / 100.0;
        if t < 0.01 {
            return Err(CaseError::Invalid(
                "a NACA 4-digit profile needs a thickness of at least 1 %".into(),
            ));
        }
        if m > 0.0 && p == 0.0 {
            return Err(CaseError::Invalid(
                "a cambered profile needs the position of its camber (the second digit)".into(),
            ));
        }
        Ok((m, p, t))
    }

    /// The upper and lower surfaces from the leading edge to the trailing edge, in chords, with
    /// the closed trailing edge (the thickness law whose last coefficient is −0.1036).
    pub fn surfaces(&self) -> Result<Surfaces> {
        let (m, p, t) = self.digits()?;
        let mut upper = Vec::with_capacity(SURFACE_POINTS + 1);
        let mut lower = Vec::with_capacity(SURFACE_POINTS + 1);
        for i in 0..=SURFACE_POINTS {
            let beta = std::f64::consts::PI * i as f64 / SURFACE_POINTS as f64;
            let x = 0.5 * (1.0 - beta.cos());
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
                (
                    m / ((1.0 - p) * (1.0 - p)) * (1.0 - 2.0 * p + 2.0 * p * x - x * x),
                    2.0 * m / ((1.0 - p) * (1.0 - p)) * (p - x),
                )
            };
            let (s, c) = dyc.atan().sin_cos();
            upper.push((x - yt * s, yc + yt * c));
            lower.push((x + yt * s, yc - yt * c));
        }
        // The ends are the leading and the trailing edge exactly.
        for side in [&mut upper, &mut lower] {
            if let Some(first) = side.first_mut() {
                *first = (0.0, 0.0);
            }
            if let Some(last) = side.last_mut() {
                *last = (1.0, 0.0);
            }
        }
        Ok((upper, lower))
    }

    pub(crate) fn files(&self) -> Result<FileSet> {
        self.digits()?;
        let chord = size("the chord", self.chord, 1.0e4)?;
        let speed = size("the stream speed", self.speed, 1.0e5)?;
        let reynolds = size("the Reynolds number", self.reynolds, 1.0e10)?;
        let far = size("the distance to the far field", self.far, 1.0e3)?;
        let wake = size("the wake length", self.wake, 1.0e3)?;
        let yplus = size("the y+", self.yplus, 1.0e4)?;
        let intensity = size("the turbulence intensity", self.intensity, 1.0)?;
        if !self.alpha.is_finite() || self.alpha.abs() > 45.0 {
            return Err(CaseError::Invalid(format!(
                "the angle of attack must be within ±45°, not {}",
                self.alpha
            )));
        }
        let n_s = count("the cells along a surface", self.cells_chord, 12, 2000)?;
        let n_j = count("the cells to the far field", self.cells_normal, 8, 1000)?;
        let n_w = count("the cells along the wake", self.cells_wake, 4, 1000)?;
        let cells = 2 * n_s * n_j + 2 * n_w * n_j;
        if cells as u64 > MAX_CELLS {
            return Err(CaseError::Invalid(format!(
                "{cells} cells is more than the {MAX_CELLS} a new case may have"
            )));
        }
        let nu = speed * chord / reynolds;
        let depth = DEPTH * chord;

        // The first cell: the `y+` asked, for a flat plate at the chord's Reynolds number, from
        // Schlichting's skin friction `Cf = (2 log10 Re − 0.65)^−2.3`.
        let cf = (2.0 * reynolds.log10() - 0.65).powf(-2.3);
        let u_tau = speed * (0.5 * cf).sqrt();
        let y1 = yplus * nu / u_tau;
        let radius = far * chord;
        let ej = expansion_for(y1, radius, n_j);

        let (upper, lower) = self.surfaces()?;
        let mid = SURFACE_POINTS / 2;
        let at = |side: &[(f64, f64)], i: usize| side.get(i).copied().unwrap_or((0.0, 0.0));
        let (xu, yu) = at(&upper, mid);
        let (xl, yl) = at(&lower, mid);
        // The points strictly between surface samples `from` and `to` (either order), listed in
        // the direction the edge runs.
        let inner = |side: &[(f64, f64)], from: usize, to: usize, z: f64| -> String {
            let pts: Vec<usize> = if from < to {
                (from + 1..to).collect()
            } else {
                (to + 1..from).rev().collect()
            };
            pts.iter()
                .map(|&i| {
                    let (x, y) = at(side, i);
                    format!("({:.9} {:.9} {z:.9})", x * chord, y * chord)
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        // The outer boundary of a front block: the quarter circle from the stagnation side to
        // the point straight above (or below) the leading edge.
        let outer = |sign: f64, z: f64| -> String {
            (1..=ARC_POINTS)
                .map(|i| {
                    let a = 0.5 * std::f64::consts::PI * i as f64 / ARC_POINTS as f64;
                    format!(
                        "({:.9} {:.9} {z:.9})",
                        -radius * a.cos(),
                        sign * radius * a.sin()
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        };

        // Along a surface from the leading edge: a spacing between uniform and cosine, finer at
        // both ends, in `SECTIONS` sections; each half of a surface takes half of them.
        let surface_grading = |first: usize, last: usize, reverse: bool| -> String {
            let pi = std::f64::consts::PI;
            let law = |xi: f64| 0.3 * xi + 0.7 * 0.5 * (1.0 - (pi * xi).cos());
            let cell = |xi: f64| 0.3 + 0.7 * 0.5 * pi * (pi * xi).sin();
            let half = 0.5 / n_s as f64;
            let mut sections: Vec<(f64, f64, f64)> = (first..last)
                .map(|k| {
                    let (a, b) = (k as f64 / SECTIONS as f64, (k + 1) as f64 / SECTIONS as f64);
                    (law(b) - law(a), 1.0, cell(b - half) / cell(a + half))
                })
                .collect();
            let total: f64 = sections.iter().map(|s| s.0).sum();
            for s in &mut sections {
                s.0 /= total;
                s.1 = 1.0 / (last - first) as f64;
            }
            if reverse {
                sections.reverse();
                for s in &mut sections {
                    s.2 = 1.0 / s.2;
                }
            }
            let body: Vec<String> = sections
                .iter()
                .map(|(l, d, e)| format!("({l:.6} {d:.6} {e:.6})"))
                .collect();
            format!("({})", body.join(" "))
        };
        // Along the wake from the trailing edge, starting as fine as the last surface cell.
        let te_cell = 0.3 / n_s as f64 * chord;
        let ew = expansion_for(te_cell, wake * chord, n_w);

        let xo = (1.0 + wake) * chord;
        let c = chord;
        let corners = [
            (0.0, 0.0),         // 0 leading edge
            (c, 0.0),           // 1 trailing edge
            (-radius, 0.0),     // 2 far field ahead
            (c, radius),        // 3 far field above the trailing edge
            (c, -radius),       // 4 far field below the trailing edge
            (xo, 0.0),          // 5 end of the wake
            (xo, radius),       // 6
            (xo, -radius),      // 7
            (xu * c, yu * c),   // 8 upper surface, mid chord
            (xl * c, yl * c),   // 9 lower surface, mid chord
            (0.5 * c, radius),  // 10 far field above mid chord
            (0.5 * c, -radius), // 11
        ];
        let layer = corners.len();
        let mut vertices = String::new();
        for z in [0.0, depth] {
            for (x, y) in corners {
                vertices.push_str(&format!("    ({x:.9} {y:.9} {z:.9})\n"));
            }
        }
        // Six blocks: the front and the rear of each surface, and the wake above and below the
        // trailing edge. No two blocks share the same pair of end points on an edge, so each
        // surface edge is unambiguous.
        let blocks: [[usize; 4]; 6] = [
            [0, 8, 10, 2],
            [8, 1, 3, 10],
            [1, 9, 11, 4],
            [9, 0, 2, 11],
            [1, 5, 6, 3],
            [5, 1, 4, 7],
        ];
        let hex = |b: &[usize; 4]| {
            let [a, bb, cc, d] = *b;
            format!(
                "{a} {bb} {cc} {d} {} {} {} {}",
                a + layer,
                bb + layer,
                cc + layer,
                d + layer
            )
        };
        let (n_a, n_b) = (n_s / 2, n_s - n_s / 2);
        let half = SECTIONS / 2;
        let grading = [
            surface_grading(0, half, false),
            surface_grading(half, SECTIONS, false),
            surface_grading(half, SECTIONS, true),
            surface_grading(0, half, true),
        ];
        let cells_i = [n_a, n_b, n_b, n_a, n_w, n_w];
        let i_grading = [
            grading[0].clone(),
            grading[1].clone(),
            grading[2].clone(),
            grading[3].clone(),
            format!("{ew:.12}"),
            format!("{:.12}", 1.0 / ew),
        ];

        // The far-field edge of the two front blocks: its cells are not spread like the
        // surface's. Round the nose the profile's normal turns through some 65° within a tenth
        // of the cells, so the far field gets those cells over the same turn of its quarter
        // circle, or the grid lines leave the nose almost along the wall.
        let length = |side: &[(f64, f64)], to: usize| -> f64 {
            side.windows(2)
                .take(to)
                .map(|w| match w {
                    [a, b] => ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt(),
                    _ => 0.0,
                })
                .sum()
        };
        let (before, after) = (at(&upper, NOSE_SAMPLE - 1), at(&upper, NOSE_SAMPLE + 1));
        let turn = (after.0 - before.0).atan2(after.1 - before.1).abs();
        let turn = turn.clamp(0.2, 1.4);
        let nose_length = (turn / std::f64::consts::FRAC_PI_2)
            * (std::f64::consts::FRAC_PI_2 * radius)
            / (std::f64::consts::FRAC_PI_2 * radius + (0.5 * chord));
        // The share of the front block's cells that lie on the nose: the cells of the first
        // section grow geometrically over a fifth of the surface's length.
        let nose_cells = {
            let pi = std::f64::consts::PI;
            let law = |xi: f64| 0.3 * xi + 0.7 * 0.5 * (1.0 - (pi * xi).cos());
            let cell = |xi: f64| 0.3 + 0.7 * 0.5 * pi * (pi * xi).sin();
            let sec = 1.0 / half as f64 * 0.5;
            let share = length(&upper, NOSE_SAMPLE) / length(&upper, mid).max(1e-9);
            let first = (law(sec) - law(0.0)) / (law(0.5) - law(0.0));
            let m = (n_a as f64 / half as f64).max(2.0);
            let growth =
                (cell(sec - 0.5 / n_s as f64) / cell(0.5 / n_s as f64)).powf(1.0 / (m - 1.0));
            let t = (share / first).clamp(0.0, 1.0);
            let c = if growth > 1.0 + 1e-9 {
                (1.0 + t * (growth.powf(m) - 1.0)).ln() / growth.ln()
            } else {
                t * m
            };
            (c / n_a as f64).clamp(0.02, 0.5)
        };
        let far_front = format!(
            "(({nose_length:.6} {nose_cells:.6} 1) ({:.6} {:.6} 1))",
            1.0 - nose_length,
            1.0 - nose_cells
        );
        let far_front_reversed = format!(
            "(({:.6} {:.6} 1) ({nose_length:.6} {nose_cells:.6} 1))",
            1.0 - nose_length,
            1.0 - nose_cells
        );
        // The twelve edges of a block: four along i, four along j, four along k.
        let edge_grading: [[String; 4]; 6] = [
            [
                i_grading[0].clone(),
                far_front.clone(),
                far_front,
                i_grading[0].clone(),
            ],
            std::array::from_fn(|_| i_grading[1].clone()),
            std::array::from_fn(|_| i_grading[2].clone()),
            [
                i_grading[3].clone(),
                far_front_reversed.clone(),
                far_front_reversed,
                i_grading[3].clone(),
            ],
            std::array::from_fn(|_| i_grading[4].clone()),
            std::array::from_fn(|_| i_grading[5].clone()),
        ];
        let mut block_lines = String::new();
        for ((b, n), g) in blocks.iter().zip(cells_i).zip(&edge_grading) {
            let [g0, g1, g2, g3] = g;
            block_lines.push_str(&format!(
                "    hex ({}) ({n} {n_j} 1) edgeGrading ({g0} {g1} {g2} {g3} {ej:.6} {ej:.6} {ej:.6} {ej:.6} 1 1 1 1)\n",
                hex(b)
            ));
        }
        // The six blocks' faces for the boundary: (block, side) with the side the four corners
        // of a block face, by block corner position.
        let face = |b: usize, corners: [usize; 2]| -> String {
            let blk = blocks.get(b).copied().unwrap_or([0; 4]);
            let [p0, p1, p2, p3] = blk;
            let pick = |k: usize| match k {
                0 => p0,
                1 => p1,
                2 => p2,
                _ => p3,
            };
            let [s, e] = corners;
            format!(
                "({} {} {} {})",
                pick(s),
                pick(e),
                pick(e) + layer,
                pick(s) + layer
            )
        };
        let join = |parts: &[String]| parts.join(" ");
        // j = 0: the profile (corners 0-1); j = max: the far field (corners 3-2); the end of
        // the wake: corners 1-2 of the two wake blocks.
        let airfoil_faces = join(&(0..4).map(|b| face(b, [0, 1])).collect::<Vec<_>>());
        let far_faces = join(&(0..6).map(|b| face(b, [2, 3])).collect::<Vec<_>>());
        // The wake's two blocks end at the outlet on different corners of their own.
        let outlet_faces = join(&[face(4, [1, 2]), face(5, [0, 3])]);
        let mut empty_faces = Vec::new();
        for blk in &blocks {
            let [a, b, cc, d] = *blk;
            empty_faces.push(format!("({a} {b} {cc} {d})"));
            empty_faces.push(format!(
                "({} {} {} {})",
                a + layer,
                b + layer,
                cc + layer,
                d + layer
            ));
        }
        let empty_faces = join(&empty_faces);

        let mut edge_lines = String::new();
        for z in [0.0, depth] {
            let back = if z == 0.0 { 0 } else { layer };
            let list = [
                ("spline", 0, 8, inner(&upper, 0, mid, z)),
                ("spline", 8, 1, inner(&upper, mid, SURFACE_POINTS, z)),
                ("spline", 1, 9, inner(&lower, SURFACE_POINTS, mid, z)),
                ("spline", 9, 0, inner(&lower, mid, 0, z)),
                ("polyLine", 2, 10, outer(1.0, z)),
                ("polyLine", 2, 11, outer(-1.0, z)),
            ];
            for (kind, a, b, pts) in list {
                edge_lines.push_str(&format!("    {kind} {} {} ({pts})\n", a + back, b + back));
            }
        }
        let block_mesh_dict = format!(
            "{}vertices\n(\n{vertices});\n\n\
             blocks\n(\n{block_lines});\n\n\
             edges\n(\n{edge_lines});\n\n\
             boundary\n(\n\
             \x20   airfoil\n    {{\n        type wall;\n        faces ({airfoil_faces});\n    }}\n\
             \x20   farfield\n    {{\n        type patch;\n        faces ({far_faces});\n    }}\n\
             \x20   outlet\n    {{\n        type patch;\n        faces ({outlet_faces});\n    }}\n\
             \x20   frontAndBack\n    {{\n        type empty;\n        faces ({empty_faces});\n    }}\n);\n",
            header("dictionary", "", "blockMeshDict"),
        );

        let (sin_a, cos_a) = self.alpha.to_radians().sin_cos();
        let (ux, uy) = (speed * cos_a, speed * sin_a);
        let mut files = vec![
            File {
                path: "system/blockMeshDict",
                text: block_mesh_dict.clone(),
            },
            File {
                path: "constant/airfoilProperties",
                text: format!(
                    "{}naca            {};\nalpha           {};\nchord           {chord};\ndepth           {depth};\nspeed           {speed};\nreynolds        {reynolds};\ndragDir         ({cos_a} {sin_a} 0);\nliftDir         ({} {cos_a} 0);\npitchCentre     ({} 0 0);\n",
                    header("dictionary", "constant", "airfoilProperties"),
                    self.naca,
                    self.alpha,
                    -sin_a,
                    0.25 * chord,
                ),
            },
        ];
        files.extend(self.flow_files((ux, uy), nu, speed, intensity));
        Ok(FileSet {
            files,
            block_mesh_dict,
            raw_mesh: None,
            application: "simpleFoam",
        })
    }

    /// The fields, schemes and settings of the run.
    fn flow_files(&self, (ux, uy): (f64, f64), nu: f64, speed: f64, intensity: f64) -> Vec<File> {
        let spalart = self.model == AirfoilModel::SpalartAllmaras;
        let field = |class: &str,
                     name: &'static str,
                     path: &'static str,
                     dims: &str,
                     internal: &str,
                     patches: [&str; 3]| {
            let [airfoil, farfield, outlet] = patches;
            let block = |patch: &str, body: &str| {
                let mut s = format!("    {patch}\n    {{\n");
                for line in body.lines() {
                    s.push_str(&format!("        {}\n", line.trim_start()));
                }
                s.push_str("    }\n");
                s
            };
            File {
                path,
                text: format!(
                    "{}dimensions      {dims};\n\ninternalField   {internal};\n\nboundaryField\n{{\n{}{}{}{}}}\n",
                    header(class, "", name),
                    block("airfoil", airfoil),
                    block("farfield", farfield),
                    block("outlet", outlet),
                    block("frontAndBack", "type            empty;"),
                ),
            }
        };
        let inlet_out = |v: &str| {
            format!(
                "type            inletOutlet;\ninletValue      uniform {v};\nvalue           uniform {v};"
            )
        };
        let fixed = |v: &str| format!("type            fixedValue;\nvalue           uniform {v};");
        let calculated = "type            calculated;\nvalue           uniform 0;";
        let mut files = vec![
            field(
                "volVectorField",
                "U",
                "0/U",
                "[0 1 -1 0 0 0 0]",
                &format!("uniform ({ux} {uy} 0)"),
                [
                    "type            noSlip;",
                    &fixed(&format!("({ux} {uy} 0)")),
                    "type            zeroGradient;",
                ],
            ),
            field(
                "volScalarField",
                "p",
                "0/p",
                "[0 2 -2 0 0 0 0]",
                "uniform 0",
                [
                    "type            zeroGradient;",
                    "type            zeroGradient;",
                    "type            fixedValue;\nvalue           uniform 0;",
                ],
            ),
        ];
        let (model, schemes, solved, residuals, relaxation): (_, _, &[&str], _, _);
        if spalart {
            // A fully turbulent stream: the working variable at four times the viscosity.
            let nu_tilda = 4.0 * nu;
            files.push(field(
                "volScalarField",
                "nuTilda",
                "0/nuTilda",
                "[0 2 -1 0 0 0 0]",
                &format!("uniform {nu_tilda}"),
                [
                    "type            fixedValue;\nvalue           uniform 0;",
                    &fixed(&nu_tilda.to_string()),
                    &inlet_out(&nu_tilda.to_string()),
                ],
            ));
            files.push(field(
                "volScalarField",
                "nut",
                "0/nut",
                "[0 2 -1 0 0 0 0]",
                "uniform 0",
                [
                    "type            nutUSpaldingWallFunction;\nvalue           uniform 0;",
                    calculated,
                    calculated,
                ],
            ));
            model = "SpalartAllmaras";
            schemes = "    div(phi,nuTilda) bounded Gauss upwind;\n";
            solved = &["nuTilda"];
            residuals = "        nuTilda         1e-06;\n";
            relaxation = "        nuTilda         0.5;\n";
        } else {
            let k = 1.5 * (speed * intensity).powi(2);
            let omega = k / (10.0 * nu);
            for (name, path, dims, value, wall) in [
                ("k", "0/k", "[0 2 -2 0 0 0 0]", k, "kqRWallFunction"),
                (
                    "omega",
                    "0/omega",
                    "[0 0 -1 0 0 0 0]",
                    omega,
                    "omegaWallFunction",
                ),
            ] {
                files.push(field(
                    "volScalarField",
                    name,
                    path,
                    dims,
                    &format!("uniform {value}"),
                    [
                        &format!("type            {wall};\nvalue           uniform {value};"),
                        &fixed(&value.to_string()),
                        &inlet_out(&value.to_string()),
                    ],
                ));
            }
            files.push(field(
                "volScalarField",
                "nut",
                "0/nut",
                "[0 2 -1 0 0 0 0]",
                &format!("uniform {}", k / omega),
                [
                    "type            nutkWallFunction;\nvalue           uniform 0;",
                    calculated,
                    calculated,
                ],
            ));
            model = "kOmegaSST";
            schemes = "    div(phi,k)      bounded Gauss upwind;\n    div(phi,omega)  bounded Gauss upwind;\n";
            solved = &["k", "omega"];
            residuals = "        k               1e-06;\n        omega           1e-06;\n";
            relaxation = "        k               0.5;\n        omega           0.5;\n";
        }
        let extra_solvers: String = solved
            .iter()
            .map(|name| {
                format!(
                    "    {name}\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n\n"
                )
            })
            .collect();
        files.extend([
            File {
                path: "system/controlDict",
                text: format!(
                    "{}application     simpleFoam;\nstartFrom       startTime;\nstartTime       0;\nstopAt          endTime;\nendTime         4000;\ndeltaT          1;\nwriteControl    timeStep;\nwriteInterval   500;\npurgeWrite      0;\nwriteFormat     ascii;\nwritePrecision  8;\ntimeFormat      general;\ntimePrecision   6;\nrunTimeModifiable true;\n",
                    header("dictionary", "system", "controlDict")
                ),
            },
            File {
                path: "system/fvSchemes",
                text: format!(
                    "{}ddtSchemes\n{{\n    default         steadyState;\n}}\n\ngradSchemes\n{{\n    default         Gauss linear;\n    limited         cellLimited Gauss linear 1;\n}}\n\ndivSchemes\n{{\n    default         none;\n    div(phi,U)      bounded Gauss linearUpwind limited;\n{schemes}    div((nuEff*dev2(T(grad(U))))) Gauss linear;\n}}\n\nlaplacianSchemes\n{{\n    default         Gauss linear corrected;\n}}\n\ninterpolationSchemes\n{{\n    default         linear;\n}}\n\nsnGradSchemes\n{{\n    default         corrected;\n}}\n\nwallDist\n{{\n    method          meshWave;\n}}\n",
                    header("dictionary", "system", "fvSchemes")
                ),
            },
            File {
                path: "system/fvSolution",
                text: format!(
                    "{}solvers\n{{\n    p\n    {{\n        solver          PCG;\n        preconditioner  DIC;\n        tolerance       1e-08;\n        relTol          0.01;\n    }}\n\n    U\n    {{\n        solver          smoothSolver;\n        smoother        symGaussSeidel;\n        tolerance       1e-09;\n        relTol          0.05;\n    }}\n\n{extra_solvers}}}\n\nSIMPLE\n{{\n    nNonOrthogonalCorrectors 2;\n    residualControl\n    {{\n        p               1e-05;\n        U               1e-06;\n{residuals}    }}\n}}\n\nrelaxationFactors\n{{\n    fields\n    {{\n        p               0.2;\n    }}\n    equations\n    {{\n        U               0.5;\n{relaxation}    }}\n}}\n",
                    header("dictionary", "system", "fvSolution")
                ),
            },
            File {
                path: "constant/transportProperties",
                text: format!(
                    "{}transportModel Newtonian;\n\nnu              {nu};\n",
                    header("dictionary", "constant", "transportProperties")
                ),
            },
            File {
                path: "constant/turbulenceProperties",
                text: format!(
                    "{}simulationType RAS;\n\nRAS\n{{\n    RASModel        {model};\n    turbulence      on;\n    printCoeffs     on;\n}}\n",
                    header("dictionary", "constant", "turbulenceProperties")
                ),
            },
        ]);
        files
    }
}

/// The expansion ratio (last cell over first) of `n` cells that start at `first` and cover
/// `length`; 1 when uniform cells of that size already reach it.
fn expansion_for(first: f64, length: f64, n: usize) -> f64 {
    let n = n as f64;
    if first * n >= length {
        return 1.0;
    }
    // The geometric series `first (rⁿ − 1)/(r − 1) = length`, by bisection on r > 1.
    let sum = |r: f64| first * (r.powf(n) - 1.0) / (r - 1.0);
    let (mut lo, mut hi) = (1.0 + 1.0e-9, 2.0);
    while sum(hi) < length && hi < 1.0e3 {
        hi *= 2.0;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if sum(mid) < length {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi)).powf(n - 1.0)
}
