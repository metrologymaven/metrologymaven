//! Derived-element type, dispatch and failure reasons (spec node `GDT.LOC.DERIV.TYPES`), pure.
//!
//! Every derived toleranced element (an axis, a derived median line, a median surface or a set
//! of median points) is a [`DerivedElement`]; [`derive`] finds the resolved chain's reduction
//! operator, routes it to the implementation registered for its algorithm ID in a
//! [`DeriveRegistry`] and returns the element as a [`Derived`] together with that algorithm ID
//! (REQ-GDT.LOC.DERIV.TYPES-001). The derivations themselves (`GDT.LOC.DERIV.AXIS`,
//! `GDT.LOC.DERIV.MLINE`, `GDT.LOC.DERIV.MSURF`) register here; tolerance evaluation stays with
//! the consumers (orientation, position, coaxiality, symmetry, derived straightness).
//!
//! # Pinned procedure of [`derive`]
//!
//! 1. **Reduction step.** The chain's steps of kind `reduction` are counted. None is
//!    `no_reduction_in_chain` (REQ-004); more than one is `ambiguous_reduction{count}` (the
//!    request would not say which element is wanted, so it fails closed).
//! 2. **Lookup.** The reduction's algorithm ID is looked up exactly in the registry; no entry
//!    is `UNSUPPORTED{alg}` (REQ-002). No other implementation is tried in its place.
//! 3. **Inputs.** Every point coordinate and the nominal direction (when given) must be finite
//!    (`non_finite_input{field, index}`; KNP-1: NaN is an error).
//! 4. **Call.** The implementation gets the request, the chain and its reduction step. Its
//!    refusal `Unsupported{alg}` is passed through as `UNSUPPORTED{alg}`, and its failure
//!    with a typed [`DerivationReason`] as `derivation_failed{reason}` with no element
//!    (REQ-003).
//! 5. **Result.** The element and flags the implementation produced, with the registry key
//!    (the reduction's algorithm ID) as the producing algorithm ID. The implementation cannot
//!    name another ID.
//!
//! # Element invariants
//!
//! The element types have private fields and validating constructors, so an element that
//! exists is well formed: every coordinate is finite; an axis direction and every median-surface
//! normal is a unit vector within [`UNIT_TOLERANCE`] on its squared norm; an axis extent (the
//! length of the axis segment, centred on the axis point) is finite and not negative; a median
//! line has at least 2 points and the other point sets at least 1; normals, when present, match
//! the points one for one. Coordinates are in metres, in the frame of the input points.
//!
//! # Wire form
//!
//! [`Derived::to_cvalue`] is the `fid.derived-element/1` record of the exported JSON Schema
//! [`DERIVED_ELEMENT_JSON_SCHEMA`], every float as bit-exact `"0x"` + 16 hex digits.
//!
//! # Determinism
//!
//! D0. The only arithmetic is the squared-norm check `(x·x + y·y) + z·z` (pinned order, no fused
//! multiply-add); lookups are in a `BTreeMap`. Nothing panics on any input.

use crate::dmt::{AlgId, Operator, OperatorChain, OperatorKind};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The `schema` value of a serialised [`Derived`].
pub const DERIVED_ELEMENT_SCHEMA: &str = "fid.derived-element/1";

/// The exported JSON Schema (2020-12) of a serialised [`Derived`].
pub const DERIVED_ELEMENT_JSON_SCHEMA: &str = include_str!("../../schema/derived-element-v1.schema.json");

/// Largest accepted `|‖v‖² − 1|` for a unit vector (axis direction, surface normal).
pub const UNIT_TOLERANCE: f64 = 1e-12;

/// The geometric kind of the toleranced feature the element is derived from: a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureKind {
    /// `cylinder`: a cylindrical feature (hole or pin).
    Cylinder,
    /// `cone`: a conical feature.
    Cone,
    /// `width`: two opposed nominally parallel surfaces (slot or tab).
    Width,
}

impl FeatureKind {
    /// Every kind, in declaration order.
    pub const ALL: [FeatureKind; 3] = [FeatureKind::Cylinder, FeatureKind::Cone, FeatureKind::Width];

