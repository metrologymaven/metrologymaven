//! Acceptance criteria of spec node KRN.ENV.MCC.CYL that do not need the RDL or the command
//! layer (AC-1 on the fixture, AC-2, AC-3 partly, AC-4, AC-5's digests), the requirements they
//! exercise (REQ-001 to REQ-004) and the guards of `mcc_cyl` rules 7, 9 and 10.
//!
//! Expected values come from `fixtures/fit/make_mcc_cyl_fixtures.py`: planted minimum
//! circumscribed cylinders whose five contacts satisfy the KKT conditions with positive
//! multipliers, recomputed exactly in 60-digit Decimal on the rounded points (no libm,
//! independent of this crate), and hand-labelled flags and errors. `OUTPUT_PIN` is a regression
//! pin of this crate's own output (not an oracle); CI checks it on x86-64 Linux and Windows,
//! ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{Constraint, Fit, FitError, FitParams, FittedFeature, Method, PointsView};
use fid_fit::mcc_cyl::{
    algorithm, fit_mcc_cylinder, fit_mcc_cylinder_from, mcc_command_row, to_fit_result, CrossOutcome, MccCylinder,
    MccCylinderFit, Optimality, INSIDE_TOL,
};
use fid_kci::{check, parse_card, InputArity};
use fid_solv::mstart::StartSummary;
use fid_testkit::Sha256Hasher;
use fid_types::{Determinism, SideEffect};
use std::sync::OnceLock;

const FIXTURE: &str = include_str!("../../../fixtures/fit/mcc-cyl-v1/cases.txt");
const CARD: &str = include_str!("../cards/fit.cylinder.mc@1.0.0.toml");
/// AC-1 and AC-5 run with seed 7.
const SEED: u64 = 7;

/// Radius against the exact reference, metres. AC-1 asks for 1e-8 m [unverified]; the
/// certified radius is within 3.5e-18 m on the fixture, so the tolerance is about 10 times that
/// worst case, rounded up.
const TOL_R: f64 = 1e-16;
/// Axis angle against the reference, radians (worst 5.8e-16), and the reference point's
/// distance from the fitted axis, metres (worst 8.8e-15).
const TOL_AXIS: f64 = 1e-14;
const TOL_POS: f64 = 1e-13;

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

#[derive(Debug)]
struct Case {
    name: String,
    fields: Vec<(String, String)>,
}

