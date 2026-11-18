//! Mass balance: what goes in through the patches against what goes out, read
//! from the flux `phi` a solver writes with its fields.
//!
//! The same check the Qt application makes on its nozzle runs: at every time
//! written, the net flux through all the patches as a percentage of the larger
//! of the inflow and the outflow, and its mean and worst over the last times.

use vortia_mesh::PatchKind;

use crate::case::Case;
use crate::error::{PostError, Result};

/// A balance whose scale is below this has nothing flowing: its percentage is 0.
const NO_FLOW: f64 = 1.0e-300;

/// The flux through each patch at one time.
#[derive(Debug, Clone, PartialEq)]
pub struct MassBalance {
    pub time: String,
    /// `(patch, flux out of the domain)`, for the patches that carry flux (not
    /// `empty` or `wedge`).
    pub patches: Vec<(String, f64)>,
}

impl MassBalance {
    /// What comes in, as a positive number.
    pub fn inflow(&self) -> f64 {
        -self.patches.iter().map(|p| p.1.min(0.0)).sum::<f64>()
    }

    /// What goes out.
    pub fn outflow(&self) -> f64 {
        self.patches.iter().map(|p| p.1.max(0.0)).sum()
    }

    /// Out minus in: positive when more leaves than enters.
    pub fn net(&self) -> f64 {
        self.patches.iter().map(|p| p.1).sum()
    }

    /// The net flux as a percentage of the larger of the inflow and the outflow.
    pub fn percent(&self) -> f64 {
        let scale = self.inflow().max(self.outflow());
        if scale > NO_FLOW {
            100.0 * self.net() / scale
        } else {
            0.0
        }
    }
}

/// The balance of the last times written.
#[derive(Debug, Clone, PartialEq)]
pub struct BalanceHistory {
    /// The first and last time of the window.
    pub first: String,
    pub last: String,
    /// How many times it covers.
    pub samples: usize,
    pub mean_abs_percent: f64,
    pub max_abs_percent: f64,
    /// The balance of the last time, patch by patch.
    pub latest: MassBalance,
}

impl Case {
    /// The balance at `time`.
    pub fn mass_balance(&self, time: &str) -> Result<MassBalance> {
        let phi = self.flux_field(time)?;
        let mut patches = Vec::new();
        for patch in self.mesh.patches() {
            if matches!(patch.kind, PatchKind::Empty | PatchKind::Wedge) {
                continue;
            }
            patches.push((patch.name.clone(), Self::patch_flux(&phi, patch)?));
        }
        Ok(MassBalance {
            time: time.to_owned(),
            patches,
        })
    }

    /// The balance over the last `window` times that hold a flux (time 0 has
    /// none).
    pub fn mass_balance_history(&self, window: usize) -> Result<BalanceHistory> {
        if window == 0 {
            return Err(PostError::Invalid("the window holds no times".into()));
        }
        let mut balances: Vec<MassBalance> = Vec::new();
        for time in self.times()?.iter().rev() {
            if balances.len() == window {
                break;
            }
            if time.value <= 0.0 {
                continue;
            }
            if let Ok(balance) = self.mass_balance(&time.name) {
                balances.push(balance);
            }
        }
        balances.reverse();
        let (Some(first), Some(latest)) = (balances.first(), balances.last()) else {
            return Err(PostError::Invalid(
                "no time written holds a flux (phi): run the case first".into(),
            ));
        };
        let percents: Vec<f64> = balances.iter().map(|b| b.percent().abs()).collect();
        Ok(BalanceHistory {
            first: first.time.clone(),
            last: latest.time.clone(),
            samples: balances.len(),
            mean_abs_percent: percents.iter().sum::<f64>() / percents.len() as f64,
            max_abs_percent: percents.iter().copied().fold(0.0, f64::max),
            latest: latest.clone(),
        })
    }
}
