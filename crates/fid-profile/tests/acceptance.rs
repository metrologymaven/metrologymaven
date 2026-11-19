#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! TRUST.PROFILE.DEFS acceptance criteria AC-1 to AC-5 against `fixtures/profile/`. Every
//! expected document, digest and outcome comes from `fixtures/profile/make_fixtures.py` (an
//! independent stdlib Python implementation with the values typed in from the spec) or is
//! hand-labelled there from the spec's rules; none is produced by this crate.

use std::path::PathBuf;

use fid_profile::defs::{PROFILE_MEMBER_PATH, PROFILE_VERSION};
use fid_profile::{
    instantiate, AgentWrites, CanonicalProfile, MeaningCode, ProfileDoc, ProfileError, ProfileName,
    ProgramReleaseByOther, TenantMeaning, TenantParams,
};
use fid_trust_canon::Value;

fn path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/profile").join(rel)
}

fn bytes(rel: &str) -> Vec<u8> {
    std::fs::read(path(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn object(rel: &str) -> Vec<(String, Value)> {
    match fid_trust_canon::parse(&bytes(rel)).unwrap() {
        Value::Object(m) => m,
        other => panic!("{rel}: {other:?}"),
    }
}

fn pinned(key: &str) -> String {
    match object("digests.json").into_iter().find(|(k, _)| k == key) {
        Some((_, Value::String(s))) => s,
        _ => panic!("digests.json lacks {key}"),
    }
}

fn codes(p: &[MeaningCode]) -> Vec<&str> {
    p.iter().map(MeaningCode::as_str).collect()
}

// AC-1 ---------------------------------------------------------------------------------

#[test]
fn ac1_builtins_equal_fixtures_byte_for_byte_and_pinned_digests() {
    for name in ProfileName::ALL {
        let got = instantiate(name, None).unwrap();
        let want = bytes(&format!("builtin/{}.json", name.as_str()));
        assert_eq!(got.bytes(), want.as_slice(), "{name}");
        assert_eq!(got.digest().as_str(), pinned(name.as_str()), "{name}");
        assert_eq!(got, CanonicalProfile::builtin(name).unwrap());
        assert_eq!(got.document().name(), name);
        assert_eq!(got.document().version(), PROFILE_VERSION);
        assert_eq!(got.member_path(), PROFILE_MEMBER_PATH);
    }
}

#[test]
fn ac1_builtin_values_follow_req_003() {
    let reg = ProfileDoc::builtin(ProfileName::Regulated);
    let p = reg.params();
    assert!(p.release_requires_signatures());
    assert_eq!(codes(p.required_meanings()), ["AUTHORED", "APPROVED"]);
    assert!(!p.allow_software_keys());
    assert_eq!(p.program_release_by_other(), ProgramReleaseByOther::Always);
    assert!(p.tenant_meanings().is_empty());

    let assist = ProfileDoc::builtin(ProfileName::AssistiveOnly);
    assert_eq!(assist.params().agent_writes(), AgentWrites::ProposalsOnly);

    let lean = ProfileDoc::builtin(ProfileName::Lean);
    assert!(!lean.params().release_requires_signatures());
    // Open question of the leaf, set to "both": Lean accepts software keys.
    assert!(lean.params().allow_software_keys());
}

#[test]
fn ac1_three_distinct_builtins() {
    let mut digests: Vec<String> =
        ProfileName::ALL.iter().map(|n| instantiate(*n, None).unwrap().digest().as_str().to_string()).collect();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), 3);
}

// AC-2 ---------------------------------------------------------------------------------

#[test]
fn ac2_regulated_with_xqa_succeeds_and_changes_the_digest() {
    let tenant = bytes("regulated-with-xqa.json");
    let got = instantiate(ProfileName::Regulated, Some(&tenant)).unwrap();
    assert_eq!(got.bytes(), bytes("expected/regulated-with-xqa.json").as_slice());
    assert_eq!(got.digest().as_str(), pinned("regulated-with-xqa"));
    assert_ne!(got.digest(), instantiate(ProfileName::Regulated, None).unwrap().digest());
    let p = got.document().params();
    assert_eq!(codes(p.required_meanings()), ["AUTHORED", "X_QA_RELEASE", "APPROVED"]);
    assert_eq!(p.tenant_meanings().len(), 1);
    assert_eq!(p.tenant_meanings()[0].code().as_str(), "X_QA_RELEASE");
    // The non-tenant parameters are untouched.
    assert!(!p.allow_software_keys());
    assert!(p.release_requires_signatures());
}

#[test]
fn tenant_meanings_are_sorted_so_file_order_does_not_matter() {
    for (name, key) in
        [(ProfileName::Lean, "lean-two-meanings"), (ProfileName::AssistiveOnly, "assistive_only-two-meanings")]
    {
        let a = instantiate(name, Some(&bytes("tenant/two-meanings.json"))).unwrap();
        let b = instantiate(name, Some(&bytes("tenant/two-meanings-reordered.json"))).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.bytes(), bytes(&format!("expected/{key}.json")).as_slice());
        assert_eq!(a.digest().as_str(), pinned(key));
    }
}

