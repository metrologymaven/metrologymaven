//! Roundness (circularity) evaluation with cross-section extraction (`GDT.FORM.ROUND`,
//! `spec/gdt/form/round/NODE.md`, `REQ-001`..`REQ-005`, `AC-1`..`AC-4`).
//!
//! Pure KNP-1 kernel math: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! `fid_num::f` for square roots, `f64::total_cmp` with lowest-index tie breaking, NaN rejected.

use crate::dmt::{OperatorChain, OperatorKind, Param};
use crate::form::{evaluate_form, FitProvider, FormElement, FormError, FormPayload};
use core::cmp::Ordering;
use core::fmt;
use fid_canon::CValue;
use fid_fit::contract::{FitParams, FittedFeature, Method, PointsView};
use fid_fit::{ls_cone, ls_cyl};
use fid_num::f as numf;
use std::collections::BTreeMap;

/// Maximum number of cross-sections allowed in one roundness evaluation.
pub const MAX_SECTIONS: usize = 1024;

/// Reference axis fit method named by a DMT chain's section specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AxisFitKind {
    Cylinder,
    Cone,
}

impl AxisFitKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            AxisFitKind::Cylinder => "cylinder",
            AxisFitKind::Cone => "cone",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cylinder" | "ls_cyl" | "fit.cylinder.ls" | "fit_cylinder_ls" => Some(AxisFitKind::Cylinder),
            "cone" | "ls_cone" | "fit.cone.ls" | "fit_cone_ls" => Some(AxisFitKind::Cone),
            _ => None,
        }
    }
}

/// Cross-section extraction specification read from the resolved DMT chain (`REQ-003`).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionSpec {
    pub axis_fit: AxisFitKind,
    pub positions_m: Vec<f64>,
    pub band_half_width_m: f64,
}

impl SectionSpec {
    /// Extracts a [`SectionSpec`] from the `Extraction` or `Partition` step of `chain`.
    /// Returns [`RoundnessError::SectionSpecMissing`] if no section specification is present.
    pub fn from_chain(chain: &OperatorChain) -> Result<Self, RoundnessError> {
        let mut axis_fit: Option<AxisFitKind> = None;
        let mut band_half_width_m: Option<f64> = None;
        let mut indexed: Vec<(usize, f64)> = Vec::new();
        let mut start_m: Option<f64> = None;
        let mut step_m: Option<f64> = None;
        let mut count: Option<usize> = None;

        for step in chain.steps() {
            if !matches!(step.kind(), OperatorKind::Extraction | OperatorKind::Partition) {
                continue;
            }
            for (k, v) in step.params() {
                if k == "axis_fit" {
                    let Param::Token(tok) = v else {
                        return Err(RoundnessError::InvalidSectionSpec { reason: "bad_axis_fit" });
                    };
                    axis_fit = Some(
                        AxisFitKind::parse(tok)
                            .ok_or(RoundnessError::InvalidSectionSpec { reason: "unknown_axis_fit" })?,
                    );
                } else if k == "band_half_width_m" {
                    let val = v.as_f64().ok_or(RoundnessError::InvalidSectionSpec { reason: "bad_band_half_width" })?;
                    if !val.is_finite() || val <= 0.0 {
                        return Err(RoundnessError::InvalidSectionSpec { reason: "non_positive_band" });
                    }
                    band_half_width_m = Some(val);
                } else if k == "section_start_m" {
                    let val = v.as_f64().ok_or(RoundnessError::InvalidSectionSpec { reason: "bad_section_start" })?;
                    if !val.is_finite() {
                        return Err(RoundnessError::InvalidSectionSpec { reason: "non_finite_position" });
                    }
                    start_m = Some(val);
                } else if k == "section_step_m" {
                    let val = v.as_f64().ok_or(RoundnessError::InvalidSectionSpec { reason: "bad_section_step" })?;
                    if !val.is_finite() {
                        return Err(RoundnessError::InvalidSectionSpec { reason: "non_finite_position" });
                    }
                    step_m = Some(val);
                } else if k == "section_count" {
                    let Param::Int(n) = *v else {
                        return Err(RoundnessError::InvalidSectionSpec { reason: "bad_section_count" });
                    };
                    if n <= 0 || (n as usize) > MAX_SECTIONS {
                        return Err(RoundnessError::InvalidSectionSpec { reason: "section_count_out_of_range" });
                    }
                    count = Some(n as usize);
                } else if let Some(rest) = k.strip_prefix("section_") {
                    if let Some(num) = rest.strip_suffix("_m") {
                        let idx: usize = num
                            .parse()
                            .map_err(|_| RoundnessError::InvalidSectionSpec { reason: "bad_section_index" })?;
                        let val = v.as_f64().ok_or(RoundnessError::InvalidSectionSpec { reason: "bad_position" })?;
                        if !val.is_finite() {
                            return Err(RoundnessError::InvalidSectionSpec { reason: "non_finite_position" });
                        }
                        indexed.push((idx, val));
                    }
                }
            }
        }

        let Some(axis_fit) = axis_fit else {
            return Err(RoundnessError::SectionSpecMissing);
        };
        let Some(band_half_width_m) = band_half_width_m else {
            return Err(RoundnessError::SectionSpecMissing);
        };

        let positions_m = if !indexed.is_empty() {
            indexed.sort_by_key(|(idx, _)| *idx);
            if indexed.len() > MAX_SECTIONS {
                return Err(RoundnessError::InvalidSectionSpec { reason: "too_many_sections" });
            }
            indexed.into_iter().map(|(_, p)| p).collect()
        } else if let (Some(s0), Some(ds), Some(n)) = (start_m, step_m, count) {
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                out.push(s0 + (i as f64) * ds);
            }
            out
        } else {
            return Err(RoundnessError::SectionSpecMissing);
        };

