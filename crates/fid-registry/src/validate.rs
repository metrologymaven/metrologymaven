//! Build-time declaration validation. Spec node `CORE.REG.VAL`.
//!
//! A pure function from a list of command declarations to a sorted list of typed errors
//! ([`validate`]), plus a byte-level entry point with input limits ([`validate_json`]).
//! Every choice below is pinned in `spec/core/reg/val/NODE.md` (Design).
//!
//! - **Declarations are raw JSON objects** ([`CommandDecl`]), so a missing field can be
//!   reported (REQ-001); a declaration that passes every rule here also parses as a
//!   [`CommandDef`] (the DEF parse runs last as a backstop).
//! - **Strict:** unknown fields, keys that differ only in ASCII case, wrong JSON types,
//!   unknown enum values, out-of-range integers and empty numeric ranges are errors.
//!   NaN, infinity and subnormal numbers are errors everywhere in a declaration; at the
//!   byte level a literal that underflows to zero or to a subnormal is refused too. Nothing
//!   is coerced or defaulted.
//! - **MCP tool name** (CONSTITUTION §3): the ID with dots replaced by underscores, plus
//!   `_v<major>` when the major is 2 or more, at most 64 characters of `[a-z0-9_]`.
//! - **Deterministic:** no hash maps, no clock, no environment; output sorted by command,
//!   major, rule code, field and message (REQ-008).
//! - **Bounded:** iterative walks with an explicit stack, depth and size limits, echoed
//!   input cut to 128 bytes, at most [`MAX_ERRORS`] errors. No panics.

use crate::def::{CommandDef, DefError, Projection, FIELDS};
use fid_canon::CValue;
use fid_geo_frame::{AngleUnit, LengthUnit, Unit};
use fid_types::{Determinism, SideEffect};
use std::collections::{btree_map, BTreeMap, BTreeSet};
use std::fmt;

/// Default largest accepted input of [`validate_json`], in bytes (16 MiB).
pub const DEFAULT_MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Default deepest accepted nesting of a schema (the schema object itself is level 1).
pub const DEFAULT_MAX_SCHEMA_DEPTH: usize = 64;
/// Hard cap on a configured schema depth: the file adds 3 levels above a schema
/// (root object, `decls` array, declaration) and `fid-canon` parses at most 128.
pub const MAX_SCHEMA_DEPTH_CAP: usize = 124;
/// Longest MCP tool name (CONSTITUTION §3).
pub const MAX_TOOL_NAME_LEN: usize = 64;
/// Largest major: `fid_types::CommandId` allows at most 5 digits.
pub const MAX_MAJOR: u32 = 99_999;
/// Most errors reported by one run; beyond it the run ends with `too_many_errors`.
pub const MAX_ERRORS: usize = 10_000;
/// Largest integer accepted for a count keyword (`minLength` and so on): 2^53.
pub(crate) const MAX_COUNT: f64 = 9_007_199_254_740_992.0;
/// Longest input text echoed into an error, in bytes.
const KEEP: usize = 128;
/// Longest `field` pointer, in bytes.
const KEEP_POINTER: usize = 512;
/// Longest message, in bytes.
pub(crate) const KEEP_MESSAGE: usize = 512;

/// The fields REQ-001 names, in its order; all are also required by the DEF record.
pub const REQ001_FIELDS: [&str; 7] =
    ["id", "major", "params_schema", "result_schema", "side_effect", "mcp_projection", "determinism"];

/// Fields of the DEF record that may be absent (they have defaults or are conditional).
const OPTIONAL_FIELDS: [&str; 4] = ["human_only", "network", "web_note", "cli_bare"];

/// JSON Schema type names.
pub(crate) const TYPES: [&str; 7] = ["array", "boolean", "integer", "null", "number", "object", "string"];

macro_rules! rule_codes {
    ($($variant:ident => $s:literal),+ $(,)?) => {
        /// Validation rule codes, stable snake_case strings.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum RuleCode {
            $($variant,)+
        }

        impl RuleCode {
            /// Every code, in declaration order (which is alphabetical by wire string).
            pub const ALL: &'static [RuleCode] = &[$(RuleCode::$variant),+];

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(RuleCode::$variant => $s,)+
                }
            }
        }
    };
}

rule_codes! {
    BadAlgorithm => "bad_algorithm",
    BadDialect => "bad_dialect",
    BadEnum => "bad_enum",
    BadId => "bad_id",
    BadNumber => "bad_number",
    BadPrecondition => "bad_precondition",
    BadPropertyName => "bad_property_name",
    BadSchema => "bad_schema",
    BadToolName => "bad_tool_name",
    BadType => "bad_type",
    DuplicateCommand => "duplicate_command",
    DuplicateKey => "duplicate_key",
    ExternalRef => "external_ref",
    MissingField => "missing_field",
    MissingQuantity => "missing_quantity",
    ProjectionTooLoose => "projection_too_loose",
    SchemaTooDeep => "schema_too_deep",
    TooManyErrors => "too_many_errors",
    ToolNameCollision => "tool_name_collision",
    ToolNameTooLong => "tool_name_too_long",
    UnitNotAccepted => "unit_not_accepted",
    UnknownField => "unknown_field",
    UnknownQuantity => "unknown_quantity",
    UnwalkedRef => "unwalked_ref",
    WebNoteMissing => "web_note_missing",
}

impl fmt::Display for RuleCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// The declaration's `id` when it is a valid command ID (REQ-002), else empty.
    pub command: String,
    /// The declaration's `major` when it is an integer in `0..=u32::MAX`, else 0.
    pub major: u32,
    pub rule: RuleCode,
    /// A display-safe pointer into the declaration, or `None` when the error concerns the
    /// declaration as a whole. It is RFC 6901 (`~0`, `~1`) for printable ASCII keys; every
    /// other character is written as `\u{..}`, so for such keys it is not an exact RFC 6901
    /// pointer. Each segment is escaped first and then cut to 128 bytes, the whole to 512, and
    /// no cut splits an escape. The pointer is printable ASCII (`to_value` escapes again,
    /// which changes nothing for pointers made here).
    pub field: Option<String>,
    pub message: String,
}

impl ValidationError {
    fn sort_key(&self) -> (&str, u32, &str, Option<&str>, &str) {
        (&self.command, self.major, self.rule.as_str(), self.field.as_deref(), &self.message)
    }

