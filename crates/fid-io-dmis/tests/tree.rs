//! Acceptance criteria of spec leaf IO.DMIS.PARSE.TREE (AC-1, AC-2, AC-3, AC-5), the bounds
//! and a robustness property. Expected trees come from
//! `fixtures/io/dmis/parse/tree/make_expected.py`, an independent Python reference, or are
//! hand-labelled here; none is produced by this crate. Every DMIS snippet and every test
//! descriptor is written by the project (they are not the DMIS grammar).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_dmis::lex::{lex_bytes, LexError, LexOptions, Relaxation, Token, TokenKind};
use fid_io_dmis::registry::{Dialect, LabelKind, Registry, Shape, StatementDescriptor};
use fid_io_dmis::tree::{
    parse, parse_in, parse_with, Arg, DmisTree, Item, Node, ParseError, ParseOptions, TreeBound, UnknownReason,
};
use fid_io_reg::{Budget, LimitRequest, Principal};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/dmis/parse/tree")
}

fn d(class: &str, major: &str, label: LabelKind, shape: Shape) -> StatementDescriptor {
    StatementDescriptor::new(class, major, label, shape, "TEST", "P1a")
}

fn req(p: &str) -> LabelKind {
    LabelKind::Required(p.into())
}

/// The test registry; mirrors `REGISTRY` in make_expected.py.
fn test_registry() -> Registry {
    use Shape::{AnyField, AnyWord, Number, RawText, Str};
    let cart_pol = || Shape::alt(vec![Shape::kw("CART"), Shape::kw("POL")]);
    let inner = Shape::repeat(Shape::alt(vec![Number, Shape::group(Shape::repeat(Number, 1, Some(3)))]), 1, Some(3));
    Registry::from_descriptors([
        d(
            "feat_point",
            "FEAT",
            req("F"),
            Shape::seq(vec![Shape::kw("POINT"), cart_pol(), Shape::repeat(Number, 6, Some(6))]),
        ),
        d(
            "feat_circle",
            "FEAT",
            req("F"),
            Shape::seq(vec![
                Shape::kw("CIRCLE"),
                Shape::alt(vec![Shape::kw("INNER"), Shape::kw("OUTER")]),
                cart_pol(),
                Shape::repeat(Number, 6, Some(6)),
                Number,
            ]),
        ),
        d(
            "tol_diam",
            "TOL",
            req("T"),
            Shape::seq(vec![
                Shape::kw("DIAM"),
                Number,
                Number,
                Shape::opt(Shape::alt(vec![Shape::kw("MMC"), Shape::kw("LMC")])),
            ]),
        ),
        d(
            "units",
            "UNITS",
            LabelKind::None,
            Shape::seq(vec![
                Shape::alt(vec![Shape::kw("MM"), Shape::kw("INCH")]),
                Shape::opt(Shape::alt(vec![Shape::kw("ANGDEC"), Shape::kw("ANGDMS")])),
            ]),
        ),
        d(
            "text",
            "TEXT",
            LabelKind::None,
            Shape::seq(vec![Shape::alt(vec![Shape::kw("OPER"), Shape::kw("OUTFIL")]), Shape::alt(vec![RawText, Str])]),
        ),
        d("output", "OUTPUT", LabelKind::None, Shape::repeat(Shape::Label(None), 1, None)),
        d(
            "nest",
            "NESTT",
            LabelKind::Optional("V".into()),
            Shape::seq(vec![AnyWord, Shape::opt(Shape::group(inner)), Shape::opt(AnyField)]),
        ),
        d("endfil", "ENDFIL", LabelKind::None, Shape::seq(vec![])),
    ])
    .unwrap()
}

fn opts(lenient: bool) -> ParseOptions {
    ParseOptions { lenient }
}

fn esc(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(char::from(c)),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s
}

fn render_fields(args: &[Arg]) -> String {
    let fields: Vec<String> = args
        .iter()
        .map(|a| {
            let items: Vec<String> = a
                .items
                .iter()
                .map(|it| match it {
                    Item::Atom(t) => format!("{}:{}", t.kind.name(), esc(t.text.as_bytes())),
                    Item::Group(g) => format!("({})", render_fields(&g.args)),
                })
                .collect();
            items.join(" ")
        })
        .collect();
    fields.join(",")
}

fn spellings(tokens: &[&Token]) -> String {
    let mut row = vec!["tokens".to_string()];
    row.extend(tokens.iter().map(|t| esc(t.text.as_bytes())));
    row.join("\t")
}

