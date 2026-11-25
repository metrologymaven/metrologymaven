//! The `instrument-model/v1` document: reading, validation, unit conversion, canonical form
//! and digest (REQ-UNC.INST.SCHEMA-001, -002, -006).
//!
//! # Wire form
//!
//! ```text
//! { "schema": "instrument-model/v1",
//!   "id": "im:<name>@<semver>",
//!   "kind": "spec_derived" | "iemps" | "observation",
//!   "device_class": "<token>",
//!   "params": [{ "name": "<token>", "unit": "<unit>", "value": "<number>",
//!                "dist"?: { "kind": "<kind>", "params": { "<p>": "<number>", ... } } }, ...],
//!   "range":  [{ "axis": "<token>", "min": "<number>", "max": "<number>", "unit": "<unit>" }, ...],
//!   "source": "<citation>" }
//! ```
//!
//! - Every member is required except `dist`; any other member, at any level, is
//!   `schema_invalid`. `<name>` and `<semver>` follow `fid-kunc` (`[a-z0-9][a-z0-9-]*` of at
//!   most 64 bytes; `MAJOR.MINOR.PATCH`), a subset of what an `instrument-profile/v1`
//!   `um_model.id` accepts. A `<token>` is `[a-z][a-z0-9_]*` of at most 64 bytes.
//! - A `<number>` is a JSON string: a decimal in the JSON number grammar of at most 64 bytes
//!   (`-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]{1,4})?`), or the exact IEEE-754 bit pattern
//!   `0x` + 16 lowercase hex digits. Numbers are strings so that a value is never rounded
//!   by a JSON reader before the unit is applied.
//! - `<unit>` is one of [`UNITS`]. A decimal with a prefixed unit (`um`, `mm`, `km`, `urad`,
//!   `ppm`, ...) is converted to SI by **one correctly rounded parse**: the decimal exponent
//!   is shifted by the prefix's power of ten in the text and the result is parsed once
//!   (`str::parse::<f64>`, correctly rounded), so `"15"` `um` and `"0.015"` `mm` give the same
//!   double. A hex value is exact and is accepted only with an SI unit (no prefix). A value
//!   that is not finite after conversion is refused; zero is stored as `+0`.
//! - `dist` uses `fid-kunc` `dist` ([`Distribution`]). Its parameters are in the parameter's
//!   unit, `nu` and `beta` dimensionless; kinds `normal {mu, sigma}`, `rect {a, b}`,
//!   `tri {a, b, c}`, `arcsine {a, b}`, `scaled_t {mu, s, nu}`, `trap {a, b, beta}`. A
//!   distribution needs an SI unit `fid-kunc` knows (`m`, `rad`, `1`); on a `K` or `s`
//!   parameter it is `schema_invalid` at `/params/<i>/dist`.
//! - `source` is the citation, kept exactly as given and never interpreted (CONSTITUTION
//!   C6): 1 to [`MAX_SOURCE_BYTES`] bytes with no control, bidi or other invisible character
//!   (`fid_types::text`).
//!
//! # Errors (REQ-002)
//!
//! The first defect wins, checked in this order: the byte and depth limits and the JSON
//! itself (`schema_invalid` at `""`); unknown top-level members in sorted order; then
//! `schema`, `id`, `kind`, `device_class`, `source` (absent or empty: `missing_source`),
//! `params` and `range`. Inside `params`, entry by entry: unknown members, `name`
//! (duplicates refused), `unit` (absent or empty: `missing_unit` naming the parameter),
//! `value`, `dist`. Inside `range`, axis by axis: unknown members, `axis` (duplicates
//! refused), `unit`, `min`, `max`, then `min < max` (`invalid_range` naming the axis: an
//! inverted or empty interval). Fields are RFC 6901 JSON pointers into the input. An unknown
//! member's name is untrusted, so the pointer shows it cut to [`MAX_TOKEN_BYTES`] bytes, or as
//! [`UNSHOWN_NAME`] when it is not a token ([`shown_name`]).
//!
//! # Canonical form and digest (REQ-001)
//!
//! The canonical document has the same members with params sorted by name, axes sorted by
//! axis name, every unit its SI unit and every number in the hex form. Its RFC 8785 bytes
//! ([`InstrumentModel::canonical_json`]) are what is stored and hashed: the digest is SHA-256
//! of them, so two spellings of the same model have one digest. The canonical document is
//! itself a valid input that reads back to the same bytes.

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_kci::kci::sha256_digest;
use fid_kunc::dist::{DistError, Distribution, Params, Quantity, UnitCode};
use fid_kunc::model::{is_model_name, is_semver};
use fid_types::text::is_forbidden_char;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The schema tag.
pub const SCHEMA: &str = "instrument-model/v1";
/// Largest input document, in bytes (1 MiB).
pub const MAX_INPUT_BYTES: usize = 1 << 20;
/// Deepest nesting of the input.
pub const MAX_DEPTH: usize = 32;
/// Most parameters.
pub const MAX_PARAMS: usize = 1024;
/// Most range axes.
pub const MAX_AXES: usize = 64;
/// Longest source citation, in bytes.
pub const MAX_SOURCE_BYTES: usize = 4096;
/// Longest token (parameter, axis or device class name), in bytes.
pub const MAX_TOKEN_BYTES: usize = 64;
/// Longest number text, in bytes.
pub const MAX_NUMBER_BYTES: usize = 64;

