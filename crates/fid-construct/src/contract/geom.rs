//! Input geometry, input kinds and constructed elements.
//!
//! # Pinned canonical form of a constructed element
//!
//! RFC 8785 canonical JSON, keys sorted, one of
//!
//! ```text
//! {"feature":{<fields>,"kind":"<primitive>"},"format":"construct.element@1"}
//! {"format":"construct.element@1","value":{"kind":"<kind>","unit":"<unit>","value":"<f>"}}
//! ```
//!
//! `<f>` is the 16-hex-digit bit pattern of an `f64`, a vector an array of them. The
//! feature fields have the names `fid_fit::contract::encode` uses (`point: c`;
//! `line2`/`line3: p, d`; `plane: p, n`; `circle2`/`sphere: c, r`; `circle3: c, n, r`;
//! `cylinder: p, d, r`; `cone: apex, d, half_angle`; `torus: c, n, major_r, minor_r`;
//! `slot`/`rectangle: c, n, d, length, width`; `ellipse: c, n, d, a, b`;
//! `paraboloid: apex, d, focal`; `polyline: vertices`; `spline: degree` (decimal string)`,
//! knots, control`), sorted by key like every member. The output digest is `sha256:<hex>` of
//! these bytes.

use core::fmt;

use fid_fit::contract::{FittedFeature, PointsView, Primitive};
use fid_kci::kci::sha256_digest;
use fid_num::NumError;
use fid_types::Digest;

use super::error::ConstructError;
use super::json::{self, J};
use super::params::QuantityKind;

/// The format tag inside the canonical element bytes.
pub const ELEMENT_FORMAT: &str = "construct.element@1";

/// A scalar with its quantity kind (SI unit implied by the kind). Fields are private: the
/// value is finite, a zero is `+0.0`, and a `count` is a non-negative integer value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scalar {
    value: f64,
    kind: QuantityKind,
}

impl Scalar {
    /// Checks and wraps a value. [`ConstructError::Num`] for a NaN, an infinity or a `count`
    /// that is negative or not an integer.
    pub fn new(value: f64, kind: QuantityKind) -> Result<Scalar, ConstructError> {
        const OP: &str = "construct.scalar";
        if value.is_nan() {
            return Err(NumError::NotANumber { op: OP, index: None }.into());
        }
        if value.is_infinite() {
            return Err(NumError::Infinite { op: OP, index: None }.into());
        }
        if kind == QuantityKind::Count && (value < 0.0 || value % 1.0 != 0.0) {
            return Err(NumError::Domain { op: OP, detail: "count_not_integer" }.into());
        }
        Ok(Scalar { value: value + 0.0, kind })
    }

    /// The value in the kind's SI unit.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The quantity kind.
    pub fn kind(&self) -> QuantityKind {
        self.kind
    }

    /// The SI unit of the kind.
    pub fn unit(&self) -> &'static str {
        self.kind.unit()
    }
}

/// The kind of an input element, as a construction expects it or as it was supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputKind {
    /// A fitted feature of this primitive.
    Feature(Primitive),
    /// A fitted feature of any primitive (only as an expectation).
    AnyFeature,
    /// A point set.
    Points,
    /// A scalar of this quantity kind.
    Scalar(QuantityKind),
}

impl InputKind {
    /// Whether a supplied kind `got` satisfies this expectation.
    pub fn accepts(self, got: InputKind) -> bool {
        match (self, got) {
            (InputKind::AnyFeature, InputKind::Feature(_)) => true,
            (a, b) => a == b,
        }
    }
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputKind::Feature(p) => write!(f, "feature {}", p.slug()),
            InputKind::AnyFeature => f.write_str("any feature"),
            InputKind::Points => f.write_str("points"),
            InputKind::Scalar(k) => write!(f, "scalar {}", k.as_str()),
        }
    }
}

/// The geometry of one input element.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementGeom<'a> {
    /// A fitted or constructed feature (SI metres and radians).
    Feature(FittedFeature),
    /// A validated point set.
    Points(PointsView<'a>),
    /// A scalar (for example a relationship value).
    Scalar(Scalar),
}

