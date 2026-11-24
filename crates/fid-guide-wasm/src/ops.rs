//! Seven live Field Guide kernel operations calling the real workspace crates:
//! - `fit.circle2d`     (`fid-fit`: `Circle2Ls`)
//! - `fit.plane_datum`  (`fid-fit`: `PlaneLs`, `fit_tan_plane`, `tan_rock`)
//! - `fit.cylinder_mcc` (`fid-fit`: `fit_cylinder`, `fit_mcc_cylinder`)
//! - `gdt.zone_eval`    (`fid-gps`: `ZoneShape::usage`, `ZoneShape::contains`)
//! - `unc.propagate`    (`fid-kunc`: `Distribution`, `summarize`, `validate_lpu`, `UncertaintyStatement`)
//! - `dec.evaluate`     (`fid-kdec`: `Catalog`, `evaluate`, `risk`)
//! - `canon.digest`     (`fid-canon`: `parse_with_limits`, `to_canonical_bytes`, `digest_bytes`)

use std::collections::BTreeMap;

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_cyl;
use fid_fit::ls_lin::PlaneLs;
use fid_fit::ls_rnd::{self, Circle2Ls};
use fid_fit::mcc_cyl;
use fid_fit::tan_plane::{self, TanPlaneParams};
use fid_fit::tan_rock::{MaterialSide, TanVariant};
use fid_gps::zone::shape::{Pose, ZoneInput, ZoneShape};
use fid_kdec::eval::{evaluate as kdec_evaluate, EvalInput, Tolerance, DEFAULT_WARN_RATIO};
use fid_kdec::risk::{acceptance_zone_for_rule, global_risk, specific_risk, Prior, ToleranceF64};
use fid_kdec::rule::{Catalog, Outcome, RuleRef};
use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::prop::mcm::interval::summarize as mcm_summarize;
use fid_kunc::prop::valid::{validate_lpu, ValidateInput};
use fid_kunc::statement::{
    EvidenceClass, Expanded, IntervalKind, Method as UncMethod, ModelRef, Provenance, SourceKind, SourceRef,
    StatementParts, Uec0Reason, UncertaintyStatement,
};
use fid_num::kdec::Decimal;
use fid_num::{f as num_f, red};
use fid_types::Digest;

use crate::json_helpers::{
    c_bool, c_f64, c_str, c_u64, c_usize, c_vec_f64, c_vec_u32, opt_bool, opt_f64, opt_str, opt_u64, parse_points2d,
    parse_points3d, parse_vec3, req_arr, req_f64, req_str,
};
use crate::LabError;

const MAX_LAB_POINTS: usize = 512;
const MAX_MC_TRIALS: u64 = 50_000;
const MIN_MC_TRIALS: u64 = 200;

/// `fit.circle2d`: real `fid_fit::ls_rnd::Circle2Ls` fit on 2D XY points (in mm or m; unit
/// declared via `unit`, converted to SI metres for the kernel and returned in both SI metres
/// and input units). Supports optional `exclude_index` to disclose outlier impact.
pub fn fit_circle2d(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let unit = opt_str(params, "unit")?.unwrap_or("mm");
    let scale_to_m = unit_to_metres(unit)?;
    let pts2 = parse_points2d(params, "points", MAX_LAB_POINTS)?;
    if pts2.len() < 3 {
        return Err(LabError::new("too_few_points", "circle2d requires at least 3 points"));
    }

    let fit_all = run_circle2d(&pts2, scale_to_m)?;
    let mut out = circle_obj(&fit_all, scale_to_m, unit)?;

    if let Some(ex_idx_u64) = opt_u64(params, "exclude_index")? {
        let ex_idx = usize::try_from(ex_idx_u64).map_err(|_| LabError::new("invalid_param", "index overflow"))?;
        if ex_idx >= pts2.len() {
            return Err(LabError::new("invalid_param", "exclude_index out of range"));
        }
        let filtered: Vec<[f64; 2]> = pts2.iter().enumerate().filter(|(i, _)| *i != ex_idx).map(|(_, p)| *p).collect();
        if filtered.len() < 3 {
            return Err(LabError::new("too_few_points", "excluding point leaves fewer than 3 points"));
        }
        let fit_clean = run_circle2d(&filtered, scale_to_m)?;
        let mut clean_obj = circle_obj(&fit_clean, scale_to_m, unit)?;
        clean_obj.insert("excluded_index".to_string(), c_usize(ex_idx)?);
        let dr = (fit_all.radius_m - fit_clean.radius_m) / scale_to_m;
        let dc_x = (fit_all.cx_m - fit_clean.cx_m) / scale_to_m;
        let dc_y = (fit_all.cy_m - fit_clean.cy_m) / scale_to_m;
        let center_shift = num_f::hypot(dc_x, dc_y).map_err(|e| LabError::new("num_error", e.to_string()))?;
        clean_obj.insert("radius_delta".to_string(), c_f64(dr)?);
        clean_obj.insert("diameter_delta".to_string(), c_f64(2.0 * dr)?);
        clean_obj.insert("center_shift".to_string(), c_f64(center_shift)?);
        out.insert("without_outlier".to_string(), CValue::Obj(clean_obj));
    }

    Ok(CValue::Obj(out))
}

struct CircleSummary {
    cx_m: f64,
    cy_m: f64,
    radius_m: f64,
    rms_m: f64,
    min_res_m: f64,
    max_res_m: f64,
    residuals_m: Vec<f64>,
    algorithm_id: String,
}

fn run_circle2d(pts2: &[[f64; 2]], scale_to_m: f64) -> Result<CircleSummary, LabError> {
    let mut flat = Vec::with_capacity(pts2.len() * 3);
    for p in pts2 {
        flat.push(p[0] * scale_to_m);
        flat.push(p[1] * scale_to_m);
        flat.push(0.0);
    }
    let view = PointsView::new(&flat, None).map_err(|e| LabError::new("fit_input_error", e.to_string()))?;
    let fp = ls_rnd::default_params();
    let res = Circle2Ls.fit(&view, &fp, None).map_err(|e| LabError::new("fit_failed", e.to_string()))?;
    let FittedFeature::Circle2 { c, r } = res.feature else {
        return Err(LabError::new("internal_mismatch", "expected Circle2 feature"));
    };
    Ok(CircleSummary {
        cx_m: c[0],
        cy_m: c[1],
        radius_m: r,
        rms_m: res.stats.rms,
        min_res_m: res.stats.min,
        max_res_m: res.stats.max,
        residuals_m: res.residuals,
        algorithm_id: res.algorithm.to_string(),
    })
}

