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

use wp_contracts::{DriveLayout, TireDef};

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
    /// The tyre scales (front and rear: μ, cornering stiffness, load sensitivity, relaxation,
    /// past-peak C) apply to rear-wheel-drive cars only, so front-drive cars keep their tyres
    /// (and their mild understeer); the smart lock and the bar scales apply to every car.
    pub tyres_rwd_only: bool,
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
    /// Extra front downforce, C·A in m² (force = ½ρv² · C·A at the front axle; 0 = data).
    pub front_downforce: f32,
    /// Extra rear downforce, C·A in m² (0 = data). Grows with v²: a rear-stabilising term that
    /// leaves low-speed balance alone (≈ 0.25 kN at 60 mph, 0.75 kN at 100 mph per 0.6 m²).
    pub rear_downforce: f32,
    /// The ground-probe **tilt fix** ([`crate::ground::discount_tilt`]): the sweep no longer
    /// wins on smooth ground just because the body-fixed probe cylinder is tilted, which
    /// removes the corner load chatter at the full probe width (kerbs still lift the sweep).
    /// `false` = bit-identical to before. Proposed as the default for the vehicle owner.
    pub probe_tilt_fix: bool,
    /// Keyboard lock at speed (plain profile only).
    pub lock_mode: LockMode,
    /// Smart lock: the lateral acceleration whose steady-corner steer angle is allowed on top
    /// of the front peak slip, g.
    pub smart_ay_g: f32,
    /// Smart lock: front slip allowance past the tyre's peak slip angle, fraction (0.3 = 30 %
    /// past the peak, where the `sport` curve still gives ~98 % of its peak).
    pub smart_margin: f32,
    /// Smart lock: the margin at high speed (blended from [`GripBalance::smart_margin`] between
    /// [`GripBalance::smart_hi_from_kmh`] and [`GripBalance::smart_hi_full_kmh`]). More margin
    /// at speed = the fronts a little past their peak = the held-key stability a car needs at
    /// 100 mph (with the fronts at their peak the rear has no yaw moment to spare).
    pub smart_margin_hi: f32,
    /// Speed where the high-speed margin starts to blend in, km/h.
    pub smart_hi_from_kmh: f32,
    /// Speed where the high-speed margin is fully in, km/h.
    pub smart_hi_full_kmh: f32,
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
    /// Smart lock timing: `false` (default) = the rack takes its full-lock time (0.28 s) to
    /// reach the smart lock, so a short tap gives a smaller angle than before; `true` = the
    /// rack keeps the plain profile's angle rate and stops at the smart lock (a 0.1 s tap
    /// turns the wheels as far as before, the lock is reached sooner).
    pub smart_angle_rate: bool,
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
    /// Smart lock, held-key drift hold (lead 2026-10-21: the owner's clip-1 technique, the
    /// countersteer key held + full throttle, over-caught and swung through): the countersteer
    /// lock in a slide is the plain lock × this, phased in over 10° of body slip from
    /// [`GripBalance::slide_lock_from_deg`]. 1 = the plain lock (off). Countersteer only, so
    /// grip driving (below the slip threshold) is unchanged.
    pub slide_lock: f32,
    /// Body slip where [`GripBalance::slide_lock`] starts to phase in, degrees.
    pub slide_lock_from_deg: f32,
    /// Directional slide lock: > 0 = [`GripBalance::slide_lock`] only while the slide is
    /// **closing**, fully at this rate of |body slip| decrease, °/s (a growing slide keeps the
    /// full countersteer lock to catch it). 0 = always.
    pub slide_closing_dps: f32,
    /// Held-key slide lock: > 0 = [`GripBalance::slide_lock`] only once the countersteer key
    /// has been held this long, s (phased in over the next 0.3 s), so quick catching taps keep
    /// the full lock and only a held key (the owner's clip-1 technique) gets less. 0 = always.
    pub slide_lock_after_s: f32,
    /// Slide growth, °/s, from which [`GripBalance::slide_lock`] lets go (from half this, fully
    /// at this): an over-rotation still growing keeps the full countersteer lock (and Drift Lab
    /// W's drift lock extension, `PlainSteer::over_from_dps`, the same 15 °/s), so the held-key
    /// cut never cancels a catch. 0 = never.
    pub slide_lock_growth_dps: f32,
    /// Body slip, degrees, above which [`GripBalance::slide_lock`] fades out (over the next
    /// 10°): past a normal drift's angle (an over-rotation) the full countersteer lock and
    /// Drift Lab W's drift lock extension (from 40°) do the catching. 0 = no ceiling.
    pub slide_lock_to_deg: f32,
    /// Transition release (lead, 2026-10-21): within this many seconds of a throttle lift
    /// mid-slide or a steering-key reversal the held-key cut is off (faded back in over the
    /// window's last 0.2 s), so a swing's new-side countersteer can reach the travel angle;
    /// a steady held drift (no recent lift / reversal) keeps the cut. 0 = off.
    pub slide_lock_release_s: f32,
    /// The transition release also opens on a throttle lift mid-slide (else only on a key
    /// reversal). Off by default: a lift with the counter key held is W's snap-back exit,
    /// which the cut must keep (with it on, the snap-back overshoot is 3° → 42°).
    pub slide_lock_release_on_lift: bool,
    /// Apply the lab-only per-car fixes ([`car_fix`]): car-data proposals for the car-data
    /// owner, tried in the lab first (lead-approved 2026-10-21).
    pub car_fixes: bool,
    /// Front-drive cars only (FWD, not AWD; whatever [`GripBalance::tyres_rwd_only`] says):
    /// rear tyres' peak friction scale (lead 2026-10-21: the FWD cars are the furthest from the
    /// owner's shiro_turbo / rampart_50 feel; less rear grip = more rotation on lift-off).
    pub fwd_rear_mu: f32,
    /// Front-drive cars only: rear cornering stiffness scale (as [`GripBalance::rear_stiffness`]).
    pub fwd_rear_stiffness: f32,
    /// Front-drive cars only: rear anti-roll bar scale (on top of [`GripBalance::rear_arb`]).
    pub fwd_rear_arb: f32,
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
            tyres_rwd_only: false,
            front_arb: 1.0,
            rear_arb: 1.0,
            probe_width: 1.0,
            front_downforce: 0.0,
            rear_downforce: 0.0,
            probe_tilt_fix: false,
            lock_mode: LockMode::Plain,
            smart_ay_g: 1.0,
            smart_margin: 0.3,
            smart_margin_hi: 0.3,
            smart_hi_from_kmh: 100.0,
            smart_hi_full_kmh: 160.0,
            smart_slide_from_deg: 3.0,
            smart_slide_full_deg: 8.0,
            smart_fade_both: false,
            smart_angle_rate: false,
            smart_track: false,
            track_from_deg: 10.0,
            track_full_deg: 20.0,
            track_floor_deg: 1.0,
            fwd_rear_mu: 1.0,
            fwd_rear_stiffness: 1.0,
            fwd_rear_arb: 1.0,
            slide_lock: 1.0,
            slide_lock_from_deg: 15.0,
            slide_closing_dps: 0.0,
            slide_lock_after_s: 0.0,
            slide_lock_growth_dps: 0.0,
            slide_lock_to_deg: 0.0,
            slide_lock_release_s: 0.0,
            slide_lock_release_on_lift: false,
            car_fixes: false,
        }
    }
}

