//! Driver assists: pedal filtering, traction control and stability control. Each is
//! independent and nothing in the physics depends on them being on.

use crate::math::approach;

/// Full 0→1 pedal ramp time at filter strength 1, s. Keyboard drivers (strength 0.7) get a
/// ~0.23 s throttle ramp: enough to keep a digital key from spinning the tires, short enough
/// not to feel like lag (it was 0.62 s, which the owner's playtest read as "slow").
pub const PEDAL_RAMP_AT_FULL_S: f32 = 0.3;

/// Pedal filter: ramps toward the target, releasing 3× faster than it applies.
/// `strength` 0 = raw input, 1 = [`PEDAL_RAMP_AT_FULL_S`] (+ 0.02 s) full ramp.
pub fn filter_pedal(current: f32, target: f32, strength: f32, dt: f32) -> f32 {
    if strength <= 0.0 {
        return target;
    }
    let rate = 1.0 / (0.02 + PEDAL_RAMP_AT_FULL_S * strength);
    let rate = if target < current { rate * 3.0 } else { rate };
    approach(current, target, rate * dt)
}

/// Traction control: throttle cut plus brake-based traction control (the "electronic diff
/// lock" of open-diff cars).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tcs {
    /// Current throttle cut 0..=0.9.
    pub cut: f32,
    /// Brake torque on each wheel from brake-based traction control, N·m (FL, FR, RL, RR).
    pub brake: [f32; 4],
}

/// Brake-based traction control gain: brake torque on the faster driven wheel per m/s of
/// wheel-speed difference beyond the dead band, N·m/(m/s), on top of the feed-forward
/// ([`BtcAxle::ff`]).
pub const BTC_GAIN: f32 = 600.0;
/// Dead band of the left-right surface-speed difference (after removing what the cornering
/// geometry asks for), m/s, plus [`BTC_DEADBAND_PER_MPS`] × road speed.
pub const BTC_DEADBAND_MPS: f32 = 0.6;
/// Speed-proportional part of the dead band, (m/s) per (m/s).
pub const BTC_DEADBAND_PER_MPS: f32 = 0.04;
/// How fast the brake torque may build or release, N·m/s (a hydraulic modulator).
pub const BTC_RATE: f32 = 15000.0;
/// Most brake torque brake-TC uses, fraction of the wheel's service-brake maximum.
pub const BTC_MAX_FRAC: f32 = 1.0;

/// Brake-TC torque on a wheel (N·m, last tick) above which the TCS throttle cut leaves that
/// wheel's spin to the brakes and watches the slower wheel of the axle instead.
pub const TCS_BRAKE_ACTIVE_NM: f32 = 50.0;
/// From this fraction of brake-TC's limit up to the limit, the throttle cut blends back to
/// watching the faster wheel (the brakes are running out, the engine has to back off).
pub const TCS_BRAKE_HANDOFF: f32 = 0.8;

/// One driven axle as brake-based traction control sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BtcAxle {
    /// Left wheel index.
    pub l: usize,
    /// Right wheel index.
    pub r: usize,
    /// Left tire surface speed `ω·r`, m/s.
    pub v_l: f32,
    /// Right tire surface speed, m/s.
    pub v_r: f32,
    /// `v_l − v_r` the cornering geometry asks for (yaw rate × track, inside slower), m/s.
    pub v_kin: f32,
    /// Feed-forward per side (left, right): drive torque reaching the wheel minus what its tire
    /// resists, N·m (≥ 0; last tick). The brake that holds a spinning wheel steady.
    pub ff: [f32; 2],
    /// Brake torque limit per wheel, N·m.
    pub max_nm: f32,
    /// Authority 0..=1 (rear axles fade out with speed, see [`btc_rear_authority`]).
    pub authority: f32,
}

/// Rear-axle brake-TC fades out between these road speeds, m/s: braking one rear wheel adds
/// yaw, so on a driven rear axle it is a launch / crawl aid (split-µ pull-away) and the
/// throttle cut handles spin at speed. Front axles keep full authority.
pub const BTC_REAR_FADE_START_MPS: f32 = 6.0;
/// See [`BTC_REAR_FADE_START_MPS`].
pub const BTC_REAR_FADE_END_MPS: f32 = 12.0;