/// The same text format as `make_expected.py`, for one mode.
fn render(result: &Result<DmisTree, ParseError>, lenient: bool) -> String {
    let mut rows = vec![format!("mode\t{}", if lenient { "lenient" } else { "strict" })];
    match result {
        Ok(tree) => {
            for node in &tree.statements {
                let p = node.pos();
                let pos = format!("{}:{}", p.line, p.col);
                match node {
                    Node::Comment(_) => rows.push(format!("comment\t{pos}")),
                    Node::Unknown(u) => rows.push(format!(
                        "unknown\t{pos}\t{}\t{}",
                        u.keyword.as_deref().unwrap_or("-"),
                        u.reason.map_or("-", UnknownReason::as_str)
                    )),
                    Node::Statement(s) => rows.push(format!(
                        "statement\t{pos}\t{}\t{}\t{}\t{}",
                        s.class,
                        s.label.as_ref().map_or("-".to_string(), |l| esc(l.token.text.as_bytes())),
                        s.major_word(),
                        if s.slash.is_some() { format!("/{}", render_fields(&s.args)) } else { "-".to_string() }
                    )),
                }
                rows.push(spellings(&node.tokens()));
            }
        }
        Err(ParseError::Malformed { line, col, class, .. }) => {
            rows.push(format!("error\tmalformed\t{line}\t{col}\t{class}"))
        }
        Err(ParseError::LimitExceeded { bound, line }) => {
            rows.push(format!("error\tlimit_exceeded\t{}\t{line}", bound.as_str()))
        }
        Err(ParseError::Lex(e)) => match &**e {
            LexError::Malformed { line, col, reason } => {
                rows.push(format!("error\tlex\tmalformed\t{}\t{line}\t{col}", reason.as_str()))
            }
            LexError::LimitExceeded { bound, line } => {
                rows.push(format!("error\tlex\tlimit_exceeded\t{}\t{line}", bound.as_str()))
            }
            other => rows.push(format!("error\t{other}")),
        },
        Err(other) => rows.push(format!("error\t{other}")),
    }
    rows.into_iter().map(|r| r + "\n").collect()
}

fn dmi_fixtures() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("dmi"))
        .collect();
    out.sort();
    out
}

#[test]
fn every_fixture_matches_the_python_reference() {
    let registry = test_registry();
    let files = dmi_fixtures();
    assert!(files.len() >= 10, "fixtures missing");
    for path in files {
        let data = std::fs::read(&path).unwrap();
        let mut got = String::new();
        for lenient in [false, true] {
            got.push_str(&render(&parse_in(&data, &registry, None, opts(lenient)), lenient));
        }
        let want = std::fs::read_to_string(path.with_extension("tree")).unwrap();
        assert_eq!(got, want, "{}", path.display());
    }
}

#[test]
fn ac1_known_statement_and_unknown_statement_with_positions() {
    let data = std::fs::read(fixtures().join("mixed.dmi")).unwrap();
    let tree = parse_in(&data, &test_registry(), None, ParseOptions::default()).unwrap();
    assert_eq!(tree.statements.len(), 2);
    let Node::Statement(s) = &tree.statements[0] else { panic!("expected a statement") };
    assert_eq!(&*s.class, "feat_point");
    assert_eq!((s.pos.line, s.pos.col), (1, 1));
    let label = s.label.as_ref().unwrap();
    assert_eq!((label.prefix(), label.name()), ("F", "P1"));
    assert_eq!(s.major_word(), "FEAT");
    assert_eq!(s.args.len(), 8);
    assert_eq!(s.args[2].atom().unwrap().text.as_bytes(), b"10");
    let Node::Unknown(u) = &tree.statements[1] else { panic!("expected an unknown node") };
    assert_eq!(u.keyword.as_deref(), Some("FOO"));
    assert_eq!(u.reason, None);
    assert_eq!((u.pos.line, u.pos.col), (2, 1));
    assert_eq!(u.tokens.len(), 5);
}

