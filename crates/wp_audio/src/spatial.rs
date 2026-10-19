//! Deterministic spatialisation inputs: distance attenuation, Doppler ratio and air absorption.
//! The backend only pans; everything distance-related is computed here so native and browser
//! (and every client of a replay) hear the same mix.

use crate::data::SpatialConfig;

/// Result of spatialising one source against the listener.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spatial {
    /// Distance gain, 0..=1.
    pub gain: f32,
    /// Doppler frequency ratio (> 1 approaching).
    pub doppler: f32,
    /// Low-pass cutoff, Hz.
    pub lowpass_hz: f32,
    /// Distance, m.
    pub distance_m: f32,
}

/// `a - b`.
pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Dot product.
pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Euclidean length.
pub fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// Rotate `v` by unit quaternion `q = [x, y, z, w]`.
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // t = 2 * cross(q.xyz, v); v' = v + w t + cross(q.xyz, t)
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// Rotate `v` by the inverse of unit quaternion `q` (world → chassis).
pub fn rotate_inv(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    rotate([-q[0], -q[1], -q[2], q[3]], v)
}

/// World point → chassis space of a body at `pos` with orientation `rot`.
pub fn world_to_local(pos: [f32; 3], rot: [f32; 4], p: [f32; 3]) -> [f32; 3] {
    rotate_inv(rot, sub(p, pos))
}

/// Web Audio "inverse" distance model, silenced beyond `max_distance_m`.
pub fn distance_gain(cfg: &SpatialConfig, d: f32) -> f32 {
    if d >= cfg.max_distance_m {
        return 0.0;
    }
    let r = cfg.ref_distance_m.max(1e-3);
    let d = d.max(r);
    r / (r + cfg.rolloff * (d - r))
}

/// Doppler ratio for a source at `src_pos` moving at `src_vel` heard by a listener at
/// `lis_pos` moving at `lis_vel`: `(c + v_l·u) / (c + v_s·u)` with `u` the unit vector from
/// listener to source (standard formula, velocities clamped below the speed of sound).
pub fn doppler(
    cfg: &SpatialConfig,
    src_pos: [f32; 3],
    src_vel: [f32; 3],
    lis_pos: [f32; 3],
    lis_vel: [f32; 3],
) -> f32 {
    let d = sub(src_pos, lis_pos);
    let dist = len(d);
    if dist < 1e-3 || cfg.doppler_factor <= 0.0 {
        return 1.0;
    }
    let u = [d[0] / dist, d[1] / dist, d[2] / dist];
    let c = cfg.speed_of_sound_mps;
    let lim = c * 0.9;
    // Positive = moving toward the other party along the line of sight.
    let v_l = (dot(lis_vel, u) * cfg.doppler_factor).clamp(-lim, lim);
    let v_s = (-dot(src_vel, u) * cfg.doppler_factor).clamp(-lim, lim);
    let ratio = (c + v_l) / (c - v_s);
    let m = cfg.max_doppler.max(1.0);
    ratio.clamp(1.0 / m, m)
}

/// Air-absorption low-pass cutoff at distance `d`: halves every `lowpass_half_distance_m`
/// (as a rational falloff, no transcendental functions).
pub fn lowpass_hz(cfg: &SpatialConfig, d: f32) -> f32 {
    cfg.lowpass_near_hz / (1.0 + d.max(0.0) / cfg.lowpass_half_distance_m.max(1.0))
}

/// Spatialise a source.
pub fn spatialise(
    cfg: &SpatialConfig,
    src_pos: [f32; 3],
    src_vel: [f32; 3],
    lis_pos: [f32; 3],
    lis_vel: [f32; 3],
) -> Spatial {
    let d = len(sub(src_pos, lis_pos));
    Spatial {
        gain: distance_gain(cfg, d),
        doppler: doppler(cfg, src_pos, src_vel, lis_pos, lis_vel),
        lowpass_hz: lowpass_hz(cfg, d),
        distance_m: d,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SpatialConfig {
        SpatialConfig {
            ref_distance_m: 5.0,
            max_distance_m: 400.0,
            rolloff: 1.0,
            speed_of_sound_mps: 343.0,
            doppler_factor: 1.0,
            max_doppler: 2.0,
            lowpass_near_hz: 20000.0,
            lowpass_half_distance_m: 80.0,
        }
    }

    #[test]
    fn gain_is_one_inside_ref_and_falls_off() {
        let c = cfg();
        assert_eq!(distance_gain(&c, 1.0), 1.0);
        assert!((distance_gain(&c, 10.0) - 0.5).abs() < 1e-6);
        assert!(distance_gain(&c, 100.0) < distance_gain(&c, 50.0));
        assert_eq!(distance_gain(&c, 500.0), 0.0);
    }

    #[test]
    fn doppler_rises_when_approaching_and_falls_when_receding() {
        let c = cfg();
        // Source 100 m ahead on +Y driving toward the listener at 30 m/s.
        let toward = doppler(&c, [0.0, 100.0, 0.0], [0.0, -30.0, 0.0], [0.0; 3], [0.0; 3]);
        let away = doppler(&c, [0.0, 100.0, 0.0], [0.0, 30.0, 0.0], [0.0; 3], [0.0; 3]);
        assert!(toward > 1.05 && toward < 1.15, "{toward}");
        assert!(away < 0.95 && away > 0.85, "{away}");
        // Physical value: c / (c - v).
        assert!((toward - 343.0 / 313.0).abs() < 1e-4);
        // Listener moving toward a still source also raises pitch.
        assert!(doppler(&c, [0.0, 100.0, 0.0], [0.0; 3], [0.0; 3], [0.0, 30.0, 0.0]) > 1.05);
    }

    #[test]
    fn rotate_round_trips() {
        // 90° yaw about +Z.
        let h = 0.5f32.sqrt();
        let q = [0.0, 0.0, h, h];
        let v = rotate(q, [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-6 && (v[1] - 1.0).abs() < 1e-6);
        let back = rotate_inv(q, v);
        assert!((back[0] - 1.0).abs() < 1e-6 && back[1].abs() < 1e-6);
    }
}
