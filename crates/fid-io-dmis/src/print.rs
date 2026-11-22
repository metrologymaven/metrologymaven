//! Canonical DMIS printer and parse-print-parse identity harness (spec node
//! `IO.DMIS.PARSE.PRINT`).
//!
//! [`print`] renders a [`DmisTree`] back to canonical DMIS text:
//!
//! - Registered statements emit upper-case words (`Word.upper`), upper-case label prefixes with
//!   authored names (`F(p1)`), and numeric, string, raw-text and comment tokens with their exact
//!   authored spelling (numbers are never re-formatted).
//! - [`Unknown`] nodes are printed from their raw tokens verbatim so they re-parse to the same
//!   `Unknown` node.
//! - Comments inside a continuation chain (`Statement::trivia` or comments inside `Unknown`) are
//!   emitted on their own `$$...` physical line after a `$` continuation marker so strict lexing
//!   preserves them inside the same logical statement.
//! - Lines wrap greedily at argument separators with the `$` continuation marker so no line
//!   exceeds `options.width` (default 80, clamped to `[1, MAX_LINE_BYTES]`), except when a single
//!   token itself is longer, in which case that token stands alone on its line.
//! - Output is D0 deterministic: printing the same tree with the same [`PrintOptions`] always
//!   produces byte-identical output.
//!
//! [`print_bytes`] performs the same formatting directly into `Vec<u8>` so even comments,
//! strings or raw-text tokens containing non-UTF-8 bytes round-trip losslessly.

use crate::lex::{Token, TokenKind, MAX_LINE_BYTES};
use crate::registry::{Dialect, Registry};
use crate::tree::{
    parse_in, Arg, DmisTree, Group, Item, Label, Node, ParseError, ParseOptions, Statement, Trivia, Unknown,
};
use std::fmt;
use std::path::{Path, PathBuf};

/// Default maximum line width in columns/bytes.
pub const DEFAULT_WIDTH: u16 = 80;

/// Maximum corpus manifest size accepted by [`verify_corpus_manifest`] (4 MiB).
pub const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

/// Maximum number of entries accepted in a corpus manifest.
pub const MAX_MANIFEST_ENTRIES: usize = 10_000;

/// Options controlling canonical DMIS printing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrintOptions {
    /// Maximum line width in columns/bytes (default [`DEFAULT_WIDTH`] = 80).
    /// Values of `0` are clamped to `1`; `u16::MAX` (65,535) is always within [`MAX_LINE_BYTES`].
    pub width: u16,
    /// Emit `\r\n` line endings when `true`, or `\n` when `false` (default).
    pub crlf: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self { width: DEFAULT_WIDTH, crlf: false }
    }
}

impl PrintOptions {
    /// Effective width clamped to `[1, MAX_LINE_BYTES]`.
    fn effective_width(self) -> usize {
        usize::from(self.width).clamp(1, MAX_LINE_BYTES)
    }

    fn eol(self) -> &'static [u8] {
        if self.crlf {
            b"\r\n"
        } else {
            b"\n"
        }
    }
}

/// Prints `tree` in canonical DMIS layout under `options`.
///
/// For any tree whose string, raw-text and comment tokens are valid UTF-8, the returned string
/// has the exact bytes of [`print_bytes`]. Invalid UTF-8 inside untrusted text tokens is
/// lossily replaced with U+FFFD (use [`print_bytes`] to preserve arbitrary raw bytes).
pub fn print(tree: &DmisTree, options: &PrintOptions) -> String {
    let bytes = print_bytes(tree, options);
    match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// Prints `tree` in canonical DMIS layout directly as bytes, preserving raw bytes in string,
/// raw-text and comment tokens.
pub fn print_bytes(tree: &DmisTree, options: &PrintOptions) -> Vec<u8> {
    let mut out = Vec::new();
    let max_w = options.effective_width();
    let eol = options.eol();

    for node in &tree.statements {
        match node {
            Node::Comment(tok) => {
                out.extend_from_slice(tok.text.as_bytes());
                out.extend_from_slice(eol);
            }
            Node::Statement(stmt) => {
                let raw = stmt.tokens();
                emit_token_stream(&raw, true, max_w, eol, &mut out);
            }
            Node::Unknown(unk) => {
                let refs: Vec<&Token> = unk.tokens.iter().collect();
                emit_token_stream(&refs, false, max_w, eol, &mut out);
            }
        }
    }
    out
}

/// One token prepared for emission.
struct Emitted<'a> {
    tok: &'a Token,
    bytes: Vec<u8>,
}

