//! Drift Lab W car / tyre experiments (lead, after step 2's finding that a held countersteer
//! cannot hold a power slide in our physics): a process-wide **lab override** of a few car and
//! tyre properties, to find which ones make "countersteer key held + full throttle in 2nd" hold
//! a steady 30–45° drift (reference clip 1) without breaking grip driving, taps or the handbrake
//! pivot. Lab only, like [`crate::plain`] and [`crate::ride_lab`]: never in shipped data, the
//! replay log or the netcode; the same in Arcade and Simulation. Without it the vehicle step is
//! unchanged (bit-identical). Good values are routed to the tyre / car data owner.

use std::sync::RwLock;

use wp_contracts::TireDef;

use crate::tire::SpinGrip;

/// Car / tyre lab knobs. [`CarTune::default`] = the data as is.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CarTune {
    /// How much the tyre's longitudinal slip counts against its **lateral** force in the
    /// combined-slip model, 0..=1 (1 = the data's friction ellipse: a spinning tyre loses most
    /// of its side force; 0 = side force ignores wheelspin). Longitudinal force is unchanged.
    /// Applies to the driven-and-rear tyres selected by [`CarTune::combined_rear_only`].
    pub lat_under_spin: f32,
    /// Apply [`CarTune::lat_under_spin`] / [`CarTune::spin_model`] to the rear tyres only (else
    /// all four).
    pub combined_rear_only: bool,
    /// Rear tyres' lateral peak slip angle scale (bigger = the peak comes later and the force
    /// curve is wider: a more forgiving slide).
    pub rear_peak_slip: f32,
    /// Front tyres' lateral peak slip angle scale.
    pub front_peak_slip: f32,
    /// Front steering lock scale (× the car's max steer angle).
    pub lock: f32,
    /// Rear differential locked (else the car's own diff).
    pub rear_locked: bool,
    /// Rear differential open (tyre-slip test; `rear_locked` wins if both are set).
    pub rear_open: bool,
    /// Engine torque scale.
    pub torque: f32,
    /// Diagnostic, **not physical**: a yaw damping torque `−k · m · L² · r` (k in 1/s, m the
    /// mass, L the wheelbase, r the yaw rate). 0 = off. Shows how much yaw damping a held drift
    /// would need; the physical knobs above are the ones to route.
    pub yaw_damping: f32,
    /// Tyre model A/B (tyre-slip workstream): `true` = the combined-slip model with side force
    /// that keeps rising with slip angle under drive wheelspin ([`crate::tire::SpinGrip`]);
    /// `false` = the data's friction ellipse. Applies to the tyres chosen by
    /// [`CarTune::combined_rear_only`] (default: the rears).
    pub spin_model: bool,
    /// [`crate::tire::SpinGrip::b0`].
    pub spin_b0: f32,
    /// [`crate::tire::SpinGrip::b2`], 1/rad.
    pub spin_b2: f32,
    /// [`crate::tire::SpinGrip::c`].
    pub spin_c: f32,
    /// [`crate::tire::SpinGrip::p`].
    pub spin_p: f32,
    /// [`crate::tire::SpinGrip::fx_yield`].
    pub spin_fx_yield: f32,
    /// [`crate::tire::SpinGrip::on_braking`].
    pub spin_on_braking: bool,
    /// [`crate::tire::SpinGrip::ellipse`].
    pub spin_ellipse: f32,
    /// [`crate::tire::SpinGrip::fx_slide`].
    pub spin_fx_slide: f32,
    /// [`crate::tire::SpinGrip::fx_slide_from_deg`].
    pub spin_fx_slide_from_deg: f32,
    /// [`crate::tire::SpinGrip::fx_slide_full_deg`].
    pub spin_fx_slide_full_deg: f32,
    /// Rear tyres' relaxation length scale (slip-angle lag; 1 = data).
    pub rear_relax: f32,
    /// Apply each tyre's self-aligning moment (pneumatic trail × side force) to the body as a
    /// yaw torque (the data path only feeds it to force feedback).
    pub aligning_on_body: bool,
    /// Compliant rack / column (tyre-slip step 2), degrees of road-wheel angle per kN·m of the
    /// front tyres' steering-axis moment (aligning moment + caster trail × side force). The
    /// wheels yield toward the travel, like a real car's column and the driver's hands. 0 =
    /// rigid (the data).
    pub rack_compliance: f32,
    /// Lag of the rack compliance, s (column / hands inertia).
    pub rack_compliance_tau_s: f32,
    /// The compliant rack only yields toward the centre (never past the key's position, as
    /// at a lock stop / the driver holding the key's angle).
    pub rack_yield_to_centre_only: bool,
    /// Drift-only lock extension (tyre-slip step 3): extra steering lock, as a fraction of
    /// the car's lock, available once the body slip passes [`CarTune::drift_lock_from_deg`]
    /// (ramped over the next 10°). 0 = off.
    pub drift_lock_extra: f32,
    /// Body slip where the drift-only lock extension starts, degrees.
    pub drift_lock_from_deg: f32,
    /// Front tyres' lateral Magic Formula `C` override (0 = data). Lower = flatter past the
    /// peak (asymptote `sin(C·π/2)` of the peak: 1.4 → 0.81, 1.2 → 0.95); B is scaled so the
    /// cornering stiffness B·C·D is unchanged.
    pub front_lat_c: f32,
    /// Front tyres' deep-slip falloff (tyre-slip step 3): fraction of side force lost at large
    /// slip angles, ramped (smoothstep) from `front_deep_from_deg` to `front_deep_full_deg`.
    /// Leaves grip driving (slip below the start) untouched. 0 = off.
    pub front_deep_falloff: f32,
    /// Slip angle where the front deep-slip falloff starts, degrees.
    pub front_deep_from_deg: f32,
    /// Slip angle where the front deep-slip falloff is complete, degrees.
    pub front_deep_full_deg: f32,
    /// Side-force floor under heavy drive wheelspin ([`crate::tire::SpinSideFloor::floor`]).
    /// Default = the shipped value ([`crate::tire::SPIN_SIDE_FLOOR`]); 0 = off (the old
    /// friction ellipse, for A/B).
    pub side_floor: f32,
}

