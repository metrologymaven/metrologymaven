//! Bounded, non-recursive Rust lexer for the syntax-level scan (spec node `KRN.NUM.LINT.SYN`).
//!
//! It turns a source file into identifier, lifetime, literal and punctuation tokens with
//! 1-based line and column (columns count characters), skipping whitespace and comments
//! (nested block comments and doc comments included). It checks that every delimiter is
//! balanced. Anything it cannot classify is a [`LexError`], which the scan reports as a
//! `KNP1-PARSE` finding:
//!
//! - an unterminated string, raw string, character literal or block comment;
//! - an unbalanced or mismatched delimiter, or nesting deeper than [`MAX_DEPTH`];
//! - a non-ASCII identifier (the scan compares ASCII names only, so it cannot classify one);
//! - a character that is not Rust punctuation (a backtick, a stray backslash, a control or
//!   other non-ASCII character outside a literal or comment);
//! - more than [`MAX_TOKENS`] tokens.
//!
//! The lexer never recurses and never indexes out of bounds, so hostile input cannot panic it
//! or overflow the stack.

/// Most tokens accepted in one file.
pub const MAX_TOKENS: usize = 2_000_000;
/// Deepest delimiter nesting accepted.
pub const MAX_DEPTH: usize = 512;
/// Longest character or byte literal body accepted (`\u{10FFFF}` is 10 characters).
const MAX_CHAR_BODY: usize = 12;
/// Most `#` around a raw string (Rust allows 255).
const MAX_RAW_HASHES: usize = 255;

/// A delimiter pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delim {
    Paren,
    Bracket,
    Brace,
}

/// Literal kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LitKind {
    Str,
    RawStr,
    ByteStr,
    RawByteStr,
    CStr,
    RawCStr,
    Char,
    Byte,
    Int,
    Float,
}

/// Token kinds. Punctuation the scan does not need to tell apart is [`Kind::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An identifier or keyword; `raw` for `r#name` (the text is `name`).
    Ident {
        raw: bool,
    },
    /// A lifetime or loop label (`'a`).
    Lifetime,
    /// A literal; `suffix` when an identifier suffix follows it (`1f64`, `"x"s`).
    Lit {
        kind: LitKind,
        suffix: bool,
    },
    /// `::`
    PathSep,
    /// `.` (not part of `..`)
    Dot,
    /// `..`, `...` or `..=`
    DotDot,
    /// `!` (not part of `!=`)
    Bang,
    /// `#`
    Pound,
    /// `$`
    Dollar,
    /// `<`
    Lt,
    /// `>` (not part of `->` or `=>`)
    Gt,
    /// `=` (not part of `=>`)
    Eq,
    /// `,`
    Comma,
    /// `;`
    Semi,
    Open(Delim),
    Close(Delim),
    /// Any other punctuation; the text says which (`->`, `=>`, `!=`, `*`, `+`, ...).
    Other,
}

/// One token with its 1-based position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    /// Identifier name (without `r#`), literal body as written (without quotes or prefix),
    /// or the punctuation text.
    pub text: String,
    pub line: u32,
    pub col: u32,
}

/// Why the lexer stopped, with the position of the offending text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

struct Cur<'a> {
    c: &'a [char],
    i: usize,
    line: u32,
    col: u32,
}

impl Cur<'_> {
    fn peek(&self, k: usize) -> Option<char> {
        self.c.get(self.i.saturating_add(k)).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek(0)?;
        self.i += 1;
        if ch == '\n' {
            self.line = self.line.saturating_add(1);
            self.col = 1;
        } else {
            self.col = self.col.saturating_add(1);
        }
        Some(ch)
    }

    fn eat_while(&mut self, out: &mut String, f: impl Fn(char) -> bool) {
        while let Some(ch) = self.peek(0) {
            if !f(ch) {
                break;
            }
            out.push(ch);
            self.bump();
        }
    }
}

/// Rust's `Pattern_White_Space`.
fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{85}' | '\u{200E}' | '\u{200F}' | '\u{2028}' | '\u{2029}'
    )
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn err(line: u32, col: u32, message: impl Into<String>) -> LexError {
    LexError { line, col, message: message.into() }
}

