#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! KRN.OUT.STAT acceptance tests AC-1..AC-4.
//!
//! Expected masks come from `fixtures/out/stat-contam-v1/cases.txt`, written by
//! `fixtures/out/make_stat_fixtures.py`: an independent Python model (exact rational n-sigma
//! with a margin check, a naive all-pairs Qn, its own least-squares fits) of the spec's rules.
//! The small cases below are hand-labelled from the rules; nothing expected here was produced
//! by this crate.
//!
//! AC-3 (cross-target identity): the masks (and the robust centre, scale and limit, bit for
//! bit) are compared against the same pinned fixture on every CI target this test runs on:
//! x86-64 (Linux, Windows), ARM64 (macOS aarch64-apple-darwin) and wasm32-wasip1 under
//! wasmtime (`ci/wasm32-crates.txt` lists `fid-filter`). Equal to one pinned value on each
//! target means identical across targets.

use fid_filter::policy::{
    apply, MethodStatus, OutlierPolicy, PolicyError, PolicyMethod, PolicySpec, ScaleEstimator, Side,
    DEFAULT_MAX_EXCLUDED_FRACTION,
};
use fid_filter::stat::{median, qn_raw, NSigma, RobustScale, StatOutcome, MAD_FACTOR, QN_FACTOR, VERSION};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView, Primitive};
use fid_fit::ls_lin::PlaneLs;
use fid_fit::ls_rnd::{default_params, Circle2Ls};
use fid_num::NumError;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/out/stat-contam-v1/cases.txt");

const PLANE: FittedFeature = FittedFeature::Plane { p: [0.0; 3], n: [0.0, 0.0, 1.0] };

struct Robust {
    estimator: ScaleEstimator,
    side: Side,
    center: f64,
    scale: f64,
    limit: f64,
    mask: Vec<u32>,
}

struct NSig {
    side: Side,
    sd: f64,
    mask: Vec<u32>,
}

struct Case {
    name: String,
    primitive: String,
    xyz: Vec<f64>,
    r: Vec<f64>,
    planted: Vec<u32>,
    robust: Vec<Robust>,
    nsigma: Vec<NSig>,
}

fn hex(v: &str) -> f64 {
    assert_eq!(v.len(), 16, "{v}");
    f64::from_bits(u64::from_str_radix(v, 16).unwrap())
}

fn side(s: &str) -> Side {
    Side::parse(s).unwrap_or_else(|| panic!("side {s}"))
}

fn fixture() -> (f64, Vec<Case>) {
    let text = std::fs::read_to_string(CASES).expect("fixture");
    let mut k = None;
    let mut cases: Vec<Case> = Vec::new();
    let mut cur: Option<(Case, usize)> = None;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let w: Vec<&str> = line.split(' ').collect();
        let idx = |from: usize| w[from..].iter().map(|x| x.parse().unwrap()).collect::<Vec<u32>>();
        match w[0] {
            "k" => k = Some(hex(w[1])),
            "case" => {
                assert!(cur.is_none(), "case inside case");
                let c = Case {
                    name: w[1].to_string(),
                    primitive: w[2].to_string(),
                    xyz: Vec::new(),
                    r: Vec::new(),
                    planted: Vec::new(),
                    robust: Vec::new(),
                    nsigma: Vec::new(),
                };
                cur = Some((c, w[3].parse().unwrap()));
            }
            "end" => {
                let (c, n) = cur.take().expect("end without case");
                assert_eq!(c.xyz.len(), 3 * n, "{}", c.name);
                assert_eq!(c.r.len(), n, "{}", c.name);
                cases.push(c);
            }
            _ => {
                let (c, _) = cur.as_mut().expect("line outside a case");
                match w[0] {
                    "p" => c.xyz.extend([hex(w[1]), hex(w[2]), hex(w[3])]),
                    "r" => c.r.push(hex(w[1])),
                    "planted" => c.planted = idx(1),
                    "robust" => {
                        assert_eq!(w[6], "decided");
                        c.robust.push(Robust {
                            estimator: ScaleEstimator::parse(w[1]).unwrap(),
                            side: side(w[2]),
                            center: hex(w[3]),
                            scale: hex(w[4]),
                            limit: hex(w[5]),
                            mask: idx(7),
                        });
                    }
                    "nsigma" => {
                        assert_eq!(w[3], "decided");
                        c.nsigma.push(NSig { side: side(w[1]), sd: hex(w[2]), mask: idx(4) });
                    }
                    other => panic!("unknown line {other}"),
                }
            }
        }
    }
    assert!(cur.is_none(), "unterminated case");
    (k.expect("k"), cases)
}

