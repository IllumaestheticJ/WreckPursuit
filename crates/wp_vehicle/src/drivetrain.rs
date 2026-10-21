//! Gearbox (manual / automatic with a load-based schedule, kickdown, corner hold, rev-matched
//! clutch shifts and auto-reverse; power-on shifts behind a torque converter), clutch
//! engagement and torque converter. Differentials live in [`crate::diff`].

use wp_contracts::DrivetrainDef;

use crate::math::lerp;

/// Fraction of a clutch-gearbox shift spent releasing the clutch in the old gear (torque fades
/// out); the same fraction at the end re-engages it in the new gear (torque fills back in).
/// The middle is neutral, where the engine is rev-matched to the new gear.
pub const SHIFT_RELEASE_FRAC: f32 = 0.3;

/// Automatic shift schedule (shared by clutch and torque-converter automatics).
///
/// Shift points are fractions of the redline, interpolated by *load* (the pedal, held up by a
/// slowly falling filter, so a brief lift does not trigger an upshift):
/// upshift at [`UP_LIGHT`]…[`UP_FULL`] (linear in load), downshift at
/// [`DOWN_LIGHT`]…[`DOWN_FULL`] (by √load, so part throttle already asks for the lower
/// gear). A shift is
/// only taken when the engine speed in the new gear lands inside the band with
/// [`BAND_MARGIN`] to spare, and the opposite direction is locked for [`MIN_IN_GEAR_S`]
/// after a shift, so a constant pedal cannot make the box hunt.
pub mod schedule {
    /// Upshift point at zero load (economy), fraction of redline.
    pub const UP_LIGHT: f32 = 0.36;
    /// Upshift point at full load, fraction of redline: just below the limiter. (For every
    /// car in the roster the next gear never gives more wheel torque before the redline, so
    /// revving out is the fastest schedule.)
    pub const UP_FULL: f32 = 0.975;
    /// Downshift point at zero load (coasting), fraction of redline.
    pub const DOWN_LIGHT: f32 = 0.2;
    /// Downshift point at full load, fraction of redline.
    pub const DOWN_FULL: f32 = 0.45;
    /// Downshift point while braking (sequential, ready for the corner exit).
    pub const DOWN_BRAKING: f32 = 0.36;
    /// Highest engine speed a braking downshift may produce, fraction of redline.
    pub const BRAKING_DOWN_CEILING: f32 = 0.72;
    /// Engine speed in the new gear must clear the opposite threshold by this, fraction of
    /// redline.
    pub const BAND_MARGIN: f32 = 0.05;
    /// Above this the box upshifts whatever else holds it (limiter protection).
    pub const FORCED_UP: f32 = 0.985;
    /// Body slip past which the box counts the car as sliding and holds its gear, degrees
    /// (lead, 2026-10-21: no kickdown at a held drift's entry, no upshift from a spinning
    /// rear's revs mid-drift).
    pub const SLIDE_HOLD_DEG: f32 = 10.0;
    /// Driven-wheel slip ratio, × the tyre's peak, past which the box counts the car as
    /// sliding (wheelspin).
    pub const SLIDE_HOLD_SPIN: f32 = 1.5;
    /// Kickdown picks the lowest gear whose engine speed stays below this, fraction of redline.
    pub const KICKDOWN_CEILING: f32 = 0.85;
    /// Kickdown: pedal at least this far down…
    pub const KICKDOWN_PEDAL: f32 = 0.9;
    /// …and this far ahead of the smoothed load (a stab, not a held pedal).
    pub const KICKDOWN_STAB: f32 = 0.3;
    /// No shift in the opposite direction for this long after a shift, s.
    pub const MIN_IN_GEAR_S: f32 = 1.0;
    /// No further shift in the same direction for this long, s (sequential downshifts).
    pub const MIN_SAME_DIR_S: f32 = 0.25;
    /// Lateral tire usage above which the box holds its gear (mid-corner at the limit).
    pub const CORNER_HOLD_USAGE: f32 = 0.75;
    /// After braking, upshifts wait this long (the driver is about to turn in and power out
    /// of the corner on the gear the braking downshifts chose), s.
    pub const BRAKE_HOLD_S: f32 = 1.5;
    /// Load filter time constants: rising / falling, s.
    pub const LOAD_RISE_S: f32 = 0.12;
    /// See [`LOAD_RISE_S`].
    pub const LOAD_FALL_S: f32 = 0.8;
}

