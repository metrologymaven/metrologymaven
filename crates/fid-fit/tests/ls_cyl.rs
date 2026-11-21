//! Acceptance criteria of spec node KRN.LSQ.AXIS.CYL that do not need the RDL (AC-3, AC-4)
//! and the requirements they exercise (REQ-001, -002, -004 partly, -005), plus the wrong-axis
//! check of `ls_cyl` rule 8.
//!
//! Expected values come from `fixtures/fit/make_axis_cyl_fixtures.py`: the least-squares
//! cylinder of each rounded point set computed by Gauss-Newton in 60-digit Decimal (no libm,
//! independent of this crate), and hand-labelled flags. `OUTPUT_PIN` is a regression pin of this
//! crate's own output (not an oracle); CI checks it on x86-64 Linux and Windows, ARM64 macOS and
//! wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{Constraint, Fit, FitError, FitParams, FittedFeature, Method, PointsView};
use fid_fit::init::{AxialStart, AxisFrame, AGREEMENT_TOL};
use fid_fit::ls_cyl::{cylinder_command_row, fit_cylinder, fit_cylinder_from, CylinderFit, CylinderLs};
use fid_kci::{check, parse_card, InputArity};
use fid_solv::mstart::{StartSpec, StartSummary};
use fid_testkit::Sha256Hasher;
use fid_types::{Determinism, SideEffect};
use std::sync::OnceLock;

const FIXTURE: &str = include_str!("../../../fixtures/fit/axis-cyl-v1/cases.txt");
const CARD: &str = include_str!("../cards/fit.cylinder.ls@1.0.0.toml");
const SEED: u64 = 20_261_121;

/// Tolerances against the Decimal reference (the same minimum computed two ways). Each is
/// about 10 times the worst difference on the fixture, `wrong-start` excluded (printed by
/// `matches_reference`).
/// Axis angle, radians (worst 2.2e-12).
const TOL_AXIS: f64 = 2e-11;
/// Distance of the reference foot point from the fitted axis and radius difference, relative
/// to the reference radius (worst 1.7e-10 and 1.6e-10).
const TOL_LEN_REL: f64 = 2e-9;
/// Factor on the axis and length tolerances for `wrong-start`, whose wrong local minimum is
/// less well conditioned (worst 2.9e-9 rad and 1.6e-9).
const WRONG_START_FACTOR: f64 = 1000.0;
/// Residual RMS, relative (worst 6.7e-13); the plane RMS uses the same.
const TOL_RMS_REL: f64 = 1e-11;

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

