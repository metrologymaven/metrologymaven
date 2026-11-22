//! Bounded DMIS lexer (spec node `IO.DMIS.PARSE.LEX`).
//!
//! Turns DMIS bytes into tokens grouped by logical statement. The rules below are the
//! project's own reading of the leaf spec, not text from the DMIS standard:
//!
//! - **Encoding**: one UTF-8 byte-order mark (EF BB BF) at the very start of the input is
//!   skipped; line 1 columns count from the byte after it.
//! - **Physical lines** end at LF; one CR right before the LF (or at the end of input) is
//!   dropped. A line longer than [`MAX_LINE_BYTES`] fails with
//!   `limit_exceeded{line_length}`. Blank lines outside a statement are skipped.
//! - **Comment lines**: a line whose first non-blank bytes are `$$` is one comment token
//!   running to the end of the line. Outside a continuation chain it is a logical
//!   statement of its own; inside a chain it joins the statement and the chain goes on.
//! - **Continuation**: a `$` followed only by spaces or tabs up to the end of the line
//!   joins the next physical line to the statement. A chain longer than
//!   [`MAX_CHAIN_LINES`] physical lines fails with `limit_exceeded{continuation_lines}`;
//!   input that ends inside a chain fails with `malformed{dangling_continuation}` at the
//!   last marker. Any other `$` is `malformed{unexpected_byte}`.
//! - **Tokens** (spaces and tabs separate them): words `[A-Za-z][A-Za-z0-9_]*`
//!   (case-insensitive: upper-case form plus the authored spelling); labels, an all-letter
//!   prefix directly followed by `(`, a name `[A-Za-z0-9_]+` and `)`, such as `F(P1)` or
//!   `fa(p1)`; the punctuation `/ , = ( )`; numbers
//!   `[+-]?(digits[.digits?] | .digits)([eE][+-]?digits)?` not followed by a letter, digit,
//!   `_` or `.`, kept as authored; strings in single quotes with `''` standing for one quote,
//!   closed on the same physical line.
//! - **`TEXT` statements**: after the first `,` of a statement whose first token is the word
//!   `TEXT`, if the next non-blank byte is not a quote, the rest of the physical line is one
//!   raw-text token (no continuation).
//! - **Bytes**: string, comment and raw-text content may hold any bytes and is returned as
//!   untrusted; elsewhere a byte of 0x80 or above is `malformed{non_ascii}` and any other
//!   byte that starts no token is `malformed{unexpected_byte}`.
//! - Every token carries the 1-based physical line and byte column of its first byte. A
//!   statement holds at most [`MAX_STATEMENT_TOKENS`] tokens (`limit_exceeded{statement_tokens}`).
//!   Every token is also charged as one element, and its kept bytes as tracked allocation,
//!   to the reader [`Budget`]; the input size is bounded by the budget's byte limit.
//!
//! Dialect relaxations ([`Relaxation`]) are off unless named in [`LexOptions`].

use fid_io_reg::{BoundedSource, Budget, IoError, LimitRequest, Position, Principal, UntrustedText};
use std::collections::BTreeSet;
use std::fmt;
use std::io::Read;

/// Longest physical line, in bytes, without its line terminator: 64 KiB [unverified: bound].
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// Most physical lines in one continuation chain [unverified: bound].
pub const MAX_CHAIN_LINES: u32 = 10_000;
/// Most tokens in one logical statement [unverified: bound].
pub const MAX_STATEMENT_TOKENS: usize = 100_000;

/// The UTF-8 byte-order mark, skipped once at the start of the input.
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

/// 1-based physical line and byte column of a token's first byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    /// 1-based physical line.
    pub line: u32,
    /// 1-based column, counted in bytes from the start of the line.
    pub col: u32,
}

/// The bytes of a token exactly as authored.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Lexeme(Box<[u8]>);

impl Lexeme {
    /// The authored bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True for an empty lexeme (never produced by the lexer).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Lexeme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lexeme({:?})", UntrustedText::from_utf8_lossy(&self.0))
    }
}

/// What a token is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A word; `upper` is its upper-case form, the lexeme keeps the authored spelling.
    Word {
        /// Upper-case form.
        upper: Box<str>,
    },
    /// A label such as `F(P1)`.
    Label {
        /// The prefix in upper case (`F`, `FA`, `T`, ...).
        prefix: Box<str>,
        /// The name between the parentheses, as authored.
        name: Box<str>,
    },
    /// A number; the lexeme is the authored text.
    Number,
    /// A quoted string; `value` is its content with `''` collapsed, untrusted bytes.
    Str {
        /// Decoded content bytes.
        value: Box<[u8]>,
    },
    /// `/`
    Slash,
    /// `,`
    Comma,
    /// `=`
    Equals,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// The rest of a `TEXT` line; the lexeme is the untrusted text.
    RawText,
    /// A comment; the lexeme starts with `$$`.
    Comment,
}

