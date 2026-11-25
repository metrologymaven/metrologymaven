//! Acceptance criteria of spec node `HAL.IDL.SENS.GAUENV` (AC-1 .. AC-5) and its requirements.
//!
//! Fixture bytes, digests, case outcomes and both package exports come from
//! `fixtures/hal/idl/sens/gauenv/make_fixtures.py` (an independent stdlib oracle that also
//! checks every hand-labelled case outcome); the settings, describe-info and evaluator tests
//! are hand-labelled from the leaf's rules. All device data is invented (D-038).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::caps::environment::{self, check_readings as check_env, core_setting_schemas, ENVIRONMENT_CORE};
use fid_hal_idl::caps::gauge::{self, check_readings as check_gauge, check_zero, GAUGE_CORE};
use fid_hal_idl::caps::reading::{check_series, MAX_IDENTITY_BYTES};
use fid_hal_idl::caps::stream::{check_sequence_contiguity, BatchRecord, BatchSummary, RecordOrigin, STREAM_COMMON};
use fid_hal_idl::{
    validate_and_digest_json, validate_package, CapabilityPackage, ConfigSchema, Configuration, EnvBatch, EnvReading,
    EnvironmentPackage, EnvironmentSettings, ErrorCode, GaugeBatch, GaugeInfo, GaugeKind, GaugeOperation, GaugePackage,
    GaugeReading, PackageDescription, Quantity, ReadingError, SettingSchema, SettingType, SettingValue, SiUnit,
    UntrustedText, ZeroCheck,
};
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fix(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/sens/gauenv").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fix(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn json(name: &str) -> CValue {
    fid_canon::parse(&read(name)).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn n(v: &CValue) -> u64 {
    match v {
        CValue::Num(x) => *x as u64,
        CValue::Str(t) => t.parse().unwrap(),
        other => panic!("not a number: {other:?}"),
    }
}

fn bits(text: &str) -> u64 {
    u64::from_str_radix(text.strip_prefix("0x").unwrap(), 16).unwrap()
}

fn q(v: f64, u: SiUnit) -> Quantity {
    Quantity::new(v, u).unwrap()
}

fn case(name: &str) -> Vec<u8> {
    let CValue::Arr(cases) = json("cases.json") else { panic!("cases") };
    let c = cases.iter().find(|c| s(get(c, "name")) == name).unwrap_or_else(|| panic!("no case {name}"));
    s(get(c, "json")).as_bytes().to_vec()
}

fn caliper() -> GaugeReading {
    GaugeReading::from_json(&read("caliper_read.json")).unwrap()
}

fn room() -> EnvReading {
    EnvReading::from_json(&read("room_env.json")).unwrap()
}

fn want_summary(v: &CValue) -> BatchSummary {
    BatchSummary {
        first_seq: n(get(v, "first_seq")),
        last_seq: n(get(v, "last_seq")),
        bytes: n(get(v, "bytes")),
        span_ns: match get(v, "span_ns") {
            CValue::Null => None,
            other => Some(n(other)),
        },
    }
}

// ------------------------------------------------------------------ AC-1, AC-2

#[test]
fn ac1_caliper_reading_passes_and_round_trips() {
    let bytes = read("caliper_read.json");
    let want = get(&json("expected.json"), "caliper_read").clone();
    let r = GaugeReading::from_json(&bytes).unwrap();
    r.validate().unwrap();
    assert_eq!(r.value.unit(), SiUnit::Metre);
    assert_eq!(r.resolution.unit(), SiUnit::Metre);
    assert_eq!(r.value.value().to_bits(), bits(s(get(&want, "value"))));
    assert_eq!(r.resolution.value().to_bits(), bits(s(get(&want, "resolution"))));
    assert_eq!(r.value.value(), 0.01234);
    assert_eq!(r.resolution.value(), 1e-5);
    assert_eq!((r.value.value() / r.resolution.value()).to_bits(), bits(s(get(&want, "ratio"))));
    assert_eq!(r.steps(), n(get(&want, "steps")) as f64);
    assert_eq!(r.calibration_ref.as_ref().unwrap().as_str(), s(get(&want, "calibration_ref")));
    assert_eq!(r.t_dev, None);
    assert_eq!(r.gauge_identity.as_display(), "Example Gauges EC-150 caliper, serial EX-0001 (invented, D-038)");

    // Round-trips identically.
    let again = r.to_json().unwrap();
    assert_eq!(again, bytes);
    assert_eq!(again.len() as u64, n(get(&want, "bytes")));
    assert_eq!(Digest::from_bytes(fid_canon::digest_bytes(&again).0).as_str(), s(get(&want, "sha256")));
    assert_eq!(GaugeReading::from_json(&again).unwrap(), r);
}

#[test]
fn ac2_unquantised_value_fails() {
    let e = GaugeReading::from_json(&case("g_not_quantised")).unwrap_err();
    assert_eq!(e, ReadingError::NotQuantised);
    assert_eq!(e.code(), "reading_not_quantised");
    assert_eq!(gauge::item_of(&e), gauge::ITEM_QUANTISATION);

    // The same fault in memory.
    let mut r = caliper();
    r.value = q(0.012345, SiUnit::Metre);
    assert_eq!(r.validate(), Err(ReadingError::NotQuantised));
    assert_eq!(r.to_json().unwrap_err().code(), "reading_not_quantised");
    // One hundred-thousandth of a step off at 1234 steps is outside 1e-9 relative too.
    r.value = q(0.0123400001, SiUnit::Metre);
    assert_eq!(r.validate(), Err(ReadingError::NotQuantised));
    // Negative values and zero are quantised like positive ones.
    for v in [-0.01234, 0.0, -0.0] {
        r.value = q(v, SiUnit::Metre);
        r.validate().unwrap();
    }
}

#[test]
fn quantisation_needs_a_usable_resolution() {
    let mut r = caliper();
    r.resolution = q(0.0, SiUnit::Metre);
    assert_eq!(r.validate(), Err(ReadingError::OutOfRange { field: "resolution", reason: "not_positive" }));
    r.resolution = q(-1e-5, SiUnit::Metre);
    assert_eq!(r.validate().unwrap_err().code(), "value_out_of_range");
    // So many steps that every value would look quantised.
    r.value = q(1.0, SiUnit::Metre);
    r.resolution = q(1e-300, SiUnit::Metre);
    assert_eq!(r.validate(), Err(ReadingError::OutOfRange { field: "value", reason: "too_many_steps" }));
    assert_eq!(gauge::item_of(&r.validate().unwrap_err()), gauge::ITEM_QUANTISATION);
    // Units: m or rad, the resolution in the value's unit.
    let mut r = caliper();
    r.resolution = q(1e-5, SiUnit::Radian);
    assert_eq!(r.validate(), Err(ReadingError::UnitUnsupported { field: "resolution", unit: "rad".to_string() }));
    r.value = q(0.5235, SiUnit::Radian);
    r.resolution = q(1e-4, SiUnit::Radian);
    r.validate().unwrap();
    r.value = q(1.0, SiUnit::Kelvin);
    r.resolution = q(1.0, SiUnit::Kelvin);
    assert_eq!(r.validate().unwrap_err().code(), "unit_unsupported");
}

// ------------------------------------------------------------------ AC-3, AC-4

#[test]
fn ac3_humidity_above_one_is_out_of_range() {
    let e = EnvReading::from_json(&case("e_humidity_1_3")).unwrap_err();
    assert_eq!(e, ReadingError::OutOfRange { field: "humidity", reason: "outside_0_1" });
    assert_eq!(e.code(), "value_out_of_range");
    assert_eq!(environment::item_of(&e), environment::ITEM_UNITS);

    let mut r = room();
    r.humidity = Some(q(1.3, SiUnit::One));
    assert_eq!(r.validate().unwrap_err().code(), "value_out_of_range");
    r.humidity = Some(q(-0.01, SiUnit::One));
    assert_eq!(r.validate().unwrap_err().code(), "value_out_of_range");
    r.humidity = Some(q(1.0, SiUnit::One));
    r.validate().unwrap();
}

#[test]
fn ac4_room_environment_passes() {
    let bytes = read("room_env.json");
    let want = get(&json("expected.json"), "room_env").clone();
    let r = EnvReading::from_json(&bytes).unwrap();
    assert_eq!(r.temperature.value(), 293.15);
    assert_eq!(r.temperature.value().to_bits(), bits(s(get(&want, "temperature"))));
    assert_eq!(r.temperature.unit(), SiUnit::Kelvin);
    assert_eq!(r.temperature_u.value(), 0.05);
    assert_eq!(r.temperature_u.value().to_bits(), bits(s(get(&want, "temperature_u"))));
    assert_eq!(r.temperature_u.unit(), SiUnit::Kelvin);
    assert_eq!(r.sampling_time.unit(), SiUnit::Second);
    assert_eq!(r.humidity.unwrap().unit(), SiUnit::One);
    assert_eq!(r.pressure.unwrap().unit(), SiUnit::Pascal);
    let again = r.to_json().unwrap();
    assert_eq!(again, bytes);
    assert_eq!(again.len() as u64, n(get(&want, "bytes")));
    assert_eq!(Digest::from_bytes(fid_canon::digest_bytes(&again).0).as_str(), s(get(&want, "sha256")));
}

#[test]
fn environment_domains_and_units() {
    let check = |edit: &dyn Fn(&mut EnvReading)| {
        let mut r = room();
        edit(&mut r);
        r.validate()
    };
    assert_eq!(
        check(&|r| r.temperature_u = q(0.0, SiUnit::Kelvin)),
        Err(ReadingError::OutOfRange { field: "temperature_u", reason: "not_positive" })
    );
    assert_eq!(
        check(&|r| r.temperature = q(293.15, SiUnit::Metre)),
        Err(ReadingError::UnitUnsupported { field: "temperature", unit: "m".to_string() })
    );
    assert_eq!(
        check(&|r| r.pressure = Some(q(-1.0, SiUnit::Pascal))),
        Err(ReadingError::OutOfRange { field: "pressure", reason: "not_positive" })
    );
    assert_eq!(
        check(&|r| r.sampling_time = q(0.0, SiUnit::Second)),
        Err(ReadingError::OutOfRange { field: "sampling_time", reason: "not_positive" })
    );
    assert_eq!(
        check(&|r| r.sensor_identity = UntrustedText::new("")),
        Err(ReadingError::Invalid { pointer: "/sensor_identity".to_string(), reason: "identity_invalid" })
    );
    let long = "x".repeat(MAX_IDENTITY_BYTES + 1);
    assert_eq!(check(&|r| r.sensor_identity = UntrustedText::new(&long)).unwrap_err().code(), "reading_invalid");
    // Uncertainty presence: a reading without it is not a reading.
    assert_eq!(
        EnvReading::from_json(&case("e_uncertainty_missing")).unwrap_err(),
        ReadingError::Invalid { pointer: "/temperature_u".to_string(), reason: "field_missing" }
    );
    // Optional quantities may be absent.
    check(&|r| {
        r.humidity = None;
        r.pressure = None;
        r.t_dev = None;
    })
    .unwrap();
}

// ------------------------------------------------------------------ the case table

#[test]
fn every_case_has_the_oracle_outcome() {
    let CValue::Arr(cases) = json("cases.json") else { panic!("cases") };
    assert!(cases.len() >= 80);
    for c in &cases {
        let name = s(get(c, "name"));
        let bytes = s(get(c, "json")).as_bytes().to_vec();
        let want = s(get(c, "expect"));
        let got: Result<Option<BatchSummary>, ReadingError> = match (s(get(c, "form")), s(get(c, "kind"))) {
            ("reading", "gauge") => GaugeReading::from_json(&bytes).map(|r| {
                assert_eq!(r.to_json().unwrap(), bytes, "{name}");
                None
            }),
            ("reading", "env") => EnvReading::from_json(&bytes).map(|r| {
                assert_eq!(r.to_json().unwrap(), bytes, "{name}");
                None
            }),
            ("batch", "gauge") => GaugeBatch::from_json(&bytes).map(|b| {
                assert_eq!(b.to_json().unwrap(), bytes, "{name}");
                Some(b.summary().unwrap())
            }),
            ("batch", "env") => EnvBatch::from_json(&bytes).map(|b| {
                assert_eq!(b.to_json().unwrap(), bytes, "{name}");
                Some(b.summary().unwrap())
            }),
            other => panic!("{name}: unknown case form {other:?}"),
        };
        match got {
            Ok(summary) => {
                assert_eq!(want, "ok", "{name}: passed, oracle says {want}");
                if let Some(sum) = summary {
                    assert_eq!(sum, want_summary(get(c, "summary")), "{name}");
                }
            }
            Err(e) => assert_eq!(e.code(), want, "{name}: {e}"),
        }
    }
}

#[test]
fn only_canonical_bytes_are_accepted() {
    for name in ["g_whitespace", "g_key_order", "g_trailing_newline"] {
        assert_eq!(
            GaugeReading::from_json(&case(name)).unwrap_err(),
            ReadingError::Invalid { pointer: String::new(), reason: "not_canonical" },
            "{name}"
        );
    }
    assert_eq!(
        GaugeBatch::from_json(&case("gb_whitespace")).unwrap_err(),
        ReadingError::Invalid { pointer: String::new(), reason: "not_canonical" }
    );
    assert_eq!(
        GaugeReading::from_json(&case("g_duplicate_key")).unwrap_err(),
        ReadingError::Invalid { pointer: String::new(), reason: "json" }
    );
}

// ------------------------------------------------------------------ D-045: no origin on the wire

#[test]
fn readings_and_batches_cannot_claim_an_origin() {
    for name in ["g_origin_device", "g_origin_simulated"] {
        assert_eq!(
            GaugeReading::from_json(&case(name)).unwrap_err(),
            ReadingError::Invalid { pointer: "/origin".to_string(), reason: "field_unknown" },
            "{name}"
        );
    }
    assert_eq!(
        EnvReading::from_json(&case("e_origin_device")).unwrap_err(),
        ReadingError::Invalid { pointer: "/origin".to_string(), reason: "field_unknown" }
    );
    assert_eq!(
        GaugeBatch::from_json(&case("gb_origin_device")).unwrap_err(),
        ReadingError::Invalid { pointer: "/origin".to_string(), reason: "field_unknown" }
    );
    assert_eq!(
        GaugeBatch::from_json(&case("gb_reading_origin_device")).unwrap_err(),
        ReadingError::Invalid { pointer: "/readings/0/origin".to_string(), reason: "field_unknown" }
    );

    // Batch summaries feed Stream-common@1, labelled synthetic; they cannot become device data.
    let a = GaugeBatch::from_json(&case("gb_ok")).unwrap();
    let mut b = a.clone();
    b.first_seq = a.last_seq().unwrap() + 1;
    for r in &mut b.readings {
        r.t_host += 1_000_000_000;
    }
    let sums = vec![a.summary().unwrap(), b.summary().unwrap()];
    check_sequence_contiguity(&sums).unwrap();
    let record = BatchRecord::new(RecordOrigin::Synthetic, 60_000_000_000, sums.clone()).unwrap();
    assert_eq!(record.origin(), RecordOrigin::Synthetic);
    let e = BatchRecord::new(RecordOrigin::Device, 60_000_000_000, sums).unwrap_err();
    assert_eq!(e.code(), "stream_record_invalid");
}

// ------------------------------------------------------------------ series rules

#[test]
fn repeated_readings_are_refused() {
    assert_eq!(GaugeBatch::from_json(&case("gb_repeat_no_t_dev")).unwrap_err(), ReadingError::Duplicate { index: 2 });
    assert_eq!(
        GaugeBatch::from_json(&case("gb_repeat_with_t_dev")).unwrap_err(),
        ReadingError::NotMonotonic { field: "t_dev", index: 2 }
    );
    assert_eq!(EnvBatch::from_json(&case("eb_repeat_no_t_dev")).unwrap_err(), ReadingError::Duplicate { index: 2 });

    // In memory: a batch with the second reading sent twice.
    let mut b = GaugeBatch::from_json(&case("gb_ok")).unwrap();
    let second = b.readings[1].clone();
    b.readings.insert(2, second);
    assert_eq!(b.validate(), Err(ReadingError::Duplicate { index: 2 }));
    assert_eq!(b.to_json().unwrap_err().code(), "reading_duplicate");
    // Readings received together may share a host time when they differ.
    let ok = GaugeBatch::from_json(&case("gb_ok_shared_t_host")).unwrap();
    assert_eq!(ok.readings[0].t_host, ok.readings[1].t_host);
    // A repeat that is not adjacent is caught within the shared host time too.
    let mut c = ok.clone();
    let first = c.readings[0].clone();
    c.readings.insert(2, first);
    c.readings[3].t_host = c.readings[0].t_host;
    assert_eq!(check_series(&c.readings), Err(ReadingError::Duplicate { index: 2 }));
}

#[test]
fn stream_fields_do_not_change() {
    let cases = [
        ("gb_identity_changed", "gauge_identity", 1),
        ("gb_resolution_changed", "resolution", 2),
        ("gb_unit_changed", "unit", 1),
        ("gb_calibration_changed", "calibration_ref", 2),
        ("gb_t_dev_partial", "t_dev", 1),
    ];
    for (name, field, index) in cases {
        assert_eq!(
            GaugeBatch::from_json(&case(name)).unwrap_err(),
            ReadingError::Inconsistent { field, index },
            "{name}"
        );
    }
    let cases = [
        ("eb_sampling_time_changed", "sampling_time", 1),
        ("eb_humidity_dropped", "humidity", 2),
        ("eb_identity_changed", "sensor_identity", 1),
    ];
    for (name, field, index) in cases {
        assert_eq!(
            EnvBatch::from_json(&case(name)).unwrap_err(),
            ReadingError::Inconsistent { field, index },
            "{name}"
        );
    }
    assert_eq!(
        GaugeBatch::from_json(&case("gb_t_host_decreasing")).unwrap_err(),
        ReadingError::NotMonotonic { field: "t_host", index: 2 }
    );
}

#[test]
fn batch_ranges_hold_one_number_per_reading() {
    assert_eq!(
        GaugeBatch::from_json(&case("gb_seq_range_mismatch")).unwrap_err(),
        ReadingError::Invalid { pointer: "/last_seq".to_string(), reason: "seq_range_mismatch" }
    );
    assert_eq!(
        GaugeBatch::from_json(&case("gb_range_reversed")).unwrap_err(),
        ReadingError::Invalid { pointer: "/last_seq".to_string(), reason: "range_reversed" }
    );
    assert_eq!(
        GaugeBatch::from_json(&case("gb_empty")).unwrap_err(),
        ReadingError::Invalid { pointer: "/readings".to_string(), reason: "count_out_of_range" }
    );
    let b = GaugeBatch::from_json(&case("gb_ok_t_dev")).unwrap();
    assert_eq!((b.first_seq, b.last_seq()), (100, Some(102)));
    assert_eq!(b.summary().unwrap().span_ns, Some(200_000_000));
    let mut huge = b.clone();
    huge.first_seq = u64::MAX - 1;
    assert_eq!(
        huge.validate().unwrap_err(),
        ReadingError::Invalid { pointer: "/last_seq".to_string(), reason: "seq_overflow" }
    );
    let empty = GaugeBatch { first_seq: 0, readings: Vec::new() };
    assert_eq!(empty.to_json().unwrap_err().code(), "reading_invalid");
}

// ------------------------------------------------------------------ AC-5

fn op_classes(desc: &PackageDescription) -> BTreeMap<&str, SideEffect> {
    desc.operations.iter().map(|o| (o.name.as_str(), o.class)).collect()
}

fn check_export(desc: &PackageDescription, json_name: &str, sha_name: &str) {
    let oracle_bytes = read(json_name);
    let oracle = PackageDescription::from_json(&oracle_bytes).unwrap();
    assert_eq!(oracle.canonical_json().unwrap(), desc.canonical_json().unwrap(), "{json_name}");
    let want = String::from_utf8(read(sha_name)).unwrap();
    assert_eq!(desc.digest().unwrap().as_str(), want.trim());
    assert_eq!(validate_and_digest_json(&oracle_bytes).unwrap().as_str(), want.trim());
}

#[test]
fn ac5_gauge_export_validates_and_matches_the_oracle() {
    let desc = GaugePackage::description();
    desc.validate().unwrap();
    validate_package::<GaugePackage>().unwrap();
    // REQ-004: read and zero are D1; the rest is the streaming template; nothing writes
    // calibration.
    let want = BTreeMap::from([
        ("read", SideEffect::D1),
        ("start_stream", SideEffect::D1),
        ("stop_stream", SideEffect::D1),
        ("stream_status", SideEffect::R0),
        ("zero", SideEffect::D1),
    ]);
    assert_eq!(op_classes(&desc), want);
    assert_eq!(
        GaugeOperation::ALL.map(|o| (o.name(), o.class())),
        [("read", SideEffect::D1), ("zero", SideEffect::D1)]
    );
    assert!(desc.operations.iter().all(|o| !o.name.contains("calib")));
    // Gauge-core@1 lists its items, then Stream-common@1.
    assert_eq!(desc.profile.id, "Gauge-core@1");
    let ids: Vec<&str> = desc.profile.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["GA-01", "GA-02", "GA-03", "GA-04", "SC-01", "SC-02", "SC-03", "SC-04"]);
    let titles: Vec<&str> = GAUGE_CORE.items.iter().map(|i| i.title).collect();
    assert_eq!(titles, ["Quantisation", "Unit consistency", "Zero behaviour", "Timestamp monotonicity"]);
    assert_eq!(STREAM_COMMON.items.len(), 4);
    // One data-plane batch schema with declared encodings and a sequence range.
    assert_eq!(desc.batch_schemas.len(), 1);
    let bs = &desc.batch_schemas[0];
    assert!(bs.seq_range && !bs.validity_mask);
    let cols: Vec<&str> = bs.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cols, ["value", "resolution", "t_host", "t_dev"]);
    for key in ["GaugeReading", "GaugeInfo", "Quantity", "StreamStatus", "SessionId"] {
        assert!(desc.defs.contains_key(key), "{key}");
    }
    check_export(&desc, "gauge_v1.json", "gauge_v1.sha256");
}

