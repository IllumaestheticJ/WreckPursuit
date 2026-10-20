//! Grip-driving balance lab (owner 2026-10-20: "turn-in is still severe understeer", "still
//! understeers really badly" in Drift Lab W, grip driving). A process-wide **lab override** of
//! the front / rear tyre balance, the roll-stiffness split and the keyboard steering lock, so
//! the cars' limit balance can be moved from push toward the reference's neutral-to-loose and
//! the good values routed to the car / tyre data owner (docs/integration/grip-balance.md).
//!
//! Like [`crate::car_lab`], [`crate::ride_lab`] and [`crate::plain`] it is a developer tool:
//! not in the replay log or the netcode, never set by the server, the same in Arcade and
//! Simulation. Without it the vehicle step is unchanged (bit-identical). It stacks on top of
//! the other lab overrides (its tyre scales apply after [`crate::car_lab::CarTune::tire_for`],
//! its bar scales after [`crate::ride_lab::RideTune::arb`]).
//!
//! **Smart lock** ([`LockMode::Smart`]): with the plain keyboard rack, a held key asks for
//! `lock(speed)` of road-wheel angle — about 25° at 60 mph with the default 75 % rack curve —
//! while the `sport` tyre's side force peaks at about 6° of slip. So at speed the fronts are
//! driven far past their peak, scrub, and the car pushes: limit understeer that no tyre or
//! balance change can fix. The smart lock caps the lock at speed to what the front tyres can
//! use: the steer angle of a steady corner at [`GripBalance::smart_ay_g`] (`L · a / v²`) plus
//! the front tyre's peak slip angle × (1 + [`GripBalance::smart_margin`]). Full lock is still
//! reached in the rack's `t_full` at every speed (the reference's constant-rate wheel), and at
//! parking speeds the car's full lock is unchanged.

use std::sync::RwLock;

use wp_contracts::TireDef;

/// How the keyboard (plain profile) lock is limited at speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockMode {
    /// The plain profile as it is (its speed-sensitivity curve, rack or tyre).
    #[default]
    Plain,
    /// Lock = steady-corner angle at [`GripBalance::smart_ay_g`] + front peak slip × (1 +
    /// margin), capped at the car's lock (see the module docs). Replaces the plain curve.
    Smart,
}