impl GripBalance {
    /// The lab's "new" balance (docs/integration/grip-balance.md; step 20, "lift-off"):
    /// - the smart lock: a 0.7 g corner angle + front peak slip × 0.7 (margin −0.3) up to
    ///   100 km/h, the margin rising to +0.3 by 160 km/h; countersteer gets the plain lock back
    ///   from 8° to 15° of body slip;
    /// - on rear-wheel-drive cars, front cornering stiffness × 0.6 (the smart lock's size and
    ///   turn-in) and the rear close to the data: stiffness × 0.9 and a flatter side force past
    ///   the peak (shape C 1.2, cornering stiffness kept), so a lift while steering does not
    ///   step the rear out (owner, step 18) and over-rotations stay catchable;
    /// - rear downforce C·A 0.8 m², the probe tilt fix;
    /// - front-drive cars: the data tyres with the rear anti-roll bar × 2;
    /// - the held-key slide lock and the lab car fixes.
    ///
    /// Tuned on the "firm" ride (`ride_lab::RideTune::firm`): with W's soft medium-weight bars
    /// (× 0.15) the rear still steps out on a lift.
    pub fn lab_default() -> Self {
        Self {
            lock_mode: LockMode::Smart,
            smart_ay_g: 0.7,
            smart_margin: -0.3,
            smart_margin_hi: 0.3,
            smart_slide_from_deg: 8.0,
            smart_slide_full_deg: 15.0,
            front_stiffness: 0.6,
            rear_stiffness: 0.9,
            rear_lat_c: 1.2,
            tyres_rwd_only: true,
            rear_downforce: 0.8,
            probe_tilt_fix: true,
            // Front-drive cars: the rear bar × 2 (on the firm ride the soft ride's looser FWD
            // rear tyres, 0.97 / 0.7, stepped out on a lift).
            fwd_rear_arb: 2.0,
            // Held countersteer key + full throttle (the owner's clip-1 technique) over-caught
            // and swung through or spun: 0 / 27 held. Once the countersteer key has been held
            // 0.5 s, its lock past 15–25° of body slip × 0.5: 19 / 27, while a key-tapping
            // driver's catches keep the full lock (docs/integration/grip-balance.md, "Held-key
            // hold"). Grip driving never reaches 15° of slip, so it is unchanged.
            slide_lock: 0.5,
            slide_lock_from_deg: 15.0,
            slide_lock_after_s: 0.5,
            // ... except while the slide still grows (W's over-rotation catch, step 11).
            slide_lock_growth_dps: 15.0,
            slide_lock_to_deg: 40.0,
            // ... and for 0.75 s after a steering-key reversal (a transition's new-side
            // countersteer may reach the travel angle; not on a lift: W's snap-back exit).
            slide_lock_release_s: 0.75,
            car_fixes: true,
            ..Self::default()
        }
    }

