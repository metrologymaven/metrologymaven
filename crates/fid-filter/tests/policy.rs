#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! KRN.OUT.POL acceptance tests AC-1..AC-3 and the `apply` entry point.
//!
//! Expected values come from `fixtures/out/pol-params-v1/cases.txt`, written by
//! `fixtures/out/make_pol_fixtures.py` (an independent Python model of the pinned rules,
//! checked against hand labels in `cases.in`); nothing expected here was produced by this crate.

use fid_filter::policy::{
    apply, check_guards, outlier_algorithm_id, select_algorithm_id, ExclusionMask, OutlierPolicy, PolicyError,
    PolicyKind, PolicyMethod, PolicySpec, ScaleEstimator, Side, DEFAULT_MAX_EXCLUDED_FRACTION, MAX_ENCODED_BYTES,
};
use fid_fit::contract::{AlgorithmId, FittedFeature, PointsView, Primitive};
use fid_kci::kci::sha256::sha256_digest;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/out/pol-params-v1/cases.txt");

struct Case {
    what: String,
    name: String,
    lines: Vec<(String, String)>,
}

impl Case {
    fn get(&self, key: &str) -> &str {
        self.lines
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("{}: no '{key}' line", self.name))
    }

    fn f(&self, key: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(self.get(key), 16).unwrap())
    }

    fn list(&self, key: &str) -> Vec<u32> {
        self.get(key).split_whitespace().map(|x| x.parse().unwrap()).collect()
    }
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string(CASES).expect("fixture");
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line == "end" {
            out.push(cur.take().expect("end without case"));
            continue;
        }
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match cur.as_mut() {
            None => cur = Some(Case { what: k.to_string(), name: v.to_string(), lines: Vec::new() }),
            Some(c) => c.lines.push((k.to_string(), v.to_string())),
        }
    }
    assert!(cur.is_none(), "unterminated case");
    out
}

fn expected_invalid(c: &Case) -> Option<PolicyError> {
    let e = c.get("expect");
    if e == "ok" {
        return None;
    }
    let parts: Vec<&str> = e.split(' ').collect();
    assert_eq!(parts[0], "invalid_parameter", "{}", c.name);
    // The error fields are &'static str: leak the two fixture strings (test only).
    let field: &'static str = Box::leak(parts[1].to_string().into_boxed_str());
    let reason: &'static str = Box::leak(parts[2].to_string().into_boxed_str());
    Some(PolicyError::InvalidParameter { field, reason })
}

fn spec_of(c: &Case) -> PolicySpec {
    let policy = match c.get("kind") {
        "nsigma" => OutlierPolicy::NSigma { k: c.f("k"), side: Side::parse(c.get("side")).unwrap() },
        "robust_scale" => OutlierPolicy::RobustScale {
            k: c.f("k"),
            scale: ScaleEstimator::parse(c.get("scale")).unwrap(),
            side: Side::parse(c.get("side")).unwrap(),
        },
        "ransac" => OutlierPolicy::Ransac {
            threshold_m: c.f("threshold_m"),
            iterations: c.get("iterations").parse().unwrap(),
            min_inliers: c.get("min_inliers").parse().unwrap(),
            seed: c.get("seed").parse().unwrap(),
        },
        "nominal_gate" => OutlierPolicy::NominalGate { max_distance_m: c.f("max_distance_m") },
        "tolerance_band" => OutlierPolicy::ToleranceBand { lower_m: c.f("lower_m"), upper_m: c.f("upper_m") },
        k => panic!("{}: kind {k}", c.name),
    };
    PolicySpec { policy, max_excluded_fraction: c.f("fraction"), target: Primitive::parse(c.get("target")).unwrap() }
}

fn of_kind<'a>(all: &'a [Case], what: &str) -> Vec<&'a Case> {
    let v: Vec<&Case> = all.iter().filter(|c| c.what == what).collect();
    assert!(!v.is_empty(), "no {what} cases");
    v
}

