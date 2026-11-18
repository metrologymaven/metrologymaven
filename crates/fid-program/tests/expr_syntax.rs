//! Acceptance criteria of spec node AUTO.PROG.EXPR.GRAMMAR. Expected trees and error
//! offsets come from `fixtures/program/expr/make_cases.py` (an independent Python parser,
//! with hand-labelled offsets for the invalid corpus); bounds are spelled out from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{canonicalize, parse, to_canonical_bytes, CValue};
use fid_program::expr::syntax::{
    BinaryOp, DecText, Limit, Name, UnaryOp, UnitSym, MAX_DEPTH, MAX_SOURCE_BYTES, MAX_TOKENS,
};
use fid_program::{parse_expr, print_expr, Expr, ExprKind, ParseError, Span};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn dir(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/program/expr").join(sub)
}

/// `(name, contents)` of every `*.txt` in a corpus, sorted.
fn corpus(sub: &str) -> Vec<(String, String)> {
    let mut out: Vec<_> = std::fs::read_dir(dir(sub))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    out.sort();
    out
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

/// The fixture JSON shape (documented in make_cases.py).
fn to_json(e: &Expr) -> CValue {
    let span = CValue::Arr(vec![CValue::Num(e.span.start as f64), CValue::Num(e.span.end as f64)]);
    let mut f: Vec<(&str, CValue)> = match &e.kind {
        ExprKind::Int(v) => vec![("k", s("int")), ("v", s(&v.to_string()))],
        ExprKind::Dec(d) => vec![("k", s("dec")), ("v", s(d.as_str()))],
        ExprKind::Qty(d, u) => vec![("k", s("qty")), ("v", s(d.as_str())), ("unit", s(u.as_str()))],
        ExprKind::Str(v) => vec![("k", s("str")), ("v", s(v))],
        ExprKind::Bool(b) => vec![("k", s("bool")), ("v", CValue::Bool(*b))],
        ExprKind::Ident(n) => vec![("k", s("ident")), ("name", s(n.as_str()))],
        ExprKind::Field(b, n) => vec![("k", s("field")), ("base", to_json(b)), ("name", s(n.as_str()))],
        ExprKind::Index(b, i) => vec![("k", s("index")), ("base", to_json(b)), ("index", to_json(i))],
        ExprKind::Unary(op, a) => vec![("k", s("unary")), ("op", s(op.as_str())), ("arg", to_json(a))],
        ExprKind::Binary(op, l, r) => {
            vec![("k", s("binary")), ("op", s(op.as_str())), ("lhs", to_json(l)), ("rhs", to_json(r))]
        }
        ExprKind::Cond(c, a, b) => {
            vec![("k", s("cond")), ("if", to_json(c)), ("then", to_json(a)), ("else", to_json(b))]
        }
        ExprKind::Call(n, args) => {
            vec![("k", s("call")), ("name", s(n.as_str())), ("args", CValue::Arr(args.iter().map(to_json).collect()))]
        }
    };
    f.push(("span", span));
    obj(f)
}

fn children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Field(b, _) | ExprKind::Unary(_, b) => vec![&**b],
        ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) => vec![&**a, &**b],
        ExprKind::Cond(a, b, c) => vec![&**a, &**b, &**c],
        ExprKind::Call(_, args) => args.iter().collect(),
        _ => vec![],
    }
}

/// REQ-004: every span lies inside the source and inside its parent's span.
fn check_spans(e: &Expr, src: &str) {
    assert!(e.span.start <= e.span.end && e.span.end <= src.len(), "{src:?}: {:?}", e.span);
    assert!(src.is_char_boundary(e.span.start) && src.is_char_boundary(e.span.end));
    for c in children(e) {
        assert!(e.span.start <= c.span.start && c.span.end <= e.span.end, "{src:?}: child outside parent");
        check_spans(c, src);
    }
}

