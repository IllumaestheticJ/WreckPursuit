//! Aerodynamics: drag at the CG, lift/downforce at each axle.

use crate::math::V3;

/// Air density, kg/m³.
pub const AIR_DENSITY: f32 = 1.225;

/// Aero forces for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AeroForces {
    /// Drag force at the centre of mass (world), N.
    pub drag: V3,
    /// Vertical force at the front axle along chassis up, N (+ = lift).
    pub lift_front: f32,
    /// Vertical force at the rear axle along chassis up, N (+ = lift).
    pub lift_rear: f32,
}

/// Compute aero forces. `cl_front/rear` = net lift coefficient × area (lift − downforce), m².
pub fn aero_forces(
    vel: V3,
    forward: V3,
    cd_a: f32,
    drag_scale: f32,
    cl_front: f32,
    cl_rear: f32,
) -> AeroForces {
    let speed = vel.length();
    let vf = vel.dot(forward);
    let q = 0.5 * AIR_DENSITY * vf * vf;
    AeroForces {
        drag: vel * (-0.5 * AIR_DENSITY * cd_a * drag_scale * speed),
        lift_front: q * cl_front,
        lift_rear: q * cl_rear,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_opposes_motion_and_grows_with_square() {
        let f = |v: f32| {
            aero_forces(V3::new(0.0, v, 0.0), V3::Y, 0.66, 1.0, 0.0, 0.0)
                .drag
                .y
        };
        assert!(f(10.0) < 0.0);
        assert!((f(20.0) / f(10.0) - 4.0).abs() < 1e-4);
    }
}
