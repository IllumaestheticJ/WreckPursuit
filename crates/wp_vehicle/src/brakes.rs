//! Brakes: bias, handbrake and ABS.

use wp_contracts::BrakeDef;

/// ABS release duration per cycle, s (one release + reapply ≈ 15 Hz).
pub const ABS_RELEASE_S: f32 = 1.0 / 30.0;
/// Brake torque fraction kept during an ABS release.
pub const ABS_RELEASE_FRACTION: f32 = 0.45;

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

/// Per-wheel ABS state machine. Call once per tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Abs {
    release: [f32; 4],
}

impl Abs {
    /// Modulate brake torques. `slip_ratio` from the last tick, `vx` contact speeds,
    /// `peak_slip` per wheel.
    pub fn apply(
        &mut self,
        torques: &mut [f32; 4],
        slip_ratio: [f32; 4],
        vx: [f32; 4],
        peak_slip: [f32; 4],
        dt: f32,
    ) {
        for i in 0..4 {
            if torques[i] > 0.0 && vx[i].abs() > 2.0 && slip_ratio[i] < -1.4 * peak_slip[i] {
                self.release[i] = ABS_RELEASE_S;
            }
            if self.release[i] > 0.0 {
                torques[i] *= ABS_RELEASE_FRACTION;
                self.release[i] -= dt;
            }
        }
    }

    /// True if ABS is currently releasing any wheel.
    pub fn active(&self) -> bool {
        self.release.iter().any(|t| *t > 0.0)
    }
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
        abs.apply(&mut t, [-0.5, 0.0, 0.0, 0.0], [20.0; 4], [0.1; 4], 0.004);
        assert!(t[0] < 500.0 && t[1] == 1000.0);
    }
}
