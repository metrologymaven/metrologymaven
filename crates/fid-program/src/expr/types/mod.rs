//! Static types, unit algebra and variable scoping of Command Program expressions. Spec node
//! `AUTO.PROG.EXPR.TYPES`.
//!
//! [`check_types`] walks a program once in step order and gives every expression a static
//! [`Type`]; every name a program can use (part-family parameters, program variables, invoke
//! and call result bindings, counted-loop variables, subroutine parameters) gets a declared
//! type and a block scope. It never evaluates anything and never fails: every problem is a
//! [`Diag`] in the [`TypeReport`], in a fixed order, so the report is byte-identical across
//! runs and platforms (REQ-AUTO.PROG.EXPR.TYPES-001 to -007).
//!
//! Every choice below is pinned here and stated in the PR that built this leaf.
//!
//! **Types.** `bool`, `int`, `decimal`, `string`, `quantity<kind>`, `enum<a|b>`,
//! `element<entity>`, `list<T>`, `record<command@major>` (a command result shape from the
//! manifest) and `formula<kind>`. Variable and subroutine types are written in that same
//! syntax in the program (`VarDecl::ty`, `Subroutine::returns`); a type source that does
//! not parse, or names a kind, entity or command the catalogue or manifest lacks, is
//! `type_unknown`. Part-family parameters map from their `AUTO.PROG.FAMILY` types:
//! `boolean`→`bool`, `integer`→`int`, `number`→`decimal`, `string`→`string`,
//! `quantity`+`quantity_kind`→`quantity<kind>`, `ref:<e>`→`element<e>`,
//! `data_handle`→`element<data_handle>`.
//!
//! **Kinds and dimensions.** Kinds come from the registry [`QuantityCatalogue`](fid_registry::QuantityCatalogue); their
//! dimensions from the pinned table [`KIND_DIMS`] (the catalogue carries no dimensions).
//! `*` and `/` of quantities add or subtract the exponents of [`Dim`]; the result is the one
//! catalogue kind with that dimension, `decimal` when the dimension is empty, or else an
//! anonymous derived kind `quantity<dim:m^2*s^-1>` (also when two catalogue kinds share the
//! dimension). A derived kind may only meet `*`, `/` and comparisons with the same derived
//! kind; `+ - %`, `min` and `max` on it are `type_mismatch`. A catalogue kind missing from
//! [`KIND_DIMS`] can be added, compared and scaled by plain numbers but not multiplied by
//! another quantity (`type_unknown`).
//!
//! **`dimensionless` and `decimal`.** The catalogue kind `dimensionless` (ratios with units
//! such as `%` or `ppm`) has [`Dim::NONE`] and stays a kind: `quantity<dimensionless>` is
//! not a plain number, so `+`, `-` and comparisons against `int`/`decimal` are
//! `unit_mismatch`, and it is never assigned to a `decimal`. Its products follow the
//! dimension (`quantity<dimensionless> * quantity<length>` is a length). A product or
//! quotient whose dimension comes out empty is always `decimal`, never
//! `quantity<dimensionless>`, because the catalogue unit it would carry is unknown.
//!
//! **Catalogue.** The catalogue must be the one the manifest names: if
//! `manifest.quantity_catalogue_digest` differs from [`QuantityCatalogue::digest`](fid_registry::QuantityCatalogue::digest), the
//! report holds one `catalogue_mismatch` declaration diagnostic naming both digests and
//! types nothing. A unit symbol of two kinds is refused the same way (`catalogue_invalid`).
//!
//! **Operators.** `int` op `int` is `int` for `* % + -`; `/` of two `int`s is `decimal`
//! (maintainer decision on PR #418), so integer division is written `floor(a / b)`. `int`
//! with `decimal` is `decimal` (the only implicit conversion). `+ - %`, comparisons, `min` and `max` need equal kinds (`unit_mismatch`
//! naming both kinds, `number` for a plain number). `==`/`!=` also accept two equal
//! non-numeric types, two `enum`s that share a member, and an `enum` against a string
//! literal that is one of its members (`mode == "fast"`); a non-member literal or a
//! computed `string` is `type_mismatch`. A member string literal may be assigned or passed
//! where an `enum` is declared. `and`, `or`, `not`
//! and conditions need `bool`. A quantity divided out to an empty dimension is `decimal`.
//!
//! **Scopes.** One global block holds the part-family parameters, then the program
//! variables. `main` and each subroutine body are blocks inside it (a subroutine sees the
//! globals and its own parameters, not `main`'s bindings); each `if` branch, `else` and
//! loop body is a block. An invoke or call `bind` declares a new name, visible from the
//! next step to the end of the enclosing block; an `assign` only assigns. A name declared
//! twice in one block is `redeclared`; an inner block may shadow an outer name.
//! Part-family parameters and counted-loop variables are read-only: `assign` to one is
//! `read_only`.
//!
//! **Jumps.** Scoping is lexical and ignores `jump`: a `jump` forward over the step that
//! binds a name leaves the name in scope (typed) but unassigned on that path. Whether every
//! use is preceded by an assignment on every path is a control-flow question for
//! `AUTO.PROG.FLOW` (definite-assignment check), not checked here.
//!
//! **Values of record (C1).** Every expression type carries `not_of_record`. A manifest
//! parameter is a value of record when its schema, or any schema inside it through
//! `items`, `prefixItems`, `allOf`, `anyOf`, `oneOf`, `properties` or a local `$ref`, has
//! `"value_of_record": true` or, until `CORE.REG.DEF` names that keyword,
//! `"x-ref": "result"`; an unresolvable `$ref` counts as one (fail closed). An
//! `{"expr": ...}` argument for it is `expression_value_of_record` whatever its type.
//! `{"literal": ...}` arguments are not checked here.
//!
//! **Order.** Declaration diagnostics first (parameters, variables, subroutine
//! signatures), then steps in document order (`main`, then each subroutine, depth first);
//! within a step its fields in a fixed order; within one expression by span start, then
//! span end. Spans are byte ranges of that expression's own source text, so a site is the
//! step ID plus a slot naming the field (`expr`, `cond`, `args.<name>`, `branches.<i>.cond`,
//! `from`, `to`, `step`, `var`, `bind`, `command`, `subroutine`).

