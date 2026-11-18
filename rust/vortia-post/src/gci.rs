//! The grid convergence index of three meshes (Roache; Celik et al., 2008, procedure for the
//! estimation of discretisation error), for one number taken from each mesh. It follows
//! `benchmarks/common/gci.py`, which stays the reference for the cases the checks refuse.

use crate::error::{PostError, Result};

/// What three meshes say about a number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gci {
    /// The order of convergence the three values show.
    pub observed_order: f64,
    /// The value Richardson extrapolation gives for a mesh of no size.
    pub extrapolated: f64,
    /// The index of the fine pair, the uncertainty of the fine mesh's value.
    pub fine: f64,
    /// The index of the coarse pair.
    pub coarse: f64,
    /// Whether the indices are relative to the values (they are absolute when a value is zero).
    pub relative: bool,
    /// `coarse / (r21^p fine)`: near 1 when the meshes are in the asymptotic range.
    pub asymptotic_ratio: f64,
}

fn invalid<T>(text: &str) -> Result<T> {
    Err(PostError::Invalid(text.to_owned()))
}

/// The index of the values `f1` (fine), `f2` and `f3` (coarse), with the refinement ratios
/// `r21 = h2/h1` and `r32 = h3/h2` and the safety factor `safety` (1.25 for three meshes).
///
/// It refuses what has no order to estimate: values that are not finite, equal ones that differ
/// nowhere, an oscillating sequence, or one whose change shrinks faster than the ratios allow.
pub fn gci(f1: f64, f2: f64, f3: f64, r21: f64, r32: f64, safety: f64) -> Result<Gci> {
    if ![f1, f2, f3, r21, r32, safety].iter().all(|v| v.is_finite()) {
        return invalid("all inputs must be finite");
    }
    if r21 <= 1.0 || r32 <= 1.0 || safety <= 0.0 {
        return invalid("the refinement ratios must be above 1 and the safety factor positive");
    }
    let (e21, e32) = (f2 - f1, f3 - f2);
    if e21 == 0.0 && e32 == 0.0 && f1 == f2 {
        return Ok(Gci {
            observed_order: 0.0,
            extrapolated: f1,
            fine: 0.0,
            coarse: 0.0,
            relative: true,
            asymptotic_ratio: 1.0,
        });
    }
    if e21 == 0.0 || e32 == 0.0 {
        return invalid("two meshes give the same value; the order cannot be estimated");
    }
    if (e21 > 0.0) != (e32 > 0.0) {
        return invalid("the values oscillate with the mesh; the index does not apply");
    }
    let ratio = (e32 / e21).abs();
    let equal = (r21 - r32).abs() <= 1e-12;
    if ratio <= r32.ln() / r21.ln() {
        return invalid(if equal {
            "an equal-ratio study needs the coarse change above the fine one (a positive order)"
        } else {
            "an unequal-ratio study needs a positive order"
        });
    }
    let order = if equal {
        ratio.ln() / r21.ln()
    } else {
        let mut p = ratio.ln().abs() / r21.ln();
        let mut settled = false;
        for _ in 0..50 {
            let (num, den) = (r21.powf(p) - 1.0, r32.powf(p) - 1.0);
            if !(num > 0.0 && den > 0.0 && num.is_finite() && den.is_finite()) {
                return invalid("the order of convergence left its valid range");
            }
            let next = (ratio.ln() + (num / den).ln()).abs() / r21.ln();
            if !next.is_finite() || next <= 0.0 {
                return invalid("the order of convergence is not positive");
            }
            let done = (next - p).abs() < 1e-10 * next.abs().max(1.0);
            p = next;
            if done {
                settled = true;
                break;
            }
        }
        if !settled {
            return invalid("the order of convergence did not settle");
        }
        p
    };
    if !order.is_finite() || order <= 0.0 {
        return invalid("the order of convergence is not positive");
    }
    let (rp21, rp32) = (r21.powf(order), r32.powf(order));
    let extrapolated = (rp21 * f1 - f2) / (rp21 - 1.0);
    let (a12, a23) = (
        safety * e21.abs() / (rp21 - 1.0),
        safety * e32.abs() / (rp32 - 1.0),
    );
    let relative = f1 != 0.0 && f2 != 0.0;
    let (fine, coarse) = if relative {
        (a12 / f1.abs(), a23 / f2.abs())
    } else {
        (a12, a23)
    };
    if fine <= 0.0 {
        return invalid("the fine index is zero; the asymptotic ratio is undefined");
    }
    Ok(Gci {
        observed_order: order,
        extrapolated,
        fine,
        coarse,
        relative,
        asymptotic_ratio: coarse / (rp21 * fine),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1.0)
    }

    /// The numbers `benchmarks/common/gci.py` gives for the same inputs.
    #[test]
    fn it_agrees_with_the_reference_tool() {
        let cases = [
            (
                (0.97, 0.95, 0.9, 2.0, 2.0),
                (
                    1.3219280948873593,
                    0.9833333333333335,
                    0.017182130584192514,
                    0.04385964912280712,
                    1.0210526315789474,
                ),
            ),
            (
                (1.0004, 1.002, 1.01, 2.0, 2.0),
                (
                    2.3219280948873227,
                    1.0,
                    0.0004998000799680443,
                    0.002495009980040008,
                    0.9984031936127741,
                ),
            ),
            (
                (4.565, 4.5, 4.3, 1.5, 1.5),
                (
                    2.7719526888428776,
                    4.596296296296297,
                    0.008569632063608085,
                    0.02674897119341587,
                    1.0144444444444447,
                ),
            ),
            (
                (2.0, 2.1, 2.5, 2.0, 1.6),
                (
                    3.235442213542381,
                    1.9881208917295174,
                    0.007424442669051521,
                    0.0665947073063312,
                    0.9523809524163127,
                ),
            ),
        ];
        for ((f1, f2, f3, r21, r32), (p, star, g12, g23, asym)) in cases {
            let g = gci(f1, f2, f3, r21, r32, 1.25).unwrap();
            assert!(
                close(g.observed_order, p),
                "{f1}: order {}",
                g.observed_order
            );
            assert!(
                close(g.extrapolated, star),
                "{f1}: extrapolated {}",
                g.extrapolated
            );
            assert!(close(g.fine, g12) && close(g.coarse, g23), "{f1}: {g:?}");
            assert!(close(g.asymptotic_ratio, asym), "{f1}: {g:?}");
            assert!(g.relative);
        }
    }

    #[test]
    fn identical_values_have_no_uncertainty() {
        let g = gci(2.0, 2.0, 2.0, 2.0, 2.0, 1.25).unwrap();
        assert_eq!((g.fine, g.coarse, g.extrapolated), (0.0, 0.0, 2.0));
    }

    #[test]
    fn what_has_no_order_is_refused() {
        let refused = |f1, f2, f3, r21, r32| gci(f1, f2, f3, r21, r32, 1.25).is_err();
        // Not finite, ratios of 1 or below, a zero safety factor.
        assert!(refused(f64::NAN, 1.0, 2.0, 2.0, 2.0));
        assert!(refused(1.0, 1.1, 1.2, 1.0, 2.0));
        assert!(gci(1.0, 1.1, 1.3, 2.0, 2.0, 0.0).is_err());
        // Two meshes with the same value, an oscillation, a change that does not grow.
        assert!(refused(1.0, 1.0, 1.2, 2.0, 2.0));
        assert!(refused(1.0, 1.1, 1.0, 2.0, 2.0));
        assert!(refused(1.0, 1.2, 1.3, 2.0, 2.0));
    }

    #[test]
    fn a_value_of_zero_gives_absolute_indices() {
        let g = gci(0.0, 0.01, 0.05, 2.0, 2.0, 1.25).unwrap();
        assert!(!g.relative);
        assert!(g.fine > 0.0 && g.fine.is_finite());
    }
}