/// REQ-006 plus the printer's promises: reparses to the same tree, never longer, idempotent.
fn check_round_trip(e: &Expr, src: &str) {
    let printed = print_expr(e);
    let back = parse_expr(&printed).unwrap_or_else(|err| panic!("{src:?} printed as {printed:?}: {err}"));
    assert!(e.same_tree(&back), "{src:?} printed as {printed:?} parses to another tree");
    assert_eq!(print_expr(&back), printed, "printing is not idempotent for {src:?}");
    assert!(printed.len() <= src.len(), "{src:?} printed longer: {printed:?}");
    assert_eq!(back.span, Span::new(0, printed.len()), "{printed:?}");
}

/// AC-1: each valid fixture parses to the expected tree, spans included.
#[test]
fn ac1_valid_corpus_matches_reference_trees() {
    let valid = corpus("valid");
    assert!(valid.len() >= 40, "valid corpus is missing files");
    for (name, src) in valid {
        let e = parse_expr(&src).unwrap_or_else(|err| panic!("{name}: {err}"));
        let want = canonicalize(&std::fs::read(dir("valid").join(format!("{name}.json"))).unwrap()).unwrap();
        let got = to_canonical_bytes(&to_json(&e)).unwrap();
        assert_eq!(String::from_utf8(got).unwrap(), String::from_utf8(want).unwrap(), "{name}");
        check_spans(&e, &src);
        check_round_trip(&e, &src);
    }
}

fn number(v: &CValue) -> usize {
    match v {
        CValue::Num(f) => *f as usize,
        CValue::I64(i) => *i as usize,
        CValue::U64(u) => *u as usize,
        other => panic!("not a number: {other:?}"),
    }
}

/// AC-2: each invalid fixture fails with `expr_syntax` at the hand-labelled offset.
#[test]
fn ac2_invalid_corpus_fails_at_labelled_offset() {
    let expected = match parse(&std::fs::read(dir("invalid").join("expected.json")).unwrap()).unwrap() {
        CValue::Obj(m) => m,
        _ => panic!("expected.json is not an object"),
    };
    let invalid = corpus("invalid");
    assert_eq!(invalid.len(), expected.len(), "fixture set and expected.json disagree");
    for name in ["unbalanced-open-paren", "unbalanced-close-paren", "qty-unclosed", "assign", "increment"] {
        assert!(expected.contains_key(name), "AC-2 names {name}");
    }
    for (name, src) in invalid {
        let Some(CValue::Obj(want)) = expected.get(&name) else { panic!("{name}: no expectation") };
        assert_eq!(want.get("error"), Some(&CValue::Str("expr_syntax".into())));
        let offset = number(&want["offset"]);
        let err = parse_expr(&src).unwrap_err();
        assert_eq!(err.code(), "expr_syntax", "{name}");
        assert_eq!(err, ParseError::Syntax { offset }, "{name}: {src:?}");
    }
}

fn nested_parens(n: usize) -> String {
    format!("{}x{}", "(".repeat(n), ")".repeat(n))
}

