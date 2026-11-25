//! Acceptance criteria of spec node IO.CAD.IGES.SECT (AC-1 .. AC-6), the requirements'
//! error paths, and the writer. Expected values come from the independent Python oracle
//! `fixtures/io/cad/iges/sect/make_fixtures.py`, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_iges::{
    parse, parse_opts, parse_with, write, Bound, DirEntry, IgesError, IgesFile, IgesParam, IgesText, IgesUnit, Lexeme,
    ParseOptions, Section, UnitSource,
};
use fid_io_reg::{BoundedSource, Budget, LimitRequest, Principal};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cad/iges/sect")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

const ACCEPT: &[&str] = &["three_entities", "d_exponent", "hollerith", "reals", "unit_name", "legacy_layout"];
const CANONICAL: &[&str] = &["three_entities", "d_exponent", "hollerith", "reals", "unit_name"];

fn text(t: &IgesText) -> String {
    t.to_untrusted().as_display().to_string()
}

fn dump_param(p: &IgesParam) -> String {
    match p {
        IgesParam::Int(v) => format!("int|{v}"),
        IgesParam::Ptr(v) => format!("int|{v}"),
        IgesParam::Real(v, l) => format!("real|{:016x}|{}", v.to_bits(), l.as_str()),
        IgesParam::Str(t) => format!("str|{}", text(t)),
        IgesParam::Default => "default".to_owned(),
    }
}

/// The parse result in the oracle's dump format (see make_fixtures.py).
fn dump(f: &IgesFile) -> String {
    let mut out = String::new();
    for s in &f.start {
        writeln!(out, "start|{}", text(s)).unwrap();
    }
    writeln!(out, "delims|{}|{}", char::from(f.global.param_delim), char::from(f.global.record_delim)).unwrap();
    for (i, p) in f.global.params.iter().enumerate() {
        writeln!(out, "g|{}|{}", i + 3, dump_param(p)).unwrap();
    }
    let u = &f.units;
    writeln!(
        out,
        "unit|{}|{}|{}|{:016x}",
        u.source().as_str(),
        u.unit().name(),
        u.factor_decimal(),
        u.factor_m().to_bits()
    )
    .unwrap();
    for (seq, e) in &f.directory {
        let s = e.status;
        writeln!(
            out,
            "de|{seq}|type={}|ptr={}|structure={}|font={}|level={}|view={}|xform={}|labeldisp={}|\
             status={:02}{:02}{:02}{:02}|weight={}|color={}|lines={}|form={}|subscript={}|res1={}|res2={}|label={}",
            e.entity_type,
            e.param_ptr,
            e.structure,
            e.line_font,
            e.level,
            e.view,
            e.transform,
            e.label_display,
            s.blank,
            s.subordinate,
            s.entity_use,
            s.hierarchy,
            e.line_weight,
            e.color,
            e.param_line_count,
            e.form,
            e.subscript,
            text(&e.reserved1),
            text(&e.reserved2),
            text(&e.label),
        )
        .unwrap();
    }
    for (seq, ps) in &f.params {
        for (i, p) in ps.iter().enumerate() {
            writeln!(out, "p|{seq}|{i}|{}", dump_param(p)).unwrap();
        }
    }
    out
}

fn label(e: &DirEntry) -> String {
    text(&e.label)
}

#[test]
fn every_accepted_fixture_matches_the_oracle_dump() {
    for stem in ACCEPT {
        let f = parse(&read(&format!("{stem}.igs"))).unwrap_or_else(|e| panic!("{stem}: {e}"));
        let want = String::from_utf8(read(&format!("expected/{stem}.dump"))).unwrap();
        assert_eq!(dump(&f), want, "{stem}");
    }
}

