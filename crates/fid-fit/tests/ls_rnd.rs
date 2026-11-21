//! Acceptance criteria of spec node KRN.LSQ.RND that do not need the RDL or the command layer
//! (AC-3, AC-4, the fit part of AC-5) and the requirements they exercise (REQ-001 to -006).
//!
//! Expected values come from `fixtures/fit/make_rnd_fixtures.py`: the least-squares circle or
//! sphere of each rounded point set computed by Gauss-Newton in 60-digit Decimal (no libm,
//! independent of this crate), and hand-labelled error cases. `OUTPUT_PIN` is a regression pin
//! of this crate's own output (not an oracle); CI checks it on x86-64 Linux and Windows, ARM64
//! macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{Constraint, Fit, FitError, FitParams, FitResult, FittedFeature, Method, PointsView, Weights};
use fid_fit::ls_lin::projection_frame;
use fid_fit::ls_rnd::{command_rows, default_params, Circle2Ls, Circle3Ls, SphereLs, DEFAULT_MIN_COVERAGE};
use fid_kci::{check, parse_card, InputArity};
use fid_testkit::Sha256Hasher;
use fid_types::{Determinism, SideEffect};
use std::sync::OnceLock;

const FIXTURE: &str = include_str!("../../../fixtures/fit/rnd-partial-v1/cases.txt");
const CARDS: [&str; 3] = [
    include_str!("../cards/fit.circle2.ls@1.0.0.toml"),
    include_str!("../cards/fit.circle3.ls@1.0.0.toml"),
    include_str!("../cards/fit.sphere.ls@1.0.0.toml"),
];

/// Tolerances against the Decimal reference (the same minimum computed two ways), each about 10
/// times the worst difference on the fixture (printed by `ac3_matches_reference`). Relative to
/// the reference radius: centre distance and radius difference (worst 7.8e-11 and 7.6e-11).
const TOL_LEN_REL: f64 = 1e-9;
/// Normal angle of the 3D circle, radians (worst 1.7e-12).
const TOL_NORMAL: f64 = 2e-11;
/// Residual RMS, relative (worst 5.3e-13).
const TOL_RMS_REL: f64 = 1e-11;
/// AC-3's literal bound on the relative radius error of converged cases [unverified].
const AC3_RADIUS_REL: f64 = 1e-6;
// The radius tolerance is within AC-3's bound, so meeting it meets AC-3.
const _: () = assert!(TOL_LEN_REL <= AC3_RADIUS_REL);

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

#[derive(Debug)]
struct Case {
    name: String,
    kind: String,
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
            let w: Vec<&str> = rest.split_whitespace().collect();
            assert_eq!(w[1], "kind");
            cur = Some(Case { name: w[0].to_string(), kind: w[2].to_string(), fields: Vec::new() });
        } else if l == "end" {
            out.push(cur.take().unwrap());
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            cur.as_mut().unwrap().fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn params(c: &Case) -> FitParams {
    let mut p = default_params();
    if let Some(r) = c.get("fixed_r") {
        p.constraints.push(Constraint::FixedRadius(r[0]));
    }
    if let Some(v) = c.get("plane") {
        p.constraints.push(Constraint::ProjectionPlane { p: [v[0], v[1], v[2]], n: [v[3], v[4], v[5]] });
    }
    if let Some(v) = c.get("min_coverage") {
        p.thresholds.min_coverage = v[0];
    }
    if let Some(t) = c.text("max_iter") {
        p.nlls.max_iter = t.parse().unwrap();
    }
    p
}

fn fitter(kind: &str) -> &'static dyn Fit {
    match kind {
        "circle2" => &Circle2Ls,
        "circle3" => &Circle3Ls,
        "sphere" => &SphereLs,
        k => panic!("{k}"),
    }
}

fn run_with(c: &Case, p: &FitParams) -> Result<FitResult, FitError> {
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    fitter(&c.kind).fit(&pts, p, None)
}