    /// The JSON form `{"command", "major", "rule", "field", "message"}` (`field` may be null).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command".to_string(), CValue::Str(self.command.clone()));
        o.insert("major".to_string(), CValue::Num(f64::from(self.major)));
        o.insert("rule".to_string(), CValue::Str(self.rule.as_str().to_string()));
        o.insert("field".to_string(), self.field.as_deref().map_or(CValue::Null, |f| CValue::Str(escape(f))));
        o.insert("message".to_string(), CValue::Str(self.message.clone()));
        CValue::Obj(o)
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}@{}: {}", self.command, self.major, self.rule)?;
        if let Some(p) = &self.field {
            write!(f, " at {p:?}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for ValidationError {}

/// The JSON array of a run's errors, in the order given; hash or print it with `fid-canon`.
pub fn errors_to_value(errors: &[ValidationError]) -> CValue {
    CValue::Arr(errors.iter().map(ValidationError::to_value).collect())
}

/// Input-level failures of [`validate_json`]: the input is refused as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// Longer than [`Limits::max_input_bytes`].
    InputTooLarge,
    /// Nested deeper than 3 + [`Limits::max_schema_depth`] anywhere.
    SchemaTooDeep,
    /// A number literal that is not finite, or that underflows to zero or a subnormal
    /// (the literal, cut to 128 bytes).
    BadNumber(String),
    /// The JSON is malformed or has an exact duplicate key (`fid-canon` code).
    Json(&'static str),
    /// The file is not `{"decls": [...]}`.
    BadFile(String),
}

impl InputError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            InputError::InputTooLarge => "input_too_large",
            InputError::SchemaTooDeep => "schema_too_deep",
            InputError::BadNumber(_) => "bad_number",
            InputError::Json(c) => c,
            InputError::BadFile(_) => "bad_file",
        }
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputError::BadNumber(s) => write!(f, "bad_number: {s:?}"),
            InputError::BadFile(s) => write!(f, "bad_file: {s}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for InputError {}

/// Input limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest input of [`validate_json`] in bytes; clamped to `fid_canon::MAX_INPUT_BYTES`.
    pub max_input_bytes: usize,
    /// Deepest schema nesting; clamped to [`MAX_SCHEMA_DEPTH_CAP`].
    pub max_schema_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_input_bytes: DEFAULT_MAX_INPUT_BYTES, max_schema_depth: DEFAULT_MAX_SCHEMA_DEPTH }
    }
}

impl Limits {
    fn clamped(self) -> Limits {
        Limits {
            max_input_bytes: self.max_input_bytes.min(fid_canon::MAX_INPUT_BYTES),
            max_schema_depth: self.max_schema_depth.min(MAX_SCHEMA_DEPTH_CAP),
        }
    }
}

/// The quantity kinds `x-quantity` may name and the units `x-unit` may use with them.
pub trait QuantityKinds {
    /// Whether `kind` is a kind of the catalogue.
    fn has_kind(&self, kind: &str) -> bool;
    /// Whether `unit` is an accepted unit of `kind`.
    fn accepts(&self, kind: &str, unit: &str) -> bool;
}

/// The unit catalogue of `GEO.FRAME.UNITS` (`fid-geo-frame`): kinds `length` and `angle`.
/// The kind names are the variants of [`fid_geo_frame::Unit`]; no unit is defined here.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeoFrameUnits;

/// The kind name of a catalogue unit.
pub fn unit_kind(unit: Unit) -> &'static str {
    match unit {
        Unit::Length(_) => "length",
        Unit::Angle(_) => "angle",
    }
}

impl QuantityKinds for GeoFrameUnits {
    fn has_kind(&self, kind: &str) -> bool {
        [Unit::Length(LengthUnit::M), Unit::Angle(AngleUnit::Rad)].into_iter().any(|u| unit_kind(u) == kind)
    }

    fn accepts(&self, kind: &str, unit: &str) -> bool {
        Unit::from_symbol(unit).is_some_and(|u| unit_kind(u) == kind)
    }
}

/// One command declaration as written: a JSON value that should be a [`CommandDef`] object.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandDecl(pub CValue);

impl CommandDecl {
    /// Wraps a raw JSON value.
    pub fn from_value(v: CValue) -> Self {
        CommandDecl(v)
    }

    /// The declaration of a parsed definition.
    pub fn from_def(def: &CommandDef) -> Self {
        CommandDecl(def.to_value())
    }
}

/// Whether `id` matches `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$` (REQ-002).
pub fn is_valid_id(id: &str) -> bool {
    let Some((domain, rest)) = id.split_once('.') else { return false };
    let segment = |s: &str, underscore: bool| {
        let mut b = s.bytes();
        b.next().is_some_and(|c| c.is_ascii_lowercase())
            && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || (underscore && c == b'_'))
    };
    segment(domain, false) && segment(rest, true)
}

/// The MCP tool name of `id@major` (CONSTITUTION §3): dots to underscores, and `_v<major>`
/// when `major` is 2 or more.
pub fn tool_name(id: &str, major: u32) -> String {
    let base = id.replace('.', "_");
    if major >= 2 {
        format!("{base}_v{major}")
    } else {
        base
    }
}

/// The loosest projection a side-effect class allows (REQ-004).
pub fn max_projection(class: SideEffect) -> Projection {
    match class {
        SideEffect::R0 | SideEffect::R1 | SideEffect::I1 | SideEffect::W1 | SideEffect::W2 => Projection::Exposed,
        SideEffect::W3 | SideEffect::X1 | SideEffect::D1 => Projection::RequestOnly,
        SideEffect::M1 | SideEffect::S1 | SideEffect::A0 => Projection::Never,
    }
}

/// Validates declarations with the default limits and the `GEO.FRAME.UNITS` catalogue.
pub fn validate(decls: &[CommandDecl]) -> Vec<ValidationError> {
    validate_with(decls, &Limits::default(), &GeoFrameUnits)
}