/// Everything [`Gearbox::auto_shift`] looks at, per tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShiftInputs {
    /// Engine redline, rpm.
    pub redline_rpm: f32,
    /// Weighted driven-wheel speed, rad/s.
    pub axle_omega: f32,
    /// Raw driver throttle 0..=1 (in reverse: the raw throttle pedal, pedals are swapped by
    /// the caller).
    pub throttle: f32,
    /// Raw driver brake 0..=1.
    pub brake: f32,
    /// Forward speed, m/s.
    pub speed: f32,
    /// Highest usable gear (gearbox damage).
    pub max_gear: i8,
    /// Lateral tire usage 0..1 (`Σ|Fy| / Σ μ·Fz` over wheels in contact).
    pub lateral_usage: f32,
    /// The car is sliding (|body slip| past [`schedule::SLIDE_HOLD_DEG`] or the driven wheels
    /// spinning past the tyre's peak): the box holds its gear (see [`Gearbox::auto_shift`]).
    pub sliding: bool,
    /// Driven-wheel speed implied by the road (rolling speed, no wheelspin), rad/s: what the
    /// box shifts on while sliding.
    pub road_axle_omega: f32,
}

/// Gearbox state. `gear`: -1 reverse, 0 neutral, 1.. forward.
#[derive(Debug, Clone, PartialEq)]
pub struct Gearbox {
    /// Engaged gear (the old gear until a clutch shift's neutral phase, see
    /// [`Gearbox::effective_gear`]).
    pub gear: i8,
    pub(crate) target: i8,
    /// Time left in the current shift, s.
    pub(crate) shift_timer: f32,
    /// Time since the last shift started, s (anti-hunting).
    pub(crate) cooldown: f32,
    pub(crate) reverse_hold: f32,
    /// Length of the current shift, s.
    pub(crate) shift_total: f32,
    /// Smoothed driver demand used by the schedule.
    pub(crate) load: f32,
    /// Direction of the last shift: 1 up, -1 down, 0 none yet.
    pub(crate) last_dir: i8,
    /// Time left in the post-braking upshift hold, s.
    pub(crate) brake_hold: f32,
}

impl Gearbox {
    /// New gearbox in first gear.
    pub fn new() -> Self {
        Self {
            gear: 1,
            target: 1,
            shift_timer: 0.0,
            cooldown: f32::MAX,
            reverse_hold: 0.0,
            shift_total: 0.0,
            load: 0.0,
            last_dir: 0,
            brake_hold: 0.0,
        }
    }

    /// Put in `gear` immediately (teleports / resets).
    pub fn set(&mut self, gear: i8) {
        self.gear = gear;
        self.target = gear;
        self.shift_timer = 0.0;
        self.shift_total = 0.0;
        self.cooldown = f32::MAX;
        self.last_dir = 0;
        self.brake_hold = 0.0;
    }

    /// True while a shift is in progress.
    pub fn shifting(&self) -> bool {
        self.shift_timer > 0.0
    }

    /// Gear the box is heading for (equals `gear` when not shifting).
    pub fn target(&self) -> i8 {
        self.target
    }

    /// Start a shift to `target`, taking `shift_time` seconds.
    pub fn shift(&mut self, target: i8, shift_time: f32) {
        if target == self.gear || self.shifting() {
            return;
        }
        self.last_dir = (target - self.gear).signum();
        self.target = target;
        self.shift_timer = shift_time.max(1e-3);
        self.shift_total = self.shift_timer;
        self.cooldown = 0.0;
    }

    /// Advance timers by `dt`.
    pub fn update(&mut self, dt: f32) {
        self.cooldown = (self.cooldown + dt).min(1e6);
        if self.shift_timer > 0.0 {
            self.shift_timer -= dt;
            if self.shift_timer <= 0.0 {
                self.shift_timer = 0.0;
                self.gear = self.target;
            }
        }
    }

    /// Progress of the current shift 0..1 (1 when not shifting).
    pub fn progress(&self) -> f32 {
        if !self.shifting() {
            return 1.0;
        }
        (1.0 - self.shift_timer / self.shift_total.max(1e-3)).clamp(0.0, 1.0)
    }

    /// Clutch-gearbox shift phases: the gear whose ratio applies now (old gear while
    /// releasing, 0 in the neutral middle, the new gear while re-engaging).
    pub fn effective_gear(&self) -> i8 {
        if !self.shifting() {
            return self.gear;
        }
        let p = self.progress();
        if p < SHIFT_RELEASE_FRAC {
            self.gear
        } else if p < 1.0 - SHIFT_RELEASE_FRAC {
            0
        } else {
            self.target
        }
    }