    /// The step-14 … step-19 "new" (one click away, "balance: step 14"): the smart lock, RWD
    /// cornering stiffness × 0.6 front / × 0.45 rear (same peak grip at bigger slip angles, so
    /// the car rotates at the limit), rear downforce C·A 0.8 m², the probe tilt fix, the FWD rear
    /// knobs of the soft ride, the held-key slide lock and the lab car fixes. The soft rear steps
    /// out on a lift while steering (11 / 18 of tyre-slip's lift set; owner, step 18: "no side
    /// grip"), which [`GripBalance::lab_default`] fixes.
    pub fn step14() -> Self {
        Self {
            lock_mode: LockMode::Smart,
            smart_ay_g: 0.7,
            smart_margin: -0.3,
            smart_margin_hi: 0.3,
            smart_slide_from_deg: 8.0,
            smart_slide_full_deg: 15.0,
            front_stiffness: 0.6,
            rear_stiffness: 0.45,
            tyres_rwd_only: true,
            rear_downforce: 0.8,
            probe_tilt_fix: true,
            // Front-drive cars (lead 2026-10-21: the FWD cars were the furthest from the
            // owner's shiro_turbo / rampart_50 feel — no lift-off rotation, 2× the slalom roll
            // rate): a looser, stiffer-barred rear. RWD and AWD cars are unchanged.
            fwd_rear_mu: 0.97,
            fwd_rear_stiffness: 0.7,
            fwd_rear_arb: 2.0,
            // Held countersteer key + full throttle (the owner's clip-1 technique) over-caught
            // and swung through or spun: 0 / 27 held. Once the countersteer key has been held
            // 0.5 s, its lock past 15–25° of body slip × 0.5: 19 / 27, while a key-tapping
            // driver's catches keep the full lock (docs/integration/grip-balance.md, "Held-key
            // hold"). Grip driving never reaches 15° of slip, so it is unchanged.
            slide_lock: 0.5,
            slide_lock_from_deg: 15.0,
            slide_lock_after_s: 0.5,
            // ... except while the slide still grows (W's over-rotation catch, step 11).
            slide_lock_growth_dps: 15.0,
            slide_lock_to_deg: 40.0,
            car_fixes: true,
            ..Self::default()
        }
    }