fn circle_obj(s: &CircleSummary, scale_to_m: f64, unit: &str) -> Result<BTreeMap<String, CValue>, LabError> {
    let mut m = BTreeMap::new();
    let res_unit: Vec<f64> = s.residuals_m.iter().map(|r| *r / scale_to_m).collect();
    let pv_m = s.max_res_m - s.min_res_m;
    m.insert("algorithm_id".to_string(), c_str(&s.algorithm_id));
    m.insert("unit".to_string(), c_str(unit));
    m.insert("center_x".to_string(), c_f64(s.cx_m / scale_to_m)?);
    m.insert("center_y".to_string(), c_f64(s.cy_m / scale_to_m)?);
    m.insert("radius".to_string(), c_f64(s.radius_m / scale_to_m)?);
    m.insert("diameter".to_string(), c_f64((2.0 * s.radius_m) / scale_to_m)?);
    m.insert("rms".to_string(), c_f64(s.rms_m / scale_to_m)?);
    m.insert("min_residual".to_string(), c_f64(s.min_res_m / scale_to_m)?);
    m.insert("max_residual".to_string(), c_f64(s.max_res_m / scale_to_m)?);
    m.insert("roundness_pv".to_string(), c_f64(pv_m / scale_to_m)?);
    m.insert("residuals".to_string(), c_vec_f64(&res_unit)?);
    m.insert("radius_m".to_string(), c_f64(s.radius_m)?);
    m.insert("point_count".to_string(), c_usize(s.residuals_m.len())?);
    Ok(m)
}

/// `fit.plane_datum`: compares `PlaneLs` (Gaussian) against `fit_tan_plane` (external
/// minimax or constrained L1 tangential datum simulator) plus rocking report.
pub fn fit_plane_datum(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let unit = opt_str(params, "unit")?.unwrap_or("mm");
    let scale_to_m = unit_to_metres(unit)?;
    let pts3 = parse_points3d(params, "points", MAX_LAB_POINTS)?;
    if pts3.len() < 3 {
        return Err(LabError::new("too_few_points", "plane_datum requires at least 3 points"));
    }
    let variant = match opt_str(params, "variant")?.unwrap_or("external_minimax") {
        "external_minimax" | "minimax" => TanVariant::ExternalMinimax,
        "constrained_l1" | "l1" => TanVariant::ConstrainedL1,
        other => {
            return Err(LabError::new(
                "invalid_param",
                format!("unsupported tangential variant `{other}` (use external_minimax or constrained_l1)"),
            ));
        }
    };
    let outward = match params.get("outward") {
        Some(v) => parse_vec3(v, "outward")?,
        None => [0.0, 0.0, 1.0],
    };
    let rock_tol = opt_f64(params, "rock_tol")?.unwrap_or(0.05);
    let seed = opt_u64(params, "seed")?.unwrap_or(7);

    let mut flat = Vec::with_capacity(pts3.len() * 3);
    for p in &pts3 {
        flat.push(p[0] * scale_to_m);
        flat.push(p[1] * scale_to_m);
        flat.push(p[2] * scale_to_m);
    }
    let view = PointsView::new(&flat, None).map_err(|e| LabError::new("fit_input_error", e.to_string()))?;

    let ls_res = PlaneLs
        .fit(&view, &FitParams::new(Method::LeastSquares), None)
        .map_err(|e| LabError::new("ls_plane_failed", e.to_string()))?;
    let FittedFeature::Plane { p: ls_p, n: ls_n_raw } = ls_res.feature else {
        return Err(LabError::new("internal_mismatch", "expected Plane feature"));
    };
    let ls_dot = red::dot(&ls_n_raw, &outward).map_err(|e| LabError::new("num_error", e.to_string()))?;
    let ls_n = if ls_dot < 0.0 { [-ls_n_raw[0], -ls_n_raw[1], -ls_n_raw[2]] } else { ls_n_raw };

    let tp = TanPlaneParams::new(Some(variant), MaterialSide::Outward(outward), 1e-9, rock_tol);
    let tan_res = tan_plane::fit_tan_plane(&view, &FitParams::new(Method::Tangential), &tp, Some(seed))
        .map_err(|e| LabError::new("tan_plane_failed", e.to_string()))?;
    let FittedFeature::Plane { p: tan_p, n: tan_n } = tan_res.feature else {
        return Err(LabError::new("internal_mismatch", "expected tangential Plane feature"));
    };

    let cos_tilt = red::dot(&ls_n, &tan_n).map_err(|e| LabError::new("num_error", e.to_string()))?;
    let cos_tilt = num_f::total_clamp(cos_tilt, -1.0, 1.0).map_err(|e| LabError::new("num_error", e.to_string()))?;
    let tilt_rad = num_f::acos(cos_tilt).map_err(|e| LabError::new("num_error", e.to_string()))?;

    let mut ls_obj = BTreeMap::new();
    ls_obj.insert("algorithm_id".to_string(), c_str(ls_res.algorithm.to_string()));
    ls_obj
        .insert("origin".to_string(), c_vec_f64(&[ls_p[0] / scale_to_m, ls_p[1] / scale_to_m, ls_p[2] / scale_to_m])?);
    ls_obj.insert("normal".to_string(), c_vec_f64(&ls_n)?);
    ls_obj.insert("rms".to_string(), c_f64(ls_res.stats.rms / scale_to_m)?);
    ls_obj.insert("flatness_span".to_string(), c_f64((ls_res.stats.max - ls_res.stats.min) / scale_to_m)?);

    let depths_unit: Vec<f64> = tan_res.residuals.iter().map(|d| *d / scale_to_m).collect();
    let mut tan_obj = BTreeMap::new();
    tan_obj.insert("algorithm_id".to_string(), c_str(tan_res.algorithm.to_string()));
    tan_obj.insert(
        "origin".to_string(),
        c_vec_f64(&[tan_p[0] / scale_to_m, tan_p[1] / scale_to_m, tan_p[2] / scale_to_m])?,
    );
    tan_obj.insert("normal".to_string(), c_vec_f64(&tan_n)?);
    tan_obj.insert("objective".to_string(), c_f64(tan_res.objective / scale_to_m)?);
    tan_obj.insert("max_depth".to_string(), c_f64(tan_res.stats.max / scale_to_m)?);
    tan_obj.insert("contacts".to_string(), c_vec_u32(&tan_res.contacts)?);
    tan_obj.insert("depths".to_string(), c_vec_f64(&depths_unit)?);
    tan_obj.insert("optimality".to_string(), c_str(tan_res.optimality.as_str()));
    tan_obj.insert("rocking".to_string(), c_bool(tan_res.rock.rocking));
    tan_obj.insert("rock_candidates".to_string(), c_usize(tan_res.rock.candidates.len())?);
    tan_obj.insert("max_violation_m".to_string(), c_f64(tan_res.max_violation)?);

    let mut out = BTreeMap::new();
    out.insert("unit".to_string(), c_str(unit));
    out.insert("interpretation_status".to_string(), c_str("provisional"));
    out.insert("standards_ref".to_string(), c_str("ISO 5459:2011; ASME Y14.5-2018 §7"));
    out.insert("tilt_mrad".to_string(), c_f64(tilt_rad * 1_000.0)?);
    out.insert("least_squares".to_string(), CValue::Obj(ls_obj));
    out.insert("tangential".to_string(), CValue::Obj(tan_obj));
    Ok(CValue::Obj(out))
}