fn token_bytes(tok: &Token, canonical_words: bool) -> Vec<u8> {
    if !canonical_words {
        return tok.text.as_bytes().to_vec();
    }
    match &tok.kind {
        TokenKind::Word { upper } => upper.as_bytes().to_vec(),
        TokenKind::Label { prefix, name } => {
            let mut buf = Vec::with_capacity(prefix.len().saturating_add(name.len()).saturating_add(2));
            buf.extend_from_slice(prefix.as_bytes());
            buf.push(b'(');
            buf.extend_from_slice(name.as_bytes());
            buf.push(b')');
            buf
        }
        // Numbers, strings, raw text, comments and punctuation keep their exact authored bytes.
        TokenKind::Number
        | TokenKind::Str { .. }
        | TokenKind::RawText
        | TokenKind::Comment
        | TokenKind::Slash
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::LParen
        | TokenKind::RParen => tok.text.as_bytes().to_vec(),
    }
}

/// Whether two adjacent non-comment tokens on the same physical line need a single space
/// separator so the lexer does not fuse or mis-lex them.
fn needs_space(prev: &TokenKind, next: &TokenKind) -> bool {
    let prev_val =
        matches!(prev, TokenKind::Word { .. } | TokenKind::Label { .. } | TokenKind::Number | TokenKind::Str { .. });
    let next_val =
        matches!(next, TokenKind::Word { .. } | TokenKind::Label { .. } | TokenKind::Number | TokenKind::Str { .. });
    if prev_val && next_val {
        return true;
    }
    // A word directly followed by '(' would lex as a Label (e.g. FOO(1)).
    matches!(prev, TokenKind::Word { .. }) && matches!(next, TokenKind::LParen)
}

/// Emits one logical statement (a `Statement` or `Unknown`) with greedy wrapping at argument
/// separators (`Comma`) and strict-mode continuation handling for embedded `Comment` tokens.
fn emit_token_stream(tokens: &[&Token], canonical_words: bool, max_w: usize, eol: &[u8], out: &mut Vec<u8>) {
    if tokens.is_empty() {
        return;
    }
    let prepared: Vec<Emitted<'_>> =
        tokens.iter().map(|&tok| Emitted { tok, bytes: token_bytes(tok, canonical_words) }).collect();

    let mut cur_line: Vec<u8> = Vec::new();
    let mut last_kind: Option<&TokenKind> = None;
    let mut i = 0usize;

    while i < prepared.len() {
        let Some(item) = prepared.get(i) else {
            break;
        };

        // Embedded comment line inside a continuation chain.
        if matches!(item.tok.kind, TokenKind::Comment) {
            if !cur_line.is_empty() {
                cur_line.push(b'$');
                out.extend_from_slice(&cur_line);
                out.extend_from_slice(eol);
                cur_line.clear();
                last_kind = None;
            }
            out.extend_from_slice(&item.bytes);
            out.extend_from_slice(eol);
            i += 1;
            continue;
        }

        // Pair a non-punctuation token with an immediately following Comma so wrapping happens
        // cleanly *after* the comma separator whenever the pair fits on a line with '$'.
        // Exception: when the token after the comma is RawText (a `TEXT` statement), the lexer
        // switches into raw-text mode immediately after `,` and would treat a trailing `$` as
        // raw text rather than continuation. Leave `,` to pair with `RawText` instead.
        let pair_comma = if !matches!(item.tok.kind, TokenKind::Comma) {
            match prepared.get(i + 1) {
                Some(next) if matches!(next.tok.kind, TokenKind::Comma) => {
                    let next_after_is_raw =
                        prepared.get(i + 2).is_some_and(|t| matches!(t.tok.kind, TokenKind::RawText));
                    if next_after_is_raw {
                        false
                    } else {
                        let has_after_pair = i + 2 < prepared.len();
                        let cont_for_pair = usize::from(has_after_pair);
                        item.bytes.len().saturating_add(next.bytes.len()).saturating_add(cont_for_pair) <= max_w
                    }
                }
                _ => false,
            }
        } else {
            false
        };

        // Always keep Comma + RawText together on the same line: after `,` in a TEXT statement,
        // the lexer treats the next non-blank byte (including `$`) as raw text to end of line.
        let pair_raw_text = if matches!(item.tok.kind, TokenKind::Comma) {
            prepared.get(i + 1).is_some_and(|t| matches!(t.tok.kind, TokenKind::RawText))
        } else {
            false
        };

        let unit_end = if pair_comma || pair_raw_text { i + 2 } else { i + 1 };
        let has_more = unit_end < prepared.len();
        let cont_len = usize::from(has_more);

        let unit_len: usize =
            prepared.get(i..unit_end).unwrap_or(&[]).iter().fold(0usize, |acc, e| acc.saturating_add(e.bytes.len()));

        let sep_len = match last_kind {
            Some(prev) if needs_space(prev, &item.tok.kind) => 1usize,
            _ => 0usize,
        };

        let fits_current =
            cur_line.len().saturating_add(sep_len).saturating_add(unit_len).saturating_add(cont_len) <= max_w;

        if fits_current {
            if sep_len == 1 {
                cur_line.push(b' ');
            }
            for e in prepared.get(i..unit_end).unwrap_or(&[]) {
                cur_line.extend_from_slice(&e.bytes);
                last_kind = Some(&e.tok.kind);
            }
            i = unit_end;
            continue;
        }

        // Does not fit on the current line. If the current line is non-empty, flush it with '$'.
        if !cur_line.is_empty() {
            cur_line.push(b'$');
            out.extend_from_slice(&cur_line);
            out.extend_from_slice(eol);
            cur_line.clear();
            last_kind = None;
            // Re-evaluate this unit on the fresh line without advancing `i`.
            continue;
        }

        // Current line is empty, yet the unit still exceeds `max_w` (with its trailing '$', if any).
        // That means a single token is longer than `max_w` (or equal to `max_w` with more tokens).
        // Per REQ-IO.DMIS.PARSE.PRINT-003, that token stands alone on its line.
        for e in prepared.get(i..unit_end).unwrap_or(&[]) {
            cur_line.extend_from_slice(&e.bytes);
        }
        if has_more {
            cur_line.push(b'$');
            out.extend_from_slice(&cur_line);
            out.extend_from_slice(eol);
            cur_line.clear();
            last_kind = None;
        } else {
            last_kind = prepared.get(unit_end.saturating_sub(1)).map(|e| &e.tok.kind);
        }
        i = unit_end;
    }

    if !cur_line.is_empty() {
        out.extend_from_slice(&cur_line);
        out.extend_from_slice(eol);
    }
}