impl Default for CarTune {
    fn default() -> Self {
        let d = SpinGrip::default();
        Self {
            lat_under_spin: 1.0,
            combined_rear_only: true,
            rear_peak_slip: 1.0,
            front_peak_slip: 1.0,
            lock: 1.0,
            rear_locked: false,
            rear_open: false,
            torque: 1.0,
            yaw_damping: 0.0,
            spin_model: false,
            spin_b0: d.b0,
            spin_b2: d.b2,
            spin_c: d.c,
            spin_p: d.p,
            spin_fx_yield: d.fx_yield,
            spin_on_braking: d.on_braking,
            spin_ellipse: d.ellipse,
            spin_fx_slide: d.fx_slide,
            spin_fx_slide_from_deg: d.fx_slide_from_deg,
            spin_fx_slide_full_deg: d.fx_slide_full_deg,
            rear_relax: 1.0,
            aligning_on_body: false,
            rack_compliance: 0.0,
            rack_compliance_tau_s: 0.05,
            rack_yield_to_centre_only: false,
            drift_lock_extra: 0.0,
            drift_lock_from_deg: 15.0,
            front_lat_c: 0.0,
            front_deep_falloff: 0.0,
            front_deep_from_deg: 8.0,
            front_deep_full_deg: 20.0,
            side_floor: crate::tire::SPIN_SIDE_FLOOR.floor,
        }
    }
}

impl CarTune {
    /// The step-4 test set (docs/integration/drift-lab-w.md, step 4): rear side grip under
    /// spin 0.15, lock × 0.9, diagnostic yaw damping 0.2 /s. It does **not** hold the drift:
    /// scored direction-aware, the held countersteer keeps the intended side only ~0.5 s, then
    /// the car swings through and circles the other way. Kept as an A/B starting point.
    pub fn spin_grip_damper() -> Self {
        Self {
            lat_under_spin: 0.15,
            lock: 0.9,
            yaw_damping: 0.2,
            ..Self::default()
        }
    }

