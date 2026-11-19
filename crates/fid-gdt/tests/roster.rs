//! Acceptance criteria of spec node GDT.RES.ROWFLOW.ROSTER (AC-1 .. AC-3) plus the error paths
//! of REQ-001, REQ-002 and REQ-005. The `has_role` truth table and the expected errors are
//! hand-labelled in `fixtures/gdt/rowflow/roster_ok_truth.json`; the expected digest comes from
//! the independent stdlib-Python reference `fixtures/gdt/rowflow/make_digest.py`.
//! Every name and identity in the fixtures is fictional.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_gdt::dmt_flow::roster::{
    Basis, Date, Dialect, Roster, RosterError, RosterView, DOCUMENT_FIELD, MAX_ENTRIES, MAX_PRINCIPALS_PER_ENTRY,
    MAX_ROSTER_BYTES,
};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/rowflow")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn load(name: &str) -> Result<Roster, RosterError> {
    Roster::load(&read(name))
}

fn obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("not an object: {other:?}"),
    }
}

fn s<'a>(v: &'a CValue, key: &str) -> &'a str {
    match obj(v).get(key) {
        Some(CValue::Str(s)) => s,
        other => panic!("{key}: {other:?}"),
    }
}

fn dialect(s: &str) -> Dialect {
    Dialect::parse(s).unwrap()
}

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}

/// One minimal valid entry as JSON text, with `extra` spliced in.
fn entry(id: &str, principal: &str, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","display_name":"Example (fictional)","principals":["{principal}"],"dialects":["asme"],"basis":"community","valid_from":"2026-01-01"{extra}}}"#
    )
}