fn outcomes() -> &'static [(Case, Result<FitResult, FitError>)] {
    static ALL: OnceLock<Vec<(Case, Result<FitResult, FitError>)>> = OnceLock::new();
    ALL.get_or_init(|| {
        parse()
            .into_iter()
            .map(|c| {
                let r = run_with(&c, &params(&c));
                (c, r)
            })
            .collect()
    })
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

/// Angle between two lines, radians (test-side std math).
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b).abs())
}

/// `(3D centre, normal, radius)` of a fitted feature; circle2's plane coordinates are mapped to
/// 3D through the projection frame of `params`.
fn geometry(c: &Case, f: &FittedFeature) -> ([f64; 3], Option<[f64; 3]>, f64) {
    match *f {
        FittedFeature::Circle2 { c: uv, r } => {
            let (p0, e1, e2) = projection_frame(&params(c)).unwrap();
            let x = [0, 1, 2].map(|k| p0[k] + uv[0] * e1[k] + uv[1] * e2[k]);
            (x, None, r)
        }
        FittedFeature::Circle3 { c, n, r } => (c, Some(n), r),
        FittedFeature::Sphere { c, r } => (c, None, r),
        ref other => panic!("{other:?}"),
    }
}

/// REQ-001 to -003, -006 and AC-3's converging cases: every fit matches the Decimal
/// least-squares element within the tolerances, and its radius within AC-3's 1e-6 relative.
#[test]
fn ac3_matches_reference() {
    let mut worst = [0.0f64; 4];
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect") == Some("start")) {
        let f = r.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let (cen, nrm, rad) = geometry(c, &f.feature);
        let rr = c.one("ref_r");
        let dc = norm(sub(cen, c.v3("ref_c"))) / rr;
        let dr = (rad - rr).abs() / rr;
        let dn = match nrm {
            Some(nv) => angle(nv, c.v3("ref_n")),
            None => 0.0,
        };
        let drms = (f.stats.rms - c.one("ref_rms")).abs() / c.one("ref_rms");
        assert!(dc <= TOL_LEN_REL, "{}: centre {dc:e}", c.name);
        assert!(dr <= TOL_LEN_REL, "{}: radius {dr:e}", c.name);
        assert!(dn <= TOL_NORMAL, "{}: normal {dn:e}", c.name);
        assert!(drms <= TOL_RMS_REL, "{}: rms {drms:e}", c.name);
        if c.get("fixed_r").is_some() {
            assert_eq!(rad, c.one("fixed_r"), "{}: REQ-006 fixed radius", c.name);
        }
        for (w, x) in worst.iter_mut().zip([dc, dr, dn, drms]) {
            *w = w.max(x);
        }
        n += 1;
    }
    println!("worst centre {:e} radius {:e} normal {:e} rms {:e}", worst[0], worst[1], worst[2], worst[3]);
    assert_eq!(n, 25);
}

/// The planted geometry is recovered to the noise level (a sanity check of the fixture, not of
/// the optimum): radius within 1e-3 relative of the truth.
#[test]
fn fits_are_near_the_planted_geometry() {
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect") == Some("start")) {
        let (_, _, rad) = geometry(c, &r.as_ref().unwrap().feature);
        let tr = c.one("truth_r");
        assert!((rad - tr).abs() / tr < 1e-3, "{}", c.name);
    }
}

fn encode_err(e: &FitError) -> String {
    match e {
        FitError::TooFewPoints { n, need } => format!("too_few_points n={n} need={need}"),
        FitError::Collinear { n, rank, .. } => format!("collinear n={n} rank={rank}"),
        FitError::InsufficientCoverage { n, .. } => format!("insufficient_coverage n={n}"),
        FitError::Coplanar { n, rank, .. } => format!("coplanar n={n} rank={rank}"),
        FitError::IllConditioned { .. } => "ill_conditioned".to_string(),
        FitError::NotConverged { .. } => "not_converged".to_string(),
        FitError::InvalidParams { field } => format!("invalid_params {field}"),
        e => format!("{e:?}"),
    }
}