    /// Wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            FeatureKind::Cylinder => "cylinder",
            FeatureKind::Cone => "cone",
            FeatureKind::Width => "width",
        }
    }

    /// Parses the wire form (exact).
    pub fn parse(s: &str) -> Option<FeatureKind> {
        FeatureKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Why an implementation could not produce the element: a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DerivationReason {
    /// `too_few_sections`: fewer usable sections than the chain's minimum.
    TooFewSections,
    /// `too_few_points`: too few points for the derivation.
    TooFewPoints,
    /// `fit_failed`: the association the chain names failed.
    FitFailed,
    /// `no_opposite_point`: a point had no opposite point (median surface or points).
    NoOppositePoint,
}

impl DerivationReason {
    /// Every reason, in declaration order.
    pub const ALL: [DerivationReason; 4] = [
        DerivationReason::TooFewSections,
        DerivationReason::TooFewPoints,
        DerivationReason::FitFailed,
        DerivationReason::NoOppositePoint,
    ];

    /// Wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            DerivationReason::TooFewSections => "too_few_sections",
            DerivationReason::TooFewPoints => "too_few_points",
            DerivationReason::FitFailed => "fit_failed",
            DerivationReason::NoOppositePoint => "no_opposite_point",
        }
    }

    /// Parses the wire form (exact).
    pub fn parse(s: &str) -> Option<DerivationReason> {
        DerivationReason::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

/// A flag an implementation passes through with the element: a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeriveFlag {
    /// `local_optimum_only`: the fit behind the element is certified only as a local optimum.
    /// Such an element is never a datum or a value of record without a global cross-check.
    LocalOptimumOnly,
}

impl DeriveFlag {
    /// Every flag, in declaration order.
    pub const ALL: [DeriveFlag; 1] = [DeriveFlag::LocalOptimumOnly];

    /// Wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            DeriveFlag::LocalOptimumOnly => "local_optimum_only",
        }
    }

    /// Parses the wire form (exact).
    pub fn parse(s: &str) -> Option<DeriveFlag> {
        DeriveFlag::ALL.into_iter().find(|f| f.as_str() == s)
    }
}

/// Why an element could not be built. [`ElementError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementError {
    /// `non_finite{field}`: a coordinate or the extent is NaN or infinite.
    NonFinite {
        /// `point`, `direction`, `extent`, `points` or `normals`.
        field: &'static str,
        /// The point or normal index, for a list.
        index: Option<usize>,
    },
    /// `not_unit{field}`: a direction or normal is not a unit vector within [`UNIT_TOLERANCE`].
    NotUnit {
        /// `direction` or `normals`.
        field: &'static str,
        /// The normal index, for a list.
        index: Option<usize>,
    },
    /// `negative_extent`: an axis extent below zero.
    NegativeExtent,
    /// `too_few_points{min}`: fewer points than the element needs.
    TooFewPoints {
        /// The minimum.
        min: usize,
        /// How many were given.
        got: usize,
    },
    /// `normals_mismatch`: the normals do not match the points one for one.
    NormalsMismatch {
        /// Points given.
        points: usize,
        /// Normals given.
        normals: usize,
    },
}

impl ElementError {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            ElementError::NonFinite { .. } => "non_finite",
            ElementError::NotUnit { .. } => "not_unit",
            ElementError::NegativeExtent => "negative_extent",
            ElementError::TooFewPoints { .. } => "too_few_points",
            ElementError::NormalsMismatch { .. } => "normals_mismatch",
        }
    }
}

impl fmt::Display for ElementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            ElementError::NonFinite { field, index } | ElementError::NotUnit { field, index } => match index {
                Some(i) => write!(f, "{c}{{{field}[{i}]}}"),
                None => write!(f, "{c}{{{field}}}"),
            },
            ElementError::NegativeExtent => f.write_str(c),
            ElementError::TooFewPoints { min, got } => write!(f, "{c}{{{min}:{got}}}"),
            ElementError::NormalsMismatch { points, normals } => write!(f, "{c}{{{points}:{normals}}}"),
        }
    }
}

impl std::error::Error for ElementError {}

fn finite3(v: &[f64; 3]) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

fn is_unit(v: &[f64; 3]) -> bool {
    let n2 = (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2];
    (n2 - 1.0).abs() <= UNIT_TOLERANCE
}

