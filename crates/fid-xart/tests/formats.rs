//! The call-set, matrix and digest-table readers are strict and fail closed (REQ-004, REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use fid_kci::kci::sha256_digest;
use fid_xart::calls::parse_calls;
use fid_xart::matrix::parse_matrix;
use fid_xart::table::DigestTable;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/xart-calls-v1")
}

#[test]
fn call_set_fixture_parses_and_its_digest_is_the_file_hash() {
    let bytes = std::fs::read(fixtures().join("calls.txt")).unwrap();
    let set = parse_calls(&bytes).unwrap();
    assert_eq!(set.calls.len(), 14);
    assert_eq!(set.digest, sha256_digest(&bytes));
    let table = DigestTable::parse(&std::fs::read(fixtures().join("reference-table.json")).unwrap()).unwrap();
    assert_eq!(set.digest, table.call_set_digest);
    for (c, r) in set.calls.iter().zip(&table.rows) {
        assert_eq!((c.name.as_str(), c.algorithm.as_str()), (r.name.as_str(), r.algorithm.as_str()));
    }
}

#[test]
fn bad_call_sets_are_refused() {
    let call = r#"{"algorithm":"test.echo.identity@1.0.0","inputs":[],"params":""}"#;
    let ok = format!("a\t{call}\n");
    assert!(parse_calls(ok.as_bytes()).is_ok());
    let cases = [
        format!("a\t{call}\r\n"),
        format!("a\t{call}"),
        format!("a\t{call}\na\t{call}\n"),
        format!("a {call}\n"),
        "a\t{\"algorithm\": \"test.echo.identity@1.0.0\",\"inputs\":[],\"params\":\"\"}\n".to_string(),
        "a\t{\"algorithm\":\"not an id\",\"inputs\":[],\"params\":\"\"}\n".to_string(),
        "a\t{\"algorithm\":\"test.echo.identity@1.0.0\",\"params\":\"\"}\n".to_string(),
        format!("bad name\t{call}\n"),
        "# only a comment\n".to_string(),
    ];
    for c in cases {
        assert_eq!(parse_calls(c.as_bytes()).map_err(|e| e.code()), Err("bad_call_set"), "{c:?}");
    }
}

#[test]
fn shipped_matrix_parses() {
    let m =
        parse_matrix(&std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("xart-matrix.toml")).unwrap()).unwrap();
    let hosts: Vec<(&str, bool)> = m.hosts.iter().map(|h| (h.name.as_str(), h.optional)).collect();
    assert_eq!(
        hosts,
        [
            ("linux-arm64", false),
            ("linux-x86_64", false),
            ("macos-arm64", false),
            ("macos-x86_64", false),
            ("windows-arm64", false),
            ("windows-x86_64", false),
        ]
    );
}

#[test]
fn bad_matrices_are_refused() {
    let host = |n: &str, o: &str| format!("[[host]]\nname = \"{n}\"\nrunner = \"ubuntu-latest\"\noptional = {o}\n");
    let ok = format!("# c\nschema = \"1\"\n\n{}{}", host("a", "false"), host("b", "true"));
    assert_eq!(parse_matrix(ok.as_bytes()).unwrap().hosts.len(), 2);
    let cases = [
        ok.replace('\n', "\r\n"),
        ok.replace("schema = \"1\"", "schema = \"2\""),
        format!("schema = \"1\"\n{}{}", host("b", "false"), host("a", "false")),
        format!("schema = \"1\"\n{}{}", host("a", "false"), host("a", "true")),
        format!("schema = \"1\"\n{}", host("A", "false")),
        format!("schema = \"1\"\n{}", host("a", "yes")),
        format!("schema = \"1\"\n{}", host("a", "false").replace("runner", "label")),
        format!("schema = \"1\"\n{}extra = \"x\"\n", host("a", "false")),
        "schema = \"1\"\n[[host]]\nname = \"a\"\n".to_string(),
        "schema = \"1\"\n".to_string(),
        format!("\u{feff}schema = \"1\"\n{}", host("a", "false")),
        format!("schema = \"1\"\n{}", host("a", "false").replace("name = ", "name =\t")),
    ];
    for c in cases {
        assert_eq!(parse_matrix(c.as_bytes()).map_err(|e| e.code()), Err("bad_matrix"), "{c:?}");
    }
}

#[test]
fn tables_round_trip_and_hand_edits_are_refused() {
    let bytes = std::fs::read(fixtures().join("reference-table.json")).unwrap();
    let t = DigestTable::parse(&bytes).unwrap();
    assert_eq!(t.to_json().unwrap(), bytes);
    let text = String::from_utf8(bytes).unwrap();
    let cases = [
        text.replace(',', ", "),
        text.replace("{\"artifact\"", "{\"x\":\"y\",\"artifact\""),
        text.replace("fid-xart-table@1", "fid-xart-table@2"),
        text.replace("\"call\":\"1\"", "\"call\":\"2\""),
        text.replace("\"exit\":\"1\"", "\"exit\":\"2\""),
        text.replace("\"kind\":\"wasm\"", "\"kind\":\"jvm\""),
        text.replace("\"result_digest\":\"sha256:", "\"result_digest\":\"sha1:"),
        text.replacen("\"exit\":\"0\"", "\"exit\":0", 1),
        text.replace("\"name\":\"echo-empty\"", "\"name\":\"echo-one-input\",\"name\":\"echo-empty\""),
    ];
    for c in cases {
        assert_ne!(c, text);
        assert_eq!(DigestTable::parse(c.as_bytes()).map_err(|e| e.code()), Err("bad_table"), "{c}");
    }
}
