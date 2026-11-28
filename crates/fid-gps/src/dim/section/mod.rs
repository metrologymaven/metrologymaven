//! 2D element extraction from a span of a section curve (spec node `GDT.DIM.SECT.SPAN`), pure.
//!
//! [`extract_section_element`] takes a section curve ([`SectionCurveView`], read as an
//! artifact, never computed here), a named span of it ([`CurveSpan`]), the element kind
//! ([`ElementKind`]: a line, a circle or a point) and the resolved operator chain, and returns
//! the element fitted in the section plane's coordinates with the provenance a later
//! staleness check needs ([`SectionElement`]: section ID and digest, frame, plane, span, fit
//! algorithm and the digest of the fit result). The checks on the element (distance, angle,
//! radius) are `GDT.DIM.SECT.CMDS`; staleness is `GDT.DIM.SECT.STALE`.
//!
//! # Pinned procedure
//!
//! Checks run in this order and the first failure is returned.
//!
//! 1. **Section.** The span's `section_id` must equal the curve's (`section_mismatch`).
//! 2. **Chain.** The chain's steps of kind `association` name the fit: none is
//!    `chain_without_association`, more than one `ambiguous_association{count}`. The one
//!    association step must carry no parameters (`association_params_unsupported{alg}`: a
//!    constrained step is refused, never run unconstrained). The chain's other steps
//!    (partition, extraction, filtration, reduction and the rest) are **not applied here**:
//!    the span selection below is the only extraction, and the caller owns any other step of
//!    the chain. The association's algorithm ID must be a `fid-fit` ID `fit.<primitive>.<method>@<x.y.z>` whose primitive is
//!    the kind's (`line2`, `circle2`, `point`), else `fit_kind_mismatch{kind, alg}`. Only the
//!    least-squares fits of `fid-fit` are run (`fit.line2.ls`, `fit.point.ls` of
//!    `KRN.LSQ.LIN` and `fit.circle2.ls` of `KRN.LSQ.RND`, at the versions on main); any other
//!    method or version is `fit_unavailable{alg}`. The element is never fitted by another
//!    method than the chain names.
//! 3. **Span values.** `start` and `end` must be finite (`bad_span{not_finite}`) and not
//!    `-0.0` (`bad_span{negative_zero}`, so a recorded span has one encoding). Each must lie in
//!    `[0, length]` (`span_out_of_range{start|end}`, start checked first).
//! 4. **Span order.** Without `wrap`, `start < end` (`bad_span{start_not_before_end}`). With
//!    `wrap`, the curve must be closed (`bad_span{wrap_on_open_curve}`) and `start > end`
//!    (`bad_span{wrap_needs_start_after_end}`).
//! 5. **Selection.** The vertices whose arc-length parameter (the curve's, see
//!    [`curve`]) lies in `[start, end]`, both bounds inclusive and compared exactly in total
//!    order, in curve order. A wrapping span takes the vertices with parameter `>= start` and
//!    then, from vertex 0, those with parameter `<= end`. Each vertex has one parameter in
//!    `[0, length)`, so on a closed curve vertex 0 is selected by `start = 0`, never by
//!    `end = length`. Fewer than the kind needs (line 2, circle 3, point 1) is
//!    `span_too_short{needed, got}`.
//! 6. **Fit.** The selected `[u, v]` go to the fit as the points `(u, v, 0)` with the fit's
//!    default parameters (`FitParams::new(LeastSquares)` for the line and the point,
//!    `ls_rnd::default_params()` for the circle, whose angular coverage check needs at least
//!    30 degrees of arc), through the `fid_fit::contract::Fit` trait. Their in-plane frame for
//!    the plane `z = 0` is the identity, so the fitted parameters are in the section plane's
//!    `u, v` coordinates. Every `fid-fit` refusal (too few points, collinear data for a
//!    circle, coincident points for a line, insufficient coverage, ill conditioned, not
//!    converged, flagged) is `fit_refused{code}` with the flags it carries. A result whose
//!    algorithm differs from the chain's, whose parameters are not finite, whose radius is not
//!    positive, or whose point leaves the plane is `fit_refused{bad_result}`.
//! 7. **Element.** Line `Line2d { p, d }` (the foot of the perpendicular from the centroid and
//!    the unit direction, as `fid-fit` canonicalises them), circle `Circle2d { c, r }`, point
//!    `Point2d { c }` (the centroid of the selection; a one-vertex span gives that vertex).
//!
//! # Determinism
//!
//! The arc lengths are pinned in [`curve`]; selection is exact comparison; the fits are
//! `fid-fit`'s (D0, KNP-1). No `HashMap`, no fused multiply-add, no `f64::min`/`f64::max`, and
//! nothing panics: every outcome is a returned `Result`.
//!
//! # Interpretation status
//!
//! The chain comes from a DMT row, whose interpretation may not be signed off. This element
//! records the fit the chain named; the `provisional` label of D-045 is carried by the result
//! record built from it (`GDT.CHAR.INTERP`), not here.