/// Lexes `src` (a UTF-8 source file; a leading BOM must already be removed).
pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = src.chars().collect();
    let mut cur = Cur { c: &chars, i: 0, line: 1, col: 1 };
    // Shebang line (rustc's rule): `#!` is a shebang unless the first thing after it, skipping
    // whitespace and non-doc comments, is `[` (then it starts an inner attribute).
    if cur.peek(0) == Some('#') && cur.peek(1) == Some('!') && !attribute_follows(&chars) {
        while cur.peek(0).is_some_and(|c| c != '\n') {
            cur.bump();
        }
    }
    let mut toks: Vec<Token> = Vec::new();
    let mut stack: Vec<(Delim, u32, u32)> = Vec::new();
    while let Some(ch) = cur.peek(0) {
        if toks.len() >= MAX_TOKENS {
            return Err(err(cur.line, cur.col, format!("more than {MAX_TOKENS} tokens")));
        }
        let (line, col) = (cur.line, cur.col);
        if is_ws(ch) {
            cur.bump();
            continue;
        }
        let next = cur.peek(1);
        if ch == '/' && next == Some('/') {
            while cur.peek(0).is_some_and(|c| c != '\n') {
                cur.bump();
            }
            continue;
        }
        if ch == '/' && next == Some('*') {
            block_comment(&mut cur, line, col)?;
            continue;
        }
        let tok = |kind: Kind, text: String| Token { kind, text, line, col };
        // Literals and identifiers with a prefix letter.
        if ch == '\'' {
            toks.push(quote(&mut cur, line, col)?);
            continue;
        }
        if ch == '"' {
            cur.bump();
            let body = string_body(&mut cur, line, col)?;
            let suffix = suffix(&mut cur);
            toks.push(tok(Kind::Lit { kind: LitKind::Str, suffix }, body));
            continue;
        }
        if let Some(t) = prefixed_literal(&mut cur, line, col)? {
            toks.push(t);
            continue;
        }
        if ch.is_ascii_digit() {
            toks.push(number(&mut cur, line, col));
            continue;
        }
        if is_ident_start(ch) {
            let mut name = String::new();
            cur.eat_while(&mut name, is_ident_continue);
            if cur.peek(0).is_some_and(|c| !c.is_ascii() && c.is_alphanumeric()) {
                return Err(err(line, col, "non-ASCII identifier: the scan classifies ASCII identifiers only"));
            }
            toks.push(tok(Kind::Ident { raw: false }, name));
            continue;
        }
        if !ch.is_ascii() {
            let what = if ch.is_alphanumeric() {
                "non-ASCII identifier: the scan classifies ASCII identifiers only".to_string()
            } else {
                format!("unexpected character U+{:04X}", u32::from(ch))
            };
            return Err(err(line, col, what));
        }
        // Punctuation.
        let (kind, len) = match (ch, next) {
            (':', Some(':')) => (Kind::PathSep, 2),
            ('.', Some('.')) => {
                let third = cur.peek(2);
                (Kind::DotDot, if matches!(third, Some('.' | '=')) { 3 } else { 2 })
            }
            ('.', _) => (Kind::Dot, 1),
            ('-', Some('>')) | ('=', Some('>')) | ('!', Some('=')) => (Kind::Other, 2),
            ('!', _) => (Kind::Bang, 1),
            ('#', _) => (Kind::Pound, 1),
            ('$', _) => (Kind::Dollar, 1),
            ('<', _) => (Kind::Lt, 1),
            ('>', _) => (Kind::Gt, 1),
            ('=', _) => (Kind::Eq, 1),
            (',', _) => (Kind::Comma, 1),
            (';', _) => (Kind::Semi, 1),
            ('(', _) => (Kind::Open(Delim::Paren), 1),
            ('[', _) => (Kind::Open(Delim::Bracket), 1),
            ('{', _) => (Kind::Open(Delim::Brace), 1),
            (')', _) => (Kind::Close(Delim::Paren), 1),
            (']', _) => (Kind::Close(Delim::Bracket), 1),
            ('}', _) => (Kind::Close(Delim::Brace), 1),
            (':' | '+' | '-' | '*' | '/' | '%' | '^' | '&' | '|' | '@' | '?' | '~', _) => (Kind::Other, 1),
            _ => return Err(err(line, col, format!("unexpected character U+{:04X}", u32::from(ch)))),
        };
        let mut text = String::new();
        for _ in 0..len {
            if let Some(c) = cur.bump() {
                text.push(c);
            }
        }
        match kind {
            Kind::Open(d) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(err(line, col, format!("delimiters nested deeper than {MAX_DEPTH}")));
                }
                stack.push((d, line, col));
            }
            Kind::Close(d) => match stack.pop() {
                Some((o, _, _)) if o == d => {}
                Some((_, ol, oc)) => {
                    return Err(err(line, col, format!("mismatched closing delimiter `{text}` (opened at {ol}:{oc})")))
                }
                None => return Err(err(line, col, format!("unmatched closing delimiter `{text}`"))),
            },
            _ => {}
        }
        toks.push(tok(kind, text));
    }
    if let Some((_, l, c)) = stack.last() {
        return Err(err(*l, *c, "unclosed delimiter"));
    }
    Ok(toks)
}