#[test]
fn ac5_environment_export_validates_and_matches_the_oracle() {
    let desc = EnvironmentPackage::description();
    desc.validate().unwrap();
    validate_package::<EnvironmentPackage>().unwrap();
    // Subscribing is the template's start_stream: no operations of its own.
    let want = BTreeMap::from([
        ("start_stream", SideEffect::D1),
        ("stop_stream", SideEffect::D1),
        ("stream_status", SideEffect::R0),
    ]);
    assert_eq!(op_classes(&desc), want);
    assert_eq!(desc.profile.id, "Environment-core@1");
    let ids: Vec<&str> = desc.profile.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["EN-01", "EN-02", "EN-03", "EN-04", "SC-01", "SC-02", "SC-03", "SC-04"]);
    let titles: Vec<&str> = ENVIRONMENT_CORE.items.iter().map(|i| i.title).collect();
    assert_eq!(titles, ["Units", "Uncertainty presence", "Sampling-time consistency", "Timestamp monotonicity"]);
    let bs = &desc.batch_schemas[0];
    let units: Vec<(&str, SiUnit)> = bs.columns.iter().map(|c| (c.name.as_str(), c.unit)).collect();
    assert_eq!(
        units,
        [
            ("temperature", SiUnit::Kelvin),
            ("temperature_u", SiUnit::Kelvin),
            ("humidity", SiUnit::One),
            ("pressure", SiUnit::Pascal),
            ("sampling_time", SiUnit::Second),
            ("t_host", SiUnit::Second),
            ("t_dev", SiUnit::Second),
        ]
    );
    check_export(&desc, "environment_v1.json", "environment_v1.sha256");
    // The digests of the two packages differ.
    assert_ne!(desc.digest().unwrap(), GaugePackage::description().digest().unwrap());
}

