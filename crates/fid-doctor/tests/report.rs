//! Acceptance criteria of spec node PLAT.DOCTOR.REPORT (AC-1 .. AC-6), the independent Python
//! oracle's cases (`fixtures/plat/doctor/report/`, written by `make_report.py`) and edge cases.
//! Expected values come from the spec text, hand labels or the oracle, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_doctor::lookup::assess;
use fid_doctor::probe::{Arch, EnvSnapshot, Probed, UnknownReason};
use fid_doctor::report::{
    fold_char, quote_text, DoctorReport, Identity, IdentityPlatform, Redactor, ReportError, MAX_REPORT_BYTES,
    PLACEHOLDER_HOST, PLACEHOLDER_PATH, PLACEHOLDER_USER, SCHEMA_PATH,
};
use fid_platform_core::hw_profile;
use fid_platform_core::matrix::{self, Phase};
use std::ffi::OsString;
use std::path::PathBuf;

const INSTALL_WIN: &str = "C:\\Program Files\\Metrology Maven";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn read_text(rel: &str) -> String {
    String::from_utf8(read(rel)).unwrap()
}

fn json(rel: &str) -> CValue {
    fid_canon::parse(&read(rel)).unwrap()
}

fn obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected an object, got {other:?}"),
    }
}

fn s<'a>(v: &'a CValue, key: &str) -> &'a str {
    match obj(v).get(key) {
        Some(CValue::Str(x)) => x,
        other => panic!("{key}: expected a string, got {other:?}"),
    }
}

fn strings(v: &CValue, key: &str) -> Vec<String> {
    match obj(v).get(key) {
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|x| match x {
                CValue::Str(t) => t.clone(),
                other => panic!("{key}: expected strings, got {other:?}"),
            })
            .collect(),
        other => panic!("{key}: expected an array, got {other:?}"),
    }
}

fn identity_of(v: &CValue) -> Identity {
    Identity {
        user: s(v, "user").to_string(),
        hosts: strings(v, "hosts"),
        home: s(v, "home").to_string(),
        user_dirs: strings(v, "user_dirs"),
        domains: strings(v, "domains"),
    }
}

fn simple_identity(user: &str, host: &str, home: &str) -> Identity {
    Identity {
        user: user.into(),
        hosts: vec![host.into()],
        home: home.into(),
        user_dirs: Vec::new(),
        domains: Vec::new(),
    }
}

fn id_win() -> Identity {
    simple_identity("jdoe", "WS-QA-17.lab.example", "C:\\Users\\jdoe")
}

/// LOOKUP AC-1's inputs, assessed, in a new report.
fn win11_report() -> DoctorReport {
    let snap = EnvSnapshot::from_json(&read("fixtures/plat/doctor/env-win11-discrete.json")).unwrap();
    let m = matrix::parse(&read("fixtures/plat/matrix/valid.toml")).unwrap();
    let p = hw_profile::parse(&read("fixtures/plat/hw/profiles-valid.toml")).unwrap();
    let a = assess(&snap, &m, Some(&p), Phase::P1a);
    DoctorReport::new("0.1.0", Phase::P1a, snap, a).unwrap()
}

/// The rendered form of one JSON leaf, by the documented text rules (computed here, not by
/// the crate): strings quoted with `"` and `\` escaped (the fixtures' leaves hold no control
/// or format characters), numbers in ECMAScript form, literals as in JSON.
fn leaf_forms(v: &CValue, out: &mut Vec<String>) {
    match v {
        CValue::Null => out.push("null".into()),
        CValue::Bool(b) => out.push(b.to_string()),
        CValue::Str(x) => {
            assert!(!x.chars().any(char::is_control), "fixture leaf with a control character");
            out.push(format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\"")));
        }
        CValue::Num(n) => out.push(fid_canon::format_number(*n).unwrap()),
        CValue::Arr(a) => a.iter().for_each(|x| leaf_forms(x, out)),
        CValue::Obj(m) => m.values().for_each(|x| leaf_forms(x, out)),
        other => panic!("unexpected value {other:?}"),
    }
}

fn assert_every_leaf_in_text(report: &DoctorReport, text: &str) {
    let mut leaves = Vec::new();
    leaf_forms(&report.to_cvalue(), &mut leaves);
    for leaf in leaves {
        assert!(text.contains(&leaf), "leaf {leaf} missing from the text");
    }
}

