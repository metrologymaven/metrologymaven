//! Acceptance criteria of spec node IO.REG.ENV.REPORT (AC-1 .. AC-4) and the strict
//! reading, capping and display rules behind them. Golden JSON comes from the independent
//! stdlib-Python reference `fixtures/io/reg/env/report/make_fixtures.py`; other expected
//! values are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::report::{ENTITY_TYPES_CAPPED, LOSSY_CAPPED, MAX_ENTITY_TYPES, MAX_LOSSY};
use fid_io_reg::{
    Code, ExportReport, FormatId, ImportReport, Location, Position, ReportBuilder, RootHandle, TypeName, UntrustedText,
    MAX_LISTED,
};
use fid_types::Digest;

const BASIC: &[u8] = include_bytes!("../../../fixtures/io/reg/env/report/basic.json");
const BASIC_EXPORT: &[u8] = include_bytes!("../../../fixtures/io/reg/env/report/basic_export.json");

fn ty(s: &str) -> TypeName {
    TypeName::new(s).unwrap()
}

fn code(s: &str) -> Code {
    Code::new(s).unwrap()
}

fn loc(s: &str) -> Location {
    Location::handle(RootHandle::parse(s).unwrap()).unwrap()
}

fn fmt_id() -> FormatId {
    FormatId::new("fid-test-points").unwrap()
}

fn none() -> UntrustedText {
    UntrustedText::new("")
}

/// Same inputs as `basic_import()` in make_fixtures.py.
fn basic_import() -> ImportReport {
    let mut b = ReportBuilder::new();
    for t in ["point_set", "mesh", "point_set", "mesh", "point_set"] {
        b.count(&ty(t), 1);
    }
    b.warn(&code("unit_assumed"), None, &none());
    b.warn(&code("nan_coordinate"), Some(Position::line_col(120, 4, 7)), &UntrustedText::new("x=NaN"));
    b.warn(
        &code("unknown_header"),
        Some(Position::byte(8)),
        &UntrustedText::new("evil\u{202e}txt.exe\nnext\tline \"q\" \\ \u{1}\u{7f}"),
    );
    let long = format!("{}{}", "\u{20ac}".repeat(200), "\u{1F600}".repeat(100));
    b.warn(&code("long_comment"), Some(Position::line_col(300, 9, 1)), &UntrustedText::new(&long));
    b.drop_item(
        &ty("polyline"),
        Position::line_col(512, 20, 3),
        &code("unsupported_entity"),
        &UntrustedText::new("POLYLINE 7 vertices"),
    );
    b.finish_import(fmt_id(), loc("root://in/ok.ftp"), false)
}

/// Same inputs as `basic_export()` in make_fixtures.py.
fn basic_export() -> ExportReport {
    let mut b = ReportBuilder::new();
    b.lossy(&ty("mesh.normals"));
    b.lossy(&ty("color"));
    b.lossy(&ty("color"));
    b.drop_item(&ty("polyline"), Position::byte(0), &code("not_representable"), &none());
    b.warn(&code("precision_reduced"), None, &none());
    let sha = Digest::parse(&format!("sha256:{}", "ab".repeat(32))).unwrap();
    b.finish_export(fmt_id(), loc("root://out/a.ftp"), sha, 1234, true)
}

#[test]
fn ac1_basic_import_matches_golden_json() {
    let r = basic_import();
    assert_eq!(r.canonical_bytes().unwrap(), BASIC);
    assert_eq!(r.warnings_total(), 4);
    assert_eq!(r.dropped_total(), 1);
    assert_eq!(r.entity_counts().get(&ty("point_set")), Some(&3));
    assert_eq!(r.entity_counts().get(&ty("mesh")), Some(&2));
    // Digest is SHA-256 of exactly those bytes.
    let want = Digest::from_bytes(fid_canon::digest_bytes(BASIC).0);
    assert_eq!(r.digest().unwrap(), want);
    // Strict read-back gives the same report.
    assert_eq!(ImportReport::from_json(BASIC).unwrap(), r);
}

