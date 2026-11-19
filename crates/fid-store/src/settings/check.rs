//! The setting schema subset and the value validator (REQ-CORE.STO.SET-001, -002).
//!
//! A setting schema is a registry schema (`fid_registry::check_schema`) that also stays inside
//! a closed keyword subset that [`validate_value`] evaluates completely, so no keyword is ever
//! silently ignored: `pattern`, `format`, `$ref`, the combinators and every other keyword are
//! refused at declaration ([`check_setting_schema`]). Annotation keywords (`title`,
//! `description`, `default`, `examples`, `$comment`, `deprecated`, `readOnly`, `writeOnly`,
//! `$schema` and the project keywords `x-quantity`, `x-unit`, `x-ui`, `x-untrusted`) are
//! accepted and do not validate. Every `number` must carry `x-quantity` (CORE.REG.VAL's rule).
//!
//! Recursion follows the schema, never the value: it descends only through `items`,
//! `properties` and `additionalProperties`, and schemas are at most [`MAX_SCHEMA_DEPTH`] levels
//! deep, so a hostile value cannot deepen the stack.

use fid_canon::{to_canonical_bytes, CValue};
use std::collections::BTreeSet;
use std::fmt;

/// Deepest accepted setting schema (the root object is level 1).
pub const MAX_SCHEMA_DEPTH: usize = 16;

/// The keywords a setting schema may use, sorted (byte order).
pub const SUPPORTED_KEYWORDS: [&str; 31] = [
    "$comment",
    "$schema",
    "additionalProperties",
    "const",
    "default",
    "deprecated",
    "description",
    "enum",
    "examples",
    "exclusiveMaximum",
    "exclusiveMinimum",
    "items",
    "maxItems",
    "maxLength",
    "maxProperties",
    "maximum",
    "minItems",
    "minLength",
    "minProperties",
    "minimum",
    "properties",
    "readOnly",
    "required",
    "title",
    "type",
    "uniqueItems",
    "writeOnly",
    "x-quantity",
    "x-ui",
    "x-unit",
    "x-untrusted",
];

const TYPE_NAMES: [&str; 7] = ["array", "boolean", "integer", "null", "number", "object", "string"];

/// Why a value failed its setting's schema: an RFC 6901 pointer into the value and a message.
/// Neither echoes the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueError {
    pub pointer: String,
    pub message: String,
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at {:?}: {}", self.pointer, self.message)
    }
}

fn fail<T>(pointer: &str, message: impl Into<String>) -> Result<T, ValueError> {
    Err(ValueError { pointer: pointer.to_string(), message: message.into() })
}

fn segment(name: &str) -> String {
    // Property names of a setting schema are printable ASCII (check_schema); data keys are
    // escaped like that too so a pointer stays display-safe.
    let mut out = String::new();
    for c in name.chars().take(64) {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c if c.is_ascii_graphic() || c == ' ' => out.push(c),
            c => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
        }
    }
    out
}

// ------------------------------------------------------------------ schema subset

