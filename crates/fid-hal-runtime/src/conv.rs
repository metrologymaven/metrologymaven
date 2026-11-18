//! Deterministic conversion of declared encodings to f64 SI values (spec node `HAL.ACQ.CONV`,
//! REQ-HAL.ACQ-005).
//!
//! Drivers hand over raw payload arrays in the exact encoding their capability declares
//! ([`EncodingDescriptor`]); the runtime converts them here, the same way on every platform
//! (determinism class D0), so replay never depends on a conversion done inside vendor code.
//!
//! Each valid sample is widened exactly to f64 and converted as `offset + (scale × x)` with
//! two separately rounded IEEE 754 operations: one multiply, then one add. Rust never
//! contracts floating-point expressions into a fused multiply-add, and this module calls no
//! `mul_add`, so the product is always rounded before the addition (REQ-001). The loop is
//! scalar with no runtime SIMD dispatch.

use fid_hal_idl::{EncodingDescriptor, NumericType, SiUnit};
use std::fmt;

/// The largest integer magnitude that is converted (2^53): every integer up to it is exactly
/// representable as f64. A larger magnitude fails with `encoding_precision_loss` (REQ-002).
pub const MAX_EXACT_INT: u64 = 1 << 53;

/// A raw payload array, borrowed in its native numeric type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawArray<'a> {
    I16(&'a [i16]),
    I32(&'a [i32]),
    I64(&'a [i64]),
    U16(&'a [u16]),
    U32(&'a [u32]),
    F32(&'a [f32]),
    F64(&'a [f64]),
}

impl RawArray<'_> {
    /// The native type of the samples.
    pub fn numeric(&self) -> NumericType {
        match self {
            RawArray::I16(_) => NumericType::I16,
            RawArray::I32(_) => NumericType::I32,
            RawArray::I64(_) => NumericType::I64,
            RawArray::U16(_) => NumericType::U16,
            RawArray::U32(_) => NumericType::U32,
            RawArray::F32(_) => NumericType::F32,
            RawArray::F64(_) => NumericType::F64,
        }
    }

    /// The number of samples.
    pub fn len(&self) -> usize {
        match self {
            RawArray::I16(s) => s.len(),
            RawArray::I32(s) => s.len(),
            RawArray::I64(s) => s.len(),
            RawArray::U16(s) => s.len(),
            RawArray::U32(s) => s.len(),
            RawArray::F32(s) => s.len(),
            RawArray::F64(s) => s.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One validity bit per sample (`true` = valid), stored as a bitset. Bits past `len` are
/// always zero, so equal masks compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValidityMask {
    words: Vec<u64>,
    len: usize,
}

impl ValidityMask {
    /// A mask of `len` samples, all valid.
    pub fn all_valid(len: usize) -> ValidityMask {
        let mut words = vec![u64::MAX; len.div_ceil(64)];
        let rem = len % 64;
        if rem != 0 {
            if let Some(last) = words.last_mut() {
                *last = (1u64 << rem) - 1;
            }
        }
        ValidityMask { words, len }
    }

    /// A mask with one bit per element of `bits`.
    pub fn from_bools(bits: &[bool]) -> ValidityMask {
        let mut words = vec![0u64; bits.len().div_ceil(64)];
        for (i, &b) in bits.iter().enumerate() {
            if b {
                if let Some(w) = words.get_mut(i / 64) {
                    *w |= 1u64 << (i % 64);
                }
            }
        }
        ValidityMask { words, len: bits.len() }
    }

    /// The number of samples the mask covers.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether sample `i` is valid; `false` past the end.
    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.words.get(i / 64).is_some_and(|w| (w >> (i % 64)) & 1 == 1)
    }

    /// The number of valid samples.
    pub fn count_valid(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// Converted samples: `values[i]` is the SI value of raw sample `i` when `mask.get(i)`;
/// masked samples were skipped and hold `+0.0`, which carries no meaning (never NaN).
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    values: Vec<f64>,
    mask: ValidityMask,
    unit: SiUnit,
}

impl Decoded {
    /// All values, index-aligned with the raw array (masked entries are `+0.0`).
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The validity mask, identical to the one passed in (REQ-004).
    pub fn mask(&self) -> &ValidityMask {
        &self.mask
    }

    /// The SI unit of every value (the descriptor's unit).
    pub fn unit(&self) -> SiUnit {
        self.unit
    }

    /// The value of sample `i`, or `None` when it is masked or out of range.
    pub fn get(&self, i: usize) -> Option<f64> {
        if self.mask.get(i) {
            self.values.get(i).copied()
        } else {
            None
        }
    }

    /// The valid samples as `(index, value)`, in index order.
    pub fn valid(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        self.values.iter().copied().enumerate().filter(|(i, _)| self.mask.get(*i))
    }

    pub fn into_parts(self) -> (Vec<f64>, ValidityMask, SiUnit) {
        (self.values, self.mask, self.unit)
    }
}

/// Why a conversion failed. Errors name the first offending index; conversion is all or
/// nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvError {
    /// The raw array's native type differs from the descriptor's (`encoding_invalid`).
    TypeMismatch { raw: NumericType, declared: NumericType },
    /// The raw array and the mask differ in length (`encoding_length_mismatch`, REQ-005).
    LengthMismatch { raw: usize, mask: usize },
    /// A valid integer sample's magnitude exceeds 2^53 (`encoding_precision_loss`, REQ-002).
    PrecisionLoss { index: usize },
    /// A valid float sample is NaN or infinite (`encoding_value_not_finite`, REQ-003).
    ValueNotFinite { index: usize },
    /// A finite sample converted to a non-finite value (overflow; `quantity_not_finite`, as in
    /// `EncodingDescriptor::decode`).
    ResultNotFinite { index: usize },
}

impl ConvError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ConvError::TypeMismatch { .. } => "encoding_invalid",
            ConvError::LengthMismatch { .. } => "encoding_length_mismatch",
            ConvError::PrecisionLoss { .. } => "encoding_precision_loss",
            ConvError::ValueNotFinite { .. } => "encoding_value_not_finite",
            ConvError::ResultNotFinite { .. } => "quantity_not_finite",
        }
    }

    /// The offending sample index, for per-sample errors.
    pub fn index(&self) -> Option<usize> {
        match self {
            ConvError::PrecisionLoss { index }
            | ConvError::ValueNotFinite { index }
            | ConvError::ResultNotFinite { index } => Some(*index),
            ConvError::TypeMismatch { .. } | ConvError::LengthMismatch { .. } => None,
        }
    }
}

impl fmt::Display for ConvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvError::TypeMismatch { raw, declared } => {
                write!(f, "encoding_invalid: raw array is {raw} but the descriptor declares {declared}")
            }
            ConvError::LengthMismatch { raw, mask } => {
                write!(f, "encoding_length_mismatch: {raw} raw values but a {mask}-bit mask")
            }
            ConvError::PrecisionLoss { index } => {
                write!(f, "encoding_precision_loss: integer magnitude above 2^53 at index {index}")
            }
            ConvError::ValueNotFinite { index } => {
                write!(f, "encoding_value_not_finite: NaN or infinite raw value at index {index}")
            }
            ConvError::ResultNotFinite { index } => {
                write!(f, "quantity_not_finite: conversion overflows at index {index}")
            }
        }
    }
}