fn params(c: &Case) -> FitParams {
    let mut p = FitParams::new(Method::LeastSquares);
    if let Some(r) = c.get("fixed_r") {
        p.constraints.push(Constraint::FixedRadius(r[0]));
    }
    if c.get("fixed_dir").is_some() {
        p.constraints.push(Constraint::FixedAxisDirection(c.v3("fixed_dir")));
    }
    p
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

/// Distance from `q` to the line through `p` with unit direction `d`.
fn line_dist(p: [f64; 3], d: [f64; 3], q: [f64; 3]) -> f64 {
    norm(cross(sub(q, p), d))
}

fn cyl(f: &CylinderFit) -> ([f64; 3], [f64; 3], f64) {
    match f.result.feature {
        FittedFeature::Cylinder { p, d, r } => (p, d, r),
        ref other => panic!("{other:?}"),
    }
}

/// The one-start override of `wrong-start`: the oracle's start axis, point and radius, an
/// orthonormal frame (test-side std math) and a spec with one start.
fn override_start(c: &Case) -> AxialStart {
    let axis = c.v3("start_axis");
    let k = if axis[0].abs() <= axis[1].abs() && axis[0].abs() <= axis[2].abs() {
        [1.0, 0.0, 0.0]
    } else if axis[1].abs() <= axis[2].abs() {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let e1 = cross(axis, k);
    let n1 = norm(e1);
    let e1 = [e1[0] / n1, e1[1] / n1, e1[2] / n1];
    let e2 = cross(axis, e1);
    let r = c.one("start_r");
    let rms = r;
    AxialStart {
        primitive: fid_fit::contract::Primitive::Cylinder,
        frame: AxisFrame { point: c.v3("start_point"), axis, e1, e2 },
        spec: StartSpec {
            primary: vec![0.0, 0.0, 0.0, 0.0, r],
            half_widths: vec![0.0; 5],
            count: 1,
            seed: SEED,
            stream: 1,
            tolerances: vec![
                AGREEMENT_TOL,
                AGREEMENT_TOL,
                AGREEMENT_TOL * rms,
                AGREEMENT_TOL * rms,
                AGREEMENT_TOL * rms,
            ],
        },
        candidates: Vec::new(),
        selected: 0,
        rms_radius: rms,
        apex: None,
    }
}

fn run(c: &Case) -> Result<CylinderFit, FitError> {
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    if c.get("start_axis").is_some() {
        fit_cylinder_from(&pts, &params(c), &override_start(c))
    } else {
        fit_cylinder(&pts, &params(c), Some(SEED))
    }
}

fn outcomes() -> &'static [(Case, Result<CylinderFit, FitError>)] {
    static ALL: OnceLock<Vec<(Case, Result<CylinderFit, FitError>)>> = OnceLock::new();
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

fn flag(c: &Case) -> Option<&str> {
    c.text("flag")
}

/// REQ-001, REQ-004 (constraints) and AC-3's partial arcs: every fit with a reference matches
/// the Decimal least-squares cylinder; no unflagged case is flagged.
#[test]
fn matches_reference() {
    let mut worst = [0.0f64; 4];
    let mut n = 0;
    for (c, r) in
        outcomes().iter().filter(|(c, _)| c.get("ref_axis").is_some() && flag(c) != Some("never_confidently_wrong"))
    {
        let f = r.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let (p, d, rad) = cyl(f);
        let ref_r = c.one("ref_r");
        let errs = [
            angle(d, c.v3("ref_axis")),
            line_dist(p, d, c.v3("ref_point")) / ref_r,
            (rad - ref_r).abs() / ref_r,
            (f.result.stats.rms - c.one("ref_rms")).abs() / c.one("ref_rms"),
        ];
        // The wrong local minimum of `wrong-start` is less well conditioned than the others.
        let k = if c.name == "wrong-start" { WRONG_START_FACTOR } else { 1.0 };
        let tols = [k * TOL_AXIS, k * TOL_LEN_REL, k * TOL_LEN_REL, TOL_RMS_REL];
        for (k, ((&e, &t), w)) in errs.iter().zip(&tols).zip(worst.iter_mut()).enumerate() {
            assert!(e <= t, "{}: error {k} = {e:e} above {t:e}", c.name);
            if e > *w && c.name != "wrong-start" {
                *w = e;
            }
        }
        let pr = c.one("plane_rms");
        assert!((f.plane_rms - pr).abs() <= TOL_RMS_REL * pr, "{}: plane_rms {} vs {pr}", c.name, f.plane_rms);
        if flag(c).is_none() {
            assert!(!f.local_optimum_only, "{}: local_optimum_only", c.name);
            assert!(!f.suspect_axis, "{}: suspect_axis (ratio {:e})", c.name, f.rms_ratio);
        }
        n += 1;
    }
    eprintln!(
        "worst axis {:e} rad, point {:e}, radius {:e}, rms {:e} (relative)",
        worst[0], worst[1], worst[2], worst[3]
    );
    assert_eq!(n, 28);
}

/// REQ-004: a fixed radius is not a parameter (the radius is the constraint bit for bit), a
/// fixed axis direction keeps the direction (normalised) and leaves `u, v, r`.
#[test]
fn constraints_hold() {
    for (c, r) in outcomes() {
        let f = match r {
            Ok(f) => f,
            Err(_) => continue,
        };
        let (_, d, rad) = cyl(f);
        if let Some(fr) = c.get("fixed_r") {
            assert_eq!(rad.to_bits(), fr[0].to_bits(), "{}", c.name);
            assert_eq!(f.param_names, ["a", "b", "u", "v"]);
            assert_eq!(f.result.diagnostics.rank, 4);
        } else if c.get("fixed_dir").is_some() {
            let fd = c.v3("fixed_dir");
            assert!(angle(d, fd) <= 1e-15, "{}", c.name);
            assert_eq!(f.param_names, ["u", "v", "r"]);
            assert_eq!(f.starts.len(), 1);
            assert_eq!(f.result.diagnostics.rank, 3);
        }
    }
}

/// AC-3 and rule 8 on short, noisy 30-degree arcs (0.2 radii long, 40 to 100 micrometres of
/// noise), where INIT's start can be nearly perpendicular to the true axis and the starts can
/// land on different minima: no fit is confidently wrong. Each case either matches the Decimal
/// reference, or sets `local_optimum_only` or `suspect_axis`, or is refused with
/// `implausible_fit`. Which of these each case ends in is this crate's behaviour, pinned by
/// `ac3_output_bits_pinned`; AC-3 needs at least one case with two plausible axes (distinct
/// converged minima) flagged `local_optimum_only`.
#[test]
fn ac3_short_arcs_never_confidently_wrong() {
    let mut counts = [0usize; 4];
    for (c, r) in outcomes().iter().filter(|(c, _)| flag(c) == Some("never_confidently_wrong")) {
        match r {
            Err(FitError::ImplausibleFit { rms, bound }) => {
                assert!(rms >= bound);
                counts[3] += 1;
            }
            // Every start ran out of iterations along the flat valley: a typed refusal too.
            Err(FitError::MultiStartFailed { starts: 8, .. }) => counts[3] += 1,
            Err(e) => panic!("{}: {e:?}", c.name),
            Ok(f) if f.local_optimum_only => {
                let distinct = f
                    .per_start
                    .iter()
                    .filter_map(|o| match o {
                        StartSummary::Converged { objective, .. } => Some(objective.to_bits()),
                        StartSummary::Failed(_) => None,
                    })
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                if distinct > 1 {
                    counts[0] += 1;
                }
            }
            Ok(f) if f.suspect_axis => counts[1] += 1,
            Ok(f) => {
                let (_, d, rad) = cyl(f);
                let a = angle(d, c.v3("ref_axis"));
                assert!(a <= 1e-6, "{}: unflagged fit {a:e} rad from the reference", c.name);
                assert!((rad - c.one("ref_r")).abs() <= 1e-6 * c.one("ref_r"), "{}", c.name);
                counts[2] += 1;
            }
        }
        // The reference least-squares cylinder itself is better than the plane.
        assert!(c.one("ref_rms") < c.one("plane_rms"), "{}", c.name);
    }
    eprintln!(
        "short arcs: flagged with distinct minima {}, suspect {}, matching {}, refused {}",
        counts[0], counts[1], counts[2], counts[3]
    );
    assert!(counts[0] >= 1, "AC-3: no short arc flagged local_optimum_only with distinct minima");
}

/// Rule 8: a fit that converged to a wrong axis (one start on the half cylinder's bisector,
/// perpendicular to the true axis) matches the Decimal local minimum from that start, and is
/// flagged `suspect_axis` though its single start trivially "agrees".
#[test]
fn wrong_axis_is_flagged() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "wrong-start").unwrap();
    let f = r.as_ref().unwrap();
    let (_, d, _) = cyl(f);
    assert!(angle(d, c.v3("truth_axis")) > 1.0, "converged near the true axis");
    assert!(!f.local_optimum_only);
    assert!(f.suspect_axis, "ratio {:e}", f.rms_ratio);
    assert!(f.rms_ratio > 0.1 && f.rms_ratio < 1.0, "ratio {:e}", f.rms_ratio);
    // The free fit of the same data finds the true axis and is not flagged.
    let pts_v = c.get("points").unwrap();
    let g = fit_cylinder(&PointsView::new(&pts_v, None).unwrap(), &params(c), Some(SEED)).unwrap();
    assert!(angle(cyl(&g).1, c.v3("truth_axis")) < 1e-3);
    assert!(!g.suspect_axis && g.rms_ratio < 0.01, "ratio {:e}", g.rms_ratio);
}

