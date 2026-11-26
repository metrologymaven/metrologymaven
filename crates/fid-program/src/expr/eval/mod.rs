//! Deterministic control evaluator and function library. Spec node `AUTO.PROG.EXPR.EVAL`.
//!
//! [`eval_expr`] evaluates one parsed expression against an [`Env`] of typed variables. It
//! first types the expression with [`type_of`](crate::expr::types::type_of) and refuses
//! anything that does not type-check ([`EvalError::Type`], nothing evaluated), then walks the
//! tree with an explicit stack. Every [`Value`] it returns is tagged `not_of_record`
//! (REQ-001); the tag is part of the value and of its canonical form ([`Value::to_value`]), so
//! it survives storage in variables and use as an invoke argument. The only inputs are the
//! tree, the environment and the catalogue: no file, network, clock, environment variable,
//! randomness or bus (REQ-007, checked by a source allowlist test).
//!
//! Every choice below is pinned here and stated in the PR that built this leaf.
//!
//! **Values.** `bool`; `int` (`i64`); an exact decimal [`Dec`] (`coef * 10^-scale`, at most
//! 38 digits, `|scale| <= 400`), plain or with the unit symbol it was written in; a real
//! (`f64` in SI, finite, with its quantity kind or none for a plain number); `string`; an
//! enum member; an element ID; a list; a record. The static type `decimal` holds a plain
//! decimal or a plain real, `quantity<k>` a decimal in a unit of `k` or a real of kind `k`.
//!
//! **Order.** Operands left to right, children before their operator, the first error wins.
//! `and` and `or` short-circuit; `if` evaluates its condition, then only the chosen branch
//! (whose `int` value becomes an exact decimal when the `if` is typed `decimal`).
//!
//! **Exact decimals (REQ-002).** `int` and plain decimals, and two decimals written in the
//! same unit, stay exact under `+`, `-`, `%` and comparison; under `*` a decimal in a unit
//! times a plain number (or `int`) also stays exact and keeps its unit. Sums and remainders
//! take the larger scale, products the sum of the scales. A result whose coefficient needs
//! more than 38 digits is first stripped of trailing zeros (only as far as needed); if it
//! still does not fit, or its scale cannot be brought within ±400, evaluation fails with
//! `decimal_overflow`. A decimal literal is held to the same bounds. `%` is the truncated
//! remainder (the sign of the dividend).
//!
//! **Into `f64` (REQ-003).** A decimal that meets a real, a decimal in another unit, or any
//! `/` (division is never exact) is converted once: a plain decimal by the correctly rounded
//! `fid_num::kdec::Decimal::to_f64`, a decimal in a unit by the single-rounding conversion of
//! `CORE.BUS.UNIT` (`fid_bus::unit::normalise`, the factors and Π of the catalogue, the same
//! bits as `GEO.FRAME.UNITS` for its units). A conversion beyond the largest finite `f64`, or
//! of a non-zero decimal that rounds to zero, fails with `eval_error` `domain`. An `int`
//! meets a real as `i as f64` (round to nearest even); `int / int` is
//! `(a as f64) / (b as f64)`. Real arithmetic is IEEE 754 binary64 `+ - * /` and the exact
//! `%` (fmod), one operation at a time, never fused.
//!
//! **Comparisons (REQ-004).** Exact pairs compare exactly. A real against a plain decimal or
//! an `int` uses `fid_num::kdec::cmp`; a real against a decimal in a unit compares the real
//! with the exact rational `decimal * factor` (Π of the catalogue as the exact rational
//! `PI_256 / 2^254`), so `0.1[mm]` never equals the nearest `f64` of 0.0001 m. Two decimals
//! in different units are each converted (REQ-003) and the reals compared. Two reals compare
//! by IEEE order (`-0 == +0`). `==` on non-numbers compares by value; an enum member equals a
//! string with the same text; lists and records compare element by element.
//!
//! **Errors (REQ-006).** `eval_error` with the span of the failing node and a kind:
//! `div_zero` (`/` or `%` by a zero), `index_range` (an index below 0 or at least the length),
//! `int_overflow` (an `int` result outside `i64`, also from `abs`, `round`, `floor`,
//! `ceil`), `non_finite` (a real result that is infinite), `domain` (`sqrt` of a negative,
//! an out-of-range conversion, a bad `format` template). `decimal_overflow` as above.
//!
//! **Library.** (details in the `func` module docs): `abs`, `min`, `max`, `round` (half to even), `floor`, `ceil`,
//! `sqrt`, `sin`, `cos`, `tan`, `atan2` (all through `fid_num::f`), `len` (list items or
//! Unicode scalar values), `contains`, `format`, `to_unit`.

