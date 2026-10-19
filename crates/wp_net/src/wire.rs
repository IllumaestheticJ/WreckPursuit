//! Byte-level reader/writer used by every message codec.
//!
//! Layout rules (see the README protocol spec):
//! - fixed-width integers and `f32` are little-endian;
//! - `varint` = unsigned LEB128 (7 bits per byte, low bits first, high bit = "more");
//! - `zigzag` = signed integer mapped to unsigned (`0,-1,1,-2,…` → `0,1,2,3,…`) then varint;
//! - `string` / `bytes` = varint length followed by the raw bytes (strings are UTF-8).

use crate::NetError;

/// Append-only byte buffer with typed writers.
#[derive(Debug, Default, Clone)]
pub struct ByteWriter {
    buf: Vec<u8>,
}

impl ByteWriter {
    /// Empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Finished bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True if nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Write one byte.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Write one signed byte.
    pub fn i8(&mut self, v: i8) {
        self.buf.push(v as u8);
    }

    /// Write a little-endian `u16`.
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `i16`.
    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `u32`.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `u64`.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `f32` (bit-exact).
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write an unsigned LEB128 varint.
    pub fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    /// Write a zigzag-encoded signed varint.
    pub fn zigzag(&mut self, v: i64) {
        self.varint(((v << 1) ^ (v >> 63)) as u64);
    }

    /// Write a bool as one byte (0/1).
    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }

    /// Write a length-prefixed byte string.
    pub fn bytes(&mut self, v: &[u8]) {
        self.varint(v.len() as u64);
        self.buf.extend_from_slice(v);
    }

    /// Write a length-prefixed UTF-8 string.
    pub fn string(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }

    /// Write a contract type as a length-prefixed postcard encoding (`typed<T>` in the README
    /// tables). Used for the v0.3.0 typed payloads (events, captain / race commands, weather),
    /// so the wire follows the contract types without a hand-written codec per variant.
    pub fn typed<T: serde::Serialize>(&mut self, v: &T) {
        // Serialising plain data into a Vec cannot fail (no maps with non-string keys, no
        // I/O); an empty payload would be rejected by the reader, never misread.
        let bytes = postcard::to_allocvec(v).unwrap_or_default();
        self.bytes(&bytes);
    }
}

/// Cursor over received bytes with typed readers. Every reader fails with
/// [`NetError::Decode`] instead of panicking on truncated or malformed input.
#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

fn truncated() -> NetError {
    NetError::Decode("message truncated".into())
}

impl<'a> ByteReader<'a> {
    /// Read from the start of `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], NetError> {
        if self.remaining() < n {
            return Err(truncated());
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Read one byte.
    pub fn u8(&mut self) -> Result<u8, NetError> {
        Ok(self.take(1)?[0])
    }

    /// Read one signed byte.
    pub fn i8(&mut self) -> Result<i8, NetError> {
        Ok(self.take(1)?[0] as i8)
    }

    /// Read a little-endian `u16`.
    pub fn u16(&mut self) -> Result<u16, NetError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    /// Read a little-endian `i16`.
    pub fn i16(&mut self) -> Result<i16, NetError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    /// Read a little-endian `u32`.
    pub fn u32(&mut self) -> Result<u32, NetError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// Read a little-endian `u64`.
    pub fn u64(&mut self) -> Result<u64, NetError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// Read a little-endian `f32`.
    pub fn f32(&mut self) -> Result<f32, NetError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// Read an unsigned LEB128 varint (at most 10 bytes).
    pub fn varint(&mut self) -> Result<u64, NetError> {
        let mut v = 0u64;
        for i in 0..10 {
            let byte = self.u8()?;
            v |= ((byte & 0x7f) as u64) << (7 * i);
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(NetError::Decode("varint too long".into()))
    }

    /// Read a varint that must fit in `u32`.
    pub fn varint_u32(&mut self) -> Result<u32, NetError> {
        u32::try_from(self.varint()?).map_err(|_| NetError::Decode("varint exceeds u32".into()))
    }

    /// Read a varint used as a collection length, rejecting anything above `max`.
    pub fn len(&mut self, max: usize) -> Result<usize, NetError> {
        let n = self.varint()? as usize;
        if n > max {
            return Err(NetError::Decode(format!("length {n} exceeds limit {max}")));
        }
        Ok(n)
    }

    /// Read a zigzag-encoded signed varint.
    pub fn zigzag(&mut self) -> Result<i64, NetError> {
        let u = self.varint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    /// Read a bool byte (anything non-zero is `true`).
    pub fn bool(&mut self) -> Result<bool, NetError> {
        Ok(self.u8()? != 0)
    }

    /// Read a length-prefixed byte string of at most `max` bytes.
    pub fn bytes(&mut self, max: usize) -> Result<Vec<u8>, NetError> {
        let n = self.len(max)?;
        Ok(self.take(n)?.to_vec())
    }

    /// Read a length-prefixed UTF-8 string of at most `max` bytes.
    pub fn string(&mut self, max: usize) -> Result<String, NetError> {
        String::from_utf8(self.bytes(max)?).map_err(|_| NetError::Decode("invalid utf-8".into()))
    }

    /// Read a `typed<T>` payload of at most `max` bytes (see [`ByteWriter::typed`]). The
    /// postcard body must be consumed exactly.
    pub fn typed<T: serde::de::DeserializeOwned>(&mut self, max: usize) -> Result<T, NetError> {
        let n = self.len(max)?;
        let body = self.take(n)?;
        let (v, rest) = postcard::take_from_bytes::<T>(body)
            .map_err(|e| NetError::Decode(format!("typed payload: {e}")))?;
        if !rest.is_empty() {
            return Err(NetError::Decode("typed payload: trailing bytes".into()));
        }
        Ok(v)
    }

    /// Fail unless every byte was consumed.
    pub fn finish(&self) -> Result<(), NetError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(NetError::Decode(format!(
                "{} trailing bytes",
                self.remaining()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_and_zigzag_round_trip() {
        let mut w = ByteWriter::new();
        let unsigned = [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX];
        let signed = [0i64, -1, 1, -64, 64, i32::MIN as i64, i64::MAX, i64::MIN];
        for v in unsigned {
            w.varint(v);
        }
        for v in signed {
            w.zigzag(v);
        }
        let bytes = w.into_bytes();
        let mut r = ByteReader::new(&bytes);
        for v in unsigned {
            assert_eq!(r.varint().unwrap(), v);
        }
        for v in signed {
            assert_eq!(r.zigzag().unwrap(), v);
        }
        r.finish().unwrap();
    }

    #[test]
    fn small_values_are_one_byte() {
        let mut w = ByteWriter::new();
        w.varint(127);
        w.zigzag(-64);
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let mut r = ByteReader::new(&[0x80, 0x80]);
        assert!(r.varint().is_err());
        let mut r = ByteReader::new(&[5, b'a']);
        assert!(r.string(16).is_err());
        let mut r = ByteReader::new(&[200]);
        assert!(r.bytes(16).is_err());
    }
}