fn same_bits(a: f64, b: f64, what: &str) {
    assert_eq!(a.to_bits(), b.to_bits(), "{what}: {a:e} != {b:e}");
}

#[test]
fn fixture_is_what_the_spec_names() {
    let (k, cases) = fixture();
    assert_eq!(k, 3.0);
    assert_eq!(cases.len(), 4);
    for c in &cases {
        let n = c.r.len();
        // 2 % gross outliers, planted at distinct ascending indices.
        assert_eq!(c.planted.len(), n * 2 / 100, "{}", c.name);
        assert!(c.planted.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(c.robust.len(), 6);
        assert_eq!(c.nsigma.len(), 3);
    }
    assert!(cases.iter().any(|c| c.primitive == "plane") && cases.iter().any(|c| c.primitive == "circle2"));
}

/// The fixture's residuals are the least-squares residuals: fid-fit's plane and circle fits of
/// the same points give the same values (the plane's normal sign is arbitrary, so its
/// residuals may all flip). The policies below run on the committed residuals.
#[test]
fn fixture_residuals_are_the_least_squares_residuals() {
    let (_, cases) = fixture();
    for c in &cases {
        let pts = PointsView::new(&c.xyz, None).unwrap();
        let fit = match c.primitive.as_str() {
            "plane" => PlaneLs.fit(&pts, &FitParams::new(Method::LeastSquares), None).unwrap(),
            "circle2" => Circle2Ls.fit(&pts, &default_params(), None).unwrap(),
            other => panic!("primitive {other}"),
        };
        let flip = c.primitive == "plane" && {
            let same: f64 = fit.residuals.iter().zip(&c.r).map(|(a, b)| (a - b).abs()).sum();
            let opposite: f64 = fit.residuals.iter().zip(&c.r).map(|(a, b)| (a + b).abs()).sum();
            opposite < same
        };
        for (i, (&got, &want)) in fit.residuals.iter().zip(&c.r).enumerate() {
            let got = if flip { -got } else { got };
            assert!((got - want).abs() <= 1e-9, "{} point {i}: {got:e} vs {want:e}", c.name);
        }
    }
}

fn check_robust(c: &Case, k: f64, e: &Robust) -> StatOutcome {
    let m = RobustScale::new(k, e.estimator, e.side).unwrap();
    let o = m.evaluate(&c.r).unwrap();
    let what = format!("{} {} {}", c.name, e.estimator.slug(), e.side.slug());
    assert_eq!(o.status, MethodStatus::Decided, "{what}");
    same_bits(o.center, e.center, &what);
    same_bits(o.scale, e.scale, &what);
    same_bits(o.threshold, e.limit, &what);
    assert_eq!(o.mask.point_count(), c.r.len());
    assert_eq!(o.mask.indices(), e.mask.as_slice(), "{what}");
    o
}

/// AC-1 (and AC-3 on every target): the robust-scale policy with k = 3, both sides, excludes
/// every planted 50-sigma outlier and at most 1 % of the clean points, with MAD and with Qn.
#[test]
fn ac1_robust_scale_finds_every_planted_outlier() {
    let (k, cases) = fixture();
    for c in &cases {
        let clean = c.r.len() - c.planted.len();
        for e in c.robust.iter().filter(|e| e.side == Side::Both) {
            let o = check_robust(c, k, e);
            for p in &c.planted {
                assert!(o.mask.contains(*p), "{} {}: planted {p} kept", c.name, e.estimator.slug());
            }
            let false_ex = o.mask.indices().iter().filter(|i| !c.planted.contains(i)).count();
            assert!(false_ex * 100 <= clean, "{} {}: {false_ex} of {clean} clean excluded", c.name, e.estimator.slug());
        }
    }
}

/// REQ-003 (and AC-3): the one-sided variants equal the independent model too, and are the
/// both-sides mask split by the sign of the deviation from the median.
#[test]
fn robust_sides_match_the_model() {
    let (k, cases) = fixture();
    for c in &cases {
        for est in ScaleEstimator::ALL {
            let get = |s: Side| c.robust.iter().find(|e| e.estimator == *est && e.side == s).unwrap();
            let both = check_robust(c, k, get(Side::Both));
            let pos = check_robust(c, k, get(Side::Positive));
            let neg = check_robust(c, k, get(Side::Negative));
            let mut union: Vec<u32> = pos.mask.indices().iter().chain(neg.mask.indices()).copied().collect();
            union.sort_unstable();
            assert_eq!(union, both.mask.indices());
            assert!(pos.mask.indices().iter().all(|&i| c.r[i as usize] - both.center > 0.0));
            assert!(neg.mask.indices().iter().all(|&i| c.r[i as usize] - both.center < 0.0));
        }
    }
}

/// AC-2 (and AC-3): the n-sigma policy with k = 3 gives, index for index, the mask of the
/// independent exact-rational model, on every side; its scale is the population standard
/// deviation.
#[test]
fn ac2_nsigma_equals_the_independent_mask() {
    let (k, cases) = fixture();
    for c in &cases {
        for e in &c.nsigma {
            let o = NSigma::new(k, e.side).unwrap().evaluate(&c.r).unwrap();
            let what = format!("{} nsigma {}", c.name, e.side.slug());
            assert_eq!(o.status, MethodStatus::Decided, "{what}");
            assert_eq!(o.center.to_bits(), 0.0_f64.to_bits());
            assert!((o.scale - e.sd).abs() <= 1e-12 * e.sd, "{what}: sd {:e} vs {:e}", o.scale, e.sd);
            same_bits(o.threshold, k * o.scale, &what);
            assert_eq!(o.mask.indices(), e.mask.as_slice(), "{what}");
        }
    }
}

/// AC-4: 50 identical residuals: no point excluded and `zero_scale` reported, by every method
/// and side, and through `apply` the record says so.
#[test]
fn ac4_identical_residuals_report_zero_scale() {
    for value in [1.25e-5, 0.0, -3.0e-7, 5e-324] {
        let r = vec![value; 50];
        let mut outcomes = Vec::new();
        for &s in Side::ALL {
            outcomes.push(NSigma::new(3.0, s).unwrap().evaluate(&r).unwrap());
            for &est in ScaleEstimator::ALL {
                outcomes.push(RobustScale::new(3.0, est, s).unwrap().evaluate(&r).unwrap());
            }
        }
        for o in outcomes {
            assert_eq!(o.status, MethodStatus::ZeroScale, "{value:e}");
            assert_eq!(o.status.code(), "zero_scale");
            assert!(o.mask.is_empty());
            assert_eq!(o.mask.point_count(), 50);
            assert_eq!((o.scale, o.threshold), (0.0, 0.0));
        }
    }
    // Signed zeros are equal residuals too.
    let r: Vec<f64> = (0..50).map(|i| if i % 2 == 0 { 0.0 } else { -0.0 }).collect();
    let o = NSigma::new(3.0, Side::Both).unwrap().evaluate(&r).unwrap();
    assert_eq!(o.status, MethodStatus::ZeroScale);

    // Through apply: nothing excluded, and the record says why.
    let xyz: Vec<f64> = (0..50).flat_map(|i| [i as f64, (i * 7 % 11) as f64, 0.0]).collect();
    let pts = PointsView::new(&xyz, None).unwrap();
    let r = vec![2.0e-6; 50];
    let m = RobustScale::new(3.0, ScaleEstimator::Qn, Side::Both).unwrap();
    let spec = PolicySpec {
        policy: m.policy(),
        max_excluded_fraction: DEFAULT_MAX_EXCLUDED_FRACTION,
        target: Primitive::Plane,
    };
    let red = apply(&spec, &m, &pts, &PLANE, &r, None).unwrap();
    assert!(!red.is_reduced());
    assert_eq!(red.exclusion().excluded_count(), 0);
    assert_eq!(red.exclusion().status(), MethodStatus::ZeroScale);
}

/// REQ-004: a zero scale with residuals that are not all equal (more than half equal to the
/// median, so the MAD and the Qn order statistic are zero) excludes nothing either, instead
/// of excluding every other point against a zero limit. Hand-labelled: 30 zeros and 20
/// distinct values; the median is 0, 30 of 50 absolute deviations are 0, and
/// C(30, 2) = 435 >= k = C(26, 2) = 325 differences are 0.
#[test]
fn zero_mad_and_qn_with_unequal_residuals_report_zero_scale() {
    let mut r = vec![0.0; 30];
    r.extend((1..=20).map(|i| i as f64 * 1e-6));
    assert_eq!(qn_raw(&r).unwrap(), 0.0);
    for &est in ScaleEstimator::ALL {
        let o = RobustScale::new(3.0, est, Side::Both).unwrap().evaluate(&r).unwrap();
        assert_eq!(o.status, MethodStatus::ZeroScale, "{}", est.slug());
        assert!(o.mask.is_empty());
        assert_eq!(o.center, 0.0);
    }
    // The standard deviation is not zero there, so n-sigma decides.
    let o = NSigma::new(3.0, Side::Both).unwrap().evaluate(&r).unwrap();
    assert_eq!(o.status, MethodStatus::Decided);
}

/// REQ-004: a subnormal scale whose limit `k * scale` rounds to zero is a zero scale too, not
/// a zero limit that would exclude every point off the centre. Hand-labelled with
/// a = 5e-324 and k = 0.25: residuals -a 0 a give a MAD scale of 1.4826 a = a, a standard
/// deviation of about 0.82 a = a and a Qn scale of 2.219 a = 2a, so every limit is
/// 0.25 a or 0.5 a, which rounds to 0.
#[test]
fn limit_rounding_to_zero_reports_zero_scale() {
    let a = 5e-324;
    let r = [-a, 0.0, a];
    let mut outcomes = vec![NSigma::new(0.25, Side::Both).unwrap().evaluate(&r).unwrap()];
    for &est in ScaleEstimator::ALL {
        outcomes.push(RobustScale::new(0.25, est, Side::Both).unwrap().evaluate(&r).unwrap());
    }
    for o in outcomes {
        assert_eq!(o.status, MethodStatus::ZeroScale);
        assert!(o.mask.is_empty());
        assert_eq!((o.scale, o.threshold), (0.0, 0.0));
    }
    // With k = 3 the limit is 3a: positive, so the method decides (and nothing is beyond it).
    let o = RobustScale::new(3.0, ScaleEstimator::Mad, Side::Both).unwrap().evaluate(&r).unwrap();
    assert_eq!(o.status, MethodStatus::Decided);
    same_bits(o.threshold, 3.0 * a, "limit");
    assert!(o.mask.is_empty());
}

/// The Qn factor is the exact normal-consistency constant 1 / (sqrt(2) Phi^-1(5/8)),
/// correctly rounded (not the 2.2219 printed in 1993).
#[test]
fn qn_factor_is_the_exact_constant() {
    assert_eq!(QN_FACTOR.to_bits(), 0x4001_c0ce_d054_0ad3);
    assert_eq!(MAD_FACTOR, 1.4826);
}

/// Hand-labelled medians and Qn order statistics.
#[test]
fn median_and_qn_hand_cases() {
    assert_eq!(median(&[3.0, 1.0, 2.0]).unwrap(), 2.0);
    assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]).unwrap(), 2.5);
    assert_eq!(median(&[7.0]).unwrap(), 7.0);
    // The two middle values are halved before they are added, so this does not overflow.
    assert_eq!(median(&[f64::MAX, f64::MAX]).unwrap(), f64::MAX);
    // n = 4: h = 3, k = 3; differences 1 2 3 4 6 7, the third is 3.
    assert_eq!(qn_raw(&[8.0, 1.0, 4.0, 2.0]).unwrap(), 3.0);
    // n = 3: h = 2, k = 1; the smallest difference.
    assert_eq!(qn_raw(&[5.0, 1.0, 3.5]).unwrap(), 1.5);
    // n = 2: the one difference.
    assert_eq!(qn_raw(&[0.0, -1.0]).unwrap(), 1.0);
    // n = 5: h = 3, k = 3; differences of 0 1 3 7 15: 1 2 3 4 6 7 8 12 14 15, the third is 3.
    assert_eq!(qn_raw(&[15.0, 0.0, 7.0, 1.0, 3.0]).unwrap(), 3.0);
    assert_eq!(qn_raw(&[1.0]), Err(PolicyError::InvalidParameter { field: "residuals", reason: "too_few_for_qn" }));
    assert!(matches!(median(&[]), Err(PolicyError::Num(NumError::EmptyInput { .. }))));
    assert!(matches!(median(&[1.0, f64::NAN]), Err(PolicyError::Num(NumError::NotANumber { .. }))));
}

