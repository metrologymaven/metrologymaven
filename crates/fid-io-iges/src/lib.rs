//! Bounded parser and deterministic writer for the fixed-column ASCII form of IGES (spec
//! node `IO.CAD.IGES.SECT`), IGES 5.3 assumed [unverified: edition].
//!
//! The parser reads the five sections (Start, Global, Directory Entry, Parameter Data,
//! Terminate) of 80-column lines and yields the Global parameters, the directory and
//! each entry's parameter list. It checks every cross-reference between sections before
//! anything is interpreted:
//!
//! 1. every line is exactly 80 printable-ASCII columns (plus an optional `\n`, `\r\n` or
//!    `\r` ending), sections appear in order and their sequence numbers (columns 74-80)
//!    run 1, 2, 3, ...; the compressed ASCII and binary forms are rejected with
//!    `iges_form_unsupported`;
//! 2. the Terminate counts equal the section lengths (`truncated_file{section}`,
//!    REQ-IO.CAD.IGES.SECT-004);
//! 3. the Global section, with its declared parameter and record delimiters;
//! 4. every two-line directory entry, whose parameter pointer and line count must lie
//!    inside the Parameter Data section (`malformed{de}`, REQ-IO.CAD.IGES.SECT-001);
//! 5. every parameter record, whose lines must all carry the entry's sequence number as
//!    back-pointer (`malformed{de, back_pointer}`, REQ-IO.CAD.IGES.SECT-002) and whose
//!    first parameter must repeat the entity type;
//! 6. the unit, from the Global unit flag and name or the caller ([`resolve_units`],
//!    REQ-IO.CAD.IGES.SECT-003).
//!
//! Reals are correctly rounded, `D` exponents included, and keep their authored lexeme;
//! strings (Hollerith constants, Start lines, labels) are untrusted text
//! (REQ-IO.CAD.IGES.SECT-005). Every allocation is charged to the `IO.REG.LIMITS`
//! [`Budget`] before it happens. Nothing here panics on any input.
//!
//! The writer ([`write`]) emits the same structure in a fixed, deterministic layout; parse
//! → write → parse yields identical Global parameters, directory and parameter lists
//! ([`IgesFile::content_eq`], REQ-IO.CAD.IGES.SECT-006).
//!
//! What an entity means is out of scope (`IO.CAD.IGES.ENT`): the parser cannot tell a
//! pointer from an integer without the entity's schema, so it yields [`IgesParam::Int`]
//! for both; [`IgesParam::Ptr`] exists for callers that build files to write.

mod error;
mod lines;
mod parse;
mod real;
mod unit;
mod write;

pub use error::{Bound, IgesError, Section};
pub use unit::{declared_unit, resolve_units, IgesUnit, UnitResolution, UnitSource};
pub use write::write;

use fid_io_reg::{BoundedSource, Budget, LimitRequest, Principal, UntrustedText};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

/// Default limit on one integer or real token, in bytes.
pub const DEFAULT_MAX_TOKEN_BYTES: usize = 4096;

/// Parser options beyond the `IO.REG.LIMITS` budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// The caller's unit, used when the file declares none it can resolve.
    pub unit: Option<IgesUnit>,
    /// Maximum length of one integer or real token, in bytes.
    pub max_token_bytes: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions { unit: None, max_token_bytes: DEFAULT_MAX_TOKEN_BYTES }
    }
}

/// Text taken from the file: a Hollerith string, a Start line, a label or a reserved
/// directory field. Always printable ASCII (the parser rejects other bytes). It is file
/// content and therefore untrusted (CONSTITUTION C6): like [`UntrustedText`] it offers no
/// `&str` view, no `Display` and no conversion into `String`; use
/// [`IgesText::to_untrusted`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct IgesText(Box<str>);

impl IgesText {
    /// Wraps text, if it is printable ASCII (`' '..='~'`) only.
    pub fn new(text: &str) -> Option<IgesText> {
        if text.bytes().all(printable) {
            Some(IgesText(Box::from(text)))
        } else {
            None
        }
    }