// ------------------------------------------------------------------ errors

/// Why a document is not an instrument model. Each names the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// `schema_invalid{field}`: the JSON pointer of the offending member.
    SchemaInvalid { field: String },
    /// `missing_unit{param}`: the parameter has no unit or an empty one.
    MissingUnit { param: String },
    /// `invalid_range{axis}`: the axis's `min` is not below its `max`.
    InvalidRange { axis: String },
    /// `missing_source`: the citation is absent or empty.
    MissingSource,
}

impl ModelError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ModelError::SchemaInvalid { .. } => "schema_invalid",
            ModelError::MissingUnit { .. } => "missing_unit",
            ModelError::InvalidRange { .. } => "invalid_range",
            ModelError::MissingSource => "missing_source",
        }
    }

    /// The named field: the pointer, the parameter, the axis, or `source`.
    pub fn field(&self) -> &str {
        match self {
            ModelError::SchemaInvalid { field } => field,
            ModelError::MissingUnit { param } => param,
            ModelError::InvalidRange { axis } => axis,
            ModelError::MissingSource => "source",
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.code(), self.field())
    }
}

impl std::error::Error for ModelError {}

fn invalid(field: &str) -> ModelError {
    ModelError::SchemaInvalid { field: field.to_owned() }
}

// ------------------------------------------------------------------ units

/// The SI units of the canonical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SiUnit {
    /// `m`
    Metre,
    /// `rad`
    Radian,
    /// `K`
    Kelvin,
    /// `s`
    Second,
    /// `1` (dimensionless)
    One,
}

impl SiUnit {
    /// The symbol.
    pub fn as_str(self) -> &'static str {
        match self {
            SiUnit::Metre => "m",
            SiUnit::Radian => "rad",
            SiUnit::Kelvin => "K",
            SiUnit::Second => "s",
            SiUnit::One => "1",
        }
    }

    /// The `fid-kunc` unit of a distribution in this unit, if there is one.
    pub fn unit_code(self) -> Option<UnitCode> {
        match self {
            SiUnit::Metre => Some(UnitCode::Metre),
            SiUnit::Radian => Some(UnitCode::Radian),
            SiUnit::One => Some(UnitCode::One),
            SiUnit::Kelvin | SiUnit::Second => None,
        }
    }
}

/// Every accepted unit symbol: its SI unit and the power of ten that converts to it.
pub const UNITS: [(&str, SiUnit, i32); 16] = [
    ("1", SiUnit::One, 0),
    ("K", SiUnit::Kelvin, 0),
    ("km", SiUnit::Metre, 3),
    ("m", SiUnit::Metre, 0),
    ("mK", SiUnit::Kelvin, -3),
    ("mm", SiUnit::Metre, -3),
    ("mrad", SiUnit::Radian, -3),
    ("ms", SiUnit::Second, -3),
    ("nm", SiUnit::Metre, -9),
    ("ppm", SiUnit::One, -6),
    ("rad", SiUnit::Radian, 0),
    ("s", SiUnit::Second, 0),
    ("um", SiUnit::Metre, -6),
    ("um/m", SiUnit::One, -6),
    ("urad", SiUnit::Radian, -6),
    ("us", SiUnit::Second, -6),
];

/// The SI unit and power of ten of a unit symbol.
pub fn unit_of(symbol: &str) -> Option<(SiUnit, i32)> {
    UNITS.iter().find(|(s, _, _)| *s == symbol).map(|&(_, u, p)| (u, p))
}