pub mod curve;

pub use curve::{SectionCurveView, SectionPlane, V2, V3};

use crate::dmt::{OperatorChain, OperatorKind};
use fid_fit::contract::{
    algorithm_id, encode, Fit, FitError, FitParams, FitResult, FittedFeature, Method, PointsView, Primitive,
};
use fid_fit::{ls_lin, ls_rnd};
use fid_geo_frame::FrameId;
use fid_types::Digest;
use std::cmp::Ordering;
use std::fmt;

/// The kind of element extracted from a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    /// `line2d`: a least-squares line in the plane.
    Line2d,
    /// `circle2d`: a least-squares circle (arc) in the plane.
    Circle2d,
    /// `point2d`: the centroid of the span's vertices.
    Point2d,
}

impl ElementKind {
    /// Wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            ElementKind::Line2d => "line2d",
            ElementKind::Circle2d => "circle2d",
            ElementKind::Point2d => "point2d",
        }
    }

    /// Fewest vertices the kind needs (REQ-GDT.DIM.SECT.SPAN-003).
    pub const fn needed(self) -> usize {
        match self {
            ElementKind::Line2d => 2,
            ElementKind::Circle2d => 3,
            ElementKind::Point2d => 1,
        }
    }

    /// The `fid-fit` primitive the chain's fit must produce.
    pub const fn primitive(self) -> Primitive {
        match self {
            ElementKind::Line2d => Primitive::Line2,
            ElementKind::Circle2d => Primitive::Circle2,
            ElementKind::Point2d => Primitive::Point,
        }
    }

    fn version(self) -> (u32, u32, u32) {
        match self {
            ElementKind::Line2d | ElementKind::Point2d => ls_lin::VERSION,
            ElementKind::Circle2d => ls_rnd::VERSION,
        }
    }
}

/// A named span of a section curve: arc-length parameters in metres along the curve
/// (REQ-GDT.DIM.SECT.SPAN-001). `wrap` lets a span of a closed curve run past vertex 0
/// (`start > end`).
#[derive(Debug, Clone, PartialEq)]
pub struct CurveSpan {
    /// The section the span belongs to.
    pub section_id: String,
    /// Start parameter, metres.
    pub start: f64,
    /// End parameter, metres.
    pub end: f64,
    /// The span wraps past vertex 0 of a closed curve.
    pub wrap: bool,
}

/// The fitted parameters of an element, metres, in the section plane's `u, v` coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Params2d {
    /// A line through `p` with unit direction `d`.
    Line2d {
        /// Foot of the perpendicular from the centroid of the selection.
        p: V2,
        /// Unit direction.
        d: V2,
    },
    /// A circle of centre `c` and radius `r`.
    Circle2d {
        /// Centre.
        c: V2,
        /// Radius, positive.
        r: f64,
    },
    /// A point.
    Point2d {
        /// The point.
        c: V2,
    },
}

/// The element of one span with its provenance (REQ-GDT.DIM.SECT.SPAN-002). Fields are
/// private: it is built only by [`extract_section_element`].
#[derive(Debug, Clone, PartialEq)]
pub struct SectionElement {
    kind: ElementKind,
    params: Params2d,
    frame: FrameId,
    plane: SectionPlane,
    section_id: String,
    section_digest: Digest,
    span: CurveSpan,
    fit_alg: String,
    fit_digest: Digest,
    first_vertex: usize,
    vertex_count: usize,
}

impl SectionElement {
    /// The element kind.
    pub fn kind(&self) -> ElementKind {
        self.kind
    }
    /// The fitted parameters in the section plane.
    pub fn params(&self) -> &Params2d {
        &self.params
    }
    /// The frame the section plane is expressed in.
    pub fn frame(&self) -> FrameId {
        self.frame
    }
    /// The section plane.
    pub fn plane(&self) -> &SectionPlane {
        &self.plane
    }
    /// The section ID the curve states.
    pub fn section_id(&self) -> &str {
        &self.section_id
    }
    /// The curve's digest (recomputed from its vertices, [`SectionCurveView::digest`]).
    pub fn section_digest(&self) -> Digest {
        self.section_digest
    }
    /// The span, as given.
    pub fn span(&self) -> &CurveSpan {
        &self.span
    }
    /// The fit's algorithm ID, as the chain names it.
    pub fn fit_alg(&self) -> &str {
        &self.fit_alg
    }
    /// SHA-256 of `fid_fit::contract::encode` of the fit result (feature, residuals,
    /// statistics, diagnostics).
    pub fn fit_digest(&self) -> Digest {
        self.fit_digest
    }
    /// Index of the first selected vertex.
    pub fn first_vertex(&self) -> usize {
        self.first_vertex
    }
    /// How many vertices were selected (and fitted), in curve order from
    /// [`SectionElement::first_vertex`], wrapping past the last vertex for a wrapping span.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }
}