// ------------------------------------------------------------------ settings (REQ-HAL.IDL.SENS-006 at runtime)

fn int_setting(name: &str) -> SettingSchema {
    SettingSchema {
        name: name.to_string(),
        ty: SettingType::Int,
        range: None,
        acquisition_relevant: true,
        live_changeable: false,
    }
}

#[test]
fn undeclared_settings_are_refused() {
    let pkg = GaugePackage::streaming().unwrap();
    gauge::check_device_schema(&pkg, &ConfigSchema::new(Vec::new()).unwrap()).unwrap();
    // An undotted setting: Gauge@1 has no core settings (#612 lesson: fail closed).
    let e = gauge::check_device_schema(&pkg, &ConfigSchema::new(vec![int_setting("gain")]).unwrap()).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
    // A dotted setting under a namespace describe does not list.
    let e =
        gauge::check_device_schema(&pkg, &ConfigSchema::new(vec![int_setting("acme.filter")]).unwrap()).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
    // Listed and declared by the driver: accepted.
    let pkg = GaugePackage::streaming_with(&["acme"], vec![int_setting("acme.filter")]).unwrap();
    gauge::check_device_schema(&pkg, &ConfigSchema::new(vec![int_setting("acme.filter")]).unwrap()).unwrap();
    let e = GaugePackage::streaming_with(&["acme"], vec![int_setting("other.filter")]).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");

    let env = EnvironmentPackage::streaming().unwrap();
    let schema = ConfigSchema::new(core_setting_schemas()).unwrap();
    environment::check_device_schema(&env, &schema).unwrap();
    // A sensor with a fixed sampling time declares nothing.
    environment::check_device_schema(&env, &ConfigSchema::new(Vec::new()).unwrap()).unwrap();
    let mut with_gain = core_setting_schemas();
    with_gain.push(int_setting("gain"));
    let e = environment::check_device_schema(&env, &ConfigSchema::new(with_gain).unwrap()).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");

    // One package's description is not the other's.
    assert_eq!(
        gauge::check_device_schema(&env, &schema).unwrap_err(),
        ReadingError::Settings { setting: String::new(), reason: "not_gauge" }
    );
    assert_eq!(
        environment::check_device_schema(&GaugePackage::streaming().unwrap(), &schema).unwrap_err(),
        ReadingError::Settings { setting: String::new(), reason: "not_environment" }
    );
}