mod dec;
mod func;
mod num;
mod walk;

pub use dec::{Dec, MAX_DEC_DIGITS, MAX_SCALE};
pub use func::{display, MAX_FORMAT_BYTES};

use crate::expr::syntax::{Expr, Name, Span};
use crate::expr::types::infer::Kinds;
use crate::expr::types::{type_of, ExprDiag, Kind, Type};
use fid_canon::CValue;
use fid_registry::QuantityCatalogue;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Deepest nesting of lists and records in one [`Value`].
pub const MAX_VALUE_DEPTH: u8 = 32;

/// A runtime value. Always tagged `not_of_record` (REQ-001, CONSTITUTION C1); build one with
/// the constructors, which refuse non-finite reals, malformed decimals and deep nesting.
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    v: V,
    depth: u8,
    not_of_record: bool,
}

/// The payload of a [`Value`].
#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Bool(bool),
    Int(i64),
    /// Exact decimal, plain (`None`) or in the unit symbol it was written in.
    Dec(Dec, Option<String>),
    /// Finite `f64` in SI, with its quantity kind (`None`: a plain number).
    Real(f64, Option<Kind>),
    Str(String),
    /// An enum member.
    Enum(String),
    /// An element ID.
    Element(String),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
}

/// Why a [`Value`] could not be built or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueError(pub &'static str);

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ValueError {}

impl Value {
    fn leaf(v: V) -> Value {
        Value { v, depth: 0, not_of_record: true }
    }

    pub fn bool(b: bool) -> Value {
        Value::leaf(V::Bool(b))
    }

    pub fn int(i: i64) -> Value {
        Value::leaf(V::Int(i))
    }

    pub fn str(s: &str) -> Value {
        Value::leaf(V::Str(s.to_string()))
    }

    pub fn enum_member(s: &str) -> Value {
        Value::leaf(V::Enum(s.to_string()))
    }

    pub fn element(id: &str) -> Value {
        Value::leaf(V::Element(id.to_string()))
    }