impl Case {
    fn text(&self, k: &str) -> Option<&str> {
        self.fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
    fn get(&self, k: &str) -> Option<Vec<f64>> {
        self.text(k).map(hexes)
    }
    fn v3(&self, k: &str) -> [f64; 3] {
        let v = self.get(k).unwrap();
        [v[0], v[1], v[2]]
    }
    fn ints(&self, k: &str) -> Option<Vec<u32>> {
        self.text(k).map(|s| s.split_whitespace().map(|t| t.parse().unwrap()).collect())
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
    let mut p = FitParams::new(Method::MinimumCircumscribed);
    if c.get("fixed_dir").is_some() {
        p.constraints.push(Constraint::FixedAxisDirection(c.v3("fixed_dir")));
    }
    p
}

fn run(c: &Case) -> Result<MccCylinder, FitError> {
    let v = c.get("points").unwrap();
    fit_mcc_cylinder(&PointsView::new(&v, None).unwrap(), &params(c), Some(SEED))
}

fn outcomes() -> &'static [(Case, Result<MccCylinder, FitError>)] {
    static ALL: OnceLock<Vec<(Case, Result<MccCylinder, FitError>)>> = OnceLock::new();
    ALL.get_or_init(|| {
        parse()
            .into_iter()
            .map(|c| {
                let t = std::time::Instant::now();
                let r = run(&c);
                eprintln!(
                    "{}: {:?} in {:.2?}",
                    c.name,
                    r.as_ref()
                        .map(|m| (
                            m.local_optimum_only,
                            m.suspect_axis,
                            m.zone_ratio,
                            m.agreement,
                            m.cross_check_passed,
                            m.cross_check.iter().map(|x| (x.outcome.as_str(), x.lower, x.upper)).collect::<Vec<_>>(),
                            cyl(m).2
                        ))
                        .map_err(|e| e.code()),
                    t.elapsed()
                );
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
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b).abs())
}

fn cyl(m: &MccCylinder) -> ([f64; 3], [f64; 3], f64) {
    match m.feature {
        FittedFeature::Cylinder { p, d, r } => (p, d, r),
        ref o => panic!("{o:?}"),
    }
}

fn flag(c: &Case) -> Option<&str> {
    c.text("flag")
}

/// AC-1 and REQ-001/-002: every planted envelope is found and certified: radius, axis and
/// position within the stated tolerances of the exact reference, the contact set the five
/// planted contacts, every point inside within 1e-12 m, the starts agreeing, no flag.
#[test]
fn ac1_planted_envelopes() {
    let mut n = 0;
    let mut worst = [0.0f64; 3];
    for (c, r) in outcomes().iter().filter(|(c, _)| c.name.starts_with("env-")) {
        let m = r.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let (p, d, rad) = cyl(m);
        let errs = [
            (rad - c.get("ref_r").unwrap()[0]).abs(),
            angle(d, c.v3("ref_axis")),
            norm(cross(sub(c.v3("ref_point"), p), d)),
        ];
        for (k, (e, t)) in errs.iter().zip([TOL_R, TOL_AXIS, TOL_POS]).enumerate() {
            assert!(*e <= t, "{}: error {k} = {e:e} above {t:e}", c.name);
            worst[k] = worst[k].max(*e);
        }
        assert_eq!(m.contacts, c.ints("contacts").unwrap(), "{}", c.name);
        assert!(m.max_violation <= INSIDE_TOL && m.residuals.iter().all(|&x| x <= INSIDE_TOL), "{}", c.name);
        assert!(m.agreement && !m.local_optimum_only && !m.suspect_axis, "{}", c.name);
        assert_eq!(m.optimality, Optimality::StrictLocal);
        assert!(!m.certificate.flags().local_optimum_only);
        // Rule 11: every direction of the global cross-check passed.
        assert!(m.cross_check_passed && m.cross_check.len() == 4, "{}: {:?}", c.name, m.cross_check);
        assert!(m.cross_check.iter().all(|x| x.outcome == CrossOutcome::Passed && x.lower >= rad * (1.0 - 1e-9)));
        n += 1;
    }
    eprintln!("worst radius {:e} m, axis {:e} rad, position {:e} m", worst[0], worst[1], worst[2]);
    assert_eq!(n, 5);
}

/// REQ-003 and AC-3: with the direction fixed at the planted optimum's, the convex 2D problem
/// gives the same cylinder, `Global` optimality and `local_optimum_only: false`. (Equality with
/// `fit.circle2d.mcc` waits for KRN.ENV.MCC.CVX.)
#[test]
fn ac3_fixed_direction_is_global() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "fixed-dir").unwrap();
    let m = r.as_ref().unwrap();
    let (p, d, rad) = cyl(m);
    assert!(angle(d, c.v3("fixed_dir")) <= 1e-15);
    assert!((rad - c.get("ref_r").unwrap()[0]).abs() <= TOL_R);
    assert!(norm(cross(sub(c.v3("ref_point"), p), d)) <= TOL_POS);
    assert_eq!(m.optimality, Optimality::Global);
    assert!(!m.local_optimum_only);
    assert_eq!(m.starts.len(), 1);
    assert_eq!(m.certificate.params().len(), 3);
    assert!(m.cross_check.is_empty() && m.cross_check_passed);
}

/// Review of #374 (rule 11): the short oval pilot diameter has a certified near-axis local
/// minimum that a cylinder across the pilot beats (`better_r`, the fixture's exact feasible
/// radius along the ovality's major axis). The fit either returns a radius at most `better_r`
/// or is refused or flagged `local_optimum_only`; through the trait it returns such a radius or
/// is refused. Never a confidently wrong cylinder.
#[test]
fn pilot_never_confidently_wrong() {
    let (c, r) = outcomes().iter().find(|(c, _)| flag(c) == Some("pilot_not_global")).unwrap();
    let better = c.get("better_r").unwrap()[0];
    let small = |m: &MccCylinder| cyl(m).2 <= better * (1.0 + 1e-12);
    match r {
        Ok(m) if small(m) => {}
        Ok(m) => {
            assert!(m.local_optimum_only && !m.cross_check_passed, "{m:?}");
            // The cross-check found a direction at least as good as the fixture's feasible one.
            let better_dirs: Vec<_> = m.cross_check.iter().filter(|x| x.outcome == CrossOutcome::Better).collect();
            assert!(better_dirs.iter().any(|x| x.upper <= better * (1.0 + 1e-9)), "{:?}", m.cross_check);
            assert!(m.certificate.flags().local_optimum_only);
        }
        Err(e) => eprintln!("pilot: refused {}", e.code()),
    }
    let v = c.get("points").unwrap();
    match MccCylinderFit.fit(&PointsView::new(&v, None).unwrap(), &params(c), Some(SEED)) {
        Ok(f) => match f.feature {
            FittedFeature::Cylinder { r, .. } => assert!(r <= better * (1.0 + 1e-12), "{r}"),
            ref o => panic!("{o:?}"),
        },
        Err(e) => assert!(matches!(e.code(), "flagged_fit" | "solver" | "implausible_fit"), "{e:?}"),
    }
}

/// Oval cross-sections (should-fix of the #374 review, kept failing closed): four active rows
/// for five parameters (free direction) and two for three (fixed direction) are a degenerate
/// active set, which the certificate does not support. The fit is refused with code `solver`,
/// directly and through the trait.
#[test]
fn degenerate_active_set_is_refused() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| flag(c) == Some("degenerate_active_set")) {
        let e = r.as_ref().map(cyl).unwrap_err();
        assert!(matches!(e, FitError::SeqFailed { .. }), "{}: {e:?}", c.name);
        let v = c.get("points").unwrap();
        let t = MccCylinderFit.fit(&PointsView::new(&v, None).unwrap(), &params(c), Some(SEED));
        assert_eq!(t.as_ref().unwrap_err(), e, "{}", c.name);
        n += 1;
    }
    assert_eq!(n, 2);
}

