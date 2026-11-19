//! Runtime support for the declaration macros (spec node `CORE.REG.DEF.MACRO`).
//!
//! Domain crates declare commands with three macros from `fid-registry-macros`, re-exported
//! by this crate:
//!
//! - `#[derive(FidSchema)]` on a parameter or result type generates its JSON Schema 2020-12
//!   document ([`FidSchema::schema`]): fields in declaration order, `additionalProperties:
//!   false`, `required` sorted, `x-quantity` / `x-unit` from `#[fid(quantity, unit)]`. An `f64`
//!   field without a quantity does not compile unless it is marked
//!   `#[fid(dimensionless_unchecked)]` (REQ-002).
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
/// `u64`, `i64`, `usize`, `isize` and `f32` are deliberately not implemented: JSON numbers are
/// exact only up to 2^53, and the registry's numbers are `f64`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not implement `FidSchema`",
    label = "a command parameter or result type must have a schema",
    note = "add `#[derive(FidSchema)]` to the type (u64, i64, usize, isize and f32 have no schema)"
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

/// Plain `number`; a field of this type needs `#[fid(quantity = ...)]` or
/// `#[fid(dimensionless_unchecked)]` (REQ-002).
impl FidSchema for f64 {
    fn schema() -> CValue {
        CValue::Obj(typed("number"))
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
