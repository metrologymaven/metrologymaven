//! The `instrument-model/v1` reader, canonical form, digest and range check of spec node
//! UNC.INST.SCHEMA (AC-1 digest, AC-2 error fields, AC-4 range, AC-5 differing content),
//! against `fixtures/unc/inst/` written by the independent reference `make_fixtures.py`
//! (digests from hashlib, bit patterns from `struct`, errors and verdicts hand-labelled from
//! the spec). All models there are invented examples (D-038, D-045).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_kunc::dist::{Params, UnitCode};
use fid_types::Digest;
use fid_unc_inst::model::{hex, shown_name, MAX_DEPTH, MAX_INPUT_BYTES, MAX_TOKEN_BYTES, UNSHOWN_NAME};
use fid_unc_inst::{
    check_range, BoundSide, ContextError, InstrumentModel, ModelError, ModelKind, NotCheckedReason, ObservationContext,
    RangeVerdict, SiUnit,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/unc/inst")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(dir().join(rel)).unwrap()
}

fn model(rel: &str) -> InstrumentModel {
    InstrumentModel::parse(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    let CValue::Obj(o) = v else { panic!("not an object: {v:?}") };
    o
}

fn s<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

fn expected() -> CValue {
    parse(&read("expected.json")).unwrap()
}

fn digests() -> Vec<(String, Digest)> {
    String::from_utf8(read("digests.txt"))
        .unwrap()
        .lines()
        .map(|l| {
            let (f, d) = l.split_once(' ').unwrap();
            (f.to_owned(), Digest::parse(d).unwrap())
        })
        .collect()
}

// ------------------------------------------------------------------ AC-1 digest, canonical form

#[test]
fn ac1_every_example_has_the_reference_digest() {
    let d = digests();
    assert_eq!(d.len(), 6);
    for (file, digest) in &d {
        let m = model(file);
        assert_eq!(m.digest(), *digest, "{file}");
        assert_eq!(fid_kci::kci::sha256_digest(m.canonical_json()), *digest, "{file}");
    }
}

#[test]
fn ac1_tracker_canonical_form_is_the_reference_and_reads_back_to_itself() {
    let m = model("tracker_example.json");
    assert_eq!(m.canonical_json(), read("tracker_example.canonical.json").as_slice());
    assert_eq!(fid_canon::canonicalize(m.canonical_json()).unwrap(), m.canonical_json());
    for (file, _) in digests() {
        let m = model(&file);
        let again = InstrumentModel::parse(m.canonical_json()).unwrap();
        assert_eq!(again.canonical_json(), m.canonical_json(), "{file}");
        assert_eq!(again, m, "{file}");
    }
}

#[test]
fn ac1_tracker_values_are_si_and_sorted() {
    let m = model("tracker_example.json");
    assert_eq!(m.id(), "im:tracker-example@1.0.0");
    assert_eq!((m.name(), m.semver()), ("tracker-example", "1.0.0"));
    assert_eq!(m.kind(), ModelKind::Observation);
    assert_eq!(m.device_class(), "tracker");
    let names: Vec<&str> = m.params().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["angle_sigma", "probing_noise", "range_const_sigma", "range_prop_sigma"]);
    // Hand-labelled from the spec: 15 um, 6 um/m, 5 urad, 10 um in SI.
    let p = m.param("range_const_sigma").unwrap();
    assert_eq!((p.unit, p.value), (SiUnit::Metre, 15e-6));
    let d = p.dist.unwrap();
    assert_eq!(d.unit(), UnitCode::Metre);
    assert_eq!(d.params(), Params::Normal { mu: 0.0, sigma: 15e-6 });
    assert_eq!(m.param("range_prop_sigma").map(|p| (p.unit, p.value)), Some((SiUnit::One, 6e-6)));
    assert_eq!(m.param("angle_sigma").map(|p| (p.unit, p.value)), Some((SiUnit::Radian, 5e-6)));
    assert_eq!(m.param("probing_noise").unwrap().dist.unwrap().params(), Params::Rect { a: -10e-6, b: 10e-6 });
    let axes: Vec<(&str, f64, f64, SiUnit)> =
        m.range().iter().map(|a| (a.axis.as_str(), a.min, a.max, a.unit)).collect();
    assert_eq!(axes, [("range", 0.5, 40.0, SiUnit::Metre), ("temperature", 283.15, 313.15, SiUnit::Kelvin)]);
    // REQ-006: the citation is kept exactly as given.
    let doc = parse(&read("tracker_example.json")).unwrap();
    assert_eq!(m.source().raw(), s(obj(&doc), "source"));
}