/// Compares two parsed DMIS trees for structural and semantic equality, ignoring source
/// positions (`Pos`).
///
/// For `Word` and `Label` tokens inside registered statements, the canonical upper-case word and
/// upper-case label prefix (with authored label name) are compared because the canonical printer
/// normalises keywords and label prefixes to upper case. For `Number`, `Str`, `RawText`, `Comment`
/// and punctuation tokens, both kind and exact authored lexeme bytes must match. For `Unknown`
/// nodes, raw tokens must match verbatim (kind and authored bytes).
pub fn trees_equal_ignoring_pos(a: &DmisTree, b: &DmisTree) -> bool {
    if a.statements.len() != b.statements.len() {
        return false;
    }
    a.statements.iter().zip(b.statements.iter()).all(|(na, nb)| node_equal(na, nb))
}

fn node_equal(a: &Node, b: &Node) -> bool {
    match (a, b) {
        (Node::Comment(ta), Node::Comment(tb)) => token_equal_raw(ta, tb),
        (Node::Unknown(ua), Node::Unknown(ub)) => unknown_equal(ua, ub),
        (Node::Statement(sa), Node::Statement(sb)) => statement_equal(sa, sb),
        _ => false,
    }
}

fn unknown_equal(a: &Unknown, b: &Unknown) -> bool {
    a.keyword == b.keyword
        && a.reason == b.reason
        && a.tokens.len() == b.tokens.len()
        && a.tokens.iter().zip(b.tokens.iter()).all(|(ta, tb)| token_equal_raw(ta, tb))
}

fn statement_equal(a: &Statement, b: &Statement) -> bool {
    a.class == b.class
        && label_opt_equal(a.label.as_ref(), b.label.as_ref())
        && token_equal_canonical(&a.major, &b.major)
        && token_opt_equal_canonical(a.slash.as_ref(), b.slash.as_ref())
        && args_equal(&a.args, &b.args)
        && trivia_equal(&a.trivia, &b.trivia)
}

fn label_opt_equal(a: Option<&Label>, b: Option<&Label>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(la), Some(lb)) => {
            token_equal_canonical(&la.token, &lb.token) && token_equal_canonical(&la.equals, &lb.equals)
        }
        _ => false,
    }
}

