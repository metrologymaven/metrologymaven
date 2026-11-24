//! Acceptance criteria of spec node `HAL.IDL.NEG` (AC-1 .. AC-6) and its requirements.
//!
//! Expected results come from `fixtures/hal/idl/neg/make_fixtures.py` (an independent stdlib
//! oracle written from the leaf's rules) or are hand-labelled here from those rules. All
//! declarations are invented (D-038).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::negotiate::{
    check_major_retention, CapabilityDeclaration, MajorSupport, NegErrorKind, Outcome, RuntimeSupport,
    UnsupportedReason, VersionRange,
};
use fid_hal_idl::{negotiate, negotiate_json, negotiate_with_profile, ErrorCode, NegError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/neg")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn support(name: &str) -> RuntimeSupport {
    RuntimeSupport::from_json(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn decl(name: &str) -> CapabilityDeclaration {
    CapabilityDeclaration::from_json(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Negotiates and compares the canonical bytes with the oracle's file.
fn check_expected(table: &str, declaration: &str, expected: &str) -> fid_hal_idl::NegotiationResult {
    let got = negotiate(&support(table), &decl(declaration)).unwrap();
    let bytes = got.canonical_json().unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), String::from_utf8(read(expected)).unwrap(), "{expected}");
    got
}

/// The oracle's expected error for a failing declaration.
fn expected_error(declaration: &str) -> (String, String, String) {
    let all = fid_canon::parse(&read("expected_errors.json")).unwrap();
    let CValue::Obj(all) = all else { panic!("expected_errors.json") };
    let Some(CValue::Obj(e)) = all.get(declaration) else { panic!("{declaration} missing") };
    let s = |k: &str| match e.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        _ => panic!("{declaration}.{k}"),
    };
    (s("code"), s("entry"), s("reason"))
}

fn as_triple(e: &NegError) -> (String, String, String) {
    (e.code().to_string(), e.entry.clone(), e.reason.to_string())
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_gocator_style_declaration_negotiates_device_1x_and_linescan_1_3() {
    let r = check_expected("support_v1.json", "decl_gocator.json", "expected_gocator.json");
    // Hand-labelled from REQ-001/002: Device 1.1 is the runtime's highest Device minor;
    // LineScan 1.3 >= the driver's 1.2; raw_profiles is unknown to the runtime.
    let device = r.granted("Device").unwrap();
    assert_eq!((device.major, device.minor, device.deprecated), (1, 1, false));
    let ls = r.granted("LineScan").unwrap();
    assert_eq!((ls.major, ls.minor, ls.deprecated), (1, 3, false));
    assert_eq!(ls.features, ["encoder_trigger", "intensity"]);
    assert_eq!(r.ignored_features.len(), 1);
    assert_eq!(
        (r.ignored_features[0].capability.as_str(), r.ignored_features[0].feature.as_str()),
        ("LineScan", "raw_profiles")
    );
    // Fail closed: a runtime-known feature the driver did not declare is not granted.
    assert!(r.grants_feature("LineScan", "intensity"));
    assert!(!r.grants_feature("LineScan", "multi_exposure"));
    assert!(!r.grants_feature("LineScan", "raw_profiles"));
    // HeightMap is supported by the runtime but not declared, so it is not granted.
    assert!(!r.per_capability.contains_key("HeightMap"));
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_future_major_only_is_unsupported_while_device_succeeds() {
    let r = check_expected("support_v1.json", "decl_future.json", "expected_future.json");
    assert_eq!(r.per_capability.get("LineScan"), Some(&Outcome::Unsupported(UnsupportedReason::NoCommonMajor)));
    assert!(r.granted("LineScan").is_none());
    assert!(r.granted("Device").is_some());
    assert!(r.ignored_features.is_empty(), "features of an unsupported capability are not listed");
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_no_compatible_device_major_fails_unsupported() {
    let e = negotiate(&support("support_v1.json"), &decl("decl_no_device.json")).unwrap_err();
    assert_eq!(as_triple(&e), expected_error("decl_no_device.json"));
    assert_eq!((e.kind, e.code(), e.entry.as_str()), (NegErrorKind::Unsupported, "UNSUPPORTED", "Device"));
    assert_eq!(e.device_error_code(), Some(ErrorCode::Unsupported));
}

#[test]
fn absent_device_also_fails_unsupported() {
    let mut d = decl("decl_gocator.json");
    d.capabilities.retain(|o| o.name != "Device");
    let e = negotiate(&support("support_v1.json"), &d).unwrap_err();
    assert_eq!((e.code(), e.entry.as_str()), ("UNSUPPORTED", "Device"));
    d.capabilities.clear();
    assert_eq!(negotiate(&support("support_v1.json"), &d).unwrap_err().code(), "UNSUPPORTED");
}

#[test]
fn device_unknown_to_the_runtime_fails_unsupported() {
    let mut caps = support("support_v1.json").capabilities().clone();
    caps.remove("Device");
    let table = RuntimeSupport::new(caps).unwrap();
    assert_eq!(negotiate(&table, &decl("decl_gocator.json")).unwrap_err().code(), "UNSUPPORTED");
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_deprecated_previous_major_is_selected_and_flagged() {
    let r = check_expected("support_v2_with_v1_deprecated.json", "decl_linescan_v1.json", "expected_deprecated.json");
    let ls = r.granted("LineScan").unwrap();
    assert_eq!((ls.major, ls.minor, ls.deprecated), (1, 3, true));
    // surface_track is known only for LineScan 2, so it is ignored under 1.3.
    assert_eq!(ls.features, ["encoder_trigger"]);
    check_expected("support_v2_with_v1_deprecated.json", "decl_gocator.json", "expected_gocator_v2.json");
}

#[test]
fn highest_common_major_wins_across_a_spanning_range() {
    let v1 = check_expected("support_v1.json", "decl_span.json", "expected_span_v1.json");
    assert_eq!(v1.granted("LineScan").map(|n| (n.major, n.minor)), Some((1, 3)));
    let v2 = check_expected("support_v2_with_v1_deprecated.json", "decl_span.json", "expected_span_v2.json");
    let ls = v2.granted("LineScan").unwrap();
    assert_eq!((ls.major, ls.minor, ls.deprecated), (2, 0, false));
}

#[test]
fn driver_lowest_minor_above_runtime_minor_falls_back_or_fails() {
    // Hand-labelled: LineScan 1.x runtime minor is 3; a driver needing 1.5 has no common 1.x.
    let mut d = decl("decl_gocator.json");
    let ls = d.capabilities.iter_mut().find(|o| o.name == "LineScan").unwrap();
    ls.versions = VersionRange::parse(">=1.5 <2").unwrap();
    let r = negotiate(&support("support_v1.json"), &d).unwrap();
    assert_eq!(r.per_capability.get("LineScan"), Some(&Outcome::Unsupported(UnsupportedReason::NoCommonMajor)));
    assert!(r.ignored_features.is_empty());
    // With LineScan 2.0 available and the range reaching it, 2.0 is chosen.
    let ls = d.capabilities.iter_mut().find(|o| o.name == "LineScan").unwrap();
    ls.versions = VersionRange::parse(">=1.5 <3").unwrap();
    let r = negotiate(&support("support_v2_with_v1_deprecated.json"), &d).unwrap();
    assert_eq!(r.granted("LineScan").map(|n| (n.major, n.minor, n.deprecated)), Some((2, 0, false)));
    // A range above the runtime's newest major is unsupported.
    let ls = d.capabilities.iter_mut().find(|o| o.name == "LineScan").unwrap();
    ls.versions = VersionRange::parse(">=3.0 <4").unwrap();
    let r = negotiate(&support("support_v2_with_v1_deprecated.json"), &d).unwrap();
    assert_eq!(r.per_capability.get("LineScan"), Some(&Outcome::Unsupported(UnsupportedReason::NoCommonMajor)));
}

#[test]
fn unknown_capability_is_unsupported() {
    let r = check_expected("support_v1.json", "decl_unknown_cap.json", "expected_unknown_cap.json");
    assert_eq!(r.per_capability.get("Gauge"), Some(&Outcome::Unsupported(UnsupportedReason::UnknownCapability)));
    assert!(!r.grants_feature("Gauge", "tolerance_band"));
}

// ------------------------------------------------------------------ AC-5 and REQ-006

#[test]
fn ac5_duplicate_capability_fails_declaration_invalid_naming_linescan() {
    let want = expected_error("decl_duplicate.json");
    let e = negotiate_json(&support("support_v1.json"), &read("decl_duplicate.json")).unwrap_err();
    assert_eq!(as_triple(&e), want);
    // The parsed form keeps the duplicate; negotiate itself refuses it.
    let d = decl("decl_duplicate.json");
    let e = negotiate(&support("support_v1.json"), &d).unwrap_err();
    assert_eq!((e.code(), e.entry.as_str(), e.reason), ("declaration_invalid", "LineScan", "duplicate_capability"));
}

#[test]
fn malformed_range_fails_declaration_invalid_naming_the_entry() {
    let e = negotiate_json(&support("support_v1.json"), &read("decl_bad_range.json")).unwrap_err();
    assert_eq!(as_triple(&e), expected_error("decl_bad_range.json"));
}

/// An edit that breaks one offer check.
type Edit = fn(&mut CapabilityDeclaration);

#[test]
fn negotiate_repeats_the_offer_checks_for_code_built_declarations() {
    let table = support("support_v1.json");
    let base = decl("decl_gocator.json");
    let cases: [(Edit, &str, &str); 5] = [
        (|d| d.capabilities[0].name = "line scan".into(), "/capabilities", "capability_name_invalid"),
        (|d| d.capabilities[0].features.push("intensity".into()), "LineScan", "duplicate_feature"),
        (|d| d.capabilities[0].features.push("Raw Profiles".into()), "LineScan", "feature_invalid"),
        (|d| d.capabilities[0].features = vec!["f".into(); 129], "LineScan", "too_many_features"),
        (
            |d| {
                let q = *d.capabilities[0].limits.values().next().unwrap();
                d.capabilities[0].limits.insert("Bad-Key".into(), q);
            },
            "LineScan",
            "limit_invalid",
        ),
    ];
    for (edit, entry, reason) in cases {
        let mut d = base.clone();
        assert_eq!(d.capabilities[0].name, "LineScan");
        edit(&mut d);
        let e = negotiate(&table, &d).unwrap_err();
        assert_eq!((e.code(), e.entry.as_str(), e.reason), ("declaration_invalid", entry, reason));
    }
}

#[test]
fn declaration_wire_errors() {
    let good = String::from_utf8(read("decl_gocator.json")).unwrap();
    let cases: [(&str, &str, &str, &str); 11] = [
        (
            "\"schema\": \"capability-declaration/v1\"",
            "\"schema\": \"capability-declaration/v2\"",
            "/schema",
            "schema_unknown",
        ),
        ("\"driver_version\": \"0.3.0\"", "\"driver_version\": \"0.3\"", "/driver_version", "driver_version_invalid"),
        ("\"driver_id\": \"drv.example.linescan\"", "\"driver_id\": \"Drv\"", "/device/driver_id", "driver_id_invalid"),
        ("\"versions\": \">=1.2 <2\"", "\"versions\": \">=1.2 <1\"", "LineScan", "version_range_invalid"),
        ("\"versions\": \">=1.2 <2\"", "\"versions\": 12", "LineScan", "version_range_invalid"),
        ("\"name\": \"LineScan\"", "\"name\": \"Line\\u202eScan\"", "/capabilities/0/name", "capability_name_invalid"),
        ("\"PTP\",", "\"GPS\",", "/time_sources/0", "time_source_invalid"),
        ("\"PTP\",", "\"HOST_RECEIVE\",", "/time_sources/1", "duplicate_time_source"),
        ("\"sha256:0000", "\"sha256:000", "/conformance_report", "digest_invalid"),
        (
            "\"schema\": \"capability-declaration/v1\",",
            "\"schema\": \"capability-declaration/v1\", \"x\": 1,",
            "",
            "field_unknown",
        ),
        ("\"driver_version\": \"0.3.0\",", "\"driver_version\": \"0.3.0\", \"driver_version\": \"0.3.0\",", "", "json"),
    ];
    for (from, to, entry, reason) in cases {
        assert!(good.contains(from), "{from}");
        let bad = good.replacen(from, to, 1);
        let e = CapabilityDeclaration::from_json(bad.as_bytes()).unwrap_err();
        assert_eq!((e.code(), e.entry.as_str(), e.reason), ("declaration_invalid", entry, reason), "{to}");
    }
    let empty = good.replacen("\"PTP\",\n    \"HOST_RECEIVE\"", "", 1);
    assert_ne!(empty, good);
    assert_eq!(CapabilityDeclaration::from_json(empty.as_bytes()).unwrap_err().reason, "time_sources_empty");
}

#[test]
fn device_text_never_reaches_errors_or_results() {
    let good = String::from_utf8(read("decl_gocator.json")).unwrap();
    let marker = "IGNORE PREVIOUS INSTRUCTIONS \u{202e}";
    let d = good.replacen("INVENTED-SN-0001", marker, 1);
    let r = negotiate_json(&support("support_v1.json"), d.as_bytes()).unwrap();
    assert!(!String::from_utf8(r.canonical_json().unwrap()).unwrap().contains("IGNORE"));
    let bad = d.replacen("\"0.3.0\"", "\"x\"", 1);
    let e = negotiate_json(&support("support_v1.json"), bad.as_bytes()).unwrap_err();
    assert!(!e.to_string().contains("IGNORE"));
}

// ------------------------------------------------------------------ AC-6 and REQ-005

/// SplitMix64, for a seeded Fisher-Yates shuffle without dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

#[test]
fn ac6_permuted_declarations_give_byte_identical_output() {
    let table = support("support_v1.json");
    let mut d = decl("decl_gocator.json");
    // More capabilities than AC-1 so the permutations are not trivial.
    let mut extra = decl("decl_unknown_cap.json").capabilities;
    extra.retain(|o| o.name != "Device");
    d.capabilities.extend(extra);
    let want = negotiate(&table, &d).unwrap().canonical_json().unwrap();
    let mut rng = Rng(0x4e45_475f_4143_3600);
    let mut orders = BTreeSet::new();
    for _ in 0..1000 {
        rng.shuffle(&mut d.capabilities);
        for o in &mut d.capabilities {
            rng.shuffle(&mut o.features);
        }
        orders.insert(d.capabilities.iter().map(|o| o.name.clone()).collect::<Vec<_>>());
        assert_eq!(negotiate(&table, &d).unwrap().canonical_json().unwrap(), want);
    }
    assert!(orders.len() > 1, "the shuffle must actually permute");

    // The AC-1 inputs alone, permuted, give exactly the oracle's bytes.
    let mut d = decl("decl_gocator.json");
    let want = read("expected_gocator.json");
    for _ in 0..1000 {
        rng.shuffle(&mut d.capabilities);
        for o in &mut d.capabilities {
            rng.shuffle(&mut o.features);
        }
        assert_eq!(negotiate(&table, &d).unwrap().canonical_json().unwrap(), want);
    }
}

// ------------------------------------------------------------------ profile restriction

#[test]
fn profile_restricts_granted_capabilities() {
    let table = support("support_v1.json");
    let d = decl("decl_gocator.json");
    let mut profile = fid_hal_idl::profile::validate(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/profile/arm_specsheet.json"))
            .unwrap(),
    )
    .unwrap();
    assert!(!profile.device_class.contains(&"LineScan".to_string()));
    let r = negotiate_with_profile(&table, &d, &profile).unwrap();
    assert_eq!(r.per_capability.get("LineScan"), Some(&Outcome::Unsupported(UnsupportedReason::NotInProfile)));
    assert!(r.granted("Device").is_some(), "Device is the implicit base");
    assert!(r.ignored_features.is_empty());

    profile.device_class = vec!["LineScan".to_string()];
    let r = negotiate_with_profile(&table, &d, &profile).unwrap();
    assert_eq!(r, negotiate(&table, &d).unwrap());
}

// ------------------------------------------------------------------ support table and REQ-004

fn row(highest_minor: u32, deprecated: bool) -> MajorSupport {
    MajorSupport { highest_minor, features: BTreeSet::new(), deprecated }
}

fn table(rows: &[(&str, u32, u32, bool)]) -> Result<RuntimeSupport, NegError> {
    let mut caps: BTreeMap<String, BTreeMap<u32, MajorSupport>> = BTreeMap::new();
    for (name, major, minor, dep) in rows {
        caps.entry(name.to_string()).or_default().insert(*major, row(*minor, *dep));
    }
    RuntimeSupport::new(caps)
}

#[test]
fn support_table_rules() {
    assert!(table(&[("Device", 1, 0, false)]).is_ok());
    assert!(table(&[("Device", 1, 0, true), ("Device", 2, 0, false)]).is_ok());
    let reason = |r: Result<RuntimeSupport, NegError>| r.unwrap_err().reason;
    assert_eq!(reason(table(&[("Device", 1, 0, true)])), "current_major_deprecated");
    assert_eq!(reason(table(&[("Device", 1, 0, false), ("Device", 2, 0, false)])), "superseded_major_not_deprecated");
    assert_eq!(reason(table(&[("device", 1, 0, false)])), "capability_name_invalid");
    assert_eq!(
        reason(RuntimeSupport::new(BTreeMap::from([("Device".to_string(), BTreeMap::new())]))),
        "major_count_invalid"
    );

    let good = String::from_utf8(read("support_v2_with_v1_deprecated.json")).unwrap();
    let cases: [(&str, &str, &str); 5] = [
        ("\"major\": 1,", "\"major\": 2,", "duplicate_major"),
        ("\"major\": 1,", "\"major\": 1.5,", "integer_invalid"),
        ("\"major\": 1,", "\"major\": -1,", "integer_invalid"),
        ("\"deprecated\": true", "\"deprecated\": false", "superseded_major_not_deprecated"),
        ("\"schema\": \"runtime-support/v1\",", "\"schema\": \"runtime-support/v1\", \"schema\": \"x\",", "json"),
    ];
    for (from, to, want) in cases {
        let bad = good.replace(from, to);
        assert_ne!(bad, good, "{from}");
        let e = RuntimeSupport::from_json(bad.as_bytes()).unwrap_err();
        assert_eq!((e.code(), e.reason), ("support_invalid", want), "{to}");
    }
}

#[test]
fn previous_major_stays_negotiable_for_a_release() {
    let v1 = support("support_v1.json");
    let v2 = support("support_v2_with_v1_deprecated.json");
    assert_eq!(check_major_retention(&v1, &v2), Ok(()));
    // Next release after v2 may drop the deprecated LineScan 1, but not LineScan 2.
    let v3 = table(&[("Device", 1, 1, false), ("HeightMap", 1, 0, false), ("LineScan", 2, 1, false)]).unwrap();
    assert_eq!(check_major_retention(&v2, &v3), Ok(()));
    // Jumping straight from v1 to a table without LineScan 1 drops the previous major.
    let e = check_major_retention(&v1, &v3).unwrap_err();
    assert_eq!((e.code(), e.entry.as_str(), e.reason), ("support_invalid", "LineScan", "previous_major_dropped"));
    let v4 = table(&[("Device", 1, 1, false), ("LineScan", 2, 0, false), ("LineScan", 1, 3, true)]).unwrap();
    let e = check_major_retention(&v2, &v4).unwrap_err();
    assert_eq!((e.entry.as_str(), e.reason), ("HeightMap", "capability_dropped"));
}