/// Errors of the section-curve view and of [`extract_section_element`].
/// [`SpanError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanError {
    /// `bad_section{reason}`: the curve is malformed (see [`curve`]); `index` names the vertex
    /// where one is concerned.
    BadSection {
        /// Why.
        reason: &'static str,
        /// The vertex index, if any.
        index: Option<usize>,
    },
    /// `unsupported_section_schema{version}`: a `fid.scan.sect.curve` version other than 1.
    UnsupportedSectionSchema {
        /// The version digits of the format tag.
        version: String,
    },
    /// `section_digest_mismatch`: the curve's stated digest is not the digest of its content.
    SectionDigestMismatch,
    /// `section_mismatch`: the span names another section than the curve's.
    SectionMismatch,
    /// `bad_span{reason}` (REQ-003, and the wrap rules).
    BadSpan {
        /// Why.
        reason: &'static str,
    },
    /// `span_out_of_range{bound}` (REQ-004): `start` or `end` is outside `[0, length]`.
    SpanOutOfRange {
        /// `start` or `end`.
        bound: &'static str,
    },
    /// `span_too_short{needed, got}` (REQ-003).
    SpanTooShort {
        /// Vertices the kind needs.
        needed: usize,
        /// Vertices the span selects.
        got: usize,
    },
    /// `chain_without_association`: the chain names no fit.
    ChainWithoutAssociation,
    /// `ambiguous_association{count}`: the chain has more than one association step.
    AmbiguousAssociation {
        /// How many.
        count: usize,
    },
    /// `association_params_unsupported{alg}`: the association step carries parameters (for
    /// example a constraint), which the fits run here do not take; it is refused rather than
    /// run unconstrained.
    AssociationParamsUnsupported {
        /// The association step's algorithm ID.
        alg: String,
    },
    /// `fit_kind_mismatch{kind, alg}`: the chain's fit does not produce the element kind.
    FitKindMismatch {
        /// The element kind asked for.
        kind: ElementKind,
        /// The chain's algorithm ID.
        alg: String,
    },
    /// `fit_unavailable{alg}`: the chain's fit is not one run here (procedure step 2).
    FitUnavailable {
        /// The algorithm ID.
        alg: String,
    },
    /// `fit_refused{code}`: `fid-fit` refused the fit, or its result is not a basis for an
    /// element (procedure step 6).
    FitRefused {
        /// `fid-fit`'s error code, or `bad_result`.
        code: &'static str,
        /// The fit was flagged `local_optimum_only`.
        local_optimum_only: bool,
        /// The fit was flagged `suspect_axis`.
        suspect_axis: bool,
    },
    /// `allocation_failed`: memory for the vertices or the fit input could not be reserved.
    Allocation,
}

impl SpanError {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            SpanError::BadSection { .. } => "bad_section",
            SpanError::UnsupportedSectionSchema { .. } => "unsupported_section_schema",
            SpanError::SectionDigestMismatch => "section_digest_mismatch",
            SpanError::SectionMismatch => "section_mismatch",
            SpanError::BadSpan { .. } => "bad_span",
            SpanError::SpanOutOfRange { .. } => "span_out_of_range",
            SpanError::SpanTooShort { .. } => "span_too_short",
            SpanError::ChainWithoutAssociation => "chain_without_association",
            SpanError::AmbiguousAssociation { .. } => "ambiguous_association",
            SpanError::AssociationParamsUnsupported { .. } => "association_params_unsupported",
            SpanError::FitKindMismatch { .. } => "fit_kind_mismatch",
            SpanError::FitUnavailable { .. } => "fit_unavailable",
            SpanError::FitRefused { .. } => "fit_refused",
            SpanError::Allocation => "allocation_failed",
        }
    }

    /// The refusal of a `fid-fit` error, keeping the flags of a flagged fit.
    pub fn from_fit(e: &FitError) -> SpanError {
        match e {
            FitError::FlaggedFit { local_optimum_only, suspect_axis, .. } => SpanError::FitRefused {
                code: e.code(),
                local_optimum_only: *local_optimum_only,
                suspect_axis: *suspect_axis,
            },
            other => SpanError::FitRefused { code: other.code(), local_optimum_only: false, suspect_axis: false },
        }
    }

    fn bad_result() -> SpanError {
        SpanError::FitRefused { code: "bad_result", local_optimum_only: false, suspect_axis: false }
    }
}

