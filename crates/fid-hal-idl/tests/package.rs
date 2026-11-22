//! Acceptance criteria of spec node `HAL.IDL.GEN.FMT` (AC-1 .. AC-6) and REQ-001 .. REQ-005.
//!
//! Fixtures are generated independently by `fixtures/hal/idl/gen/fmt/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::{
    is_semver, validate_and_digest_json, validate_package, BatchColumnDesc, BatchSchemaDesc, CapabilityPackage,
    ConformanceItemDesc, ConformanceProfileDesc, EncodingDescriptor, ErrorCode, NumericType, OperationDesc,
    PackageDescription, PackageManifest, SemanticsNote, SiUnit,
};
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn fix(name: &str) -> PathBuf {
    Path::new(ROOT).join("fixtures/hal/idl/gen/fmt").join(name)
}

fn read_fix(name: &str) -> Vec<u8> {
    fs::read(fix(name)).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_demo_v1_validates_and_matches_independent_sha256() {
    let bytes = read_fix("demo_v1.json");
    let want_digest = fs::read_to_string(fix("demo_v1.sha256")).unwrap();
    let want_digest = want_digest.trim();

    let desc = PackageDescription::from_json(&bytes).unwrap();
    desc.validate().unwrap();
    let got_digest = desc.digest().unwrap();
    assert_eq!(got_digest.as_str(), want_digest);
    assert_eq!(validate_and_digest_json(&bytes).unwrap(), got_digest);

    // Round-trip canonical JSON reproduces the exact same PackageDescription and digest.
    let canon = desc.canonical_json().unwrap();
    let desc2 = PackageDescription::from_json(&canon).unwrap();
    assert_eq!(desc2.digest().unwrap(), got_digest);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_dup_op_fails_at_operations_1_name() {
    let err = PackageDescription::from_json(&read_fix("dup_op.json")).unwrap_err();
    assert_eq!(err.code(), "package_invalid");
    assert_eq!(err.pointer, "/operations/1/name");
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_bad_class_z9_fails_at_class_pointer() {
    let err = PackageDescription::from_json(&read_fix("bad_class.json")).unwrap_err();
    assert_eq!(err.code(), "package_invalid");
    assert_eq!(err.pointer, "/operations/0/class");
    assert!(err.reason.contains("Z9"), "reason was: {}", err.reason);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_no_encoding_fails_at_column_encoding_pointer() {
    let err = PackageDescription::from_json(&read_fix("no_encoding.json")).unwrap_err();
    assert_eq!(err.code(), "package_invalid");
    assert_eq!(err.pointer, "/batch_schemas/0/columns/0/encoding");
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_foreign_keyword_fails_naming_pattern_properties() {
    let err = PackageDescription::from_json(&read_fix("foreign_keyword.json")).unwrap_err();
    assert_eq!(err.code(), "package_invalid");
    assert_eq!(err.pointer, "/operations/0/request/patternProperties");
    assert!(err.reason.contains("patternProperties"), "reason was: {}", err.reason);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_permuted_operations_produce_identical_canonical_digest() {
    let mut desc = PackageDescription::from_json(&read_fix("demo_v1.json")).unwrap();
    let d1 = desc.digest().unwrap();

    // Reverse operations, errors, and profile items: canonical digest must be unchanged.
    desc.operations.reverse();
    for op in &mut desc.operations {
        op.errors.reverse();
    }
    desc.profile.items.reverse();
    let d2 = desc.digest().unwrap();
    assert_eq!(d1, d2);
}

// ------------------------------------------------------------------ CapabilityPackage trait & REQ-002..005

struct DemoPackage;

impl CapabilityPackage for DemoPackage {
    fn description() -> PackageDescription {
        let mut defs = BTreeMap::new();
        defs.insert(
            "GainValue".to_string(),
            CValue::Obj(BTreeMap::from([
                ("maximum".to_string(), CValue::Num(100.0)),
                ("minimum".to_string(), CValue::Num(0.0)),
                ("type".to_string(), CValue::Str("number".to_string())),
            ])),
        );
        let req_set_gain = CValue::Obj(BTreeMap::from([
            ("additionalProperties".to_string(), CValue::Bool(false)),
            (
                "properties".to_string(),
                CValue::Obj(BTreeMap::from([
                    (
                        "channel".to_string(),
                        CValue::Obj(BTreeMap::from([
                            ("maxLength".to_string(), CValue::Num(32.0)),
                            ("type".to_string(), CValue::Str("string".to_string())),
                        ])),
                    ),
                    (
                        "gain".to_string(),
                        CValue::Obj(BTreeMap::from([(
                            "$ref".to_string(),
                            CValue::Str("#/$defs/GainValue".to_string()),
                        )])),
                    ),
                ])),
            ),
            ("required".to_string(), CValue::Arr(vec![CValue::Str("gain".to_string())])),
            ("type".to_string(), CValue::Str("object".to_string())),
        ]));
        let resp_set_gain = CValue::Obj(BTreeMap::from([
            ("additionalProperties".to_string(), CValue::Bool(false)),
            (
                "properties".to_string(),
                CValue::Obj(BTreeMap::from([(
                    "applied".to_string(),
                    CValue::Obj(BTreeMap::from([("$ref".to_string(), CValue::Str("#/$defs/GainValue".to_string()))])),
                )])),
            ),
            ("required".to_string(), CValue::Arr(vec![CValue::Str("applied".to_string())])),
            ("type".to_string(), CValue::Str("object".to_string())),
        ]));
        let req_read = CValue::Obj(BTreeMap::from([
            ("additionalProperties".to_string(), CValue::Bool(false)),
            (
                "properties".to_string(),
                CValue::Obj(BTreeMap::from([(
                    "mode".to_string(),
                    CValue::Obj(BTreeMap::from([
                        (
                            "enum".to_string(),
                            CValue::Arr(vec![CValue::Str("single".to_string()), CValue::Str("average".to_string())]),
                        ),
                        ("type".to_string(), CValue::Str("string".to_string())),
                    ])),
                )])),
            ),
            ("required".to_string(), CValue::Arr(vec![CValue::Str("mode".to_string())])),
            ("type".to_string(), CValue::Str("object".to_string())),
        ]));
        let resp_read = CValue::Obj(BTreeMap::from([
            ("additionalProperties".to_string(), CValue::Bool(false)),
            (
                "properties".to_string(),
                CValue::Obj(BTreeMap::from([(
                    "samples".to_string(),
                    CValue::Obj(BTreeMap::from([
                        (
                            "items".to_string(),
                            CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("number".to_string()))])),
                        ),
                        ("type".to_string(), CValue::Str("array".to_string())),
                    ])),
                )])),
            ),
            ("required".to_string(), CValue::Arr(vec![CValue::Str("samples".to_string())])),
            ("type".to_string(), CValue::Str("object".to_string())),
        ]));

        let fixture_digest = Digest::from_bytes(fid_canon::digest_bytes(b"demo-fixture-v1").0);

        PackageDescription {
            manifest: PackageManifest {
                name: "Demo".to_string(),
                version: "1.0.0".to_string(),
                deprecated: false,
                tier3_usable: true,
            },
            defs,
            operations: vec![
                OperationDesc {
                    name: "read_sample".to_string(),
                    class: SideEffect::R0,
                    request: req_read,
                    response: resp_read,
                    errors: vec![ErrorCode::Transient, ErrorCode::CommLost],
                    live_settings: Vec::new(),
                },
                OperationDesc {
                    name: "set_gain".to_string(),
                    class: SideEffect::D1,
                    request: req_set_gain,
                    response: resp_set_gain,
                    errors: vec![ErrorCode::ConfigInvalid, ErrorCode::OutOfRange],
                    live_settings: vec!["sensor.gain".to_string()],
                },
            ],
            batch_schemas: vec![BatchSchemaDesc {
                id: "demo.points".to_string(),
                columns: vec![
                    BatchColumnDesc {
                        name: "x_pos".to_string(),
                        unit: SiUnit::Metre,
                        encoding: EncodingDescriptor::new(NumericType::I32, 1e-6, 0.0, SiUnit::Metre).unwrap(),
                    },
                    BatchColumnDesc {
                        name: "temp_k".to_string(),
                        unit: SiUnit::Kelvin,
                        encoding: EncodingDescriptor::new(NumericType::F32, 1.0, 273.15, SiUnit::Kelvin).unwrap(),
                    },
                ],
                validity_mask: true,
                seq_range: true,
            }],
            semantics: SemanticsNote::new(
                "# Demo@1 semantics\n\n\
                 Two-operation demonstration capability package for `HAL.IDL.GEN.FMT`.\n\
                 - `read_sample` reads one calibrated point without energising motion.\n\
                 - `set_gain` updates the live gain setting.\n",
            ),
            profile: ConformanceProfileDesc {
                id: "hal.demo.v1".to_string(),
                items: vec![
                    ConformanceItemDesc {
                        id: "DEMO-001".to_string(),
                        summary: "read_sample returns calibrated positions in metres.".to_string(),
                        fixture_digest: Some(fixture_digest),
                    },
                    ConformanceItemDesc {
                        id: "DEMO-002".to_string(),
                        summary: "Live gain update stays within [0, 100].".to_string(),
                        fixture_digest: None,
                    },
                ],
            },
        }
    }
}

#[test]
fn capability_package_trait_matches_demo_v1_fixture_digest() {
    let want = fs::read_to_string(fix("demo_v1.sha256")).unwrap();
    let got = validate_package::<DemoPackage>().unwrap();
    assert_eq!(got.as_str(), want.trim());
}

#[test]
fn req002_req004_req005_additional_error_paths_and_schema_file() {
    // SemVer checks
    assert!(is_semver("1.0.0"));
    assert!(is_semver("0.2.3-rc.1+build.42"));
    assert!(!is_semver("1.0"));
    assert!(!is_semver("01.0.0"));
    assert!(!is_semver("v1.0.0"));

    // Non-semver version fails at /manifest/version
    let mut bad_ver = DemoPackage::description();
    bad_ver.manifest.version = "v1.0".to_string();
    let err = bad_ver.validate().unwrap_err();
    assert_eq!(err.code(), "package_invalid");
    assert_eq!(err.pointer, "/manifest/version");

    // External $ref is forbidden (REQ-004, REQ-005)
    let mut ext_ref = DemoPackage::description();
    ext_ref.operations[0].request = CValue::Obj(BTreeMap::from([(
        "$ref".to_string(),
        CValue::Str("https://example.invalid/evil.json".to_string()),
    )]));
    let err = ext_ref.validate().unwrap_err();
    assert_eq!(err.pointer, "/operations/0/request/$ref");

    // Cyclic internal $ref is refused
    let mut cyc = DemoPackage::description();
    cyc.defs.insert(
        "LoopA".to_string(),
        CValue::Obj(BTreeMap::from([("$ref".to_string(), CValue::Str("#/$defs/LoopB".to_string()))])),
    );
    cyc.defs.insert(
        "LoopB".to_string(),
        CValue::Obj(BTreeMap::from([("$ref".to_string(), CValue::Str("#/$defs/LoopA".to_string()))])),
    );
    let err = cyc.validate().unwrap_err();
    assert!(err.pointer.starts_with("/$defs/Loop"), "pointer: {}", err.pointer);

    // additionalProperties: true is refused (REQ-004)
    let mut bad_ap = DemoPackage::description();
    bad_ap.operations[0].request = CValue::Obj(BTreeMap::from([
        ("additionalProperties".to_string(), CValue::Bool(true)),
        ("type".to_string(), CValue::Str("object".to_string())),
    ]));
    let err = bad_ap.validate().unwrap_err();
    assert_eq!(err.pointer, "/operations/0/request/additionalProperties");

    // Executable content in semantics note is refused (REQ-005)
    let mut exec_sem = DemoPackage::description();
    exec_sem.semantics = SemanticsNote::new("Note with <script>alert(1)</script>");
    let err = exec_sem.validate().unwrap_err();
    assert_eq!(err.pointer, "/semantics/text");

    // Semantics digest tamper is refused
    let mut bad_sem_dig = DemoPackage::description();
    bad_sem_dig.semantics.digest = Digest::from_bytes([0u8; 32]);
    let err = bad_sem_dig.validate().unwrap_err();
    assert_eq!(err.pointer, "/semantics/digest");

    // Unknown Device@1 error code in raw JSON fails at /operations/0/errors/0
    let mut raw = String::from_utf8(read_fix("demo_v1.json")).unwrap();
    raw = raw.replacen("OUT_OF_RANGE", "MADE_UP_CODE", 1);
    let err = PackageDescription::from_json(raw.as_bytes()).unwrap_err();
    assert_eq!(err.code(), "package_invalid");
    assert_eq!(err.pointer, "/operations/0/errors/0");

    // Published JSON Schema at schemas/hal/capability-package-v1.json parses as valid canonical JSON.
    let schema_bytes = fs::read(Path::new(ROOT).join("schemas/hal/capability-package-v1.json")).unwrap();
    let schema_val = fid_canon::parse(&schema_bytes).unwrap();
    let CValue::Obj(m) = schema_val else {
        panic!("schema must be a JSON object");
    };
    assert_eq!(m.get("title"), Some(&CValue::Str("capability-package/v1".to_string())));

    // Performance sanity check (50-operation package validates quickly).
    let mut big = DemoPackage::description();
    let tmpl = big.operations[0].clone();
    big.operations.clear();
    for i in 0..50 {
        let mut op = tmpl.clone();
        op.name = format!("op_{i:02}");
        big.operations.push(op);
    }
    let t0 = Instant::now();
    big.validate().unwrap();
    let _ = big.digest().unwrap();
    assert!(t0.elapsed().as_millis() < 200, "50-op validation took {:?}", t0.elapsed());
}
