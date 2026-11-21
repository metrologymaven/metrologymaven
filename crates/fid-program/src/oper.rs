//! Operator-interaction descriptors and their pure validation. Spec node `AUTO.PROG.OPER`.
//!
//! A descriptor (`operator-v1`) shows a prompt to an operator and asks for typed answers in
//! an ordered list of fields. Program operator steps (the `descriptor` object of an
//! `AUTO.PROG.FMT` `operator` step), script dialogs (`AUTO.PY.DLG`) and operator playback
//! (`AUTO.RUN.PLAY`) all use it. This module decodes and checks descriptors
//! ([`OperatorDescriptor::from_object`]) and validates answers ([`validate`]); rendering and
//! journaling belong to the caller. A descriptor with no fields is display only (a comment
//! step shown to the operator): its only valid answer is the empty object.
//!
//! Descriptors and answers are untrusted input: every size is bounded, unknown keys are
//! refused, and nothing here panics. Numeric bounds are compared as exact decimals on the
//! authored text ([`Decimal`]); no binary floating point is involved (REQ-003). Text answers
//! come back as [`UntrustedText`] (REQ-005, CONSTITUTION C6). Nothing here carries
//! authority: an answer is an input of a run, journaled by whoever runs the step.
//!
//! Wire forms (every choice is pinned in `spec/auto/prog/oper/NODE.md`, Design):
//!
//! ```text
//! descriptor: {"fields": [<field>, ...], "format": "operator-v1", "image"?: "root://<id>/<path>",
//!              "prompt": "<text>"}
//! field:      {"default"?: <answer value>, "label"?: "<text>", "name": "<ident>",
//!              "quantity_kind"?: "<kind>", "response": "<type>", "rules"?: <rules>, "unit"?: "<symbol>"}
//! rules:      {"choices"?: ["<text>", ...], "max"?: "<decimal>", "max_length"?: <count>,
//!              "min"?: "<decimal>", "required"?: <bool>}
//! answer:     {"<field name>": <answer value>, ...}
//! values:     ack true | bool true/false | integer "-12" | decimal "10.10"
//!             | quantity {"unit": "<symbol>", "value": "<decimal>"} | text "..." | choice "A"
//!             | element "<element reference>"
//! ```

use crate::fmt::Opaque;
use fid_canon::{parse_with_limits, CValue};
use fid_registry::QuantityCatalogue;
use fid_types::text::is_forbidden_char;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The `format` value of a descriptor.
pub const OPERATOR_FORMAT: &str = "operator-v1";
/// Most fields in one descriptor.
pub const MAX_FIELDS: usize = 64;
/// Longest field name, in bytes (the `AUTO.PROG.FMT` identifier pattern).
pub const MAX_NAME: usize = 64;
/// Longest prompt, in bytes.
pub const MAX_PROMPT: usize = 8192;
/// Longest field label or choice, in bytes.
pub const MAX_LABEL: usize = 256;
/// Most choices of one `choice` field.
pub const MAX_CHOICES: usize = 64;
/// Longest image handle or element reference, in bytes.
pub const MAX_HANDLE: usize = 4096;
/// Longest decimal or integer text, in bytes.
pub const MAX_DECIMAL_TEXT: usize = 64;
/// Most significant digits of a decimal (first non-zero digit to last digit written).
pub const MAX_SIG_DIGITS: usize = 40;
/// Longest text answer, in bytes, whatever `max_length` says; also the largest `max_length`.
pub const MAX_TEXT_ANSWER: usize = 65_536;
/// Most keys in one answer object.
pub const MAX_ANSWER_KEYS: usize = 256;
/// Largest descriptor or answer document accepted from JSON text, in bytes.
pub const MAX_DOC_BYTES: usize = 1024 * 1024;
/// Deepest nesting accepted from JSON text.
pub const MAX_DOC_DEPTH: usize = 8;

/// Keys of a descriptor, sorted.
pub const DESCRIPTOR_KEYS: [&str; 4] = ["fields", "format", "image", "prompt"];
/// Keys of a field, sorted.
pub const FIELD_KEYS: [&str; 7] = ["default", "label", "name", "quantity_kind", "response", "rules", "unit"];
/// Rule names, sorted.
pub const RULE_KEYS: [&str; 5] = ["choices", "max", "max_length", "min", "required"];

/// Pattern of a field name.
pub const NAME_PATTERN: &str = "^[A-Za-z_][A-Za-z0-9_]{0,63}$";
/// Pattern of a decimal text.
pub const DECIMAL_PATTERN: &str = "^[+-]?[0-9]+([.][0-9]+)?$";
/// Pattern of an integer text.
pub const INTEGER_PATTERN: &str = "^[+-]?[0-9]+$";
/// Pattern of an image handle (lexical form only; resolution is `IO.REG.ROOTS`).
pub const IMAGE_PATTERN: &str = "^root://[A-Za-z0-9][A-Za-z0-9_-]{0,63}(/[A-Za-z0-9._-]*[A-Za-z0-9_-])+$";

