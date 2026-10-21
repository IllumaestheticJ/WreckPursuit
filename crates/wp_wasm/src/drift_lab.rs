//! Drift Lab B / C (lead helpers `lead/drift-lab-b`, `lead/drift-lab-c`; owner rows 90 / 91 /
//! 104 / 106): the testbed's "tune" panel and readout for the drift assist in
//! `wp_vehicle::drift`.
//!
//! | function | what |
//! |---|---|
//! | `drift_tuning_json()` | the constants in effect (override or defaults), JSON |
//! | `drift_tuning_defaults_json()` | the shipped defaults, JSON |
//! | `drift_tuning_set(json)` | set the runtime override (`""` clears it); unknown fields are ignored, missing ones take the default |
//! | `lab_scope_set_tagged_only(on)` / `set_lab_tagged(entity, on)` | "use W handling everywhere": the lab overrides on the tagged (player) car only |
//! | `drift_readout(entity)` | `[mode, slide_deg, onset, envelope_deg, authority, throttle_scale, key_throttle, engine_throttle, held_deg, sliding]`, mode 0 off · 1 grip · 2 straighten · 3 hold (countersteer key) · 4 angle-up (key into the turn) · 5 exceeded · 6 transition; `key_throttle` = the driver's throttle input, `engine_throttle` = what reaches the engine (after shaping, pedal filter, TCS / ESC), `held_deg` = the slide angle Hold / AngleUp keeps, `sliding` 1 = a slide is confirmed (the steering's slide help is on, row 102) |
//!
//! The override is process-wide and not logged in recordings (a developer tool; see
//! `wp_vehicle::drift`).

use wasm_bindgen::prelude::*;
use wp_contracts::{EntityId, VehicleSim};
use wp_vehicle::drift::{self, DriftTuning};

use crate::{err, json, with};

/// The drift-assist constants in effect, JSON.
#[wasm_bindgen]
pub fn drift_tuning_json() -> String {
    json(&drift::tuning())
}

/// The shipped drift-assist constants, JSON.
#[wasm_bindgen]
pub fn drift_tuning_defaults_json() -> String {
    json(&DriftTuning::default())
}

/// Set the runtime override from JSON (`""` = back to the defaults).
#[wasm_bindgen]
pub fn drift_tuning_set(tuning_json: &str) -> Result<(), JsValue> {
    if tuning_json.trim().is_empty() {
        drift::set_tuning_override(None);
        return Ok(());
    }
    let t: DriftTuning = serde_json::from_str(tuning_json).map_err(err)?;
    drift::set_tuning_override(Some(t));
    Ok(())
}

/// Drift readout of a vehicle this tick: `[mode, slide_deg, onset, envelope_deg, authority,
/// throttle_scale, key_throttle, engine_throttle, held_deg, sliding]` (see the module docs).
#[wasm_bindgen]
pub fn drift_readout(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let v = s
            .vehicle(EntityId(entity))
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let tel = v.vehicle.telemetry();
        let d = tel.drift;
        Ok(vec![
            d.mode.code() as f32,
            d.slide_angle.to_degrees(),
            d.onset,
            d.envelope.to_degrees(),
            d.authority,
            d.throttle_scale,
            v.input.throttle.clamp(0.0, 1.0),
            tel.throttle,
            d.hold_angle.to_degrees(),
            if tel.sliding { 1.0 } else { 0.0 },
        ])
    })
}

// --- Drift Lab W (lead/drift-lab-w): plain keyboard steering, reference-parity cars -------
//
// | function | what |
// |---|---|
// | `plain_steer_json()` | the plain steering constants in effect (`null` = plain off), JSON |
// | `plain_steer_defaults_json()` | the defaults (`wp_vehicle::plain::PlainSteer`), JSON |
// | `plain_steer_set(json)` | turn plain steering on with these constants (`""` = off); missing fields take the default |
// | `plain_readout(entity)` | `[steer_key, rack, lock_frac, slide_deg, throttle_key, engine_throttle, gear, speed_kmh, wheel_deg, heading_deg]` (see `plain_readout`) |
// | `parity_car_ids()` | JSON array of the parity test cars (not in `car_ids()`) |

