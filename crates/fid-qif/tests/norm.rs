//! Acceptance criteria AC-1 .. AC-5 of spec node IO.QIF.NORM and the requirements behind
//! them. Expected normalised bytes come from the independent Python reference normaliser
//! `fixtures/io/qif/norm/make_expected.py`; expected comparison results are hand-labelled
//! from the rules (paths, lines and values read off the fixture files).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::{Budget, LimitRequest, Principal};
use fid_qif::norm::{default_budget, rule_file_digest, RULE_IDS};
use fid_qif::{
    check_version, compare, compare_with, normalise, normalise_with, Diff, Equality, NormRules, LOCK_FILE, RULE_FILE,
};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/qif/norm")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

/// Every `*.qif` fixture that the reader accepts.
fn accepted() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".qif") && !n.starts_with("forbidden"))
        .collect();
    names.sort();
    assert!(names.len() >= 11, "fixtures missing: {names:?}");
    names
}

fn shown(t: &Option<fid_io_reg::UntrustedText>) -> Option<String> {
    t.as_ref().map(|t| t.as_display().to_string())
}

fn different(eq: Equality) -> Diff {
    match eq {
        Equality::Different(d) => d,
        Equality::Equal => panic!("expected a difference"),
    }
}

#[test]
fn normalised_bytes_match_the_reference_normaliser() {
    for name in accepted() {
        let expected = fixture(&format!("expected/{}", name.replace(".qif", ".norm")));
        let got = normalise(&fixture(&name)).unwrap();
        assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(&expected), "{name}");
    }
}

#[test]
fn normalisation_is_idempotent_and_deterministic() {
    for name in accepted() {
        let once = normalise(&fixture(&name)).unwrap();
        assert_eq!(normalise(&once).unwrap(), once, "{name}: norm(norm(x)) != norm(x)");
        assert_eq!(normalise(&fixture(&name)).unwrap(), once, "{name}: not deterministic");
        assert_eq!(compare(&fixture(&name), &once).unwrap(), Equality::Equal, "{name}");
    }
}

#[test]
fn ac1_indentation_and_attribute_order_are_equal() {
    assert_eq!(compare(&fixture("ws_a.qif"), &fixture("ws_b.qif")).unwrap(), Equality::Equal);
}

#[test]
fn ac2_decimal_lexical_forms_differ_at_the_value_path() {
    let d = different(compare(&fixture("dec_a.qif"), &fixture("dec_b.qif")).unwrap());
    assert_eq!(d.path.as_display().to_string(), "/q:QIFDocument[1]/q:Nominal[1]/q:Value[1]");
    assert_eq!((d.line_a, d.line_b), (Some(4), Some(4)));
    assert_eq!(shown(&d.a).as_deref(), Some("1.50"));
    assert_eq!(shown(&d.b).as_deref(), Some("1.5"));
}

#[test]
fn ac3_list_whitespace_runs_are_equal() {
    assert_eq!(compare(&fixture("list_ws.qif"), &fixture("list_ws_single.qif")).unwrap(), Equality::Equal);
}

#[test]
fn ac4_prefix_and_default_namespace_are_equal() {
    assert_eq!(compare(&fixture("prefix_a.qif"), &fixture("prefix_b.qif")).unwrap(), Equality::Equal);
}

#[test]
fn ac5_shipped_rule_file_matches_lock() {
    check_version(RULE_FILE, LOCK_FILE).unwrap();
    let rules = NormRules::builtin().unwrap();
    assert_eq!(rules.version(), "qif-norm@1");
    assert_eq!(rules.rules(), RULE_IDS.map(String::from).as_slice());
}