#[test]
fn ac2_too_few_arguments_is_malformed_or_unknown_when_lenient() {
    let data = b"F(P1)=FEAT/POINT,CART,1,2";
    let registry = test_registry();
    let err = parse_in(data, &registry, None, ParseOptions::default()).unwrap_err();
    assert_eq!(err.code(), "malformed");
    match err {
        ParseError::Malformed { line, col, class, statement } => {
            assert_eq!(&*class, "feat_point");
            assert_eq!((line, col), (1, 25));
            assert_eq!((statement.line, statement.col), (1, 1));
        }
        other => panic!("{other:?}"),
    }
    let tree = parse_in(data, &registry, None, opts(true)).unwrap();
    let Node::Unknown(u) = &tree.statements[0] else { panic!("expected an unknown node") };
    assert_eq!(u.reason, Some(UnknownReason::ShapeMismatch));
    assert_eq!(u.keyword.as_deref(), Some("FEAT"));
    assert_eq!((u.pos.line, u.pos.col), (1, 1));
}

#[test]
fn ac3_the_tree_gives_back_every_token_of_every_fixture() {
    let registry = test_registry();
    let mut checked = 0;
    for path in dmi_fixtures() {
        let data = std::fs::read(&path).unwrap();
        let Ok(tree) = parse_in(&data, &registry, None, opts(true)) else { continue };
        let lexed: Vec<Token> =
            lex_bytes(&data, &LexOptions::strict()).unwrap().into_iter().flat_map(|s| s.tokens).collect();
        let got: Vec<Token> = tree.tokens().into_iter().cloned().collect();
        assert_eq!(got, lexed, "{}", path.display());
        checked += 1;
    }
    assert!(checked >= 10);
}

fn vendor_dialect() -> Dialect {
    Dialect {
        name: "vendor".into(),
        descriptors: vec![StatementDescriptor::new(
            "vendor_x",
            "VENDOR",
            LabelKind::None,
            Shape::seq(vec![Shape::kw("X"), Shape::Number]),
            "IO.DMIS.DIAL",
            "P1a",
        )],
        lex: LexOptions::strict(),
    }
}

#[test]
fn ac5_dialect_descriptor_applies_only_when_selected() {
    let data = b"VENDOR/X,1\n";
    let dialect = vendor_dialect();
    for registry in [Registry::builtin().unwrap(), test_registry()] {
        let plain = parse_in(data, &registry, None, ParseOptions::default()).unwrap();
        let Node::Unknown(u) = &plain.statements[0] else { panic!("expected an unknown node") };
        assert_eq!(u.keyword.as_deref(), Some("VENDOR"));
        let with = parse_in(data, &registry, Some(&dialect), ParseOptions::default()).unwrap();
        let Node::Statement(s) = &with.statements[0] else { panic!("expected a statement") };
        assert_eq!(&*s.class, "vendor_x");
    }
    let Node::Statement(_) = &parse(data, Some(&dialect)).unwrap().statements[0] else {
        panic!("expected a statement")
    };
    let Node::Unknown(_) = &parse(data, None).unwrap().statements[0] else { panic!("expected an unknown node") };
}

#[test]
fn dialect_with_a_duplicate_class_is_a_registry_error() {
    let mut dialect = vendor_dialect();
    dialect.descriptors[0].class = "units".into();
    let err = parse_in(b"UNITS/MM\n", &test_registry(), Some(&dialect), ParseOptions::default()).unwrap_err();
    assert_eq!(err.code(), "duplicate_class");
}

#[test]
fn dialect_lex_options_are_used_and_inline_comments_are_kept() {
    let mut dialect = vendor_dialect();
    dialect.lex = LexOptions::strict().with(Relaxation::InlineComment);
    let data = b"UNITS/MM $$ trailing\n";
    let tree = parse_in(data, &test_registry(), Some(&dialect), ParseOptions::default()).unwrap();
    let Node::Statement(s) = &tree.statements[0] else { panic!("expected a statement") };
    assert_eq!(&*s.class, "units");
    assert_eq!(s.trivia.len(), 1);
    assert_eq!(s.trivia[0].before, 3);
    let spelled: Vec<&[u8]> = tree.tokens().iter().map(|t| t.text.as_bytes()).collect();
    assert_eq!(spelled, vec![&b"UNITS"[..], b"/", b"MM", b"$$ trailing"]);
    // Strict lexing rejects the same input.
    assert_eq!(parse_in(data, &test_registry(), None, ParseOptions::default()).unwrap_err().code(), "malformed");
}