/// `fit.cylinder_mcc`: compares Gaussian least-squares cylinder (`fit_cylinder`) with
/// Minimum Circumscribed Cylinder mating envelope (`fit_mcc_cylinder`). The mating-envelope
/// reading is a GD&T interpretation, so the result carries `interpretation_status:
/// "provisional"` (D-045) and never reads as settled.
pub fn fit_cylinder_mcc(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let unit = opt_str(params, "unit")?.unwrap_or("mm");
    let scale_to_m = unit_to_metres(unit)?;
    let pts3 = parse_points3d(params, "points", MAX_LAB_POINTS)?;
    if pts3.len() < 5 {
        return Err(LabError::new("too_few_points", "cylinder_mcc requires at least 5 points"));
    }
    let seed = opt_u64(params, "seed")?.unwrap_or(0);
    let mut flat = Vec::with_capacity(pts3.len() * 3);
    for p in &pts3 {
        flat.push(p[0] * scale_to_m);
        flat.push(p[1] * scale_to_m);
        flat.push(p[2] * scale_to_m);
    }
    let view = PointsView::new(&flat, None).map_err(|e| LabError::new("fit_input_error", e.to_string()))?;

    let ls_fit = ls_cyl::fit_cylinder(&view, &FitParams::new(Method::LeastSquares), Some(seed))
        .map_err(|e| LabError::new("ls_cylinder_failed", e.to_string()))?;
    let FittedFeature::Cylinder { p: ls_p, d: ls_d, r: ls_r } = ls_fit.result.feature else {
        return Err(LabError::new("internal_mismatch", "expected Cylinder feature"));
    };

    let mcc_fit = mcc_cyl::fit_mcc_cylinder(&view, &FitParams::new(Method::MinimumCircumscribed), Some(seed))
        .map_err(|e| LabError::new("mcc_cylinder_failed", e.to_string()))?;
    let FittedFeature::Cylinder { p: mc_p, d: mc_d, r: mc_r } = mcc_fit.feature else {
        return Err(LabError::new("internal_mismatch", "expected MCC Cylinder feature"));
    };

    let mut ls_obj = BTreeMap::new();
    ls_obj.insert("algorithm_id".to_string(), c_str(ls_fit.result.algorithm.to_string()));
    ls_obj.insert(
        "axis_point".to_string(),
        c_vec_f64(&[ls_p[0] / scale_to_m, ls_p[1] / scale_to_m, ls_p[2] / scale_to_m])?,
    );
    ls_obj.insert("axis_dir".to_string(), c_vec_f64(&ls_d)?);
    ls_obj.insert("diameter".to_string(), c_f64((2.0 * ls_r) / scale_to_m)?);
    ls_obj.insert("rms".to_string(), c_f64(ls_fit.result.stats.rms / scale_to_m)?);

    let mut mc_obj = BTreeMap::new();
    mc_obj.insert("algorithm_id".to_string(), c_str(mcc_fit.algorithm.to_string()));
    mc_obj.insert(
        "axis_point".to_string(),
        c_vec_f64(&[mc_p[0] / scale_to_m, mc_p[1] / scale_to_m, mc_p[2] / scale_to_m])?,
    );
    mc_obj.insert("axis_dir".to_string(), c_vec_f64(&mc_d)?);
    mc_obj.insert("diameter".to_string(), c_f64((2.0 * mc_r) / scale_to_m)?);
    mc_obj.insert("contacts".to_string(), c_vec_u32(&mcc_fit.contacts)?);
    mc_obj.insert("local_optimum_only".to_string(), c_bool(mcc_fit.local_optimum_only));

    let mut out = BTreeMap::new();
    out.insert("unit".to_string(), c_str(unit));
    out.insert("diameter_envelope_excess".to_string(), c_f64((2.0 * (mc_r - ls_r)) / scale_to_m)?);
    out.insert("interpretation_status".to_string(), c_str("provisional"));
    out.insert("least_squares".to_string(), CValue::Obj(ls_obj));
    out.insert("minimum_circumscribed".to_string(), CValue::Obj(mc_obj));
    Ok(CValue::Obj(out))
}