fn encode_err(e: &FitError) -> String {
    match e {
        FitError::TooFewPoints { n, need } => format!("too_few_points n={n} need={need}"),
        FitError::Coplanar { n, rank, .. } => format!("coplanar n={n} rank={rank}"),
        e => format!("{e:?}"),
    }
}

/// AC-4: 4 points are `TooFewPoints { n: 4, need: 5 }`; points on a circle are `Coplanar`
/// with a condition number (the contract's typed degeneracy, see the leaf's Open questions).
#[test]
fn ac4_error_paths() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect") != Some("start")) {
        let e = r.as_ref().map(|_| ()).unwrap_err();
        assert_eq!(encode_err(e), c.text("expect").unwrap(), "{}", c.name);
        if let FitError::Coplanar { cond, .. } = e {
            assert!(*cond > 1e9);
        }
        n += 1;
    }
    assert_eq!(n, 2);
}

/// REQ-002 and REQ-005: every free fit runs INIT's 8 starts, reports one outcome per start,
/// the selection, the diagnostics and the seed; the `Fit` trait returns the same result.
#[test]
fn multistart_record_and_diagnostics() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "arc-90-1").unwrap();
    let f = r.as_ref().unwrap();
    assert_eq!(f.starts.len(), 8);
    assert_eq!(f.per_start.len(), 8);
    assert!(f.per_start.iter().all(|s| matches!(s, StartSummary::Converged { .. })));
    assert_eq!(f.param_names, ["a", "b", "u", "v", "r"]);
    let d = &f.result.diagnostics;
    assert_eq!(d.rank, 5);
    assert!(d.iterations > 0 && d.cond >= 1.0 && d.grad_norm.is_finite());
    assert_eq!(f.result.algorithm.to_string(), "fit.cylinder.ls@1.0.0");
    let pts_v = c.get("points").unwrap();
    let pts = PointsView::new(&pts_v, None).unwrap();
    let t = CylinderLs.fit(&pts, &params(c), Some(SEED)).unwrap();
    assert_eq!(t, f.result);
    // Same seed: bit-identical; another seed changes the random starts only.
    assert_eq!(&fit_cylinder(&pts, &params(c), Some(SEED)).unwrap(), f);
    let other = fit_cylinder(&pts, &params(c), Some(7)).unwrap();
    assert_ne!(other.starts, f.starts);
    assert!(angle(cyl(&other).1, cyl(f).1) < 1e-9);
}

