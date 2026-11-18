//! Verifier-side RFC 8785 (JCS) canonicaliser and journal entry hash. Spec node
//! `TRUST.CHAIN.CANON`.
//!
//! This crate is a second, independent implementation of the rules that CORE's
//! journal writer (`fid-canon`) implements. It shares no code with it
//! (REQ-TRUST.CHAIN-007, REQ-TRUST.CHAIN.CANON-004); the two are compared on the
//! same golden vectors so that a bug in one is not silently agreed with.
//!
//! - [`parse`]: a bounded JSON parser producing an ordered [`Value`] tree. It
//!   rejects duplicate member names, lone surrogates, numbers outside the
//!   IEEE 754 double range, nesting deeper than [`MAX_DEPTH`] and inputs larger
//!   than [`MAX_INPUT_BYTES`], all with `canon.invalid`.
//! - [`serialize`]: RFC 8785 output: members sorted by UTF-16 code units,
//!   ECMAScript Number::toString formatting ([`format_number`]), minimal escaping.
//! - [`entry_hash`]: SHA-256 over the canonical entry header with the top-level
//!   `hash` member omitted, after checking the journal's numeric conventions
//!   (`canon.convention`).
//!
//! The number formatter derives the shortest round-trip digits by searching
//! correctly rounded `{:.Ne}` renderings for the fewest digits that parse back to
//! the same double; it does not use Rust's shortest-digit `{:e}`/`{}` output, so
//! it does not share that path with other implementations.

mod entry;
mod number;
mod parse;
mod write;

use std::fmt;

pub use entry::{entry_hash, entry_hash_value, ConventionRule, MAX_SAFE_INTEGER, U64_FIELDS};
pub use number::format_number;
pub use parse::{parse, parse_with_limits, Limits};
pub use write::{canonicalize, digest_canonical, serialize};

/// Largest accepted input, in bytes (REQ-TRUST.CHAIN.CANON-002).
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Deepest accepted nesting of arrays and objects; `[]` has depth 1 (REQ-TRUST.CHAIN.CANON-002).
pub const MAX_DEPTH: usize = 128;

/// An ordered JSON value. Objects keep their members in input order; the
/// serialiser sorts them.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// A finite double.
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

/// Why an input is not acceptable JSON for canonicalisation (`canon.invalid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKind {
    /// Not JSON text (RFC 8259 grammar), including a byte-order mark or trailing data.
    Syntax,
    /// Not well-formed UTF-8.
    Utf8,
    /// Two members of one object have the same name.
    DuplicateKey,
    /// A `\u` escape encodes an unpaired UTF-16 surrogate.
    LoneSurrogate,
    /// A number outside the finite IEEE 754 double range (or a non-finite [`Value::Number`]).
    NumberRange,
    /// Nesting deeper than the depth limit.
    TooDeep,
    /// Input larger than the size limit.
    TooLarge,
}

impl InvalidKind {
    /// Stable detail name.
    pub fn name(self) -> &'static str {
        match self {
            InvalidKind::Syntax => "syntax",
            InvalidKind::Utf8 => "utf8",
            InvalidKind::DuplicateKey => "duplicate_key",
            InvalidKind::LoneSurrogate => "lone_surrogate",
            InvalidKind::NumberRange => "number_range",
            InvalidKind::TooDeep => "too_deep",
            InvalidKind::TooLarge => "too_large",
        }
    }
}

/// Typed errors. [`CanonError::code`] gives the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonError {
    /// `canon.invalid`. `offset` is the byte offset in the input where the problem
    /// was found (0 for errors raised while serialising a constructed [`Value`]).
    Invalid { kind: InvalidKind, offset: usize },
    /// `canon.convention`: a header field breaks the journal's numeric conventions.
    /// `path` is a JSON Pointer (RFC 6901) to the field; `""` is the header itself.
    Convention { path: String, rule: ConventionRule },
}

impl CanonError {
    /// Stable machine-readable code: `canon.invalid` or `canon.convention`.
    pub fn code(&self) -> &'static str {
        match self {
            CanonError::Invalid { .. } => "canon.invalid",
            CanonError::Convention { .. } => "canon.convention",
        }
    }

    pub(crate) fn invalid(kind: InvalidKind, offset: usize) -> CanonError {
        CanonError::Invalid { kind, offset }
    }
}

impl fmt::Display for CanonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonError::Invalid { kind, offset } => write!(f, "canon.invalid: {} at byte {offset}", kind.name()),
            CanonError::Convention { path, rule } => write!(f, "canon.convention: {} at {path:?}", rule.name()),
        }
    }
}

impl std::error::Error for CanonError {}
