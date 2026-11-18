//! Sketches to start from, each already dimensioned.

use crate::model::{Constraint, Sketch};

/// A straight duct: a wall of one height from the inlet to the outlet.
pub fn duct(length: f64, radius: f64) -> Sketch {
    let mut s = Sketch::default();
    let axis = s.fixed_point(0.0, 0.0);
    let inlet = s.fixed_point(0.0, radius);
    let outlet = s.point(length, radius);
    s.line(inlet, outlet);
    s.constrain(Constraint::DeltaX {
        a: inlet,
        b: outlet,
        value: length,
    });
    s.constrain(Constraint::DeltaY {
        a: axis,
        b: outlet,
        value: radius,
    });
    s
}

/// A converging-diverging nozzle of straight walls: an inlet, a throat and an outlet, with a lead
/// radius at each and the length of each cone.
pub fn nozzle(
    inlet_radius: f64,
    throat_radius: f64,
    outlet_radius: f64,
    converging_length: f64,
    diverging_length: f64,
) -> Sketch {
    let mut s = Sketch::default();
    let axis = s.fixed_point(0.0, 0.0);
    let inlet = s.fixed_point(0.0, inlet_radius);
    let throat = s.point(converging_length, throat_radius);
    let outlet = s.point(converging_length + diverging_length, outlet_radius);
    s.line(inlet, throat);
    s.line(throat, outlet);
    s.constrain(Constraint::DeltaX {
        a: inlet,
        b: throat,
        value: converging_length,
    });
    s.constrain(Constraint::DeltaX {
        a: throat,
        b: outlet,
        value: diverging_length,
    });
    s.constrain(Constraint::DeltaY {
        a: axis,
        b: throat,
        value: throat_radius,
    });
    s.constrain(Constraint::DeltaY {
        a: axis,
        b: outlet,
        value: outlet_radius,
    });
    s
}

/// A nozzle whose throat is an arc of the given radius, between a converging and a diverging line.
/// The radius is a dimension of the arc; the walls meet it where the dimensions put them.
pub fn rounded_nozzle(
    inlet_radius: f64,
    throat_radius: f64,
    outlet_radius: f64,
    converging_length: f64,
    diverging_length: f64,
    throat_arc_radius: f64,
) -> Sketch {
    let mut s = Sketch::default();
    let axis = s.fixed_point(0.0, 0.0);
    let inlet = s.fixed_point(0.0, inlet_radius);
    // The arc's two ends sit either side of the throat, its centre above it so the arc bulges
    // towards the axis, counter-clockwise from the converging side to the diverging side.
    let half = (throat_arc_radius * 0.3).min(converging_length.min(diverging_length) * 0.4);
    let sweep = half / throat_arc_radius;
    let drop = throat_arc_radius * (1.0 - sweep.cos());
    let cx = converging_length;
    let cy = throat_radius + throat_arc_radius;
    let from = s.point(cx - half, throat_radius + drop);
    let to = s.point(cx + half, throat_radius + drop);
    let centre = s.point(cx, cy);
    let outlet = s.point(converging_length + diverging_length, outlet_radius);
    s.line(inlet, from);
    s.arc(from, to, centre);
    s.line(to, outlet);
    s.constrain(Constraint::Radius {
        arc: 1,
        value: throat_arc_radius,
    });
    s.constrain(Constraint::DeltaX {
        a: inlet,
        b: centre,
        value: converging_length,
    });
    s.constrain(Constraint::DeltaX {
        a: centre,
        b: outlet,
        value: diverging_length,
    });
    s.constrain(Constraint::DeltaY {
        a: axis,
        b: outlet,
        value: outlet_radius,
    });
    s.constrain(Constraint::DeltaY {
        a: axis,
        b: centre,
        value: throat_radius + throat_arc_radius,
    });
    s
}