/// After a leading `#!`: true when, skipping whitespace and non-doc comments, `[` follows.
/// An unterminated comment counts as an attribute (fail closed: the text is lexed, not dropped).
fn attribute_follows(c: &[char]) -> bool {
    let at = |k: usize| c.get(k).copied();
    let mut k = 2;
    loop {
        match (at(k), at(k + 1), at(k + 2), at(k + 3)) {
            (Some(x), ..) if is_ws(x) => k += 1,
            (Some('/'), Some('/'), third, fourth) => {
                let doc = third == Some('!') || (third == Some('/') && fourth != Some('/'));
                if doc {
                    return false;
                }
                while at(k).is_some_and(|x| x != '\n') {
                    k += 1;
                }
            }
            (Some('/'), Some('*'), third, fourth) => {
                let doc = third == Some('!') || (third == Some('*') && !matches!(fourth, Some('*' | '/')));
                if doc {
                    return false;
                }
                let mut depth = 1usize;
                k += 2;
                while depth > 0 {
                    match (at(k), at(k + 1)) {
                        (None, _) => return true,
                        (Some('/'), Some('*')) => {
                            depth += 1;
                            k += 2;
                        }
                        (Some('*'), Some('/')) => {
                            depth -= 1;
                            k += 2;
                        }
                        _ => k += 1,
                    }
                }
            }
            (Some('['), ..) => return true,
            _ => return false,
        }
    }
}

fn block_comment(cur: &mut Cur<'_>, line: u32, col: u32) -> Result<(), LexError> {
    cur.bump();
    cur.bump();
    let mut depth: usize = 1;
    while depth > 0 {
        match (cur.peek(0), cur.peek(1)) {
            (None, _) => return Err(err(line, col, "unterminated block comment")),
            (Some('/'), Some('*')) => {
                depth += 1;
                cur.bump();
                cur.bump();
            }
            (Some('*'), Some('/')) => {
                depth -= 1;
                cur.bump();
                cur.bump();
            }
            _ => {
                cur.bump();
            }
        }
    }
    Ok(())
}

/// Body of a `"`-delimited string with escapes; the opening quote is consumed.
fn string_body(cur: &mut Cur<'_>, line: u32, col: u32) -> Result<String, LexError> {
    let mut body = String::new();
    loop {
        match cur.bump() {
            None => return Err(err(line, col, "unterminated string literal")),
            Some('"') => return Ok(body),
            Some('\\') => {
                body.push('\\');
                match cur.bump() {
                    Some(c) => body.push(c),
                    None => return Err(err(line, col, "unterminated string literal")),
                }
            }
            Some(c) => body.push(c),
        }
    }
}