/// `gdt.zone_eval`: real `fid_gps::zone::shape::ZoneShape::usage` & `contains` evaluation
/// with explicit `interpretation_status: "provisional"` per D-045.
pub fn gdt_zone_eval(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let unit = opt_str(params, "unit")?.unwrap_or("mm");
    let scale_to_m = unit_to_metres(unit)?;
    let shape_name = req_str(params, "shape")?;
    let tolerance_val = req_f64(params, "tolerance")?;
    if tolerance_val < 0.0 {
        return Err(LabError::new("invalid_param", "tolerance must be non-negative"));
    }
    let origin = match params.get("origin") {
        Some(v) => {
            let o = parse_vec3(v, "origin")?;
            [o[0] * scale_to_m, o[1] * scale_to_m, o[2] * scale_to_m]
        }
        None => [0.0, 0.0, 0.0],
    };
    let dir_raw = match params.get("direction") {
        Some(v) => parse_vec3(v, "direction")?,
        None => [0.0, 0.0, 1.0],
    };
    let dir = normalize3(dir_raw, "direction")?;
    let pose = Pose::new(origin, dir);
    let dialect = opt_str(params, "dialect")?.unwrap_or("ASME Y14.5-2018");

    let pts_raw = parse_points3d(params, "points", MAX_LAB_POINTS)?;
    let pts_m: Vec<[f64; 3]> =
        pts_raw.iter().map(|p| [p[0] * scale_to_m, p[1] * scale_to_m, p[2] * scale_to_m]).collect();

    let (shape, usage, contains) = match shape_name {
        "two_planes" => {
            let s = ZoneShape::TwoPlanes;
            let u =
                s.usage(&pose, ZoneInput::Points(&pts_m)).map_err(|e| LabError::new("zone_error", e.to_string()))?;
            let c = s
                .contains(&pose, tolerance_val * scale_to_m, ZoneInput::Points(&pts_m))
                .map_err(|e| LabError::new("zone_error", e.to_string()))?;
            (s, u, c)
        }
        "cylinder" => {
            let s = ZoneShape::Cylinder;
            let u =
                s.usage(&pose, ZoneInput::Points(&pts_m)).map_err(|e| LabError::new("zone_error", e.to_string()))?;
            let c = s
                .contains(&pose, tolerance_val * scale_to_m, ZoneInput::Points(&pts_m))
                .map_err(|e| LabError::new("zone_error", e.to_string()))?;
            (s, u, c)
        }
        "circle" => {
            let plane = match params.get("plane") {
                Some(v) => normalize3(parse_vec3(v, "plane")?, "plane")?,
                None => [0.0, 0.0, 1.0],
            };
            let s = ZoneShape::Circle { plane };
            let u =
                s.usage(&pose, ZoneInput::Points(&pts_m)).map_err(|e| LabError::new("zone_error", e.to_string()))?;
            let c = s
                .contains(&pose, tolerance_val * scale_to_m, ZoneInput::Points(&pts_m))
                .map_err(|e| LabError::new("zone_error", e.to_string()))?;
            (s, u, c)
        }
        "sphere" => {
            let s = ZoneShape::Sphere;
            let u =
                s.usage(&pose, ZoneInput::Points(&pts_m)).map_err(|e| LabError::new("zone_error", e.to_string()))?;
            let c = s
                .contains(&pose, tolerance_val * scale_to_m, ZoneInput::Points(&pts_m))
                .map_err(|e| LabError::new("zone_error", e.to_string()))?;
            (s, u, c)
        }
        "offset" => {
            let shift = opt_f64(params, "disposition_shift")?.unwrap_or(0.0) * scale_to_m;
            let devs_arr = req_arr(params, "deviations")?;
            let mut devs_m = Vec::with_capacity(devs_arr.len());
            for v in devs_arr {
                let CValue::Num(x) = v else {
                    return Err(LabError::new("invalid_param", "deviations must be finite numbers"));
                };
                if !x.is_finite() {
                    return Err(LabError::new("invalid_param", "non-finite deviation"));
                }
                devs_m.push(*x * scale_to_m);
            }
            let s = ZoneShape::Offset { disposition_shift: shift };
            let input = ZoneInput::Deviations { points: &pts_m, deviations: &devs_m };
            let u = s.usage(&pose, input).map_err(|e| LabError::new("zone_error", e.to_string()))?;
            let c = s
                .contains(&pose, tolerance_val * scale_to_m, input)
                .map_err(|e| LabError::new("zone_error", e.to_string()))?;
            (s, u, c)
        }
        other => return Err(LabError::new("invalid_param", format!("unknown zone shape `{other}`"))),
    };

    let usage_unit = usage.size_m / scale_to_m;
    let margin_unit = tolerance_val - usage_unit;
    let ratio = if tolerance_val > 0.0 { usage_unit / tolerance_val } else { 0.0 };

    let mut out = BTreeMap::new();
    out.insert("shape".to_string(), c_str(shape.name()));
    out.insert("dialect".to_string(), c_str(dialect));
    out.insert("unit".to_string(), c_str(unit));
    out.insert("tolerance".to_string(), c_f64(tolerance_val)?);
    out.insert("usage_size".to_string(), c_f64(usage_unit)?);
    out.insert("usage_size_m".to_string(), c_f64(usage.size_m)?);
    out.insert("driving_index".to_string(), c_u64(u64::from(usage.driving_index))?);
    out.insert("contains".to_string(), c_bool(contains));
    out.insert("margin".to_string(), c_f64(margin_unit)?);
    out.insert("utilization_pct".to_string(), c_f64(ratio * 100.0)?);
    out.insert("interpretation_status".to_string(), c_str("provisional"));
    out.insert("standards_ref".to_string(), c_str("ASME Y14.5-2018; ISO 1101:2017"));
    Ok(CValue::Obj(out))
}