/// Brake-TC authority on a driven rear axle at road speed `speed` (m/s).
pub fn btc_rear_authority(speed: f32) -> f32 {
    ((BTC_REAR_FADE_END_MPS - speed.abs()) / (BTC_REAR_FADE_END_MPS - BTC_REAR_FADE_START_MPS))
        .clamp(0.0, 1.0)
}

impl Tcs {
    /// Update from the largest driven-wheel slip ratio and return the throttle multiplier.
    pub fn update(&mut self, enabled: bool, driven_slip: f32, peak_slip: f32, dt: f32) -> f32 {
        if !enabled {
            self.cut = 0.0;
            return 1.0;
        }
        // Proportional: no cut up to 1.2× peak slip, full (0.9) cut at 3.2× peak.
        let target = ((driven_slip - 1.2 * peak_slip) / (2.0 * peak_slip)).clamp(0.0, 0.9);
        let rate = if target > self.cut { 15.0 } else { 4.0 };
        self.cut = approach(self.cut, target, rate * dt);
        1.0 - self.cut
    }

    /// Brake-based traction control: on every listed axle (driven, diff not locked) brake the
    /// wheel that spins faster than its partner by more than the cornering geometry explains,
    /// so an open (or weakly locking) diff passes the torque to the wheel with grip. Only while
    /// `driving` (on the throttle, off the brake); releases at [`BTC_RATE`] otherwise. Returns
    /// the per-wheel brake torque to add, N·m.
    pub fn update_brakes(
        &mut self,
        enabled: bool,
        driving: bool,
        axles: &[BtcAxle],
        speed: f32,
        dt: f32,
    ) -> [f32; 4] {
        if !enabled {
            self.brake = [0.0; 4];
            return self.brake;
        }
        let mut target = [0.0f32; 4];
        let mut limit = [0.0f32; 4];
        let band = BTC_DEADBAND_MPS + BTC_DEADBAND_PER_MPS * speed.abs();
        for a in axles {
            limit[a.l] = a.max_nm;
            limit[a.r] = a.max_nm;
            if driving {
                let excess = (a.v_l - a.v_r) - a.v_kin;
                let (fast, ff, e) = if excess >= 0.0 {
                    (a.l, a.ff[0], excess)
                } else {
                    (a.r, a.ff[1], -excess)
                };
                // Feed-forward fades in across the dead band, the proportional term beyond it.
                let hold = ff * (e / band).clamp(0.0, 1.0);
                let push = BTC_GAIN * (e - band).max(0.0);
                target[fast] = (a.authority * (hold + push)).clamp(0.0, a.max_nm);
            }
        }
        for i in 0..4 {
            self.brake[i] = approach(self.brake[i], target[i], BTC_RATE * dt).min(limit[i]);
        }
        self.brake
    }
}

/// Stability control output for one tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EscOutput {
    /// Extra brake torque per wheel (FL, FR, RL, RR), N·m.
    pub brake: [f32; 4],
    /// Throttle multiplier.
    pub throttle_scale: f32,
}

/// Understeer coefficient of the ESC's reference car, s²/m²: the yaw rate it expects is
/// `v·tan δ / (L·(1 + K·v²))`. 0.0007 ≈ 1° of road-wheel angle per g on a 2.6 m wheelbase,
/// a sporty-but-safe reference. (It was 0.0025 ≈ 3.7°/g, more understeer than any car on the
/// roster, so a car cornering normally read as "oversteering": ESC braked the outer front
/// and halved the throttle in 40–70% of a 0.64 g city corner, playtest row 28.)
pub const ESC_REFERENCE_K: f32 = 0.0007;
/// Yaw-rate excess that counts as oversteer, rad/s: this plus [`ESC_OVERSTEER_FRAC`] × the
/// expected yaw rate.
pub const ESC_OVERSTEER_BASE: f32 = 0.1;
/// See [`ESC_OVERSTEER_BASE`].
pub const ESC_OVERSTEER_FRAC: f32 = 0.15;

