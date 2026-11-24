//! Acceptance criteria of spec node KRN.ENV.TAN.PLANE (AC-1 to AC-5, kernel part) and the error
//! paths.
//!
//! Expected values come from `fixtures/fit/make_tan_plane_fixtures.py`: exact rational geometry
//! (fractions) of seeded plates built from support points and strictly interior bulk points,
//! the exact global optimum of each variant by enumeration over the supports, and correctly
//! rounded values from integer square roots (no platform libm, nothing from this crate).
//! `OUTPUT_PIN` is a regression pin of this crate's own output (not an oracle); CI checks it on
//! x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::needless_range_loop)]
// `FitError` is large by design (see the note in `fid-fit`'s lib.rs); the helpers return it whole.
#![allow(clippy::result_large_err)]

use fid_fit::contract::{Constraint, Fit, FitError, FitParams, FittedFeature, Method, PointsView, Weights};
use fid_fit::tan_plane::{
    algorithm_id, command_row, encode, fit_tan_plane, output_digest, Optimality, TanPlane, TanPlaneParams,
    TanPlaneResult,
};
use fid_fit::tan_rock::{MaterialSide, TanVariant, TieBreak};
use fid_kci::{check, parse_card, CardRegistry, InputArity};
use fid_solv::cert::{CertKind, Certificate};
use fid_testkit::Sha256Hasher;
use fid_types::{Determinism, SideEffect};

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/tan-plane-v1/cases.txt");
const CROWNED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/tan-rock-v1/crowned-plate.txt");

const CARDS: [&str; 3] = [
    include_str!("../cards/fit.plane.tan_minimax@1.0.0.toml"),
    include_str!("../cards/fit.plane.tan_l2@1.0.0.toml"),
    include_str!("../cards/fit.plane.tan_l1@1.0.0.toml"),
];

const SIDE_TOL: f64 = 1e-12;
const ROCK_TOL: f64 = 1e-9;
const SEED: u64 = 1;

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn ints(s: &str) -> Vec<u32> {
    s.split(',').map(|c| c.parse().unwrap()).collect()
}

#[derive(Debug, Clone)]
struct Expect {
    objective: f64,
    sets: Vec<Vec<u32>>,
    normal: [f64; 3],
}

#[derive(Debug, Clone)]
struct Case {
    name: String,
    outward: [f64; 3],
    fixed: [f64; 3],
    xyz: Vec<f64>,
    minimax: Option<Expect>,
    l1: Option<Expect>,
    fixed_values: [f64; 3],
    fixed_contacts: Vec<u32>,
}