/// The SI value of a number text (see the module docs) given in a unit with power of ten
/// `shift`. `None` for anything that is not a number, a hex value with a prefixed unit, or a
/// result that is not finite.
pub fn number_to_si(text: &str, shift: i32) -> Option<f64> {
    if text.is_empty() || text.len() > MAX_NUMBER_BYTES {
        return None;
    }
    let v = match text.strip_prefix("0x") {
        Some(hex) => {
            if shift != 0 || hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return None;
            }
            f64::from_bits(u64::from_str_radix(hex, 16).ok()?)
        }
        None => decimal(text, shift)?,
    };
    if !v.is_finite() {
        return None;
    }
    Some(if v == 0.0 { 0.0 } else { v })
}

fn digits(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    i
}

/// A decimal in the JSON number grammar, times `10^shift`, rounded once.
fn decimal(text: &str, shift: i32) -> Option<f64> {
    let b = text.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let int_start = i;
    i = digits(b, i);
    let int_len = i - int_start;
    if int_len == 0 || (int_len > 1 && b.get(int_start) == Some(&b'0')) {
        return None;
    }
    if b.get(i) == Some(&b'.') {
        let frac_start = i + 1;
        i = digits(b, frac_start);
        if i == frac_start {
            return None;
        }
    }
    let mantissa_end = i;
    let mut exp: i32 = 0;
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        let negative = b.get(i) == Some(&b'-');
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        i = digits(b, exp_start);
        if i == exp_start || i - exp_start > 4 {
            return None;
        }
        let e: i32 = text.get(exp_start..i)?.parse().ok()?;
        exp = if negative { -e } else { e };
    }
    if i != b.len() {
        return None;
    }
    let mantissa = text.get(..mantissa_end)?;
    let scaled = format!("{mantissa}e{}", exp.checked_add(shift)?);
    scaled.parse::<f64>().ok()
}

/// The canonical text of a double: `0x` and 16 lowercase hex digits of its bits.
pub fn hex(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

// ------------------------------------------------------------------ the model

/// What evidence a model is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModelKind {
    /// Derived from published specification terms (`UNC.INST.MPE`).
    SpecDerived,
    /// An Instrument Error Model Parameter Set (`UNC.INST.IEMPS`).
    Iemps,
    /// A per-observation model (`UNC.INST.PORT`).
    Observation,
}

impl ModelKind {
    /// All kinds.
    pub const ALL: [ModelKind; 3] = [ModelKind::SpecDerived, ModelKind::Iemps, ModelKind::Observation];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ModelKind::SpecDerived => "spec_derived",
            ModelKind::Iemps => "iemps",
            ModelKind::Observation => "observation",
        }
    }

    /// Reads a wire name.
    pub fn parse(s: &str) -> Option<ModelKind> {
        ModelKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// An untrusted string: kept exactly as given, never interpreted (CONSTITUTION C6). There is
/// no `Display`; read it with [`UntrustedString::raw`] and treat it as data.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UntrustedString(String);

impl UntrustedString {
    /// The text exactly as given.
    pub fn raw(&self) -> &str {
        &self.0
    }
}

/// One named parameter in SI.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// The name.
    pub name: String,
    /// The SI unit.
    pub unit: SiUnit,
    /// The value in `unit`.
    pub value: f64,
    /// The distribution, if the model gives one.
    pub dist: Option<Distribution>,
}

/// One declared range axis, closed bounds in SI.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeAxis {
    /// The axis name (`range`, `temperature`, ...).
    pub axis: String,
    /// The lower bound.
    pub min: f64,
    /// The upper bound, above `min`.
    pub max: f64,
    /// The SI unit of both bounds.
    pub unit: SiUnit,
}

/// A validated instrument model with its canonical bytes and digest. Built only by
/// [`InstrumentModel::parse`].
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentModel {
    name: String,
    semver: String,
    kind: ModelKind,
    device_class: String,
    params: Vec<Param>,
    range: Vec<RangeAxis>,
    source: UntrustedString,
    canonical: Vec<u8>,
    digest: Digest,
}

