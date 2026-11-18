//! Hand-written lexer. Works on byte offsets; every slice goes through `str::get`, so a
//! hostile source cannot make it index out of bounds or split a character.

use super::{is_hidden_char, is_unit_char, syntax, Limit, ParseError, MAX_NAME_BYTES, MAX_TOKENS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kw {
    And,
    Else,
    False,
    If,
    Not,
    Or,
    Then,
    True,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum P {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tok {
    Int(i64),
    /// Decimal text `<digits>.<digits>`.
    Dec(String),
    /// Number text and unit symbol.
    Qty(String, String),
    Str(String),
    Ident(String),
    Kw(Kw),
    P(P),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

fn char_at(src: &str, i: usize) -> Option<char> {
    src.get(i..).and_then(|s| s.chars().next())
}

fn keyword(s: &str) -> Option<Kw> {
    Some(match s {
        "and" => Kw::And,
        "else" => Kw::Else,
        "false" => Kw::False,
        "if" => Kw::If,
        "not" => Kw::Not,
        "or" => Kw::Or,
        "then" => Kw::Then,
        "true" => Kw::True,
        _ => return None,
    })
}

/// Split `src` into tokens. Returns the tokens before the first lexical error and that
/// error: `expr_too_large` when a 1 025th token would begin, or `expr_syntax` at the first
/// malformed token (see the EBNF document for where each lexical error points). The caller
/// parses the returned tokens first, so errors are reported in source order.
pub(super) fn lex(src: &str) -> (Vec<Token>, Option<ParseError>) {
    let mut out = Vec::new();
    let err = lex_into(src, &mut out).err();
    (out, err)
}

fn lex_into(src: &str, out: &mut Vec<Token>) -> Result<(), ParseError> {
    let b = src.as_bytes();
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
            continue;
        }
        if out.len() >= MAX_TOKENS {
            return Err(ParseError::TooLarge(Limit::Tokens));
        }
        let start = i;
        let tok = match c {
            b'0'..=b'9' => {
                let (tok, end) = number(src, start)?;
                i = end;
                tok
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                    i += 1;
                }
                if i - start > MAX_NAME_BYTES {
                    return Err(syntax(start));
                }
                let word = src.get(start..i).ok_or(syntax(start))?;
                match keyword(word) {
                    Some(k) => Tok::Kw(k),
                    None => Tok::Ident(word.to_string()),
                }
            }
            b'"' => {
                let (s, end) = string(src, start)?;
                i = end;
                Tok::Str(s)
            }
            _ => {
                let next = b.get(i + 1).copied();
                let (p, len) = match (c, next) {
                    (b'(', _) => (P::LParen, 1),
                    (b')', _) => (P::RParen, 1),
                    (b'[', _) => (P::LBracket, 1),
                    (b']', _) => (P::RBracket, 1),
                    (b'.', _) => (P::Dot, 1),
                    (b',', _) => (P::Comma, 1),
                    (b'+', _) => (P::Plus, 1),
                    (b'-', _) => (P::Minus, 1),
                    (b'*', _) => (P::Star, 1),
                    (b'/', _) => (P::Slash, 1),
                    (b'%', _) => (P::Percent, 1),
                    (b'<', Some(b'=')) => (P::Le, 2),
                    (b'<', _) => (P::Lt, 1),
                    (b'>', Some(b'=')) => (P::Ge, 2),
                    (b'>', _) => (P::Gt, 1),
                    (b'=', Some(b'=')) => (P::EqEq, 2),
                    (b'!', Some(b'=')) => (P::Ne, 2),
                    // A lone `=` or `!`, or any other byte (including the first byte of a
                    // non-ASCII character outside a string).
                    _ => return Err(syntax(start)),
                };
                i += len;
                Tok::P(p)
            }
        };
        out.push(Token { tok, start, end: i });
    }
    Ok(())
}

/// Integer, decimal or quantity starting at `start` (an ASCII digit).
fn number(src: &str, start: usize) -> Result<(Tok, usize), ParseError> {
    let b = src.as_bytes();
    let mut i = start;
    let mut value: Option<i64> = Some(0);
    while let Some(&d) = b.get(i).filter(|c| c.is_ascii_digit()) {
        value = value.and_then(|v| v.checked_mul(10)).and_then(|v| v.checked_add(i64::from(d - b'0')));
        i += 1;
    }
    let mut point = false;
    if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        point = true;
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    let text = src.get(start..i).ok_or(syntax(start))?.to_string();
    if b.get(i) == Some(&b'[') {
        // Quantity: the unit bracket is part of the token. Any malformed unit (empty,
        // unclosed, or a character outside the unit set) points at the `[`.
        let open = i;
        i += 1;
        let unit_start = i;
        loop {
            match char_at(src, i) {
                Some(']') => break,
                Some(ch) if is_unit_char(ch) => i += ch.len_utf8(),
                _ => return Err(syntax(open)),
            }
        }
        if i == unit_start {
            return Err(syntax(open));
        }
        let unit = src.get(unit_start..i).ok_or(syntax(open))?.to_string();
        return Ok((Tok::Qty(text, unit), i + 1));
    }
    if point {
        return Ok((Tok::Dec(text), i));
    }
    match value {
        Some(v) => Ok((Tok::Int(v), i)),
        // Above i64::MAX.
        None => Err(syntax(start)),
    }
}

/// String literal starting at `start` (a `"`). Unclosed: offset of the opening quote; bad
/// escape: offset of its backslash; raw control or bidirectional formatting character
/// ([`is_hidden_char`]): its own offset. Such characters may be written as `\u{...}`.
fn string(src: &str, start: usize) -> Result<(String, usize), ParseError> {
    let mut out = String::new();
    let mut i = start + 1;
    loop {
        let Some(ch) = char_at(src, i) else {
            return Err(syntax(start));
        };
        match ch {
            '"' => return Ok((out, i + 1)),
            '\\' => {
                let esc = i;
                i += 1;
                let (c, len) = match char_at(src, i) {
                    Some('"') => ('"', 1),
                    Some('\\') => ('\\', 1),
                    Some('n') => ('\n', 1),
                    Some('r') => ('\r', 1),
                    Some('t') => ('\t', 1),
                    Some('u') => unicode_escape(src, i + 1).ok_or(syntax(esc))?,
                    _ => return Err(syntax(esc)),
                };
                out.push(c);
                i += len;
            }
            c if is_hidden_char(c) => return Err(syntax(i)),
            c => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
}

/// `{h..h}` (1 to 6 hex digits, a Unicode scalar value) at `i`, just after `\u`. Returns the
/// character and the number of bytes from the `u` to the closing brace inclusive.
fn unicode_escape(src: &str, i: usize) -> Option<(char, usize)> {
    let b = src.as_bytes();
    if b.get(i) != Some(&b'{') {
        return None;
    }
    let mut j = i + 1;
    let mut v: u32 = 0;
    while let Some(&h) = b.get(j).filter(|c| c.is_ascii_hexdigit()) {
        if j - i > 6 {
            return None;
        }
        let d = char::from(h).to_digit(16)?;
        v = v * 16 + d;
        j += 1;
    }
    if j == i + 1 || j - i - 1 > 6 || b.get(j) != Some(&b'}') {
        return None;
    }
    let c = char::from_u32(v)?;
    // From the `u` (at i - 1) to the `}` (at j) inclusive.
    Some((c, j + 2 - i))
}

#[cfg(test)]
mod tests {
    use super::keyword;
    use crate::expr::syntax::KEYWORDS;

    #[test]
    fn every_keyword_lexes_as_a_keyword() {
        assert!(KEYWORDS.iter().all(|k| keyword(k).is_some()));
        assert!(keyword("iffy").is_none());
    }
}