/// `unc.propagate`: real `fid-kunc` uncertainty propagation combining typed `Distribution`
/// contributors via LPU and seeded Monte Carlo (`summarize`, `validate_lpu`, and canonical
/// UEC-1 `UncertaintyStatement` digest).
pub fn unc_propagate(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let model = opt_str(params, "model")?.unwrap_or("linear_sum");
    let trials = opt_u64(params, "trials")?.unwrap_or(4_000);
    if !(MIN_MC_TRIALS..=MAX_MC_TRIALS).contains(&trials) {
        return Err(LabError::new("invalid_param", format!("trials must be in {MIN_MC_TRIALS}..={MAX_MC_TRIALS}")));
    }
    let seed = opt_u64(params, "seed")?.unwrap_or(42);
    let coverage_p = opt_f64(params, "coverage_p")?.unwrap_or(0.95);
    if !(0.5..1.0).contains(&coverage_p) {
        return Err(LabError::new("invalid_param", "coverage_p must lie in [0.5, 1.0)"));
    }
    let k_factor = opt_f64(params, "k")?.unwrap_or(1.959_963_984_540_054);
    if k_factor <= 0.0 {
        return Err(LabError::new("invalid_param", "coverage factor k must be positive"));
    }

    let raw_inputs = req_arr(params, "inputs")?;
    if raw_inputs.is_empty() || raw_inputs.len() > 12 {
        return Err(LabError::new("invalid_param", "inputs must contain 1..=12 contributors"));
    }

    struct ParsedInput {
        name: String,
        dist: Distribution,
        sensitivity: f64,
        mean: f64,
        u_std: f64,
    }

    let mut inputs = Vec::with_capacity(raw_inputs.len());
    for item in raw_inputs {
        let CValue::Obj(obj) = item else {
            return Err(LabError::new("invalid_param", "each input must be a JSON object"));
        };
        let name = req_str(obj, "name")?.to_string();
        let kind = req_str(obj, "kind")?;
        let sensitivity = opt_f64(obj, "sensitivity")?.unwrap_or(1.0);
        let dist = match kind {
            "normal" => {
                let mu = req_f64(obj, "mean")?;
                let sigma = req_f64(obj, "sigma")?;
                Distribution::normal(Quantity::new(mu, UnitCode::Metre), Quantity::new(sigma, UnitCode::Metre))
            }
            "rect" | "rectangular" => {
                let center = req_f64(obj, "mean")?;
                let half = req_f64(obj, "half_width")?;
                Distribution::rect(
                    Quantity::new(center - half, UnitCode::Metre),
                    Quantity::new(center + half, UnitCode::Metre),
                )
            }
            "tri" | "triangular" => {
                let center = req_f64(obj, "mean")?;
                let half = req_f64(obj, "half_width")?;
                let mode = opt_f64(obj, "mode")?.unwrap_or(center);
                Distribution::tri(
                    Quantity::new(center - half, UnitCode::Metre),
                    Quantity::new(center + half, UnitCode::Metre),
                    Quantity::new(mode, UnitCode::Metre),
                )
            }
            "arcsine" | "u_shaped" => {
                let center = req_f64(obj, "mean")?;
                let half = req_f64(obj, "half_width")?;
                Distribution::arcsine(
                    Quantity::new(center - half, UnitCode::Metre),
                    Quantity::new(center + half, UnitCode::Metre),
                )
            }
            other => {
                return Err(LabError::new("invalid_param", format!("unknown distribution kind `{other}`")));
            }
        }
        .map_err(|e| LabError::new("dist_error", e.to_string()))?;

        let mean = dist.expectation().map_err(|e| LabError::new("dist_error", e.to_string()))?;
        let u_std = dist.std_uncertainty().map_err(|e| LabError::new("dist_error", e.to_string()))?;
        inputs.push(ParsedInput { name, dist, sensitivity, mean, u_std });
    }

    // Draw seeded samples per input using fid-kunc's deterministic counter RNG.
    let mut sample_cols = Vec::with_capacity(inputs.len());
    for (idx, inp) in inputs.iter().enumerate() {
        let stream = u64::try_from(idx + 1).map_err(|_| LabError::new("overflow", "stream overflow"))?;
        let draws =
            inp.dist.sample(seed, stream, 0..trials).map_err(|e| LabError::new("sample_error", e.to_string()))?;
        sample_cols.push(draws);
    }

    // Evaluate model at nominal means and across all MC trials.
    let (fx, lpu_u_std, contribs_u): (f64, f64, Vec<f64>) = match model {
        "linear_sum" => {
            let terms: Vec<f64> = inputs.iter().map(|i| i.sensitivity * i.mean).collect();
            let fx = red::sum(&terms).map_err(|e| LabError::new("num_error", e.to_string()))?;
            let cu: Vec<f64> = inputs.iter().map(|i| (i.sensitivity * i.u_std).abs()).collect();
            let uc = red::norm2(&cu).map_err(|e| LabError::new("num_error", e.to_string()))?;
            (fx, uc, cu)
        }
        "radial_2d" => {
            if inputs.len() < 2 {
                return Err(LabError::new("invalid_param", "radial_2d model requires at least 2 inputs (dx, dy)"));
            }
            let x0 = inputs[0].mean;
            let y0 = inputs[1].mean;
            let r0 = num_f::hypot(x0, y0).map_err(|e| LabError::new("num_error", e.to_string()))?;
            let (cx, cy) = if r0 > 0.0 { (x0 / r0, y0 / r0) } else { (0.0, 0.0) };
            let mut cu = Vec::with_capacity(inputs.len());
            cu.push((cx * inputs[0].u_std).abs());
            cu.push((cy * inputs[1].u_std).abs());
            for inp in inputs.iter().skip(2) {
                cu.push((inp.sensitivity * inp.u_std).abs());
            }
            let uc = red::norm2(&cu).map_err(|e| LabError::new("num_error", e.to_string()))?;
            let extra: Vec<f64> = inputs.iter().skip(2).map(|i| i.sensitivity * i.mean).collect();
            let fx = if extra.is_empty() {
                r0
            } else {
                r0 + red::sum(&extra).map_err(|e| LabError::new("num_error", e.to_string()))?
            };
            (fx, uc, cu)
        }
        other => return Err(LabError::new("invalid_param", format!("unknown model `{other}`"))),
    };

    let n_trials = usize::try_from(trials).map_err(|_| LabError::new("overflow", "trials overflow"))?;
    let mut mc_vals = Vec::with_capacity(n_trials);
    for t in 0..n_trials {
        let y = match model {
            "linear_sum" => {
                let mut acc = 0.0;
                for (col_idx, inp) in inputs.iter().enumerate() {
                    acc += inp.sensitivity * sample_cols[col_idx][t];
                }
                acc
            }
            "radial_2d" => {
                let r = num_f::hypot(sample_cols[0][t], sample_cols[1][t])
                    .map_err(|e| LabError::new("num_error", e.to_string()))?;
                let mut acc = r;
                for (col_idx, inp) in inputs.iter().enumerate().skip(2) {
                    acc += inp.sensitivity * sample_cols[col_idx][t];
                }
                acc
            }
            _ => 0.0,
        };
        mc_vals.push(y);
    }

    let mcm = mcm_summarize(&mc_vals, coverage_p, fx).map_err(|e| LabError::new("mcm_error", e.to_string()))?;
    let lpu_expanded = k_factor * lpu_u_std;

    let n_dig_u64 = opt_u64(params, "n_dig")?.unwrap_or(2);
    if !(1..=3).contains(&n_dig_u64) {
        return Err(LabError::new("invalid_param", "n_dig must be 1, 2 or 3"));
    }
    let n_dig = u8::try_from(n_dig_u64).map_err(|_| LabError::new("invalid_param", "n_dig out of range"))?;

    // `lpu_validated` is `Some(_)` only when `validate_lpu` actually ran against this Monte
    // Carlo run; with a zero LPU interval there is nothing to validate and the flag stays unset.
    let (lpu_validated, val_d_low, val_d_high, val_delta) = if lpu_expanded > 0.0 {
        let vin = ValidateInput::new(fx, lpu_expanded, coverage_p, mcm.sym.low, mcm.sym.high, coverage_p, mcm.u)
            .with_n_dig(n_dig);
        let vout = validate_lpu(&vin).map_err(|e| LabError::new("validate_error", e.to_string()))?;
        (Some(vout.validated), vout.d_low, vout.d_high, vout.delta)
    } else {
        (None, (fx - mcm.sym.low).abs(), (mcm.sym.high - fx).abs(), 0.0)
    };

    // Build a real canonical UEC-1 UncertaintyStatement so the digest is kernel-authentic. An
    // LPU statement carries the LPU numbers, and `lpu_validated_by_mc` records the validation
    // that ran above. When LPU yields zero uncertainty (e.g. a zero Jacobian) no valid LPU
    // statement exists, so the statement records the Monte Carlo result itself (method MCM,
    // which never carries `lpu_validated_by_mc`).
    let (stmt, stmt_method) = if lpu_expanded > 0.0 {
        let expanded = Expanded { value: lpu_expanded, k: k_factor };
        let low = fx - lpu_expanded;
        let high = fx + lpu_expanded;
        (build_uec1_statement(UncMethod::Lpu, lpu_u_std, Some(expanded), coverage_p, low, high, lpu_validated)?, "LPU")
    } else {
        (build_uec1_statement(UncMethod::Mcm, mcm.u, None, coverage_p, mcm.sym.low, mcm.sym.high, None)?, "MCM")
    };
    let hist = build_histogram(&mc_vals, 20)?;

    let var_total = lpu_u_std * lpu_u_std;
    let mut contrib_rows = Vec::with_capacity(inputs.len());
    for (inp, &u_c) in inputs.iter().zip(contribs_u.iter()) {
        let share = if var_total > 0.0 { (u_c * u_c) / var_total } else { 0.0 };
        let mut row = BTreeMap::new();
        row.insert("name".to_string(), c_str(&inp.name));
        row.insert("u_std".to_string(), c_f64(inp.u_std)?);
        row.insert("u_contrib".to_string(), c_f64(u_c)?);
        row.insert("variance_share_pct".to_string(), c_f64(share * 100.0)?);
        contrib_rows.push(CValue::Obj(row));
    }

    let mut out = BTreeMap::new();
    out.insert("model".to_string(), c_str(model));
    out.insert("trials".to_string(), c_u64(trials)?);
    out.insert("seed".to_string(), c_u64(seed)?);
    out.insert("coverage_p".to_string(), c_f64(coverage_p)?);
    out.insert("fx".to_string(), c_f64(fx)?);
    out.insert("lpu_u_std".to_string(), c_f64(lpu_u_std)?);
    out.insert("lpu_expanded".to_string(), c_f64(lpu_expanded)?);
    out.insert("lpu_low".to_string(), c_f64(fx - lpu_expanded)?);
    out.insert("lpu_high".to_string(), c_f64(fx + lpu_expanded)?);
    out.insert("mcm_mean".to_string(), c_f64(mcm.mean)?);
    out.insert("mcm_u_std".to_string(), c_f64(mcm.u)?);
    out.insert("mcm_sym_low".to_string(), c_f64(mcm.sym.low)?);
    out.insert("mcm_sym_high".to_string(), c_f64(mcm.sym.high)?);
    out.insert("mcm_shortest_low".to_string(), c_f64(mcm.shortest.low)?);
    out.insert("mcm_shortest_high".to_string(), c_f64(mcm.shortest.high)?);
    out.insert("mcm_bias".to_string(), c_f64(mcm.mean_minus_fx)?);
    out.insert("lpu_check_passed".to_string(), c_bool(lpu_validated == Some(true)));
    out.insert("mc_validation".to_string(), c_str(if lpu_validated.is_some() { "ran" } else { "not_run" }));
    out.insert("validation_d_low".to_string(), c_f64(val_d_low)?);
    out.insert("validation_d_high".to_string(), c_f64(val_d_high)?);
    out.insert("validation_delta".to_string(), c_f64(val_delta)?);
    out.insert("evidence_class".to_string(), c_str("UEC-1"));
    out.insert("statement_digest".to_string(), c_str(stmt.digest().to_string()));
    out.insert("statement_method".to_string(), c_str(stmt_method));
    out.insert("contributors".to_string(), CValue::Arr(contrib_rows));
    out.insert("histogram".to_string(), hist);
    out.insert("standards_ref".to_string(), c_str("JCGM 100:2008; JCGM 101:2008"));
    Ok(CValue::Obj(out))
}

