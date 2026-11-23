//! Line-element extraction and line-element straightness (`GDT.FORM.STRAIGHT.LINEEL`,
//! `spec/gdt/form/straight/lineel/NODE.md`, `REQ-001`..`REQ-005`, `AC-1`..`AC-5`).
//!
//! Pure KNP-1 kernel math: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! `fid_num::f` for square roots, exact combinatorial convex-hull minimum-width parallel strip
//! for 2D minimum-zone straightness, `f64::total_cmp` with lowest-index tie breaking, NaN rejected.

use crate::dmt::{OperatorChain, OperatorKind, Param};
use crate::form::{
    evaluate_form, signed_residuals_of, FitOutcome, FitProvider, FormElement, FormError, FormFlag, FormPayload,
    KrnFitProvider,
};
use core::cmp::Ordering;
use core::fmt;
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_fit::contract::{normalize_direction, FittedFeature};
use fid_num::f as numf;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Algorithm ID for the exact combinatorial convex-hull minimum-zone 2D line fit.
pub const ALG_LINE2D_MZ: &str = "fit.line2d.mz@1.0.0";
/// Maximum line elements allowed in one extraction request.
pub const MAX_LINE_ELEMENTS: usize = 4096;
/// Tolerance for unit-direction validation (`non_unit_direction`).
const UNIT_TOL: f64 = 1e-9;

/// Selection of integer line-element indices `k` (`origin + k * spacing * plane_normal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementRange {
    /// `0..count` (`k = 0, 1, ..., count - 1`).
    Count(usize),
    /// Inclusive integer range `start..=end_inclusive`.
    Range { start: i64, end_inclusive: i64 },
}

impl ElementRange {
    /// Returns the ordered integer element indices `k`.
    pub fn indices(self) -> Result<Vec<i64>, LineElementError> {
        match self {
            ElementRange::Count(c) => {
                if c == 0 || c > MAX_LINE_ELEMENTS {
                    return Err(LineElementError::InvalidParam { param: "count_or_range" });
                }
                Ok((0..c).map(|i| i as i64).collect())
            }
            ElementRange::Range { start, end_inclusive } => {
                if end_inclusive < start {
                    return Err(LineElementError::InvalidParam { param: "count_or_range" });
                }
                let span = end_inclusive.checked_sub(start).and_then(|d| d.checked_add(1));
                let Some(len) = span else {
                    return Err(LineElementError::InvalidParam { param: "count_or_range" });
                };
                if len <= 0 || (len as usize) > MAX_LINE_ELEMENTS {
                    return Err(LineElementError::InvalidParam { param: "count_or_range" });
                }
                Ok((start..=end_inclusive).collect())
            }
        }
    }
}

/// Explicit extraction parameters supplied by the resolved DMT chain (`REQ-001`, `REQ-004`).
/// Never supplies defaults in code; any missing parameter fails `extraction_params_missing{param}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineElementParams {
    pub plane_normal: [f64; 3],
    pub origin: [f64; 3],
    pub spacing: f64,
    pub count_or_range: ElementRange,
    pub band_half_width: f64,
    pub line_direction: [f64; 3],
}

impl LineElementParams {
    /// Reads all six extraction parameters strictly from `chain`'s `Extraction`/`Partition` steps.
    /// Fails with [`LineElementError::ExtractionParamsMissing`] naming the first missing parameter
    /// in canonical order (`REQ-004`, `AC-3`).
    pub fn from_chain(chain: &OperatorChain) -> Result<Self, LineElementError> {
        let mut params: BTreeMap<&str, &Param> = BTreeMap::new();
        for step in chain.steps() {
            if matches!(step.kind(), OperatorKind::Extraction | OperatorKind::Partition) {
                for (k, v) in step.params() {
                    params.insert(k.as_str(), v);
                }
            }
        }

        let read_vec3 = |prefix: &'static str, kx: &str, ky: &str, kz: &str| -> Result<[f64; 3], LineElementError> {
            let get = |k: &str| -> Result<f64, LineElementError> {
                let p = params.get(k).ok_or(LineElementError::ExtractionParamsMissing { param: prefix })?;
                let v = p.as_f64().ok_or(LineElementError::InvalidParam { param: prefix })?;
                if !v.is_finite() {
                    return Err(LineElementError::NanInput);
                }
                Ok(v)
            };
            Ok([get(kx)?, get(ky)?, get(kz)?])
        };