    /// Clutch engagement the shift allows (1 outside shifts): fades 1→0 while releasing, 0 in
    /// neutral, 0→1 while re-engaging. Multiplies the clutch and the engine throttle of a
    /// clutch gearbox, so torque fades out and fills back in instead of switching.
    pub fn shift_engagement(&self) -> f32 {
        if !self.shifting() {
            return 1.0;
        }
        let p = self.progress();
        let r = SHIFT_RELEASE_FRAC.max(1e-3);
        if p < r {
            1.0 - p / r
        } else if p < 1.0 - r {
            0.0
        } else {
            (p - (1.0 - r)) / r
        }
    }

    /// In the neutral phase of a clutch shift: seconds left before re-engaging (the engine
    /// is rev-matched over this time). `None` outside that phase.
    pub fn neutral_time_left(&self) -> Option<f32> {
        if self.effective_gear() != 0 || !self.shifting() || self.target == 0 {
            return None;
        }
        Some((self.shift_timer - SHIFT_RELEASE_FRAC * self.shift_total).max(0.0))
    }

    /// Gearbox ratio (without final drive) of `gear`; reverse is negative, neutral 0.
    pub fn ratio_of(def: &DrivetrainDef, gear: i8) -> f32 {
        match gear {
            g if g < 0 => -def.reverse_ratio,
            0 => 0.0,
            g => def.gear_ratios[(g as usize - 1).min(def.gear_ratios.len() - 1)],
        }
    }

    /// Total ratio engine:wheel including final drive; 0 in neutral.
    ///
    /// Clutch gearbox while shifting: the old gear while the clutch releases, neutral (0) in
    /// the middle, the new gear while it re-engages (see [`Gearbox::effective_gear`]). A
    /// torque-converter automatic (`def.torque_converter` set) shifts between forward gears
    /// under power instead — clutch-to-clutch, like a real planetary automatic: the effective
    /// ratio moves from the old gear to the new one over the shift (the inertia phase pulls
    /// the engine to its new speed), the lock-up clutch stays open and the converter carries
    /// the torque. Shifts into neutral or reverse still interrupt drive.
    pub fn total_ratio(&self, def: &DrivetrainDef) -> f32 {
        if !self.shifting() {
            return Self::ratio_of(def, self.gear) * def.final_drive;
        }
        if def.torque_converter.is_none() {
            return Self::ratio_of(def, self.effective_gear()) * def.final_drive;
        }
        if self.gear < 1 || self.target < 1 {
            return 0.0;
        }
        lerp(
            Self::ratio_of(def, self.gear),
            Self::ratio_of(def, self.target),
            self.progress(),
        ) * def.final_drive
    }

