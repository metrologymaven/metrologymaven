//! Straightness of a derived median line with optional MMC (spec node `GDT.FORM.STRAIGHT.DERIVED`).
//!
//! Pure KNP-1 kernel math: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! `fid_num::f` for square roots, `f64::total_cmp` with lowest-index tie breaking, and NaN
//! rejected as an error.

use crate::dmt::{OperatorChain, OperatorKind, Param};
use crate::form::{
    resolve_association_and_min, FitOutcome, FitProvider, FormAssociation, FormElement, FormError, FormFlag,
};
use core::cmp::Ordering;
use core::fmt;
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_lin;
use fid_num::f as numf;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Algorithm identifier for KNP-1 minimum-zone 3D line (smallest enclosing cylinder) fit.
pub const MZ_LINE3D_ALGORITHM_ID: &str = "fit.line3d.mz@1.0.0";

// ------------------------------------------------------------------ derivation & MMC interfaces

/// Derived geometric element returned by a `loc::derive::derive()` provider (`GDT.LOC.DERIV.TYPES`).
#[derive(Debug, Clone, PartialEq)]
pub enum DerivedElement {
    /// Ordered sequence of 3D points forming the derived median line of a feature of size.
    MedianLine { points: Vec<[f64; 3]>, reduction_alg: String },
}

/// Derivation provider (`GDT.LOC.DERIV.MLINE` or test stub) producing a [`DerivedElement`]
/// from input surface points and the chain's reduction operator (`REQ-001`, `REQ-004`).
pub trait MedianLineDeriver {
    fn derive(
        &self,
        points: &[[f64; 3]],
        reduction_alg: &str,
        params: &BTreeMap<String, Param>,
    ) -> Result<DerivedElement, String>;
}

/// Pass-through deriver for callers/tests that already provide sampled median-line points.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityMedianLineDeriver;

impl MedianLineDeriver for IdentityMedianLineDeriver {
    fn derive(
        &self,
        points: &[[f64; 3]],
        reduction_alg: &str,
        _params: &BTreeMap<String, Param>,
    ) -> Result<DerivedElement, String> {
        if points.len() < 3 {
            return Err("too_few_sections".to_owned());
        }
        Ok(DerivedElement::MedianLine { points: points.to_vec(), reduction_alg: reduction_alg.to_owned() })
    }
}

/// Material-condition modifier on the straightness characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MaterialModifier {
    Rfs,
    Mmc,
    Lmc,
}

impl MaterialModifier {
    pub const fn as_str(self) -> &'static str {
        match self {
            MaterialModifier::Rfs => "RFS",
            MaterialModifier::Mmc => "MMC",
            MaterialModifier::Lmc => "LMC",
        }
    }
}

/// Tolerance specification passed to [`evaluate_derived_straightness`] (`REQ-003`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DerivedToleranceSpec {
    /// Stated geometric tolerance in SI metres (e.g. `1e-5` for 0.010 mm).
    pub stated_si: f64,
    /// Material condition modifier (`Rfs`, `Mmc`, `Lmc`).
    pub modifier: MaterialModifier,
}

/// Stated and bonus tolerance computed by a material-condition engine (`GDT.ZONE.MATL`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialAllowance {
    pub stated_si: f64,
    pub bonus_si: f64,
}

/// Material-condition engine hook (`GDT.ZONE.MATL` or test stub) computing bonus tolerance
/// separately from stated tolerance (`REQ-003`, `AC-3`).
pub trait MaterialConditionProvider {
    fn compute_allowance(&self, spec: &DerivedToleranceSpec) -> Result<MaterialAllowance, String>;
}

/// No-op material-condition provider for RFS characteristics (refuses MMC/LMC).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMaterialBonusProvider;

