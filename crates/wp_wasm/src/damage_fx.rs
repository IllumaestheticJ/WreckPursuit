//! Readable damage for the renderer (feature F1, "damage you can read"): per vehicle, the
//! mechanical damage state the testbed turns into steam, leak trails and bent wheels.
//! Display only; nothing here feeds back into the simulation.

use wasm_bindgen::prelude::*;
use wp_contracts::DamageSim;

/// Floats per vehicle in [`damage_fx`].
pub const DAMAGE_FX_STRIDE: usize = 11;

/// Mechanical damage per vehicle, [`DAMAGE_FX_STRIDE`] floats each, same order as
/// `vehicle_states()`:
/// `[overheating 0..1, fuel_leak_rate (fraction of the tank per s), toe_delta FL FR RL RR (rad),
///   camber_delta FL FR RL RR (rad), seconds to full overheat (-1 = not overheating)]`.
/// From `HandlingEffects`; the toe/camber are the same numbers the tyres use, so a wheel that is
/// drawn bent also pulls.
#[wasm_bindgen]
pub fn damage_fx() -> Result<Vec<f32>, JsValue> {
    crate::get(|s| {
        let sim = s.sim();
        let mut out = Vec::with_capacity(sim.vehicles().len() * DAMAGE_FX_STRIDE);
        for v in sim.vehicles() {
            let e = v.damage.handling_effects();
            out.push(e.overheating);
            out.push(e.fuel_leak_rate);
            out.extend_from_slice(&e.toe_delta);
            out.extend_from_slice(&e.camber_delta);
            out.push(v.damage.seconds_to_overheat().unwrap_or(-1.0));
        }
        out
    })
}
