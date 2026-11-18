//! Acceptance criteria of spec leaf IO.DMIS.PARSE.LEX (AC-1 .. AC-5) and the error paths.
//! Expected token streams are hand-labelled here or come from
//! `fixtures/io/dmis/parse/lex/make_expected.py`, an independent Python lexer; none is
//! produced by this crate. Every DMIS snippet is written by the project.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_dmis::lex::{
    fuzz_budget, lex_bytes, lex_slice, Bound, LexError, LexOptions, LogicalStatement, Pos, Reason, Relaxation,
    TokenKind, MAX_CHAIN_LINES, MAX_LINE_BYTES, MAX_STATEMENT_TOKENS,
};
use fid_io_reg::{Budget, LimitKind, LimitRequest, Principal};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/dmis/parse/lex")
}

fn strict(data: &[u8]) -> Result<Vec<LogicalStatement>, LexError> {
    lex_bytes(data, &LexOptions::strict())
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

/// The same text format as `make_expected.py`.
fn render(result: &Result<Vec<LogicalStatement>, LexError>) -> String {
    let mut out = String::new();
    match result {
        Err(LexError::Malformed { line, col, reason }) => {
            out.push_str(&format!("error\tmalformed\t{}\t{line}\t{col}\n", reason.as_str()));
        }
        Err(LexError::LimitExceeded { bound, line }) => {
            out.push_str(&format!("error\tlimit_exceeded\t{}\t{line}\n", bound.as_str()));
        }
        Err(e) => out.push_str(&format!("error\t{e}\n")),
        Ok(stmts) => {
            for st in stmts {
                out.push_str(&format!("statement\t{}\t{}\n", st.first_line, st.last_line));
                for t in &st.tokens {
                    let mut row = vec![format!("{}:{}", t.pos.line, t.pos.col), t.kind.name().to_string()];
                    row.push(esc(t.text.as_bytes()));
                    match &t.kind {
                        TokenKind::Word { upper } => row.push(esc(upper.as_bytes())),
                        TokenKind::Label { prefix, name } => {
                            row.push(esc(prefix.as_bytes()));
                            row.push(esc(name.as_bytes()));
                        }
                        TokenKind::Str { value } => row.push(esc(value)),
                        TokenKind::Comment => row.push(esc(&t.text.as_bytes()[2..])),
                        _ => {}
                    }
                    out.push_str(&row.join("\t"));
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

#[test]
fn every_fixture_matches_the_python_reference() {
    let mut seen = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("dmi") {
            continue;
        }
        let want = std::fs::read_to_string(path.with_extension("tokens")).unwrap();
        let got = render(&strict(&std::fs::read(&path).unwrap()));
        assert_eq!(got, want, "{}", path.display());
        seen += 1;
    }
    assert!(seen >= 5);
}

#[test]
fn ac1_continued_statement_comment_and_doubled_quote() {
    let stmts = strict(&fixture("continued.dmi")).unwrap();
    assert_eq!(stmts.len(), 1);
    let st = &stmts[0];
    assert_eq!((st.first_line, st.last_line), (1, 4));
    // Hand-labelled from the fixture text.
    let kinds: Vec<&str> = st.tokens.iter().map(|t| t.kind.name()).collect();
    assert_eq!(kinds, ["word", "slash", "string", "comma", "comment", "number"]);
    let pos: Vec<Pos> = st.tokens.iter().map(|t| t.pos).collect();
    let want = [(1, 1), (1, 7), (2, 3), (2, 24), (3, 1), (4, 3)].map(|(line, col)| Pos { line, col });
    assert_eq!(pos, want);
    match &st.tokens[2].kind {
        TokenKind::Str { value } => assert_eq!(&**value, b"Bracket 'B' check"),
        k => panic!("{k:?}"),
    }
    assert_eq!(st.tokens[2].text.as_bytes(), b"'Bracket ''B'' check'");
    assert_eq!(
        format!("{}", st.tokens[2].untrusted_text().unwrap().as_display()),
        "Bracket 'B' check"
    );
    assert_eq!(
        format!("{}", st.tokens[4].untrusted_text().unwrap().as_display()),
        " the version number is on the next line"
    );
    assert_eq!(st.tokens[5].ascii(), Some("05.3"));
}

#[test]
fn ac2_numeric_lexemes_are_kept_exactly() {
    let stmts = strict(b"F(P1)=FEAT/POINT,CART,1.50,2,3.000,0,0,1").unwrap();
    let nums: Vec<&str> =
        stmts[0].tokens.iter().filter(|t| t.kind == TokenKind::Number).map(|t| t.ascii().unwrap()).collect();
    assert_eq!(nums, ["1.50", "2", "3.000", "0", "0", "1"]);
    match &stmts[0].tokens[0].kind {
        TokenKind::Label { prefix, name } => assert_eq!((&**prefix, &**name), ("F", "P1")),
        k => panic!("{k:?}"),
    }
}

#[test]
fn ac3_overlong_line_is_limit_exceeded_on_line_1() {
    let mut line = b"TEXT/OPER,".to_vec();
    line.resize(65 * 1024, b'x');
    line.push(b'\n');
    assert_eq!(strict(&line), Err(LexError::LimitExceeded { bound: Bound::LineLength, line: 1 }));
}

#[test]
fn line_length_bound_is_exact_and_ignores_the_terminator() {
    let mut ok = vec![b'A'; MAX_LINE_BYTES];
    ok.extend_from_slice(b"\r\n");
    assert_eq!(strict(&ok).unwrap().len(), 1);
    let mut bad = b"A\n".to_vec();
    bad.extend(std::iter::repeat_n(b'B', MAX_LINE_BYTES + 1));
    assert_eq!(strict(&bad), Err(LexError::LimitExceeded { bound: Bound::LineLength, line: 2 }));
}

#[test]
fn ac4_open_string_is_malformed_with_position() {
    assert_eq!(
        strict(&fixture("open_string.dmi")),
        Err(LexError::Malformed { line: 3, col: 11, reason: Reason::UnterminatedString })
    );
}

#[test]
fn continuation_chain_bound_names_the_first_line() {
    let mut ok = b"$$ head\n".to_vec();
    for _ in 1..MAX_CHAIN_LINES {
        ok.extend_from_slice(b"1,$\n");
    }
    ok.extend_from_slice(b"1\n");
    let stmts = strict(&ok).unwrap();
    assert_eq!(stmts[1].first_line, 2);
    assert_eq!(stmts[1].last_line, MAX_CHAIN_LINES + 1);

    let mut bad = b"$$ head\n".to_vec();
    for _ in 0..MAX_CHAIN_LINES {
        bad.extend_from_slice(b"1,$\n");
    }
    bad.extend_from_slice(b"1\n");
    assert_eq!(strict(&bad), Err(LexError::LimitExceeded { bound: Bound::ContinuationLines, line: 2 }));
}

#[test]
fn statement_token_bound() {
    // 3 lines of 40,000 commas: 120,000 tokens in one statement.
    let mut data = b"\n".to_vec();
    for _ in 0..3 {
        data.extend(std::iter::repeat_n(b',', 40_000));
        data.extend_from_slice(b"$\n");
    }
    data.extend_from_slice(b"X\n");
    assert!(MAX_STATEMENT_TOKENS < 120_000);
    assert_eq!(strict(&data), Err(LexError::LimitExceeded { bound: Bound::StatementTokens, line: 2 }));
}

#[test]
fn hand_labelled_error_paths() {
    let m = |line, col, reason| Err(LexError::Malformed { line, col, reason });
    assert_eq!(strict(b"A/B,$"), m(1, 5, Reason::DanglingContinuation));
    assert_eq!(strict(b"A/B,$\n\n$$ c\n"), m(1, 5, Reason::DanglingContinuation));
    assert_eq!(strict(b"A/B $ C"), m(1, 5, Reason::UnexpectedByte));
    assert_eq!(strict(b"A/B,$$ note"), m(1, 5, Reason::UnexpectedByte));
    assert_eq!(strict(b"A/1.5MM"), m(1, 3, Reason::BadNumber));
    assert_eq!(strict(b"A/1.2.3"), m(1, 3, Reason::BadNumber));
    assert_eq!(strict(b"A/1E"), m(1, 3, Reason::BadNumber));
    assert_eq!(strict(b"A/-"), m(1, 3, Reason::BadNumber));
    assert_eq!(strict(b"A/."), m(1, 3, Reason::BadNumber));
    assert_eq!(strict(b"OK\nA/\xc3\xbc"), m(2, 3, Reason::NonAscii));
    assert_eq!(strict(b"A/\"x\""), m(1, 3, Reason::UnexpectedByte));
    assert_eq!(strict(b"A/B\x00"), m(1, 4, Reason::UnexpectedByte));
    assert_eq!(strict(b"A\rB\n"), m(1, 2, Reason::UnexpectedByte));
    // A `$` inside a string is content, so the line is not continued.
    assert_eq!(strict(b"A/'x,$\nB'"), m(1, 3, Reason::UnterminatedString));
    // Codes.
    assert_eq!(strict(b"'").unwrap_err().code(), "malformed");
    let long = vec![b'A'; MAX_LINE_BYTES + 1];
    assert_eq!(strict(&long).unwrap_err().code(), "limit_exceeded");
}

#[test]
fn edge_inputs() {
    assert_eq!(strict(b"").unwrap(), vec![]);
    assert_eq!(strict(b"\n \t\r\n\n").unwrap(), vec![]);
    let r = strict(b"  a1_b(  )  ").unwrap();
    let st = &r[0];
    let kinds: Vec<&str> = st.tokens.iter().map(|t| t.kind.name()).collect();
    // `a1_b` is not an all-letter prefix and the parentheses hold no name: no label.
    assert_eq!(kinds, ["word", "lparen", "rparen"]);
    assert_eq!(st.tokens[0].pos, Pos { line: 1, col: 3 });
    assert_eq!(st.tokens[0].text.as_bytes(), b"a1_b");
    match &st.tokens[0].kind {
        TokenKind::Word { upper } => assert_eq!(&**upper, "A1_B"),
        k => panic!("{k:?}"),
    }
    // Function-call shape with a non-name argument stays word + punctuation.
    let st = strict(b"SIN(1.5)").unwrap();
    let kinds: Vec<&str> = st[0].tokens.iter().map(|t| t.kind.name()).collect();
    assert_eq!(kinds, ["word", "lparen", "number", "rparen"]);
    // An empty string and a string of one quote.
    let st = strict(b"X/'',''''").unwrap();
    match (&st[0].tokens[2].kind, &st[0].tokens[4].kind) {
        (TokenKind::Str { value: a }, TokenKind::Str { value: b }) => {
            assert_eq!((&**a, &**b), (&b""[..], &b"'"[..]));
        }
        k => panic!("{k:?}"),
    }
    // TEXT with nothing after the comma, and a continued TEXT before the comma.
    let st = strict(b"TEXT/OPER,\nTEXT/$\nOPER,  go now  ").unwrap();
    assert_eq!(st.len(), 2);
    assert_eq!(st[0].tokens.len(), 4);
    let raw = st[1].tokens.last().unwrap();
    assert_eq!(raw.kind, TokenKind::RawText);
    assert_eq!(raw.pos, Pos { line: 3, col: 8 });
    assert_eq!(raw.text.as_bytes(), b"go now  ");
    assert_eq!(raw.ascii(), None);
}

#[test]
fn relaxations_apply_only_when_enabled() {
    let inline = b"A/B,C $$ trailing note\nD";
    assert_eq!(strict(inline), Err(LexError::Malformed { line: 1, col: 7, reason: Reason::UnexpectedByte }));
    let opts = LexOptions::strict().with(Relaxation::InlineComment);
    let st = lex_bytes(inline, &opts).unwrap();
    assert_eq!(st.len(), 2);
    let c = st[0].tokens.last().unwrap();
    assert_eq!((c.kind.name(), c.pos), ("comment", Pos { line: 1, col: 7 }));
    assert_eq!(format!("{}", c.untrusted_text().unwrap().as_display()), " trailing note");
    // InlineComment does not make a bare CR a line end.
    assert_eq!(
        lex_bytes(b"A\rB", &opts),
        Err(LexError::Malformed { line: 1, col: 2, reason: Reason::UnexpectedByte })
    );

    let cr = LexOptions::strict().with(Relaxation::BareCrLineEnd);
    let st = lex_bytes(b"A\rB\r\nC", &cr).unwrap();
    let lines: Vec<(u32, u32)> = st.iter().map(|s| (s.first_line, s.tokens[0].pos.col)).collect();
    assert_eq!(lines, [(1, 1), (2, 1), (3, 1)]);
    // BareCrLineEnd does not enable inline comments.
    assert!(lex_bytes(inline, &cr).is_err());
    assert!(opts.allows(Relaxation::InlineComment) && !opts.allows(Relaxation::BareCrLineEnd));
}

#[test]
fn reader_budget_bounds_total_size_and_tokens() {
    let small = Budget::new(Principal::Script, &LimitRequest::default().bytes(8)).unwrap();
    match lex_slice(b"UNITS/MM,ANGDEC", small, &LexOptions::strict()) {
        Err(LexError::Io(e)) => assert_eq!(e.code(), "limit_exceeded"),
        r => panic!("{r:?}"),
    }
    let few = Budget::new(Principal::Script, &LimitRequest::default().elements(3)).unwrap();
    match lex_slice(b"A/B,C", few, &LexOptions::strict()) {
        Err(LexError::Io(fid_io_reg::IoError::LimitExceeded { limit: LimitKind::Elements, at, .. })) => {
            assert_eq!((at.line, at.col), (Some(1), Some(4)));
        }
        r => panic!("{r:?}"),
    }
}

/// xorshift64*: deterministic pseudo-random bytes for the fuzz smoke test.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// AC-5 on stable: mutated fixtures and random snippets never panic, and every token of
/// an accepted input lies inside its statement's lines with a column of at least 1. The
/// cargo-fuzz target `crates/fid-io-dmis/fuzz` runs the same property on nightly.
#[test]
fn ac5_fuzz_smoke_never_panics() {
    const ALPHABET: &[u8] = b"$$$''',,//==()AZaz09.+-eE_ \t\n\n\r\xc3\xbc\xff\x00";
    let seeds: Vec<Vec<u8>> =
        ["continued.dmi", "open_string.dmi", "numbers.dmi", "mixed.dmi", "bytes.dmi"].into_iter().map(fixture).collect();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let options = [
        LexOptions::strict(),
        LexOptions::strict().with(Relaxation::InlineComment),
        LexOptions::strict().with(Relaxation::BareCrLineEnd).with(Relaxation::InlineComment),
    ];
    for round in 0..20_000 {
        let mut data = if round % 4 == 0 { Vec::new() } else { seeds[rng.below(seeds.len())].clone() };
        for _ in 0..1 + rng.below(8) {
            let b = ALPHABET[rng.below(ALPHABET.len())];
            match rng.below(3) {
                0 if !data.is_empty() => {
                    let i = rng.below(data.len());
                    data[i] = b;
                }
                1 if !data.is_empty() => {
                    data.remove(rng.below(data.len()));
                }
                _ => {
                    let i = rng.below(data.len() + 1);
                    data.insert(i, b);
                }
            }
        }
        let budget = fuzz_budget().unwrap();
        if let Ok(stmts) = lex_slice(&data, budget, &options[round % options.len()]) {
            for st in &stmts {
                assert!(st.first_line <= st.last_line);
                for t in &st.tokens {
                    assert!(t.pos.line >= st.first_line && t.pos.line <= st.last_line && t.pos.col >= 1);
                    assert!(!t.text.is_empty());
                }
            }
        }
    }
}