    /// From bytes the line check has already proved printable ASCII.
    pub(crate) fn from_ascii(bytes: &[u8]) -> IgesText {
        let s: String = bytes.iter().map(|&b| if printable(b) { char::from(b) } else { '?' }).collect();
        IgesText(s.into_boxed_str())
    }

    /// The text as the harness's untrusted-text type.
    pub fn to_untrusted(&self) -> UntrustedText {
        UntrustedText::new(&self.0)
    }

    /// Length in bytes (equal to the length in characters).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True for the empty string.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The raw text, for the writer and the unit-name lookup only.
    pub(crate) fn raw(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for IgesText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_untrusted(), f)
    }
}

pub(crate) fn printable(b: u8) -> bool {
    (b' '..=b'~').contains(&b)
}

/// The authored spelling of a real parameter, e.g. `1.5D-3`. Always matches
/// `[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([EeDd][+-]?[0-9]+)?` with a decimal point or an
/// exponent, parses to a finite value, and is at most [`DEFAULT_MAX_TOKEN_BYTES`] long
/// when built with [`Lexeme::new`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lexeme(Box<str>);

impl Lexeme {
    /// The lexeme, if `s` is a well-formed real token with a finite value.
    pub fn new(s: &str) -> Option<Lexeme> {
        if s.len() <= DEFAULT_MAX_TOKEN_BYTES {
            Lexeme::checked(s.as_bytes())
        } else {
            None
        }
    }

    pub(crate) fn checked(b: &[u8]) -> Option<Lexeme> {
        if real::shape(b) == Some(real::Shape::Real) && real::value(b).is_some() {
            Some(Lexeme(Box::from(std::str::from_utf8(b).ok()?)))
        } else {
            None
        }
    }

    /// The shortest lexeme that parses back to exactly `v`, if `v` is finite.
    pub fn shortest(v: f64) -> Option<Lexeme> {
        if v.is_finite() {
            Lexeme::new(&real::shortest(v))
        } else {
            None
        }
    }

    /// The lexeme text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The correctly rounded value of the lexeme.
    pub fn value(&self) -> Option<f64> {
        real::value(self.0.as_bytes())
    }
}

/// One parameter of the Global section or of a parameter record.
#[derive(Debug, Clone)]
pub enum IgesParam {
    /// An integer (also every pointer the parser reads; see the crate documentation).
    Int(i64),
    /// A real, correctly rounded to `f64`, with its authored lexeme.
    Real(f64, Lexeme),
    /// A Hollerith string (untrusted, CONSTITUTION C6).
    Str(IgesText),
    /// A pointer to a directory entry (negative for some entity-defined meanings); built by
    /// callers, written as an integer, read back as [`IgesParam::Int`].
    Ptr(i32),
    /// A defaulted (empty) parameter.
    Default,
}

impl IgesParam {
    /// A real from its value alone, spelled with the shortest round-trip lexeme.
    pub fn real(v: f64) -> Option<IgesParam> {
        Lexeme::shortest(v).map(|l| IgesParam::Real(v, l))
    }

    /// Structural equality. Reals compare by bit pattern and lexeme; an integer equals a
    /// pointer with the same value (they are written identically).
    pub fn content_eq(&self, other: &IgesParam) -> bool {
        match (self, other) {
            (IgesParam::Int(a), IgesParam::Int(b)) => a == b,
            (IgesParam::Ptr(a), IgesParam::Ptr(b)) => a == b,
            (IgesParam::Int(a), IgesParam::Ptr(b)) | (IgesParam::Ptr(b), IgesParam::Int(a)) => *a == i64::from(*b),
            (IgesParam::Real(x, lx), IgesParam::Real(y, ly)) => x.to_bits() == y.to_bits() && lx == ly,
            (IgesParam::Str(a), IgesParam::Str(b)) => a == b,
            (IgesParam::Default, IgesParam::Default) => true,
            _ => false,
        }
    }

    /// The integer value of an `Int` or `Ptr`.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            IgesParam::Int(v) => Some(*v),
            IgesParam::Ptr(v) => Some(i64::from(*v)),
            _ => None,
        }
    }

    /// The value as a pointer (an `Int` or `Ptr` that fits in `i32`).
    pub fn as_ptr(&self) -> Option<i32> {
        self.as_int().and_then(|v| i32::try_from(v).ok())
    }

    /// The value of a `Real`, or of an `Int` (exactly when it is at most 2^53 in size).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            IgesParam::Real(v, _) => Some(*v),
            IgesParam::Int(v) if v.unsigned_abs() <= 1 << 53 => Some(*v as f64),
            _ => None,
        }
    }
}

