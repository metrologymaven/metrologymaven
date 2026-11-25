//! Acceptance criteria of spec node `HAL.IDL.SENS.LINE` (AC-1 .. AC-5) and its requirements.
//!
//! Expected sizes, digests, decoded probes, case outcomes and the package export come from
//! `fixtures/hal/idl/sens/line/make_fixtures.py` (an independent stdlib oracle that also
//! checks every hand-labelled case outcome); the settings and trigger tests are hand-labelled
//! from the leaf's rules. All device data is invented (D-038).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::caps::linescan::{
    check_batch, check_device_schema, check_trigger_mode, core_setting_schemas, ProfileBatch, CORE_SETTINGS,
    ITEM_ENCODING, ITEM_LENGTHS, ITEM_MASK, ITEM_TIMESTAMPS, ITEM_TRIGGER, MAX_HEADER_BYTES, PROFILE_ID,
    SIZE_BUDGET_BYTES, TRIGGER_TOLERANCE_PERCENT,
};
use fid_hal_idl::caps::stream::{
    check_batch_size, check_sequence_contiguity, BatchRecord, BatchSummary, RecordOrigin, StreamingDescription,
    STREAM_COMMON,
};
use fid_hal_idl::{
    validate_and_digest_json, validate_package, CapabilityPackage, ConfigSchema, Configuration, EncodingDescriptor,
    LineScanError, LineScanPackage, LineScanSettings, NumericType, PackageDescription, Quantity, RawValue,
    SettingRange, SettingSchema, SettingType, SettingValue, SiUnit, Trigger, TriggerMode, ValidityMask, LINESCAN_CORE,
};
use fid_types::SideEffect;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fix(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/sens/line").join(name)
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

fn big() -> (Vec<u8>, Vec<u8>) {
    (read("profiles_100x2000.header.json"), read("profiles_100x2000.bin"))
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

fn f64_bits(text: &str) -> u64 {
    u64::from_str_radix(text.strip_prefix("0x").unwrap(), 16).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_seeded_batch_passes_within_the_size_budget() {
    let (header, payload) = big();
    let expected = json("expected.json");
    let summary = check_batch(&header, &payload).unwrap();

    let total = (header.len() + payload.len()) as u64;
    assert_eq!(total, n(get(&expected, "total_bytes")));
    assert_eq!(header.len() as u64, n(get(&expected, "header_bytes")));
    assert!(total <= SIZE_BUDGET_BYTES, "{total} > {SIZE_BUDGET_BYTES}");
    assert_eq!(SIZE_BUDGET_BYTES, n(get(&expected, "budget_bytes")));
    // REQ-005: the coordinates are 1.6 MB; mask, timestamps, IDs and header the rest.
    assert_eq!(2 * 100 * 2000 * 4, 1_600_000);
    assert_eq!(summary, want_summary(get(&expected, "summary")));
    assert_eq!(
        fid_types::Digest::from_bytes(fid_canon::digest_bytes(&payload).0).as_str(),
        s(get(&expected, "payload_sha256"))
    );

    let batch = ProfileBatch::decode(&header, &payload).unwrap();
    assert_eq!(batch.profiles, 100);
    assert_eq!(batch.points_per_profile, 2000);
    assert!(batch.intensity.is_none() && batch.pose.is_none());
    assert_eq!(batch.x.descriptor.numeric(), NumericType::I32);
    assert_eq!(batch.mask.count_valid(), n(get(&expected, "valid_samples")));

    // Decoded values agree bit for bit with the oracle's offset + scale x raw.
    let CValue::Arr(probes) = get(&expected, "probes") else { panic!("probes") };
    for p in probes {
        let i = n(get(p, "index"));
        let valid = matches!(get(p, "valid"), CValue::Bool(true));
        assert_eq!(batch.mask.is_valid(i), valid, "sample {i}");
        if valid {
            for (name, a) in [("x", &batch.x), ("z", &batch.z)] {
                let q = a.descriptor.decode(a.raw(i as usize).unwrap()).unwrap();
                assert_eq!(q.unit(), SiUnit::Metre);
                assert_eq!(q.value().to_bits(), f64_bits(s(get(p, name))), "{name}[{i}]");
            }
        }
    }

    // The wire form round-trips byte for byte.
    let again = batch.encode().unwrap();
    assert_eq!(again.header, header);
    assert_eq!(again.payload, payload);
    assert_eq!(again.size(), total);
    assert_eq!(batch.summary().unwrap(), summary);
}

#[test]
fn ac1_summaries_feed_the_stream_common_items() {
    let (header, payload) = big();
    let one = check_batch(&header, &payload).unwrap();
    // 1.6 MB per batch is within the 1 to 8 MB recommendation.
    assert!(one.within_recommendation());
    // Sixty seconds of back-to-back batches like this one pass SC-01 and SC-03.
    let width = one.last_seq - one.first_seq + 1;
    let batches: Vec<BatchSummary> = (0..6061)
        .map(|k| BatchSummary {
            first_seq: one.first_seq + k * width,
            last_seq: one.last_seq + k * width,
            bytes: one.bytes,
            span_ns: one.span_ns,
        })
        .collect();
    check_sequence_contiguity(&batches).unwrap();
    let record = BatchRecord::new(RecordOrigin::Synthetic, 60_000_000_000, batches.clone()).unwrap();
    let tally = check_batch_size(&record).unwrap();
    assert_eq!(tally.origin(), RecordOrigin::Synthetic);
    // D-045: summaries of LineScan batches cannot be relabelled as device data.
    let e = BatchRecord::new(RecordOrigin::Device, 60_000_000_000, batches).unwrap_err();
    assert_eq!(e.code(), "stream_record_invalid");
}

// ------------------------------------------------------------------ AC-2 .. AC-4 and the case table

#[test]
fn every_case_has_the_oracle_outcome() {
    let CValue::Arr(cases) = json("cases.json") else { panic!("cases") };
    assert!(cases.len() >= 30);
    for c in &cases {
        let name = s(get(c, "name"));
        let header = s(get(c, "header")).as_bytes().to_vec();
        let payload = hex(s(get(c, "payload_hex")));
        let want = s(get(c, "expect"));
        match check_batch(&header, &payload) {
            Ok(summary) => {
                assert_eq!(want, "ok", "{name}: passed, oracle says {want}");
                assert_eq!(summary, want_summary(get(c, "summary")), "{name}");
                // A passing batch re-encodes to the oracle's bytes.
                let enc = ProfileBatch::decode(&header, &payload).unwrap().encode().unwrap();
                assert_eq!(enc.header, header, "{name}");
                assert_eq!(enc.payload, payload, "{name}");
            }
            Err(e) => assert_eq!(e.code(), want, "{name}: {e}"),
        }
    }
}

fn hex(t: &str) -> Vec<u8> {
    (0..t.len()).step_by(2).map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap()).collect()
}

fn case(name: &str) -> (Vec<u8>, Vec<u8>) {
    let CValue::Arr(cases) = json("cases.json") else { panic!("cases") };
    let c = cases.iter().find(|c| s(get(c, "name")) == name).unwrap_or_else(|| panic!("no case {name}"));
    (s(get(c, "header")).as_bytes().to_vec(), hex(s(get(c, "payload_hex"))))
}

#[test]
fn ac2_z_one_point_short_is_array_length_mismatch() {
    let (h, p) = case("z_one_point_short");
    let e = ProfileBatch::decode(&h, &p).unwrap_err();
    assert_eq!(e, LineScanError::ArrayLength { array: "z", expected: 12, found: 11 });
    assert_eq!(e.code(), "array_length_mismatch");
    assert_eq!(e.item(), Some(ITEM_LENGTHS));

    // The same fault in memory: the z bytes one element short.
    let mut b = ProfileBatch::decode(&case("ok_base").0, &case("ok_base").1).unwrap();
    b.z.bytes.truncate(b.z.bytes.len() - 4);
    assert_eq!(b.validate().unwrap_err(), LineScanError::ArrayLength { array: "z", expected: 12, found: 11 });
    assert_eq!(b.encode().unwrap_err().code(), "array_length_mismatch");

    // On the seeded batch too.
    let mut big = ProfileBatch::decode(&big().0, &big().1).unwrap();
    big.z.bytes.truncate(big.z.bytes.len() - 4);
    assert_eq!(big.validate().unwrap_err().code(), "array_length_mismatch");
}

#[test]
fn ac3_x_in_radians_is_encoding_invalid() {
    let (h, p) = case("x_unit_rad");
    let e = ProfileBatch::decode(&h, &p).unwrap_err();
    assert_eq!(e.code(), "encoding_invalid");
    assert_eq!(e.item(), Some(ITEM_ENCODING));
    assert!(matches!(e, LineScanError::Encoding { array: "x", .. }), "{e:?}");

    let mut b = ProfileBatch::decode(&big().0, &big().1).unwrap();
    b.x.descriptor = EncodingDescriptor::new(NumericType::I32, 1e-7, 0.0, SiUnit::Radian).unwrap();
    assert_eq!(b.validate().unwrap_err().code(), "encoding_invalid");
}

#[test]
fn ac4_decreasing_t_host_is_not_monotonic() {
    let (h, p) = case("t_host_decreasing");
    let e = ProfileBatch::decode(&h, &p).unwrap_err();
    assert_eq!(e, LineScanError::NotMonotonic { array: "t_host", index: 2 });
    assert_eq!(e.code(), "timestamps_not_monotonic");
    assert_eq!(e.item(), Some(ITEM_TIMESTAMPS));

    let mut b = ProfileBatch::decode(&big().0, &big().1).unwrap();
    b.t_host[50] = b.t_host[49] - 1;
    assert_eq!(b.validate().unwrap_err(), LineScanError::NotMonotonic { array: "t_host", index: 50 });
    // Equal host times (profiles received together) are allowed.
    let mut b = ProfileBatch::decode(&big().0, &big().1).unwrap();
    b.t_host[50] = b.t_host[49];
    b.validate().unwrap();
}

#[test]
fn masked_samples_are_never_decoded() {
    let (h, p) = case("ok_masked_nan");
    let mut b = ProfileBatch::decode(&h, &p).unwrap();
    assert!(!b.mask.is_valid(5));
    assert!(matches!(b.x.raw(5), Some(RawValue::F32(v)) if v.is_nan()));
    b.mask.set(5, true);
    let e = b.validate().unwrap_err();
    assert_eq!(e, LineScanError::SampleNotFinite { array: "x", index: 5 });
    assert_eq!(e.item(), Some(ITEM_MASK));
    let (h, p) = case("mask_padding_set");
    assert_eq!(ProfileBatch::decode(&h, &p).unwrap_err().item(), Some(ITEM_MASK));
}

#[test]
fn headers_are_bounded_and_carry_no_origin() {
    // D-045: a header cannot say where its data came from.
    let (h, p) = case("origin_device");
    let e = ProfileBatch::decode(&h, &p).unwrap_err();
    assert_eq!(e, LineScanError::Header { pointer: "/origin".to_string(), reason: "field_unknown" });

    let (_, p) = case("ok_base");
    let mut huge = b"{\"schema\":\"".to_vec();
    huge.resize(MAX_HEADER_BYTES + 1, b'a');
    assert_eq!(ProfileBatch::decode(&huge, &p).unwrap_err().code(), "batch_header_invalid");

    // A header that claims more samples than the limit is refused before any allocation.
    let (h, p) = case("ok_base");
    let text =
        String::from_utf8(h).unwrap().replace("\"points_per_profile\":\"4\"", "\"points_per_profile\":\"65537\"");
    assert_eq!(
        ProfileBatch::decode(text.as_bytes(), &p).unwrap_err(),
        LineScanError::Header { pointer: "/points_per_profile".to_string(), reason: "out_of_range" }
    );
}

#[test]
fn pose_references_are_per_profile() {
    let (h, p) = case("ok_full");
    let b = ProfileBatch::decode(&h, &p).unwrap();
    let r = b.pose_ref(1).unwrap();
    assert_eq!((r.stream_id.as_str(), r.seq), ("tracker-1.pose", 51));
    assert!(b.pose_ref(3).is_none());
    assert_eq!(b.intensity.as_ref().unwrap().descriptor.unit(), SiUnit::One);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_package_export_validates_and_matches_the_oracle() {
    let desc = LineScanPackage::description();
    desc.validate().unwrap();
    validate_package::<LineScanPackage>().unwrap();

    // The streaming operations, hand-labelled from HAL.IDL.SENS.STREAM.
    let classes: BTreeMap<&str, SideEffect> = desc.operations.iter().map(|o| (o.name.as_str(), o.class)).collect();
    let want = BTreeMap::from([
        ("start_stream", SideEffect::D1),
        ("stop_stream", SideEffect::D1),
        ("stream_status", SideEffect::R0),
    ]);
    assert_eq!(classes, want);

    // LineScan-core@1 lists the REQ-004 items plus the Stream-common@1 items.
    assert_eq!(desc.profile.id, PROFILE_ID);
    let ids: Vec<&str> = desc.profile.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["LS-01", "LS-02", "LS-03", "LS-04", "LS-05", "SC-01", "SC-02", "SC-03", "SC-04"]);
    let titles: Vec<&str> = LINESCAN_CORE.items.iter().map(|i| i.title).collect();
    assert_eq!(
        titles,
        [
            "Encoding declaration",
            "Array lengths",
            "Timestamp monotonicity",
            "Masked-sample handling",
            "Trigger-mode behaviour"
        ]
    );
    assert_eq!(
        [ITEM_ENCODING, ITEM_LENGTHS, ITEM_TIMESTAMPS, ITEM_MASK, ITEM_TRIGGER],
        ["LS-01", "LS-02", "LS-03", "LS-04", "LS-05"]
    );
    assert_eq!(STREAM_COMMON.items.len(), 4);

    // The batch schema declares x and z in m with a validity mask and a sequence range.
    let bs = &desc.batch_schemas[0];
    assert!(bs.validity_mask && bs.seq_range);
    for c in bs.columns.iter().filter(|c| c.name == "x" || c.name == "z") {
        assert_eq!(c.encoding.unit(), SiUnit::Metre);
    }

    // The independently written export is the same package, with the same digest.
    let oracle_bytes = read("linescan_v1.json");
    let oracle = PackageDescription::from_json(&oracle_bytes).unwrap();
    assert_eq!(oracle.canonical_json().unwrap(), desc.canonical_json().unwrap());
    let want_digest = String::from_utf8(read("linescan_v1.sha256")).unwrap();
    assert_eq!(desc.digest().unwrap().as_str(), want_digest.trim());
    assert_eq!(validate_and_digest_json(&oracle_bytes).unwrap().as_str(), want_digest.trim());

    // The parsed export is a valid streaming package with the LineScan core settings.
    let core: BTreeSet<String> = CORE_SETTINGS.iter().map(|s| s.to_string()).collect();
    StreamingDescription::from_parts(oracle, BTreeSet::new(), core, Vec::new()).unwrap();
}

// ------------------------------------------------------------------ settings (REQ-HAL.IDL.SENS-006 at runtime)

fn schema_with(extra: Vec<SettingSchema>) -> ConfigSchema {
    let mut s = core_setting_schemas();
    s.extend(extra);
    ConfigSchema::new(s).unwrap()
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

fn q(v: f64, u: SiUnit) -> SettingValue {
    SettingValue::Quantity(Quantity::new(v, u).unwrap())
}

/// An invented time-triggered configuration: 20 us exposure, 10 um spacing, 10 kHz, 20 mm by
/// 40 mm region of interest.
fn config(mode: &str) -> Configuration {
    let mut values = BTreeMap::new();
    values.insert("encoder_divisor".to_string(), SettingValue::Int(4));
    values.insert("exposure".to_string(), q(20e-6, SiUnit::Second));
    values.insert("point_spacing".to_string(), q(10e-6, SiUnit::Metre));
    values.insert("roi_x_max".to_string(), q(0.01, SiUnit::Metre));
    values.insert("roi_x_min".to_string(), q(-0.01, SiUnit::Metre));
    values.insert("roi_z_max".to_string(), q(0.17, SiUnit::Metre));
    values.insert("roi_z_min".to_string(), q(0.13, SiUnit::Metre));
    values.insert("trigger_mode".to_string(), SettingValue::Enum(mode.to_string()));
    values.insert("trigger_period".to_string(), q(1e-4, SiUnit::Second));
    Configuration { values }
}

#[test]
fn core_settings_are_typed_and_read() {
    let pkg = LineScanPackage::streaming().unwrap();
    let schema = schema_with(Vec::new());
    check_device_schema(&pkg, &schema).unwrap();
    let set = LineScanSettings::from_configuration(&pkg, &schema, &config("time")).unwrap();
    assert_eq!(set.trigger, Trigger::Time { period_s: 1e-4 });
    assert_eq!(set.trigger.mode(), TriggerMode::Time);
    assert_eq!(set.exposure_s, 20e-6);
    assert_eq!(set.roi.z_min, 0.13);
    let set = LineScanSettings::from_configuration(&pkg, &schema, &config("encoder")).unwrap();
    assert_eq!(set.trigger, Trigger::Encoder { divisor: 4 });
    let set = LineScanSettings::from_configuration(&pkg, &schema, &config("external")).unwrap();
    assert_eq!(set.trigger, Trigger::External);
}

#[test]
fn undeclared_vendor_settings_are_refused() {
    let pkg = LineScanPackage::streaming().unwrap();
    // An undotted setting outside the core names (#612 lesson: fail closed).
    let e = check_device_schema(&pkg, &schema_with(vec![int_setting("gain")])).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
    // A dotted setting under a namespace describe does not list.
    let e = check_device_schema(&pkg, &schema_with(vec![int_setting("acme.gain")])).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
    // Listed and declared by the driver: accepted.
    let pkg = LineScanPackage::streaming_with(&["acme"], vec![int_setting("acme.gain")]).unwrap();
    check_device_schema(&pkg, &schema_with(vec![int_setting("acme.gain")])).unwrap();
    // A driver cannot declare an extension under a namespace it does not list.
    let e = LineScanPackage::streaming_with(&["acme"], vec![int_setting("other.gain")]).unwrap_err();
    assert_eq!(e.code(), "extension_undeclared");
}

#[test]
fn device_schemas_must_declare_the_core_settings() {
    let pkg = LineScanPackage::streaming().unwrap();
    let mut s = core_setting_schemas();
    s.retain(|x| x.name != "exposure");
    let e = check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap_err();
    assert_eq!(e, LineScanError::Settings { setting: "exposure".to_string(), reason: "missing" });

    let mut s = core_setting_schemas();
    s.iter_mut().find(|x| x.name == "point_spacing").unwrap().ty = SettingType::Quantity(SiUnit::Radian);
    let e = check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap_err();
    assert_eq!(e, LineScanError::Settings { setting: "point_spacing".to_string(), reason: "type_wrong" });

    let mut s = core_setting_schemas();
    s.iter_mut().find(|x| x.name == "exposure").unwrap().acquisition_relevant = false;
    let e = check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap_err();
    assert_eq!(e, LineScanError::Settings { setting: "exposure".to_string(), reason: "not_digested" });

    // A device that only supports two modes may say so; an unknown mode is refused.
    let mut s = core_setting_schemas();
    s.iter_mut().find(|x| x.name == "trigger_mode").unwrap().ty =
        SettingType::Enum(vec!["time".to_string(), "external".to_string()]);
    check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap();
    let mut s = core_setting_schemas();
    s.iter_mut().find(|x| x.name == "trigger_mode").unwrap().ty = SettingType::Enum(vec!["burst".to_string()]);
    let e = check_device_schema(&pkg, &ConfigSchema::new(s).unwrap()).unwrap_err();
    assert_eq!(e.code(), "settings_invalid");

    // A device range still applies (OUT_OF_RANGE from HAL.IDL.BASE.CFG).
    let mut s = core_setting_schemas();
    s.iter_mut().find(|x| x.name == "exposure").unwrap().range = Some(SettingRange::Quantity { min: 1e-6, max: 1e-5 });
    let schema = ConfigSchema::new(s).unwrap();
    let e = LineScanSettings::from_configuration(&pkg, &schema, &config("time")).unwrap_err();
    assert_eq!(e.code(), "OUT_OF_RANGE");

    // Another package's description is not a LineScan package.
    // A description without the LineScan core settings is not a LineScan package.
    let other =
        StreamingDescription::from_parts(LineScanPackage::description(), BTreeSet::new(), BTreeSet::new(), Vec::new())
            .unwrap();
    assert_eq!(
        check_device_schema(&other, &schema_with(Vec::new())).unwrap_err(),
        LineScanError::Settings { setting: String::new(), reason: "not_linescan" }
    );
}

#[test]
fn trigger_parameters_follow_the_mode() {
    let pkg = LineScanPackage::streaming().unwrap();
    let schema = schema_with(Vec::new());
    let read = |cfg: &Configuration| LineScanSettings::from_configuration(&pkg, &schema, cfg);

    let mut c = config("time");
    c.values.insert("trigger_period".to_string(), q(0.0, SiUnit::Second));
    assert_eq!(
        read(&c).unwrap_err(),
        LineScanError::Settings { setting: "trigger_period".to_string(), reason: "not_positive" }
    );
    // The period is not used in encoder mode, the divisor not in time mode.
    let mut c = config("encoder");
    c.values.insert("trigger_period".to_string(), q(0.0, SiUnit::Second));
    read(&c).unwrap();
    c.values.insert("encoder_divisor".to_string(), SettingValue::Int(0));
    assert_eq!(
        read(&c).unwrap_err(),
        LineScanError::Settings { setting: "encoder_divisor".to_string(), reason: "not_positive" }
    );
    let mut c = config("time");
    c.values.insert("encoder_divisor".to_string(), SettingValue::Int(0));
    read(&c).unwrap();

    let mut c = config("time");
    c.values.insert("exposure".to_string(), q(-1e-6, SiUnit::Second));
    assert_eq!(read(&c).unwrap_err().code(), "settings_invalid");
    let mut c = config("time");
    c.values.insert("roi_x_min".to_string(), q(0.01, SiUnit::Metre));
    assert_eq!(
        read(&c).unwrap_err(),
        LineScanError::Settings { setting: "roi_x_max".to_string(), reason: "roi_empty" }
    );
    let mut c = config("time");
    c.values.insert("trigger_mode".to_string(), SettingValue::Enum("burst".to_string()));
    assert_eq!(read(&c).unwrap_err().code(), "CONFIG_INVALID");
    let mut c = config("time");
    c.values.insert("gain".to_string(), SettingValue::Int(1));
    assert_eq!(read(&c).unwrap_err().code(), "CONFIG_INVALID");
}

// ------------------------------------------------------------------ LS-05

fn time(period_s: f64) -> LineScanSettings {
    let pkg = LineScanPackage::streaming().unwrap();
    let mut c = config("time");
    c.values.insert("trigger_period".to_string(), q(period_s, SiUnit::Second));
    LineScanSettings::from_configuration(&pkg, &schema_with(Vec::new()), &c).unwrap()
}

#[test]
fn ls05_time_mode_profiles_follow_the_period() {
    let expected = json("expected.json");
    let b = ProfileBatch::decode(&big().0, &big().1).unwrap();
    // The oracle's device-time steps are within 1 % of 100 us, so 5 % passes.
    let (lo, hi) = (n(get(&expected, "t_dev_step_min_ns")), n(get(&expected, "t_dev_step_max_ns")));
    assert!(lo >= 95_000 && hi <= 105_000, "{lo} {hi}");
    assert_eq!(TRIGGER_TOLERANCE_PERCENT, 5);
    check_trigger_mode(&time(1e-4), std::slice::from_ref(&b)).unwrap();

    let e = check_trigger_mode(&time(2e-4), std::slice::from_ref(&b)).unwrap_err();
    assert_eq!((e.item, e.reason, e.index), (ITEM_TRIGGER, "interval_off_period", Some(0)));

    // Two batches whose ranges join are checked across the join as well.
    let mut next = b.clone();
    next.first_seq = b.last_seq().unwrap() + 1;
    let shift = b.t_dev.as_ref().unwrap()[99] - b.t_dev.as_ref().unwrap()[0] + 100_000;
    next.t_dev = Some(b.t_dev.as_ref().unwrap().iter().map(|t| t + shift).collect());
    check_trigger_mode(&time(1e-4), &[b.clone(), next.clone()]).unwrap();
    next.t_dev = Some(b.t_dev.as_ref().unwrap().iter().map(|t| t + shift + 50_000).collect());
    let e = check_trigger_mode(&time(1e-4), &[b.clone(), next]).unwrap_err();
    assert_eq!((e.reason, e.index), ("interval_off_period", Some(1)));

    // Fail closed: no device time, nothing to check, nothing recorded.
    let (h, p) = case("ok_no_t_dev");
    let no_dev = ProfileBatch::decode(&h, &p).unwrap();
    assert_eq!(check_trigger_mode(&time(1e-4), &[no_dev]).unwrap_err().reason, "no_device_time");
    let mut single = b.clone();
    single.profiles = 1;
    single.t_host.truncate(1);
    single.t_dev.as_mut().unwrap().truncate(1);
    assert_eq!(check_trigger_mode(&time(1e-4), &[single]).unwrap_err().reason, "not_exercised");
    assert_eq!(check_trigger_mode(&time(1e-4), &[]).unwrap_err().reason, "no_batches");

    // Encoder and external profiles have no timing rule.
    let pkg = LineScanPackage::streaming().unwrap();
    let enc = LineScanSettings::from_configuration(&pkg, &schema_with(Vec::new()), &config("encoder")).unwrap();
    check_trigger_mode(&enc, std::slice::from_ref(&b)).unwrap();
}

#[test]
fn masks_count_and_round_trip() {
    let mut m = ValidityMask::all_valid(12);
    assert_eq!(m.as_bytes(), &[0xff, 0x0f]);
    m.set(0, false);
    assert_eq!(ValidityMask::from_bytes(12, m.as_bytes().to_vec()).unwrap().count_valid(), 11);
}