impl TokenKind {
    /// Stable snake_case name of the kind.
    pub fn name(&self) -> &'static str {
        match self {
            TokenKind::Word { .. } => "word",
            TokenKind::Label { .. } => "label",
            TokenKind::Number => "number",
            TokenKind::Str { .. } => "string",
            TokenKind::Slash => "slash",
            TokenKind::Comma => "comma",
            TokenKind::Equals => "equals",
            TokenKind::LParen => "lparen",
            TokenKind::RParen => "rparen",
            TokenKind::RawText => "raw_text",
            TokenKind::Comment => "comment",
        }
    }
}

/// One token.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    /// What it is.
    pub kind: TokenKind,
    /// The authored bytes.
    pub text: Lexeme,
    /// Where its first byte is.
    pub pos: Pos,
}

impl Token {
    /// The authored text of a structural token (word, label, number, punctuation), which
    /// is always ASCII. `None` for strings, raw text and comments: use
    /// [`Token::untrusted_text`] for those.
    pub fn ascii(&self) -> Option<&str> {
        match self.kind {
            TokenKind::Str { .. } | TokenKind::RawText | TokenKind::Comment => None,
            _ => std::str::from_utf8(self.text.as_bytes()).ok(),
        }
    }

    /// The file text of a string (decoded), raw-text token or comment (after `$$`), as
    /// untrusted text; invalid UTF-8 becomes U+FFFD. `None` for other kinds.
    pub fn untrusted_text(&self) -> Option<UntrustedText> {
        match &self.kind {
            TokenKind::Str { value } => Some(UntrustedText::from_utf8_lossy(value)),
            TokenKind::RawText => Some(UntrustedText::from_utf8_lossy(self.text.as_bytes())),
            TokenKind::Comment => Some(UntrustedText::from_utf8_lossy(self.text.as_bytes().get(2..).unwrap_or(&[]))),
            _ => None,
        }
    }
}

/// Physical lines joined into one statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalStatement {
    /// Tokens in input order.
    pub tokens: Vec<Token>,
    /// First physical line.
    pub first_line: u32,
    /// Last physical line.
    pub last_line: u32,
}

/// A dialect lexical relaxation (supplied by `IO.DMIS.DIAL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relaxation {
    /// `$$` after tokens on a line starts a comment running to the end of that line.
    InlineComment,
    /// A CR not followed by LF also ends a physical line.
    BareCrLineEnd,
    /// Accepts lowercase letters in label prefixes/names (dialect relaxation `allow_lowercase_labels`).
    AllowLowercaseLabels,
}

/// Lexer options; the default enables no relaxation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LexOptions {
    /// Enabled relaxations.
    pub relaxations: BTreeSet<Relaxation>,
}

impl LexOptions {
    /// Strict options: no relaxation.
    pub fn strict() -> Self {
        LexOptions::default()
    }

    /// These options with `r` enabled as well.
    pub fn with(mut self, r: Relaxation) -> Self {
        self.relaxations.insert(r);
        self
    }

    /// True if `r` is enabled.
    pub fn allows(&self, r: Relaxation) -> bool {
        self.relaxations.contains(&r)
    }
}

/// Which lexer bound was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bound {
    /// [`MAX_LINE_BYTES`].
    LineLength,
    /// [`MAX_CHAIN_LINES`].
    ContinuationLines,
    /// [`MAX_STATEMENT_TOKENS`].
    StatementTokens,
}

impl Bound {
    /// Stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            Bound::LineLength => "line_length",
            Bound::ContinuationLines => "continuation_lines",
            Bound::StatementTokens => "statement_tokens",
        }
    }
}

/// Why input is malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// A quoted string is not closed on its line.
    UnterminatedString,
    /// A byte that starts no token, or a `$` that is not a continuation marker.
    UnexpectedByte,
    /// A byte of 0x80 or above outside a string, comment or raw text.
    NonAscii,
    /// A number that breaks the number rule.
    BadNumber,
    /// The input ends inside a continuation chain.
    DanglingContinuation,
}