fn v3(t: &[&str]) -> [f64; 3] {
    [hex(t[0]), hex(t[1]), hex(t[2])]
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for line in read(CASES).lines().filter(|l| !l.starts_with('#')) {
        let t: Vec<&str> = line.split(' ').collect();
        match t[0] {
            "case" => {
                cur = Some(Case {
                    name: t[1].to_string(),
                    outward: [0.0; 3],
                    fixed: [0.0; 3],
                    xyz: Vec::new(),
                    minimax: None,
                    l1: None,
                    fixed_values: [0.0; 3],
                    fixed_contacts: Vec::new(),
                })
            }
            "note" => {}
            "outward" => cur.as_mut().unwrap().outward = v3(&t[1..]),
            "fixed" => cur.as_mut().unwrap().fixed = v3(&t[1..]),
            "p" => cur.as_mut().unwrap().xyz.extend(v3(&t[1..])),
            "expect" => {
                let e = Expect { objective: hex(t[2]), sets: t[3].split(';').map(ints).collect(), normal: v3(&t[4..]) };
                let c = cur.as_mut().unwrap();
                match t[1] {
                    "minimax" => c.minimax = Some(e),
                    "l1" => c.l1 = Some(e),
                    other => panic!("unknown variant {other}"),
                }
            }
            "fixedv" => {
                let c = cur.as_mut().unwrap();
                c.fixed_values = v3(&t[1..4]);
                c.fixed_contacts = ints(t[4]);
            }
            "end" => out.push(cur.take().unwrap()),
            other => panic!("unknown line {other}"),
        }
    }
    out
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

fn tparams(c: &Case, v: Option<TanVariant>) -> TanPlaneParams {
    TanPlaneParams::new(v, MaterialSide::outward(c.outward).unwrap(), SIDE_TOL, ROCK_TOL)
}

fn run(c: &Case, v: TanVariant, fixed: bool) -> Result<TanPlaneResult, FitError> {
    let pts = PointsView::new(&c.xyz, None).unwrap();
    let mut params = FitParams::new(Method::Tangential);
    if fixed {
        params.constraints.push(Constraint::FixedAxisDirection(c.fixed));
    }
    fit_tan_plane(&pts, &params, &tparams(c, Some(v)), Some(SEED))
}

fn normal(r: &TanPlaneResult) -> [f64; 3] {
    match r.feature {
        FittedFeature::Plane { n, .. } => n,
        _ => panic!("not a plane"),
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Objective tolerance: `1e-12` relative to `max(1 mm, |expected|)`, far below the 1e-9 m of
/// AC-1 and far above the rounding of the binary64 evaluation.
fn obj_tol(x: f64) -> f64 {
    let m = if x.abs() > 1e-3 { x.abs() } else { 1e-3 };
    1e-12 * m
}

/// The common checks of a certified free-normal result against the oracle.
fn check_free(c: &Case, r: &TanPlaneResult, e: &Expect, v: TanVariant) {
    let tag = format!("{} {}", c.name, v);
    assert_eq!(r.variant, v, "{tag}");
    assert_eq!(r.algorithm, algorithm_id(v), "{tag}");
    assert!(close(r.objective, e.objective, obj_tol(e.objective)), "{tag}: {:e} vs {:e}", r.objective, e.objective);
    assert!(e.sets.contains(&r.contacts), "{tag}: contacts {:?} not in {:?}", r.contacts, e.sets);
    let n = normal(r);
    if e.sets.len() == 1 {
        for k in 0..3 {
            assert!(close(n[k], e.normal[k], 1e-12), "{tag}: normal {n:?} vs {:?}", e.normal);
        }
    }
    // REQ-002: every point on the material side within side_tol.
    assert!(r.max_violation <= SIDE_TOL, "{tag}");
    assert!(r.residuals.iter().all(|&d| d >= -SIDE_TOL), "{tag}");
    assert_eq!(r.residuals.len(), c.xyz.len() / 3);
    // Certified strict local minimum (linear constraints, nonlinear objective).
    assert_eq!(r.optimality, Optimality::StrictLocal, "{tag}");
    let cert = r.certificate.as_ref().unwrap();
    assert_eq!(cert.kind(), CertKind::Tangential);
    assert!(cert.is_linear());
    cert.check_consistency().unwrap();
    let bytes = cert.to_canonical_json().unwrap();
    assert_eq!(&Certificate::parse(&bytes).unwrap(), cert);
    // The contact set is the tangent side of the certified active set.
    let tangent: Vec<u32> = cert.active_set().iter().filter(|l| l.side == 0).map(|l| l.point).collect();
    assert_eq!(tangent, r.contacts, "{tag}");
    // The objective is the largest depth (minimax) or the sum of the depths (L1).
    let from_depths = match v {
        TanVariant::ExternalMinimax => r.residuals.iter().copied().fold(0.0_f64, |a, b| if b > a { b } else { a }),
        _ => r.residuals.iter().sum(),
    };
    assert!(close(from_depths, r.objective, obj_tol(r.objective) * 10.0), "{tag}");
    // The plane passes through the contacts.
    for &i in &r.contacts {
        assert!(r.residuals[i as usize].abs() <= SIDE_TOL, "{tag}");
    }
}

/// AC-1 (kernel part): every fixture plate, external minimax and constrained L1 with a free
/// normal, against the exact global optimum.
#[test]
fn ac1_free_normal_matches_exact_optimum() {
    for c in cases() {
        if let Some(e) = &c.minimax {
            let r = run(&c, TanVariant::ExternalMinimax, false).unwrap_or_else(|e| panic!("{} minimax: {e:?}", c.name));
            check_free(&c, &r, e, TanVariant::ExternalMinimax);
            assert!(!r.rock.rocking, "{}", c.name);
        }
        if let Some(e) = &c.l1 {
            let r = run(&c, TanVariant::ConstrainedL1, false).unwrap_or_else(|e| panic!("{} l1: {e:?}", c.name));
            check_free(&c, &r, e, TanVariant::ConstrainedL1);
        }
    }
}

/// AC-1 (kernel part), constrained L2 with a free normal: not certifiable by the vertex
/// certificate (Open questions), so it fails closed with a typed error and no value.
#[test]
fn ac1_free_normal_l2_is_refused() {
    let c = case("pins-small");
    assert_eq!(
        run(&c, TanVariant::ConstrainedL2, false).unwrap_err(),
        FitError::NotCertifiable { variant: TanVariant::ConstrainedL2 }
    );
}

/// AC-1 and AC-4: with a fixed normal every variant is exact; the plane passes through the
/// extreme point (its depth is exactly 0), and the rocking report has one candidate.
#[test]
fn ac4_fixed_normal_all_variants() {
    for c in cases() {
        for (k, v) in
            [TanVariant::ExternalMinimax, TanVariant::ConstrainedL1, TanVariant::ConstrainedL2].into_iter().enumerate()
        {
            let tag = format!("{} {v}", c.name);
            let r = run(&c, v, true).unwrap();
            let want = c.fixed_values[k];
            assert!(close(r.objective, want, 1e-12 * if want > 1e-3 { want } else { 1e-3 }), "{tag}");
            assert_eq!(r.contacts, c.fixed_contacts, "{tag}");
            assert_eq!(r.optimality, Optimality::ClosedForm);
            assert!(r.certificate.is_none());
            assert_eq!(r.max_violation, 0.0);
            assert!(r.residuals.iter().all(|&d| d >= 0.0), "{tag}");
            let extreme = c.fixed_contacts[0] as usize;
            assert_eq!(r.residuals[extreme], 0.0, "{tag}");
            assert!(!r.rock.rocking && r.rock.candidates.len() == 1 && r.rock.tie_break == TieBreak::Single);
            // The normal is the fixed direction, unit length, on the outward side.
            let n = normal(&r);
            let f = c.fixed;
            let len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            for j in 0..3 {
                assert!(close(n[j], f[j] / len, 1e-15), "{tag}");
            }
        }
    }
}

/// AC-2 (kernel part): a symmetric crowned plate rocks in constrained L1 on its four facets
/// through the apex; the report lists all four, ordered and with one selected.
#[test]
fn ac2_crowned_plate_rocks_in_l1() {
    let c = case("crown-sym");
    let e = c.l1.clone().unwrap();
    let r = run(&c, TanVariant::ConstrainedL1, false).unwrap();
    check_free(&c, &r, &e, TanVariant::ConstrainedL1);
    assert!(r.rock.rocking);
    assert_eq!(r.rock.selected, 0);
    let mut sets: Vec<Vec<u32>> = r.rock.candidates.iter().map(|k| k.contacts.clone()).collect();
    sets.sort();
    assert_eq!(sets, e.sets);
    for k in &r.rock.candidates {
        assert!(close(k.objective, e.objective, obj_tol(e.objective)));
    }
    assert_eq!(r.contacts, r.rock.candidates[0].contacts);
}

/// AC-2 on the `KRN.ENV.TAN.ROCK` crowned plate itself. Constrained L1 lists the four facet
/// sets with `rocking` true, each with the objective of the ROCK fixture's exact oracle
/// (`3f50ffffde000066`, documented in `crowned-plate.txt`). External minimax reaches the
/// horizontal plane through the apex; its 20 coplanar far-side points and the apex put 21 rows
/// in the small-slack pool (`C(21, 4) = 5985` bases, over `cert::active::MAX_CANDIDATES`), which
/// the certificate pipeline now reduces to one exact basis (`KRN.SOLV.CERT.AS`, REQ-007), so it
/// certifies the AC's answer: one candidate {0}, objective 1.5 h, `rocking` false, normal +z.
/// Expected values from the ROCK fixture's exact oracle (`make_tan_rock_fixtures.py`:
/// "minimax 1.5h = 4.57763671875e-05 m", contact {0}, the horizontal plane), checked against the
/// fixture text so the constant cannot drift from it.
#[test]
fn ac2_tan_rock_crowned_plate() {
    let text = read(CROWNED);
    let xyz: Vec<f64> = text
        .lines()
        .filter(|l| l.starts_with("p "))
        .flat_map(|l| l[2..].split(' ').map(hex).collect::<Vec<_>>())
        .collect();
    let pts = PointsView::new(&xyz, None).unwrap();
    let tp = |v| TanPlaneParams::new(Some(v), MaterialSide::outward([0.0, 0.0, 1.0]).unwrap(), SIDE_TOL, ROCK_TOL);
    let fit = |v| fit_tan_plane(&pts, &FitParams::new(Method::Tangential), &tp(v), Some(SEED));
    let r = fit(TanVariant::ConstrainedL1).unwrap();
    assert!(r.rock.rocking);
    let sets: Vec<Vec<u32>> = r.rock.candidates.iter().map(|k| k.contacts.clone()).collect();
    assert_eq!(sets, [vec![0, 1, 2], vec![0, 1, 4], vec![0, 2, 3], vec![0, 3, 4]]);
    let want = hex("3f50ffffde000066");
    for k in &r.rock.candidates {
        assert!(close(k.objective, want, obj_tol(want)), "{:016x}", k.objective.to_bits());
    }
    assert_eq!(r.rock.tie_break, TieBreak::ContactOrder);
    assert_eq!(r.optimality, Optimality::StrictLocal);
    assert!(r.max_violation <= SIDE_TOL);
    assert!(text.contains("minimax 1.5h = 4.57763671875e-05 m"));
    let want = 1.5 * 2f64.powi(-15);
    assert_eq!(want, 4.57763671875e-05);
    let m = fit(TanVariant::ExternalMinimax).unwrap();
    assert!(close(m.objective, want, obj_tol(want)), "{:016x}", m.objective.to_bits());
    assert_eq!(m.contacts, [0]);
    assert!(!m.rock.rocking);
    let sets: Vec<Vec<u32>> = m.rock.candidates.iter().map(|k| k.contacts.clone()).collect();
    assert_eq!(sets, [vec![0]]);
    assert_eq!(m.optimality, Optimality::StrictLocal);
    assert!(m.max_violation <= SIDE_TOL);
    let n = normal(&m);
    for (j, want_n) in [0.0, 0.0, 1.0].into_iter().enumerate() {
        assert!(close(n[j], want_n, 1e-15), "{n:?}");
    }
    assert!(m.certificate.is_some());
}

/// AC-3 (kernel part): no variant is `InvalidParams{field: "variant"}`, both through the trait
/// constructor and the full entry point (there is no default).
#[test]
fn ac3_missing_variant() {
    let c = case("pins-small");
    let tp = tparams(&c, None);
    assert_eq!(TanPlane::new(tp.clone()).unwrap_err(), FitError::InvalidParams { field: "variant" });
    let pts = PointsView::new(&c.xyz, None).unwrap();
    assert_eq!(
        fit_tan_plane(&pts, &FitParams::new(Method::Tangential), &tp, Some(SEED)).unwrap_err(),
        FitError::InvalidParams { field: "variant" }
    );
}

/// Stable encoding of an error: code and fields.
fn encode_err(e: &FitError) -> String {
    match e {
        FitError::SeqFailed { cause, starts } => format!("solver cause={cause} starts={starts}"),
        FitError::NotCertifiable { variant } => format!("not_certifiable variant={}", variant.slug()),
        FitError::InvalidParams { field } => format!("invalid_params field={field}"),
        FitError::TooFewPoints { n, need } => format!("too_few_points n={n} need={need}"),
        FitError::Collinear { n, rank, cond } => format!("collinear n={n} rank={rank} cond={:016x}", cond.to_bits()),
        FitError::SideViolation { index, violation } => {
            format!("side_violation index={index} violation={:016x}", violation.to_bits())
        }
        other => panic!("unpinned error {other:?}"),
    }
}

fn base(c: &Case) -> (Vec<f64>, FitParams, TanPlaneParams) {
    (c.xyz.clone(), FitParams::new(Method::Tangential), tparams(c, Some(TanVariant::ConstrainedL1)))
}

fn error_cases() -> Vec<FitError> {
    let c = case("pins-small");
    let mut out = Vec::new();
    let go = |xyz: &[f64], p: &FitParams, t: &TanPlaneParams, seed: Option<u64>| {
        let pts = PointsView::new(xyz, None).unwrap();
        fit_tan_plane(&pts, p, t, seed).unwrap_err()
    };
    let (xyz, p, t) = base(&c);
    out.push(go(&xyz, &FitParams::new(Method::LeastSquares), &t, Some(SEED)));
    let mut q = p.clone();
    q.weights = Weights::PerPoint(vec![1.0; xyz.len() / 3]);
    out.push(go(&xyz, &q, &t, Some(SEED)));
    let mut q = p.clone();
    q.constraints.push(Constraint::FixedRadius(1.0));
    out.push(go(&xyz, &q, &t, Some(SEED)));
    out.push(go(&xyz, &p, &TanPlaneParams { side_tol: -1.0, ..t.clone() }, Some(SEED)));
    out.push(go(&xyz, &p, &TanPlaneParams { rock_tol: 2.0, ..t.clone() }, Some(SEED)));
    out.push(go(&xyz, &p, &TanPlaneParams { starts: 0, ..t.clone() }, Some(SEED)));
    out.push(go(&xyz, &p, &t, None));
    out.push(go(&xyz, &p, &TanPlaneParams { material_side: MaterialSide::PointNormals, ..t.clone() }, Some(SEED)));
    // A fixed normal perpendicular to the outward direction has no material-free side.
    let mut q = p.clone();
    q.constraints.push(Constraint::FixedAxisDirection([1.0, 0.0, 0.0]));
    out.push(go(&xyz, &q, &t, Some(SEED)));
    out.push(go(&xyz[..6], &p, &t, Some(SEED)));
    out.push(go(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0], &p, &t, Some(SEED)));
    out.push(go(&[], &q, &t, Some(SEED)));
    out.push(go(&xyz, &p, &tparams(&c, Some(TanVariant::ConstrainedL2)), Some(SEED)));
    out
}

#[test]
fn error_paths() {
    let got: Vec<String> = error_cases().iter().map(encode_err).collect();
    let want = [
        "invalid_params field=method",
        "invalid_params field=weights",
        "invalid_params field=constraints",
        "invalid_params field=side_tol",
        "invalid_params field=rock_tol",
        "invalid_params field=starts",
        "invalid_params field=seed",
        "invalid_params field=material_side",
        "invalid_params field=constraints.fixed_axis_direction",
        "too_few_points n=2 need=3",
        "collinear n=3 rank=1 cond=7ff0000000000000",
        "too_few_points n=0 need=1",
        "not_certifiable variant=constrained_l2",
    ];
    assert_eq!(got, want);
}

/// Without random starts (at most 1 + 4 ring starts) no seed is needed, and none is recorded.
#[test]
fn ring_only_needs_no_seed() {
    let c = case("pins-small");
    let pts = PointsView::new(&c.xyz, None).unwrap();
    let tp = TanPlaneParams { starts: 5, ..tparams(&c, Some(TanVariant::ExternalMinimax)) };
    let r = fit_tan_plane(&pts, &FitParams::new(Method::Tangential), &tp, None).unwrap();
    assert_eq!(r.seed, None);
    check_free(&c, &r, c.minimax.as_ref().unwrap(), TanVariant::ExternalMinimax);
}

/// Per-point outward normals give the same result as the equivalent outward direction.
#[test]
fn point_normals_side() {
    let c = case("pins-small");
    let normals: Vec<f64> = (0..c.xyz.len() / 3).flat_map(|_| c.outward).collect();
    let pts = PointsView::new(&c.xyz, Some(&normals)).unwrap();
    let a =
        TanPlaneParams { material_side: MaterialSide::PointNormals, ..tparams(&c, Some(TanVariant::ConstrainedL1)) };
    let r = fit_tan_plane(&pts, &FitParams::new(Method::Tangential), &a, Some(SEED)).unwrap();
    let s = run(&c, TanVariant::ConstrainedL1, false).unwrap();
    assert_eq!(encode(&r).unwrap(), encode(&s).unwrap());
}

/// The `Fit` trait: the contract summary of the same fit. Residuals are signed as the contract
/// has them (positive on the canonical normal's side): pins-small's outward normal has a
/// positive first component, so the sign rule keeps it and the residuals are `-depth`;
/// slant-far's has a negative first component, so the rule flips it and they are `+depth`. The
/// gap is the certificate's value bound times the form scale and the scale.
#[test]
fn fit_trait_summary() {
    for (name, flipped) in [("pins-small", false), ("slant-far", true)] {
        let c = case(name);
        let pts = PointsView::new(&c.xyz, None).unwrap();
        let f = TanPlane::new(tparams(&c, Some(TanVariant::ConstrainedL1))).unwrap();
        assert_eq!(f.algorithm_id().to_string(), "fit.plane.tan_l1@1.0.0");
        let s = f.fit(&pts, &FitParams::new(Method::Tangential), Some(SEED)).unwrap();
        let r = run(&c, TanVariant::ConstrainedL1, false).unwrap();
        assert_eq!(s.feature, r.feature.canonical().unwrap());
        let n = normal(&r);
        let cn = match s.feature {
            FittedFeature::Plane { n, .. } => n,
            _ => panic!("not a plane"),
        };
        assert_eq!(n[0] < 0.0, flipped, "{name}");
        assert_eq!(cn[0] * n[0] < 0.0, flipped, "{name}");
        let want: Vec<f64> = r.residuals.iter().map(|&d| if flipped { d } else { 0.0 - d }).collect();
        assert_eq!(s.residuals, want, "{name}");
        if flipped {
            assert!(s.residuals.iter().all(|&x| x >= -SIDE_TOL));
        } else {
            assert!(s.residuals.iter().all(|&x| x <= SIDE_TOL));
        }
        let cert = s.certificate.unwrap();
        assert_eq!(cert.support, r.contacts.iter().map(|&i| i as usize).collect::<Vec<_>>());
        let full = r.certificate.as_ref().unwrap();
        let scale = fid_fit::contract::Centred::new(&pts).unwrap().map().scale();
        assert_eq!(cert.gap, (r.form_scale * full.value_bound()) * scale, "{name}");
        assert!(r.form_scale < 1.0 && cert.gap > 0.0);
        assert_eq!(s.diagnostics.rank, 3);
    }
}

/// Contacts come from the certified active set (free normal) or exact ties with the extreme
/// point (fixed normal), never from a `side_tol` comparison: `side_tol = 0` gives the same
/// contacts, and so does a band wide enough to take in other points.
#[test]
fn contacts_do_not_depend_on_side_tol() {
    let c = case("pins-small");
    let pts = PointsView::new(&c.xyz, None).unwrap();
    for v in TanVariant::ALL {
        for fixed in [false, true] {
            if !fixed && *v == TanVariant::ConstrainedL2 {
                continue;
            }
            let base = run(&c, *v, fixed).unwrap();
            let mut params = FitParams::new(Method::Tangential);
            if fixed {
                params.constraints.push(Constraint::FixedAxisDirection(c.fixed));
            }
            for side_tol in [0.0, 1e-4] {
                let tp = TanPlaneParams { side_tol, ..tparams(&c, Some(*v)) };
                match fit_tan_plane(&pts, &params, &tp, Some(SEED)) {
                    Ok(r) => {
                        assert_eq!(r.contacts, base.contacts, "{v} fixed={fixed} side_tol={side_tol}");
                        assert_eq!(r.residuals, base.residuals);
                    }
                    // A certified result whose float depths dip below 0 by rounding is refused
                    // when nothing is tolerated; it must not change the contacts otherwise.
                    Err(FitError::SideViolation { violation, .. }) => {
                        assert!(side_tol == 0.0 && !fixed && violation < 1e-15, "{v} {violation:e}");
                    }
                    Err(e) => panic!("{v} fixed={fixed}: {e:?}"),
                }
            }
            if fixed {
                // The extreme point's depth is exactly 0 and none is negative.
                assert_eq!(base.max_violation, 0.0);
                assert!(base.contacts.iter().all(|&i| base.residuals[i as usize] == 0.0));
            }
        }
    }
}

/// Spec cards (KRN.MAN.CARD): one per algorithm ID, clean card check, one input.
#[test]
fn spec_cards() {
    let cards: Vec<_> = CARDS.iter().map(|t| parse_card(t).unwrap()).collect();
    let ids: Vec<_> = TanVariant::ALL.iter().map(|&v| algorithm_id(v)).collect();
    let names: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    assert_eq!(names, ["fit.plane.tan_minimax@1.0.0", "fit.plane.tan_l2@1.0.0", "fit.plane.tan_l1@1.0.0"]);
    let card_ids: Vec<_> = cards.iter().map(|c| c.id.clone()).collect();
    let report = check(&ids, &card_ids);
    assert!(report.is_clean(), "{report:?}");
    for card in &cards {
        assert_eq!(card.determinism, "D0");
        assert_eq!(card.input_arity, InputArity::exactly(1));
    }
    CardRegistry::new(cards).unwrap();
}

/// The `fit.tan_plane@1` data row: W2 with R1 dry run, D0, valid fields.
#[test]
fn command_row_validates() {
    let row = command_row().unwrap();
    row.validate().unwrap();
    assert_eq!(row.command.as_str(), "fit.tan_plane@1");
    assert_eq!((row.side_effect, row.dry_run, row.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
    for name in ["variant", "material_side", "side_tol", "rock_tol", "starts", "seed"] {
        assert!(row.params.iter().any(|f| f.name == name), "{name}");
    }
    for name in ["objective", "contacts", "max_violation", "optimality", "certificate.digest", "rock.rocking"] {
        assert!(row.result.iter().any(|f| f.name == name), "{name}");
    }
    let variant = row.params.iter().find(|f| f.name == "variant").unwrap();
    assert!(variant.required);
}

/// The canonical bytes use the `F64Bits` form and are the same on a repeat run.
#[test]
fn canonical_bytes() {
    let c = case("pins-small");
    let a = run(&c, TanVariant::ExternalMinimax, false).unwrap();
    let b = run(&c, TanVariant::ExternalMinimax, false).unwrap();
    let bytes = encode(&a).unwrap();
    assert_eq!(bytes, encode(&b).unwrap());
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("{\"algorithm\":\"fit.plane.tan_minimax@1.0.0\",\"variant\":\"external_minimax\""));
    assert!(text.contains(&format!("\"objective\":\"0x{:016x}\"", a.objective.to_bits())));
    let digest = a.certificate.as_ref().unwrap().digest().as_str().to_string();
    assert!(text.contains(&format!("\"certificate\":\"{digest}\"")));
    assert!(text.ends_with(",\"seed\":\"1\"}"));
    assert!(output_digest(&a).unwrap().as_str().starts_with("sha256:"));
}

// ------------------------------------------------------------------ AC-5 output pin

/// Output-bit pin: regression pin of this crate's own output (the canonical bytes of every
/// AC-1 free-normal and fixed-normal result, the AC-2 results on the ROCK crowned plate (the
/// constrained-L1 report and the certified minimax result), then the error paths in
/// the stable encoding of `encode_err`), recorded from a local x86-64 run and checked by CI on
/// every target. One value serves all targets because they agree bit for bit; a target that
/// diverged would get its own `#[cfg(target_arch)]` value here. Re-recorded in #575 (previous
/// value `sha256:9131e34ab01ab7a76503a891459f21df4da7118a347f384dceb02d0c4403c8e6`): the
/// least-squares start normal from `ls_lin::PlaneLs` now comes from `fid_num::la::sym_eig`.
const OUTPUT_PIN: &str = "sha256:f79fa4893b35c08bdf96539f9293f9a634164daaa45ca7e09a94e86968d5a353";

#[test]
fn ac5_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in cases() {
        for v in [TanVariant::ExternalMinimax, TanVariant::ConstrainedL1] {
            let has = match v {
                TanVariant::ExternalMinimax => c.minimax.is_some(),
                _ => c.l1.is_some(),
            };
            if has {
                h.update(&encode(&run(&c, v, false).unwrap()).unwrap());
            }
        }
        for v in TanVariant::ALL {
            h.update(&encode(&run(&c, *v, true).unwrap()).unwrap());
        }
    }
    let text = read(CROWNED);
    let xyz: Vec<f64> = text
        .lines()
        .filter(|l| l.starts_with("p "))
        .flat_map(|l| l[2..].split(' ').map(hex).collect::<Vec<_>>())
        .collect();
    let pts = PointsView::new(&xyz, None).unwrap();
    for v in [TanVariant::ConstrainedL1, TanVariant::ExternalMinimax] {
        let tp = TanPlaneParams::new(Some(v), MaterialSide::outward([0.0, 0.0, 1.0]).unwrap(), SIDE_TOL, ROCK_TOL);
        match fit_tan_plane(&pts, &FitParams::new(Method::Tangential), &tp, Some(SEED)) {
            Ok(r) => h.update(&encode(&r).unwrap()),
            Err(e) => h.update(encode_err(&e).as_bytes()),
        }
    }
    for e in error_cases() {
        h.update(encode_err(&e).as_bytes());
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
