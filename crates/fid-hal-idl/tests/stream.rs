//! Acceptance criteria of spec node `HAL.IDL.SENS.STREAM` (AC-1 .. AC-4) and its requirements.
//!
//! Expected package bytes, digests and batch-size tallies come from
//! `fixtures/hal/idl/sens/stream/make_fixtures.py` (an independent stdlib oracle); everything
//! else is hand-labelled from the leaf's rules. The demo package and all data are invented
//! (D-038).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::caps::stream::{
    check_batch_size, check_config_schema, check_extensions_digested, check_sequence_contiguity,
    check_start_stop_idempotence, stream_defs, BatchRecord, BatchSummary, ControlStep, RecordOrigin, StopReason,
    StreamError, StreamEvent, StreamOperation, StreamState, StreamStatus, StreamingDescription, StreamingPackage,
    ITEM_BATCH_SIZE, ITEM_CONTIGUITY, ITEM_EXTENSIONS, ITEM_IDEMPOTENCE, PROFILE_ID, STREAM_COMMON, TEST_STREAM_NS,
};
use fid_hal_idl::{
    config_digest, validate_and_digest_json, BatchColumnDesc, BatchSchemaDesc, CapabilityPackage, ConfigSchema,
    Configuration, ConformanceItemDesc, ConformanceProfileDesc, DevicePackage, EncodingDescriptor, ErrorCode,
    NumericType, PackageDescription, PackageManifest, Quantity, SemanticsNote, SettingSchema, SettingType,
    SettingValue, SiUnit,
};
use fid_types::SideEffect;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fix(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/sens/stream").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fix(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

const DEMO_SEMANTICS: &str = "# DemoStream@1 semantics\n\
\n\
An invented package that exists only to exercise the HAL.IDL.SENS.STREAM template. It \
streams one length column and stands for no real device.\n";

/// The invented demo package's own parts, before the template.
fn demo_base() -> PackageDescription {
    PackageDescription {
        manifest: PackageManifest {
            name: "DemoStream".to_string(),
            version: "1.0.0".to_string(),
            deprecated: false,
            tier3_usable: true,
        },
        defs: BTreeMap::new(),
        operations: Vec::new(),
        batch_schemas: vec![BatchSchemaDesc {
            id: "demo.samples".to_string(),
            columns: vec![BatchColumnDesc {
                name: "value".to_string(),
                unit: SiUnit::Metre,
                encoding: EncodingDescriptor::new(NumericType::I32, 1e-6, 0.0, SiUnit::Metre).unwrap(),
            }],
            validity_mask: true,
            seq_range: true,
        }],
        semantics: SemanticsNote::new(DEMO_SEMANTICS),
        profile: ConformanceProfileDesc {
            id: "DemoStream-core@1".to_string(),
            items: vec![ConformanceItemDesc {
                id: "DS-01".to_string(),
                summary: "Payload encoding: the value column declares its exact encoding.".to_string(),
                fixture_digest: None,
            }],
        },
    }
}

fn int_setting(name: &str) -> SettingSchema {
    SettingSchema {
        name: name.to_string(),
        ty: SettingType::Int,
        range: None,
        acquisition_relevant: true,
        live_changeable: false,
    }
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_built_package_has_the_stream_operations_and_matches_the_oracle() {
    let built = StreamingPackage::new(demo_base()).build().unwrap();
    let desc = built.description();

    // Hand-labelled from REQ-HAL.IDL.SENS.STREAM-001.
    let classes: BTreeMap<&str, SideEffect> = desc.operations.iter().map(|o| (o.name.as_str(), o.class)).collect();
    let want = BTreeMap::from([
        ("start_stream", SideEffect::D1),
        ("stop_stream", SideEffect::D1),
        ("stream_status", SideEffect::R0),
    ]);
    assert_eq!(classes, want);
    for (name, _) in stream_defs() {
        assert!(desc.defs.contains_key(&name), "missing type schema {name}");
    }
    desc.validate().unwrap();

    // The independently written export is the same package, with the same digest.
    let oracle_bytes = read("demo_stream_v1.json");
    let oracle = PackageDescription::from_json(&oracle_bytes).unwrap();
    assert_eq!(oracle.canonical_json().unwrap(), desc.canonical_json().unwrap());
    let want_digest = String::from_utf8(read("demo_stream_v1.sha256")).unwrap();
    assert_eq!(desc.digest().unwrap().as_str(), want_digest.trim());
    assert_eq!(validate_and_digest_json(&oracle_bytes).unwrap().as_str(), want_digest.trim());

    // The parsed export is accepted as a streaming package too.
    StreamingDescription::from_parts(oracle, BTreeSet::new(), BTreeSet::new(), Vec::new()).unwrap();
}

#[test]
fn ac1_operation_descriptions_are_fixed() {
    for op in StreamOperation::ALL {
        let d = op.description();
        assert_eq!(d.name, op.name());
        assert_eq!(d.class, op.class());
        assert!(d.errors.contains(&ErrorCode::Internal), "{}", op.name());
        assert!(d.live_settings.is_empty());
    }
    assert_eq!(StreamOperation::StartStream.class(), SideEffect::D1);
    assert_eq!(StreamOperation::StopStream.class(), SideEffect::D1);
    assert_eq!(StreamOperation::StreamStatus.class(), SideEffect::R0);
}

#[test]
fn a_description_without_the_template_is_refused() {
    // Device@1 has no stream operations.
    let e =
        StreamingDescription::from_parts(DevicePackage::description(), BTreeSet::new(), BTreeSet::new(), Vec::new())
            .unwrap_err();
    assert_eq!(e.code(), "stream_template_missing");

    // A built package whose stream_status was turned into a D1 operation.
    let mut desc = StreamingPackage::new(demo_base()).build().unwrap().description().clone();
    let op = desc.operations.iter_mut().find(|o| o.name == "stream_status").unwrap();
    op.class = SideEffect::D1;
    let e = StreamingDescription::from_parts(desc, BTreeSet::new(), BTreeSet::new(), Vec::new()).unwrap_err();
    assert_eq!(e, StreamError::Conflict { what: "operation", name: "stream_status".to_string() });

    // A built package without the SC-03 profile item.
    let mut desc = StreamingPackage::new(demo_base()).build().unwrap().description().clone();
    desc.profile.items.retain(|i| i.id != "SC-03");
    let e = StreamingDescription::from_parts(desc, BTreeSet::new(), BTreeSet::new(), Vec::new()).unwrap_err();
    assert_eq!(e, StreamError::Missing { what: "profile item", name: "SC-03".to_string() });
}

#[test]
fn the_builder_refuses_clashing_parts() {
    let mut base = demo_base();
    base.operations.push(StreamOperation::StartStream.description());
    let e = StreamingPackage::new(base).build().unwrap_err();
    assert_eq!(e, StreamError::Conflict { what: "operation", name: "start_stream".to_string() });

    let mut base = demo_base();
    base.defs.insert(
        "StreamState".to_string(),
        CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("string".to_string()))])),
    );
    let e = StreamingPackage::new(base).build().unwrap_err();
    assert_eq!(e.code(), "stream_conflict");

    // An identical SessionId (as in Device@1) is kept, not a clash.
    let mut base = demo_base();
    base.defs.insert("SessionId".to_string(), stream_defs().remove("SessionId").unwrap());
    StreamingPackage::new(base).build().unwrap();

    let mut base = demo_base();
    base.profile.items.push(ConformanceItemDesc {
        id: "SC-01".to_string(),
        summary: "A package item that reuses a Stream-common ID.".to_string(),
        fixture_digest: None,
    });
    assert_eq!(StreamingPackage::new(base).build().unwrap_err().code(), "stream_conflict");

    let e = StreamingPackage::new(demo_base()).vendor_namespace("acme").vendor_namespace("acme").build().unwrap_err();
    assert_eq!(e.code(), "stream_conflict");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_extension_without_namespace_is_extension_undeclared() {
    let e = StreamingPackage::new(demo_base())
        .vendor_namespace("acme")
        .extension(int_setting("secret_mode"))
        .build()
        .unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "secret_mode".to_string() });
}