mod check;
mod infer;

pub use check::{assignable, check_types, value_of_record, MAX_TYPE_DEPTH};
pub use infer::{type_of, LIBRARY};

use crate::fmt::Uuid7;
use crate::Span;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

/// Symbols of the seven SI base dimensions, in [`Dim::exp`] order: length, mass, time,
/// electric current, thermodynamic temperature, amount of substance, luminous intensity.
pub const BASE_SYMBOLS: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

/// A dimension: SI base exponents plus a separate plane-angle exponent, the layout of the
/// kernel `FormulaIr` dimension so the two agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Dim {
    pub exp: [i8; 7],
    pub angle: i8,
}

const fn dim(exp: [i8; 7], angle: i8) -> Dim {
    Dim { exp, angle }
}

/// Dimensions of the catalogue kinds the checker knows, sorted by name. A catalogue kind
/// outside this table has no dimension.
pub const KIND_DIMS: [(&str, Dim); 19] = [
    ("acceleration", dim([1, 0, -2, 0, 0, 0, 0], 0)),
    ("amount_of_substance", dim([0, 0, 0, 0, 0, 1, 0], 0)),
    ("angle", dim([0; 7], 1)),
    ("angular_velocity", dim([0, 0, -1, 0, 0, 0, 0], 1)),
    ("area", dim([2, 0, 0, 0, 0, 0, 0], 0)),
    ("dimensionless", Dim::NONE),
    ("electric_current", dim([0, 0, 0, 1, 0, 0, 0], 0)),
    ("energy", dim([2, 1, -2, 0, 0, 0, 0], 0)),
    ("force", dim([1, 1, -2, 0, 0, 0, 0], 0)),
    ("frequency", dim([0, 0, -1, 0, 0, 0, 0], 0)),
    ("length", dim([1, 0, 0, 0, 0, 0, 0], 0)),
    ("luminous_intensity", dim([0, 0, 0, 0, 0, 0, 1], 0)),
    ("mass", dim([0, 1, 0, 0, 0, 0, 0], 0)),
    ("pressure", dim([-1, 1, -2, 0, 0, 0, 0], 0)),
    ("temperature", dim([0, 0, 0, 0, 1, 0, 0], 0)),
    ("thermal_expansion", dim([0, 0, 0, 0, -1, 0, 0], 0)),
    ("time", dim([0, 0, 1, 0, 0, 0, 0], 0)),
    ("velocity", dim([1, 0, -1, 0, 0, 0, 0], 0)),
    ("volume", dim([3, 0, 0, 0, 0, 0, 0], 0)),
];