/// The plain steering constants in effect (`null` when plain steering is off), JSON.
#[wasm_bindgen]
pub fn plain_steer_json() -> String {
    json(&wp_vehicle::plain::active())
}

/// The default plain steering constants, JSON.
#[wasm_bindgen]
pub fn plain_steer_defaults_json() -> String {
    json(&wp_vehicle::plain::PlainSteer::default())
}

/// Turn the plain keyboard steering profile on with the constants in `json` (missing fields
/// take the default), or off with `""`. Process-wide, not logged in recordings.
#[wasm_bindgen]
pub fn plain_steer_set(steer_json: &str) -> Result<(), JsValue> {
    if steer_json.trim().is_empty() {
        wp_vehicle::plain::set_override(None);
        return Ok(());
    }
    let p: wp_vehicle::plain::PlainSteer = serde_json::from_str(steer_json).map_err(err)?;
    wp_vehicle::plain::set_override(Some(p));
    Ok(())
}

/// Drift Lab W readout of a vehicle: `[steer_key (−1..=1, + = right), rack (−1..=1, + =
/// right; NaN when plain steering is off), lock_frac (0..=1; NaN when off), slide_deg (body
/// slip, + = moving toward the left of the heading), throttle_key, engine_throttle, gear,
/// speed_kmh, wheel_deg (centre road-wheel angle, + = left), heading_deg (yaw, + = left),
/// auto_clutch (1 / 0), clutch engagement (0..=1), clutch_locked (1 / 0), engine_rpm,
/// driven_wheel_rpm (mean of the driven wheels), clutch_pedal (DriverInput.clutch)]`.
#[wasm_bindgen]
pub fn plain_readout(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let v = s
            .vehicle(EntityId(entity))
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let tel = v.vehicle.telemetry();
        let ch = v.vehicle.chassis();
        let fwd = ch.rot * wp_vehicle::math::V3::Y;
        let right = ch.rot * wp_vehicle::math::V3::X;
        let v_fwd = ch.lin_vel.dot(fwd);
        let v_left = -ch.lin_vel.dot(right);
        let slide = if v_fwd.abs() < 1.0 {
            0.0
        } else {
            wp_vehicle::math::atan2(v_left, v_fwd.abs()).to_degrees()
        };
        let (rack, frac) = v.vehicle.plain_readout().unwrap_or((f32::NAN, f32::NAN));
        let st = v.vehicle.state();
        let driven: &[usize] = match v.vehicle.def().drivetrain.layout {
            wp_contracts::DriveLayout::Fwd => &[0, 1],
            wp_contracts::DriveLayout::Rwd => &[2, 3],
            _ => &[0, 1, 2, 3],
        };
        let wheel_rpm = driven
            .iter()
            .map(|&i| st.wheel_states[i].omega)
            .sum::<f32>()
            / driven.len() as f32
            * 60.0
            / std::f32::consts::TAU;
        let auto_clutch = s.assists(EntityId(entity)).is_some_and(|a| a.auto_clutch);
        Ok(vec![
            v.input.steer,
            rack,
            frac,
            slide,
            v.input.throttle.clamp(0.0, 1.0),
            tel.throttle,
            v.vehicle.gear() as f32,
            ch.lin_vel.length() * 3.6,
            tel.steer_rad.to_degrees(),
            wp_vehicle::math::atan2(-fwd.x, fwd.y).to_degrees(),
            if auto_clutch { 1.0 } else { 0.0 },
            tel.clutch,
            if tel.clutch_locked { 1.0 } else { 0.0 },
            st.engine_rpm,
            wheel_rpm,
            v.input.clutch.clamp(0.0, 1.0),
        ])
    })
}

