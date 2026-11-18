//! Field tables: the single description of every object in `program-v1`.
//!
//! The decoder reads these tables to refuse unknown fields, require mandatory ones and
//! type-check every value, and [`program_schema`] turns the same tables into the
//! checked-in JSON Schema (REQ-AUTO.PROG.FMT-007). A field exists in the format only if
//! it is listed here.

use super::uuid::Uuid7;
use fid_canon::CValue;
use std::collections::BTreeMap;

/// Variable, label, subroutine and argument names.
pub const IDENT_PATTERN: &str = "^[A-Za-z_][A-Za-z0-9_]{0,63}$";
/// Registry command ID without the major (same rule as `fid-registry` definitions).
pub const COMMAND_PATTERN: &str = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$";
/// Canonical UUIDv7 text.
pub const UUID7_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

/// The type of one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    /// Exactly this string.
    Const(&'static str),
    /// Exactly this integer.
    IntConst(u32),
    /// An integer in `min..=u32::MAX`.
    U32 {
        min: u32,
    },
    Uuid7,
    Ident,
    CommandId,
    /// Any string (expression source, type source, comment text).
    Text,
    /// Any JSON value (an argument literal).
    Json,
    /// Any JSON object (opaque here, validated by its owning module).
    Object,
    /// An array of opaque objects.
    Objects,
    /// An array of the named `$defs` entry with at least `min` items.
    ArrayOf(&'static str, u32),
    /// An object from names to arguments.
    ArgMap,
}

/// One field of an object.
pub(crate) struct Field {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
}

/// One closed object shape; `def` is its `$defs` name.
pub(crate) struct Shape {
    pub def: &'static str,
    pub fields: &'static [Field],
}

const fn req(name: &'static str, ty: Ty) -> Field {
    Field { name, ty, required: true }
}

const fn opt(name: &'static str, ty: Ty) -> Field {
    Field { name, ty, required: false }
}

const STEPS: Ty = Ty::ArrayOf("step", 0);
const STEP_ID: Field = req("step_id", Ty::Uuid7);

pub(crate) const PROGRAM: Shape = Shape {
    def: "program",
    fields: &[
        req("format", Ty::Const("fid-program")),
        req("format_version", Ty::IntConst(1)),
        req("program_id", Ty::Uuid7),
        req("meta", Ty::Object),
        req("params", Ty::Objects),
        req("variables", Ty::ArrayOf("var_decl", 0)),
        req("subroutines", Ty::ArrayOf("subroutine", 0)),
        req("main", STEPS),
    ],
};

pub(crate) const VAR_DECL: Shape = Shape { def: "var_decl", fields: &[req("name", Ty::Ident), req("type", Ty::Text)] };

pub(crate) const SUBROUTINE: Shape = Shape {
    def: "subroutine",
    fields: &[
        req("name", Ty::Ident),
        req("params", Ty::ArrayOf("var_decl", 0)),
        opt("returns", Ty::Text),
        req("body", STEPS),
    ],
};

pub(crate) const BRANCH: Shape = Shape { def: "branch", fields: &[req("cond", Ty::Text), req("body", STEPS)] };

pub(crate) const ARG_LITERAL: Shape = Shape { def: "arg_literal", fields: &[req("literal", Ty::Json)] };
pub(crate) const ARG_EXPR: Shape = Shape { def: "arg_expr", fields: &[req("expr", Ty::Text)] };

pub(crate) const STEP_INVOKE: Shape = Shape {
    def: "step_invoke",
    fields: &[
        STEP_ID,
        req("kind", Ty::Const("invoke")),
        req("command", Ty::CommandId),
        req("major", Ty::U32 { min: 0 }),
        req("args", Ty::ArgMap),
        opt("bind", Ty::Ident),
    ],
};

pub(crate) const STEP_ASSIGN: Shape = Shape {
    def: "step_assign",
    fields: &[STEP_ID, req("kind", Ty::Const("assign")), req("var", Ty::Ident), req("expr", Ty::Text)],
};

pub(crate) const STEP_IF: Shape = Shape {
    def: "step_if",
    fields: &[
        STEP_ID,
        req("kind", Ty::Const("if")),
        req("branches", Ty::ArrayOf("branch", 1)),
        req("else_body", STEPS),
    ],
};

pub(crate) const STEP_LOOP_COUNTED: Shape = Shape {
    def: "step_loop_counted",
    fields: &[
        STEP_ID,
        req("kind", Ty::Const("loop")),
        req("mode", Ty::Const("counted")),
        req("var", Ty::Ident),
        req("from", Ty::Text),
        req("to", Ty::Text),
        req("step", Ty::Text),
        req("max_iterations", Ty::U32 { min: 1 }),
        req("body", STEPS),
    ],
};

pub(crate) const STEP_LOOP_WHILE: Shape = Shape {
    def: "step_loop_while",
    fields: &[
        STEP_ID,
        req("kind", Ty::Const("loop")),
        req("mode", Ty::Const("while")),
        req("cond", Ty::Text),
        req("max_iterations", Ty::U32 { min: 1 }),
        req("body", STEPS),
    ],
};

pub(crate) const STEP_CALL: Shape = Shape {
    def: "step_call",
    fields: &[
        STEP_ID,
        req("kind", Ty::Const("call")),
        req("subroutine", Ty::Ident),
        req("args", Ty::ArgMap),
        opt("bind", Ty::Ident),
    ],
};

pub(crate) const STEP_RETURN: Shape =
    Shape { def: "step_return", fields: &[STEP_ID, req("kind", Ty::Const("return")), opt("expr", Ty::Text)] };

pub(crate) const STEP_LABEL: Shape =
    Shape { def: "step_label", fields: &[STEP_ID, req("kind", Ty::Const("label")), req("name", Ty::Ident)] };

pub(crate) const STEP_JUMP: Shape = Shape {
    def: "step_jump",
    fields: &[STEP_ID, req("kind", Ty::Const("jump")), req("label", Ty::Ident), opt("cond", Ty::Text)],
};

pub(crate) const STEP_OPERATOR: Shape = Shape {
    def: "step_operator",
    fields: &[STEP_ID, req("kind", Ty::Const("operator")), req("descriptor", Ty::Object)],
};

pub(crate) const STEP_COMMENT: Shape =
    Shape { def: "step_comment", fields: &[STEP_ID, req("kind", Ty::Const("comment")), req("text", Ty::Text)] };

/// Every step shape. `invoke` is the only one with a command field (REQ-AUTO.PROG.FMT-004).
pub(crate) const STEP_SHAPES: [&Shape; 11] = [
    &STEP_INVOKE,
    &STEP_ASSIGN,
    &STEP_IF,
    &STEP_LOOP_COUNTED,
    &STEP_LOOP_WHILE,
    &STEP_CALL,
    &STEP_RETURN,
    &STEP_LABEL,
    &STEP_JUMP,
    &STEP_OPERATOR,
    &STEP_COMMENT,
];

const UNIONS: [(&str, &[&Shape]); 2] = [("arg", &[&ARG_LITERAL, &ARG_EXPR]), ("step", &STEP_SHAPES)];

fn const_of(shape: &Shape, field: &str) -> Option<&'static str> {
    shape.fields.iter().find(|f| f.name == field).and_then(|f| match f.ty {
        Ty::Const(c) => Some(c),
        _ => None,
    })
}

