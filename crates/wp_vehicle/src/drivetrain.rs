//! Gearbox (manual / automatic with kickdown and auto-reverse; power-on shifts behind a torque
//! converter), clutch engagement, torque converter, and
//! differential torque transfer (open, clutch-type LSD, locked).

use wp_contracts::{DiffDef, DrivetrainDef};

use crate::math::lerp;

/// Gearbox state. `gear`: -1 reverse, 0 neutral, 1.. forward.
#[derive(Debug, Clone, PartialEq)]
pub struct Gearbox {
    /// Currently engaged gear (the old gear while a shift is in progress).
    pub gear: i8,
    pub(crate) target: i8,
    pub(crate) shift_timer: f32,
    pub(crate) cooldown: f32,
    pub(crate) reverse_hold: f32,
}

impl Gearbox {
    /// New gearbox in first gear.
    pub fn new() -> Self {
        Self {
            gear: 1,
            target: 1,
            shift_timer: 0.0,
            cooldown: 0.0,
            reverse_hold: 0.0,
        }
    }

    /// Put in `gear` immediately (teleports / resets).
    pub fn set(&mut self, gear: i8) {
        self.gear = gear;
        self.target = gear;
        self.shift_timer = 0.0;
        self.cooldown = 0.0;
    }

    /// True while a shift is in progress (drive torque interrupted, except on a torque-converter
    /// automatic, see [`Gearbox::total_ratio`]).
    pub fn shifting(&self) -> bool {
        self.shift_timer > 0.0
    }

    /// Start a shift to `target`, taking `shift_time` seconds.
    pub fn shift(&mut self, target: i8, shift_time: f32) {
        if target == self.gear || self.shifting() {
            return;
        }
        self.target = target;
        self.shift_timer = shift_time.max(1e-3);
        self.cooldown = shift_time + 0.4;
    }