fn env_config(v: f64) -> Configuration {
    let mut values = BTreeMap::new();
    values.insert("sampling_time".to_string(), SettingValue::Quantity(q(v, SiUnit::Second)));
    Configuration { values }
}

#[test]
fn sampling_time_setting_is_typed_and_positive() {
    let pkg = EnvironmentPackage::streaming().unwrap();
    let schema = ConfigSchema::new(core_setting_schemas()).unwrap();
    let set = EnvironmentSettings::from_configuration(&pkg, &schema, &env_config(1.0)).unwrap();
    assert_eq!(set.sampling_time_s, Some(1.0));
    assert_eq!(
        EnvironmentSettings::from_configuration(&pkg, &schema, &env_config(0.0)).unwrap_err(),
        ReadingError::Settings { setting: "sampling_time".to_string(), reason: "not_positive" }
    );
    let none = ConfigSchema::new(Vec::new()).unwrap();
    let set = EnvironmentSettings::from_configuration(&pkg, &none, &Configuration { values: BTreeMap::new() }).unwrap();
    assert_eq!(set.sampling_time_s, None);

    let mut s = core_setting_schemas();
    s[0].ty = SettingType::Quantity(SiUnit::Kelvin);
    assert_eq!(
        environment::check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap_err(),
        ReadingError::Settings { setting: "sampling_time".to_string(), reason: "type_wrong" }
    );
    let mut s = core_setting_schemas();
    s[0].acquisition_relevant = false;
    assert_eq!(
        environment::check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap_err(),
        ReadingError::Settings { setting: "sampling_time".to_string(), reason: "not_digested" }
    );
    // A value the schema does not declare fails the configuration rules.
    let e = EnvironmentSettings::from_configuration(&pkg, &none, &env_config(1.0)).unwrap_err();
    assert_eq!(e.code(), "CONFIG_INVALID");
    let core: BTreeSet<&str> = environment::CORE_SETTINGS.into_iter().collect();
    assert_eq!(core, BTreeSet::from(["sampling_time"]));
}