/// AC-1: rendered to JSON, the report validates (schema checked by `check_report_schema.py`)
/// and equals the golden file byte for byte.
#[test]
fn ac1_json_equals_golden() {
    let golden = read("fixtures/plat/doctor/report-win11.json");
    let r = win11_report().redact(&Redactor::new(&id_win(), INSTALL_WIN).unwrap()).unwrap();
    assert!(r.redacted);
    assert_eq!(String::from_utf8(r.to_json().unwrap()).unwrap(), String::from_utf8(golden.clone()).unwrap());
    let parsed = DoctorReport::parse(&golden).unwrap();
    assert_eq!(parsed, r);
    assert_eq!(parsed.to_json().unwrap(), golden);
    // The schema file is valid JSON and names its version.
    let schema = json(SCHEMA_PATH);
    assert_eq!(s(&schema, "$id"), "urn:metrology-maven:doctor-report:1");
}

/// AC-2: the text view equals the golden text and every JSON leaf value appears in it.
#[test]
fn ac2_text_equals_golden_and_holds_every_leaf() {
    let r = DoctorReport::parse(&read("fixtures/plat/doctor/report-win11.json")).unwrap();
    let text = r.to_text().unwrap();
    assert_eq!(text, read_text("fixtures/plat/doctor/report-win11.txt"));
    assert_every_leaf_in_text(&r, &text);
    for section in ["\nPlatform\n", "\nHardware\n", "\nGPU\n", "\nVerdicts\n", "\nUnknowns\n"] {
        assert!(text.contains(section), "section {section:?}");
    }
}

/// Every PLAT.DOCTOR.LOOKUP oracle case as a report: parse, canonical round trip, the oracle's
/// text, every leaf in the text.
#[test]
fn oracle_reports_round_trip_and_render() {
    let cases = json("fixtures/plat/doctor/report/cases.json");
    let CValue::Arr(cases) = cases else { panic!("cases.json must be an array") };
    assert!(cases.len() >= 50, "{} cases", cases.len());
    for case in &cases {
        let name = s(case, "name");
        let bytes = fid_canon::to_canonical_bytes(obj(case).get("report").unwrap()).unwrap();
        let r = DoctorReport::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.to_json().unwrap(), bytes, "{name}: JSON round trip");
        let text = r.to_text().unwrap();
        assert_eq!(text, s(case, "text"), "{name}: text");
        assert_every_leaf_in_text(&r, &text);
    }
}

/// AC-3: redaction removes the user name, host name and home path and shows placeholders.
#[test]
fn ac3_redaction_removes_identifying_strings() {
    let cases = json("fixtures/plat/doctor/report/redact_cases.json");
    let ident = obj(&cases).get("identifying").unwrap();
    let redactor = Redactor::new(&identity_of(obj(ident).get("identity").unwrap()), s(ident, "install_dir")).unwrap();
    let input = DoctorReport::parse(&read("fixtures/plat/doctor/report/identifying.json")).unwrap();
    assert!(!input.redacted);
    let red = input.redact(&redactor).unwrap();
    assert!(red.redacted);
    let golden = read("fixtures/plat/doctor/report/identifying.redacted.json");
    assert_eq!(String::from_utf8(red.to_json().unwrap()).unwrap(), String::from_utf8(golden).unwrap());
    let text = red.to_text().unwrap();
    assert_eq!(text, read_text("fixtures/plat/doctor/report/identifying.redacted.txt"));
    let json_text = String::from_utf8(red.to_json().unwrap()).unwrap().replace("\\\\", "\\");
    for view in [json_text.to_ascii_lowercase(), text.replace("\\\\", "\\").to_ascii_lowercase()] {
        for gone in ["jdoe", "ws-qa-17", "lab.example", "c:\\users\\jdoe\\data"] {
            assert!(!view.contains(gone), "{gone} remains");
        }
        for mark in [PLACEHOLDER_USER, PLACEHOLDER_HOST, PLACEHOLDER_PATH] {
            assert!(view.contains(mark), "{mark} missing");
        }
    }
    // The input did hold them.
    let raw = read_text("fixtures/plat/doctor/report/identifying.json").to_ascii_lowercase();
    assert!(raw.contains("jdoe") && raw.contains("ws-qa-17"));
}