    /// "balance: grip" (lead, 2026-10-20: "grip-drivable below the tyres' limit"): the smart
    /// lock of [`Self::lab_default`] with the RWD tyres close to the data in the linear range
    /// (front cornering stiffness × 0.9, rear × 0.8 instead of 0.6 / 0.45) and the limit
    /// looseness from the peak instead: rear μ × 0.96, a flatter rear side force past the peak
    /// (shape C 1.0 instead of 1.4, cornering stiffness kept) so the rear steps out
    /// progressively, and rear downforce 0.8 m² for speed. Body slip stays under ~1° to 0.6 g
    /// and reaches 4–6° at the limit; smart lock margin −0.1 → +0.6 (docs/integration/
    /// grip-balance.md, "Balance: grip").
    pub fn grip() -> Self {
        Self {
            front_stiffness: 0.9,
            rear_stiffness: 0.8,
            rear_mu: 0.96,
            rear_lat_c: 1.0,
            // The stiffer fronts peak at a smaller slip angle, so the lock allows a little
            // more past their peak to keep "new"'s wheel angle and turn-in (0.4 s tap at
            // 60 mph: 24.6 vs 24.9 °/s).
            smart_margin: -0.1,
            smart_margin_hi: 0.6,
            ..Self::lab_default()
        }
    }

    /// The balance to pair with Drift Lab W's "slow weight (clip 4)" ride preset
    /// (`crate::ride_lab::RideTune::slow_weight`). Slow weight (no anti-roll bars, softer
    /// springs, more yaw inertia) already loosens the cars, and on top of [`Self::lab_default`]'s
    /// softer tyres a held key spins at 60 mph. So: the smart lock (margin +0.3, rising to
    /// +1.0 by 160 km/h) + rear downforce 0.8 m² + the probe fix, with the data's tyres.
    /// parity_rwd_d + slow weight: 3.2° body slip at 60 mph, 2.9° at 100 mph (stable), drift
    /// robustness 6 / 27 (docs/integration/grip-balance.md).
    pub fn for_slow_weight() -> Self {
        Self {
            lock_mode: LockMode::Smart,
            smart_margin: 0.3,
            smart_margin_hi: 1.0,
            smart_slide_from_deg: 8.0,
            smart_slide_full_deg: 15.0,
            rear_downforce: 0.8,
            probe_tilt_fix: true,
            ..Self::default()
        }
    }

    /// "drift: best (test)" (lead, with tyre-slip's measurements): the smart lock alone
    /// ([`Self::lock_only`]) + rear downforce 0.8 m² + RWD rear cornering stiffness × 0.45. With
    /// tyre-slip's tuned velocity-relative steering (the panel button sets both) it held 20 / 27
    /// of the drift robustness set, the best so far (docs/integration/grip-balance.md).
    pub fn drift_best() -> Self {
        Self {
            rear_downforce: 0.8,
            rear_stiffness: 0.45,
            tyres_rwd_only: true,
            ..Self::lock_only()
        }
    }

    /// Step 2's "new" balance (kept for the A/B): smart lock (margin −0.3 → +1.0 between 100
    /// and 160 km/h), RWD cornering stiffness × 0.75 / × 0.65, no downforce, no probe fix.
    pub fn step2() -> Self {
        Self {
            lock_mode: LockMode::Smart,
            smart_margin: -0.3,
            smart_margin_hi: 1.0,
            smart_slide_from_deg: 8.0,
            smart_slide_full_deg: 15.0,
            front_stiffness: 0.75,
            rear_stiffness: 0.65,
            tyres_rwd_only: true,
            ..Self::default()
        }
    }