fn token_opt_equal_canonical(a: Option<&Token>, b: Option<&Token>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(ta), Some(tb)) => token_equal_canonical(ta, tb),
        _ => false,
    }
}

fn trivia_equal(a: &[Trivia], b: &[Trivia]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(ta, tb)| ta.before == tb.before && token_equal_raw(&ta.token, &tb.token))
}

/// Compares two argument lists iteratively (bounded depth, no recursion stack blow-up).
fn args_equal(a: &[Arg], b: &[Arg]) -> bool {
    let mut stack: Vec<(&[Arg], &[Arg])> = vec![(a, b)];
    while let Some((la, lb)) = stack.pop() {
        if la.len() != lb.len() {
            return false;
        }
        for (aa, ab) in la.iter().zip(lb.iter()) {
            if !token_opt_equal_canonical(aa.comma.as_ref(), ab.comma.as_ref()) {
                return false;
            }
            if aa.items.len() != ab.items.len() {
                return false;
            }
            for (ia, ib) in aa.items.iter().zip(ab.items.iter()) {
                match (ia, ib) {
                    (Item::Atom(ta), Item::Atom(tb)) => {
                        if !token_equal_canonical(ta, tb) {
                            return false;
                        }
                    }
                    (Item::Group(ga), Item::Group(gb)) => {
                        if !group_delims_equal(ga, gb) {
                            return false;
                        }
                        stack.push((&ga.args, &gb.args));
                    }
                    _ => return false,
                }
            }
        }
    }
    true
}

fn group_delims_equal(a: &Group, b: &Group) -> bool {
    token_equal_canonical(&a.open, &b.open) && token_equal_canonical(&a.close, &b.close)
}

fn token_equal_raw(a: &Token, b: &Token) -> bool {
    a.kind == b.kind && a.text == b.text
}

fn token_equal_canonical(a: &Token, b: &Token) -> bool {
    match (&a.kind, &b.kind) {
        (TokenKind::Word { upper: ua }, TokenKind::Word { upper: ub }) => ua == ub,
        (TokenKind::Label { prefix: pa, name: na }, TokenKind::Label { prefix: pb, name: nb }) => pa == pb && na == nb,
        _ => token_equal_raw(a, b),
    }
}

/// Outcome of running the parse -> print -> parse identity check on one DMIS input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityOutcome {
    /// Number of logical statements in the parsed tree.
    pub statements: usize,
    /// Printed bytes produced from the first parse.
    pub printed: Vec<u8>,
}

/// Failure from [`check_identity`] or [`verify_corpus_manifest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// Reading a manifest or corpus file failed.
    Io {
        /// File path that failed.
        path: PathBuf,
        /// Error message.
        message: String,
    },
    /// The corpus manifest TOML is malformed or exceeds bounds.
    Manifest {
        /// Manifest path.
        path: PathBuf,
        /// Error message.
        message: String,
    },
    /// First parse failed.
    FirstParse {
        /// Entry label or file path.
        entry: String,
        /// Underlying parse error.
        error: ParseError,
    },
    /// Second parse (of the printed output) failed.
    Reparse {
        /// Entry label or file path.
        entry: String,
        /// Underlying parse error.
        error: ParseError,
    },
    /// First and second trees differ (ignoring source positions).
    TreeMismatch {
        /// Entry label or file path.
        entry: String,
        /// First differing statement index (0-based), if any.
        statement_index: Option<usize>,
    },
    /// Printing the same tree twice produced different bytes (REQ-IO.DMIS.PARSE.PRINT-004).
    NonDeterministic {
        /// Entry label or file path.
        entry: String,
    },
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityError::Io { path, message } => {
                write!(f, "io error reading {}: {message}", path.display())
            }
            IdentityError::Manifest { path, message } => {
                write!(f, "invalid corpus manifest {}: {message}", path.display())
            }
            IdentityError::FirstParse { entry, error } => {
                write!(f, "{entry}: first parse failed: {error}")
            }
            IdentityError::Reparse { entry, error } => {
                write!(f, "{entry}: re-parse after print failed: {error}")
            }
            IdentityError::TreeMismatch { entry, statement_index } => match statement_index {
                Some(idx) => write!(f, "{entry}: tree mismatch at statement {idx}"),
                None => write!(f, "{entry}: tree statement count mismatch"),
            },
            IdentityError::NonDeterministic { entry } => {
                write!(f, "{entry}: printing the same tree twice gave different bytes")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

/// Checks parse -> print -> parse identity (and D0 determinism of [`print_bytes`]) on `data`.
pub fn check_identity(
    entry: &str,
    data: &[u8],
    registry: &Registry,
    dialect: Option<&Dialect>,
    parse_options: ParseOptions,
    print_options: &PrintOptions,
) -> Result<IdentityOutcome, IdentityError> {
    let first = parse_in(data, registry, dialect, parse_options)
        .map_err(|error| IdentityError::FirstParse { entry: entry.to_owned(), error })?;
    let printed = print_bytes(&first, print_options);
    let printed_again = print_bytes(&first, print_options);
    if printed != printed_again {
        return Err(IdentityError::NonDeterministic { entry: entry.to_owned() });
    }
    let second = parse_in(&printed, registry, dialect, parse_options)
        .map_err(|error| IdentityError::Reparse { entry: entry.to_owned(), error })?;
    if !trees_equal_ignoring_pos(&first, &second) {
        let diff_idx = first.statements.iter().zip(second.statements.iter()).position(|(a, b)| !node_equal(a, b));
        return Err(IdentityError::TreeMismatch { entry: entry.to_owned(), statement_index: diff_idx });
    }
    Ok(IdentityOutcome { statements: first.statements.len(), printed })
}

/// Report from running the identity harness over an `IO.CORP.MANIF` DMIS manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusIdentityReport {
    /// Number of `accept` entries verified.
    pub checked: usize,
    /// Number of non-`accept` (e.g. `reject`) or remote-only entries skipped.
    pub skipped: usize,
    /// Relative paths of checked entries, in manifest order.
    pub checked_paths: Vec<String>,
}