fn check_points(points: &[[f64; 3]], min: usize) -> Result<(), ElementError> {
    if points.len() < min {
        return Err(ElementError::TooFewPoints { min, got: points.len() });
    }
    match points.iter().position(|p| !finite3(p)) {
        Some(i) => Err(ElementError::NonFinite { field: "points", index: Some(i) }),
        None => Ok(()),
    }
}

/// An axis: a unit direction through `point`, bounded by a segment of length `extent` centred
/// on `point` (`point ± extent/2 · direction`).
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    point: [f64; 3],
    direction: [f64; 3],
    extent: f64,
}

impl Axis {
    /// Builds an axis (see the module's element invariants).
    pub fn new(point: [f64; 3], direction: [f64; 3], extent: f64) -> Result<Axis, ElementError> {
        if !finite3(&point) {
            return Err(ElementError::NonFinite { field: "point", index: None });
        }
        if !finite3(&direction) {
            return Err(ElementError::NonFinite { field: "direction", index: None });
        }
        if !is_unit(&direction) {
            return Err(ElementError::NotUnit { field: "direction", index: None });
        }
        if !extent.is_finite() {
            return Err(ElementError::NonFinite { field: "extent", index: None });
        }
        if extent < 0.0 {
            return Err(ElementError::NegativeExtent);
        }
        Ok(Axis { point, direction, extent })
    }

    /// The axis point (the midpoint of the extent), in metres.
    pub fn point(&self) -> [f64; 3] {
        self.point
    }

    /// The unit direction.
    pub fn direction(&self) -> [f64; 3] {
        self.direction
    }

    /// The length of the axis segment, in metres.
    pub fn extent(&self) -> f64 {
        self.extent
    }
}

/// A derived median line: the ordered points of the line (for example section centres).
#[derive(Debug, Clone, PartialEq)]
pub struct MedianLine {
    points: Vec<[f64; 3]>,
}

impl MedianLine {
    /// The fewest points of a median line.
    pub const MIN_POINTS: usize = 2;

    /// Builds a median line from its ordered points.
    pub fn new(points: Vec<[f64; 3]>) -> Result<MedianLine, ElementError> {
        check_points(&points, MedianLine::MIN_POINTS)?;
        Ok(MedianLine { points })
    }

    /// The ordered points, in metres.
    pub fn points(&self) -> &[[f64; 3]] {
        &self.points
    }
}

/// A derived median surface: its points, with an optional unit normal per point.
#[derive(Debug, Clone, PartialEq)]
pub struct MedianSurface {
    points: Vec<[f64; 3]>,
    normals: Option<Vec<[f64; 3]>>,
}

impl MedianSurface {
    /// The fewest points of a median surface.
    pub const MIN_POINTS: usize = 1;

    /// Builds a median surface.
    pub fn new(points: Vec<[f64; 3]>, normals: Option<Vec<[f64; 3]>>) -> Result<MedianSurface, ElementError> {
        check_points(&points, MedianSurface::MIN_POINTS)?;
        if let Some(ns) = &normals {
            if ns.len() != points.len() {
                return Err(ElementError::NormalsMismatch { points: points.len(), normals: ns.len() });
            }
            for (i, n) in ns.iter().enumerate() {
                if !finite3(n) {
                    return Err(ElementError::NonFinite { field: "normals", index: Some(i) });
                }
                if !is_unit(n) {
                    return Err(ElementError::NotUnit { field: "normals", index: Some(i) });
                }
            }
        }
        Ok(MedianSurface { points, normals })
    }

    /// The points, in metres.
    pub fn points(&self) -> &[[f64; 3]] {
        &self.points
    }

    /// The unit normals, one per point, when the derivation gives them.
    pub fn normals(&self) -> Option<&[[f64; 3]]> {
        self.normals.as_deref()
    }
}

/// Median points (for symmetry-type evaluations).
#[derive(Debug, Clone, PartialEq)]
pub struct MedianPoints {
    points: Vec<[f64; 3]>,
}

impl MedianPoints {
    /// The fewest median points.
    pub const MIN_POINTS: usize = 1;

    /// Builds a set of median points.
    pub fn new(points: Vec<[f64; 3]>) -> Result<MedianPoints, ElementError> {
        check_points(&points, MedianPoints::MIN_POINTS)?;
        Ok(MedianPoints { points })
    }