    /// Advance timers by `dt`.
    pub fn update(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        if self.shift_timer > 0.0 {
            self.shift_timer -= dt;
            if self.shift_timer <= 0.0 {
                self.shift_timer = 0.0;
                self.gear = self.target;
            }
        }
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
    /// While shifting: 0 (torque cut) for a clutch gearbox. A torque-converter automatic
    /// (`def.torque_converter` set) shifts between forward gears under power instead —
    /// clutch-to-clutch, like a real planetary automatic: the effective ratio moves from the
    /// old gear to the new one over the shift (the inertia phase pulls the engine to its new
    /// speed), the lock-up clutch stays open and the converter carries the torque. Shifts into
    /// neutral or reverse still interrupt drive.
    pub fn total_ratio(&self, def: &DrivetrainDef) -> f32 {
        if !self.shifting() {
            return Self::ratio_of(def, self.gear) * def.final_drive;
        }
        if def.torque_converter.is_none() || self.gear < 1 || self.target < 1 {
            return 0.0;
        }
        let progress = (1.0 - self.shift_timer / def.shift_time_s.max(1e-3)).clamp(0.0, 1.0);
        lerp(
            Self::ratio_of(def, self.gear),
            Self::ratio_of(def, self.target),
            progress,
        ) * def.final_drive
    }

    /// Automatic shifting. `axle_omega` = weighted driven-wheel speed (rad/s), `throttle` =
    /// driver demand, `speed` = forward speed (m/s), `brake` = driver brake pedal,
    /// `max_gear` = highest usable gear (gearbox damage). Handles kickdown and auto-reverse
    /// (hold brake at a standstill to engage reverse; press throttle to go back to first).
    #[allow(clippy::too_many_arguments)]
    pub fn auto_shift(
        &mut self,
        def: &DrivetrainDef,
        redline_rpm: f32,
        axle_omega: f32,
        throttle: f32,
        brake: f32,
        speed: f32,
        max_gear: i8,
        dt: f32,
    ) {
        if self.shifting() {
            return;
        }
        // Reverse handling.
        if self.gear >= 1 {
            if speed.abs() < 0.5 && brake > 0.5 && throttle < 0.05 {
                self.reverse_hold += dt;
                if self.reverse_hold > 0.25 {
                    self.reverse_hold = 0.0;
                    self.shift(-1, def.shift_time_s);
                    return;
                }
            } else {
                self.reverse_hold = 0.0;
            }
        } else if self.gear <= 0 {
            // In reverse the pedals are swapped by the caller; `throttle` here is the raw
            // driver throttle pedal.
            if throttle > 0.05 && speed > -0.5 {
                self.shift(1, def.shift_time_s);
            }
            return;
        }
        if max_gear < 1 {
            self.shift(0, def.shift_time_s);
            return;
        }
        if self.gear > max_gear {
            self.shift(max_gear, def.shift_time_s);
            return;
        }
        if self.cooldown > 0.0 {
            return;
        }
        let rpm_at = |gear: i8| {
            (axle_omega * Self::ratio_of(def, gear) * def.final_drive).abs()
                * crate::engine::RPM_PER_RADPS
        };
        let rpm = rpm_at(self.gear);
        let up = redline_rpm * lerp(0.62, 0.97, throttle);
        let down = redline_rpm * lerp(0.28, 0.62, throttle);
        if rpm > up && self.gear < max_gear {
            self.shift(self.gear + 1, def.shift_time_s);
        } else if self.gear > 1 && rpm < down && rpm_at(self.gear - 1) < 0.92 * redline_rpm {
            self.shift(self.gear - 1, def.shift_time_s);
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

/// Automatic clutch / launch control. Returns the new engagement 0..=1:
/// - rolling (gearbox input speed implied by *ground* speed above 1.2 × idle and 90% of the
///   launch rpm): engage fully at 10/s;
/// - driver on the throttle below that: centrifugal-style engagement
///   `((rpm − idle) / (launch − idle))² · torque_ratio`, where `torque_ratio` = 1.2 × peak
///   engine torque / clutch capacity, so clutch torque reaches engine torque near
///   [`launch_rpm`] and the engine settles there (TCS trims clutch torque during this phase,
///   see `Vehicle`);
/// - off the throttle at crawl speed: open (no stall when stopping).
#[allow(clippy::too_many_arguments)]
pub fn auto_clutch_update(
    engagement: f32,
    input_rpm: f32,
    engine_rpm: f32,
    idle_rpm: f32,
    redline_rpm: f32,
    throttle: f32,
    torque_ratio: f32,
    dt: f32,
) -> f32 {
    let launch = launch_rpm(idle_rpm, redline_rpm, throttle);
    let rolling = input_rpm >= (idle_rpm * 1.2).max(launch * 0.9);
    if rolling {
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

/// Differential torque transfer `X` for one axle: the left wheel receives `share_l − X`, the
/// right `share_r + X`. `axle_torque` is the torque entering the diff (for ramp selection),
/// `k_eq` the equalising gain (N·m per rad/s) that would lock the pair in a few steps.
pub fn diff_transfer(
    diff: &DiffDef,
    axle_torque: f32,
    omega_l: f32,
    omega_r: f32,
    k_eq: f32,
) -> f32 {
    let lock = match *diff {
        DiffDef::Open => return 0.0,
        DiffDef::Locked => f32::MAX,
        DiffDef::Lsd {
            preload_nm,
            ramp_power,
            ramp_coast,
        } => {
            let driving = axle_torque * (omega_l + omega_r) >= 0.0;
            preload_nm + axle_torque.abs() * if driving { ramp_power } else { ramp_coast }
        }
    };
    (k_eq * (omega_l - omega_r)).clamp(-lock, lock)
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

    #[test]
    fn shifts_up_with_hysteresis_and_cut() {
        let d = dt();
        let mut g = Gearbox::new();
        let axle = 6400.0 / crate::engine::RPM_PER_RADPS / (d.gear_ratios[0] * d.final_drive);
        g.auto_shift(&d, 6500.0, axle, 1.0, 0.0, 10.0, 6, 0.01);
        assert!(g.shifting());
        assert_eq!(g.total_ratio(&d), 0.0);
        g.update(d.shift_time_s + 0.01);
        assert_eq!(g.gear, 2);
    }

    #[test]
    fn auto_reverse_after_holding_brake() {
        let d = dt();
        let mut g = Gearbox::new();
        for _ in 0..80 {
            g.auto_shift(&d, 6500.0, 0.0, 0.0, 1.0, 0.0, 6, 0.01);
            g.update(0.01);
        }
        assert_eq!(g.gear, -1);
        assert!(g.total_ratio(&d) < 0.0);
    }

    #[test]
    fn auto_clutch_launch_control() {
        let dt = 0.01;
        // Standing start: engagement follows engine rpm toward the launch rpm.
        assert_eq!(
            auto_clutch_update(0.0, 0.0, 800.0, 800.0, 6500.0, 1.0, 0.7, dt),
            0.0
        );
        let mid = auto_clutch_update(0.0, 0.0, 2500.0, 800.0, 6500.0, 1.0, 0.7, dt);
        assert!(mid > 0.0 && mid < 1.0);
        // Rolling: engages fully; crawling off throttle: open.
        assert!(auto_clutch_update(0.5, 4000.0, 4000.0, 800.0, 6500.0, 1.0, 0.7, dt) > 0.5);
        assert_eq!(
            auto_clutch_update(1.0, 500.0, 900.0, 800.0, 6500.0, 0.0, 0.7, dt),
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

    #[test]
    fn diff_types() {
        assert_eq!(diff_transfer(&DiffDef::Open, 1000.0, 10.0, 0.0, 100.0), 0.0);
        let lsd = DiffDef::Lsd {
            preload_nm: 50.0,
            ramp_power: 0.5,
            ramp_coast: 0.2,
        };
        assert_eq!(diff_transfer(&lsd, 1000.0, 10.0, 0.0, 1000.0), 550.0);
        assert_eq!(diff_transfer(&lsd, -1000.0, 10.0, 0.0, 1000.0), 250.0);
        assert_eq!(
            diff_transfer(&DiffDef::Locked, 1000.0, 10.0, 0.0, 100.0),
            1000.0
        );
    }
}