    /// Automatic shifting: auto-reverse, gearbox-damage limits, kickdown, corner hold and the
    /// load-based schedule in [`schedule`] (see there for the anti-hunting rules). Hold brake
    /// at a standstill to engage reverse; press throttle to go back to first.
    pub fn auto_shift(&mut self, def: &DrivetrainDef, inp: &ShiftInputs, dt: f32) {
        use schedule::*;
        let thr = inp.throttle.clamp(0.0, 1.0);
        let tau = if thr > self.load {
            LOAD_RISE_S
        } else {
            LOAD_FALL_S
        };
        let stab = thr - self.load;
        self.load += (thr - self.load) * (dt / tau).min(1.0);
        self.brake_hold = if inp.brake > 0.1 {
            BRAKE_HOLD_S
        } else {
            (self.brake_hold - dt).max(0.0)
        };
        if self.shifting() {
            return;
        }
        let t = def.shift_time_s;
        // Destroyed gearbox (no usable gear): neutral, and stay there — checked before the
        // reverse handling, which would otherwise pick 1st or reverse again every shift.
        if inp.max_gear < 1 {
            self.shift(0, t);
            return;
        }
        // Reverse handling.
        if self.gear >= 1 {
            if inp.speed.abs() < 0.5 && inp.brake > 0.5 && thr < 0.05 {
                self.reverse_hold += dt;
                if self.reverse_hold > 0.25 {
                    self.reverse_hold = 0.0;
                    self.shift(-1, t);
                    return;
                }
            } else {
                self.reverse_hold = 0.0;
            }
        } else {
            if thr > 0.05 && inp.speed > -0.5 {
                self.shift(1, t);
            }
            return;
        }
        if self.gear > inp.max_gear {
            self.shift(inp.max_gear, t);
            return;
        }
        let red = inp.redline_rpm.max(1.0);
        // Engine speed in `gear` at the current wheel speed, as a fraction of redline.
        let frac_at = |gear: i8| {
            (inp.axle_omega * Self::ratio_of(def, gear) * def.final_drive).abs()
                * crate::engine::RPM_PER_RADPS
                / red
        };
        let now = frac_at(self.gear);
        let n_max = inp.max_gear;
        let since = self.cooldown;
        let reversal_ok = |dir: i8| since >= MIN_IN_GEAR_S || self.last_dir != -dir;
        let repeat_ok = since >= MIN_SAME_DIR_S;
        let cornering = inp.lateral_usage >= CORNER_HOLD_USAGE;
        let braking = inp.brake > 0.1;

        // Sliding (a held drift, a power-on slide, wheelspin): hold the gear like a driver
        // would. No kickdown, no shift from the spinning wheels' revs; shift only on road
        // speed — up when the road speed alone reaches the limiter, down when it would lug
        // the engine.
        if inp.sliding {
            let road_at = |gear: i8| {
                (inp.road_axle_omega * Self::ratio_of(def, gear) * def.final_drive).abs()
                    * crate::engine::RPM_PER_RADPS
                    / red
            };
            let road = road_at(self.gear);
            if road >= FORCED_UP && self.gear < n_max && repeat_ok {
                self.shift(self.gear + 1, t);
            } else if self.gear > 1 && road < DOWN_LIGHT && repeat_ok {
                self.shift(self.gear - 1, t);
            }
            return;
        }
        // Limiter protection beats everything.
        if now >= FORCED_UP && self.gear < n_max && repeat_ok {
            self.shift(self.gear + 1, t);
            return;
        }
        // Kickdown: a stab of the pedal drops to the lowest gear that stays below the ceiling.
        if thr >= KICKDOWN_PEDAL && stab >= KICKDOWN_STAB && !cornering && !braking {
            let target = (1..self.gear)
                .find(|g| frac_at(*g) <= KICKDOWN_CEILING)
                .unwrap_or(self.gear);
            if target < self.gear {
                self.shift(target, t);
                return;
            }
        }
        if !repeat_ok {
            return;
        }
        // Shift points rise with the pedal at once and fall back slowly after a lift.
        let load = thr.max(self.load);
        let up = lerp(UP_LIGHT, UP_FULL, load);
        // The downshift point rises quickly with load (√): asking for power at part throttle
        // gets the lower gear, cruising gets the tall one.
        let down = if braking {
            DOWN_BRAKING
        } else {
            lerp(DOWN_LIGHT, DOWN_FULL, load.sqrt())
        };
        // Upshift: past the point, not braking (or just after), not holding a corner, and the
        // next gear lands clear of the downshift point.
        if self.gear < n_max
            && now > up
            && self.brake_hold <= 0.0
            && !cornering
            && frac_at(self.gear + 1) >= down + BAND_MARGIN
            && reversal_ok(1)
        {
            self.shift(self.gear + 1, t);
            return;
        }
        // Downshift: below the point, and the lower gear lands clear of the upshift point
        // (braking: below a safe ceiling instead; corner hold blocks power-on downshifts
        // unless the engine is near stalling).
        if self.gear > 1 && now < down && reversal_ok(-1) {
            let lower = frac_at(self.gear - 1);
            let fits = if braking {
                lower <= BRAKING_DOWN_CEILING
            } else {
                lower <= up - BAND_MARGIN
            };
            let lugging = now < DOWN_LIGHT;
            if fits && (!cornering || braking || lugging) {
                self.shift(self.gear - 1, t);
            }
        }
    }
}

impl Default for Gearbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Launch rpm the automatic clutch slips up to: idle + `throttle` × 45% of the idle→redline
/// span.
pub fn launch_rpm(idle_rpm: f32, redline_rpm: f32, throttle: f32) -> f32 {
    idle_rpm + throttle.clamp(0.0, 1.0) * 0.45 * (redline_rpm - idle_rpm)
}

/// Gearbox input speed, × idle, above which a moving car's automatic clutch stays (or
/// becomes) fully engaged: the anti-stall line. Below it the clutch may slip / open so the
/// engine does not drop under idle.
pub const AUTO_CLUTCH_HOLD_IDLE_FRAC: f32 = 1.2;
/// Lowest forward gear that never launch-slips while rolling (owner row 117): 3rd and up
/// engage fully as soon as the ground turns the gearbox input above
/// [`AUTO_CLUTCH_HOLD_IDLE_FRAC`] × idle.
pub const AUTO_CLUTCH_TALL_GEAR: i8 = 3;