/// AC-3's 20-degree cases (REQ-005) and AC-5's fit part: the 20-degree arc and cap return
/// `InsufficientCoverage` with the coverage, the threshold and the point count (the 20-degree
/// cap measures about 21 degrees about its algebraic centre, which the short cap misplaces; it
/// is still below the 30-degree default). Two points for a sphere are `TooFewPoints`; collinear
/// points for a 3D circle are `Collinear`.
#[test]
fn ac3_ac5_error_paths() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect") != Some("start")) {
        let e = r.as_ref().map(|_| ()).unwrap_err();
        let code = encode_err(e);
        if c.text("expect") == Some("refused") {
            // Hand-labelled "must be refused": any of the refusals of a weakly determined fit.
            let ok = matches!(
                e,
                FitError::InsufficientCoverage { .. } | FitError::Coplanar { .. } | FitError::IllConditioned { .. }
            );
            assert!(ok, "{}: {code}", c.name);
            println!("{}: {code}", c.name);
        } else {
            assert_eq!(code, c.text("expect").unwrap(), "{}", c.name);
        }
        if let (FitError::InsufficientCoverage { coverage, need, .. }, true) = (e, c.name.ends_with("-20")) {
            assert_eq!(*need, DEFAULT_MIN_COVERAGE);
            let deg = coverage.to_degrees();
            assert!(deg > 15.0 && deg < 25.0, "{}: coverage {deg}", c.name);
        }
        n += 1;
    }
    assert_eq!(n, 14);
}

/// Review of #417: the fit-level condition limit is `thresholds.cond_max` (default 1e8); the
/// 0.02-degree arc of `c2-cond` is fitted when it is raised to the solver's own limit.
#[test]
fn cond_limit_is_a_parameter() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "c2-cond").unwrap();
    assert!(matches!(r, Err(FitError::IllConditioned { cond: Some(k) }) if *k > 1e8), "{r:?}");
    let mut p = params(c);
    p.thresholds.cond_max = 1e12;
    let f = run_with(c, &p).unwrap();
    assert!(f.diagnostics.cond > 1e8);
}

/// REQ-005: the coverage threshold is the caller's; lowered to 10 degrees, the 20-degree cases
/// are fitted.
#[test]
fn coverage_threshold_is_a_parameter() {
    for (c, _) in outcomes().iter().filter(|(c, _)| c.name.ends_with("-20")) {
        let mut p = params(c);
        p.thresholds.min_coverage = 10f64.to_radians();
        let f = run_with(c, &p).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let (_, _, rad) = geometry(c, &f.feature);
        let tr = c.one("truth_r");
        assert!((rad - tr).abs() / tr < 0.05, "{}", c.name);
    }
}

/// Rule 1: constraints, method and weights outside the leaf's are refused.
#[test]
fn unsupported_params_are_refused() {
    let (c, _) = outcomes().iter().find(|(c, _)| c.name == "sp-360-1").unwrap();
    let mut p = params(c);
    p.constraints.push(Constraint::FixedAxisDirection([0.0, 0.0, 1.0]));
    assert!(matches!(run_with(c, &p), Err(FitError::InvalidParams { field: "constraints" })));
    let mut p = params(c);
    p.constraints.push(Constraint::ProjectionPlane { p: [0.0; 3], n: [0.0, 0.0, 1.0] });
    assert!(matches!(run_with(c, &p), Err(FitError::InvalidParams { field: "constraints" })));
    let mut p = params(c);
    p.weights = Weights::PerPoint(vec![1.0; 200]);
    assert!(matches!(run_with(c, &p), Err(FitError::InvalidParams { field: "weights" })));
    let mut p = params(c);
    p.method = Method::MinimumZone;
    assert!(matches!(run_with(c, &p), Err(FitError::InvalidParams { field: "method" })));
}