    /// An exact decimal from `[-]<digits>[.<digits>]`, within the bounds of [`Dec`].
    pub fn dec(text: &str, unit: Option<&str>) -> Result<Value, ValueError> {
        let (neg, body) = match text.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, text),
        };
        let d = Dec::from_text(body, neg).map_err(|_| ValueError("decimal"))?;
        Ok(Value::leaf(V::Dec(d, unit.map(str::to_string))))
    }

    pub(crate) fn of_dec(d: Dec, unit: Option<String>) -> Value {
        Value::leaf(V::Dec(d, unit))
    }

    /// A real; refuses NaN and infinities (REQ-006: NaN is an error, never a value).
    pub fn real(si: f64, kind: Option<Kind>) -> Result<Value, ValueError> {
        if !si.is_finite() {
            return Err(ValueError("non_finite"));
        }
        Ok(Value::leaf(V::Real(si, kind)))
    }

    pub fn list(items: Vec<Value>) -> Result<Value, ValueError> {
        let depth = Value::nest(items.iter())?;
        Ok(Value { v: V::List(items), depth, not_of_record: true })
    }

    pub fn record(fields: BTreeMap<String, Value>) -> Result<Value, ValueError> {
        let depth = Value::nest(fields.values())?;
        Ok(Value { v: V::Record(fields), depth, not_of_record: true })
    }

    fn nest<'a>(items: impl Iterator<Item = &'a Value>) -> Result<u8, ValueError> {
        let inner = items.map(|v| v.depth).max().unwrap_or(0);
        let depth = inner.saturating_add(1);
        if depth > MAX_VALUE_DEPTH {
            return Err(ValueError("too_deep"));
        }
        Ok(depth)
    }

    pub fn v(&self) -> &V {
        &self.v
    }

    /// Always true: no expression value is ever a value of record (C1).
    pub fn not_of_record(&self) -> bool {
        self.not_of_record
    }

    /// Canonical form: `{"not_of_record": true, "t": <tag>, ...}` with `bool` `{"v": b}`,
    /// `int` `{"v": "<i64>"}`, `dec` `{"unit": <symbol>|null, "v": "<text>"}`, `real`
    /// `{"bits": "0x<16 hex>", "kind": <kind>|null}`, `str`, `enum`, `element` `{"v": s}`,
    /// `list` `{"v": [...]}`, `record` `{"v": {...}}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("not_of_record".to_string(), CValue::Bool(self.not_of_record));
        let (tag, extra): (&str, Vec<(&str, CValue)>) = match &self.v {
            V::Bool(b) => ("bool", vec![("v", CValue::Bool(*b))]),
            V::Int(i) => ("int", vec![("v", CValue::I64(*i))]),
            V::Dec(d, u) => (
                "dec",
                vec![
                    ("unit", u.as_ref().map_or(CValue::Null, |u| CValue::Str(u.clone()))),
                    ("v", CValue::Str(d.text())),
                ],
            ),
            V::Real(x, k) => (
                "real",
                vec![
                    ("bits", CValue::F64Bits(*x)),
                    ("kind", k.as_ref().map_or(CValue::Null, |k| CValue::Str(k.to_string()))),
                ],
            ),
            V::Str(s) => ("str", vec![("v", CValue::Str(s.clone()))]),
            V::Enum(s) => ("enum", vec![("v", CValue::Str(s.clone()))]),
            V::Element(s) => ("element", vec![("v", CValue::Str(s.clone()))]),
            V::List(xs) => ("list", vec![("v", CValue::Arr(xs.iter().map(Value::to_value).collect()))]),
            V::Record(m) => {
                ("record", vec![("v", CValue::Obj(m.iter().map(|(k, v)| (k.clone(), v.to_value())).collect()))])
            }
        };
        o.insert("t".to_string(), CValue::Str(tag.to_string()));
        for (k, v) in extra {
            o.insert(k.to_string(), v);
        }
        CValue::Obj(o)
    }

    /// Reads the canonical form of [`Value::to_value`] (exactly its keys, `not_of_record`
    /// true; a `real` kind must be a catalogue kind name, not a derived `dim:` kind).
    pub fn from_value(c: &CValue) -> Result<Value, ValueError> {
        Value::read(c, 0)
    }

    fn read(c: &CValue, depth: u8) -> Result<Value, ValueError> {
        if depth > MAX_VALUE_DEPTH {
            return Err(ValueError("too_deep"));
        }
        let CValue::Obj(o) = c else { return Err(ValueError("shape")) };
        if o.get("not_of_record") != Some(&CValue::Bool(true)) {
            return Err(ValueError("not_of_record"));
        }
        let Some(CValue::Str(tag)) = o.get("t") else { return Err(ValueError("shape")) };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        let want: &[&str] = match tag.as_str() {
            "dec" => &["not_of_record", "t", "unit", "v"],
            "real" => &["bits", "kind", "not_of_record", "t"],
            _ => &["not_of_record", "t", "v"],
        };
        if keys != want {
            return Err(ValueError("shape"));
        }
        let v = o.get("v");
        let text = |v: Option<&CValue>| match v {
            Some(CValue::Str(s)) => Ok(s.clone()),
            _ => Err(ValueError("shape")),
        };
        match tag.as_str() {
            "bool" => match v {
                Some(CValue::Bool(b)) => Ok(Value::bool(*b)),
                _ => Err(ValueError("shape")),
            },
            "int" => {
                let s = text(v)?;
                let ok = !s.is_empty()
                    && s.strip_prefix('-').unwrap_or(&s).bytes().all(|b| b.is_ascii_digit())
                    && s != "-"
                    && s.len() <= 20;
                let i: i64 =
                    if ok { s.parse().map_err(|_| ValueError("int"))? } else { return Err(ValueError("int")) };
                Ok(Value::int(i))
            }
            "dec" => {
                let unit = match o.get("unit") {
                    Some(CValue::Null) => None,
                    Some(CValue::Str(u)) => Some(u.clone()),
                    _ => return Err(ValueError("shape")),
                };
                Value::dec(&text(v)?, unit.as_deref())
            }
            "real" => {
                let Some(CValue::Str(bits)) = o.get("bits") else { return Err(ValueError("shape")) };
                let hex = bits.strip_prefix("0x").ok_or(ValueError("bits"))?;
                if hex.len() != 16 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                    return Err(ValueError("bits"));
                }
                let b = u64::from_str_radix(hex, 16).map_err(|_| ValueError("bits"))?;
                let kind = match o.get("kind") {
                    Some(CValue::Null) => None,
                    Some(CValue::Str(k)) if !k.starts_with("dim:") => Some(Kind::Named(k.clone())),
                    _ => return Err(ValueError("kind")),
                };
                Value::real(f64::from_bits(b), kind)
            }
            "str" => Ok(Value::str(&text(v)?)),
            "enum" => Ok(Value::enum_member(&text(v)?)),
            "element" => Ok(Value::element(&text(v)?)),
            "list" => match v {
                Some(CValue::Arr(xs)) => {
                    let items = xs.iter().map(|x| Value::read(x, depth + 1)).collect::<Result<Vec<_>, _>>()?;
                    Value::list(items)
                }
                _ => Err(ValueError("shape")),
            },
            "record" => match v {
                Some(CValue::Obj(m)) => {
                    let mut fields = BTreeMap::new();
                    for (k, x) in m {
                        fields.insert(k.clone(), Value::read(x, depth + 1)?);
                    }
                    Value::record(fields)
                }
                _ => Err(ValueError("shape")),
            },
            _ => Err(ValueError("tag")),
        }
    }
}