// ------------------------------------------------------------------ response types

/// The closed set of response types (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResponseType {
    Ack,
    Bool,
    Integer,
    Decimal,
    Quantity,
    Text,
    Choice,
    Element,
}

impl ResponseType {
    /// Every type, in the order of the spec.
    pub const ALL: [ResponseType; 8] = [
        ResponseType::Ack,
        ResponseType::Bool,
        ResponseType::Integer,
        ResponseType::Decimal,
        ResponseType::Quantity,
        ResponseType::Text,
        ResponseType::Choice,
        ResponseType::Element,
    ];

    /// The type with this exact wire name, or `None`.
    pub fn parse(name: &str) -> Option<ResponseType> {
        ResponseType::ALL.into_iter().find(|t| t.as_str() == name)
    }

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseType::Ack => "ack",
            ResponseType::Bool => "bool",
            ResponseType::Integer => "integer",
            ResponseType::Decimal => "decimal",
            ResponseType::Quantity => "quantity",
            ResponseType::Text => "text",
            ResponseType::Choice => "choice",
            ResponseType::Element => "element",
        }
    }

    /// The rules this type allows, sorted. Any other rule is `rule_not_allowed`.
    pub fn allowed_rules(self) -> &'static [&'static str] {
        match self {
            ResponseType::Ack | ResponseType::Bool | ResponseType::Element => &["required"],
            ResponseType::Integer | ResponseType::Decimal | ResponseType::Quantity => &["max", "min", "required"],
            ResponseType::Text => &["max_length", "required"],
            ResponseType::Choice => &["choices", "required"],
        }
    }

    /// Whether a field of this type may declare a default (all but `ack`).
    pub fn allows_default(self) -> bool {
        self != ResponseType::Ack
    }
}

impl fmt::Display for ResponseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ------------------------------------------------------------------ exact decimals

/// An authored decimal `[+-] digits [ "." digits ]`, kept as written and compared exactly
/// by value: sign, integer digits without leading zeros, fraction digits without trailing
/// zeros (REQ-003). `10.10 == 10.1` and `-0 == 0`. No exponent is accepted.
#[derive(Debug, Clone)]
pub struct Decimal {
    authored: String,
    neg: bool,
    int: String,
    frac: String,
}

impl Decimal {
    /// Parses a decimal text of at most [`MAX_DECIMAL_TEXT`] bytes and [`MAX_SIG_DIGITS`]
    /// significant digits; `None` otherwise.
    pub fn parse(text: &str) -> Option<Decimal> {
        if text.is_empty() || text.len() > MAX_DECIMAL_TEXT {
            return None;
        }
        let (neg, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (int, frac) = match body.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (body, None),
        };
        let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
        if !all_digits(int) || frac.is_some_and(|f| !all_digits(f)) {
            return None;
        }
        let frac = frac.unwrap_or_default();
        if int.bytes().chain(frac.bytes()).skip_while(|c| *c == b'0').count() > MAX_SIG_DIGITS {
            return None;
        }
        let int = int.trim_start_matches('0');
        let frac = frac.trim_end_matches('0');
        let neg = neg && !(int.is_empty() && frac.is_empty());
        Some(Decimal { authored: text.to_string(), neg, int: int.to_string(), frac: frac.to_string() })
    }

    /// Parses an integer text `[+-] digits` (no fraction part, so `10.0` is not one).
    pub fn parse_integer(text: &str) -> Option<Decimal> {
        if text.contains('.') {
            return None;
        }
        Decimal::parse(text)
    }

    /// The text exactly as authored.
    pub fn authored(&self) -> &str {
        &self.authored
    }

    /// Whether the authored text has no fraction part.
    pub fn is_integer_text(&self) -> bool {
        !self.authored.contains('.')
    }

    fn cmp_magnitude(&self, other: &Decimal) -> Ordering {
        self.int
            .len()
            .cmp(&other.int.len())
            .then_with(|| self.int.as_bytes().cmp(other.int.as_bytes()))
            .then_with(|| self.frac.as_bytes().cmp(other.frac.as_bytes()))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Decimal) -> Ordering {
        match (self.neg, other.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.cmp_magnitude(other),
            (true, true) => other.cmp_magnitude(self),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Decimal) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Decimal) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Decimal {}

// ------------------------------------------------------------------ texts and values

/// Text written by the program or script author (a prompt or label), checked for forbidden
/// code points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredText(String);

impl AuthoredText {
    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Text an operator typed: data, never instructions (CONSTITUTION C6, REQ-005). It has no
/// `Display` and no `Deref`, so it cannot slip into a message or prompt by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedText(String);

impl UntrustedText {
    /// The raw text, to be handled as untrusted data.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }

