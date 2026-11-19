//! Primitive encoding and the bounded payload decoder (see the crate docs for the schema).

use crate::error::{EncodeError, ProtoError};
use fid_io_reg::{Budget, Position};

/// Nominal tracked-allocation cost of one decoded scalar (`f64` or index). Costs are
/// fixed numbers, not `size_of`, so a limit trips at the same input on every platform;
/// each is at least the `size_of` of what it stands for on 64-bit targets (checked by the
/// unit test `costs_cover_real_sizes`), so tracked allocation never under-counts.
pub(crate) const COST_SCALAR: u64 = 8;
/// Nominal cost of one text or `WireText` slot (a boxed `str`), on top of its bytes.
pub(crate) const COST_SLOT: u64 = 64;
/// Nominal cost of one decoded [`crate::Record`] in a `Records` batch (the `Vec` is
/// allocated at its exact final capacity, so there is no growth on top).
pub(crate) const COST_RECORD: u64 = 160;
/// Nominal cost of one entry of a session's record table (`Vec<RecordKind>` grown by
/// doubling: at most two bytes per entry).
pub(crate) const COST_TABLE_ENTRY: u64 = 8;

/// Appends little-endian primitives to a payload.
#[derive(Debug, Default)]
pub(crate) struct Writer {
    pub(crate) buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub(crate) fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    pub(crate) fn f64(&mut self, v: f64) -> Result<(), EncodeError> {
        if !v.is_finite() {
            return Err(EncodeError::NonFinite);
        }
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }
    pub(crate) fn len(&mut self, n: usize) -> Result<(), EncodeError> {
        let n = u32::try_from(n).map_err(|_| EncodeError::TooLong)?;
        self.u32(n);
        Ok(())
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) -> Result<(), EncodeError> {
        self.len(b.len())?;
        self.buf.extend_from_slice(b);
        Ok(())
    }
    pub(crate) fn f64s(&mut self, v: &[f64]) -> Result<(), EncodeError> {
        self.len(v.len())?;
        v.iter().try_for_each(|&x| self.f64(x))
    }
    pub(crate) fn u32s(&mut self, v: &[u32]) -> Result<(), EncodeError> {
        self.len(v.len())?;
        v.iter().for_each(|&x| self.u32(x));
        Ok(())
    }
}

/// Reads one payload. Every failure is `invalid_message` naming the frame, except an
/// allocation charge past the budget, which is the harness's `limit_exceeded{alloc}`.
pub(crate) struct Decoder<'a, 'b> {
    s: &'a [u8],
    i: usize,
    frame: u64,
    at: Position,
    budget: &'b mut Budget,
}

impl<'a, 'b> Decoder<'a, 'b> {
    /// `at` is the input position of the frame (used for limit errors).
    pub(crate) fn new(payload: &'a [u8], frame: u64, at: Position, budget: &'b mut Budget) -> Self {
        Decoder { s: payload, i: 0, frame, at, budget }
    }

    pub(crate) fn invalid(&self, detail: &'static str) -> ProtoError {
        ProtoError::InvalidMessage { frame: self.frame, detail }
    }

    fn remaining(&self) -> usize {
        self.s.len().saturating_sub(self.i)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        let end = self.i.checked_add(n).ok_or_else(|| self.invalid("payload_truncated"))?;
        let s: &'a [u8] = self.s;
        let out = s.get(self.i..end).ok_or_else(|| self.invalid("payload_truncated"))?;
        self.i = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtoError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub(crate) fn charge(&mut self, n: u64) -> Result<(), ProtoError> {
        self.budget.charge_alloc(n, self.at).map_err(ProtoError::Harness)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProtoError> {
        Ok(u8::from_le_bytes(self.array()?))
    }
    pub(crate) fn u16(&mut self) -> Result<u16, ProtoError> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, ProtoError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, ProtoError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub(crate) fn bool(&mut self) -> Result<bool, ProtoError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(self.invalid("bad_bool")),
        }
    }
    /// An `Option` tag: `false` for absent, `true` for present.
    pub(crate) fn present(&mut self) -> Result<bool, ProtoError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(self.invalid("bad_option_tag")),
        }
    }
    pub(crate) fn f64(&mut self) -> Result<f64, ProtoError> {
        let v = f64::from_bits(self.u64()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(self.invalid("non_finite_float"))
        }
    }

    /// A `u32` count of items each at least `min_item` bytes on the wire. The count is
    /// checked against the bytes left before anything is allocated, then
    /// `count * cost` is charged as tracked allocation.
    pub(crate) fn count(&mut self, min_item: usize, cost: u64) -> Result<usize, ProtoError> {
        let n = usize::try_from(self.u32()?).map_err(|_| self.invalid("count_exceeds_payload"))?;
        let need = n.checked_mul(min_item).ok_or_else(|| self.invalid("count_exceeds_payload"))?;
        if need > self.remaining() {
            return Err(self.invalid("count_exceeds_payload"));
        }
        self.charge((n as u64).saturating_mul(cost))?;
        Ok(n)
    }

    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>, ProtoError> {
        let n = self.count(1, 1)?;
        Ok(self.take(n)?.to_vec())
    }

    /// A UTF-8 string; invalid UTF-8 is `invalid_message` (no lossy repair on the wire).
    pub(crate) fn text(&mut self) -> Result<Box<str>, ProtoError> {
        let n = self.count(1, 1)?;
        self.charge(COST_SLOT)?;
        let raw = self.take(n)?;
        match std::str::from_utf8(raw) {
            Ok(s) => Ok(Box::from(s)),
            Err(_) => Err(self.invalid("bad_utf8")),
        }
    }

    pub(crate) fn f64s(&mut self) -> Result<Vec<f64>, ProtoError> {
        let n = self.count(8, COST_SCALAR)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.f64()?);
        }
        Ok(v)
    }

    pub(crate) fn f64_array<const N: usize>(&mut self) -> Result<[f64; N], ProtoError> {
        let mut out = [0.0; N];
        for x in out.iter_mut() {
            *x = self.f64()?;
        }
        Ok(out)
    }

    pub(crate) fn u32s(&mut self) -> Result<Vec<u32>, ProtoError> {
        let n = self.count(4, COST_SCALAR)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.u32()?);
        }
        Ok(v)
    }

    /// Fails `trailing_bytes` unless the whole payload was consumed.
    pub(crate) fn finish(&self) -> Result<(), ProtoError> {
        if self.i == self.s.len() {
            Ok(())
        } else {
            Err(self.invalid("trailing_bytes"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{COST_RECORD, COST_SCALAR, COST_SLOT, COST_TABLE_ENTRY};
    use crate::{Record, RecordKind, WireText};
    use std::mem::size_of;

    fn size(n: usize) -> u64 {
        n as u64
    }

    #[test]
    fn costs_cover_real_sizes() {
        assert!(COST_SCALAR >= size(size_of::<f64>()));
        assert!(COST_SCALAR >= size(size_of::<u32>()));
        assert!(COST_SLOT >= size(size_of::<WireText>()));
        assert!(COST_SLOT >= size(size_of::<Option<WireText>>()));
        assert!(COST_RECORD >= size(size_of::<Record>()), "size_of::<Record>() = {}", size_of::<Record>());
        assert!(COST_TABLE_ENTRY >= 2 * size(size_of::<RecordKind>()));
    }
}