/// AC-3, string level: the oracle's hand-labelled redaction cases.
#[test]
fn ac3_redaction_string_cases() {
    let cases = json("fixtures/plat/doctor/report/redact_cases.json");
    let CValue::Arr(strings) = obj(&cases).get("strings").unwrap() else { panic!() };
    assert!(strings.len() >= 20);
    for c in strings {
        let r = Redactor::new(&identity_of(obj(c).get("identity").unwrap()), s(c, "install_dir")).unwrap();
        assert_eq!(r.redact_text(s(c, "input")), s(c, "expected"), "case {}", s(c, "name"));
    }
    let CValue::Arr(errors) = obj(&cases).get("errors").unwrap() else { panic!() };
    for c in errors {
        let name = s(c, "name");
        let err = Redactor::new(&identity_of(obj(c).get("identity").unwrap()), s(c, "install_dir")).unwrap_err();
        assert_eq!(err.code(), s(c, "code"), "{name}");
        let detail = match &err {
            ReportError::IdentityUnavailable { item } => item.to_string(),
            ReportError::InvalidInstallDir { detail } => detail.clone(),
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(detail, s(c, "detail"), "{name}");
    }
}

/// AC-4: with `include_identifying` the strings are unchanged and `redacted` is false.
#[test]
fn ac4_include_identifying_keeps_strings() {
    let bytes = read("fixtures/plat/doctor/report/identifying.json");
    let input = DoctorReport::parse(&bytes).unwrap();
    let redactor = Redactor::new(&id_win(), INSTALL_WIN).unwrap();
    let kept = input.for_sharing(&redactor, true).unwrap();
    assert!(!kept.redacted);
    assert_eq!(kept.to_json().unwrap(), bytes);
    assert!(String::from_utf8(kept.to_json().unwrap()).unwrap().contains("jdoe"));
    // The default (no flag) redacts.
    let shared = input.for_sharing(&redactor, false).unwrap();
    assert!(shared.redacted);
    assert_eq!(shared.to_json().unwrap(), read("fixtures/plat/doctor/report/identifying.redacted.json"));
}

/// AC-5: an unsupported schema version is its own typed error.
#[test]
fn ac5_schema99_is_unsupported() {
    let err = DoctorReport::parse(&read("fixtures/plat/doctor/report-schema99.json")).unwrap_err();
    assert_eq!(err, ReportError::UnsupportedSchema { found: "99".into() });
    assert_eq!(err.code(), "unsupported_schema");
}

/// AC-5 / REQ-005: the oracle's hand-labelled invalid reports fail with the labelled code and
/// JSON Pointer.
#[test]
fn ac5_invalid_reports_fail_typed() {
    let cases = json("fixtures/plat/doctor/report/invalid_cases.json");
    let CValue::Arr(cases) = cases else { panic!() };
    assert!(cases.len() >= 25);
    for c in &cases {
        let name = s(c, "name");
        let bytes = fid_canon::to_canonical_bytes(obj(c).get("report").unwrap()).unwrap();
        let err = DoctorReport::parse(&bytes).expect_err(name);
        assert_eq!(err.code(), s(c, "code"), "{name}: {err}");
        if let ReportError::SchemaInvalid { pointer, .. } = &err {
            assert_eq!(pointer, s(c, "pointer"), "{name}: {err}");
        }
    }
}

/// REQ-005: size cap, syntax, duplicate keys, root type.
#[test]
fn parse_bounds_and_syntax() {
    let golden = read("fixtures/plat/doctor/report-win11.json");
    // Exactly at the cap (valid JSON padded with whitespace) parses; one byte more is too large.
    let mut at_cap = golden.clone();
    at_cap.resize(MAX_REPORT_BYTES, b' ');
    assert!(DoctorReport::parse(&at_cap).is_ok());
    at_cap.push(b' ');
    assert_eq!(
        DoctorReport::parse(&at_cap).unwrap_err(),
        ReportError::TooLarge { len: MAX_REPORT_BYTES + 1, max: MAX_REPORT_BYTES }
    );
    assert_eq!(DoctorReport::parse(b"{").unwrap_err().code(), "parse");
    assert_eq!(DoctorReport::parse(b"[]").unwrap_err().code(), "schema_invalid");
    let dup = b"{\"schema_version\":1,\"schema_version\":1}";
    assert_eq!(DoctorReport::parse(dup).unwrap_err(), ReportError::Parse { code: "duplicate_key" });
    assert_eq!(DoctorReport::parse(&[0xff, 0xfe]).unwrap_err().code(), "parse");
}

/// AC-6: escape sequences in strings never reach the text as escape bytes.
#[test]
fn ac6_escape_bytes_are_rendered_as_data() {
    let bytes = read("fixtures/plat/doctor/report-escape.json");
    assert!(bytes.windows(6).any(|w| w == b"\\u001b"), "fixture holds an escape character");
    let r = DoctorReport::parse(&bytes).unwrap();
    let text = r.to_text().unwrap();
    assert!(!text.as_bytes().contains(&0x1b), "ESC in the text");
    assert!(!text.chars().any(|c| c != '\n' && (c.is_control() || c == '\u{202e}' || c == '\u{202c}')));
    assert_eq!(text, read_text("fixtures/plat/doctor/report-escape.txt"));
    assert!(text.contains("\\u{1b}[31mRED"));
}

#[test]
fn quote_text_escapes_controls_quotes_and_backslashes() {
    assert_eq!(
        quote_text("a\"b\\c\u{1b}d\u{7f}e\u{2066}f\u{feff}"),
        "\"a\\\"b\\\\c\\u{1b}d\\u{7f}e\\u{2066}f\\u{feff}\""
    );
    assert_eq!(quote_text("é ü"), "\"é ü\"");
}

#[test]
fn tool_version_is_checked() {
    let base = win11_report();
    for bad in ["", "   ", "1.0\n", "1.0\u{202e}", &"9".repeat(65)] {
        let err = DoctorReport::new(bad, Phase::P1a, base.snapshot.clone(), base.assessment.clone()).unwrap_err();
        assert_eq!(err, ReportError::InvalidToolVersion, "{bad:?}");
    }
    assert!(DoctorReport::new(&"9".repeat(64), Phase::P1a, base.snapshot.clone(), base.assessment).is_ok());
}

/// A detail that redaction makes longer than 200 characters is cut to 199 and marked with `…`
/// (never silently), and still parses back.
#[test]
fn long_redacted_detail_is_cut_visibly() {
    let mut r = win11_report();
    let detail = ["x"; 100].join(" ");
    assert_eq!(detail.chars().count(), 199);
    r.snapshot.ram = Probed::Unknown { reason: UnknownReason::Error, detail };
    let ident = simple_identity("x", "h", "/home/x");
    let red = r.redact(&Redactor::new(&ident, "/opt/fid").unwrap()).unwrap();
    let full = [PLACEHOLDER_USER; 100].join(" ");
    let mut expected: String = full.chars().take(199).collect();
    expected.push('\u{2026}');
    assert_eq!(red.snapshot.ram, Probed::Unknown { reason: UnknownReason::Error, detail: expected });
    let back = DoctorReport::parse(&red.to_json().unwrap()).unwrap();
    assert_eq!(back.to_json().unwrap(), red.to_json().unwrap());
}

/// An architecture name that redaction pushes past 64 bytes is refused, not cut.
#[test]
fn arch_redaction_overflow_fails_closed() {
    let mut r = win11_report();
    r.snapshot.arch = Probed::Known(Arch::parse(&["x"; 21].join("-")).unwrap());
    let ident = simple_identity("x", "h", "/home/x");
    let err = r.redact(&Redactor::new(&ident, "/opt/fid").unwrap()).unwrap_err();
    assert_eq!(err, ReportError::RedactionOverflow { pointer: "/snapshot/arch/value".into() });
}

/// The OS identity is available on every CI host and its own strings are redacted.
#[test]
fn identity_from_os_is_redacted() {
    let id = Identity::from_os().unwrap();
    assert!(!id.user.is_empty() && !id.hosts.is_empty() && !id.home.is_empty());
    let install = if cfg!(windows) { "C:\\Program Files\\Metrology Maven" } else { "/opt/metrology-maven" };
    let r = Redactor::new(&id, install).unwrap();
    assert_eq!(r.redact_text(&format!("user {} here", id.user)), format!("user {PLACEHOLDER_USER} here"));
    for host in &id.hosts {
        assert_eq!(r.redact_text(&format!("on {host} now")), format!("on {PLACEHOLDER_HOST} now"));
    }
    assert_eq!(r.redact_text(&format!("at {} now", id.home)), format!("at {PLACEHOLDER_PATH} now"));
}

/// Two renderings of the same inputs are byte-identical (D0).
#[test]
fn rendering_is_deterministic() {
    let a = win11_report();
    let b = win11_report();
    assert_eq!(a.to_json().unwrap(), b.to_json().unwrap());
    assert_eq!(a.to_text().unwrap(), b.to_text().unwrap());
}

// ------------------------------------------------------------------ identity gathering

fn os(v: &str) -> OsString {
    OsString::from(v)
}

#[cfg(unix)]
fn non_utf8() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(vec![0x6a, 0xff])
}