/// AC-1: every invalid set returns the named typed error; every valid set passes and
/// round-trips through the canonical encoding unchanged (bytes equal to the Python model's).
#[test]
fn ac1_parameter_sets_validate_and_round_trip() {
    let all = cases();
    let specs = of_kind(&all, "spec");
    let mut seen = std::collections::BTreeSet::new();
    for c in specs {
        let spec = spec_of(c);
        seen.insert(spec.policy.kind());
        match expected_invalid(c) {
            Some(e) => {
                assert_eq!(spec.validate(), Err(e.clone()), "{}", c.name);
                assert_eq!(spec.encode(), Err(e), "{}: an invalid spec has no encoding", c.name);
            }
            None => {
                assert_eq!(spec.validate(), Ok(()), "{}", c.name);
                let bytes = spec.encode().unwrap();
                assert_eq!(std::str::from_utf8(&bytes).unwrap(), c.get("canonical"), "{}", c.name);
                let back = PolicySpec::decode(&bytes).unwrap();
                assert_eq!(back.encode().unwrap(), bytes, "{}", c.name);
                assert_eq!(back, spec, "{}", c.name);
            }
        }
    }
    assert_eq!(seen.len(), PolicyKind::ALL.len(), "every variant has fixture cases");
}

/// AC-1 (decoding): canonical bytes decode; malformed, incomplete and non-canonical bytes
/// return the named error.
#[test]
fn ac1_decoding() {
    let all = cases();
    for c in of_kind(&all, "decode") {
        let input = c.get("input");
        let got = PolicySpec::decode(input.as_bytes());
        match expected_invalid(c) {
            Some(e) => assert_eq!(got, Err(e), "{}", c.name),
            None => {
                let spec = got.unwrap_or_else(|e| panic!("{}: {e}", c.name));
                assert_eq!(spec.encode().unwrap(), c.get("canonical").as_bytes(), "{}", c.name);
            }
        }
    }
}

#[test]
fn decode_refuses_oversized_input() {
    let big = vec![b'{'; MAX_ENCODED_BYTES + 1];
    assert_eq!(PolicySpec::decode(&big), Err(PolicyError::InvalidParameter { field: "encoding", reason: "too_long" }));
    let deep = vec![b'{'; MAX_ENCODED_BYTES];
    assert_eq!(
        PolicySpec::decode(&deep),
        Err(PolicyError::InvalidParameter { field: "encoding", reason: "malformed" })
    );
}

/// REQ-002, REQ-005 and AC-3 (fixture): range check, order, refused repeats and the
/// excluded-set digest.
#[test]
fn ac3_masks() {
    let all = cases();
    for c in of_kind(&all, "mask") {
        let count: usize = c.get("count").parse().unwrap();
        let got = ExclusionMask::new(count, c.list("indices"));
        let e = c.get("expect");
        if let Some(index) = e.strip_prefix("index_out_of_range ") {
            assert_eq!(got, Err(PolicyError::IndexOutOfRange { index: index.parse().unwrap() }), "{}", c.name);
            continue;
        }
        if let Some(index) = e.strip_prefix("duplicate_index ") {
            assert_eq!(got, Err(PolicyError::DuplicateIndex { index: index.parse().unwrap() }), "{}", c.name);
            continue;
        }
        assert_eq!(e, "ok", "{}", c.name);
        let m = got.unwrap();
        assert_eq!(m.indices(), c.list("sorted").as_slice(), "{}", c.name);
        assert_eq!(m.point_count(), count, "{}", c.name);
        assert_eq!(m.digest().as_str(), c.get("digest"), "{}", c.name);
        assert_eq!(sha256_digest(&m.canonical_bytes().unwrap()), m.digest(), "{}", c.name);
    }
}