#[test]
fn empty_tenant_file_is_the_plain_builtin() {
    for name in ProfileName::ALL {
        let got = instantiate(name, Some(&bytes("tenant/empty.json"))).unwrap();
        assert_eq!(got, instantiate(name, None).unwrap());
    }
}

// AC-3 / AC-4 and the full hand-labelled case table --------------------------------------

#[test]
fn ac3_regulated_software_keys_is_forbidden() {
    let e = instantiate(ProfileName::Regulated, Some(&bytes("tenant/bad-software-keys.json"))).unwrap_err();
    assert_eq!(e.code(), "profile.param_forbidden");
    assert_eq!(e, ProfileError::ParamForbidden { key: "allow_software_keys".to_string() });
}

#[test]
fn ac4_regulated_sequence_without_approved_is_invalid() {
    let e = instantiate(ProfileName::Regulated, Some(&bytes("tenant/bad-no-approved.json"))).unwrap_err();
    assert_eq!(e.code(), "profile.param_invalid");
    assert!(matches!(e, ProfileError::ParamInvalid { key: "required_meanings", .. }), "{e}");
}

#[test]
fn case_table_outcomes() {
    let cases = match fid_trust_canon::parse(&bytes("tenant/cases.json")).unwrap() {
        Value::Array(items) => items,
        other => panic!("{other:?}"),
    };
    assert!(cases.len() >= 40, "{}", cases.len());
    for case in cases {
        let Value::Object(m) = case else { panic!("case is not an object") };
        let get = |k: &str| match m.iter().find(|(n, _)| n == k) {
            Some((_, Value::String(s))) => s.clone(),
            _ => panic!("case lacks {k}"),
        };
        let (file, profile, expect) = (get("file"), get("profile"), get("expect"));
        let name = ProfileName::from_wire(&profile).unwrap();
        let rel = file.strip_prefix("../").map(str::to_string).unwrap_or_else(|| format!("tenant/{file}"));
        let got = instantiate(name, Some(&bytes(&rel)));
        let code = match &got {
            Ok(_) => "ok",
            Err(e) => e.code(),
        };
        assert_eq!(code, expect, "{file} under {profile}: {got:?}");
    }
}

#[test]
fn forbidden_keys_are_reported_before_values_are_checked() {
    // `required_meanings` is forbidden on Lean even when its value would also be invalid.
    for body in [&br#"{"required_meanings": 7}"#[..], br#"{"tenant_meanings": "x", "colour": 1}"#] {
        let e = instantiate(ProfileName::Lean, Some(body)).unwrap_err();
        assert_eq!(e.code(), "profile.param_forbidden", "{e}");
    }
}