    /// The raw text, to be handled as untrusted data.
    pub fn into_untrusted_string(self) -> String {
        self.0
    }
}

/// A checked answer value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerValue {
    /// The operator acknowledged.
    Ack,
    Bool(bool),
    Integer(Decimal),
    Decimal(Decimal),
    /// A quantity in the field's declared unit, kept as written; nothing is converted.
    Quantity {
        value: Decimal,
        unit: String,
    },
    /// Marked untrusted (REQ-005).
    Text(UntrustedText),
    /// One of the field's authored choices.
    Choice(String),
    /// An element reference, lexically checked only; resolving it is the caller's job.
    Element(String),
}

impl AnswerValue {
    /// The wire value. A text answer is written as a plain string: whoever journals it
    /// records it as untrusted data.
    pub fn to_value(&self) -> CValue {
        match self {
            AnswerValue::Ack => CValue::Bool(true),
            AnswerValue::Bool(b) => CValue::Bool(*b),
            AnswerValue::Integer(d) | AnswerValue::Decimal(d) => CValue::Str(d.authored().to_string()),
            AnswerValue::Quantity { value, unit } => {
                let mut m = BTreeMap::new();
                m.insert("unit".to_string(), CValue::Str(unit.clone()));
                m.insert("value".to_string(), CValue::Str(value.authored().to_string()));
                CValue::Obj(m)
            }
            AnswerValue::Text(t) => CValue::Str(t.as_untrusted_str().to_string()),
            AnswerValue::Choice(s) | AnswerValue::Element(s) => CValue::Str(s.clone()),
        }
    }
}

/// The answers of a valid response: field name to value. Fields left unanswered take their
/// default; an optional field without a default and without an answer is absent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidAnswer {
    pub fields: BTreeMap<String, AnswerValue>,
}

// ------------------------------------------------------------------ descriptor types

/// Validation rules of a field; only those its type allows are ever set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// Whether an answer must be given (defaults to `true`). A field with a default is
    /// never `missing`.
    pub required: bool,
    /// Inclusive lower bound.
    pub min: Option<Decimal>,
    /// Inclusive upper bound.
    pub max: Option<Decimal>,
    /// Most Unicode scalar values of a text answer.
    pub max_length: Option<usize>,
    /// The authored choices, in order.
    pub choices: Vec<String>,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { required: true, min: None, max: None, max_length: None, choices: Vec::new() }
    }
}

/// One field of a descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub label: Option<AuthoredText>,
    pub response: ResponseType,
    /// `(kind, unit)`, present exactly on a `quantity` field (REQ-002).
    pub quantity: Option<(String, String)>,
    pub rules: Rules,
    pub default: Option<AnswerValue>,
}

/// A checked `operator-v1` descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorDescriptor {
    pub prompt: AuthoredText,
    /// An image as a `root://` handle, lexically checked; resolving it is `IO.REG.ROOTS`.
    pub image: Option<String>,
    pub fields: Vec<Field>,
}

// ------------------------------------------------------------------ errors

/// Error codes. The first eight are the spec's; `unknown_field`, `bad_descriptor` and
/// `bad_answer` cover input the spec's list does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorCode {
    Missing,
    BelowMin,
    AboveMax,
    TooLong,
    WrongType,
    WrongUnit,
    NotAChoice,
    RuleNotAllowed,
    /// An answer names a field the descriptor does not have.
    UnknownField,
    /// A descriptor is malformed (shape, names, kinds, bounds, defaults).
    BadDescriptor,
    /// An answer document is not a bounded JSON object.
    BadAnswer,
}

impl ErrorCode {
    /// The stable wire code.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Missing => "missing",
            ErrorCode::BelowMin => "below_min",
            ErrorCode::AboveMax => "above_max",
            ErrorCode::TooLong => "too_long",
            ErrorCode::WrongType => "wrong_type",
            ErrorCode::WrongUnit => "wrong_unit",
            ErrorCode::NotAChoice => "not_a_choice",
            ErrorCode::RuleNotAllowed => "rule_not_allowed",
            ErrorCode::UnknownField => "unknown_field",
            ErrorCode::BadDescriptor => "bad_descriptor",
            ErrorCode::BadAnswer => "bad_answer",
        }
    }
}

/// One typed error. `field` is the field name (for descriptor errors, the declared name
/// when it is a string; for `unknown_field`, the answer key); `index` is the field's
/// position in the descriptor for descriptor errors. `detail` never echoes input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub code: ErrorCode,
    pub field: Option<String>,
    pub index: Option<usize>,
    pub detail: &'static str,
}

impl FieldError {
    /// The stable wire code.
    pub fn code(&self) -> &'static str {
        self.code.as_str()
    }

    fn top(code: ErrorCode, detail: &'static str) -> FieldError {
        FieldError { code, field: None, index: None, detail }
    }
}

/// Input text in messages: at most 64 characters, escaped.
fn shown(s: &str) -> String {
    let cut: String = s.chars().take(64).collect();
    if cut.len() < s.len() {
        format!("{cut:?}...")
    } else {
        format!("{cut:?}")
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())?;
        if let Some(i) = self.index {
            write!(f, ": fields/{i}")?;
        }
        if let Some(n) = &self.field {
            write!(f, ": {}", shown(n))?;
        }
        write!(f, ": {}", self.detail)
    }
}