impl MaterialConditionProvider for NoMaterialBonusProvider {
    fn compute_allowance(&self, spec: &DerivedToleranceSpec) -> Result<MaterialAllowance, String> {
        match spec.modifier {
            MaterialModifier::Rfs => Ok(MaterialAllowance { stated_si: spec.stated_si, bonus_si: 0.0 }),
            MaterialModifier::Mmc | MaterialModifier::Lmc => Err("material_condition_unavailable".to_owned()),
        }
    }
}

// ------------------------------------------------------------------ errors & payload

/// Errors returned by [`evaluate_derived_straightness`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedStraightnessError {
    /// Pass-through failure from `derive()` (`REQ-004`, `AC-4`).
    DerivationFailed { reason: String },
    /// Resolved chain did not contain a reduction operator producing a median line.
    ReductionMissing,
    /// Underlying form / fit error (`too_few_points`, `nan_input`, `association_unknown`, `fit_refused`).
    Form(FormError),
    /// Unsupported algorithm in the chain (`UNSUPPORTED{alg}`).
    Unsupported { alg: String },
    /// Material-condition engine refusal or invalid tolerance.
    MaterialConditionFailed { reason: String },
}

impl DerivedStraightnessError {
    pub const fn code(&self) -> &'static str {
        match self {
            DerivedStraightnessError::DerivationFailed { .. } => "derivation_failed",
            DerivedStraightnessError::ReductionMissing => "reduction_missing",
            DerivedStraightnessError::Form(e) => e.code(),
            DerivedStraightnessError::Unsupported { .. } => "UNSUPPORTED",
            DerivedStraightnessError::MaterialConditionFailed { .. } => "material_condition_failed",
        }
    }
}

impl fmt::Display for DerivedStraightnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DerivedStraightnessError::DerivationFailed { reason } => write!(f, "derivation_failed{{{reason}}}"),
            DerivedStraightnessError::ReductionMissing => {
                write!(f, "reduction_missing: chain has no median-line reduction operator")
            }
            DerivedStraightnessError::Form(e) => write!(f, "{e}"),
            DerivedStraightnessError::Unsupported { alg } => write!(f, "UNSUPPORTED{{{alg}}}"),
            DerivedStraightnessError::MaterialConditionFailed { reason } => {
                write!(f, "material_condition_failed{{{reason}}}")
            }
        }
    }
}

impl std::error::Error for DerivedStraightnessError {}

/// Payload returned by [`evaluate_derived_straightness`] (`GDT.FORM.STRAIGHT.DERIVED` Scope):
/// `{ value, reference_line, extreme_index, stated, bonus?, flags }`.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedStraightnessPayload {
    /// Cylindrical zone diameter in SI metres (`2 * max distance` to `reference_line`).
    pub value_si: f64,
    /// Canonical associated 3D reference line (`FittedFeature::Line3`).
    pub reference_line: FittedFeature,
    /// Index into the derived median-line points attaining the maximum radial distance
    /// (lowest index on exact tie).
    pub extreme_index: usize,
    /// Stated tolerance in SI metres (`REQ-003`).
    pub stated_si: f64,
    /// Separate bonus tolerance in SI metres when MMC/LMC applies (`REQ-003`, `AC-3`).
    pub bonus_si: Option<f64>,
    /// Association actually used (`MinimumZone` or `LeastSquares`).
    pub association: FormAssociation,
    /// Algorithm identifier of the 3D line fit actually run.
    pub algorithm_id: String,
    /// Advisory flags (e.g. `ConservativeUpperBound`, `LocalOptimumOnly`).
    pub flags: BTreeSet<FormFlag>,
    /// Number of median-line points evaluated.
    pub point_count: usize,
}

