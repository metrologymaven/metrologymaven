//! Acceptance criteria of spec node IO.CAD.STEP.P21 (AC-1 .. AC-6) and the error paths.
//! Expected values are hand-labelled here or come from
//! `fixtures/io/cad/step/p21/make_expected.py`, an independent stdlib Python reader;
//! none are produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::{BoundedSource, Budget, LimitRequest, Principal, UntrustedText};
use fid_io_step::{
    parse, parse_source, parse_with, print, print_with, Bound, DanglingRef, P21File, Param, ParseOptions, PrintOptions,
    RealStyle, StepError, StepText,
};
use std::path::{Path, PathBuf};

const ACCEPT: &[&str] = &["minimal", "dangling", "escapes", "reals"];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cad/step/p21")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

fn names(file: &P21File, n: u64) -> Vec<String> {
    file.get(n).unwrap().names().map(|i| i.as_str().to_owned()).collect()
}

fn param(file: &P21File, n: u64, i: usize) -> &Param {
    &file.get(n).unwrap().parts[0].1[i]
}

fn wrap(body: &str) -> String {
    format!("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\n{body}\nENDSEC;\nEND-ISO-10303-21;\n")
}

fn summary(file: &P21File) -> String {
    let mut s = format!("header {}\ninstances {}\n", file.header.len(), file.report.instances);
    for d in &file.report.dangling {
        s.push_str(&format!("dangling {} {}\n", d.from, d.to));
    }
    s
}

