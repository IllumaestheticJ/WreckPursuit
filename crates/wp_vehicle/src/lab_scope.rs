//! The shipped handling and the lab overrides on top of it ([`crate::plain`],
//! [`crate::ride_lab`], [`crate::car_lab`], [`crate::grip_lab`], the [`crate::drift`] tuning).
//!
//! **Unify (lead, 2026-10-21):** Drift Lab W's step-23 handling is the game's handling. With
//! no lab override set, every vehicle gets:
//!
//! - the spin-model tyres ([`crate::car_lab::CarTune::shipped`]),
//! - the firm ride ([`crate::ride_lab::RideTune::firm`]),
//! - the step-20+ grip balance ([`crate::grip_lab::GripBalance::shipped`]).
//!
//! AI racers, police and traffic get these too. The owner rule is one physics for every car.
//! The per-car fixes now live in `data/cars`.
//!
//! The plain keyboard steering ([`shipped_plain`]: W's step-13 defaults with the new-side
//! catch gain at 0.7) is a **player input**
//! aid. With it come the smart lock, the countersteer limit, the catch, the held-key release
//! and the transition rack. It applies only to vehicles flagged as player-driven
//! ([`crate::vehicle::Vehicle::set_player_input`]; `wp_sim` clears the flag for AI racers,
//! police and traffic). AI keep their own steering.
//!
//! The lab overrides (the hidden tuning panel, the measurement examples) replace a default
//! process-wide. [`set_legacy`]`(true)` brings back the pre-unify stock path (no plain
//! steering, data tyres and ride) for A/B comparisons. Neither is in replays, snapshots or
//! netcode. With no override set, the defaults are constants, so the sim stays deterministic.

use std::sync::atomic::{AtomicBool, Ordering};

static LEGACY: AtomicBool = AtomicBool::new(false);
static PLAIN_OFF: AtomicBool = AtomicBool::new(false);

/// Use the pre-unify stock handling where no override is set (`true`), or the shipped
/// handling (`false`, the default). Lab A/B and the legacy tests only.
pub fn set_legacy(on: bool) {
    LEGACY.store(on, Ordering::Relaxed);
}

/// Whether the pre-unify stock handling is in effect ([`set_legacy`]).
pub fn legacy() -> bool {
    LEGACY.load(Ordering::Relaxed)
}

/// Turn the plain steering off for player cars too (`true`): the normal steering model, as
/// for AI. Lab A/B only; default `false`.
pub fn set_plain_off(on: bool) {
    PLAIN_OFF.store(on, Ordering::Relaxed);
}

/// Whether the plain steering is turned off ([`set_plain_off`]).
pub fn plain_off() -> bool {
    PLAIN_OFF.load(Ordering::Relaxed)
}

/// The shipped plain steering: [`crate::plain::PlainSteer::default`] (Drift Lab W step 13)
/// with the new-side catch gain 0.7 instead of 0.9. At 0.9 rampart_50 (an owner reference
/// car) swings back on every flick (0 / 9 caught on tyre-slip's transition harness); at 0.7 the
/// parity cars, rampart_50 and debug_coupe catch 9 / 9 (docs/integration/grip-balance.md,
/// Unify).
pub fn shipped_plain() -> crate::plain::PlainSteer {
    crate::plain::PlainSteer {
        trans_catch_gain: 0.7,
        ..crate::plain::PlainSteer::default()
    }
}

/// The plain steering for a vehicle whose player-input flag is `player`: the lab override,
/// else [`shipped_plain`]. `None` for AI-driven vehicles,
/// with [`set_plain_off`], or in [`legacy`] mode without an override.
pub fn plain(player: bool) -> Option<crate::plain::PlainSteer> {
    if !player || plain_off() {
        return None;
    }
    crate::plain::active().or_else(|| (!legacy()).then(shipped_plain))
}

/// The ride tuning (every vehicle): the lab override, else the shipped
/// [`crate::ride_lab::RideTune::firm`] (`None` = the data as is, [`legacy`] only).
pub fn ride(_player: bool) -> Option<crate::ride_lab::RideTune> {
    crate::ride_lab::active().or_else(|| (!legacy()).then(crate::ride_lab::RideTune::firm))
}

/// The car / tyre tuning (every vehicle): the lab override, else the shipped
/// [`crate::car_lab::CarTune::shipped`] (`None` = the data as is, [`legacy`] only).
pub fn car(_player: bool) -> Option<crate::car_lab::CarTune> {
    crate::car_lab::active().or_else(|| (!legacy()).then(crate::car_lab::CarTune::shipped))
}

/// The grip balance (every vehicle): the lab override, else the shipped
/// [`crate::grip_lab::GripBalance::shipped`] (`None` = the data as is, [`legacy`] only). Its
/// steering parts (smart lock, slide lock) only act through the plain steering, so only on
/// player cars.
pub fn grip(_player: bool) -> Option<crate::grip_lab::GripBalance> {
    crate::grip_lab::active().or_else(|| (!legacy()).then(crate::grip_lab::GripBalance::shipped))
}

/// The drift tuning ([`crate::drift::tuning`]): the override, else the defaults. Every
/// vehicle.
pub fn drift(_player: bool) -> crate::drift::DriftTuning {
    crate::drift::tuning()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_defaults_and_player_only_steering() {
        // (Process-wide; the lib's other unit tests never turn legacy or plain-off on.)
        assert!(plain(true).is_some() && plain(false).is_none());
        assert_eq!(ride(false), Some(crate::ride_lab::RideTune::firm()));
        assert_eq!(car(false), Some(crate::car_lab::CarTune::shipped()));
        assert_eq!(grip(false), Some(crate::grip_lab::GripBalance::shipped()));
    }
}
