//! Part-family parameters and binding validation. Spec node `AUTO.PROG.FAMILY`.
//!
//! One program serves a family of similar parts: it declares typed parameters
//! ([`ParamDecl`], the objects of the program's `params` array) and may carry a table of
//! named variants ([`Variant`], the objects of the optional `variants` array), each a
//! complete binding. [`check_binding`] validates a binding against the declarations before
//! a run. Quantity kinds and units come from the registry's quantity catalogue
//! ([`QuantityCatalogue`], `CORE.REG.DEF.SCHEMA`); this module defines no unit and converts
//! nothing: a bound quantity keeps its authored decimal text and unit symbol
//! (REQ-AUTO.PROG.FAMILY-006).
//!
//! Every choice below is pinned in `spec/auto/prog/family/NODE.md` (Design).
//!
//! Wire forms (inside the canonical program document of `AUTO.PROG.FMT`):
//!
//! ```text
//! param:   {"default"?: <value>, "description"?: "<text>", "name": "<ident>",
//!           "quantity_kind"?: "<kind>", "required": <bool>, "type": "<type>"}
//! variant: {"binding": {"<param>": <value>, ...}, "name": "<variant name>"}
//! value:   true | 4 | "text" | {"unit": "<symbol>", "value": "<decimal text>"}
//! ```

use crate::fmt::{Opaque, Program};
use fid_canon::{parse_with_limits, CValue};
use fid_registry::{EntityType, QuantityCatalogue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Most parameter declarations in one program.
pub const MAX_PARAMS: usize = 1024;
/// Most variants in one program.
pub const MAX_VARIANTS: usize = 1024;
/// Longest parameter name, in bytes (the `AUTO.PROG.FMT` identifier pattern).
pub const MAX_NAME: usize = 64;
/// Longest variant name, in bytes.
pub const MAX_VARIANT_NAME: usize = 64;
/// Longest parameter description, in bytes.
pub const MAX_DESCRIPTION: usize = 4096;
/// Longest element reference or data handle, in bytes.
pub const MAX_HANDLE: usize = 4096;
/// Longest quantity decimal text, in bytes (`GEO.FRAME.UNITS` `MAX_TEXT_LEN`).
pub const MAX_DECIMAL_TEXT: usize = 1024;
/// Most significant digits of a quantity decimal text (`GEO.FRAME.UNITS` `MAX_SIG_DIGITS`).
pub const MAX_SIG_DIGITS: usize = 40;
/// Largest binding document accepted by [`binding_from_json`], in bytes.
pub const MAX_BINDING_BYTES: usize = 1024 * 1024;
/// Deepest nesting accepted by [`binding_from_json`].
pub const MAX_BINDING_DEPTH: usize = 16;
/// Largest magnitude an `integer` value may have: 2^53 - 1 (JavaScript's `MAX_SAFE_INTEGER`).
/// JSON numbers are parsed to `f64`, so integer text at 2^53 or above may already have been
/// rounded (`9007199254740993` parses to 2^53); refusing 2^53 itself keeps every accepted
/// value exactly the integer that was written.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Field names of a parameter declaration, sorted.
pub const DECL_FIELDS: [&str; 6] = ["default", "description", "name", "quantity_kind", "required", "type"];
/// Field names of a variant, sorted.
pub const VARIANT_FIELDS: [&str; 2] = ["binding", "name"];

// ------------------------------------------------------------------ types

/// A parameter type. The names follow the registry's parameter schemas (`CORE.REG.DEF.SCHEMA`):
/// the JSON Schema types `boolean`, `integer`, `number` and `string`; `quantity` for a
/// number carrying `x-quantity` (here with any unit of the kind); `ref:<entity>` for a string
/// carrying `x-ref` with a registry [`EntityType`]; and `data_handle` for an input data handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamType {
    Boolean,
    Integer,
    Number,
    String,
    Quantity,
    Ref(EntityType),
    DataHandle,
}

impl ParamType {
    /// The type with this exact wire name, or `None`.
    pub fn parse(name: &str) -> Option<ParamType> {
        Some(match name {
            "boolean" => ParamType::Boolean,
            "integer" => ParamType::Integer,
            "number" => ParamType::Number,
            "string" => ParamType::String,
            "quantity" => ParamType::Quantity,
            "data_handle" => ParamType::DataHandle,
            _ => return name.strip_prefix("ref:").and_then(EntityType::parse).map(ParamType::Ref),
        })
    }