#[test]
fn extensions_must_use_a_listed_namespace() {
    // Declared under its listed namespace: accepted, and kept next to the description.
    let built = StreamingPackage::new(demo_base())
        .vendor_namespace("acme")
        .extension(int_setting("acme.gain"))
        .build()
        .unwrap();
    assert_eq!(built.vendor_namespaces(), &names(&["acme"]));
    assert_eq!(built.extensions().len(), 1);

    // A namespace that is not listed.
    let e = StreamingPackage::new(demo_base())
        .vendor_namespace("acme")
        .extension(int_setting("other.gain"))
        .build()
        .unwrap_err();
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "other.gain".to_string() });

    // No namespace listed at all.
    let e = StreamingPackage::new(demo_base()).extension(int_setting("acme.gain")).build().unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");

    // An extension that is not digested breaks the HAL.IDL.BASE.CFG rule.
    let mut quiet = int_setting("acme.gain");
    quiet.acquisition_relevant = false;
    let e = StreamingPackage::new(demo_base()).vendor_namespace("acme").extension(quiet).build().unwrap_err();
    assert_eq!(e.code(), "CONFIG_INVALID");

    // A malformed namespace.
    for bad in ["Acme", "ac.me", "", "9acme"] {
        let e = StreamingPackage::new(demo_base()).vendor_namespace(bad).build().unwrap_err();
        assert_eq!(e.code(), "namespace_invalid", "{bad:?}");
    }
}