/// REQ-001 hand case: residuals nine zeros and one 10: mean 1, population variance
/// (9 * 1 + 81) / 10 = 9, sd 3, so k = 3 gives the limit 9 and only index 9 is beyond it;
/// k = 3.5 gives 10.5 and nothing is. The negative side never excludes a positive residual.
#[test]
fn nsigma_hand_case() {
    let mut r = vec![0.0; 10];
    r[9] = 10.0;
    let o = NSigma::new(3.0, Side::Both).unwrap().evaluate(&r).unwrap();
    assert!((o.scale - 3.0).abs() <= 1e-15);
    assert_eq!(o.mask.indices(), &[9]);
    assert_eq!(NSigma::new(3.0, Side::Positive).unwrap().evaluate(&r).unwrap().mask.indices(), &[9]);
    let neg = NSigma::new(3.0, Side::Negative).unwrap().evaluate(&r).unwrap();
    assert!(neg.mask.is_empty());
    assert_eq!(neg.status, MethodStatus::Decided);
    let wide = NSigma::new(3.5, Side::Both).unwrap().evaluate(&r).unwrap();
    assert!(wide.mask.is_empty());
    assert_eq!(wide.status, MethodStatus::Decided);
    // n-sigma measures from zero, not from the mean: shifted residuals are a different case.
    let shifted: Vec<f64> = r.iter().map(|x| x - 100.0).collect();
    let o = NSigma::new(3.0, Side::Both).unwrap().evaluate(&shifted).unwrap();
    assert_eq!(o.mask.len(), 10);
}

