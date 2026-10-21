//! Drift Lab W ride tuning (owner 2026-10-20: "suspension feels far too switchy"): a
//! process-wide **lab override** that scales every car's springs, anti-roll bars and dampers
//! and moves where the tyre forces act on the body (roll-centre / pitch-centre height), so
//! the weight transfer can be made calmer and slower in the lab and good values routed to the
//! car data owner. Like [`crate::plain`] it is a developer tool: not in the replay log or the
//! netcode, never set by the server, the same in Arcade and Simulation.
//!
//! Without the override the vehicle step is unchanged (bit-identical).
//!
//! Roll / pitch centre: our tyre forces act on the body at the contact patch, i.e. a roll
//! centre at the ground, so all of the lateral load transfer goes through the springs and the
//! body rolls about the ground. Raising the point where the lateral (longitudinal) tyre force
//! acts by `roll_centre_m` (`pitch_centre_m`) shortens the lever to the CG: less roll (pitch)
//! for the same cornering (braking / acceleration). Caveat: in this model the part of the load
//! transfer the links would carry is not added back to the tyres, so a high roll centre also
//! lowers the total lateral load transfer a little (slightly more grip).

use std::sync::RwLock;

use wp_contracts::{SuspensionDef, VehicleDef, GRAVITY_MPS2};

/// Ride tuning scales (Drift Lab W tune panel). [`RideTune::default`] = the car data as is.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RideTune {
    /// Spring rate scale (all corners).
    pub spring: f32,
    /// Anti-roll bar rate scale (both axles: the front / rear balance is kept).
    pub arb: f32,
    /// Damper scale, bump (compression).
    pub damper_bump: f32,
    /// Damper scale, rebound (extension).
    pub damper_rebound: f32,
    /// Height above the ground where the tyres' lateral force acts on the body, m (0 = the
    /// contact patch, the data's behaviour); clamped to the CG height.
    pub roll_centre_m: f32,
    /// Height above the ground where the tyres' longitudinal force acts on the body, m
    /// (anti-dive / anti-squat feel); clamped to the CG height.
    pub pitch_centre_m: f32,
    /// Keep the data's static ride height when the springs are scaled: the spring preload is
    /// raised so the static compression (and so the bump-stop gap at rest) stays as in the
    /// data. Without it, softer springs sag onto the bump stops (springs below × 0.85).
    pub keep_ride_height: bool,
    /// Suspension travel scale (more travel = more room before the bump stops in roll).
    pub travel: f32,
    /// Body roll inertia scale (Ixx; applied when the car's body is built — the lab restarts
    /// the car).
    pub roll_inertia: f32,
    /// Body pitch inertia scale (Iyy; at body build).
    pub pitch_inertia: f32,
    /// Body yaw inertia scale (Izz; at body build).
    pub yaw_inertia: f32,
}

impl Default for RideTune {
    fn default() -> Self {
        Self {
            spring: 1.0,
            arb: 1.0,
            damper_bump: 1.0,
            damper_rebound: 1.0,
            roll_centre_m: 0.0,
            pitch_centre_m: 0.0,
            keep_ride_height: false,
            travel: 1.0,
            roll_inertia: 1.0,
            pitch_inertia: 1.0,
            yaw_inertia: 1.0,
        }
    }
}

impl RideTune {
    /// Slow weight transfer (owner: "the car weight transitions way too fast"; reference clip 4):
    /// springs × 0.5 and no anti-roll bars with the static ride height kept (preload raised),
    /// 1.7 × the travel so the bigger roll stays off the bump stops, dampers × 1.8 / 2.4, roll /
    /// pitch / yaw inertia × 2.4 / 2.0 / 1.5. parity_rwd_d / rampart_50, right key held at
    /// 80 km/h: 10→90 % roll rise 0.60 s, peak roll 6.2° at ~1 g, ≤ 5 % overshoot, ≤ 81 % of the
    /// travel (docs/integration/drift-lab-w.md, step 8). Not the lab default: it breaks the
    /// spin model's knife-edge held drift.
    pub fn slow_weight() -> Self {
        Self {
            spring: 0.5,
            arb: 0.0,
            damper_bump: 1.8,
            damper_rebound: 2.4,
            keep_ride_height: true,
            travel: 1.7,
            roll_inertia: 2.4,
            pitch_inertia: 2.0,
            yaw_inertia: 1.5,
            ..Self::default()
        }
    }