        let read_f64 = |key: &'static str| -> Result<f64, LineElementError> {
            let p = params.get(key).ok_or(LineElementError::ExtractionParamsMissing { param: key })?;
            let v = p.as_f64().ok_or(LineElementError::InvalidParam { param: key })?;
            if !v.is_finite() {
                return Err(LineElementError::NanInput);
            }
            Ok(v)
        };

        let plane_normal = read_vec3("plane_normal", "plane_normal_x", "plane_normal_y", "plane_normal_z")?;
        let origin = read_vec3("origin", "origin_x", "origin_y", "origin_z")?;
        let spacing = read_f64("spacing")?;

        let count_or_range = if let Some(Param::Int(c)) = params.get("count") {
            if *c <= 0 {
                return Err(LineElementError::InvalidParam { param: "count_or_range" });
            }
            ElementRange::Count(*c as usize)
        } else if let (Some(Param::Int(s)), Some(Param::Int(e))) = (params.get("range_start"), params.get("range_end"))
        {
            ElementRange::Range { start: *s, end_inclusive: *e }
        } else {
            return Err(LineElementError::ExtractionParamsMissing { param: "count_or_range" });
        };

        let band_half_width = read_f64("band_half_width")?;
        let line_direction = read_vec3("line_direction", "line_direction_x", "line_direction_y", "line_direction_z")?;

        let out = Self { plane_normal, origin, spacing, count_or_range, band_half_width, line_direction };
        out.validate()?;
        Ok(out)
    }

    /// Validates finiteness, positive spacing/band, unit directions and non-collinearity.
    pub fn validate(&self) -> Result<(), LineElementError> {
        for v in self.plane_normal.iter().chain(self.origin.iter()).chain(self.line_direction.iter()) {
            if !v.is_finite() {
                return Err(LineElementError::NanInput);
            }
        }
        if !self.spacing.is_finite() || !self.band_half_width.is_finite() {
            return Err(LineElementError::NanInput);
        }
        if self.spacing <= 0.0 {
            return Err(LineElementError::InvalidParam { param: "spacing" });
        }
        if self.band_half_width <= 0.0 {
            return Err(LineElementError::InvalidParam { param: "band_half_width" });
        }
        let nn = norm3(self.plane_normal)?;
        if (nn - 1.0).abs() > UNIT_TOL {
            return Err(LineElementError::NonUnitDirection { param: "plane_normal" });
        }
        let nd = norm3(self.line_direction)?;
        if (nd - 1.0).abs() > UNIT_TOL {
            return Err(LineElementError::NonUnitDirection { param: "line_direction" });
        }
        // Line direction cannot be parallel to cutting-plane normal.
        let proj = sub3(self.line_direction, scale3(self.plane_normal, dot3(self.line_direction, self.plane_normal)));
        if norm3(proj)? <= 1e-9 {
            return Err(LineElementError::InvalidParam { param: "line_direction" });
        }
        let _ = self.count_or_range.indices()?;
        Ok(())
    }
}

/// One extracted planar line element (`GDT.FORM.STRAIGHT.LINEEL` Scope).
#[derive(Debug, Clone, PartialEq)]
pub struct LineElement {
    /// Integer element index `k`.
    pub index: i64,
    /// Point on cutting plane `k`: `origin + k * spacing * plane_normal`.
    pub plane_origin: [f64; 3],
    /// Unit normal of cutting plane `k`.
    pub plane_normal: [f64; 3],
    /// Original indices of points assigned to element `k`, in ascending input order.
    pub point_indices: Vec<usize>,
    /// Projected 2D coordinates `(u along line_direction, v perpendicular in plane)` in SI metres.
    pub points_2d: Vec<[f64; 2]>,
}

/// One successfully evaluated line element (`REQ-002`).
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedLineElement {
    pub index: i64,
    pub value_si: f64,
    pub flags: BTreeSet<FormFlag>,
    pub payload: FormPayload,
}

/// One line element skipped during evaluation (`REQ-003`, `AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLineElement {
    pub index: i64,
    pub reason: &'static str,
    pub required: usize,
    pub given: usize,
}

/// Complete line-element straightness payload (`GDT.FORM.STRAIGHT.LINEEL` Design).
#[derive(Debug, Clone, PartialEq)]
pub struct LineElementPayload {
    pub max_value_si: f64,
    pub attaining_element: i64,
    pub per_element: Vec<EvaluatedLineElement>,
    pub skipped_elements: Vec<SkippedLineElement>,
}