    /// Tyre-slip workstream's spin-model set (docs/integration/tyre-slip.md): the spin-weighted
    /// combined slip on the rears, no yaw damper, steering / ride as they are. On parity_rwd_d
    /// with the lab driver (counter key at 6° + full throttle, 0.6 g entry) it holds ~41–44° for
    /// 5 s; it is **tuned to a knife edge** (other entries and nearby values still spin).
    pub fn spin_candidate() -> Self {
        Self {
            spin_model: true,
            ..Self::default()
        }
    }

    /// Tyre-slip step 3 set: the spin model (re-tuned) + a drift-only lock reduction (−50 %
    /// counter-lock once the body slip passes 20°) + a front deep-slip falloff (−13 % side
    /// force from 17° to 42° of front slip) + rear relaxation × 1.5. Base lock unchanged, so
    /// grip driving stays within ~6 %. Holds 15 / 27 of the robustness set (data 0 / 27) but
    /// as a slow 27–54° oscillation, not steady (docs/integration/tyre-slip.md, step 3).
    pub fn spin_lock_candidate() -> Self {
        Self {
            spin_model: true,
            spin_b0: 6.45,
            spin_b2: 1.98,
            spin_c: 0.82,
            spin_p: 7.0,
            spin_fx_slide: 1.0,
            spin_fx_slide_from_deg: 3.1,
            spin_fx_slide_full_deg: 3.3,
            rear_relax: 1.48,
            drift_lock_extra: -0.5,
            drift_lock_from_deg: 20.3,
            front_deep_falloff: 0.13,
            front_deep_from_deg: 17.4,
            front_deep_full_deg: 42.0,
            ..Self::default()
        }
    }

    /// The steering half of the "drift: spin + VR (test)" lab preset (tyres:
    /// [`CarTune::spin_candidate`]): Drift Lab W's velocity-relative steering at half gain
    /// (fade 7 → 17° of body slip, align time 0.26 s, slide key gain 0.31). With the spin model
    /// it holds 13 / 27 of the robustness set, all in the 30–45° band (docs/integration/
    /// tyre-slip.md, "Velocity-relative steering × tyres"); not steady, speed bleeds.
    pub fn spin_vr_plain() -> crate::plain::PlainSteer {
        crate::plain::PlainSteer {
            velocity_relative: true,
            align_gain: 0.5,
            align_from_deg: 7.0,
            align_full_deg: 17.0,
            t_align_s: 0.26,
            slide_key_gain: 0.31,
            ..crate::plain::PlainSteer::default()
        }
    }

    /// The steering half of the "transition: swing rack (test)" lab preset (with
    /// `GripBalance::lab_default()`, the lab ride with yaw inertia × 1.5 and the spin model):
    /// the plain profile with the swing rack × 2.5 from 80°/s of yaw (docs/integration/
    /// tyre-slip.md, "Drift transitions").
    pub fn transition_plain() -> crate::plain::PlainSteer {
        crate::plain::PlainSteer {
            swing_rate: 2.5,
            swing_yaw_dps: 80.0,
            ..crate::plain::PlainSteer::default()
        }
    }

    /// "Spin model v2 (ellipse)" (tyre-slip, drift transitions; lab A/B next to
    /// [`CarTune::spin_candidate`]): the spin model with its combined force pulled back into
    /// the friction ellipse (`spin_ellipse` 0.7), so a throttle lift hands the drive force's
    /// share of the grip back to cornering and the owner's lift flick swings the car through.
    /// Tuned on the step-20 lab default (`GripBalance::lab_default()`, `RideTune::firm()`)
    /// with [`CarTune::spin_v2_plain`] (examples/transition.rs `v2search`): held-drift set
    /// 25 / 27 (v1 26), lift flicks caught 13 / 18 at the reference gaps (v1 0), lift-off in a
    /// normal corner 0 / 18 step-outs (docs/integration/tyre-slip.md, "Spin model v2").
    pub fn spin_v2_candidate() -> Self {
        Self {
            spin_model: true,
            spin_ellipse: 0.7,
            spin_fx_slide: 0.35,
            spin_b0: 7.15,
            spin_b2: 1.41,
            spin_c: 0.78,
            spin_p: 5.0,
            rear_relax: 0.73,
            ..Self::default()
        }
    }