/// A point exactly at the limit is kept; one ulp beyond it is excluded. Hand-labelled with
/// k = 2 (a power of two, so the limit is exact): residuals -1 0 0 1 x have median 0 and
/// absolute deviations 0 0 1 1 |x|, so the MAD is 1, the scale 1.4826 and the limit
/// 2 * 1.4826.
#[test]
fn robust_limit_is_inclusive_of_kept_points() {
    let limit = 2.0 * MAD_FACTOR;
    let m = RobustScale::new(2.0, ScaleEstimator::Mad, Side::Both).unwrap();
    let at = m.evaluate(&[-1.0, 0.0, 0.0, 1.0, limit]).unwrap();
    same_bits(at.scale, MAD_FACTOR, "scale");
    same_bits(at.threshold, limit, "limit");
    assert!(at.mask.is_empty());
    let beyond = f64::from_bits(limit.to_bits() + 1);
    assert_eq!(m.evaluate(&[-1.0, 0.0, 0.0, 1.0, beyond]).unwrap().mask.indices(), &[4]);
    assert_eq!(m.evaluate(&[-1.0, 0.0, 0.0, 1.0, -beyond]).unwrap().mask.indices(), &[4]);
    let pos = RobustScale::new(2.0, ScaleEstimator::Mad, Side::Positive).unwrap();
    assert!(pos.evaluate(&[-1.0, 0.0, 0.0, 1.0, -beyond]).unwrap().mask.is_empty());
    let neg = RobustScale::new(2.0, ScaleEstimator::Mad, Side::Negative).unwrap();
    assert_eq!(neg.evaluate(&[-1.0, 0.0, 0.0, 1.0, -beyond]).unwrap().mask.indices(), &[4]);
    // Qn of the same five values: differences 0 1 1 1 1 2 x-1 x x x+1 (x = 2 * 1.4826), the
    // third is 1, so the scale is the bare factor too.
    let q = RobustScale::new(2.0, ScaleEstimator::Qn, Side::Both).unwrap().evaluate(&[-1.0, 0.0, 0.0, 1.0, limit]);
    same_bits(q.unwrap().scale, QN_FACTOR, "qn scale");
}