impl LineElementPayload {
    /// Canonical JSON representation for D0 cross-platform digest checks (`AC-5`).
    pub fn to_cvalue(&self) -> Result<CValue, FormError> {
        let hex = |x: f64| CValue::Str(format!("0x{:016x}", x.to_bits()));
        let mut per_arr = Vec::with_capacity(self.per_element.len());
        for el in &self.per_element {
            let mut em = BTreeMap::new();
            em.insert(
                "flags".to_owned(),
                CValue::Arr(el.flags.iter().map(|fl| CValue::Str(fl.as_str().to_owned())).collect()),
            );
            em.insert("form".to_owned(), el.payload.to_cvalue()?);
            em.insert("index".to_owned(), CValue::Str(el.index.to_string()));
            em.insert("value_si_hex".to_owned(), hex(el.value_si));
            per_arr.push(CValue::Obj(em));
        }
        let mut skip_arr = Vec::with_capacity(self.skipped_elements.len());
        for sk in &self.skipped_elements {
            let mut sm = BTreeMap::new();
            sm.insert("given".to_owned(), CValue::Str(sk.given.to_string()));
            sm.insert("index".to_owned(), CValue::Str(sk.index.to_string()));
            sm.insert("reason".to_owned(), CValue::Str(sk.reason.to_owned()));
            sm.insert("required".to_owned(), CValue::Str(sk.required.to_string()));
            skip_arr.push(CValue::Obj(sm));
        }
        let mut m = BTreeMap::new();
        m.insert("attaining_element".to_owned(), CValue::Str(self.attaining_element.to_string()));
        m.insert("max_value_si_hex".to_owned(), hex(self.max_value_si));
        m.insert("per_element".to_owned(), CValue::Arr(per_arr));
        m.insert("skipped_elements".to_owned(), CValue::Arr(skip_arr));
        Ok(CValue::Obj(m))
    }

    /// SHA-256 content digest of the canonical payload bytes (`AC-5`).
    pub fn digest(&self) -> Result<Digest, FormError> {
        let v = self.to_cvalue()?;
        let bytes = to_canonical_bytes(&v).map_err(|_| FormError::FitRefused { code: "canonical_encode" })?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

/// Errors returned by line-element extraction and evaluation (`GDT.FORM.STRAIGHT.LINEEL` Design).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineElementError {
    ExtractionParamsMissing { param: &'static str },
    InvalidParam { param: &'static str },
    NonUnitDirection { param: &'static str },
    NoLineElements,
    NanInput,
    Form { element: i64, source: FormError },
}

impl LineElementError {
    pub const fn code(&self) -> &'static str {
        match self {
            LineElementError::ExtractionParamsMissing { .. } => "extraction_params_missing",
            LineElementError::InvalidParam { .. } => "invalid_param",
            LineElementError::NonUnitDirection { .. } => "non_unit_direction",
            LineElementError::NoLineElements => "no_line_elements",
            LineElementError::NanInput => "nan_input",
            LineElementError::Form { source, .. } => source.code(),
        }
    }
}

impl fmt::Display for LineElementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LineElementError::ExtractionParamsMissing { param } => {
                write!(f, "extraction_params_missing{{param: {param}}}")
            }
            LineElementError::InvalidParam { param } => write!(f, "invalid_param{{param: {param}}}"),
            LineElementError::NonUnitDirection { param } => write!(f, "non_unit_direction{{param: {param}}}"),
            LineElementError::NoLineElements => f.write_str("no_line_elements"),
            LineElementError::NanInput => f.write_str("nan_input"),
            LineElementError::Form { element, source } => write!(f, "element {element}: {source}"),
        }
    }
}

impl std::error::Error for LineElementError {}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm3(v: [f64; 3]) -> Result<f64, LineElementError> {
    numf::sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).map_err(|_| LineElementError::NanInput)
}