    /// The wire name.
    pub fn name(self) -> String {
        match self {
            ParamType::Boolean => "boolean".into(),
            ParamType::Integer => "integer".into(),
            ParamType::Number => "number".into(),
            ParamType::String => "string".into(),
            ParamType::Quantity => "quantity".into(),
            ParamType::DataHandle => "data_handle".into(),
            ParamType::Ref(e) => format!("ref:{}", e.as_str()),
        }
    }
}

impl fmt::Display for ParamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// A bound value, classified by JSON shape only; [`check_binding`] decides whether it fits
/// a declaration. Classification never fails, so a value of the wrong shape is reported as
/// `wrong_type` against the parameter it is bound to.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundValue {
    /// A JSON boolean.
    Bool(bool),
    /// A JSON number.
    Number(f64),
    /// A JSON string.
    Text(String),
    /// An object with exactly the string fields `unit` and `value`: an authored quantity,
    /// kept as written.
    Quantity { value: String, unit: String },
    /// Anything else (null, arrays, other objects, and the `fid-canon` integer and bit-exact
    /// forms, which JSON text never parses to). Never fits a declaration.
    Other(CValue),
}

impl BoundValue {
    /// Classifies a JSON value.
    pub fn from_value(v: CValue) -> BoundValue {
        match v {
            CValue::Bool(b) => BoundValue::Bool(b),
            CValue::Num(x) => BoundValue::Number(x),
            CValue::Str(s) => BoundValue::Text(s),
            CValue::Obj(m) => match quantity_parts(&m) {
                Some((value, unit)) => BoundValue::Quantity { value, unit },
                None => BoundValue::Other(CValue::Obj(m)),
            },
            other => BoundValue::Other(other),
        }
    }

    /// The JSON value, exactly as classified (a quantity keeps its text and unit).
    pub fn to_value(&self) -> CValue {
        match self {
            BoundValue::Bool(b) => CValue::Bool(*b),
            BoundValue::Number(x) => CValue::Num(*x),
            BoundValue::Text(s) => CValue::Str(s.clone()),
            BoundValue::Quantity { value, unit } => {
                let mut m = BTreeMap::new();
                m.insert("unit".to_string(), CValue::Str(unit.clone()));
                m.insert("value".to_string(), CValue::Str(value.clone()));
                CValue::Obj(m)
            }
            BoundValue::Other(v) => v.clone(),
        }
    }
}

fn quantity_parts(m: &BTreeMap<String, CValue>) -> Option<(String, String)> {
    if m.len() != 2 {
        return None;
    }
    match (m.get("value"), m.get("unit")) {
        (Some(CValue::Str(v)), Some(CValue::Str(u))) => Some((v.clone(), u.clone())),
        _ => None,
    }
}

/// A binding: parameter name to bound value, in byte order of the names.
pub type Binding = BTreeMap<String, BoundValue>;

/// One parameter declaration (REQ-AUTO.PROG.FAMILY-001).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    pub ty: ParamType,
    /// Present exactly when `ty` is [`ParamType::Quantity`]: a kind of the unit catalogue.
    pub quantity_kind: Option<String>,
    /// A required parameter without a default must be bound.
    pub required: bool,
    /// Used when the binding omits the parameter; must itself fit the declaration.
    pub default: Option<BoundValue>,
    pub description: Option<String>,
}

impl ParamDecl {
    /// The wire object (absent options are omitted).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), CValue::Str(self.name.clone()));
        m.insert("type".to_string(), CValue::Str(self.ty.name()));
        m.insert("required".to_string(), CValue::Bool(self.required));
        if let Some(k) = &self.quantity_kind {
            m.insert("quantity_kind".to_string(), CValue::Str(k.clone()));
        }
        if let Some(d) = &self.default {
            m.insert("default".to_string(), d.to_value());
        }
        if let Some(d) = &self.description {
            m.insert("description".to_string(), CValue::Str(d.clone()));
        }
        CValue::Obj(m)
    }
}