impl DerivedStraightnessPayload {
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("algorithm_id".to_owned(), CValue::Str(self.algorithm_id.clone()));
        m.insert("association".to_owned(), CValue::Str(self.association.as_str().to_owned()));
        if let Some(bonus) = self.bonus_si {
            m.insert("bonus_si".to_owned(), CValue::Num(bonus));
            m.insert("bonus_si_hex".to_owned(), CValue::Str(format!("{:016x}", bonus.to_bits())));
        }
        m.insert("extreme_index".to_owned(), CValue::Num(self.extreme_index as f64));
        m.insert(
            "flags".to_owned(),
            CValue::Arr(self.flags.iter().map(|f| CValue::Str(f.as_str().to_owned())).collect()),
        );
        m.insert("point_count".to_owned(), CValue::Num(self.point_count as f64));
        m.insert("reference_line".to_owned(), line3_cvalue(&self.reference_line));
        m.insert("stated_si".to_owned(), CValue::Num(self.stated_si));
        m.insert("stated_si_hex".to_owned(), CValue::Str(format!("{:016x}", self.stated_si.to_bits())));
        m.insert("value_si".to_owned(), CValue::Num(self.value_si));
        m.insert("value_si_hex".to_owned(), CValue::Str(format!("{:016x}", self.value_si.to_bits())));
        CValue::Obj(m)
    }

    pub fn canonical_digest(&self) -> Result<Digest, DerivedStraightnessError> {
        let bytes =
            to_canonical_bytes(&self.to_cvalue()).map_err(|_| DerivedStraightnessError::Form(FormError::NanInput))?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

// ------------------------------------------------------------------ chain inspection helper

/// Returns `true` if `chain` contains a reduction operator producing a median line
/// (`REQ-GDT.FORM.STRAIGHT.CMD-001`).
pub fn chain_has_median_line_reduction(chain: &OperatorChain) -> bool {
    find_median_line_reduction(chain).is_some()
}

fn find_median_line_reduction(chain: &OperatorChain) -> Option<(&str, &BTreeMap<String, Param>)> {
    for op in chain.steps() {
        if op.kind() == OperatorKind::Reduction {
            let alg = op.alg().as_str();
            if alg.contains("median_line") || alg.contains("mline") || alg.contains("derived_line") {
                return Some((alg, op.params()));
            }
        }
    }
    None
}

// ------------------------------------------------------------------ evaluation entry point

/// Evaluates straightness of a derived median line with optional MMC (`REQ-001`..`REQ-005`).
pub fn evaluate_derived_straightness(
    points: &[[f64; 3]],
    chain: &OperatorChain,
    deriver: &dyn MedianLineDeriver,
    krn: &dyn FitProvider,
    tol: &DerivedToleranceSpec,
    matl: &dyn MaterialConditionProvider,
) -> Result<DerivedStraightnessPayload, DerivedStraightnessError> {
    if !tol.stated_si.is_finite() || tol.stated_si < 0.0 {
        return Err(DerivedStraightnessError::MaterialConditionFailed {
            reason: "invalid_stated_tolerance".to_owned(),
        });
    }
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(DerivedStraightnessError::Form(FormError::NanInput));
        }
    }

    // REQ-001 / REQ-004: obtain the derived median line through derive() with the chain's
    // reduction operator; pass derivation failures through with no value.
    let (red_alg, red_params) = find_median_line_reduction(chain).ok_or(DerivedStraightnessError::ReductionMissing)?;
    let DerivedElement::MedianLine { points: median_pts, .. } = deriver
        .derive(points, red_alg, red_params)
        .map_err(|reason| DerivedStraightnessError::DerivationFailed { reason })?;

    for p in &median_pts {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(DerivedStraightnessError::Form(FormError::NanInput));
        }
    }

    // Resolve association criterion & minimum point count from the chain.
    let (requested, required_min) =
        resolve_association_and_min(FormElement::Line3d, chain).map_err(DerivedStraightnessError::Form)?;
    if median_pts.len() < required_min {
        return Err(DerivedStraightnessError::Form(FormError::TooFewPoints {
            required: required_min,
            given: median_pts.len(),
        }));
    }

    // REQ-001 / REQ-002: associate 3D line via MZ or LS, falling back to LS with
    // `ConservativeUpperBound` while KRN reports MZ unavailable for 3D lines.
    let mut flags = BTreeSet::new();
    let (association, outcome) = match requested {
        FormAssociation::MinimumZone => {
            if krn.mz_available(FormElement::Line3d) {
                (
                    FormAssociation::MinimumZone,
                    krn.fit_minimum_zone(FormElement::Line3d, &median_pts).map_err(DerivedStraightnessError::Form)?,
                )
            } else {
                flags.insert(FormFlag::ConservativeUpperBound);
                (
                    FormAssociation::LeastSquares,
                    krn.fit_least_squares(FormElement::Line3d, &median_pts).map_err(DerivedStraightnessError::Form)?,
                )
            }
        }
        FormAssociation::LeastSquares => (
            FormAssociation::LeastSquares,
            krn.fit_least_squares(FormElement::Line3d, &median_pts).map_err(DerivedStraightnessError::Form)?,
        ),
    };

    if outcome.local_optimum_only {
        flags.insert(FormFlag::LocalOptimumOnly);
    }

    let reference_line = outcome
        .reference
        .canonical()
        .map_err(|e| DerivedStraightnessError::Form(FormError::FitRefused { code: e.code() }))?;
    let FittedFeature::Line3 { p: line_p, d: line_d } = reference_line else {
        return Err(DerivedStraightnessError::Form(FormError::FitRefused { code: "wrong_reference_kind" }));
    };

    // REQ-001: compute radial distance of every median-line point to the associated 3D line in
    // index order, find total-order maximum (lowest index on tie), and double for zone diameter.
    let mut max_dist = -1.0_f64;
    let mut extreme_index = 0_usize;
    for (idx, q) in median_pts.iter().enumerate() {
        let dist = point_to_line3_dist(*q, line_p, line_d)?;
        if idx == 0 || dist.total_cmp(&max_dist) == Ordering::Greater {
            max_dist = dist;
            extreme_index = idx;
        }
    }
    let recomputed_diameter = canonical_zero(2.0 * max_dist);
    if !recomputed_diameter.is_finite() || recomputed_diameter < 0.0 {
        return Err(DerivedStraightnessError::Form(FormError::NanInput));
    }

    // Recompute from active set if MZ returned an active_zone_width consistent with distances,
    // otherwise use exact 2 * max_dist from the canonical reference line.
    let value_si = match (association, outcome.active_zone_width) {
        (FormAssociation::MinimumZone, Some(w)) if w.is_finite() && w >= 0.0 => {
            if (w - recomputed_diameter).abs() <= 1.0e-12 {
                canonical_zero(w)
            } else {
                recomputed_diameter
            }
        }
        _ => recomputed_diameter,
    };

    // REQ-003: with MMC/LMC, call material-condition engine and report stated and bonus separately.
    let (stated_si, bonus_si) = match tol.modifier {
        MaterialModifier::Rfs => (canonical_zero(tol.stated_si), None),
        MaterialModifier::Mmc | MaterialModifier::Lmc => {
            let allow = matl
                .compute_allowance(tol)
                .map_err(|reason| DerivedStraightnessError::MaterialConditionFailed { reason })?;
            if !allow.stated_si.is_finite()
                || allow.stated_si < 0.0
                || !allow.bonus_si.is_finite()
                || allow.bonus_si < 0.0
            {
                return Err(DerivedStraightnessError::MaterialConditionFailed {
                    reason: "invalid_allowance".to_owned(),
                });
            }
            (canonical_zero(allow.stated_si), Some(canonical_zero(allow.bonus_si)))
        }
    };

    Ok(DerivedStraightnessPayload {
        value_si,
        reference_line,
        extreme_index,
        stated_si,
        bonus_si,
        association,
        algorithm_id: outcome.algorithm_id,
        flags,
        point_count: median_pts.len(),
    })
}