impl std::error::Error for FieldError {}

// ------------------------------------------------------------------ lexical rules

fn is_name(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_NAME
        && b.next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Non-empty, at most `max` bytes, no forbidden code point (`\n` allowed when `newline`).
fn is_display_text(s: &str, max: usize, newline: bool) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(|c| is_forbidden_char(c) && !(newline && c == '\n'))
}

/// `root://<root-id>/<component>[/<component>...]`: root ID as `IO.REG.ROOTS` (1 to 64 of
/// `[A-Za-z0-9_-]`, starting with a letter or digit); components of `[A-Za-z0-9._-]`, not
/// ending in `.` (so never `.` or `..`). Stricter than `IO.REG.ROOTS`, which still resolves it.
fn is_image_handle(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("root://") else { return false };
    if s.len() > MAX_HANDLE {
        return false;
    }
    let mut parts = rest.split('/');
    let id_ok = parts.next().is_some_and(|id| {
        let mut b = id.bytes();
        id.len() <= 64
            && b.next().is_some_and(|c| c.is_ascii_alphanumeric())
            && b.all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    });
    let mut n = 0usize;
    let comps_ok = parts.all(|c| {
        n += 1;
        !c.is_empty()
            && !c.ends_with('.')
            && c.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    });
    id_ok && comps_ok && n > 0
}

/// A non-empty element reference of at most [`MAX_HANDLE`] bytes without forbidden code points.
fn is_element_ref(s: &str) -> bool {
    is_display_text(s, MAX_HANDLE, false)
}

// ------------------------------------------------------------------ value checks

