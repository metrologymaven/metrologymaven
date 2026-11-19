//! Acceptance criteria of spec node HAL.IDL.BASE.CFG (AC-1 .. AC-4). Expected digests and
//! canonical bytes come from `fixtures/hal/idl/base/cfg/make_fixtures.py` (an independent
//! stdlib Python reference); expected error codes, settings and reasons are hand-labelled
//! there from the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::base::config::{
    config_digest, digest_canonical_bytes, validate, ConfigErrorCode, ConfigReason, ConfigSchema, Configuration,
    SettingRange, SettingSchema, SettingType, SettingValue,
};
use fid_hal_idl::{Quantity, SiUnit};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/base/cfg")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn schema() -> ConfigSchema {
    ConfigSchema::from_json(&read("linescan_cfg.schema.json")).unwrap()
}

fn base_config() -> Configuration {
    Configuration::from_json(&schema(), &read("linescan_cfg.json")).unwrap()
}

fn expected_digest() -> String {
    String::from_utf8(read("linescan_cfg.sha256")).unwrap().trim_end().to_string()
}

/// The cases of a `*_cases.json` file as objects.
fn cases(name: &str) -> Vec<std::collections::BTreeMap<String, CValue>> {
    let CValue::Arr(items) = fid_canon::parse(&read(name)).unwrap() else { panic!("{name}: not an array") };
    items
        .into_iter()
        .map(|c| match c {
            CValue::Obj(o) => o,
            other => panic!("{name}: case is not an object: {other:?}"),
        })
        .collect()
}

fn text(o: &std::collections::BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("field {k}: {other:?}"),
    }
}

/// AC-1 (D0): the digest of the line-scan configuration equals the reference digest, and
/// its canonical subset equals the reference bytes. This test runs on the Linux, Windows and
/// macOS runners, so the three digests agree.
#[test]
fn ac1_linescan_digest() {
    let s = schema();
    let cfg = base_config();
    assert_eq!(digest_canonical_bytes(&s, &cfg).unwrap(), read("linescan_cfg.canonical.json"));
    let d = config_digest(&s, &cfg).unwrap();
    assert_eq!(d.as_str(), expected_digest());
    assert_eq!(d.to_string(), expected_digest());
    assert_eq!(d.digest().as_str(), expected_digest());
}