impl fmt::Display for SpanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            SpanError::BadSection { reason, index: None } | SpanError::BadSpan { reason } => {
                write!(f, "{c}{{{reason}}}")
            }
            SpanError::BadSection { reason, index: Some(i) } => write!(f, "{c}{{{reason}, index: {i}}}"),
            SpanError::UnsupportedSectionSchema { version } => write!(f, "{c}{{version: {version}}}"),
            SpanError::SpanOutOfRange { bound } => write!(f, "{c}{{{bound}}}"),
            SpanError::SpanTooShort { needed, got } => write!(f, "{c}{{needed: {needed}, got: {got}}}"),
            SpanError::AmbiguousAssociation { count } => write!(f, "{c}{{count: {count}}}"),
            SpanError::FitKindMismatch { kind, alg } => {
                write!(f, "{c}{{kind: {}, alg: {}}}", kind.as_str(), alg.escape_debug())
            }
            SpanError::FitUnavailable { alg } | SpanError::AssociationParamsUnsupported { alg } => {
                write!(f, "{c}{{{}}}", alg.escape_debug())
            }
            SpanError::FitRefused { code, local_optimum_only, suspect_axis } => {
                write!(f, "{c}{{{code}}}")?;
                if *local_optimum_only {
                    f.write_str("[local_optimum_only]")?;
                }
                if *suspect_axis {
                    f.write_str("[suspect_axis]")?;
                }
                Ok(())
            }
            SpanError::SectionDigestMismatch
            | SpanError::SectionMismatch
            | SpanError::ChainWithoutAssociation
            | SpanError::Allocation => f.write_str(c),
        }
    }
}

impl std::error::Error for SpanError {}

/// Procedure step 2: the chain's one association algorithm, checked against the kind.
fn chain_fit(chain: &OperatorChain, kind: ElementKind) -> Result<String, SpanError> {
    let mut assoc = chain.steps().iter().filter(|s| s.kind() == OperatorKind::Association);
    let alg = match (assoc.next(), assoc.next()) {
        (None, _) => return Err(SpanError::ChainWithoutAssociation),
        (Some(one), None) if one.params().is_empty() => one.alg().as_str().to_owned(),
        (Some(one), None) => {
            return Err(SpanError::AssociationParamsUnsupported { alg: one.alg().as_str().to_owned() });
        }
        (Some(_), Some(_)) => {
            return Err(SpanError::AmbiguousAssociation { count: 2 + assoc.count() });
        }
    };
    let parts = alg.split_once('@').and_then(|(body, _)| {
        let mut it = body.split('.');
        match (it.next(), it.next(), it.next(), it.next()) {
            (Some("fit"), Some(p), Some(m), None) => Some((Primitive::parse(p)?, Method::parse(m)?)),
            _ => None,
        }
    });
    let method = match parts {
        Some((p, m)) if p == kind.primitive() => m,
        _ => return Err(SpanError::FitKindMismatch { kind, alg }),
    };
    if method != Method::LeastSquares {
        return Err(SpanError::FitUnavailable { alg });
    }
    match algorithm_id(kind.primitive(), method, kind.version()) {
        Ok(id) if id.to_string() == alg => Ok(alg),
        _ => Err(SpanError::FitUnavailable { alg }),
    }
}

/// Procedure steps 3 and 4.
fn check_span(curve: &SectionCurveView, span: &CurveSpan) -> Result<(), SpanError> {
    for x in [span.start, span.end] {
        if !x.is_finite() {
            return Err(SpanError::BadSpan { reason: "not_finite" });
        }
        if x == 0.0 && x.is_sign_negative() {
            return Err(SpanError::BadSpan { reason: "negative_zero" });
        }
    }
    let len = curve.length();
    for (x, bound) in [(span.start, "start"), (span.end, "end")] {
        if x.total_cmp(&0.0) == Ordering::Less || x.total_cmp(&len) == Ordering::Greater {
            return Err(SpanError::SpanOutOfRange { bound });
        }
    }
    let order = span.start.total_cmp(&span.end);
    if !span.wrap {
        if order != Ordering::Less {
            return Err(SpanError::BadSpan { reason: "start_not_before_end" });
        }
    } else if !curve.closed() {
        return Err(SpanError::BadSpan { reason: "wrap_on_open_curve" });
    } else if order != Ordering::Greater {
        return Err(SpanError::BadSpan { reason: "wrap_needs_start_after_end" });
    }
    Ok(())
}

