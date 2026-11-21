//! Acceptance criteria of spec node KRN.LSQ.AXIS.CONE that do not need the RDL or the command
//! layer (AC-3, AC-4, the fit part of AC-5) and the requirements they exercise (REQ-001, -002,
//! -003, -005), plus the wrong-axis check of `ls_cone` rule 9.
//!
//! Expected values come from `fixtures/fit/make_axis_cone_fixtures.py`: the least-squares cone
//! of each rounded point set computed by Gauss-Newton in 60-digit Decimal (no libm, independent
//! of this crate), and hand-labelled refusals. `OUTPUT_PIN` is a regression pin of this crate's
//! own output (not an oracle); CI checks it on x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{Fit, FitError, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_cone::{cone_command_row, fit_cone, ConeFit, ConeLs, ConeOptions};
use fid_kci::{check, parse_card, InputArity};
use fid_solv::mstart::StartSummary;
use fid_testkit::Sha256Hasher;
use fid_types::{Determinism, SideEffect};
use std::sync::OnceLock;

const FIXTURE: &str = include_str!("../../../fixtures/fit/axis-cone-v1/cases.txt");
const CARD: &str = include_str!("../cards/fit.cone.ls@1.0.0.toml");
const SEED: u64 = 20_261_121;

/// AC-3's thresholds: 0.1 degree.
const THRESHOLD: f64 = 0.1 * std::f64::consts::PI / 180.0;

/// Tolerances against the Decimal reference (the same minimum computed two ways), each about
/// 10 times the worst difference on the fixture (printed by `matches_reference`).
/// Axis angle, radians.
const TOL_AXIS: f64 = 1e-9;
/// Distance of the reference foot point from the fitted axis and the radius difference at the
/// reference point, relative to the reference radius.
const TOL_LEN_REL: f64 = 1e-8;
/// Half-angle, radians.
const TOL_ALPHA: f64 = 1e-9;
/// Residual RMS, relative; the plane RMS uses the same.
const TOL_RMS_REL: f64 = 1e-10;

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

#[derive(Debug)]
struct Case {
    name: String,
    fields: Vec<(String, String)>,
}