/// AC-2 and AC-3: one-setting variations digest to the reference values; non-relevant
/// settings leave the digest unchanged, relevant settings and extensions change it.
#[test]
fn ac2_ac3_digest_cases() {
    let s = schema();
    let base = expected_digest();
    let all = cases("digest_cases.json");
    assert!(all.len() >= 8);
    for case in all {
        let name = text(&case, "name");
        let cfg = Configuration::from_value(&s, case["config"].clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let d = config_digest(&s, &cfg).unwrap();
        assert_eq!(d.as_str(), text(&case, "sha256"), "{name}");
        let same = case["same_as_base"] == CValue::Bool(true);
        assert_eq!(d.as_str() == base, same, "{name}");
    }
}

/// AC-2 on the typed API: `display_name` is not digested, `exposure` is.
#[test]
fn ac2_typed_changes() {
    let s = schema();
    let base = config_digest(&s, &base_config()).unwrap();

    let mut cfg = base_config();
    cfg.values.insert("display_name".into(), SettingValue::Text("another name".into()));
    assert_eq!(config_digest(&s, &cfg).unwrap(), base);

    let mut cfg = base_config();
    cfg.values.insert("exposure".into(), SettingValue::Quantity(Quantity::new(5e-4, SiUnit::Second).unwrap()));
    assert_ne!(config_digest(&s, &cfg).unwrap(), base);
}

/// Maintainer decision: -0.0 and +0.0 are the same setpoint, with the same digest and the
/// same written form.
#[test]
fn negative_zero_is_normalised() {
    let s = schema();
    let digest_with = |v: f64| {
        let mut cfg = base_config();
        cfg.values.insert("roi_start".into(), SettingValue::Quantity(Quantity::new(v, SiUnit::Metre).unwrap()));
        (config_digest(&s, &cfg).unwrap(), fid_canon::to_canonical_bytes(&cfg.to_value()).unwrap())
    };
    assert_eq!(digest_with(-0.0), digest_with(0.0));
    let case = cases("digest_cases.json").into_iter().find(|c| text(c, "name") == "roi_start_negative_zero").unwrap();
    let read = Configuration::from_value(&s, case["config"].clone()).unwrap();
    let Some(SettingValue::Quantity(q)) = read.values.get("roi_start") else { panic!("roi_start") };
    assert_eq!(q.value().to_bits(), 0);
}

/// AC-3 on the typed API: a declared extension changes the digest.
#[test]
fn ac3_extension_changes_digest() {
    let s = schema();
    let base = config_digest(&s, &base_config()).unwrap();
    let gain = s.get("acme.gain").unwrap();
    assert!(gain.is_extension() && gain.is_digested());
    let mut cfg = base_config();
    cfg.values.insert("acme.gain".into(), SettingValue::Int(5));
    assert_ne!(config_digest(&s, &cfg).unwrap(), base);
}

/// AC-4 and the further rules: every invalid configuration fails with the hand-labelled
/// code, setting and reason.
#[test]
fn ac4_invalid_configurations() {
    let s = schema();
    let all = cases("invalid_cases.json");
    assert!(all.len() >= 20);
    for case in all {
        let name = text(&case, "name");
        let e = Configuration::from_value(&s, case["config"].clone())
            .err()
            .unwrap_or_else(|| panic!("{name} must be rejected"));
        assert_eq!(
            (e.code(), e.setting.as_str(), e.reason.as_str()),
            (text(&case, "code").as_str(), text(&case, "setting").as_str(), text(&case, "reason").as_str()),
            "{name}"
        );
    }
}

/// AC-4 on the typed API: `validate` names `acme.secret` and the out-of-range exposure.
#[test]
fn ac4_typed_validation() {
    let s = schema();
    assert_eq!(validate(&s, &base_config()), Ok(()));

    let mut cfg = base_config();
    cfg.values.insert("acme.secret".into(), SettingValue::Int(1));
    let e = validate(&s, &cfg).unwrap_err();
    assert_eq!(
        (e.code, e.setting.as_str(), e.reason),
        (ConfigErrorCode::ConfigInvalid, "acme.secret", ConfigReason::Undeclared)
    );
    assert_eq!(e.code(), "CONFIG_INVALID");
    // The digest refuses an invalid configuration rather than hashing it.
    assert_eq!(config_digest(&s, &cfg).unwrap_err(), e);

    let mut cfg = base_config();
    cfg.values.insert("exposure".into(), SettingValue::Quantity(Quantity::new(0.5, SiUnit::Second).unwrap()));
    let e = validate(&s, &cfg).unwrap_err();
    assert_eq!((e.code(), e.setting.as_str()), ("OUT_OF_RANGE", "exposure"));

    let mut cfg = base_config();
    cfg.values.insert("exposure".into(), SettingValue::Quantity(Quantity::new(2.5e-4, SiUnit::Metre).unwrap()));
    let e = validate(&s, &cfg).unwrap_err();
    assert_eq!((e.code(), e.reason), ("CONFIG_INVALID", ConfigReason::WrongUnit));

    let mut cfg = base_config();
    cfg.values.insert("averaging".into(), SettingValue::Text("8".into()));
    assert_eq!(validate(&s, &cfg).unwrap_err().reason, ConfigReason::WrongType);

    let mut cfg = base_config();
    cfg.values.remove("trigger_mode");
    let e = validate(&s, &cfg).unwrap_err();
    assert_eq!((e.setting.as_str(), e.reason), ("trigger_mode", ConfigReason::Missing));
}

/// Malformed schemas are refused with CONFIG_INVALID naming the setting.
#[test]
fn invalid_schemas() {
    let all = cases("schema_invalid_cases.json");
    assert!(all.len() >= 15);
    for case in all {
        let name = text(&case, "name");
        let e =
            ConfigSchema::from_value(case["schema"].clone()).err().unwrap_or_else(|| panic!("{name} must be rejected"));
        assert_eq!(
            (e.code(), e.setting.as_str(), e.reason.as_str()),
            ("CONFIG_INVALID", text(&case, "setting").as_str(), text(&case, "reason").as_str()),
            "{name}"
        );
    }
}

/// REQ-001: every setting carries its type, unit, range and both flags.
#[test]
fn schema_is_typed() {
    let s = schema();
    let exposure = s.get("exposure").unwrap();
    assert_eq!(exposure.ty, SettingType::Quantity(SiUnit::Second));
    assert_eq!(exposure.unit(), Some(SiUnit::Second));
    assert_eq!(exposure.range, Some(SettingRange::Quantity { min: 1e-6, max: 0.01 }));
    assert!(exposure.acquisition_relevant && !exposure.live_changeable);
    let name = s.get("display_name").unwrap();
    assert_eq!((name.unit(), name.acquisition_relevant, name.live_changeable), (None, false, true));
    assert!(!name.is_digested());
    assert_eq!(s.get("averaging").unwrap().range, Some(SettingRange::Int { min: 1, max: 64 }));
    assert_eq!(s.settings().count(), 9);

    // The constructor applies the same rules as the reader.
    let dup = vec![exposure.clone(), exposure.clone()];
    let e = ConfigSchema::new(dup).unwrap_err();
    assert_eq!((e.setting.as_str(), e.reason), ("exposure", ConfigReason::Duplicate));
    let hidden_ext = SettingSchema {
        name: "acme.mode".into(),
        ty: SettingType::Bool,
        range: None,
        acquisition_relevant: false,
        live_changeable: false,
    };
    assert_eq!(ConfigSchema::new(vec![hidden_ext]).unwrap_err().reason, ConfigReason::ExtensionFlag);
}

/// The configuration wire form round-trips to the canonical bytes of the fixture.
#[test]
fn configuration_round_trip() {
    let cfg = base_config();
    let bytes = fid_canon::to_canonical_bytes(&cfg.to_value()).unwrap();
    assert_eq!(bytes, fid_canon::canonicalize(&read("linescan_cfg.json")).unwrap());
    assert_eq!(Configuration::from_json(&schema(), &bytes).unwrap(), cfg);
}

/// Malformed JSON and duplicate keys are CONFIG_INVALID for the whole document.
#[test]
fn bad_json() {
    let s = schema();
    for input in [&b"{"[..], b"{\"schema\":\"configuration/0\",\"schema\":\"configuration/0\",\"values\":{}}", b"\xff"]
    {
        let e = Configuration::from_json(&s, input).unwrap_err();
        assert_eq!((e.code(), e.setting.as_str(), e.reason), ("CONFIG_INVALID", "", ConfigReason::Json));
    }
}
