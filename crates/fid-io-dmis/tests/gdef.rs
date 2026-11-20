//! Acceptance criteria of spec leaf IO.DMIS.PARSE.GDEF (AC-1, AC-2, AC-4), error paths and the
//! step budget of the family's shapes. Expected views come from
//! `fixtures/io/dmis/parse/gdef/make_expected.py`, an independent Python reference, or are
//! hand-labelled here; none is produced by this crate. Every DMIS line in these tests and
//! fixtures is synthetic and written by the project.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use fid_io_dmis::conf::{generate, parse_declarations, parse_inventory, Support};
use fid_io_dmis::gdef::{
    descriptors, view, Coord, FeatKind, GdefView, Side, TolFlag, FAMILY, FEATURES, TOLERANCES, TOL_TAIL_MAX,
};
use fid_io_dmis::lex::Token;
use fid_io_dmis::registry::{LabelKind, Registry, Shape, StatementDescriptor, MAX_MATCH_STEPS};
use fid_io_dmis::tree::{parse, parse_in, DmisTree, Node, ParseError, ParseOptions, Statement, STEPS_PER_INPUT_BYTE};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/dmis/parse/gdef")
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

fn spell(t: &Token) -> String {
    esc(t.text.as_bytes())
}

fn joined(tokens: &[&Token]) -> String {
    if tokens.is_empty() {
        "-".to_string()
    } else {
        tokens.iter().map(|t| spell(t)).collect::<Vec<_>>().join(" ")
    }
}

fn opt(t: Option<&Token>) -> String {
    t.map_or("-".to_string(), spell)
}

fn feat_word(kind: FeatKind) -> &'static str {
    FEATURES.iter().find(|f| f.kind == kind).unwrap().word
}

/// The view records of `tree` in the format of make_expected.py.
fn render(tree: &DmisTree) -> String {
    let mut out = String::new();
    for node in &tree.statements {
        let p = node.pos();
        let at = format!("{}:{}", p.line, p.col);
        let row: Vec<String> = match node {
            Node::Comment(_) => vec!["comment".into(), at],
            Node::Unknown(u) => vec!["unknown".into(), at, u.keyword.as_deref().unwrap_or("-").into()],
            Node::Statement(s) => {
                let class = s.class.to_string();
                match view(s).unwrap_or_else(|| panic!("no view for {class} at {at}")) {
                    GdefView::Header(h) => {
                        vec!["header".into(), at, class, opt(h.name), opt(h.version), joined(&h.extra)]
                    }
                    GdefView::Text(t) => vec!["text".into(), at, class, spell(t.channel), spell(t.text)],
                    GdefView::Units(u) => {
                        vec!["units".into(), at, class, spell(u.length), spell(u.angle), opt(u.temp)]
                    }
                    GdefView::Datum(d) => vec!["datum".into(), at, class, spell(d.feature), spell(d.datum)],
                    GdefView::Feat(f) => vec![
                        "feat".into(),
                        at,
                        class,
                        spell(&f.label.token),
                        if f.actual { "actual" } else { "def" }.into(),
                        feat_word(f.kind).into(),
                        match f.coord {
                            Some(Coord::Cart) => "CART",
                            Some(Coord::Pol) => "POL",
                            None => "-",
                        }
                        .into(),
                        match f.side {
                            Some(Side::Inner) => "INNER",
                            Some(Side::Outer) => "OUTER",
                            None => "-",
                        }
                        .into(),
                        joined(&f.minor),
                        joined(&f.values),
                        joined(&f.refs),
                    ],
                    GdefView::Tol(t) => vec![
                        "tol".into(),
                        at,
                        class,
                        spell(&t.label.token),
                        if t.actual { "actual" } else { "def" }.into(),
                        t.kind.word.into(),
                        t.kind.category.as_str().into(),
                        joined(&t.values),
                        joined(&t.words),
                        joined(&t.refs),
                        match t.flag {
                            Some(TolFlag::InTol) => "INTOL",
                            Some(TolFlag::OutTol) => "OUTOL",
                            None => "-",
                        }
                        .into(),
                    ],
                }
            }
        };
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    out
}

fn parse_fixture(name: &str) -> DmisTree {
    parse(&std::fs::read(fixtures().join(name)).unwrap(), None).unwrap()
}

fn check_fixture(name: &str) -> DmisTree {
    let tree = parse_fixture(name);
    let stem = Path::new(name).file_stem().unwrap().to_str().unwrap();
    let want = std::fs::read_to_string(fixtures().join(format!("{stem}.views"))).unwrap();
    assert_eq!(render(&tree), want, "{name}");
    tree
}

fn statements(tree: &DmisTree) -> impl Iterator<Item = &Statement> {
    tree.statements.iter().filter_map(|n| match n {
        Node::Statement(s) => Some(&**s),
        _ => None,
    })
}

#[test]
fn ac1_every_feature_type_parses_into_its_view() {
    let tree = check_fixture("features.dmi");
    assert!(!tree.statements.iter().any(|n| matches!(n, Node::Unknown(_))));
    let mut kinds = BTreeSet::new();
    for s in statements(&tree) {
        if let Some(GdefView::Feat(f)) = view(s) {
            assert!(!f.actual);
            assert!(kinds.insert(f.kind), "{:?} defined twice", f.kind);
        }
    }
    let all: BTreeSet<FeatKind> = FEATURES.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, all);
    // Values stay authored lexemes: `10.0` is not reformatted.
    let point = statements(&tree).find(|s| &*s.class == "feat_point").unwrap();
    let Some(GdefView::Feat(f)) = view(point) else { panic!("expected a feature view") };
    assert_eq!(f.values[0].text.as_bytes(), b"10.0");
    assert_eq!(f.label.name(), "PT1");
}

