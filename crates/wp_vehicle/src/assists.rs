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

/// Traction control: throttle-cut fraction state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tcs {
    /// Current throttle cut 0..=0.9.
    pub cut: f32,
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