#[test]
fn dotted_live_settings_must_be_declared_extensions() {
    let mut base = demo_base();
    let mut op = StreamOperation::StartStream.description();
    op.name = "set_gain".to_string();
    op.live_settings = vec!["acme.gain".to_string()];
    base.operations.push(op);

    let e = StreamingPackage::new(base.clone()).vendor_namespace("acme").build().unwrap_err();
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "acme.gain".to_string() });

    StreamingPackage::new(base).vendor_namespace("acme").extension(int_setting("acme.gain")).build().unwrap();
}

#[test]
fn device_config_schemas_follow_the_rule() {
    let core = names(&["exposure_count"]);
    let schema = ConfigSchema::new(vec![int_setting("exposure_count"), int_setting("acme.gain")]).unwrap();
    check_config_schema(&names(&["acme"]), &core, &schema).unwrap();
    let e = check_config_schema(&names(&["other"]), &core, &schema).unwrap_err();
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "acme.gain".to_string() });

    // A core name the package does not declare is refused too (fail closed).
    let e = check_config_schema(&names(&["acme"]), &BTreeSet::new(), &schema).unwrap_err();
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "exposure_count".to_string() });

    // Through a built package.
    let built = StreamingPackage::new(demo_base())
        .vendor_namespace("acme")
        .core_setting("exposure_count")
        .extension(int_setting("acme.gain"))
        .build()
        .unwrap();
    built.check_config_schema(&schema).unwrap();

    // Malformed or duplicate core names.
    for bad in ["Exposure", "acme.gain", "", "9x"] {
        let e = StreamingPackage::new(demo_base()).core_setting(bad).build().unwrap_err();
        assert_eq!(e.code(), "setting_invalid", "{bad:?}");
    }
    let e = StreamingPackage::new(demo_base()).core_setting("x").core_setting("x").build().unwrap_err();
    assert_eq!(e.code(), "stream_conflict");
}

#[test]
fn ac2_runtime_form_secret_mode_is_extension_undeclared() {
    // A driver's config-schema/0 holding `secret_mode`: neither a core setting of the package
    // nor an extension under a listed namespace.
    let schema = ConfigSchema::new(vec![int_setting("exposure_count"), int_setting("secret_mode")]).unwrap();
    let built =
        StreamingPackage::new(demo_base()).vendor_namespace("acme").core_setting("exposure_count").build().unwrap();
    let e = built.check_config_schema(&schema).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "secret_mode".to_string() });
}

#[test]
fn undotted_live_settings_must_be_core_settings() {
    let mut base = demo_base();
    let mut op = StreamOperation::StartStream.description();
    op.name = "set_exposure".to_string();
    op.live_settings = vec!["exposure".to_string()];
    base.operations.push(op);
    let e = StreamingPackage::new(base.clone()).build().unwrap_err();
    assert_eq!(e, StreamError::ExtensionUndeclared { setting: "exposure".to_string() });
    StreamingPackage::new(base).core_setting("exposure").build().unwrap();
}

// ------------------------------------------------------------------ AC-3

fn expected() -> CValue {
    fid_canon::parse(&read("expected.json")).unwrap()
}

fn expected_field<'a>(v: &'a CValue, file: &str, key: &str) -> &'a CValue {
    let CValue::Obj(top) = v else { panic!("expected.json is not an object") };
    let CValue::Obj(entry) = &top[file] else { panic!("{file} entry is not an object") };
    &entry[key]
}

