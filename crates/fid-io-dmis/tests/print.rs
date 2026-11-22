//! Integration tests for `IO.DMIS.PARSE.PRINT` (canonical DMIS printer and
//! parse-print-parse identity harness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_dmis::print::{
    check_identity, print, print_bytes, trees_equal_ignoring_pos, verify_corpus_manifest, PrintOptions,
};
use fid_io_dmis::registry::Registry;
use fid_io_dmis::tree::{parse, parse_in, Node, ParseOptions};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// AC-1: every `accept` entry in the DMIS corpus manifest satisfies parse -> print -> parse
/// identity (positions ignored), while `reject` entries are skipped.
#[test]
fn ac1_corpus_manifest_identity_harness() {
    let manifest = repo_root().join("fixtures/io/dmis/parse/print/dmis.toml");
    let reg = Registry::builtin().expect("builtin registry");
    let report = verify_corpus_manifest(&manifest, &reg, None, ParseOptions::default(), &PrintOptions::default())
        .expect("corpus identity harness must pass");

    assert_eq!(report.checked, 4);
    assert_eq!(report.skipped, 1);
    assert_eq!(
        report.checked_paths,
        vec![
            "files/features_and_numbers.dmi",
            "files/long_300col.dmi",
            "files/results_output.dmo",
            "files/unknown_and_comments.dmi",
        ]
    );
}

/// Also verify parse -> print -> parse identity over the existing GDEF and lex fixtures.
#[test]
fn existing_gdef_and_lex_fixtures_roundtrip() {
    let root = repo_root();
    let reg = Registry::builtin().expect("builtin registry");
    let paths = [
        "fixtures/io/dmis/parse/gdef/features.dmi",
        "fixtures/io/dmis/parse/gdef/results.dmo",
        "fixtures/io/dmis/parse/gdef/defs.dmi",
        "fixtures/io/dmis/parse/lex/continued.dmi",
        "fixtures/io/dmis/parse/lex/mixed.dmi",
        "fixtures/io/dmis/parse/lex/numbers.dmi",
    ];
    for rel in paths {
        let data = std::fs::read(root.join(rel)).expect("read fixture");
        let outcome = check_identity(rel, &data, &reg, None, ParseOptions { lenient: true }, &PrintOptions::default())
            .unwrap_or_else(|e| panic!("identity failed on {rel}: {e}"));
        assert!(outcome.statements > 0, "{rel} should contain statements");
    }
}

/// AC-2 & REQ-IO.DMIS.PARSE.PRINT-002: authored numeric spelling (such as `3.000`, `+0.250`,
/// `05.3`) and quoted string spelling (`''` escapes) are preserved verbatim, while keywords and
/// label prefixes are upper-cased.
#[test]
fn ac2_preserves_authored_number_and_string_spelling() {
    let input = b"dmismn/'Bracket ''B'' check',05.3\n\
                  f(c1)=feat/circle,inner,cart,3.000,+0.250,-1.50,0,0,1,12.500\n\
                  endfil\n";
    let tree = parse(input, None).expect("parse input");
    let printed = print(&tree, &PrintOptions::default());

    assert!(printed.contains("3.000"), "printed text must keep 3.000: {printed}");
    assert!(printed.contains("+0.250"), "printed text must keep +0.250: {printed}");
    assert!(printed.contains("05.3"), "printed text must keep 05.3: {printed}");
    assert!(printed.contains("'Bracket ''B'' check'"), "printed text must keep string spelling: {printed}");
    assert!(printed.contains("DMISMN/'Bracket ''B'' check',05.3"), "keywords must be upper-case: {printed}");
    assert!(
        printed.contains("F(c1)=FEAT/CIRCLE,INNER,CART,3.000,+0.250,-1.50,0,0,1,12.500"),
        "label prefix upper-cased, name preserved: {printed}"
    );

    let reparsed = parse(printed.as_bytes(), None).expect("reparse printed");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}

/// AC-3 & REQ-IO.DMIS.PARSE.PRINT-003: a 300+ column statement wrapped at width 80 produces
/// lines all <= 80 columns and re-parses to the identical tree (ignoring positions).
#[test]
fn ac3_wraps_300_column_statement_to_width_80() {
    let mut src = String::from("F(PAT1)=FEAT/PATERN");
    for i in 1..=35 {
        src.push_str(&format!(",F(HOLE_{i:02})"));
    }
    src.push('\n');
    assert!(src.len() > 300, "test input must exceed 300 columns");

    let tree = parse(src.as_bytes(), None).expect("parse long statement");
    let opts = PrintOptions { width: 80, crlf: false };
    let printed = print(&tree, &opts);

    let lines: Vec<&str> = printed.lines().collect();
    assert!(lines.len() >= 4, "300+ col statement should wrap into multiple lines");
    for (idx, line) in lines.iter().enumerate() {
        assert!(line.len() <= 80, "line {} exceeded 80 columns ({} bytes): {line}", idx + 1, line.len());
    }
    // Every non-final line ends with the continuation marker `$`.
    for line in &lines[..lines.len() - 1] {
        assert!(line.ends_with('$'), "continued line must end with $: {line}");
    }

    let reparsed = parse(printed.as_bytes(), None).expect("reparse wrapped");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}