/// The step shape for a `kind` (and, for loops, `mode`).
pub(crate) fn step_shape(kind: &str, mode: Option<&str>) -> Option<&'static Shape> {
    STEP_SHAPES.iter().copied().find(|s| {
        const_of(s, "kind") == Some(kind)
            && match const_of(s, "mode") {
                Some(m) => mode == Some(m),
                None => true,
            }
    })
}

pub(crate) fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

pub(crate) fn is_command_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
        && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && s.bytes().last().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub(crate) fn as_u32(v: &CValue) -> Option<u32> {
    match v {
        CValue::Num(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX) => Some(*n as u32),
        _ => None,
    }
}

impl Ty {
    /// Whether `v` has this type. Nested `$defs` entries are checked when decoded.
    pub(crate) fn admits(self, v: &CValue) -> bool {
        match self {
            Ty::Const(c) => matches!(v, CValue::Str(s) if s == c),
            Ty::IntConst(n) => as_u32(v) == Some(n),
            Ty::U32 { min } => as_u32(v).is_some_and(|x| x >= min),
            Ty::Uuid7 => matches!(v, CValue::Str(s) if Uuid7::parse(s).is_some()),
            Ty::Ident => matches!(v, CValue::Str(s) if is_ident(s)),
            Ty::CommandId => matches!(v, CValue::Str(s) if is_command_id(s)),
            Ty::Text => matches!(v, CValue::Str(_)),
            Ty::Json => true,
            Ty::Object => matches!(v, CValue::Obj(_)),
            Ty::Objects => matches!(v, CValue::Arr(items) if items.iter().all(|i| matches!(i, CValue::Obj(_)))),
            Ty::ArrayOf(_, min) => matches!(v, CValue::Arr(items) if items.len() >= min as usize),
            Ty::ArgMap => matches!(v, CValue::Obj(m) if m.keys().all(|k| is_ident(k))),
        }
    }