/// AC-3: a 5 KiB source and 65 nested parentheses both fail with `expr_too_large`.
#[test]
fn ac3_bounds() {
    let big = "1+".repeat(2560);
    assert_eq!(big.len(), 5120);
    let err = parse_expr(&big).unwrap_err();
    assert_eq!((err.code(), err), ("expr_too_large", ParseError::TooLarge(Limit::Bytes)));

    let err = parse_expr(&nested_parens(65)).unwrap_err();
    assert_eq!((err.code(), err), ("expr_too_large", ParseError::TooLarge(Limit::Depth)));
    // The bound itself is accepted.
    let e = parse_expr(&nested_parens(64)).unwrap();
    assert_eq!(e.kind, ExprKind::Ident(Name::new("x").unwrap()));
    assert_eq!(e.span, Span::new(0, 129));

    // 4096 bytes are accepted, 4097 are not (bytes are checked before anything else).
    let at = format!("\"{}\"", "a".repeat(MAX_SOURCE_BYTES - 2));
    assert!(parse_expr(&at).is_ok());
    let over = format!("\"{}\"", "a".repeat(MAX_SOURCE_BYTES - 1));
    assert_eq!(parse_expr(&over), Err(ParseError::TooLarge(Limit::Bytes)));
    assert_eq!(parse_expr(&format!("{over})")), Err(ParseError::TooLarge(Limit::Bytes)));

    // f(a,a,...,a) with n arguments has 2n + 2 tokens.
    let call = |n: usize| format!("f({})", vec!["a"; n].join(","));
    assert!(parse_expr(&call((MAX_TOKENS - 2) / 2)).is_ok());
    assert_eq!(parse_expr(&call((MAX_TOKENS - 2) / 2 + 1)), Err(ParseError::TooLarge(Limit::Tokens)));
    // The token bound wins over a later syntax error.
    assert_eq!(parse_expr(&format!("{} )", call(520))), Err(ParseError::TooLarge(Limit::Tokens)));

    // Every construct counts towards depth.
    let chain = |n: usize| vec!["a"; n].join("+");
    assert!(parse_expr(&chain(MAX_DEPTH + 1)).is_ok());
    assert_eq!(parse_expr(&chain(MAX_DEPTH + 2)), Err(ParseError::TooLarge(Limit::Depth)));
    let neg = |n: usize| format!("{}x", "-".repeat(n));
    assert!(parse_expr(&neg(MAX_DEPTH)).is_ok());
    assert_eq!(parse_expr(&neg(MAX_DEPTH + 1)), Err(ParseError::TooLarge(Limit::Depth)));
    let nots = |n: usize| format!("{}x", "not ".repeat(n));
    assert_eq!(parse_expr(&nots(MAX_DEPTH + 1)), Err(ParseError::TooLarge(Limit::Depth)));
    let fields = |n: usize| format!("x{}", ".f".repeat(n));
    assert!(parse_expr(&fields(MAX_DEPTH)).is_ok());
    assert_eq!(parse_expr(&fields(MAX_DEPTH + 1)), Err(ParseError::TooLarge(Limit::Depth)));
    let index = |n: usize| format!("{}0{}", "a[".repeat(n), "]".repeat(n));
    assert!(parse_expr(&index(MAX_DEPTH)).is_ok());
    assert_eq!(parse_expr(&index(MAX_DEPTH + 1)), Err(ParseError::TooLarge(Limit::Depth)));
    let calls = |n: usize| format!("{}0{}", "f(".repeat(n), ")".repeat(n));
    assert!(parse_expr(&calls(MAX_DEPTH)).is_ok());
    assert_eq!(parse_expr(&calls(MAX_DEPTH + 1)), Err(ParseError::TooLarge(Limit::Depth)));
    let conds = |n: usize| format!("{}x{}", "if c then ".repeat(n), " else y".repeat(n));
    assert!(parse_expr(&conds(MAX_DEPTH)).is_ok());
    assert_eq!(parse_expr(&conds(MAX_DEPTH + 1)), Err(ParseError::TooLarge(Limit::Depth)));
    // Right-nested by parentheses inside a binary operator: a+(a+(a+...)).
    let right = |n: usize| format!("{}a{}", "a+(".repeat(n), ")".repeat(n));
    assert!(parse_expr(&right(MAX_DEPTH / 2)).is_ok());
    assert_eq!(parse_expr(&right(MAX_DEPTH / 2 + 1)), Err(ParseError::TooLarge(Limit::Depth)));

    // Hostile depth far beyond the bound fails cleanly (no stack overflow).
    let deep = "(".repeat(1000);
    assert_eq!(parse_expr(&deep), Err(ParseError::TooLarge(Limit::Depth)));
    let deep = "-".repeat(1000);
    assert_eq!(parse_expr(&deep), Err(ParseError::TooLarge(Limit::Depth)));
}

