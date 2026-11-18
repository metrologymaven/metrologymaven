//! Declared exact encodings of data-plane payloads (REQ-HAL.IDL.UNITS-002 and -003).

use super::{f64_bits, kept, obj, object, string, take, Quantity, SiUnit, UnitsError};
use fid_canon::CValue;
use std::fmt;

/// The native numeric type of a raw payload value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NumericType {
    I16,
    I32,
    I64,
    U16,
    U32,
    F32,
    F64,
}

impl NumericType {
    /// Every type, in declaration order.
    pub const ALL: [NumericType; 7] = [
        NumericType::I16,
        NumericType::I32,
        NumericType::I64,
        NumericType::U16,
        NumericType::U32,
        NumericType::F32,
        NumericType::F64,
    ];

    /// The wire string, e.g. `i32`.
    pub fn as_str(self) -> &'static str {
        match self {
            NumericType::I16 => "i16",
            NumericType::I32 => "i32",
            NumericType::I64 => "i64",
            NumericType::U16 => "u16",
            NumericType::U32 => "u32",
            NumericType::F32 => "f32",
            NumericType::F64 => "f64",
        }
    }

    /// Parses the wire string; any other type makes the descriptor `encoding_invalid`.
    pub fn parse(s: &str) -> Result<NumericType, UnitsError> {
        NumericType::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| UnitsError::EncodingInvalid(format!("numeric type {:?} is not supported", kept(s))))
    }
}

impl fmt::Display for NumericType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One raw payload value in its native type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawValue {
    I16(i16),
    I32(i32),
    I64(i64),
    U16(u16),
    U32(u32),
    F32(f32),
    F64(f64),
}

impl RawValue {
    /// The native type of this value.
    pub fn numeric(self) -> NumericType {
        match self {
            RawValue::I16(_) => NumericType::I16,
            RawValue::I32(_) => NumericType::I32,
            RawValue::I64(_) => NumericType::I64,
            RawValue::U16(_) => NumericType::U16,
            RawValue::U32(_) => NumericType::U32,
            RawValue::F32(_) => NumericType::F32,
            RawValue::F64(_) => NumericType::F64,
        }
    }

    /// The value as `f64`: exact for every type except `i64` beyond ±2^53, which rounds to
    /// nearest, ties to even (IEEE 754 conversion).
    pub fn to_f64(self) -> f64 {
        match self {
            RawValue::I16(v) => f64::from(v),
            RawValue::I32(v) => f64::from(v),
            RawValue::I64(v) => v as f64,
            RawValue::U16(v) => f64::from(v),
            RawValue::U32(v) => f64::from(v),
            RawValue::F32(v) => f64::from(v),
            RawValue::F64(v) => v,
        }
    }
}

/// The declared exact encoding of a payload field: decoded = offset + scale × raw, in `unit`.
/// Wire form: `{"numeric": "i32", "offset": "0x…", "scale": "0x…", "unit": "m"}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncodingDescriptor {
    numeric: NumericType,
    scale: f64,
    offset: f64,
    unit: SiUnit,
}

impl EncodingDescriptor {
    /// The wire fields.
    pub const FIELDS: [&'static str; 4] = ["numeric", "offset", "scale", "unit"];

    /// A descriptor; a zero (either sign) or non-finite scale, or a non-finite offset, fails
    /// with `encoding_invalid` (REQ-003).
    pub fn new(numeric: NumericType, scale: f64, offset: f64, unit: SiUnit) -> Result<EncodingDescriptor, UnitsError> {
        if !scale.is_finite() {
            return Err(UnitsError::EncodingInvalid("scale is not finite".into()));
        }
        if scale == 0.0 {
            return Err(UnitsError::EncodingInvalid("scale is zero".into()));
        }
        if !offset.is_finite() {
            return Err(UnitsError::EncodingInvalid("offset is not finite".into()));
        }
        Ok(EncodingDescriptor { numeric, scale, offset, unit })
    }

    pub fn numeric(&self) -> NumericType {
        self.numeric
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn offset(&self) -> f64 {
        self.offset
    }

    pub fn unit(&self) -> SiUnit {
        self.unit
    }

    /// The decoded value of one raw value, as REQ-002 defines it: `offset + scale × raw`,
    /// evaluated as one IEEE 754 multiply and then one add (never fused), so every platform
    /// gets the same bits (D0). A raw value of another native type fails with
    /// `encoding_invalid`; a NaN raw float or an overflow fails with `quantity_not_finite`.
    ///
    /// This is the reference definition for conformance tests; converting whole payloads is
    /// `HAL.ACQ.CONV`.
    pub fn decode(&self, raw: RawValue) -> Result<Quantity, UnitsError> {
        if raw.numeric() != self.numeric {
            return Err(UnitsError::EncodingInvalid(format!(
                "raw value is {} but the descriptor declares {}",
                raw.numeric(),
                self.numeric
            )));
        }
        let product = self.scale * raw.to_f64();
        Quantity::new(self.offset + product, self.unit)
    }

    /// The wire value.
    pub fn to_value(self) -> CValue {
        obj([
            ("numeric", CValue::Str(self.numeric.as_str().to_string())),
            ("offset", CValue::F64Bits(self.offset)),
            ("scale", CValue::F64Bits(self.scale)),
            ("unit", CValue::Str(self.unit.as_str().to_string())),
        ])
    }

    /// Reads and validates the wire value. An unknown numeric type or a target unit outside
    /// the [`SiUnit`] set is `encoding_invalid` (REQ-003), not `unit_unsupported`.
    pub fn from_value(v: CValue) -> Result<EncodingDescriptor, UnitsError> {
        let mut o = object(v, "encoding", &EncodingDescriptor::FIELDS)?;
        let numeric = NumericType::parse(&string("numeric", take(&mut o, "numeric")?)?)?;
        let scale = f64_bits("scale", take(&mut o, "scale")?)?;
        let offset = f64_bits("offset", take(&mut o, "offset")?)?;
        let unit_text = string("unit", take(&mut o, "unit")?)?;
        let unit = SiUnit::parse(&unit_text).map_err(|_| {
            UnitsError::EncodingInvalid(format!("target unit {:?} is not an SI unit of the quantity set", kept(&unit_text)))
        })?;
        EncodingDescriptor::new(numeric, scale, offset, unit)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn invalid_descriptors() {
        let m = SiUnit::Metre;
        for (s, o) in [(0.0, 0.0), (-0.0, 0.0), (f64::NAN, 0.0), (f64::INFINITY, 0.0), (1.0, f64::NAN), (1.0, f64::NEG_INFINITY)] {
            let e = EncodingDescriptor::new(NumericType::I32, s, o, m).map_err(|e| e.code());
            assert_eq!(e, Err("encoding_invalid"), "scale {s} offset {o}");
        }
    }

    #[test]
    fn decode_checks_type_and_overflow() {
        let d = EncodingDescriptor::new(NumericType::I16, 0.5, 1.0, SiUnit::Metre).unwrap();
        assert_eq!(d.decode(RawValue::I16(4)).map(|q| q.value()), Ok(3.0));
        assert_eq!(d.decode(RawValue::I32(4)).map_err(|e| e.code()), Err("encoding_invalid"));
        let big = EncodingDescriptor::new(NumericType::F64, f64::MAX, 0.0, SiUnit::Metre).unwrap();
        assert_eq!(big.decode(RawValue::F64(2.0)), Err(UnitsError::QuantityNotFinite));
    }
}