/// Runs the parse -> print -> parse identity harness over every local `accept` entry in an
/// `IO.CORP.MANIF` TOML manifest (such as `corpus/dmis.toml`).
pub fn verify_corpus_manifest(
    manifest_path: &Path,
    registry: &Registry,
    dialect: Option<&Dialect>,
    parse_options: ParseOptions,
    print_options: &PrintOptions,
) -> Result<CorpusIdentityReport, IdentityError> {
    let bytes = std::fs::read(manifest_path)
        .map_err(|e| IdentityError::Io { path: manifest_path.to_path_buf(), message: e.to_string() })?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(IdentityError::Manifest {
            path: manifest_path.to_path_buf(),
            message: format!("manifest exceeds {MAX_MANIFEST_BYTES} bytes"),
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| IdentityError::Manifest {
        path: manifest_path.to_path_buf(),
        message: format!("manifest is not valid UTF-8: {e}"),
    })?;
    let table: toml::Table = toml::from_str(text)
        .map_err(|e| IdentityError::Manifest { path: manifest_path.to_path_buf(), message: e.to_string() })?;
    let Some(entries_val) = table.get("entry") else {
        return Err(IdentityError::Manifest {
            path: manifest_path.to_path_buf(),
            message: "missing [[entry]] array".to_owned(),
        });
    };
    let Some(entries) = entries_val.as_array() else {
        return Err(IdentityError::Manifest {
            path: manifest_path.to_path_buf(),
            message: "entry must be an array of tables".to_owned(),
        });
    };
    if entries.len() > MAX_MANIFEST_ENTRIES {
        return Err(IdentityError::Manifest {
            path: manifest_path.to_path_buf(),
            message: format!("entry count exceeds {MAX_MANIFEST_ENTRIES}"),
        });
    }

    let base_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut checked = 0usize;
    let mut skipped = 0usize;
    let mut checked_paths = Vec::new();

    for (idx, val) in entries.iter().enumerate() {
        let Some(entry) = val.as_table() else {
            return Err(IdentityError::Manifest {
                path: manifest_path.to_path_buf(),
                message: format!("entry[{idx}] is not a table"),
            });
        };
        let is_accept =
            entry.get("expect").and_then(toml::Value::as_table).is_some_and(|exp| exp.contains_key("accept"));
        if !is_accept {
            skipped += 1;
            continue;
        }
        let Some(rel_path) = entry.get("path").and_then(toml::Value::as_str) else {
            skipped += 1;
            continue;
        };
        let full_path = base_dir.join(rel_path);
        let data = std::fs::read(&full_path)
            .map_err(|e| IdentityError::Io { path: full_path.clone(), message: e.to_string() })?;
        check_identity(rel_path, &data, registry, dialect, parse_options, print_options)?;
        checked += 1;
        checked_paths.push(rel_path.to_owned());
    }

    Ok(CorpusIdentityReport { checked, skipped, checked_paths })
}