// ------------------------------------------------------------------ KNP-1 exact MZ 3D line fitter

/// KNP-1 [`FitProvider`] supplying exact minimum-zone 3D line fitting (smallest enclosing
/// cylinder around 3D median-line points, `fit.line3d.mz@1.0.0`) plus least-squares fallback
/// via `fid_fit::ls_lin::Line3Ls`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MzLine3dFitProvider;

impl FitProvider for MzLine3dFitProvider {
    fn mz_available(&self, element: FormElement) -> bool {
        matches!(element, FormElement::Line3d)
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        if element != FormElement::Line3d {
            return Err(FormError::FitRefused { code: "mz_unavailable" });
        }
        fit_line3d_mz(points)
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        if element != FormElement::Line3d {
            return Err(FormError::FitRefused { code: "unsupported_element" });
        }
        fit_line3d_ls(points)
    }
}

fn fit_line3d_ls(points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
    let mut flat = Vec::with_capacity(points.len() * 3);
    for p in points {
        flat.push(p[0]);
        flat.push(p[1]);
        flat.push(p[2]);
    }
    let view = PointsView::new(&flat, None).map_err(|e| FormError::FitRefused { code: e.code() })?;
    let res = ls_lin::Line3Ls
        .fit(&view, &FitParams::new(Method::LeastSquares), None)
        .map_err(|e| FormError::FitRefused { code: e.code() })?;
    let FittedFeature::Line3 { p, d } = res.feature else {
        return Err(FormError::FitRefused { code: "wrong_reference_kind" });
    };
    let mut residuals = Vec::with_capacity(points.len());
    for q in points {
        residuals.push(point_to_line3_dist(*q, p, d).map_err(|_| FormError::NanInput)?);
    }
    Ok(FitOutcome {
        reference: res.feature,
        signed_residuals: residuals,
        active_zone_width: None,
        algorithm_id: res.algorithm.to_string(),
        local_optimum_only: false,
    })
}

