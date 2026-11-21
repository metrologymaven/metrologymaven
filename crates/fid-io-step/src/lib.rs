//! Bounded ISO 10303-21 (clear-text STEP) exchange-structure parser and printer (spec
//! node `IO.CAD.STEP.P21`).
//!
//! The parser is a single pass over the input bytes: a byte lexer and a parser whose
//! parameter nesting is handled with an explicit stack (no recursion), bounded by the
//! `IO.REG.LIMITS` [`Budget`] (file size, elements, nesting depth, allocation, wall time)
//! and by [`ParseOptions`] (instance count, string length, binary length, token length).
//! It yields the header records and a map from entity number to [`Instance`]. References
//! are not resolved while parsing; [`P21File::resolve`] resolves them lazily, and
//! references to undefined entity numbers are listed in [`ParseReport::dangling`].
//!
//! The printer ([`print`]) writes the same structure back deterministically: instances
//! in ascending entity number, one per line, strings re-encoded with `\X2\`/`\X4\`
//! escapes, reals in their authored lexeme when they carry one and in the shortest
//! round-trip form otherwise.
//!
//! What any entity means is out of scope (other `IO.CAD.STEP` leaves). Edition-3 anchor,
//! reference and signature sections are rejected with `step_feature_unsupported`.
//! Every outcome is a returned `Result`; nothing here panics on any input.
//!
//! The [`header`] module (spec node `IO.CAD.STEP.HDR`) reads the header of a parsed file:
//! schema acceptance, `FILE_NAME`/`FILE_DESCRIPTION` as untrusted text, and the length
//! unit, plane-angle unit and uncertainty of every geometric representation context.

mod error;
pub mod header;
mod parse;
mod print;
mod real;
mod text;

pub use error::{Bound, StepError};
pub use header::{
    resolve_header, AngleUnit, HeaderError, HeaderOptions, HeaderWarning, StepHeaderInfo, StepLengthUnit, StepSchema,
    Uncertainty, UnitContext, WarningDetail,
};
pub use print::{print, print_with, PrintOptions, RealStyle};

use fid_io_reg::{BoundedSource, Budget, LimitRequest, Principal, UntrustedText};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::sync::Arc;

/// Default instance budget (REQ-IO.CAD.STEP.P21-004).
pub const DEFAULT_MAX_INSTANCES: u64 = 50_000_000;
/// Default list and typed-parameter nesting limit (REQ-IO.CAD.STEP.P21-004).
pub const DEFAULT_MAX_DEPTH: u32 = 64;
/// Default limit on the decoded length of one string, in UTF-8 bytes (1 MiB).
pub const DEFAULT_MAX_STRING_BYTES: usize = 1 << 20;
/// Default limit on the hex digits of one binary parameter.
pub const DEFAULT_MAX_BINARY_DIGITS: usize = 1 << 21;
/// Default limit on one keyword, number or entity-number token, in bytes.
pub const DEFAULT_MAX_TOKEN_BYTES: usize = 4096;

/// Parser bounds beyond the `IO.REG.LIMITS` budget. The effective nesting limit is the
/// smaller of `max_depth` and the budget's depth limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Maximum number of instances (header records plus data instances).
    pub max_instances: u64,
    /// Maximum nesting of list and typed parameters inside one record.
    pub max_depth: u32,
    /// Maximum decoded length of one string, in UTF-8 bytes.
    pub max_string_bytes: usize,
    /// Maximum hex digits of one binary parameter.
    pub max_binary_digits: usize,
    /// Maximum length of one keyword, number or entity-number token, in bytes.
    pub max_token_bytes: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            max_instances: DEFAULT_MAX_INSTANCES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_string_bytes: DEFAULT_MAX_STRING_BYTES,
            max_binary_digits: DEFAULT_MAX_BINARY_DIGITS,
            max_token_bytes: DEFAULT_MAX_TOKEN_BYTES,
        }
    }
}

/// An upper-case identifier: an entity name, a typed-parameter type name or an
/// enumeration value. Matches `!?[A-Z_][A-Z0-9_]*` (the `!` marks a user-defined name).
/// Names read from a file are upper-cased and interned, so equal names share storage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ident(Arc<str>);

impl Ident {
    /// The identifier, if `s` is a valid upper-case identifier.
    pub fn new(s: &str) -> Option<Ident> {
        if valid_ident(s) {
            Some(Ident(Arc::from(s)))
        } else {
            None
        }
    }

    /// The identifier text (always ASCII `!`, `A-Z`, `0-9`, `_`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn valid_ident(s: &str) -> bool {
    let b = s.as_bytes();
    let body = b.strip_prefix(b"!").unwrap_or(b);
    match body.split_first() {
        Some((first, rest)) => {
            (first.is_ascii_uppercase() || *first == b'_')
                && rest.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
        }
        None => false,
    }
}