// ------------------------------------------------------------------ describe info

fn info() -> GaugeInfo {
    GaugeInfo {
        kind: GaugeKind::Caliper,
        min: q(0.0, SiUnit::Metre),
        max: q(0.15, SiUnit::Metre),
        resolution: q(1e-5, SiUnit::Metre),
    }
}

#[test]
fn gauge_info_bounds_readings() {
    let i = info();
    i.validate().unwrap();
    assert_eq!(GaugeInfo::from_value(&i.to_value()).unwrap(), i);
    i.check_reading(&caliper()).unwrap();

    let mut r = caliper();
    r.value = q(0.2, SiUnit::Metre);
    assert_eq!(i.check_reading(&r), Err(ReadingError::OutOfRange { field: "value", reason: "outside_range" }));
    let mut r = caliper();
    r.resolution = q(1e-6, SiUnit::Metre);
    assert_eq!(i.check_reading(&r), Err(ReadingError::Inconsistent { field: "resolution", index: 0 }));
    let mut r = caliper();
    r.value = q(0.0123, SiUnit::Radian);
    r.resolution = q(1e-5, SiUnit::Radian);
    assert_eq!(i.check_reading(&r), Err(ReadingError::Inconsistent { field: "unit", index: 0 }));

    let mut bad = info();
    bad.max = q(0.0, SiUnit::Metre);
    assert_eq!(bad.validate(), Err(ReadingError::OutOfRange { field: "range", reason: "empty" }));
    let mut bad = info();
    bad.min = q(0.0, SiUnit::Radian);
    assert_eq!(bad.validate().unwrap_err().code(), "unit_unsupported");
    let mut v = info().to_value();
    if let CValue::Obj(o) = &mut v {
        o.insert("kind".to_string(), CValue::Str("ruler".to_string()));
    }
    assert_eq!(
        GaugeInfo::from_value(&v),
        Err(ReadingError::Invalid { pointer: "/kind".to_string(), reason: "kind_unknown" })
    );
}