/// AC-2 (as built, see the leaf's Open questions): the slab's two near-equal envelope axes lie
/// about 90 degrees from the least-squares frame, beyond the tangent-plane parameters' reach.
/// The fit must then fail closed (no start certifies, or the certified minimum is larger than
/// the least-squares-axis radius, rule 9); it never returns one axis unflagged.
#[test]
fn ac2_two_axes_never_unflagged() {
    let (c, r) = outcomes().iter().find(|(c, _)| flag(c) == Some("two_axes")).unwrap();
    match r {
        Ok(m) => assert!(!m.agreement && m.local_optimum_only, "{}", c.name),
        Err(e) => assert!(matches!(e.code(), "solver" | "implausible_fit"), "{}: {e:?}", c.name),
    }
}

/// Ties: a sixth point on the planted surface at a zero-multiplier position. The fit certifies
/// a five-point subset of the six tied points with the reference radius, unflagged.
#[test]
fn tied_contacts() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "tie").unwrap();
    let tie: u32 = c.text("tie_point").unwrap().parse().unwrap();
    let mut six = c.ints("contacts").unwrap();
    six.push(tie);
    let m = r.as_ref().unwrap_or_else(|e| panic!("tie: {e:?}"));
    assert_eq!(m.contacts.len(), 5);
    assert!(m.contacts.iter().all(|k| six.contains(k)), "{:?}", m.contacts);
    assert!((cyl(m).2 - c.get("ref_r").unwrap()[0]).abs() <= TOL_R);
    assert!(!m.local_optimum_only && !m.suspect_axis && m.cross_check_passed);
}