/// Automatic clutch / launch control. Returns the new engagement 0..=1. `input_rpm` is the
/// gearbox input speed implied by the ground (signed; wheelspin does not count), `gear` the
/// selected gear.
/// - **Engaged and moving** (engagement ≈ 1, input above [`AUTO_CLUTCH_HOLD_IDLE_FRAC`] ×
///   idle): stays fully engaged in any gear, so the revs follow road speed and a tall gear bogs
///   instead of slipping (owner row 117: 3rd gear held a constant rpm while the car accelerated,
///   a "CVT" feel, because the clutch dropped back into launch slip after every shift).
/// - **Tall gear rolling** (3rd and up, input above that line): engages fully at 10/s.
/// - **Launch rolling** (input above 1.2 × idle and 90% of the launch rpm): engages fully at
///   10/s.
/// - **Pulling away** (driver on the throttle below that, clutch not yet engaged):
///   centrifugal-style engagement `((rpm − idle) / (launch − idle))² · torque_ratio`, where
///   `torque_ratio` = 1.2 × peak engine torque / clutch capacity, so clutch torque reaches
///   engine torque near [`launch_rpm`] and the engine settles there (TCS trims clutch torque
///   during this phase, see `Vehicle`). Only from (near) standstill or after the clutch opened.
/// - Off the throttle at crawl speed: open (no stall when stopping).
#[allow(clippy::too_many_arguments)]
pub fn auto_clutch_update(
    engagement: f32,
    input_rpm: f32,
    engine_rpm: f32,
    idle_rpm: f32,
    redline_rpm: f32,
    throttle: f32,
    torque_ratio: f32,
    gear: i8,
    dt: f32,
) -> f32 {
    let launch = launch_rpm(idle_rpm, redline_rpm, throttle);
    let above_idle = input_rpm >= idle_rpm * AUTO_CLUTCH_HOLD_IDLE_FRAC;
    let held = engagement >= 0.999 && above_idle;
    let tall = gear >= AUTO_CLUTCH_TALL_GEAR && above_idle;
    let rolling = above_idle && input_rpm >= launch * 0.9;
    if held || tall || rolling {
        (engagement + 10.0 * dt).min(1.0)
    } else if throttle > 0.02 {
        let x = ((engine_rpm - idle_rpm) / (launch - idle_rpm).max(200.0)).clamp(0.0, 1.0);
        (x * x * torque_ratio).min(1.0)
    } else {
        0.0
    }
}

/// Hydrodynamic torque converter (contract `TorqueConverterDef`). The impeller (engine) absorbs
/// `C·ω_e²·f(SR)`, with `C` chosen so the engine absorbs exactly its full-throttle torque at
/// `stall_rpm` with the turbine held still, and `f(SR)` = 1 up to the coupling point
/// (speed ratio 0.85) falling linearly to 0 at SR = 1. The turbine delivers that torque times
/// `TR(SR)` = `stall_torque_ratio` at stall falling linearly to 1 at the coupling point. When the
/// turbine over-runs the impeller (coasting) torque flows back 1:1 (engine braking). The lock-up
/// clutch itself is handled by the vehicle's clutch model above `coupling_rpm`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Converter {
    /// The definition.
    pub def: wp_contracts::TorqueConverterDef,
    /// Capacity constant `C`, N·m/(rad/s)².
    pub capacity: f32,
}

/// Speed ratio at the coupling point.
pub const CONVERTER_COUPLING_SR: f32 = 0.85;

impl Converter {
    /// Build from the definition and the engine's full-load torque at `stall_rpm` (N·m).
    pub fn new(def: wp_contracts::TorqueConverterDef, torque_at_stall_nm: f32) -> Self {
        let w = def.stall_rpm / crate::engine::RPM_PER_RADPS;
        Self {
            def,
            capacity: torque_at_stall_nm / (w * w).max(1.0),
        }
    }