impl ElementGeom<'_> {
    /// The kind of this geometry.
    pub fn kind(&self) -> InputKind {
        match self {
            ElementGeom::Feature(f) => InputKind::Feature(f.primitive()),
            ElementGeom::Points(_) => InputKind::Points,
            ElementGeom::Scalar(s) => InputKind::Scalar(s.kind()),
        }
    }

    /// Checks a supplied input: a feature must pass `FittedFeature::canonical` (finite,
    /// non-degenerate directions, positive extents); points and scalars are valid by
    /// construction.
    pub(crate) fn validate(&self, index: usize) -> Result<(), ConstructError> {
        if let ElementGeom::Feature(f) = self {
            f.canonical().map_err(|e| ConstructError::InvalidFeature { index: Some(index), code: e.code() })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Out {
    Feature(FittedFeature),
    Value(Scalar),
}

/// The result of a construction: a feature, or a value with quantity kind and unit (a
/// relationship). The field is private: a feature is in `FittedFeature::canonical` form, a
/// value a checked [`Scalar`].
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructedElement {
    out: Out,
}

impl ConstructedElement {
    /// A feature result, canonicalised (`FittedFeature::canonical`).
    /// [`ConstructError::InvalidFeature`] (no index) when the feature is not valid.
    pub fn feature(f: &FittedFeature) -> Result<ConstructedElement, ConstructError> {
        let f = f.canonical().map_err(|e| ConstructError::InvalidFeature { index: None, code: e.code() })?;
        Ok(ConstructedElement { out: Out::Feature(f) })
    }

    /// A value result ([`Scalar::new`]).
    pub fn value(value: f64, kind: QuantityKind) -> Result<ConstructedElement, ConstructError> {
        Ok(ConstructedElement { out: Out::Value(Scalar::new(value, kind)?) })
    }

    /// The feature, if this is one.
    pub fn as_feature(&self) -> Option<&FittedFeature> {
        match &self.out {
            Out::Feature(f) => Some(f),
            Out::Value(_) => None,
        }
    }

    /// The value, if this is one.
    pub fn as_value(&self) -> Option<Scalar> {
        match &self.out {
            Out::Feature(_) => None,
            Out::Value(s) => Some(*s),
        }
    }

    /// The element as an input to another construction.
    pub fn to_geom(&self) -> ElementGeom<'static> {
        match &self.out {
            Out::Feature(f) => ElementGeom::Feature(f.clone()),
            Out::Value(s) => ElementGeom::Scalar(*s),
        }
    }

    /// The canonical bytes (module documentation).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let body = match &self.out {
            Out::Feature(f) => ("feature", feature_json(f)),
            Out::Value(s) => (
                "value",
                J::O(vec![
                    ("kind", json::s(s.kind().as_str())),
                    ("unit", json::s(s.unit())),
                    ("value", json::f(s.value())),
                ]),
            ),
        };
        json::write(&J::O(vec![("format", json::s(ELEMENT_FORMAT)), body]))
    }

    /// The output digest: SHA-256 of [`ConstructedElement::canonical_bytes`].
    pub fn digest(&self) -> Digest {
        sha256_digest(&self.canonical_bytes())
    }
}

fn vs(points: &[[f64; 3]]) -> J {
    J::A(points.iter().map(|p| json::fs(p.as_slice())).collect())
}

fn feature_json(feat: &FittedFeature) -> J {
    use json::{f, fs};
    use FittedFeature as F;
    let mut m: Vec<(&'static str, J)> = match feat {
        F::Point { c } => vec![("c", fs(c))],
        F::Line2 { p, d } => vec![("p", fs(p)), ("d", fs(d))],
        F::Line3 { p, d } => vec![("p", fs(p)), ("d", fs(d))],
        F::Plane { p, n } => vec![("p", fs(p)), ("n", fs(n))],
        F::Circle2 { c, r } => vec![("c", fs(c)), ("r", f(*r))],
        F::Circle3 { c, n, r } => vec![("c", fs(c)), ("n", fs(n)), ("r", f(*r))],
        F::Sphere { c, r } => vec![("c", fs(c)), ("r", f(*r))],
        F::Cylinder { p, d, r } => vec![("p", fs(p)), ("d", fs(d)), ("r", f(*r))],
        F::Cone { apex, d, half_angle } => vec![("apex", fs(apex)), ("d", fs(d)), ("half_angle", f(*half_angle))],
        F::Torus { c, n, major_r, minor_r } => {
            vec![("c", fs(c)), ("n", fs(n)), ("major_r", f(*major_r)), ("minor_r", f(*minor_r))]
        }
        F::Slot { c, n, d, length, width } | F::Rectangle { c, n, d, length, width } => {
            vec![("c", fs(c)), ("n", fs(n)), ("d", fs(d)), ("length", f(*length)), ("width", f(*width))]
        }
        F::Ellipse { c, n, d, a, b } => vec![("c", fs(c)), ("n", fs(n)), ("d", fs(d)), ("a", f(*a)), ("b", f(*b))],
        F::Paraboloid { apex, d, focal } => vec![("apex", fs(apex)), ("d", fs(d)), ("focal", f(*focal))],
        F::Polyline { vertices } => vec![("vertices", vs(vertices))],
        F::Spline { degree, knots, control } => {
            vec![("degree", J::S(degree.to_string())), ("knots", fs(knots)), ("control", vs(control))]
        }
    };
    m.push(("kind", json::s(feat.primitive().slug())));
    J::O(m)
}