#[test]
fn ac1_respelled_units_and_order_give_one_digest() {
    let a = model("tracker_example.json");
    let b = model("tracker_example_respelled.json");
    assert_eq!(a.canonical_json(), b.canonical_json());
    assert_eq!(a.digest(), b.digest());
}

#[test]
fn ac5_same_id_different_content_has_another_digest() {
    let a = model("tracker_example.json");
    let c = model("tracker_example_conflict.json");
    assert_eq!(a.id(), c.id());
    assert_ne!(a.digest(), c.digest());
}

#[test]
fn every_distribution_kind_and_unit_reads() {
    let scanner = model("scanner_example.json");
    assert_eq!(
        scanner.param("point_noise").unwrap().dist.unwrap().params(),
        Params::ScaledT { mu: 0.0, s: 25e-6, nu: 10.0 }
    );
    assert_eq!(
        scanner.param("incidence_gain").unwrap().dist.unwrap().params(),
        Params::Trap { a: 0.0, b: 1.0, beta: 0.5 }
    );
    assert_eq!(scanner.range()[1].axis, "standoff");
    assert_eq!((scanner.range()[1].min, scanner.range()[1].max), (0.1, 0.3));
    let arm = model("arm_example.json");
    assert_eq!(arm.kind(), ModelKind::SpecDerived);
    assert_eq!(arm.param("probing_form").unwrap().dist.unwrap().params(), Params::Tri { a: -20e-6, b: 20e-6, c: 0.0 });
    assert_eq!(arm.param("probing_size").unwrap().dist, None);
    let cmm = model("cmm_example.json");
    assert_eq!(cmm.param("dwell").map(|p| (p.unit, p.value)), Some((SiUnit::Second, 0.05)));
    assert_eq!(cmm.param("length_error_prop").map(|p| (p.unit, p.value)), Some((SiUnit::One, 2e-6)));
    assert_eq!(
        cmm.param("length_error_const").unwrap().dist.unwrap().params(),
        Params::Arcsine { a: -1.5e-6, b: 1.5e-6 }
    );
}

// ------------------------------------------------------------------ AC-2 errors

#[test]
fn ac2_each_invalid_file_fails_naming_its_field() {
    let labels = parse(&read("invalid/expected.json")).unwrap();
    let labels = obj(&labels);
    assert_eq!(labels.len(), 21);
    for (file, want) in labels {
        let want = obj(want);
        let err = InstrumentModel::parse(&read(&format!("invalid/{file}"))).unwrap_err();
        assert_eq!((err.code(), err.field()), (s(want, "code"), s(want, "field")), "{file}");
    }
    // The three files AC-2 names, spelled out.
    let err = |f: &str| InstrumentModel::parse(&read(&format!("invalid/{f}"))).unwrap_err();
    assert_eq!(err("no_unit.json"), ModelError::MissingUnit { param: "range_const_sigma".into() });
    assert_eq!(err("inverted_range.json"), ModelError::InvalidRange { axis: "range".into() });
    assert_eq!(err("no_source.json"), ModelError::MissingSource);
}

#[test]
fn unknown_member_names_are_cut_or_replaced_in_the_field() {
    let doc = String::from_utf8(read("tracker_example.json")).unwrap();
    // `k` is JSON string text, inserted as a top-level member name.
    let with_key = |k: &str| {
        let d = doc.replacen('{', &format!("{{\"{k}\": \"x\","), 1);
        InstrumentModel::parse(d.as_bytes()).unwrap_err()
    };
    assert_eq!(with_key("notes").field(), "/notes");
    let long = "z".repeat(300);
    assert_eq!(with_key(&long).field(), format!("/{}", "z".repeat(MAX_TOKEN_BYTES)));
    for bad in ["Notes", "a/b", "a~b", r"zz‮bidi", "9lives", r"été"] {
        assert_eq!(with_key(bad).field(), format!("/{UNSHOWN_NAME}"), "{bad}");
    }
    assert_eq!(shown_name("ok_name"), "ok_name");
    // Cut at a character boundary: 63 ASCII bytes and a two-byte character keep 63 bytes.
    assert_eq!(shown_name(&format!("{}\u{e9}x", "a".repeat(MAX_TOKEN_BYTES - 1))), "a".repeat(MAX_TOKEN_BYTES - 1));
    assert_eq!(shown_name("ab\u{202e}"), UNSHOWN_NAME);
}