#[cfg(windows)]
fn non_utf8() -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(&[0x6a, 0xD800])
}

type Env = Vec<(&'static str, OsString)>;

fn lookup(env: &Env) -> impl Fn(&str) -> Option<OsString> + '_ {
    move |name| env.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone())
}

fn win_env() -> Env {
    vec![
        ("USERNAME", os("jdoe")),
        ("USERPROFILE", os("C:\\Users\\jdoe")),
        ("COMPUTERNAME", os("WS-QA-17")),
        ("USERDOMAIN", os("CORP")),
        ("USERDNSDOMAIN", os("CORP.EXAMPLE.COM")),
    ]
}

fn unix_env() -> Env {
    vec![("USER", os("mtest")), ("HOME", os("/home/mtest")), ("HOSTNAME", os("rig-04"))]
}

fn with(mut env: Env, key: &'static str, value: Option<OsString>) -> Env {
    env.retain(|(k, _)| *k != key);
    if let Some(v) = value {
        env.push((key, v));
    }
    env
}

#[test]
fn identity_from_lookup_windows_gathers_dns_names_and_domains() {
    let env = win_env();
    let id =
        Identity::from_lookup(IdentityPlatform::Windows, lookup(&env), || vec!["ws-qa-17-long-name".into()]).unwrap();
    assert_eq!(
        id,
        Identity {
            user: "jdoe".into(),
            hosts: vec![
                "WS-QA-17".into(),
                "ws-qa-17-long-name".into(),
                "WS-QA-17.CORP.EXAMPLE.COM".into(),
                "ws-qa-17-long-name.CORP.EXAMPLE.COM".into(),
            ],
            home: "C:\\Users\\jdoe".into(),
            user_dirs: Vec::new(),
            domains: vec!["CORP".into(), "CORP.EXAMPLE.COM".into()],
        }
    );
    // A sys host equal to COMPUTERNAME in another case is not listed twice.
    let id = Identity::from_lookup(IdentityPlatform::Windows, lookup(&env), || vec!["ws-qa-17".into()]).unwrap();
    assert_eq!(id.hosts, vec!["WS-QA-17".to_string(), "WS-QA-17.CORP.EXAMPLE.COM".to_string()]);
    // HOMEDRIVE + HOMEPATH stand in for a missing USERPROFILE.
    let env = with(
        with(with(win_env(), "USERPROFILE", None), "HOMEDRIVE", Some(os("D:"))),
        "HOMEPATH",
        Some(os("\\Users\\jdoe")),
    );
    let id = Identity::from_lookup(IdentityPlatform::Windows, lookup(&env), Vec::new).unwrap();
    assert_eq!(id.home, "D:\\Users\\jdoe");
    // Every gathered name is redacted.
    let env = win_env();
    let id =
        Identity::from_lookup(IdentityPlatform::Windows, lookup(&env), || vec!["ws-qa-17-long-name".into()]).unwrap();
    let r = Redactor::new(&id, INSTALL_WIN).unwrap();
    assert_eq!(
        r.redact_text("CORP\\jdoe on ws-qa-17-long-name.corp.example.com mail jdoe@corp.example.com"),
        "<redacted:host>\\<redacted:user> on <redacted:host> mail <redacted:user>@<redacted:host>"
    );
}

