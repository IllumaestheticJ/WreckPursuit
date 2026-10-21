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
//! The plain keyboard steering ([`shipped_plain`]: W's step-13b defaults) is a **player
//! input**
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

/// The shipped plain steering: [`crate::plain::PlainSteer::default`] (Drift Lab W step 13b:
/// the new-side catch armed by the slide crossing, so rampart_50 no longer over-catches at
/// gain 0.9).
pub fn shipped_plain() -> crate::plain::PlainSteer {
    crate::plain::PlainSteer::default()
}

/// The plain steering for a vehicle whose player-input flag is `player`
/// ([`crate::vehicle::Vehicle::set_player_input`]):
/// - `Some(true)` (a player's car in `wp_sim`): the lab override, else [`shipped_plain`];
/// - `Some(false)` (AI, police, traffic, harness drivers): `None`;
/// - `None` (a bare vehicle / rig no sim manages: tests, lab tools): only the lab override, so
///   physics tests with analog inputs keep the normal steering model.
///
/// Always `None` with [`set_plain_off`]; [`legacy`] drops the shipped fallback.
pub fn plain(player: Option<bool>) -> Option<crate::plain::PlainSteer> {
    if plain_off() {
        return None;
    }
    match player {
        Some(false) => None,
        Some(true) => crate::plain::active().or_else(|| (!legacy()).then(shipped_plain)),
        None => crate::plain::active(),
    }
}

/// The ride tuning (every vehicle): the lab override, else the shipped
/// [`crate::ride_lab::RideTune::firm`] (`None` = the data as is, [`legacy`] only).
pub fn ride() -> Option<crate::ride_lab::RideTune> {
    crate::ride_lab::active().or_else(|| (!legacy()).then(crate::ride_lab::RideTune::firm))
}

/// The car / tyre tuning (every vehicle): the lab override, else the shipped
/// [`crate::car_lab::CarTune::shipped`] (`None` = the data as is, [`legacy`] only).
pub fn car() -> Option<crate::car_lab::CarTune> {
    crate::car_lab::active().or_else(|| (!legacy()).then(crate::car_lab::CarTune::shipped))
}

/// The grip balance (every vehicle): the lab override, else the shipped
/// [`crate::grip_lab::GripBalance::shipped`] (`None` = the data as is, [`legacy`] only). Its
/// steering parts (smart lock, slide lock) only act through the plain steering, so only on
/// player cars.
pub fn grip() -> Option<crate::grip_lab::GripBalance> {
    crate::grip_lab::active().or_else(|| (!legacy()).then(crate::grip_lab::GripBalance::shipped))
}

/// The drift tuning ([`crate::drift::tuning`]): the override, else the defaults. Every
/// vehicle.
pub fn drift() -> crate::drift::DriftTuning {
    crate::drift::tuning()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_defaults_and_player_only_steering() {
        // (Process-wide; the lib's other unit tests never turn legacy or plain-off on.)
        assert!(plain(Some(true)).is_some() && plain(Some(false)).is_none());
        assert!(plain(None).is_none(), "a bare rig: only a lab override");
        assert_eq!(ride(), Some(crate::ride_lab::RideTune::firm()));
        assert_eq!(car(), Some(crate::car_lab::CarTune::shipped()));
        assert_eq!(grip(), Some(crate::grip_lab::GripBalance::shipped()));
    }
}