/// The authored spelling of a real parameter, e.g. `1.0E+02`. Always matches
/// `[+-]?[0-9]+\.[0-9]*([Ee][+-]?[0-9]+)?`. A parsed lexeme is at most
/// [`ParseOptions::max_token_bytes`] long; [`Lexeme::new`] accepts at most
/// [`DEFAULT_MAX_TOKEN_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lexeme(Box<str>);

impl Lexeme {
    /// The lexeme, if `s` is a well-formed real token.
    pub fn new(s: &str) -> Option<Lexeme> {
        if s.len() <= DEFAULT_MAX_TOKEN_BYTES && real::lexeme_ok(s) {
            Some(Lexeme(Box::from(s)))
        } else {
            None
        }
    }

    /// The lexeme text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The text of a string parameter, decoded to Unicode. It is file content and therefore
/// untrusted (CONSTITUTION C6): like [`UntrustedText`] it offers no `&str` view, no
/// `Display` and no conversion into `String`; use [`StepText::to_untrusted`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StepText(Box<str>);

impl StepText {
    /// Wraps text (for building files to print, and for comparisons in tests).
    pub fn new(text: &str) -> StepText {
        StepText(Box::from(text))
    }

    /// The text as the harness's untrusted-text type.
    pub fn to_untrusted(&self) -> UntrustedText {
        UntrustedText::new(&self.0)
    }

    /// Length in UTF-8 bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True for the empty string.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The raw text, for the printer only.
    pub(crate) fn raw(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for StepText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_untrusted(), f)
    }
}

/// A binary parameter: a bit string whose first hex digit gives the number (0 to 3) of
/// unused leading bits in the following hex digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BitVec {
    unused: u8,
    hex: Box<[u8]>,
}

impl BitVec {
    /// A bit string from its unused-bit count and hex digit values (each 0 to 15). The
    /// count is at most 3, and 0 when there are no digits.
    pub fn new(unused: u8, hex: Vec<u8>) -> Option<BitVec> {
        let ok = unused <= 3 && hex.iter().all(|&d| d < 16) && (unused == 0 || !hex.is_empty());
        if ok {
            Some(BitVec { unused, hex: hex.into_boxed_slice() })
        } else {
            None
        }
    }

    /// Number of bits.
    pub fn len(&self) -> usize {
        self.hex.len().saturating_mul(4).saturating_sub(usize::from(self.unused))
    }

    /// True if there are no bits.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bit `i` (0 is the first, most significant used bit), if `i < len()`.
    pub fn get(&self, i: usize) -> Option<bool> {
        if i >= self.len() {
            return None;
        }
        let j = i.checked_add(usize::from(self.unused))?;
        let digit = *self.hex.get(j / 4)?;
        Some((digit >> (3 - (j % 4))) & 1 == 1)
    }

    /// The number of unused leading bits.
    pub fn unused_bits(&self) -> u8 {
        self.unused
    }

    /// The hex digit values.
    pub fn hex_digits(&self) -> &[u8] {
        &self.hex
    }
}

/// One parameter of a record.
#[derive(Debug, Clone)]
pub enum Param {
    /// An integer.
    Int(i64),
    /// A real, correctly rounded to f64, with its authored lexeme when it came from a
    /// file (or when a caller supplies one to print).
    Real(f64, Option<Lexeme>),
    /// A string, decoded to Unicode (untrusted, CONSTITUTION C6).
    Str(StepText),
    /// An enumeration value such as `.T.`.
    Enum(Ident),
    /// A binary.
    Bin(BitVec),
    /// An entity reference `#n`, unresolved.
    Ref(u64),
    /// A typed parameter `NAME(p)`.
    Typed(Ident, Box<Param>),
    /// A list `(a,b,...)`.
    List(Vec<Param>),
    /// Unset `$`.
    Unset,
    /// Derived `*`.
    Derived,
}

impl Param {
    /// Structural equality. Reals compare by bit pattern; their lexemes are compared only
    /// when `lexemes` is true. Iterative, so any nesting depth is safe.
    pub fn content_eq(&self, other: &Param, lexemes: bool) -> bool {
        let mut stack: Vec<(&Param, &Param)> = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            let same = match (a, b) {
                (Param::Int(x), Param::Int(y)) => x == y,
                (Param::Real(x, lx), Param::Real(y, ly)) => x.to_bits() == y.to_bits() && (!lexemes || lx == ly),
                (Param::Str(x), Param::Str(y)) => x == y,
                (Param::Enum(x), Param::Enum(y)) => x == y,
                (Param::Bin(x), Param::Bin(y)) => x == y,
                (Param::Ref(x), Param::Ref(y)) => x == y,
                (Param::Typed(n, x), Param::Typed(m, y)) => {
                    stack.push((&**x, &**y));
                    n == m
                }
                (Param::List(x), Param::List(y)) => {
                    stack.extend(x.iter().zip(y.iter()));
                    x.len() == y.len()
                }
                (Param::Unset, Param::Unset) | (Param::Derived, Param::Derived) => true,
                _ => false,
            };
            if !same {
                return false;
            }
        }
        true
    }
}