/// Body of a raw string: `cur` is at the first `#` or `"` after the `r`.
fn raw_body(cur: &mut Cur<'_>, line: u32, col: u32) -> Result<String, LexError> {
    let mut hashes = 0usize;
    while cur.peek(0) == Some('#') {
        hashes += 1;
        cur.bump();
    }
    if hashes > MAX_RAW_HASHES {
        return Err(err(line, col, "raw string with more than 255 `#`"));
    }
    if cur.bump() != Some('"') {
        return Err(err(line, col, "malformed raw string literal"));
    }
    let mut body = String::new();
    loop {
        match cur.bump() {
            None => return Err(err(line, col, "unterminated raw string literal")),
            Some('"') => {
                let closes = (0..hashes).all(|k| cur.peek(k) == Some('#'));
                if closes {
                    for _ in 0..hashes {
                        cur.bump();
                    }
                    return Ok(body);
                }
                body.push('"');
            }
            Some(c) => body.push(c),
        }
    }
}

/// True when the identifier suffix after a literal is non-empty (it is consumed).
fn suffix(cur: &mut Cur<'_>) -> bool {
    let mut s = String::new();
    if cur.peek(0).is_some_and(is_ident_start) {
        cur.eat_while(&mut s, is_ident_continue);
    }
    !s.is_empty()
}

/// A character literal body (opening quote consumed), up to and including the closing quote.
fn char_body(cur: &mut Cur<'_>, line: u32, col: u32) -> Result<String, LexError> {
    let mut body = String::new();
    for _ in 0..=MAX_CHAR_BODY {
        match cur.bump() {
            None | Some('\n') => break,
            Some('\'') if !body.is_empty() => return Ok(body),
            Some('\\') if body.is_empty() => {
                body.push('\\');
                if let Some(c) = cur.bump() {
                    body.push(c);
                }
            }
            Some(c) => body.push(c),
        }
    }
    Err(err(line, col, "unterminated character literal"))
}

/// `'`: a character literal or a lifetime / label.
fn quote(cur: &mut Cur<'_>, line: u32, col: u32) -> Result<Token, LexError> {
    let c1 = cur.peek(1);
    let c2 = cur.peek(2);
    let is_char = c1 == Some('\\') || (c1.is_some_and(|c| c != '\n' && c != '\'') && c2 == Some('\''));
    if is_char {
        cur.bump();
        let body = char_body(cur, line, col)?;
        let suffix = suffix(cur);
        return Ok(Token { kind: Kind::Lit { kind: LitKind::Char, suffix }, text: body, line, col });
    }
    if c1.is_some_and(is_ident_start) {
        cur.bump();
        if cur.peek(0) == Some('r') && cur.peek(1) == Some('#') && cur.peek(2).is_some_and(is_ident_start) {
            cur.bump();
            cur.bump();
        }
        let mut name = String::new();
        cur.eat_while(&mut name, is_ident_continue);
        if cur.peek(0).is_some_and(|c| !c.is_ascii() && c.is_alphanumeric()) {
            return Err(err(line, col, "non-ASCII lifetime: the scan classifies ASCII identifiers only"));
        }
        return Ok(Token { kind: Kind::Lifetime, text: name, line, col });
    }
    Err(err(line, col, "malformed character literal or lifetime"))
}