/// Checks that `schema` stays in the supported subset: an object at every schema position,
/// only [`SUPPORTED_KEYWORDS`], well-typed `type`, a quantity on every `number`, at most
/// [`MAX_SCHEMA_DEPTH`] levels. `Err` holds a pointer into the schema and a message.
pub fn check_setting_schema(schema: &CValue) -> Result<(), ValueError> {
    let mut stack: Vec<(&CValue, String, usize)> = vec![(schema, String::new(), 1)];
    while let Some((node, ptr, depth)) = stack.pop() {
        if depth > MAX_SCHEMA_DEPTH {
            return fail(&ptr, format!("setting schemas are at most {MAX_SCHEMA_DEPTH} levels deep"));
        }
        let CValue::Obj(o) = node else {
            return fail(&ptr, "a setting schema position must be an object");
        };
        for (k, v) in o {
            if SUPPORTED_KEYWORDS.binary_search(&k.as_str()).is_err() {
                return fail(&ptr, format!("keyword {:?} is not supported in setting schemas", segment(k)));
            }
            match k.as_str() {
                "items" => stack.push((v, format!("{ptr}/items"), depth + 1)),
                "additionalProperties" => {
                    if !matches!(v, CValue::Bool(_)) {
                        stack.push((v, format!("{ptr}/additionalProperties"), depth + 1));
                    }
                }
                "properties" => {
                    let CValue::Obj(props) = v else { return fail(&ptr, "properties must be an object") };
                    for (name, sub) in props {
                        stack.push((sub, format!("{ptr}/properties/{}", segment(name)), depth + 1));
                    }
                }
                _ => {}
            }
        }
        let types = match o.get("type") {
            None => Vec::new(),
            Some(CValue::Str(t)) => vec![t.as_str()],
            Some(CValue::Arr(ts)) => {
                let mut out = Vec::new();
                for t in ts {
                    let CValue::Str(t) = t else { return fail(&ptr, "type must name JSON types") };
                    out.push(t.as_str());
                }
                out
            }
            Some(_) => return fail(&ptr, "type must be a string or an array of strings"),
        };
        if types.iter().any(|t| !TYPE_NAMES.contains(t)) {
            return fail(&ptr, "type names an unknown JSON type");
        }
        if types.contains(&"number") && !matches!(o.get("x-quantity"), Some(CValue::Str(_))) {
            return fail(&ptr, "a number needs x-quantity");
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ value validation

fn as_number(v: &CValue) -> Option<f64> {
    match v {
        CValue::Num(n) | CValue::F64Bits(n) => Some(*n),
        CValue::I64(n) => Some(*n as f64),
        CValue::U64(n) => Some(*n as f64),
        _ => None,
    }
}

fn is_type(v: &CValue, t: &str) -> bool {
    match t {
        "null" => matches!(v, CValue::Null),
        "boolean" => matches!(v, CValue::Bool(_)),
        "string" => matches!(v, CValue::Str(_)),
        "array" => matches!(v, CValue::Arr(_)),
        "object" => matches!(v, CValue::Obj(_)),
        "number" => as_number(v).is_some_and(f64::is_finite),
        "integer" => match v {
            CValue::I64(_) | CValue::U64(_) => true,
            CValue::Num(n) | CValue::F64Bits(n) => n.is_finite() && n.fract() == 0.0,
            _ => false,
        },
        _ => false,
    }
}

fn usize_as_f64(n: usize) -> f64 {
    n as f64
}

/// Validates `value` against a setting schema that passed [`check_setting_schema`]. Returns
/// the first failure in a fixed order (keywords by name, array items and object members in
/// order). A non-finite number anywhere fails.
pub fn validate_value(schema: &CValue, value: &CValue) -> Result<(), ValueError> {
    validate_at(schema, value, "", 1)
}

fn validate_at(schema: &CValue, value: &CValue, ptr: &str, depth: usize) -> Result<(), ValueError> {
    if depth > MAX_SCHEMA_DEPTH {
        return fail(ptr, "schema too deep");
    }
    let CValue::Obj(o) = schema else { return fail(ptr, "schema position is not an object") };
    if let Some(n) = as_number(value) {
        if !n.is_finite() {
            return fail(ptr, "number is not finite");
        }
    }
    // Keywords in name order (BTreeMap), so the first failure is deterministic.
    for (k, kv) in o {
        match k.as_str() {
            "type" => {
                let ok = match kv {
                    CValue::Str(t) => is_type(value, t),
                    CValue::Arr(ts) => ts.iter().any(|t| matches!(t, CValue::Str(t) if is_type(value, t))),
                    _ => false,
                };
                if !ok {
                    return fail(ptr, "wrong JSON type");
                }
            }
            "enum" => {
                let CValue::Arr(options) = kv else { return fail(ptr, "enum is not an array") };
                if !options.iter().any(|x| x == value) {
                    return fail(ptr, "not one of the enumerated values");
                }
            }
            "const" if kv != value => return fail(ptr, "not the constant value"),
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                let (Some(n), Some(bound)) = (as_number(value), as_number(kv)) else { continue };
                let ok = match k.as_str() {
                    "minimum" => n >= bound,
                    "maximum" => n <= bound,
                    "exclusiveMinimum" => n > bound,
                    _ => n < bound,
                };
                if !ok {
                    return fail(ptr, format!("out of range ({k})"));
                }
            }
            "minLength" | "maxLength" => {
                let (CValue::Str(s), Some(bound)) = (value, as_number(kv)) else { continue };
                let len = usize_as_f64(s.chars().count());
                if (k == "minLength" && len < bound) || (k == "maxLength" && len > bound) {
                    return fail(ptr, format!("string length out of range ({k})"));
                }
            }
            "minItems" | "maxItems" => {
                let (CValue::Arr(a), Some(bound)) = (value, as_number(kv)) else { continue };
                let len = usize_as_f64(a.len());
                if (k == "minItems" && len < bound) || (k == "maxItems" && len > bound) {
                    return fail(ptr, format!("item count out of range ({k})"));
                }
            }
            "uniqueItems" => {
                let (CValue::Arr(a), CValue::Bool(true)) = (value, kv) else { continue };
                let mut seen = BTreeSet::new();
                for (i, item) in a.iter().enumerate() {
                    let bytes = to_canonical_bytes(item).map_err(|e| ValueError {
                        pointer: format!("{ptr}/{i}"),
                        message: format!("item has no canonical form: {}", e.code()),
                    })?;
                    if !seen.insert(bytes) {
                        return fail(&format!("{ptr}/{i}"), "duplicate item");
                    }
                }
            }
            "items" => {
                let CValue::Arr(a) = value else { continue };
                for (i, item) in a.iter().enumerate() {
                    validate_at(kv, item, &format!("{ptr}/{i}"), depth + 1)?;
                }
            }
            "minProperties" | "maxProperties" => {
                let (CValue::Obj(m), Some(bound)) = (value, as_number(kv)) else { continue };
                let len = usize_as_f64(m.len());
                if (k == "minProperties" && len < bound) || (k == "maxProperties" && len > bound) {
                    return fail(ptr, format!("member count out of range ({k})"));
                }
            }
            "required" => {
                let (CValue::Obj(m), CValue::Arr(names)) = (value, kv) else { continue };
                for n in names {
                    if let CValue::Str(n) = n {
                        if !m.contains_key(n) {
                            return fail(ptr, format!("missing member {:?}", segment(n)));
                        }
                    }
                }
            }
            "properties" => {
                let (CValue::Obj(m), CValue::Obj(props)) = (value, kv) else { continue };
                for (name, sub) in props {
                    if let Some(member) = m.get(name) {
                        validate_at(sub, member, &format!("{ptr}/{}", segment(name)), depth + 1)?;
                    }
                }
            }
            "additionalProperties" => {
                let CValue::Obj(m) = value else { continue };
                let declared = match o.get("properties") {
                    Some(CValue::Obj(p)) => Some(p),
                    _ => None,
                };
                for (name, member) in m {
                    if declared.is_some_and(|p| p.contains_key(name)) {
                        continue;
                    }
                    let at = format!("{ptr}/{}", segment(name));
                    match kv {
                        CValue::Bool(false) => return fail(&at, "member not allowed"),
                        CValue::Bool(true) => {}
                        sub => validate_at(sub, member, &at, depth + 1)?,
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