impl InstrumentModel {
    /// Reads and validates a document (bounded: [`MAX_INPUT_BYTES`], [`MAX_DEPTH`]).
    pub fn parse(bytes: &[u8]) -> Result<InstrumentModel, ModelError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(invalid(""));
        }
        let v = parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_DEPTH).map_err(|_| invalid(""))?;
        read(v)
    }

    /// `im:<name>@<semver>`.
    pub fn id(&self) -> String {
        format!("im:{}@{}", self.name, self.semver)
    }

    /// The name, without `im:`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The semver.
    pub fn semver(&self) -> &str {
        &self.semver
    }

    /// The kind.
    pub fn kind(&self) -> ModelKind {
        self.kind
    }

    /// The device class.
    pub fn device_class(&self) -> &str {
        &self.device_class
    }

    /// The parameters, sorted by name.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// The parameter `name`.
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// The range axes, sorted by axis name.
    pub fn range(&self) -> &[RangeAxis] {
        &self.range
    }

    /// The source citation (untrusted).
    pub fn source(&self) -> &UntrustedString {
        &self.source
    }

    /// The RFC 8785 bytes of the canonical document.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 of [`InstrumentModel::canonical_json`].
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn obj<const N: usize>(members: [(&str, CValue); N]) -> CValue {
    CValue::Obj(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn dist_value(d: &Distribution) -> CValue {
    let (kind, params): (&str, Vec<(&str, f64)>) = match d.params() {
        Params::Normal { mu, sigma } => ("normal", vec![("mu", mu), ("sigma", sigma)]),
        Params::Rect { a, b } => ("rect", vec![("a", a), ("b", b)]),
        Params::Tri { a, b, c } => ("tri", vec![("a", a), ("b", b), ("c", c)]),
        Params::Arcsine { a, b } => ("arcsine", vec![("a", a), ("b", b)]),
        Params::ScaledT { mu, s, nu } => ("scaled_t", vec![("mu", mu), ("s", s), ("nu", nu)]),
        Params::Trap { a, b, beta } => ("trap", vec![("a", a), ("b", b), ("beta", beta)]),
    };
    let p = params.into_iter().map(|(k, v)| (k.to_owned(), CValue::Str(hex(v)))).collect();
    obj([("kind", s(kind)), ("params", CValue::Obj(p))])
}

fn canonical_value(m: &InstrumentModel) -> CValue {
    let params = m
        .params
        .iter()
        .map(|p| {
            let mut o = BTreeMap::new();
            if let Some(d) = &p.dist {
                o.insert("dist".to_owned(), dist_value(d));
            }
            o.insert("name".to_owned(), s(&p.name));
            o.insert("unit".to_owned(), s(p.unit.as_str()));
            o.insert("value".to_owned(), CValue::Str(hex(p.value)));
            CValue::Obj(o)
        })
        .collect();
    let range = m
        .range
        .iter()
        .map(|a| {
            obj([
                ("axis", s(&a.axis)),
                ("max", CValue::Str(hex(a.max))),
                ("min", CValue::Str(hex(a.min))),
                ("unit", s(a.unit.as_str())),
            ])
        })
        .collect();
    obj([
        ("device_class", s(&m.device_class)),
        ("id", s(&m.id())),
        ("kind", s(m.kind.as_str())),
        ("params", CValue::Arr(params)),
        ("range", CValue::Arr(range)),
        ("schema", s(SCHEMA)),
        ("source", s(m.source.raw())),
    ])
}

// ------------------------------------------------------------------ reading

/// `[a-z][a-z0-9_]*`, 1 to [`MAX_TOKEN_BYTES`] bytes.
pub fn is_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= MAX_TOKEN_BYTES
        && t.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Splits `im:<name>@<semver>`; `None` if it is not that.
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    let (name, semver) = id.strip_prefix("im:")?.split_once('@')?;
    (is_model_name(name) && is_semver(semver)).then_some((name, semver))
}

/// An object's members with the pointer of the object, checked against the allowed keys.
struct Members {
    o: BTreeMap<String, CValue>,
    at: String,
}

impl Members {
    fn of(v: CValue, at: &str, allowed: &[&str]) -> Result<Members, ModelError> {
        let CValue::Obj(o) = v else { return Err(invalid(at)) };
        // BTreeMap order is byte order: the first unknown member in sorted order.
        if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(invalid(&pointer(at, &shown_name(k))));
        }
        Ok(Members { o, at: at.to_owned() })
    }

    fn ptr(&self, k: &str) -> String {
        pointer(&self.at, k)
    }

    fn take(&mut self, k: &str) -> Option<CValue> {
        self.o.remove(k)
    }

    fn need(&mut self, k: &str) -> Result<CValue, ModelError> {
        self.take(k).ok_or_else(|| invalid(&self.ptr(k)))
    }

    fn string(&mut self, k: &str) -> Result<String, ModelError> {
        match self.need(k)? {
            CValue::Str(v) => Ok(v),
            _ => Err(invalid(&self.ptr(k))),
        }
    }
}