/// Extracts planar line elements from 3D `points` under explicit `params` (`REQ-001`).
pub fn extract_line_elements(
    points: &[[f64; 3]],
    params: &LineElementParams,
) -> Result<Vec<LineElement>, LineElementError> {
    params.validate()?;
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(LineElementError::NanInput);
        }
    }

    // Orthonormal in-plane 2D frame `(u_dir, v_dir)` in fixed order:
    // u_dir = normalize(line_direction - (line_direction . n) * n), v_dir = normalize(n x u_dir).
    let n = params.plane_normal;
    let u_raw = sub3(params.line_direction, scale3(n, dot3(params.line_direction, n)));
    let nu = norm3(u_raw)?;
    if !nu.is_finite() || nu <= 0.0 {
        return Err(LineElementError::InvalidParam { param: "line_direction" });
    }
    let u_dir = [u_raw[0] / nu, u_raw[1] / nu, u_raw[2] / nu];
    let v_raw = cross3(n, u_dir);
    let nv = norm3(v_raw)?;
    if !nv.is_finite() || nv <= 0.0 {
        return Err(LineElementError::InvalidParam { param: "line_direction" });
    }
    let v_dir = [v_raw[0] / nv, v_raw[1] / nv, v_raw[2] / nv];

    let indices = params.count_or_range.indices()?;
    let mut elements = Vec::with_capacity(indices.len());
    for k in indices {
        let offset = (k as f64) * params.spacing;
        let p_k =
            [params.origin[0] + offset * n[0], params.origin[1] + offset * n[1], params.origin[2] + offset * n[2]];
        let mut point_indices = Vec::new();
        let mut points_2d = Vec::new();
        for (i, &q) in points.iter().enumerate() {
            let rel = sub3(q, p_k);
            let dist = dot3(rel, n);
            if dist.abs().total_cmp(&params.band_half_width) != Ordering::Greater {
                let proj = sub3(rel, scale3(n, dist));
                point_indices.push(i);
                points_2d.push([dot3(proj, u_dir), dot3(proj, v_dir)]);
            }
        }
        elements.push(LineElement { index: k, plane_origin: p_k, plane_normal: n, point_indices, points_2d });
    }
    Ok(elements)
}

/// Evaluates line-element straightness across extracted cutting planes via `GDT.FORM.FCORE`
/// (`FormElement::Line2d`) (`REQ-001`..`REQ-005`).
pub fn evaluate_line_elements(
    points: &[[f64; 3]],
    params: &LineElementParams,
    chain: &OperatorChain,
    krn: &dyn FitProvider,
) -> Result<LineElementPayload, LineElementError> {
    let extracted = extract_line_elements(points, params)?;
    let mut per_element: Vec<EvaluatedLineElement> = Vec::new();
    let mut skipped_elements: Vec<SkippedLineElement> = Vec::new();

    for el in extracted {
        let pts3: Vec<[f64; 3]> = el.points_2d.iter().map(|p| [p[0], p[1], 0.0]).collect();
        match evaluate_form(FormElement::Line2d, &pts3, chain, krn) {
            Ok(payload) => {
                per_element.push(EvaluatedLineElement {
                    index: el.index,
                    value_si: payload.value_si,
                    flags: payload.flags.clone(),
                    payload,
                });
            }
            Err(FormError::TooFewPoints { required, given }) => {
                skipped_elements.push(SkippedLineElement {
                    index: el.index,
                    reason: "too_few_points",
                    required,
                    given,
                });
            }
            Err(other) => {
                return Err(LineElementError::Form { element: el.index, source: other });
            }
        }
    }

    if per_element.is_empty() {
        return Err(LineElementError::NoLineElements);
    }

    let mut max_val = per_element[0].value_si;
    let mut attaining = per_element[0].index;
    for el in per_element.iter().skip(1) {
        if el.value_si.total_cmp(&max_val) == Ordering::Greater {
            max_val = el.value_si;
            attaining = el.index;
        }
    }

    Ok(LineElementPayload { max_value_si: max_val, attaining_element: attaining, per_element, skipped_elements })
}

// -------------------------------------------------- exact KNP-1 combinatorial MZ 2D line fitter

/// Default [`FitProvider`] supplying exact combinatorial minimum-zone 2D line fitting
/// (`fit.line2d.mz@1.0.0`, convex-hull minimum parallel strip) for `FormElement::Line2d`,
/// delegating all other elements and least-squares fits to [`KrnFitProvider`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Line2dKrnProvider;

impl FitProvider for Line2dKrnProvider {
    fn mz_available(&self, element: FormElement) -> bool {
        element == FormElement::Line2d || KrnFitProvider.mz_available(element)
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        if element == FormElement::Line2d {
            return mz_line2d_fit(points);
        }
        KrnFitProvider.fit_minimum_zone(element, points)
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        KrnFitProvider.fit_least_squares(element, points)
    }
}