    /// Drift Lab W's default (owner: calmer, slower weight transfer, step 3): springs × 0.9
    /// (keeps ≥ 50 mm of bump travel at rest), anti-roll bars × 0.25, dampers × 1.4 bump /
    /// × 1.8 rebound. Muscle cars: roll 1.76 → 1.50 Hz, ζ 0.48 → 0.94 (model); no rocking after
    /// a keyboard lane change (docs/integration/drift-lab-w.md, step 3).
    pub fn lab_default() -> Self {
        Self {
            spring: 0.9,
            arb: 0.25,
            damper_bump: 1.4,
            damper_rebound: 1.8,
            ..Self::default()
        }
    }

    /// Drift Lab W step 10 "new" ride (lead: weight + snap-back as one default): a milder
    /// [`RideTune::slow_weight`] that keeps the clip-1 hold — springs × 0.7, bars × 0.15,
    /// dampers × 1.6 / 2.1 (ride height kept, travel × 1.4), roll / pitch inertia × 1.8 / 1.6,
    /// yaw inertia unchanged (more yaw inertia breaks the hold). Measured: roll ≈ 1.05 Hz,
    /// ζ ≈ 1.0, step-steer rise 0.40 s (lab default 0.31, slow weight 0.60), turn-in unchanged.
    pub fn medium_weight() -> Self {
        Self {
            spring: 0.7,
            arb: 0.15,
            damper_bump: 1.6,
            damper_rebound: 2.1,
            keep_ride_height: true,
            travel: 1.4,
            roll_inertia: 1.8,
            pitch_inertia: 1.6,
            ..Self::default()
        }
    }

    /// Drift Lab W's default after the owner's step-18 ride (springs / bars 0.95 / 1.0 and
    /// 1.5 / 1.5 cornered better than medium weight's 0.7 / 0.15: the soft bars let the body
    /// roll onto the outside rear tyre, so it stepped out on steer + lift). Springs and bars
    /// × 1.25, with the weight transfer kept slow by the dampers (× 4 bump / × 5 rebound,
    /// roll ζ ≈ 1.6) instead of soft bars; the rest as [`RideTune::medium_weight`].
    /// Step-14 balance, rampart_50 / parity_rwd_d / shiro_turbo: steer + lift peak slip
    /// 16–17° → 11–12°, body slip at 0.8 g 2.9–3.6° → 2.0–2.3°, step-steer roll rise
    /// 0.45–0.52 s → 0.58–0.68 s, peak roll 5.2–6.3° → 2.6–3.2°, peak g 0.94 → 0.86–0.92
    /// (docs/integration/grip-balance.md, "Stiff ride"). Above × 1.25 the handling barely
    /// changes and the roll only gets smaller.
    pub fn stiff_ride() -> Self {
        Self {
            spring: 1.25,
            arb: 1.25,
            damper_bump: 4.0,
            damper_rebound: 5.0,
            ..Self::medium_weight()
        }
    }

    /// `def` scaled ([`RideTune::scaled`]) for a corner carrying `corner_load_n` at rest:
    /// travel × [`RideTune::travel`] and, with [`RideTune::keep_ride_height`], the preload
    /// raised so the static compression stays as in the data.
    pub fn scaled_at(&self, def: &SuspensionDef, corner_load_n: f32) -> SuspensionDef {
        let mut s = self.scaled(def);
        s.travel_m = def.travel_m * self.travel.clamp(0.5, 3.0);
        if self.keep_ride_height {
            // Static: k (x + preload) = load → x = load / k − preload; keep x.
            let x0 = corner_load_n / def.spring_n_per_m - def.preload_m;
            s.preload_m = corner_load_n / s.spring_n_per_m - x0;
        }
        s
    }

    /// Body inertia `[roll, pitch, yaw]` (contract order) with the inertia scales.
    pub fn inertia(&self, i: [f32; 3]) -> [f32; 3] {
        [
            i[0] * self.roll_inertia.clamp(0.2, 5.0),
            i[1] * self.pitch_inertia.clamp(0.2, 5.0),
            i[2] * self.yaw_inertia.clamp(0.2, 5.0),
        ]
    }