/// Time constant of the ESC's reference yaw rate, s: the car's yaw response does not follow
/// the steering instantly, so neither does the yaw rate the ESC expects. Without it, keyboard
/// taps (the rack swinging 0 → lock → 0 several times a second) read as alternating
/// under- and oversteer and the ESC braked and cut the throttle through ordinary corners.
pub const ESC_REFERENCE_LAG_S: f32 = 0.2;

/// Yaw rate the ESC expects (rad/s, positive = left) for centre steer angle `center_steer`
/// (rad, positive = left) at forward `speed` (m/s): the reference car's steady-state yaw rate
/// ([`ESC_REFERENCE_K`]), capped at `front_mu · g / v`.
pub fn esc_reference(center_steer: f32, speed: f32, wheelbase: f32, front_mu: f32) -> f32 {
    let v = speed.max(1.0);
    let cap = front_mu * wp_contracts::GRAVITY_MPS2 / v;
    (speed * crate::math::tan(center_steer) / (wheelbase * (1.0 + ESC_REFERENCE_K * v * v)))
        .clamp(-cap, cap)
}

/// Stability control state: the lagged reference yaw rate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Esc {
    /// Reference yaw rate after the [`ESC_REFERENCE_LAG_S`] lag, rad/s.
    pub reference: f32,
}

impl Esc {
    /// Advance the reference by `dt` and return this tick's output (see [`esc`]). With ESC
    /// off the state stays at 0 and the output is empty.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        enabled: bool,
        yaw_rate: f32,
        center_steer: f32,
        speed: f32,
        wheelbase: f32,
        front_mu: f32,
        dt: f32,
    ) -> EscOutput {
        if !enabled {
            self.reference = 0.0;
            return esc(false, yaw_rate, 0.0, speed);
        }
        let raw = esc_reference(center_steer, speed, wheelbase, front_mu);
        self.reference += (raw - self.reference) * (dt / ESC_REFERENCE_LAG_S).min(1.0);
        esc(true, yaw_rate, self.reference, speed)
    }
}