/// Typed parameters with no default (REQ-KRN.OUT-001, REQ-003): `k` is validated with the
/// `KRN.OUT.POL` rules; the side and the estimator are required arguments.
#[test]
fn parameters_are_validated() {
    let bad = |k: f64, reason: &'static str| {
        assert_eq!(NSigma::new(k, Side::Both).unwrap_err(), PolicyError::InvalidParameter { field: "k", reason });
        assert_eq!(
            RobustScale::new(k, ScaleEstimator::Mad, Side::Both).unwrap_err(),
            PolicyError::InvalidParameter { field: "k", reason }
        );
    };
    bad(0.0, "out_of_range");
    bad(-0.0, "out_of_range");
    bad(-3.0, "out_of_range");
    bad(f64::NAN, "non_finite");
    bad(f64::INFINITY, "non_finite");
    bad(5e-324, "subnormal");
    let m = RobustScale::new(2.5, ScaleEstimator::Qn, Side::Negative).unwrap();
    assert_eq!((m.k(), m.estimator(), m.side()), (2.5, ScaleEstimator::Qn, Side::Negative));
    assert_eq!(m.policy(), OutlierPolicy::RobustScale { k: 2.5, scale: ScaleEstimator::Qn, side: Side::Negative });
    let n = NSigma::new(3.0, Side::Positive).unwrap();
    assert_eq!((n.k(), n.side()), (3.0, Side::Positive));
    assert_eq!(VERSION, (1, 0, 0));
    assert_eq!(n.algorithm_id().to_string(), "filter.outlier.nsigma@1.0.0");
    assert_eq!(m.algorithm_id().to_string(), "filter.outlier.robust_scale@1.0.0");
}

