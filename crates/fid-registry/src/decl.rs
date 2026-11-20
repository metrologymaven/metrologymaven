//! Runtime support for the declaration macros (spec node `CORE.REG.DEF.MACRO`).
//!
//! Domain crates declare commands with three macros from `fid-registry-macros`, re-exported
//! by this crate:
//!
//! - `#[derive(FidSchema)]` on a parameter or result type generates its JSON Schema 2020-12
//!   document ([`FidSchema::schema`]): fields in declaration order, `additionalProperties:
//!   false`, `required` sorted, `x-quantity` / `x-unit` from `#[fid(quantity, unit)]`. A field
//!   whose type is or contains `f64` must declare a quantity (`quantity = "dimensionless"` for
//!   a pure ratio), or it does not compile (REQ-002). This is enforced by the types, not by
//!   reading the source text: `f64` has no [`FidSchema`] at all, only [`rt::FidNumber`], and
//!   the derive asks for `FidNumber` only on a field that declares a quantity, so a type alias
//!   of `f64` cannot slip through.
//! - `#[fid_command(...)]` on a handler `fn(Ctx, P) -> Result<R, E>` generates a hidden
//!   `__fid_command_<name>()` that returns a [`CommandDecl`]: the [`CommandDef`] built from
//!   the attribute's policy fields, with `params_schema` from `P` and `result_schema` from
//!   `R` (REQ-003), plus a [`HandlerRef`] to the handler itself.
//! - `fid_commands![a, b, ...]` generates `pub fn declarations() -> Vec<CommandDecl>` in
//!   source order (REQ-006). It is one ordinary function: no `unsafe`, no static
//!   constructors, no linker sections.
//!
//! Everything a declaration needs is checked when the crate compiles: missing policy fields,
//! malformed IDs, step bounds and message keys, `web_note` for a web-restricted command, and
//! types that do not implement [`FidSchema`]. The generated code therefore builds the
//! [`CommandDef`] directly and cannot fail at run time; `CORE.REG.VAL` still validates the
//! whole set at registry build.
//!
//! The helpers in [`rt`] are what the generated code calls; they are public only for that.

use crate::def::CommandDef;
use fid_canon::CValue;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A type that has a parameter or result schema. Implement it with `#[derive(FidSchema)]`;
/// it is implemented here for the JSON scalar types and the standard containers.
///
/// `f64` is deliberately not implemented: a number field must declare its quantity, and then
/// goes through [`rt::FidNumber`] (REQ-002). `u64`, `i64`, `usize`, `isize` and `f32` are not
/// implemented either: JSON numbers are exact only up to 2^53, and the registry's numbers are
/// `f64`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not implement `FidSchema`",
    label = "a command parameter or result type must have a schema",
    note = "add `#[derive(FidSchema)]` to the type; an f64 field needs #[fid(quantity = \"...\")]             (u64, i64, usize, isize and f32 have no schema)"
)]
pub trait FidSchema {
    /// The JSON Schema 2020-12 document of this type.
    fn schema() -> CValue;
}

fn typed(t: &str) -> BTreeMap<String, CValue> {
    let mut o = BTreeMap::new();
    o.insert("type".to_string(), CValue::Str(t.to_string()));
    o
}

impl FidSchema for bool {
    fn schema() -> CValue {
        CValue::Obj(typed("boolean"))
    }
}

impl FidSchema for String {
    fn schema() -> CValue {
        CValue::Obj(typed("string"))
    }
}

macro_rules! integer_schema {
    ($($t:ty),+) => {
        $(
            /// `integer` with the type's bounds.
            impl FidSchema for $t {
                fn schema() -> CValue {
                    let mut o = typed("integer");
                    o.insert("minimum".to_string(), CValue::Num(f64::from(<$t>::MIN)));
                    o.insert("maximum".to_string(), CValue::Num(f64::from(<$t>::MAX)));
                    CValue::Obj(o)
                }
            }
        )+
    };
}