/// `dec.evaluate`: real `fid_kdec` conformance decision evaluation & specific/global risk
/// across ISO 14253-1 / JCGM 106 rules (including UEC-0 refusal demonstration).
pub fn dec_evaluate(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let value = req_f64(params, "value")?;
    let lsl_str = req_str(params, "lsl")?;
    let usl_str = req_str(params, "usl")?;
    let rule_ref_str = opt_str(params, "rule_ref")?.unwrap_or("dr:iso14253-1:2017/default@1");
    let u_std = opt_f64(params, "u_std")?;
    let k = opt_f64(params, "k")?.unwrap_or(2.0);
    let uec0 = opt_bool(params, "uec0", false)?;
    let prior_mu = opt_f64(params, "prior_mu")?;
    let prior_sigma = opt_f64(params, "prior_sigma")?;

    let lsl = Decimal::parse(lsl_str).map_err(|e| LabError::new("invalid_param", format!("lsl: {e}")))?;
    let usl = Decimal::parse(usl_str).map_err(|e| LabError::new("invalid_param", format!("usl: {e}")))?;
    let tol = Tolerance { lsl: Some(lsl), usl: Some(usl), unit: "si:m".to_string() };

    let catalog = Catalog::builtin().map_err(|e| LabError::new("catalog_error", e.to_string()))?;
    let rref = RuleRef::parse(rule_ref_str).map_err(|e| LabError::new("invalid_rule", e.to_string()))?;
    let rule = catalog.resolve(&rref).map_err(|e| LabError::new("rule_not_found", e.to_string()))?;

    let stmt = if uec0 {
        let mut sp = StatementParts::new(EvidenceClass::Uec0);
        sp.uec0_reason = Some(Uec0Reason::UMissing);
        UncertaintyStatement::new(sp).map_err(|e| LabError::new("stmt_error", e.to_string()))?
    } else {
        let u = u_std.ok_or_else(|| LabError::new("invalid_param", "u_std required when uec0 is false"))?;
        if u <= 0.0 || k <= 0.0 {
            return Err(LabError::new("invalid_param", "u_std and k must be strictly positive"));
        }
        build_uec1_lpu_for_dec(u, k)?
    };

    let warn_ratio = Decimal::parse(DEFAULT_WARN_RATIO).map_err(|e| LabError::new("internal_error", e.to_string()))?;
    let rec = kdec_evaluate(&EvalInput { value, statement: &stmt, tolerance: &tol, rule: &rule, warn_ratio })
        .map_err(|e| LabError::new("eval_error", e.to_string()))?;

    let (spec_c, spec_p) =
        specific_risk(rec.outcome(), rec.p_conformance()).map_err(|e| LabError::new("risk_error", e.to_string()))?;

    let (acc_low, acc_high, glob_c, glob_p) = if let Some(u) = u_std.filter(|_| !uec0) {
        let tol_f = ToleranceF64::from_tolerance(&tol).map_err(|e| LabError::new("risk_error", e.to_string()))?;
        let zone = acceptance_zone_for_rule(tol_f, &rule, u, rec.expanded_u()).ok();
        let (al, ah) = match zone {
            Some(z) => (z.lower, z.upper),
            None => (None, None),
        };
        let (gc, gp) = match (zone, prior_mu, prior_sigma) {
            (Some(z), Some(mu), Some(sig)) => {
                let prior = Prior::new(mu, sig, "field-guide-lab-prior")
                    .map_err(|e| LabError::new("invalid_prior", e.to_string()))?;
                let (c, p) =
                    global_risk(tol_f, z, u, &prior).map_err(|e| LabError::new("risk_error", e.to_string()))?;
                (Some(c), Some(p))
            }
            _ => (None, None),
        };
        (al, ah, gc, gp)
    } else {
        (None, None, None, None)
    };

    let flags: Vec<CValue> = rec.flags().iter().map(|fl| c_str(fl.wire())).collect();
    let outcome_str = match rec.outcome() {
        Outcome::Conforms => "CONFORMS".to_string(),
        Outcome::Nonconforms => "NONCONFORMS".to_string(),
        Outcome::Indeterminate => "INDETERMINATE".to_string(),
        Outcome::NotDecidable(reason) => format!("NOT_DECIDABLE({})", reason.wire()),
    };

    let mut out = BTreeMap::new();
    out.insert("outcome".to_string(), c_str(outcome_str));
    out.insert("rule_ref".to_string(), c_str(rule.rule_ref().to_string()));
    out.insert("evidence_class".to_string(), c_str(if uec0 { "UEC-0" } else { "UEC-1" }));
    out.insert("value".to_string(), c_f64(value)?);
    out.insert("lsl".to_string(), c_str(lsl_str));
    out.insert("usl".to_string(), c_str(usl_str));
    if let Some(u) = rec.expanded_u() {
        out.insert("expanded_u".to_string(), c_f64(u)?);
    }
    if let Some(p) = rec.p_conformance() {
        out.insert("p_conformance".to_string(), c_f64(p)?);
    }
    if let Some((plo, phi)) = rec.p_bounds() {
        out.insert("p_low".to_string(), c_f64(plo)?);
        out.insert("p_high".to_string(), c_f64(phi)?);
    }
    if let Some(v) = acc_low {
        out.insert("acceptance_low".to_string(), c_f64(v)?);
    }
    if let Some(v) = acc_high {
        out.insert("acceptance_high".to_string(), c_f64(v)?);
    }
    if let Some(v) = spec_c {
        out.insert("specific_consumer_risk".to_string(), c_f64(v)?);
    }
    if let Some(v) = spec_p {
        out.insert("specific_producer_risk".to_string(), c_f64(v)?);
    }
    if let Some(v) = glob_c {
        out.insert("global_consumer_risk".to_string(), c_f64(v)?);
    }
    if let Some(v) = glob_p {
        out.insert("global_producer_risk".to_string(), c_f64(v)?);
    }
    out.insert("flags".to_string(), CValue::Arr(flags));
    // No Monte Carlo run backs the lab's statement (see `build_uec1_lpu_for_dec`).
    out.insert("mc_validation".to_string(), c_str("not_run"));
    out.insert("interpretation_status".to_string(), c_str("provisional"));
    out.insert("standards_ref".to_string(), c_str("ISO 14253-1:2017; JCGM 106:2012"));
    Ok(CValue::Obj(out))
}