/// One named variant: a complete binding.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    pub binding: Binding,
}

impl Variant {
    /// The wire object.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), CValue::Str(self.name.clone()));
        m.insert("binding".to_string(), binding_to_value(&self.binding));
        CValue::Obj(m)
    }
}

// ------------------------------------------------------------------ errors

/// Binding and declaration errors. Each names the parameter or variant it is about.
#[derive(Debug, Clone, PartialEq)]
pub enum BindingError {
    /// A required parameter without a default is not bound (REQ-002).
    MissingParameter { param: String },
    /// The bound value is not of the declared type (REQ-003). Also a quantity whose value
    /// text is not a decimal of the `GEO.FRAME.UNITS` grammar.
    WrongType { param: String, expected: ParamType },
    /// The quantity's unit is not a unit of the declared kind in the catalogue (REQ-003).
    WrongUnit { param: String, unit: String, kind: String },
    /// The binding names a parameter the program does not declare (REQ-004). Names are
    /// case-sensitive, so a case variant of a declared name is unknown.
    UnknownParameter { param: String },
    /// Two declarations share a name, compared ASCII-case-insensitively; names the later one.
    DuplicateParameter { index: usize, param: String },
    /// A declaration is malformed. `param` is the declared name when it is a string.
    BadDeclaration { index: usize, param: Option<String>, reason: &'static str },
    /// Two variants share a name, compared ASCII-case-insensitively; names the later one (REQ-005).
    DuplicateVariant { index: usize, variant: String },
    /// A variant is malformed. `variant` is its name when that is a string.
    BadVariant { index: usize, variant: Option<String>, reason: &'static str },
    /// An error of a variant's own binding (REQ-005).
    InVariant { variant: String, error: Box<BindingError> },
    /// No variant has this exact name.
    UnknownVariant { variant: String },
    /// A binding document is not a bounded JSON object.
    BadBinding { reason: &'static str },
    /// More than [`MAX_PARAMS`] declarations or [`MAX_VARIANTS`] variants.
    FamilyTooLarge { what: &'static str },
}

impl BindingError {
    /// Stable machine-readable code; an [`BindingError::InVariant`] has its inner code.
    pub fn code(&self) -> &'static str {
        match self {
            BindingError::MissingParameter { .. } => "missing_parameter",
            BindingError::WrongType { .. } => "wrong_type",
            BindingError::WrongUnit { .. } => "wrong_unit",
            BindingError::UnknownParameter { .. } => "unknown_parameter",
            BindingError::DuplicateParameter { .. } => "duplicate_parameter",
            BindingError::BadDeclaration { .. } => "bad_declaration",
            BindingError::DuplicateVariant { .. } => "duplicate_variant",
            BindingError::BadVariant { .. } => "bad_variant",
            BindingError::InVariant { error, .. } => error.code(),
            BindingError::UnknownVariant { .. } => "unknown_variant",
            BindingError::BadBinding { .. } => "bad_binding",
            BindingError::FamilyTooLarge { .. } => "family_too_large",
        }
    }

    /// The parameter the error is about, if any.
    pub fn param(&self) -> Option<&str> {
        match self {
            BindingError::MissingParameter { param }
            | BindingError::WrongType { param, .. }
            | BindingError::WrongUnit { param, .. }
            | BindingError::UnknownParameter { param }
            | BindingError::DuplicateParameter { param, .. } => Some(param),
            BindingError::BadDeclaration { param, .. } => param.as_deref(),
            BindingError::InVariant { error, .. } => error.param(),
            _ => None,
        }
    }

    /// The variant the error is about, if any.
    pub fn variant(&self) -> Option<&str> {
        match self {
            BindingError::DuplicateVariant { variant, .. }
            | BindingError::InVariant { variant, .. }
            | BindingError::UnknownVariant { variant } => Some(variant),
            BindingError::BadVariant { variant, .. } => variant.as_deref(),
            _ => None,
        }
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

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            BindingError::MissingParameter { param } | BindingError::UnknownParameter { param } => {
                write!(f, "{code}: {}", shown(param))
            }
            BindingError::WrongType { param, expected } => write!(f, "{code}: {} must be {expected}", shown(param)),
            BindingError::WrongUnit { param, unit, kind } => {
                write!(f, "{code}: {} has unit {}, not a unit of {}", shown(param), shown(unit), shown(kind))
            }
            BindingError::DuplicateParameter { index, param } => {
                write!(f, "{code}: {} at params/{index}", shown(param))
            }
            BindingError::BadDeclaration { index, reason, .. } => write!(f, "{code}: params/{index}: {reason}"),
            BindingError::DuplicateVariant { index, variant } => {
                write!(f, "{code}: {} at variants/{index}", shown(variant))
            }
            BindingError::BadVariant { index, reason, .. } => write!(f, "{code}: variants/{index}: {reason}"),
            BindingError::InVariant { variant, error } => write!(f, "variant {}: {error}", shown(variant)),
            BindingError::UnknownVariant { variant } => write!(f, "{code}: {}", shown(variant)),
            BindingError::BadBinding { reason } => write!(f, "{code}: {reason}"),
            BindingError::FamilyTooLarge { what } => write!(f, "{code}: too many {what}"),
        }
    }
}

impl std::error::Error for BindingError {}

// ------------------------------------------------------------------ lexical rules

/// `^[A-Za-z_][A-Za-z0-9_]{0,63}$`, the `AUTO.PROG.FMT` identifier pattern, so a parameter
/// can be named in an expression.
fn is_ident(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_NAME
        && b.next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`.
fn is_variant_name(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_VARIANT_NAME
        && b.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// A non-empty element reference or data handle of at most [`MAX_HANDLE`] bytes without
/// control characters. FAMILY does not resolve it.
fn is_handle(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_HANDLE && !s.chars().any(char::is_control)
}

/// The authored-decimal grammar of `GEO.FRAME.UNITS` (REQ-GEO.FRAME.UNITS-004), checks 1 to 4:
/// at most [`MAX_DECIMAL_TEXT`] bytes; `[+-] digits [ "." digits ] [ ("e"|"E") [+-] digits ]`;
/// at most [`MAX_SIG_DIGITS`] significant digits (first non-zero digit to last digit written).
/// The overflow check (5) needs a conversion, which FAMILY never performs.
pub fn is_decimal(t: &str) -> bool {
    if t.len() > MAX_DECIMAL_TEXT {
        return false;
    }
    let b = t.as_bytes();
    let digits = |i: &mut usize| -> usize {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int_start = i;
    if digits(&mut i) == 0 {
        return false;
    }
    let int = (int_start, i);
    let mut frac = (i, i);
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        if digits(&mut i) == 0 {
            return false;
        }
        frac = (start, i);
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if i != b.len() {
        return false;
    }
    let mantissa = b.get(int.0..int.1).unwrap_or_default().iter().chain(b.get(frac.0..frac.1).unwrap_or_default());
    mantissa.skip_while(|c| **c == b'0').count() <= MAX_SIG_DIGITS
}

fn is_integer(x: f64) -> bool {
    x.is_finite() && x.fract() == 0.0 && x.abs() <= MAX_SAFE_INTEGER
}

// ------------------------------------------------------------------ checks

/// Whether `v` fits `d`: `None` when it does, else the `wrong_type` or `wrong_unit` error.
fn value_error(d: &ParamDecl, v: &BoundValue, cat: &dyn QuantityCatalogue) -> Option<BindingError> {
    let fits = match (d.ty, v) {
        (ParamType::Boolean, BoundValue::Bool(_)) => true,
        (ParamType::Integer, BoundValue::Number(x)) => is_integer(*x),
        (ParamType::Number, BoundValue::Number(x)) => x.is_finite(),
        (ParamType::String, BoundValue::Text(_)) => true,
        (ParamType::Ref(_) | ParamType::DataHandle, BoundValue::Text(s)) => is_handle(s),
        (ParamType::Quantity, BoundValue::Quantity { value, unit }) => {
            if !is_decimal(value) {
                false
            } else {
                let kind = d.quantity_kind.as_deref().unwrap_or_default();
                if cat.accepts(kind, unit) {
                    return None;
                }
                return Some(BindingError::WrongUnit { param: d.name.clone(), unit: unit.clone(), kind: kind.into() });
            }
        }
        _ => false,
    };
    (!fits).then(|| BindingError::WrongType { param: d.name.clone(), expected: d.ty })
}

fn decl_errors<'a>(
    decls: impl Iterator<Item = (usize, &'a ParamDecl)>,
    cat: &dyn QuantityCatalogue,
) -> Vec<BindingError> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, d) in decls {
        let bad = |reason| BindingError::BadDeclaration { index, param: Some(d.name.clone()), reason };
        if !is_ident(&d.name) {
            out.push(bad("name must match ^[A-Za-z_][A-Za-z0-9_]{0,63}$"));
        } else if !seen.insert(d.name.to_ascii_lowercase()) {
            out.push(BindingError::DuplicateParameter { index, param: d.name.clone() });
        }
        let kind_ok = match (d.ty == ParamType::Quantity, d.quantity_kind.as_deref()) {
            (true, None) => {
                out.push(bad("a quantity needs quantity_kind"));
                false
            }
            (true, Some(k)) if cat.kind(k).is_none() => {
                out.push(bad("quantity_kind is not a kind of the unit catalogue"));
                false
            }
            (false, Some(_)) => {
                out.push(bad("quantity_kind is only allowed on a quantity"));
                false
            }
            _ => true,
        };
        if d.description.as_ref().is_some_and(|s| s.len() > MAX_DESCRIPTION) {
            out.push(bad("description is longer than 4096 bytes"));
        }
        if let (true, Some(v)) = (kind_ok, &d.default) {
            if value_error(d, v, cat).is_some() {
                out.push(bad("default does not fit the declared type or unit"));
            }
        }
    }
    out
}

/// Checks declarations on their own: names (identifier pattern, unique ignoring ASCII case),
/// `quantity_kind` present exactly on quantities and known to the catalogue, description
/// length, and that every default fits its declaration. Errors in declaration order.
pub fn check_decls(decls: &[ParamDecl], cat: &dyn QuantityCatalogue) -> Vec<BindingError> {
    if decls.len() > MAX_PARAMS {
        return vec![BindingError::FamilyTooLarge { what: "params" }];
    }
    decl_errors(decls.iter().enumerate(), cat)
}

/// Validates a binding against declarations (REQ-AUTO.PROG.FAMILY-002 to -004).
///
/// The declarations are checked first with [`check_decls`]; if they have errors, those are
/// returned alone. Otherwise: for each declaration in order, a bound value must fit it
/// (`wrong_type`, `wrong_unit`), and an unbound required parameter without a default is
/// `missing_parameter`; then each binding key in byte order that no declaration names
/// exactly is `unknown_parameter`. Nothing is converted or filled in.
pub fn check_binding(decls: &[ParamDecl], binding: &Binding, cat: &dyn QuantityCatalogue) -> Vec<BindingError> {
    let mut out = check_decls(decls, cat);
    if !out.is_empty() {
        return out;
    }
    for d in decls {
        match binding.get(&d.name) {
            Some(v) => out.extend(value_error(d, v, cat)),
            None if d.required && d.default.is_none() => {
                out.push(BindingError::MissingParameter { param: d.name.clone() })
            }
            None => {}
        }
    }
    let declared: BTreeSet<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    for k in binding.keys() {
        if !declared.contains(k.as_str()) {
            out.push(BindingError::UnknownParameter { param: k.clone() });
        }
    }
    out
}

// ------------------------------------------------------------------ decoding

/// A binding from a JSON object.
pub fn binding_from_value(v: CValue) -> Result<Binding, BindingError> {
    match v {
        CValue::Obj(m) => Ok(m.into_iter().map(|(k, v)| (k, BoundValue::from_value(v))).collect()),
        _ => Err(BindingError::BadBinding { reason: "a binding must be a JSON object" }),
    }
}

/// A binding from JSON text (a binding file), bounded by [`MAX_BINDING_BYTES`] and
/// [`MAX_BINDING_DEPTH`]; duplicate keys are refused by `fid-canon`.
pub fn binding_from_json(bytes: &[u8]) -> Result<Binding, BindingError> {
    if bytes.len() > MAX_BINDING_BYTES {
        return Err(BindingError::BadBinding { reason: "binding_too_large" });
    }
    let v = parse_with_limits(bytes, MAX_BINDING_BYTES, MAX_BINDING_DEPTH)
        .map_err(|e| BindingError::BadBinding { reason: e.code() })?;
    binding_from_value(v)
}

/// The JSON object of a binding.
pub fn binding_to_value(b: &Binding) -> CValue {
    CValue::Obj(b.iter().map(|(k, v)| (k.clone(), v.to_value())).collect())
}

fn decl_from_obj(index: usize, m: &Opaque) -> Result<ParamDecl, BindingError> {
    let name_hint = match m.get("name") {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let bad = |reason| BindingError::BadDeclaration { index, param: name_hint.clone(), reason };
    if m.keys().any(|k| !DECL_FIELDS.contains(&k.as_str())) {
        return Err(bad("unknown field"));
    }
    let Some(name) = name_hint.clone() else {
        return Err(bad("name must be a string"));
    };
    let ty = match m.get("type") {
        Some(CValue::Str(t)) => ParamType::parse(t).ok_or_else(|| bad("type is not a registry parameter type"))?,
        _ => return Err(bad("type must be a string")),
    };
    let quantity_kind = match m.get("quantity_kind") {
        None => None,
        Some(CValue::Str(k)) => Some(k.clone()),
        Some(_) => return Err(bad("quantity_kind must be a string")),
    };
    let required = match m.get("required") {
        Some(CValue::Bool(b)) => *b,
        _ => return Err(bad("required must be a boolean")),
    };
    let description = match m.get("description") {
        None => None,
        Some(CValue::Str(s)) => Some(s.clone()),
        Some(_) => return Err(bad("description must be a string")),
    };
    let default = m.get("default").cloned().map(BoundValue::from_value);
    Ok(ParamDecl { name, ty, quantity_kind, required, default, description })
}

fn variant_from_obj(index: usize, m: &Opaque) -> Result<Variant, BindingError> {
    let name_hint = match m.get("name") {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let bad = |reason| BindingError::BadVariant { index, variant: name_hint.clone(), reason };
    if m.keys().any(|k| !VARIANT_FIELDS.contains(&k.as_str())) {
        return Err(bad("unknown field"));
    }
    let Some(name) = name_hint.clone() else {
        return Err(bad("name must be a string"));
    };
    if !is_variant_name(&name) {
        return Err(bad("name must match ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$"));
    }
    let binding = match m.get("binding") {
        Some(v @ CValue::Obj(_)) => binding_from_value(v.clone())?,
        _ => return Err(bad("binding must be an object")),
    };
    Ok(Variant { name, binding })
}

// ------------------------------------------------------------------ family

/// A program's validated parameter declarations and variant table.
#[derive(Debug, Clone, PartialEq)]
pub struct Family {
    params: Vec<ParamDecl>,
    variants: Vec<Variant>,
}

impl Family {
    /// The family of a parsed program: its `params` and optional `variants`.
    pub fn from_program(p: &Program, cat: &dyn QuantityCatalogue) -> Result<Family, Vec<BindingError>> {
        Family::from_parts(&p.params, p.variants.as_deref().unwrap_or_default(), cat)
    }

    /// Decodes and checks declarations and variants, failing closed with every error found:
    /// declaration errors in declaration order, then variant errors in variant order. A
    /// variant must be well formed, have a name unique ignoring ASCII case, and its binding
    /// must pass [`check_binding`] (REQ-005); its binding errors come wrapped in
    /// [`BindingError::InVariant`]. Variant bindings are not checked when a declaration is bad.
    pub fn from_parts(
        params: &[Opaque],
        variants: &[Opaque],
        cat: &dyn QuantityCatalogue,
    ) -> Result<Family, Vec<BindingError>> {
        if params.len() > MAX_PARAMS {
            return Err(vec![BindingError::FamilyTooLarge { what: "params" }]);
        }
        if variants.len() > MAX_VARIANTS {
            return Err(vec![BindingError::FamilyTooLarge { what: "variants" }]);
        }
        let mut errors = Vec::new();
        let mut decls = Vec::new();
        for (i, m) in params.iter().enumerate() {
            match decl_from_obj(i, m) {
                Ok(d) => decls.push((i, d)),
                Err(e) => errors.push((i, e)),
            }
        }
        for e in decl_errors(decls.iter().map(|(i, d)| (*i, d)), cat) {
            let i = match &e {
                BindingError::BadDeclaration { index, .. } | BindingError::DuplicateParameter { index, .. } => *index,
                _ => 0,
            };
            errors.push((i, e));
        }
        errors.sort_by_key(|(i, _)| *i);
        let mut errors: Vec<BindingError> = errors.into_iter().map(|(_, e)| e).collect();
        let decls_ok = errors.is_empty();
        let decls: Vec<ParamDecl> = decls.into_iter().map(|(_, d)| d).collect();

        let mut table = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, m) in variants.iter().enumerate() {
            let v = match variant_from_obj(index, m) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            if !seen.insert(v.name.to_ascii_lowercase()) {
                errors.push(BindingError::DuplicateVariant { index, variant: v.name });
                continue;
            }
            if decls_ok {
                for e in check_binding(&decls, &v.binding, cat) {
                    errors.push(BindingError::InVariant { variant: v.name.clone(), error: Box::new(e) });
                }
            }
            table.push(v);
        }
        if errors.is_empty() {
            Ok(Family { params: decls, variants: table })
        } else {
            Err(errors)
        }
    }

    /// The declarations, in program order.
    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    /// The variants, in program order.
    pub fn variants(&self) -> &[Variant] {
        &self.variants
    }

    /// The binding of the variant with exactly this name.
    pub fn variant(&self, name: &str) -> Result<&Binding, BindingError> {
        self.variants
            .iter()
            .find(|v| v.name == name)
            .map(|v| &v.binding)
            .ok_or_else(|| BindingError::UnknownVariant { variant: name.to_string() })
    }

    /// [`check_binding`] against this family's declarations.
    pub fn check(&self, binding: &Binding, cat: &dyn QuantityCatalogue) -> Vec<BindingError> {
        check_binding(&self.params, binding, cat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_grammar() {
        for ok in ["0", "120", "4.724", "-1.5e-3", "+2E+10", "0.000", "1.000"] {
            assert!(is_decimal(ok), "{ok}");
        }
        for bad in ["", "1,5", ".5", "1.", "1e", " 1", "1 ", "nan", "inf", "--1", "0x10", "1.2.3", "١"] {
            assert!(!is_decimal(bad), "{bad}");
        }
        assert!(is_decimal(&"1".repeat(40)));
        assert!(!is_decimal(&"1".repeat(41)));
        assert!(is_decimal(&format!("0.{}1", "0".repeat(100))));
        assert!(!is_decimal(&"0".repeat(MAX_DECIMAL_TEXT + 1)));
    }

    #[test]
    fn names() {
        assert!(is_ident("hole_spacing") && is_ident("_x") && is_ident(&"a".repeat(64)));
        assert!(!is_ident("") && !is_ident("1a") && !is_ident("a-b") && !is_ident(&"a".repeat(65)));
        assert!(is_variant_name("B-120") && is_variant_name("b.4_in"));
        assert!(!is_variant_name("-B") && !is_variant_name("B 120") && !is_variant_name(""));
    }

    #[test]
    fn param_type_names_round_trip() {
        for t in ["boolean", "integer", "number", "string", "quantity", "data_handle", "ref:element", "ref:datum"] {
            assert_eq!(ParamType::parse(t).map(ParamType::name).as_deref(), Some(t));
        }
        for t in ["Integer", "ref:", "ref:Element", "ref:widget", "length", "int"] {
            assert_eq!(ParamType::parse(t), None, "{t}");
        }
    }

    #[test]
    fn integers() {
        assert!(is_integer(4.0) && is_integer(-0.0) && is_integer(MAX_SAFE_INTEGER));
        assert!(!is_integer(4.5) && !is_integer(f64::NAN) && !is_integer(f64::INFINITY));
        assert!(!is_integer(MAX_SAFE_INTEGER + 1.0) && !is_integer(-MAX_SAFE_INTEGER - 1.0));
        assert!(is_integer(-MAX_SAFE_INTEGER));
        assert!(!is_integer(MAX_SAFE_INTEGER * 2.0));
    }
}
