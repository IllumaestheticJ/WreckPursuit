//! Brakes: bias, handbrake and ABS.

use wp_contracts::{AbsLevel, BrakeDef};

/// ABS Full starts a release once a wheel's braking slip ratio passes this many × the tyre's
/// peak.
pub const ABS_FULL_TRIGGER: f32 = 1.4;
/// ABS Half triggers much later (reference "Half"): the wheel runs deep into slip and briefly
/// locks before each release, so stops are longer than Full but the car still steers.
pub const ABS_HALF_TRIGGER: f32 = 3.0;
/// A release ends (the wheel has spun back up) once the braking slip is back under this many ×
/// the tyre's peak: Full.
pub const ABS_FULL_RECOVER: f32 = 1.0;
/// Half's release ends later in the spin-up (the next lock comes sooner, the wheel slips more).
pub const ABS_HALF_RECOVER: f32 = 2.5;
/// On a release the brake pressure drops at once to this fraction of what it was (Full)…
pub const ABS_DUMP_FRACTION: f32 = 0.3;
/// …Half dumps less (the wheel stays in deep slip longer: "short lock-ups, still steerable").
pub const ABS_HALF_DUMP_FRACTION: f32 = 0.6;
/// …and keeps falling (to zero in this many seconds) until the wheel spins back up. Releasing a
/// fixed fraction for a fixed time (the old ABS: 45 % for 1/30 s) never let a wheel recover on
/// a road whose grip could not turn it against 45 % of the brake (wet, ice, trail braking): it
/// stayed locked for the whole stop (`examples/abs_lock.rs`, 2026-10-26).
pub const ABS_DUMP_S: f32 = 0.03;
/// When the wheel has spun up, the pressure steps back to this fraction of the pressure it
/// locked at (so on a slippery road it adapts in a few cycles)…
pub const ABS_REAPPLY_STEP: f32 = 0.75;
/// …then ramps back toward the driver's full demand at this rate, fraction per second.
pub const ABS_REAPPLY_RATE: f32 = 3.0;
/// ABS Full's drag-torque control decouples the driveline while a driven wheel it has released
/// to under this pressure fraction is still past the trigger (`Vehicle::step`).
pub const ABS_DRAG_PRESSURE: f32 = 0.05;
/// ABS works above this contact speed, m/s; below it the wheels may stop with the car.
pub const ABS_MIN_SPEED_MPS: f32 = 2.0;

/// Motorsport ABS holds each wheel's braking slip at this fraction of the tyre's peak slip ratio
/// (just under the peak: the tyre's longitudinal force is flat there and the wheel never runs
/// past it).
pub const MOTORSPORT_SLIP_FRACTION: f32 = 0.95;
/// While the tyre also carries side force (trail braking) the target slip shrinks on the
/// friction ellipse (`tire::combined_forces`), down to this fraction of the straight-line target,
/// so the front tyres keep steering.
pub const MOTORSPORT_MIN_ELLIPSE: f32 = 0.5;

/// The ABS hardware fitted to a car. A car property (a garage upgrade), not a driver assist:
/// the driver's [`AbsLevel`] still picks Off / Half / Full, and the hardware decides how
/// [`AbsLevel::Full`] works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbsHardware {
    /// The stock road-car ABS ([`Abs`]).
    #[default]
    Stock,
    /// Motorsport ABS (garage upgrade "Motorsport ABS"): with ABS Full, per-wheel slip control
    /// at the tyre's peak slip, modulated every sub-step ([`MotorsportAbs`]).
    Motorsport,
}

/// Release trigger of `level`, multiples of peak slip (`None` for `Off`).
pub fn abs_trigger(level: AbsLevel) -> Option<f32> {
    match level {
        AbsLevel::Off => None,
        AbsLevel::Half => Some(ABS_HALF_TRIGGER),
        AbsLevel::Full => Some(ABS_FULL_TRIGGER),
    }
}

/// Service-brake torque per wheel (FL, FR, RL, RR) for `pedal` 0..=1. The bias is normalised
/// so the biased axle reaches its full `max_torque_*` (the contract's "at bias 1" value) and
/// the other axle is scaled by `(1 − bias) / bias` (or vice versa).
pub fn service_torques(def: &BrakeDef, pedal: f32) -> [f32; 4] {
    let b = def.bias_front.clamp(0.0, 1.0);
    let norm = b.max(1.0 - b).max(1e-3);
    let f = pedal * def.max_torque_front_nm * b / norm;
    let r = pedal * def.max_torque_rear_nm * (1.0 - b) / norm;
    [f, f, r, r]
}