fn expected_count(file: &str, key: &str) -> usize {
    match expected_field(&expected(), file, key) {
        CValue::Num(n) => *n as usize,
        other => panic!("{file}/{key}: {other:?}"),
    }
}

fn expected_string(file: &str, key: &str) -> String {
    match expected_field(&expected(), file, key) {
        CValue::Str(s) => s.clone(),
        other => panic!("{file}/{key}: {other:?}"),
    }
}

fn expected_bool(file: &str, key: &str) -> bool {
    match expected_field(&expected(), file, key) {
        CValue::Bool(b) => *b,
        other => panic!("{file}/{key}: {other:?}"),
    }
}

#[test]
fn ac3_batch_size_item_passes_at_97_percent_and_fails_at_80() {
    let ok = BatchRecord::from_json(&read("sizes_ok.json")).unwrap();
    assert_eq!(ok.origin(), RecordOrigin::Synthetic);
    assert!(expected_bool("sizes_ok.json", "pass"));
    let tally = check_batch_size(&ok).unwrap();
    assert_eq!(tally.total(), expected_count("sizes_ok.json", "total"));
    assert_eq!(tally.within(), expected_count("sizes_ok.json", "within"));
    assert_eq!(tally.origin(), RecordOrigin::Synthetic);
    assert_eq!(tally.span_total_ns().map(|n| n.to_string()), Some(expected_string("sizes_ok.json", "span_total_ns")));

    let bad = BatchRecord::from_json(&read("sizes_bad.json")).unwrap();
    assert!(!expected_bool("sizes_bad.json", "pass"));
    let e = check_batch_size(&bad).unwrap_err();
    assert_eq!((e.item, e.reason), (ITEM_BATCH_SIZE, "below_threshold"));
    let within = bad.batches().iter().filter(|b| b.within_recommendation()).count();
    assert_eq!(within, expected_count("sizes_bad.json", "within"));
}

#[test]
fn ac3_records_round_trip_canonically() {
    for name in ["sizes_ok.json", "sizes_bad.json"] {
        let bytes = read(name);
        let rec = BatchRecord::from_json(&bytes).unwrap();
        let ours = fid_canon::to_canonical_bytes(&rec.to_value()).unwrap();
        assert_eq!(ours, fid_canon::canonicalize(&bytes).unwrap(), "{name}");
    }
}

#[test]
fn batch_size_boundaries_and_fail_closed_cases() {
    let b = |bytes, span_ns| BatchSummary { first_seq: 0, last_seq: 0, bytes, span_ns };
    // Hand-labelled from REQ-003: 1-8 MB inclusive (10^6 bytes) or at most 50 ms.
    assert!(b(1_000_000, None).within_recommendation());
    assert!(b(8_000_000, None).within_recommendation());
    assert!(!b(999_999, None).within_recommendation());
    assert!(!b(8_000_001, None).within_recommendation());
    assert!(b(1, Some(50_000_000)).within_recommendation());
    assert!(b(u64::MAX, Some(0)).within_recommendation());
    assert!(!b(1, Some(50_000_001)).within_recommendation());

    let record = |n_in: usize, n_out: usize, duration_ns| {
        let batches = [vec![b(2_000_000, None); n_in], vec![b(10, None); n_out]].concat();
        BatchRecord::new(RecordOrigin::Synthetic, duration_ns, batches).unwrap()
    };
    // Exactly 95 % passes; just under fails.
    assert!(check_batch_size(&record(95, 5, TEST_STREAM_NS)).is_ok());
    assert_eq!(check_batch_size(&record(189, 11, TEST_STREAM_NS)).unwrap_err().reason, "below_threshold");
    // Too short, and empty.
    assert_eq!(check_batch_size(&record(100, 0, TEST_STREAM_NS - 1)).unwrap_err().reason, "stream_too_short");
    assert_eq!(check_batch_size(&record(0, 0, TEST_STREAM_NS)).unwrap_err().reason, "no_batches");

    // Where spans are present, they must all be present and sum to at least 60 s.
    let spanned = |span: u64, n: usize| vec![b(2_000_000, Some(span)); n];
    let rec = |batches| BatchRecord::new(RecordOrigin::Simulated, TEST_STREAM_NS, batches).unwrap();
    let full = check_batch_size(&rec(spanned(600_000_000, 100))).unwrap();
    assert_eq!(full.span_total_ns(), Some(TEST_STREAM_NS));
    assert_eq!(full.origin(), RecordOrigin::Simulated);
    let short = check_batch_size(&rec(spanned(600_000_000, 99))).unwrap_err();
    assert_eq!(short.reason, "stream_too_short");
    let mut mixed = spanned(600_000_000, 100);
    mixed.push(b(2_000_000, None));
    assert_eq!(check_batch_size(&rec(mixed)).unwrap_err().reason, "spans_partial");
    assert_eq!(check_batch_size(&record(100, 0, TEST_STREAM_NS)).unwrap().span_total_ns(), None);
}

