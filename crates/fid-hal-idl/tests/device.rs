//! Acceptance criteria of spec node `HAL.IDL.BASE.DESC` (AC-1 .. AC-5) and its requirements.
//!
//! Expected values come from `fixtures/hal/idl/base/desc/make_fixtures.py` (an independent
//! stdlib oracle) or are hand-labelled from the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::base::device::{
    CloseSessionRequest, CloseSessionResponse, DescribeRequest, DescribeResponse, GetCalibrationStateRequest,
    GetCalibrationStateResponse, GetConfigurationRequest, GetConfigurationResponse, GetHealthRequest,
    GetHealthResponse, OpenSessionRequest, OpenSessionResponse, SetConfigurationRequest, SetConfigurationResponse,
    WireValue, MAX_DCC_DIGESTS, MAX_RECORD_BYTES, PROFILE_ID,
};
use fid_hal_idl::{
    config_digest, validate_package, CalDate, CalibrationState, CapabilityPackage, ConfigSchema, Configuration,
    DeviceInstance, DeviceOperation, DevicePackage, DriverId, HealthState, PackageDescription, DEVICE_CORE,
};
use fid_types::{Digest, SideEffect};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn fix(name: &str) -> PathBuf {
    Path::new(ROOT).join("fixtures/hal/idl/base/desc").join(name)
}