/// Validates declarations: all errors of the run, sorted (REQ-008).
pub fn validate_with(decls: &[CommandDecl], limits: &Limits, kinds: &dyn QuantityKinds) -> Vec<ValidationError> {
    let limits = limits.clamped();
    let mut sink = Sink::default();
    // (id, major) -> count, and tool name -> distinct (id, major), for REQ-006 and REQ-007.
    let mut by_key: BTreeMap<(String, u32), usize> = BTreeMap::new();
    let mut by_tool: BTreeMap<String, BTreeSet<(String, u32)>> = BTreeMap::new();
    for decl in decls {
        if sink.full() {
            break;
        }
        if let Some((id, major)) = check_decl(&decl.0, &limits, kinds, &mut sink) {
            by_tool.entry(tool_name(&id, major)).or_default().insert((id.clone(), major));
            *by_key.entry((id, major)).or_insert(0) += 1;
        }
    }
    for ((id, major), n) in &by_key {
        if *n > 1 {
            sink.push(ValidationError {
                command: id.clone(),
                major: *major,
                rule: RuleCode::DuplicateCommand,
                field: None,
                message: format!("{n} declarations share {id}@{major}"),
            });
        }
    }
    for (tool, members) in &by_tool {
        if members.len() < 2 {
            continue;
        }
        for (id, major) in members {
            let others: Vec<String> =
                members.iter().filter(|m| (&m.0, m.1) != (id, *major)).map(|(i, m)| format!("{i}@{m}")).collect();
            sink.push(ValidationError {
                command: id.clone(),
                major: *major,
                rule: RuleCode::ToolNameCollision,
                field: Some("/id".to_string()),
                message: cut(&format!("tool name {tool} is also derived by {}", others.join(", ")), KEEP_MESSAGE),
            });
        }
    }
    sink.finish()
}

/// Validates a declaration file `{"decls": [...]}`, refusing it as a whole when it is too
/// large, too deep, malformed or holds an unrepresentable number literal.
pub fn validate_json(
    bytes: &[u8],
    limits: &Limits,
    kinds: &dyn QuantityKinds,
) -> Result<Vec<ValidationError>, InputError> {
    let limits = limits.clamped();
    if bytes.len() > limits.max_input_bytes {
        return Err(InputError::InputTooLarge);
    }
    prescan(bytes, 3 + limits.max_schema_depth)?;
    let value = fid_canon::parse_with_limits(bytes, limits.max_input_bytes, fid_canon::MAX_DEPTH)
        .map_err(|e| InputError::Json(e.code()))?;
    let CValue::Obj(mut root) = value else {
        return Err(InputError::BadFile("the file must be an object".to_string()));
    };
    if let Some(k) = root.keys().find(|k| k.as_str() != "decls") {
        return Err(InputError::BadFile(format!("unknown field {}", quote(k))));
    }
    let Some(CValue::Arr(items)) = root.remove("decls") else {
        return Err(InputError::BadFile("decls must be an array".to_string()));
    };
    let decls: Vec<CommandDecl> = items.into_iter().map(CommandDecl).collect();
    Ok(validate_with(&decls, &limits, kinds))
}

// ------------------------------------------------------------------ byte pre-scan

/// One pass over the bytes before parsing: bracket depth outside strings, and every number
/// literal must be finite and must not underflow (a non-zero mantissa giving zero or a
/// subnormal). Malformed text is left to the parser.
pub(crate) fn prescan(b: &[u8], max_depth: usize) -> Result<(), InputError> {
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > max_depth {
                    return Err(InputError::SchemaTooDeep);
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            b'-' | b'0'..=b'9' => {
                let start = i;
                while b.get(i).is_some_and(|c| matches!(c, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
                    i += 1;
                }
                check_literal(b.get(start..i).unwrap_or_default())?;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

fn check_literal(lit: &[u8]) -> Result<(), InputError> {
    let Ok(text) = std::str::from_utf8(lit) else { return Ok(()) };
    let Ok(v) = text.parse::<f64>() else { return Ok(()) };
    let mantissa = text.split(['e', 'E']).next().unwrap_or_default();
    let nonzero = mantissa.bytes().any(|c| matches!(c, b'1'..=b'9'));
    if !v.is_finite() || (nonzero && (v == 0.0 || v.is_subnormal())) || above_2_53(text) {
        return Err(InputError::BadNumber(cut(text, KEEP)));
    }
    Ok(())
}

/// Whether the exact decimal value of a literal (`-`, digits, optional `.` digits, optional
/// exponent) exceeds 2^53 in magnitude, in any notation (`9007199254740993`,
/// `9007199254740993.0`, `9007199254740993e0`, `1e300`). Above 2^53 an integer may round and
/// a non-integer always does, so all are refused. Decided on digit strings, never through
/// `f64`. A literal whose exponent does not fit `i64` counts as too big (fail closed).
fn above_2_53(text: &str) -> bool {
    const LIMIT: &[u8] = b"9007199254740992";
    let body = text.trim_start_matches('-');
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => (body.get(..i).unwrap_or_default(), body.get(i + 1..).unwrap_or_default()),
        None => (body, "0"),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    // Significant digits D and the count of integer digits of D x 10^(exp - frac.len()).
    let all: Vec<u8> = int.bytes().chain(frac.bytes()).collect();
    let Some(first) = all.iter().position(|&c| c != b'0') else { return false };
    let digits = all.get(first..).unwrap_or_default();
    let Ok(exp) = exp.parse::<i64>() else { return true };
    let (Ok(n_digits), Ok(n_frac)) = (i64::try_from(digits.len()), i64::try_from(frac.len())) else { return true };
    let int_len = n_digits.saturating_add(exp).saturating_sub(n_frac);
    if int_len != 16 {
        return int_len > 16;
    }
    // Exactly 16 integer digits: compare them, then any non-zero fraction digit exceeds.
    let mut head = [b'0'; 16];
    for (h, d) in head.iter_mut().zip(digits) {
        *h = *d;
    }
    let rest = digits.get(LIMIT.len()..).unwrap_or_default();
    head.as_slice() > LIMIT || (head.as_slice() == LIMIT && rest.iter().any(|&c| c != b'0'))
}

// ------------------------------------------------------------------ error sink

#[derive(Default)]
struct Sink {
    errors: Vec<ValidationError>,
}

impl Sink {
    fn full(&self) -> bool {
        self.errors.len() > MAX_ERRORS
    }

    fn push(&mut self, e: ValidationError) {
        if !self.full() {
            self.errors.push(e);
        }
    }

    fn finish(mut self) -> Vec<ValidationError> {
        sort(&mut self.errors);
        if self.errors.len() > MAX_ERRORS {
            self.errors.truncate(MAX_ERRORS);
            self.errors.push(ValidationError {
                command: String::new(),
                major: 0,
                rule: RuleCode::TooManyErrors,
                field: None,
                message: format!("more than {MAX_ERRORS} errors; the rest are not reported"),
            });
            sort(&mut self.errors);
        }
        self.errors
    }
}

fn sort(errors: &mut [ValidationError]) {
    errors.sort_by(order);
}

fn order(a: &ValidationError, b: &ValidationError) -> std::cmp::Ordering {
    a.sort_key().cmp(&b.sort_key())
}

/// Copies at most `max` bytes of `s`, cut at a char boundary.
pub(crate) fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_string()
}

/// Writes every character outside printable ASCII (U+0020..=U+007E) as `\u{..}`. The output
/// is printable ASCII, and escaping it again changes nothing.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            out.push(c);
        } else {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        }
    }
    out
}