    /// The points, in metres.
    pub fn points(&self) -> &[[f64; 3]] {
        &self.points
    }
}

/// Which variant a [`DerivedElement`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    /// `axis`.
    Axis,
    /// `median_line`.
    MedianLine,
    /// `median_surface`.
    MedianSurface,
    /// `median_points`.
    MedianPoints,
}

impl ElementKind {
    /// Wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            ElementKind::Axis => "axis",
            ElementKind::MedianLine => "median_line",
            ElementKind::MedianSurface => "median_surface",
            ElementKind::MedianPoints => "median_points",
        }
    }
}

/// A derived toleranced element.
#[derive(Debug, Clone, PartialEq)]
pub enum DerivedElement {
    /// An axis of an associated feature.
    Axis(Axis),
    /// A derived median line.
    MedianLine(MedianLine),
    /// A derived median surface.
    MedianSurface(MedianSurface),
    /// Median points.
    MedianPoints(MedianPoints),
}

fn v3(p: &[f64; 3]) -> CValue {
    CValue::Arr(vec![CValue::F64Bits(p[0]), CValue::F64Bits(p[1]), CValue::F64Bits(p[2])])
}

fn list(ps: &[[f64; 3]]) -> CValue {
    CValue::Arr(ps.iter().map(v3).collect())
}

impl DerivedElement {
    /// The variant.
    pub fn kind(&self) -> ElementKind {
        match self {
            DerivedElement::Axis(_) => ElementKind::Axis,
            DerivedElement::MedianLine(_) => ElementKind::MedianLine,
            DerivedElement::MedianSurface(_) => ElementKind::MedianSurface,
            DerivedElement::MedianPoints(_) => ElementKind::MedianPoints,
        }
    }

    /// The element as a JSON value: `{kind, ...}` with bit-exact floats.
    pub fn to_cvalue(&self) -> CValue {
        let mut m: BTreeMap<String, CValue> = BTreeMap::new();
        m.insert("kind".to_owned(), CValue::Str(self.kind().as_str().to_owned()));
        match self {
            DerivedElement::Axis(a) => {
                m.insert("direction".to_owned(), v3(&a.direction));
                m.insert("extent".to_owned(), CValue::F64Bits(a.extent));
                m.insert("point".to_owned(), v3(&a.point));
            }
            DerivedElement::MedianLine(l) => {
                m.insert("points".to_owned(), list(&l.points));
            }
            DerivedElement::MedianSurface(s) => {
                if let Some(ns) = &s.normals {
                    m.insert("normals".to_owned(), list(ns));
                }
                m.insert("points".to_owned(), list(&s.points));
            }
            DerivedElement::MedianPoints(p) => {
                m.insert("points".to_owned(), list(&p.points));
            }
        }
        CValue::Obj(m)
    }
}

/// What [`derive`] returns: the element, the algorithm ID of the reduction operator that
/// produced it, and the flags the implementation passed through.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    element: DerivedElement,
    alg: AlgId,
    flags: BTreeSet<DeriveFlag>,
}

impl Derived {
    /// The element.
    pub fn element(&self) -> &DerivedElement {
        &self.element
    }

    /// The producing algorithm ID (the chain's reduction operator), for the ResultRecord.
    pub fn alg(&self) -> &AlgId {
        &self.alg
    }

    /// The flags, sorted.
    pub fn flags(&self) -> &BTreeSet<DeriveFlag> {
        &self.flags
    }

    /// The `fid.derived-element/1` record: `{alg, element, flags, schema}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m: BTreeMap<String, CValue> = BTreeMap::new();
        m.insert("alg".to_owned(), CValue::Str(self.alg.as_str().to_owned()));
        m.insert("element".to_owned(), self.element.to_cvalue());
        m.insert(
            "flags".to_owned(),
            CValue::Arr(self.flags.iter().map(|f| CValue::Str(f.as_str().to_owned())).collect()),
        );
        m.insert("schema".to_owned(), CValue::Str(DERIVED_ELEMENT_SCHEMA.to_owned()));
        CValue::Obj(m)
    }
}

