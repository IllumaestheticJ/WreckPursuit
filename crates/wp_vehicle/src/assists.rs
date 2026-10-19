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

/// Yaw-rate tracking ESC. `yaw_rate` (+ = turning left), `center_steer` (rad, + = left),
/// `speed` forward m/s. Conservative: acts only on clear oversteer/understeer.
pub fn esc(
    enabled: bool,
    yaw_rate: f32,
    center_steer: f32,
    speed: f32,
    wheelbase: f32,
) -> EscOutput {
    let mut out = EscOutput {
        brake: [0.0; 4],
        throttle_scale: 1.0,
    };
    if !enabled || speed < 8.0 {
        return out;
    }
    let cap = 0.9 * wp_contracts::GRAVITY_MPS2 / speed;
    let desired = (speed * crate::math::tan(center_steer)
        / (wheelbase * (1.0 + 0.0025 * speed * speed)))
        .clamp(-cap, cap);
    let excess = yaw_rate.abs() - desired.abs();
    let same_dir = yaw_rate * desired >= 0.0;
    if excess > 0.1 || (!same_dir && yaw_rate.abs() > 0.15) {
        // Oversteer: brake the outer front wheel.
        let outer = if yaw_rate > 0.0 { 1 } else { 0 };
        out.brake[outer] = ((excess.max(yaw_rate.abs()) - 0.1) * 4000.0).clamp(0.0, 1500.0);
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
        let o = esc(true, 0.8, 0.02, 20.0, 2.7);
        assert!(o.brake[1] > 0.0);
        assert_eq!(esc(false, 0.8, 0.02, 20.0, 2.7).brake, [0.0; 4]);
    }
}
