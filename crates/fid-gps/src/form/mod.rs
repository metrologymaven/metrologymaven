//! Form evaluation contract and reference-association policy (spec node `GDT.FORM.FCORE`,
//! `REQ-001`..`REQ-006`, `AC-1`..`AC-6`).
//!
//! Pure KNP-1 kernel math: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! `fid_num::f` for square roots, `f64::total_cmp` for max/min with lowest-index tie breaking,
//! NaN rejected as an error.

use crate::dmt::{OperatorChain, OperatorKind, Param};
use core::cmp::Ordering;
use core::fmt;
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView};
use fid_fit::{ls_cyl, ls_lin, ls_rnd, vor};
use fid_num::f as numf;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Evaluation basis recorded by form checks (`GDT.FORM.FCORE` Scope).
pub const EVALUATION_BASIS_POINTS: &str = "POINTS";

/// Geometric element kind evaluated by the form core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormElement {
    Plane,
    Line2d,
    Line3d,
    Circle2d,
    Cylinder,
}

impl FormElement {
    pub const fn as_str(self) -> &'static str {
        match self {
            FormElement::Plane => "plane",
            FormElement::Line2d => "line2d",
            FormElement::Line3d => "line3d",
            FormElement::Circle2d => "circle2d",
            FormElement::Cylinder => "cylinder",
        }
    }

    /// Geometric minimum point count when the resolved row does not supply an explicit
    /// `min_points` parameter.
    pub const fn geometric_min_points(self) -> usize {
        match self {
            FormElement::Plane => 4,
            FormElement::Line2d => 3,
            FormElement::Line3d => 3,
            FormElement::Circle2d => 4,
            FormElement::Cylinder => 5,
        }
    }
}

/// Reference association actually used to evaluate a form characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormAssociation {
    MinimumZone,
    LeastSquares,
}

impl FormAssociation {
    pub const fn as_str(self) -> &'static str {
        match self {
            FormAssociation::MinimumZone => "minimum_zone",
            FormAssociation::LeastSquares => "least_squares",
        }
    }
}

/// Advisory/caveat flags attached to a [`FormPayload`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormFlag {
    /// Minimum zone was requested by the chain, but KRN reported MZ unavailable for the
    /// element type, so least-squares residual range was used as a conservative upper bound
    /// (`REQ-003`, `AC-2`).
    ConservativeUpperBound,
    /// The underlying KRN fit reported `local_optimum_only` (`REQ-006`, `AC-5`).
    LocalOptimumOnly,
}

impl FormFlag {
    pub const fn as_str(self) -> &'static str {
        match self {
            FormFlag::ConservativeUpperBound => "conservative_upper_bound",
            FormFlag::LocalOptimumOnly => "local_optimum_only",
        }
    }
}

/// Raw outcome produced by a [`FitProvider`] for one form evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct FitOutcome {
    /// Canonical reference element parameters.
    pub reference: FittedFeature,
    /// Signed residual of each input point, in input index order, in SI metres.
    pub signed_residuals: Vec<f64>,
    /// Minimum-zone width recomputed from the active set (metres), when available (`REQ-002`).
    pub active_zone_width: Option<f64>,
    /// Algorithm ID of the fit actually executed.
    pub algorithm_id: String,
    /// Whether the fit reported `local_optimum_only` (`REQ-006`).
    pub local_optimum_only: bool,
}

/// Errors returned by [`evaluate_form`]. [`FormError::code`] is stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    /// `too_few_points{required, given}` (`REQ-001`, `AC-3`).
    TooFewPoints { required: usize, given: usize },
    /// `association_unknown{alg}`: chain has no association step or names an unknown criterion.
    AssociationUnknown { alg: String },
    /// `nan_input`: a point coordinate, residual, or zone width was NaN or infinite.
    NanInput,
    /// `residual_count_mismatch`: fit returned a different residual count than input points.
    ResidualCountMismatch { expected: usize, got: usize },
    /// `fit_refused{code}`: pass-through error from KRN fit (degenerate set, unavailable fit, etc.).
    FitRefused { code: &'static str },
}

impl FormError {
    pub const fn code(&self) -> &'static str {
        match self {
            FormError::TooFewPoints { .. } => "too_few_points",
            FormError::AssociationUnknown { .. } => "association_unknown",
            FormError::NanInput => "nan_input",
            FormError::ResidualCountMismatch { .. } => "residual_count_mismatch",
            FormError::FitRefused { .. } => "fit_refused",
        }
    }
}