/// AC-1: three DEs (point, line, arc) with types, labels and parameter lists; unit mm,
/// factor 0.001.
#[test]
fn ac1_three_entities() {
    let f = parse(&read("three_entities.igs")).unwrap();
    let keys: Vec<u32> = f.directory.keys().copied().collect();
    assert_eq!(keys, [1, 3, 5]);
    let types: Vec<i32> = f.directory.values().map(|e| e.entity_type).collect();
    assert_eq!(types, [116, 110, 100]);
    let labels: Vec<String> = f.directory.values().map(label).collect();
    assert_eq!(labels, ["PT1", "LN1", "ARC1"]);
    let lens: Vec<usize> = f.params.values().map(Vec::len).collect();
    assert_eq!(lens, [4, 6, 7]);
    let point: Vec<f64> = f.params[&1].iter().take(3).map(|p| p.as_f64().unwrap()).collect();
    assert_eq!(point, [10.0, 20.0, 0.0]);
    assert_eq!(f.params[&1][3], IgesParam::Int(0));
    assert_eq!(f.units.unit(), IgesUnit::Millimetre);
    assert_eq!(f.units.source(), UnitSource::Flag);
    assert_eq!(f.units.factor_decimal(), "0.001");
    assert_eq!(f.units.factor_ratio(), (1, 1000));
    assert_eq!(f.units.factor_m().to_bits(), 0.001f64.to_bits());
    assert!(!f.units.factor_m_is_exact());
    assert_eq!(f.global.unit_flag(), Some(2));
    assert_eq!(f.global.model_space_scale(), Some(1.0));
    assert_eq!(f.global.min_resolution(), Some(0.0001));
    assert_eq!(f.global.max_coordinate(), Some(100.0));
    assert_eq!(f.global.native_system_id().unwrap().as_display().to_string(), "MM-ORACLE");
    assert_eq!(f.start.len(), 2);
}

/// AC-2 / REQ-001: a parameter pointer outside the Parameter Data section.
#[test]
fn ac2_bad_pointer_names_the_entry() {
    let e = parse(&read("bad_pointer.igs")).unwrap_err();
    assert_eq!(e.code(), "malformed", "{e}");
    assert_eq!(e.de(), Some(3));
    assert!(matches!(e, IgesError::Malformed { reason: "param_pointer_out_of_range", .. }), "{e}");
    assert_eq!(e.position().unwrap().line, Some(8));
}

/// AC-3 / REQ-004: a Terminate count that disagrees with the Parameter Data section.
#[test]
fn ac3_bad_terminate_names_the_section() {
    let e = parse(&read("bad_terminate.igs")).unwrap_err();
    assert!(matches!(e, IgesError::TruncatedFile { section: Section::Parameter, .. }), "{e}");
    assert_eq!(e.code(), "truncated_file");
    let e = parse(&read("no_terminate.igs")).unwrap_err();
    assert!(matches!(e, IgesError::TruncatedFile { section: Section::Terminate, .. }), "{e}");
}