/// Placeholder for an unknown member name that cannot be echoed.
pub const UNSHOWN_NAME: &str = "*";

/// How an unknown member name appears in an error's field pointer: cut to at most
/// [`MAX_TOKEN_BYTES`] bytes (at a character boundary), and replaced by [`UNSHOWN_NAME`] when
/// what is left is not a token or holds a forbidden character. Untrusted input never reaches
/// an error message beyond that.
pub fn shown_name(k: &str) -> String {
    let mut end = k.len().min(MAX_TOKEN_BYTES);
    while !k.is_char_boundary(end) {
        end -= 1;
    }
    match k.get(..end) {
        Some(cut) if is_token(cut) && !cut.chars().any(is_forbidden_char) => cut.to_owned(),
        _ => UNSHOWN_NAME.to_owned(),
    }
}

/// RFC 6901: `~` is `~0` and `/` is `~1`.
fn pointer(at: &str, key: &str) -> String {
    format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"))
}

const TOP: [&str; 7] = ["device_class", "id", "kind", "params", "range", "schema", "source"];
const PARAM: [&str; 4] = ["dist", "name", "unit", "value"];
const AXIS: [&str; 4] = ["axis", "max", "min", "unit"];

fn read(v: CValue) -> Result<InstrumentModel, ModelError> {
    let mut m = Members::of(v, "", &TOP)?;
    if m.string("schema")? != SCHEMA {
        return Err(invalid("/schema"));
    }
    let id = m.string("id")?;
    let (name, semver) = split_id(&id).ok_or_else(|| invalid("/id"))?;
    let kind = ModelKind::parse(&m.string("kind")?).ok_or_else(|| invalid("/kind"))?;
    let device_class = m.string("device_class")?;
    if !is_token(&device_class) {
        return Err(invalid("/device_class"));
    }
    let source = read_source(m.take("source"))?;
    let params = read_params(m.need("params")?)?;
    let range = read_range(m.need("range")?)?;
    let mut model = InstrumentModel {
        name: name.to_owned(),
        semver: semver.to_owned(),
        kind,
        device_class,
        params,
        range,
        source,
        canonical: Vec::new(),
        digest: Digest::TOMBSTONE,
    };
    model.canonical = to_canonical_bytes(&canonical_value(&model)).map_err(|_| invalid(""))?;
    model.digest = sha256_digest(&model.canonical);
    Ok(model)
}

fn read_source(v: Option<CValue>) -> Result<UntrustedString, ModelError> {
    match v {
        None => Err(ModelError::MissingSource),
        Some(CValue::Str(t)) if t.is_empty() => Err(ModelError::MissingSource),
        Some(CValue::Str(t)) if t.len() <= MAX_SOURCE_BYTES && !t.chars().any(is_forbidden_char) => {
            Ok(UntrustedString(t))
        }
        Some(_) => Err(invalid("/source")),
    }
}

fn array(v: CValue, at: &str, max: usize) -> Result<Vec<CValue>, ModelError> {
    match v {
        CValue::Arr(a) if !a.is_empty() && a.len() <= max => Ok(a),
        _ => Err(invalid(at)),
    }
}

/// A number member in a unit with power of ten `shift`.
fn number(m: &mut Members, k: &str, shift: i32) -> Result<f64, ModelError> {
    let t = m.string(k)?;
    number_to_si(&t, shift).ok_or_else(|| invalid(&m.ptr(k)))
}