impl fmt::Display for FormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormError::TooFewPoints { required, given } => {
                write!(f, "too_few_points{{required={required}, given={given}}}")
            }
            FormError::AssociationUnknown { alg } => write!(f, "association_unknown{{{alg}}}"),
            FormError::NanInput => f.write_str("nan_input"),
            FormError::ResidualCountMismatch { expected, got } => {
                write!(f, "residual_count_mismatch{{expected={expected}, got={got}}}")
            }
            FormError::FitRefused { code } => write!(f, "fit_refused{{{code}}}"),
        }
    }
}

impl std::error::Error for FormError {}

/// Common form evaluation payload (`GDT.FORM.FCORE` Scope & `REQ-001`..`REQ-006`).
#[derive(Debug, Clone, PartialEq)]
pub struct FormPayload {
    pub element: FormElement,
    /// Evaluated form deviation in SI metres.
    pub value_si: f64,
    /// Canonical reference element parameters.
    pub reference: FittedFeature,
    /// Reference association actually applied.
    pub association: FormAssociation,
    /// Algorithm ID of the reference fit executed.
    pub algorithm_id: String,
    /// Lowest index of the point achieving the maximum signed residual (`REQ-005`, `AC-4`).
    pub peak_index: usize,
    /// Lowest index of the point achieving the minimum signed residual (`REQ-005`).
    pub valley_index: usize,
    /// Sorted, duplicate-free flag set (`conservative_upper_bound`, `local_optimum_only`).
    pub flags: BTreeSet<FormFlag>,
}

impl FormPayload {
    /// Canonical JSON representation (with exact IEEE-754 hex bits for `value_si` and
    /// canonical `fid-fit` digest for `reference`) for cross-platform `D0` verification (`AC-6`).
    pub fn to_cvalue(&self) -> Result<CValue, FormError> {
        let hex = |x: f64| CValue::Str(format!("0x{:016x}", x.to_bits()));
        let v2 = |v: [f64; 2]| CValue::Arr(vec![hex(v[0]), hex(v[1])]);
        let v3 = |v: [f64; 3]| CValue::Arr(vec![hex(v[0]), hex(v[1]), hex(v[2])]);
        let mut ref_m: BTreeMap<String, CValue> = BTreeMap::new();
        match self.reference {
            FittedFeature::Plane { p, n } => {
                ref_m.insert("kind".to_owned(), CValue::Str("plane".to_owned()));
                ref_m.insert("n".to_owned(), v3(n));
                ref_m.insert("p".to_owned(), v3(p));
            }
            FittedFeature::Line2 { p, d } => {
                ref_m.insert("d".to_owned(), v2(d));
                ref_m.insert("kind".to_owned(), CValue::Str("line2".to_owned()));
                ref_m.insert("p".to_owned(), v2(p));
            }
            FittedFeature::Line3 { p, d } => {
                ref_m.insert("d".to_owned(), v3(d));
                ref_m.insert("kind".to_owned(), CValue::Str("line3".to_owned()));
                ref_m.insert("p".to_owned(), v3(p));
            }
            FittedFeature::Circle2 { c, r } => {
                ref_m.insert("c".to_owned(), v2(c));
                ref_m.insert("kind".to_owned(), CValue::Str("circle2".to_owned()));
                ref_m.insert("r".to_owned(), hex(r));
            }
            FittedFeature::Cylinder { p, d, r } => {
                ref_m.insert("d".to_owned(), v3(d));
                ref_m.insert("kind".to_owned(), CValue::Str("cylinder".to_owned()));
                ref_m.insert("p".to_owned(), v3(p));
                ref_m.insert("r".to_owned(), hex(r));
            }
            _ => return Err(FormError::FitRefused { code: "unsupported_form_feature" }),
        }
        let mut m: BTreeMap<String, CValue> = BTreeMap::new();
        m.insert("algorithm_id".to_owned(), CValue::Str(self.algorithm_id.clone()));
        m.insert("association".to_owned(), CValue::Str(self.association.as_str().to_owned()));
        m.insert("element".to_owned(), CValue::Str(self.element.as_str().to_owned()));
        m.insert("evaluation_basis".to_owned(), CValue::Str(EVALUATION_BASIS_POINTS.to_owned()));
        m.insert(
            "flags".to_owned(),
            CValue::Arr(self.flags.iter().map(|fl| CValue::Str(fl.as_str().to_owned())).collect()),
        );
        m.insert("peak_index".to_owned(), CValue::Str(self.peak_index.to_string()));
        m.insert("reference".to_owned(), CValue::Obj(ref_m));
        m.insert("valley_index".to_owned(), CValue::Str(self.valley_index.to_string()));
        m.insert("value_si_hex".to_owned(), hex(self.value_si));
        Ok(CValue::Obj(m))
    }