/// Hand-labelled checks of REQ-001 details the corpora do not pin down.
#[test]
fn req001_lexical_details() {
    let ok = |src: &str| parse_expr(src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
    // A number directly before `[` is a quantity; with a space it is an index.
    assert!(matches!(ok("2[mm]").kind, ExprKind::Qty(..)));
    assert!(matches!(ok("2 [0]").kind, ExprKind::Index(..)));
    // `2.x` is a field of the integer 2; `2.5.x` a field of the decimal.
    assert!(matches!(&ok("2.x").kind, ExprKind::Field(b, _) if b.kind == ExprKind::Int(2)));
    assert!(matches!(&ok("2.5.x").kind, ExprKind::Field(b, _) if matches!(b.kind, ExprKind::Dec(_))));
    // Quantity number text may exceed i64 (kept verbatim).
    assert!(matches!(ok("99999999999999999999[mm]").kind, ExprKind::Qty(..)));
    assert_eq!(ok("9223372036854775807").kind, ExprKind::Int(i64::MAX));
    // Escapes.
    assert_eq!(ok(r#""\u{1F600}\t""#).kind, ExprKind::Str("\u{1F600}\t".into()));
    assert_eq!(parse_expr(r#""\u{D800}""#), Err(ParseError::Syntax { offset: 1 }));
    assert_eq!(parse_expr(r#""\u{110000}""#), Err(ParseError::Syntax { offset: 1 }));
    assert_eq!(parse_expr(r#""\u{1234567}""#), Err(ParseError::Syntax { offset: 1 }));
    assert_eq!(parse_expr(r#""\u{}""#), Err(ParseError::Syntax { offset: 1 }));
    // Names: 64 bytes accepted, keywords refused.
    assert!(parse_expr(&"n".repeat(64)).is_ok());
    for k in fid_program::expr::syntax::KEYWORDS {
        assert!(Name::new(k).is_none());
    }
    // Whitespace-only source ends too early at its length.
    assert_eq!(parse_expr(" \n"), Err(ParseError::Syntax { offset: 2 }));
    // Errors are in source order: the parse error at 2 comes before the lexical one at 4.
    assert_eq!(parse_expr("a b = 1"), Err(ParseError::Syntax { offset: 2 }));
    assert_eq!(parse_expr("(a = 1"), Err(ParseError::Syntax { offset: 3 }));
    // Non-ASCII outside strings points at the character's first byte.
    assert_eq!(parse_expr("x + \u{00e9}"), Err(ParseError::Syntax { offset: 4 }));
}

/// REQ-005: no construct assigns, declares, loops, performs I/O or names a command.
#[test]
fn req005_no_state_changing_constructs() {
    for src in [
        "x = 1",
        "x := 1",
        "x += 1",
        "let x = 1",
        "var x",
        "for i in xs",
        "while true",
        "do(x) while y",
        "x => x",
        "\\x -> x",
        "probe.measure@1(x)",
        "a; b",
        "print(x); y",
        "{x}",
        "`ls`",
        "$x",
        "#x",
        "x?",
        "x!",
        "x ? 1 : 2",
    ] {
        let err = parse_expr(src).unwrap_err();
        assert_eq!(err.code(), "expr_syntax", "{src:?} parsed");
    }
}

// ------------------------------------------------------------------ AC-4 property test

/// SplitMix64 (public-domain reference algorithm).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const NAMES: [&str; 10] = ["x", "iffy", "notable", "_t", "r2", "andy", "or_", "THEN", "elsewhere", "true_pos"];
const UNITS: [&str; 10] = ["mm", "deg", "\u{00b5}m", "\u{03bc}m", "m/s^2", "%", "\u{00b0}C", "\u{03a9}", "N*m", "1/K"];
/// String characters, including every escape the printer writes.
const CHARS: &str = "aZ \"\\\n\r\t\u{1}\u{7f}\u{e9}\u{1F600}{}";

fn digits(rng: &mut Rng, max: usize) -> String {
    (0..1 + rng.below(max)).map(|_| char::from(b'0' + rng.below(10) as u8)).collect()
}

fn gen(rng: &mut Rng, depth: usize, budget: &mut usize) -> Expr {
    let leaf = depth == 0 || *budget == 0 || rng.below(4) == 0;
    *budget = budget.saturating_sub(1);
    let span = Span::default();
    let name = |rng: &mut Rng| Name::new(*rng.pick(&NAMES[..])).unwrap();
    let kind = if leaf {
        match rng.below(7) {
            0 => ExprKind::Int(if rng.below(4) == 0 { (rng.next() >> 1) as i64 } else { rng.below(1000) as i64 }),
            1 => ExprKind::Dec(DecText::new(&format!("{}.{}", digits(rng, 4), digits(rng, 4))).unwrap()),
            2 => {
                let text =
                    if rng.below(2) == 0 { digits(rng, 3) } else { format!("{}.{}", digits(rng, 2), digits(rng, 3)) };
                ExprKind::Qty(DecText::new(&text).unwrap(), UnitSym::new(*rng.pick(&UNITS[..])).unwrap())
            }
            3 => {
                let chars: Vec<char> = CHARS.chars().collect();
                ExprKind::Str((0..rng.below(8)).map(|_| *rng.pick(&chars)).collect())
            }
            4 => ExprKind::Bool(rng.below(2) == 0),
            _ => ExprKind::Ident(name(rng)),
        }
    } else {
        let mut sub = |rng: &mut Rng| Box::new(gen(rng, depth - 1, budget));
        match rng.below(6) {
            0 => ExprKind::Field(sub(rng), name(rng)),
            1 => ExprKind::Index(sub(rng), sub(rng)),
            2 => ExprKind::Unary(*rng.pick(&[UnaryOp::Neg, UnaryOp::Not]), sub(rng)),
            3 => ExprKind::Binary(*rng.pick(&BinaryOp::ALL), sub(rng), sub(rng)),
            4 => ExprKind::Cond(sub(rng), sub(rng), sub(rng)),
            _ => {
                let n = rng.below(4);
                let args = (0..n).map(|_| *sub(rng)).collect();
                ExprKind::Call(name(rng), args)
            }
        }
    };
    Expr::new(kind, span)
}

/// AC-4: 10 000 generated trees (seed 7) print and parse back to equal trees.
#[test]
fn ac4_print_then_parse_identity() {
    let mut rng = Rng(7);
    for i in 0..10_000 {
        let mut budget = 40;
        let e = gen(&mut rng, 6, &mut budget);
        let printed = print_expr(&e);
        let back = parse_expr(&printed).unwrap_or_else(|err| panic!("case {i}: {printed:?}: {err}"));
        assert!(e.same_tree(&back), "case {i}: {printed:?} parses to another tree");
        assert_eq!(print_expr(&back), printed, "case {i}");
        check_spans(&back, &printed);
        assert_eq!(back.span, Span::new(0, printed.len()), "case {i}: {printed:?}");
    }
}

/// Random hostile sources (seed 7): the parser never panics, errors point inside the source
/// at a character boundary, and whatever parses round-trips through the printer. A cheap
/// stand-in, run on every CI build, for the AC-5 fuzz target `expr_parse`.
#[test]
fn hostile_fragments_never_panic() {
    // `|`-separated so the list stays compact; `|` itself is not a fragment.
    const FRAGMENTS: &str = "a|x1|if|then|else|and|or|not|true|false|(|)|[|]|.|,|+|-|*|/|%|<|<=|>|>=|==|!=|=|!|0|007|\
                             12.5|2[mm]|3[|\"s\"|\"\\n\"|\"\\u{41}\"|\"|\\| |\t|\n|f(|\u{e9}|\u{00b5}|\
                             9999999999999999999|[\u{00b0}C]|e5|_|@|#|\u{1}";
    let fragments: Vec<&str> = FRAGMENTS.split('|').collect();
    let mut rng = Rng(7);
    let mut parsed = 0usize;
    for _ in 0..50_000 {
        let n = rng.below(24);
        let src: String = (0..n).map(|_| *rng.pick(&fragments)).collect();
        match parse_expr(&src) {
            Ok(e) => {
                parsed += 1;
                check_spans(&e, &src);
                check_round_trip(&e, &src);
            }
            Err(ParseError::Syntax { offset }) => {
                assert!(offset <= src.len() && src.is_char_boundary(offset), "{src:?}: {offset}")
            }
            Err(ParseError::TooLarge(l)) => panic!("{src:?}: unexpected {l:?}"),
        }
    }
    assert!(parsed > 300, "only {parsed} sources parsed; the generator is too hostile to cover printing");
}
