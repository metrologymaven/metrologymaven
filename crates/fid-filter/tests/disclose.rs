#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! KRN.OUT.DISC acceptance tests AC-1..AC-4.
//!
//! Expected bytes, indices, digests and deltas come from `fixtures/out/disc-record-v1/`,
//! written by `fixtures/out/make_disc_fixtures.py`: an independent Python model (its own plane
//! fit, statistics, gate, SHA-256 and record encoding) of the spec's rules. The small cases
//! below are hand-labelled from the rules; nothing expected here was produced by this crate.
//!
//! AC-2 (cross-target identity): the record bytes are compared against the same pinned golden
//! files on every CI target this test runs on: x86-64 (Linux, Windows), ARM64 (macOS
//! aarch64-apple-darwin) and wasm32-wasip1 under wasmtime (`ci/wasm32-crates.txt` lists
//! `fid-filter`). Equal to one pinned value on each target means identical across targets.

use fid_filter::disclose::{
    DiscloseError, ExclusionRecord, FitSnapshot, ImpactDisclosure, AFTER, BEFORE, RECORD_FORMAT,
};
use fid_filter::gate::{NominalGate, VERSION};
use fid_filter::policy::{
    apply, outlier_algorithm_id, MethodStatus, OutlierPolicy, PolicyError, PolicyKind, PolicySpec,
};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView, Primitive, ResidualStats};
use fid_fit::ls_lin::PlaneLs;
use fid_kci::kci::sha256::sha256_digest;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/out/disc-record-v1/");

struct Raw {
    count: usize,
    gate: f64,
    empty_gate: f64,
    fraction: f64,
    xyz: Vec<f64>,
    d: Vec<f64>,
    res: Vec<f64>,
    before: Option<FitSnapshot>,
    after: Option<FitSnapshot>,
    delta: Vec<f64>,
    stats_delta: Vec<f64>,
    excluded: Vec<u32>,
    digest: String,
    empty_digest: String,
}

fn hex(v: &str) -> f64 {
    assert_eq!(v.len(), 16, "{v}");
    f64::from_bits(u64::from_str_radix(v, 16).unwrap())
}

fn hexes(w: &[&str]) -> Vec<f64> {
    w.iter().map(|x| hex(x)).collect()
}

fn snapshot(w: &[&str]) -> FitSnapshot {
    assert_eq!(w.len(), 12, "{w:?}");
    let v = hexes(&w[..6]);
    let s = hexes(&w[7..]);
    (
        FittedFeature::Plane { p: [v[0], v[1], v[2]], n: [v[3], v[4], v[5]] },
        ResidualStats { n: w[6].parse().unwrap(), rms: s[0], max: s[1], min: s[2], mean: s[3], std_dev: s[4] },
    )
}

struct Fixture {
    count: usize,
    gate: f64,
    empty_gate: f64,
    fraction: f64,
    xyz: Vec<f64>,
    d: Vec<f64>,
    res: Vec<f64>,
    before: FitSnapshot,
    after: FitSnapshot,
    delta: Vec<f64>,
    stats_delta: Vec<f64>,
    excluded: Vec<u32>,
    digest: String,
    empty_digest: String,
}

fn fixture() -> Fixture {
    let text = std::fs::read_to_string(format!("{DIR}cases.txt")).expect("fixture");
    let mut f = Raw {
        count: 0,
        gate: 0.0,
        empty_gate: 0.0,
        fraction: 0.0,
        xyz: Vec::new(),
        d: Vec::new(),
        res: Vec::new(),
        before: None,
        after: None,
        delta: Vec::new(),
        stats_delta: Vec::new(),
        excluded: Vec::new(),
        digest: String::new(),
        empty_digest: String::new(),
    };
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "count" => f.count = w[1].parse().unwrap(),
            "max_distance_m" => f.gate = hex(w[1]),
            "empty_max_distance_m" => f.empty_gate = hex(w[1]),
            "max_excluded_fraction" => f.fraction = hex(w[1]),
            "pt" => f.xyz.extend(hexes(&w[1..])),
            "d" => f.d = hexes(&w[1..]),
            "res" => f.res = hexes(&w[1..]),
            "before" => f.before = Some(snapshot(&w[1..])),
            "after" => f.after = Some(snapshot(&w[1..])),
            "delta" => f.delta = hexes(&w[1..]),
            "stats_delta" => f.stats_delta = hexes(&w[1..]),
            "excluded" => f.excluded = w[1..].iter().map(|x| x.parse().unwrap()).collect(),
            "digest" => f.digest = w[1].to_string(),
            "empty_digest" => f.empty_digest = w[1].to_string(),
            other => panic!("unknown fixture line {other}"),
        }
    }
    assert_eq!(f.xyz.len(), 3 * f.count);
    assert_eq!(f.d.len(), f.count);
    assert_eq!(f.res.len(), f.count);
    Fixture {
        count: f.count,
        gate: f.gate,
        empty_gate: f.empty_gate,
        fraction: f.fraction,
        xyz: f.xyz,
        d: f.d,
        res: f.res,
        before: f.before.expect("before"),
        after: f.after.expect("after"),
        delta: f.delta,
        stats_delta: f.stats_delta,
        excluded: f.excluded,
        digest: f.digest,
        empty_digest: f.empty_digest,
    }
}