fn doc(entries: &[String]) -> Vec<u8> {
    format!(r#"{{"version":"dmt-reviewers/v1","entries":[{}]}}"#, entries.join(",")).into_bytes()
}

fn err(bytes: &[u8]) -> String {
    Roster::load(bytes).map(|_| ()).unwrap_err().to_string()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_has_role_matches_truth_table() {
    let roster = load("roster_ok.json").unwrap();
    assert_eq!(roster.len(), 3);
    let truth = parse(&read("roster_ok_truth.json")).unwrap();
    let cases = match obj(&truth).get("cases") {
        Some(CValue::Arr(a)) => a,
        other => panic!("cases: {other:?}"),
    };
    assert!(cases.len() >= 12, "AC-1 needs every reviewer at both dates");
    for case in cases {
        let p = s(case, "principal");
        let d = dialect(s(case, "dialect"));
        let at = date(s(case, "at"));
        let expect = match obj(case).get("expect") {
            Some(CValue::Null) => None,
            Some(CValue::Str(id)) => Some(id.as_str()),
            other => panic!("expect: {other:?}"),
        };
        let got = roster.has_role(p, d, at);
        assert_eq!(got.as_ref().map(|id| id.as_str()), expect, "{p} {d:?} {at}");
        // The trait the state machine uses gives the same answer.
        assert_eq!(RosterView::has_role(&roster, p, d, at), got);
    }
}

#[test]
fn ac1_fixture_shape() {
    let roster = load("roster_ok.json").unwrap();
    let alpha = roster.get("rev-alpha").unwrap();
    assert_eq!(alpha.dialects, vec![Dialect::Asme]);
    assert_eq!(alpha.basis, Basis::PaidAuthor);
    assert_eq!(alpha.valid_until, None);
    let bravo = roster.get("rev-bravo").unwrap();
    assert_eq!(bravo.dialects, vec![Dialect::Iso]);
    assert_eq!(bravo.basis, Basis::Community);
    let charlie = roster.get("rev-charlie").unwrap();
    assert_eq!(charlie.basis, Basis::Partner);
    assert_eq!(charlie.valid_until, Some(date("2026-12-01")));
    assert_eq!(roster.reviewer_of("human:fictional-bravo-2").map(|r| r.as_str()), Some("rev-bravo"));
    assert_eq!(roster.version(), "dmt-reviewers/v1");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_error_fixtures() {
    let truth = parse(&read("roster_ok_truth.json")).unwrap();
    let errors = obj(obj(&truth).get("errors").unwrap());
    assert_eq!(errors.len(), 3);
    for (file, want) in errors {
        let want = match want {
            CValue::Str(w) => w,
            other => panic!("{other:?}"),
        };
        assert_eq!(&err(&read(file)), want, "{file}");
    }
    assert_eq!(load("roster_dup_id.json").unwrap_err().code(), "roster_duplicate");
    assert_eq!(load("roster_dup_principal.json").unwrap_err().code(), "roster_duplicate");
    assert_eq!(load("roster_unknown_email.json").unwrap_err(), RosterError::Invalid { field: "email".into() });
}

#[test]
fn principal_repeated_inside_one_entry_is_a_duplicate() {
    let e = r#"{"id":"rev-a","display_name":"A (fictional)","principals":["human:x","human:x"],"dialects":["iso"],"basis":"partner","valid_from":"2026-01-01"}"#;
    assert_eq!(err(&doc(&[e.to_string()])), "roster_duplicate{human:x}");
}

#[test]
fn unknown_and_missing_fields_are_named() {
    // REQ-005: no field for credentials, contact details or licence keys.
    for key in ["credential_document", "licence_key", "phone", "email"] {
        let e = entry("rev-a", "human:a", &format!(r#","{key}":"x""#));
        assert_eq!(err(&doc(&[e])), format!("roster_invalid{{{key}}}"));
    }
    assert_eq!(err(br#"{"version":"dmt-reviewers/v1","entries":[],"credentials":[]}"#), "roster_invalid{credentials}");
    for key in ["id", "display_name", "principals", "dialects", "basis", "valid_from"] {
        let full = entry("rev-a", "human:a", "");
        let mut v = parse(full.as_bytes()).unwrap();
        if let CValue::Obj(m) = &mut v {
            m.remove(key);
        }
        let text = String::from_utf8(fid_canon::to_canonical_bytes(&v).unwrap()).unwrap();
        assert_eq!(err(&doc(&[text])), format!("roster_invalid{{{key}}}"), "missing {key}");
    }
    assert_eq!(err(br#"{"entries":[]}"#), "roster_invalid{version}");
    assert_eq!(err(br#"{"version":"dmt-reviewers/v1"}"#), "roster_invalid{entries}");
}

#[test]
fn malformed_values_are_refused() {
    let wrap = |e: String| format!(r#"{{"version":"dmt-reviewers/v1","entries":[{e}]}}"#);
    let cases: Vec<(String, &str)> = vec![
        (r#"{"version":"dmt-reviewers/v2","entries":[]}"#.to_string(), "roster_invalid{version}"),
        (r#"{"version":"dmt-reviewers/v1","entries":{}}"#.to_string(), "roster_invalid{entries}"),
        (r#"{"version":"dmt-reviewers/v1","entries":[1]}"#.to_string(), "roster_invalid{entries}"),
        (r#"[]"#.to_string(), "roster_invalid{$}"),
        (r#"{"version":"dmt-reviewers/v1","entries":["#.to_string(), "roster_invalid{$}"),
        (r#"{"version":"dmt-reviewers/v1","version":"dmt-reviewers/v1","entries":[]}"#.to_string(), "roster_invalid{version}"),
        (wrap(entry("Rev-A", "human:a", "")), "roster_invalid{id}"),
        (wrap(entry("rev-a", "human a", "")), "roster_invalid{principals}"),
        (
            wrap(entry("rev-a", "human:a", r#","valid_until":"2026-01-01""#)),
            "roster_invalid{valid_until}",
        ),
        (
            wrap(entry("rev-a", "human:a", r#","valid_until":null"#)),
            "roster_invalid{valid_until}",
        ),
        (
            r#"{"version":"dmt-reviewers/v1","entries":[{"id":"rev-a","display_name":"A","principals":["human:a"],"dialects":["vda"],"basis":"community","valid_from":"2026-01-01"}]}"#.to_string(),
            "roster_invalid{dialects}",
        ),
        (
            r#"{"version":"dmt-reviewers/v1","entries":[{"id":"rev-a","display_name":"A","principals":["human:a"],"dialects":["iso","iso"],"basis":"community","valid_from":"2026-01-01"}]}"#.to_string(),
            "roster_invalid{dialects}",
        ),
        (
            r#"{"version":"dmt-reviewers/v1","entries":[{"id":"rev-a","display_name":"A","principals":["human:a"],"dialects":["iso"],"basis":"volunteer","valid_from":"2026-01-01"}]}"#.to_string(),
            "roster_invalid{basis}",
        ),
        (
            r#"{"version":"dmt-reviewers/v1","entries":[{"id":"rev-a","display_name":"A","principals":[],"dialects":["iso"],"basis":"partner","valid_from":"2026-02-30"}]}"#.to_string(),
            "roster_invalid{principals}",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(err(input.as_bytes()), want, "{input}");
    }
    let bad_date = r#"{"version":"dmt-reviewers/v1","entries":[{"id":"rev-a","display_name":"A","principals":["human:a"],"dialects":["iso"],"basis":"partner","valid_from":"2026-02-30"}]}"#;
    assert_eq!(err(bad_date.as_bytes()), "roster_invalid{valid_from}");
    let control = r#"{"version":"dmt-reviewers/v1","entries":[{"id":"rev-a","display_name":"A\nB","principals":["human:a"],"dialects":["iso"],"basis":"partner","valid_from":"2026-02-01"}]}"#;
    assert_eq!(err(control.as_bytes()), "roster_invalid{display_name}");
}

// ------------------------------------------------------------------ maintainer decisions (PR #165)

#[test]
fn principals_must_be_lowercase() {
    // A case variant of a bound identity cannot be bound as a second principal.
    let two = [entry("rev-a", "human:fictional-alpha", ""), entry("rev-b", "HUMAN:Fictional-Alpha", "")];
    assert_eq!(err(&doc(&two)), "roster_invalid{principals}");
    for bad in ["human:Fictional-alpha", "Human:fictional-alpha", "HUMAN:FICTIONAL-ALPHA"] {
        assert_eq!(err(&doc(&[entry("rev-a", bad, "")])), "roster_invalid{principals}", "{bad}");
    }
    let ok = Roster::load(&doc(&[entry("rev-a", "human:fictional-alpha.2_x", "")])).unwrap();
    assert!(ok.reviewer_of("human:fictional-alpha.2_x").is_some());
    // Lookup is exact: an upper-case query never matches.
    assert_eq!(ok.has_role("HUMAN:FICTIONAL-ALPHA.2_X", Dialect::Asme, date("2026-06-01")), None);
}

#[test]
fn only_human_principals_are_accepted() {
    for bad in
        ["agent:fictional-bot", "service:fictional-ci", "fictional-alpha", "human:", "humanfictional", ":human:x"]
    {
        assert_eq!(err(&doc(&[entry("rev-a", bad, "")])), "roster_invalid{principals}", "{bad}");
    }
    assert!(Roster::load(&doc(&[entry("rev-a", "human:fictional-alpha", "")])).is_ok());
}

#[test]
fn display_names_refuse_format_characters() {
    let mk = |n: &str| {
        doc(&[format!(
            r#"{{"id":"rev-a","display_name":"{n}","principals":["human:a"],"dialects":["iso"],"basis":"partner","valid_from":"2026-01-01"}}"#
        )])
    };
    let format_chars = [
        '\u{202E}',
        '\u{202A}',
        '\u{2066}',
        '\u{200B}',
        '\u{200C}',
        '\u{200D}',
        '\u{2060}',
        '\u{FEFF}',
        '\u{00AD}',
        '\u{E0041}',
    ];
    for c in format_chars {
        let name = format!("Alpha{c}Example");
        assert_eq!(err(&mk(&name)), "roster_invalid{display_name}", "U+{:04X}", u32::from(c));
    }
    // Ordinary non-ASCII letters are fine.
    assert!(Roster::load(&mk("Ålpha Ëxample (fictional)")).is_ok());
}

// ------------------------------------------------------------------ REQ-005 and size bounds

#[test]
fn display_name_is_at_most_80_characters() {
    let name80: String = "é".repeat(80);
    let name81: String = "é".repeat(81);
    let mk = |n: &str| {
        doc(&[format!(
            r#"{{"id":"rev-a","display_name":"{n}","principals":["human:a"],"dialects":["iso"],"basis":"partner","valid_from":"2026-01-01"}}"#
        )])
    };
    let ok = Roster::load(&mk(&name80)).unwrap();
    assert_eq!(ok.get("rev-a").unwrap().display_name, name80);
    assert_eq!(err(&mk(&name81)), "field_too_long{display_name}");
    assert_eq!(err(&doc(&[entry(&"a".repeat(65), "human:a", "")])), "field_too_long{id}");
    assert_eq!(err(&doc(&[entry("rev-a", &"p".repeat(129), "")])), "field_too_long{principals}");
}

#[test]
fn sizes_are_bounded() {
    // Too many entries.
    let many: Vec<String> = (0..=MAX_ENTRIES).map(|i| entry(&format!("r{i}"), &format!("human:p{i}"), "")).collect();
    assert_eq!(err(&doc(&many)), "roster_invalid{entries}");
    // Exactly the limit loads (and stays well within the byte limit).
    let bytes = doc(&many[..MAX_ENTRIES]);
    assert!(bytes.len() <= MAX_ROSTER_BYTES);
    assert_eq!(Roster::load(&bytes).unwrap().len(), MAX_ENTRIES);
    // Too many principals on one entry.
    let ps: Vec<String> = (0..=MAX_PRINCIPALS_PER_ENTRY).map(|i| format!("\"human:p{i}\"")).collect();
    let e = format!(
        r#"{{"id":"rev-a","display_name":"A","principals":[{}],"dialects":["iso"],"basis":"partner","valid_from":"2026-01-01"}}"#,
        ps.join(",")
    );
    assert_eq!(err(&doc(&[e])), "roster_invalid{principals}");
    // Oversized input and deep nesting fail closed.
    let mut big = doc(&[]);
    big.resize(MAX_ROSTER_BYTES + 1, b' ');
    assert_eq!(err(&big), format!("roster_invalid{{{DOCUMENT_FIELD}}}"));
    let deep = format!(r#"{{"version":"dmt-reviewers/v1","entries":[{}]}}"#, "[".repeat(10) + &"]".repeat(10));
    assert_eq!(err(deep.as_bytes()), "roster_invalid{$}");
}

#[test]
fn no_panic_on_truncated_or_mutated_input() {
    let bytes = read("roster_ok.json");
    for n in 0..bytes.len() {
        let _ = Roster::load(&bytes[..n]);
    }
    for i in 0..bytes.len() {
        let mut m = bytes.clone();
        m[i] = m[i].wrapping_add(1);
        let _ = Roster::load(&m);
    }
}

// ------------------------------------------------------------------ AC-3 and REQ-004

#[test]
fn ac3_digest_is_order_independent_and_matches_reference() {
    let a = load("roster_ok.json").unwrap();
    let b = load("roster_ok_reordered.json").unwrap();
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
    assert_eq!(a, b);
    let want = String::from_utf8(read("roster_ok.digest")).unwrap();
    assert_eq!(a.digest().as_str(), want.trim_end());
    assert_eq!(RosterView::roster_digest(&a), a.digest());
    // The digest covers the canonical bytes exactly.
    assert_eq!(fid_canon::digest_bytes(a.canonical_bytes()).0, a.digest().to_bytes());
    assert_eq!(fid_canon::canonicalize(a.canonical_bytes()).unwrap(), a.canonical_bytes());
}

#[test]
fn digest_changes_with_content() {
    let base = Roster::load(&doc(&[entry("rev-a", "human:a", "")])).unwrap();
    let changed = Roster::load(&doc(&[entry("rev-a", "human:a", r#","valid_until":"2027-01-01""#)])).unwrap();
    let renamed = Roster::load(&doc(&[entry("rev-b", "human:a", "")])).unwrap();
    assert_ne!(base.digest(), changed.digest());
    assert_ne!(base.digest(), renamed.digest());
    let empty = Roster::load(&doc(&[])).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.canonical_bytes(), br#"{"entries":{},"version":"dmt-reviewers/v1"}"#);
}

#[test]
fn loading_is_deterministic() {
    let bytes = read("roster_ok.json");
    let first = Roster::load(&bytes).unwrap();
    for _ in 0..3 {
        assert_eq!(Roster::load(&bytes).unwrap(), first);
    }
}