/// Braking slip of a wheel: `−κ` in the direction of travel (positive while the brake slows
/// the wheel below road speed, either direction).
pub fn braking_slip(slip_ratio: f32, vx: f32) -> f32 {
    if vx < 0.0 {
        slip_ratio
    } else {
        -slip_ratio
    }
}

/// Per-wheel stock ABS: a release / reapply cycle on the brake pressure. Call once per tick.
///
/// A wheel whose braking slip passes the level's trigger is released: its pressure drops to
/// [`ABS_DUMP_FRACTION`] and on toward zero ([`ABS_DUMP_S`]) until the wheel has spun back up
/// (slip under the level's recover threshold). The pressure then steps back to
/// [`ABS_REAPPLY_STEP`] × the pressure the wheel locked at and ramps up
/// ([`ABS_REAPPLY_RATE`]) until the next lock. On a slippery road the lock pressure falls each
/// cycle, so the cycle settles around the road's grip on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Abs {
    /// Brake pressure per wheel, fraction of the driver's demand (1 = not modulating).
    pub(crate) pressure: [f32; 4],
    /// Pressure at the wheel's last lock (the reapply step aims under it).
    pub(crate) lock_pressure: [f32; 4],
    /// 1 while a wheel is being released, else 0.
    pub(crate) releasing: [f32; 4],
}

impl Default for Abs {
    fn default() -> Self {
        Self {
            pressure: [1.0; 4],
            lock_pressure: [1.0; 4],
            releasing: [0.0; 4],
        }
    }
}

impl Abs {
    /// Modulate brake torques at `level` (`Off` clears the state). `slip_ratio` from the last
    /// tick, `vx` contact speeds, `peak_slip` per wheel.
    pub fn apply(
        &mut self,
        level: AbsLevel,
        torques: &mut [f32; 4],
        slip_ratio: [f32; 4],
        vx: [f32; 4],
        peak_slip: [f32; 4],
        dt: f32,
    ) {
        let (trigger, recover, dump) = match level {
            AbsLevel::Off => {
                *self = Self::default();
                return;
            }
            AbsLevel::Half => (ABS_HALF_TRIGGER, ABS_HALF_RECOVER, ABS_HALF_DUMP_FRACTION),
            AbsLevel::Full => (ABS_FULL_TRIGGER, ABS_FULL_RECOVER, ABS_DUMP_FRACTION),
        };
        for i in 0..4 {
            if torques[i] <= 0.0 || vx[i].abs() <= ABS_MIN_SPEED_MPS {
                // Off the brake (or stopping): the next press starts at the driver's pressure.
                self.pressure[i] = 1.0;
                self.lock_pressure[i] = 1.0;
                self.releasing[i] = 0.0;
                continue;
            }
            let slip = braking_slip(slip_ratio[i], vx[i]);
            // Never past a full lock (slip 1), so Half still releases a locked wheel.
            let limit = (trigger * peak_slip[i]).min(0.95);
            if self.releasing[i] == 0.0 && slip > limit {
                self.releasing[i] = 1.0;
                self.lock_pressure[i] = self.pressure[i];
                self.pressure[i] *= dump;
            }
            if self.releasing[i] > 0.0 {
                if slip < recover * peak_slip[i] {
                    self.releasing[i] = 0.0;
                    self.pressure[i] = ABS_REAPPLY_STEP * self.lock_pressure[i];
                } else {
                    self.pressure[i] = (self.pressure[i] - dt / ABS_DUMP_S).max(0.0);
                }
            } else {
                self.pressure[i] = (self.pressure[i] + ABS_REAPPLY_RATE * dt).min(1.0);
            }
            torques[i] *= self.pressure[i];
        }
    }

    /// True if ABS is currently modulating any wheel.
    pub fn active(&self) -> bool {
        self.pressure.iter().any(|p| *p < 1.0)
    }
}

/// Motorsport ABS's per-tick set-up, read by every sub-step of the wheel update. Rebuilt each
/// tick from the brake demand, so it carries no state between ticks (not in the snapshot).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MotorsportAbs {
    /// Braking-slip target per wheel ([`motorsport_slip_target`]); 0 = not controlling.
    pub(crate) target: [f32; 4],
    /// Brake torque the controller may modulate (service brake + ESC + brake-TC), N·m.
    pub(crate) modulated: [f32; 4],
    /// Brake torque it leaves alone (the handbrake), N·m.
    pub(crate) fixed: [f32; 4],
    /// True while the last sub-step held any wheel under its demand.
    pub(crate) active: bool,
}