/// Deterministic KNP-1 minimum-zone 3D line fit (smallest enclosing cylinder of a 3D point set).
///
/// For any unit axis direction `d`, the exact smallest enclosing cylinder along `d` has
/// cross-section equal to the exact 2D minimum enclosing circle (MEC) of the points projected
/// orthogonal to `d`. We evaluate starting directions from the LS 3D line axis and the
/// endpoint chord `q_{n-1} - q_0`, then refine `(alpha, beta)` tangent perturbations on `S^2`
/// via deterministic multi-scale pattern search until `step < 1e-13`.
pub fn fit_line3d_mz(points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
    if points.len() < 3 {
        return Err(FormError::TooFewPoints { required: 3, given: points.len() });
    }
    let ls = fit_line3d_ls(points)?;
    let FittedFeature::Line3 { d: ls_dir, .. } = ls.reference else {
        return Err(FormError::FitRefused { code: "wrong_reference_kind" });
    };

    // Candidate starting directions: LS 3D line + chord between first and last point.
    let mut starts: Vec<[f64; 3]> = vec![ls_dir];
    let chord = sub3(points[points.len() - 1], points[0]);
    if let Ok(chord_u) = normalize3(chord) {
        starts.push(chord_u);
    }

    let mut best_r = f64::INFINITY;
    let mut best_d = ls_dir;
    let mut best_p = [0.0_f64; 3];

    for start_d in starts {
        let (d_opt, p_opt, r_opt) = refine_direction_mec(points, start_d)?;
        if r_opt.total_cmp(&best_r) == Ordering::Less {
            best_r = r_opt;
            best_d = d_opt;
            best_p = p_opt;
        }
    }

    let raw_feature = FittedFeature::Line3 { p: best_p, d: best_d };
    let feature = raw_feature.canonical().map_err(|e| FormError::FitRefused { code: e.code() })?;
    let FittedFeature::Line3 { p: cp, d: cd } = feature else {
        return Err(FormError::FitRefused { code: "wrong_reference_kind" });
    };

    let mut residuals = Vec::with_capacity(points.len());
    let mut max_r = 0.0_f64;
    for (idx, q) in points.iter().enumerate() {
        let r = point_to_line3_dist(*q, cp, cd).map_err(|_| FormError::NanInput)?;
        if idx == 0 || r.total_cmp(&max_r) == Ordering::Greater {
            max_r = r;
        }
        residuals.push(r);
    }
    let width = canonical_zero(2.0 * max_r);

    Ok(FitOutcome {
        reference: feature,
        signed_residuals: residuals,
        active_zone_width: Some(width),
        algorithm_id: MZ_LINE3D_ALGORITHM_ID.to_owned(),
        local_optimum_only: false,
    })
}

