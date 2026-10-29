//! RGBA canvas, PNG writer and a tiny pixel font for the detail painter.
//!
//! The PNG writer is ours (8-bit RGBA, filter "Sub" per row, zlib via `miniz_oxide`, already a
//! workspace dependency and pure Rust): deterministic bytes for the same pixels.

/// Linear-ish RGBA texel, each channel 0..1 (stored as sRGB-encoded values: the painter works
/// in display colours, like the reference art).
pub type Rgba = [f32; 4];

/// A square RGBA float canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    /// Width and height, px.
    pub size: u32,
    /// Row-major texels (`size`²).
    pub px: Vec<Rgba>,
}

impl Canvas {
    /// A canvas filled with `c`.
    pub fn new(size: u32, c: Rgba) -> Canvas {
        Canvas {
            size,
            px: vec![c; (size * size) as usize],
        }
    }

    /// Texel index of (x, y).
    pub fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.size + x) as usize
    }

    /// The canvas as RGBA8 (rounded, clamped).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            for c in p {
                out.push((c.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        out
    }
}

/// CRC-32 (IEEE) of `data`.
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// Encode `w` × `h` RGBA8 pixels as a PNG (deterministic).
pub fn png_rgba8(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = (w * 4) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    for y in 0..h as usize {
        let row = &rgba[y * stride..(y + 1) * stride];
        raw.push(1); // filter: Sub
        for x in 0..stride {
            let left = if x >= 4 { row[x - 4] } else { 0 };
            raw.push(row[x].wrapping_sub(left));
        }
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 9);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

/// Decode a PNG written by [`png_rgba8`] (tests; RGBA8, filters 0–4) into (w, h, RGBA8).
pub fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.len() < 8 || &bytes[1..4] != b"PNG" {
        return None;
    }
    let mut i = 8;
    let (mut w, mut h) = (0u32, 0u32);
    let mut idat = Vec::new();
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().ok()?) as usize;
        let kind = &bytes[i + 4..i + 8];
        let data = bytes.get(i + 8..i + 8 + len)?;
        match kind {
            b"IHDR" => {
                w = u32::from_be_bytes(data[0..4].try_into().ok()?);
                h = u32::from_be_bytes(data[4..8].try_into().ok()?);
                if data[8] != 8 || data[9] != 6 {
                    return None;
                }
            }
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        i += 12 + len;
    }
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&idat).ok()?;
    let stride = (w * 4) as usize;
    let mut out = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        let f = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= 4 { out[y * stride + x - 4] } else { 0 };
            let b = if y > 0 { out[(y - 1) * stride + x] } else { 0 };
            let c = if x >= 4 && y > 0 {
                out[(y - 1) * stride + x - 4]
            } else {
                0
            };
            let pred = match f {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                _ => {
                    let p = a as i16 + b as i16 - c as i16;
                    let (pa, pb, pc) = (
                        (p - a as i16).abs(),
                        (p - b as i16).abs(),
                        (p - c as i16).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
            };
            out[y * stride + x] = src[x].wrapping_add(pred);
        }
    }
    Some((w, h, out))
}

/// FNV-1a 64 of `data`, 16 hex digits.
pub fn fnv1a_hex(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

/// Integer hash of a texel / seed to 0..1 (deterministic noise).
pub fn hash01(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x
        .wrapping_mul(0x8da6_b343)
        .wrapping_add(y.wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// 5×7 pixel glyphs (rows top to bottom, 5 bits each, MSB = left) for the characters badges and
/// plates use. Unknown characters draw nothing.
pub fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        _ => [0; 7],
    }
}

/// Whether text `s` (5×7 glyphs, 1-column spacing) covers the point (u, v) of its box, both
/// 0..1 across the whole string (u right, v down).
pub fn text_hit(s: &str, u: f32, v: f32) -> bool {
    let n = s.chars().count();
    if n == 0 || !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return false;
    }
    let cols = n * 6 - 1;
    let cx = (u * cols as f32) as usize;
    let cy = (v * 7.0) as usize;
    if cx % 6 == 5 {
        return false;
    }
    let Some(ch) = s.chars().nth(cx / 6) else {
        return false;
    };
    glyph(ch)[cy.min(6)] & (0x10 >> (cx % 6)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trips() {
        let mut px = Vec::new();
        for i in 0..(16 * 8) {
            px.extend_from_slice(&[(i * 7) as u8, (i * 3) as u8, 200, (255 - i) as u8]);
        }
        let png = png_rgba8(16, 8, &px);
        assert_eq!(decode_png(&png), Some((16, 8, px.clone())));
        assert_eq!(png, png_rgba8(16, 8, &px));
    }

    #[test]
    fn text_draws_glyphs() {
        assert!(text_hit("T", 0.5, 0.05));
        assert!(!text_hit("T", 0.05, 0.5));
    }
}