#[test]
fn export_report_matches_golden_json() {
    let r = basic_export();
    assert_eq!(r.canonical_bytes().unwrap(), BASIC_EXPORT);
    let lossy: Vec<&str> = r.lossy().iter().map(TypeName::as_str).collect();
    assert_eq!(lossy, ["color", "mesh.normals", "polyline"]);
    assert_eq!(ExportReport::from_json(BASIC_EXPORT).unwrap(), r);
}

#[test]
fn ac2_dropped_items_are_capped_with_exact_total() {
    let mut b = ReportBuilder::new();
    let (k, why) = (ty("point"), code("nan_coordinate"));
    for i in 0..25_000u64 {
        b.drop_item(&k, Position::byte(i), &why, &none());
        b.warn(&why, Some(Position::byte(i)), &none());
    }
    let r = b.finish_import(fmt_id(), loc("root://in/big.ftp"), false);
    assert_eq!(r.dropped_total(), 25_000);
    assert_eq!(r.dropped().len(), MAX_LISTED);
    assert_eq!(r.warnings_total(), 25_000);
    assert_eq!(r.warnings().len(), MAX_LISTED);
    // The first 10,000, in source order.
    for (i, d) in r.dropped().iter().enumerate() {
        assert_eq!(d.pos.byte, i as u64);
    }
    for (i, w) in r.warnings().iter().enumerate() {
        assert_eq!(w.pos.unwrap().byte, i as u64);
    }
    let json = r.canonical_bytes().unwrap();
    let text = std::str::from_utf8(&json).unwrap();
    assert!(text.contains("\"dropped_total\":\"25000\""));
    assert_eq!(ImportReport::from_json(&json).unwrap(), r);
}

#[test]
fn ac3_long_detail_is_truncated_untrusted_text() {
    let mut b = ReportBuilder::new();
    b.warn(&code("long_comment"), None, &UntrustedText::new(&"x".repeat(1_000)));
    let r = b.finish_import(fmt_id(), loc("root://in/a.ftp"), false);
    let d = &r.warnings()[0].detail;
    assert_eq!(d.char_len(), 256);
    assert!(d.truncated());
    assert_eq!(d.text(), &UntrustedText::new(&"x".repeat(256)));
}

#[test]
fn truncation_boundary_counts_unicode_scalar_values() {
    let mk = |s: &str| fid_io_reg::ReportText::new(&UntrustedText::new(s));
    let at = mk(&"\u{1F600}".repeat(256));
    assert_eq!((at.char_len(), at.truncated()), (256, false));
    let over = mk(&"\u{1F600}".repeat(257));
    assert_eq!((over.char_len(), over.truncated()), (256, true));
    assert_eq!(over.text(), &UntrustedText::new(&"\u{1F600}".repeat(256)));
    let empty = mk("");
    assert_eq!((empty.char_len(), empty.truncated()), (0, false));
}

#[test]
fn ac4_insertion_order_of_entity_types_does_not_matter() {
    let build = |order: &[(&str, u64)]| {
        let mut b = ReportBuilder::new();
        for (t, n) in order {
            b.count(&ty(t), *n);
        }
        b.finish_import(fmt_id(), loc("root://in/a.ftp"), true)
    };
    let a = build(&[("point_set", 3), ("mesh", 2), ("curve", 1)]);
    let b = build(&[("curve", 1), ("mesh", 1), ("point_set", 3), ("mesh", 1)]);
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    assert_eq!(a.digest().unwrap(), b.digest().unwrap());
    let c = build(&[("curve", 1), ("mesh", 2), ("point_set", 4)]);
    assert_ne!(a.digest().unwrap(), c.digest().unwrap());
}

