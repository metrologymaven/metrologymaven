#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! KRN.OUT.GATE acceptance tests AC-1..AC-4.
//!
//! Expected masks and digests come from `fixtures/out/gate-band-v1/cases.txt`, written by
//! `fixtures/out/make_gate_fixtures.py` (an independent Python model of REQ-KRN.OUT.GATE-001
//! and -002, checked against hand labels of the planted points); nothing expected here was
//! produced by this crate.
//!
//! AC-4 (cross-target identity): the masks are compared index for index, and their digests
//! byte for byte, against the same pinned fixture on every CI target this test runs on:
//! x86-64 (Linux, Windows), ARM64 (macOS aarch64-apple-darwin) and wasm32-wasip1 under
//! wasmtime (`ci/wasm32-crates.txt` lists `fid-filter`). Equal to one pinned value on each
//! target means identical across targets.

use fid_filter::gate::{NominalGate, ToleranceBand, VERSION};
use fid_filter::policy::{
    apply, ExclusionMask, OutlierPolicy, PolicyError, PolicyMethod, PolicySpec, DEFAULT_MAX_EXCLUDED_FRACTION,
};
use fid_fit::contract::{FittedFeature, PointsView, Primitive};
use fid_kci::kci::sha256::sha256_digest;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/out/gate-band-v1/cases.txt");

const PLANE: FittedFeature = FittedFeature::Plane { p: [0.0; 3], n: [0.0, 0.0, 1.0] };

struct Label {
    index: u32,
    band_excluded: bool,
    gate_excluded: bool,
    note: String,
}

struct Fixture {
    count: usize,
    lower_m: f64,
    upper_m: f64,
    max_distance_m: f64,
    d: Vec<f64>,
    band_excluded: Vec<u32>,
    band_digest: String,
    gate_excluded: Vec<u32>,
    gate_digest: String,
    labels: Vec<Label>,
}

fn hex(v: &str) -> f64 {
    assert_eq!(v.len(), 16, "{v}");
    f64::from_bits(u64::from_str_radix(v, 16).unwrap())
}

fn verdict(v: &str) -> bool {
    match v {
        "exclude" => true,
        "keep" => false,
        other => panic!("verdict {other}"),
    }
}

fn fixture() -> Fixture {
    let text = std::fs::read_to_string(CASES).expect("fixture");
    let mut one: Vec<(String, String)> = Vec::new();
    let mut d = Vec::new();
    let mut labels = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k {
            "d" => d.push(hex(v)),
            "label" => {
                let mut it = v.splitn(4, ' ');
                labels.push(Label {
                    index: it.next().unwrap().parse().unwrap(),
                    band_excluded: verdict(it.next().unwrap()),
                    gate_excluded: verdict(it.next().unwrap()),
                    note: it.next().unwrap().to_string(),
                });
            }
            _ => {
                assert!(one.iter().all(|(o, _)| o != k), "duplicate key {k}");
                one.push((k.to_string(), v.to_string()));
            }
        }
    }
    let get = |key: &str| one.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap();
    let list = |key: &str| get(key).split_whitespace().map(|x| x.parse().unwrap()).collect::<Vec<u32>>();
    Fixture {
        count: get("count").parse().unwrap(),
        lower_m: hex(&get("lower_m")),
        upper_m: hex(&get("upper_m")),
        max_distance_m: hex(&get("max_distance_m")),
        d,
        band_excluded: list("band_excluded"),
        band_digest: get("band_digest"),
        gate_excluded: list("gate_excluded"),
        gate_digest: get("gate_digest"),
        labels,
    }
}

fn points(n: usize) -> Vec<f64> {
    let mut xyz = Vec::with_capacity(3 * n);
    for i in 0..n {
        xyz.extend_from_slice(&[i as f64, (i * 7 % 11) as f64, 0.0]);
    }
    xyz
}

/// The mask's digest, computed here from its indices independently of the mask code.
fn digest_of(count: usize, indices: &[u32]) -> String {
    let mut pre = (count as u32).to_le_bytes().to_vec();
    for i in indices {
        pre.extend_from_slice(&i.to_le_bytes());
    }
    sha256_digest(&pre).as_str().to_string()
}