impl PartialEq for IgesParam {
    /// [`IgesParam::content_eq`].
    fn eq(&self, other: &IgesParam) -> bool {
        self.content_eq(other)
    }
}

fn params_eq(a: &[IgesParam], b: &[IgesParam]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.content_eq(y))
}

/// The Global section: its two delimiters and parameters 3 onwards.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalParams {
    /// Parameter 1, the parameter delimiter (`,` by default).
    pub param_delim: u8,
    /// Parameter 2, the record delimiter (`;` by default).
    pub record_delim: u8,
    /// Parameters 3, 4, ... in order (`params[0]` is parameter 3).
    pub params: Vec<IgesParam>,
}

impl Default for GlobalParams {
    fn default() -> Self {
        GlobalParams { param_delim: b',', record_delim: b';', params: Vec::new() }
    }
}

impl GlobalParams {
    /// Global parameter `n` (1-based, as numbered in the standard), for `n >= 3`.
    pub fn param(&self, n: usize) -> Option<&IgesParam> {
        self.params.get(n.checked_sub(3)?)
    }

    fn text(&self, n: usize) -> Option<UntrustedText> {
        match self.param(n) {
            Some(IgesParam::Str(t)) => Some(t.to_untrusted()),
            _ => None,
        }
    }

    fn number(&self, n: usize) -> Option<f64> {
        self.param(n).and_then(IgesParam::as_f64)
    }

    /// Parameter 3, the product identification from the sending system (untrusted).
    pub fn sender_product_id(&self) -> Option<UntrustedText> {
        self.text(3)
    }

    /// Parameter 4, the file name (untrusted).
    pub fn file_name(&self) -> Option<UntrustedText> {
        self.text(4)
    }

    /// Parameter 5, the native system identification: the originating system (untrusted).
    pub fn native_system_id(&self) -> Option<UntrustedText> {
        self.text(5)
    }

    /// Parameter 6, the preprocessor version of the originating system (untrusted).
    pub fn preprocessor_version(&self) -> Option<UntrustedText> {
        self.text(6)
    }

    /// Parameter 13, the model-space scale.
    pub fn model_space_scale(&self) -> Option<f64> {
        self.number(13)
    }

    /// Parameter 14, the unit flag.
    pub fn unit_flag(&self) -> Option<i64> {
        self.param(14).and_then(IgesParam::as_int)
    }

    /// Parameter 15, the unit name (untrusted).
    pub fn unit_name(&self) -> Option<UntrustedText> {
        self.text(15)
    }

    /// Parameter 19, the minimum user-intended resolution, in file units.
    pub fn min_resolution(&self) -> Option<f64> {
        self.number(19)
    }

    /// Parameter 20, the approximate maximum coordinate value, in file units.
    pub fn max_coordinate(&self) -> Option<f64> {
        self.number(20)
    }

    /// Equal delimiters and parameters.
    pub fn content_eq(&self, other: &GlobalParams) -> bool {
        self.param_delim == other.param_delim
            && self.record_delim == other.record_delim
            && params_eq(&self.params, &other.params)
    }
}

/// The status number of a directory entry (field 9): four two-digit numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Status {
    /// Blank status (digits 1-2).
    pub blank: u8,
    /// Subordinate entity switch (digits 3-4).
    pub subordinate: u8,
    /// Entity use flag (digits 5-6).
    pub entity_use: u8,
    /// Hierarchy (digits 7-8).
    pub hierarchy: u8,
}

