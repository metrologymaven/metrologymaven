#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! RPT.STATS.TREND.CONTRACT acceptance tests. Every expected value comes from the independent
//! stdlib reference `fixtures/rpt/stats/chart_series/make_fixtures.py` (hand-written examples,
//! hand-labelled cases), never from this crate.

use fid_canon::CValue;
use fid_chart_series::{
    check_supported, digest, parse, to_canonical_bytes, validate, AxisKind, ChartSeries, ChartSeriesError,
    ContractVersion, Digest, Exclusion, InvalidReason, Marker, Quantity, RefKind, ReviewStatus, SeriesKind,
    SeriesStatus, Unit, UnsupportedCause, UnverifiedRef, SCHEMA_JSON,
};
use fid_geo_frame::units::{AngleUnit, LengthUnit};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/rpt/stats/chart_series/", $name))
    };
}

const EXAMPLES: [(&str, &[u8]); 8] = [
    ("trend.json", fixture!("v1_examples/trend.json")),
    ("histogram.json", fixture!("v1_examples/histogram.json")),
    ("control_xbar.json", fixture!("v1_examples/control_xbar.json")),
    ("control_r.json", fixture!("v1_examples/control_r.json")),
    ("control_s.json", fixture!("v1_examples/control_s.json")),
    ("control_i.json", fixture!("v1_examples/control_i.json")),
    ("control_mr.json", fixture!("v1_examples/control_mr.json")),
    ("vector_group.json", fixture!("v1_examples/vector_group.json")),
];
const TREND: &[u8] = fixture!("v1_examples/trend.json");
const MINOR9: &[u8] = fixture!("v1_examples/trend_minor9.json");
const V2: &[u8] = fixture!("v2_trend.json");
const EXPECTED: &[u8] = fixture!("expected.json");
const CASES: &[u8] = fixture!("cases.json");
const SCHEMA_FILE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/schema/chart-series.schema.json"));

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(key).unwrap_or_else(|| panic!("no key {key}")),
        _ => panic!("not an object"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
}