#[test]
fn hostile_input_is_bounded() {
    let big = vec![b' '; fid_profile::defs::MAX_INPUT_BYTES + 1];
    assert_eq!(instantiate(ProfileName::Lean, Some(&big)).unwrap_err().code(), "profile.malformed");
    let deep = format!("{{\"tenant_meanings\":{}{}}}", "[".repeat(20), "]".repeat(20));
    assert_eq!(instantiate(ProfileName::Lean, Some(deep.as_bytes())).unwrap_err().code(), "profile.malformed");
    for junk in [&b""[..], b"\xff", b"\xef\xbb\xbf{}", b"{} {}", b"null", b"\"x\""] {
        assert_eq!(instantiate(ProfileName::Lean, Some(junk)).unwrap_err().code(), "profile.malformed");
    }
}

#[test]
fn checked_constructors() {
    let qa = TenantMeaning::new("X_QA", "QA", "QA released it.").unwrap();
    let seq = vec![MeaningCode::new("X_QA").unwrap(), MeaningCode::new("APPROVED").unwrap()];
    let t = TenantParams::new(ProfileName::Regulated, vec![qa.clone()], Some(seq.clone())).unwrap();
    assert_eq!(t.required_meanings().unwrap(), seq.as_slice());
    assert_eq!(
        TenantParams::new(ProfileName::Lean, vec![qa.clone()], Some(seq)).unwrap_err().code(),
        "profile.param_forbidden"
    );
    assert_eq!(
        TenantParams::new(ProfileName::Lean, vec![qa.clone(), qa.clone()], None).unwrap_err().code(),
        "profile.param_invalid"
    );
    let unknown = vec![MeaningCode::new("X_OTHER").unwrap(), MeaningCode::new("APPROVED").unwrap()];
    assert_eq!(
        TenantParams::new(ProfileName::Regulated, vec![qa], Some(unknown)).unwrap_err().code(),
        "profile.param_invalid"
    );
}

// AC-5 ---------------------------------------------------------------------------------
// The tests above run on Windows, Linux and macOS CI (and wasm32); each compares against the
// same pinned digests, so equal digests on every platform follow. This adds a repeat check.

#[test]
fn ac5_digest_is_stable_across_repeated_instantiation() {
    let tenant = bytes("regulated-with-xqa.json");
    let first = instantiate(ProfileName::Regulated, Some(&tenant)).unwrap();
    for _ in 0..16 {
        assert_eq!(instantiate(ProfileName::Regulated, Some(&tenant)).unwrap(), first);
    }
}

// Reading a document back (verifier side) --------------------------------------------------

#[test]
fn read_accepts_every_expected_document() {
    for rel in [
        "builtin/lean.json",
        "builtin/regulated.json",
        "builtin/assistive_only.json",
        "expected/regulated-with-xqa.json",
        "expected/lean-two-meanings.json",
        "expected/assistive_only-two-meanings.json",
        "expected/lean-nonascii.json",
    ] {
        let b = bytes(rel);
        let got = CanonicalProfile::read(&b).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(got.bytes(), b.as_slice(), "{rel}");
    }
}

#[test]
fn nonascii_text_passes_through_unescaped() {
    let got = instantiate(ProfileName::Lean, Some(&bytes("tenant/nonascii.json"))).unwrap();
    assert_eq!(got.bytes(), bytes("expected/lean-nonascii.json").as_slice());
    assert_eq!(got.digest().as_str(), pinned("lean-nonascii"));
    assert_eq!(got.document().params().tenant_meanings()[0].label(), "Qualit\u{e4}tsfreigabe \u{2713}");
}