fn golden(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}{name}")).expect("golden")
}

fn gate_spec(gate: f64, fraction: f64) -> PolicySpec {
    PolicySpec {
        policy: OutlierPolicy::NominalGate { max_distance_m: gate },
        max_excluded_fraction: fraction,
        target: Primitive::Plane,
    }
}

fn gate_id() -> fid_fit::contract::AlgorithmId {
    outlier_algorithm_id(PolicyKind::NominalGate, VERSION)
}

/// The record the fixture describes, through the real pipeline: the nominal gate run by
/// `apply`, then the record of its exclusion with the fixture's fits.
fn record(
    f: &Fixture,
    gate: f64,
    impact: Option<(FitSnapshot, FitSnapshot)>,
) -> Result<ExclusionRecord, DiscloseError> {
    let pts = PointsView::new(&f.xyz, None).unwrap();
    let spec = gate_spec(gate, f.fraction);
    let method = NominalGate::new(gate).unwrap();
    let reduced = apply(&spec, &method, &pts, &f.before.0, &f.res, Some(&f.d)).unwrap();
    ExclusionRecord::from_exclusion(reduced.exclusion(), impact)
}

fn both(f: &Fixture) -> Option<(FitSnapshot, FitSnapshot)> {
    Some((f.before.clone(), f.after.clone()))
}

fn digest_input(count: u32, excluded: &[u32]) -> Vec<u8> {
    let mut b = count.to_le_bytes().to_vec();
    for i in excluded {
        b.extend_from_slice(&i.to_le_bytes());
    }
    b
}

// ----- AC-1: golden bytes of a plane fit with 3 excluded points; indices sorted.

#[test]
fn ac1_golden_record_bytes() {
    let f = fixture();
    let r = record(&f, f.gate, both(&f)).unwrap();
    assert_eq!(r.excluded(), &f.excluded[..]);
    assert_eq!(r.excluded_count(), 3);
    assert_eq!(r.point_count(), f.count);
    assert_eq!(r.status(), MethodStatus::Decided);
    assert_eq!(r.digest().as_str(), f.digest);
    let input = r.digest_input().unwrap();
    assert_eq!(input, digest_input(f.count as u32, &f.excluded));
    assert_eq!(sha256_digest(&input).as_str(), f.digest);
    let bytes = r.encode().unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), String::from_utf8(golden("record.bin")).unwrap());
    assert_eq!(bytes, golden("record.bin"));
    assert!(bytes.starts_with(format!("{{\"format\":\"{RECORD_FORMAT}\"").as_bytes()));
}

#[test]
fn ac1_indices_are_sorted_whatever_the_input_order() {
    let f = fixture();
    let params = gate_spec(f.gate, f.fraction).encode().unwrap();
    let r = ExclusionRecord::new(gate_id(), params, f.count, vec![17, 4, 11], MethodStatus::Decided, both(&f)).unwrap();
    assert_eq!(r.excluded(), &[4, 11, 17]);
    assert_eq!(r.encode().unwrap(), golden("record.bin"));
}

