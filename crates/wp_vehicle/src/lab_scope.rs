//! Which vehicles the Drift Lab W overrides apply to ([`crate::plain`], [`crate::ride_lab`],
//! [`crate::car_lab`], the [`crate::drift`] tuning override).
//!
//! By default an override applies to every vehicle in the process (the lab pad has one car;
//! the unit tests drive one rig). The testbed's "use W handling everywhere" toggle (lead,
//! 2026-10-20) keeps the W profile on the **player's** car in free drive, city chases and
//! races, so it switches to [`set_tagged_only`]`(true)` and tags the player's vehicle
//! ([`crate::vehicle::Vehicle::set_lab_tagged`]): AI racers, police and traffic keep the stock
//! handling.
//!
//! Lab-only, like the overrides themselves: not in the car data, replays, snapshots or
//! netcode. With the overrides off, nothing here changes a step.

use std::sync::atomic::{AtomicBool, Ordering};

static TAGGED_ONLY: AtomicBool = AtomicBool::new(false);

/// Limit the lab overrides to tagged vehicles (`true`) or apply them to all (`false`, the
/// default).
pub fn set_tagged_only(on: bool) {
    TAGGED_ONLY.store(on, Ordering::Relaxed);
}

/// Whether the lab overrides apply only to tagged vehicles.
pub fn tagged_only() -> bool {
    TAGGED_ONLY.load(Ordering::Relaxed)
}

/// Whether the lab overrides apply to a vehicle whose tag is `tagged`.
pub fn applies(tagged: bool) -> bool {
    tagged || !tagged_only()
}

/// The plain steering override ([`crate::plain::active`]) for a vehicle tagged `tagged`.
pub fn plain(tagged: bool) -> Option<crate::plain::PlainSteer> {
    if applies(tagged) {
        crate::plain::active()
    } else {
        None
    }
}

/// The ride override ([`crate::ride_lab::active`]) for a vehicle tagged `tagged`.
pub fn ride(tagged: bool) -> Option<crate::ride_lab::RideTune> {
    if applies(tagged) {
        crate::ride_lab::active()
    } else {
        None
    }
}

/// The car / tyre override ([`crate::car_lab::active`]) for a vehicle tagged `tagged`.
pub fn car(tagged: bool) -> Option<crate::car_lab::CarTune> {
    if applies(tagged) {
        crate::car_lab::active()
    } else {
        None
    }
}

/// The drift tuning ([`crate::drift::tuning`]) for a vehicle tagged `tagged`: the shipped
/// defaults for a vehicle the override does not apply to.
pub fn drift(tagged: bool) -> crate::drift::DriftTuning {
    if applies(tagged) {
        crate::drift::tuning()
    } else {
        crate::drift::DriftTuning::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untagged_vehicles_only_drop_out_in_tagged_mode() {
        // (Process-wide; the lib's other unit tests never turn tagged-only on.)
        assert!(applies(false) && applies(true));
        set_tagged_only(true);
        let (untagged, tagged) = (applies(false), applies(true));
        set_tagged_only(false);
        assert!(!untagged && tagged);
        assert!(applies(false));
    }
}