#[test]
fn identity_from_lookup_unix_falls_back_to_logname() {
    let env = with(with(unix_env(), "USER", None), "LOGNAME", Some(os("mtest")));
    let id = Identity::from_lookup(IdentityPlatform::Unix, lookup(&env), || vec!["rig-04.lab".into()]).unwrap();
    assert_eq!(
        id,
        Identity {
            user: "mtest".into(),
            hosts: vec!["rig-04".into(), "rig-04.lab".into()],
            home: "/home/mtest".into(),
            user_dirs: Vec::new(),
            domains: vec!["lab".into()],
        }
    );
}

/// Unix: the domain part of a dotted host name joins the domains, so the fully qualified
/// name and the bare domain are both redacted.
#[test]
fn identity_from_lookup_unix_derives_domains() {
    let env = unix_env();
    let id =
        Identity::from_lookup(IdentityPlatform::Unix, lookup(&env), || vec!["rig-04.lab.acme.com".into()]).unwrap();
    assert_eq!(id.hosts, vec!["rig-04".to_string(), "rig-04.lab.acme.com".to_string()]);
    assert_eq!(id.domains, vec!["lab.acme.com".to_string()]);
    let r = Redactor::new(&id, "/opt/metrology-maven").unwrap();
    assert_eq!(
        r.redact_text("on rig-04.lab.acme.com (RIG-04) in lab.acme.com"),
        "on <redacted:host> (<redacted:host>) in <redacted:host>"
    );
}