    /// `def` with this tuning's scales applied.
    pub fn scaled(&self, def: &SuspensionDef) -> SuspensionDef {
        let mut s = def.clone();
        s.spring_n_per_m *= self.spring.max(0.05);
        s.arb_n_per_m *= self.arb.max(0.0);
        s.damper_bump_ns_per_m *= self.damper_bump.max(0.0);
        s.damper_rebound_ns_per_m *= self.damper_rebound.max(0.0);
        s
    }
}

static OVERRIDE: RwLock<Option<RideTune>> = RwLock::new(None);

/// Set (`Some`) or clear (`None`) the ride tuning override for every vehicle in this process.
/// Testbed Drift Lab W and its measurements only; see the module docs.
pub fn set_override(t: Option<RideTune>) {
    if let Ok(mut o) = OVERRIDE.write() {
        *o = t;
    }
}

/// The ride tuning in effect, or `None` (the car data as is).
pub fn active() -> Option<RideTune> {
    OVERRIDE.read().ok().and_then(|o| *o)
}

/// Linear roll / pitch model of a car under `tune` (springs and bars at the wheels, motion
/// ratio 1 as in our strut model; the body rolls about the roll centre and pitches about the
/// pitch centre; the tyres are rigid).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct RideModel {
    /// Roll stiffness (springs + bars, less the CG's gravity moment), N·m/rad.
    pub roll_stiffness: f32,
    /// Roll natural frequency, Hz.
    pub roll_hz: f32,
    /// Roll damping ratio, bump/rebound mean (1 = critical).
    pub roll_zeta: f32,
    /// Steady roll gradient, degrees per g of lateral acceleration.
    pub roll_deg_per_g: f32,
    /// Pitch natural frequency, Hz.
    pub pitch_hz: f32,
    /// Pitch damping ratio, bump/rebound mean.
    pub pitch_zeta: f32,
}

/// [`RideModel`] of `def` under `tune`.
pub fn ride_model(def: &VehicleDef, tune: &RideTune) -> RideModel {
    let s: Vec<SuspensionDef> = def.suspension.iter().map(|d| tune.scaled(d)).collect();
    let m = def.mass_kg;
    let h = def.cg_height_m;
    let h_roll = (h - tune.roll_centre_m.clamp(0.0, h)).max(0.0);
    let h_pitch = (h - tune.pitch_centre_m.clamp(0.0, h)).max(0.0);
    let tf = def.track_front_m;
    let tr = def.track_rear_m;
    // Per axle: two springs at ±t/2 → k·t²/2; a bar of rate a (force a·(xl − xr) on each
    // wheel) → a·t² (its moment per radian of roll: a · t·φ · t).
    let k_roll = s[0].spring_n_per_m * tf * tf / 2.0
        + s[0].arb_n_per_m * tf * tf
        + s[2].spring_n_per_m * tr * tr / 2.0
        + s[2].arb_n_per_m * tr * tr;
    let c = |d: &SuspensionDef| 0.5 * (d.damper_bump_ns_per_m + d.damper_rebound_ns_per_m);
    let c_roll = c(&s[0]) * tf * tf / 2.0 + c(&s[2]) * tr * tr / 2.0;
    let k_eff = (k_roll - m * GRAVITY_MPS2 * h_roll).max(1.0);
    let inertia = tune.inertia(def.inertia_kgm2);
    let i_roll = inertia[0] + m * h_roll * h_roll;
    let roll_w = (k_eff / i_roll).sqrt();
    // Pitch: axle distances from the CG along the wheelbase.
    let a = def.wheelbase_m * (1.0 - def.front_weight_frac);
    let b = def.wheelbase_m * def.front_weight_frac;
    let k_pitch = 2.0 * (s[0].spring_n_per_m * a * a + s[2].spring_n_per_m * b * b);
    let c_pitch = 2.0 * (c(&s[0]) * a * a + c(&s[2]) * b * b);
    let k_pitch_eff = (k_pitch - m * GRAVITY_MPS2 * h_pitch).max(1.0);
    let i_pitch = inertia[1] + m * h_pitch * h_pitch;
    let pitch_w = (k_pitch_eff / i_pitch).sqrt();
    RideModel {
        roll_stiffness: k_eff,
        roll_hz: roll_w / std::f32::consts::TAU,
        roll_zeta: c_roll / (2.0 * (k_eff * i_roll).sqrt()),
        roll_deg_per_g: (m * GRAVITY_MPS2 * h_roll / k_eff).to_degrees(),
        pitch_hz: pitch_w / std::f32::consts::TAU,
        pitch_zeta: c_pitch / (2.0 * (k_pitch_eff * i_pitch).sqrt()),
    }
}