fn check_mask(m: &ExclusionMask, f: &Fixture, want: &[u32], digest: &str) {
    assert_eq!(m.point_count(), f.count);
    assert_eq!(m.indices(), want);
    assert_eq!(m.digest().as_str(), digest);
    assert_eq!(digest_of(f.count, want), digest, "fixture digest matches its own indices");
}

#[test]
fn fixture_is_what_the_spec_names() {
    let f = fixture();
    assert_eq!(f.count, 2000);
    assert_eq!(f.d.len(), f.count);
    assert_eq!((f.lower_m, f.upper_m, f.max_distance_m), (-0.0001, 0.0002, 0.0001));
    assert!(f.labels.len() >= 4);
    // Every limit has a point exactly on it.
    for limit in [f.lower_m, f.upper_m, f.max_distance_m, -f.max_distance_m] {
        assert!(f.d.iter().any(|d| d.to_bits() == limit.to_bits()), "{limit}");
    }
}

/// AC-1: the tolerance band [-0.0001, 0.0002] m keeps points exactly on a limit and excludes
/// points one ulp outside; the whole mask equals the fixture's.
#[test]
fn ac1_tolerance_band_on_the_fixture() {
    let f = fixture();
    let band = ToleranceBand::new(f.lower_m, f.upper_m).unwrap();
    let m = band.mask(&f.d).unwrap();
    check_mask(&m, &f, &f.band_excluded, &f.band_digest);
    for l in &f.labels {
        assert_eq!(m.contains(l.index), l.band_excluded, "band: {} (index {})", l.note, l.index);
    }
    // Through the trait, as apply() calls it.
    let xyz = points(f.count);
    let pts = PointsView::new(&xyz, None).unwrap();
    let res = vec![0.0; f.count];
    let via = band.exclude(&pts, &PLANE, &res, Some(&f.d)).unwrap();
    assert_eq!(via, m);
}

/// AC-1, hand case: one ulp each side of each limit (bit increments move away from zero).
#[test]
fn ac1_one_ulp_each_side_by_hand() {
    let up = |x: f64| f64::from_bits(x.to_bits() + 1); // one ulp farther from zero
    let down = |x: f64| f64::from_bits(x.to_bits() - 1); // one ulp nearer to zero
    let (lo, hi) = (-0.0001, 0.0002);
    let d = [lo, up(lo), down(lo), hi, up(hi), down(hi), 0.0, -0.0];
    let m = ToleranceBand::new(lo, hi).unwrap().mask(&d).unwrap();
    assert_eq!(m.indices(), &[1, 4]);
    let g = 0.0001;
    let d = [g, up(g), down(g), -g, up(-g), down(-g), 0.0, -0.0, 5e-324];
    let m = NominalGate::new(g).unwrap().mask(&d).unwrap();
    assert_eq!(m.indices(), &[1, 4]);
    // A degenerate band [x, x] keeps exactly x.
    let m = ToleranceBand::new(0.001, 0.001).unwrap().mask(&[0.001, up(0.001), down(0.001)]).unwrap();
    assert_eq!(m.indices(), &[1, 2]);
}

/// AC-2: the nominal gate of 0.0001 m gives the fixture's mask.
#[test]
fn ac2_nominal_gate_on_the_fixture() {
    let f = fixture();
    let gate = NominalGate::new(f.max_distance_m).unwrap();
    let m = gate.mask(&f.d).unwrap();
    check_mask(&m, &f, &f.gate_excluded, &f.gate_digest);
    for l in &f.labels {
        assert_eq!(m.contains(l.index), l.gate_excluded, "gate: {} (index {})", l.note, l.index);
    }
    let xyz = points(f.count);
    let pts = PointsView::new(&xyz, None).unwrap();
    let res = vec![0.0; f.count];
    assert_eq!(gate.exclude(&pts, &PLANE, &res, Some(&f.d)).unwrap(), m);
}