        Ok(Self { axis_fit, positions_m, band_half_width_m })
    }
}

/// One explicit 2D circular cross-section (`GDT.FORM.ROUND` Design).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionPoints {
    /// Axial position of the section plane in SI metres.
    pub position_m: f64,
    /// Planar `[x, y]` coordinates in the section plane in SI metres.
    pub points2d: Vec<[f64; 2]>,
}

/// Reference axis recovered from a 3D point cloud (`REQ-003`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedAxis {
    pub origin: [f64; 3],
    pub direction: [f64; 3],
    pub algorithm_id: String,
    pub local_optimum_only: bool,
}

/// Summary of 3D cross-section extraction recorded on the result (`REQ-003`, `AC-2`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractionSummary {
    pub axis: ExtractedAxis,
    pub positions_m: Vec<f64>,
    pub band_half_width_m: f64,
}

impl ExtractionSummary {
    pub fn to_cvalue(&self) -> CValue {
        let hex = |x: f64| CValue::Str(format!("0x{:016x}", x.to_bits()));
        let v3 = |v: [f64; 3]| CValue::Arr(vec![hex(v[0]), hex(v[1]), hex(v[2])]);
        let mut axis_m = BTreeMap::new();
        axis_m.insert("algorithm_id".to_owned(), CValue::Str(self.axis.algorithm_id.clone()));
        axis_m.insert("direction".to_owned(), v3(self.axis.direction));
        axis_m.insert("local_optimum_only".to_owned(), CValue::Bool(self.axis.local_optimum_only));
        axis_m.insert("origin".to_owned(), v3(self.axis.origin));
        let mut m = BTreeMap::new();
        m.insert("axis".to_owned(), CValue::Obj(axis_m));
        m.insert("band_half_width_m_hex".to_owned(), hex(self.band_half_width_m));
        m.insert("positions_m_hex".to_owned(), CValue::Arr(self.positions_m.iter().copied().map(hex).collect()));
        CValue::Obj(m)
    }
}

/// Per-section roundness evaluation result (`REQ-001`, `REQ-002`).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionResult {
    pub section_index: usize,
    pub position_m: f64,
    pub payload: FormPayload,
}

impl SectionResult {
    pub fn to_cvalue(&self) -> Result<CValue, FormError> {
        let hex = |x: f64| CValue::Str(format!("0x{:016x}", x.to_bits()));
        let mut m = BTreeMap::new();
        m.insert("form".to_owned(), self.payload.to_cvalue()?);
        m.insert("position_m_hex".to_owned(), hex(self.position_m));
        m.insert("section_index".to_owned(), CValue::Str(self.section_index.to_string()));
        m.insert("value_si_hex".to_owned(), hex(self.payload.value_si));
        Ok(CValue::Obj(m))
    }
}

/// Complete kernel roundness result (`REQ-001`..`REQ-003`).
#[derive(Debug, Clone, PartialEq)]
pub struct RoundnessResult {
    /// Maximum section value in SI metres (`REQ-002`).
    pub value_si: f64,
    /// Lowest index of the section achieving `value_si` (`GDT.FORM.ROUND` Design).
    pub worst_section_index: usize,
    /// Per-section results in input/specification order.
    pub sections: Vec<SectionResult>,
    /// Populated when sections were extracted from a 3D cloud (`REQ-003`, `AC-2`).
    pub extracted: Option<ExtractionSummary>,
}

/// Errors returned by roundness evaluation (`REQ-004`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundnessError {
    TooFewPoints { section: usize, required: usize, given: usize },
    SectionSpecMissing,
    InvalidSectionSpec { reason: &'static str },
    NoSections,
    Form { section: Option<usize>, source: FormError },
}