/// Grip-balance lab knobs. [`GripBalance::default`] = the data as is ("before").
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GripBalance {
    /// Front tyres' peak friction scale (lateral and longitudinal `D`).
    pub front_mu: f32,
    /// Rear tyres' peak friction scale (lateral and longitudinal `D`).
    pub rear_mu: f32,
    /// Front tyres' cornering stiffness scale: the lateral curve squeezed along the slip axis
    /// (`B × k`, peak slip angle ÷ k), so the peak force is the same but comes at a smaller
    /// slip angle.
    pub front_stiffness: f32,
    /// Rear tyres' cornering stiffness scale (see [`GripBalance::front_stiffness`]).
    pub rear_stiffness: f32,
    /// Front tyres' load sensitivity scale (more = a heavily loaded front tyre loses more of
    /// its friction coefficient: push).
    pub front_load_sens: f32,
    /// Rear tyres' load sensitivity scale.
    pub rear_load_sens: f32,
    /// Front tyres' relaxation length scale (slip-angle lag).
    pub front_relax: f32,
    /// Rear tyres' lateral Magic Formula `C` override (0 = data, `sport` 1.4). Lower = the
    /// side force falls less past the peak (asymptote `sin(C·π/2)` of the peak: 1.4 → 0.81,
    /// 1.2 → 0.95), so the rear steps out progressively instead of snapping; `B` is scaled so
    /// the cornering stiffness `B·C·D` (and the linear-range balance) is unchanged.
    pub rear_lat_c: f32,
    /// Front tyres' lateral Magic Formula `C` override (0 = data); as
    /// [`GripBalance::rear_lat_c`].
    pub front_lat_c: f32,
    /// Front anti-roll bar scale (after the ride lab's). Less front bar = less front lateral
    /// load transfer = more front grip.
    pub front_arb: f32,
    /// Rear anti-roll bar scale (after the ride lab's). More rear bar = looser.
    pub rear_arb: f32,
    /// Wheel ground-probe width scale (1 = the vehicle's probe, 0.8 × the tyre width). The
    /// probe cylinder rolls with the body, so in a corner it touches the road on its edge and
    /// the tyre load chatters tick to tick (≈ 0.6–1 kN RMS at 0.9 g on every wheel, none when
    /// straight); a narrow probe (0.01) removes the chatter. Lab A/B for the vehicle owner.
    pub probe_width: f32,
    /// Keyboard lock at speed (plain profile only).
    pub lock_mode: LockMode,
    /// Smart lock: the lateral acceleration whose steady-corner steer angle is allowed on top
    /// of the front peak slip, g.
    pub smart_ay_g: f32,
    /// Smart lock: front slip allowance past the tyre's peak slip angle, fraction (0.3 = 30 %
    /// past the peak, where the `sport` curve still gives ~98 % of its peak).
    pub smart_margin: f32,
    /// Smart lock: steering **toward the slide** (countersteer: the wheels turned toward the
    /// side the car is sliding to) gets the plain profile's lock back, faded in with body slip from
    /// this angle, degrees ...
    pub smart_slide_from_deg: f32,
    /// ... to this one (all of it), degrees. So drifts keep their countersteer lock while grip
    /// driving and turn-in are limited to what the fronts can use.
    pub smart_slide_full_deg: f32,
    /// Smart lock: the plain lock comes back in a slide for **both** directions (into the
    /// turn as well as countersteer), with the same body-slip fade.
    pub smart_fade_both: bool,
    /// Smart lock, **track** cap: the wheels never turn into the corner further than the
    /// front axle's direction of travel + the front peak slip × (1 + margin), i.e. the fronts
    /// stay at their peak however the car is rotating (a driver's hands at the limit; the
    /// reference's wheel follows the travel, clip 8). Faded out in slides by
    /// [`GripBalance::track_from_deg`] .. [`GripBalance::track_full_deg`] of body slip, so a
    /// drift steers as without it. Countersteer is never capped.
    pub smart_track: bool,
    /// Body slip where the track cap starts to fade out, degrees.
    pub track_from_deg: f32,
    /// Body slip where the track cap is gone, degrees.
    pub track_full_deg: f32,
    /// The track cap never asks for less than this into-turn angle, degrees (so the key always
    /// turns the wheels a little into the corner).
    pub track_floor_deg: f32,
}

impl Default for GripBalance {
    fn default() -> Self {
        Self {
            front_mu: 1.0,
            rear_mu: 1.0,
            front_stiffness: 1.0,
            rear_stiffness: 1.0,
            front_load_sens: 1.0,
            rear_load_sens: 1.0,
            front_relax: 1.0,
            rear_lat_c: 0.0,
            front_lat_c: 0.0,
            front_arb: 1.0,
            rear_arb: 1.0,
            probe_width: 1.0,
            lock_mode: LockMode::Plain,
            smart_ay_g: 1.0,
            smart_margin: 0.3,
            smart_slide_from_deg: 3.0,
            smart_slide_full_deg: 8.0,
            smart_fade_both: false,
            smart_track: false,
            track_from_deg: 10.0,
            track_full_deg: 20.0,
            track_floor_deg: 1.0,
        }
    }
}

impl GripBalance {
    /// The lab's "new" balance (docs/integration/grip-balance.md, step 1): the smart lock
    /// with the data's tyres; countersteer gets the plain lock back from 8° to 15° of body
    /// slip (lower thresholds over-correct a 100 km/h lane change into a spin).
    pub fn lab_default() -> Self {
        Self {
            lock_mode: LockMode::Smart,
            smart_slide_from_deg: 8.0,
            smart_slide_full_deg: 15.0,
            ..Self::default()
        }
    }

    /// Whether this set changes the tyres at all.
    pub fn changes_tyres(&self) -> bool {
        [
            self.front_mu,
            self.rear_mu,
            self.front_stiffness,
            self.rear_stiffness,
            self.front_load_sens,
            self.rear_load_sens,
            self.front_relax,
        ]
        .iter()
        .any(|k| (k - 1.0).abs() > 1e-6)
            || self.rear_lat_c > 0.0
            || self.front_lat_c > 0.0
    }