#[test]
fn bidi_and_non_nfc_text_is_invalid() {
    for (rel, profile) in [
        ("tenant/bad-tenant-label-bidi.json", ProfileName::Lean),
        ("tenant/bad-tenant-description-bidi.json", ProfileName::Regulated),
        ("tenant/bad-tenant-label-nfd.json", ProfileName::Lean),
    ] {
        let e = instantiate(profile, Some(&bytes(rel))).unwrap_err();
        assert!(matches!(e, ProfileError::ParamInvalid { key: "tenant_meanings", .. }), "{rel}: {e}");
    }
    // The same rules hold when a document is read back.
    let nfd = String::from_utf8(bytes("expected/lean-nonascii.json")).unwrap().replacen("\u{e4}", "a\u{308}", 1);
    assert_eq!(CanonicalProfile::read(nfd.as_bytes()).unwrap_err().code(), "profile.param_invalid");
    let bidi = String::from_utf8(bytes("expected/lean-nonascii.json")).unwrap().replacen(" \u{2713}", "\u{202e}", 1);
    assert_eq!(CanonicalProfile::read(bidi.as_bytes()).unwrap_err().code(), "profile.param_invalid");
}

#[test]
fn read_reports_a_bad_sequence_code_under_required_meanings() {
    let doc = edited("builtin/regulated.json", "[\"AUTHORED\",\"APPROVED\"]", "[\"authored\",\"APPROVED\"]");
    let e = CanonicalProfile::read(&doc).unwrap_err();
    assert!(matches!(e, ProfileError::ParamInvalid { key: "required_meanings", .. }), "{e}");
}

fn edited(rel: &str, from: &str, to: &str) -> Vec<u8> {
    let s = String::from_utf8(bytes(rel)).unwrap();
    assert!(s.contains(from), "{rel} lacks {from}");
    s.replacen(from, to, 1).into_bytes()
}

#[test]
fn read_fails_closed() {
    let reg = "builtin/regulated.json";
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (edited(reg, "\"allow_software_keys\":false", "\"allow_software_keys\":true"), "profile.param_forbidden"),
        (
            edited(reg, "\"release_requires_signatures\":true", "\"release_requires_signatures\":false"),
            "profile.param_forbidden",
        ),
        (edited(reg, "\"always\"", "\"agent_authored\""), "profile.param_forbidden"),
        (edited(reg, "[\"AUTHORED\",\"APPROVED\"]", "[\"AUTHORED\"]"), "profile.param_invalid"),
        (edited(reg, "[\"AUTHORED\",\"APPROVED\"]", "[\"APPROVED\",\"X_NOPE\"]"), "profile.param_invalid"),
        (
            edited("builtin/lean.json", "\"required_meanings\":[]", "\"required_meanings\":[\"APPROVED\"]"),
            "profile.param_forbidden",
        ),
        (edited("builtin/assistive_only.json", "\"proposals_only\"", "\"branch\""), "profile.param_forbidden"),
        (edited(reg, "\"version\":1", "\"version\":2"), "profile.malformed"),
        (edited(reg, "\"version\":1", "\"version\":1,\"extra\":0"), "profile.malformed"),
        (edited(reg, "\"name\":\"regulated\"", "\"name\":\"strict\""), "profile.malformed"),
        (edited(reg, "{\"name\"", "{ \"name\""), "profile.malformed"),
        (edited(reg, "\"version\":1", "\"version\":1.0"), "profile.malformed"),
        (edited(reg, "\"version\":1}", "\"version\":1}\n"), "profile.malformed"),
        (edited(reg, "\"version\":1", "\"version\":1,\"version\":1"), "profile.malformed"),
        // Tenant meanings out of order are not canonical.
        (edited("expected/lean-two-meanings.json", "X_CUSTOMER_OK", "X_ZZZ"), "profile.malformed"),
    ];
    for (input, want) in cases {
        let got = CanonicalProfile::read(&input);
        let code = got.as_ref().map(|_| "ok").unwrap_or_else(|e| e.code());
        assert_eq!(code, want, "{}: {got:?}", String::from_utf8_lossy(&input));
    }
}