/// Yaw-rate tracking ESC. `yaw_rate` (+ = turning left), `desired` the yaw rate it expects
/// (see [`Esc`]), `speed` forward m/s. Conservative: acts only on clear oversteer (yaw rate
/// above the reference by [`ESC_OVERSTEER_BASE`] + [`ESC_OVERSTEER_FRAC`] × reference: the
/// outer front brakes in proportion to the excess, throttle halved) or understeer (0.15 rad/s
/// below: the inner rear brakes, throttle × 0.7).
pub fn esc(enabled: bool, yaw_rate: f32, desired: f32, speed: f32) -> EscOutput {
    let mut out = EscOutput {
        brake: [0.0; 4],
        throttle_scale: 1.0,
    };
    if !enabled || speed < 8.0 {
        return out;
    }
    let excess = yaw_rate.abs() - desired.abs();
    let same_dir = yaw_rate * desired >= 0.0;
    let threshold = ESC_OVERSTEER_BASE + ESC_OVERSTEER_FRAC * desired.abs();
    if excess > threshold || (!same_dir && yaw_rate.abs() > 0.15) {
        // Oversteer: brake the outer front wheel.
        let outer = if yaw_rate > 0.0 { 1 } else { 0 };
        let over = if same_dir {
            excess - threshold
        } else {
            yaw_rate.abs() - 0.1
        };
        out.brake[outer] = (over * 4000.0).clamp(0.0, 1500.0);
        out.throttle_scale = 0.5;
    } else if same_dir && excess < -0.15 {
        // Understeer: brake the inner rear wheel.
        let inner = if desired > 0.0 { 2 } else { 3 };
        out.brake[inner] = ((-excess - 0.15) * 2500.0).clamp(0.0, 800.0);
        out.throttle_scale = 0.7;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_ramps_and_releases_faster() {
        let up = filter_pedal(0.0, 1.0, 1.0, 0.1);
        let down = 1.0 - filter_pedal(1.0, 0.0, 1.0, 0.1);
        assert!(up > 0.0 && up < 1.0 && down > up * 2.9);
        assert_eq!(filter_pedal(0.0, 1.0, 0.0, 0.1), 1.0);
    }

    #[test]
    fn tcs_cuts_on_wheelspin() {
        let mut t = Tcs::default();
        let m = t.update(true, 0.5, 0.1, 0.05);
        assert_eq!(Tcs::default().update(true, 0.1, 0.1, 0.05), 1.0);
        assert!(m < 1.0);
        assert_eq!(Tcs::default().update(false, 0.5, 0.1, 0.05), 1.0);
    }

    #[test]
    fn brake_tc_brakes_the_spinning_wheel_only() {
        let axle = BtcAxle {
            l: 2,
            r: 3,
            v_l: 12.0,
            v_r: 4.0,
            v_kin: 0.0,
            ff: [0.0; 2],
            max_nm: 1500.0,
            authority: 1.0,
        };
        let mut t = Tcs::default();
        let mut b = [0.0; 4];
        for _ in 0..100 {
            b = t.update_brakes(true, true, &[axle], 4.0, 0.01);
        }
        assert!(b[2] > 1000.0 && b[3] == 0.0, "{b:?}");
        // A cornering speed difference is not wheelspin.
        let corner = BtcAxle { v_kin: 8.0, ..axle };
        let mut t = Tcs::default();
        assert_eq!(t.update_brakes(true, true, &[corner], 4.0, 0.01), [0.0; 4]);
        // Rear axles fade out with speed.
        assert_eq!(btc_rear_authority(3.0), 1.0);
        assert_eq!(btc_rear_authority(9.0), 0.5);
        assert_eq!(btc_rear_authority(20.0), 0.0);
        // Off the throttle: released at the modulator rate; TCS off: cleared at once.
        let mut t = Tcs {
            brake: [0.0, 0.0, 300.0, 0.0],
            ..Tcs::default()
        };
        assert_eq!(t.update_brakes(true, false, &[axle], 4.0, 0.01)[2], 150.0);
        let mut t = Tcs {
            brake: [0.0, 0.0, 30.0, 0.0],
            ..Tcs::default()
        };
        assert_eq!(t.update_brakes(false, true, &[axle], 4.0, 0.01), [0.0; 4]);
    }

    #[test]
    fn esc_brakes_outer_front_on_oversteer() {
        let desired = esc_reference(0.02, 20.0, 2.7, 1.0);
        let o = esc(true, 0.8, desired, 20.0);
        assert!(o.brake[1] > 0.0);
        assert_eq!(esc(false, 0.8, desired, 20.0).brake, [0.0; 4]);
    }

    #[test]
    fn esc_leaves_ordinary_cornering_alone() {
        // A near-neutral car (0.5°/g) at 0.8 g on an 80 m circle: steer = L/R + 0.5° × 0.8.
        let (v, r, l) = (25.0f32, 80.0f32, 2.6f32);
        let steer = l / r + (0.5f32 * 0.8).to_radians();
        let yaw = v / r;
        let mut e = Esc::default();
        let mut o = EscOutput::default();
        for _ in 0..240 {
            o = e.update(true, yaw, steer, v, l, 1.0, 1.0 / 240.0);
        }
        assert_eq!(o.brake, [0.0; 4]);
        assert_eq!(o.throttle_scale, 1.0);
    }

    #[test]
    fn esc_reference_ignores_keyboard_taps() {
        // Rack swinging 0 ↔ 0.08 rad at 5 Hz while the car yaws steadily at the mean.
        let (v, l) = (20.0f32, 2.6f32);
        let yaw = esc_reference(0.04, v, l, 1.0);
        let mut e = Esc { reference: yaw };
        for i in 0..480 {
            let steer = if (i / 24) % 2 == 0 { 0.08 } else { 0.0 };
            let o = e.update(true, yaw, steer, v, l, 1.0, 1.0 / 240.0);
            assert_eq!(o.throttle_scale, 1.0, "tick {i}");
        }
    }
}