    /// SHA-256 content digest of the canonical payload bytes (`AC-6`).
    pub fn digest(&self) -> Result<Digest, FormError> {
        let val = self.to_cvalue()?;
        let bytes = to_canonical_bytes(&val).map_err(|_| FormError::FitRefused { code: "canonical_encode" })?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

/// Kernel fit provider injected into [`evaluate_form`]. Allows real KRN fits ([`KrnFitProvider`])
/// and deterministic stubs in tests (`AC-1`, `AC-2`, `AC-5`).
pub trait FitProvider {
    /// Returns `true` when a minimum-zone fit is available for `element`.
    fn mz_available(&self, element: FormElement) -> bool;
    /// Runs the minimum-zone fit for `element`.
    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError>;
    /// Runs the least-squares fit for `element`.
    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError>;
}

/// Evaluates form deviation through the shared `GDT.FORM.FCORE` contract (`REQ-001`..`REQ-006`).
pub fn evaluate_form(
    element: FormElement,
    points: &[[f64; 3]],
    chain: &OperatorChain,
    krn: &dyn FitProvider,
) -> Result<FormPayload, FormError> {
    // Validate input finiteness up front (KNP-1: NaN is an error).
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(FormError::NanInput);
        }
    }

    // Resolve association step, requested policy, and row minimum point count.
    let (requested, required_min) = resolve_association_and_min(element, chain)?;

    // REQ-001: minimum point check before calling any fit.
    if points.len() < required_min {
        return Err(FormError::TooFewPoints { required: required_min, given: points.len() });
    }

    // REQ-002 / REQ-003 / REQ-004: choose association and fallback policy.
    let mut flags = BTreeSet::new();
    let (association, outcome) = match requested {
        FormAssociation::MinimumZone => {
            if krn.mz_available(element) {
                (FormAssociation::MinimumZone, krn.fit_minimum_zone(element, points)?)
            } else {
                flags.insert(FormFlag::ConservativeUpperBound);
                (FormAssociation::LeastSquares, krn.fit_least_squares(element, points)?)
            }
        }
        FormAssociation::LeastSquares => (FormAssociation::LeastSquares, krn.fit_least_squares(element, points)?),
    };

    // REQ-006: propagate local_optimum_only from KRN fit.
    if outcome.local_optimum_only {
        flags.insert(FormFlag::LocalOptimumOnly);
    }

    // Canonicalise reference feature.
    let reference = outcome.reference.canonical().map_err(|e| FormError::FitRefused { code: e.code() })?;

    // REQ-005: fixed-order scan of signed residuals with total_cmp and lowest-index tie breaking.
    if outcome.signed_residuals.len() != points.len() {
        return Err(FormError::ResidualCountMismatch { expected: points.len(), got: outcome.signed_residuals.len() });
    }
    let Some(&first_r) = outcome.signed_residuals.first() else {
        return Err(FormError::TooFewPoints { required: required_min, given: 0 });
    };
    if !first_r.is_finite() {
        return Err(FormError::NanInput);
    }
    let mut max_val = first_r;
    let mut min_val = first_r;
    let mut peak_index = 0_usize;
    let mut valley_index = 0_usize;
    for (idx, &r) in outcome.signed_residuals.iter().enumerate().skip(1) {
        if !r.is_finite() {
            return Err(FormError::NanInput);
        }
        if r.total_cmp(&max_val) == Ordering::Greater {
            max_val = r;
            peak_index = idx;
        }
        if r.total_cmp(&min_val) == Ordering::Less {
            min_val = r;
            valley_index = idx;
        }
    }

    let range = canonical_zero(max_val - min_val);
    if !range.is_finite() || range < 0.0 {
        return Err(FormError::NanInput);
    }

    let value_si = match (association, outcome.active_zone_width) {
        (FormAssociation::MinimumZone, Some(w)) => {
            if !w.is_finite() || w < 0.0 {
                return Err(FormError::NanInput);
            }
            canonical_zero(w)
        }
        _ => range,
    };

    Ok(FormPayload {
        element,
        value_si,
        reference,
        association,
        algorithm_id: outcome.algorithm_id,
        peak_index,
        valley_index,
        flags,
    })
}