#[test]
fn origin_cannot_be_forged_as_device() {
    // D-045: editing "synthetic" to "device" in a record file is refused.
    let edited = String::from_utf8(read("sizes_ok.json")).unwrap().replace("\"synthetic\"", "\"device\"");
    assert!(edited.contains("\"origin\": \"device\""));
    assert_eq!(
        BatchRecord::from_json(edited.as_bytes()).unwrap_err(),
        StreamError::Record { pointer: "/origin".to_string(), reason: "origin_device_refused" }
    );
    // The constructor refuses it as well, and simulated records are accepted.
    let e = BatchRecord::new(RecordOrigin::Device, TEST_STREAM_NS, Vec::new()).unwrap_err();
    assert_eq!(e.code(), "stream_record_invalid");
    let sim = String::from_utf8(read("sizes_ok.json")).unwrap().replace("\"synthetic\"", "\"simulated\"");
    assert_eq!(BatchRecord::from_json(sim.as_bytes()).unwrap().origin(), RecordOrigin::Simulated);
}

#[test]
fn batch_records_are_read_strictly() {
    let ok = String::from_utf8(read("sizes_ok.json")).unwrap();
    let cases = [
        (ok.replace("stream-batches/0", "stream-batches/1"), "/schema"),
        (ok.replace("\"synthetic\"", "\"measured\""), "/origin"),
        (ok.replacen("\"0\"", "\"00\"", 1), "/batches/0/first_seq"),
        (ok.replacen("\"bytes\": \"1000000\"", "\"bytes\": 1000000", 1), "/batches/0/bytes"),
        (ok.replacen("\"span_ns\"", "\"span_ms\"", 1), "/batches/0/span_ms"),
    ];
    for (input, pointer) in cases {
        match BatchRecord::from_json(input.as_bytes()) {
            Err(StreamError::Record { pointer: p, .. }) => assert_eq!(p, pointer),
            other => panic!("{pointer}: {other:?}"),
        }
    }
    assert_eq!(BatchRecord::from_json(b"{").unwrap_err().code(), "stream_record_invalid");
}

// ------------------------------------------------------------------ SC-03 contiguity

#[test]
fn sequence_contiguity() {
    for name in ["sizes_ok.json", "sizes_bad.json"] {
        let rec = BatchRecord::from_json(&read(name)).unwrap();
        assert_eq!(check_sequence_contiguity(rec.batches()).is_ok(), expected_bool(name, "contiguous"), "{name}");
    }
    let r = |first_seq, last_seq| BatchSummary { first_seq, last_seq, bytes: 2_000_000, span_ns: None };
    let fail = |batches: &[BatchSummary]| {
        let e = check_sequence_contiguity(batches).unwrap_err();
        assert_eq!(e.item, ITEM_CONTIGUITY);
        (e.reason, e.index)
    };
    // Hand-labelled.
    assert_eq!(fail(&[r(0, 99), r(100, 199), r(300, 399)]), ("gap", Some(2)));
    assert_eq!(fail(&[r(0, 99), r(99, 199)]), ("overlap", Some(1)));
    assert_eq!(fail(&[r(0, 99), r(50, 60)]), ("overlap", Some(1)));
    assert_eq!(fail(&[r(10, 9)]), ("range_reversed", Some(0)));
    assert_eq!(fail(&[r(0, u64::MAX), r(0, 0)]), ("overlap", Some(1)));
    assert_eq!(fail(&[]), ("no_batches", None));
    check_sequence_contiguity(&[r(7, 7), r(8, 100)]).unwrap();
}

// ------------------------------------------------------------------ SC-02 idempotence