integer_schema!(u8, u16, u32, i8, i16, i32);

/// The schema of `T`; the derive leaves an `Option` field out of `required`. A value is
/// absent, never `null`.
impl<T: FidSchema> FidSchema for Option<T> {
    fn schema() -> CValue {
        T::schema()
    }
}

/// `array` of `T`.
impl<T: FidSchema> FidSchema for Vec<T> {
    fn schema() -> CValue {
        let mut o = typed("array");
        o.insert("items".to_string(), T::schema());
        CValue::Obj(o)
    }
}

/// `array` of distinct `T` (`uniqueItems`).
impl<T: FidSchema> FidSchema for BTreeSet<T> {
    fn schema() -> CValue {
        let mut o = typed("array");
        o.insert("items".to_string(), T::schema());
        o.insert("uniqueItems".to_string(), CValue::Bool(true));
        CValue::Obj(o)
    }
}

/// A reference to a command handler: the handler as a typed `fn` pointer
/// (`fn(Ctx, P) -> Result<R, E>`), erased to [`Any`] so declarations of different types fit
/// one list, and recovered by the bus with [`HandlerRef::get`] for the exact pointer type.
/// No `unsafe` and no transmute: a lookup with the wrong type returns `None`.
pub struct HandlerRef {
    path: &'static str,
    ptr: Box<dyn Any + Send + Sync>,
}

impl HandlerRef {
    /// Wraps a handler pointer. `path` is the handler's name as written in the crate.
    pub fn new<F: Any + Send + Sync>(path: &'static str, ptr: F) -> HandlerRef {
        HandlerRef { path, ptr: Box::new(ptr) }
    }

    /// The handler's name as written in the declaring crate.
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// The handler pointer, if `F` is its exact type.
    pub fn get<F: Any>(&self) -> Option<&F> {
        self.ptr.downcast_ref::<F>()
    }
}

impl fmt::Debug for HandlerRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandlerRef").field("path", &self.path).finish_non_exhaustive()
    }
}

/// One declared command: its definition and its handler (`CORE.REG.DEF.MACRO` Design).
///
/// Not to be confused with [`crate::validate::CommandDecl`], the raw JSON declaration that
/// `CORE.REG.VAL` checks; [`CommandDecl::to_validation`] converts.
#[derive(Debug)]
pub struct CommandDecl {
    pub def: CommandDef,
    pub handler: HandlerRef,
}

impl CommandDecl {
    /// The raw declaration `CORE.REG.VAL` validates.
    pub fn to_validation(&self) -> crate::validate::CommandDecl {
        crate::validate::CommandDecl::from_def(&self.def)
    }
}

/// Helpers called by the generated code. Not a stable API of their own.
pub mod rt {
    use super::FidSchema;
    pub use crate::def::{
        CommandDef, Determinism, FsEffect, Idempotency, Network, Precondition, Projection, SideEffect, Surface, Undo,
        Web,
    };
    pub use fid_canon::CValue;
    pub use fid_kci::AlgorithmId;
    use std::collections::BTreeMap;

    /// Builder for an object schema.
    #[derive(Debug, Default)]
    pub struct Object {
        properties: BTreeMap<String, CValue>,
        required: Vec<String>,
        extra: BTreeMap<String, CValue>,
    }

    impl Object {
        /// An empty object schema.
        pub fn new() -> Object {
            Object::default()
        }

        /// Adds a property, required or not.
        pub fn field(&mut self, name: &str, schema: CValue, required: bool) {
            self.properties.insert(name.to_string(), schema);
            if required {
                self.required.push(name.to_string());
            }
        }

        /// Sets a keyword of the object schema itself (`title`, `description`).
        pub fn set(&mut self, key: &str, value: CValue) {
            self.extra.insert(key.to_string(), value);
        }