/// Both methods run under apply(): the guards hold and the record carries the algorithm ID,
/// the canonical parameters, the excluded set and its digest.
#[test]
fn apply_runs_both_methods_under_the_guards() {
    let f = fixture();
    let xyz = points(f.count);
    let pts = PointsView::new(&xyz, None).unwrap();
    let res = vec![0.0; f.count];

    let band = ToleranceBand::new(f.lower_m, f.upper_m).unwrap();
    let spec = PolicySpec {
        policy: OutlierPolicy::ToleranceBand { lower_m: f.lower_m, upper_m: f.upper_m },
        max_excluded_fraction: DEFAULT_MAX_EXCLUDED_FRACTION,
        target: Primitive::Plane,
    };
    let r = apply(&spec, &band, &pts, &PLANE, &res, Some(&f.d)).unwrap();
    let ex = r.exclusion();
    assert_eq!(ex.algorithm().to_string(), "filter.outlier.tolerance_band@1.0.0");
    assert_eq!(ex.params(), spec.encode().unwrap().as_slice());
    assert_eq!(ex.excluded(), f.band_excluded.as_slice());
    assert_eq!(ex.digest().as_str(), f.band_digest);
    assert_eq!(r.view().unwrap().len(), f.count - f.band_excluded.len());

    let gate = NominalGate::new(f.max_distance_m).unwrap();
    let spec = PolicySpec {
        policy: OutlierPolicy::NominalGate { max_distance_m: f.max_distance_m },
        max_excluded_fraction: 0.5,
        target: Primitive::Plane,
    };
    let r = apply(&spec, &gate, &pts, &PLANE, &res, Some(&f.d)).unwrap();
    assert_eq!(r.exclusion().algorithm().to_string(), "filter.outlier.nominal_gate@1.0.0");
    assert_eq!(r.exclusion().excluded(), f.gate_excluded.as_slice());
    assert_eq!(r.exclusion().digest().as_str(), f.gate_digest);
    // The gate excludes half the fixture: a lower ceiling refuses it instead of fitting.
    let tight = PolicySpec { max_excluded_fraction: DEFAULT_MAX_EXCLUDED_FRACTION, ..spec };
    assert!(matches!(
        apply(&tight, &gate, &pts, &PLANE, &res, Some(&f.d)),
        Err(PolicyError::ExcessiveExclusion { .. })
    ));
    // A spec with other parameters than the method runs with is refused.
    let other = PolicySpec { policy: OutlierPolicy::NominalGate { max_distance_m: 0.0002 }, ..spec };
    assert_eq!(
        apply(&other, &gate, &pts, &PLANE, &res, Some(&f.d)).unwrap_err(),
        PolicyError::InvalidParameter { field: "policy", reason: "method_mismatch" }
    );
}

/// AC-3: 2 000 points with 1 999 distances is `LengthMismatch`; lower > upper is
/// `InvalidParameter`; missing distances and non-finite distances are typed errors too.
#[test]
fn ac3_error_paths() {
    let f = fixture();
    let xyz = points(2000);
    let pts = PointsView::new(&xyz, None).unwrap();
    let res = vec![0.0; 2000];
    let short = &f.d[..1999];
    let band = ToleranceBand::new(f.lower_m, f.upper_m).unwrap();
    let gate = NominalGate::new(f.max_distance_m).unwrap();
    let methods: [&dyn PolicyMethod; 2] = [&band, &gate];
    for m in methods {
        assert_eq!(
            m.exclude(&pts, &PLANE, &res, Some(short)).unwrap_err(),
            PolicyError::LengthMismatch { what: "nominal_distances", expected: 2000, got: 1999 }
        );
        assert_eq!(m.exclude(&pts, &PLANE, &res, None).unwrap_err(), PolicyError::NominalDistances { required: true });
        let mut bad = f.d.clone();
        bad[1234] = f64::NAN;
        assert!(matches!(m.exclude(&pts, &PLANE, &res, Some(&bad)), Err(PolicyError::Num(_))));
        bad[1234] = f64::NEG_INFINITY;
        assert!(matches!(m.exclude(&pts, &PLANE, &res, Some(&bad)), Err(PolicyError::Num(_))));
        // Through apply() too.
        let spec = PolicySpec { policy: m.policy(), max_excluded_fraction: 0.5, target: Primitive::Plane };
        assert_eq!(
            apply(&spec, m, &pts, &PLANE, &res, Some(short)).unwrap_err(),
            PolicyError::LengthMismatch { what: "nominal_distances", expected: 2000, got: 1999 }
        );
        assert_eq!(
            apply(&spec, m, &pts, &PLANE, &res, None).unwrap_err(),
            PolicyError::NominalDistances { required: true }
        );
    }
    assert_eq!(
        ToleranceBand::new(0.0002, -0.0001).unwrap_err(),
        PolicyError::InvalidParameter { field: "lower_m", reason: "above_upper" }
    );
    assert_eq!(ToleranceBand::new(1e-3, 1e-3 * 0.5).unwrap_err().code(), "invalid_parameter");
}