/// Spec card (REQ-KRN.LSQ-009, KRN.MAN.CARD) and the command data row (W2, dry run R1, D0).
#[test]
fn card_and_command_row() {
    let card = parse_card(CARD).unwrap();
    assert_eq!(card.id.to_string(), "fit.cylinder.ls@1.0.0");
    assert_eq!(card.determinism, "D0");
    assert_eq!(card.input_arity, InputArity::exactly(1));
    let report = check(&[CylinderLs.algorithm_id()], &[card.id.clone()]);
    assert!(report.is_clean(), "{report:?}");
    let row = cylinder_command_row().unwrap();
    row.validate().unwrap();
    assert_eq!(row.command.as_str(), "fit.ls_cylinder@1");
    assert_eq!((row.side_effect, row.dry_run, row.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
    assert!(row.result.iter().any(|f| f.name == "multistart.local_optimum_only"));
}

fn put(h: &mut Sha256Hasher, xs: &[f64]) {
    for x in xs {
        h.update(&x.to_bits().to_le_bytes());
    }
}

/// AC-3: output-bit pin of every case (feature, residuals, statistics, diagnostics, starts,
/// per-start outcomes, flags and every error code), recorded from a local x86-64 run and
/// checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime.
const OUTPUT_PIN: &str = "sha256:698a2056078c1fea0d8eff000d7fccea3c4ac07135f6081d2706494b20487422";

#[test]
fn ac3_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for (c, r) in outcomes() {
        h.update(c.name.as_bytes());
        match r {
            Ok(f) => {
                let (p, d, rad) = cyl(f);
                put(&mut h, &p);
                put(&mut h, &d);
                put(&mut h, &[rad]);
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