impl Case {
    fn get(&self, k: &str) -> Option<Vec<f64>> {
        self.fields.iter().find(|(key, _)| key == k).map(|(_, v)| hexes(v))
    }
    fn v3(&self, k: &str) -> [f64; 3] {
        let v = self.get(k).unwrap();
        [v[0], v[1], v[2]]
    }
    fn one(&self, k: &str) -> f64 {
        self.get(k).unwrap()[0]
    }
    fn text(&self, k: &str) -> Option<&str> {
        self.fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("case ") {
            cur = Some(Case { name: rest.split_whitespace().next().unwrap().to_string(), fields: Vec::new() });
        } else if l == "end" {
            out.push(cur.take().unwrap());
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            cur.as_mut().unwrap().fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn options(c: &Case) -> ConeOptions {
    ConeOptions {
        fixed_half_angle: c.get("fixed_alpha").map(|v| v[0]),
        near_zero: THRESHOLD,
        near_ninety: THRESHOLD,
        ..ConeOptions::default()
    }
}

fn params() -> FitParams {
    FitParams::new(Method::LeastSquares)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Angle between two oriented directions, radians (test-side std math).
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

/// Distance from `q` to the line through `p` with unit direction `d`.
fn line_dist(p: [f64; 3], d: [f64; 3], q: [f64; 3]) -> f64 {
    norm(cross(sub(q, p), d))
}

fn run(c: &Case) -> Result<ConeFit, FitError> {
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    fit_cone(&pts, &params(), &options(c), Some(SEED))
}

fn outcomes() -> &'static [(Case, Result<ConeFit, FitError>)] {
    static ALL: OnceLock<Vec<(Case, Result<ConeFit, FitError>)>> = OnceLock::new();
    ALL.get_or_init(|| {
        parse()
            .into_iter()
            .map(|c| {
                let r = run(&c);
                (c, r)
            })
            .collect()
    })
}

/// REQ-001 and the fixed half-angle: every fit with a reference matches the Decimal
/// least-squares cone (axis, a point on it, the radius there, the half-angle, the apex and the
/// residual RMS) and is not flagged.
#[test]
fn matches_reference() {
    let mut worst = [0.0f64; 6];
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.get("ref_axis").is_some()) {
        let f = r.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let ref_r = c.one("ref_r");
        let ref_p = c.v3("ref_point");
        // The fitted radius at the reference point's axial position.
        let r_at = f.radius + dot(sub(ref_p, f.point), f.axis) * f.half_angle.tan();
        let (apex, d, h) = match f.result.feature {
            FittedFeature::Cone { apex, d, half_angle } => (apex, d, half_angle),
            ref other => panic!("{other:?}"),
        };
        assert_eq!((d, h), (f.axis, f.half_angle), "{}", c.name);
        let errs = [
            angle(d, c.v3("ref_axis")),
            line_dist(f.point, d, ref_p) / ref_r,
            (r_at - ref_r).abs() / ref_r,
            (h - c.one("ref_alpha")).abs(),
            norm(sub(apex, c.v3("ref_apex"))) / (ref_r / c.one("ref_alpha").tan()),
            (f.result.stats.rms - c.one("ref_rms")).abs() / c.one("ref_rms"),
        ];
        let tols = [TOL_AXIS, TOL_LEN_REL, TOL_LEN_REL, TOL_ALPHA, TOL_LEN_REL, TOL_RMS_REL];
        for (k, ((&e, &t), w)) in errs.iter().zip(&tols).zip(worst.iter_mut()).enumerate() {
            assert!(e <= t, "{}: error {k} = {e:e} above {t:e}", c.name);
            if e > *w {
                *w = e;
            }
        }
        let pr = c.one("plane_rms");
        assert!((f.plane_rms - pr).abs() <= TOL_RMS_REL * pr, "{}: plane_rms {} vs {pr}", c.name, f.plane_rms);
        assert!(!f.local_optimum_only, "{}: local_optimum_only", c.name);
        assert!(!f.suspect_axis, "{}: suspect_axis (ratio {:e})", c.name, f.rms_ratio);
        assert!(f.result.diagnostics.cond < ConeOptions::default().cond_limit / 4.0, "{}", c.name);
        n += 1;
    }
    eprintln!(
        "worst axis {:e} rad, point {:e}, radius {:e}, alpha {:e} rad, apex {:e}, rms {:e}",
        worst[0], worst[1], worst[2], worst[3], worst[4], worst[5]
    );
    assert_eq!(n, 29);
}

fn encode_err(e: &FitError) -> String {
    match e {
        FitError::TooFewPoints { n, need } => format!("too_few_points n={n} need={need}"),
        FitError::Coplanar { n, rank, .. } => format!("coplanar n={n} rank={rank}"),
        e => format!("{e:?}"),
    }
}

/// The fixed half-angle is not a parameter: the fitted half-angle is the constraint bit for
/// bit, the parameters are `a, b, u, v, r` and the Jacobian has rank 5.
#[test]
fn fixed_half_angle_holds() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.get("fixed_alpha").is_some()) {
        let f = r.as_ref().unwrap();
        assert_eq!(f.half_angle.to_bits(), c.one("fixed_alpha").to_bits(), "{}", c.name);
        assert_eq!(f.param_names, ["a", "b", "u", "v", "r"]);
        assert_eq!(f.result.diagnostics.rank, 5);
        assert!(f.starts.iter().all(|s| s.len() == 5));
        n += 1;
    }
    assert_eq!(n, 2);
}