#[test]
fn ac5_rule_edit_without_version_bump_fails() {
    let edited = RULE_FILE.replace("\"n\",", "\"n\",\n    \"label\",");
    assert_ne!(edited, RULE_FILE);
    let e = check_version(&edited, LOCK_FILE).unwrap_err();
    assert_eq!(e.code(), "norm_version_not_bumped");
    // A comment-only edit is an edit too.
    let e = check_version(&format!("{RULE_FILE}# trailing note\n"), LOCK_FILE).unwrap_err();
    assert_eq!(e.code(), "norm_version_not_bumped");
    // CRLF checkouts hash like LF ones.
    check_version(&RULE_FILE.replace('\n', "\r\n"), LOCK_FILE).unwrap();
}

#[test]
fn ac5_version_bump_needs_a_lock_line() {
    let bumped = RULE_FILE.replace("version = \"qif-norm@1\"", "version = \"qif-norm@2\"");
    assert_eq!(check_version(&bumped, LOCK_FILE).unwrap_err().code(), "norm_lock_stale");
    let lock = format!("{LOCK_FILE}qif-norm@2 {}\n", rule_file_digest(&bumped));
    check_version(&bumped, &lock).unwrap();
    // The old rule file is no longer current.
    assert_eq!(check_version(RULE_FILE, &lock).unwrap_err().code(), "norm_lock_stale");
}

#[test]
fn ac5_malformed_lock_is_refused() {
    for lock in [
        "",
        "qif-norm@1\n",
        "qif-norm@1 sha256:XYZ\n",
        "qif-norm@01 sha256:258b0f78fdcb272bf965c41c04d2cfa0b84a1c9a2d01373ec048d349aedf1a0e\n",
        "qif-norm@2 sha256:258b0f78fdcb272bf965c41c04d2cfa0b84a1c9a2d01373ec048d349aedf1a0e\n\
         qif-norm@1 sha256:258b0f78fdcb272bf965c41c04d2cfa0b84a1c9a2d01373ec048d349aedf1a0e\n",
    ] {
        assert_eq!(check_version(RULE_FILE, lock).unwrap_err().code(), "norm_lock_invalid", "{lock:?}");
    }
}

#[test]
fn rule_file_validation() {
    let cases = [
        RULE_FILE.replace("\"drop-ws-text\", ", ""),
        RULE_FILE.replace("= \"xs\"", "= \"n7\""),
        RULE_FILE.replace("= \"xs\"", "= \"xmlfoo\""),
        RULE_FILE.replace("= \"xs\"", "= \"q\""),
        RULE_FILE.replace("qif-norm@1", "qif-norm@x"),
        RULE_FILE.replace("\"n\",", "\"n\",\n    \"{}empty\","),
        format!("{RULE_FILE}\nextra = 1\n"),
    ];
    for (i, text) in cases.iter().enumerate() {
        assert_eq!(NormRules::parse(text).unwrap_err().code(), "norm_rules_invalid", "case {i}");
    }
}

#[test]
fn req004_numeric_lexical_forms_are_kept() {
    let out = normalise(
        b"<a xmlns='http://qifstandards.org/xsd/qif3'><Value> 1.50 </Value><Values>1.0E+00  -0.0</Values></a>",
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "<q:a xmlns:q=\"http://qifstandards.org/xsd/qif3\"><q:Value>1.50</q:Value><q:Values>1.0E+00 -0.0</q:Values></q:a>"
    );
}

#[test]
fn attribute_difference_names_the_attribute() {
    let a = br#"<r xmlns="http://qifstandards.org/xsd/qif3"><Nominal id="1" label="D1"/></r>"#;
    let b = b"<r xmlns=\"http://qifstandards.org/xsd/qif3\">\n<Nominal id=\"1\" label=\"D2\"/></r>";
    let d = different(compare(a, b).unwrap());
    assert_eq!(d.path.as_display().to_string(), "/q:r[1]/q:Nominal[1]/@label");
    assert_eq!((d.line_a, d.line_b), (Some(1), Some(2)));
    assert_eq!((shown(&d.a).as_deref(), shown(&d.b).as_deref()), (Some("D1"), Some("D2")));

    let c = br#"<r xmlns="http://qifstandards.org/xsd/qif3"><Nominal id="1"/></r>"#;
    let d = different(compare(a, c).unwrap());
    assert_eq!(d.path.as_display().to_string(), "/q:r[1]/q:Nominal[1]/@label");
    assert_eq!((shown(&d.a).as_deref(), shown(&d.b)), (Some("D1"), None));
}