/// AC-3 as worded: index 100 for 100 points.
#[test]
fn ac3_index_100_of_100() {
    let idx: Vec<u32> = (0..50).chain([100]).collect();
    assert_eq!(ExclusionMask::new(100, idx), Err(PolicyError::IndexOutOfRange { index: 100 }));
    assert_eq!(
        ExclusionMask::new(fid_fit::contract::MAX_POINTS + 1, Vec::new()),
        Err(PolicyError::InvalidParameter { field: "point_count", reason: "out_of_range" })
    );
}

fn nsigma_spec(target: Primitive, fraction: f64) -> PolicySpec {
    PolicySpec { policy: OutlierPolicy::NSigma { k: 3.0, side: Side::Both }, max_excluded_fraction: fraction, target }
}

/// REQ-003, REQ-004 (fixture): guard order and the fraction comparison.
#[test]
fn ac2_guards_fixture() {
    let all = cases();
    for c in of_kind(&all, "guard") {
        let spec = nsigma_spec(Primitive::parse(c.get("target")).unwrap(), c.f("fraction"));
        let count: usize = c.get("count").parse().unwrap();
        let mask = ExclusionMask::new(count, c.list("indices")).unwrap();
        let got = check_guards(&spec, &mask);
        let parts: Vec<&str> = c.get("expect").split(' ').collect();
        match parts[0] {
            "ok" => assert_eq!(got, Ok(()), "{}", c.name),
            "too_few_remaining" => assert_eq!(
                got,
                Err(PolicyError::TooFewRemaining {
                    remaining: parts[1].parse().unwrap(),
                    required: parts[2].parse().unwrap()
                }),
                "{}",
                c.name
            ),
            "excessive_exclusion" => {
                let Err(PolicyError::ExcessiveExclusion { fraction, limit }) = got else {
                    panic!("{}: {got:?}", c.name)
                };
                let hex = |x: f64| format!("{:016x}", x.to_bits());
                assert_eq!((hex(fraction).as_str(), hex(limit).as_str()), (parts[1], parts[2]), "{}", c.name);
            }
            other => panic!("{}: expect {other}", c.name),
        }
    }
}

/// AC-2 as worded: 100 points, cylinder (minimum 5).
#[test]
fn ac2_cylinder_guards() {
    let spec = nsigma_spec(Primitive::Cylinder, DEFAULT_MAX_EXCLUDED_FRACTION);
    let m96 = ExclusionMask::new(100, (0..96).collect()).unwrap();
    assert_eq!(check_guards(&spec, &m96), Err(PolicyError::TooFewRemaining { remaining: 4, required: 5 }));
    let m30 = ExclusionMask::new(100, (0..30).collect()).unwrap();
    assert_eq!(check_guards(&spec, &m30), Err(PolicyError::ExcessiveExclusion { fraction: 0.3, limit: 0.25 }));
    // An invalid envelope is refused before any guard runs.
    let bad = nsigma_spec(Primitive::Cylinder, f64::NAN);
    assert_eq!(
        check_guards(&bad, &m30),
        Err(PolicyError::InvalidParameter { field: "max_excluded_fraction", reason: "non_finite" })
    );
}

/// Guards on a 1 M-point mask (performance budget REQ: functional check only, timing is not
/// asserted in CI).
#[test]
fn guards_on_a_million_points() {
    let n = 1_000_000usize;
    let spec = nsigma_spec(Primitive::Plane, DEFAULT_MAX_EXCLUDED_FRACTION);
    let m = ExclusionMask::new(n, (0..250_000u32).rev().collect()).unwrap();
    assert_eq!(check_guards(&spec, &m), Ok(()));
    let m = ExclusionMask::new(n, (0..250_001u32).collect()).unwrap();
    assert!(matches!(check_guards(&spec, &m), Err(PolicyError::ExcessiveExclusion { .. })));
}

