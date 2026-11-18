//! Bounded SPDX licence-expression parser and evaluator (REQ-KRN.NUM.LINT.GRAPH-004).
//!
//! Grammar, precedence `WITH` > `AND` > `OR`, operators upper case only:
//!
//! ```text
//! or   = and  { "OR"  and }
//! and  = with { "AND" with }
//! with = atom [ "WITH" id ]
//! atom = id | "(" or ")"
//! ```
//!
//! The legacy crates.io separator `/` is read as `OR`. Identifiers (including `+` suffixes and
//! `LicenseRef-…`) are compared literally against the allowlist, so anything unknown is denied.

use std::collections::BTreeSet;

/// Longest accepted expression, in bytes.
pub const MAX_EXPR_BYTES: usize = 1024;
/// Most tokens in one expression.
pub const MAX_TOKENS: usize = 256;
/// Deepest parenthesis nesting.
pub const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Id(String),
    And,
    Or,
    With,
    Open,
    Close,
}

/// A parsed licence expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Licence(String),
    With(String, String),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

fn is_id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'+' || b == b':'
}

fn lex(text: &str) -> Result<Vec<Tok>, String> {
    if text.len() > MAX_EXPR_BYTES {
        return Err(format!("expression longer than {MAX_EXPR_BYTES} bytes"));
    }
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b' ' || c == b'\t' {
            i += 1;
            continue;
        }
        let tok = match c {
            b'(' => {
                i += 1;
                Tok::Open
            }
            b')' => {
                i += 1;
                Tok::Close
            }
            b'/' => {
                i += 1;
                Tok::Or
            }
            _ if is_id_byte(c) => {
                let start = i;
                while i < b.len() && is_id_byte(b[i]) {
                    i += 1;
                }
                let word = text.get(start..i).unwrap_or_default();
                match word {
                    "AND" => Tok::And,
                    "OR" => Tok::Or,
                    "WITH" => Tok::With,
                    _ => Tok::Id(word.to_string()),
                }
            }
            _ => return Err(format!("unexpected character at byte {i}")),
        };
        out.push(tok);
        if out.len() > MAX_TOKENS {
            return Err(format!("more than {MAX_TOKENS} tokens"));
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn or(&mut self, depth: usize) -> Result<Expr, String> {
        let mut left = self.and(depth)?;
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            let right = self.and(depth)?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self, depth: usize) -> Result<Expr, String> {
        let mut left = self.with(depth)?;
        while self.peek() == Some(&Tok::And) {
            self.pos += 1;
            let right = self.with(depth)?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn with(&mut self, depth: usize) -> Result<Expr, String> {
        let atom = self.atom(depth)?;
        if self.peek() != Some(&Tok::With) {
            return Ok(atom);
        }
        self.pos += 1;
        let Expr::Licence(id) = atom else {
            return Err("WITH must follow a single licence identifier".to_string());
        };
        match self.bump() {
            Some(Tok::Id(exc)) => Ok(Expr::With(id, exc)),
            _ => Err("WITH must be followed by an exception identifier".to_string()),
        }
    }

    fn atom(&mut self, depth: usize) -> Result<Expr, String> {
        match self.bump() {
            Some(Tok::Id(id)) => Ok(Expr::Licence(id)),
            Some(Tok::Open) => {
                if depth >= MAX_DEPTH {
                    return Err(format!("parentheses nested deeper than {MAX_DEPTH}"));
                }
                let inner = self.or(depth + 1)?;
                match self.bump() {
                    Some(Tok::Close) => Ok(inner),
                    _ => Err("missing `)`".to_string()),
                }
            }
            Some(t) => Err(format!("unexpected token {t:?}")),
            None => Err("unexpected end of expression".to_string()),
        }
    }
}

/// Parses an SPDX expression. Errors name the problem; they never panic.
pub fn parse(text: &str) -> Result<Expr, String> {
    let toks = lex(text)?;
    if toks.is_empty() {
        return Err("empty expression".to_string());
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.or(0)?;
    if p.pos != p.toks.len() {
        return Err(format!("unexpected token after position {}", p.pos));
    }
    Ok(e)
}

/// Evaluates a parsed expression: `OR` needs one allowed side, `AND` both, and `WITH` an
/// allowed licence and an allowed exception.
pub fn allowed(e: &Expr, licences: &BTreeSet<String>, exceptions: &BTreeSet<String>) -> bool {
    match e {
        Expr::Licence(id) => licences.contains(id),
        Expr::With(id, exc) => licences.contains(id) && exceptions.contains(exc),
        Expr::And(a, b) => allowed(a, licences, exceptions) && allowed(b, licences, exceptions),
        Expr::Or(a, b) => allowed(a, licences, exceptions) || allowed(b, licences, exceptions),
    }
}

/// Parses and evaluates `text`. `Err` carries the parse error.
pub fn evaluate(text: &str, licences: &BTreeSet<String>, exceptions: &BTreeSet<String>) -> Result<bool, String> {
    parse(text).map(|e| allowed(&e, licences, exceptions))
}