/// The request a consumer passes to [`derive`].
#[derive(Debug, Clone, Copy)]
pub struct DeriveRequest<'a> {
    /// The measured points of the toleranced feature, in metres, in index order.
    pub points: &'a [[f64; 3]],
    /// The feature's geometric kind.
    pub feature_kind: FeatureKind,
    /// The nominal direction of the feature (an axis is oriented to its hemisphere), when the
    /// derivation needs one.
    pub nominal_direction: Option<[f64; 3]>,
}

/// What an implementation is called with.
#[derive(Debug, Clone, Copy)]
pub struct DeriveCall<'a> {
    /// The consumer's request (inputs already checked finite).
    pub request: &'a DeriveRequest<'a>,
    /// The resolved chain.
    pub chain: &'a OperatorChain,
    /// The chain's reduction step whose algorithm ID selected the implementation.
    pub reduction: &'a Operator,
}

/// What an implementation produced: a valid element and the flags it passes through.
#[derive(Debug, Clone, PartialEq)]
pub struct Produced {
    /// The element.
    pub element: DerivedElement,
    /// The flags.
    pub flags: BTreeSet<DeriveFlag>,
}

impl Produced {
    /// An element with no flags.
    pub fn new(element: DerivedElement) -> Produced {
        Produced { element, flags: BTreeSet::new() }
    }
}

/// How an implementation refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImplError {
    /// The implementation does not support part of the chain (for example a non-axial fit).
    Unsupported {
        /// The algorithm ID it does not support.
        alg: String,
    },
    /// The data cannot support the element.
    Failed(DerivationReason),
}

/// A registered derivation.
pub type DeriveFn = fn(&DeriveCall<'_>) -> Result<Produced, ImplError>;

/// Errors of [`DeriveRegistry::register`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// `duplicate_alg{alg}`: an implementation is already registered for the algorithm ID.
    DuplicateAlg {
        /// The algorithm ID.
        alg: String,
    },
}

