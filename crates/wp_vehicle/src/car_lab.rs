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
    /// [`crate::tire::SpinGrip::fx_slide`].
    pub spin_fx_slide: f32,
    /// Rear tyres' relaxation length scale (slip-angle lag; 1 = data).
    pub rear_relax: f32,
    /// Apply each tyre's self-aligning moment (pneumatic trail × side force) to the body as a
    /// yaw torque (the data path only feeds it to force feedback).
    pub aligning_on_body: bool,
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
            spin_fx_slide: d.fx_slide,
            rear_relax: 1.0,
            aligning_on_body: false,
        }
    }
}

impl CarTune {
    /// Best "countersteer key held + full throttle" set found so far on parity_rwd_d
    /// (docs/integration/drift-lab-w.md, step 4): rear side grip under spin 0.15, lock × 0.9,
    /// diagnostic yaw damping 0.2 /s. Step 4 reported ~4.7 s held at 25–28°; the tyre-slip
    /// re-measure (direction-aware) shows it holds the intended side only ~0.5 s, then swings
    /// through and circles the other way on the held key.
    pub fn drift_candidate() -> Self {
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
            fx_slide: self.spin_fx_slide,
        })
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