    /// `tire` with the scales of corner `i` (0 FL, 1 FR, 2 RL, 3 RR) applied in place.
    pub fn apply_tire(&self, tire: &mut TireDef, i: usize) {
        let front = i < 2;
        let (mu, stiff, ls) = if front {
            (self.front_mu, self.front_stiffness, self.front_load_sens)
        } else {
            (self.rear_mu, self.rear_stiffness, self.rear_load_sens)
        };
        let mu = mu.clamp(0.3, 2.0);
        tire.lateral.d *= mu;
        tire.longitudinal.d *= mu;
        let k = stiff.clamp(0.2, 5.0);
        tire.lateral.b *= k;
        tire.peak_slip_angle_rad /= k;
        tire.load_sensitivity *= ls.clamp(0.0, 5.0);
        if front {
            tire.relaxation_length_m *= self.front_relax.clamp(0.05, 10.0);
        }
        let c = if front {
            self.front_lat_c
        } else {
            self.rear_lat_c
        };
        if c > 0.0 {
            // Keep the cornering stiffness B·C·D while changing the tail past the peak.
            let c = c.clamp(1.0, 2.0);
            tire.lateral.b *= tire.lateral.c / c;
            tire.lateral.c = c;
        }
    }

    /// Anti-roll bar scale of the front (`front = true`) or rear axle.
    pub fn arb_scale(&self, front: bool) -> f32 {
        if front {
            self.front_arb.max(0.0)
        } else {
            self.rear_arb.max(0.0)
        }
    }

    /// Smart lock at `speed` (m/s), rad: `L · a / v² + peak × (1 + margin)`, capped at
    /// `plain_lock` (the plain profile's lock at this speed). `None` unless
    /// [`LockMode::Smart`]. `toward_slide` (the wheels point, or the key asks for, the side the
    /// car slides to) and `body_slip` (rad) bring the plain lock back for countersteer (see
    /// [`GripBalance::smart_slide_from_deg`]), so a drift steers exactly as without it.
    pub fn smart_lock(
        &self,
        speed: f32,
        wheelbase: f32,
        front_peak_slip: f32,
        plain_lock: f32,
        toward_slide: bool,
        body_slip: f32,
    ) -> Option<f32> {
        if self.lock_mode != LockMode::Smart {
            return None;
        }
        let v2 = (speed * speed).max(1.0);
        let geo = wheelbase * self.smart_ay_g.max(0.0) * wp_contracts::GRAVITY_MPS2 / v2;
        let tyre = front_peak_slip * (1.0 + self.smart_margin.max(0.0));
        let smart = (geo + tyre).min(plain_lock);
        if !toward_slide && !self.smart_fade_both {
            return Some(smart);
        }
        let (lo, hi) = (
            self.smart_slide_from_deg,
            self.smart_slide_full_deg
                .max(self.smart_slide_from_deg + 0.1),
        );
        let x = ((body_slip.abs().to_degrees() - lo) / (hi - lo)).clamp(0.0, 1.0);
        Some(smart + (plain_lock - smart) * x * x * (3.0 - 2.0 * x))
    }
}

impl GripBalance {
    /// The track cap (see [`GripBalance::smart_track`]) applied to centre angle `angle` (rad, +
    /// = left) with the front axle's travel `front_axle` (rad, + = left), the front peak slip
    /// and the body slip (rad). Countersteer and angles inside the cap are unchanged.
    pub fn track_cap(
        &self,
        angle: f32,
        front_axle: f32,
        front_peak_slip: f32,
        body_slip: f32,
    ) -> f32 {
        if self.lock_mode != LockMode::Smart || !self.smart_track || angle == 0.0 {
            return angle;
        }
        let (lo, hi) = (
            self.track_from_deg,
            self.track_full_deg.max(self.track_from_deg + 0.1),
        );
        let x = ((body_slip.abs().to_degrees() - lo) / (hi - lo)).clamp(0.0, 1.0);
        let keep = 1.0 - x * x * (3.0 - 2.0 * x);
        if keep <= 0.0 {
            return angle;
        }
        let a = front_peak_slip * (1.0 + self.smart_margin.max(0.0));
        let floor = self.track_floor_deg.max(0.0).to_radians();
        // Into the turn = the side the wheel points to; the cap is travel + peak on that side.
        let s = angle.signum();
        let cap = (s * front_axle + a).max(floor);
        let capped = s * (s * angle).min(cap);
        angle + keep * (capped - angle)
    }
}