/// REQ-004 and the contract: results carry the solver's diagnostics and the algorithm ID; the
/// fits are deterministic and ignore the seed.
#[test]
fn diagnostics_and_determinism() {
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect") == Some("start")) {
        let f = r.as_ref().unwrap();
        let d = &f.diagnostics;
        let free = match (c.kind.as_str(), c.get("fixed_r").is_some()) {
            ("circle2", fixed) => 3 - usize::from(fixed),
            ("sphere", fixed) => 4 - usize::from(fixed),
            (_, fixed) => 6 - usize::from(fixed),
        };
        assert_eq!(d.rank, free, "{}", c.name);
        assert!(d.iterations > 0 && d.cond > 0.5 && d.grad_norm.is_finite(), "{}: {d:?}", c.name);
        assert_eq!(f.algorithm.to_string(), format!("fit.{}.ls@1.0.0", c.kind));
        assert_eq!(f.residuals.len(), 200);
        let pts_v = c.get("points").unwrap();
        let pts = PointsView::new(&pts_v, None).unwrap();
        assert_eq!(&fitter(&c.kind).fit(&pts, &params(c), Some(7)).unwrap(), f, "{}", c.name);
    }
}

/// Spec cards (REQ-KRN.LSQ-009, KRN.MAN.CARD) and the command data rows (W2, dry run R1, D0).
#[test]
fn cards_and_command_rows() {
    let ids = [Circle2Ls.algorithm_id(), Circle3Ls.algorithm_id(), SphereLs.algorithm_id()];
    let mut card_ids = Vec::new();
    for text in CARDS {
        let card = parse_card(text).unwrap();
        assert_eq!(card.determinism, "D0");
        assert_eq!(card.input_arity, InputArity::exactly(1));
        card_ids.push(card.id);
    }
    let report = check(&ids, &card_ids);
    assert!(report.is_clean(), "{report:?}");
    let rows = command_rows().unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.command.as_str()).collect();
    assert_eq!(names, ["fit.ls_circle2@1", "fit.ls_circle3@1", "fit.ls_sphere@1"]);
    for row in &rows {
        row.validate().unwrap();
        assert_eq!((row.side_effect, row.dry_run, row.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
    }
}

fn put(h: &mut Sha256Hasher, xs: &[f64]) {
    for x in xs {
        h.update(&x.to_bits().to_le_bytes());
    }
}

/// AC-4: output-bit pin of every case (feature, residuals, statistics, diagnostics and every
/// error), recorded from a local x86-64 run and checked by CI on x86-64 Linux and Windows,
/// ARM64 macOS and wasm32 under wasmtime. Re-recorded for the review of #417 (first value
/// `sha256:214dded4e3be8427ac46c38a4ac1ae4597a9e61a23b19de3c5628c19e0ac99fd`): sphere coverage
/// about the fitted centre, the planarity and condition checks, and the new error cases.
const OUTPUT_PIN: &str = "sha256:5721864dd9314081515f6459c4a0e20dbc7bd7977bec103c2fe06250b564a62e";

#[test]
fn ac4_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for (c, r) in outcomes() {
        h.update(c.name.as_bytes());
        match r {
            Ok(f) => {
                match &f.feature {
                    FittedFeature::Circle2 { c, r } => {
                        put(&mut h, c);
                        put(&mut h, &[*r]);
                    }
                    FittedFeature::Circle3 { c, n, r } => {
                        put(&mut h, c);
                        put(&mut h, n);
                        put(&mut h, &[*r]);
                    }
                    FittedFeature::Sphere { c, r } => {
                        put(&mut h, c);
                        put(&mut h, &[*r]);
                    }
                    other => panic!("{other:?}"),
                }
                put(&mut h, &f.residuals);
                let s = &f.stats;
                put(&mut h, &[s.rms, s.max, s.min, s.mean, s.std_dev]);
                let g = &f.diagnostics;
                put(&mut h, &[g.grad_norm, g.cond]);
                h.update(&(g.rank as u64).to_le_bytes());
                h.update(&(g.iterations as u64).to_le_bytes());
                h.update(g.stop.as_str().as_bytes());
            }
            Err(e) => h.update(format!("{e:?}").as_bytes()),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
