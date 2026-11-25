//! RPT.STATS.CAP.INDEX tests. The cases in `fixtures/rpt/stats/cap_cases.json` are
//! hand-chosen and hand-labelled; their expected values (exact rationals, correctly rounded
//! doubles, the pinned-order bits and the result digest) come from the independent Python
//! reference `fixtures/rpt/stats/make_cap_cases.py`, never from running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{parse_json, Json};
use fid_stats::{
    capability, BadInput, EstimatorId, Family, IndexPair, IndexValue, Side, StatsError, CAP_INDEX_ALGORITHM_ID,
    DEFAULT_MIN_N, MAX_ESTIMATOR_ID_BYTES,
};

fn fixture() -> Json {
    let path = format!("{}/fixtures/rpt/stats/cap_cases.json", fid_testkit::WORKSPACE_ROOT);
    parse_json(&std::fs::read_to_string(path).unwrap())
}

fn opt(j: &Json) -> Option<f64> {
    if j.is_null() {
        None
    } else {
        Some(j.str().parse::<f64>().unwrap())
    }
}

fn hex_bits(s: &str) -> u64 {
    assert_eq!(s.len(), 16, "{s}");
    u64::from_str_radix(s, 16).unwrap()
}

fn family(s: &str) -> Family {
    match s {
        "C" => Family::C,
        "P" => Family::P,
        other => panic!("family {other}"),
    }
}

fn run(c: &Json) -> Result<IndexPair, StatsError> {
    capability(
        c.get("mean").str().parse().unwrap(),
        c.get("sigma").str().parse().unwrap(),
        c.get("n").num() as u64,
        opt(c.get("lsl")),
        opt(c.get("usl")),
        c.get("min_n").num() as u64,
        family(c.get("family").str()),
        EstimatorId::new(c.get("estimator").str()).unwrap(),
    )
}

/// Ordered distance in ulps between two finite doubles of the same sign.
fn ulps(a: f64, b: f64) -> u64 {
    assert_eq!(a.is_sign_negative(), b.is_sign_negative(), "{a} vs {b}");
    a.to_bits().abs_diff(b.to_bits())
}

fn token(v: IndexValue) -> String {
    match v {
        IndexValue::Value(x) => format!("v:{:016x}", x.to_bits()),
        other => other.status().unwrap().to_owned(),
    }
}

fn check_index(id: &str, which: &str, got: IndexValue, want: &Json) {
    if let Json::Str(s) = want.get("status") {
        assert_eq!(got.status(), Some(s.as_str()), "{id} {which}");
        assert_eq!(got.value(), None, "{id} {which}: a status carries no number");
        return;
    }
    let w = want.get("value");
    let x = got.value().unwrap_or_else(|| panic!("{id} {which}: expected a number, got {got:?}"));
    assert!(x.is_finite(), "{id} {which}");
    // D0: the pinned operation order gives these bits on every target.
    assert_eq!(x.to_bits(), hex_bits(w.get("pinned").str()), "{id} {which}: pinned bits");
    // AC-1: within the stated ulp of the correctly rounded exact value.
    let expected = f64::from_bits(hex_bits(w.get("expected").str()));
    let tol = w.get("tolerance_ulp").num() as u64;
    assert!(tol <= 1, "{id} {which}");
    assert!(ulps(x, expected) <= tol, "{id} {which}: {x} vs {expected} (tolerance {tol} ulp)");
}

fn side(s: &str) -> Side {
    match s {
        "BOTH" => Side::Both,
        "UPPER_ONLY" => Side::UpperOnly,
        "LOWER_ONLY" => Side::LowerOnly,
        other => panic!("side {other}"),
    }
}

/// AC-1, AC-3, AC-4 and every other fixture case: numbers, statuses and errors as the
/// reference labels them; then the cross-target digest of all results.
#[test]
fn fixture_cases_and_cross_target_digest() {
    let fx = fixture();
    assert_eq!(fx.get("algorithm").str(), CAP_INDEX_ALGORITHM_ID);
    assert_eq!(fx.get("default_min_n").num() as u64, DEFAULT_MIN_N);
    let cases = fx.get("cases").arr();
    assert!(cases.len() >= 30);
    let mut lines = String::new();
    let mut seen_one_ulp = false;
    for c in cases {
        let id = c.get("id").str();
        let want = c.get("expect");
        match run(c) {
            Err(e) => {
                let reason = want.get("error");
                assert!(!reason.is_null(), "{id}: unexpected error {e}");
                assert_eq!(e.code(), "stats.bad_capability_input", "{id}");
                let StatsError::BadCapabilityInput { reason: r } = e;
                assert_eq!(r.as_str(), reason.str(), "{id}");
                lines.push_str(&format!("{id} error:{}\n", r.as_str()));
            }
            Ok(p) => {
                assert!(want.get("error").is_null(), "{id}: expected {:?}, got {p:?}", want.get("error"));
                assert_eq!(p.side, side(want.get("side").str()), "{id}");
                assert_eq!(p.family, family(c.get("family").str()), "{id}");
                assert_eq!(p.estimator.as_str(), c.get("estimator").str(), "{id}");
                assert_eq!(p.algorithm, CAP_INDEX_ALGORITHM_ID, "{id}");
                check_index(id, "potential", p.potential, want.get("potential"));
                check_index(id, "centred", p.centred, want.get("centred"));
                seen_one_ulp |= ["potential", "centred"]
                    .iter()
                    .any(|k| want.get(k).get("value").get("tolerance_ulp") == &Json::Num(1.0));
                lines.push_str(&format!("{id} {} {}\n", token(p.potential), token(p.centred)));
            }
        }
    }
    assert!(seen_one_ulp, "the fixture keeps one case that is 1 ulp from the correctly rounded value");
    let got = fid_testkit::sha256_hex(lines.as_bytes());
    assert_eq!(got.strip_prefix("sha256:").unwrap_or(&got), fx.get("digest").str(), "cross-target result digest");
}