/// AC-3 and REQ-003: a 0.01 degree cone (near-cylinder), an 89.99 degree cone (near-plane)
/// and a band 3e-4 radii long (a short axial extent) are each refused with `IllPosedCone`
/// carrying the half-angle and the condition number, code `ill_conditioned`; no fit is
/// returned, also not through the `Fit` trait (default thresholds 0.1 degree).
#[test]
fn ac3_ill_posed_cones_are_refused() {
    let limit = ConeOptions::default().cond_limit;
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect").is_some_and(|e| e.starts_with("ill_posed"))) {
        let Err(FitError::IllPosedCone { half_angle, cond }) = r else {
            panic!("{}: {r:?}", c.name);
        };
        let e = r.as_ref().unwrap_err();
        assert_eq!(e.code(), "ill_conditioned");
        assert!(e.to_string().contains("half-angle"));
        let cond = cond.unwrap();
        let near_zero = *half_angle <= THRESHOLD;
        let near_ninety = *half_angle >= std::f64::consts::FRAC_PI_2 - THRESHOLD;
        let high_cond = cond > limit;
        let why = c.text("expect").unwrap().strip_prefix("ill_posed ").unwrap();
        match why {
            "near_zero" => assert!(near_zero, "{}: {half_angle:e}", c.name),
            "near_ninety_or_cond" => assert!(near_ninety || high_cond, "{}: {half_angle:e} {cond:e}", c.name),
            "cond" => assert!(high_cond && !near_zero && !near_ninety, "{}: {half_angle:e} {cond:e}", c.name),
            other => panic!("{other}"),
        }
        // The planted half-angle is within the threshold where the label says so.
        let truth = c.one("truth_alpha");
        assert!((*half_angle - truth).abs() < THRESHOLD || why == "cond", "{}", c.name);
        let pts_v = c.get("points").unwrap();
        let t = ConeLs.fit(&PointsView::new(&pts_v, None).unwrap(), &params(), Some(SEED));
        assert_eq!(t.map(|_| ()).unwrap_err().code(), "ill_conditioned", "{}", c.name);
        n += 1;
    }
    assert_eq!(n, 3);
}

/// REQ-003 with the thresholds lowered: the near-cylinder cone of AC-3 is then a fit (the
/// apex-free parameters stay well conditioned near 0), so the refusal above is the threshold's.
#[test]
fn near_cylinder_fits_below_a_lower_threshold() {
    let (c, _) = outcomes().iter().find(|(c, _)| c.name == "ha-0.01").unwrap();
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    let o = ConeOptions { near_zero: THRESHOLD / 100.0, ..ConeOptions::default() };
    let f = fit_cone(&pts, &params(), &o, Some(SEED)).unwrap();
    assert!((f.half_angle - c.one("truth_alpha")).abs() < 1e-4, "{:e}", f.half_angle);
    // Its apex is far away (radius / tan(alpha), tens of metres): not reported.
    assert!(f.apex_distance > 10.0 && f.apex.is_none(), "{:e}", f.apex_distance);
}

/// Too few points and coplanar data are refused with the contract's typed errors.
#[test]
fn error_paths() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| {
        let e = c.text("expect").unwrap();
        e != "start" && !e.starts_with("ill_posed")
    }) {
        let e = r.as_ref().map(|_| ()).unwrap_err();
        assert_eq!(encode_err(e), c.text("expect").unwrap(), "{}", c.name);
        n += 1;
    }
    assert_eq!(n, 2);
}

/// Rule 1: contract constraints are refused (the cone's fixed half-angle is an option), and so
/// are invalid options.
#[test]
fn invalid_params_are_refused() {
    let (c, _) = outcomes().iter().find(|(c, _)| c.name == "cone-20-360-1").unwrap();
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    let mut p = params();
    p.constraints.push(fid_fit::contract::Constraint::FixedRadius(0.02));
    assert_eq!(
        fit_cone(&pts, &p, &ConeOptions::default(), Some(SEED)),
        Err(FitError::InvalidParams { field: "constraints" })
    );
    let o = ConeOptions { fixed_half_angle: Some(2.0), ..ConeOptions::default() };
    assert_eq!(
        fit_cone(&pts, &params(), &o, Some(SEED)),
        Err(FitError::InvalidParams { field: "cone.fixed_half_angle" })
    );
    let o = ConeOptions { cond_limit: f64::NAN, ..ConeOptions::default() };
    assert_eq!(fit_cone(&pts, &params(), &o, Some(SEED)), Err(FitError::InvalidParams { field: "cone.cond_limit" }));
}

