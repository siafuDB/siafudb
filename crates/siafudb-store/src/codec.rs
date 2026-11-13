//! Little-endian encoding with bounds-checked decoding.

use crate::Result;
use crate::store::StoreError;

#[derive(Default)]
pub(crate) struct Enc(pub Vec<u8>);

impl Enc {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn raw(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
}

pub(crate) struct Dec<'a> {
    buf: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Dec<'a> {
    pub fn new(buf: &'a [u8], what: &'static str) -> Self {
        Self { buf, pos: 0, what }
    }
    fn err(&self) -> StoreError {
        StoreError::Corrupt(format!("{}: truncated at byte {}", self.what, self.pos))
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or_else(|| self.err())?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub fn arr<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("length checked"))
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.arr::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_truncation() {
        let mut e = Enc::default();
        e.u8(1)
            .u16(0x0203)
            .u32(0x0405_0607)
            .u64(u64::MAX)
            .raw(b"xy");
        let mut d = Dec::new(&e.0, "t");
        assert_eq!(d.u8().unwrap(), 1);
        assert_eq!(d.u16().unwrap(), 0x0203);
        assert_eq!(d.u32().unwrap(), 0x0405_0607);
        assert_eq!(d.u64().unwrap(), u64::MAX);
        assert_eq!(d.take(2).unwrap(), b"xy");
        assert_eq!(d.remaining(), 0);
        assert!(matches!(d.u8(), Err(StoreError::Corrupt(_))));
        assert!(matches!(
            Dec::new(b"abc", "t").take(usize::MAX),
            Err(StoreError::Corrupt(_))
        ));
    }
}