/// JSON array of the Drift Lab W reference-parity test cars (`data/cars/parity/`), which are
/// not in `car_ids()`.
#[wasm_bindgen]
pub fn parity_car_ids() -> String {
    json(&crate::data::parity_car_ids())
}

// --- Drift Lab W ride tuning (wp_vehicle::ride_lab) --------------------------------------

/// The ride tuning override in effect (`null` = the car data as is), JSON.
#[wasm_bindgen]
pub fn ride_tune_json() -> String {
    json(&wp_vehicle::ride_lab::active())
}

/// `{ "data": <no scaling>, "lab": <step 3 default, the "before">, "slow_weight": <clip-4
/// preset>, "medium_weight": <step 10 ride>, "stiff_ride": <step 18 default, the "new"> }`,
/// JSON.
#[wasm_bindgen]
pub fn ride_tune_defaults_json() -> String {
    json(&serde_json::json!({
        "data": wp_vehicle::ride_lab::RideTune::default(),
        "lab": wp_vehicle::ride_lab::RideTune::lab_default(),
        "slow_weight": wp_vehicle::ride_lab::RideTune::slow_weight(),
        "medium_weight": wp_vehicle::ride_lab::RideTune::medium_weight(),
        "stiff_ride": wp_vehicle::ride_lab::RideTune::stiff_ride(),
    }))
}

/// Set the ride tuning override from JSON (missing fields = no scaling), or clear it with `""`.
/// Process-wide, not logged in recordings.
#[wasm_bindgen]
pub fn ride_tune_set(tune_json: &str) -> Result<(), JsValue> {
    if tune_json.trim().is_empty() {
        wp_vehicle::ride_lab::set_override(None);
        return Ok(());
    }
    let t: wp_vehicle::ride_lab::RideTune = serde_json::from_str(tune_json).map_err(err)?;
    wp_vehicle::ride_lab::set_override(Some(t));
    Ok(())
}

/// Roll / pitch model of a vehicle's car under the tuning in effect:
/// `[roll_hz, roll_zeta, roll_deg_per_g, pitch_hz, pitch_zeta, body_roll_deg]`
/// (`body_roll_deg` = the body's roll now, + = right side down).
#[wasm_bindgen]
pub fn ride_readout(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let v = s
            .vehicle(EntityId(entity))
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let t = wp_vehicle::ride_lab::active().unwrap_or_default();
        let m = wp_vehicle::ride_lab::ride_model(v.vehicle.def(), &t);
        let right = v.vehicle.chassis().rot * wp_vehicle::math::V3::X;
        let roll =
            -wp_vehicle::math::atan2(right.z, (right.x * right.x + right.y * right.y).sqrt());
        Ok(vec![
            m.roll_hz,
            m.roll_zeta,
            m.roll_deg_per_g,
            m.pitch_hz,
            m.pitch_zeta,
            roll.to_degrees(),
        ])
    })
}

// --- Drift Lab W car / tyre experiments (wp_vehicle::car_lab) -----------------------------

/// The car / tyre lab override in effect (`null` = the data), JSON.
#[wasm_bindgen]
pub fn car_tune_json() -> String {
    json(&wp_vehicle::car_lab::active())
}

/// `{ "data": <no change>, "spin_damper": <step-4 test set — does not hold>, "spin": <tyre-slip spin model>, "spin_lock": <+ drift lock / front falloff>, "spin_vr_plain": <PlainSteer of the "drift: spin + VR (test)" preset>, "transition_plain": <PlainSteer of the "transition: swing rack (test)" preset> }`, JSON.
#[wasm_bindgen]
pub fn car_tune_defaults_json() -> String {
    json(&serde_json::json!({
        "data": wp_vehicle::car_lab::CarTune::default(),
        "spin_damper": wp_vehicle::car_lab::CarTune::spin_grip_damper(),
        "spin": wp_vehicle::car_lab::CarTune::spin_candidate(),
        "spin_lock": wp_vehicle::car_lab::CarTune::spin_lock_candidate(),
        "spin_vr_plain": wp_vehicle::car_lab::CarTune::spin_vr_plain(),
        "transition_plain": wp_vehicle::car_lab::CarTune::transition_plain(),
    }))
}