impl RoundnessError {
    pub const fn code(&self) -> &'static str {
        match self {
            RoundnessError::TooFewPoints { .. } => "too_few_points",
            RoundnessError::SectionSpecMissing => "section_spec_missing",
            RoundnessError::InvalidSectionSpec { .. } => "invalid_section_spec",
            RoundnessError::NoSections => "no_sections",
            RoundnessError::Form { source, .. } => source.code(),
        }
    }
}

impl fmt::Display for RoundnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoundnessError::TooFewPoints { section, required, given } => {
                write!(f, "too_few_points{{section: {section}, required: {required}, given: {given}}}")
            }
            RoundnessError::SectionSpecMissing => f.write_str("section_spec_missing"),
            RoundnessError::InvalidSectionSpec { reason } => write!(f, "invalid_section_spec{{{reason}}}"),
            RoundnessError::NoSections => f.write_str("no_sections"),
            RoundnessError::Form { section: Some(s), source } => write!(f, "section {s}: {source}"),
            RoundnessError::Form { section: None, source } => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for RoundnessError {}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm3(v: [f64; 3]) -> Result<f64, RoundnessError> {
    numf::sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])
        .map_err(|_| RoundnessError::Form { section: None, source: FormError::NanInput })
}

/// Deterministic KNP-1 orthonormal basis `(e1, e2)` perpendicular to unit axis `d`.
fn section_basis(d: [f64; 3]) -> Result<([f64; 3], [f64; 3]), RoundnessError> {
    let ax = d[0].abs();
    let ay = d[1].abs();
    let az = d[2].abs();
    let seed = if ax.total_cmp(&ay) != Ordering::Greater && ax.total_cmp(&az) != Ordering::Greater {
        [1.0, 0.0, 0.0]
    } else if ay.total_cmp(&az) != Ordering::Greater {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let u = cross3(d, seed);
    let nu = norm3(u)?;
    if !nu.is_finite() || nu <= 0.0 {
        return Err(RoundnessError::Form { section: None, source: FormError::FitRefused { code: "degenerate_axis" } });
    }
    let e1 = [u[0] / nu, u[1] / nu, u[2] / nu];
    let v = cross3(d, e1);
    let nv = norm3(v)?;
    if !nv.is_finite() || nv <= 0.0 {
        return Err(RoundnessError::Form { section: None, source: FormError::FitRefused { code: "degenerate_axis" } });
    }
    let e2 = [v[0] / nv, v[1] / nv, v[2] / nv];
    Ok((e1, e2))
}

/// Fits the reference axis to a 3D point cloud using the KRN least-squares cylinder or cone fit,
/// then extracts planar cross-sections at `spec.positions_m` within `spec.band_half_width_m`
/// preserving input point order (`REQ-003`, `AC-2`).
pub fn extract_sections_3d(
    points: &[[f64; 3]],
    spec: &SectionSpec,
) -> Result<(Vec<SectionPoints>, ExtractionSummary), RoundnessError> {
    if spec.positions_m.is_empty() || spec.positions_m.len() > MAX_SECTIONS {
        return Err(RoundnessError::InvalidSectionSpec { reason: "bad_positions" });
    }
    if !spec.band_half_width_m.is_finite() || spec.band_half_width_m <= 0.0 {
        return Err(RoundnessError::InvalidSectionSpec { reason: "non_positive_band" });
    }
    let mut flat = Vec::with_capacity(points.len() * 3);
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(RoundnessError::Form { section: None, source: FormError::NanInput });
        }
        flat.push(p[0]);
        flat.push(p[1]);
        flat.push(p[2]);
    }
    let view = PointsView::new(&flat, None)
        .map_err(|e| RoundnessError::Form { section: None, source: FormError::FitRefused { code: e.code() } })?;
    let params = FitParams::new(Method::LeastSquares);

    let axis = match spec.axis_fit {
        AxisFitKind::Cylinder => {
            let cyl = ls_cyl::fit_cylinder(&view, &params, Some(0)).map_err(|e| RoundnessError::Form {
                section: None,
                source: FormError::FitRefused { code: e.code() },
            })?;
            let FittedFeature::Cylinder { p, d, .. } = cyl.result.feature else {
                return Err(RoundnessError::Form {
                    section: None,
                    source: FormError::FitRefused { code: "unexpected_axis_feature" },
                });
            };
            let dot_pd = dot3(p, d);
            let foot = [p[0] - dot_pd * d[0], p[1] - dot_pd * d[1], p[2] - dot_pd * d[2]];
            ExtractedAxis {
                origin: foot,
                direction: d,
                algorithm_id: cyl.result.algorithm.to_string(),
                local_optimum_only: cyl.local_optimum_only,
            }
        }
        AxisFitKind::Cone => {
            let cone = ls_cone::fit_cone(&view, &params, &ls_cone::ConeOptions::default(), Some(0)).map_err(|e| {
                RoundnessError::Form { section: None, source: FormError::FitRefused { code: e.code() } }
            })?;
            let FittedFeature::Cone { apex, d, .. } = cone.result.feature else {
                return Err(RoundnessError::Form {
                    section: None,
                    source: FormError::FitRefused { code: "unexpected_axis_feature" },
                });
            };
            // Foot of the world origin onto the cone axis so axial coordinate s = dot(q - origin, d).
            let dot_ad = dot3(apex, d);
            let foot = [apex[0] - dot_ad * d[0], apex[1] - dot_ad * d[1], apex[2] - dot_ad * d[2]];
            ExtractedAxis {
                origin: foot,
                direction: d,
                algorithm_id: cone.result.algorithm.to_string(),
                local_optimum_only: cone.local_optimum_only,
            }
        }
    };

    let (e1, e2) = section_basis(axis.direction)?;
    let mut out = Vec::with_capacity(spec.positions_m.len());
    for &pos_m in &spec.positions_m {
        if !pos_m.is_finite() {
            return Err(RoundnessError::InvalidSectionSpec { reason: "non_finite_position" });
        }
        let mut pts2 = Vec::new();
        for &q in points {
            let rel = sub3(q, axis.origin);
            let s = dot3(rel, axis.direction);
            if (s - pos_m).abs().total_cmp(&spec.band_half_width_m) != Ordering::Greater {
                pts2.push([dot3(rel, e1), dot3(rel, e2)]);
            }
        }
        out.push(SectionPoints { position_m: pos_m, points2d: pts2 });
    }

    let summary =
        ExtractionSummary { axis, positions_m: spec.positions_m.clone(), band_half_width_m: spec.band_half_width_m };
    Ok((out, summary))
}