/// The kind of a runtime error (REQ-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvalKind {
    DivZero,
    IndexRange,
    IntOverflow,
    NonFinite,
    Domain,
}

impl EvalKind {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            EvalKind::DivZero => "div_zero",
            EvalKind::IndexRange => "index_range",
            EvalKind::IntOverflow => "int_overflow",
            EvalKind::NonFinite => "non_finite",
            EvalKind::Domain => "domain",
        }
    }
}

/// Why evaluation returned no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// `eval_error`: a runtime error at the failing node (REQ-006).
    Eval { kind: EvalKind, span: Span },
    /// `decimal_overflow`: an exact decimal needs more than 38 digits (REQ-002).
    DecimalOverflow { span: Span },
    /// `eval_type`: the expression does not type-check; nothing was evaluated.
    Type { diags: Vec<ExprDiag> },
    /// `eval_env`: a binding is refused (bad name, value not of its declared type, a type
    /// with no runtime value, or a catalogue other than the one the environment was built with).
    Env { name: String, reason: &'static str },
    /// `eval_internal`: a value disagrees with the static type (never expected after
    /// typing; refused rather than guessed).
    Internal { span: Span },
}

impl EvalError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::Eval { .. } => "eval_error",
            EvalError::DecimalOverflow { .. } => "decimal_overflow",
            EvalError::Type { .. } => "eval_type",
            EvalError::Env { .. } => "eval_env",
            EvalError::Internal { .. } => "eval_internal",
        }
    }

    /// `{"code", "kind": <kind>|null, "span": {"end", "start"}|null}`.
    pub fn to_value(&self) -> CValue {
        let (kind, span) = match self {
            EvalError::Eval { kind, span } => (Some(kind.as_str()), Some(*span)),
            EvalError::DecimalOverflow { span } | EvalError::Internal { span } => (None, Some(*span)),
            EvalError::Type { .. } | EvalError::Env { .. } => (None, None),
        };
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        o.insert("kind".to_string(), kind.map_or(CValue::Null, |k| CValue::Str(k.to_string())));
        let span = span.map_or(CValue::Null, |s| {
            CValue::Obj(BTreeMap::from([
                ("end".to_string(), CValue::I64(i64::try_from(s.end).unwrap_or(i64::MAX))),
                ("start".to_string(), CValue::I64(i64::try_from(s.start).unwrap_or(i64::MAX))),
            ]))
        });
        o.insert("span".to_string(), span);
        CValue::Obj(o)
    }
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::Eval { kind, span } => write!(f, "eval_error {} at {}..{}", kind.as_str(), span.start, span.end),
            EvalError::DecimalOverflow { span } => write!(f, "decimal_overflow at {}..{}", span.start, span.end),
            EvalError::Type { diags } => write!(f, "eval_type: {} diagnostic(s)", diags.len()),
            EvalError::Env { name, reason } => write!(f, "eval_env {name}: {reason}"),
            EvalError::Internal { span } => write!(f, "eval_internal at {}..{}", span.start, span.end),
        }
    }
}

