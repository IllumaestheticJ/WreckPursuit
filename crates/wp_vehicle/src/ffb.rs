//! Force feedback: rack torque from front tire aligning torque + caster trail, road rumble
//! from suspension velocity and surface, speed-dependent damper.

use wp_contracts::{FfbOutput, SurfaceType};

/// Per-front-wheel FFB inputs.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FfbWheel {
    /// Lateral tire force, N (+ = left).
    pub fy: f32,
    /// Aligning torque, N·m (+ = counter-clockwise).
    pub mz: f32,
    /// Mechanical trail from caster, m.
    pub caster_trail: f32,
}

fn roughness(s: SurfaceType) -> f32 {
    match s {
        SurfaceType::Gravel | SurfaceType::Dirt => 0.35,
        SurfaceType::Grass => 0.25,
        SurfaceType::Curb => 0.6,
        SurfaceType::Metal => 0.15,
        _ => 0.0,
    }
}

/// `rack_torque = −Σ(mz − caster_trail·fy) / ratio · ffb_scale` (positive pushes toward +steer
/// = right), rumble 0..=1, damper 0..=1.
pub fn compute(
    front: [FfbWheel; 2],
    steering_ratio: f32,
    ffb_scale: f32,
    susp_velocity_sum: f32,
    surfaces: [SurfaceType; 4],
    speed: f32,
) -> FfbOutput {
    let axis: f32 = front.iter().map(|w| w.mz - w.caster_trail * w.fy).sum();
    let rough = surfaces.iter().map(|s| roughness(*s)).fold(0.0, f32::max);
    FfbOutput {
        rack_torque_nm: -axis / steering_ratio.max(1.0) * ffb_scale,
        road_rumble: (susp_velocity_sum * 0.15 + rough * (speed.abs() / 10.0).min(1.0))
            .clamp(0.0, 1.0),
        damper: (speed.abs() / 40.0).clamp(0.1, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_centres_toward_right_in_left_turn() {
        // Left turn: front tires push left (+fy), aligning torque clockwise (−mz).
        let w = FfbWheel {
            fy: 3000.0,
            mz: -60.0,
            caster_trail: 0.03,
        };
        let out = compute([w, w], 15.0, 1.0, 0.0, [SurfaceType::Asphalt; 4], 20.0);
        assert!(out.rack_torque_nm > 0.0);
    }
}
