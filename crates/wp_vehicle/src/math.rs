//! Deterministic scalar helpers (only IEEE `+ - * / sqrt`, so results are bit-identical on
//! native and WASM) and small vector utilities on Rapier's math types.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_6, PI};

pub use wp_math::sin_cos;

/// Rapier/glam 3-vector (glam runs in scalar-math mode under `enhanced-determinism`).
pub type V3 = rapier3d::math::Vector;
/// Rapier/glam unit quaternion.
pub type Quat = rapier3d::math::Rotation;

const SQRT3: f32 = 1.732_050_8;
const TAN_PI_12: f32 = 0.267_949_2;

/// Arc tangent, |error| < 1e-6 rad.
pub fn atan(x: f32) -> f32 {
    if x.is_nan() {
        return x;
    }
    let (sign, mut a) = if x < 0.0 { (-1.0, -x) } else { (1.0, x) };
    let invert = a > 1.0;
    if invert {
        a = 1.0 / a;
    }
    let mut offset = 0.0;
    if a > TAN_PI_12 {
        a = (a * SQRT3 - 1.0) / (SQRT3 + a);
        offset = FRAC_PI_6;
    }
    let a2 = a * a;
    let p = a
        * (1.0
            + a2 * (-1.0 / 3.0
                + a2 * (1.0 / 5.0 + a2 * (-1.0 / 7.0 + a2 * (1.0 / 9.0 + a2 * (-1.0 / 11.0))))));
    let mut r = p + offset;
    if invert {
        r = FRAC_PI_2 - r;
    }
    sign * r
}

/// Four-quadrant arc tangent of `y / x`.
pub fn atan2(y: f32, x: f32) -> f32 {
    if x > 0.0 {
        atan(y / x)
    } else if x < 0.0 {
        if y >= 0.0 {
            atan(y / x) + PI
        } else {
            atan(y / x) - PI
        }
    } else if y > 0.0 {
        FRAC_PI_2
    } else if y < 0.0 {
        -FRAC_PI_2
    } else {
        0.0
    }
}

/// Sine.
pub fn sin(x: f32) -> f32 {
    sin_cos(x).0
}

/// Tangent.
pub fn tan(x: f32) -> f32 {
    let (s, c) = sin_cos(x);
    s / c
}

/// Linear interpolation.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Move `current` toward `target` by at most `max_delta`.
pub fn approach(current: f32, target: f32, max_delta: f32) -> f32 {
    current + (target - current).clamp(-max_delta, max_delta)
}

/// Sign with a true zero.
pub fn sign0(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Convert `[f32; 3]` to a vector.
pub fn v3(a: [f32; 3]) -> V3 {
    V3::new(a[0], a[1], a[2])
}

/// Convert a vector to `[f32; 3]`.
pub fn arr(v: V3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// Quaternion for a heading `yaw` about +Z (0 = facing +Y), built without libm.
pub fn yaw_quat(yaw: f32) -> Quat {
    let (s, c) = sin_cos(yaw * 0.5);
    Quat::from_xyzw(0.0, 0.0, s, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan_matches_std() {
        let mut x = -50.0f32;
        while x < 50.0 {
            assert!((atan(x) - x.atan()).abs() < 1e-6, "atan {x}");
            x += 0.0137;
        }
        assert!((atan(f32::INFINITY) - FRAC_PI_2).abs() < 1e-7);
    }

    #[test]
    fn atan2_quadrants() {
        for (y, x) in [
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, -1.0),
            (-1.0, 1.0),
            (0.0, -2.0),
            (3.0, 0.0),
        ] {
            assert!((atan2(y, x) - f32::atan2(y, x)).abs() < 2e-6, "{y} {x}");
        }
    }
}