/// Sets for the example's one-knob sweep (`examples/grip_balance.rs -- sweep`).
pub fn sweep_sets() -> Vec<(String, GripBalance)> {
    let d = GripBalance::default();
    let smart = GripBalance {
        lock_mode: LockMode::Smart,
        ..d
    };
    let mut v: Vec<(String, GripBalance)> = vec![("smart lock".into(), smart)];
    for m in [0.0f32, 0.6, 1.0] {
        v.push((
            format!("smart lock, margin {m}"),
            GripBalance {
                smart_margin: m,
                ..smart
            },
        ));
    }
    for (name, g) in [
        ("rear μ 0.92", GripBalance { rear_mu: 0.92, ..d }),
        (
            "front μ 1.08",
            GripBalance {
                front_mu: 1.08,
                ..d
            },
        ),
        (
            "rear stiffness 0.7",
            GripBalance {
                rear_stiffness: 0.7,
                ..d
            },
        ),
        (
            "front stiffness 1.3",
            GripBalance {
                front_stiffness: 1.3,
                ..d
            },
        ),
        (
            "front load sens 0",
            GripBalance {
                front_load_sens: 0.0,
                ..d
            },
        ),
        (
            "rear load sens 2.5",
            GripBalance {
                rear_load_sens: 2.5,
                ..d
            },
        ),
        (
            "front bar 0, rear bar ×20",
            GripBalance {
                front_arb: 0.0,
                rear_arb: 20.0,
                ..d
            },
        ),
    ] {
        v.push((name.into(), g));
        v.push((
            format!("smart + {name}"),
            GripBalance {
                lock_mode: LockMode::Smart,
                ..g
            },
        ));
    }
    v
}

static OVERRIDE: RwLock<Option<GripBalance>> = RwLock::new(None);

/// Set (`Some`) or clear (`None`) the grip-balance lab override for every vehicle in this
/// process. Testbed Drift Lab W and its measurements only; see the module docs.
pub fn set_override(g: Option<GripBalance>) {
    if let Ok(mut o) = OVERRIDE.write() {
        *o = g;
    }
}

/// The grip-balance override in effect, or `None` (the data as is).
pub fn active() -> Option<GripBalance> {
    OVERRIDE.read().ok().and_then(|o| *o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_changes_nothing_and_smart_lock_caps_at_speed() {
        let d = GripBalance::default();
        assert!(!d.changes_tyres());
        assert_eq!(d.smart_lock(30.0, 2.7, 0.109, 0.66, false, 0.0), None);
        let s = GripBalance::lab_default();
        // Parking speed: the car's full lock.
        let lock = s.smart_lock(2.0, 2.7, 0.109, 0.66, false, 0.0).unwrap();
        assert!((lock - 0.66).abs() < 1e-6);
        // 60 mph: ≈ 2° of geometry + 1.3 × 6.2° of tyre ≈ 10°.
        let lock = s
            .smart_lock(26.8, 2.7, 0.109, 0.66, false, 0.0)
            .unwrap()
            .to_degrees();
        assert!((9.0..11.5).contains(&lock), "{lock}");
        // Countersteer in a 20° slide: the plain lock again; into the turn: still limited ...
        let ctr = s.smart_lock(26.8, 2.7, 0.109, 0.66, true, 0.35).unwrap();
        assert!((ctr - 0.66).abs() < 1e-6);
        let into = s.smart_lock(26.8, 2.7, 0.109, 0.66, false, 0.35).unwrap();
        assert!(into.to_degrees() < 11.5);
        // ... unless the fade applies both ways.
        let both = GripBalance {
            smart_fade_both: true,
            ..s
        };
        let into = both
            .smart_lock(26.8, 2.7, 0.109, 0.66, false, 0.35)
            .unwrap();
        assert!((into - 0.66).abs() < 1e-6);
    }
}