fn items(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn invalid(field: &str, reason: InvalidReason) -> ChartSeriesError {
    ChartSeriesError::Invalid { field: field.to_owned(), reason }
}

fn trend() -> ChartSeries {
    parse(TREND).unwrap().series
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_every_kind_round_trips_bit_for_bit() {
    let expected = fid_canon::parse(EXPECTED).unwrap();
    let mut kinds = Vec::new();
    for (name, bytes) in EXAMPLES {
        let want = get(get(&expected, "examples"), name);
        let parsed = parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(parsed.ignored_fields.is_empty(), "{name}");
        let s = &parsed.series;
        assert_eq!(s.contract, ContractVersion::CURRENT, "{name}");
        assert_eq!(s.kind.as_str(), text(get(want, "kind")), "{name}");
        assert_eq!(s.points.len() as f64, num(get(want, "points")), "{name}");
        assert_eq!(s.ref_lines.len() as f64, num(get(want, "ref_lines")), "{name}");
        assert_eq!(to_canonical_bytes(s).unwrap(), bytes, "{name}: canonical round trip");
        assert_eq!(digest(s).unwrap().as_str(), text(get(want, "digest")), "{name}: digest");
        // And once more through the re-encoded bytes.
        assert_eq!(parse(&to_canonical_bytes(s).unwrap()).unwrap(), parsed, "{name}");
        kinds.push(s.kind);
    }
    kinds.sort();
    assert_eq!(kinds, SeriesKind::ALL, "one example per kind");
}

#[test]
fn ac1_f64_bits_survive() {
    let s = trend();
    // -0.0 and the smallest subnormal, spelled by their bits in the fixture.
    assert_eq!(s.points[2].y.to_bits(), 0x8000_0000_0000_0000);
    assert_eq!(s.points[3].y.to_bits(), 1);
    let v = parse(EXAMPLES[7].1).unwrap().series;
    assert_eq!(v.points[2].x, 1e308);
    assert_eq!(v.points[2].y, -1e-308);
    assert!(matches!(v.x.kind, AxisKind::Quantity { quantity: Quantity::Length, unit: Unit::Length(LengthUnit::Mm) }));
    let i = parse(EXAMPLES[5].1).unwrap().series;
    assert!(matches!(i.y.kind, AxisKind::Quantity { quantity: Quantity::Angle, unit: Unit::Angle(AngleUnit::Deg) }));
    assert_eq!(i.points[2].markers[0], Marker::OutOfTolerance);
    let x = parse(EXAMPLES[2].1).unwrap().series;
    assert_eq!(
        x.points[1].markers[1],
        Marker::RuleViolation {
            set_id: "nelson".to_owned(),
            set_version: 1,
            rule_id: "5".to_owned(),
            review: ReviewStatus::Unverified
        }
    );
    assert_eq!(x.provenance.frozen_limits.as_deref(), Some("lim-2026-0001"));
    let h = parse(EXAMPLES[1].1).unwrap().series;
    assert_eq!(h.provenance.binning_rule.as_deref(), Some("fixed_width@1"));
    assert_eq!(h.points[0].x_upper, Some(-7.5));
}

#[test]
fn ac1_schema_is_exported_2020_12() {
    assert_eq!(SCHEMA_JSON, SCHEMA_FILE);
    let s = fid_canon::parse(SCHEMA_JSON.as_bytes()).unwrap();
    assert_eq!(text(get(&s, "$schema")), "https://json-schema.org/draft/2020-12/schema");
    let strict_kind = get(get(get(get(&s, "$defs"), "series_strict"), "properties"), "kind");
    let kinds: Vec<&str> = items(get(strict_kind, "enum")).iter().map(text).collect();
    let ours: Vec<&str> = SeriesKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(kinds, ours);
    // Validation of the fixtures against this schema with an independent validator:
    // fixtures/rpt/stats/chart_series/make_fixtures.py --check (ci/fixture-checks.txt).
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_major_2_is_unsupported() {
    let e = parse(V2).unwrap_err();
    assert_eq!(e.code(), "report.chart_series_unsupported");
    assert_eq!(
        e,
        ChartSeriesError::Unsupported {
            major: 2,
            minor: 0,
            kind: Some("TREND".to_owned()),
            cause: UnsupportedCause::Major
        }
    );
    let expected = fid_canon::parse(EXPECTED).unwrap();
    let v2 = get(&expected, "v2");
    assert_eq!(text(get(v2, "code")), e.code());
    assert_eq!(num(get(v2, "major")), 2.0);
    assert!(e.to_string().contains("contract 2.0"), "{e}");

    // The same check on a typed series that states major 2.
    let mut s = trend();
    s.contract.major = 2;
    let want = ChartSeriesError::Unsupported {
        major: 2,
        minor: 0,
        kind: Some("TREND".to_owned()),
        cause: UnsupportedCause::Major,
    };
    assert_eq!(check_supported(&s, 1, SeriesKind::ALL), Err(want.clone()));
    assert_eq!(validate(&s), Err(want.clone()));
    assert_eq!(to_canonical_bytes(&s), Err(want));
}

#[test]
fn check_supported_kinds_and_majors() {
    let s = trend();
    assert_eq!(check_supported(&s, 1, SeriesKind::ALL), Ok(()));
    assert_eq!(check_supported(&s, 1, &[SeriesKind::Trend]), Ok(()));
    let kind_err = ChartSeriesError::Unsupported {
        major: 1,
        minor: 0,
        kind: Some("TREND".to_owned()),
        cause: UnsupportedCause::Kind,
    };
    assert_eq!(check_supported(&s, 1, &[SeriesKind::Histogram]), Err(kind_err.clone()));
    assert_eq!(check_supported(&s, 1, &[]), Err(kind_err), "an empty kind list supports nothing");
    // A future major-2 consumer refuses this major-1 series.
    assert_eq!(
        check_supported(&s, 2, SeriesKind::ALL),
        Err(ChartSeriesError::Unsupported {
            major: 1,
            minor: 0,
            kind: Some("TREND".to_owned()),
            cause: UnsupportedCause::Major
        })
    );
}

#[test]
fn unsupported_kind_echo_is_bounded() {
    let long = format!(r#"{{"contract":{{"major":3,"minor":0}},"kind":"{}"}}"#, "A".repeat(33));
    let e = parse(long.as_bytes()).unwrap_err();
    assert!(matches!(e, ChartSeriesError::Unsupported { major: 3, kind: None, .. }), "{e:?}");
    let odd = format!(r#"{{"contract":{{"major":0,"minor":0}},"kind":"trend{}"}}"#, '\u{202e}');
    assert!(matches!(parse(odd.as_bytes()).unwrap_err(), ChartSeriesError::Unsupported { major: 0, kind: None, .. }));
    let none = br#"{"contract":{"major":2,"minor":4}}"#;
    assert!(matches!(parse(none).unwrap_err(), ChartSeriesError::Unsupported { major: 2, minor: 4, kind: None, .. }));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_later_minor_ignores_and_reports_unknown_fields() {
    let expected = fid_canon::parse(EXPECTED).unwrap();
    let m9 = get(&expected, "minor9");
    let parsed = parse(MINOR9).unwrap();
    assert_eq!(parsed.series.contract, ContractVersion { major: 1, minor: 9 });
    assert_eq!(num(get(m9, "minor")), 9.0);
    let ignored: Vec<&str> = items(get(m9, "ignored")).iter().map(text).collect();
    assert_eq!(parsed.ignored_fields, ignored);
    // This crate cannot write minor 9's fields, so it does not write the series at all.
    assert_eq!(to_canonical_bytes(&parsed.series), Err(invalid("contract.minor", InvalidReason::NotEncodable)));
    // Apart from the ignored field and the minor, it is the trend example.
    let mut s = parsed.series;
    s.contract.minor = 0;
    assert_eq!(to_canonical_bytes(&s).unwrap(), TREND);
}

// ------------------------------------------------------------------ AC-4 and the other cases

#[test]
fn ac4_and_hand_labelled_cases() {
    let doc = fid_canon::parse(CASES).unwrap();
    let cases = items(get(&doc, "cases"));
    assert!(cases.len() >= 80, "{}", cases.len());
    let mut seen_ac4 = false;
    for c in cases {
        let name = text(get(c, "name"));
        let input = text(get(c, "text"));
        let got = parse(input.as_bytes());
        if matches!(get(c, "ok"), CValue::Bool(true)) {
            let parsed = got.unwrap_or_else(|e| panic!("{name}: {e}"));
            let ignored: Vec<&str> = items(get(c, "ignored")).iter().map(text).collect();
            assert_eq!(parsed.ignored_fields, ignored, "{name}");
            if name == "integral_float_version_accepted" {
                assert_eq!(to_canonical_bytes(&parsed.series).unwrap(), TREND, "{name}");
            }
            continue;
        }
        let e = got.err().unwrap_or_else(|| panic!("{name}: accepted"));
        assert_eq!(e.code(), text(get(c, "code")), "{name}");
        let ChartSeriesError::Invalid { field, reason } = &e else { panic!("{name}: {e:?}") };
        assert_eq!(field, text(get(c, "field")), "{name}");
        assert_eq!(reason.as_str(), text(get(c, "reason")), "{name}");
        if name == "ac4_no_dataset_digest" {
            assert_eq!(e, invalid("provenance.dataset_digest", InvalidReason::Missing));
            assert_eq!(e.to_string(), "report.chart_series_invalid: provenance.dataset_digest: missing");
            seen_ac4 = true;
        }
    }
    assert!(seen_ac4);
}

// ------------------------------------------------------------------ typed series

#[test]
fn typed_series_are_validated_before_writing() {
    let mut s = trend();
    s.points[0].y = f64::NAN;
    assert_eq!(validate(&s), Err(invalid("points[0].y", InvalidReason::NonFinite)));
    assert_eq!(to_canonical_bytes(&s), Err(invalid("points[0].y", InvalidReason::NonFinite)));

    let mut s = trend();
    s.y.kind = AxisKind::Quantity { quantity: Quantity::Angle, unit: Unit::Length(LengthUnit::Mm) };
    assert_eq!(validate(&s), Err(invalid("y.unit", InvalidReason::UnitMismatch)));

    let mut s = trend();
    s.provenance.dataset_digest = Digest::TOMBSTONE;
    assert_eq!(digest(&s), Err(invalid("provenance.dataset_digest", InvalidReason::Tombstone)));

    let mut s = trend();
    s.ref_lines[1].value = f64::INFINITY;
    assert_eq!(validate(&s), Err(invalid("ref_lines[1].value", InvalidReason::NonFinite)));

    let mut s = trend();
    s.points[4].markers = vec![Marker::Indeterminate, Marker::OutOfTolerance];
    assert_eq!(validate(&s), Err(invalid("points[4].markers[1]", InvalidReason::NotCanonicalOrder)));

    let mut s = trend();
    s.contract.minor = 1_000_000;
    assert_eq!(validate(&s), Err(invalid("contract.minor", InvalidReason::OutOfRange)));

    // Markers and reference lines at their bounds are accepted.
    let mut s = parse(EXAMPLES[2].1).unwrap().series;
    s.points[0].markers = (0..64)
        .map(|i| Marker::RuleViolation {
            set_id: "nelson".to_owned(),
            set_version: 1,
            rule_id: format!("r{i:02}"),
            review: ReviewStatus::Unverified,
        })
        .collect();
    assert_eq!(validate(&s), Ok(()));
    let mut s = trend();
    s.ref_lines = (0..64).map(|i| fid_chart_series::RefLine { value: f64::from(i), ..s.ref_lines[0] }).collect();
    assert_eq!(validate(&s), Ok(()));
}

#[test]
fn histogram_degenerate_bin_and_zero_edges() {
    let mut h = parse(EXAMPLES[1].1).unwrap().series;
    // RPT.STATS.TREND.HIST's all-equal data set: one bin [v, v].
    h.points.truncate(1);
    h.points[0].x_upper = Some(h.points[0].x);
    assert_eq!(validate(&h), Ok(()));
    // -0.0 to +0.0 is ascending in totalOrder; +0.0 to -0.0 is not.
    h.points[0].x = -0.0;
    h.points[0].x_upper = Some(0.0);
    assert_eq!(validate(&h), Ok(()));
    h.points[0].x = 0.0;
    h.points[0].x_upper = Some(-0.0);
    assert_eq!(validate(&h), Err(invalid("points[0].x_upper", InvalidReason::UpperBelowLower)));
    h.points[0].x_upper = None;
    h.points[0].y = 9_007_199_254_740_992.0;
    assert_eq!(validate(&h), Ok(()));
    h.points[0].y = 9_007_199_254_740_994.0;
    assert_eq!(validate(&h), Err(invalid("points[0].y", InvalidReason::BadCount)));
}

#[test]
fn json_limits_are_refusals() {
    assert_eq!(parse(b"").unwrap_err(), invalid("$", InvalidReason::Syntax));
    assert_eq!(parse(b"\xff").unwrap_err(), invalid("$", InvalidReason::InvalidUtf8));
    let deep = format!("{}{}", "[".repeat(20), "]".repeat(20));
    assert_eq!(parse(deep.as_bytes()).unwrap_err(), invalid("$", InvalidReason::InputLimit));
    let t = std::str::from_utf8(TREND).unwrap().replacen("\"minor\":0", "\"minor\":1e400", 1);
    assert_eq!(parse(t.as_bytes()).unwrap_err(), invalid("$", InvalidReason::NonFiniteNumber));
}

#[test]
fn reason_names_are_unique() {
    use InvalidReason as R;
    let all = [
        R::Syntax,
        R::DuplicateKey,
        R::InputLimit,
        R::InvalidUtf8,
        R::NonFiniteNumber,
        R::WrongType,
        R::Missing,
        R::UnknownField,
        R::NotInteger,
        R::OutOfRange,
        R::BadHex,
        R::NonFinite,
        R::BadFormat,
        R::UnknownValue,
        R::TooMany,
        R::UnitMismatch,
        R::NoValueUnit,
        R::AxisKind,
        R::NotCanonicalOrder,
        R::ConflictingMarkers,
        R::NotApplicable,
        R::RequiredForKind,
        R::BadCount,
        R::UpperBelowLower,
        R::Tombstone,
        R::NotEncodable,
        R::NegativeZero,
        R::Duplicate,
        R::StatusConflict,
        R::NotReferenced,
        R::NotInUnverified,
        R::ConflictingReview,
    ];
    let mut names: Vec<&str> = all.iter().map(|r| r.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all.len());
}

// ------------------------------------------------------------------ review, status and provenance

const CONSTANTS: &str = "spc.constants.normal@1.0.0";

fn example(name: &str) -> ChartSeries {
    let (_, bytes) = EXAMPLES.iter().find(|(n, _)| *n == name).unwrap();
    parse(bytes).unwrap().series
}

fn rule_set(id: &str, v: u32) -> UnverifiedRef {
    UnverifiedRef::RuleSet { set_id: id.to_owned(), set_version: v }
}

#[test]
fn review_status_is_carried_and_kept_distinct() {
    let x = example("control_xbar.json");
    assert!(x.points[1]
        .markers
        .iter()
        .all(|m| matches!(m, Marker::RuleViolation { review: ReviewStatus::Unverified, .. })));
    assert_eq!(x.provenance.constants, [CONSTANTS]);
    assert_eq!(
        x.provenance.unverified,
        [UnverifiedRef::Constants(CONSTANTS.to_owned()), rule_set("nelson", 1), rule_set("western_electric", 1)]
    );
    let i = example("control_i.json");
    assert!(matches!(
        &i.points[2].markers[1],
        Marker::RuleViolation { set_id, review: ReviewStatus::Unverified, .. } if set_id == "western_electric"
    ));
    let s = example("control_s.json");
    assert!(matches!(&s.points[1].markers[0], Marker::RuleViolation { review: ReviewStatus::ClaimedByFile, .. }));

    // A claimed set must be listed as unverified; a reviewed set must not be.
    let mut t = s.clone();
    t.provenance.unverified.pop();
    assert_eq!(validate(&t), Err(invalid("points[1].markers[0].review", InvalidReason::NotInUnverified)));
    let mut t = s.clone();
    if let Marker::RuleViolation { review, .. } = &mut t.points[1].markers[0] {
        *review = ReviewStatus::ExpertReviewed;
    }
    assert_eq!(validate(&t), Err(invalid("points[1].markers[0].review", InvalidReason::ConflictingReview)));
    t.provenance.unverified.pop();
    assert_eq!(validate(&t), Ok(()));
    // The three statuses have three spellings and none upgrades on a round trip.
    let names: Vec<&str> = ReviewStatus::ALL.iter().map(|r| r.as_str()).collect();
    assert_eq!(names, ["UNVERIFIED", "CLAIMED_BY_FILE", "EXPERT_REVIEWED"]);
    let back = parse(&to_canonical_bytes(&t).unwrap()).unwrap().series;
    assert!(matches!(&back.points[1].markers[0], Marker::RuleViolation { review: ReviewStatus::ExpertReviewed, .. }));
    assert_eq!(parse(&to_canonical_bytes(&s).unwrap()).unwrap().series, s);
}

#[test]
fn control_only_markers_and_lines() {
    let mut t = trend();
    t.points[0].markers = vec![Marker::RuleViolation {
        set_id: "nelson".to_owned(),
        set_version: 1,
        rule_id: "1".to_owned(),
        review: ReviewStatus::Unverified,
    }];
    assert_eq!(validate(&t), Err(invalid("points[0].markers[0]", InvalidReason::NotApplicable)));
    for (name, kind) in
        [("trend.json", RefKind::Ucl), ("histogram.json", RefKind::Centre), ("vector_group.json", RefKind::Lcl)]
    {
        let mut s = example(name);
        let unit = match if s.kind == SeriesKind::Histogram { s.x.kind } else { s.y.kind } {
            AxisKind::Quantity { unit, .. } => unit,
            AxisKind::Ordinal => panic!("{name}"),
        };
        s.ref_lines = vec![fid_chart_series::RefLine { kind, value: 1.0, unit }];
        assert_eq!(validate(&s), Err(invalid("ref_lines[0].kind", InvalidReason::NotApplicable)), "{name}");
    }
    let mut t = trend();
    t.provenance.constants = vec![CONSTANTS.to_owned()];
    assert_eq!(validate(&t), Err(invalid("provenance.constants", InvalidReason::NotApplicable)));
    let mut x = example("control_xbar.json");
    x.provenance.constants.clear();
    x.provenance.unverified.remove(0);
    assert_eq!(validate(&x), Err(invalid("provenance.constants", InvalidReason::RequiredForKind)));
}

#[test]
fn status_rules() {
    let mut t = trend();
    t.status = SeriesStatus::InsufficientData;
    assert_eq!(validate(&t), Err(invalid("points", InvalidReason::StatusConflict)));
    t.points.clear();
    assert_eq!(validate(&t), Ok(()));
    t.status = SeriesStatus::Ok;
    assert_eq!(validate(&t), Err(invalid("status", InvalidReason::StatusConflict)));

    // SPC.CHART-004: points without limit lines and without rule markers.
    let mut x = example("control_xbar.json");
    x.status = SeriesStatus::InsufficientData;
    assert_eq!(validate(&x), Err(invalid("points[1].markers[0]", InvalidReason::StatusConflict)));
    for p in &mut x.points {
        p.markers.clear();
    }
    assert_eq!(validate(&x), Err(invalid("ref_lines[0].kind", InvalidReason::StatusConflict)));
    x.ref_lines.clear();
    assert_eq!(validate(&x), Ok(()));
    let bytes = to_canonical_bytes(&x).unwrap();
    assert!(std::str::from_utf8(&bytes).unwrap().contains(r#""status":"INSUFFICIENT_DATA""#));
}

#[test]
fn provenance_lists_are_canonical() {
    let mut t = trend();
    assert_eq!(
        t.provenance.excluded,
        [
            Exclusion { source_index: 5, reason: "NOT_DECIDABLE".to_owned() },
            Exclusion { source_index: 7, reason: "USER".to_owned() }
        ]
    );
    t.provenance.excluded.swap(0, 1);
    assert_eq!(validate(&t), Err(invalid("provenance.excluded[1]", InvalidReason::NotCanonicalOrder)));
    t.provenance.excluded[0] = Exclusion { source_index: 7, reason: "user".to_owned() };
    assert_eq!(validate(&t), Err(invalid("provenance.excluded[0].reason", InvalidReason::BadFormat)));
    t.provenance.excluded.clear();
    assert_eq!(validate(&t), Ok(()), "an empty exclusion list is valid");

    let mut x = example("control_xbar.json");
    x.provenance.unverified.push(UnverifiedRef::Constants("spc.constants.other@1.0.0".to_owned()));
    assert_eq!(validate(&x), Err(invalid("provenance.unverified[3]", InvalidReason::NotCanonicalOrder)));
    x.provenance.unverified.pop();
    x.provenance.unverified.insert(1, UnverifiedRef::Constants("spc.constants.other@1.0.0".to_owned()));
    assert_eq!(validate(&x), Err(invalid("provenance.unverified[1]", InvalidReason::NotReferenced)));
    let mut t = trend();
    t.provenance.unverified = vec![rule_set("nelson", 1)];
    assert_eq!(validate(&t), Err(invalid("provenance.unverified[0]", InvalidReason::NotApplicable)));
}

#[test]
fn reference_lines_refuse_negative_zero_and_equal_pairs() {
    let mut h = example("histogram.json");
    assert_eq!(h.ref_lines[0].value.to_bits(), 0, "the NOMINAL line is +0.0");
    h.ref_lines[0].value = -0.0;
    assert_eq!(validate(&h), Err(invalid("ref_lines[0].value", InvalidReason::NegativeZero)));
    let mut t = trend();
    t.ref_lines[2] = fid_chart_series::RefLine { kind: RefKind::Lsl, ..t.ref_lines[1] };
    assert_eq!(validate(&t), Err(invalid("ref_lines[2]", InvalidReason::Duplicate)));
    // Equal values of different kinds are two lines (control_i: NOMINAL and CENTRE at 45 deg).
    let i = example("control_i.json");
    assert_eq!(i.ref_lines[0].value, i.ref_lines[2].value);
    assert_eq!(validate(&i), Ok(()));
}