/// `canon.digest`: real `fid-canon` RFC 8785 canonicalization & SHA-256 digest with optional
/// second document comparison showing `IDENTICAL` vs `DIVERGENT` replay verification.
pub fn canon_digest(params: &BTreeMap<String, CValue>) -> Result<CValue, LabError> {
    let raw_a = req_str(params, "json_a")?;
    let val_a = fid_canon::parse_with_limits(raw_a.as_bytes(), 65_536, 32)
        .map_err(|e| LabError::new("canon_parse_error", e.to_string()))?;
    let canon_a = to_canonical_bytes(&val_a).map_err(|e| LabError::new("canon_encode_error", e.to_string()))?;
    let canon_a_str = String::from_utf8(canon_a.clone()).map_err(|_| LabError::new("utf8_error", "invalid utf8"))?;
    let digest_a = digest_bytes(&canon_a).to_string();

    let mut out = BTreeMap::new();
    out.insert("canonical_a".to_string(), c_str(canon_a_str));
    out.insert("digest_a".to_string(), c_str(&digest_a));
    out.insert("byte_length_a".to_string(), c_usize(canon_a.len())?);

    if let Some(raw_b) = opt_str(params, "json_b")? {
        let val_b = fid_canon::parse_with_limits(raw_b.as_bytes(), 65_536, 32)
            .map_err(|e| LabError::new("canon_parse_error", e.to_string()))?;
        let canon_b = to_canonical_bytes(&val_b).map_err(|e| LabError::new("canon_encode_error", e.to_string()))?;
        let canon_b_str =
            String::from_utf8(canon_b.clone()).map_err(|_| LabError::new("utf8_error", "invalid utf8"))?;
        let digest_b = digest_bytes(&canon_b).to_string();
        let identical = digest_a == digest_b;
        out.insert("canonical_b".to_string(), c_str(canon_b_str));
        out.insert("digest_b".to_string(), c_str(&digest_b));
        out.insert("byte_length_b".to_string(), c_usize(canon_b.len())?);
        out.insert("replay_verdict".to_string(), c_str(if identical { "IDENTICAL" } else { "DIVERGENT" }));
    }

    out.insert("standards_ref".to_string(), c_str("RFC 8785 (JCS); CONSTITUTION §4 & §6"));
    Ok(CValue::Obj(out))
}