#[test]
fn ac1_minimal_has_twelve_instances_a_complex_one_and_a_decoded_string() {
    let bytes = read("minimal.stp");
    let file = parse(&bytes).unwrap();
    assert_eq!(file.data.len(), 12);
    assert_eq!(file.header.len(), 3);
    assert_eq!(names(&file, 9), ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"]);
    assert_eq!(names(&file, 4), ["AXIS2_PLACEMENT_3D"]);
    // 'Gr\X2\00FC00DF\X0\e' is "Grüße" (hand-labelled).
    let want = StepText::new("Gr\u{fc}\u{df}e");
    assert_eq!(param(&file, 6, 1), &Param::Str(want.clone()));
    assert_eq!(want.to_untrusted(), UntrustedText::new("Grüße"));
    // Offsets point at the '#'.
    let text = String::from_utf8(bytes.clone()).unwrap();
    for n in [1u64, 5, 9, 12] {
        let at = text.find(&format!("\n#{n}")).unwrap() + 1;
        assert_eq!(file.get(n).unwrap().offset, at as u64, "#{n}");
    }
    // Other parameter kinds (hand-labelled).
    assert_eq!(param(&file, 4, 1), &Param::Ref(1));
    assert_eq!(param(&file, 6, 2), &Param::Unset);
    assert_eq!(param(&file, 12, 3), &Param::Int(-7));
    match param(&file, 12, 1) {
        Param::Bin(b) => {
            assert_eq!(b.len(), 1);
            assert_eq!(b.get(0), Some(true));
        }
        other => panic!("{other:?}"),
    }
    match param(&file, 10, 1) {
        Param::Typed(t, inner) => {
            assert_eq!(t.as_str(), "LENGTH_MEASURE");
            match &**inner {
                Param::Real(v, Some(l)) => {
                    assert_eq!(*v, 25.4);
                    assert_eq!(l.as_str(), "25.4");
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    match param(&file, 5, 1) {
        Param::List(xs) => {
            let lex: Vec<_> = xs
                .iter()
                .map(|p| match p {
                    Param::Real(_, Some(l)) => l.as_str().to_owned(),
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(lex, ["12.5", "-3.25E-2", "1.0E+02"]);
        }
        other => panic!("{other:?}"),
    }
    assert!(file.report.dangling.is_empty());
    // The intern table is sorted and holds each name once.
    let table: Vec<&str> = file.names.iter().map(|i| i.as_str()).collect();
    let mut sorted = table.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(table, sorted);
}

#[test]
fn ac2_duplicate_entity_is_malformed_naming_the_number() {
    let err = parse(&read("dup_entity.stp")).unwrap_err();
    match &err {
        StepError::Malformed { reason, entity, at } => {
            assert_eq!(*reason, "duplicate_entity");
            assert_eq!(*entity, Some(7));
            let text = String::from_utf8(read("dup_entity.stp")).unwrap();
            assert_eq!(at.byte, (text.rfind("#7=").unwrap()) as u64);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(err.code(), "malformed");
}

#[test]
fn ac3_dangling_reference_is_reported_and_resolution_fails() {
    let file = parse(&read("dangling.stp")).unwrap();
    assert_eq!(file.report.dangling, [DanglingRef { from: 5, to: 99 }]);
    let err = file.resolve(5, 99).unwrap_err();
    assert_eq!(err, StepError::DanglingReference { from: 5, to: 99 });
    assert_eq!(err.code(), "dangling_reference");
    assert_eq!(file.resolve(6, 5).unwrap().number, 5);
}

fn nested_lists(n: usize) -> String {
    wrap(&format!("#1=A({}{});", "(".repeat(n), ")".repeat(n)))
}

#[test]
fn ac4_nesting_beyond_64_fails_with_depth() {
    let ok = parse(nested_lists(64).as_bytes()).unwrap();
    assert_eq!(ok.report.max_depth, 64);
    match parse(nested_lists(65).as_bytes()).unwrap_err() {
        StepError::LimitExceeded { bound, value, .. } => {
            assert_eq!(bound, Bound::Depth);
            assert_eq!(value, 64);
        }
        other => panic!("{other:?}"),
    }
    // Typed parameters nest too.
    let typed = wrap(&format!("#1=A({}1{});", "T(".repeat(65), ")".repeat(65)));
    assert!(matches!(parse(typed.as_bytes()), Err(StepError::LimitExceeded { bound: Bound::Depth, .. })));
    // A very deep input fails fast, without recursion.
    let deep = nested_lists(1_000_000);
    assert!(matches!(parse(deep.as_bytes()), Err(StepError::LimitExceeded { bound: Bound::Depth, .. })));
}

#[test]
fn string_longer_than_one_mib_fails_with_string() {
    let at_limit = wrap(&format!("#1=A('{}');", "x".repeat(1 << 20)));
    assert!(parse(at_limit.as_bytes()).is_ok());
    let over = wrap(&format!("#1=A('{}');", "x".repeat((1 << 20) + 1)));
    let err = parse(over.as_bytes()).unwrap_err();
    assert!(matches!(err, StepError::LimitExceeded { bound: Bound::String, value: 1_048_576, .. }), "{err:?}");
    assert_eq!(err.code(), "limit_exceeded");
}

#[test]
fn instance_budget_file_size_and_tokens_are_bounded() {
    let body = "#1=A();\n#2=A();\n#3=A();";
    let opts = ParseOptions { max_instances: 3, ..ParseOptions::default() };
    let mut budget = Budget::new(Principal::Script, &LimitRequest::default()).unwrap();
    // One header record plus three instances exceeds 3.
    let err = parse_with(wrap(body).as_bytes(), &mut budget, &opts).unwrap_err();
    assert!(matches!(err, StepError::LimitExceeded { bound: Bound::Instances, value: 3, .. }), "{err:?}");

    let mut small = Budget::new(Principal::Script, &LimitRequest::default().bytes(16)).unwrap();
    let err = parse_with(wrap(body).as_bytes(), &mut small, &ParseOptions::default()).unwrap_err();
    assert!(matches!(err, StepError::LimitExceeded { bound: Bound::Bytes, .. }), "{err:?}");

    let minimal = read("minimal.stp");
    let tight = Budget::new(Principal::Script, &LimitRequest::default().bytes(100)).unwrap();
    let src = BoundedSource::new(&minimal[..], tight);
    let err = parse_source(src, &ParseOptions::default()).unwrap_err();
    assert!(matches!(err, StepError::LimitExceeded { bound: Bound::Bytes, .. }), "{err:?}");
    let src = BoundedSource::new(&minimal[..], Budget::new(Principal::Script, &LimitRequest::default()).unwrap());
    assert_eq!(parse_source(src, &ParseOptions::default()).unwrap().data.len(), 12);

    let long = wrap(&format!("#1=A({});", "1".repeat(5000)));
    assert!(matches!(parse(long.as_bytes()), Err(StepError::LimitExceeded { bound: Bound::Token, .. })));

    let mut few = Budget::new(Principal::Script, &LimitRequest::default().elements(5)).unwrap();
    let err = parse_with(wrap("#1=A(1,2,3,4,5,6);").as_bytes(), &mut few, &ParseOptions::default()).unwrap_err();
    assert!(matches!(err, StepError::LimitExceeded { bound: Bound::Elements, .. }), "{err:?}");
}

#[test]
fn ac5_parse_print_parse_is_identical_for_every_accepted_fixture() {
    for stem in ACCEPT {
        let file = parse(&read(&format!("{stem}.stp"))).unwrap();
        // Default print keeps authored lexemes: the reparse is identical, lexemes included.
        let printed = print(&file).unwrap();
        let again = parse(printed.as_bytes()).unwrap();
        assert!(again.content_eq(&file, true), "{stem}");
        assert_eq!(again.report.dangling, file.report.dangling, "{stem}");
        // Shortest-real print matches the independent Python reference byte for byte.
        let shortest = print_with(&file, &PrintOptions { reals: RealStyle::Shortest }).unwrap();
        let want = String::from_utf8(read(&format!("expected/{stem}.canon.stp"))).unwrap();
        assert_eq!(shortest, want, "{stem}");
        let reparsed = parse(shortest.as_bytes()).unwrap();
        assert!(reparsed.content_eq(&file, false), "{stem}");
        assert_eq!(print_with(&reparsed, &PrintOptions { reals: RealStyle::Shortest }).unwrap(), shortest);
        let want = String::from_utf8(read(&format!("expected/{stem}.summary.txt"))).unwrap();
        assert_eq!(summary(&file), want, "{stem}");
    }
}

#[test]
fn escapes_decode_to_hand_labelled_text() {
    let file = parse(&read("escapes.stp")).unwrap();
    let strs = |n: u64| -> Vec<StepText> {
        file.get(n).unwrap().parts[0]
            .1
            .iter()
            .map(|p| match p {
                Param::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    let want1 = [
        "it's",
        "back\\slash",
        "\u{c4}",
        "\u{e9}",
        "\u{3b1}\u{3b2}",
        "\u{1f600}",
        "caf\u{e9}",
        "na\u{ef}ve",
        "split across lines",
    ];
    assert_eq!(strs(1), want1.iter().map(|s| StepText::new(s)).collect::<Vec<_>>());
    let want2 = ["a\u{e9}b\u{1f600}c", "", "/* not a comment */"];
    assert_eq!(strs(2), want2.iter().map(|s| StepText::new(s)).collect::<Vec<_>>());
    assert_eq!(strs(3), [StepText::new("na\u{ef}ve")]);
    assert_eq!(strs(4), [StepText::new("\u{1f600}")]);
}

#[test]
fn reals_are_correctly_rounded_and_keep_their_lexeme() {
    let file = parse(&read("reals.stp")).unwrap();
    let xs = match param(&file, 1, 0) {
        Param::List(xs) => xs.clone(),
        other => panic!("{other:?}"),
    };
    let got: Vec<(u64, String)> = xs
        .iter()
        .map(|p| match p {
            Param::Real(v, Some(l)) => (v.to_bits(), l.as_str().to_owned()),
            other => panic!("{other:?}"),
        })
        .collect();
    // Hand-labelled bit patterns of selected entries.
    assert_eq!(got[0], (0, "0.".to_owned()));
    assert_eq!(got[1], (0x8000_0000_0000_0000, "-0.".to_owned()));
    assert_eq!(got[5].0, 0x3FB9_9999_9999_999A); // 0.1
    assert_eq!(got[11].0, 0x0010_0000_0000_0000); // smallest normal
    assert_eq!(got[12], (1, "4.9E-324".to_owned())); // smallest subnormal
    assert_eq!(got[13].0, 0x7FEF_FFFF_FFFF_FFFF); // largest finite
    assert_eq!(got[14].0, 0x3FD3_3333_3333_3334); // 0.30000000000000004
    assert_eq!(got[10].1, "1.e5");
    // A caller-supplied lexeme that does not match the value is not printed.
    let mut f = file.clone();
    if let Some(inst) = f.data.get_mut(&1) {
        if let Param::List(xs) = &mut inst.parts[0].1[0] {
            xs[0] = Param::Real(2.0, fid_io_step::Lexeme::new("3."));
        }
    }
    assert!(print(&f).unwrap().contains("REALS((2.,"));
}

#[test]
fn error_paths_are_classified() {
    let cases: &[(bool, &str, &str)] = &[
        (false, "ISO-10303-21;\nHEADER;\nENDSEC;\nANCHOR;\nENDSEC;\nEND-ISO-10303-21;\n", "step_feature_unsupported"),
        (
            false,
            "ISO-10303-21;\nHEADER;\nENDSEC;\nREFERENCE;\nENDSEC;\nEND-ISO-10303-21;\n",
            "step_feature_unsupported",
        ),
        (
            false,
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\nSIGNATURE;\n",
            "step_feature_unsupported",
        ),
        (
            false,
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA(('x'));\nENDSEC;\nEND-ISO-10303-21;\n",
            "step_feature_unsupported",
        ),
        (true, "#1=A('\\PB\\x');", "step_feature_unsupported"),
        (true, "@1=A();", "step_feature_unsupported"),
        (true, "#1=A('\\Q');", "malformed"),
        (true, "#1=A(1,);", "malformed"),
        (true, "#1=A(T(1,2));", "malformed"),
        (true, "#1=A(1e5);", "malformed"),
        (true, "#1=A(1.E999);", "malformed"),
        (true, "#1=A(99999999999999999999);", "malformed"),
        (true, "#1=A(#99999999999999999999);", "malformed"),
        (true, "#1=A(\"4F\");", "malformed"),
        (true, "#1=();", "malformed"),
        (true, "#1=A('\u{1}');", "malformed"),
        (false, "ISO-10303-22;\n", "malformed"),
        (false, "ISO-10303-21;\nHEADER;\nENDSEC;\nEND-ISO-10303-21;\n", "malformed"),
        (false, "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\njunk", "malformed"),
        (false, "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=A('open", "truncated_file"),
        (false, "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n/* open", "truncated_file"),
        (false, "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=A((1,2", "truncated_file"),
        (false, "", "truncated_file"),
    ];
    for &(wrapped, body, code) in cases {
        let input = if wrapped { wrap(body) } else { body.to_owned() };
        let err = parse(input.as_bytes()).unwrap_err();
        assert_eq!(err.code(), code, "{input:?}: {err}");
    }
}

#[test]
fn every_prefix_of_a_fixture_fails_cleanly() {
    let bytes = read("minimal.stp");
    for n in 0..bytes.len() {
        let r = parse(&bytes[..n]);
        // Only whitespace may follow the final ';'.
        if let Ok(f) = r {
            assert!(bytes[n..].iter().all(u8::is_ascii_whitespace), "prefix {n} parsed to {} instances", f.data.len());
        }
    }
}

/// AC-6 on stable: a deterministic mutation run over the fixtures (the cargo-fuzz target
/// `p21_parse` covers the same property with libFuzzer). Never panics, and every
/// accepted input round-trips through the printer.
#[test]
fn ac6_mutated_inputs_never_panic_and_accepted_ones_round_trip() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    const PIECES: &[&[u8]] = &[
        b"(",
        b")",
        b",",
        b";",
        b"'",
        b"\"",
        b"#",
        b"$",
        b"*",
        b".",
        b"\\",
        b"\\X2\\",
        b"\\X0\\",
        b"/*",
        b"*/",
        b"E",
        b"-",
        b"9",
        b"=",
        b"\n",
        b"\xff",
        b"\xc3\xa9",
        b"ENDSEC;",
        b"#1",
        b"T(",
        b"@",
    ];
    let mut accepted = 0;
    for stem in ACCEPT.iter().chain(["dup_entity"].iter()) {
        let base = read(&format!("{stem}.stp"));
        for _ in 0..1500 {
            let mut m = base.clone();
            for _ in 0..(1 + next() % 4) {
                let at = (next() as usize) % (m.len() + 1);
                match next() % 4 {
                    0 if at < m.len() => {
                        m.remove(at);
                    }
                    1 if at < m.len() => m[at] = next() as u8,
                    2 => m.truncate(at),
                    _ => {
                        let piece = PIECES[(next() as usize) % PIECES.len()];
                        m.splice(at..at, piece.iter().copied());
                    }
                }
            }
            if let Ok(file) = parse(&m) {
                accepted += 1;
                let printed = print(&file).unwrap();
                let again = parse(printed.as_bytes()).unwrap_or_else(|e| panic!("{e}: {printed}"));
                assert!(again.content_eq(&file, true), "{printed}");
            }
        }
    }
    assert!(accepted > 0);
}