/// Static load of corner `i` (0 FL, 1 FR, 2 RL, 3 RR) from the car's mass and weight split, N.
pub fn corner_load(def: &VehicleDef, i: usize) -> f32 {
    let w = def.mass_kg * GRAVITY_MPS2;
    let f = def.front_weight_frac.clamp(0.0, 1.0);
    0.5 * w * if i < 2 { f } else { 1.0 - f }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car() -> VehicleDef {
        let p = format!(
            "{}/../../data/cars/rampart_50.json",
            env!("CARGO_MANIFEST_DIR")
        );
        wp_contracts::load_vehicle_def(p).unwrap()
    }

    #[test]
    fn default_scales_leave_the_data_alone() {
        let d = car();
        let t = RideTune::default();
        for s in &d.suspension {
            assert_eq!(&t.scaled(s), s);
        }
    }

    #[test]
    fn keep_ride_height_keeps_the_static_compression() {
        let d = car();
        let t = RideTune {
            spring: 0.5,
            keep_ride_height: true,
            ..RideTune::default()
        };
        for (i, s) in d.suspension.iter().enumerate() {
            let load = corner_load(&d, i);
            let x0 = crate::suspension::static_compression(s, load);
            let x1 = crate::suspension::static_compression(&t.scaled_at(s, load), load);
            assert!((x0 - x1).abs() < 1e-4, "{i}: {x0} vs {x1}");
        }
    }

    #[test]
    fn slow_weight_preset_rolls_slowly_and_near_critically() {
        let d = car();
        let m = ride_model(&d, &RideTune::slow_weight());
        assert!((0.65..1.1).contains(&m.roll_hz), "{m:?}");
        assert!(m.roll_zeta > 0.9, "{m:?}");
        let base = ride_model(&d, &RideTune::default());
        assert!(m.pitch_hz < base.pitch_hz, "{m:?}");
    }

    #[test]
    fn medium_weight_sits_between_the_lab_default_and_slow_weight() {
        let d = car();
        let lab = ride_model(&d, &RideTune::lab_default());
        let mid = ride_model(&d, &RideTune::medium_weight());
        let slow = ride_model(&d, &RideTune::slow_weight());
        assert!(
            slow.roll_hz < mid.roll_hz && mid.roll_hz < lab.roll_hz,
            "{mid:?}"
        );
        assert!(mid.roll_zeta > 0.85, "{mid:?}");
        assert_eq!(RideTune::medium_weight().yaw_inertia, 1.0);
    }

    #[test]
    fn stiff_ride_is_stiffer_than_medium_weight_but_more_damped() {
        let d = car();
        let mid = ride_model(&d, &RideTune::medium_weight());
        let stiff = ride_model(&d, &RideTune::stiff_ride());
        assert!(stiff.roll_deg_per_g < 0.5 * mid.roll_deg_per_g, "{stiff:?}");
        // Slow weight transfer from the dampers, not soft bars: over-damped, no overshoot.
        assert!(stiff.roll_zeta > 1.3, "{stiff:?}");
        assert!(stiff.roll_zeta > mid.roll_zeta, "{stiff:?}");
        let t = RideTune::stiff_ride();
        assert_eq!((t.roll_inertia, t.yaw_inertia), (1.8, 1.0));
        assert!(t.keep_ride_height);
    }

    #[test]
    fn softer_bars_and_more_damping_calm_the_roll() {
        let d = car();
        let base = ride_model(&d, &RideTune::default());
        let calm = ride_model(
            &d,
            &RideTune {
                arb: 0.6,
                damper_bump: 1.4,
                damper_rebound: 1.4,
                ..RideTune::default()
            },
        );
        assert!(calm.roll_hz < base.roll_hz);
        assert!(calm.roll_zeta > base.roll_zeta);
        assert!(calm.roll_deg_per_g > base.roll_deg_per_g);
        // A higher roll centre: less roll per g.
        let rc = ride_model(
            &d,
            &RideTune {
                roll_centre_m: 0.15,
                ..RideTune::default()
            },
        );
        assert!(rc.roll_deg_per_g < base.roll_deg_per_g);
        assert!(base.roll_hz > 0.5 && base.roll_hz < 5.0, "{base:?}");
    }
}