fn step(op: StreamOperation, state: StreamState, next_seq: u64, events: Vec<StreamEvent>) -> ControlStep {
    ControlStep { op, outcome: Ok(StreamStatus { state, next_seq }), events }
}

fn good_trace() -> Vec<ControlStep> {
    use StreamOperation::{StartStream, StopStream, StreamStatus as Status};
    use StreamState::{Idle, Streaming};
    vec![
        step(StartStream, Streaming, 0, vec![StreamEvent::Started]),
        step(StartStream, Streaming, 300, vec![]),
        step(Status, Streaming, 400, vec![StreamEvent::Gap { from: 310, to: 319 }]),
        step(StopStream, Idle, 500, vec![StreamEvent::Stopped { reason: StopReason::Requested }]),
        step(StopStream, Idle, 500, vec![]),
        step(Status, Idle, 500, vec![]),
    ]
}

fn idem_fail(trace: &[ControlStep]) -> (&'static str, Option<usize>) {
    let e = check_start_stop_idempotence(trace).unwrap_err();
    assert_eq!(e.item, ITEM_IDEMPOTENCE);
    (e.reason, e.index)
}

#[test]
fn start_stop_idempotence() {
    check_start_stop_idempotence(&good_trace()).unwrap();

    let mut t = good_trace();
    t[1].events.push(StreamEvent::Started);
    assert_eq!(idem_fail(&t), ("not_idempotent", Some(1)));

    let mut t = good_trace();
    t[4].outcome = Ok(StreamStatus { state: StreamState::Idle, next_seq: 600 });
    assert_eq!(idem_fail(&t), ("not_idempotent", Some(4)));

    let mut t = good_trace();
    t[4].events.push(StreamEvent::Stopped { reason: StopReason::Requested });
    assert_eq!(idem_fail(&t), ("not_idempotent", Some(4)));

    let mut t = good_trace();
    t[4].outcome = Err(ErrorCode::Internal);
    assert_eq!(idem_fail(&t), ("operation_failed", Some(4)));

    let mut t = good_trace();
    t[0].events.clear();
    assert_eq!(idem_fail(&t), ("events_wrong", Some(0)));

    let mut t = good_trace();
    t[3].events = vec![StreamEvent::Stopped { reason: StopReason::Limit }];
    assert_eq!(idem_fail(&t), ("events_wrong", Some(3)));

    let mut t = good_trace();
    t[1].outcome = Ok(StreamStatus { state: StreamState::Idle, next_seq: 300 });
    assert_eq!(idem_fail(&t), ("state_wrong", Some(1)));

    let mut t = good_trace();
    t[5].outcome = Ok(StreamStatus { state: StreamState::Idle, next_seq: 501 });
    assert_eq!(idem_fail(&t), ("not_idempotent", Some(5)));

    // A trace that never repeats a stop does not exercise the rule.
    let mut t = good_trace();
    t.truncate(4);
    assert_eq!(idem_fail(&t), ("not_exercised", None));
    assert_eq!(idem_fail(&[]), ("not_exercised", None));
}

// ------------------------------------------------------------------ SC-04 extensions

fn ext_schema() -> ConfigSchema {
    let exposure = SettingSchema {
        name: "exposure".to_string(),
        ty: SettingType::Quantity(SiUnit::Second),
        range: None,
        acquisition_relevant: true,
        live_changeable: true,
    };
    let label = SettingSchema {
        name: "label".to_string(),
        ty: SettingType::Text,
        range: None,
        acquisition_relevant: false,
        live_changeable: true,
    };
    ConfigSchema::new(vec![exposure, label, int_setting("acme.gain")]).unwrap()
}

fn ext_config(gain: i64) -> Configuration {
    Configuration {
        values: BTreeMap::from([
            ("exposure".to_string(), SettingValue::Quantity(Quantity::new(0.002, SiUnit::Second).unwrap())),
            ("label".to_string(), SettingValue::Text("demo".to_string())),
            ("acme.gain".to_string(), SettingValue::Int(gain)),
        ]),
    }
}