#[test]
fn ac2_results_expose_values_and_flags() {
    let tree = check_fixture("results.dmo");
    assert!(!tree.statements.iter().any(|n| matches!(n, Node::Unknown(_))));
    let mut flags = Vec::new();
    for s in statements(&tree) {
        match view(s) {
            Some(GdefView::Tol(t)) => {
                assert!(t.actual);
                assert!(!t.values.is_empty());
                flags.push(t.flag.unwrap());
            }
            Some(GdefView::Feat(f)) => assert!(f.actual),
            _ => {}
        }
    }
    assert!(flags.contains(&TolFlag::InTol) && flags.contains(&TolFlag::OutTol));
    // The lower-case result keeps its authored spelling.
    let rad = statements(&tree).find(|s| &*s.class == "tol_rad_actual").unwrap();
    let Some(GdefView::Tol(t)) = view(rad) else { panic!("expected a tolerance view") };
    assert_eq!((t.kind_word.text.as_bytes(), t.values[0].text.as_bytes()), (&b"rad"[..], &b"-0.15"[..]));
    assert_eq!(t.flag, Some(TolFlag::OutTol));
}

#[test]
fn definitions_and_header_statements_parse_into_views() {
    let tree = check_fixture("defs.dmi");
    assert!(!tree.statements.iter().any(|n| matches!(n, Node::Unknown(_))));
    let tols: BTreeSet<&str> = statements(&tree)
        .filter_map(|s| match view(s) {
            Some(GdefView::Tol(t)) => Some(t.kind.word),
            _ => None,
        })
        .collect();
    let all: BTreeSet<&str> = TOLERANCES.iter().map(|t| t.word).collect();
    assert_eq!(tols, all, "defs.dmi defines every tolerance kind once");
}

/// Hand-labelled: a module header, and statements of other families staying `Unknown`.
#[test]
fn module_header_parses_and_other_statements_stay_unknown() {
    let tree = parse(b"DMISMD/'synthetic module',05.3\nSNSLCT/S(PRB1)\nENDFIL\n", None).unwrap();
    let Node::Statement(s) = &tree.statements[0] else { panic!("expected a statement") };
    assert_eq!(&*s.class, "dmismd");
    let Some(GdefView::Header(h)) = view(s) else { panic!("expected a header view") };
    assert_eq!(h.version.unwrap().text.as_bytes(), b"05.3");
    let Node::Unknown(u) = &tree.statements[1] else { panic!("expected an unknown node") };
    assert_eq!(u.keyword.as_deref(), Some("SNSLCT"));
    let Node::Statement(s) = &tree.statements[2] else { panic!("expected a statement") };
    assert_eq!(&*s.class, "endfil");
}

fn strict_error(data: &[u8]) -> ParseError {
    let err = parse(data, None).unwrap_err();
    let lenient = parse_in(data, &Registry::builtin().unwrap(), None, ParseOptions { lenient: true }).unwrap();
    let Node::Unknown(u) = &lenient.statements[0] else { panic!("expected an unknown node") };
    assert_eq!(u.reason.map(|r| r.as_str()), Some("shape_mismatch"));
    err
}

fn malformed(line: u32, col: u32, class: &str) -> ParseError {
    ParseError::Malformed { line, col, class: class.into(), statement: fid_io_dmis::lex::Pos { line: 1, col: 1 } }
}