/// REQ-005: the apex is reported only when its distance from the reference point is at most
/// `apex_report_limit_m`; nothing else in the result depends on the limit.
#[test]
fn apex_reported_only_within_limit() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "cone-5-360-1").unwrap();
    let f = r.as_ref().unwrap();
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    let with = |limit: f64| {
        let o = ConeOptions { apex_report_limit_m: limit, ..options(c) };
        fit_cone(&pts, &params(), &o, Some(SEED)).unwrap()
    };
    let near = with(f.apex_distance / 2.0);
    let far = with(f64::INFINITY);
    let exact = with(f.apex_distance);
    assert_eq!(near.apex, None);
    let FittedFeature::Cone { apex, .. } = f.result.feature else { panic!() };
    assert_eq!(far.apex, Some(apex));
    assert_eq!(exact.apex, Some(apex));
    assert_eq!(near.result, far.result);
    assert_eq!((near.point, near.radius, near.half_angle), (far.point, far.radius, far.half_angle));
    // The apex is `apex_distance` behind the reference point along the axis.
    let back = sub(f.point, apex);
    assert!((norm(back) - f.apex_distance).abs() <= 1e-12 * f.apex_distance);
    assert!(angle(back, f.axis) < 1e-12);
    assert!((f.apex_distance - f.radius / f.half_angle.tan()).abs() <= 1e-12 * f.apex_distance);
}

/// REQ-002 and REQ-005: every fit runs INIT's 8 starts, reports one outcome per start, the
/// selection, the diagnostics and the seed; the same seed is bit-identical; another seed moves
/// only the random starts.
#[test]
fn multistart_record_and_diagnostics() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "cone-45-120-1").unwrap();
    let f = r.as_ref().unwrap();
    assert_eq!(f.starts.len(), 8);
    assert_eq!(f.per_start.len(), 8);
    assert!(f.per_start.iter().all(|s| matches!(s, StartSummary::Converged { .. })));
    assert_eq!(f.param_names, ["a", "b", "u", "v", "r", "alpha"]);
    let d = &f.result.diagnostics;
    assert_eq!(d.rank, 6);
    assert!(d.iterations > 0 && d.cond >= 1.0 && d.grad_norm.is_finite());
    assert_eq!(f.result.algorithm.to_string(), "fit.cone.ls@1.0.0");
    assert!(f.result.stats.rms < f.plane_rms);
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    assert_eq!(&fit_cone(&pts, &params(), &options(c), Some(SEED)).unwrap(), f);
    let other = fit_cone(&pts, &params(), &options(c), Some(7)).unwrap();
    assert_ne!(other.starts, f.starts);
    assert!(angle(other.axis, f.axis) < 1e-9);
    assert!((other.half_angle - f.half_angle).abs() < 1e-9);
}

/// Rule 10 (fail closed): through the `Fit` trait (default options) a clean fit is returned
/// unchanged and a flagged one is `FlaggedFit`; every error is the same error.
#[test]
fn trait_matches_fit_cone() {
    for (c, r) in outcomes().iter().filter(|(c, _)| c.get("fixed_alpha").is_none()) {
        let pts_v = c.get("points").unwrap();
        let pts = PointsView::new(&pts_v, None).unwrap();
        let t = ConeLs.fit(&pts, &params(), Some(SEED));
        match r {
            Ok(f) if f.local_optimum_only || f.suspect_axis => {
                assert_eq!(t.unwrap_err().code(), "flagged_fit", "{}", c.name)
            }
            Ok(f) => assert_eq!(t.as_ref(), Ok(&f.result), "{}", c.name),
            Err(e) => assert_eq!(t.as_ref(), Err(e), "{}", c.name),
        }
    }
}

/// The residuals are signed (positive outside): moving one point outward along its radial
/// direction raises its residual.
#[test]
fn residuals_are_signed_outward() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "cone-20-360-1").unwrap();
    let f = r.as_ref().unwrap();
    let mut pts_v = c.get("points").unwrap();
    let q = [pts_v[0], pts_v[1], pts_v[2]];
    let w = sub(q, f.point);
    let t = dot(w, f.axis);
    let perp = sub(w, [t * f.axis[0], t * f.axis[1], t * f.axis[2]]);
    let rho = norm(perp);
    let e = (rho - f.radius) * f.half_angle.cos() - t * f.half_angle.sin();
    assert!((f.result.residuals[0] - e).abs() < 1e-9, "{} vs {e}", f.result.residuals[0]);
    for (x, pk) in pts_v.iter_mut().zip(perp) {
        *x += 1e-4 * pk / rho;
    }
    let g = fit_cone(&PointsView::new(&pts_v, None).unwrap(), &params(), &options(c), Some(SEED)).unwrap();
    assert!(g.result.residuals[0] > f.result.residuals[0] + 5e-5);
}