/// Catalogue kinds known to have no dimension in [`KIND_DIMS`] on purpose. A test checks
/// that every kind of every catalogue in the workspace is in one of the two lists, so a new
/// kind is a decision, not a silent `type_unknown`. Empty today.
pub const NO_DIMENSION_KINDS: [&str; 0] = [];

/// The pinned dimension of a kind name, if any.
pub fn kind_dim(name: &str) -> Option<Dim> {
    KIND_DIMS.binary_search_by(|(k, _)| (*k).cmp(name)).ok().and_then(|i| KIND_DIMS.get(i)).map(|e| e.1)
}

impl Dim {
    /// The empty dimension (a plain number).
    pub const NONE: Dim = Dim { exp: [0; 7], angle: 0 };
    /// Plane angle.
    pub const ANGLE: Dim = Dim { exp: [0; 7], angle: 1 };

    /// Whether every exponent is zero.
    pub fn is_none(&self) -> bool {
        *self == Dim::NONE
    }

    /// `self * other` (`sign` 1) or `self / other` (`sign` -1); `None` on `i8` overflow.
    pub fn combine(self, other: Dim, sign: i8) -> Option<Dim> {
        let mut out = self;
        for (o, b) in out.exp.iter_mut().zip(other.exp) {
            *o = o.checked_add(b.checked_mul(sign)?)?;
        }
        out.angle = out.angle.checked_add(other.angle.checked_mul(sign)?)?;
        Some(out)
    }

    /// The square root: every exponent halved, `None` if any is odd.
    pub fn half(self) -> Option<Dim> {
        let h = |e: i8| (e % 2 == 0).then_some(e / 2);
        let mut out = self;
        for e in out.exp.iter_mut() {
            *e = h(*e)?;
        }
        out.angle = h(out.angle)?;
        Some(out)
    }
}

impl fmt::Display for Dim {
    /// `m^2*s^-1*rad`: base symbols in [`BASE_SYMBOLS`] order then `rad`, exponent 1 bare.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        let parts = BASE_SYMBOLS.iter().zip(self.exp).chain(std::iter::once((&"rad", self.angle)));
        for (sym, e) in parts {
            if e == 0 {
                continue;
            }
            if !first {
                f.write_str("*")?;
            }
            first = false;
            if e == 1 {
                f.write_str(sym)?;
            } else {
                write!(f, "{sym}^{e}")?;
            }
        }
        if first {
            f.write_str("1")?;
        }
        Ok(())
    }
}

/// A quantity kind: a catalogue kind by name, or an anonymous derived dimension.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Named(String),
    Derived(Dim),
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Named(n) => f.write_str(n),
            Kind::Derived(d) => write!(f, "dim:{d}"),
        }
    }
}

/// A command result shape (or a nested object of one). `None` marks a field whose schema
/// has no static type (`$ref`, a type union, ...); using it is `type_unknown`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordType {
    /// `command@major`, then `.field` for each nested object.
    pub name: String,
    pub fields: BTreeMap<String, Option<Type>>,
}

/// A static type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    Bool,
    Int,
    Decimal,
    Str,
    Quantity(Kind),
    /// A closed set of string values.
    Enum(BTreeSet<String>),
    /// A reference to an entity of this registry entity type (or `data_handle`).
    Element(String),
    List(Box<Type>),
    Record(Arc<RecordType>),
    /// A compiled formula handle (`AUTO.PROG.EXPR.FORMULA`); no operator applies to it.
    Formula(Kind),
}

impl Type {
    /// Whether the type is `int` or `decimal`.
    pub fn is_number(&self) -> bool {
        matches!(self, Type::Int | Type::Decimal)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => f.write_str("bool"),
            Type::Int => f.write_str("int"),
            Type::Decimal => f.write_str("decimal"),
            Type::Str => f.write_str("string"),
            Type::Quantity(k) => write!(f, "quantity<{k}>"),
            Type::Enum(vs) => {
                f.write_str("enum<")?;
                for (i, v) in vs.iter().enumerate() {
                    if i > 0 {
                        f.write_str("|")?;
                    }
                    f.write_str(v)?;
                }
                f.write_str(">")
            }
            Type::Element(e) => write!(f, "element<{e}>"),
            Type::List(t) => write!(f, "list<{t}>"),
            Type::Record(r) => write!(f, "record<{}>", r.name),
            Type::Formula(k) => write!(f, "formula<{k}>"),
        }
    }
}