impl MotorsportAbs {
    /// Total brake torque (N·m) for wheel `i` this sub-step, or `None` when the controller is not
    /// working that wheel (not fitted, off the brake, slower than [`ABS_MIN_SPEED_MPS`]).
    /// `vx` is the contact (or hub) road speed, `free_omega` the wheel speed the sub-step
    /// reaches with no brake, `i_eff` the wheel's effective inertia, `h` the sub-step.
    ///
    /// Each wheel is held at its own slip target ([`motorsport_torque`]). (Rear select-low was
    /// tried for split grip: no less yaw — the fronts make it — and 20 % longer stops.)
    pub fn brake(
        &mut self,
        i: usize,
        vx: f32,
        free_omega: f32,
        radius: f32,
        i_eff: f32,
        h: f32,
    ) -> Option<f32> {
        if self.target[i] <= 0.0 || self.modulated[i] <= 0.0 || vx.abs() <= ABS_MIN_SPEED_MPS {
            return None;
        }
        let target_omega = vx * (1.0 - self.target[i]) / radius.max(0.05);
        let t = motorsport_torque(
            self.modulated[i],
            self.fixed[i],
            free_omega,
            target_omega,
            i_eff,
            h,
        );
        self.active |= t < self.modulated[i] - 1.0;
        Some(self.fixed[i] + t)
    }
}

/// Motorsport ABS's braking-slip target for one wheel (positive, fraction of road speed):
/// [`MOTORSPORT_SLIP_FRACTION`] × peak slip, shrunk on the friction ellipse by the tyre's
/// slip angle use (`alpha` / `peak_alpha`) so a trail-braked tyre keeps its side force.
pub fn motorsport_slip_target(peak_slip: f32, alpha: f32, peak_alpha: f32) -> f32 {
    let sy = (alpha / peak_alpha.max(1e-3)).abs().min(1.0);
    let ellipse = (1.0 - sy * sy).max(0.0).sqrt().max(MOTORSPORT_MIN_ELLIPSE);
    MOTORSPORT_SLIP_FRACTION * peak_slip * ellipse
}