/// Hostile text as it may appear in a message: cut to 128 bytes, escaped, then quoted with
/// `{:?}` (which also escapes `"` and `\`).
pub(crate) fn quote(s: &str) -> String {
    format!("{:?}", escape(&cut(s, KEEP)))
}

// ------------------------------------------------------------------ one declaration

/// The per-declaration context: identity for errors, limits and the catalogue.
struct Ctx<'c> {
    sink: &'c mut Sink,
    kinds: &'c dyn QuantityKinds,
    max_depth: usize,
    command: String,
    major: u32,
    /// Top-level fields already reported as too deep.
    deep: BTreeSet<String>,
    /// Whether a structural error (shape, type, enum, ID, number, key) was reported.
    structural: bool,
    /// Local `$ref`s found by the walk, resolved after it.
    refs: Vec<PendingRef>,
}

/// A local `$ref` waiting for resolution.
struct PendingRef {
    /// The top-level field holding the schema (`params_schema` or `result_schema`).
    root: String,
    /// The JSON pointer after `#`.
    target: String,
    /// Where the `$ref` is, for the error.
    at: String,
}

impl Ctx<'_> {
    fn push(&mut self, rule: RuleCode, field: Option<String>, message: String) {
        if matches!(
            rule,
            RuleCode::UnknownField
                | RuleCode::MissingField
                | RuleCode::BadType
                | RuleCode::BadEnum
                | RuleCode::BadId
                | RuleCode::BadNumber
                | RuleCode::DuplicateKey
                | RuleCode::SchemaTooDeep
        ) {
            self.structural = true;
        }
        self.sink.push(ValidationError {
            command: self.command.clone(),
            major: self.major,
            rule,
            field,
            message: cut(&message, KEEP_MESSAGE),
        });
    }

    fn at(&mut self, rule: RuleCode, field: &str, message: String) {
        self.push(rule, Some(pointer(&[Seg::Key(field)])), message);
    }
}

/// Checks one declaration (rules 001 to 005 and the field rules). Returns its (ID, major)
/// when both are valid, for the cross-declaration rules.
fn check_decl(v: &CValue, limits: &Limits, kinds: &dyn QuantityKinds, sink: &mut Sink) -> Option<(String, u32)> {
    let mut ctx = Ctx {
        sink,
        kinds,
        max_depth: limits.max_schema_depth,
        command: String::new(),
        major: 0,
        deep: BTreeSet::new(),
        structural: false,
        refs: Vec::new(),
    };
    let CValue::Obj(m) = v else {
        ctx.push(RuleCode::BadType, None, "a declaration must be an object".to_string());
        return None;
    };
    if let Some(CValue::Str(s)) = m.get("id").filter(|v| matches!(v, CValue::Str(s) if is_valid_id(s))) {
        ctx.command = cut(s, KEEP);
    }
    if let Some(CValue::Num(n)) = m.get("major").filter(|v| is_int_in(v, 0.0, f64::from(u32::MAX))) {
        ctx.major = *n as u32;
    }

    for k in m.keys() {
        if !FIELDS.contains(&k.as_str()) {
            ctx.push(RuleCode::UnknownField, Some(pointer(&[Seg::Key(k)])), format!("unknown field {}", quote(k)));
        }
    }
    for f in FIELDS {
        if !OPTIONAL_FIELDS.contains(&f) && !m.contains_key(f) {
            ctx.at(RuleCode::MissingField, f, format!("the declaration lacks required field {f}"));
        }
    }

    let id = match m.get("id") {
        Some(CValue::Str(s)) if is_valid_id(s) => Some(s.clone()),
        Some(CValue::Str(s)) => {
            let msg = format!("id {} does not match ^[a-z][a-z0-9]*\\.[a-z][a-z0-9_]*$", quote(s));
            ctx.at(RuleCode::BadId, "id", msg);
            None
        }
        Some(_) => {
            ctx.at(RuleCode::BadType, "id", "id must be a string".to_string());
            None
        }
        None => None,
    };
    let major = match m.get("major") {
        Some(CValue::Num(n)) if is_int_in(&CValue::Num(*n), 1.0, f64::from(MAX_MAJOR)) => Some(*n as u32),
        Some(CValue::Num(n)) => {
            ctx.at(RuleCode::BadId, "major", format!("major {n} is not an integer 1..={MAX_MAJOR}"));
            None
        }
        Some(_) => {
            ctx.at(RuleCode::BadType, "major", "major must be a number".to_string());
            None
        }
        None => None,
    };
    let side_effect = enum_field(&mut ctx, m, "side_effect", |s| SideEffect::parse(s).ok());
    let projection = enum_field(&mut ctx, m, "mcp_projection", |s| Projection::parse(s).ok());
    enum_field(&mut ctx, m, "determinism", |s| Determinism::parse(s).ok());
    for f in ["params_schema", "result_schema"] {
        if m.get(f).is_some_and(|s| !matches!(s, CValue::Obj(_))) {
            ctx.at(RuleCode::BadType, f, format!("{f} must be a JSON object"));
        }
    }

    walk(&mut ctx, m);
    for r in std::mem::take(&mut ctx.refs) {
        let lands = m.get(&r.root).is_some_and(|root| resolve(root, &r.target) == Some(Kind::Schema));
        if !lands {
            let msg = format!("$ref {} does not point at a schema position the validator checks", quote(&r.target));
            ctx.push(RuleCode::UnwalkedRef, Some(r.at), msg);
        }
    }

    // REQ-003: the derived tool name.
    if let (Some(id), Some(major)) = (&id, major) {
        let name = tool_name(id, major);
        if !name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            ctx.at(RuleCode::BadToolName, "id", format!("tool name {name} has a character outside [a-z0-9_]"));
        }
        if name.len() > MAX_TOOL_NAME_LEN {
            let msg = format!("tool name {name} has {} characters, more than {MAX_TOOL_NAME_LEN}", name.len());
            ctx.at(RuleCode::ToolNameTooLong, "id", msg);
        }
    }
    // REQ-004: projection not looser than the class maximum.
    if let (Some(class), Some(p)) = (side_effect, projection) {
        let max = max_projection(class);
        if p > max {
            let msg = format!("{} is projected {p}, looser than the class maximum {max}", class.as_str());
            ctx.at(RuleCode::ProjectionTooLoose, "mcp_projection", msg);
        }
    }
    // Backstop: everything else the DEF record requires (titles, effects, algorithms, ...).
    if !ctx.structural {
        if let Err(e) = CommandDef::from_value(v.clone()) {
            let (rule, field, message) = def_rule(&e);
            ctx.push(rule, field, message.to_string());
        }
    }
    match (id, major) {
        (Some(id), Some(major)) => Some((id, major)),
        _ => None,
    }
}