/// The grip-balance lab override in effect (`null` = the data), JSON.
#[wasm_bindgen]
pub fn grip_balance_json() -> String {
    json(&wp_vehicle::grip_lab::active())
}

/// `{ "before": <the data as is>, "new": <the lab's new balance>, "step2": <step 2's set>,
/// "slow_weight": <the balance for the slow-weight ride preset>, "drift_best": <drift: best (test)>,
/// "lock_only": <step 1: the smart lock alone> }`, JSON.
#[wasm_bindgen]
pub fn grip_balance_defaults_json() -> String {
    json(&serde_json::json!({
        "before": wp_vehicle::grip_lab::GripBalance::default(),
        // "new" = the step-14 default the owner drives (shiro_turbo / rampart_50 are the
        // owner's feel references, lead 2026-10-21); "grip" = the grip-drivable rework (test).
        "new": wp_vehicle::grip_lab::GripBalance::lab_default(),
        "grip": wp_vehicle::grip_lab::GripBalance::grip(),
        "step2": wp_vehicle::grip_lab::GripBalance::step2(),
        "slow_weight": wp_vehicle::grip_lab::GripBalance::for_slow_weight(),
        "drift_best": wp_vehicle::grip_lab::GripBalance::drift_best(),
        "lock_only": wp_vehicle::grip_lab::GripBalance::lock_only(),
    }))
}

/// Set the grip-balance lab override from JSON (missing fields = the data), or clear it with
/// `""`. Process-wide, not logged in recordings.
#[wasm_bindgen]
pub fn grip_balance_set(json_text: &str) -> Result<(), JsValue> {
    if json_text.trim().is_empty() {
        wp_vehicle::grip_lab::set_override(None);
        return Ok(());
    }
    let g: wp_vehicle::grip_lab::GripBalance = serde_json::from_str(json_text).map_err(err)?;
    wp_vehicle::grip_lab::set_override(Some(g));
    Ok(())
}

/// Set the car / tyre lab override from JSON (missing fields = the data), or clear it with `""`.
/// Process-wide, not logged in recordings.
#[wasm_bindgen]
pub fn car_tune_set(tune_json: &str) -> Result<(), JsValue> {
    if tune_json.trim().is_empty() {
        wp_vehicle::car_lab::set_override(None);
        return Ok(());
    }
    let t: wp_vehicle::car_lab::CarTune = serde_json::from_str(tune_json).map_err(err)?;
    wp_vehicle::car_lab::set_override(Some(t));
    Ok(())
}

/// "Use W handling everywhere" (Drift Lab W): `true` limits the lab overrides (plain
/// steering, ride, car / tyre, drift tuning) to tagged vehicles ([`set_lab_tagged`]), so AI
/// racers, police and traffic keep the stock handling; `false` (the default) applies them to
/// every vehicle (`wp_vehicle::lab_scope`).
#[wasm_bindgen]
pub fn lab_scope_set_tagged_only(on: bool) {
    wp_vehicle::lab_scope::set_tagged_only(on);
}

/// Whether the lab overrides are limited to tagged vehicles.
#[wasm_bindgen]
pub fn lab_scope_tagged_only() -> bool {
    wp_vehicle::lab_scope::tagged_only()
}

/// Tag (or untag) a vehicle for the lab overrides (re-applies its body inertia). Not in the
/// replay log; the testbed tags the player at every session start, replays included.
#[wasm_bindgen]
pub fn set_lab_tagged(entity: u32, on: bool) -> Result<(), JsValue> {
    with(|s| s.set_lab_tagged(EntityId(entity), on))
}

/// Whether a vehicle is tagged for the lab overrides.
#[wasm_bindgen]
pub fn lab_tagged(entity: u32) -> Result<bool, JsValue> {
    with(|s| {
        s.vehicle(EntityId(entity))
            .map(|v| v.vehicle.lab_tagged())
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}
