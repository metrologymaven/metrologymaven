//! Acceptance criteria of spec node ECO.RULES.BLOCK (AC-1 .. AC-5), against fixtures written by
//! the independent Python reference `fixtures/eco/rules/block/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_rules_block::{digest, find_blocks, render, Block, BlockError, BlockErrorKind};
use std::time::Instant;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/eco/rules/block/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{DIR}{name}")).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

fn unhex(s: &str) -> String {
    assert!(s.len() % 2 == 0, "odd hex length");
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect();
    String::from_utf8(bytes).unwrap()
}

fn kind(s: &str) -> BlockErrorKind {
    match s {
        "unbalanced" => BlockErrorKind::Unbalanced,
        "nested" => BlockErrorKind::Nested,
        "id-mismatch" => BlockErrorKind::IdMismatch,
        "bad-marker" => BlockErrorKind::BadMarker,
        other => panic!("unknown kind {other}"),
    }
}

/// The text a fixture name refers to; `<file>+crlf` is the file with every LF turned into CRLF
/// (built here because the repository checks text out with LF endings).
fn fixture_text(name: &str) -> String {
    match name.strip_suffix("+crlf") {
        Some(base) => read(base).replace('\n', "\r\n"),
        None => read(name),
    }
}

fn check_block(name: &str, b: &Block, f: &[&str]) {
    assert_eq!(f.len(), 9, "{name}: bad expected row");
    assert_eq!(b.id, f[1], "{name}: id");
    assert_eq!(b.source_repo, f[2], "{name}: repo");
    assert_eq!(b.source_rev, f[3], "{name}: rev");
    assert_eq!(b.sha256.hex(), f[4], "{name}: recorded digest");
    assert_eq!(b.computed_digest().hex(), f[5], "{name}: body digest");
    assert_eq!(b.begin_line.to_string(), f[6], "{name}: begin line");
    assert_eq!(b.end_line.to_string(), f[7], "{name}: end line");
    assert_eq!(b.body, unhex(f[8]), "{name}: body");
    assert_eq!(b.is_intact(), f[4] == f[5], "{name}: intact");
}

/// Every fixture (LF and CRLF) parses exactly as the Python reference says: blocks with their
/// fields, or the error kind and line (covers AC-1, AC-3, AC-4 and the other error kinds).
#[test]
fn all_fixtures_match_reference() {
    let expected = read("expected.txt");
    let mut rows = expected.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).peekable();
    let mut files = 0;
    while let Some(row) = rows.next() {
        let f: Vec<&str> = row.split('\t').collect();
        let name = f[0];
        let got = find_blocks(&fixture_text(name));
        match f[1] {
            "ok" => {
                let blocks = got.unwrap_or_else(|e| panic!("{name}: unexpected {e}"));
                let n: usize = f[2].parse().unwrap();
                assert_eq!(blocks.len(), n, "{name}: block count");
                for b in &blocks {
                    let brow: Vec<&str> = rows.next().unwrap().split('\t').collect();
                    assert_eq!(brow[0], "block");
                    check_block(name, b, &brow);
                }
            }
            "error" => {
                let want = BlockError { kind: kind(f[2]), line: f[3].parse().unwrap() };
                assert_eq!(got, Err(want), "{name}");
            }
            other => panic!("bad row status {other}"),
        }
        files += 1;
    }
    assert_eq!(files, 16, "fixture rows");
}

/// AC-1: one.md has one block with the recorded ID, revision and digest, and sha256(body)
/// equals the recorded digest.
#[test]
fn ac1_one_block() {
    let blocks = find_blocks(&read("one.md")).unwrap();
    assert_eq!(blocks.len(), 1);
    let b = &blocks[0];
    assert_eq!((b.id.as_str(), b.source_repo.as_str()), ("core", "metrologymaven/fid-contrib"));
    assert_eq!(b.source_rev, "0123456789abcdef0123456789abcdef01234567");
    assert_eq!(b.sha256.as_str(), "sha256:4ccba268100be81b10ea06b0f89908c3a7169ebdf98c0f7aee6b8db65f6573b8");
    assert_eq!(digest(&b.body), b.sha256);
    assert!(b.is_intact());
    assert_eq!((b.begin_line, b.end_line), (5, 13));
}