    /// `(impeller load on the engine, turbine output torque)` in N·m for engine speed `w_e`
    /// and turbine speed `w_t` (rad/s, both in the engine's rotation sense).
    pub fn torques(&self, w_e: f32, w_t: f32) -> (f32, f32) {
        let w_e = w_e.max(0.0);
        if w_t > w_e {
            // Over-run: the wheels drive the engine.
            let t = -self.capacity * (w_t * w_t - w_e * w_e);
            return (t, t);
        }
        let sr = (w_t.max(0.0) / w_e.max(1e-3)).clamp(0.0, 1.0);
        let f = ((1.0 - sr) / (1.0 - CONVERTER_COUPLING_SR)).clamp(0.0, 1.0);
        let t_p = self.capacity * w_e * w_e * f;
        let tr = self.def.stall_torque_ratio
            + (1.0 - self.def.stall_torque_ratio) * (sr / CONVERTER_COUPLING_SR).min(1.0);
        (t_p, t_p * tr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt() -> DrivetrainDef {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        wp_contracts::load_vehicle_def(p).unwrap().drivetrain
    }

    /// Axle speed (rad/s) that puts the engine at `frac` of 6500 rpm in `gear`.
    fn axle_at(d: &DrivetrainDef, gear: i8, frac: f32) -> f32 {
        frac * 6500.0 / crate::engine::RPM_PER_RADPS / (Gearbox::ratio_of(d, gear) * d.final_drive)
    }

    fn inputs(axle_omega: f32, throttle: f32) -> ShiftInputs {
        ShiftInputs {
            redline_rpm: 6500.0,
            axle_omega,
            throttle,
            brake: 0.0,
            speed: 10.0,
            max_gear: 6,
            lateral_usage: 0.0,
            sliding: false,
            road_axle_omega: 0.0,
        }
    }

    /// Run the box `seconds` at constant inputs (10 ms steps).
    fn hold(g: &mut Gearbox, d: &DrivetrainDef, inp: &ShiftInputs, seconds: f32) {
        for _ in 0..(seconds * 100.0) as usize {
            g.auto_shift(d, inp, 0.01);
            g.update(0.01);
        }
    }

    #[test]
    fn upshift_releases_goes_neutral_and_reengages() {
        let d = dt();
        let mut g = Gearbox::new();
        hold(&mut g, &d, &inputs(axle_at(&d, 1, 0.5), 1.0), 0.5);
        assert_eq!(g.gear, 1, "below the full-load point");
        g.auto_shift(&d, &inputs(axle_at(&d, 1, 0.98), 1.0), 0.01);
        assert!(g.shifting() && g.target() == 2);
        let r1 = Gearbox::ratio_of(&d, 1) * d.final_drive;
        let r2 = Gearbox::ratio_of(&d, 2) * d.final_drive;
        // Release (old gear, clutch fading), neutral, re-engage (new gear, clutch filling).
        g.update(0.1 * d.shift_time_s);
        assert_eq!(g.total_ratio(&d), r1);
        assert!(g.shift_engagement() > 0.0 && g.shift_engagement() < 1.0);
        g.update(0.4 * d.shift_time_s);
        assert_eq!(g.total_ratio(&d), 0.0);
        assert_eq!(g.shift_engagement(), 0.0);
        assert!(g.neutral_time_left().is_some());
        g.update(0.3 * d.shift_time_s);
        assert_eq!(g.total_ratio(&d), r2);
        g.update(0.21 * d.shift_time_s);
        assert_eq!(g.gear, 2);
        assert_eq!(g.shift_engagement(), 1.0);
    }

    #[test]
    fn shift_points_follow_load() {
        let d = dt();
        // Light throttle upshifts early, full throttle holds the gear to near the redline.
        let mut light = Gearbox::new();
        hold(&mut light, &d, &inputs(axle_at(&d, 1, 0.7), 0.2), 1.0);
        assert_eq!(light.gear, 2, "light load upshifts well before the redline");
        let mut full = Gearbox::new();
        hold(&mut full, &d, &inputs(axle_at(&d, 1, 0.9), 1.0), 1.0);
        assert_eq!(full.gear, 1, "full load holds first at 90%");
    }

    #[test]
    fn no_hunting_at_any_constant_load_and_speed() {
        let d = dt();
        // Every pedal 0..1 and every road speed: at most one shift direction, ever.
        for t in 0..=10 {
            let thr = t as f32 / 10.0;
            for s in 1..=60 {
                let axle = s as f32 * 2.0; // rad/s, ~0.6..37 m/s with a 0.3 m wheel
                let mut g = Gearbox::new();
                g.set(3);
                let mut dirs = Vec::new();
                let mut last = g.gear;
                for _ in 0..1000 {
                    g.auto_shift(&d, &inputs(axle, thr), 0.01);
                    g.update(0.01);
                    if g.gear != last {
                        dirs.push((g.gear - last).signum());
                        last = g.gear;
                    }
                }
                assert!(
                    dirs.windows(2).all(|w| w[0] == w[1]),
                    "hunting at throttle {thr} axle {axle}: {dirs:?}"
                );
            }
        }
    }

    #[test]
    fn sliding_holds_the_gear_and_shifts_on_road_speed() {
        // Lead 2026-10-21: in a held drift the box neither kicks down at full throttle nor
        // upshifts from the spinning rear's revs.
        let d = dt();
        let slide = |axle: f32, road: f32, throttle: f32| ShiftInputs {
            sliding: true,
            road_axle_omega: road,
            ..inputs(axle, throttle)
        };
        // Cruising in 2nd, the pedal stabbed at the drift's entry: no kickdown.
        let road = axle_at(&d, 2, 0.4);
        let mut g = Gearbox::new();
        g.set(2);
        hold(&mut g, &d, &inputs(road, 0.15), 1.5);
        g.auto_shift(&d, &slide(road, road, 1.0), 0.01);
        assert!(!g.shifting(), "kickdown while sliding");
        // Mid-drift the rears spin to the limiter: no upshift while the road speed is low.
        let spin = axle_at(&d, 2, 1.0);
        hold(&mut g, &d, &slide(spin, road, 1.0), 2.0);
        assert_eq!(g.gear, 2, "upshifted on wheelspin");
        // The road speed itself at the limiter: up a gear.
        let fast = axle_at(&d, 2, 0.99);
        hold(&mut g, &d, &slide(fast, fast, 1.0), 1.0);
        assert_eq!(g.gear, 3);
        // Not sliding: the normal schedule upshifts on the same revs.
        let mut n = Gearbox::new();
        n.set(2);
        hold(&mut n, &d, &inputs(spin, 1.0), 2.0);
        assert!(n.gear > 2);
    }

    #[test]
    fn kickdown_skips_gears_and_corner_hold_blocks_shifts() {
        let d = dt();
        // Cruising in 5th at light throttle, then the pedal is stabbed.
        let axle = axle_at(&d, 5, 0.3);
        let mut g = Gearbox::new();
        g.set(5);
        hold(&mut g, &d, &inputs(axle, 0.15), 1.5);
        assert_eq!(g.gear, 5);
        g.auto_shift(&d, &inputs(axle, 1.0), 0.01);
        assert!(g.shifting());
        assert!(g.target() <= 3, "kickdown to {} from 5", g.target());
        let rpm = (axle * Gearbox::ratio_of(&d, g.target()) * d.final_drive).abs()
            * crate::engine::RPM_PER_RADPS;
        assert!(rpm <= 0.86 * 6500.0, "kickdown over-revs: {rpm}");

        // At the tire limit mid-corner: past the upshift point but the box holds.
        let mut c = Gearbox::new();
        c.set(2);
        let cornering = ShiftInputs {
            lateral_usage: 0.9,
            ..inputs(axle_at(&d, 2, 0.8), 0.5)
        };
        hold(&mut c, &d, &cornering, 1.0);
        assert_eq!(c.gear, 2, "upshifted mid-corner");
        // ...and a stab mid-corner is no kickdown either.
        let mut k = Gearbox::new();
        k.set(4);
        let corner_lugging = ShiftInputs {
            lateral_usage: 0.9,
            ..inputs(axle_at(&d, 4, 0.35), 0.1)
        };
        hold(&mut k, &d, &corner_lugging, 1.0);
        k.auto_shift(
            &d,
            &ShiftInputs {
                throttle: 1.0,
                ..corner_lugging
            },
            0.01,
        );
        assert!(!k.shifting());
        // The limiter still wins.
        hold(
            &mut c,
            &d,
            &ShiftInputs {
                axle_omega: axle_at(&d, 2, 0.99),
                ..cornering
            },
            0.1,
        );
        assert!(c.gear == 3 || c.target() == 3);
    }

    #[test]
    fn braking_downshifts_sequentially_without_over_revving() {
        let d = dt();
        let mut g = Gearbox::new();
        g.set(6);
        // Decelerate from 6th at 50% redline to a crawl over 5 s with the brake on.
        let mut max_frac = 0.0f32;
        for i in 0..500 {
            let axle = axle_at(&d, 6, 0.5) * (1.0 - i as f32 / 520.0);
            let inp = ShiftInputs {
                brake: 0.8,
                ..inputs(axle, 0.0)
            };
            g.auto_shift(&d, &inp, 0.01);
            g.update(0.01);
            let frac = (axle * Gearbox::ratio_of(&d, g.gear) * d.final_drive).abs()
                * crate::engine::RPM_PER_RADPS
                / 6500.0;
            max_frac = max_frac.max(frac);
        }
        assert!(g.gear <= 2, "ended in {}", g.gear);
        assert!(
            max_frac <= 0.75,
            "braking downshift over-revved to {max_frac}"
        );

        // Off the brake, coasting into the corner at 60% of redline: the braking gear is held
        // for the corner exit instead of upshifting straight away.
        let mut h = Gearbox::new();
        h.set(3);
        let braking = ShiftInputs {
            brake: 0.8,
            ..inputs(axle_at(&d, 3, 0.6), 0.0)
        };
        hold(&mut h, &d, &braking, 0.3);
        let coasting = inputs(axle_at(&d, 3, 0.6), 0.0);
        hold(&mut h, &d, &coasting, 1.3);
        assert_eq!(h.gear, 3, "upshifted right after braking");
        hold(&mut h, &d, &coasting, 1.0);
        assert!(h.gear > 3, "light-load upshifts once the hold is over");
    }

    #[test]
    fn destroyed_gearbox_stays_in_neutral_under_throttle() {
        // Gearbox health 0 → max_gear 0: the box must drop to neutral and stay there, not
        // cycle neutral → 1st → neutral (each cycle would leak drive through the re-engage
        // phase).
        let d = dt();
        let mut g = Gearbox::new();
        g.set(2);
        let broken = ShiftInputs {
            max_gear: 0,
            ..inputs(axle_at(&d, 2, 0.4), 1.0)
        };
        hold(&mut g, &d, &broken, 0.5);
        assert_eq!(g.gear, 0);
        // Reverse is lost too.
        let mut r = Gearbox::new();
        r.set(-1);
        hold(&mut r, &d, &broken, 0.5);
        assert_eq!(r.gear, 0);
        for _ in 0..300 {
            g.auto_shift(&d, &broken, 0.01);
            g.update(0.01);
            assert_eq!(g.total_ratio(&d), 0.0, "drive leaked in gear {}", g.gear);
        }
    }

    #[test]
    fn auto_reverse_after_holding_brake() {
        let d = dt();
        let mut g = Gearbox::new();
        let stopped = ShiftInputs {
            brake: 1.0,
            speed: 0.0,
            ..inputs(0.0, 0.0)
        };
        hold(&mut g, &d, &stopped, 0.8);
        assert_eq!(g.gear, -1);
        assert!(g.total_ratio(&d) < 0.0);
    }

    #[test]
    fn auto_clutch_launch_control() {
        let dt = 0.01;
        // Standing start: engagement follows engine rpm toward the launch rpm.
        assert_eq!(
            auto_clutch_update(0.0, 0.0, 800.0, 800.0, 6500.0, 1.0, 0.7, 1, dt),
            0.0
        );
        let mid = auto_clutch_update(0.0, 0.0, 2500.0, 800.0, 6500.0, 1.0, 0.7, 1, dt);
        assert!(mid > 0.0 && mid < 1.0);
        // Rolling: engages fully; crawling off throttle: open.
        assert!(auto_clutch_update(0.5, 4000.0, 4000.0, 800.0, 6500.0, 1.0, 0.7, 1, dt) > 0.5);
        assert_eq!(
            auto_clutch_update(1.0, 500.0, 900.0, 800.0, 6500.0, 0.0, 0.7, 1, dt),
            0.0
        );
        // Row 117: engaged and moving above the anti-stall line stays engaged at full
        // throttle in any gear (2000 rpm < 90% of the 3365 rpm launch rpm).
        assert_eq!(
            auto_clutch_update(1.0, 2000.0, 2000.0, 800.0, 6500.0, 1.0, 0.7, 2, dt),
            1.0
        );
        // 3rd and up engage at 10/s once rolling above it, even from an open clutch…
        let tall = auto_clutch_update(0.0, 1200.0, 3000.0, 800.0, 6500.0, 1.0, 0.7, 3, dt);
        assert!((tall - 10.0 * dt).abs() < 1e-6, "{tall}");
        // …while 1st / 2nd from an open clutch still launch-slip up to the launch rpm.
        let launch = auto_clutch_update(0.0, 1200.0, 2000.0, 800.0, 6500.0, 1.0, 0.7, 2, dt);
        assert!(launch < 1.0);
        // Below the anti-stall line an engaged clutch lets go off the throttle.
        assert_eq!(
            auto_clutch_update(1.0, 900.0, 900.0, 800.0, 6500.0, 0.0, 0.7, 3, dt),
            0.0
        );
    }

    #[test]
    fn converter_stall_and_coupling() {
        let tc = wp_contracts::TorqueConverterDef {
            stall_rpm: 2400.0,
            stall_torque_ratio: 2.0,
            coupling_rpm: 2000.0,
        };
        let c = Converter::new(tc, 250.0);
        let w = 2400.0 / crate::engine::RPM_PER_RADPS;
        let (tp, tt) = c.torques(w, 0.0);
        assert!((tp - 250.0).abs() < 0.5, "stall load {tp}");
        assert!((tt - 500.0).abs() < 1.0, "stall multiplication {tt}");
        let (tp, tt) = c.torques(w, w * 0.9);
        assert!(tt <= tp * 1.0001 && tp < 250.0);
        let (tp, tt) = c.torques(w, w * 1.1);
        assert!(tp < 0.0 && tt == tp, "over-run brakes the engine");
    }
}