/// Whether `v` is a JSON number holding an integer in `lo..=hi`.
fn is_int_in(v: &CValue, lo: f64, hi: f64) -> bool {
    matches!(v, CValue::Num(n) if n.fract() == 0.0 && (lo..=hi).contains(n))
}

fn enum_field<T>(ctx: &mut Ctx<'_>, m: &BTreeMap<String, CValue>, f: &str, parse: fn(&str) -> Option<T>) -> Option<T> {
    match m.get(f) {
        Some(CValue::Str(s)) => {
            let v = parse(s);
            if v.is_none() {
                ctx.at(RuleCode::BadEnum, f, format!("{f} {} is not a known value", quote(s)));
            }
            v
        }
        Some(_) => {
            ctx.at(RuleCode::BadType, f, format!("{f} must be a string"));
            None
        }
        None => None,
    }
}

/// The rule, field and a fixed message for a DEF error; the error's own text (which may
/// echo input) is never used.
fn def_rule(e: &DefError) -> (RuleCode, Option<String>, &'static str) {
    let p = |k: &str| Some(pointer(&[Seg::Key(k)]));
    match e {
        DefError::UnknownField(_) => (RuleCode::UnknownField, None, "the definition record has an unknown field"),
        DefError::MissingField(k) => (RuleCode::MissingField, p(k), "the definition record lacks a required field"),
        DefError::BadType { field, .. } => (RuleCode::BadType, p(field), "a field has the wrong JSON type"),
        DefError::BadEnum { field, .. } => (RuleCode::BadEnum, p(field), "a field has an unknown value"),
        DefError::BadId(_) => (RuleCode::BadId, p("id"), "the id is not a valid command name"),
        DefError::BadAlgorithm(_) => (RuleCode::BadAlgorithm, p("algorithms"), "an algorithm ID is malformed"),
        DefError::BadPrecondition(_) => (RuleCode::BadPrecondition, p("preconditions"), "a precondition is malformed"),
        DefError::WebMissing => (RuleCode::MissingField, p("web"), "web is required"),
        DefError::WebNoteMissing => {
            (RuleCode::WebNoteMissing, p("web_note"), "web is partial or unavailable but web_note is missing or empty")
        }
        DefError::Json(_) => (RuleCode::BadType, None, "the definition record is not valid JSON"),
    }
}

// ------------------------------------------------------------------ the walk

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The declaration object.
    Decl,
    /// A schema position: an object or a boolean.
    Schema,
    /// An object whose values are schemas, keyed by names (`properties`, `$defs`, ...).
    SchemaMap,
    /// `patternProperties`: values are schemas, keys are regexes (case matters).
    PatternMap,
    /// An array of schemas (`allOf`, `prefixItems`, ...).
    SchemaList,
    /// Anything else.
    Data,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Seg<'a> {
    Key(&'a str),
    Idx(usize),
}

enum Kids<'a> {
    Obj(btree_map::Iter<'a, String, CValue>),
    Arr(std::iter::Enumerate<std::slice::Iter<'a, CValue>>),
}

struct Frame<'a> {
    kind: Kind,
    depth: usize,
    kids: Kids<'a>,
}