fn resolve_association_and_min(
    element: FormElement,
    chain: &OperatorChain,
) -> Result<(FormAssociation, usize), FormError> {
    let assoc_steps: Vec<_> = chain.steps().iter().filter(|op| op.kind() == OperatorKind::Association).collect();
    if assoc_steps.len() != 1 {
        return Err(FormError::AssociationUnknown {
            alg: if assoc_steps.is_empty() { "none".to_owned() } else { format!("ambiguous({})", assoc_steps.len()) },
        });
    }
    let step = assoc_steps[0];
    let alg = step.alg().as_str();

    // Row minimum point count: explicit `min_points` param if present, else geometric floor.
    let required_min = match step.params().get("min_points") {
        Some(Param::Int(n)) if *n >= 1 => usize::try_from(*n).unwrap_or(usize::MAX),
        Some(Param::Int(_)) => return Err(FormError::AssociationUnknown { alg: "bad_min_points".to_owned() }),
        _ => element.geometric_min_points(),
    };

    // Explicit `criterion` token takes precedence; otherwise infer from algorithm method token.
    let association = match step.params().get("criterion") {
        Some(Param::Token(t)) if matches!(t.as_str(), "minimum_zone" | "min_zone" | "chebyshev" | "mz") => {
            FormAssociation::MinimumZone
        }
        Some(Param::Token(t)) if matches!(t.as_str(), "least_squares" | "gaussian" | "ls") => {
            FormAssociation::LeastSquares
        }
        Some(Param::Token(t)) => return Err(FormError::AssociationUnknown { alg: t.clone() }),
        _ => classify_alg(alg)?,
    };
    Ok((association, required_min))
}

fn classify_alg(alg: &str) -> Result<FormAssociation, FormError> {
    // Parse `<module>.<feature>.<method>@<semver>` or legacy tokens.
    let base = alg.split('@').next().unwrap_or(alg);
    let method = base.rsplit('.').next().unwrap_or(base);
    if method == "mz"
        || method.starts_with("mz_")
        || method.contains("chebyshev")
        || method.contains("min_zone")
        || method.contains("minimum_zone")
    {
        Ok(FormAssociation::MinimumZone)
    } else if method == "ls"
        || method.starts_with("ls_")
        || method.contains("gaussian")
        || method.contains("least_squares")
    {
        Ok(FormAssociation::LeastSquares)
    } else {
        Err(FormError::AssociationUnknown { alg: alg.to_owned() })
    }
}

/// Real KRN [`FitProvider`] backed by `fid-fit` (`KRN.LSQ.LIN`, `KRN.LSQ.RND`, `KRN.LSQ.AXIS.CYL`,
/// and `KRN.MZ.VOR` for `Circle2d`).
#[derive(Debug, Clone, Copy, Default)]
pub struct KrnFitProvider;

impl FitProvider for KrnFitProvider {
    fn mz_available(&self, element: FormElement) -> bool {
        matches!(element, FormElement::Circle2d)
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        match element {
            FormElement::Circle2d => {
                let flat = flatten_3d(points);
                let view = PointsView::new(&flat, None).map_err(|e| FormError::FitRefused { code: e.code() })?;
                let mz = vor::mz_circle2d(&view).map_err(|e| FormError::FitRefused { code: e.code() })?;
                let feature = FittedFeature::Circle2 { c: mz.center, r: mz.r_mid };
                let residuals = signed_residuals_of(&feature, points)?;
                Ok(FitOutcome {
                    reference: feature,
                    signed_residuals: residuals,
                    active_zone_width: Some(canonical_zero(mz.width)),
                    algorithm_id: vor::ALGORITHM_ID.to_owned(),
                    local_optimum_only: false,
                })
            }
            _ => Err(FormError::FitRefused { code: "mz_unavailable" }),
        }
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        let flat = flatten_3d(points);
        let view = PointsView::new(&flat, None).map_err(|e| FormError::FitRefused { code: e.code() })?;
        let ls_params = FitParams::new(Method::LeastSquares);
        match element {
            FormElement::Plane => {
                let res = ls_lin::PlaneLs
                    .fit(&view, &ls_params, None)
                    .map_err(|e| FormError::FitRefused { code: e.code() })?;
                let residuals = signed_residuals_of(&res.feature, points)?;
                Ok(FitOutcome {
                    reference: res.feature,
                    signed_residuals: residuals,
                    active_zone_width: None,
                    algorithm_id: res.algorithm.to_string(),
                    local_optimum_only: false,
                })
            }
            FormElement::Line2d => {
                let res = ls_lin::Line2Ls
                    .fit(&view, &ls_params, None)
                    .map_err(|e| FormError::FitRefused { code: e.code() })?;
                let residuals = signed_residuals_of(&res.feature, points)?;
                Ok(FitOutcome {
                    reference: res.feature,
                    signed_residuals: residuals,
                    active_zone_width: None,
                    algorithm_id: res.algorithm.to_string(),
                    local_optimum_only: false,
                })
            }
            FormElement::Line3d => {
                let res = ls_lin::Line3Ls
                    .fit(&view, &ls_params, None)
                    .map_err(|e| FormError::FitRefused { code: e.code() })?;
                let residuals = signed_residuals_of(&res.feature, points)?;
                Ok(FitOutcome {
                    reference: res.feature,
                    signed_residuals: residuals,
                    active_zone_width: None,
                    algorithm_id: res.algorithm.to_string(),
                    local_optimum_only: false,
                })
            }
            FormElement::Circle2d => {
                let res = ls_rnd::Circle2Ls
                    .fit(&view, &ls_rnd::default_params(), None)
                    .map_err(|e| FormError::FitRefused { code: e.code() })?;
                let residuals = signed_residuals_of(&res.feature, points)?;
                Ok(FitOutcome {
                    reference: res.feature,
                    signed_residuals: residuals,
                    active_zone_width: None,
                    algorithm_id: res.algorithm.to_string(),
                    local_optimum_only: false,
                })
            }
            FormElement::Cylinder => {
                let cyl = ls_cyl::fit_cylinder(&view, &ls_params, Some(0))
                    .map_err(|e| FormError::FitRefused { code: e.code() })?;
                let residuals = signed_residuals_of(&cyl.result.feature, points)?;
                Ok(FitOutcome {
                    reference: cyl.result.feature,
                    signed_residuals: residuals,
                    active_zone_width: None,
                    algorithm_id: cyl.result.algorithm.to_string(),
                    local_optimum_only: cyl.local_optimum_only,
                })
            }
        }
    }
}

