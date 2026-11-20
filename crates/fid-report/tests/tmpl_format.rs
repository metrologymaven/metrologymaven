#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]
//! Acceptance tests of RPT.MODEL.TMPL.FORMAT (AC-1, AC-2, AC-3) and the pinned Design
//! rules. Expected bytes, digests, pointers and reasons come from
//! fixtures/rpt/model/tmpl/make_fixtures.py (independent Python) or are hand-labelled here
//! from the Design, never from this crate. AC-4 (fuzz) is tests/tmpl_format_fuzz.rs; AC-5
//! (JSON Schema) is fixtures/rpt/model/tmpl/check_schema.py.

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_report::tmpl::format::*;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/model/tmpl")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

fn digest_of(name: &str) -> String {
    let text = String::from_utf8(read("digests.txt")).unwrap();
    text.lines().find_map(|l| l.strip_prefix(name).map(|d| d.trim().to_string())).unwrap()
}

fn invalid(bytes: &[u8]) -> (String, &'static str) {
    match parse_template(bytes) {
        Err(TemplateError::Invalid { pointer, reason }) => (pointer, reason),
        other => panic!("expected template_invalid, got {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_valid_v1_reserialises_to_the_independent_canonical_bytes() {
    let t = parse_template(&read("valid_v1.json")).unwrap();
    let out = serialise(&t).unwrap();
    assert_eq!(out.as_bytes(), read("valid_v1.canonical.json").as_slice());
    assert_eq!(out.digest(), digest_of("valid_v1"));
    // Parse, serialise, parse: byte-identical and equal.
    let t2 = parse_template(out.as_bytes()).unwrap();
    assert_eq!(t2, t);
    assert_eq!(serialise(&t2).unwrap(), out);
    // The model as read.
    assert_eq!(t.schema_version(), 1);
    assert!(t.locked());
    assert!(matches!(t.clearance(), Some(Clearance::Cleared { .. })));
    assert_eq!(t.pages().len(), 2);
    assert_eq!(t.block_count(), 5);
    let table = &t.pages()[0].blocks()[1];
    assert_eq!(table.kind().as_str(), "table");
    assert_eq!(table.kind_version(), 2);
    assert_eq!(table.repeat_header_rows(), 1);
    assert_eq!(table.continuation_block(), Some(t.pages()[0].blocks()[2].id()));
    assert_eq!(table.slot().unwrap().kind(), SlotKind::Characteristics);
    assert_eq!(t.pages()[1].blocks()[1].slot().unwrap().kind(), SlotKind::Tags);
}

/// Strings outside an untrusted wrapper may only be structural: IDs, kind IDs, decimal
/// integers, the media type and enum words, each checked against its own pattern here.
fn structural(key: &str, s: &str) -> bool {
    let uuid = |s: &str| s.len() == 36 && s.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-');
    let word = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
    match key {
        "id" | "template_id" | "continuation_block" => uuid(s),
        "kind" | "state" => word(s),
        "kind_version" | "schema_version" | "repeat_header_rows" => s.bytes().all(|c| c.is_ascii_digit()),
        "media_type" => s == MEDIA_TYPE,
        _ => false,
    }
}

fn check_marked(v: &CValue, key: &str, under_marker: bool, strings: &mut usize) {
    match v {
        CValue::Str(s) => {
            *strings += 1;
            assert!(under_marker || structural(key, s), "unmarked string at key {key:?}: {s:?}");
        }
        CValue::Arr(a) => a.iter().for_each(|c| check_marked(c, key, under_marker, strings)),
        CValue::Obj(o) => {
            let marked = under_marker || o.get("untrusted") == Some(&CValue::Bool(true));
            for (k, c) in o {
                check_marked(c, k, marked, strings);
            }
        }
        _ => {}
    }
}

#[test]
fn ac1_every_content_string_is_marked_untrusted() {
    let t = parse_template(&read("valid_v1.json")).unwrap();
    let v = parse(serialise(&t).unwrap().as_bytes()).unwrap();
    let mut n = 0;
    check_marked(&v, "", false, &mut n);
    assert!(n > 30);
    // The typed model holds the title as Untrusted<Text>; the hostile text survives intact
    // and its only human form escapes the bidi and control characters.
    assert!(t.title().untrusted_value().as_str().contains('\u{202e}'));
    let esc = t.title().escaped();
    assert!(esc.contains("\\u{202E}") && esc.contains("\\u{0007}") && !esc.contains('\u{202e}'));
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_too_large_17_mib_fails_at_the_root() {
    let mut big = read("valid_v1.json");
    big.resize(17 * 1024 * 1024, b' ');
    assert_eq!(invalid(&big), (String::new(), "too_large"));
    // Exactly the budget is accepted.
    big.truncate(MAX_TEMPLATE_BYTES);
    assert!(parse_template(&big).is_ok());
}

#[test]
fn ac2_named_fixture_files_fail_with_their_pointers() {
    let deep = format!("/pages/0/blocks/0/payload/value{}", "/a".repeat(26));
    assert_eq!(invalid(&read("deep.json")), (deep, "too_deep"));
    assert_eq!(invalid(&read("unknown_field.json")), ("/comment".to_string(), "unknown_field"));
    assert_eq!(
        invalid(&read("bad_continuation.json")),
        ("/pages/0/blocks/1/continuation_block".to_string(), "continuation_not_on_page")
    );
    for f in ["deep.json", "unknown_field.json", "bad_continuation.json"] {
        assert_eq!(parse_template(&read(f)).unwrap_err().code(), "report.template_invalid");
    }
}

#[test]
fn hand_labelled_cases() {
    let v = parse(&read("invalid_cases.json")).unwrap();
    let CValue::Obj(o) = v else { panic!() };
    let Some(CValue::Arr(cases)) = o.get("cases") else { panic!() };
    assert!(cases.len() >= 50);
    for c in cases {
        let CValue::Obj(c) = c else { panic!() };
        let s = |k: &str| match c.get(k) {
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => None,
        };
        let name = s("name").unwrap();
        let got = parse_template(s("text").unwrap().as_bytes());
        match s("expect").unwrap().as_str() {
            "ok" => {
                let t = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                let b = serialise(&t).unwrap();
                assert_eq!(serialise(&parse_template(b.as_bytes()).unwrap()).unwrap(), b, "{name}");
            }
            code => {
                let e = got.expect_err(&name);
                assert_eq!(e.code(), code, "{name}: {e:?}");
                if code == "report.template_schema_unsupported" {
                    assert_eq!(e, TemplateError::SchemaUnsupported { version: 99 }, "{name}");
                    continue;
                }
                assert_eq!(e.reason(), s("reason").as_deref(), "{name}: {e:?}");
                if let Some(p) = s("pointer") {
                    assert_eq!(e.pointer(), Some(p.as_str()), "{name}: {e:?}");
                }
            }
        }
    }
}

fn uuid(n: u64) -> String {
    format!("019a9ec0-6400-7000-8000-{n:012x}")
}

fn doc_with_pages(pages: &[usize]) -> Vec<u8> {
    let mut next = 1u64;
    let mut out = String::from("{\"clearance\":null,\"locked\":false,\"media_type\":\"");
    out.push_str(MEDIA_TYPE);
    out.push_str("\",\"pages\":[");
    for (pi, n) in pages.iter().enumerate() {
        if pi > 0 {
            out.push(',');
        }
        out.push_str("{\"blocks\":[");
        for bi in 0..*n {
            if bi > 0 {
                out.push(',');
            }
            next += 1;
            out.push_str(&format!(
                "{{\"continuation_block\":null,\"id\":\"{}\",\"kind\":\"text\",\"kind_version\":\"1\",\
                 \"payload\":{{\"untrusted\":true,\"value\":{{}}}},\"repeat_header_rows\":\"0\",\"slot\":null}}",
                uuid(next)
            ));
        }
        next += 1;
        out.push_str(&format!("],\"id\":\"{}\"}}", uuid(next)));
    }
    out.push_str(&format!(
        "],\"schema_version\":\"1\",\"template_id\":\"{}\",\"title\":{{\"text\":\"t\",\"untrusted\":true}}}}",
        uuid(1)
    ));
    out.into_bytes()
}

#[test]
fn block_and_page_budgets() {
    // 10 000 blocks are accepted; the 10 001st is named.
    assert_eq!(parse_template(&doc_with_pages(&[9_999, 1])).unwrap().block_count(), 10_000);
    assert_eq!(invalid(&doc_with_pages(&[10_000, 1])), ("/pages/1/blocks/0".to_string(), "too_many_blocks"));
    assert_eq!(invalid(&doc_with_pages(&[5_000, 5_001])), ("/pages/1/blocks/5000".to_string(), "too_many_blocks"));
    // 10 000 pages are accepted; the 10 001st is named.
    assert_eq!(parse_template(&doc_with_pages(&[0; 10_000])).unwrap().pages().len(), 10_000);
    assert_eq!(invalid(&doc_with_pages(&[0; 10_001])), ("/pages/10000".to_string(), "too_many_pages"));
}

/// One page of `n` blocks: block j continues to j + 1, and the last continues back to
/// the one before it, so the chain ends in a 2-block loop.
fn chain_ending_in_loop(n: usize) -> Vec<u8> {
    let id = |j: usize| uuid(j as u64 + 2);
    let mut out = String::from("{\"clearance\":null,\"locked\":false,\"media_type\":\"");
    out.push_str(MEDIA_TYPE);
    out.push_str("\",\"pages\":[{\"blocks\":[");
    for j in 0..n {
        if j > 0 {
            out.push(',');
        }
        let next = if j + 1 < n { j + 1 } else { j - 1 };
        out.push_str(&format!(
            "{{\"continuation_block\":\"{}\",\"id\":\"{}\",\"kind\":\"text\",\"kind_version\":\"1\",\
             \"payload\":{{\"untrusted\":true,\"value\":{{}}}},\"repeat_header_rows\":\"0\",\"slot\":null}}",
            id(next),
            id(j)
        ));
    }
    out.push_str(&format!(
        "],\"id\":\"{}\"}}],\"schema_version\":\"1\",\"template_id\":\"{}\",\
         \"title\":{{\"text\":\"t\",\"untrusted\":true}}}}",
        uuid(n as u64 + 2),
        uuid(1)
    ));
    out.into_bytes()
}

#[test]
fn continuation_loop_at_the_end_of_a_10_000_block_chain() {
    // Hand-labelled: blocks 9998 and 9999 form the loop; 9998 is first in page order.
    let t = std::time::Instant::now();
    assert_eq!(
        invalid(&chain_ending_in_loop(10_000)),
        ("/pages/0/blocks/9998/continuation_block".to_string(), "continuation_cycle")
    );
    println!("10 000-block chain: {:?}", t.elapsed());
    // A loop through every block of a page is named at block 0.
    assert_eq!(
        invalid(&chain_ending_in_loop(2)),
        ("/pages/0/blocks/0/continuation_block".to_string(), "continuation_cycle")
    );
}

#[test]
fn string_budget_counts_decoded_bytes_and_covers_keys() {
    let base = String::from_utf8(read("valid_v1.canonical.json")).unwrap();
    // 21 846 three-byte characters are 65 538 bytes: over, though fewer characters.
    let euro = base.replace("\"Characteristic table é中\"", &format!("\"{}\"", "€".repeat(21_846)));
    assert_eq!(invalid(euro.as_bytes()), ("/pages/0/blocks/0/payload/value/text".to_string(), "string_too_long"));
    // 65 536 bytes written as six-byte escapes pass the string budget (then fail the payload bound).
    let esc = base.replace("\"Characteristic table é中\"", &format!("\"{}\"", "\\u0078".repeat(65_536)));
    assert_eq!(invalid(esc.as_bytes()), ("/pages/0/blocks/0/payload/value".to_string(), "value_too_large"));
    // A key over the budget is named by its own pointer.
    let key = "k".repeat(65_537);
    let long_key = base.replace("{\"text\":\"Characteristic", &format!("{{\"{key}\":\"x\",\"text\":\"Characteristic"));
    let (p, r) = invalid(long_key.as_bytes());
    assert_eq!(r, "string_too_long");
    assert!(p.starts_with("/pages/0/blocks/0/payload/value/kkk"));
    assert_eq!(p.len(), MAX_ERROR_POINTER_BYTES);
}

#[test]
fn syntax_and_encoding_errors_name_the_element_reached() {
    // Invalid UTF-8 inside the title text.
    let mut b = b"{\"title\": {\"text\": \"ab".to_vec();
    b.extend_from_slice(&[0xff, b'"', b'}', b'}']);
    assert_eq!(invalid(&b), ("/title/text".to_string(), "invalid_utf8"));
    // A bad literal inside a page.
    assert_eq!(invalid(b"{\"pages\": [{\"id\": nope}]}"), ("/pages/0/id".to_string(), "syntax"));
    for junk in [&b""[..], b"{", b"}", b"]]]", b"\"", b"{\"a\":}", b"[1,]", b"{\"a\" 1}", b"\xef\xbb\xbf{}", b"1e999"] {
        assert_eq!(parse_template(junk).unwrap_err().code(), "report.template_invalid", "{junk:?}");
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_v0_migrates_to_the_independent_v1_bytes_twice() {
    let a = serialise(&parse_template(&read("v0_sample.json")).unwrap()).unwrap();
    let b = serialise(&parse_template(&read("v0_sample.json")).unwrap()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.as_bytes(), read("v0_sample.migrated.json").as_slice());
    assert_eq!(a.digest(), digest_of("v0_sample_migrated"));
    // migrate() itself: pure and deterministic, byte-identical canonical output.
    let v0 = parse(&read("v0_sample.json")).unwrap();
    let m1 = to_canonical_bytes(&migrate(&v0, 0).unwrap()).unwrap();
    let m2 = to_canonical_bytes(&migrate(&v0, 0).unwrap()).unwrap();
    assert_eq!(m1, m2);
    assert_eq!(m1, read("v0_sample.migrated.json"));
}

#[test]
fn ac3_migrating_the_current_version_is_identity_and_newer_is_unsupported() {
    for name in ["valid_v1.json", "valid_v1.canonical.json", "unknown_field.json"] {
        let v = parse(&read(name)).unwrap();
        assert_eq!(migrate(&v, SCHEMA_VERSION).unwrap(), v, "{name}");
    }
    let v = parse(&read("valid_v1.json")).unwrap();
    for from in [2, 99, u32::MAX] {
        assert_eq!(migrate(&v, from), Err(TemplateError::SchemaUnsupported { version: from }));
    }
    let mut big = String::from_utf8(read("valid_v1.canonical.json")).unwrap();
    big = big.replace("\"schema_version\":\"1\"", "\"schema_version\":\"4294967295\"");
    assert_eq!(parse_template(big.as_bytes()), Err(TemplateError::SchemaUnsupported { version: u32::MAX }));
    big = big.replace("\"4294967295\"", "\"4294967296\"");
    assert_eq!(invalid(big.as_bytes()), ("/schema_version".to_string(), "bad_version"));
}

// ------------------------------------------------------------------ pinned rules

#[test]
fn errors_mark_the_pointer_untrusted_and_escape_it_for_humans() {
    let doc = String::from_utf8(read("valid_v1.canonical.json")).unwrap();
    let hostile = doc.replacen("{\"clearance\"", "{\"a\u{202e}b~/\":true,\"clearance\"", 1);
    let e = parse_template(hostile.as_bytes()).unwrap_err();
    assert_eq!(e.pointer(), Some("/a\u{202e}b~0~1"));
    assert_eq!(e.reason(), Some("unknown_field"));
    let v = e.to_value();
    let CValue::Obj(o) = &v else { panic!() };
    let Some(CValue::Obj(p)) = o.get("pointer") else { panic!() };
    assert_eq!(p.get("untrusted"), Some(&CValue::Bool(true)));
    assert_eq!(o.get("code"), Some(&CValue::Str("report.template_invalid".into())));
    let shown = e.to_string();
    assert!(shown.contains("\\u{202E}") && !shown.contains('\u{202e}'), "{shown}");
    let u = TemplateError::SchemaUnsupported { version: 99 };
    assert_eq!(u.code(), "report.template_schema_unsupported");
    assert_eq!(u.pointer(), None);
}

#[test]
fn template_json_refuses_values_by_construction() {
    let ok = parse(br#"{"cells":[{"label":"Dev.","ref":"char-12"}],"note":null,"show":true}"#).unwrap();
    assert!(TemplateJson::new(&ok, "").is_ok());
    let cases: [(&[u8], &str, &str); 5] = [
        (br#"{"w":1}"#, "/w", "number"),
        (br#"[true,[0.5]]"#, "/1/0", "number"),
        (br#"{"c":{"schema":"value_binding@7"}}"#, "/c", "bound_value"),
        (br#"{"b":{"schema":"value_text@1"},"a":{"schema":"value_binding@1"}}"#, "/a", "bound_value"),
        (br#"{"schema":"value_text@2"}"#, "", "value_text"),
    ];
    for (json, pointer, reason) in cases {
        let e = TemplateJson::new(&parse(json).unwrap(), "/x").unwrap_err();
        assert_eq!(e, TemplateError::Invalid { pointer: format!("/x{pointer}"), reason }, "{json:?}");
    }
    // A schema tag that is not a string is ordinary data.
    assert!(TemplateJson::new(&parse(br#"{"schema":true}"#).unwrap(), "").is_ok());
}

#[test]
fn constants_match_the_design() {
    assert_eq!(MEDIA_TYPE, "application/vnd.fid.report-template+json");
    assert_eq!(SCHEMA_VERSION, 1);
    assert_eq!(MAX_TEMPLATE_BYTES, 16 * 1024 * 1024);
    assert_eq!(MAX_TEMPLATE_DEPTH, 32);
    assert_eq!(MAX_TEMPLATE_BLOCKS, 10_000);
    assert_eq!(MAX_PAGES, 10_000);
    assert_eq!(MAX_STRING_BYTES, 65_536);
    assert_eq!(MAX_REPEAT_HEADER_ROWS, 8);
    assert_eq!(V0_FORMAT, "fid.report-template");
    let words: Vec<&str> = SlotKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(words, ["characteristics", "elements", "tags", "stages", "runs", "saved_query", "field_table"]);
    for k in SlotKind::ALL {
        assert_eq!(SlotKind::parse(k.as_str()), Some(k));
    }
}

#[test]
fn published_schema_file_names_its_version() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/report-template.v1.json");
    let v = parse(&std::fs::read(path).unwrap()).unwrap();
    let CValue::Obj(o) = v else { panic!() };
    assert_eq!(o.get("$schema"), Some(&CValue::Str("https://json-schema.org/draft/2020-12/schema".into())));
}