impl std::error::Error for EvalError {}

/// The variable environment: each name with its declared type and current value.
#[derive(Debug, Clone, Default)]
pub struct Env {
    digest: Option<Digest>,
    vars: BTreeMap<String, (Type, Value)>,
}

impl Env {
    pub fn new() -> Env {
        Env::default()
    }

    /// Binds (or rebinds) `name`. Refused unless `name` is an identifier, the catalogue is the
    /// one of every earlier binding, and `value` is a value of `ty` (a `formula` has no runtime
    /// value here; a record must carry every typed field and no undeclared one).
    pub fn bind(&mut self, name: &str, ty: Type, value: Value, cat: &dyn QuantityCatalogue) -> Result<(), EvalError> {
        let refuse = |reason: &'static str| EvalError::Env { name: name.to_string(), reason };
        if Name::new(name).is_none() {
            return Err(refuse("name"));
        }
        if self.digest.is_some_and(|d| d != cat.digest()) {
            return Err(refuse("catalogue"));
        }
        let kinds = Kinds::new(cat).map_err(|_| refuse("catalogue"))?;
        if !walk::conforms(&kinds, &ty, &value) {
            return Err(refuse("type"));
        }
        self.digest = Some(cat.digest());
        self.vars.insert(name.to_string(), (ty, value));
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name).map(|(_, v)| v)
    }
}

/// Evaluates `expr` (REQ-001 to REQ-007). Fails closed: an expression that does not
/// type-check, or an environment built with another catalogue, evaluates nothing.
pub fn eval_expr(expr: &Expr, env: &Env, cat: &dyn QuantityCatalogue) -> Result<Value, EvalError> {
    if env.digest.is_some_and(|d| d != cat.digest()) {
        return Err(EvalError::Env { name: String::new(), reason: "catalogue" });
    }
    let types: BTreeMap<String, Type> = env.vars.iter().map(|(k, (t, _))| (k.clone(), t.clone())).collect();
    let (ty, diags) = type_of(expr, &types, cat);
    let ty = match ty {
        Some(t) if diags.is_empty() => t,
        _ => return Err(EvalError::Type { diags }),
    };
    let kinds = Kinds::new(cat).map_err(|_| EvalError::Type { diags: Vec::new() })?;
    let ctx = walk::Ctx { cat, kinds: &kinds, env, types: &types };
    let v = walk::run(&ctx, expr)?;
    if !walk::conforms(&kinds, &ty, &v) {
        return Err(EvalError::Internal { span: expr.span });
    }
    Ok(v)
}