        /// `{"type": "object", "additionalProperties": false, "properties": ..., "required":
        /// [sorted]}`; `required` is left out when empty.
        pub fn finish(self) -> CValue {
            let mut o = self.extra;
            o.insert("type".to_string(), CValue::Str("object".to_string()));
            o.insert("additionalProperties".to_string(), CValue::Bool(false));
            o.insert("properties".to_string(), CValue::Obj(self.properties));
            let mut required = self.required;
            required.sort();
            required.dedup();
            if !required.is_empty() {
                o.insert("required".to_string(), CValue::Arr(required.into_iter().map(CValue::Str).collect()));
            }
            CValue::Obj(o)
        }
    }

    /// `{"type": "string", "enum": [...]}` with the values in declaration order.
    pub fn string_enum(values: &[&str]) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("type".to_string(), CValue::Str("string".to_string()));
        o.insert("enum".to_string(), CValue::Arr(values.iter().map(|v| CValue::Str((*v).to_string())).collect()));
        CValue::Obj(o)
    }

    /// The schema of `T`.
    pub fn schema_of<T: FidSchema + ?Sized>() -> CValue {
        T::schema()
    }

    /// A type a field with `#[fid(quantity = ...)]` may have: `f64`, the integer types with a
    /// schema, and `Option` or `Vec` of those. The derive uses this, not [`FidSchema`], for a
    /// field that declares a quantity; `f64` has only this trait (REQ-002).
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot carry a quantity",
        label = "a field with #[fid(quantity)] must be a number, or an Option or Vec of numbers"
    )]
    pub trait FidNumber {
        /// The schema of the number, before `x-quantity` and `x-unit` are added.
        fn number_schema() -> CValue;
    }

    impl FidNumber for f64 {
        fn number_schema() -> CValue {
            CValue::Obj(super::typed("number"))
        }
    }

    macro_rules! integer_number {
        ($($t:ty),+) => { $(impl FidNumber for $t { fn number_schema() -> CValue { <$t as FidSchema>::schema() } })+ };
    }
    integer_number!(u8, u16, u32, i8, i16, i32);

    impl<T: FidNumber> FidNumber for Option<T> {
        fn number_schema() -> CValue {
            T::number_schema()
        }
    }

    impl<T: FidNumber> FidNumber for Vec<T> {
        fn number_schema() -> CValue {
            let mut o = super::typed("array");
            o.insert("items".to_string(), T::number_schema());
            CValue::Obj(o)
        }
    }

    /// The schema of a field that declares a quantity.
    pub fn number_schema_of<T: FidNumber + ?Sized>() -> CValue {
        T::number_schema()
    }

    /// A type a field with `#[fid(path_like)]` may have: `String`, or `Option<String>` for an
    /// optional file parameter. A file handle is one string (CORE.BUS.POL.FS), never a number,
    /// a list or an object.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a path_like parameter",
        label = "a field with #[fid(path_like)] must be a String or an Option<String>"
    )]
    pub trait PathLikeField {}

    impl PathLikeField for String {}

    impl PathLikeField for Option<String> {}

    /// The schema of a `path_like` field, before `x-extensions` is added:
    /// `{"path_like": true, "type": "string"}`.
    pub fn path_like_schema_of<T: PathLikeField + ?Sized>() -> CValue {
        let mut o = super::typed("string");
        o.insert("path_like".to_string(), CValue::Bool(true));
        CValue::Obj(o)
    }

    /// Literal kinds of `#[fid(default = ...)]`.
    #[derive(Debug)]
    pub struct BoolLit;
    /// A string literal.
    #[derive(Debug)]
    pub struct StrLit;
    /// An integer literal, possibly negative.
    #[derive(Debug)]
    pub struct IntLit;
    /// A float literal, possibly negative.
    #[derive(Debug)]
    pub struct FloatLit;

    /// Field types that accept a `default` literal of kind `L`. The literal is converted to
    /// [`Self::Value`] by the compiler, so a value out of the type's range or of the wrong kind
    /// (`default = "yes"` on a `bool`, `default = 300` or `-1` on a `u8`) does not compile.
    #[diagnostic::on_unimplemented(
        message = "this `default` literal does not fit a field of type `{Self}`",
        label = "the default must be a literal of the field's type"
    )]
    pub trait AcceptsDefault<L> {
        /// The Rust type the literal is checked as.
        type Value;
        /// The JSON value of the default.
        fn default_value(v: Self::Value) -> CValue;
    }

    impl AcceptsDefault<BoolLit> for bool {
        type Value = bool;
        fn default_value(v: bool) -> CValue {
            CValue::Bool(v)
        }
    }

    impl AcceptsDefault<StrLit> for String {
        type Value = &'static str;
        fn default_value(v: &'static str) -> CValue {
            CValue::Str(v.to_string())
        }
    }

    macro_rules! integer_default {
        ($($t:ty),+) => {
            $(impl AcceptsDefault<IntLit> for $t {
                type Value = $t;
                fn default_value(v: $t) -> CValue { CValue::Num(f64::from(v)) }
            })+
        };
    }
    integer_default!(u8, u16, u32, i8, i16, i32);

    /// An integer default on an `f64` field is checked as `i32`, so it is exact.
    impl AcceptsDefault<IntLit> for f64 {
        type Value = i32;
        fn default_value(v: i32) -> CValue {
            CValue::Num(f64::from(v))
        }
    }

    impl AcceptsDefault<FloatLit> for f64 {
        type Value = f64;
        fn default_value(v: f64) -> CValue {
            CValue::Num(v)
        }
    }

    impl<L, T: AcceptsDefault<L>> AcceptsDefault<L> for Option<T> {
        type Value = T::Value;
        fn default_value(v: T::Value) -> CValue {
            T::default_value(v)
        }
    }

    /// Sets `key` on the schema itself (container keywords: `description`, `default`,
    /// `minItems`, `maxItems`).
    pub fn set(schema: &mut CValue, key: &str, value: CValue) {
        if let CValue::Obj(o) = schema {
            o.insert(key.to_string(), value);
        }
    }

    /// Sets `key` on the element schema: through the `items` of arrays down to the first
    /// schema that is not an array (value keywords: `x-quantity`, `x-unit`, `minimum`,
    /// `maximum`, `minLength`, `maxLength`, `pattern`, `x-untrusted`). Iterative.
    pub fn set_leaf(schema: &mut CValue, key: &str, value: CValue) {
        let mut at = schema;
        loop {
            let CValue::Obj(o) = at else { return };
            let is_array = matches!(o.get("type"), Some(CValue::Str(t)) if t == "array") && o.contains_key("items");
            if !is_array {
                o.insert(key.to_string(), value);
                return;
            }
            match o.get_mut("items") {
                Some(items) => at = items,
                None => return,
            }
        }
    }

    /// A string value.
    pub fn text<S: AsRef<str> + ?Sized>(s: &S) -> CValue {
        CValue::Str(s.as_ref().to_string())
    }

    /// A string.
    pub fn string<S: AsRef<str> + ?Sized>(s: &S) -> String {
        s.as_ref().to_string()
    }

    /// A number value for a schema keyword (`minimum`, `maxLength`, ...).
    pub fn number<N: SchemaNumber>(n: N) -> CValue {
        CValue::Num(n.to_f64())
    }

    /// Numbers a schema keyword may take. Integers above 2^53 are not exact in JSON; the
    /// registry's schema checker refuses them (`bad_number`).
    pub trait SchemaNumber {
        fn to_f64(self) -> f64;
    }

    macro_rules! lossless {
        ($($t:ty),+) => { $(impl SchemaNumber for $t { fn to_f64(self) -> f64 { f64::from(self) } })+ };
    }
    lossless!(u8, u16, u32, i8, i16, i32, f64);

    macro_rules! wide {
        ($($t:ty),+) => {
            $(impl SchemaNumber for $t {
                #[allow(clippy::cast_precision_loss)]
                fn to_f64(self) -> f64 { self as f64 }
            })+
        };
    }
    wide!(u64, i64, usize, isize);
}
