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

/// A channel with a square block in it, as a closed region with named edges: `inlet` on the left,
/// `outlet` on the right, `floor` and `ceiling`, and the block's four edges named `block`. The
/// channel's lower left corner is fixed at the origin; the block sits `block_x` from the inlet and
/// `block_y` above the floor.
pub fn channel_with_block(
    length: f64,
    height: f64,
    block: f64,
    block_x: f64,
    block_y: f64,
) -> Sketch {
    let mut s = Sketch::default();
    let p0 = s.fixed_point(0.0, 0.0);
    let p1 = s.point(length, 0.0);
    let p2 = s.point(length, height);
    let p3 = s.point(0.0, height);
    let edges = [
        (p0, p1, "floor"),
        (p1, p2, "outlet"),
        (p2, p3, "ceiling"),
        (p3, p0, "inlet"),
    ];
    for (a, b, name) in edges {
        let e = s.line(a, b);
        s.name(e, name);
    }
    s.constrain(Constraint::Horizontal { a: p0, b: p1 });
    s.constrain(Constraint::Horizontal { a: p3, b: p2 });
    s.constrain(Constraint::Vertical { a: p0, b: p3 });
    s.constrain(Constraint::Vertical { a: p1, b: p2 });
    s.constrain(Constraint::DeltaX {
        a: p0,
        b: p1,
        value: length,
    });
    s.constrain(Constraint::DeltaY {
        a: p0,
        b: p3,
        value: height,
    });

    let q0 = s.point(block_x, block_y);
    let q1 = s.point(block_x + block, block_y);
    let q2 = s.point(block_x + block, block_y + block);
    let q3 = s.point(block_x, block_y + block);
    for (a, b) in [(q0, q1), (q1, q2), (q2, q3), (q3, q0)] {
        let e = s.line(a, b);
        s.name(e, "block");
    }
    s.constrain(Constraint::Horizontal { a: q0, b: q1 });
    s.constrain(Constraint::Horizontal { a: q3, b: q2 });
    s.constrain(Constraint::Vertical { a: q0, b: q3 });
    s.constrain(Constraint::Vertical { a: q1, b: q2 });
    s.constrain(Constraint::DeltaX {
        a: q0,
        b: q1,
        value: block,
    });
    s.constrain(Constraint::DeltaY {
        a: q0,
        b: q3,
        value: block,
    });
    s.constrain(Constraint::DeltaX {
        a: p0,
        b: q0,
        value: block_x,
    });
    s.constrain(Constraint::DeltaY {
        a: p0,
        b: q0,
        value: block_y,
    });
    s
}