fn refine_direction_mec(points: &[[f64; 3]], init_d: [f64; 3]) -> Result<([f64; 3], [f64; 3], f64), FormError> {
    let mut d = normalize3(init_d).map_err(|_| FormError::FitRefused { code: "degenerate" })?;
    let (mut p, mut r) = eval_direction_mec(points, d)?;

    let mut step = 0.1_f64;
    let diag = core::f64::consts::FRAC_1_SQRT_2;
    let dirs: [(f64, f64); 8] =
        [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (diag, diag), (-diag, diag), (diag, -diag), (-diag, -diag)];

    for _ in 0..80 {
        let (e1, e2) = ortho_basis(d)?;
        let mut improved = false;
        let mut next_d = d;
        let mut next_p = p;
        let mut next_r = r;
        for &(da, db) in &dirs {
            let cand_raw = [
                d[0] + step * (da * e1[0] + db * e2[0]),
                d[1] + step * (da * e1[1] + db * e2[1]),
                d[2] + step * (da * e1[2] + db * e2[2]),
            ];
            let Ok(cand_d) = normalize3(cand_raw) else { continue };
            let (cand_p, cand_r) = eval_direction_mec(points, cand_d)?;
            if cand_r.total_cmp(&next_r) == Ordering::Less && (next_r - cand_r) > 1.0e-17 {
                next_d = cand_d;
                next_p = cand_p;
                next_r = cand_r;
                improved = true;
            }
        }
        if improved {
            d = next_d;
            p = next_p;
            r = next_r;
        } else {
            step *= 0.25;
            if step < 1.0e-13 {
                break;
            }
        }
    }
    Ok((d, p, r))
}

fn eval_direction_mec(points: &[[f64; 3]], d: [f64; 3]) -> Result<([f64; 3], f64), FormError> {
    let (e1, e2) = ortho_basis(d)?;
    let mut proj: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for q in points {
        proj.push([dot3(*q, e1), dot3(*q, e2)]);
    }
    let (c2, r) = exact_mec_2d(&proj)?;
    let p3 = [c2[0] * e1[0] + c2[1] * e2[0], c2[0] * e1[1] + c2[1] * e2[1], c2[0] * e1[2] + c2[1] * e2[2]];
    Ok((p3, r))
}

/// Deterministic KNP-1 exact 2D minimum enclosing circle (incremental active-set over the
/// 2D convex hull, zero randomness, exact to machine precision).
fn exact_mec_2d(pts: &[[f64; 2]]) -> Result<([f64; 2], f64), FormError> {
    let hull = convex_hull_2d(pts)?;
    let n = hull.len();
    if n == 0 {
        return Err(FormError::TooFewPoints { required: 1, given: 0 });
    }
    if n == 1 {
        return Ok((hull[0], 0.0));
    }

    let mut center = hull[0];
    let mut r2 = 0.0_f64;
    let eps = 1.0e-15;

    for i in 1..n {
        if dist2_sq(hull[i], center) > r2 * (1.0 + eps) + 1.0e-30 {
            center = hull[i];
            r2 = 0.0;
            for j in 0..i {
                if dist2_sq(hull[j], center) > r2 * (1.0 + eps) + 1.0e-30 {
                    center = [0.5 * (hull[i][0] + hull[j][0]), 0.5 * (hull[i][1] + hull[j][1])];
                    r2 = dist2_sq(hull[i], center);
                    for k in 0..j {
                        if dist2_sq(hull[k], center) > r2 * (1.0 + eps) + 1.0e-30 {
                            if let Some((cc, cr2)) = circumcircle_2d(hull[i], hull[j], hull[k]) {
                                center = cc;
                                r2 = cr2;
                            }
                        }
                    }
                }
            }
        }
    }

    // Recompute exact radius over all input points in index order.
    let mut max_r2 = 0.0_f64;
    for p in pts {
        let d2 = dist2_sq(*p, center);
        if d2.total_cmp(&max_r2) == Ordering::Greater {
            max_r2 = d2;
        }
    }
    let r = numf::sqrt(max_r2).map_err(|_| FormError::NanInput)?;
    Ok((center, r))
}

