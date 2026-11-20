//! Construction parameters and their digest (REQ-KRN.CONS.DECL-004).
//!
//! Every angular and distance threshold a construction uses is a value of
//! [`ConstructParams`]: the two general thresholds are mandatory fields, and any further
//! threshold or value (an offset distance, a branch index, a second tolerance) is a named
//! value with an explicit quantity kind. A construction reads them only through the
//! accessors here; the parameter digest covers all of them.
//!
//! # Pinned canonical form
//!
//! RFC 8785 canonical JSON (see the module's JSON writer), keys sorted:
//!
//! ```text
//! {"angular_threshold":"<f>","distance_threshold":"<f>","format":"construct.params@1",
//!  "values":[{"kind":"<kind>","name":"<name>","unit":"<unit>","value":"<f>"},...]}
//! ```
//!
//! (shown wrapped; the bytes have no whitespace). `<f>` is the 16-hex-digit bit pattern of
//! the `f64`; `values` is sorted by name, names unique. The digest is
//! `sha256:<hex>` of these bytes (`fid_kci::kci::sha256_digest`, `KRN.MAN`).

use fid_kci::kci::sha256_digest;
use fid_types::Digest;

use super::error::ConstructError;
use super::id::name_ok;
use super::json::{self, J};

/// The format tag inside the canonical parameter bytes.
pub const PARAMS_FORMAT: &str = "construct.params@1";

/// Most named values one parameter set may hold.
pub const MAX_VALUES: usize = 64;

/// A quantity kind with its SI unit (the `GEO.FRAME.UNITS` kinds `length` and `angle`, plus
/// `dimensionless` and `count`, unit `1`, as in `fid_fit::contract::QUANTITIES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuantityKind {
    Length,
    Angle,
    Dimensionless,
    Count,
}

impl QuantityKind {
    /// Every kind, in declaration order.
    pub const ALL: [QuantityKind; 4] =
        [QuantityKind::Length, QuantityKind::Angle, QuantityKind::Dimensionless, QuantityKind::Count];

    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            QuantityKind::Length => "length",
            QuantityKind::Angle => "angle",
            QuantityKind::Dimensionless => "dimensionless",
            QuantityKind::Count => "count",
        }
    }

    /// The SI unit: `m`, `rad` or `1`.
    pub const fn unit(self) -> &'static str {
        match self {
            QuantityKind::Length => "m",
            QuantityKind::Angle => "rad",
            QuantityKind::Dimensionless | QuantityKind::Count => "1",
        }
    }
}

/// One named parameter value.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Named {
    name: &'static str,
    kind: QuantityKind,
    value: f64,
}

/// The parameters of one construction call. Fields are private: every value is finite,
/// thresholds are non-negative (`+0.0`, never `-0.0`), the angular threshold is at most
/// `pi`, and named values are sorted by unique name.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructParams {
    angular_threshold: f64,
    distance_threshold: f64,
    values: Vec<Named>,
}

fn threshold(x: f64, field: &'static str, max: f64) -> Result<f64, ConstructError> {
    if !x.is_finite() || x.is_sign_negative() || x > max {
        return Err(ConstructError::InvalidParams { field });
    }
    Ok(x)
}

impl ConstructParams {
    /// The general thresholds: `angular_threshold` in radians, finite, in `[+0, pi]`;
    /// `distance_threshold` in metres, finite, `>= +0`. A negative zero is refused, so one
    /// threshold has one bit pattern. [`ConstructError::InvalidParams`] names the field.
    pub fn new(angular_threshold: f64, distance_threshold: f64) -> Result<ConstructParams, ConstructError> {
        Ok(ConstructParams {
            angular_threshold: threshold(angular_threshold, "angular_threshold", core::f64::consts::PI)?,
            distance_threshold: threshold(distance_threshold, "distance_threshold", f64::MAX)?,
            values: Vec::new(),
        })
    }

    /// Adds a named value. `name` follows `[a-z][a-z0-9_]*` (at most 32 bytes) and is not
    /// already present; `value` is finite (a `-0.0` is stored as `+0.0`); a `count` is a
    /// non-negative integer value. At most [`MAX_VALUES`] values.
    /// [`ConstructError::InvalidParams`] (`values`) otherwise.
    pub fn with(mut self, name: &'static str, kind: QuantityKind, value: f64) -> Result<Self, ConstructError> {
        let bad = ConstructError::InvalidParams { field: "values" };
        if !name_ok(name) || !value.is_finite() || self.values.len() >= MAX_VALUES {
            return Err(bad);
        }
        if kind == QuantityKind::Count && (value < 0.0 || value % 1.0 != 0.0) {
            return Err(bad);
        }
        let at = match self.values.binary_search_by(|v| v.name.cmp(name)) {
            Ok(_) => return Err(bad),
            Err(at) => at,
        };
        self.values.try_reserve(1).map_err(|_| bad)?;
        self.values.insert(at, Named { name, kind, value: value + 0.0 });
        Ok(self)
    }

    /// The general angular threshold, radians.
    pub fn angular_threshold(&self) -> f64 {
        self.angular_threshold
    }

    /// The general distance threshold, metres.
    pub fn distance_threshold(&self) -> f64 {
        self.distance_threshold
    }

    /// The named value `name` of kind `kind`. [`ConstructError::MissingParam`] when there is
    /// no such name or it has another kind; a construction never falls back to a default.
    pub fn get(&self, name: &'static str, kind: QuantityKind) -> Result<f64, ConstructError> {
        match self.values.binary_search_by(|v| v.name.cmp(name)) {
            Ok(i) if self.values[i].kind == kind => Ok(self.values[i].value),
            _ => Err(ConstructError::MissingParam { name }),
        }
    }

    /// The named values, sorted by name: `(name, kind, value)`.
    pub fn values(&self) -> impl Iterator<Item = (&'static str, QuantityKind, f64)> + '_ {
        self.values.iter().map(|v| (v.name, v.kind, v.value))
    }

    /// The canonical bytes (module documentation).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let values = self
            .values
            .iter()
            .map(|v| {
                J::O(vec![
                    ("kind", json::s(v.kind.as_str())),
                    ("name", json::s(v.name)),
                    ("unit", json::s(v.kind.unit())),
                    ("value", json::f(v.value)),
                ])
            })
            .collect();
        json::write(&J::O(vec![
            ("angular_threshold", json::f(self.angular_threshold)),
            ("distance_threshold", json::f(self.distance_threshold)),
            ("format", json::s(PARAMS_FORMAT)),
            ("values", J::A(values)),
        ]))
    }

    /// The parameter digest: SHA-256 of [`ConstructParams::canonical_bytes`].
    pub fn digest(&self) -> Digest {
        sha256_digest(&self.canonical_bytes())
    }
}