#[test]
fn ac1_fixture_is_a_genuine_plane_fit() {
    // The fixture's fits are inputs of the record; check that fid-fit's PlaneLs agrees with
    // them (to a tolerance: the Python fit is a second implementation).
    let f = fixture();
    let check = |xyz: &[f64], snap: &FitSnapshot| {
        let pts = PointsView::new(xyz, None).unwrap();
        let fit = PlaneLs.fit(&pts, &FitParams::new(Method::LeastSquares), None).unwrap();
        let (FittedFeature::Plane { p, n }, FittedFeature::Plane { p: ep, n: en }) = (&fit.feature, &snap.0) else {
            panic!("not planes");
        };
        for k in 0..3 {
            assert!((n[k] - en[k]).abs() < 1e-9, "normal {k}: {} vs {}", n[k], en[k]);
        }
        let offset: f64 = (0..3).map(|k| (p[k] - ep[k]) * en[k]).sum();
        assert!(offset.abs() < 1e-12, "offset {offset}");
        assert_eq!(fit.stats.n, snap.1.n);
        assert!((fit.stats.rms - snap.1.rms).abs() <= 1e-9 * snap.1.rms, "rms");
        assert!((fit.stats.std_dev - snap.1.std_dev).abs() <= 1e-9 * snap.1.std_dev, "std_dev");
    };
    check(&f.xyz, &f.before);
    let kept: Vec<f64> = f
        .xyz
        .chunks_exact(3)
        .enumerate()
        .filter(|(i, _)| !f.excluded.contains(&(*i as u32)))
        .flat_map(|(_, p)| p.to_vec())
        .collect();
    check(&kept, &f.after);
}

// ----- AC-2: identical bytes on every target (see the module documentation) and stable.

#[test]
fn ac2_encoding_is_stable() {
    let f = fixture();
    let a = record(&f, f.gate, both(&f)).unwrap().encode().unwrap();
    let b = record(&f, f.gate, both(&f)).unwrap().encode().unwrap();
    assert_eq!(a, b);
    assert_eq!(a, golden("record.bin"));
    assert_eq!(record(&f, f.empty_gate, None).unwrap().encode().unwrap(), golden("empty.bin"));
}

// ----- AC-3: an empty mask has excluded_count 0 and no impact disclosure.

#[test]
fn ac3_empty_mask_has_no_impact() {
    let f = fixture();
    let r = record(&f, f.empty_gate, None).unwrap();
    assert_eq!(r.excluded_count(), 0);
    assert!(r.excluded().is_empty());
    assert!(r.impact().is_none());
    assert_eq!(r.digest().as_str(), f.empty_digest);
    assert_eq!(r.digest_input().unwrap(), digest_input(f.count as u32, &[]));
    let bytes = r.encode().unwrap();
    assert_eq!(bytes, golden("empty.bin"));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"excluded_count\":\"0\",\"excluded\":[]"), "{text}");
    assert!(text.ends_with("\"impact\":null}"), "{text}");
}

#[test]
fn ac3_empty_mask_refuses_an_impact() {
    let f = fixture();
    assert_eq!(record(&f, f.empty_gate, both(&f)).unwrap_err(), DiscloseError::ImpactUnexpected);
}

#[test]
fn ac3_zero_scale_status_is_recorded() {
    // Hand-labelled: a residual-statistics method that had no scale reports zero_scale with
    // an empty set; the record keeps the reason.
    let spec = PolicySpec {
        policy: OutlierPolicy::NSigma { k: 3.0, side: fid_filter::policy::Side::Both },
        max_excluded_fraction: 0.25,
        target: Primitive::Plane,
    };
    let id = outlier_algorithm_id(PolicyKind::NSigma, (1, 0, 0));
    let r = ExclusionRecord::new(id, spec.encode().unwrap(), 10, Vec::new(), MethodStatus::ZeroScale, None).unwrap();
    assert_eq!(r.status(), MethodStatus::ZeroScale);
    let text = String::from_utf8(r.encode().unwrap()).unwrap();
    assert!(text.contains("\"status\":\"zero_scale\",\"impact\":null}"), "{text}");
}

// ----- AC-4: each difference is after minus before, bit-equal to a test-side computation.