#[test]
fn extension_values_are_in_the_reported_digest() {
    let schema = ext_schema();
    let acme = names(&["acme"]);
    let reported = config_digest(&schema, &ext_config(4)).unwrap();
    let core = names(&["exposure", "label"]);
    check_extensions_digested(&acme, &core, &schema, &ext_config(4), &reported).unwrap();

    let fail = |ns: &BTreeSet<String>, cfg: &Configuration| {
        let e = check_extensions_digested(ns, &core, &schema, cfg, &reported).unwrap_err();
        assert_eq!(e.item, ITEM_EXTENSIONS);
        e.reason
    };
    // A device that left the extension out of its digest reports the digest of another value.
    assert_eq!(fail(&acme, &ext_config(5)), "digest_mismatch");
    assert_eq!(fail(&names(&["other"]), &ext_config(4)), "extension_undeclared");
    let mut missing = ext_config(4);
    missing.values.remove("acme.gain");
    assert_eq!(fail(&acme, &missing), "config_invalid");

    // An undotted setting outside the package's core names is an undeclared extension.
    let e = check_extensions_digested(&acme, &names(&["exposure"]), &schema, &ext_config(4), &reported).unwrap_err();
    assert_eq!((e.item, e.reason), (ITEM_EXTENSIONS, "extension_undeclared"));
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_stream_common_lists_four_items_with_stable_ids() {
    assert_eq!(STREAM_COMMON.id, PROFILE_ID);
    assert_eq!(PROFILE_ID, "Stream-common@1");
    let ids: Vec<&str> = STREAM_COMMON.items.iter().map(|i| i.id).collect();
    // Hand-labelled: SC-01 batch size (REQ-003); SC-02 idempotence, SC-03 contiguity,
    // SC-04 extensions (REQ-004).
    assert_eq!(ids, ["SC-01", "SC-02", "SC-03", "SC-04"]);
    assert_eq!(
        [ITEM_BATCH_SIZE, ITEM_IDEMPOTENCE, ITEM_CONTIGUITY, ITEM_EXTENSIONS],
        ["SC-01", "SC-02", "SC-03", "SC-04"]
    );
    let titles: Vec<&str> = STREAM_COMMON.items.iter().map(|i| i.title).collect();
    assert_eq!(
        titles,
        [
            "Batch size within the recommendation",
            "Start and stop idempotence",
            "Sequence contiguity",
            "Extension declaration and digesting",
        ]
    );

    // Every built package carries the four items (its profile then checks REQ-007).
    let built = StreamingPackage::new(demo_base()).build().unwrap();
    let profile_ids: BTreeSet<&str> = built.description().profile.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(profile_ids, BTreeSet::from(["DS-01", "SC-01", "SC-02", "SC-03", "SC-04"]));
}

// ------------------------------------------------------------------ wire forms

#[test]
fn events_and_status_round_trip_and_are_read_strictly() {
    let events = [
        StreamEvent::Started,
        StreamEvent::Stopped { reason: StopReason::EndOfData },
        StreamEvent::Gap { from: 5, to: 9 },
    ];
    for ev in events {
        assert_eq!(StreamEvent::from_value(&ev.to_value()).unwrap(), ev);
    }
    let gap = fid_canon::to_canonical_bytes(&StreamEvent::Gap { from: 5, to: 9 }.to_value()).unwrap();
    assert_eq!(gap, br#"{"from":"5","kind":"gap","to":"9"}"#);

    let parse = |s: &str| StreamEvent::from_value(&fid_canon::parse(s.as_bytes()).unwrap());
    for bad in [
        r#"{"kind":"gap","from":"9","to":"5"}"#,
        r#"{"kind":"started","reason":"requested"}"#,
        r#"{"kind":"stopped"}"#,
        r#"{"kind":"stopped","reason":"bored"}"#,
        r#"{"kind":"paused"}"#,
        r#"{"kind":"gap","from":"1"}"#,
        r#"["started"]"#,
    ] {
        assert_eq!(parse(bad).unwrap_err().code(), "stream_record_invalid", "{bad}");
    }

    let st = StreamStatus { state: StreamState::Streaming, next_seq: u64::MAX };
    assert_eq!(StreamStatus::from_value(&st.to_value()).unwrap(), st);
    let bytes = fid_canon::to_canonical_bytes(&st.to_value()).unwrap();
    assert_eq!(bytes, br#"{"next_seq":"18446744073709551615","state":"streaming"}"#);
    let bad = fid_canon::parse(br#"{"next_seq":"1","state":"paused"}"#).unwrap();
    assert_eq!(StreamStatus::from_value(&bad).unwrap_err().code(), "stream_record_invalid");
}