#[test]
fn structural_difference_reports_both_sides() {
    let a = b"<r><x/><y/></r>";
    let b = b"<r><x/><x/></r>";
    let d = different(compare(a, b).unwrap());
    assert_eq!(d.path.as_display().to_string(), "/r[1]/y[1]");
    assert_eq!((shown(&d.a).as_deref(), shown(&d.b).as_deref()), (Some("<y>"), Some("<x>")));

    let d = different(compare(b"<r><x/></r>", b"<r><x/><z k=\"v\"/></r>").unwrap());
    assert_eq!(d.path.as_display().to_string(), "/r[1]");
    assert_eq!((shown(&d.a).as_deref(), shown(&d.b).as_deref()), (Some("</r>"), Some("<z k=\"v\">")));
}

#[test]
fn forbidden_and_malformed_input_are_errors() {
    let forbidden = fixture("forbidden_doctype.qif");
    assert_eq!(normalise(&forbidden).unwrap_err().code(), "xml_forbidden_construct");
    // A difference before the bad construct does not hide it.
    assert_eq!(compare(&fixture("dec_a.qif"), &forbidden).unwrap_err().code(), "xml_forbidden_construct");
    let truncated = b"<r xmlns=\"urn:x\"><a>1</a>";
    assert_eq!(normalise(truncated).unwrap_err().code(), "truncated_file");
    assert_eq!(compare(b"<r><a>2</a></r>", b"<r><a>1</a><b></r>").unwrap_err().code(), "malformed");
}

#[test]
fn output_amplification_is_bounded_in_normalise_and_compare() {
    // A long namespace name declared on the root but used only by many siblings is
    // re-declared on each of them in the canonical form.
    let uri = format!("urn:example:{}", "u".repeat(1000));
    let doc = format!("<r xmlns:p=\"{uri}\">{}</r>", "<p:a/>".repeat(200));
    let input = doc.len() as u64;
    assert!(input < 4000);
    let e = normalise(doc.as_bytes()).unwrap_err();
    assert_eq!(e.code(), "limit_exceeded");
    assert_eq!(compare(doc.as_bytes(), doc.as_bytes()).unwrap_err().code(), "limit_exceeded");
    // Two siblings stay within 4 x input + 4096 and normalise.
    let small = format!("<r xmlns:p=\"{uri}\">{}</r>", "<p:a/>".repeat(2));
    let out = normalise(small.as_bytes()).unwrap();
    assert!(out.len() as u64 <= fid_qif::norm::output_cap(small.len() as u64, u64::MAX));
    assert_eq!(compare(small.as_bytes(), &out).unwrap(), Equality::Equal);
    assert_eq!(fid_qif::norm::output_cap(input, u64::MAX), 4 * input + 4096);
    assert_eq!(fid_qif::norm::output_cap(input, 100), 100);
}

#[test]
fn bounded_by_the_budget() {
    let tiny = |alloc| Budget::new(Principal::Script, &LimitRequest::default().alloc(alloc)).unwrap();
    // 10,000 bytes of text are held back until `</a>` shows `a` has no child element.
    let big = format!("<r><a>{}</a></r>", "t".repeat(10_000));
    let rules = NormRules::builtin().unwrap();
    let e = normalise_with(big.as_bytes(), tiny(4096), &rules).unwrap_err();
    assert_eq!(e.code(), "limit_exceeded");
    let e = compare_with(big.as_bytes(), big.as_bytes(), tiny(4096), default_budget().unwrap(), &rules).unwrap_err();
    assert_eq!(e.code(), "limit_exceeded");
    // Within the budget the same document normalises.
    normalise_with(big.as_bytes(), default_budget().unwrap(), &rules).unwrap();
}
