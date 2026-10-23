//! The free per-car tune as garage state: four 5-step sliders (suspension, gearing,
//! differential, brake balance), 3 = stock. Setting it costs nothing and writes no ledger
//! line ([`crate::Economy::set_tune`]); the career save keeps it on each [`crate::OwnedCar`].
//!
//! The economy only stores and validates it. What the steps do to the car is
//! `wp_vehicle::tune` (applied at spawn). This type has the same serde shape as
//! `wp_vehicle::CarTune`; both become the contract's `CarTune` when the integration lead
//! applies request WP-4 (`docs/integration/tune.md`), so saves written now keep loading.

use serde::{Deserialize, Serialize};

/// Number of steps on each slider.
pub const TUNE_STEPS: u8 = 5;

/// Stock position of every slider.
pub const TUNE_STOCK: u8 = 3;

/// One car's tune: each slider `1..=5`, 3 = stock. Missing fields read as stock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarTune {
    /// Suspension, 1 soft … 5 stiff.
    #[serde(default = "tune_stock")]
    pub suspension: u8,
    /// Gear ratio, 1 short … 5 long.
    #[serde(default = "tune_stock")]
    pub gearing: u8,
    /// Differential, 1 open … 3 stock … 5 locked.
    #[serde(default = "tune_stock")]
    pub diff: u8,
    /// Brake balance, 1 rear … 5 front.
    #[serde(default = "tune_stock")]
    pub brake_balance: u8,
}

fn tune_stock() -> u8 {
    TUNE_STOCK
}

impl CarTune {
    /// Every slider at stock.
    pub const STOCK: Self = Self {
        suspension: TUNE_STOCK,
        gearing: TUNE_STOCK,
        diff: TUNE_STOCK,
        brake_balance: TUNE_STOCK,
    };

    /// True when every slider is at stock.
    pub fn is_stock(&self) -> bool {
        *self == Self::STOCK
    }

    /// True when every slider is in `1..=TUNE_STEPS`.
    pub fn is_valid(&self) -> bool {
        [self.suspension, self.gearing, self.diff, self.brake_balance]
            .iter()
            .all(|v| (1..=TUNE_STEPS).contains(v))
    }
}

impl Default for CarTune {
    fn default() -> Self {
        Self::STOCK
    }
}
