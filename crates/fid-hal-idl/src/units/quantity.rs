//! SI units and quantities (REQ-HAL.IDL.UNITS-001, parent REQ-HAL.IDL-003).

use super::{f64_bits, kept, obj, object, string, take, UnitsError};
use fid_canon::CValue;
use std::fmt;

/// The closed set of control-plane units: SI metre, radian, second, kelvin, pascal and the
/// dimensionless unit `1`. Prefixed units (`mm`), angles in degrees and Celsius are not
/// units here; they belong to authored values and display (`GEO.FRAME.UNITS`, `VIZ`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SiUnit {
    Metre,
    Radian,
    Second,
    Kelvin,
    Pascal,
    /// Dimensionless (ratios, humidity as a fraction, counts of nothing in particular).
    One,
}

impl SiUnit {
    /// Every unit, in declaration order.
    pub const ALL: [SiUnit; 6] = [SiUnit::Metre, SiUnit::Radian, SiUnit::Second, SiUnit::Kelvin, SiUnit::Pascal, SiUnit::One];

    /// The wire symbol: `m`, `rad`, `s`, `K`, `Pa` or `1`.
    pub fn as_str(self) -> &'static str {
        match self {
            SiUnit::Metre => "m",
            SiUnit::Radian => "rad",
            SiUnit::Second => "s",
            SiUnit::Kelvin => "K",
            SiUnit::Pascal => "Pa",
            SiUnit::One => "1",
        }
    }

    /// Parses a wire symbol, case-sensitively; anything else is `unit_unsupported`.
    pub fn parse(s: &str) -> Result<SiUnit, UnitsError> {
        SiUnit::ALL.into_iter().find(|u| u.as_str() == s).ok_or_else(|| UnitsError::UnitUnsupported(kept(s)))
    }
}

impl fmt::Display for SiUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A finite `f64` value with an SI unit. Wire form: `{"unit": "m", "value": "0x…"}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    value: f64,
    unit: SiUnit,
}

impl Quantity {
    /// The wire fields.
    pub const FIELDS: [&'static str; 2] = ["unit", "value"];

    /// A quantity; NaN and infinities fail with `quantity_not_finite`.
    pub fn new(value: f64, unit: SiUnit) -> Result<Quantity, UnitsError> {
        if !value.is_finite() {
            return Err(UnitsError::QuantityNotFinite);
        }
        Ok(Quantity { value, unit })
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn unit(&self) -> SiUnit {
        self.unit
    }

    /// The wire value.
    pub fn to_value(self) -> CValue {
        obj([("unit", CValue::Str(self.unit.as_str().to_string())), ("value", CValue::F64Bits(self.value))])
    }

    /// Reads the wire value. The unit is checked before the value, so `{"unit": "mm",
    /// "value": NaN}` reports `unit_unsupported`.
    pub fn from_value(v: CValue) -> Result<Quantity, UnitsError> {
        let mut o = object(v, "quantity", &Quantity::FIELDS)?;
        let unit = SiUnit::parse(&string("unit", take(&mut o, "unit")?)?)?;
        let value = f64_bits("value", take(&mut o, "value")?)?;
        Quantity::new(value, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_round_trip_and_reject_others() {
        for u in SiUnit::ALL {
            assert_eq!(SiUnit::parse(u.as_str()), Ok(u));
        }
        for s in ["mm", "M", "deg", "degC", "kPa", "", "1.0", "rad "] {
            assert_eq!(SiUnit::parse(s).map_err(|e| e.code()), Err("unit_unsupported"), "{s:?}");
        }
    }

    #[test]
    fn non_finite_values_fail() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(Quantity::new(v, SiUnit::Metre), Err(UnitsError::QuantityNotFinite));
        }
        assert!(Quantity::new(-0.0, SiUnit::One).is_ok());
    }
}
