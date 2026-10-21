//! Punctured tyres (playtest row 132, "an audible cue when spikes hit the target"):
//!
//! - the **blow-out pop** itself is the world pack's `tire/burst` one-shot, played per wheel on
//!   `PoliceEvent::SpikeHit` (recorded in `world_rec`; loud and `Critical` so it is never culled);
//! - a **flap / thrum** loop per deflated wheel (`tire/flat_flap`, synth `flap`): one slap of the
//!   loose carcass per wheel revolution, so its rate follows wheel speed (`freq_hz` = revolutions
//!   per second; the backend retriggers at that rate without re-pitching);
//! - a **rim grind** loop per wheel running on its rim (deflation ≥ `rim_deflation`, until
//!   `wp_vehicle` reports rim contact): the world pack's recorded metal scrape, pitched and
//!   levelled by wheel speed.
//!
//! Deflation comes from the bridge each frame ([`crate::AudioDirector::set_tire_deflation`],
//! `HandlingEffects::tire_puncture`). Presentation only.

use serde::{Deserialize, Serialize};
use wp_contracts::VehicleState;
use wp_math::clamp01;

use crate::types::slot;

/// Flap loop of wheel `i` (FL, FR, RL, RR): `FLAP + i`.
pub const FLAP_SLOT: u32 = slot::SURFACE + 0x20;
/// Rim grind loop of wheel `i`: `RIM_SLOT + i`.
pub const RIM_SLOT: u32 = slot::SURFACE + 0x28;
/// Cue of the flap loop in a world pack.
pub const FLAP_CUE: &str = "tire/flat_flap";

/// Tuning (`audio_config.json` → `flat_tyre`; every field optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlatTyreConfig {
    /// Deflation from which a wheel flaps.
    pub flap_deflation: f32,
    /// Flap gain at full deflation and `full_speed_mps`.
    pub flap_gain: f32,
    /// Deflation from which the wheel runs on its rim (until `wp_vehicle` reports rim contact).
    pub rim_deflation: f32,
    /// Rim grind gain at `full_speed_mps`.
    pub rim_gain: f32,
    /// Wheel surface speed at which flap and grind reach full level, m/s.
    pub full_speed_mps: f32,
    /// Below this wheel surface speed nothing flaps or grinds, m/s.
    pub min_speed_mps: f32,
    /// Blow-out pop gain (per punctured wheel).
    pub pop_gain: f32,
    /// Speak a scanner line when spikes hit.
    pub radio_line: bool,
}

impl Default for FlatTyreConfig {
    fn default() -> Self {
        Self {
            flap_deflation: 0.25,
            flap_gain: 0.6,
            rim_deflation: 0.9,
            rim_gain: 0.55,
            full_speed_mps: 20.0,
            min_speed_mps: 0.8,
            pop_gain: 1.6,
            radio_line: true,
        }
    }
}

/// One wheel's flat-tyre loops this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelFlat {
    /// Wheel index (FL, FR, RL, RR).
    pub wheel: usize,
    /// Flap: (gain, revolutions per second, deflation).
    pub flap: Option<(f32, f32, f32)>,
    /// Rim grind: (gain, pitch).
    pub rim: Option<(f32, f32)>,
}

/// What each wheel of a vehicle with `deflation` (per wheel, 0..=1) sounds like.
/// `radius_m` = loaded wheel radii. Detached or airborne wheels are silent.
pub fn wheels(
    cfg: &FlatTyreConfig,
    s: &VehicleState,
    deflation: [f32; 4],
    radius_m: [f32; 4],
) -> Vec<WheelFlat> {
    let mut out = Vec::new();
    for (i, w) in s.wheel_states.iter().enumerate() {
        let d = clamp01(deflation[i]);
        if d < cfg.flap_deflation || w.detached || !w.contact {
            continue;
        }
        let surface = (w.omega * radius_m[i]).abs();
        if surface < cfg.min_speed_mps {
            continue;
        }
        let speed = clamp01(surface / cfg.full_speed_mps.max(0.1));
        let rev_hz = w.omega.abs() / std::f32::consts::TAU;
        // Flap is loudest while the carcass is loose but not yet folded under the rim.
        let looseness = clamp01((d - cfg.flap_deflation) / (1.0 - cfg.flap_deflation).max(1e-3));
        let flap = Some((
            cfg.flap_gain * (0.35 + 0.65 * speed) * (0.4 + 0.6 * looseness),
            rev_hz,
            d,
        ));
        let rim = (d >= cfg.rim_deflation)
            .then_some((cfg.rim_gain * (0.25 + 0.75 * speed), 0.7 + 0.6 * speed));
        out.push(WheelFlat {
            wheel: i,
            flap,
            rim,
        });
    }
    out
}

/// Scanner lines for a spike hit (picked by the hit count, deterministic).
pub const RADIO_LINES: [&str; 3] = [
    "Spikes hit! Suspect's tyres are out.",
    "Good hit on the spikes, he's running on rims.",
    "Spikes got him, tyres are going flat.",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn state(omega: f32) -> VehicleState {
        let mut s = VehicleState::default();
        for w in &mut s.wheel_states {
            w.contact = true;
            w.omega = omega;
        }
        s
    }

    #[test]
    fn inflated_wheels_are_silent_and_flats_flap_at_wheel_rate() {
        let cfg = FlatTyreConfig::default();
        let r = [0.33; 4];
        assert!(wheels(&cfg, &state(60.0), [0.0; 4], r).is_empty());
        // FL half flat, RR flat on its rim, at ~20 m/s.
        let w = wheels(&cfg, &state(60.0), [0.5, 0.0, 0.0, 1.0], r);
        assert_eq!(w.len(), 2);
        let (g, hz, _) = w[0].flap.unwrap();
        assert!(
            (hz - 60.0 / std::f32::consts::TAU).abs() < 1e-3,
            "one flap per revolution"
        );
        assert!(g > 0.0 && w[0].rim.is_none(), "half flat: no rim contact");
        assert!(w[1].rim.is_some(), "flat on the rim grinds");
        // Rate follows wheel speed.
        let slow = wheels(&cfg, &state(15.0), [0.5, 0.0, 0.0, 0.0], r)[0]
            .flap
            .unwrap();
        assert!((slow.1 * 4.0 - hz).abs() < 1e-3 && slow.0 < g);
        // Stopped: nothing.
        assert!(wheels(&cfg, &state(0.5), [1.0; 4], r).is_empty());
    }

    #[test]
    fn slots_do_not_collide() {
        for i in 0..4 {
            for s in [FLAP_SLOT + i, RIM_SLOT + i] {
                assert!(!(slot::SURFACE..slot::SURFACE + 0x10).contains(&s));
                assert!(s < slot::SIREN && !slot::is_one_shot(s));
            }
        }
    }
}