/// Computes signed geometric residuals against a canonical `FittedFeature` in fixed point order.
pub fn signed_residuals_of(feature: &FittedFeature, points: &[[f64; 3]]) -> Result<Vec<f64>, FormError> {
    let mut out = Vec::with_capacity(points.len());
    match *feature {
        FittedFeature::Plane { p, n } => {
            for q in points {
                let r = (((q[0] - p[0]) * n[0]) + ((q[1] - p[1]) * n[1])) + ((q[2] - p[2]) * n[2]);
                out.push(canonical_zero(r));
            }
        }
        FittedFeature::Line2 { p, d } => {
            // Signed perpendicular distance in XY plane: (-dy, dx) . (q - p).
            for q in points {
                let r = ((q[0] - p[0]) * (-d[1])) + ((q[1] - p[1]) * d[0]);
                out.push(canonical_zero(r));
            }
        }
        FittedFeature::Line3 { p, d } => {
            for q in points {
                let wx = q[0] - p[0];
                let wy = q[1] - p[1];
                let wz = q[2] - p[2];
                let proj = ((wx * d[0]) + (wy * d[1])) + (wz * d[2]);
                let px = wx - (proj * d[0]);
                let py = wy - (proj * d[1]);
                let pz = wz - (proj * d[2]);
                let dist = numf::sqrt(((px * px) + (py * py)) + (pz * pz)).map_err(|_| FormError::NanInput)?;
                out.push(canonical_zero(dist));
            }
        }
        FittedFeature::Circle2 { c, r } => {
            for q in points {
                let dx = q[0] - c[0];
                let dy = q[1] - c[1];
                let rad = numf::sqrt((dx * dx) + (dy * dy)).map_err(|_| FormError::NanInput)?;
                out.push(canonical_zero(rad - r));
            }
        }
        FittedFeature::Cylinder { p, d, r } => {
            for q in points {
                let wx = q[0] - p[0];
                let wy = q[1] - p[1];
                let wz = q[2] - p[2];
                let proj = ((wx * d[0]) + (wy * d[1])) + (wz * d[2]);
                let px = wx - (proj * d[0]);
                let py = wy - (proj * d[1]);
                let pz = wz - (proj * d[2]);
                let rad = numf::sqrt(((px * px) + (py * py)) + (pz * pz)).map_err(|_| FormError::NanInput)?;
                out.push(canonical_zero(rad - r));
            }
        }
        _ => return Err(FormError::FitRefused { code: "unsupported_form_feature" }),
    }
    Ok(out)
}

fn flatten_3d(points: &[[f64; 3]]) -> Vec<f64> {
    let mut flat = Vec::with_capacity(points.len() * 3);
    for p in points {
        flat.push(p[0]);
        flat.push(p[1]);
        flat.push(p[2]);
    }
    flat
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