/// identity_unavailable for each missing, blank or non-UTF-8 item (hand-labelled).
#[test]
fn identity_unavailable_for_each_missing_blank_or_non_utf8_item() {
    use IdentityPlatform::{Unix, Windows};
    let blank = || Some(os("  "));
    let bad = || Some(non_utf8());
    // (label, platform, env, sys hosts, item)
    let cases: Vec<(&str, IdentityPlatform, Env, Vec<String>, &'static str)> = vec![
        ("win user missing", Windows, with(win_env(), "USERNAME", None), vec![], "user"),
        ("win user blank", Windows, with(win_env(), "USERNAME", blank()), vec![], "user"),
        ("win user non-utf8", Windows, with(win_env(), "USERNAME", bad()), vec![], "user"),
        ("win home missing", Windows, with(win_env(), "USERPROFILE", None), vec![], "home"),
        ("win home blank", Windows, with(win_env(), "USERPROFILE", blank()), vec![], "home"),
        ("win home non-utf8", Windows, with(win_env(), "USERPROFILE", bad()), vec![], "home"),
        ("win home relative", Windows, with(win_env(), "USERPROFILE", Some(os("jdoe"))), vec![], "home"),
        (
            "win homepath non-utf8",
            Windows,
            with(with(win_env(), "USERPROFILE", None), "HOMEPATH", bad()),
            vec![],
            "home",
        ),
        ("win host missing", Windows, with(win_env(), "COMPUTERNAME", None), vec![], "host"),
        ("win host blank", Windows, with(win_env(), "COMPUTERNAME", blank()), vec![], "host"),
        ("win host non-utf8", Windows, with(win_env(), "COMPUTERNAME", bad()), vec!["ws-qa-17".into()], "host"),
        ("win host control only", Windows, with(win_env(), "COMPUTERNAME", None), vec!["\u{7}".into()], "host"),
        ("win domain non-utf8", Windows, with(win_env(), "USERDOMAIN", bad()), vec![], "domain"),
        ("win dns domain non-utf8", Windows, with(win_env(), "USERDNSDOMAIN", bad()), vec![], "domain"),
        ("win onedrive non-utf8", Windows, with(win_env(), "OneDrive", bad()), vec![], "home"),
        ("win onedrive commercial non-utf8", Windows, with(win_env(), "OneDriveCommercial", bad()), vec![], "home"),
        ("win onedrive consumer non-utf8", Windows, with(win_env(), "OneDriveConsumer", bad()), vec![], "home"),
        ("unix user missing", Unix, with(unix_env(), "USER", None), vec![], "user"),
        ("unix user blank", Unix, with(unix_env(), "USER", blank()), vec![], "user"),
        ("unix user non-utf8", Unix, with(unix_env(), "USER", bad()), vec![], "user"),
        ("unix logname non-utf8", Unix, with(with(unix_env(), "USER", None), "LOGNAME", bad()), vec![], "user"),
        ("unix home missing", Unix, with(unix_env(), "HOME", None), vec![], "home"),
        ("unix home blank", Unix, with(unix_env(), "HOME", blank()), vec![], "home"),
        ("unix home non-utf8", Unix, with(unix_env(), "HOME", bad()), vec![], "home"),
        ("unix host missing", Unix, with(unix_env(), "HOSTNAME", None), vec![], "host"),
        ("unix host blank", Unix, with(unix_env(), "HOSTNAME", blank()), vec![], "host"),
        ("unix host non-utf8", Unix, with(unix_env(), "HOSTNAME", bad()), vec!["rig-04".into()], "host"),
    ];
    for (label, platform, env, sys, item) in cases {
        let err = Identity::from_lookup(platform, lookup(&env), move || sys).unwrap_err();
        assert_eq!(err, ReportError::IdentityUnavailable { item }, "{label}");
    }
    // A missing host is fine when the OS reports one.
    let env = with(unix_env(), "HOSTNAME", None);
    assert!(Identity::from_lookup(Unix, lookup(&env), || vec!["rig-04".into()]).is_ok());
}