impl RegistryError {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            RegistryError::DuplicateAlg { .. } => "duplicate_alg",
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DuplicateAlg { alg } => write!(f, "{}{{{}}}", self.code(), alg.escape_debug()),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Errors of [`derive`]. [`DeriveError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeriveError {
    /// `no_reduction_in_chain`: the chain has no reduction step (REQ-004).
    NoReductionInChain,
    /// `ambiguous_reduction{count}`: the chain has more than one reduction step.
    AmbiguousReduction {
        /// How many.
        count: usize,
    },
    /// `UNSUPPORTED{alg}`: no implementation is registered for the reduction's algorithm ID
    /// (REQ-002), or the implementation does not support part of the chain.
    Unsupported {
        /// The algorithm ID.
        alg: String,
    },
    /// `derivation_failed{reason}`: the implementation could not produce the element (REQ-003).
    DerivationFailed {
        /// The typed reason.
        reason: DerivationReason,
    },
    /// `non_finite_input{field}`: a point coordinate or the nominal direction is NaN or
    /// infinite.
    NonFiniteInput {
        /// `points` or `nominal_direction`.
        field: &'static str,
        /// The point index (0 for the nominal direction).
        index: usize,
    },
}

impl DeriveError {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            DeriveError::NoReductionInChain => "no_reduction_in_chain",
            DeriveError::AmbiguousReduction { .. } => "ambiguous_reduction",
            DeriveError::Unsupported { .. } => "UNSUPPORTED",
            DeriveError::DerivationFailed { .. } => "derivation_failed",
            DeriveError::NonFiniteInput { .. } => "non_finite_input",
        }
    }
}

impl fmt::Display for DeriveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            DeriveError::NoReductionInChain => f.write_str(c),
            DeriveError::AmbiguousReduction { count } => write!(f, "{c}{{{count}}}"),
            DeriveError::Unsupported { alg } => write!(f, "{c}{{{}}}", alg.escape_debug()),
            DeriveError::DerivationFailed { reason } => write!(f, "{c}{{{}}}", reason.as_str()),
            DeriveError::NonFiniteInput { field, index } => write!(f, "{c}{{{field}[{index}]}}"),
        }
    }
}

impl std::error::Error for DeriveError {}

/// The implementations [`derive`] dispatches to, keyed by reduction-operator algorithm ID.
#[derive(Debug, Clone, Default)]
pub struct DeriveRegistry {
    impls: BTreeMap<AlgId, DeriveFn>,
}

impl DeriveRegistry {
    /// An empty registry.
    pub fn new() -> DeriveRegistry {
        DeriveRegistry::default()
    }

    /// The registry of the derivations this crate implements, built at startup. Each
    /// derivation leaf (`GDT.LOC.DERIV.AXIS`, `.MLINE`, `.MSURF`) registers itself here; none
    /// is on main yet, so today it is empty and every reduction is `UNSUPPORTED`.
    pub fn builtin() -> DeriveRegistry {
        DeriveRegistry::new()
    }

    /// Registers `f` for `alg`; a second registration of the same ID is `duplicate_alg`.
    pub fn register(&mut self, alg: AlgId, f: DeriveFn) -> Result<(), RegistryError> {
        if self.impls.contains_key(&alg) {
            return Err(RegistryError::DuplicateAlg { alg: alg.as_str().to_owned() });
        }
        self.impls.insert(alg, f);
        Ok(())
    }

    /// The implementation registered for `alg`.
    pub fn get(&self, alg: &AlgId) -> Option<DeriveFn> {
        self.impls.get(alg).copied()
    }

    /// The registered algorithm IDs, ascending.
    pub fn algs(&self) -> impl Iterator<Item = &AlgId> {
        self.impls.keys()
    }

    /// How many implementations are registered.
    pub fn len(&self) -> usize {
        self.impls.len()
    }

    /// Whether none is registered.
    pub fn is_empty(&self) -> bool {
        self.impls.is_empty()
    }

    /// [`derive`] with this registry.
    pub fn derive(&self, request: &DeriveRequest<'_>, chain: &OperatorChain) -> Result<Derived, DeriveError> {
        derive(self, request, chain)
    }
}

/// Derives the element the chain's reduction operator names (see the module's pinned
/// procedure).
pub fn derive(
    registry: &DeriveRegistry,
    request: &DeriveRequest<'_>,
    chain: &OperatorChain,
) -> Result<Derived, DeriveError> {
    let mut reductions = chain.steps().iter().filter(|s| s.kind() == OperatorKind::Reduction);
    let Some(reduction) = reductions.next() else {
        return Err(DeriveError::NoReductionInChain);
    };
    let extra = reductions.count();
    if extra > 0 {
        return Err(DeriveError::AmbiguousReduction { count: extra + 1 });
    }
    let alg = reduction.alg();
    let Some(f) = registry.get(alg) else {
        return Err(DeriveError::Unsupported { alg: alg.as_str().to_owned() });
    };
    if let Some(i) = request.points.iter().position(|p| !finite3(p)) {
        return Err(DeriveError::NonFiniteInput { field: "points", index: i });
    }
    if let Some(d) = &request.nominal_direction {
        if !finite3(d) {
            return Err(DeriveError::NonFiniteInput { field: "nominal_direction", index: 0 });
        }
    }
    let call = DeriveCall { request, chain, reduction };
    match f(&call) {
        Ok(p) => Ok(Derived { element: p.element, alg: alg.clone(), flags: p.flags }),
        Err(ImplError::Unsupported { alg }) => Err(DeriveError::Unsupported { alg }),
        Err(ImplError::Failed(reason)) => Err(DeriveError::DerivationFailed { reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_forms_round_trip() {
        for k in FeatureKind::ALL {
            assert_eq!(FeatureKind::parse(k.as_str()), Some(k));
        }
        for r in DerivationReason::ALL {
            assert_eq!(DerivationReason::parse(r.as_str()), Some(r));
        }
        for f in DeriveFlag::ALL {
            assert_eq!(DeriveFlag::parse(f.as_str()), Some(f));
        }
        assert_eq!(FeatureKind::parse("Cylinder"), None);
    }

    #[test]
    fn unit_check_is_on_the_squared_norm() {
        assert!(is_unit(&[0.0, 0.0, 1.0]));
        assert!(is_unit(&[0.6, 0.8, 0.0]));
        assert!(!is_unit(&[0.0, 0.0, 1.0 + 1e-11]));
        assert!(!is_unit(&[0.0, 0.0, 0.0]));
    }
}