#[test]
fn entity_types_and_lossy_names_are_bounded() {
    let mut b = ReportBuilder::new();
    for i in 0..MAX_ENTITY_TYPES + 5 {
        b.count(&ty(&format!("t{i}")), 1);
        b.lossy(&ty(&format!("f{i}")));
    }
    assert_eq!(b.warnings_total(), 2);
    let e = b.clone().finish_export(fmt_id(), loc("root://out/a.ftp"), Digest::TOMBSTONE, 0, false);
    assert_eq!(e.lossy().len(), MAX_LOSSY);
    assert_eq!(e.warnings()[1].code.as_str(), LOSSY_CAPPED);
    let r = b.finish_import(fmt_id(), loc("root://in/a.ftp"), false);
    assert_eq!(r.entity_counts().len(), MAX_ENTITY_TYPES);
    assert_eq!(r.warnings()[0].code.as_str(), ENTITY_TYPES_CAPPED);
    assert_eq!(ImportReport::from_json(&r.canonical_bytes().unwrap()).unwrap(), r);
    assert_eq!(ExportReport::from_json(&e.canonical_bytes().unwrap()).unwrap(), e);
}

#[test]
fn names_are_validated() {
    for ok in ["a", "unit_assumed", "x9_y"] {
        assert!(Code::new(ok).is_ok(), "{ok}");
    }
    for bad in ["", "A", "9a", "a-b", "a.b", "_a", "a b", "\u{e9}", "a".repeat(65).as_str()] {
        assert!(Code::new(bad).is_err(), "{bad:?}");
    }
    assert!(TypeName::new("mesh.normals").is_ok());
    for bad in ["mesh.", ".mesh", "mesh..n", "Mesh"] {
        assert!(TypeName::new(bad).is_err(), "{bad:?}");
    }
    assert!(FormatId::new("stl-binary").is_ok());
    assert!(FormatId::new("STL").is_err());
    assert!(FormatId::new("-x").is_err());
    assert!(Location::human_path("root://in/a").is_err());
    assert!(Location::human_path("").is_err());
    assert!(Location::human_path(&"p".repeat(4_097)).is_err());
}

/// Replaces the first occurrence of `from` in the golden JSON and expects rejection.
fn rejects(from: &str, to: &str) {
    let src = std::str::from_utf8(BASIC).unwrap();
    assert!(src.contains(from), "{from}");
    let bad = src.replacen(from, to, 1);
    let err = ImportReport::from_json(bad.as_bytes()).unwrap_err();
    assert!(err.code().starts_with("report_"), "{from} -> {to}: {err}");
}

#[test]
fn strict_reading_rejects_bad_reports() {
    // Unknown fields, at every level.
    rejects("{\"dropped\":", "{\"extra\":1,\"dropped\":");
    rejects("\"truncated\":false}", "\"truncated\":false,\"x\":null}");
    rejects("\"col\":3,", "\"col\":3,\"col2\":3,");
    // Missing field.
    rejects("\"dry_run\":false,", "");
    // Duplicate key (fid-canon refuses).
    rejects("\"dry_run\":false,", "\"dry_run\":false,\"dry_run\":true,");
    // Wrong types and malformed counts.
    rejects("\"dropped_total\":\"1\"", "\"dropped_total\":1");
    rejects("\"dropped_total\":\"1\"", "\"dropped_total\":\"01\"");
    rejects("\"mesh\":\"2\"", "\"mesh\":\"-2\"");
    rejects("\"mesh\":\"2\"", "\"mesh\":\"18446744073709551616\"");
    rejects("\"col\":3", "\"col\":0");
    rejects("\"col\":3", "\"col\":3.5");
    rejects("\"dry_run\":false", "\"dry_run\":0");
    // Totals inconsistent with the listed items.
    rejects("\"warnings_total\":\"4\"", "\"warnings_total\":\"5\"");
    rejects("\"dropped_total\":\"1\"", "\"dropped_total\":\"0\"");
    // Bad names.
    rejects("\"code\":\"unit_assumed\"", "\"code\":\"Unit\"");
    rejects("\"kind\":\"polyline\"", "\"kind\":\"poly line\"");
    rejects("\"format\":\"fid-test-points\"", "\"format\":\"\"");
    rejects("\"source\":\"root://in/ok.ftp\"", "\"source\":\"root://in/../x\"");
    rejects("\"source\":\"root://in/ok.ftp\"", "\"source\":\"\"");
    // Truncation flag must agree with the kept length.
    rejects("\"text\":\"x=NaN\",\"truncated\":false", "\"text\":\"x=NaN\",\"truncated\":true");
    let src = std::str::from_utf8(BASIC).unwrap();
    let long = format!("\"text\":\"{}\",\"truncated\":false", "y".repeat(257));
    let bad = src.replacen("\"text\":\"x=NaN\",\"truncated\":false", &long, 1);
    assert!(ImportReport::from_json(bad.as_bytes()).is_err());
    // An import report is not an export report and vice versa.
    assert!(ExportReport::from_json(BASIC).is_err());
    assert!(ImportReport::from_json(BASIC_EXPORT).is_err());
}

