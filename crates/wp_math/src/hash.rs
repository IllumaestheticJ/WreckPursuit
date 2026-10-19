//! Stable state hashing for determinism checks.

/// Quantise a value to a fixed-point integer with `scale` steps per unit
/// (e.g. `quantize(x, 1e4)` = 0.1 mm resolution for metres). NaN maps to `i64::MIN`.
pub fn quantize(v: f32, scale: f32) -> i64 {
    if v.is_nan() {
        return i64::MIN;
    }
    round_half_away(v as f64 * scale as f64)
}

/// `y.round() as i64` (half away from zero, saturating, NaN → 0) without the `round` libcall,
/// which neither x86-64 baseline nor wasm32 has as an instruction; bit-identical results (see
/// the `fast_round_matches_std` test). `y - trunc(y)` is exact for every `f64`: below 2^52 it
/// is the fractional part, above it `y` is already an integer.
fn round_half_away(y: f64) -> i64 {
    let t = y as i64;
    let frac = y - t as f64;
    if frac >= 0.5 {
        t.saturating_add(1)
    } else if frac <= -0.5 {
        t.saturating_sub(1)
    } else {
        t
    }
}

/// FNV-1a 64-bit hasher. Stable across platforms, versions and runs (unlike `std` hashers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateHasher {
    state: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHasher {
    const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01B3;

    /// New hasher with the standard FNV offset basis.
    pub fn new() -> Self {
        Self {
            state: Self::OFFSET,
        }
    }

    /// Mix raw bytes.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.state ^= *b as u64;
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }

    /// Mix a `u32` (little-endian).
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mix a `u64` (little-endian).
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mix an `i64` (little-endian).
    pub fn write_i64(&mut self, v: i64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mix an `f32` quantised with `scale` steps per unit (see [`quantize`]).
    pub fn write_f32_q(&mut self, v: f32, scale: f32) {
        self.write_i64(quantize(v, scale));
    }

    /// Current hash value.
    pub fn finish(&self) -> u64 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_round_matches_std() {
        let reference = |y: f64| y.round() as i64;
        let mut edge = vec![
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.5,
            -1.5,
            2.5,
            -2.5,
            0.499_999_999_999_999_94,
            -0.499_999_999_999_999_94,
            4_503_599_627_370_495.5,
            -4_503_599_627_370_495.5,
            4_503_599_627_370_496.0,
            9_007_199_254_740_993.0,
            9.223_372_036_854_775e18,
            -9.223_372_036_854_775e18,
            1e19,
            -1e19,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            f64::MIN_POSITIVE,
        ];
        // Random bit patterns and random values around every scale the hashes use.
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for i in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            edge.push(f64::from_bits(x));
            let f = f32::from_bits(x as u32);
            let scale = [1.0f32, 1e3, 1e4, 1e5][i % 4];
            edge.push(f as f64 * scale as f64);
            edge.push((x % 2_000_001) as f64 * 0.5 - 500_000.0);
        }
        for y in edge {
            assert_eq!(round_half_away(y), reference(y), "y = {y:e}");
        }
    }

    #[test]
    fn fnv_reference_value() {
        let mut h = StateHasher::new();
        h.write_bytes(b"a");
        assert_eq!(h.finish(), 0xAF63_DC4C_8601_EC8C);
    }

    #[test]
    fn quantisation_absorbs_noise() {
        assert_eq!(quantize(1.000_01, 1e3), quantize(1.0, 1e3));
        assert_eq!(quantize(-0.0, 1e3), quantize(0.0, 1e3));
    }
}
