//! Direction vectors for probe and normal data (REQ-HAL.IDL.UNITS-006).

use super::{f64_bits, UnitsError};
use fid_canon::CValue;

/// A unit direction vector in a right-handed Cartesian frame (the frame of the record that
/// carries it). For probe-approach and surface-normal payloads the vector points **out of
/// the material**; producers of those payloads own that sign, since a vector alone cannot
/// show it.
///
/// Validation: all components finite and `|sqrt(x·x + y·y + z·z) − 1| <= 1e-9`, the sum
/// taken left to right with separate multiplies and adds (no FMA), so the verdict is the
/// same on every platform (D0). Wire form: `["0x…", "0x…", "0x…"]` (bit strings of x, y, z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitVector3 {
    x: f64,
    y: f64,
    z: f64,
}

impl UnitVector3 {
    /// Largest accepted deviation of the norm from 1.
    pub const TOLERANCE: f64 = 1e-9;

    /// A validated direction; anything else fails with `direction_not_unit`.
    pub fn new(x: f64, y: f64, z: f64) -> Result<UnitVector3, UnitsError> {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return Err(UnitsError::DirectionNotUnit);
        }
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let norm = (xx + yy + zz).sqrt();
        let deviation = (norm - 1.0).abs();
        if deviation.is_nan() || deviation > UnitVector3::TOLERANCE {
            return Err(UnitsError::DirectionNotUnit);
        }
        Ok(UnitVector3 { x, y, z })
    }

    pub fn x(&self) -> f64 {
        self.x
    }

    pub fn y(&self) -> f64 {
        self.y
    }

    pub fn z(&self) -> f64 {
        self.z
    }

    /// `[x, y, z]`.
    pub fn components(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// The wire value.
    pub fn to_value(self) -> CValue {
        CValue::Arr(self.components().into_iter().map(CValue::F64Bits).collect())
    }

    /// Reads and validates the wire value.
    pub fn from_value(v: CValue) -> Result<UnitVector3, UnitsError> {
        let CValue::Arr(items) = v else {
            return Err(UnitsError::BadType { field: "direction".into(), expected: "an array of 3 f64 bit strings" });
        };
        let [x, y, z]: [CValue; 3] = items.try_into().map_err(|_| UnitsError::BadType {
            field: "direction".into(),
            expected: "an array of 3 f64 bit strings",
        })?;
        UnitVector3::new(f64_bits("direction", x)?, f64_bits("direction", y)?, f64_bits("direction", z)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_and_not_unit() {
        assert!(UnitVector3::new(0.0, 0.0, 1.0).is_ok());
        assert!(UnitVector3::new(0.6, 0.8, 0.0).is_ok());
        for v in [(0.0, 0.0, 0.0), (1.0, 0.0, 1e-4), (f64::NAN, 0.0, 0.0), (f64::MAX, 0.0, 0.0)] {
            assert_eq!(UnitVector3::new(v.0, v.1, v.2), Err(UnitsError::DirectionNotUnit), "{v:?}");
        }
    }
}