/// Hand-labelled positions (1-based byte columns) of shape mismatches.
#[test]
fn malformed_statements_report_the_closest_class() {
    // The side word is missing: feat_circle gets farthest, stopping at `CART` (column 19).
    assert_eq!(strict_error(b"F(C1)=FEAT/CIRCLE,CART,1,2,3,0,0,1,10\n"), malformed(1, 19, "feat_circle"));
    // One number too many: stops at the seventh number (column 35).
    assert_eq!(strict_error(b"F(P1)=FEAT/POINT,CART,1,2,3,4,5,6,7\n"), malformed(1, 35, "feat_point"));
    // A result without its flag: runs out after `0.012` (column 17).
    assert_eq!(strict_error(b"TA(D1)=TOL/DIAM,0.012\n"), malformed(1, 17, "tol_diam_actual"));
    // The angle unit is missing: runs out after `MM` (column 7).
    assert_eq!(strict_error(b"UNITS/MM\n"), malformed(1, 7, "units"));
    // No FEAT descriptor takes a `T` label: the first one is reported at the statement start.
    assert_eq!(strict_error(b"T(X)=FEAT/POINT,CART,1,2,3\n"), malformed(1, 1, "feat_point"));
    // A pattern needs at least one feature label.
    assert_eq!(strict_error(b"F(PA)=FEAT/PATERN,DAT(A)\n"), malformed(1, 19, "feat_patern"));
    // A flag word is not a definition modifier: stops at `INTOL` (column 19).
    assert_eq!(strict_error(b"T(X)=TOL/DIAM,0.1,INTOL\n"), malformed(1, 19, "tol_diam"));
    // A result without any value: stops at `OUTOL` where the number was due (column 20).
    assert_eq!(strict_error(b"TA(X)=TOL/FLAT,MMC,OUTOL\n"), malformed(1, 20, "tol_flat_actual"));
}

/// Hand-labelled: the labelled datum form `DA(..)=DATDEF` parses; `D(..)=DATDEF` is not part
/// of this family and is rejected (reported against the first DATDEF class, `datdef`).
#[test]
fn datum_actual_form_parses_and_d_label_is_rejected() {
    let tree = parse(b"DA(A)=DATDEF/FA(P1)\n", None).unwrap();
    let Node::Statement(s) = &tree.statements[0] else { panic!("expected a statement") };
    assert_eq!(&*s.class, "datdef_actual");
    let Some(GdefView::Datum(d)) = view(s) else { panic!("expected a datum view") };
    assert!(d.actual);
    assert_eq!((d.feature.text.as_bytes(), d.datum.text.as_bytes()), (&b"FA(P1)"[..], &b"DA(A)"[..]));
    assert_eq!(strict_error(b"D(A)=DATDEF/FA(P1)\n"), malformed(1, 1, "datdef"));
}

#[test]
fn every_descriptor_belongs_to_the_family_and_has_a_view() {
    let registry = Registry::builtin().unwrap();
    assert_eq!(registry.len(), descriptors().len());
    for d in registry.descriptors() {
        assert_eq!(&*d.family, FAMILY);
        assert_eq!(&*d.phase, "P1a");
    }
    // Every class except `dmismd` (hand-labelled in its own test) is reached by a fixture.
    let mut seen = BTreeSet::new();
    for name in ["features.dmi", "defs.dmi", "results.dmo"] {
        for s in statements(&parse_fixture(name)) {
            assert!(view(s).is_some());
            seen.insert(s.class.to_string());
        }
    }
    for d in registry.descriptors() {
        if !seen.contains(&*d.class) {
            assert_eq!(&*d.class, "dmismd", "{} is not exercised", d.class);
        }
    }
}

/// AC-4: every descriptor's keyword has a `partial` or `full` import declaration whose tests
/// exist (and are not ignored) in this crate, and the generator accepts the declarations
/// against that inventory. CI repeats the check with the workspace's real `--list` inventory.
#[test]
fn ac4_every_descriptor_has_a_proven_support_declaration() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance/declarations.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let decls = parse_declarations(&text).unwrap();
    let inventory_text = support::crate_test_inventory();
    let inventory = parse_inventory(&inventory_text);
    assert!(inventory.contains("fid-io-dmis::gdef::ac4_every_descriptor_has_a_proven_support_declaration"));
    for d in descriptors() {
        let decl = decls.iter().find(|s| s.keyword == *d.major).unwrap_or_else(|| panic!("{} undeclared", d.major));
        assert!(decl.import >= Support::Partial, "{} declares no import support", d.major);
        assert!(!decl.tests.is_empty());
        for t in &decl.tests {
            assert!(inventory.contains(t), "{t} is not a test of this crate");
        }
    }
    generate(&text, &inventory_text).unwrap();
}