fn convex_hull_2d(pts: &[[f64; 2]]) -> Result<Vec<[f64; 2]>, FormError> {
    let mut sorted = pts.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])));
    sorted.dedup_by(|a, b| a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits());
    if sorted.len() <= 2 {
        return Ok(sorted);
    }
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for p in &sorted {
        while lower.len() >= 2 && cross2(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for p in sorted.iter().rev() {
        while upper.len() >= 2 && cross2(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    Ok(lower)
}

fn cross2(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn dist2_sq(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

fn circumcircle_2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let ax = b[0] - a[0];
    let ay = b[1] - a[1];
    let bx = c[0] - a[0];
    let by = c[1] - a[1];
    let det = 2.0 * (ax * by - ay * bx);
    if det.abs() <= 1.0e-30 {
        return None;
    }
    let a2 = ax * ax + ay * ay;
    let b2 = bx * bx + by * by;
    let ux = (a2 * by - b2 * ay) / det;
    let uy = (ax * b2 - bx * a2) / det;
    let center = [a[0] + ux, a[1] + uy];
    let r2 = ux * ux + uy * uy;
    if !center[0].is_finite() || !center[1].is_finite() || !r2.is_finite() {
        return None;
    }
    Some((center, r2))
}

// ------------------------------------------------------------------ 3D vector helpers (KNP-1)

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize3(v: [f64; 3]) -> Result<[f64; 3], FormError> {
    let n2 = dot3(v, v);
    if !n2.is_finite() || n2 <= 1.0e-30 {
        return Err(FormError::FitRefused { code: "degenerate" });
    }
    let n = numf::sqrt(n2).map_err(|_| FormError::NanInput)?;
    Ok([v[0] / n, v[1] / n, v[2] / n])
}

fn ortho_basis(d: [f64; 3]) -> Result<([f64; 3], [f64; 3]), FormError> {
    let seed = if d[0].abs().total_cmp(&0.9) == Ordering::Less { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let e1 = normalize3(cross3(d, seed))?;
    let e2 = normalize3(cross3(d, e1))?;
    Ok((e1, e2))
}

fn point_to_line3_dist(q: [f64; 3], p: [f64; 3], d: [f64; 3]) -> Result<f64, DerivedStraightnessError> {
    let v = sub3(q, p);
    let proj = dot3(v, d);
    let perp = [v[0] - proj * d[0], v[1] - proj * d[1], v[2] - proj * d[2]];
    let r2 = dot3(perp, perp);
    if !r2.is_finite() {
        return Err(DerivedStraightnessError::Form(FormError::NanInput));
    }
    let clamped = if r2 < 0.0 { 0.0 } else { r2 };
    numf::sqrt(clamped).map_err(|_| DerivedStraightnessError::Form(FormError::NanInput))
}

fn canonical_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

fn line3_cvalue(f: &FittedFeature) -> CValue {
    let mut m = BTreeMap::new();
    if let FittedFeature::Line3 { p, d } = f {
        m.insert("d".to_owned(), CValue::Arr(d.iter().map(|&v| CValue::Num(canonical_zero(v))).collect()));
        m.insert("kind".to_owned(), CValue::Str("line3".to_owned()));
        m.insert("p".to_owned(), CValue::Arr(p.iter().map(|&v| CValue::Num(canonical_zero(v))).collect()));
    }
    CValue::Obj(m)
}