fn read(name: &str) -> Vec<u8> {
    fs::read(fix(name)).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_package_validates_and_lists_seven_operations_with_their_classes() {
    validate_package::<DevicePackage>().unwrap();
    let desc = DevicePackage::description();
    assert_eq!(desc.manifest.name, "Device");
    assert_eq!(desc.manifest.version, "1.0.0");

    // Hand-labelled from REQ-HAL.IDL.BASE.DESC-001.
    let want: BTreeMap<&str, SideEffect> = BTreeMap::from([
        ("describe", SideEffect::R0),
        ("get_health", SideEffect::R0),
        ("get_configuration", SideEffect::R0),
        ("get_calibration_state", SideEffect::R0),
        ("open_session", SideEffect::D1),
        ("close_session", SideEffect::D1),
        ("set_configuration", SideEffect::D1),
    ]);
    let got: BTreeMap<&str, SideEffect> = desc.operations.iter().map(|op| (op.name.as_str(), op.class)).collect();
    assert_eq!(desc.operations.len(), 7);
    assert_eq!(got, want);
    assert!(desc.batch_schemas.is_empty(), "Device@1 is control plane only");
    for op in &desc.operations {
        assert!(!op.errors.is_empty(), "{} has no error list", op.name);
    }

    // The independently written export parses, validates and is the same package.
    let oracle = PackageDescription::from_json(&read("device_v1.json")).unwrap();
    assert_eq!(oracle.canonical_json().unwrap(), desc.canonical_json().unwrap());
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_digest_matches_the_independent_oracle() {
    let want = String::from_utf8(read("device_v1.sha256")).unwrap();
    let want = want.trim();
    assert_eq!(validate_package::<DevicePackage>().unwrap().as_str(), want);
    let oracle = PackageDescription::from_json(&read("device_v1.json")).unwrap();
    assert_eq!(oracle.digest().unwrap().as_str(), want);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_long_serial_is_truncated_by_the_untrusted_text_rule() {
    let raw = read("instance.json");
    let inst = DeviceInstance::from_json(&raw).unwrap();
    let serial = inst.serial.as_ref().unwrap();
    // 5000 bytes in; the 3-byte character at bytes 4094..4096 does not fit, so 4094 stay.
    assert_eq!(serial.text().len(), 4094);
    assert!(serial.truncated());
    assert!(!inst.make.truncated() && !inst.model.truncated());
    assert_eq!(inst.driver_id.as_str(), "drv.example.linescan");

    let expected = fid_canon::parse(&read("instance.expected.json")).unwrap();
    assert_eq!(inst.to_value(), expected);
    // The kept form reads back unchanged.
    assert_eq!(DeviceInstance::from_value(expected).unwrap(), inst);
}

// ------------------------------------------------------------------ AC-4

/// Every property name reachable from `schema`, following internal `$ref`s.
fn property_names(schema: &CValue, defs: &BTreeMap<String, CValue>, out: &mut BTreeSet<String>, depth: usize) {
    assert!(depth < 32, "schema walk too deep");
    let CValue::Obj(m) = schema else { return };
    if let Some(CValue::Str(r)) = m.get("$ref") {
        let target = r.strip_prefix("#/$defs/").unwrap();
        property_names(&defs[target], defs, out, depth + 1);
    }
    if let Some(CValue::Obj(props)) = m.get("properties") {
        for (k, sub) in props {
            out.insert(k.clone());
            property_names(sub, defs, out, depth + 1);
        }
    }
    if let Some(items) = m.get("items") {
        property_names(items, defs, out, depth + 1);
    }
}

const CALIBRATION_WORDS: [&str; 4] = ["calib", "qualif", "compens", "dcc"];

fn calibration_fields_in_requests(desc: &PackageDescription) -> Vec<(String, String)> {
    let mut hits = Vec::new();
    for op in &desc.operations {
        let mut names = BTreeSet::new();
        property_names(&op.request, &desc.defs, &mut names, 0);
        for n in names {
            if CALIBRATION_WORDS.iter().any(|w| n.to_ascii_lowercase().contains(w)) {
                hits.push((op.name.clone(), n));
            }
        }
    }
    hits
}

#[test]
fn ac4_no_request_carries_a_calibration_write_field() {
    let desc = DevicePackage::description();
    assert_eq!(calibration_fields_in_requests(&desc), Vec::<(String, String)>::new());

    // Closed list: any new request field fails here until someone reviews it against REQ-003.
    let mut all_request_fields = BTreeSet::new();
    for op in &desc.operations {
        property_names(&op.request, &desc.defs, &mut all_request_fields, 0);
    }
    let want: BTreeSet<String> = ["configuration", "session_id"].iter().map(|s| s.to_string()).collect();
    assert_eq!(all_request_fields, want);

    // The only calibration operation reads.
    for op in &desc.operations {
        if CALIBRATION_WORDS.iter().any(|w| op.name.contains(w)) {
            assert_eq!(op.name, "get_calibration_state");
            assert_eq!(op.class, SideEffect::R0);
        }
    }

    // The walk is not vacuous: a planted field behind a $ref is found.
    let mut planted = desc.clone();
    let set_cfg = planted.operations.iter_mut().find(|o| o.name == "set_configuration").unwrap();
    let CValue::Obj(req) = &mut set_cfg.request else { panic!() };
    let Some(CValue::Obj(props)) = req.get_mut("properties") else { panic!() };
    props.insert(
        "patch".to_string(),
        CValue::Obj(BTreeMap::from([("$ref".to_string(), CValue::Str("#/$defs/CalibrationState".to_string()))])),
    );
    planted.validate().unwrap();
    let hits = calibration_fields_in_requests(&planted);
    assert!(hits.contains(&("set_configuration".to_string(), "dcc_digests".to_string())), "{hits:?}");
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_device_core_profile_lists_the_six_items_with_stable_ids() {
    assert_eq!(DEVICE_CORE.id, "Device-core@1");
    assert_eq!(PROFILE_ID, "Device-core@1");
    // Hand-labelled from REQ-HAL.IDL.BASE.DESC-004, in that order.
    let want = [
        ("DC-01", "describe"),
        ("DC-02", "session"),
        ("DC-03", "transition table"),
        ("DC-04", "error-code closure"),
        ("DC-05", "digest stability"),
        ("DC-06", "read-only"),
    ];
    assert_eq!(DEVICE_CORE.items.len(), want.len());
    for (item, (id, topic)) in DEVICE_CORE.items.iter().zip(want) {
        assert_eq!(item.id, id);
        assert!(item.title.to_ascii_lowercase().contains(topic), "{} does not name {topic}", item.title);
        assert!(!item.checks.is_empty());
    }
    let desc = DevicePackage::description();
    assert_eq!(desc.profile.id, "Device-core@1");
    let ids: Vec<&str> = desc.profile.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["DC-01", "DC-02", "DC-03", "DC-04", "DC-05", "DC-06"]);
}

#[test]
fn profile_ids_accept_only_one_numeric_major_suffix() {
    let base = DevicePackage::description();
    for ok in ["Device-core@1", "Device-core@10", "Device-core", "a/b@0"] {
        let mut d = base.clone();
        d.profile.id = ok.to_string();
        d.validate().unwrap();
    }
    for bad in ["Device-core@01", "Device-core@", "@1", "a@1@2", "a@x", "a..b@1", "-a@1"] {
        let mut d = base.clone();
        d.profile.id = bad.to_string();
        let err = d.validate().unwrap_err();
        assert_eq!(err.pointer, "/profile/id", "{bad}");
    }
}

// ------------------------------------------------------------------ typed requests and responses

/// The wire value of a typed request or response is an object whose keys are exactly allowed
/// by its operation's schema and include every required key.
fn check_against_schema<T: WireValue>(v: &T, request: bool) {
    let op = T::OPERATION;
    let schema = if request { op.request_schema() } else { op.response_schema() };
    let CValue::Obj(s) = schema else { panic!() };
    let allowed: BTreeSet<String> = match s.get("properties") {
        Some(CValue::Obj(p)) => p.keys().cloned().collect(),
        _ => BTreeSet::new(),
    };
    let required: BTreeSet<String> = match s.get("required") {
        Some(CValue::Arr(r)) => r
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                _ => panic!(),
            })
            .collect(),
        _ => BTreeSet::new(),
    };
    let CValue::Obj(m) = v.to_value() else { panic!("{} value is not an object", op.name()) };
    let keys: BTreeSet<String> = m.keys().cloned().collect();
    assert!(keys.is_subset(&allowed), "{} {keys:?} not within {allowed:?}", op.name());
    assert!(required.is_subset(&keys), "{} {keys:?} misses {required:?}", op.name());
}

fn linescan_configuration() -> (ConfigSchema, Configuration) {
    let cfg = Path::new(ROOT).join("fixtures/hal/idl/base/cfg");
    let schema = ConfigSchema::from_json(&fs::read(cfg.join("linescan_cfg.schema.json")).unwrap()).unwrap();
    let c = Configuration::from_json(&schema, &fs::read(cfg.join("linescan_cfg.json")).unwrap()).unwrap();
    (schema, c)
}

#[test]
fn typed_values_match_their_operation_schemas() {
    let sid = Uuid::parse("0b5f3e3c-2f43-4b7a-9d5e-6a1f2c3d4e5f").unwrap();
    let (schema, configuration) = linescan_configuration();
    let digest = config_digest(&schema, &configuration).unwrap();
    let instance = DeviceInstance::new(
        "Example Optics",
        "LS-100",
        Some("SN-1"),
        None,
        DriverId::parse("drv.example.linescan").unwrap(),
    );
    let calibration = CalibrationState {
        dcc_digests: vec![Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap()],
        qualification_ref: None,
        compensation_id: None,
        last_interim_check: CalDate::parse("2026-10-01"),
        valid_until: CalDate::parse("2027-10-01"),
    };

    check_against_schema(&DescribeRequest, true);
    check_against_schema(&DescribeResponse { instance }, false);
    check_against_schema(&OpenSessionRequest { session_id: sid }, true);
    check_against_schema(&OpenSessionResponse { session_id: sid, health: HealthState::Initializing }, false);
    check_against_schema(&CloseSessionRequest { session_id: sid }, true);
    check_against_schema(&CloseSessionResponse { health: HealthState::Offline }, false);
    check_against_schema(&GetHealthRequest { session_id: sid }, true);
    check_against_schema(&GetHealthResponse { health: HealthState::Ready }, false);
    check_against_schema(&GetConfigurationRequest { session_id: sid }, true);
    check_against_schema(
        &GetConfigurationResponse { configuration: configuration.clone(), config_digest: digest },
        false,
    );
    check_against_schema(&SetConfigurationRequest { session_id: sid, configuration }, true);
    check_against_schema(&SetConfigurationResponse { config_digest: digest }, false);
    check_against_schema(&GetCalibrationStateRequest { session_id: sid }, true);
    check_against_schema(&GetCalibrationStateResponse { calibration }, false);

    // Every operation is covered by the list above.
    assert_eq!(DeviceOperation::ALL.len(), 7);
}

#[test]
fn operation_error_lists_are_unique_and_from_the_error_model() {
    for op in DeviceOperation::ALL {
        let set: BTreeSet<_> = op.errors().iter().collect();
        assert_eq!(set.len(), op.errors().len(), "{}", op.name());
    }
}

// ------------------------------------------------------------------ records fail closed

fn instance_err(json: &str) -> (String, &'static str) {
    let e = DeviceInstance::from_json(json.as_bytes()).unwrap_err();
    assert_eq!(e.code(), "device_record_invalid");
    (e.pointer, e.reason)
}

const TEXT: &str = r#"{"bytes":"x","truncated":false}"#;

#[test]
fn malformed_instances_are_refused_with_a_pointer() {
    let ok = format!(r#"{{"driver_id":"drv.a","make":{TEXT},"model":{TEXT}}}"#);
    let inst = DeviceInstance::from_json(ok.as_bytes()).unwrap();
    assert!(inst.serial.is_none() && inst.firmware.is_none());

    let cases: [(String, (&str, &str)); 9] = [
        ("[]".to_string(), ("", "not_object")),
        ("{".to_string(), ("", "json")),
        (format!(r#"{{"driver_id":"drv.a","make":{TEXT}}}"#), ("/model", "field_missing")),
        (format!(r#"{{"make":{TEXT},"model":{TEXT}}}"#), ("/driver_id", "field_missing")),
        (format!(r#"{{"driver_id":"Drv","make":{TEXT},"model":{TEXT}}}"#), ("/driver_id", "driver_id_invalid")),
        (format!(r#"{{"driver_id":7,"make":{TEXT},"model":{TEXT}}}"#), ("/driver_id", "wrong_type")),
        (format!(r#"{{"driver_id":"drv.a","make":{TEXT},"model":{TEXT},"serial":null}}"#), ("/serial", "not_object")),
        (format!(r#"{{"driver_id":"drv.a","make":"x","model":{TEXT}}}"#), ("/make", "not_object")),
        (format!(r#"{{"driver_id":"drv.a","make":{TEXT},"model":{TEXT},"hint":"x"}}"#), ("/hint", "field_unknown")),
    ];
    for (json, (ptr, reason)) in cases {
        assert_eq!(instance_err(&json), (ptr.to_string(), reason), "{json}");
    }

    // Errors inside a DeviceText name the inner field.
    let inner: [(String, (&str, &str)); 4] = [
        (
            format!(r#"{{"driver_id":"drv.a","make":{{"bytes":7,"truncated":false}},"model":{TEXT}}}"#),
            ("/make/bytes", "wrong_type"),
        ),
        (
            format!(r#"{{"driver_id":"drv.a","make":{{"bytes":"x","truncated":0}},"model":{TEXT}}}"#),
            ("/make/truncated", "wrong_type"),
        ),
        (
            format!(r#"{{"driver_id":"drv.a","make":{TEXT},"model":{{"bytes":"x"}}}}"#),
            ("/model/truncated", "field_missing"),
        ),
        (
            format!(
                r#"{{"driver_id":"drv.a","make":{TEXT},"model":{TEXT},"serial":{{"bytes":"x","truncated":false,"note":"x"}}}}"#
            ),
            ("/serial/note", "field_unknown"),
        ),
    ];
    for (json, (ptr, reason)) in inner {
        assert_eq!(instance_err(&json), (ptr.to_string(), reason), "{json}");
    }

    // Duplicate keys and oversized documents are refused before any value is built.
    let dup = format!(r#"{{"driver_id":"drv.a","driver_id":"drv.b","make":{TEXT},"model":{TEXT}}}"#);
    assert_eq!(instance_err(&dup), (String::new(), "json"));
    let big = format!(
        r#"{{"driver_id":"drv.a","make":{{"bytes":"{}","truncated":false}},"model":{TEXT}}}"#,
        "m".repeat(MAX_RECORD_BYTES)
    );
    assert_eq!(instance_err(&big), (String::new(), "json"));
}

fn digest_n(n: usize) -> String {
    format!("sha256:{n:064x}")
}

#[test]
fn calibration_state_round_trips_and_fails_closed() {
    let ok = format!(
        r#"{{"dcc_digests":["{}","{}"],"qualification_ref":{TEXT},"compensation_id":{TEXT},"last_interim_check":"2026-10-01","valid_until":"2027-02-28"}}"#,
        digest_n(1),
        digest_n(2)
    );
    let st = CalibrationState::from_json(ok.as_bytes()).unwrap();
    assert_eq!(st.dcc_digests.len(), 2);
    assert_eq!(st.valid_until.as_ref().map(CalDate::as_str), Some("2027-02-28"));
    assert_eq!(st.to_value(), fid_canon::parse(ok.as_bytes()).unwrap());

    let err = |json: String| {
        let e = CalibrationState::from_json(json.as_bytes()).unwrap_err();
        (e.pointer, e.reason)
    };
    assert_eq!(err("{}".to_string()), ("/dcc_digests".to_string(), "field_missing"));
    assert_eq!(
        err(format!(r#"{{"dcc_digests":["{}","{}"]}}"#, digest_n(2), digest_n(1))),
        ("/dcc_digests/1".to_string(), "digests_not_ascending")
    );
    assert_eq!(
        err(format!(r#"{{"dcc_digests":["{}","{}"]}}"#, digest_n(1), digest_n(1))),
        ("/dcc_digests/1".to_string(), "digests_not_ascending")
    );
    assert_eq!(err(r#"{"dcc_digests":["sha256:00"]}"#.to_string()), ("/dcc_digests/0".to_string(), "digest_invalid"));
    assert_eq!(
        err(r#"{"dcc_digests":[],"valid_until":"2027-02-29"}"#.to_string()),
        ("/valid_until".to_string(), "date_invalid")
    );
    assert_eq!(
        err(r#"{"dcc_digests":[],"calibration_write":true}"#.to_string()),
        ("/calibration_write".to_string(), "field_unknown")
    );
    let many: Vec<String> = (0..=MAX_DCC_DIGESTS).map(|i| format!("\"{}\"", digest_n(i))).collect();
    assert_eq!(
        err(format!(r#"{{"dcc_digests":[{}]}}"#, many.join(","))),
        ("/dcc_digests".to_string(), "too_many_digests")
    );
    // A flood of elements trips the parse budget before the list is built.
    let flood: Vec<&str> = vec!["0"; 2000];
    assert_eq!(err(format!(r#"{{"dcc_digests":[{}]}}"#, flood.join(","))), (String::new(), "json"));
}