type Refusal = (ErrorCode, &'static str);

fn bounds(d: &Decimal, rules: &Rules) -> Result<(), Refusal> {
    if rules.min.as_ref().is_some_and(|m| d < m) {
        return Err((ErrorCode::BelowMin, "below the minimum"));
    }
    if rules.max.as_ref().is_some_and(|m| d > m) {
        return Err((ErrorCode::AboveMax, "above the maximum"));
    }
    Ok(())
}

/// Checks one present answer value against a field: type first, then unit, then rules.
fn check_value(f: &Field, v: &CValue) -> Result<AnswerValue, Refusal> {
    const WRONG: Refusal = (ErrorCode::WrongType, "not a value of the field's response type");
    match (f.response, v) {
        (ResponseType::Ack, CValue::Bool(true)) => Ok(AnswerValue::Ack),
        (ResponseType::Bool, CValue::Bool(b)) => Ok(AnswerValue::Bool(*b)),
        (ResponseType::Integer, CValue::Str(s)) => {
            let d = Decimal::parse_integer(s).ok_or(WRONG)?;
            bounds(&d, &f.rules)?;
            Ok(AnswerValue::Integer(d))
        }
        (ResponseType::Decimal, CValue::Str(s)) => {
            let d = Decimal::parse(s).ok_or(WRONG)?;
            bounds(&d, &f.rules)?;
            Ok(AnswerValue::Decimal(d))
        }
        (ResponseType::Quantity, CValue::Obj(m)) => {
            if m.len() != 2 {
                return Err(WRONG);
            }
            let (Some(CValue::Str(value)), Some(CValue::Str(unit))) = (m.get("value"), m.get("unit")) else {
                return Err(WRONG);
            };
            let d = Decimal::parse(value).ok_or(WRONG)?;
            let declared = f.quantity.as_ref().map(|(_, u)| u.as_str());
            if declared != Some(unit.as_str()) {
                return Err((ErrorCode::WrongUnit, "not in the field's declared unit"));
            }
            bounds(&d, &f.rules)?;
            Ok(AnswerValue::Quantity { value: d, unit: unit.clone() })
        }
        (ResponseType::Text, CValue::Str(s)) => {
            if s.is_empty() && f.rules.required {
                return Err((ErrorCode::Missing, "a required text is empty"));
            }
            let limit = f.rules.max_length.unwrap_or(MAX_TEXT_ANSWER);
            if s.len() > MAX_TEXT_ANSWER || s.chars().count() > limit {
                return Err((ErrorCode::TooLong, "longer than the field allows"));
            }
            Ok(AnswerValue::Text(UntrustedText(s.clone())))
        }
        (ResponseType::Choice, CValue::Str(s)) => {
            if f.rules.choices.iter().any(|c| c == s) {
                Ok(AnswerValue::Choice(s.clone()))
            } else {
                Err((ErrorCode::NotAChoice, "not one of the field's choices"))
            }
        }
        (ResponseType::Element, CValue::Str(s)) if is_element_ref(s) => Ok(AnswerValue::Element(s.clone())),
        _ => Err(WRONG),
    }
}

/// Validates an answer against a checked descriptor (REQ-004): pure and deterministic.
///
/// For each field in descriptor order: a present value is checked by type, then unit, then
/// rules; an absent value takes the field's default, else is `missing` when the field is
/// required. Then each answer key in byte order that names no field is `unknown_field`.
/// Every failing field is reported, at most one error per field.
pub fn validate(descriptor: &OperatorDescriptor, answer: &CValue) -> Result<ValidAnswer, Vec<FieldError>> {
    let CValue::Obj(m) = answer else {
        return Err(vec![FieldError::top(ErrorCode::BadAnswer, "an answer must be a JSON object")]);
    };
    if m.len() > MAX_ANSWER_KEYS {
        return Err(vec![FieldError::top(ErrorCode::BadAnswer, "an answer has too many keys")]);
    }
    let mut out = ValidAnswer::default();
    let mut errors = Vec::new();
    for f in &descriptor.fields {
        let err = |(code, detail): Refusal| FieldError { code, field: Some(f.name.clone()), index: None, detail };
        match (m.get(&f.name), &f.default) {
            (Some(v), _) => match check_value(f, v) {
                Ok(a) => {
                    out.fields.insert(f.name.clone(), a);
                }
                Err(r) => errors.push(err(r)),
            },
            (None, Some(d)) => {
                out.fields.insert(f.name.clone(), d.clone());
            }
            (None, None) if f.rules.required => errors.push(err((ErrorCode::Missing, "no answer given"))),
            (None, None) => {}
        }
    }
    let names: BTreeSet<&str> = descriptor.fields.iter().map(|f| f.name.as_str()).collect();
    for k in m.keys() {
        if !names.contains(k.as_str()) {
            errors.push(FieldError {
                code: ErrorCode::UnknownField,
                field: Some(k.clone()),
                index: None,
                detail: "the descriptor has no such field",
            });
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// An answer document from JSON text, bounded by [`MAX_DOC_BYTES`] and [`MAX_DOC_DEPTH`];
/// duplicate keys are refused by `fid-canon`.
pub fn answer_from_json(bytes: &[u8]) -> Result<CValue, FieldError> {
    if bytes.len() > MAX_DOC_BYTES {
        return Err(FieldError::top(ErrorCode::BadAnswer, "the answer document is too large"));
    }
    parse_with_limits(bytes, MAX_DOC_BYTES, MAX_DOC_DEPTH)
        .map_err(|_| FieldError::top(ErrorCode::BadAnswer, "the answer is not bounded JSON without duplicate keys"))
}

// ------------------------------------------------------------------ descriptor decoding

struct Collect {
    errors: Vec<FieldError>,
    index: Option<usize>,
    field: Option<String>,
}

impl Collect {
    fn push(&mut self, code: ErrorCode, detail: &'static str) {
        self.errors.push(FieldError { code, field: self.field.clone(), index: self.index, detail });
    }

    fn bad(&mut self, detail: &'static str) {
        self.push(ErrorCode::BadDescriptor, detail);
    }
}

fn decode_rules(c: &mut Collect, ty: ResponseType, v: Option<&CValue>) -> Rules {
    let mut r = Rules::default();
    let m = match v {
        None => return r,
        Some(CValue::Obj(m)) => m,
        Some(_) => {
            c.bad("rules must be an object");
            return r;
        }
    };
    let allowed = ty.allowed_rules();
    for (k, v) in m {
        if !RULE_KEYS.contains(&k.as_str()) {
            c.bad("unknown rule");
            continue;
        }
        if !allowed.contains(&k.as_str()) {
            c.push(ErrorCode::RuleNotAllowed, "this rule is not allowed for the field's response type");
            continue;
        }
        let bound = |s: &str| if ty == ResponseType::Integer { Decimal::parse_integer(s) } else { Decimal::parse(s) };
        match (k.as_str(), v) {
            ("required", CValue::Bool(b)) => r.required = *b,
            ("min", CValue::Str(s)) => match bound(s) {
                Some(d) => r.min = Some(d),
                None => c.bad("min must be a decimal text (an integer text on an integer field)"),
            },
            ("max", CValue::Str(s)) => match bound(s) {
                Some(d) => r.max = Some(d),
                None => c.bad("max must be a decimal text (an integer text on an integer field)"),
            },
            ("max_length", CValue::Num(x)) if x.fract() == 0.0 && *x >= 1.0 && *x <= MAX_TEXT_ANSWER as f64 => {
                // Integral and within 1..=65536, so the conversion is exact.
                r.max_length = Some(*x as usize);
            }
            ("max_length", _) => c.bad("max_length must be an integer from 1 to 65536"),
            ("choices", CValue::Arr(items)) => {
                if items.is_empty() || items.len() > MAX_CHOICES {
                    c.bad("choices must hold 1 to 64 entries");
                    continue;
                }
                let mut seen = BTreeSet::new();
                for item in items {
                    match item {
                        CValue::Str(s) if is_display_text(s, MAX_LABEL, false) => {
                            if seen.insert(s.as_str()) {
                                r.choices.push(s.clone());
                            } else {
                                c.bad("choices must be unique");
                            }
                        }
                        _ => c.bad("a choice must be a non-empty text of at most 256 bytes"),
                    }
                }
            }
            ("required", _) => c.bad("required must be a boolean"),
            _ => c.bad("min and max must be strings, choices an array"),
        }
    }
    if let (Some(lo), Some(hi)) = (&r.min, &r.max) {
        if lo > hi {
            c.bad("min is above max");
        }
    }
    r
}

fn decode_field(c: &mut Collect, m: &Opaque, cat: &dyn QuantityCatalogue) -> Option<Field> {
    let start = c.errors.len();
    if m.keys().any(|k| !FIELD_KEYS.contains(&k.as_str())) {
        c.bad("unknown key in a field");
    }
    let name = match m.get("name") {
        Some(CValue::Str(s)) if is_name(s) => Some(s.clone()),
        _ => {
            c.bad("name must match ^[A-Za-z_][A-Za-z0-9_]{0,63}$");
            None
        }
    };
    let label = match m.get("label") {
        None => None,
        Some(CValue::Str(s)) if is_display_text(s, MAX_LABEL, false) => Some(AuthoredText(s.clone())),
        Some(_) => {
            c.bad("label must be a non-empty text of at most 256 bytes without control characters");
            None
        }
    };
    let response = match m.get("response") {
        Some(CValue::Str(s)) => ResponseType::parse(s),
        _ => None,
    };
    let Some(response) = response else {
        c.bad("response must be one of ack, bool, integer, decimal, quantity, text, choice, element");
        return None;
    };
    let quantity = match (response, m.get("quantity_kind"), m.get("unit")) {
        (ResponseType::Quantity, Some(CValue::Str(k)), Some(CValue::Str(u))) => {
            if cat.accepts(k, u) {
                Some((k.clone(), u.clone()))
            } else {
                c.bad("quantity_kind and unit are not a kind and one of its units in the unit catalogue");
                None
            }
        }
        (ResponseType::Quantity, _, _) => {
            c.bad("a quantity field needs quantity_kind and unit strings");
            None
        }
        (_, None, None) => None,
        _ => {
            c.bad("quantity_kind and unit are only allowed on a quantity field");
            None
        }
    };
    let rules = decode_rules(c, response, m.get("rules"));
    if response == ResponseType::Choice && !matches!(m.get("rules"), Some(CValue::Obj(r)) if r.contains_key("choices"))
    {
        c.bad("a choice field needs choices");
    }
    let mut field = Field { name: name.clone().unwrap_or_default(), label, response, quantity, rules, default: None };
    if let Some(d) = m.get("default") {
        if !response.allows_default() {
            c.bad("an ack field takes no default");
        } else if c.errors.len() == start {
            match check_value(&field, d) {
                Ok(v) => field.default = Some(v),
                Err(_) => c.bad("default does not satisfy the field's type, unit and rules"),
            }
        }
    }
    (c.errors.len() == start && name.is_some()).then_some(field)
}

impl OperatorDescriptor {
    /// Decodes and checks a descriptor object (an `operator` step's `descriptor`), failing
    /// closed with every error found: descriptor-level errors first, then field errors in
    /// field order. Kinds and units are looked up in `cat` (REQ-002); `quantity_kind` and
    /// `unit` must be a kind and one of its accepted units.
    pub fn from_object(m: &Opaque, cat: &dyn QuantityCatalogue) -> Result<OperatorDescriptor, Vec<FieldError>> {
        let mut c = Collect { errors: Vec::new(), index: None, field: None };
        if m.keys().any(|k| !DESCRIPTOR_KEYS.contains(&k.as_str())) {
            c.bad("unknown key in the descriptor");
        }
        if !matches!(m.get("format"), Some(CValue::Str(s)) if s == OPERATOR_FORMAT) {
            c.bad("format must be operator-v1");
        }
        let prompt = match m.get("prompt") {
            Some(CValue::Str(s)) if is_display_text(s, MAX_PROMPT, true) => Some(AuthoredText(s.clone())),
            _ => {
                c.bad("prompt must be a non-empty text of at most 8192 bytes without control characters but newline");
                None
            }
        };
        let image = match m.get("image") {
            None => None,
            Some(CValue::Str(s)) if is_image_handle(s) => Some(s.clone()),
            Some(_) => {
                c.bad("image must be a root:// handle");
                None
            }
        };
        let items: &[CValue] = match m.get("fields") {
            Some(CValue::Arr(a)) if a.len() <= MAX_FIELDS => a,
            Some(CValue::Arr(_)) => {
                c.bad("a descriptor has at most 64 fields");
                &[]
            }
            _ => {
                c.bad("fields must be an array");
                &[]
            }
        };
        let mut fields = Vec::with_capacity(items.len());
        let mut seen = BTreeSet::new();
        for (i, item) in items.iter().enumerate() {
            c.index = Some(i);
            c.field = match item {
                CValue::Obj(f) => match f.get("name") {
                    Some(CValue::Str(s)) => Some(s.clone()),
                    _ => None,
                },
                _ => None,
            };
            let CValue::Obj(f) = item else {
                c.bad("a field must be an object");
                continue;
            };
            if let Some(n) = &c.field {
                if is_name(n) && !seen.insert(n.to_ascii_lowercase()) {
                    c.bad("two fields share a name (compared ignoring ASCII case)");
                }
            }
            if let Some(field) = decode_field(&mut c, f, cat) {
                fields.push(field);
            }
        }
        match (c.errors.is_empty(), prompt) {
            (true, Some(prompt)) => Ok(OperatorDescriptor { prompt, image, fields }),
            _ => Err(c.errors),
        }
    }

    /// [`OperatorDescriptor::from_object`] on JSON text (a dialog file), bounded by
    /// [`MAX_DOC_BYTES`] and [`MAX_DOC_DEPTH`]; duplicate keys are refused by `fid-canon`.
    pub fn from_json(bytes: &[u8], cat: &dyn QuantityCatalogue) -> Result<OperatorDescriptor, Vec<FieldError>> {
        let bad = |detail| vec![FieldError::top(ErrorCode::BadDescriptor, detail)];
        if bytes.len() > MAX_DOC_BYTES {
            return Err(bad("the descriptor document is too large"));
        }
        match parse_with_limits(bytes, MAX_DOC_BYTES, MAX_DOC_DEPTH) {
            Ok(CValue::Obj(m)) => OperatorDescriptor::from_object(&m, cat),
            Ok(_) => Err(bad("a descriptor must be a JSON object")),
            Err(_) => Err(bad("the descriptor is not bounded JSON without duplicate keys")),
        }
    }

    /// Whether the descriptor only shows its prompt (no fields, no response).
    pub fn is_display_only(&self) -> bool {
        self.fields.is_empty()
    }
}

// ------------------------------------------------------------------ schema

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(n: usize) -> CValue {
    // Every bound here is far below 2^53, so the conversion is exact.
    CValue::Num(n as f64)
}

fn text_schema(max: usize) -> CValue {
    obj([("type", s("string")), ("minLength", num(1)), ("maxLength", num(max))])
}

fn pattern(p: &str) -> CValue {
    obj([("type", s("string")), ("pattern", s(p))])
}

fn value_schema(t: ResponseType) -> CValue {
    match t {
        ResponseType::Ack => obj([("const", CValue::Bool(true))]),
        ResponseType::Bool => obj([("type", s("boolean"))]),
        ResponseType::Integer => pattern(INTEGER_PATTERN),
        ResponseType::Decimal => pattern(DECIMAL_PATTERN),
        ResponseType::Quantity => obj([
            ("type", s("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", CValue::Arr(vec![s("unit"), s("value")])),
            ("properties", obj([("unit", obj([("type", s("string"))])), ("value", pattern(DECIMAL_PATTERN))])),
        ]),
        ResponseType::Text => obj([("type", s("string")), ("maxLength", num(MAX_TEXT_ANSWER))]),
        ResponseType::Choice => text_schema(MAX_LABEL),
        ResponseType::Element => text_schema(MAX_HANDLE),
    }
}

fn rule_schema(t: ResponseType, rule: &str) -> CValue {
    match rule {
        "required" => obj([("type", s("boolean"))]),
        "min" | "max" if t == ResponseType::Integer => pattern(INTEGER_PATTERN),
        "min" | "max" => pattern(DECIMAL_PATTERN),
        "max_length" => obj([("type", s("integer")), ("minimum", num(1)), ("maximum", num(MAX_TEXT_ANSWER))]),
        _ => obj([
            ("type", s("array")),
            ("minItems", num(1)),
            ("maxItems", num(MAX_CHOICES)),
            ("uniqueItems", CValue::Bool(true)),
            ("items", text_schema(MAX_LABEL)),
        ]),
    }
}

fn field_schema(t: ResponseType) -> CValue {
    let rules: BTreeMap<String, CValue> =
        t.allowed_rules().iter().map(|r| (r.to_string(), rule_schema(t, r))).collect();
    let mut rules_obj = BTreeMap::new();
    rules_obj.insert("type".to_string(), s("object"));
    rules_obj.insert("additionalProperties".to_string(), CValue::Bool(false));
    rules_obj.insert("properties".to_string(), CValue::Obj(rules));
    if t == ResponseType::Choice {
        rules_obj.insert("required".to_string(), CValue::Arr(vec![s("choices")]));
    }
    let mut props = BTreeMap::new();
    props.insert("name".to_string(), pattern(NAME_PATTERN));
    props.insert("label".to_string(), text_schema(MAX_LABEL));
    props.insert("response".to_string(), obj([("const", s(t.as_str()))]));
    props.insert("rules".to_string(), CValue::Obj(rules_obj));
    let mut required = vec![s("name"), s("response")];
    if t.allows_default() {
        props.insert("default".to_string(), value_schema(t));
    }
    if t == ResponseType::Quantity {
        props.insert("quantity_kind".to_string(), obj([("type", s("string"))]));
        props.insert("unit".to_string(), obj([("type", s("string"))]));
        required.extend([s("quantity_kind"), s("unit")]);
    }
    if t == ResponseType::Choice {
        required.push(s("rules"));
    }
    obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", CValue::Arr(required)),
        ("properties", CValue::Obj(props)),
    ])
}

/// JSON Schema (2020-12) of `operator-v1`, generated from the type and rule tables above;
/// checked in as `schema/operator-v1.json` (REQ-006). It cannot express byte lengths,
/// forbidden code points, field-name uniqueness, exact decimal bounds or the unit
/// catalogue; [`OperatorDescriptor::from_object`] enforces those.
pub fn operator_schema() -> CValue {
    let mut defs = BTreeMap::new();
    let mut members = Vec::new();
    for t in ResponseType::ALL {
        let def = format!("field_{}", t.as_str());
        members.push(obj([("$ref", s(&format!("#/$defs/{def}")))]));
        defs.insert(def, field_schema(t));
    }
    defs.insert("field".to_string(), obj([("oneOf", CValue::Arr(members))]));
    let props = obj([
        ("format", obj([("const", s(OPERATOR_FORMAT))])),
        ("prompt", text_schema(MAX_PROMPT)),
        ("image", pattern(IMAGE_PATTERN)),
        (
            "fields",
            obj([("type", s("array")), ("maxItems", num(MAX_FIELDS)), ("items", obj([("$ref", s("#/$defs/field"))]))]),
        ),
    ]);
    obj([
        ("$schema", s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", s("https://metrologymaven.io/schema/operator-v1.json")),
        ("title", s("Metrology Maven operator-interaction descriptor, operator-v1")),
        (
            "description",
            s("Prompt and typed fields for an operator step or dialog. Lengths here count characters; the \
               parser also bounds bytes. See spec node AUTO.PROG.OPER."),
        ),
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", CValue::Arr(vec![s("fields"), s("format"), s("prompt")])),
        ("properties", props),
        ("$defs", CValue::Obj(defs)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(t: &str) -> Decimal {
        let parsed = Decimal::parse(t);
        assert!(parsed.is_some(), "{t}");
        parsed.unwrap_or(Decimal { authored: String::new(), neg: false, int: String::new(), frac: String::new() })
    }

    #[test]
    fn decimal_grammar() {
        for ok in ["0", "-0", "+1", "10.10", "007.500", "-22.01"] {
            assert!(Decimal::parse(ok).is_some(), "{ok}");
        }
        for bad in ["", "+", "-", ".5", "1.", "1e3", " 1", "1 ", "1,5", "--1", "0x1", "1.2.3", "١", "NaN"] {
            assert!(Decimal::parse(bad).is_none(), "{bad}");
        }
        assert!(Decimal::parse(&"9".repeat(40)).is_some());
        assert!(Decimal::parse(&"9".repeat(41)).is_none());
        assert!(Decimal::parse(&"0".repeat(MAX_DECIMAL_TEXT + 1)).is_none());
        assert!(Decimal::parse_integer("10.0").is_none() && Decimal::parse_integer("-10").is_some());
    }

    #[test]
    fn decimal_order_is_exact() {
        assert_eq!(d("10.10"), d("10.1"));
        assert_eq!(d("-0"), d("0.000"));
        assert_eq!(d("007"), d("7"));
        assert!(d("22.01") > d("22"));
        assert!(d("-22.01") < d("-22"));
        assert!(d("0.05") < d("0.5"));
        assert!(d("100") > d("99.999"));
        assert!(d("-1") < d("0"));
        assert!(d("0.30000000000000000001") > d("0.3"));
    }

    #[test]
    fn image_handles() {
        for ok in ["root://shop/img/part.png", "root://a/b", "root://A_1-x/.hidden.jpg"] {
            assert!(is_image_handle(ok), "{ok}");
        }
        for bad in [
            "root://shop",
            "root://shop/",
            "root://shop/../x",
            "root://shop/a/./b",
            "root://shop//b",
            "root:///b",
            "root://-x/b",
            "root://shop/a\\b",
            "root://shop/C:x",
            "/etc/passwd",
            "root://shop/a.",
        ] {
            assert!(!is_image_handle(bad), "{bad}");
        }
    }

    #[test]
    fn rule_tables_are_sorted_subsets() {
        for t in ResponseType::ALL {
            let r = t.allowed_rules();
            assert!(r.windows(2).all(|w| w[0] < w[1]));
            assert!(r.iter().all(|x| RULE_KEYS.contains(x)));
            assert_eq!(ResponseType::parse(t.as_str()), Some(t));
        }
        assert!(RULE_KEYS.windows(2).all(|w| w[0] < w[1]));
        assert!(FIELD_KEYS.windows(2).all(|w| w[0] < w[1]));
        assert!(DESCRIPTOR_KEYS.windows(2).all(|w| w[0] < w[1]));
    }
}