type Evaluate = dyn Fn(&[f64]) -> Result<StatOutcome, PolicyError>;

/// Degenerate and unusable input is refused with a typed error, never mis-measured.
#[test]
fn bad_input_is_refused() {
    let methods: Vec<Box<Evaluate>> = vec![
        Box::new(|r: &[f64]| NSigma::new(3.0, Side::Both).unwrap().evaluate(r)),
        Box::new(|r: &[f64]| RobustScale::new(3.0, ScaleEstimator::Mad, Side::Both).unwrap().evaluate(r)),
        Box::new(|r: &[f64]| RobustScale::new(3.0, ScaleEstimator::Qn, Side::Both).unwrap().evaluate(r)),
    ];
    for m in &methods {
        assert!(matches!(m(&[]), Err(PolicyError::Num(NumError::EmptyInput { .. }))));
        assert!(matches!(m(&[1.0, f64::NAN, 2.0]), Err(PolicyError::Num(NumError::NotANumber { index: Some(1), .. }))));
        assert!(matches!(
            m(&[f64::NEG_INFINITY, 2.0]),
            Err(PolicyError::Num(NumError::Infinite { index: Some(0), .. }))
        ));
        // A spread that overflows binary64 is an error, never an infinite or a zero limit.
        let e = m(&[-f64::MAX, f64::MAX, 0.0]).unwrap_err();
        assert!(matches!(e, PolicyError::Num(NumError::Infinite { .. })), "{e}");
        assert_eq!(e.code(), "num");
    }
    assert_eq!(
        RobustScale::new(3.0, ScaleEstimator::Qn, Side::Both).unwrap().evaluate(&[1.0]).unwrap_err(),
        PolicyError::InvalidParameter { field: "residuals", reason: "too_few_for_qn" }
    );
    // One residual: no spread, so MAD and n-sigma report a zero scale.
    for o in [
        NSigma::new(3.0, Side::Both).unwrap().evaluate(&[4.0]).unwrap(),
        RobustScale::new(3.0, ScaleEstimator::Mad, Side::Both).unwrap().evaluate(&[4.0]).unwrap(),
    ] {
        assert_eq!(o.status, MethodStatus::ZeroScale);
        assert!(o.mask.is_empty());
    }
}