/// Parameters are validated with the KRN.OUT.POL rules, so a method value is always valid.
#[test]
fn parameters_are_validated() {
    let inv = |field, reason| PolicyError::InvalidParameter { field, reason };
    assert_eq!(NominalGate::new(0.0).unwrap_err(), inv("max_distance_m", "out_of_range"));
    assert_eq!(NominalGate::new(-0.0).unwrap_err(), inv("max_distance_m", "out_of_range"));
    assert_eq!(NominalGate::new(-1e-4).unwrap_err(), inv("max_distance_m", "out_of_range"));
    assert_eq!(NominalGate::new(f64::NAN).unwrap_err(), inv("max_distance_m", "non_finite"));
    assert_eq!(NominalGate::new(f64::INFINITY).unwrap_err(), inv("max_distance_m", "non_finite"));
    assert_eq!(NominalGate::new(5e-324).unwrap_err(), inv("max_distance_m", "subnormal"));
    assert_eq!(ToleranceBand::new(f64::NAN, 1.0).unwrap_err(), inv("lower_m", "non_finite"));
    assert_eq!(ToleranceBand::new(-1.0, f64::INFINITY).unwrap_err(), inv("upper_m", "non_finite"));
    assert_eq!(ToleranceBand::new(-0.0, 1.0).unwrap_err(), inv("lower_m", "negative_zero"));
    assert_eq!(ToleranceBand::new(-1.0, -5e-324).unwrap_err(), inv("upper_m", "subnormal"));
    // A one-sided band and a band entirely on one side of nominal are valid.
    let b = ToleranceBand::new(0.0, 1e-3).unwrap();
    assert_eq!((b.lower_m(), b.upper_m()), (0.0, 1e-3));
    assert!(ToleranceBand::new(-2e-3, -1e-3).is_ok());
    assert_eq!(NominalGate::new(2.5e-5).unwrap().max_distance_m(), 2.5e-5);
}

#[test]
fn algorithm_ids_and_policies() {
    assert_eq!(VERSION, (1, 0, 0));
    let g = NominalGate::new(1e-4).unwrap();
    assert_eq!(g.algorithm_id().to_string(), "filter.outlier.nominal_gate@1.0.0");
    assert_eq!(g.policy(), OutlierPolicy::NominalGate { max_distance_m: 1e-4 });
    let b = ToleranceBand::new(-1e-4, 2e-4).unwrap();
    assert_eq!(b.algorithm_id().to_string(), "filter.outlier.tolerance_band@1.0.0");
    assert_eq!(b.policy(), OutlierPolicy::ToleranceBand { lower_m: -1e-4, upper_m: 2e-4 });
}

/// An empty input gives the empty mask; nothing excluded is still a recorded mask.
#[test]
fn empty_and_all_inside() {
    let g = NominalGate::new(1.0).unwrap();
    let m = g.mask(&[]).unwrap();
    assert_eq!((m.point_count(), m.len()), (0, 0));
    let m = ToleranceBand::new(-1.0, 1.0).unwrap().mask(&[0.5, -0.5, 1.0, -1.0]).unwrap();
    assert!(m.is_empty());
    assert_eq!(m.point_count(), 4);
}
