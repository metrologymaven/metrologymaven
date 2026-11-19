//! The settings error codes mapped into the closed bus error set (CORE.STO.SET Errors,
//! CORE.BUS.PIPE.ERR). The expected table is hand-labelled from the two leaves; every pair must
//! exist in the published catalogue `schema/bus-errors.json`, read here as plain JSON (fid-store
//! sits below fid-bus and does not depend on it).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_store::settings::SettingsError;
use std::path::Path;

fn every_error() -> Vec<SettingsError> {
    let d = || "d".to_string();
    vec![
        SettingsError::AdminSetting(d()),
        SettingsError::Policy(d()),
        SettingsError::ScopeNotAllowed(d()),
        SettingsError::ScopeNotWritable(d()),
        SettingsError::UnknownKey(d()),
        SettingsError::Validation(d()),
        SettingsError::Journal(d()),
        SettingsError::Storage(d()),
    ]
}

#[test]
fn every_settings_code_maps_to_the_hand_labelled_bus_pair() {
    let got: Vec<(&str, (&str, &str))> = every_error().iter().map(|e| (e.code(), e.bus_code())).collect();
    assert_eq!(
        got,
        [
            ("admin_setting", ("policy", "class_forbidden")),
            ("policy", ("policy", "agent_write_forbidden")),
            ("scope_not_allowed", ("validation", "schema")),
            ("scope_not_writable", ("precondition", "capability_missing")),
            ("unknown_key", ("validation", "bad_reference")),
            ("validation", ("validation", "schema")),
            ("journal_unavailable", ("internal", "journal_unavailable")),
            ("storage", ("internal", "storage")),
        ]
    );
}

#[test]
fn every_bus_pair_is_in_the_published_catalogue() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/bus-errors.json");
    let bytes = std::fs::read(&path).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap().trim_end();
    let CValue::Obj(top) = parse(text.as_bytes()).unwrap() else { panic!("catalogue is not an object") };
    let Some(CValue::Obj(codes)) = top.get("codes") else { panic!("catalogue has no codes") };
    for e in every_error() {
        let (code, reason) = e.bus_code();
        let Some(CValue::Arr(reasons)) = codes.get(code) else { panic!("{}: no bus code {code}", e.code()) };
        assert!(reasons.contains(&CValue::Str(reason.to_string())), "{}: no bus reason {code}/{reason}", e.code());
    }
}