/// A display-safe JSON pointer: RFC 6901 (`~0`, `~1`) for printable ASCII keys, with every
/// other character written as `\u{..}`. Each segment is escaped first and then cut to 128
/// bytes, the whole to 512; a cut never splits an escape (`~0`, `~1`, `\u{..}`).
pub(crate) fn pointer(path: &[Seg<'_>]) -> String {
    let mut out = String::new();
    for seg in path {
        if out.len() >= KEEP_POINTER {
            break;
        }
        out.push('/');
        let seg_start = out.len();
        let fits = |out: &String, add: usize| out.len() - seg_start + add <= KEEP && out.len() + add <= KEEP_POINTER;
        match seg {
            Seg::Key(k) => {
                for c in k.chars() {
                    let token = match c {
                        '~' => "~0".to_string(),
                        '/' => "~1".to_string(),
                        ' '..='~' => c.to_string(),
                        _ => format!("\\u{{{:x}}}", u32::from(c)),
                    };
                    if !fits(&out, token.len()) {
                        break;
                    }
                    out.push_str(&token);
                }
                // The 512-byte budget ran out before any of a non-empty key fit: stop here.
                if out.len() == seg_start && !k.is_empty() {
                    out.pop();
                    break;
                }
            }
            Seg::Idx(i) => {
                let token = i.to_string();
                if !fits(&out, token.len()) {
                    out.pop();
                    break;
                }
                out.push_str(&token);
            }
        }
    }
    out
}

fn child_kind(parent: Kind, seg: Seg<'_>, child: &CValue) -> Kind {
    match (parent, seg) {
        (Kind::Decl, Seg::Key("params_schema" | "result_schema")) if matches!(child, CValue::Obj(_)) => Kind::Schema,
        (Kind::Schema, Seg::Key(k)) => match k {
            "properties" | "$defs" | "definitions" | "dependentSchemas" => Kind::SchemaMap,
            "patternProperties" => Kind::PatternMap,
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => Kind::SchemaList,
            "items" if matches!(child, CValue::Arr(_)) => Kind::SchemaList,
            "items"
            | "additionalItems"
            | "additionalProperties"
            | "propertyNames"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "unevaluatedItems"
            | "unevaluatedProperties"
            | "contentSchema" => Kind::Schema,
            _ => Kind::Data,
        },
        (Kind::SchemaMap | Kind::PatternMap | Kind::SchemaList, _) => Kind::Schema,
        _ => Kind::Data,
    }
}

/// Follows an RFC 6901 pointer (`/a/b`, `~1` and `~0` unescaped) from a schema root and
/// returns the kind the walk gives the target, or `None` when it does not exist.
fn resolve(root: &CValue, target: &str) -> Option<Kind> {
    let (mut cur, mut kind) = (root, Kind::Schema);
    for raw in target.split('/').skip(1) {
        let key = raw.replace("~1", "/").replace("~0", "~");
        let (seg, child) = match cur {
            CValue::Obj(m) => (Seg::Key(key.as_str()), m.get(&key)?),
            CValue::Arr(a) => {
                let canonical = key == "0" || (!key.starts_with('0') && key.bytes().all(|b| b.is_ascii_digit()));
                let i: usize = if canonical { key.parse().ok()? } else { return None };
                (Seg::Idx(i), a.get(i)?)
            }
            _ => return None,
        };
        kind = child_kind(kind, seg, child);
        cur = child;
    }
    Some(kind)
}

/// Pre-order walk of a declaration with an explicit stack.
fn walk(ctx: &mut Ctx<'_>, root: &BTreeMap<String, CValue>) {
    let mut path: Vec<Seg<'_>> = Vec::new();
    case_check(ctx, root, &path);
    let mut stack: Vec<Frame<'_>> = vec![Frame { kind: Kind::Decl, depth: 0, kids: Kids::Obj(root.iter()) }];
    while let Some(top) = stack.last_mut() {
        let (parent, depth) = (top.kind, top.depth);
        let next = match &mut top.kids {
            Kids::Obj(it) => it.next().map(|(k, v)| (Seg::Key(k.as_str()), v)),
            Kids::Arr(it) => it.next().map(|(i, v)| (Seg::Idx(i), v)),
        };
        let Some((seg, child)) = next else {
            stack.pop();
            if !stack.is_empty() {
                path.pop();
            }
            continue;
        };
        let kind = child_kind(parent, seg, child);
        path.push(seg);
        visit(ctx, child, kind, &path);
        let kids = match child {
            CValue::Obj(m) => Some(Kids::Obj(m.iter())),
            CValue::Arr(a) => Some(Kids::Arr(a.iter().enumerate())),
            _ => None,
        };
        match kids {
            Some(_) if depth + 1 > ctx.max_depth => {
                let field = match path.first() {
                    Some(Seg::Key(k)) => (*k).to_string(),
                    _ => String::new(),
                };
                if ctx.deep.insert(field.clone()) {
                    let msg = format!("{} is nested deeper than {} levels", quote(&field), ctx.max_depth);
                    ctx.at(RuleCode::SchemaTooDeep, &field, msg);
                }
                path.pop();
            }
            Some(kids) => stack.push(Frame { kind, depth: depth + 1, kids }),
            None => {
                path.pop();
            }
        }
    }
}

/// 2^53: integers up to this magnitude convert to `f64` exactly.
const EXACT_INT: u64 = 1 << 53;

/// A numeric value as `f64`. `I64`/`U64` beyond 2^53 in magnitude would round, so they are
/// not numbers here (the caller reports them); nothing is converted inexactly.
pub(crate) fn number(v: &CValue) -> Option<f64> {
    match v {
        CValue::Num(n) | CValue::F64Bits(n) => Some(*n),
        CValue::I64(i) if i.unsigned_abs() <= EXACT_INT => Some(*i as f64),
        CValue::U64(u) if *u <= EXACT_INT => Some(*u as f64),
        _ => None,
    }
}

fn visit(ctx: &mut Ctx<'_>, v: &CValue, kind: Kind, path: &[Seg<'_>]) {
    if let CValue::Num(n) | CValue::F64Bits(n) = v {
        if !n.is_finite() {
            ctx.push(RuleCode::BadNumber, Some(pointer(path)), "the number is not finite".to_string());
        } else if n.is_subnormal() {
            ctx.push(RuleCode::BadNumber, Some(pointer(path)), format!("the number {n:e} is subnormal"));
        }
    }
    let bad_schema = |ctx: &mut Ctx<'_>, why: &str| ctx.push(RuleCode::BadSchema, Some(pointer(path)), why.to_string());
    match (kind, v) {
        (Kind::Schema, CValue::Obj(m)) => {
            case_check(ctx, m, path);
            schema_checks(ctx, m, path);
        }
        (Kind::Schema, CValue::Bool(_)) => {}
        (Kind::Schema, _) => bad_schema(ctx, "a schema must be an object or a boolean"),
        (Kind::SchemaMap, CValue::Obj(m)) => {
            case_check(ctx, m, path);
            name_check(ctx, m, path);
        }
        (Kind::PatternMap, CValue::Obj(_)) => {}
        (Kind::SchemaMap | Kind::PatternMap, _) => bad_schema(ctx, "must be an object of schemas"),
        (Kind::SchemaList, CValue::Arr(a)) if !a.is_empty() => {}
        (Kind::SchemaList, _) => bad_schema(ctx, "must be a non-empty array of schemas"),
        (Kind::Decl | Kind::Data, CValue::Obj(m)) => case_check(ctx, m, path),
        _ => {}
    }
}

/// Names under `properties`, `$defs`, `definitions` and `dependentSchemas` are non-empty
/// and printable ASCII without space (U+0021..=U+007E): no control, format, invisible or
/// look-alike characters.
fn name_check(ctx: &mut Ctx<'_>, m: &BTreeMap<String, CValue>, path: &[Seg<'_>]) {
    for k in m.keys() {
        check_name(ctx, k, pointer(path));
    }
}

/// One property name (see [`name_check`]); also used for `required` entries and the keys
/// and entries of `dependentRequired`.
fn check_name(ctx: &mut Ctx<'_>, name: &str, field: String) {
    if name.is_empty() || !name.bytes().all(|b| (b'!'..=b'~').contains(&b)) {
        let msg = format!("name {} is not printable ASCII without space", quote(name));
        ctx.push(RuleCode::BadPropertyName, Some(field), msg);
    }
}

/// The only `$schema` accepted, and only at a schema root.
pub const SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Keywords of earlier drafts whose subschemas the walk would not check.
pub(crate) const OLD_DRAFT_KEYWORDS: [&str; 3] = ["$recursiveAnchor", "$recursiveRef", "dependencies"];

/// An array of distinct strings, each a valid name; anything else is `bad_schema`.
fn name_list(ctx: &mut Ctx<'_>, v: &CValue, field: String, what: &str) {
    let mut seen = BTreeSet::new();
    let ok = match v {
        CValue::Arr(items) => items.iter().all(|it| matches!(it, CValue::Str(s) if seen.insert(s.as_str()))),
        _ => false,
    };
    if !ok {
        ctx.push(RuleCode::BadSchema, Some(field), format!("{what} must be an array of distinct strings"));
        return;
    }
    for name in seen {
        check_name(ctx, name, field.clone());
    }
}