/// Procedure step 5: `(first, count)` of the selection.
fn select(curve: &SectionCurveView, span: &CurveSpan) -> (usize, usize) {
    let arc = curve.arc_lengths();
    let n = arc.len();
    // First vertex with parameter >= start, and the number with parameter <= end.
    let lo = arc.partition_point(|a| a.total_cmp(&span.start) == Ordering::Less);
    let hi = arc.partition_point(|a| a.total_cmp(&span.end) != Ordering::Greater);
    if span.wrap {
        let tail = n.saturating_sub(lo);
        let first = if tail > 0 { lo } else { 0 };
        (first, tail.saturating_add(hi))
    } else {
        (lo, hi.saturating_sub(lo))
    }
}

/// Procedure step 6: the fit, with `fid-fit`'s errors mapped at once.
fn run_fit(kind: ElementKind, xyz: &[f64]) -> Result<FitResult, SpanError> {
    let pts = PointsView::new(xyz, None).map_err(|e| SpanError::from_fit(&e))?;
    let line_params = FitParams::new(Method::LeastSquares);
    let result = match kind {
        ElementKind::Line2d => ls_lin::Line2Ls.fit(&pts, &line_params, None),
        ElementKind::Circle2d => ls_rnd::Circle2Ls.fit(&pts, &ls_rnd::default_params(), None),
        ElementKind::Point2d => ls_lin::PointLs.fit(&pts, &line_params, None),
    };
    result.map_err(|e| SpanError::from_fit(&e))
}

fn finite(xs: &[f64]) -> bool {
    xs.iter().all(|x| x.is_finite())
}

/// Procedure step 7: the element parameters of an accepted fit result.
fn params_of(kind: ElementKind, feature: &FittedFeature) -> Result<Params2d, SpanError> {
    let p = match (kind, feature) {
        (ElementKind::Line2d, FittedFeature::Line2 { p, d }) if finite(p) && finite(d) => {
            Params2d::Line2d { p: *p, d: *d }
        }
        (ElementKind::Circle2d, FittedFeature::Circle2 { c, r })
            if finite(c) && r.is_finite() && r.total_cmp(&0.0) == Ordering::Greater && !r.is_subnormal() =>
        {
            Params2d::Circle2d { c: *c, r: *r }
        }
        (ElementKind::Point2d, FittedFeature::Point { c: [u, v, w] }) if finite(&[*u, *v]) && *w == 0.0 => {
            Params2d::Point2d { c: [*u, *v] }
        }
        _ => return Err(SpanError::bad_result()),
    };
    Ok(p)
}

/// Extracts the element of one span by the pinned procedure of the module documentation.
pub fn extract_section_element(
    curve: &SectionCurveView,
    span: &CurveSpan,
    kind: ElementKind,
    chain: &OperatorChain,
) -> Result<SectionElement, SpanError> {
    if span.section_id != curve.section_id() {
        return Err(SpanError::SectionMismatch);
    }
    let fit_alg = chain_fit(chain, kind)?;
    check_span(curve, span)?;
    let (first, count) = select(curve, span);
    if count < kind.needed() {
        return Err(SpanError::SpanTooShort { needed: kind.needed(), got: count });
    }
    let vertices = curve.vertices();
    let mut xyz: Vec<f64> = Vec::new();
    xyz.try_reserve_exact(count.saturating_mul(3)).map_err(|_| SpanError::Allocation)?;
    for p in vertices.iter().cycle().skip(first).take(count) {
        xyz.extend_from_slice(&[p[0], p[1], 0.0]);
    }
    let result = run_fit(kind, &xyz)?;
    if result.algorithm.to_string() != fit_alg {
        return Err(SpanError::bad_result());
    }
    let params = params_of(kind, &result.feature)?;
    let bytes = encode(&result).map_err(|e| SpanError::from_fit(&e))?;
    let fit_digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
    Ok(SectionElement {
        kind,
        params,
        frame: curve.frame(),
        plane: *curve.plane(),
        section_id: curve.section_id().to_owned(),
        section_digest: curve.digest(),
        span: span.clone(),
        fit_alg,
        fit_digest,
        first_vertex: first,
        vertex_count: count,
    })
}