impl Reason {
    /// Stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::UnterminatedString => "unterminated_string",
            Reason::UnexpectedByte => "unexpected_byte",
            Reason::NonAscii => "non_ascii",
            Reason::BadNumber => "bad_number",
            Reason::DanglingContinuation => "dangling_continuation",
        }
    }
}

/// A lexing failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// The input breaks a lexical rule at `line`, `col`.
    Malformed {
        /// 1-based line.
        line: u32,
        /// 1-based byte column.
        col: u32,
        /// Why.
        reason: Reason,
    },
    /// A lexer bound was exceeded; `line` is the line (for a chain or statement, its first line).
    LimitExceeded {
        /// Which bound.
        bound: Bound,
        /// Starting line.
        line: u32,
    },
    /// The reader harness failed: budget limit (bytes, elements, allocation, wall time) or I/O.
    Io(IoError),
}

impl LexError {
    /// The stable error code (`malformed`, `limit_exceeded`, or the harness code).
    pub fn code(&self) -> &'static str {
        match self {
            LexError::Malformed { .. } => "malformed",
            LexError::LimitExceeded { .. } => "limit_exceeded",
            LexError::Io(e) => e.code(),
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::Malformed { line, col, reason } => {
                write!(f, "malformed: {} at line {line}, column {col}", reason.as_str())
            }
            LexError::LimitExceeded { bound, line } => {
                write!(f, "limit_exceeded: {} in the statement or line starting at line {line}", bound.as_str())
            }
            LexError::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LexError {}

/// Lexes everything `source` delivers.
pub fn lex<R: Read>(source: &mut BoundedSource<R>, options: &LexOptions) -> Result<Vec<LogicalStatement>, LexError> {
    let mut bytes = Vec::new();
    source.read_to_end_bounded(&mut bytes).map_err(LexError::Io)?;
    let start = if bytes.starts_with(UTF8_BOM) { UTF8_BOM.len() } else { 0 };
    let mut lexer = Lexer { src: &bytes, pos: start, line_no: 0, options, budget: source.budget_mut() };
    let mut out = Vec::new();
    while let Some(stmt) = lexer.next_statement()? {
        out.push(stmt);
    }
    Ok(out)
}

/// Lexes `data` under `budget`.
pub fn lex_slice(data: &[u8], budget: Budget, options: &LexOptions) -> Result<Vec<LogicalStatement>, LexError> {
    lex(&mut BoundedSource::new(data, budget), options)
}

/// Lexes `data` under the default limits of a script principal.
pub fn lex_bytes(data: &[u8], options: &LexOptions) -> Result<Vec<LogicalStatement>, LexError> {
    let budget = Budget::new(Principal::Script, &LimitRequest::default()).map_err(LexError::Io)?;
    lex_slice(data, budget, options)
}

/// The limits the fuzz target uses: 1 MiB input, 2^20 elements, 64 MiB tracked allocation.
pub fn fuzz_budget() -> Result<Budget, LexError> {
    Budget::new(Principal::Script, &LimitRequest::default().bytes(1 << 20).elements(1 << 20).alloc(1 << 26))
        .map_err(LexError::Io)
}

struct Line {
    start: usize,
    end: usize,
    number: u32,
}

struct Builder {
    tokens: Vec<Token>,
    first_line: u32,
    last_line: u32,
    lines: u32,
    marker: Pos,
    commas: u32,
    raw_next: bool,
}

impl Builder {
    fn is_text_statement(&self) -> bool {
        matches!(self.tokens.first(), Some(Token { kind: TokenKind::Word { upper }, .. }) if &**upper == "TEXT")
    }

    fn finish(self) -> LogicalStatement {
        LogicalStatement { tokens: self.tokens, first_line: self.first_line, last_line: self.last_line }
    }
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn col_of(i: usize) -> u32 {
    u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX)
}

fn ascii_box(bytes: &[u8]) -> Box<str> {
    String::from_utf8_lossy(bytes).into_owned().into_boxed_str()
}

fn count_digits(bytes: &[u8], from: usize) -> usize {
    bytes.get(from..).map_or(0, |rest| rest.iter().take_while(|b| b.is_ascii_digit()).count())
}

/// Length of the number starting at `i`, or `None` if it breaks the number rule.
fn number_len(bytes: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if matches!(bytes.get(j), Some(b'+' | b'-')) {
        j += 1;
    }
    let int = count_digits(bytes, j);
    j += int;
    let mut frac = 0;
    if bytes.get(j) == Some(&b'.') {
        j += 1;
        frac = count_digits(bytes, j);
        j += frac;
    }
    if int + frac == 0 {
        return None;
    }
    if matches!(bytes.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(bytes.get(k), Some(b'+' | b'-')) {
            k += 1;
        }
        let exp = count_digits(bytes, k);
        if exp == 0 {
            return None;
        }
        j = k + exp;
    }
    match bytes.get(j) {
        Some(&b) if is_name_byte(b) || b == b'.' => None,
        _ => Some(j - i),
    }
}

/// Word or label starting at `i` (an ASCII letter).
fn word_or_label(bytes: &[u8], i: usize) -> (TokenKind, usize) {
    let word_len = bytes.get(i..).map_or(0, |rest| rest.iter().take_while(|&&b| is_name_byte(b)).count());
    let j = i + word_len;
    let word = bytes.get(i..j).unwrap_or(&[]);
    if word.iter().all(u8::is_ascii_alphabetic) && bytes.get(j) == Some(&b'(') {
        let name_len = bytes.get(j + 1..).map_or(0, |rest| rest.iter().take_while(|&&b| is_name_byte(b)).count());
        let k = j + 1 + name_len;
        if name_len > 0 && bytes.get(k) == Some(&b')') {
            let prefix = ascii_box(word).to_ascii_uppercase().into_boxed_str();
            let name = ascii_box(bytes.get(j + 1..k).unwrap_or(&[]));
            return (TokenKind::Label { prefix, name }, k + 1 - i);
        }
    }
    (TokenKind::Word { upper: ascii_box(word).to_ascii_uppercase().into_boxed_str() }, word_len)
}

struct Lexer<'a, 'b> {
    src: &'a [u8],
    pos: usize,
    line_no: u32,
    options: &'a LexOptions,
    budget: &'b mut Budget,
}

impl Lexer<'_, '_> {
    /// The next physical line, bounded by [`MAX_LINE_BYTES`].
    fn next_line(&mut self) -> Result<Option<Line>, LexError> {
        let src = self.src;
        if self.pos >= src.len() {
            return Ok(None);
        }
        let start = self.pos;
        let number = self.line_no.saturating_add(1);
        self.line_no = number;
        let bare_cr = self.options.allows(Relaxation::BareCrLineEnd);
        let too_long = LexError::LimitExceeded { bound: Bound::LineLength, line: number };
        let mut i = start;
        let (mut end, next) = loop {
            match src.get(i) {
                None => break (i, i),
                Some(b'\n') => break (i, i + 1),
                Some(b'\r') if bare_cr && src.get(i + 1) != Some(&b'\n') => break (i, i + 1),
                Some(_) => i += 1,
            }
            // One CR may still be dropped, so stop scanning only beyond that.
            if i - start > MAX_LINE_BYTES + 1 {
                return Err(too_long);
            }
        };
        if end > start && src.get(end - 1) == Some(&b'\r') {
            end -= 1;
        }
        if end - start > MAX_LINE_BYTES {
            return Err(too_long);
        }
        self.pos = next;
        Ok(Some(Line { start, end, number }))
    }

    fn push(&mut self, b: &mut Builder, kind: TokenKind, text: &[u8], pos: Pos, byte: usize) -> Result<(), LexError> {
        if b.tokens.len() >= MAX_STATEMENT_TOKENS {
            return Err(LexError::LimitExceeded { bound: Bound::StatementTokens, line: b.first_line });
        }
        let at = Position::line_col(byte as u64, pos.line, pos.col);
        self.budget.charge_elements(1, at).map_err(LexError::Io)?;
        // Owned copies held besides the lexeme.
        let value_len = match &kind {
            TokenKind::Str { value } => value.len(),
            TokenKind::Word { upper } => upper.len(),
            TokenKind::Label { prefix, name } => prefix.len().saturating_add(name.len()),
            _ => 0,
        };
        let kept = text.len().saturating_add(value_len).saturating_add(std::mem::size_of::<Token>());
        self.budget.charge_alloc(kept as u64, at).map_err(LexError::Io)?;
        b.tokens.push(Token { kind, text: Lexeme(Box::from(text)), pos });
        Ok(())
    }

    fn next_statement(&mut self) -> Result<Option<LogicalStatement>, LexError> {
        let mut open: Option<Builder> = None;
        loop {
            let Some(line) = self.next_line()? else {
                return match open {
                    None => Ok(None),
                    Some(b) => Err(LexError::Malformed {
                        line: b.marker.line,
                        col: b.marker.col,
                        reason: Reason::DanglingContinuation,
                    }),
                };
            };
            let src = self.src;
            let bytes = src.get(line.start..line.end).unwrap_or(&[]);
            let first = bytes.iter().position(|&c| !is_blank(c));
            let in_chain = open.is_some();
            if !in_chain && first.is_none() {
                continue;
            }
            let mut b = open.take().unwrap_or_else(|| Builder {
                tokens: Vec::new(),
                first_line: line.number,
                last_line: line.number,
                lines: 0,
                marker: Pos { line: line.number, col: 1 },
                commas: 0,
                raw_next: false,
            });
            b.lines = b.lines.saturating_add(1);
            if b.lines > MAX_CHAIN_LINES {
                return Err(LexError::LimitExceeded { bound: Bound::ContinuationLines, line: b.first_line });
            }
            b.last_line = line.number;
            let Some(f) = first else {
                // A blank line inside a chain: the chain stays open.
                open = Some(b);
                continue;
            };
            let rest = bytes.get(f..).unwrap_or(&[]);
            if rest.starts_with(b"$$") {
                let pos = Pos { line: line.number, col: col_of(f) };
                self.push(&mut b, TokenKind::Comment, rest, pos, line.start + f)?;
                if in_chain {
                    open = Some(b);
                    continue;
                }
                return Ok(Some(b.finish()));
            }
            if self.lex_line(&line, bytes, &mut b)? {
                open = Some(b);
            } else {
                return Ok(Some(b.finish()));
            }
        }
    }

    /// Lexes one non-comment line into `b`; true if it ends with a continuation marker.
    fn lex_line(&mut self, line: &Line, bytes: &[u8], b: &mut Builder) -> Result<bool, LexError> {
        let mut i = 0usize;
        while let Some(&c) = bytes.get(i) {
            if is_blank(c) {
                i += 1;
                continue;
            }
            let pos = Pos { line: line.number, col: col_of(i) };
            let byte = line.start + i;
            let malformed = |reason| LexError::Malformed { line: pos.line, col: pos.col, reason };
            if b.raw_next {
                b.raw_next = false;
                if c != b'\'' {
                    self.push(b, TokenKind::RawText, bytes.get(i..).unwrap_or(&[]), pos, byte)?;
                    return Ok(false);
                }
            }
            let (kind, len) = match c {
                b'\'' => string(bytes, i).ok_or(malformed(Reason::UnterminatedString))?,
                b'/' => (TokenKind::Slash, 1),
                b',' => (TokenKind::Comma, 1),
                b'=' => (TokenKind::Equals, 1),
                b'(' => (TokenKind::LParen, 1),
                b')' => (TokenKind::RParen, 1),
                b'A'..=b'Z' | b'a'..=b'z' => word_or_label(bytes, i),
                b'0'..=b'9' | b'.' | b'+' | b'-' => {
                    (TokenKind::Number, number_len(bytes, i).ok_or(malformed(Reason::BadNumber))?)
                }
                b'$' => {
                    let after = bytes.get(i + 1..).unwrap_or(&[]);
                    if after.iter().all(|&x| is_blank(x)) {
                        b.marker = pos;
                        return Ok(true);
                    }
                    if self.options.allows(Relaxation::InlineComment) && after.first() == Some(&b'$') {
                        self.push(b, TokenKind::Comment, bytes.get(i..).unwrap_or(&[]), pos, byte)?;
                        return Ok(false);
                    }
                    return Err(malformed(Reason::UnexpectedByte));
                }
                0x80..=0xff => return Err(malformed(Reason::NonAscii)),
                _ => return Err(malformed(Reason::UnexpectedByte)),
            };
            let is_comma = kind == TokenKind::Comma;
            self.push(b, kind, bytes.get(i..i + len).unwrap_or(&[]), pos, byte)?;
            if is_comma {
                if b.commas == 0 && b.is_text_statement() {
                    b.raw_next = true;
                }
                b.commas = b.commas.saturating_add(1);
            }
            i += len;
        }
        Ok(false)
    }
}

/// The quoted string starting at `i`: its kind and length, or `None` if the line ends first.
fn string(bytes: &[u8], i: usize) -> Option<(TokenKind, usize)> {
    let mut j = i + 1;
    let mut value = Vec::new();
    loop {
        match bytes.get(j) {
            None => return None,
            Some(b'\'') => {
                if bytes.get(j + 1) == Some(&b'\'') {
                    value.push(b'\'');
                    j += 2;
                } else {
                    j += 1;
                    break;
                }
            }
            Some(&x) => {
                value.push(x);
                j += 1;
            }
        }
    }
    Some((TokenKind::Str { value: value.into_boxed_slice() }, j - i))
}