/// Keys of one object that are equal after ASCII lowercasing.
fn case_check(ctx: &mut Ctx<'_>, m: &BTreeMap<String, CValue>, path: &[Seg<'_>]) {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for k in m.keys() {
        let low = k.to_ascii_lowercase();
        if let Some(prev) = seen.get(&low) {
            let msg = format!("keys {} and {} differ only in ASCII case", quote(prev), quote(k));
            ctx.push(RuleCode::DuplicateKey, Some(pointer(path)), msg);
        } else {
            seen.insert(low, k);
        }
    }
}

fn schema_checks(ctx: &mut Ctx<'_>, m: &BTreeMap<String, CValue>, path: &[Seg<'_>]) {
    let kp = |k: &'static str| {
        let mut p = path.to_vec();
        p.push(Seg::Key(k));
        pointer(&p)
    };

    // `type`, and whether the schema admits JSON numbers.
    let mut numeric = false;
    let type_ok = match m.get("type") {
        None => true,
        Some(CValue::Str(s)) => {
            numeric = s == "number";
            TYPES.contains(&s.as_str())
        }
        Some(CValue::Arr(items)) => {
            let mut seen = BTreeSet::new();
            let mut ok = !items.is_empty();
            for it in items {
                match it {
                    CValue::Str(s) if TYPES.contains(&s.as_str()) && seen.insert(s.as_str()) => {
                        numeric |= s == "number";
                    }
                    _ => ok = false,
                }
            }
            ok
        }
        Some(_) => false,
    };
    if !type_ok {
        let msg = format!("type must be one of {} or a non-empty array of distinct ones", TYPES.join(", "));
        ctx.push(RuleCode::BadSchema, Some(kp("type")), msg);
    }

    // REQ-005: x-quantity on every number field, naming a catalogue kind.
    let kind = match m.get("x-quantity") {
        None => None,
        Some(CValue::Str(q)) if ctx.kinds.has_kind(q) => Some(q.as_str()),
        Some(CValue::Str(q)) => {
            let msg = format!("x-quantity {} is not a kind of the unit catalogue", quote(q));
            ctx.push(RuleCode::UnknownQuantity, Some(kp("x-quantity")), msg);
            None
        }
        Some(_) => {
            ctx.push(RuleCode::BadSchema, Some(kp("x-quantity")), "x-quantity must be a string".to_string());
            None
        }
    };
    if numeric && !m.contains_key("x-quantity") {
        let msg = "a field of type number has no x-quantity".to_string();
        ctx.push(RuleCode::MissingQuantity, Some(pointer(path)), msg);
    }
    match m.get("x-unit") {
        None => {}
        Some(CValue::Str(u)) => match kind {
            Some(k) if ctx.kinds.accepts(k, u) => {}
            Some(k) => {
                let msg = format!("x-unit {} is not an accepted unit of {k}", quote(u));
                ctx.push(RuleCode::UnitNotAccepted, Some(kp("x-unit")), msg);
            }
            // An invalid x-quantity is already reported.
            None if m.contains_key("x-quantity") => {}
            None => {
                let msg = "x-unit needs a sibling x-quantity".to_string();
                ctx.push(RuleCode::UnitNotAccepted, Some(kp("x-unit")), msg);
            }
        },
        Some(_) => ctx.push(RuleCode::BadSchema, Some(kp("x-unit")), "x-unit must be a string".to_string()),
    }

    // References: only `#` or `#/<pointer>` inside the same schema; the pointer is resolved
    // after the walk and must land on a position the walk checks as a schema.
    match m.get("$ref") {
        None => {}
        Some(CValue::Str(r)) if !r.starts_with('#') => {
            let msg = format!("$ref {} leaves the schema", quote(r));
            ctx.push(RuleCode::ExternalRef, Some(kp("$ref")), msg);
        }
        Some(CValue::Str(r)) if r == "#" => {}
        Some(CValue::Str(r)) if r.starts_with("#/") && !r.contains('%') => {
            let root = match path.first() {
                Some(Seg::Key(k)) => (*k).to_string(),
                _ => String::new(),
            };
            let target = r.get(1..).unwrap_or_default().to_string();
            ctx.refs.push(PendingRef { root, target, at: kp("$ref") });
        }
        Some(CValue::Str(r)) => {
            let msg = format!("$ref {} is a plain-name or percent-encoded reference", quote(r));
            ctx.push(RuleCode::UnwalkedRef, Some(kp("$ref")), msg);
        }
        Some(_) => ctx.push(RuleCode::BadSchema, Some(kp("$ref")), "$ref must be a string".to_string()),
    }
    if m.contains_key("$dynamicRef") {
        let msg = "$dynamicRef is not supported: its target is resolved at evaluation time".to_string();
        ctx.push(RuleCode::UnwalkedRef, Some(kp("$dynamicRef")), msg);
    }
    if path.len() > 1 && m.contains_key("$id") {
        let msg = "an embedded $id changes how references resolve".to_string();
        ctx.push(RuleCode::UnwalkedRef, Some(kp("$id")), msg);
    }

    // Numeric bounds: numbers, a positive multipleOf, and a non-empty range.
    let bound = |ctx: &mut Ctx<'_>, k: &'static str| match m.get(k) {
        None => None,
        Some(v) => match number(v) {
            Some(n) => Some(n),
            None => {
                ctx.push(
                    RuleCode::BadSchema,
                    Some(kp(k)),
                    format!("{k} must be a number (integers at most 2^53 in magnitude)"),
                );
                None
            }
        },
    };
    let lo = bound(ctx, "minimum");
    let xlo = bound(ctx, "exclusiveMinimum");
    let hi = bound(ctx, "maximum");
    let xhi = bound(ctx, "exclusiveMaximum");
    if let Some(n) = bound(ctx, "multipleOf") {
        if n <= 0.0 {
            ctx.push(RuleCode::BadSchema, Some(kp("multipleOf")), format!("multipleOf {n} must be positive"));
        }
    }
    let empty = [
        (lo, "minimum", hi, "maximum", false),
        (xlo, "exclusiveMinimum", hi, "maximum", true),
        (lo, "minimum", xhi, "exclusiveMaximum", true),
        (xlo, "exclusiveMinimum", xhi, "exclusiveMaximum", true),
    ];
    for (a, ak, b, bk, strict) in empty {
        if let (Some(a), Some(b)) = (a, b) {
            if a > b || (strict && a >= b) {
                ctx.push(RuleCode::BadSchema, Some(pointer(path)), format!("empty range: {ak} {a}, {bk} {b}"));
            }
        }
    }

    // Count keywords: integers 0..=2^53, min not above max.
    let count = |ctx: &mut Ctx<'_>, k: &'static str| match m.get(k) {
        None => None,
        Some(v) => match number(v) {
            Some(n) if n.fract() == 0.0 && (0.0..=MAX_COUNT).contains(&n) => Some(n),
            _ => {
                ctx.push(RuleCode::BadSchema, Some(kp(k)), format!("{k} must be an integer 0..=2^53"));
                None
            }
        },
    };
    for (a, b) in [
        ("minLength", "maxLength"),
        ("minItems", "maxItems"),
        ("minProperties", "maxProperties"),
        ("minContains", "maxContains"),
    ] {
        if let (Some(x), Some(y)) = (count(ctx, a), count(ctx, b)) {
            if x > y {
                ctx.push(RuleCode::BadSchema, Some(pointer(path)), format!("empty range: {a} {x}, {b} {y}"));
            }
        }
    }

    // Names: `required` entries, and `dependentRequired` keys and entries.
    if let Some(v) = m.get("required") {
        name_list(ctx, v, kp("required"), "required");
    }
    match m.get("dependentRequired") {
        None => {}
        Some(CValue::Obj(dr)) => {
            for (k, v) in dr {
                check_name(ctx, k, kp("dependentRequired"));
                let mut p = path.to_vec();
                p.push(Seg::Key("dependentRequired"));
                p.push(Seg::Key(k));
                name_list(ctx, v, pointer(&p), "a dependentRequired value");
            }
        }
        Some(_) => {
            let msg = "dependentRequired must be an object of name arrays".to_string();
            ctx.push(RuleCode::BadSchema, Some(kp("dependentRequired")), msg);
        }
    }

    // Dialect: `$schema` only at a schema root and only 2020-12; no keywords of older drafts
    // whose subschemas (and number fields) the walk would not see.
    match m.get("$schema") {
        None => {}
        Some(CValue::Str(u)) if path.len() == 1 && u == SCHEMA_DIALECT => {}
        Some(CValue::Str(u)) if path.len() == 1 => {
            let msg = format!("$schema {} is not {SCHEMA_DIALECT}", quote(u));
            ctx.push(RuleCode::BadDialect, Some(kp("$schema")), msg);
        }
        Some(CValue::Str(_)) => {
            let msg = "$schema is accepted only at the schema root".to_string();
            ctx.push(RuleCode::BadDialect, Some(kp("$schema")), msg);
        }
        Some(_) => ctx.push(RuleCode::BadDialect, Some(kp("$schema")), "$schema must be a string".to_string()),
    }
    for k in OLD_DRAFT_KEYWORDS {
        if m.contains_key(k) {
            let msg = format!("{k} belongs to an earlier JSON Schema draft and is not supported");
            ctx.push(RuleCode::BadDialect, Some(kp(k)), msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_codes_are_sorted_and_unique() {
        let codes: Vec<&str> = RuleCode::ALL.iter().map(|c| c.as_str()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn id_syntax() {
        for ok in ["a.b", "align.run_bestfit", "a1.b_2", "x.y__"] {
            assert!(is_valid_id(ok), "{ok}");
        }
        for bad in ["", "a", "a.", ".b", "A.b", "a.B", "a_b.c", "1a.b", "a._b", "a.1b", "a.b.c", "a.b@1", "a.b-c"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn tool_names() {
        assert_eq!(tool_name("align.run_bestfit", 1), "align_run_bestfit");
        assert_eq!(tool_name("align.run_bestfit", 2), "align_run_bestfit_v2");
    }

    #[test]
    fn pointer_escapes_and_cuts() {
        assert_eq!(pointer(&[Seg::Key("a/b"), Seg::Key("c~d"), Seg::Idx(3)]), "/a~1b/c~0d/3");
        let long = "k".repeat(1000);
        let segs: Vec<Seg<'_>> = (0..10).map(|_| Seg::Key(long.as_str())).collect();
        assert!(pointer(&segs).len() <= KEEP_POINTER);
    }

    #[test]
    fn literal_underflow() {
        assert!(check_literal(b"1e-400").is_err());
        assert!(check_literal(b"4e-320").is_err());
        assert!(check_literal(b"0e-400").is_ok());
        assert!(check_literal(b"-0.0").is_ok());
        assert!(check_literal(b"1e400").is_err());
        assert!(check_literal(b"2.5e-3").is_ok());
    }

    /// Follow-up of #233, item 2: literals above 2^53 in any notation, decided exactly.
    #[test]
    fn literal_above_2_53() {
        for big in [
            "9007199254740993",
            "9007199254740993.0",
            "9007199254740993e0",
            "9007199254740993E+0",
            "-9007199254740993.0",
            "900719925474099.3e1",
            "90071992547409930e-1",
            "0.9007199254740993e16",
            "9007199254740992.5",
            "9007199254740992.0000000000000001",
            "9007199254740994.0",
            "10000000000000000.0",
            "1e16",
            "1e300",
        ] {
            assert!(above_2_53(big), "{big}");
            assert!(check_literal(big.as_bytes()).is_err(), "{big}");
        }
        for ok in [
            "9007199254740992",
            "9007199254740992.0",
            "9007199254740992.000",
            "9007199254740992e0",
            "-9007199254740992.0",
            "900719925474099.2e1",
            "90071992547409920e-1",
            "0.9007199254740992e16",
            "1e15",
            "0",
            "-0.0",
            "0e999999999999999999999",
            "0.5",
            "00009007199254740992",
            "1.5e-300",
        ] {
            assert!(!above_2_53(ok), "{ok}");
            assert!(check_literal(ok.as_bytes()).is_ok(), "{ok}");
        }
    }

    /// Follow-up of #233, item 4: the segment cut applies to the escaped form.
    #[test]
    fn pointer_cuts_escaped_segments() {
        let zw = "\u{200b}".repeat(100);
        let p = pointer(&[Seg::Key(&zw)]);
        assert_eq!(p, format!("/{}", "\\u{200b}".repeat(16)));
        let tildes = "~".repeat(200);
        assert_eq!(pointer(&[Seg::Key(&tildes)]), format!("/{}", "~0".repeat(64)));
        let mixed = format!("{}\u{e9}", "a".repeat(125));
        assert_eq!(pointer(&[Seg::Key(&mixed)]), format!("/{}", "a".repeat(125)));
        let segs: Vec<Seg<'_>> = (0..10).map(|_| Seg::Key(zw.as_str())).collect();
        let whole = pointer(&segs);
        assert!(whole.len() <= KEEP_POINTER && whole.ends_with('}'), "{whole}");
    }
}