// ------------------------------------------------------------------ Gauge-core@1 evaluators

#[test]
fn gauge_core_items_on_a_stream() {
    let b = GaugeBatch::from_json(&case("gb_ok")).unwrap();
    check_gauge(None, &b.readings).unwrap();
    check_gauge(Some(&info()), &b.readings).unwrap();
    assert_eq!(check_gauge(None, &[]).unwrap_err().reason, "no_readings");

    let mut tight = info();
    tight.max = q(0.012355, SiUnit::Metre);
    let e = check_gauge(Some(&tight), &b.readings).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (gauge::ITEM_UNITS, "value_out_of_range", Some(2)));
    let mut broken = info();
    broken.resolution = q(0.0, SiUnit::Metre);
    assert_eq!(check_gauge(Some(&broken), &b.readings).unwrap_err().reason, "info_invalid");

    let mut r = b.readings.clone();
    r[1].value = q(0.012345, SiUnit::Metre);
    let e = check_gauge(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (gauge::ITEM_QUANTISATION, "reading_not_quantised", Some(1)));

    let mut r = b.readings.clone();
    r[2].resolution = q(1e-6, SiUnit::Metre);
    let e = check_gauge(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (gauge::ITEM_UNITS, "reading_inconsistent", Some(2)));

    let mut r = b.readings.clone();
    r[2].t_host = r[0].t_host - 1;
    let e = check_gauge(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (gauge::ITEM_TIMESTAMPS, "timestamps_not_monotonic", Some(2)));

    let mut r = b.readings.clone();
    r.push(r[2].clone());
    let e = check_gauge(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (gauge::ITEM_TIMESTAMPS, "reading_duplicate", Some(3)));

    let mut r = b.readings.clone();
    r[1].calibration_ref = None;
    let e = check_gauge(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (gauge::ITEM_TIMESTAMPS, "reading_inconsistent", Some(1)));
}