#[test]
fn algorithm_ids() {
    let id = outlier_algorithm_id(PolicyKind::RobustScale, (1, 0, 0));
    assert_eq!(id.to_string(), "filter.outlier.robust_scale@1.0.0");
    for k in PolicyKind::ALL {
        let id = outlier_algorithm_id(*k, (0, 1, 2));
        assert_eq!(AlgorithmId::parse(&id.to_string()).unwrap(), id);
        assert_eq!(PolicyKind::parse(k.slug()), Some(*k));
    }
    assert_eq!(select_algorithm_id("indices", (1, 0, 0)).unwrap().to_string(), "filter.select.indices@1.0.0");
    for bad in ["", "Indices", "a.b", "x@1", "9x", "a b"] {
        assert_eq!(
            select_algorithm_id(bad, (1, 0, 0)),
            Err(PolicyError::InvalidParameter { field: "method", reason: "bad_segment" }),
            "{bad:?}"
        );
    }
}

/// A test-side method: excludes the points whose absolute residual exceeds `k` metres. It
/// stands in for the method leaves (STAT, RANSAC, GATE), which are not built yet.
struct Threshold {
    policy: OutlierPolicy,
    k: f64,
    point_count_override: Option<usize>,
}

impl PolicyMethod for Threshold {
    fn policy(&self) -> OutlierPolicy {
        self.policy
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }

    fn exclude(
        &self,
        pts: &PointsView<'_>,
        _fit: &FittedFeature,
        residuals: &[f64],
        _nominal: Option<&[f64]>,
    ) -> Result<ExclusionMask, PolicyError> {
        let idx = (0u32..).zip(residuals).filter(|(_, r)| r.abs() > self.k).map(|(i, _)| i).collect();
        ExclusionMask::new(self.point_count_override.unwrap_or(pts.len()), idx)
    }
}

fn plane_points(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut xyz = Vec::new();
    let mut nrm = Vec::new();
    for i in 0..n {
        xyz.extend_from_slice(&[i as f64, (i * 7 % 11) as f64, 0.0]);
        nrm.extend_from_slice(&[0.0, 0.0, 1.0]);
    }
    (xyz, nrm)
}

const PLANE: FittedFeature = FittedFeature::Plane { p: [0.0; 3], n: [0.0, 0.0, 1.0] };