/// Literals with a letter prefix (`b'x'`, `b"…"`, `br"…"`, `r"…"`, `r#"…"#`, `c"…"`, `cr"…"`)
/// and raw identifiers (`r#name`). `None` when `cur` is not at one.
fn prefixed_literal(cur: &mut Cur<'_>, line: u32, col: u32) -> Result<Option<Token>, LexError> {
    let (c0, c1, c2) = (cur.peek(0), cur.peek(1), cur.peek(2));
    let raw_start = |c: Option<char>| matches!(c, Some('"' | '#'));
    let lit = |kind: LitKind, text: String, suffix: bool| Token { kind: Kind::Lit { kind, suffix }, text, line, col };
    match (c0, c1) {
        (Some('b'), Some('\'')) => {
            cur.bump();
            cur.bump();
            let body = char_body(cur, line, col)?;
            let s = suffix(cur);
            Ok(Some(lit(LitKind::Byte, body, s)))
        }
        (Some('b' | 'c'), Some('"')) => {
            cur.bump();
            cur.bump();
            let kind = if c0 == Some('b') { LitKind::ByteStr } else { LitKind::CStr };
            let body = string_body(cur, line, col)?;
            let s = suffix(cur);
            Ok(Some(lit(kind, body, s)))
        }
        (Some('b' | 'c'), Some('r')) if raw_start(c2) => {
            // `br#ident` is not a thing: a raw string must follow.
            cur.bump();
            cur.bump();
            let kind = if c0 == Some('b') { LitKind::RawByteStr } else { LitKind::RawCStr };
            let body = raw_body(cur, line, col)?;
            let s = suffix(cur);
            Ok(Some(lit(kind, body, s)))
        }
        (Some('r'), Some('"')) => {
            cur.bump();
            let body = raw_body(cur, line, col)?;
            let s = suffix(cur);
            Ok(Some(lit(LitKind::RawStr, body, s)))
        }
        (Some('r'), Some('#')) => {
            // `r#"…"#`, `r##"…"##` or the raw identifier `r#name`.
            let mut k = 1;
            while cur.peek(k) == Some('#') {
                k += 1;
            }
            if cur.peek(k) == Some('"') {
                cur.bump();
                let body = raw_body(cur, line, col)?;
                let s = suffix(cur);
                return Ok(Some(lit(LitKind::RawStr, body, s)));
            }
            if k == 2 && c2.is_some_and(is_ident_start) {
                cur.bump();
                cur.bump();
                let mut name = String::new();
                cur.eat_while(&mut name, is_ident_continue);
                if cur.peek(0).is_some_and(|c| !c.is_ascii() && c.is_alphanumeric()) {
                    return Err(err(line, col, "non-ASCII identifier: the scan classifies ASCII identifiers only"));
                }
                return Ok(Some(Token { kind: Kind::Ident { raw: true }, text: name, line, col }));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// A numeric literal. Float when it has a fraction (`1.` or `1.5`), a decimal exponent, or an
/// `f…` suffix (`1f64`).
fn number(cur: &mut Cur<'_>, line: u32, col: u32) -> Token {
    let mut text = String::new();
    let mut float = false;
    let radix = match (cur.peek(0), cur.peek(1)) {
        (Some('0'), Some('x' | 'o' | 'b')) => cur.peek(1),
        _ => None,
    };
    if let Some(r) = radix {
        text.push('0');
        text.push(r);
        cur.bump();
        cur.bump();
        if r == 'x' {
            cur.eat_while(&mut text, |c| c.is_ascii_hexdigit() || c == '_');
        } else {
            cur.eat_while(&mut text, |c| c.is_ascii_digit() || c == '_');
        }
    } else {
        cur.eat_while(&mut text, |c| c.is_ascii_digit() || c == '_');
        let after_dot = cur.peek(1);
        let dot_is_fraction = cur.peek(0) == Some('.')
            && after_dot != Some('.')
            && !after_dot.is_some_and(|c| is_ident_start(c) || (!c.is_ascii() && c.is_alphanumeric()));
        if dot_is_fraction {
            float = true;
            text.push('.');
            cur.bump();
            cur.eat_while(&mut text, |c| c.is_ascii_digit() || c == '_');
        }
        if matches!(cur.peek(0), Some('e' | 'E')) {
            let mut k = 1;
            if matches!(cur.peek(1), Some('+' | '-')) {
                k = 2;
            }
            let mut has_digit = false;
            let mut j = k;
            while let Some(c) = cur.peek(j) {
                if c.is_ascii_digit() {
                    has_digit = true;
                } else if c != '_' {
                    break;
                }
                j += 1;
            }
            if has_digit {
                float = true;
                for _ in 0..j {
                    if let Some(c) = cur.bump() {
                        text.push(c);
                    }
                }
            }
        }
    }
    let mut sfx = String::new();
    if cur.peek(0).is_some_and(is_ident_start) {
        cur.eat_while(&mut sfx, is_ident_continue);
    }
    if radix.is_none() && sfx.starts_with('f') {
        float = true;
    }
    text.push_str(&sfx);
    let kind = if float { LitKind::Float } else { LitKind::Int };
    Token { kind: Kind::Lit { kind, suffix: !sfx.is_empty() }, text, line, col }
}