impl std::error::Error for ConvError {}

/// Converts `raw` to f64 SI values as `desc` declares, skipping samples masked invalid.
///
/// Checks, in order: the raw type matches the descriptor; the raw and mask lengths agree;
/// then each valid sample in index order (precision, finiteness of the raw value, finiteness
/// of the result). The first failure is returned.
pub fn decode(raw: RawArray<'_>, desc: &EncodingDescriptor, mask: &ValidityMask) -> Result<Decoded, ConvError> {
    if raw.numeric() != desc.numeric() {
        return Err(ConvError::TypeMismatch { raw: raw.numeric(), declared: desc.numeric() });
    }
    if raw.len() != mask.len() {
        return Err(ConvError::LengthMismatch { raw: raw.len(), mask: mask.len() });
    }
    let (scale, offset) = (desc.scale(), desc.offset());
    let values = match raw {
        RawArray::I16(s) => convert(s, mask, scale, offset, |v, _| Ok(f64::from(v)))?,
        RawArray::I32(s) => convert(s, mask, scale, offset, |v, _| Ok(f64::from(v)))?,
        RawArray::U16(s) => convert(s, mask, scale, offset, |v, _| Ok(f64::from(v)))?,
        RawArray::U32(s) => convert(s, mask, scale, offset, |v, _| Ok(f64::from(v)))?,
        RawArray::I64(s) => convert(s, mask, scale, offset, widen_i64)?,
        RawArray::F32(s) => convert(s, mask, scale, offset, |v: f32, i| widen_float(f64::from(v), i))?,
        RawArray::F64(s) => convert(s, mask, scale, offset, widen_float)?,
    };
    Ok(Decoded { values, mask: mask.clone(), unit: desc.unit() })
}