#[test]
fn simple_case_folding() {
    assert_eq!(fold_char('É'), 'é');
    assert_eq!(fold_char('Σ'), 'σ');
    assert_eq!(fold_char('ς'), 'σ');
    assert_eq!(fold_char('ſ'), 's');
    // Turkish dotted capital I folds to i (fail closed).
    assert_eq!(fold_char('İ'), 'i');
    assert_eq!(fold_char('ı'), 'ı');
    assert_eq!(fold_char('A'), 'a');
    assert_eq!(fold_char('7'), '7');
}

/// Profile names other than `minimum` and `recommended` are redacted and re-sorted; two
/// made equal, or one made too long, are refused.
#[test]
fn profile_names_redacted_resorted_and_checked() {
    let base = DoctorReport::parse(&read("fixtures/plat/doctor/report/identifying.json")).unwrap();
    let redactor = Redactor::new(&id_win(), INSTALL_WIN).unwrap();
    let red = base.redact(&redactor).unwrap();
    let names: Vec<&str> = red.assessment.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["<redacted:host> bench", "minimum", "recommended"]);

    let mut twin = base.clone();
    let mut extra = twin.assessment.profiles[2].clone();
    extra.name = "WS-QA-17 bench".into();
    twin.assessment.profiles.insert(0, extra);
    assert_eq!(
        twin.redact(&redactor).unwrap_err(),
        ReportError::RedactionCollision { pointer: "/assessment/profiles".into() }
    );

    let mut long = base;
    long.assessment.profiles[2].name = ["jdoe"; 12].join(" ");
    assert_eq!(
        long.redact(&redactor).unwrap_err(),
        ReportError::RedactionOverflow { pointer: "/assessment/profiles/2/name".into() }
    );
}

/// Windows OneDrive folders are gathered and a path starting with one runs to its end.
#[test]
fn identity_from_lookup_windows_onedrive_folders() {
    let env = with(
        with(
            with(win_env(), "OneDrive", Some(os("C:\\Users\\jdoe\\OneDrive - Acme Corp"))),
            "OneDriveCommercial",
            Some(os("C:\\Users\\jdoe\\OneDrive - Acme Corp")),
        ),
        "OneDriveConsumer",
        Some(os("C:\\Users\\jdoe\\OneDrive")),
    );
    let id = Identity::from_lookup(IdentityPlatform::Windows, lookup(&env), Vec::new).unwrap();
    assert_eq!(
        id.user_dirs,
        vec!["C:\\Users\\jdoe\\OneDrive - Acme Corp".to_string(), "C:\\Users\\jdoe\\OneDrive".to_string()]
    );
    let r = Redactor::new(&id, INSTALL_WIN).unwrap();
    assert_eq!(r.redact_text("cannot access C:\\Users\\jdoe\\OneDrive - Acme Corp"), "cannot access <redacted:path>");
    assert_eq!(
        r.redact_text("cannot access C:\\Users\\jdoe\\OneDrive - Acme Corp now"),
        "cannot access <redacted:path> now"
    );
    // A blank OneDrive variable is ignored.
    let env = with(win_env(), "OneDrive", Some(os("  ")));
    assert!(Identity::from_lookup(IdentityPlatform::Windows, lookup(&env), Vec::new).unwrap().user_dirs.is_empty());
}