/// AC-2: a CRLF body hashes like the same body with LF endings.
#[test]
fn ac2_crlf_digest() {
    let lf = "## Rules\n\n1. one\n2. two\n";
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(digest(&crlf), digest(lf));
    let body = find_blocks(&read("one.md")).unwrap().remove(0).body;
    assert_eq!(digest(&body.replace('\n', "\r\n")), digest(&body));
    // A lone CR is content, not a line ending.
    assert_ne!(digest("a\rb"), digest("a\nb"));
}

/// AC-3: a begin marker without an end fails as unbalanced at the begin line.
#[test]
fn ac3_unbalanced() {
    assert_eq!(find_blocks(&read("unbalanced.md")), Err(BlockError { kind: BlockErrorKind::Unbalanced, line: 2 }));
    assert_eq!(find_blocks(&read("stray-end.md")), Err(BlockError { kind: BlockErrorKind::Unbalanced, line: 3 }));
}

/// AC-4: a begin marker inside an open block fails as nested at the inner begin line.
#[test]
fn ac4_nested() {
    let err = find_blocks(&read("nested.md")).unwrap_err();
    assert_eq!(err, BlockError { kind: BlockErrorKind::Nested, line: 4 });
    assert_eq!(err.to_string(), "nested at line 4");
}

/// AC-5: 200 bodies from the reference's seeded generator (Python `random.Random(42)`):
/// render gives the reference's bytes, and parsing returns identical fields.
#[test]
fn ac5_round_trip_200() {
    let cases = read("render-cases-v1.txt");
    let mut n = 0;
    for row in cases.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = row.split('\t').collect();
        let (id, repo, rev, sha, body, rendered) = (f[0], f[1], f[2], f[3], unhex(f[4]), unhex(f[5]));
        let text = render(id, repo, rev, &body).unwrap_or_else(|e| panic!("case {n}: {e}"));
        assert_eq!(text, rendered, "case {n}: rendered text");
        let blocks = find_blocks(&text).unwrap();
        assert_eq!(blocks.len(), 1, "case {n}");
        let b = &blocks[0];
        assert_eq!((b.id.as_str(), b.source_repo.as_str(), b.source_rev.as_str()), (id, repo, rev), "case {n}");
        assert_eq!(b.sha256.hex(), sha, "case {n}");
        assert_eq!(b.body, body, "case {n}");
        assert!(b.is_intact(), "case {n}");
        // The same block converted to CRLF line endings keeps its digest (bodies without CR only:
        // converting a body that already has CR would change its content).
        if !body.contains('\r') {
            let crlf = find_blocks(&text.replace('\n', "\r\n")).unwrap();
            assert_eq!(crlf[0].sha256, b.sha256, "case {n} crlf");
            assert_eq!(crlf[0].computed_digest(), b.sha256, "case {n} crlf");
        }
        n += 1;
    }
    assert_eq!(n, 200);
}

/// Performance budget (under 10 ms for 100 KB). Checked loosely here because CI runs debug
/// builds on shared runners: the scan must stay linear, well under a second.
#[test]
fn hundred_kb_is_fast() {
    let block =
        render("core", "r", "0123456789abcdef0123456789abcdef01234567", "rule text line\n".repeat(40).trim_end())
            .unwrap();
    let mut text = String::new();
    while text.len() < 100 * 1024 {
        text.push_str("some ordinary markdown text\n");
        text.push_str(&block);
    }
    let t = Instant::now();
    let blocks = find_blocks(&text).unwrap();
    assert!(blocks.iter().all(Block::is_intact));
    assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
}