fn zero_step() -> ZeroCheck {
    let before = caliper();
    let mut after = before.clone();
    after.value = q(0.0, SiUnit::Metre);
    after.t_host += 500_000_000;
    ZeroCheck { before, outcome: Ok(()), after }
}

#[test]
fn ga03_zero_behaviour() {
    check_zero(&[zero_step()]).unwrap();
    assert_eq!(check_zero(&[]).unwrap_err().reason, "no_steps");

    let reason = |edit: &dyn Fn(&mut ZeroCheck)| {
        let mut z = zero_step();
        edit(&mut z);
        let e = check_zero(&[z]).unwrap_err();
        (e.item, e.reason)
    };
    assert_eq!(reason(&|z| z.after.value = q(1e-5, SiUnit::Metre)), (gauge::ITEM_ZERO, "not_zero"));
    assert_eq!(reason(&|z| z.outcome = Err(ErrorCode::Limit)), (gauge::ITEM_ZERO, "operation_failed"));
    assert_eq!(reason(&|z| z.after.t_host = z.before.t_host), (gauge::ITEM_ZERO, "not_after"));
    assert_eq!(
        reason(&|z| z.after.calibration_ref = Some(Digest::TOMBSTONE)),
        (gauge::ITEM_ZERO, "calibration_changed")
    );
    assert_eq!(reason(&|z| z.after.calibration_ref = None), (gauge::ITEM_ZERO, "calibration_changed"));
    assert_eq!(reason(&|z| z.after.resolution = q(1e-6, SiUnit::Metre)), (gauge::ITEM_ZERO, "resolution_changed"));
    assert_eq!(
        reason(&|z| z.after.gauge_identity = UntrustedText::new("another invented gauge")),
        (gauge::ITEM_ZERO, "identity_changed")
    );
    assert_eq!(reason(&|z| z.after.t_dev = Some(7)), (gauge::ITEM_ZERO, "t_dev_changed"));
    assert_eq!(
        reason(&|z| {
            z.before.t_dev = Some(9);
            z.after.t_dev = Some(9);
        }),
        (gauge::ITEM_ZERO, "not_after")
    );
    // A reading that fails its own rules reports its own item.
    assert_eq!(
        reason(&|z| z.after.value = q(0.000005, SiUnit::Metre)),
        (gauge::ITEM_QUANTISATION, "reading_not_quantised")
    );
    // Zeroing a gauge that already read zero exercises nothing.
    let mut z = zero_step();
    z.before.value = q(0.0, SiUnit::Metre);
    assert_eq!(check_zero(&[z.clone()]).unwrap_err().reason, "not_exercised");
    check_zero(&[z, zero_step()]).unwrap();
}