fn est(s: &str) -> EstimatorId {
    EstimatorId::new(s).unwrap()
}

/// AC-2: USL 10, no LSL, μ 7, σ 1, n 30: centred 1.0, potential NOT_APPLICABLE.
#[test]
fn ac2_one_sided_upper() {
    let p = capability(7.0, 1.0, 30, None, Some(10.0), DEFAULT_MIN_N, Family::P, est("overall_sd@1")).unwrap();
    assert_eq!(p.centred, IndexValue::Value(1.0));
    assert_eq!(p.potential, IndexValue::NotApplicable);
    assert_eq!(p.side, Side::UpperOnly);
}

/// AC-3: n 24 with σ 0 gives INSUFFICIENT_DATA everywhere; n 30 with σ 0 gives
/// UNDEFINED_ZERO_SPREAD for every applicable index.
#[test]
fn ac3_status_precedence() {
    let e = || est("rbar_d2@1");
    let a = capability(5.0, 0.0, 24, Some(0.0), Some(10.0), DEFAULT_MIN_N, Family::C, e()).unwrap();
    assert_eq!((a.potential, a.centred), (IndexValue::InsufficientData, IndexValue::InsufficientData));
    let b = capability(5.0, 0.0, 30, Some(0.0), Some(10.0), DEFAULT_MIN_N, Family::C, e()).unwrap();
    assert_eq!((b.potential, b.centred), (IndexValue::UndefinedZeroSpread, IndexValue::UndefinedZeroSpread));
    // One-sided: the potential index stays NOT_APPLICABLE; the applicable one takes the status.
    let c = capability(5.0, 0.0, 24, None, Some(10.0), DEFAULT_MIN_N, Family::C, e()).unwrap();
    assert_eq!((c.potential, c.centred), (IndexValue::NotApplicable, IndexValue::InsufficientData));
    let d = capability(5.0, 0.0, 30, Some(0.0), None, DEFAULT_MIN_N, Family::C, e()).unwrap();
    assert_eq!((d.potential, d.centred), (IndexValue::NotApplicable, IndexValue::UndefinedZeroSpread));
}

/// AC-4: USL 1, LSL 2 fails with stats.bad_capability_input.
#[test]
fn ac4_reversed_limits() {
    let e = capability(1.5, 0.1, 30, Some(2.0), Some(1.0), DEFAULT_MIN_N, Family::P, est("overall_sd@1")).unwrap_err();
    assert_eq!(e.code(), "stats.bad_capability_input");
    assert_eq!(e, StatsError::BadCapabilityInput { reason: BadInput::LimitsNotOrdered });
    assert_eq!(e.to_string(), "stats.bad_capability_input: limits_not_ordered");
}

/// Every non-finite input is refused, NaN included, whichever slot it is in.
#[test]
fn non_finite_inputs() {
    for bad in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let e = || est("overall_sd@1");
        let r = |reason| Err(StatsError::BadCapabilityInput { reason });
        assert_eq!(capability(bad, 1.0, 30, Some(0.0), Some(1.0), 25, Family::P, e()), r(BadInput::NonFiniteMean));
        assert_eq!(capability(0.5, bad, 30, Some(0.0), Some(1.0), 25, Family::P, e()), r(BadInput::NonFiniteSigma));
        assert_eq!(capability(0.5, 1.0, 30, Some(bad), Some(1.0), 25, Family::P, e()), r(BadInput::NonFiniteLsl));
        assert_eq!(capability(0.5, 1.0, 30, Some(0.0), Some(bad), 25, Family::P, e()), r(BadInput::NonFiniteUsl));
    }
}

/// The estimator ID syntax, against the hand-labelled list in the fixture.
#[test]
fn estimator_ids() {
    let fx = fixture();
    let list = fx.get("estimator_ids").arr();
    assert!(list.len() >= 15);
    for e in list {
        let s = e.get("id").str();
        let r = EstimatorId::new(s);
        assert_eq!(r.is_ok(), e.get("valid").truthy(), "{s:?}");
        match r {
            Ok(id) => assert_eq!(id.as_str(), s),
            Err(err) => assert_eq!(err, StatsError::BadCapabilityInput { reason: BadInput::BadEstimatorId }),
        }
    }
    assert!(EstimatorId::new(&format!("{}@1", "a".repeat(MAX_ESTIMATOR_ID_BYTES - 2))).is_ok());
    assert!(EstimatorId::new(&format!("{}@1", "a".repeat(MAX_ESTIMATOR_ID_BYTES - 1))).is_err());
}

/// Codes are stable strings.
#[test]
fn codes() {
    assert_eq!(Family::C.as_str(), "C");
    assert_eq!(Family::P.as_str(), "P");
    assert_eq!(Side::Both.as_str(), "BOTH");
    assert_eq!(Side::UpperOnly.as_str(), "UPPER_ONLY");
    assert_eq!(Side::LowerOnly.as_str(), "LOWER_ONLY");
    assert_eq!(IndexValue::Value(1.0).status(), None);
    assert_eq!(IndexValue::NotApplicable.status(), Some("NOT_APPLICABLE"));
    assert_eq!(IndexValue::InsufficientData.status(), Some("INSUFFICIENT_DATA"));
    assert_eq!(IndexValue::UndefinedZeroSpread.status(), Some("UNDEFINED_ZERO_SPREAD"));
    assert_eq!(CAP_INDEX_ALGORITHM_ID, "stats.cap.index@1.0.0");
}
