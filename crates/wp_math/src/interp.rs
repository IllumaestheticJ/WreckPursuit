//! Interpolation helpers.

/// Linear interpolation `a + (b - a) * t`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Component-wise [`lerp`] of 3-vectors.
pub fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ]
}

/// Normalised linear interpolation of unit quaternions `[x, y, z, w]` along the shortest arc.
pub fn nlerp_quat(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    let mut q = [0.0; 4];
    for i in 0..4 {
        q[i] = lerp(a[i], b[i] * sign, t);
    }
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len > 0.0 {
        for c in &mut q {
            *c /= len;
        }
    }
    q
}

/// Clamp to `[0, 1]`.
pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// Hermite smoothstep of `x` between `edge0` and `edge1`.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp01((x - edge0) / (edge1 - edge0));
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp(2.0, 4.0, 0.0), 2.0);
        assert_eq!(lerp(2.0, 4.0, 1.0), 4.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    }

    #[test]
    fn nlerp_stays_unit() {
        let a = [0.0, 0.0, 0.0, 1.0];
        let b = [0.0, 0.0, 1.0, 0.0];
        let q = nlerp_quat(a, b, 0.3);
        let len = q.iter().map(|c| c * c).sum::<f32>();
        assert!((len - 1.0).abs() < 1e-5);
    }
}