fn unit_to_metres(unit: &str) -> Result<f64, LabError> {
    match unit {
        "m" | "si:m" => Ok(1.0),
        "mm" => Ok(1e-3),
        "um" => Ok(1e-6),
        "in" => Ok(0.0254),
        other => Err(LabError::new("invalid_unit", format!("unsupported unit `{other}` (use m, mm, um, in)"))),
    }
}

fn normalize3(v: [f64; 3], label: &str) -> Result<[f64; 3], LabError> {
    let n = red::norm2(&v).map_err(|e| LabError::new("num_error", e.to_string()))?;
    if n == 0.0 || !n.is_finite() {
        return Err(LabError::new("invalid_param", format!("`{label}` must be a non-zero vector")));
    }
    Ok([v[0] / n, v[1] / n, v[2] / n])
}

fn build_uec1_statement(
    method: UncMethod,
    u_std: f64,
    expanded: Option<Expanded>,
    p: f64,
    low: f64,
    high: f64,
    lpu_validated: Option<bool>,
) -> Result<UncertaintyStatement, LabError> {
    let zero_digest = Digest::parse("sha256:0000000000000000000000000000000000000000000000000000000000000000")
        .map_err(|e| LabError::new("digest_error", e.to_string()))?;
    let mut sp = StatementParts::new(EvidenceClass::Uec1);
    sp.method = Some(method);
    sp.unit = Some("si:m".to_string());
    sp.u_std = Some(u_std);
    sp.expanded = expanded;
    sp.interval = Some(fid_kunc::CoverageInterval { low, high, p, kind: IntervalKind::ProbSymmetric });
    sp.model = Some(ModelRef { id: "um:guide.lab".to_string(), semver: "1.0.0".to_string(), digest: zero_digest });
    sp.sources = vec![SourceRef {
        id: "guide-lab-budget".to_string(),
        kind: SourceKind::Declared,
        digest: zero_digest,
        unverified: true,
    }];
    sp.lpu_validated_by_mc = lpu_validated;
    sp.provenance = Provenance::default();
    UncertaintyStatement::new(sp).map_err(|e| LabError::new("stmt_error", e.to_string()))
}

/// The decision lab's UEC-1 LPU statement. The lab takes only `u_std` and `k`: there is no
/// measurement model or input distribution to run a Monte Carlo check against, so no
/// validation happens and `lpu_validated_by_mc` stays `None`. Under a conformance-probability
/// rule `fid_kdec::evaluate` then has no admissible PDF and returns the honest
/// `NOT_DECIDABLE(pdf_unavailable)`.
fn build_uec1_lpu_for_dec(u_std: f64, k: f64) -> Result<UncertaintyStatement, LabError> {
    let zero_digest = Digest::parse("sha256:0000000000000000000000000000000000000000000000000000000000000000")
        .map_err(|e| LabError::new("digest_error", e.to_string()))?;
    let mut sp = StatementParts::new(EvidenceClass::Uec1);
    sp.method = Some(UncMethod::Lpu);
    sp.unit = Some("si:m".to_string());
    sp.u_std = Some(u_std);
    sp.expanded = Some(Expanded { value: k * u_std, k });
    sp.model = Some(ModelRef { id: "um:guide.dec".to_string(), semver: "1.0.0".to_string(), digest: zero_digest });
    sp.sources = vec![SourceRef {
        id: "guide-dec-budget".to_string(),
        kind: SourceKind::Declared,
        digest: zero_digest,
        unverified: true,
    }];
    sp.lpu_validated_by_mc = None;
    sp.provenance = Provenance::default();
    UncertaintyStatement::new(sp).map_err(|e| LabError::new("stmt_error", e.to_string()))
}

fn build_histogram(values: &[f64], bin_count: usize) -> Result<CValue, LabError> {
    if values.is_empty() || bin_count == 0 {
        return Ok(CValue::Arr(Vec::new()));
    }
    let mut lo = values[0];
    let mut hi = values[0];
    for &v in values.iter().skip(1) {
        if v < lo {
            lo = v;
        }
        if v > hi {
            hi = v;
        }
    }
    let span = hi - lo;
    let mut counts = vec![0u64; bin_count];
    for &v in values {
        let idx = if span <= 0.0 {
            0
        } else {
            let scaled = ((v - lo) / span) * (bin_count as f64);
            let raw = scaled.floor();
            if raw < 0.0 {
                0
            } else {
                let u = raw as usize;
                if u >= bin_count {
                    bin_count - 1
                } else {
                    u
                }
            }
        };
        counts[idx] = counts[idx].saturating_add(1);
    }
    let width = if span > 0.0 { span / (bin_count as f64) } else { 0.0 };
    let mut out = Vec::with_capacity(bin_count);
    for (i, &cnt) in counts.iter().enumerate() {
        let mut b = BTreeMap::new();
        b.insert("low".to_string(), c_f64(lo + (i as f64) * width)?);
        b.insert("high".to_string(), c_f64(lo + ((i + 1) as f64) * width)?);
        b.insert("count".to_string(), c_u64(cnt)?);
        out.push(CValue::Obj(b));
    }
    Ok(CValue::Arr(out))
}
