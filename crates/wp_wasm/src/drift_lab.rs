//! Drift Lab B / C (lead helpers `lead/drift-lab-b`, `lead/drift-lab-c`; owner rows 90 / 91 /
//! 104 / 106): the testbed's "tune" panel and readout for the drift assist in
//! `wp_vehicle::drift`.
//!
//! | function | what |
//! |---|---|
//! | `drift_tuning_json()` | the constants in effect (override or defaults), JSON |
//! | `drift_tuning_defaults_json()` | the shipped defaults, JSON |
//! | `drift_tuning_set(json)` | set the runtime override (`""` clears it); unknown fields are ignored, missing ones take the default |
//! | `drift_readout(entity)` | `[mode, slide_deg, onset, envelope_deg, authority, throttle_scale, key_throttle, engine_throttle, held_deg, sliding]`, mode 0 off · 1 grip · 2 straighten · 3 hold (countersteer key) · 4 angle-up (key into the turn) · 5 exceeded · 6 transition; `key_throttle` = the driver's throttle input, `engine_throttle` = what reaches the engine (after shaping, pedal filter, TCS / ESC), `held_deg` = the slide angle Hold / AngleUp keeps, `sliding` 1 = a slide is confirmed (the steering's slide help is on, row 102) |
//!
//! The override is process-wide and not logged in recordings (a developer tool; see
//! `wp_vehicle::drift`).

use wasm_bindgen::prelude::*;
use wp_contracts::EntityId;
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