/// One directory entry (two 80-column lines of 8-column fields). Integer fields that are
/// blank in the file read as 0.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirEntry {
    /// Field 1 (and 11): entity type number.
    pub entity_type: i32,
    /// Field 2: sequence number of the first parameter line.
    pub param_ptr: u32,
    /// Field 3: structure.
    pub structure: i32,
    /// Field 4: line font pattern.
    pub line_font: i32,
    /// Field 5: level.
    pub level: i32,
    /// Field 6: view.
    pub view: i32,
    /// Field 7: transformation matrix pointer.
    pub transform: i32,
    /// Field 8: label display associativity.
    pub label_display: i32,
    /// Field 9: status number.
    pub status: Status,
    /// Field 12: line weight number.
    pub line_weight: i32,
    /// Field 13: color number.
    pub color: i32,
    /// Field 14: parameter line count.
    pub param_line_count: u32,
    /// Field 15: form number.
    pub form: i32,
    /// Field 16: reserved (untrusted text, spaces trimmed).
    pub reserved1: IgesText,
    /// Field 17: reserved (untrusted text, spaces trimmed).
    pub reserved2: IgesText,
    /// Field 18: entity label (untrusted text, spaces trimmed).
    pub label: IgesText,
    /// Field 19: entity subscript number.
    pub subscript: i32,
}

impl DirEntry {
    /// Equality ignoring the layout fields `param_ptr` and `param_line_count`, which the
    /// writer recomputes.
    pub fn content_eq(&self, other: &DirEntry) -> bool {
        DirEntry { param_ptr: 0, param_line_count: 0, ..self.clone() }
            == DirEntry { param_ptr: 0, param_line_count: 0, ..other.clone() }
    }
}

/// A parsed IGES file.
#[derive(Debug, Clone, PartialEq)]
pub struct IgesFile {
    /// The Start section lines, trailing spaces removed (untrusted).
    pub start: Vec<IgesText>,
    /// The Global section.
    pub global: GlobalParams,
    /// The directory entries by sequence number (1, 3, 5, ...).
    pub directory: BTreeMap<u32, DirEntry>,
    /// Each entry's parameters (after the leading entity type number), by the entry's
    /// sequence number.
    pub params: BTreeMap<u32, Vec<IgesParam>>,
    /// The effective length unit.
    pub units: UnitResolution,
}

impl IgesFile {
    /// The Start lines as untrusted text.
    pub fn start_untrusted(&self) -> Vec<UntrustedText> {
        self.start.iter().map(IgesText::to_untrusted).collect()
    }

    /// Equal Start lines, Global parameters, directory (ignoring the layout fields, see
    /// [`DirEntry::content_eq`]), parameter lists and unit: the identity of
    /// REQ-IO.CAD.IGES.SECT-006.
    pub fn content_eq(&self, other: &IgesFile) -> bool {
        self.start == other.start
            && self.global.content_eq(&other.global)
            && self.directory.len() == other.directory.len()
            && self.directory.iter().zip(other.directory.iter()).all(|((k, a), (j, b))| k == j && a.content_eq(b))
            && self.params.len() == other.params.len()
            && self.params.iter().zip(other.params.iter()).all(|((k, a), (j, b))| k == j && params_eq(a, b))
            && self.units == other.units
    }
}

/// Parses `input` with the default limits for a script principal and default options.
pub fn parse(input: &[u8]) -> Result<IgesFile, IgesError> {
    parse_opts(input, &ParseOptions::default())
}

/// Parses `input` with the default limits for a script principal.
pub fn parse_opts(input: &[u8], opts: &ParseOptions) -> Result<IgesFile, IgesError> {
    let mut budget = Budget::new(Principal::Script, &LimitRequest::default())?;
    parse_with(input, &mut budget, opts)
}

/// Parses `input`, charging its length and everything produced to `budget`.
pub fn parse_with(input: &[u8], budget: &mut Budget, opts: &ParseOptions) -> Result<IgesFile, IgesError> {
    let len = u64::try_from(input.len()).unwrap_or(u64::MAX);
    budget.charge_bytes(len)?;
    parse::file(input, budget, opts)
}

/// Reads the whole of `source` (every byte charged to its budget) and parses it.
pub fn parse_source<R: Read>(mut source: BoundedSource<R>, opts: &ParseOptions) -> Result<IgesFile, IgesError> {
    let mut buf = Vec::new();
    source.read_to_end_bounded(&mut buf)?;
    let (_, mut budget) = source.into_parts();
    parse::file(&buf, &mut budget, opts)
}