/// Evaluates roundness across explicit 2D cross-sections via `GDT.FORM.FCORE` (`FormElement::Circle2d`)
/// and reports the worst section (lowest section index on ties) and all section values (`REQ-001`, `REQ-002`, `REQ-004`).
pub fn evaluate_roundness_sections(
    sections: &[SectionPoints],
    chain: &OperatorChain,
    krn: &dyn FitProvider,
) -> Result<RoundnessResult, RoundnessError> {
    if sections.is_empty() {
        return Err(RoundnessError::NoSections);
    }
    if sections.len() > MAX_SECTIONS {
        return Err(RoundnessError::InvalidSectionSpec { reason: "too_many_sections" });
    }
    let mut results = Vec::with_capacity(sections.len());
    let mut worst_idx = 0usize;
    let mut worst_val = f64::NEG_INFINITY;

    for (idx, sec) in sections.iter().enumerate() {
        if !sec.position_m.is_finite() {
            return Err(RoundnessError::Form { section: Some(idx), source: FormError::NanInput });
        }
        let mut pts3 = Vec::with_capacity(sec.points2d.len());
        for p in &sec.points2d {
            if !p[0].is_finite() || !p[1].is_finite() {
                return Err(RoundnessError::Form { section: Some(idx), source: FormError::NanInput });
            }
            pts3.push([p[0], p[1], 0.0]);
        }
        let payload = evaluate_form(FormElement::Circle2d, &pts3, chain, krn).map_err(|e| match e {
            FormError::TooFewPoints { required, given } => {
                RoundnessError::TooFewPoints { section: idx, required, given }
            }
            other => RoundnessError::Form { section: Some(idx), source: other },
        })?;
        if idx == 0 || payload.value_si.total_cmp(&worst_val) == Ordering::Greater {
            worst_val = payload.value_si;
            worst_idx = idx;
        }
        results.push(SectionResult { section_index: idx, position_m: sec.position_m, payload });
    }

    Ok(RoundnessResult { value_si: worst_val, worst_section_index: worst_idx, sections: results, extracted: None })
}

/// Evaluates roundness from a 3D point cloud by reading the [`SectionSpec`] from `chain`,
/// extracting cross-sections perpendicular to the fitted reference axis, and evaluating each
/// section via `GDT.FORM.FCORE` (`REQ-001`..`REQ-004`, `AC-2`).
pub fn evaluate_roundness_3d(
    points: &[[f64; 3]],
    chain: &OperatorChain,
    krn: &dyn FitProvider,
) -> Result<RoundnessResult, RoundnessError> {
    let spec = SectionSpec::from_chain(chain)?;
    let (sections, summary) = extract_sections_3d(points, &spec)?;
    let mut res = evaluate_roundness_sections(&sections, chain, krn)?;
    res.extracted = Some(summary);
    Ok(res)
}