/// Exact widening of an i64: magnitudes up to 2^53 only.
fn widen_i64(v: i64, index: usize) -> Result<f64, ConvError> {
    if v.unsigned_abs() > MAX_EXACT_INT {
        return Err(ConvError::PrecisionLoss { index });
    }
    // Exact: |v| <= 2^53.
    Ok(v as f64)
}

/// A float raw value (f32 widening is exact) must be finite.
fn widen_float(v: f64, index: usize) -> Result<f64, ConvError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ConvError::ValueNotFinite { index })
    }
}

/// The scalar conversion loop. Masked samples are skipped and hold `+0.0`.
fn convert<T: Copy>(
    raw: &[T],
    mask: &ValidityMask,
    scale: f64,
    offset: f64,
    widen: impl Fn(T, usize) -> Result<f64, ConvError>,
) -> Result<Vec<f64>, ConvError> {
    let mut out = Vec::with_capacity(raw.len());
    for (i, &r) in raw.iter().enumerate() {
        if !mask.get(i) {
            out.push(0.0);
            continue;
        }
        let x = widen(r, i)?;
        let v = affine(scale, offset, x);
        if !v.is_finite() {
            return Err(ConvError::ResultNotFinite { index: i });
        }
        out.push(v);
    }
    Ok(out)
}

/// `offset + (scale × x)` as two separately rounded operations (never fused, REQ-001).
#[inline]
fn affine(scale: f64, offset: f64, x: f64) -> f64 {
    let p = scale * x;
    offset + p
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn mask_all_valid_has_no_stray_bits() {
        for len in [0, 1, 63, 64, 65, 128, 1999] {
            let m = ValidityMask::all_valid(len);
            assert_eq!(m.len(), len);
            assert_eq!(m.count_valid(), len);
            assert!(!m.get(len));
            assert_eq!(m, ValidityMask::from_bools(&vec![true; len]));
        }
    }

    #[test]
    fn mask_from_bools_round_trips() {
        let bits: Vec<bool> = (0..200).map(|i| i % 3 != 0).collect();
        let m = ValidityMask::from_bools(&bits);
        for (i, &b) in bits.iter().enumerate() {
            assert_eq!(m.get(i), b, "{i}");
        }
        assert_eq!(m.count_valid(), bits.iter().filter(|b| **b).count());
    }

    #[test]
    fn type_mismatch_is_encoding_invalid() {
        let d = EncodingDescriptor::new(NumericType::I32, 1.0, 0.0, SiUnit::Metre).unwrap();
        let e = decode(RawArray::I16(&[1]), &d, &ValidityMask::all_valid(1)).unwrap_err();
        assert_eq!(e.code(), "encoding_invalid");
        assert_eq!(e.index(), None);
    }
}