#[test]
fn strict_reading_rejects_bad_export_reports() {
    let src = std::str::from_utf8(BASIC_EXPORT).unwrap();
    for (from, to) in [
        ("[\"color\",\"mesh.normals\",\"polyline\"]", "[\"mesh.normals\",\"color\",\"polyline\"]"),
        ("[\"color\",\"mesh.normals\",\"polyline\"]", "[\"color\",\"color\",\"mesh.normals\",\"polyline\"]"),
        ("\"sha256\":\"sha256:ab", "\"sha256\":\"sha256:AB"),
        ("\"bytes\":\"1234\"", "\"bytes\":\"1234\",\"lossy_total\":\"3\""),
    ] {
        assert!(src.contains(from), "{from}");
        let bad = src.replacen(from, to, 1);
        assert!(ExportReport::from_json(bad.as_bytes()).is_err(), "{to}");
    }
}

#[test]
fn display_escapes_untrusted_text_and_keeps_items_on_one_line() {
    let r = basic_import();
    let shown = r.to_string();
    // Hand-labelled: the bidi override, newline, tab, control and DEL are escaped, the
    // backslash doubled; the raw characters never appear.
    assert!(shown
        .contains("  unknown_header at byte 8: \"evil\\u{202e}txt.exe\\u{a}next\\u{9}line \"q\" \\\\ \\u{1}\\u{7f}\""));
    for c in ['\u{202e}', '\t', '\u{1}', '\u{7f}'] {
        assert!(!shown.contains(c), "{c:?}");
    }
    assert_eq!(shown.lines().count(), 1 + 1 + 1 + 4 + 1 + 1);
    assert!(shown.starts_with("import fid-test-points from root://in/ok.ftp\nentities: mesh=2 point_set=3\n"));
    assert!(shown.contains("  long_comment at byte 300 (line 9, column 1): \""));
    assert!(shown.contains("(truncated)"));
    assert!(shown.contains("  polyline at byte 512 (line 20, column 3): unsupported_entity: \"POLYLINE 7 vertices\""));

    let human = Location::human_path("C:\\in\\\u{202e}gpj.exe").unwrap();
    assert_eq!(human.to_string(), "C:\\\\in\\\\\\u{202e}gpj.exe");

    let e = basic_export().to_string();
    assert!(e.starts_with("export fid-test-points to root://out/a.ftp (dry run)\n"));
    assert!(e.contains("lossy: color mesh.normals polyline\n"));
}

#[test]
fn equal_reports_have_equal_digests_and_dry_run_is_part_of_it() {
    let a = basic_import();
    assert_eq!(a.digest().unwrap(), basic_import().digest().unwrap());
    let mut b = ReportBuilder::new();
    b.count(&ty("mesh"), 2);
    let dry = b.clone().finish_import(fmt_id(), loc("root://in/a.ftp"), true);
    let wet = b.finish_import(fmt_id(), loc("root://in/a.ftp"), false);
    assert_ne!(dry.digest().unwrap(), wet.digest().unwrap());
}
