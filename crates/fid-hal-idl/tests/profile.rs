//! Acceptance criteria of spec node HAL.IDL.PROFILE (AC-1 .. AC-6). Expected digests and
//! canonical bytes come from `fixtures/hal/idl/profile/make_fixtures.py` (an independent
//! stdlib Python reference); expected error codes and pointers are hand-labelled there from
//! the leaf's rules. All profiles are invented (D-038).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::profile::{
    digest_json, validate, validate_value, InstrumentProfile, ProfileErrorKind, SourceKind, MAX_UNTRUSTED_TEXT_BYTES,
};
use fid_hal_idl::{FrameKind, SiUnit};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/profile")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn expected_digest(name: &str) -> String {
    String::from_utf8(read(name)).unwrap().trim_end().to_string()
}

fn code_of(input: &[u8]) -> (&'static str, String) {
    let e = validate(input).err().expect("must be rejected");
    (e.code(), e.pointer)
}

fn obj_mut(v: &mut CValue) -> &mut std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("expected an object"),
    }
}

/// AC-1: the spec-sheet arm profile validates and its digest is the reference digest; this test
/// runs on the Linux, Windows and macOS runners.
#[test]
fn ac1_arm_specsheet_digest() {
    let input = read("arm_specsheet.json");
    let p = validate(&input).unwrap();
    assert_eq!(p.source.kind, SourceKind::SpecSheet);
    assert_eq!(p.uec_max, 1);
    assert_eq!(p.profile_id.as_str(), "arm-demo-01@1.0.0");
    assert_eq!(p.device_class, ["PortablePose", "TactileProbe"]);
    assert_eq!(p.model.as_display(), "DemoArm 7 \u{b5} \u{3a9}");
    assert_eq!(p.mpe_terms.len(), 3);
    assert_eq!(p.mpe_terms[0].symbol, "E_UNI");
    assert_eq!(p.mpe_terms[0].standard.designator, "ISO 10360-12");
    assert_eq!(p.mpe_terms[0].value.unit(), SiUnit::Metre);
    assert_eq!(p.mpe_terms[0].value.value(), 45e-6);
    assert_eq!(p.mpe_terms[0].contributor.as_str(), "uc:arm-length-error@1.0.0");
    assert_eq!(
        p.frame_chain.iter().map(|f| f.kind).collect::<Vec<_>>(),
        [FrameKind::Sensor, FrameKind::Mount, FrameKind::Device]
    );
    assert_eq!(p.um_model.id(), "im:arm-demo-01@1.0.0");

    let expected = expected_digest("arm_specsheet.sha256");
    assert_eq!(p.digest().unwrap().as_str(), expected);
    assert_eq!(digest_json(&input).unwrap().as_str(), expected);
}

/// REQ-006 (D0): canonical bytes, with and without the signature, equal the reference bytes.
#[test]
fn canonical_bytes_match_reference() {
    let input = read("arm_specsheet.json");
    let p = validate(&input).unwrap();
    assert_eq!(p.canonical_bytes().unwrap(), read("arm_specsheet.canonical.json"));
    assert_eq!(p.canonical_bytes().unwrap(), fid_canon::canonicalize(&input).unwrap());
    let unsigned = p.unsigned_canonical_bytes().unwrap();
    assert_eq!(unsigned, read("arm_specsheet.unsigned.json"));
    assert_eq!(fid_canon::digest_bytes(&unsigned).to_string(), expected_digest("arm_specsheet.sha256"));
    // The canonical form validates to the same profile.
    assert_eq!(validate(&p.canonical_bytes().unwrap()).unwrap(), p);
}

/// AC-2: a spec-sheet profile claiming UEC-2 is refused (REQ-004, D-045).
#[test]
fn ac2_specsheet_uec2_refused() {
    let (code, ptr) = code_of(&read("arm_specsheet_uec2.json"));
    assert_eq!((code, ptr.as_str()), ("profile_uec_exceeds_source", "/uec_max"));
}

/// AC-3: temperature 25 °C .. 15 °C (in kelvin) is refused (REQ-005).
#[test]
fn ac3_envelope_reversed() {
    let (code, ptr) = code_of(&read("envelope_reversed.json"));
    assert_eq!((code, ptr.as_str()), ("profile_envelope_invalid", "/validity/temperature"));
}

/// AC-4: an MPE term in `um` is refused with the HAL.IDL.UNITS code.
#[test]
fn ac4_mpe_unit_um() {
    let (code, ptr) = code_of(&read("mpe_unit_um.json"));
    assert_eq!((code, ptr.as_str()), ("unit_unsupported", "/mpe_terms/0/value/unit"));
}