/// Spec card (REQ-KRN.LSQ-009, KRN.MAN.CARD) and the command data row (W2, dry run R1, D0).
#[test]
fn card_and_command_row() {
    let card = parse_card(CARD).unwrap();
    assert_eq!(card.id.to_string(), "fit.cone.ls@1.0.0");
    assert_eq!(card.determinism, "D0");
    assert_eq!(card.input_arity, InputArity::exactly(1));
    let report = check(&[ConeLs.algorithm_id()], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    let row = cone_command_row().unwrap();
    row.validate().unwrap();
    assert_eq!(row.command.as_str(), "fit.ls_cone@1");
    assert_eq!((row.side_effect, row.dry_run, row.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
    for name in ["cone.fixed_half_angle", "cone.near_zero", "cone.near_ninety", "cone.apex_report_limit_m"] {
        assert!(row.params.iter().any(|f| f.name == name), "{name}");
    }
    for name in ["cone.point", "cone.radius", "cone.apex_reported", "multistart.local_optimum_only"] {
        assert!(row.result.iter().any(|f| f.name == name), "{name}");
    }
}

fn put(h: &mut Sha256Hasher, xs: &[f64]) {
    for x in xs {
        h.update(&x.to_bits().to_le_bytes());
    }
}

/// AC-4: output-bit pin of every case (feature, reference point and radius, residuals,
/// statistics, diagnostics, starts, per-start outcomes, flags and every error), the cones 1 km
/// from the origin included, recorded from a local x86-64 run and checked by CI on x86-64
/// Linux and Windows, ARM64 macOS and wasm32 under wasmtime.
const OUTPUT_PIN: &str = "sha256:8435745da39c0b423cc8825f59f337c04fa18a9facb0d8b617457faea2259200";

#[test]
fn ac4_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for (c, r) in outcomes() {
        h.update(c.name.as_bytes());
        match r {
            Ok(f) => {
                let FittedFeature::Cone { apex, d, half_angle } = f.result.feature else { panic!() };
                put(&mut h, &apex);
                put(&mut h, &d);
                put(&mut h, &[half_angle]);
                put(&mut h, &f.point);
                put(&mut h, &[f.radius, f.apex_distance]);
                h.update(&[u8::from(f.apex.is_some())]);
                put(&mut h, &f.result.residuals);
                let s = &f.result.stats;
                put(&mut h, &[s.rms, s.max, s.min, s.mean, s.std_dev]);
                let g = &f.result.diagnostics;
                put(&mut h, &[g.grad_norm, g.cond]);
                h.update(&(g.rank as u64).to_le_bytes());
                h.update(&(g.iterations as u64).to_le_bytes());
                h.update(g.stop.as_str().as_bytes());
                for st in &f.starts {
                    put(&mut h, st);
                }
                for o in &f.per_start {
                    match o {
                        StartSummary::Converged { objective, params } => {
                            put(&mut h, &[*objective]);
                            put(&mut h, params);
                        }
                        StartSummary::Failed(e) => h.update(format!("{e:?}").as_bytes()),
                    }
                }
                h.update(&f.best_index.to_le_bytes());
                h.update(&[u8::from(f.local_optimum_only), u8::from(f.suspect_axis)]);
                put(&mut h, &[f.plane_rms, f.rms_ratio]);
            }
            Err(e) => h.update(encode_err(e).as_bytes()),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}

#[test]
fn explore_hard() {
    for (c, r) in outcomes() {
        match r {
            Ok(f) => eprintln!(
                "{}: lo {} sus {} weak {} ratio {:.3} u_axis {:e} cond {:.1}",
                c.name, f.local_optimum_only, f.suspect_axis, f.weak_axis, f.rms_ratio, f.uncertainty.axis_rad, f.result.diagnostics.cond
            ),
            Err(e) => eprintln!("{}: {}", c.name, encode_err(e)),
        }
    }
}