/// Through the trait: nominal distances are refused (never silently ignored) and there must
/// be one residual per point; `exclude` and `exclude_reported` give the same mask.
#[test]
fn trait_path_checks_its_evidence() {
    let (k, cases) = fixture();
    let c = &cases[0];
    let pts = PointsView::new(&c.xyz, None).unwrap();
    let m = RobustScale::new(k, ScaleEstimator::Mad, Side::Both).unwrap();
    assert_eq!(
        m.exclude(&pts, &PLANE, &c.r, Some(&c.r)).unwrap_err(),
        PolicyError::NominalDistances { required: false }
    );
    assert_eq!(
        NSigma::new(k, Side::Both).unwrap().exclude(&pts, &PLANE, &c.r[1..], None).unwrap_err(),
        PolicyError::LengthMismatch { what: "residuals", expected: c.r.len(), got: c.r.len() - 1 }
    );
    let (mask, status) = m.exclude_reported(&pts, &PLANE, &c.r, None).unwrap();
    assert_eq!(status, MethodStatus::Decided);
    assert_eq!(mask, m.exclude(&pts, &PLANE, &c.r, None).unwrap());
    assert_eq!(mask, m.evaluate(&c.r).unwrap().mask);
}

/// `apply` runs both methods under the guards and records the method's status with the
/// exclusion; a spec whose parameters differ from the method's is refused.
#[test]
fn apply_runs_both_methods_under_the_guards() {
    let (k, cases) = fixture();
    for c in &cases {
        let pts = PointsView::new(&c.xyz, None).unwrap();
        let target = if c.primitive == "plane" { Primitive::Plane } else { Primitive::Circle2 };
        let methods: Vec<(Box<dyn PolicyMethod>, Vec<u32>)> = vec![
            (Box::new(NSigma::new(k, Side::Both).unwrap()), c.nsigma[0].mask.clone()),
            (
                Box::new(RobustScale::new(k, ScaleEstimator::Mad, Side::Both).unwrap()),
                c.robust
                    .iter()
                    .find(|e| e.estimator == ScaleEstimator::Mad && e.side == Side::Both)
                    .unwrap()
                    .mask
                    .clone(),
            ),
            (
                Box::new(RobustScale::new(k, ScaleEstimator::Qn, Side::Both).unwrap()),
                c.robust
                    .iter()
                    .find(|e| e.estimator == ScaleEstimator::Qn && e.side == Side::Both)
                    .unwrap()
                    .mask
                    .clone(),
            ),
        ];
        assert_eq!(c.nsigma[0].side, Side::Both);
        for (m, want) in &methods {
            let spec = PolicySpec { policy: m.policy(), max_excluded_fraction: DEFAULT_MAX_EXCLUDED_FRACTION, target };
            let red = apply(&spec, m.as_ref(), &pts, &PLANE, &c.r, None).unwrap();
            let ex = red.exclusion();
            assert_eq!(ex.excluded(), want.as_slice(), "{}", c.name);
            assert_eq!(ex.status(), MethodStatus::Decided);
            assert_eq!(ex.status().code(), "decided");
            assert_eq!(ex.params(), spec.encode().unwrap().as_slice());
            assert_eq!(red.kept_indices().len(), c.r.len() - want.len());
            // Nominal distances are refused by apply before the method runs.
            assert_eq!(
                apply(&spec, m.as_ref(), &pts, &PLANE, &c.r, Some(&c.r)).unwrap_err(),
                PolicyError::NominalDistances { required: false }
            );
            // A ceiling below the excluded fraction refuses the reduction instead of fitting.
            let tight = PolicySpec { max_excluded_fraction: 0.01, ..spec };
            assert!(matches!(
                apply(&tight, m.as_ref(), &pts, &PLANE, &c.r, None),
                Err(PolicyError::ExcessiveExclusion { .. })
            ));
        }
        let m = NSigma::new(k, Side::Both).unwrap();
        let other = PolicySpec {
            policy: OutlierPolicy::NSigma { k: 2.0, side: Side::Both },
            max_excluded_fraction: DEFAULT_MAX_EXCLUDED_FRACTION,
            target,
        };
        assert_eq!(
            apply(&other, &m, &pts, &PLANE, &c.r, None).unwrap_err(),
            PolicyError::InvalidParameter { field: "policy", reason: "method_mismatch" }
        );
    }
}