#[test]
fn ac4_deltas_are_exact() {
    let f = fixture();
    let r = record(&f, f.gate, both(&f)).unwrap();
    let d = r.impact().expect("impact");
    let (FittedFeature::Plane { p: bp, n: bn }, FittedFeature::Plane { p: ap, n: an }) = (&f.before.0, &f.after.0)
    else {
        panic!("planes");
    };
    let before: Vec<f64> = bp.iter().chain(bn).copied().collect();
    let after: Vec<f64> = ap.iter().chain(an).copied().collect();
    let expected: Vec<u64> = after.iter().zip(&before).map(|(a, b)| (a - b).to_bits()).collect();
    let got: Vec<u64> = d.delta().iter().map(|x| x.to_bits()).collect();
    assert_eq!(got, expected);
    assert_eq!(got, f.delta.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    let (sb, sa) = (&f.before.1, &f.after.1);
    let s = d.stats_delta();
    let pairs = [
        (s.rms, sa.rms - sb.rms),
        (s.max, sa.max - sb.max),
        (s.min, sa.min - sb.min),
        (s.mean, sa.mean - sb.mean),
        (s.std_dev, sa.std_dev - sb.std_dev),
    ];
    for (k, (got, want)) in pairs.iter().enumerate() {
        assert_eq!(got.to_bits(), want.to_bits(), "stat {k}");
        assert_eq!(got.to_bits(), f.stats_delta[k].to_bits(), "stat {k}");
    }
    assert!(d.opposed_directions().is_empty());
    assert_eq!(d.before(), &f.before);
    assert_eq!(d.after(), &f.after);
}

fn stats(n: usize) -> ResidualStats {
    ResidualStats { n, rms: 1e-6, max: 2e-6, min: -2e-6, mean: 0.0, std_dev: 1e-6 }
}

#[test]
fn ac4_parameter_order_follows_the_encoding() {
    // Hand-labelled: cylinder p, d, r; a spline's degree is not a parameter, knots then
    // control points are; -0.0 after +0.0 gives -0.0.
    let cyl = |p: [f64; 3], r: f64| FittedFeature::Cylinder { p, d: [0.0, 0.0, 1.0], r };
    let d =
        ImpactDisclosure::new((cyl([1.0, 2.0, 3.0], 0.5), stats(10)), (cyl([1.5, 2.0, 2.0], 0.25), stats(8))).unwrap();
    assert_eq!(d.delta(), &[0.5, 0.0, -1.0, 0.0, 0.0, 0.0, -0.25]);
    let spline = |k: f64, c: f64| FittedFeature::Spline {
        degree: 1,
        knots: vec![0.0, 0.0, k, 1.0, 1.0],
        control: vec![[0.0, 0.0, 0.0], [c, 0.0, 0.0], [1.0, 0.0, 0.0]],
    };
    let d = ImpactDisclosure::new((spline(0.5, 0.5), stats(10)), (spline(0.25, 0.75), stats(9))).unwrap();
    assert_eq!(d.delta(), &[0.0, 0.0, -0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let pt = |x: f64| FittedFeature::Point { c: [x, 1.0, 1.0] };
    let d = ImpactDisclosure::new((pt(0.0), stats(4)), (pt(-0.0), stats(3))).unwrap();
    assert_eq!(d.delta()[0].to_bits(), (-0.0f64).to_bits());
}

#[test]
fn opposed_directions_are_named() {
    // Hand-labelled: a plane normal that crosses the sign-rule boundary flips in canonical
    // form; its components' deltas are near 2, and the record says why.
    let s = 1.0e-3;
    let before = FittedFeature::Plane { p: [0.0; 3], n: [s, 0.0, 1.0] };
    let after = FittedFeature::Plane { p: [0.0; 3], n: [s, 0.0, -1.0] };
    let d = ImpactDisclosure::new((before, stats(10)), (after, stats(9))).unwrap();
    assert_eq!(d.opposed_directions(), &["n"]);
    let text = {
        let f = fixture();
        let params = gate_spec(f.gate, f.fraction).encode().unwrap();
        let b = (FittedFeature::Plane { p: [0.0; 3], n: [s, 0.0, 1.0] }, stats(20));
        let a = (FittedFeature::Plane { p: [0.0; 3], n: [s, 0.0, -1.0] }, stats(19));
        let r = ExclusionRecord::new(gate_id(), params, 20, vec![3], MethodStatus::Decided, Some((b, a))).unwrap();
        String::from_utf8(r.encode().unwrap()).unwrap()
    };
    assert!(text.ends_with("\"opposed_directions\":[\"n\"]}}"), "{text}");
}

// ----- Refusals (fail closed, each with its reason).

fn params() -> Vec<u8> {
    gate_spec(1e-4, 0.25).encode().unwrap()
}

fn plane(n: [f64; 3]) -> FittedFeature {
    FittedFeature::Plane { p: [0.0; 3], n }
}

fn pair(nb: usize, na: usize) -> Option<(FitSnapshot, FitSnapshot)> {
    Some(((plane([0.0, 0.0, 1.0]), stats(nb)), (plane([0.0, 0.0, 1.0]), stats(na))))
}

fn new(
    excluded: Vec<u32>,
    status: MethodStatus,
    impact: Option<(FitSnapshot, FitSnapshot)>,
) -> Result<ExclusionRecord, DiscloseError> {
    ExclusionRecord::new(gate_id(), params(), 20, excluded, status, impact)
}

#[test]
fn refuses_an_exclusion_without_impact() {
    assert_eq!(new(vec![1, 2], MethodStatus::Decided, None).unwrap_err(), DiscloseError::ImpactMissing { excluded: 2 });
}

#[test]
fn refuses_zero_scale_with_exclusions() {
    assert_eq!(
        new(vec![1], MethodStatus::ZeroScale, pair(20, 19)).unwrap_err(),
        DiscloseError::StatusMismatch { status: MethodStatus::ZeroScale, excluded: 1 }
    );
}

#[test]
fn refuses_bad_index_sets() {
    assert_eq!(
        new(vec![3, 20], MethodStatus::Decided, pair(20, 18)).unwrap_err(),
        DiscloseError::Policy(PolicyError::IndexOutOfRange { index: 20 })
    );
    assert_eq!(
        new(vec![5, 3, 5], MethodStatus::Decided, pair(20, 17)).unwrap_err(),
        DiscloseError::Policy(PolicyError::DuplicateIndex { index: 5 })
    );
}

#[test]
fn refuses_what_the_guards_refuse() {
    // 6 of 20 = 0.3 > 0.25.
    let e = new((0..6).collect(), MethodStatus::Decided, pair(20, 14)).unwrap_err();
    assert_eq!(e, DiscloseError::Policy(PolicyError::ExcessiveExclusion { fraction: 0.3, limit: 0.25 }));
    // 18 of 20 excluded leaves 2 points; a plane needs 3 (fraction allows it at 0.95).
    let spec = gate_spec(1e-4, 0.95).encode().unwrap();
    let e =
        ExclusionRecord::new(gate_id(), spec, 20, (0..18).collect(), MethodStatus::Decided, pair(20, 2)).unwrap_err();
    assert_eq!(e, DiscloseError::Policy(PolicyError::TooFewRemaining { remaining: 2, required: 3 }));
}

#[test]
fn refuses_a_mismatched_or_unknown_algorithm() {
    let mismatch =
        DiscloseError::Policy(PolicyError::InvalidParameter { field: "algorithm", reason: "method_mismatch" });
    let other = outlier_algorithm_id(PolicyKind::ToleranceBand, VERSION);
    assert_eq!(
        ExclusionRecord::new(other, params(), 20, Vec::new(), MethodStatus::Decided, None).unwrap_err(),
        mismatch
    );
    let mut select = gate_id();
    select.feature = "select".to_string();
    assert_eq!(
        ExclusionRecord::new(select, params(), 20, Vec::new(), MethodStatus::Decided, None).unwrap_err(),
        mismatch
    );
    let mut module = gate_id();
    module.module = "fit".to_string();
    assert_eq!(
        ExclusionRecord::new(module, params(), 20, Vec::new(), MethodStatus::Decided, None).unwrap_err(),
        mismatch
    );
}

#[test]
fn refuses_non_canonical_params() {
    let mut p = params();
    p.push(b' ');
    let e = ExclusionRecord::new(gate_id(), p, 20, Vec::new(), MethodStatus::Decided, None).unwrap_err();
    assert!(matches!(e, DiscloseError::Policy(PolicyError::InvalidParameter { field: "encoding", .. })), "{e:?}");
}

#[test]
fn refuses_count_and_primitive_mismatches() {
    assert_eq!(
        new(vec![1], MethodStatus::Decided, pair(19, 19)).unwrap_err(),
        DiscloseError::CountMismatch { which: BEFORE, expected: 20, got: 19 }
    );
    assert_eq!(
        new(vec![1], MethodStatus::Decided, pair(20, 20)).unwrap_err(),
        DiscloseError::CountMismatch { which: AFTER, expected: 19, got: 20 }
    );
    let sphere = (FittedFeature::Sphere { c: [0.0; 3], r: 1.0 }, stats(19));
    let e =
        new(vec![1], MethodStatus::Decided, Some(((plane([0.0, 0.0, 1.0]), stats(20)), sphere.clone()))).unwrap_err();
    assert_eq!(
        e,
        DiscloseError::PrimitiveMismatch { which: AFTER, expected: Primitive::Plane, got: Primitive::Sphere }
    );
    let e = ImpactDisclosure::new((plane([0.0, 0.0, 1.0]), stats(20)), sphere).unwrap_err();
    assert_eq!(
        e,
        DiscloseError::PrimitiveMismatch { which: AFTER, expected: Primitive::Plane, got: Primitive::Sphere }
    );
}

#[test]
fn refuses_non_canonical_directions() {
    let good = (plane([0.0, 0.0, 1.0]), stats(10));
    for n in [[0.0, 0.0, -1.0], [-0.0, 0.0, 1.0], [0.0, -0.0, 1.0]] {
        assert_eq!(
            ImpactDisclosure::new(good.clone(), (plane(n), stats(9))).unwrap_err(),
            DiscloseError::NotCanonical { which: AFTER, field: "n" },
            "{n:?}"
        );
    }
    let cone = |d: [f64; 3]| FittedFeature::Cone { apex: [0.0; 3], d, half_angle: 0.5 };
    // A cone axis keeps its sign (orientation), but never -0.0.
    assert!(ImpactDisclosure::new((cone([0.0, 0.0, -1.0]), stats(10)), (cone([0.0, 0.0, -1.0]), stats(9))).is_ok());
    assert_eq!(
        ImpactDisclosure::new((cone([-0.0, 0.0, -1.0]), stats(10)), (cone([0.0, 0.0, -1.0]), stats(9))).unwrap_err(),
        DiscloseError::NotCanonical { which: BEFORE, field: "d" }
    );
    // Invalid features are refused by fid-fit's own validation.
    let e =
        ImpactDisclosure::new(good, (FittedFeature::Plane { p: [f64::NAN, 0.0, 0.0], n: [0.0, 0.0, 1.0] }, stats(9)))
            .unwrap_err();
    assert_eq!(e.code(), "fit");
}

#[test]
fn refuses_invalid_statistics() {
    let good = (plane([0.0, 0.0, 1.0]), stats(10));
    let cases: [(ResidualStats, &str, &str); 6] = [
        (ResidualStats { rms: f64::NAN, ..stats(9) }, "rms", "non_finite"),
        (ResidualStats { mean: f64::INFINITY, ..stats(9) }, "mean", "non_finite"),
        (ResidualStats { rms: -1e-6, ..stats(9) }, "rms", "negative"),
        (ResidualStats { std_dev: -1e-6, ..stats(9) }, "std_dev", "negative"),
        (ResidualStats { min: 3e-6, ..stats(9) }, "min", "min_above_max"),
        (stats(0), "n", "zero"),
    ];
    for (st, field, reason) in cases {
        assert_eq!(
            ImpactDisclosure::new(good.clone(), (plane([0.0, 0.0, 1.0]), st)).unwrap_err(),
            DiscloseError::StatsInvalid { which: AFTER, field, reason }
        );
    }
}

#[test]
fn refuses_shape_mismatch_and_overflow() {
    let poly = |k: usize| FittedFeature::Polyline { vertices: vec![[0.0; 3]; k] };
    assert_eq!(
        ImpactDisclosure::new((poly(3), stats(10)), (poly(4), stats(9))).unwrap_err(),
        DiscloseError::ShapeMismatch { field: "vertices" }
    );
    let big = |x: f64| FittedFeature::Point { c: [x, 0.0, 0.0] };
    let e = ImpactDisclosure::new((big(-f64::MAX), stats(10)), (big(f64::MAX), stats(9))).unwrap_err();
    assert_eq!(e.code(), "num");
}

#[test]
fn error_codes_are_stable() {
    let all = [
        (DiscloseError::Policy(PolicyError::IndexOutOfRange { index: 1 }), "policy"),
        (DiscloseError::StatusMismatch { status: MethodStatus::ZeroScale, excluded: 1 }, "status_mismatch"),
        (DiscloseError::ImpactMissing { excluded: 1 }, "impact_missing"),
        (DiscloseError::ImpactUnexpected, "impact_unexpected"),
        (
            DiscloseError::PrimitiveMismatch { which: AFTER, expected: Primitive::Plane, got: Primitive::Sphere },
            "primitive_mismatch",
        ),
        (DiscloseError::ShapeMismatch { field: "knots" }, "shape_mismatch"),
        (DiscloseError::NotCanonical { which: BEFORE, field: "n" }, "not_canonical"),
        (DiscloseError::StatsInvalid { which: BEFORE, field: "rms", reason: "negative" }, "stats_invalid"),
        (DiscloseError::CountMismatch { which: AFTER, expected: 1, got: 2 }, "count_mismatch"),
        (DiscloseError::Alloc { op: "x" }, "alloc_failed"),
    ];
    for (e, code) in all {
        assert_eq!(e.code(), code);
        assert!(e.to_string().starts_with(code), "{e}");
    }
}