/// AC-5: changing only the signature block leaves the digest unchanged.
#[test]
fn ac5_signature_excluded_from_digest() {
    let a = validate(&read("cmm_iemps.json")).unwrap();
    let b = validate(&read("cmm_iemps_resigned.json")).unwrap();
    assert_ne!(a.signature, b.signature);
    assert_ne!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    let expected = expected_digest("cmm_iemps.sha256");
    assert_eq!(a.digest().unwrap().as_str(), expected);
    assert_eq!(b.digest().unwrap().as_str(), expected);
    assert_eq!(a.source.kind, SourceKind::Iemps);
    assert_eq!(a.uec_max, 2);
}

/// AC-6: a 70 000-byte `make` is `untrusted_text_too_long` (limit 4 096 bytes).
#[test]
fn ac6_make_too_long() {
    let mut v = fid_canon::parse(&read("arm_specsheet.json")).unwrap();
    obj_mut(&mut v).insert("make".into(), CValue::Str("x".repeat(70_000)));
    let bytes = fid_canon::to_canonical_bytes(&v).unwrap();
    assert!(bytes.len() > 70_000);
    let (code, ptr) = code_of(&bytes);
    assert_eq!((code, ptr.as_str()), ("untrusted_text_too_long", "/make"));
    assert_eq!(MAX_UNTRUSTED_TEXT_BYTES, 4096);
}

/// Other accepted profiles: each validates, its digest is the reference digest, and its
/// canonical bytes are those of the input.
#[test]
fn valid_cases() {
    let CValue::Arr(cases) = fid_canon::parse(&read("valid_cases.json")).unwrap() else { panic!() };
    assert!(cases.len() >= 15);
    for case in cases {
        let CValue::Obj(mut c) = case else { panic!() };
        let Some(CValue::Str(name)) = c.remove("case") else { panic!() };
        let Some(CValue::Str(digest)) = c.remove("digest") else { panic!() };
        let profile = c.remove("profile").unwrap();
        let input = fid_canon::to_canonical_bytes(&profile).unwrap();
        let p = validate(&input).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.digest().unwrap().as_str(), digest, "{name}");
        assert_eq!(p.canonical_bytes().unwrap(), input, "{name}");
    }
}

/// Other rejected profiles, with the code and pointer hand-labelled from the spec.
#[test]
fn invalid_cases() {
    let CValue::Arr(cases) = fid_canon::parse(&read("invalid_cases.json")).unwrap() else { panic!() };
    assert!(cases.len() >= 60);
    let mut seen = std::collections::BTreeSet::new();
    for case in cases {
        let CValue::Obj(mut c) = case else { panic!() };
        let Some(CValue::Str(name)) = c.remove("case") else { panic!() };
        let Some(CValue::Str(code)) = c.remove("code") else { panic!() };
        let Some(CValue::Str(pointer)) = c.remove("pointer") else { panic!() };
        let profile = c.remove("profile").unwrap();
        let e = validate_value(profile).err().unwrap_or_else(|| panic!("{name} must be rejected"));
        assert_eq!((e.code(), e.pointer.as_str()), (code.as_str(), pointer.as_str()), "{name}");
        seen.insert(e.kind);
    }
    // Every code except the JSON one (tested below) is exercised by a fixture.
    for k in ProfileErrorKind::ALL {
        assert!(k == ProfileErrorKind::JsonInvalid || seen.contains(&k), "{k:?} not covered");
    }
}

/// Bytes `fid-canon` refuses: duplicate keys and invalid UTF-8.
#[test]
fn json_errors() {
    assert_eq!(code_of(b"{\"schema\":\"instrument-profile/v1\",\"schema\":\"x\"}").0, "profile_json_invalid");
    assert_eq!(code_of(b"{\"make\":\"\xff\"}").0, "profile_json_invalid");
    assert_eq!(code_of(b"").0, "profile_json_invalid");
}

/// A profile edited in code is re-validated before it is hashed or written.
#[test]
fn edited_profile_is_revalidated() {
    let mut p: InstrumentProfile = validate(&read("arm_specsheet.json")).unwrap();
    p.uec_max = 2;
    assert_eq!(p.digest().unwrap_err().code(), "profile_uec_exceeds_source");
    p.uec_max = 1;
    p.device_class.reverse();
    assert_eq!(p.canonical_bytes().unwrap_err().pointer, "/device_class");
}

/// D-045: a spec-sheet profile is declared-only and can never reach UEC-2 or above, so its
/// figures can never be presented as characterized.
#[test]
fn specsheet_is_declared_only() {
    let p = validate(&read("arm_specsheet.json")).unwrap();
    assert!(p.source.kind.is_declared());
    assert_eq!(p.source.kind.uec_cap(), 1);
    let c = validate(&read("cmm_iemps.json")).unwrap();
    assert!(!c.source.kind.is_declared());
    for uec in 2..=3u8 {
        let mut v = fid_canon::parse(&read("arm_specsheet.json")).unwrap();
        obj_mut(&mut v).insert("uec_max".into(), CValue::Num(f64::from(uec)));
        assert_eq!(validate_value(v).unwrap_err().code(), "profile_uec_exceeds_source");
    }
}