// ------------------------------------------------------------------ Environment-core@1 evaluator

#[test]
fn environment_core_items_on_a_stream() {
    let b = EnvBatch::from_json(&case("eb_ok")).unwrap();
    let pkg = EnvironmentPackage::streaming().unwrap();
    let schema = ConfigSchema::new(core_setting_schemas()).unwrap();
    let one_second = EnvironmentSettings::from_configuration(&pkg, &schema, &env_config(1.0)).unwrap();
    check_env(None, &b.readings).unwrap();
    check_env(Some(&one_second), &b.readings).unwrap();
    assert_eq!(check_env(None, &[]).unwrap_err().reason, "no_readings");

    // EN-03: the readings say 1 s, the device is configured for 2 s.
    let two_seconds = EnvironmentSettings::from_configuration(&pkg, &schema, &env_config(2.0)).unwrap();
    let e = check_env(Some(&two_seconds), &b.readings).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (environment::ITEM_SAMPLING, "reading_inconsistent", Some(0)));
    let mut r = b.readings.clone();
    r[2].sampling_time = q(2.0, SiUnit::Second);
    let e = check_env(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (environment::ITEM_SAMPLING, "reading_inconsistent", Some(2)));

    // EN-02: an uncertainty of zero is not a stated uncertainty.
    let mut r = b.readings.clone();
    r[1].temperature_u = q(0.0, SiUnit::Kelvin);
    let e = check_env(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (environment::ITEM_UNCERTAINTY, "value_out_of_range", Some(1)));

    // EN-01: units and the optional quantities.
    let mut r = b.readings.clone();
    r[1].pressure = Some(q(101325.0, SiUnit::Kelvin));
    let e = check_env(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (environment::ITEM_UNITS, "unit_unsupported", Some(1)));
    let mut r = b.readings.clone();
    r[2].pressure = None;
    let e = check_env(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (environment::ITEM_UNITS, "reading_inconsistent", Some(2)));

    // EN-04: timestamps.
    let mut r = b.readings.clone();
    r[2].t_dev = Some(r[1].t_dev.unwrap());
    let e = check_env(None, &r).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (environment::ITEM_TIMESTAMPS, "timestamps_not_monotonic", Some(2)));
}