    /// What the field must be, for error messages.
    pub(crate) fn expected(self) -> String {
        match self {
            Ty::Const(c) => format!("the string {c:?}"),
            Ty::IntConst(n) => format!("the integer {n}"),
            Ty::U32 { min } => format!("an integer in {min}..=4294967295"),
            Ty::Uuid7 => "a lowercase hyphenated UUIDv7".into(),
            Ty::Ident => "an identifier matching ^[A-Za-z_][A-Za-z0-9_]{0,63}$".into(),
            Ty::CommandId => "a registry command ID without the major".into(),
            Ty::Text => "a string".into(),
            Ty::Json => "a JSON value".into(),
            Ty::Object => "an object".into(),
            Ty::Objects => "an array of objects".into(),
            Ty::ArrayOf(_, 0) => "an array".into(),
            Ty::ArrayOf(_, min) => format!("an array of at least {min} items"),
            Ty::ArgMap => "an object whose keys are identifiers".into(),
        }
    }
}

// ------------------------------------------------------------------ schema

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(n: u32) -> CValue {
    CValue::Num(f64::from(n))
}

fn reference(def: &str) -> CValue {
    obj([("$ref", s(&format!("#/$defs/{def}")))])
}

fn pattern(p: &str) -> CValue {
    obj([("type", s("string")), ("pattern", s(p))])
}

fn ty_schema(t: Ty) -> CValue {
    match t {
        Ty::Const(c) => obj([("const", s(c))]),
        Ty::IntConst(n) => obj([("const", num(n))]),
        Ty::U32 { min } => obj([("type", s("integer")), ("minimum", num(min)), ("maximum", num(u32::MAX))]),
        Ty::Uuid7 => pattern(UUID7_PATTERN),
        Ty::Ident => pattern(IDENT_PATTERN),
        Ty::CommandId => pattern(COMMAND_PATTERN),
        Ty::Text => obj([("type", s("string"))]),
        Ty::Json => obj([]),
        Ty::Object => obj([("type", s("object"))]),
        Ty::Objects => obj([("type", s("array")), ("items", obj([("type", s("object"))]))]),
        Ty::ArrayOf(def, 0) => obj([("type", s("array")), ("items", reference(def))]),
        Ty::ArrayOf(def, min) => obj([("type", s("array")), ("items", reference(def)), ("minItems", num(min))]),
        Ty::ArgMap => obj([
            ("type", s("object")),
            ("propertyNames", obj([("pattern", s(IDENT_PATTERN))])),
            ("additionalProperties", reference("arg")),
        ]),
    }
}

fn shape_schema(shape: &Shape) -> BTreeMap<String, CValue> {
    let required = shape.fields.iter().filter(|f| f.required).map(|f| s(f.name)).collect();
    let props = shape.fields.iter().map(|f| (f.name.to_string(), ty_schema(f.ty))).collect();
    let mut m = BTreeMap::new();
    m.insert("type".into(), s("object"));
    m.insert("additionalProperties".into(), CValue::Bool(false));
    m.insert("required".into(), CValue::Arr(required));
    m.insert("properties".into(), CValue::Obj(props));
    m
}

/// JSON Schema (2020-12) of `program-v1`, generated from the field tables; checked in as
/// `schema/program-v1.json` (REQ-AUTO.PROG.FMT-007). The schema cannot express the
/// canonical-bytes rule, step-ID uniqueness or the size bounds; the parser enforces those.
pub fn program_schema() -> CValue {
    let mut root = shape_schema(&PROGRAM);
    root.insert("$schema".into(), s("https://json-schema.org/draft/2020-12/schema"));
    root.insert("$id".into(), s("https://metrologymaven.io/schema/program-v1.json"));
    root.insert("title".into(), s("Metrology Maven Command Program, format version 1"));
    root.insert(
        "description".into(),
        s("Stored as RFC 8785 canonical JSON. Step IDs are unique within a program. See spec node AUTO.PROG.FMT."),
    );
    let mut defs = BTreeMap::new();
    let plain = [&VAR_DECL, &SUBROUTINE, &BRANCH];
    for shape in plain.into_iter().chain(UNIONS.iter().flat_map(|(_, members)| members.iter().copied())) {
        defs.insert(shape.def.to_string(), CValue::Obj(shape_schema(shape)));
    }
    for (name, members) in UNIONS {
        let refs = members.iter().map(|m| reference(m.def)).collect();
        defs.insert(name.to_string(), obj([("oneOf", CValue::Arr(refs))]));
    }
    root.insert("$defs".into(), CValue::Obj(defs));
    CValue::Obj(root)
}