#[test]
fn backtracking_is_bounded_per_statement() {
    let registry = Registry::from_descriptors([d(
        "expl",
        "EXPL",
        LabelKind::None,
        Shape::seq(vec![Shape::repeat(Shape::alt(vec![Shape::AnyWord, Shape::AnyWord]), 0, None), Shape::kw("ZZZ")]),
    )])
    .unwrap();
    let mut data = b"EXPL/".to_vec();
    data.extend(std::iter::repeat(&b"A,"[..]).take(30).flatten());
    data.extend_from_slice(b"B\n");
    for lenient in [false, true] {
        let err = parse_in(&data, &registry, None, opts(lenient)).unwrap_err();
        assert_eq!(err, ParseError::LimitExceeded { bound: TreeBound::MatchSteps, line: 1 });
        assert_eq!(err.code(), "limit_exceeded");
    }
    // A short statement of the same shape is fine.
    let tree = parse_in(b"EXPL/A,B,ZZZ\n", &registry, None, ParseOptions::default()).unwrap();
    assert!(matches!(&tree.statements[0], Node::Statement(_)));
}

#[test]
fn byte_budget_of_the_reader_harness_applies() {
    let budget = Budget::new(Principal::Script, &LimitRequest::default().bytes(8)).unwrap();
    let err =
        parse_with(b"UNITS/MM,ANGDEC\n", &test_registry(), &LexOptions::strict(), opts(true), budget).unwrap_err();
    assert!(matches!(err, ParseError::Lex(_)), "{err:?}");
}

#[test]
fn builtin_registry_is_empty_until_grammar_families_are_built() {
    let registry = Registry::builtin().unwrap();
    assert!(registry.is_empty());
    let tree = parse(b"F(P1)=FEAT/POINT,CART,1,2,3,0,0,1\n", None).unwrap();
    let Node::Unknown(u) = &tree.statements[0] else { panic!("expected an unknown node") };
    assert_eq!(u.keyword.as_deref(), Some("FEAT"));
}

/// xorshift64*, so the property test needs no dependency and is reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn pick<'a>(&mut self, items: &[&'a [u8]]) -> &'a [u8] {
        items[(self.next() % items.len() as u64) as usize]
    }
}

#[test]
fn random_statements_never_panic_and_lenient_trees_are_lossless() {
    let registry = test_registry();
    let pieces: &[&[u8]] = &[
        b"F(P1)=",
        b"V(N1)=",
        b"FEAT",
        b"NESTT",
        b"OUTPUT",
        b"UNITS",
        b"TOL",
        b"TEXT",
        b"ENDFIL",
        b"FOO",
        b"/",
        b",",
        b",",
        b"(",
        b")",
        b"=",
        b"POINT",
        b"CART",
        b"MM",
        b"FA(P1)",
        b"1",
        b"-2.5",
        b"'s'",
        b" ",
        b"$\n",
        b"\n",
        b"\n$$ c\n",
        b"A",
        b"*",
        b"(((",
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut parsed = 0;
    for _ in 0..3000 {
        let n = 1 + (rng.next() % 24) as usize;
        let mut data = Vec::new();
        for _ in 0..n {
            data.extend_from_slice(rng.pick(pieces));
        }
        let strict = parse_in(&data, &registry, None, opts(false));
        let lenient = parse_in(&data, &registry, None, opts(true));
        if let Ok(tree) = &lenient {
            let lexed: Vec<Token> =
                lex_bytes(&data, &LexOptions::strict()).unwrap().into_iter().flat_map(|s| s.tokens).collect();
            let got: Vec<Token> = tree.tokens().into_iter().cloned().collect();
            assert_eq!(got, lexed, "{:?}", String::from_utf8_lossy(&data));
            parsed += 1;
        }
        match (&strict, &lenient) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            (Ok(_), Err(e)) => panic!("strict accepted what lenient refused: {e:?}"),
            (Err(ParseError::Malformed { .. }), Ok(_)) => {}
            (Err(a), Err(b)) => assert_eq!(a.code(), b.code()),
            (Err(e), Ok(_)) => panic!("unexpected strict error {e:?}"),
        }
    }
    assert!(parsed > 300, "too few inputs parsed: {parsed}");
}

#[test]
fn token_kinds_in_items_are_the_lexer_kinds() {
    let tree = parse_in(b"NESTT/A,(1,2),B='x'\n", &test_registry(), None, ParseOptions::default()).unwrap();
    let Node::Statement(s) = &tree.statements[0] else { panic!("expected a statement") };
    assert!(matches!(s.args[0].atom().unwrap().kind, TokenKind::Word { .. }));
    let Some(Item::Group(g)) = s.args[1].single() else { panic!("expected a group") };
    assert_eq!(g.args.len(), 2);
    assert_eq!(s.args[2].items.len(), 3);
}