fn token_bytes(s: &Statement) -> u64 {
    s.tokens().iter().map(|t| t.text.len() as u64).sum()
}

/// The descriptor shapes stay far below the per-statement cap of 10^4 steps on realistic
/// statements, and below the file-level budget of STEPS_PER_INPUT_BYTE per byte, so neither
/// bound ever trips on well-formed input of this family.
#[test]
fn realistic_statements_stay_well_under_the_step_cap() {
    let registry = Registry::builtin().unwrap();
    let well_under = MAX_MATCH_STEPS / 20;
    let mut worst = 0;
    for name in ["features.dmi", "defs.dmi", "results.dmo"] {
        for s in statements(&parse_fixture(name)) {
            let steps = registry.match_steps(s).unwrap();
            assert!(steps <= well_under, "{} uses {steps} steps", s.class);
            assert!(u64::from(steps) <= STEPS_PER_INPUT_BYTE * token_bytes(s), "{}", s.class);
            worst = worst.max(steps);
        }
    }
    assert!(worst > 0);

    // Larger but realistic statements: a 64-member pattern and a full tolerance body.
    let members: Vec<String> = (0..64).map(|i| format!("F(H{i})")).collect();
    let pattern = format!("F(PAT)=FEAT/PATERN,{}\n", members.join(","));
    let tail: Vec<&str> = ["0.1", "MMC", "DAT(A)", "MMC", "DAT(B)", "LMC", "DAT(C)", "RFS", "0.2", "0.3", "AVG"]
        .into_iter()
        .take(usize::try_from(TOL_TAIL_MAX - 1).unwrap())
        .collect();
    let result = format!("TA(P9)=TOL/POS,{},OUTOL\n", tail.join(","));
    for data in [pattern, result] {
        let tree = parse(data.as_bytes(), None).unwrap();
        let s = statements(&tree).next().unwrap();
        let steps = registry.match_steps(s).unwrap();
        assert!(steps <= MAX_MATCH_STEPS / 10, "{} uses {steps} steps", s.class);
        assert!(u64::from(steps) <= STEPS_PER_INPUT_BYTE * token_bytes(s));
    }
}

/// Near misses cost about as much as matches: failing paths stop at the first field that
/// cannot match. The statements are read with a catch-all registry so they keep their fields,
/// then measured against the built-in descriptors.
#[test]
fn near_misses_stay_well_under_the_step_cap() {
    let any = || Shape::repeat(Shape::AnyField, 0, None);
    let catch_all = Registry::from_descriptors([
        StatementDescriptor::new("any_feat", "FEAT", LabelKind::Required("F".into()), any(), "TEST", "P1a"),
        StatementDescriptor::new("any_tol", "TOL", LabelKind::Required("TA".into()), any(), "TEST", "P1a"),
    ])
    .unwrap();
    let builtin = Registry::builtin().unwrap();
    let numbers = vec!["1"; 40].join(",");
    let words = vec!["MMC"; 40].join(",");
    let cases = [
        format!("F(X)=FEAT/POINT,CART,{numbers}\n"),
        format!("F(X)=FEAT/CPARLN,INNER,ROUND,CART,{numbers}\n"),
        format!("F(X)=FEAT/COMPOUND,AXIAL,{words}\n"),
        format!("TA(X)=TOL/PROFS,{words}\n"),
        format!("TA(X)=TOL/PROFS,{words},INTOL\n"),
    ];
    for data in cases {
        let tree = parse_in(data.as_bytes(), &catch_all, None, ParseOptions::default()).unwrap();
        let s = statements(&tree).next().unwrap();
        let steps = builtin.match_steps(s).unwrap();
        assert!(steps <= MAX_MATCH_STEPS / 10, "{data}: {steps} steps");
        // And the real parse rejects it as malformed rather than running out of steps.
        assert_eq!(parse(data.as_bytes(), None).unwrap_err().code(), "malformed", "{data}");
    }
}

/// The per-statement cap leaves room for patterns far larger than real programs use.
#[test]
fn a_thousand_member_pattern_parses() {
    let members: Vec<String> = (0..1000).map(|i| format!("FA(H{i})")).collect();
    let data = format!("FA(PAT)=FEAT/PATERN,{}\n", members.join(","));
    let tree = parse(data.as_bytes(), None).unwrap();
    let s = statements(&tree).next().unwrap();
    assert_eq!(&*s.class, "feat_patern_actual");
    let Some(GdefView::Feat(f)) = view(s) else { panic!("expected a feature view") };
    assert_eq!(f.refs.len(), 1000);
    assert!(Registry::builtin().unwrap().match_steps(s).unwrap() < MAX_MATCH_STEPS);
}