fn read_params(v: CValue) -> Result<Vec<Param>, ModelError> {
    let items = array(v, "/params", MAX_PARAMS)?;
    let mut names = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let at = format!("/params/{i}");
        let mut m = Members::of(item, &at, &PARAM)?;
        let name = m.string("name")?;
        if !is_token(&name) || !names.insert(name.clone()) {
            return Err(invalid(&m.ptr("name")));
        }
        let symbol = match m.take("unit") {
            None => return Err(ModelError::MissingUnit { param: name }),
            Some(CValue::Str(u)) if u.is_empty() => return Err(ModelError::MissingUnit { param: name }),
            Some(CValue::Str(u)) => u,
            Some(_) => return Err(invalid(&m.ptr("unit"))),
        };
        let (unit, shift) = unit_of(&symbol).ok_or_else(|| invalid(&m.ptr("unit")))?;
        let value = number(&mut m, "value", shift)?;
        let dist = match m.take("dist") {
            None => None,
            Some(d) => Some(read_dist(d, &m.ptr("dist"), unit, shift)?),
        };
        out.push(Param { name, unit, value, dist });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn read_dist(v: CValue, at: &str, unit: SiUnit, shift: i32) -> Result<Distribution, ModelError> {
    let code = unit.unit_code().ok_or_else(|| invalid(at))?;
    let mut m = Members::of(v, at, &["kind", "params"])?;
    let kind = m.string("kind")?;
    let names: &[&str] = match kind.as_str() {
        "normal" => &["mu", "sigma"],
        "rect" | "arcsine" => &["a", "b"],
        "tri" => &["a", "b", "c"],
        "scaled_t" => &["mu", "s", "nu"],
        "trap" => &["a", "b", "beta"],
        _ => return Err(invalid(&m.ptr("kind"))),
    };
    let params_at = m.ptr("params");
    let mut p = Members::of(m.need("params")?, &params_at, names)?;
    let mut q = Vec::with_capacity(names.len());
    for &n in names {
        let dimensionless = matches!(n, "nu" | "beta");
        let x = number(&mut p, n, if dimensionless { 0 } else { shift })?;
        q.push(Quantity::new(x, if dimensionless { UnitCode::One } else { code }));
    }
    let at_param = |i: usize| q.get(i).copied().ok_or_else(|| invalid(&params_at));
    let d = match kind.as_str() {
        "normal" => Distribution::normal(at_param(0)?, at_param(1)?),
        "rect" => Distribution::rect(at_param(0)?, at_param(1)?),
        "arcsine" => Distribution::arcsine(at_param(0)?, at_param(1)?),
        "tri" => Distribution::tri(at_param(0)?, at_param(1)?, at_param(2)?),
        "scaled_t" => Distribution::scaled_t(at_param(0)?, at_param(1)?, at_param(2)?),
        _ => Distribution::trap(at_param(0)?, at_param(1)?, at_param(2)?),
    };
    d.map_err(|e| match e {
        DistError::InvalidParameter { name } => invalid(&pointer(&params_at, name)),
        _ => invalid(at),
    })
}

fn read_range(v: CValue) -> Result<Vec<RangeAxis>, ModelError> {
    let items = array(v, "/range", MAX_AXES)?;
    let mut names = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let at = format!("/range/{i}");
        let mut m = Members::of(item, &at, &AXIS)?;
        let axis = m.string("axis")?;
        if !is_token(&axis) || !names.insert(axis.clone()) {
            return Err(invalid(&m.ptr("axis")));
        }
        let symbol = m.string("unit")?;
        let (unit, shift) = unit_of(&symbol).ok_or_else(|| invalid(&m.ptr("unit")))?;
        let min = number(&mut m, "min", shift)?;
        let max = number(&mut m, "max", shift)?;
        if min >= max {
            return Err(ModelError::InvalidRange { axis });
        }
        out.push(RangeAxis { axis, min, max, unit });
    }
    out.sort_by(|a, b| a.axis.cmp(&b.axis));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_sorted_and_distinct() {
        assert!(UNITS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn decimal_grammar() {
        for bad in ["", "01", "1.", ".5", "+1", "1e", "1e12345", "1,5", "0x1", "1 ", "--1", "1e+-2", "NaN"] {
            assert_eq!(number_to_si(bad, 0), None, "{bad:?}");
        }
        assert_eq!(number_to_si("15", -6), Some(15e-6));
        assert_eq!(number_to_si("0.015", -3), Some(15e-6));
        assert_eq!(number_to_si("-0", 0).map(f64::to_bits), Some(0));
        assert_eq!(number_to_si("1e400", 0), None);
        assert_eq!(number_to_si("1E-2", 0), Some(0.01));
        assert_eq!(number_to_si("0x3ff0000000000000", 0), Some(1.0));
        assert_eq!(number_to_si("0x3ff0000000000000", -3), None);
        assert_eq!(number_to_si("0x7ff8000000000000", 0), None);
        assert_eq!(number_to_si("0x3FF0000000000000", 0), None);
    }
}