impl PartialEq for Param {
    /// [`Param::content_eq`] with lexemes compared.
    fn eq(&self, other: &Param) -> bool {
        self.content_eq(other, true)
    }
}

fn params_eq(a: &[Param], b: &[Param], lexemes: bool) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.content_eq(y, lexemes))
}

/// One instance: a header record (number 0) or a data instance `#n=...`. A simple
/// instance has one part; a complex instance lists its parts in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The entity number (0 for header records).
    pub number: u64,
    /// `(entity name, parameters)` of each part, in file order.
    pub parts: Vec<(Ident, Vec<Param>)>,
    /// Byte offset of the instance (its `#`, or the header record's name) in the input.
    pub offset: u64,
}

impl Instance {
    /// The entity names, in order.
    pub fn names(&self) -> impl Iterator<Item = &Ident> {
        self.parts.iter().map(|(n, _)| n)
    }

    /// Equality ignoring the byte offset; see [`Param::content_eq`] for `lexemes`.
    pub fn content_eq(&self, other: &Instance, lexemes: bool) -> bool {
        self.number == other.number
            && self.parts.len() == other.parts.len()
            && self.parts.iter().zip(other.parts.iter()).all(|((n, p), (m, q))| n == m && params_eq(p, q, lexemes))
    }
}

/// A reference from instance `from` to the undefined entity number `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DanglingRef {
    /// The referring instance.
    pub from: u64,
    /// The undefined entity number.
    pub to: u64,
}

/// What the parser noticed besides the content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParseReport {
    /// References to undefined entity numbers, sorted and without repeats.
    pub dangling: Vec<DanglingRef>,
    /// Instances read (header records plus data instances).
    pub instances: u64,
    /// Deepest list or typed-parameter nesting reached.
    pub max_depth: u32,
}

/// A parsed exchange structure.
#[derive(Debug, Clone, PartialEq)]
pub struct P21File {
    /// The header records, in file order (number 0 each).
    pub header: Vec<Instance>,
    /// The data instances by entity number (all data sections merged).
    pub data: BTreeMap<u64, Instance>,
    /// The parse report.
    pub report: ParseReport,
    /// Every entity, type and enumeration name seen, sorted (the intern table).
    pub names: Vec<Ident>,
}

impl P21File {
    /// The instance with entity number `n`, if defined.
    pub fn get(&self, n: u64) -> Option<&Instance> {
        self.data.get(&n)
    }

    /// Resolves the reference from instance `from` to entity number `to`; fails with
    /// `dangling_reference` naming both numbers if `to` is not defined.
    pub fn resolve(&self, from: u64, to: u64) -> Result<&Instance, StepError> {
        self.data.get(&to).ok_or(StepError::DanglingReference { from, to })
    }

    /// Equal header and instance map, ignoring byte offsets, the report and the intern
    /// table; see [`Param::content_eq`] for `lexemes`.
    pub fn content_eq(&self, other: &P21File, lexemes: bool) -> bool {
        self.header.len() == other.header.len()
            && self.header.iter().zip(other.header.iter()).all(|(a, b)| a.content_eq(b, lexemes))
            && self.data.len() == other.data.len()
            && self.data.iter().zip(other.data.iter()).all(|((k, a), (j, b))| k == j && a.content_eq(b, lexemes))
    }
}

/// Parses `input` with the default limits for a script principal and default options.
pub fn parse(input: &[u8]) -> Result<P21File, StepError> {
    let mut budget = Budget::new(Principal::Script, &LimitRequest::default())?;
    parse_with(input, &mut budget, &ParseOptions::default())
}

/// Parses `input`, charging its length and everything produced to `budget`.
pub fn parse_with(input: &[u8], budget: &mut Budget, opts: &ParseOptions) -> Result<P21File, StepError> {
    let len = u64::try_from(input.len()).unwrap_or(u64::MAX);
    budget.charge_bytes(len)?;
    parse::Parser::new(input, budget, opts).file()
}

/// Reads the whole of `source` (every byte charged to its budget) and parses it.
pub fn parse_source<R: Read>(mut source: BoundedSource<R>, opts: &ParseOptions) -> Result<P21File, StepError> {
    let mut buf = Vec::new();
    source.read_to_end_bounded(&mut buf)?;
    let (_, mut budget) = source.into_parts();
    parse::Parser::new(&buf, &mut budget, opts).file()
}
