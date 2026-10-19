//! Stable state hashing for determinism checks.

/// Quantise a value to a fixed-point integer with `scale` steps per unit
/// (e.g. `quantize(x, 1e4)` = 0.1 mm resolution for metres). NaN maps to `i64::MIN`.
pub fn quantize(v: f32, scale: f32) -> i64 {
    if v.is_nan() {
        return i64::MIN;
    }
    (v as f64 * scale as f64).round() as i64
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