/// Motorsport ABS for one wheel and one sub-step: the part of the modulated brake torque
/// `modulated` (N·m, service brake + ESC + brake-TC; the handbrake is not modulated) that keeps
/// the wheel's speed from falling under `target_omega` this sub-step. `free_omega` is where
/// the wheel would spin with no brake at all, `i_eff` its effective inertia and `h` the
/// sub-step. Exact for the semi-implicit wheel update in `Vehicle::substep`, so the slip sits
/// on the target every sub-step: no wheel locks, on any road.
pub fn motorsport_torque(
    modulated: f32,
    unmodulated: f32,
    free_omega: f32,
    target_omega: f32,
    i_eff: f32,
    h: f32,
) -> f32 {
    if modulated <= 0.0 || h <= 0.0 || free_omega * target_omega <= 0.0 {
        return modulated.max(0.0);
    }
    // Torque that brings the wheel from its free speed exactly to the target, minus what the
    // unmodulated brake already takes.
    let needed = (free_omega.abs() - target_omega.abs()) * i_eff / h - unmodulated;
    needed.clamp(0.0, modulated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bias_normalisation() {
        let def = BrakeDef {
            max_torque_front_nm: 1600.0,
            max_torque_rear_nm: 900.0,
            bias_front: 0.6,
            handbrake_rear_nm: 1000.0,
        };
        let t = service_torques(&def, 1.0);
        assert_eq!(t[0], 1600.0);
        assert!((t[2] - 600.0).abs() < 1e-3);
    }

    #[test]
    fn abs_releases_locked_wheel() {
        let mut abs = Abs::default();
        let mut t = [1000.0; 4];
        abs.apply(
            AbsLevel::Full,
            &mut t,
            [-0.5, 0.0, 0.0, 0.0],
            [20.0; 4],
            [0.1; 4],
            0.004,
        );
        assert!(t[0] < 500.0 && t[1] == 1000.0);
    }

    #[test]
    fn abs_levels_release_at_different_slips() {
        let run = |level: AbsLevel, slip: f32| {
            let mut abs = Abs::default();
            let mut t = [1000.0; 4];
            abs.apply(level, &mut t, [slip; 4], [20.0; 4], [0.1; 4], 0.004);
            t[0]
        };
        // −0.3: past Full's trigger (0.14), inside Half's (0.3 is not past 0.3).
        assert!(run(AbsLevel::Full, -0.3) < 500.0);
        assert_eq!(run(AbsLevel::Half, -0.3), 1000.0);
        // A locked wheel is released by both; Off never releases.
        assert!(run(AbsLevel::Half, -1.0) < 500.0);
        assert_eq!(run(AbsLevel::Off, -1.0), 1000.0);
        // Off clears a pending release.
        let mut abs = Abs {
            pressure: [0.2; 4],
            lock_pressure: [0.5; 4],
            releasing: [1.0; 4],
        };
        let mut t = [1000.0; 4];
        abs.apply(AbsLevel::Off, &mut t, [0.0; 4], [20.0; 4], [0.1; 4], 0.004);
        assert!(!abs.active() && t[0] == 1000.0);
    }

    #[test]
    fn abs_keeps_releasing_until_the_wheel_spins_up() {
        // The old ABS released for a fixed 1/30 s at 45 %: on ice that never freed the wheel.
        let mut abs = Abs::default();
        let mut t = [1000.0; 4];
        abs.apply(
            AbsLevel::Full,
            &mut t,
            [-0.9; 4],
            [20.0; 4],
            [0.1; 4],
            0.004,
        );
        for _ in 0..30 {
            t = [1000.0; 4];
            abs.apply(
                AbsLevel::Full,
                &mut t,
                [-0.9; 4],
                [20.0; 4],
                [0.1; 4],
                0.004,
            );
        }
        assert_eq!(t[0], 0.0, "still locked after 0.12 s: fully released");
        // Spun up: the pressure steps back under the lock pressure, then ramps.
        t = [1000.0; 4];
        abs.apply(
            AbsLevel::Full,
            &mut t,
            [-0.05; 4],
            [20.0; 4],
            [0.1; 4],
            0.004,
        );
        assert!((t[0] - 1000.0 * ABS_REAPPLY_STEP).abs() < 1e-3, "{}", t[0]);
        t = [1000.0; 4];
        abs.apply(
            AbsLevel::Full,
            &mut t,
            [-0.05; 4],
            [20.0; 4],
            [0.1; 4],
            0.004,
        );
        assert!(t[0] > 1000.0 * ABS_REAPPLY_STEP);
        // The next lock adapts: it locked at 1.0 the first time, now at ~0.76.
        t = [1000.0; 4];
        abs.apply(
            AbsLevel::Full,
            &mut t,
            [-0.5; 4],
            [20.0; 4],
            [0.1; 4],
            0.004,
        );
        assert!(abs.lock_pressure[0] < 0.8);
    }

    #[test]
    fn abs_works_reversing() {
        let mut abs = Abs::default();
        let mut t = [1000.0; 4];
        // Reversing at 5 m/s, wheel locked: slip ratio +1.
        abs.apply(AbsLevel::Full, &mut t, [1.0; 4], [-5.0; 4], [0.1; 4], 0.004);
        assert!(t[0] < 500.0);
    }

    #[test]
    fn motorsport_target_shrinks_with_slip_angle() {
        let straight = motorsport_slip_target(0.12, 0.0, 0.1);
        assert!((straight - 0.95 * 0.12).abs() < 1e-6);
        let half = motorsport_slip_target(0.12, 0.05, 0.1);
        assert!(half < straight && half > 0.0);
        let sliding = motorsport_slip_target(0.12, -0.3, 0.1);
        assert!((sliding - MOTORSPORT_MIN_ELLIPSE * straight).abs() < 1e-6);
    }

    #[test]
    fn motorsport_torque_lands_on_the_target() {
        // Free wheel at 60 rad/s, target 55 rad/s, i_eff 2, h 1/480: needs 4800 N·m.
        let h = 1.0 / 480.0;
        let t = motorsport_torque(9000.0, 0.0, 60.0, 55.0, 2.0, h);
        assert!((t - 4800.0).abs() < 1.0, "{t}");
        let om = 60.0 - h * t / 2.0;
        assert!((om - 55.0).abs() < 1e-3);
        // Less than needed: the full demand.
        assert_eq!(motorsport_torque(1000.0, 0.0, 60.0, 55.0, 2.0, h), 1000.0);
        // The handbrake already holds it: nothing more.
        assert_eq!(motorsport_torque(1000.0, 6000.0, 60.0, 55.0, 2.0, h), 0.0);
        // Wheel already under the target: no service brake (the wheel must spin up).
        assert_eq!(motorsport_torque(1000.0, 0.0, 50.0, 55.0, 2.0, h), 0.0);
        // Reversing works the same way.
        let r = motorsport_torque(9000.0, 0.0, -60.0, -55.0, 2.0, h);
        assert!((r - 4800.0).abs() < 1.0);
    }
}