/// D-045 / REQ-KRN.OUT-002: the reduced set always carries the policy ID, its canonical
/// parameters, the excluded index set and its digest; kept points keep their input order.
#[test]
fn apply_returns_kept_points_with_their_record() {
    let (xyz, nrm) = plane_points(20);
    let pts = PointsView::new(&xyz, Some(&nrm)).unwrap();
    let mut residuals = vec![0.0; 20];
    residuals[3] = 0.5;
    residuals[17] = -0.7;
    let spec = nsigma_spec(Primitive::Plane, DEFAULT_MAX_EXCLUDED_FRACTION);
    let m = Threshold { policy: spec.policy, k: 0.1, point_count_override: None };
    let r = apply(&spec, &m, &pts, &PLANE, &residuals, None).unwrap();
    assert!(r.is_reduced());
    let ex = r.exclusion();
    assert_eq!(ex.algorithm().to_string(), "filter.outlier.nsigma@0.1.0");
    assert_eq!(ex.params(), spec.encode().unwrap().as_slice());
    assert_eq!(ex.excluded(), &[3, 17]);
    assert_eq!(ex.excluded_count(), 2);
    assert_eq!(ex.point_count(), 20);
    // A method that does not override exclude_reported is recorded as having decided.
    assert_eq!(ex.status(), fid_filter::policy::MethodStatus::Decided);
    // Digest over u32le(20) u32le(3) u32le(17), computed here independently of the mask code.
    let mut pre = Vec::new();
    for v in [20u32, 3, 17] {
        pre.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(*ex.digest(), sha256_digest(&pre));
    let kept: Vec<u32> = (0..20).filter(|i| *i != 3 && *i != 17).collect();
    assert_eq!(r.kept_indices(), kept.as_slice());
    let v = r.view().unwrap();
    assert_eq!(v.len(), 18);
    for (j, &i) in kept.iter().enumerate() {
        assert_eq!(v.point(j), pts.point(i as usize));
        assert_eq!(v.normal(j), pts.normal(i as usize));
    }
}

/// Nothing excluded is recorded too: excluded_count 0 and the digest of the empty set.
#[test]
fn apply_records_an_empty_exclusion() {
    let (xyz, _) = plane_points(10);
    let pts = PointsView::new(&xyz, None).unwrap();
    let spec = nsigma_spec(Primitive::Plane, 0.0);
    let m = Threshold { policy: spec.policy, k: 1.0, point_count_override: None };
    let r = apply(&spec, &m, &pts, &PLANE, &[0.0; 10], None).unwrap();
    assert!(!r.is_reduced());
    assert_eq!(r.exclusion().excluded_count(), 0);
    assert_eq!(*r.exclusion().digest(), sha256_digest(&10u32.to_le_bytes()));
    assert_eq!(r.view().unwrap().xyz(), xyz.as_slice());
    assert!(r.view().unwrap().normals().is_none());
}

#[test]
fn apply_refuses_bad_inputs() {
    let (xyz, _) = plane_points(10);
    let pts = PointsView::new(&xyz, None).unwrap();
    let spec = nsigma_spec(Primitive::Plane, DEFAULT_MAX_EXCLUDED_FRACTION);
    let ok = Threshold { policy: spec.policy, k: 0.1, point_count_override: None };
    let zeros = [0.0; 10];

    // Invalid spec.
    let bad = PolicySpec { max_excluded_fraction: 1.5, ..spec };
    assert_eq!(
        apply(&bad, &ok, &pts, &PLANE, &zeros, None).unwrap_err(),
        PolicyError::InvalidParameter { field: "max_excluded_fraction", reason: "out_of_range" }
    );
    // The method runs with other parameters than the spec records.
    let other =
        Threshold { policy: OutlierPolicy::NSigma { k: 2.0, side: Side::Both }, k: 0.1, point_count_override: None };
    assert_eq!(
        apply(&spec, &other, &pts, &PLANE, &zeros, None).unwrap_err(),
        PolicyError::InvalidParameter { field: "policy", reason: "method_mismatch" }
    );
    // Residual count and finiteness (NaN is an error).
    assert_eq!(
        apply(&spec, &ok, &pts, &PLANE, &zeros[..9], None).unwrap_err(),
        PolicyError::LengthMismatch { what: "residuals", expected: 10, got: 9 }
    );
    let mut nan = zeros;
    nan[4] = f64::NAN;
    assert!(matches!(apply(&spec, &ok, &pts, &PLANE, &nan, None), Err(PolicyError::Num(_))));
    // Nominal distances: length and finiteness, for a gate method that needs them.
    let gate = PolicySpec { policy: OutlierPolicy::NominalGate { max_distance_m: 1e-3 }, ..spec };
    let gate_m = Threshold { policy: gate.policy, k: 0.1, point_count_override: None };
    assert_eq!(
        apply(&gate, &gate_m, &pts, &PLANE, &zeros, Some(&zeros[..3])).unwrap_err(),
        PolicyError::LengthMismatch { what: "nominal_distances", expected: 10, got: 3 }
    );
    let mut inf = zeros;
    inf[0] = f64::INFINITY;
    assert!(matches!(apply(&gate, &gate_m, &pts, &PLANE, &zeros, Some(&inf)), Err(PolicyError::Num(_))));
    // A mask over another input set.
    let wrong = Threshold { policy: spec.policy, k: 0.1, point_count_override: Some(11) };
    assert_eq!(
        apply(&spec, &wrong, &pts, &PLANE, &zeros, None).unwrap_err(),
        PolicyError::LengthMismatch { what: "mask", expected: 10, got: 11 }
    );
    // Guards apply: 3 of 10 excluded with limit 0.25.
    let mut r = zeros;
    r[0] = 1.0;
    r[1] = 1.0;
    r[2] = 1.0;
    assert_eq!(
        apply(&spec, &ok, &pts, &PLANE, &r, None).unwrap_err(),
        PolicyError::ExcessiveExclusion { fraction: 0.3, limit: 0.25 }
    );
}

/// Nominal distances are given exactly when the spec's method needs them
/// (`PolicyKind::needs_nominal`), checked before any evidence is read.
#[test]
fn apply_refuses_nominal_distances_mismatch() {
    let (xyz, _) = plane_points(10);
    let pts = PointsView::new(&xyz, None).unwrap();
    let zeros = [0.0; 10];
    let policies = [
        OutlierPolicy::NSigma { k: 3.0, side: Side::Both },
        OutlierPolicy::RobustScale { k: 3.0, scale: ScaleEstimator::Mad, side: Side::Both },
        OutlierPolicy::Ransac { threshold_m: 1e-5, iterations: 100, min_inliers: 3, seed: 7 },
        OutlierPolicy::NominalGate { max_distance_m: 1e-3 },
        OutlierPolicy::ToleranceBand { lower_m: -1e-3, upper_m: 1e-3 },
    ];
    for policy in policies {
        let spec =
            PolicySpec { policy, max_excluded_fraction: DEFAULT_MAX_EXCLUDED_FRACTION, target: Primitive::Plane };
        let m = Threshold { policy, k: 0.1, point_count_override: None };
        let needs = policy.kind().needs_nominal();
        assert_eq!(needs, matches!(policy, OutlierPolicy::NominalGate { .. } | OutlierPolicy::ToleranceBand { .. }));
        let (right, wrong) = if needs { (Some(&zeros[..]), None) } else { (None, Some(&zeros[..])) };
        assert_eq!(
            apply(&spec, &m, &pts, &PLANE, &zeros, wrong).unwrap_err(),
            PolicyError::NominalDistances { required: needs },
            "{policy:?}"
        );
        // Refused before the evidence is read: a wrong-length residual slice does not matter.
        assert_eq!(
            apply(&spec, &m, &pts, &PLANE, &zeros[..2], wrong).unwrap_err(),
            PolicyError::NominalDistances { required: needs },
            "{policy:?}"
        );
        let r = apply(&spec, &m, &pts, &PLANE, &zeros, right).unwrap();
        assert!(!r.is_reduced(), "{policy:?}");
    }
}

/// A repeated index is refused with the smallest repeated one, after the range check.
#[test]
fn masks_refuse_repeated_indices() {
    assert_eq!(ExclusionMask::new(10, vec![8, 2, 8, 2]), Err(PolicyError::DuplicateIndex { index: 2 }));
    assert_eq!(ExclusionMask::new(10, vec![3, 3, 10]), Err(PolicyError::IndexOutOfRange { index: 10 }));
    assert_eq!(ExclusionMask::new(10, vec![1, 0]).unwrap().indices(), &[0, 1]);
}

/// The error codes are stable.
#[test]
fn error_codes() {
    assert_eq!(PolicyError::IndexOutOfRange { index: 1 }.code(), "index_out_of_range");
    assert_eq!(PolicyError::DuplicateIndex { index: 1 }.code(), "duplicate_index");
    assert_eq!(PolicyError::DuplicateIndex { index: 7 }.to_string(), "duplicate_index: 7");
    assert_eq!(PolicyError::NominalDistances { required: true }.code(), "nominal_distances");
    assert_eq!(PolicyError::TooFewRemaining { remaining: 1, required: 2 }.code(), "too_few_remaining");
    assert_eq!(PolicyError::ExcessiveExclusion { fraction: 0.5, limit: 0.25 }.code(), "excessive_exclusion");
    assert_eq!(PolicyError::InvalidParameter { field: "k", reason: "out_of_range" }.code(), "invalid_parameter");
    assert_eq!(
        PolicyError::InvalidParameter { field: "k", reason: "out_of_range" }.to_string(),
        "invalid_parameter: k: out_of_range"
    );
}