/// Half cylinders 1.066 radii long and short noisy 30-degree arcs: the enclosing cylinder of
/// such data need not be the measured surface's, and the fixture has no reference for it. No
/// fit is confidently wrong: each is refused or flagged; an unflagged fit fails the test.
#[test]
fn partial_arcs_never_confidently_wrong() {
    for (c, r) in outcomes().iter().filter(|(c, _)| flag(c) == Some("never_confidently_wrong")) {
        match r {
            Err(e) => eprintln!("{}: refused {}", c.name, e.code()),
            Ok(m) if m.local_optimum_only || m.suspect_axis => {
                eprintln!("{}: flagged (zone ratio {:.3})", c.name, m.zone_ratio)
            }
            Ok(m) => panic!("{}: unflagged fit (zone ratio {:.3}) on data with no reference", c.name, m.zone_ratio),
        }
    }
}

/// AC-4 and the other error paths.
#[test]
fn ac4_error_paths() {
    for (c, r) in outcomes().iter().filter(|(c, _)| c.text("expect") != Some("start")) {
        let e = r.as_ref().map(|_| ()).unwrap_err();
        let got = match e {
            FitError::TooFewPoints { n, need } => format!("too_few_points n={n} need={need}"),
            FitError::IllConditioned { cond } => {
                assert!(cond.unwrap() > 100.0);
                "ill_conditioned".to_string()
            }
            e => format!("{e:?}"),
        };
        assert_eq!(got, c.text("expect").unwrap(), "{}", c.name);
    }
}

/// Rule 9's certificate: from a start axis perpendicular to the planted one, a certified local
/// minimum larger than the feasible start radius is refused, or the fit is flagged; it is never
/// returned unflagged with a radius above the least-squares start's.
#[test]
fn wrong_start_is_refused_or_flagged() {
    let (c, _) = outcomes().iter().find(|(c, _)| c.name == "env-360-1").unwrap();
    let v = c.get("points").unwrap();
    let pts = PointsView::new(&v, None).unwrap();
    let d = c.v3("ref_axis");
    let perp = {
        let k = if d[0].abs() < 0.5 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        cross(d, k)
    };
    match fit_mcc_cylinder_from(&pts, &params(c), c.v3("ref_point"), perp, Some(SEED)) {
        Ok(m) => {
            let good =
                cyl(&outcomes().iter().find(|(k, _)| k.name == "env-360-1").unwrap().1.as_ref().unwrap().clone()).2;
            assert!(m.suspect_axis || m.local_optimum_only || (cyl(&m).2 - good).abs() <= TOL_R, "{m:?}");
        }
        Err(e) => assert!(matches!(e, FitError::ImplausibleFit { .. } | FitError::SeqFailed { .. }), "{e:?}"),
    }
}

/// Rule 10 (fail closed): through the `Fit` trait every flagged case is `FlaggedFit`, every
/// clean one returns the contract summary of the same fit, every error is the same error.
#[test]
fn trait_fails_closed() {
    let mut flagged = 0;
    // One clean case of each kind, the flagged pilot (rule 11) and the failing ones (the full
    // fits are slow in debug builds).
    let names = ["env-360-0", "fixed-dir", "pilot", "twoaxes", "disc", "few-4"];
    for (c, r) in outcomes().iter().filter(|(c, _)| names.contains(&c.name.as_str())) {
        let v = c.get("points").unwrap();
        let t = MccCylinderFit.fit(&PointsView::new(&v, None).unwrap(), &params(c), Some(SEED));
        match r {
            Ok(m) if m.local_optimum_only || m.suspect_axis => {
                assert_eq!(t.unwrap_err().code(), "flagged_fit", "{}", c.name);
                flagged += 1;
            }
            Ok(m) => {
                let f = t.unwrap();
                assert_eq!(f.feature, m.feature.canonical().unwrap());
                assert_eq!(f.certificate.unwrap().support, m.contacts.iter().map(|&k| k as usize).collect::<Vec<_>>());
            }
            Err(e) => assert_eq!(t.as_ref().unwrap_err(), e, "{}", c.name),
        }
    }
    assert!(flagged >= 1, "no flagged case reached the trait");
}