/// REQ-IO.DMIS.PARSE.PRINT-003: when a single token exceeds `width`, it stands alone on its line
/// and the statement still round-trips cleanly.
#[test]
fn oversized_single_token_stands_alone_on_its_line() {
    let long_str = format!("'{}'", "A".repeat(100));
    let input = format!("DMISMN/{long_str},05.3\n");
    let tree = parse(input.as_bytes(), None).expect("parse oversized token");
    let printed = print(&tree, &PrintOptions { width: 80, crlf: false });

    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), 3, "expected header line, standalone token line, tail line: {printed}");
    assert_eq!(lines[0], "DMISMN/$");
    assert_eq!(lines[1], format!("{long_str}$"));
    assert_eq!(lines[2], ",05.3");

    let reparsed = parse(printed.as_bytes(), None).expect("reparse oversized token");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}

/// AC-4: a tree with `Unknown` nodes prints their raw tokens verbatim and re-parses to the same
/// `Unknown` nodes (including unregistered keywords and no-major-word statements).
#[test]
fn ac4_unknown_nodes_print_raw_tokens_and_reparse() {
    let input = b"V(X)=custom_op/'raw',3.000,(1,2\n5,6\n";
    let tree = parse(input, None).expect("parse unknown statements");
    assert_eq!(tree.statements.len(), 2);
    assert!(matches!(tree.statements[0], Node::Unknown(_)));
    assert!(matches!(tree.statements[1], Node::Unknown(_)));

    let printed = print(&tree, &PrintOptions::default());
    assert!(
        printed.contains("V(X)=custom_op/'raw',3.000,(1,2"),
        "unknown node must preserve raw token spelling: {printed}"
    );
    assert!(printed.contains("5,6"), "no-major-word unknown preserved: {printed}");

    let reparsed = parse(printed.as_bytes(), None).expect("reparse unknown");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}

/// REQ-IO.DMIS.PARSE.PRINT-004: D0 determinism and CRLF vs LF line endings.
#[test]
fn determinism_and_line_endings() {
    let input = b"DMISMN/'det',05.3\nUNITS/MM,ANGDEC\nENDFIL\n";
    let tree = parse(input, None).expect("parse");

    let lf_a = print(&tree, &PrintOptions { width: 80, crlf: false });
    let lf_b = print(&tree, &PrintOptions { width: 80, crlf: false });
    assert_eq!(lf_a.as_bytes(), lf_b.as_bytes());
    assert!(!lf_a.contains('\r'));

    let crlf_a = print(&tree, &PrintOptions { width: 80, crlf: true });
    let crlf_b = print(&tree, &PrintOptions { width: 80, crlf: true });
    assert_eq!(crlf_a.as_bytes(), crlf_b.as_bytes());
    assert_eq!(crlf_a, lf_a.replace('\n', "\r\n"));

    let reparsed = parse(crlf_a.as_bytes(), None).expect("reparse crlf");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}

/// Embedded comments in a continuation chain, TEXT raw text (including `$$`-prefixed raw text),
/// and non-UTF-8 raw bytes inside comments/strings all round-trip losslessly.
#[test]
fn embedded_comments_raw_text_and_non_utf8_bytes() {
    let input = b"T(D1)=TOL/DIAM,-0.01,0.01,$\n\
                  $$ mid-chain comment\n\
                  MMC\n\
                  TEXT/OPER,Clamp the part, then press start\n\
                  TEXT/OPER,$$not a comment because after comma\n\
                  DMISMN/'\xff\xfe',05.3\n";
    let reg = Registry::builtin().expect("builtin registry");
    let tree = parse_in(input, &reg, None, ParseOptions::default()).expect("parse edge cases");

    let bytes = print_bytes(&tree, &PrintOptions { width: 20, crlf: false });
    let reparsed = parse_in(&bytes, &reg, None, ParseOptions::default()).expect("reparse edge cases");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}

/// Adjacent tokens in Unknown statements that would fuse if printed without a space (Word + Word,
/// Word + LParen, Number + Number, Str + Str) are separated safely.
#[test]
fn token_separation_prevents_lex_fusion() {
    let input = b"UNKNOWN_KW FOO (1) 2 3 'a' 'b'\n";
    let tree = parse(input, None).expect("parse");
    let printed = print(&tree, &PrintOptions::default());
    let reparsed = parse(printed.as_bytes(), None).expect("reparse");
    assert!(trees_equal_ignoring_pos(&tree, &reparsed));
}
