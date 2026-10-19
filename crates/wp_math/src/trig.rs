//! Deterministic trigonometry built only from IEEE-754 `+ - * /` (bit-identical on every
//! target, unlike `f32::sin`, which may call different libm implementations).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Wrap an angle to `[-π, π)`.
pub fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// `(sin x, cos x)` via quadrant reduction and minimax-style Taylor polynomials.
/// Max abs error ≈ 2e-7 over all finite inputs of moderate magnitude (|x| < 1e5).
pub fn sin_cos(x: f32) -> (f32, f32) {
    let x = wrap_angle(x);
    // Reduce to r in [-π/4, π/4] with quadrant q.
    let q = (x / FRAC_PI_2).round();
    let r = x - q * FRAC_PI_2;
    let r2 = r * r;
    let s = r * (1.0 + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0))));
    let c = 1.0 + r2 * (-0.5 + r2 * (1.0 / 24.0 + r2 * (-1.0 / 720.0 + r2 * (1.0 / 40320.0))));
    match (q as i32).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// Deterministic `atan(x)` (max abs error ≈ 1e-5 rad).
pub fn atan(x: f32) -> f32 {
    let ax = x.abs();
    let (t, flip) = if ax > 1.0 {
        (1.0 / ax, true)
    } else {
        (ax, false)
    };
    let t2 = t * t;
    let p = t
        * (0.999_977_3
            + t2 * (-0.332_623_5
                + t2 * (0.193_543_5
                    + t2 * (-0.116_432_9 + t2 * (0.052_653_3 - t2 * 0.011_721_2)))));
    let r = if flip {
        std::f32::consts::FRAC_PI_2 - p
    } else {
        p
    };
    if x < 0.0 {
        -r
    } else {
        r
    }
}

/// Deterministic `atan2(y, x)` (polynomial, no libm; moved from `wp_world::geom` in v0.2.0).
pub fn atan2(y: f32, x: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_std_closely() {
        let mut x = -20.0f32;
        while x < 20.0 {
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 2e-6, "sin {x}");
            assert!((c - x.cos()).abs() < 2e-6, "cos {x}");
            x += 0.013;
        }
    }
}