/// Rule 10 on flagged fits: a clean result with either flag set is refused by the trait's
/// summary (`to_fit_result`), never returned unflagged.
#[test]
fn flagged_fits_fail_closed() {
    let (c, r) = outcomes().iter().find(|(c, _)| c.name == "env-360-0").unwrap();
    let m = r.as_ref().unwrap();
    let v = c.get("points").unwrap();
    let scale = fid_fit::contract::Centred::new(&PointsView::new(&v, None).unwrap()).unwrap().map().scale();
    assert!(to_fit_result(m.clone(), scale).is_ok());
    for (lo, sus) in [(true, false), (false, true), (true, true)] {
        let mut f = m.clone();
        f.local_optimum_only = lo;
        f.suspect_axis = sus;
        let e = to_fit_result(f, scale).unwrap_err();
        assert_eq!(e, FitError::FlaggedFit { local_optimum_only: lo, suspect_axis: sus, rms_ratio: m.zone_ratio });
    }
    // Rule 10: a strict local minimum without a passed cross-check is refused even when the
    // flag itself is clear.
    let mut f = m.clone();
    f.cross_check_passed = false;
    let e = to_fit_result(f, scale).unwrap_err();
    assert_eq!(e, FitError::FlaggedFit { local_optimum_only: true, suspect_axis: false, rms_ratio: m.zone_ratio });
}

/// Spec card and command data row (W2, dry run R1, D0).
#[test]
fn card_and_command_row() {
    let card = parse_card(CARD).unwrap();
    assert_eq!(card.id.to_string(), "fit.cylinder.mc@1.0.0");
    assert_eq!(card.input_arity, InputArity::exactly(1));
    let report = check(&[algorithm()], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    let row = mcc_command_row().unwrap();
    row.validate().unwrap();
    assert_eq!(row.command.as_str(), "fit.mc_cylinder@1");
    assert_eq!((row.side_effect, row.dry_run, row.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
}

fn put(h: &mut Sha256Hasher, xs: &[f64]) {
    for x in xs {
        h.update(&x.to_bits().to_le_bytes());
    }
}

/// AC-5 (digests): output-bit pin of every case: feature, residuals, contacts, flags,
/// certificate digest, certified per-start values, and every error code.
const OUTPUT_PIN: &str = "sha256:3c8f5957cb9db8e90d2cec61dc441fb06fc265b084d41923be76ec1c3c11cd48";

#[test]
fn ac5_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for (c, r) in outcomes() {
        h.update(c.name.as_bytes());
        match r {
            Ok(m) => {
                let (p, d, rad) = cyl(m);
                put(&mut h, &p);
                put(&mut h, &d);
                put(&mut h, &[rad, m.max_violation, m.start_radius, m.zone_ratio]);
                put(&mut h, &m.residuals);
                for k in &m.contacts {
                    h.update(&k.to_le_bytes());
                }
                h.update(m.certificate.digest().as_str().as_bytes());
                h.update(&[u8::from(m.agreement), u8::from(m.local_optimum_only), u8::from(m.suspect_axis)]);
                h.update(m.optimality.as_str().as_bytes());
                for s in &m.per_start {
                    match s {
                        StartSummary::Converged { objective, params } => {
                            put(&mut h, &[*objective]);
                            put(&mut h, params);
                        }
                        StartSummary::Failed(e) => h.update(format!("{e:?}").as_bytes()),
                    }
                }
                h.update(&m.best_index.to_le_bytes());
                h.update(&[u8::from(m.cross_check_passed)]);
                for x in &m.cross_check {
                    put(&mut h, &x.direction);
                    put(&mut h, &[x.lower, x.upper]);
                    h.update(&[u8::from(x.certified)]);
                    h.update(x.outcome.as_str().as_bytes());
                }
            }
            Err(e) => h.update(e.code().as_bytes()),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