fn cross2(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Andrew's monotone chain 2D convex hull (CCW, total-order deterministic sort, zero panics).
fn convex_hull_2d(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut pts: Vec<[f64; 2]> = points.to_vec();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])));
    pts.dedup_by(|a, b| a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits());
    if pts.len() <= 2 {
        return pts;
    }
    let mut lower: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
    for &p in &pts {
        while lower.len() >= 2 && cross2(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross2(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Exact combinatorial KNP-1 minimum-zone 2D line fit via minimum-width convex-hull parallel strip
/// (Houle & Toussaint 1988). Guaranteed global optimum with zero iterations.
pub fn mz_line2d_fit(points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
    if points.len() < 3 {
        return Err(FormError::TooFewPoints { required: 3, given: points.len() });
    }
    let mut xy = Vec::with_capacity(points.len());
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() {
            return Err(FormError::NanInput);
        }
        xy.push([p[0], p[1]]);
    }

    let hull = convex_hull_2d(&xy);
    if hull.len() < 2 {
        return Err(FormError::FitRefused { code: "collinear_or_coincident" });
    }

    // Collinear case (hull has 2 endpoints): exact width 0 along the segment line.
    if hull.len() == 2 {
        let dx = hull[1][0] - hull[0][0];
        let dy = hull[1][1] - hull[0][1];
        let len = numf::sqrt(dx * dx + dy * dy).map_err(|_| FormError::NanInput)?;
        if !len.is_finite() || len <= 0.0 {
            return Err(FormError::FitRefused { code: "coincident_points" });
        }
        let d3 =
            normalize_direction([dx / len, dy / len, 0.0]).map_err(|e| FormError::FitRefused { code: e.code() })?;
        let d = [d3[0], d3[1]];
        let p = [0.5 * (hull[0][0] + hull[1][0]), 0.5 * (hull[0][1] + hull[1][1])];
        let feature = FittedFeature::Line2 { p, d };
        let residuals = signed_residuals_of(&feature, points)?;
        return Ok(FitOutcome {
            reference: feature,
            signed_residuals: residuals,
            active_zone_width: Some(0.0),
            algorithm_id: ALG_LINE2D_MZ.to_owned(),
            local_optimum_only: false,
        });
    }

    // General case: every narrowest strip is parallel to some hull edge `(h[i], h[(i+1)%m])`.
    let m = hull.len();
    let mut best_w = f64::INFINITY;
    let mut best_d = [1.0, 0.0];
    let mut best_mid = [0.0, 0.0];

    for i in 0..m {
        let a = hull[i];
        let b = hull[(i + 1) % m];
        let ex = b[0] - a[0];
        let ey = b[1] - a[1];
        let elen = numf::sqrt(ex * ex + ey * ey).map_err(|_| FormError::NanInput)?;
        if !elen.is_finite() || elen <= 0.0 {
            continue;
        }
        let d = [ex / elen, ey / elen];
        let n = [-d[1], d[0]];
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &hpt in &hull {
            let s = (hpt[0] - a[0]) * n[0] + (hpt[1] - a[1]) * n[1];
            if s.total_cmp(&lo) == Ordering::Less {
                lo = s;
            }
            if s.total_cmp(&hi) == Ordering::Greater {
                hi = s;
            }
        }
        let w = hi - lo;
        if w.total_cmp(&best_w) == Ordering::Less {
            best_w = w;
            best_d = d;
            let mid_shift = 0.5 * (lo + hi);
            let p_raw = [a[0] + mid_shift * n[0], a[1] + mid_shift * n[1]];
            // Foot of origin on the midline for canonical representation.
            let proj = p_raw[0] * d[0] + p_raw[1] * d[1];
            best_mid = [p_raw[0] - proj * d[0], p_raw[1] - proj * d[1]];
        }
    }

    if !best_w.is_finite() {
        return Err(FormError::FitRefused { code: "mz_line2d_failed" });
    }
    let d3 = normalize_direction([best_d[0], best_d[1], 0.0]).map_err(|e| FormError::FitRefused { code: e.code() })?;
    let d_canon = [d3[0], d3[1]];
    let feature = FittedFeature::Line2 { p: best_mid, d: d_canon };
    let residuals = signed_residuals_of(&feature, points)?;
    Ok(FitOutcome {
        reference: feature,
        signed_residuals: residuals,
        active_zone_width: Some(best_w),
        algorithm_id: ALG_LINE2D_MZ.to_owned(),
        local_optimum_only: false,
    })
}