/// AC-4 / REQ-005: the `D` exponent form, value and lexeme.
#[test]
fn ac4_d_exponent() {
    let f = parse(&read("d_exponent.igs")).unwrap();
    let p = &f.params[&1];
    match &p[0] {
        IgesParam::Real(v, l) => {
            assert_eq!(v.to_bits(), 0.0015f64.to_bits());
            assert_eq!(l.as_str(), "1.5D-3");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(p[2], IgesParam::Real(-325.0, Lexeme::new("-3.25D+2").unwrap()));
    assert_eq!(f.units.unit(), IgesUnit::Metre);
    assert!(f.units.factor_m_is_exact());
}

/// REQ-002: a back-pointer that does not equal its entry's sequence number.
#[test]
fn req002_back_pointer_mismatch_names_both_numbers() {
    let e = parse(&read("back_pointer.igs")).unwrap_err();
    assert!(
        matches!(e, IgesError::Malformed { reason: "back_pointer_mismatch", de: Some(3), back_pointer: Some(1), .. }),
        "{e}"
    );
    assert!(e.to_string().contains("directory entry 3, back-pointer 1"), "{e}");
}

/// REQ-003: unit flag, unit name, caller fallback, conflict and `unit_required`.
#[test]
fn req003_units() {
    let f = parse(&read("unit_name.igs")).unwrap();
    assert_eq!(f.units.unit(), IgesUnit::Inch);
    assert_eq!(f.units.source(), UnitSource::Name);
    assert_eq!(f.units.factor_decimal(), "0.0254");
    assert_eq!(f.units.factor_ratio(), (127, 5000));

    let missing = read("unit_missing.igs");
    assert_eq!(parse(&missing).unwrap_err(), IgesError::UnitRequired);
    let opts = ParseOptions { unit: Some(IgesUnit::Millimetre), ..ParseOptions::default() };
    let f = parse_opts(&missing, &opts).unwrap();
    assert_eq!(f.units.unit(), IgesUnit::Millimetre);
    assert_eq!(f.units.source(), UnitSource::Caller);

    let three = read("three_entities.igs");
    let f = parse_opts(&three, &opts).unwrap();
    assert_eq!(f.units.source(), UnitSource::Flag);
    let inch = ParseOptions { unit: Some(IgesUnit::Inch), ..ParseOptions::default() };
    assert_eq!(parse_opts(&three, &inch).unwrap_err().code(), "unit_conflict");

    // Hollerith fixture: flag 10 (cm); the reals fixture: flag 1 (inch).
    assert_eq!(parse(&read("hollerith.igs")).unwrap().units.unit(), IgesUnit::Centimetre);
    assert_eq!(parse(&read("reals.igs")).unwrap().units.unit(), IgesUnit::Inch);
}

/// REQ-005: correctly rounded reals with kept lexemes (values from Python's float()).
#[test]
fn req005_reals_round_correctly() {
    let f = parse(&read("reals.igs")).unwrap();
    let reals: Vec<(u64, String)> = f
        .params
        .values()
        .flatten()
        .filter_map(|p| match p {
            IgesParam::Real(v, l) => Some((v.to_bits(), l.as_str().to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(reals.len(), 12);
    // Hand-labelled spot checks; the full set is in expected/reals.dump.
    assert_eq!(reals[0].0, 9_007_199_254_740_992f64.to_bits());
    assert_eq!(reals[2].0, f64::MAX.to_bits());
    assert_eq!(reals[4].0, 1); // smallest subnormal
    assert_eq!(reals[5].0, 1); // just above half of it
    assert_eq!(reals[6].0, (-0.0f64).to_bits());
    assert_eq!(reals[8].0, 1.0f64.to_bits()); // tie to even
    assert_eq!(reals[9].0, (1.0f64 + f64::EPSILON).to_bits());
    assert_eq!(reals[11].1, "6.02214076d23");
    let ints = &f.params[&9];
    assert_eq!(ints[0], IgesParam::Int(i64::MAX));
    assert_eq!(ints[1], IgesParam::Int(i64::MIN));
}

#[test]
fn hollerith_strings_are_untrusted_text_with_delimiters_inside() {
    let f = parse(&read("hollerith.igs")).unwrap();
    assert_eq!(f.global.param_delim, b'/');
    assert_eq!(f.global.sender_product_id().unwrap().as_display().to_string(), "A,B;C/D");
    let note = &f.params[&1];
    match &note[1] {
        IgesParam::Str(t) => assert_eq!(t.len(), 147),
        other => panic!("{other:?}"),
    }
    assert_eq!(note[3], IgesParam::Default);
    assert_eq!(note[4], IgesParam::Str(IgesText::new("").unwrap()));
    assert_eq!(label(&f.directory[&1]), "NOTE 1");
}

/// AC-5 / REQ-006 over the IGES corpus manifest (IO.CORP.MANIF schema): digests match,
/// every `accept` entry survives parse -> write -> parse, every `reject` entry fails with
/// its code. Canonical fixtures are also written back byte for byte (the oracle's layout).
#[test]
fn ac5_corpus_manifest_round_trips() {
    let manifest: toml::Table = std::str::from_utf8(&read("iges.toml")).unwrap().parse().unwrap();
    assert_eq!(manifest["format"].as_str(), Some("iges"));
    let entries = manifest["entry"].as_array().unwrap();
    let (mut accepted, mut rejected, mut canonical) = (0, 0, 0);
    for entry in entries {
        let path = entry["path"].as_str().unwrap();
        let bytes = read(path);
        assert_eq!(fid_canon::digest_bytes(&bytes).to_string(), entry["sha256"].as_str().unwrap(), "{path}");
        let expect = entry["expect"].as_table().unwrap();
        if let Some(acc) = expect.get("accept") {
            let n = acc["counts"]["entities"].as_integer().unwrap();
            let f = parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(i64::try_from(f.directory.len()).unwrap(), n, "{path}");
            let written = write(&f).unwrap();
            let again = parse(&written).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(again.content_eq(&f), "{path}");
            assert_eq!(write(&again).unwrap(), written, "{path}: writer not idempotent");
            let tags = entry["tags"].as_array().unwrap();
            if tags.iter().any(|t| t.as_str() == Some("canonical")) {
                assert_eq!(String::from_utf8(written).unwrap(), String::from_utf8(bytes).unwrap(), "{path}");
                canonical += 1;
            }
            accepted += 1;
        } else {
            let code = expect["reject"]["error"].as_str().unwrap();
            let e = parse(&bytes).unwrap_err();
            assert_eq!(e.code(), code, "{path}: {e}");
            rejected += 1;
        }
    }
    assert_eq!((accepted, rejected, canonical), (ACCEPT.len(), 6, CANONICAL.len()));
}

#[test]
fn compressed_and_binary_forms_are_unsupported() {
    let e = parse(&read("compressed.igs")).unwrap_err();
    assert!(matches!(e, IgesError::FormUnsupported { form: "compressed", .. }), "{e}");
    let e = parse(b"B\x00\x01\x02binary").unwrap_err();
    assert!(matches!(e, IgesError::FormUnsupported { form: "binary", .. }), "{e}");
}

// ----- error paths on mutated copies of three_entities.igs ------------------------------

/// Lines 0-1 S, 2-4 G, 5-10 D, 11-13 P, 14 T.
fn three_lines() -> Vec<String> {
    String::from_utf8(read("three_entities.igs")).unwrap().lines().map(str::to_owned).collect()
}

fn join(lines: &[String]) -> Vec<u8> {
    let mut s = lines.join("\n");
    s.push('\n');
    s.into_bytes()
}

/// Replaces columns `from..from + new.len()` of line `idx`.
fn patch(idx: usize, from: usize, new: &str) -> Vec<u8> {
    let mut lines = three_lines();
    lines[idx].replace_range(from..from + new.len(), new);
    join(&lines)
}

/// Line 0 with one more column.
fn long_line() -> Vec<u8> {
    let mut lines = three_lines();
    lines[0].push('X');
    join(&lines)
}

/// Sets byte `col` of line `idx` (or, with `None`, cuts the line to `col` bytes).
fn patch_bytes(idx: usize, col: usize, byte: Option<u8>) -> Vec<u8> {
    let mut lines: Vec<Vec<u8>> = three_lines().into_iter().map(String::into_bytes).collect();
    match byte {
        Some(b) => lines[idx][col] = b,
        None => lines[idx].truncate(col),
    }
    let mut out = lines.join(&b'\n');
    out.push(b'\n');
    out
}

#[test]
fn malformed_inputs_are_classified() {
    let cases: Vec<(&str, Vec<u8>, &str, Option<u32>)> = vec![
        ("long line", long_line(), "line_length", None),
        ("short line", patch_bytes(0, 0, None), "line_length", None),
        ("non-ascii", patch_bytes(0, 3, Some(0xe9)), "non_ascii", None),
        ("control byte", patch_bytes(0, 3, Some(0x09)), "non_ascii", None),
        ("section letter", patch(1, 72, "X"), "section_letter", None),
        ("sequence gap", patch(1, 73, "0000003"), "sequence", None),
        ("sequence in next section", patch(4, 72, "D"), "sequence", None),
        ("section order", patch(5, 72, "S"), "section_order", None),
        ("terminate format", patch(14, 8, "Q"), "terminate_format", None),
        ("global delimiter", patch(2, 0, "2H"), "global_delimiter", None),
        ("global hollerith count", patch(4, 49, "95H"), "hollerith_count", None),
        ("de type mismatch", patch(6, 0, "     117"), "de_type_mismatch", Some(1)),
        ("de field", patch(5, 24, "     1x1"), "de_field", Some(1)),
        ("de status", patch(5, 64, "0000 000"), "de_status", Some(1)),
        ("negative pointer", patch(7, 8, "      -2"), "param_pointer_out_of_range", Some(3)),
        ("zero line count", patch(10, 24, "       0"), "param_pointer_out_of_range", Some(5)),
        ("line count overrun", patch(10, 24, "       2"), "param_pointer_out_of_range", Some(5)),
        ("entity type in record", patch(11, 0, "117"), "param_entity_type", Some(1)),
        ("missing record delimiter", patch(11, 19, " "), "missing_record_delimiter", Some(1)),
        ("real overflow", patch(11, 4, "1D999,1.0,"), "real_range", Some(1)),
        ("bad number", patch(11, 4, "1.0."), "number", Some(1)),
        ("integer overflow", patch(12, 0, "99999999999999999999,"), "integer_range", Some(3)),
        ("back-pointer field", patch(12, 64, "       x"), "back_pointer_field", Some(3)),
    ];
    for (name, input, reason, de) in cases {
        let e = parse(&input).unwrap_err();
        assert_eq!(e.code(), "malformed", "{name}: {e}");
        if let IgesError::Malformed { reason: got, de: got_de, .. } = &e {
            assert_eq!(*got, reason, "{name}: {e}");
            assert_eq!(*got_de, de, "{name}: {e}");
        }
    }
    // Data after the Terminate line, and blank lines after it (allowed).
    let mut lines = three_lines();
    lines.push(String::new());
    lines.push(String::new());
    assert!(parse(&join(&lines)).is_ok());
    lines.push("junk".to_owned());
    let e = parse(&join(&lines)).unwrap_err();
    assert!(matches!(e, IgesError::Malformed { reason: "data_after_terminate", .. }), "{e}");
    // CRLF and lone CR line endings parse to the same file.
    let lf = parse(&join(&three_lines())).unwrap();
    let crlf = parse(three_lines().join("\r\n").as_bytes()).unwrap();
    let cr = parse(three_lines().join("\r").as_bytes()).unwrap();
    assert_eq!(crlf, lf);
    assert_eq!(cr, lf);
    assert_eq!(parse(b"").unwrap_err().code(), "truncated_file");
}

#[test]
fn limits_are_charged() {
    let input = read("three_entities.igs");
    let opts = ParseOptions { max_token_bytes: 3, ..ParseOptions::default() };
    let e = parse_opts(&input, &opts).unwrap_err();
    assert!(matches!(e, IgesError::LimitExceeded { bound: Bound::Token, value: 3, .. }), "{e}");

    let tight = |req: LimitRequest| {
        let mut budget = Budget::new(Principal::Script, &req).unwrap();
        parse_with(&input, &mut budget, &ParseOptions::default())
    };
    let e = tight(LimitRequest::default().bytes(100)).unwrap_err();
    assert!(matches!(e, IgesError::LimitExceeded { bound: Bound::Bytes, .. }), "{e}");
    let e = tight(LimitRequest::default().alloc(1000)).unwrap_err();
    assert!(matches!(e, IgesError::LimitExceeded { bound: Bound::Alloc, .. }), "{e}");
    let e = tight(LimitRequest::default().elements(5)).unwrap_err();
    assert!(matches!(e, IgesError::LimitExceeded { bound: Bound::Elements, .. }), "{e}");
    assert!(tight(LimitRequest::default()).is_ok());

    // The same through a BoundedSource.
    let budget = Budget::new(Principal::Script, &LimitRequest::default().bytes(100)).unwrap();
    let e = fid_io_iges::parse_source(BoundedSource::new(&input[..], budget), &ParseOptions::default()).unwrap_err();
    assert!(matches!(e, IgesError::LimitExceeded { bound: Bound::Bytes, .. }), "{e}");
    let budget = Budget::new(Principal::Script, &LimitRequest::default()).unwrap();
    let f = fid_io_iges::parse_source(BoundedSource::new(&input[..], budget), &ParseOptions::default()).unwrap();
    assert_eq!(f.directory.len(), 3);
}

#[test]
fn every_prefix_fails_cleanly() {
    let bytes = read("hollerith.igs");
    for n in 0..bytes.len() {
        if let Ok(f) = parse(&bytes[..n]) {
            // Only the final line ending may be missing.
            assert!(n + 1 >= bytes.len(), "prefix {n} parsed to {} entries", f.directory.len());
        }
    }
}

// ----- writer ----------------------------------------------------------------------------

fn built() -> IgesFile {
    let mut f = parse(&read("three_entities.igs")).unwrap();
    f.params.get_mut(&1).unwrap()[3] = IgesParam::Ptr(5);
    f
}

#[test]
fn writer_accepts_pointers_and_computed_reals() {
    let mut f = built();
    f.params.get_mut(&3).unwrap().push(IgesParam::real(0.1).unwrap());
    f.params.get_mut(&3).unwrap().push(IgesParam::real(1e300).unwrap());
    let again = parse(&write(&f).unwrap()).unwrap();
    assert!(again.content_eq(&f));
    assert_eq!(again.params[&1][3], IgesParam::Int(5));
    assert_eq!(again.params[&1][3].as_ptr(), Some(5));
    assert_eq!(IgesParam::real(f64::NAN), None);
}

/// A change applied to a valid file before writing it.
type Change = Box<dyn Fn(&mut IgesFile)>;

#[test]
fn writer_rejects_what_the_form_cannot_hold() {
    let cases: Vec<(&str, Change)> = vec![
        ("global_delimiter", Box::new(|f| f.global.record_delim = b',')),
        ("global_delimiter", Box::new(|f| f.global.param_delim = b'1')),
        ("line_content", Box::new(|f| f.start.push(IgesText::new(&"x".repeat(73)).unwrap()))),
        (
            "real_lexeme_mismatch",
            Box::new(|f| f.params.get_mut(&1).unwrap()[0] = IgesParam::Real(2.0, Lexeme::new("1.0").unwrap())),
        ),
        (
            "directory_sequence",
            Box::new(|f| {
                let e = f.directory.remove(&5).unwrap();
                f.directory.insert(7, e);
            }),
        ),
        ("params_missing", Box::new(|f| drop(f.params.remove(&3)))),
        ("params_without_entry", Box::new(|f| drop(f.params.insert(99, Vec::new())))),
        ("de_field_width", Box::new(|f| f.directory.get_mut(&1).unwrap().color = 100_000_000)),
        ("de_text_width", Box::new(|f| f.directory.get_mut(&1).unwrap().label = IgesText::new("NINECHARS").unwrap())),
        ("de_text_width", Box::new(|f| f.directory.get_mut(&1).unwrap().label = IgesText::new(" PT").unwrap())),
        ("de_text_width", Box::new(|f| f.directory.get_mut(&1).unwrap().label = IgesText::new("PT ").unwrap())),
        ("de_text_width", Box::new(|f| f.directory.get_mut(&1).unwrap().reserved1 = IgesText::new(" ").unwrap())),
        ("de_status", Box::new(|f| f.directory.get_mut(&1).unwrap().status.hierarchy = 100)),
    ];
    for (reason, change) in cases {
        let mut f = built();
        change(&mut f);
        match write(&f) {
            Err(IgesError::Unwritable { reason: got, .. }) => assert_eq!(got, reason),
            other => panic!("{reason}: {other:?}"),
        }
    }
    assert_eq!(IgesText::new("tab\there"), None);
    assert_eq!(Lexeme::new("1"), None);
    assert_eq!(Lexeme::new("1.0D999"), None);
}

/// An empty file (no entries) and a Global section with no parameters beyond the
/// delimiters both round-trip.
#[test]
fn minimal_files_round_trip() {
    let mut f = parse(&read("d_exponent.igs")).unwrap();
    f.directory = BTreeMap::new();
    f.params = BTreeMap::new();
    let again = parse(&write(&f).unwrap()).unwrap();
    assert!(again.content_eq(&f));
    let mut g = f.clone();
    g.global.params.clear();
    let bytes = write(&g).unwrap();
    assert!(String::from_utf8(bytes.clone()).unwrap().contains(&format!("{:<72}G0000001\n", "1H,,1H;;")));
    let opts = ParseOptions { unit: Some(IgesUnit::Metre), ..ParseOptions::default() };
    let again = parse_opts(&bytes, &opts).unwrap();
    assert!(again.global.content_eq(&g.global));
}

/// AC-6 on stable: a deterministic mutation run over the fixtures (the cargo-fuzz target
/// `iges_parse` covers the same property with libFuzzer). Never panics, and every
/// accepted input round-trips through the writer.
#[test]
fn ac6_mutated_inputs_never_panic_and_accepted_ones_round_trip() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    const BYTES: &[u8] = b",;/H1D9E.-+ \n\rSGDPTF0\xff";
    let mut accepted = 0;
    for stem in ACCEPT.iter().chain(["bad_pointer", "back_pointer"].iter()) {
        let base = read(&format!("{stem}.igs"));
        let mut accepted_here = 0;
        for _ in 0..1500 {
            let mut m = base.clone();
            for _ in 0..(1 + next() % 3) {
                let at = (next() as usize) % (m.len() + 1);
                match next() % 8 {
                    0 if at < m.len() => {
                        m.remove(at);
                    }
                    1 => m.insert(at, BYTES[(next() as usize) % BYTES.len()]),
                    2 => m.truncate(at),
                    _ if at < m.len() => m[at] = BYTES[(next() as usize) % BYTES.len()],
                    _ => {}
                }
            }
            // The file's own unit first; a caller unit only when the file declares none, so
            // files declaring any unit reach the write check (a caller unit would conflict).
            let mut opts = ParseOptions::default();
            let f = match parse_opts(&m, &opts) {
                Ok(f) => f,
                Err(IgesError::UnitRequired) => {
                    opts.unit = Some(IgesUnit::Millimetre);
                    match parse_opts(&m, &opts) {
                        Ok(f) => f,
                        Err(_) => continue,
                    }
                }
                Err(_) => continue,
            };
            accepted += 1;
            accepted_here += 1;
            let written = write(&f).unwrap_or_else(|e| panic!("{e}"));
            let again = parse_opts(&written, &opts).unwrap_or_else(|e| panic!("{e}"));
            assert!(again.content_eq(&f));
        }
        if ACCEPT.contains(stem) {
            assert!(accepted_here > 0, "{stem}: no mutation reached the write check");
        }
        eprintln!("{stem}: {accepted_here} of 1500 mutations accepted");
    }
    assert!(accepted > 100, "{accepted}");
}