/// Diagnostic codes (REQ-AUTO.PROG.EXPR.TYPES-001 to -007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagCode {
    TypeUnknown,
    UnitMismatch,
    UnknownUnit,
    Undeclared,
    Redeclared,
    TypeMismatch,
    ExpressionValueOfRecord,
    /// The manifest's `quantity_catalogue_digest` is not the catalogue's digest.
    CatalogueMismatch,
    /// The catalogue gives one unit symbol to two kinds.
    CatalogueInvalid,
    /// `assign` to a part-family parameter or a counted-loop variable.
    ReadOnly,
}

impl DiagCode {
    /// Stable machine-readable code.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::TypeUnknown => "type_unknown",
            DiagCode::UnitMismatch => "unit_mismatch",
            DiagCode::UnknownUnit => "unknown_unit",
            DiagCode::Undeclared => "undeclared",
            DiagCode::Redeclared => "redeclared",
            DiagCode::TypeMismatch => "type_mismatch",
            DiagCode::ExpressionValueOfRecord => "expression_value_of_record",
            DiagCode::CatalogueMismatch => "catalogue_mismatch",
            DiagCode::CatalogueInvalid => "catalogue_invalid",
            DiagCode::ReadOnly => "read_only",
        }
    }
}

/// A problem found inside one expression, before it is placed at a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprDiag {
    pub code: DiagCode,
    pub span: Span,
    /// What the code names: both kinds for `unit_mismatch`, expected then found type for
    /// `type_mismatch`, the name or unit symbol for the others.
    pub names: Vec<String>,
}

/// One diagnostic of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub code: DiagCode,
    /// The step, or `None` for a declaration (parameter, variable, subroutine signature).
    pub step: Option<Uuid7>,
    /// The field of the step (or the declaration path, e.g. `variables.d`).
    pub slot: String,
    /// Byte span in that field's expression source, when the problem is inside one.
    pub span: Option<Span>,
    pub names: Vec<String>,
}

impl Diag {
    /// `{"code", "names", "slot", "span": {"end", "start"} | null, "step": "<uuid>" | null}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".into(), CValue::Str(self.code.as_str().into()));
        o.insert("names".into(), CValue::Arr(self.names.iter().map(|n| CValue::Str(n.clone())).collect()));
        o.insert("slot".into(), CValue::Str(self.slot.clone()));
        o.insert("span".into(), self.span.map_or(CValue::Null, span_value));
        o.insert("step".into(), self.step.map_or(CValue::Null, |s| CValue::Str(s.to_string())));
        CValue::Obj(o)
    }
}

fn span_value(s: Span) -> CValue {
    let n = |x: usize| CValue::Num(x as f64);
    CValue::Obj(BTreeMap::from([("end".to_string(), n(s.end)), ("start".to_string(), n(s.start))]))
}

/// Where a top-level expression sits: its step and the field holding its source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Site {
    pub step: Uuid7,
    pub slot: String,
}

/// The type of one top-level expression. `not_of_record` is always true (C1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    pub ty: Type,
    /// The expression's span in its source.
    pub span: Span,
    pub not_of_record: bool,
}

/// What [`check_types`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeReport {
    /// The type of every top-level expression that has one.
    pub types: BTreeMap<Site, Typed>,
    pub diagnostics: Vec<Diag>,
}

impl TypeReport {
    /// Whether the program has no diagnostic.
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// `{"diagnostics": [...], "types": [{"not_of_record", "slot", "span", "step", "type"}]}`,
    /// types in [`Site`] order; serialise with `fid-canon` for byte-identical output.
    pub fn to_value(&self) -> CValue {
        let types = self
            .types
            .iter()
            .map(|(site, t)| {
                let mut o = BTreeMap::new();
                o.insert("not_of_record".into(), CValue::Bool(t.not_of_record));
                o.insert("slot".into(), CValue::Str(site.slot.clone()));
                o.insert("span".into(), span_value(t.span));
                o.insert("step".into(), CValue::Str(site.step.to_string()));
                o.insert("type".into(), CValue::Str(t.ty.to_string()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("diagnostics".into(), CValue::Arr(self.diagnostics.iter().map(Diag::to_value).collect()));
        o.insert("types".into(), CValue::Arr(types));
        CValue::Obj(o)
    }
}