    /// The steering of the "spin model v2 (ellipse)" preset: the plain profile with the swing
    /// rack × 2.08 from 86°/s of yaw (the threshold is sensitive: 82°/s loses holds, 90°/s
    /// loses catches; docs/integration/tyre-slip.md).
    pub fn spin_v2_plain() -> crate::plain::PlainSteer {
        crate::plain::PlainSteer {
            swing_rate: 2.08,
            swing_yaw_dps: 86.0,
            ..crate::plain::PlainSteer::default()
        }
    }

    /// Whether corner `i` (0 FL, 1 FR, 2 RL, 3 RR) uses [`CarTune::lat_under_spin`].
    pub fn combined_applies(&self, i: usize) -> bool {
        !self.combined_rear_only || i >= 2
    }

    /// The combined-slip model of corner `i` (`None` = the data's friction ellipse).
    pub fn spin_grip(&self, i: usize) -> Option<SpinGrip> {
        (self.spin_model && self.combined_applies(i)).then_some(SpinGrip {
            b0: self.spin_b0,
            b2: self.spin_b2,
            c: self.spin_c,
            p: self.spin_p,
            fx_yield: self.spin_fx_yield,
            on_braking: self.spin_on_braking,
            ellipse: self.spin_ellipse,
            fx_slide: self.spin_fx_slide,
            fx_slide_from_deg: self.spin_fx_slide_from_deg,
            fx_slide_full_deg: self.spin_fx_slide_full_deg,
        })
    }

    /// Side force scale of corner `i` at slip angle `alpha` (rad) from the front deep-slip
    /// falloff (1 = none).
    pub fn side_scale(&self, i: usize, alpha: f32) -> f32 {
        if i >= 2 || self.front_deep_falloff <= 0.0 {
            return 1.0;
        }
        let (a0, a1) = (
            self.front_deep_from_deg,
            self.front_deep_full_deg.max(self.front_deep_from_deg + 0.1),
        );
        let x = ((alpha.abs().to_degrees() - a0) / (a1 - a0)).clamp(0.0, 1.0);
        1.0 - self.front_deep_falloff.clamp(0.0, 0.9) * x * x * (3.0 - 2.0 * x)
    }

    /// `tire` with the peak slip angle scale of corner `i`.
    pub fn tire_for(&self, tire: &TireDef, i: usize) -> TireDef {
        let mut t = tire.clone();
        let k = if i >= 2 {
            self.rear_peak_slip
        } else {
            self.front_peak_slip
        };
        let k = k.clamp(0.2, 5.0);
        // Stretch the lateral curve along the slip axis: the peak moves to k × the slip angle.
        t.peak_slip_angle_rad *= k;
        t.lateral.b /= k;
        if i >= 2 {
            t.relaxation_length_m *= self.rear_relax.clamp(0.05, 10.0);
        } else if self.front_lat_c > 0.0 {
            // Keep the cornering stiffness B·C·D (grip driving) while flattening the tail.
            let c = self.front_lat_c.clamp(1.0, 2.0);
            t.lateral.b *= t.lateral.c / c;
            t.lateral.c = c;
        }
        t
    }
}

static OVERRIDE: RwLock<Option<CarTune>> = RwLock::new(None);

/// Set (`Some`) or clear (`None`) the car / tyre lab override for every vehicle in this
/// process. Testbed Drift Lab W and its measurements only.
pub fn set_override(t: Option<CarTune>) {
    if let Ok(mut o) = OVERRIDE.write() {
        *o = t;
    }
}

/// The car / tyre lab override in effect, or `None` (the data as is).
pub fn active() -> Option<CarTune> {
    OVERRIDE.read().ok().and_then(|o| *o)
}