#[test]
fn reader_limits_are_bounded() {
    let mut big = read("tracker_example.json");
    big.resize(MAX_INPUT_BYTES + 1, b' ');
    assert_eq!(InstrumentModel::parse(&big).unwrap_err(), ModelError::SchemaInvalid { field: String::new() });
    let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert_eq!(InstrumentModel::parse(deep.as_bytes()).unwrap_err().code(), "schema_invalid");
    let dup = br#"{"schema":"instrument-model/v1","schema":"instrument-model/v1"}"#;
    assert_eq!(InstrumentModel::parse(dup).unwrap_err().code(), "schema_invalid");
    assert_eq!(InstrumentModel::parse(b"[]").unwrap_err(), ModelError::SchemaInvalid { field: String::new() });
}

// ------------------------------------------------------------------ AC-4 range check

fn context(entries: &[CValue]) -> ObservationContext {
    entries.iter().fold(ObservationContext::new(), |ctx, e| {
        let e = obj(e);
        ctx.with_text(s(e, "axis"), s(e, "value"), s(e, "unit")).unwrap()
    })
}

#[test]
fn ac4_52m_is_out_of_model_range_at_the_40m_bound() {
    let m = model("tracker_example.json");
    let ctx = ObservationContext::new().with("range", 52.0, SiUnit::Metre).unwrap();
    let v = check_range(&m, &ctx);
    assert_eq!(
        v,
        RangeVerdict::OutOfModelRange {
            axis: "range".into(),
            bound: fid_unc_inst::RangeBound { side: BoundSide::Upper, value: 40.0, unit: SiUnit::Metre }
        }
    );
    assert!(v.must_flag());
    assert_eq!(v.code(), "out_of_model_range");
}

#[test]
fn range_cases_match_the_reference() {
    let m = model("tracker_example.json");
    let ex = expected();
    let Some(CValue::Arr(cases)) = obj(&ex).get("range_cases") else { panic!("range_cases") };
    assert_eq!(cases.len(), 12);
    for case in cases {
        let c = obj(case);
        let Some(CValue::Arr(entries)) = c.get("context") else { panic!("context") };
        let got = check_range(&m, &context(entries)).to_value();
        assert_eq!(&got, c.get("verdict").unwrap(), "{}", s(c, "name"));
    }
}

#[test]
fn range_check_flags_unobserved_and_mismatched_axes() {
    let m = model("tracker_example.json");
    let ctx = ObservationContext::new().with("range", 12.5, SiUnit::Metre).unwrap();
    let v = check_range(&m, &ctx);
    assert_eq!(v, RangeVerdict::NotChecked { axis: "temperature".into(), reason: NotCheckedReason::NotObserved });
    assert!(v.must_flag());
    let ctx = ctx.with("temperature", 293.15, SiUnit::Kelvin).unwrap();
    assert_eq!(check_range(&m, &ctx), RangeVerdict::InRange);
    assert!(!RangeVerdict::InRange.must_flag());
    // Closed bounds: both ends are inside.
    for (r, t) in [(0.5, 283.15), (40.0, 313.15)] {
        let ctx = ObservationContext::new()
            .with("range", r, SiUnit::Metre)
            .unwrap()
            .with("temperature", t, SiUnit::Kelvin)
            .unwrap();
        assert_eq!(check_range(&m, &ctx), RangeVerdict::InRange, "{r} {t}");
    }
    // The next double above 40 m is outside.
    let above = f64::from_bits(40.0f64.to_bits() + 1);
    let ctx = ObservationContext::new().with("range", above, SiUnit::Metre).unwrap();
    assert_eq!(check_range(&m, &ctx).code(), "out_of_model_range");
    assert_eq!(hex(40.0), "0x4044000000000000");
}

#[test]
fn context_refuses_non_finite_and_duplicate_axes() {
    let ctx = ObservationContext::new();
    assert_eq!(ctx.clone().with("range", f64::NAN, SiUnit::Metre).unwrap_err(), ContextError::BadValue);
    assert_eq!(ctx.clone().with("range", f64::INFINITY, SiUnit::Metre).unwrap_err(), ContextError::BadValue);
    assert_eq!(ctx.clone().with_text("range", "1", "furlong").unwrap_err(), ContextError::BadValue);
    let once = ctx.with("range", 1.0, SiUnit::Metre).unwrap();
    assert_eq!(once.with("range", 2.0, SiUnit::Metre).unwrap_err(), ContextError::Duplicate);
}