    /// Step 1's "new" balance (docs/integration/grip-balance.md): the smart lock alone
    /// (margin +0.3 at every speed) with the data's tyres.
    pub fn lock_only() -> Self {
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
            self.fwd_rear_mu,
            self.fwd_rear_stiffness,
        ]
        .iter()
        .any(|k| (k - 1.0).abs() > 1e-6)
            || self.rear_lat_c > 0.0
            || self.front_lat_c > 0.0
    }

    /// `tire` with the scales of corner `i` (0 FL, 1 FR, 2 RL, 3 RR) applied in place for a car
    /// of drive `layout` (see [`GripBalance::tyres_rwd_only`] and the `fwd_*` knobs).
    pub fn apply_tire(&self, tire: &mut TireDef, i: usize, layout: &DriveLayout) {
        let front = i < 2;
        let rwd = matches!(layout, DriveLayout::Rwd);
        if !front && matches!(layout, DriveLayout::Fwd) {
            let mu = self.fwd_rear_mu.clamp(0.3, 2.0);
            tire.lateral.d *= mu;
            tire.longitudinal.d *= mu;
            let k = self.fwd_rear_stiffness.clamp(0.2, 5.0);
            tire.lateral.b *= k;
            tire.peak_slip_angle_rad /= k;
        }
        if self.tyres_rwd_only && !rwd {
            return;
        }
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

    /// Anti-roll bar scale of the front (`front = true`) or rear axle of a car of drive
    /// `layout` ([`GripBalance::fwd_rear_arb`] on front-drive cars).
    pub fn arb_scale(&self, front: bool, layout: &DriveLayout) -> f32 {
        if front {
            self.front_arb.max(0.0)
        } else if matches!(layout, DriveLayout::Fwd) {
            self.rear_arb.max(0.0) * self.fwd_rear_arb.max(0.0)
        } else {
            self.rear_arb.max(0.0)
        }
    }

    /// Smart lock margin at `speed` (m/s): [`GripBalance::smart_margin`] blended to
    /// [`GripBalance::smart_margin_hi`] (linear in speed).
    pub fn margin_at(&self, speed: f32) -> f32 {
        let kmh = speed.abs() * 3.6;
        let (lo, hi) = (
            self.smart_hi_from_kmh,
            self.smart_hi_full_kmh.max(self.smart_hi_from_kmh + 1.0),
        );
        let x = ((kmh - lo) / (hi - lo)).clamp(0.0, 1.0);
        self.smart_margin + x * (self.smart_margin_hi - self.smart_margin)
    }

    /// Smart lock at `speed` (m/s), rad: `L · a / v² + peak × (1 + margin)`, capped at
    /// `plain_lock` (the plain profile's lock at this speed). `None` unless
    /// [`LockMode::Smart`]. In `slide` ([`LockSlide`]), `toward_slide` (the wheels point, or the
    /// key asks for, the side the car slides to) and `body_slip` (rad) bring the plain lock back for countersteer (see
    /// [`GripBalance::smart_slide_from_deg`]), so a drift steers exactly as without it.
    /// `slip_rate` (rad/s, rate of |body slip|, − = closing) and `counter_s` (how long the
    /// countersteer key has been held, s) feed the slide lock ([`GripBalance::slide_closing_dps`],
    /// [`GripBalance::slide_lock_after_s`]).
    pub fn smart_lock(
        &self,
        speed: f32,
        wheelbase: f32,
        front_peak_slip: f32,
        plain_lock: f32,
        slide: LockSlide,
    ) -> Option<f32> {
        let LockSlide {
            toward_slide,
            body_slip,
            slip_rate,
            counter_s,
            since_lift_s,
            since_reversal_s,
        } = slide;
        let since_release_s = if self.slide_lock_release_on_lift {
            since_lift_s.min(since_reversal_s)
        } else {
            since_reversal_s
        };
        if self.lock_mode != LockMode::Smart {
            return None;
        }
        let v2 = (speed * speed).max(1.0);
        let geo = wheelbase * self.smart_ay_g.max(0.0) * wp_contracts::GRAVITY_MPS2 / v2;
        let tyre = front_peak_slip * (1.0 + self.margin_at(speed)).max(0.2);
        let smart = (geo + tyre).min(plain_lock);
        if !toward_slide && !self.smart_fade_both {
            return Some(smart);
        }
        let (lo, hi) = (
            self.smart_slide_from_deg,
            self.smart_slide_full_deg
                .max(self.smart_slide_from_deg + 0.1),
        );
        let slip = body_slip.abs().to_degrees();
        let x = ((slip - lo) / (hi - lo)).clamp(0.0, 1.0);
        // Held-key hold: less countersteer lock deep in a slide (see `slide_lock`).
        let target = if toward_slide && self.slide_lock != 1.0 {
            let y = ((slip - self.slide_lock_from_deg) / 10.0).clamp(0.0, 1.0);
            // Directional: only while |slip| shrinks (slip_rate < 0, rad/s).
            let closing = if self.slide_closing_dps > 0.0 {
                (-slip_rate.to_degrees() / self.slide_closing_dps).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let held = if self.slide_lock_after_s > 0.0 {
                let t = ((counter_s - self.slide_lock_after_s) / 0.3).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            } else {
                1.0
            };
            let growing = if self.slide_lock_growth_dps > 0.0 {
                let g = self.slide_lock_growth_dps;
                let t = ((slip_rate.to_degrees() - 0.5 * g) / (0.5 * g)).clamp(0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            } else {
                1.0
            };
            let below = if self.slide_lock_to_deg > 0.0 {
                let t = ((slip - self.slide_lock_to_deg) / 10.0).clamp(0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            } else {
                1.0
            };
            let settled = if self.slide_lock_release_s > 0.0 {
                let t =
                    ((since_release_s - (self.slide_lock_release_s - 0.2)) / 0.2).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            } else {
                1.0
            };
            let k = 1.0
                + (self.slide_lock.clamp(0.3, 1.5) - 1.0)
                    * settled
                    * growing
                    * below
                    * y
                    * y
                    * (3.0 - 2.0 * y)
                    * closing
                    * held;
            plain_lock * k
        } else {
            plain_lock
        };
        Some(smart + (target - smart) * x * x * (3.0 - 2.0 * x))
    }
}

/// The slide inputs of [`GripBalance::smart_lock`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LockSlide {
    /// The wheels point, or the key asks for, the side the car slides to (countersteer).
    pub toward_slide: bool,
    /// Body slip, rad.
    pub body_slip: f32,
    /// Rate of |body slip|, rad/s (− = the slide closing).
    pub slip_rate: f32,
    /// How long the countersteer key has been held, s.
    pub counter_s: f32,
    /// Time since the last throttle lift mid-slide, s (tyre-slip's transition timer; large =
    /// none recent).
    pub since_lift_s: f32,
    /// Time since the last steering-key reversal, s (large = none recent).
    pub since_reversal_s: f32,
}

/// A lab-only per-car fix ([`car_fix`]): scales on the car data, applied by the vehicle while
/// the grip-balance lab is on with [`GripBalance::car_fixes`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarFix {
    /// Front anti-roll bar scale.
    pub arb_front: f32,
    /// Rear anti-roll bar scale.
    pub arb_rear: f32,
    /// Rear spring rate scale (the preload is raised so the static ride height stays).
    pub spring_rear: f32,
    /// Body yaw inertia scale (applied when the car's body is built).
    pub yaw_inertia: f32,
}

/// The lab's per-car fixes, by car id (docs/integration/grip-balance.md, "Feel references";
/// proposals for the car data, not applied to `data/`):
/// - `debug_coupe`: both anti-roll bars × 0.5 (the owner: "slide transition is very violent");
/// - `lucciola_350`: rear springs × 0.65 and yaw inertia × 1.2 (rear-heavy, 13° lane change).
pub fn car_fix(id: &str) -> Option<CarFix> {
    let none = CarFix {
        arb_front: 1.0,
        arb_rear: 1.0,
        spring_rear: 1.0,
        yaw_inertia: 1.0,
    };
    match id {
        "debug_coupe" => Some(CarFix {
            arb_front: 0.5,
            arb_rear: 0.5,
            ..none
        }),
        "lucciola_350" => Some(CarFix {
            spring_rear: 0.65,
            yaw_inertia: 1.2,
            ..none
        }),
        _ => None,
    }
}

impl GripBalance {
    /// The per-car fix for car `id` when [`GripBalance::car_fixes`] is on.
    pub fn fix_for(&self, id: &str) -> Option<CarFix> {
        if self.car_fixes {
            car_fix(id)
        } else {
            None
        }
    }

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
        let a = front_peak_slip * (1.0 + self.smart_margin).max(0.2);
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
        assert!(GripBalance::lab_default().changes_tyres());
        // The tyre scales skip front-drive cars when tyres_rwd_only is set.
        let mut t: TireDef = serde_json::from_str(
            r#"{"id":"t","lateral":{"b":17,"c":1.4,"d":1.1,"e":-0.3},"longitudinal":{"b":15,"c":1.65,"d":1.2,"e":0.2},
            "load_sensitivity":0.13,"fz0_n":4000,"relaxation_length_m":0.3,"rolling_resistance":0.011,
            "peak_slip_angle_rad":0.109,"peak_slip_ratio":0.101,"surface_multipliers":{},"wear_rate":0}"#,
        )
        .unwrap();
        // "new" on a front-drive car: its FWD knobs (the rear bar only: tyres unchanged), not
        // the RWD tyre scales; on a rear-drive car: the RWD rear (× 0.9, shape C 1.2).
        let mut fwd = t.clone();
        GripBalance::lab_default().apply_tire(&mut fwd, 2, &DriveLayout::Fwd);
        assert_eq!(fwd.lateral.b, 17.0);
        let mut front = t.clone();
        GripBalance::lab_default().apply_tire(&mut front, 0, &DriveLayout::Fwd);
        assert_eq!(front.lateral.b, 17.0);
        GripBalance::lab_default().apply_tire(&mut t, 2, &DriveLayout::Rwd);
        // Stiffness × 0.9, shape C 1.4 → 1.2 with the cornering stiffness B·C kept.
        assert!(
            (t.lateral.b - 17.0 * 0.9 * 1.4 / 1.2).abs() < 1e-3,
            "{}",
            t.lateral.b
        );
        let mut s14 = front.clone();
        GripBalance::step14().apply_tire(&mut s14, 0, &DriveLayout::Rwd);
        assert!((s14.lateral.b - 17.0 * 0.6).abs() < 1e-4);
        // The FWD knobs touch front-drive rears only.
        let f = GripBalance {
            fwd_rear_mu: 0.9,
            ..GripBalance::default()
        };
        assert!(f.changes_tyres());
        let mut r = t.clone();
        f.apply_tire(&mut r, 2, &DriveLayout::Rwd);
        assert_eq!(r.lateral.d, t.lateral.d);
        f.apply_tire(&mut r, 2, &DriveLayout::Fwd);
        assert!((r.lateral.d - 0.9 * t.lateral.d).abs() < 1e-6);
        // Per-car fixes: only the listed cars, only with `car_fixes`.
        assert!(GripBalance::lab_default().fix_for("debug_coupe").is_some());
        assert!(GripBalance::lab_default().fix_for("shiro_turbo").is_none());
        assert!(GripBalance::default().fix_for("debug_coupe").is_none());
        // The held-key slide lock: countersteer deep in a slide, once the key is held 0.8 s
        // (past 0.5 s + the 0.3 s phase-in); a fresh tap keeps the plain lock.
        let n = GripBalance::lab_default();
        let deep = n
            .smart_lock(
                15.0,
                2.7,
                0.109,
                0.6,
                LockSlide {
                    toward_slide: true,
                    body_slip: 40f32.to_radians(),
                    slip_rate: 0.0,
                    counter_s: 0.8,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!((deep - 0.3).abs() < 1e-4, "{deep}");
        // A key reversal 0.2 s ago releases the cut (transition); 0.8 s ago it is back.
        let rel = |since: f32| {
            n.smart_lock(
                15.0,
                2.7,
                0.109,
                0.6,
                LockSlide {
                    toward_slide: true,
                    body_slip: 40f32.to_radians(),
                    slip_rate: 0.0,
                    counter_s: 0.8,
                    since_lift_s: 1.0e3,
                    since_reversal_s: since,
                },
            )
            .unwrap()
        };
        assert!((rel(0.2) - 0.6).abs() < 1e-4, "{}", rel(0.2));
        assert!((rel(0.8) - 0.3).abs() < 1e-4, "{}", rel(0.8));
        let tap = n
            .smart_lock(
                15.0,
                2.7,
                0.109,
                0.6,
                LockSlide {
                    toward_slide: true,
                    body_slip: 40f32.to_radians(),
                    slip_rate: 0.0,
                    counter_s: 0.1,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!((tap - 0.6).abs() < 1e-4, "{tap}");
        let into = n
            .smart_lock(
                15.0,
                2.7,
                0.109,
                0.6,
                LockSlide {
                    toward_slide: false,
                    body_slip: 40f32.to_radians(),
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!(into < 0.3, "{into}");
        f.apply_tire(&mut r, 0, &DriveLayout::Fwd);
        assert!((r.lateral.d - 0.9 * t.lateral.d).abs() < 1e-6);
        // The high-speed margin blends in between 100 and 160 km/h.
        let n = GripBalance::lab_default();
        assert!((n.margin_at(80.0 / 3.6) + 0.3).abs() < 1e-6);
        assert!(n.margin_at(130.0 / 3.6).abs() < 1e-4);
        assert!((n.margin_at(200.0 / 3.6) - 0.3).abs() < 1e-6);
        assert_eq!(
            d.smart_lock(
                30.0,
                2.7,
                0.109,
                0.66,
                LockSlide {
                    toward_slide: false,
                    body_slip: 0.0,
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3
                }
            ),
            None
        );
        let s = GripBalance::lab_default();
        // Parking speed: the car's full lock.
        let lock = s
            .smart_lock(
                2.0,
                2.7,
                0.109,
                0.66,
                LockSlide {
                    toward_slide: false,
                    body_slip: 0.0,
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!((lock - 0.66).abs() < 1e-6);
        // 60 mph: ≈ 1.5° of geometry (0.7 g) + 0.7 × 6.2° of tyre ≈ 5.9°.
        let lock = s
            .smart_lock(
                26.8,
                2.7,
                0.109,
                0.66,
                LockSlide {
                    toward_slide: false,
                    body_slip: 0.0,
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap()
            .to_degrees();
        assert!((5.5..7.5).contains(&lock), "{lock}");
        // Countersteer in a 17° slide (below the slide lock's 20°): the plain lock again; into
        // the turn: still limited ...
        let ctr = s
            .smart_lock(
                26.8,
                2.7,
                0.109,
                0.66,
                LockSlide {
                    toward_slide: true,
                    body_slip: 0.30,
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!((ctr - 0.66).abs() < 1e-6);
        let into = s
            .smart_lock(
                26.8,
                2.7,
                0.109,
                0.66,
                LockSlide {
                    toward_slide: false,
                    body_slip: 0.30,
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!(into.to_degrees() < 7.5);
        // ... unless the fade applies both ways.
        let both = GripBalance {
            smart_fade_both: true,
            ..s
        };
        let into = both
            .smart_lock(
                26.8,
                2.7,
                0.109,
                0.66,
                LockSlide {
                    toward_slide: false,
                    body_slip: 0.35,
                    slip_rate: 0.0,
                    counter_s: 0.0,
                    since_lift_s: 1.0e3,
                    since_reversal_s: 1.0e3,
                },
            )
            .unwrap();
        assert!((into - 0.66).abs() < 1e-6);
    }
}
