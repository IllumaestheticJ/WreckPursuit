//! Tiny `[f32; 3]` helpers used by the solver hot loops (no external math types, so the lattice
//! code reads the same as the contract types it consumes).

/// A 3-vector in chassis space.
pub type V3 = [f32; 3];

/// `a + b`.
#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`.
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a * s`.
#[inline]
pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `a + b * s`.
#[inline]
pub fn madd(a: V3, b: V3, s: f32) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

/// Dot product.
#[inline]
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Euclidean length.
#[inline]
pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// Unit vector, or `None` if `a` is (nearly) zero.
#[inline]
pub fn normalize(a: V3) -> Option<V3> {
    let l = length(a);
    if l > 1e-9 && l.is_finite() {
        Some(scale(a, 1.0 / l))
    } else {
        None
    }
}

/// Component-wise clamp of `a` into the box `[lo, hi]`.
#[inline]
pub fn clamp_box(a: V3, lo: V3, hi: V3) -> V3 {
    [
        a[0].clamp(lo[0], hi[0]),
        a[1].clamp(lo[1], hi[1]),
        a[2].clamp(lo[2], hi[2]),
    ]
}

/// Deterministic `atan` built from `+ - * /` only (max error ≈ 0.0015 rad), used for small
/// toe/camber angles. Avoids libm so native and WASM agree bit for bit.
pub fn atan_det(x: f32) -> f32 {
    // atan(x) = π/2 - atan(1/x) for |x| > 1; polynomial on [-1, 1].
    let poly = |t: f32| {
        t * (0.999_866
            + t * t
                * (-0.330_299_5 + t * t * (0.180_141 + t * t * (-0.085_133 + t * t * 0.020_835_1))))
    };
    if x.abs() <= 1.0 {
        poly(x)
    } else {
        let s = if x > 0.0 { 1.0 } else { -1.0 };
        s * std::f32::consts::FRAC_PI_2 - poly(1.0 / x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan_matches_std() {
        let mut x = -10.0f32;
        while x < 10.0 {
            assert!((atan_det(x) - x.atan()).abs() < 2e-3, "atan {x}");
            x += 0.01;
        }
    }
}
