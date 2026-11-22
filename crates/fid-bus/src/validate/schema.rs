//! A bounded JSON Schema 2020-12 validator for parameter documents, written for the bus.
//!
//! [`Compiled::compile`] turns one schema into an arena of nodes once (patterns compiled,
//! `$ref` resolved, every keyword checked); [`Compiled::evaluate`] validates an instance
//! against it. Every keyword of the registry vocabulary (`CORE.REG.DEF.SCHEMA`) is either
//! asserted, applied, or an annotation that 2020-12 itself never asserts (`title`,
//! `description`, `default`, `examples`, `deprecated`, `readOnly`, `writeOnly`, `$comment`,
//! `$anchor`, `$dynamicAnchor`, `$vocabulary`, `contentEncoding`, `contentMediaType`,
//! `contentSchema`, and the project annotations `x-unit`, `x-ui`, `x-untrusted`,
//! `x-standards-ref`, `path_like`, `x-extensions`, `value_of_record`, which later stages
//! read). Anything else (an unknown keyword, an unasserted `format`, `$dynamicRef`, a
//! `$ref` that does not land on a schema position, a pattern outside the ECMA-262 subset)
//! fails compilation, so the command refuses every call rather than skip a keyword (fail
//! closed).
//!
//! A literal is normalised for the keywords of the schema object holding `x-quantity` only, so
//! compilation also refuses a numeric keyword in a schema object with neither its own
//! `x-quantity` nor its own `type` excluding `object`, and `x-quantity` under `contains` or
//! `not` (`quantity_rule`).
//!
//! # First failure in document order
//!
//! Every keyword reports its own first failure; a schema reports the earliest of them by
//! (instance pointer in document order, keyword rank in [`RANK`]), the first evaluated
//! winning a tie (keywords are evaluated in a fixed order). Keywords under which a failure
//! is reported at the instance itself: the assertions, `anyOf`, `oneOf`, `not`, `contains`
//! (`minContains`, `maxContains`), `required`, `dependentRequired`, `uniqueItems`. A
//! subschema's own failure is passed up unchanged by `$ref`, `allOf`, `then`, `else`,
//! `dependentSchemas` and the child applicators (`prefixItems`, `items`, `properties`,
//! `patternProperties`, `additionalProperties`, `unevaluated*`); a `false` subschema
//! fails under the keyword that applied it (`false` for a `false` root). `propertyNames`
//! fails at the member it rejects. `unevaluatedItems` and `unevaluatedProperties` run only
//! when every other keyword of their schema object passed.
//!
//! # Bounds
//!
//! The schema: at most [`MAX_SCHEMA_DEPTH`] levels and [`MAX_SCHEMA_NODES`] schema
//! positions. Evaluation: at most [`MAX_EVAL_STEPS`] steps (one per schema applied to one
//! value, one per object member or array element each keyword looks at, one per 1,024 bytes
//! of string or canonical value examined, one per KiB of input times the program size in KiB
//! per pattern match, and [`REGEX_FORMAT_STEPS`] plus one per byte for each `format: "regex"`
//! check, of which there are at most [`MAX_REGEX_FORMAT_CHECKS`]) and
//! [`MAX_EVAL_DEPTH`] nested schema applications; beyond either, validation stops with
//! `input_limit` and nothing under `anyOf`, `oneOf`, `not` or `if` can turn that into a
//! pass. Patterns are linear-time (module `ecma`).

use super::ecma::Pattern;
use super::format::Format;
use super::pointer::{cmp_utf16, JsonPointer};
use crate::error::{BusError, ValidationReason};
use crate::unit::Normalised;
use fid_canon::{format_number, to_canonical_bytes, CValue};
use fid_registry::{EntityType, SelectorForm};
use std::collections::{BTreeMap, BTreeSet};

/// The only accepted `$schema`.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// Deepest schema nesting compiled (the schema object is level 1).
pub const MAX_SCHEMA_DEPTH: usize = 64;
/// Most schema positions one schema may hold.
pub const MAX_SCHEMA_NODES: usize = 65_536;
/// Most evaluation steps for one parameter document.
pub const MAX_EVAL_STEPS: u64 = 4_000_000;
/// Most nested schema applications.
pub const MAX_EVAL_DEPTH: usize = 128;
/// Bytes of string or canonical value examined per extra step.
const BYTES_PER_STEP: usize = 1024;
/// Most `format: "regex"` checks (each compiles a pattern) in one parameter document.
pub const MAX_REGEX_FORMAT_CHECKS: usize = 64;
/// Fixed step charge of one `format: "regex"` check, on top of one step per pattern byte.
pub const REGEX_FORMAT_STEPS: u64 = 1024;

/// Keyword ranks: the order keywords are evaluated in and the tie-break between failures
/// at the same instance pointer. `false`, `json` and `input_limit` are pseudo-keywords.
pub const RANK: [&str; 46] = [
    "json",
    "input_limit",
    "false",
    "x-quantity",
    "type",
    "const",
    "enum",
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "format",
    "x-ref",
    "x-selector",
    "maxItems",
    "minItems",
    "uniqueItems",
    "contains",
    "minContains",
    "maxContains",
    "maxProperties",
    "minProperties",
    "required",
    "dependentRequired",
    "$ref",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
    "propertyNames",
    "prefixItems",
    "items",
    "properties",
    "patternProperties",
    "additionalProperties",
    "unevaluatedItems",
    "unevaluatedProperties",
    "schema",
];

fn rank(keyword: &str) -> usize {
    RANK.iter().position(|k| *k == keyword).unwrap_or(RANK.len())
}

// ------------------------------------------------------------------ failures

/// The first failure of a parameter document.
#[derive(Debug, Clone, PartialEq)]
pub struct Fail {
    /// The failing value (for `required`, the object that lacks the member).
    pub pointer: JsonPointer,
    /// The failed keyword (an entry of [`RANK`]).
    pub keyword: &'static str,
    pub reason: ValidationReason,
    /// Fixed text; never echoes the instance.
    pub message: String,
    /// A non-validation bus error from a hook (a broken invariant), returned as it is.
    pub bus: Option<BusError>,
}

impl Fail {
    pub(crate) fn new(pointer: &JsonPointer, keyword: &'static str, reason: ValidationReason, message: &str) -> Fail {
        Fail { pointer: pointer.clone(), keyword, reason, message: message.to_string(), bus: None }
    }

    fn schema(pointer: &JsonPointer, keyword: &'static str, message: &str) -> Fail {
        Fail::new(pointer, keyword, ValidationReason::Schema, message)
    }

    /// Whether `self` comes strictly before `other`.
    fn before(&self, other: &Fail) -> bool {
        match self.pointer.doc_cmp(&other.pointer) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => rank(self.keyword) < rank(other.keyword),
        }
    }
}

fn note(best: &mut Option<Fail>, f: Fail) {
    match best {
        Some(b) if !f.before(b) => {}
        _ => *best = Some(f),
    }
}

/// A numeric bound: its limit, keyword and the test a passing value meets.
type Bound = (Option<f64>, &'static str, fn(f64, f64) -> bool);

/// Validation stopped: a limit was reached or a hook hit a broken invariant. Never turned
/// into a pass by `anyOf`, `oneOf`, `not` or `if`.
struct Abort(Box<Fail>);

type Step = Result<Result<Out, Fail>, Abort>;

// ------------------------------------------------------------------ hooks

/// What an `x-ref` lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefLookup {
    /// The text is not a valid entity ID.
    BadId,
    /// No entity has this ID in the state view.
    Missing,
    /// The entity exists with this kind (e.g. `feature.plane`).
    Kind(String),
}

/// The project keywords' access to the outside: the unit catalogue and the state view.
pub trait Hooks {
    /// Normalises an authored literal object for quantity kind `kind` (`CORE.BUS.UNIT`).
    fn literal(&self, kind: &str, value: &CValue) -> Result<Normalised, BusError>;
    /// Looks an entity up in the state view.
    fn entity(&self, id: &str) -> RefLookup;
}

// ------------------------------------------------------------------ compiled form

type NodeId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JType {
    Null,
    Boolean,
    Object,
    Array,
    Number,
    Integer,
    String,
}

impl JType {
    fn parse(s: &str) -> Option<JType> {
        Some(match s {
            "null" => JType::Null,
            "boolean" => JType::Boolean,
            "object" => JType::Object,
            "array" => JType::Array,
            "number" => JType::Number,
            "integer" => JType::Integer,
            "string" => JType::String,
            _ => return None,
        })
    }

    fn matches(self, v: &CValue) -> bool {
        match (self, v) {
            (JType::Null, CValue::Null)
            | (JType::Boolean, CValue::Bool(_))
            | (JType::Object, CValue::Obj(_))
            | (JType::Array, CValue::Arr(_))
            | (JType::Number, CValue::Num(_))
            | (JType::String, CValue::Str(_)) => true,
            (JType::Integer, CValue::Num(x)) => x.is_finite() && x.fract() == 0.0,
            _ => false,
        }
    }
}

/// A finite decimal `coeff * 10^exp`, `coeff` without trailing zeros (0 for zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dec {
    coeff: u64,
    exp: i64,
}

impl Dec {
    /// The decimal of a finite `f64`'s shortest round-trip form (ECMAScript `Number::toString`).
    fn of(x: f64) -> Option<Dec> {
        let s = format_number(x).ok()?;
        let s = s.strip_prefix('-').unwrap_or(&s);
        let (mant, exp) = match s.split_once('e') {
            Some((m, e)) => (m, e.strip_prefix('+').unwrap_or(e).parse::<i64>().ok()?),
            None => (s, 0),
        };
        let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
        let digits: String = int.chars().chain(frac.chars()).collect();
        // Trailing zeros go into the exponent before parsing (`1e20` prints as 21 digits),
        // leading zeros are dropped; what is left is at most 17 significant digits.
        let trimmed = digits.trim_end_matches('0');
        let exp = exp - frac.len() as i64 + (digits.len() - trimmed.len()) as i64;
        let significant = trimmed.trim_start_matches('0');
        if significant.is_empty() {
            return Some(Dec { coeff: 0, exp: 0 });
        }
        Some(Dec { coeff: significant.parse().ok()?, exp })
    }

    /// Whether `self / m` is an integer, exactly, in decimal.
    fn is_multiple_of(self, m: Dec) -> bool {
        if self.coeff == 0 {
            return true;
        }
        if m.coeff == 0 || self.exp < m.exp {
            // `self.coeff` has no factor 10, so it cannot absorb a power of ten.
            return false;
        }
        let modulus = u128::from(m.coeff);
        let mut r = u128::from(self.coeff) % modulus;
        let mut k = self.exp - m.exp;
        while k > 0 && r != 0 {
            r = r * 10 % modulus;
            k -= 1;
        }
        r == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selector {
    Single,
    List,
}

#[derive(Debug, Default)]
struct Obj {
    reference: Option<NodeId>,
    types: Option<Vec<JType>>,
    konst: Option<Vec<u8>>,
    enumeration: Option<Vec<Vec<u8>>>,
    multiple_of: Option<Dec>,
    maximum: Option<f64>,
    exclusive_maximum: Option<f64>,
    minimum: Option<f64>,
    exclusive_minimum: Option<f64>,
    max_length: Option<u64>,
    min_length: Option<u64>,
    pattern: Option<Pattern>,
    format: Option<Format>,
    max_items: Option<u64>,
    min_items: Option<u64>,
    unique_items: bool,
    contains: Option<NodeId>,
    min_contains: Option<u64>,
    max_contains: Option<u64>,
    max_properties: Option<u64>,
    min_properties: Option<u64>,
    required: Vec<String>,
    dependent_required: Vec<(String, Vec<String>)>,
    all_of: Vec<NodeId>,
    any_of: Vec<NodeId>,
    one_of: Vec<NodeId>,
    not: Option<NodeId>,
    if_: Option<NodeId>,
    then_: Option<NodeId>,
    else_: Option<NodeId>,
    dependent_schemas: Vec<(String, NodeId)>,
    property_names: Option<NodeId>,
    prefix_items: Vec<NodeId>,
    items: Option<NodeId>,
    properties: BTreeMap<String, NodeId>,
    pattern_properties: Vec<(Pattern, NodeId)>,
    additional_properties: Option<NodeId>,
    unevaluated_items: Option<NodeId>,
    unevaluated_properties: Option<NodeId>,
    quantity: Option<String>,
    x_ref: Option<EntityType>,
    selector: Option<Selector>,
}

#[derive(Debug)]
enum Node {
    Bool(bool),
    Obj(Box<Obj>),
}

/// Why a schema does not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// RFC 6901 pointer into the schema.
    pub pointer: String,
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at {:?}: {}", self.pointer, self.message)
    }
}

/// A compiled schema.
#[derive(Debug)]
pub struct Compiled {
    nodes: Vec<Node>,
    root: NodeId,
}

fn show(path: &[String]) -> String {
    path.iter().map(|t| format!("/{}", t.replace('~', "~0").replace('/', "~1"))).collect()
}

fn count(v: &CValue) -> Option<u64> {
    const MAX: f64 = 9_007_199_254_740_992.0;
    match v {
        CValue::Num(x) if x.fract() == 0.0 && (0.0..=MAX).contains(x) => Some(*x as u64),
        CValue::I64(x) if (0..=(1i64 << 53)).contains(x) => Some(*x as u64),
        CValue::U64(x) if *x <= 1u64 << 53 => Some(*x),
        _ => None,
    }
}

fn number(v: &CValue) -> Option<f64> {
    match v {
        CValue::Num(x) if x.is_finite() => Some(*x),
        CValue::I64(x) if x.unsigned_abs() <= 1u64 << 53 => Some(*x as f64),
        CValue::U64(x) if *x <= 1u64 << 53 => Some(*x as f64),
        _ => None,
    }
}

/// Parses a `$ref` value: `#` or `#/` and an RFC 6901 pointer without `%`.
fn ref_target(s: &str) -> Option<Vec<String>> {
    let rest = s.strip_prefix('#')?;
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let rest = rest.strip_prefix('/')?;
    if rest.contains('%') {
        return None;
    }
    rest.split('/')
        .map(|t| {
            let b = t.as_bytes();
            let ok = b.iter().enumerate().all(|(i, c)| *c != b'~' || matches!(b.get(i + 1), Some(b'0' | b'1')));
            ok.then(|| t.replace("~1", "/").replace("~0", "~"))
        })
        .collect()
}

struct Builder {
    nodes: Vec<Node>,
    positions: BTreeMap<Vec<String>, NodeId>,
    refs: Vec<(NodeId, Vec<String>, Vec<String>)>,
}

impl Builder {
    fn err(path: &[String], message: &str) -> CompileError {
        CompileError { pointer: show(path), message: message.to_string() }
    }

    fn child(
        &mut self,
        v: &CValue,
        path: &mut Vec<String>,
        step: &[&str],
        depth: usize,
    ) -> Result<NodeId, CompileError> {
        for s in step {
            path.push((*s).to_string());
        }
        let r = self.build(v, path, depth + 1);
        for _ in step {
            path.pop();
        }
        r
    }

    fn list(
        &mut self,
        v: &CValue,
        path: &mut Vec<String>,
        kw: &str,
        depth: usize,
    ) -> Result<Vec<NodeId>, CompileError> {
        let CValue::Arr(items) = v else { return Err(Builder::err(path, "must be a non-empty array of schemas")) };
        if items.is_empty() {
            return Err(Builder::err(path, "must be a non-empty array of schemas"));
        }
        items
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let idx = i.to_string();
                self.child(s, path, &[kw, idx.as_str()], depth + 1)
            })
            .collect()
    }

    fn map(
        &mut self,
        v: &CValue,
        path: &mut Vec<String>,
        kw: &str,
        depth: usize,
    ) -> Result<Vec<(String, NodeId)>, CompileError> {
        let CValue::Obj(m) = v else { return Err(Builder::err(path, "must be an object of schemas")) };
        m.iter().map(|(k, s)| Ok((k.clone(), self.child(s, path, &[kw, k.as_str()], depth + 1)?))).collect()
    }

    fn strings(v: &CValue, path: &[String]) -> Result<Vec<String>, CompileError> {
        let CValue::Arr(items) = v else { return Err(Builder::err(path, "must be an array of strings")) };
        let mut out = Vec::new();
        for i in items {
            let CValue::Str(s) = i else { return Err(Builder::err(path, "must be an array of strings")) };
            if out.contains(s) {
                return Err(Builder::err(path, "must hold distinct strings"));
            }
            out.push(s.clone());
        }
        Ok(out)
    }

    fn build(&mut self, v: &CValue, path: &mut Vec<String>, depth: usize) -> Result<NodeId, CompileError> {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(Builder::err(path, "schema nests deeper than 64 levels"));
        }
        if self.nodes.len() >= MAX_SCHEMA_NODES {
            return Err(Builder::err(path, "schema has more than 65536 positions"));
        }
        let id = self.nodes.len();
        self.nodes.push(Node::Bool(true));
        self.positions.insert(path.clone(), id);
        let node = match v {
            CValue::Bool(b) => Node::Bool(*b),
            CValue::Obj(m) => Node::Obj(Box::new(self.object(m, id, path, depth)?)),
            _ => return Err(Builder::err(path, "a schema must be an object or a boolean")),
        };
        if let Some(slot) = self.nodes.get_mut(id) {
            *slot = node;
        }
        Ok(id)
    }

    fn object(
        &mut self,
        m: &BTreeMap<String, CValue>,
        id: NodeId,
        path: &mut Vec<String>,
        depth: usize,
    ) -> Result<Obj, CompileError> {
        let mut o = Obj::default();
        for (k, v) in m {
            path.push(k.clone());
            let r = self.keyword(&mut o, k, v, id, path, depth);
            path.pop();
            r?;
        }
        Ok(o)
    }

    fn keyword(
        &mut self,
        o: &mut Obj,
        k: &str,
        v: &CValue,
        id: NodeId,
        path: &[String],
        depth: usize,
    ) -> Result<(), CompileError> {
        let at_root = path.len() == 1;
        // `path` ends with `k`; children are built relative to the schema object.
        let mut base: Vec<String> = path.to_vec();
        base.pop();
        let bad = |what: &str| Builder::err(path, what);
        let num = |v: &CValue| number(v).ok_or_else(|| bad("must be a number"));
        let cnt = |v: &CValue| count(v).ok_or_else(|| bad("must be an integer 0..=2^53"));
        match k {
            "$schema" => {
                if !at_root || *v != CValue::Str(DIALECT.to_string()) {
                    return Err(bad("$schema must be the 2020-12 dialect, at the root only"));
                }
            }
            "$id" => match v {
                CValue::Str(s) if at_root && !s.contains('#') => {}
                _ => return Err(bad("$id must be a string without `#`, at the root only")),
            },
            "$comment" | "$anchor" | "$dynamicAnchor" | "$vocabulary" | "title" | "description" | "default"
            | "examples" | "deprecated" | "readOnly" | "writeOnly" | "contentEncoding" | "contentMediaType"
            | "x-unit" | "x-ui" | "x-untrusted" | "x-standards-ref" | "path_like" | "x-extensions"
            | "value_of_record" => {}
            "contentSchema" => {
                // An annotation: compiled so a `$ref` may land on it, never applied.
                self.child(v, &mut base, &[k], depth)?;
            }
            "$defs" => {
                self.map(v, &mut base, k, depth)?;
            }
            "$ref" => {
                let target = match v {
                    CValue::Str(s) => ref_target(s),
                    _ => None,
                }
                .ok_or_else(|| bad("$ref must be `#` or `#/` and a JSON pointer into this schema"))?;
                self.refs.push((id, target, path.to_vec()));
            }
            "$dynamicRef" => return Err(bad("$dynamicRef is not supported")),
            "type" => {
                let names = match v {
                    CValue::Str(s) => vec![s.clone()],
                    CValue::Arr(_) => Builder::strings(v, path)?,
                    _ => Vec::new(),
                };
                let types: Option<Vec<JType>> = names.iter().map(|n| JType::parse(n)).collect();
                match types {
                    Some(t) if !t.is_empty() => o.types = Some(t),
                    _ => return Err(bad("type must name JSON types")),
                }
            }
            "const" => o.konst = Some(to_canonical_bytes(v).map_err(|_| bad("const has no canonical form"))?),
            "enum" => {
                let CValue::Arr(items) = v else { return Err(bad("enum must be an array")) };
                let canon: Result<Vec<Vec<u8>>, _> = items.iter().map(to_canonical_bytes).collect();
                o.enumeration = Some(canon.map_err(|_| bad("enum has no canonical form"))?);
            }
            "multipleOf" => {
                let x = num(v)?;
                match Dec::of(x) {
                    Some(d) if x > 0.0 && d.coeff != 0 => o.multiple_of = Some(d),
                    _ => return Err(bad("multipleOf must be a positive number")),
                }
            }
            "maximum" => o.maximum = Some(num(v)?),
            "exclusiveMaximum" => o.exclusive_maximum = Some(num(v)?),
            "minimum" => o.minimum = Some(num(v)?),
            "exclusiveMinimum" => o.exclusive_minimum = Some(num(v)?),
            "maxLength" => o.max_length = Some(cnt(v)?),
            "minLength" => o.min_length = Some(cnt(v)?),
            "maxItems" => o.max_items = Some(cnt(v)?),
            "minItems" => o.min_items = Some(cnt(v)?),
            "maxContains" => o.max_contains = Some(cnt(v)?),
            "minContains" => o.min_contains = Some(cnt(v)?),
            "maxProperties" => o.max_properties = Some(cnt(v)?),
            "minProperties" => o.min_properties = Some(cnt(v)?),
            "pattern" => {
                let CValue::Str(s) = v else { return Err(bad("pattern must be a string")) };
                o.pattern = Some(Pattern::compile(s).map_err(|e| bad(&e))?);
            }
            "format" => {
                let f = match v {
                    CValue::Str(s) => Format::parse(s),
                    _ => None,
                };
                o.format = Some(f.ok_or_else(|| bad("format is not one the bus asserts"))?);
            }
            "uniqueItems" => match v {
                CValue::Bool(b) => o.unique_items = *b,
                _ => return Err(bad("uniqueItems must be a boolean")),
            },
            "required" => o.required = Builder::strings(v, path)?,
            "dependentRequired" => {
                let CValue::Obj(m) = v else { return Err(bad("dependentRequired must be an object")) };
                for (name, reqs) in m {
                    o.dependent_required.push((name.clone(), Builder::strings(reqs, path)?));
                }
            }
            "items" => {
                if matches!(v, CValue::Arr(_)) {
                    return Err(bad("the array form of items is not 2020-12"));
                }
                o.items = Some(self.child(v, &mut base, &[k], depth)?);
            }
            "prefixItems" => o.prefix_items = self.list(v, &mut base, k, depth)?,
            "allOf" => o.all_of = self.list(v, &mut base, k, depth)?,
            "anyOf" => o.any_of = self.list(v, &mut base, k, depth)?,
            "oneOf" => o.one_of = self.list(v, &mut base, k, depth)?,
            "contains" => o.contains = Some(self.child(v, &mut base, &[k], depth)?),
            "not" => o.not = Some(self.child(v, &mut base, &[k], depth)?),
            "if" => o.if_ = Some(self.child(v, &mut base, &[k], depth)?),
            "then" => o.then_ = Some(self.child(v, &mut base, &[k], depth)?),
            "else" => o.else_ = Some(self.child(v, &mut base, &[k], depth)?),
            "additionalProperties" => o.additional_properties = Some(self.child(v, &mut base, &[k], depth)?),
            "propertyNames" => o.property_names = Some(self.child(v, &mut base, &[k], depth)?),
            "unevaluatedItems" => o.unevaluated_items = Some(self.child(v, &mut base, &[k], depth)?),
            "unevaluatedProperties" => o.unevaluated_properties = Some(self.child(v, &mut base, &[k], depth)?),
            "properties" => o.properties = self.map(v, &mut base, k, depth)?.into_iter().collect(),
            "dependentSchemas" => o.dependent_schemas = self.map(v, &mut base, k, depth)?,
            "patternProperties" => {
                let CValue::Obj(m) = v else { return Err(bad("patternProperties must be an object")) };
                for (re, s) in m {
                    let p = Pattern::compile(re).map_err(|e| bad(&e))?;
                    let n = self.child(s, &mut base, &[k, re.as_str()], depth)?;
                    o.pattern_properties.push((p, n));
                }
            }
            "x-quantity" => match v {
                CValue::Str(s) => o.quantity = Some(s.clone()),
                _ => return Err(bad("x-quantity must be a string")),
            },
            "x-ref" => {
                let t = match v {
                    CValue::Str(s) => EntityType::parse(s),
                    _ => None,
                };
                o.x_ref = Some(t.ok_or_else(|| bad("x-ref must name an entity type"))?);
            }
            "x-selector" => {
                let f = match v {
                    CValue::Str(s) => SelectorForm::parse(s),
                    _ => None,
                };
                o.selector = Some(match f.ok_or_else(|| bad("x-selector must name a selector form"))? {
                    SelectorForm::Single => Selector::Single,
                    SelectorForm::List => Selector::List,
                });
            }
            _ => return Err(bad("unknown keyword")),
        }
        Ok(())
    }
}

/// The schema positions applied to the same instance location as `o` (in-place applicators).
fn in_place(o: &Obj) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = o.reference.into_iter().collect();
    v.extend(o.all_of.iter().chain(&o.any_of).chain(&o.one_of).copied());
    v.extend([o.not, o.if_, o.then_, o.else_].into_iter().flatten());
    v.extend(o.dependent_schemas.iter().map(|(_, n)| *n));
    v
}

fn has_numeric(o: &Obj) -> bool {
    o.minimum.is_some()
        || o.maximum.is_some()
        || o.exclusive_minimum.is_some()
        || o.exclusive_maximum.is_some()
        || o.multiple_of.is_some()
}

/// Whether a schema object's own `type` excludes `object`, so a literal (an object) can never
/// pass it.
fn excludes_object(o: &Obj) -> bool {
    o.types.as_ref().is_some_and(|ts| !ts.contains(&JType::Object))
}

/// The `x-quantity` rule (maintainer decisions in the reviews of #341). A literal is
/// normalised for the keywords of the schema object holding `x-quantity` only, so:
///
/// 1. a numeric keyword (`minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`,
///    `multipleOf`) is accepted only in a schema object that has its own `x-quantity` or its
///    own `type` excluding `object`. Elsewhere (in an applicator, a child schema, a `$defs`
///    entry, anywhere) it could meet an authored literal it would silently ignore;
/// 2. `x-quantity` reachable in place (`$ref`, `allOf`, `anyOf`, `oneOf`, `not`, `if`, `then`,
///    `else`, `dependentSchemas`) from a `contains` or `not` subschema is refused: those never
///    keep what their subschema normalised.
fn quantity_rule(nodes: &[Node], positions: &BTreeMap<Vec<String>, NodeId>) -> Result<(), CompileError> {
    let path_of = |id: NodeId| positions.iter().find(|(_, n)| **n == id).map(|(p, _)| p.clone()).unwrap_or_default();
    let obj = |id: NodeId| match nodes.get(id) {
        Some(Node::Obj(o)) => Some(o.as_ref()),
        _ => None,
    };
    for id in 0..nodes.len() {
        let Some(o) = obj(id) else { continue };
        if has_numeric(o) && o.quantity.is_none() && !excludes_object(o) {
            return Err(Builder::err(
                &path_of(id),
                "a numeric keyword needs x-quantity or a type excluding object in the same schema object",
            ));
        }
    }
    for id in 0..nodes.len() {
        let Some(o) = obj(id) else { continue };
        for start in [o.contains, o.not].into_iter().flatten() {
            let mut seen = BTreeSet::new();
            let mut work = vec![start];
            while let Some(n) = work.pop() {
                if !seen.insert(n) {
                    continue;
                }
                if let Some(x) = obj(n) {
                    if x.quantity.is_some() {
                        return Err(Builder::err(
                            &path_of(n),
                            "x-quantity under contains or not: its literals would never be normalised",
                        ));
                    }
                    work.extend(in_place(x));
                }
            }
        }
    }
    Ok(())
}

impl Compiled {
    /// Compiles a schema (see the module docs for what is refused).
    pub fn compile(schema: &CValue) -> Result<Compiled, CompileError> {
        let mut b = Builder { nodes: Vec::new(), positions: BTreeMap::new(), refs: Vec::new() };
        let root = b.build(schema, &mut Vec::new(), 1)?;
        for (id, target, at) in std::mem::take(&mut b.refs) {
            let Some(t) = b.positions.get(&target).copied() else {
                return Err(Builder::err(&at, "$ref does not land on a schema position"));
            };
            if let Some(Node::Obj(o)) = b.nodes.get_mut(id) {
                o.reference = Some(t);
            }
        }
        quantity_rule(&b.nodes, &b.positions)?;
        Ok(Compiled { nodes: b.nodes, root })
    }

    /// Validates `instance`. On success, returns the normalised literals in document order.
    pub fn evaluate(&self, instance: &CValue, hooks: &dyn Hooks) -> Result<Vec<(JsonPointer, Normalised)>, Fail> {
        let mut e = Eval { s: self, hooks, steps: 0, depth: 0, regex_formats: 0 };
        let out = match e.node(self.root, instance, &JsonPointer::root(), "false") {
            Err(Abort(f)) => return Err(*f),
            Ok(Err(f)) => return Err(f),
            Ok(Ok(out)) => out,
        };
        let mut lits = out.lits;
        lits.sort_by(|a, b| a.0.doc_cmp(&b.0));
        let mut kept: Vec<(JsonPointer, Normalised)> = Vec::new();
        for (p, n) in lits {
            match kept.last() {
                Some((q, m)) if *q == p => {
                    if m.si_bits() != n.si_bits() {
                        return Err(Fail::schema(&p, "x-quantity", "one literal normalises to two different values"));
                    }
                }
                _ => kept.push((p, n)),
            }
        }
        Ok(kept)
    }
}

// ------------------------------------------------------------------ evaluation

/// Annotations of one passing schema about its instance, and the literals normalised below.
#[derive(Debug, Default)]
struct Out {
    props: BTreeSet<String>,
    items: BTreeSet<usize>,
    all_items: bool,
    lits: Vec<(JsonPointer, Normalised)>,
}

impl Out {
    fn merge(&mut self, o: Out) {
        self.props.extend(o.props);
        self.items.extend(o.items);
        self.all_items |= o.all_items;
        self.lits.extend(o.lits);
    }
}

struct Eval<'a> {
    s: &'a Compiled,
    hooks: &'a dyn Hooks,
    steps: u64,
    depth: usize,
    regex_formats: usize,
}

fn limit(ptr: &JsonPointer, message: &str) -> Abort {
    Abort(Box::new(Fail::new(ptr, "input_limit", ValidationReason::InputLimit, message)))
}

/// A boolean schema position (or a dangling one, which cannot happen after compilation).
#[inline(never)]
fn leaf(node: Option<&Node>, ptr: &JsonPointer, via: &'static str) -> Step {
    match node {
        Some(Node::Bool(true)) => Ok(Ok(Out::default())),
        Some(Node::Bool(false)) => Ok(Err(Fail::schema(ptr, via, "no value is allowed here"))),
        _ => Err(Abort(Box::new(Fail::schema(ptr, "schema", "dangling schema position")))),
    }
}

/// Member names of an object in document order.
fn ordered(m: &BTreeMap<String, CValue>) -> Vec<(&String, &CValue)> {
    let mut v: Vec<(&String, &CValue)> = m.iter().collect();
    v.sort_by(|a, b| cmp_utf16(a.0, b.0));
    v
}

impl Eval<'_> {
    fn charge(&mut self, ptr: &JsonPointer, steps: u64) -> Result<(), Abort> {
        self.steps = self.steps.saturating_add(steps);
        if self.steps > MAX_EVAL_STEPS {
            return Err(limit(ptr, "validation exceeds its step budget"));
        }
        Ok(())
    }

    fn charge_bytes(&mut self, ptr: &JsonPointer, len: usize) -> Result<(), Abort> {
        self.charge(ptr, (len / BYTES_PER_STEP) as u64)
    }

    /// Matches a pattern, charging one step per KiB of `text` (at least one) times the
    /// pattern's program size in KiB: the PikeVM's work is O(input × program).
    fn matches(&mut self, p: &Pattern, text: &str, ptr: &JsonPointer) -> Result<bool, Abort> {
        let kib = (text.len() / BYTES_PER_STEP + 1) as u64;
        self.charge(ptr, kib.saturating_mul(p.program_kib()))?;
        Ok(p.is_match(text))
    }

    /// Charges a format check: one step per KiB of the string, and for `format: "regex"`,
    /// which compiles the string, [`REGEX_FORMAT_STEPS`] plus one step per byte, at most
    /// [`MAX_REGEX_FORMAT_CHECKS`] times per document.
    fn charge_format(&mut self, f: Format, s: &str, ptr: &JsonPointer) -> Result<(), Abort> {
        if f != Format::Regex {
            return self.charge_bytes(ptr, s.len());
        }
        self.regex_formats += 1;
        if self.regex_formats > MAX_REGEX_FORMAT_CHECKS {
            return Err(limit(ptr, "too many format: regex checks in one document"));
        }
        self.charge(ptr, REGEX_FORMAT_STEPS.saturating_add(s.len() as u64))
    }

    fn canon(&mut self, v: &CValue, ptr: &JsonPointer) -> Result<Vec<u8>, Abort> {
        let bytes = to_canonical_bytes(v)
            .map_err(|_| Abort(Box::new(Fail::schema(ptr, "json", "value has no canonical form"))))?;
        self.charge_bytes(ptr, bytes.len())?;
        Ok(bytes)
    }

    /// Applies schema `id` to `inst`; a `false` schema fails under `via`. Kept small: it is on
    /// the recursion path, so everything but the dispatch lives in other functions.
    fn node(&mut self, id: NodeId, inst: &CValue, ptr: &JsonPointer, via: &'static str) -> Step {
        self.enter(ptr)?;
        let r = match self.s.nodes.get(id) {
            Some(Node::Obj(o)) => self.object(o, inst, ptr),
            other => leaf(other, ptr, via),
        };
        self.depth -= 1;
        r
    }

    #[inline(never)]
    fn enter(&mut self, ptr: &JsonPointer) -> Result<(), Abort> {
        self.charge(ptr, 1)?;
        if self.depth >= MAX_EVAL_DEPTH {
            return Err(limit(ptr, "schema applications nest too deeply"));
        }
        self.depth += 1;
        Ok(())
    }

    fn object(&mut self, o: &Obj, inst0: &CValue, ptr: &JsonPointer) -> Step {
        let mut out = Out::default();
        let mut best: Option<Fail> = None;
        let lit = match (&o.quantity, inst0) {
            (Some(kind), CValue::Obj(_)) => Some(self.literal(kind, inst0, ptr)?),
            _ => None,
        };
        let si;
        let inst = match lit {
            Some(Ok(n)) => {
                si = CValue::Num(n.si());
                out.lits.push((ptr.clone(), n));
                &si
            }
            Some(Err(f)) => return Ok(Err(f)),
            None => inst0,
        };
        self.assertions(o, inst, ptr, &mut best)?;
        self.applicators(o, inst, ptr, &mut out, &mut best)?;
        Ok(match best {
            Some(f) => Err(f),
            None => Ok(out),
        })
    }

    /// Normalises the authored literal at `ptr` (`x-quantity` on an object instance).
    #[inline(never)]
    fn literal(&mut self, kind: &str, v: &CValue, ptr: &JsonPointer) -> Result<Result<Normalised, Fail>, Abort> {
        match self.hooks.literal(kind, v) {
            Ok(n) => Ok(Ok(n)),
            Err(BusError::Validation(reason, detail)) => {
                Ok(Err(Fail::new(ptr, "x-quantity", reason, detail.as_untrusted_str())))
            }
            Err(other) => {
                let mut f = Fail::schema(ptr, "x-quantity", "literal normalisation failed");
                f.bus = Some(other);
                Err(Abort(Box::new(f)))
            }
        }
    }

    #[inline(never)]
    fn assertions(&mut self, o: &Obj, inst: &CValue, ptr: &JsonPointer, best: &mut Option<Fail>) -> Result<(), Abort> {
        if let Some(ts) = &o.types {
            if !ts.iter().any(|t| t.matches(inst)) {
                note(best, Fail::schema(ptr, "type", "value has the wrong JSON type"));
            }
        }
        if o.konst.is_some() || o.enumeration.is_some() {
            let c = self.canon(inst, ptr)?;
            if o.konst.as_ref().is_some_and(|k| *k != c) {
                note(best, Fail::schema(ptr, "const", "value differs from the constant"));
            }
            if o.enumeration.as_ref().is_some_and(|e| !e.contains(&c)) {
                note(best, Fail::schema(ptr, "enum", "value is not one of the allowed values"));
            }
        }
        if let CValue::Num(x) = inst {
            let x = *x;
            if let Some(m) = o.multiple_of {
                if !Dec::of(x).is_some_and(|d| d.is_multiple_of(m)) {
                    note(best, Fail::schema(ptr, "multipleOf", "value is not a multiple"));
                }
            }
            let checks: [Bound; 4] = [
                (o.maximum, "maximum", |x, l| x <= l),
                (o.exclusive_maximum, "exclusiveMaximum", |x, l| x < l),
                (o.minimum, "minimum", |x, l| x >= l),
                (o.exclusive_minimum, "exclusiveMinimum", |x, l| x > l),
            ];
            for (bound, kw, ok) in checks {
                if bound.is_some_and(|l| !ok(x, l)) {
                    note(best, Fail::schema(ptr, kw, "value is out of range"));
                }
            }
        }
        if let CValue::Str(s) = inst {
            if o.max_length.is_some() || o.min_length.is_some() {
                self.charge_bytes(ptr, s.len())?;
                let n = s.chars().count() as u64;
                if o.max_length.is_some_and(|m| n > m) {
                    note(best, Fail::schema(ptr, "maxLength", "string is too long"));
                }
                if o.min_length.is_some_and(|m| n < m) {
                    note(best, Fail::schema(ptr, "minLength", "string is too short"));
                }
            }
            if let Some(p) = &o.pattern {
                if !self.matches(p, s, ptr)? {
                    note(best, Fail::schema(ptr, "pattern", "string does not match the pattern"));
                }
            }
            if let Some(f) = o.format {
                self.charge_format(f, s, ptr)?;
                if !f.check(s) {
                    note(best, Fail::schema(ptr, "format", "string does not match the format"));
                }
            }
        }
        if o.x_ref.is_some() || o.selector.is_some() {
            if let Some(f) = self.references(o, inst, ptr)? {
                note(best, f);
            }
        }
        if let CValue::Arr(a) = inst {
            let n = a.len() as u64;
            if o.max_items.is_some_and(|m| n > m) {
                note(best, Fail::schema(ptr, "maxItems", "array has too many items"));
            }
            if o.min_items.is_some_and(|m| n < m) {
                note(best, Fail::schema(ptr, "minItems", "array has too few items"));
            }
            if o.unique_items {
                // One step per element, on top of the bytes canonicalised (re-review of #341).
                self.charge(ptr, a.len() as u64)?;
                let mut seen = Vec::with_capacity(a.len());
                for (i, item) in a.iter().enumerate() {
                    seen.push(self.canon(item, &ptr.index(i))?);
                }
                seen.sort();
                if seen.windows(2).any(|w| w.first() == w.get(1)) {
                    note(best, Fail::schema(ptr, "uniqueItems", "array items are not unique"));
                }
            }
        }
        if let CValue::Obj(m) = inst {
            let n = m.len() as u64;
            if o.max_properties.is_some_and(|x| n > x) {
                note(best, Fail::schema(ptr, "maxProperties", "object has too many members"));
            }
            if o.min_properties.is_some_and(|x| n < x) {
                note(best, Fail::schema(ptr, "minProperties", "object has too few members"));
            }
            if o.required.iter().any(|r| !m.contains_key(r)) {
                note(best, Fail::schema(ptr, "required", "a required member is missing"));
            }
            let dep_missing = o
                .dependent_required
                .iter()
                .any(|(k, reqs)| m.contains_key(k) && reqs.iter().any(|r| !m.contains_key(r)));
            if dep_missing {
                note(best, Fail::schema(ptr, "dependentRequired", "a dependent required member is missing"));
            }
        }
        Ok(())
    }

    /// `x-ref` and `x-selector`: the first bad reference in document order, if any.
    fn references(&mut self, o: &Obj, inst: &CValue, ptr: &JsonPointer) -> Result<Option<Fail>, Abort> {
        let kw: &'static str = if o.x_ref.is_some() { "x-ref" } else { "x-selector" };
        let want = o.x_ref.map(EntityType::as_str);
        let check = |e: &mut Self, id: &str, at: &JsonPointer| -> Result<Option<Fail>, Abort> {
            e.charge(at, 1)?;
            let bad = |m: &str| Some(Fail::new(at, kw, ValidationReason::BadReference, m));
            Ok(match e.hooks.entity(id) {
                RefLookup::BadId => bad("value is not a valid entity ID"),
                RefLookup::Missing => bad("no entity with this ID exists"),
                RefLookup::Kind(k) => match want {
                    Some(t) if !(k == t || k.strip_prefix(t).is_some_and(|r| r.starts_with('.'))) => {
                        bad("the entity is of another kind")
                    }
                    _ => None,
                },
            })
        };
        match (o.selector.unwrap_or(Selector::Single), inst) {
            (Selector::Single, CValue::Str(id)) => check(self, id, ptr),
            (Selector::Single, _) => Ok(Some(Fail::schema(ptr, kw, "an entity reference is a string"))),
            (Selector::List, CValue::Arr(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let at = ptr.index(i);
                    let f = match item {
                        CValue::Str(id) => check(self, id, &at)?,
                        _ => Some(Fail::schema(&at, kw, "an entity reference is a string")),
                    };
                    if f.is_some() {
                        return Ok(f);
                    }
                }
                Ok(None)
            }
            (Selector::List, _) => Ok(Some(Fail::schema(ptr, "x-selector", "a list selector is an array of IDs"))),
        }
    }

    /// Applies `id` to the same instance; a pass merges its annotations, a failure is noted.
    #[inline(never)]
    fn same(
        &mut self,
        id: NodeId,
        inst: &CValue,
        ptr: &JsonPointer,
        via: &'static str,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<bool, Abort> {
        Ok(match self.node(id, inst, ptr, via)? {
            Ok(o) => {
                out.merge(o);
                true
            }
            Err(f) => {
                note(best, f);
                false
            }
        })
    }

    /// Applies `id` to a child value; a pass keeps only its literals.
    #[inline(never)]
    fn child(
        &mut self,
        id: NodeId,
        inst: &CValue,
        ptr: &JsonPointer,
        via: &'static str,
        out: &mut Out,
    ) -> Result<Option<Fail>, Abort> {
        Ok(match self.node(id, inst, ptr, via)? {
            Ok(o) => {
                out.lits.extend(o.lits);
                None
            }
            Err(f) => Some(f),
        })
    }

    // The applicators are on the recursion path, so each keyword gets its own small frame
    // (`inline(never)`): one large function would hold every keyword's temporaries in each
    // nested application and multiply the stack a deep schema needs.
    #[inline(never)]
    fn applicators(
        &mut self,
        o: &Obj,
        inst: &CValue,
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        if let (CValue::Arr(a), Some(c)) = (inst, o.contains) {
            self.contains(o, c, a, ptr, out, best)?;
        }
        if let Some(r) = o.reference {
            self.same(r, inst, ptr, "$ref", out, best)?;
        }
        for id in &o.all_of {
            self.same(*id, inst, ptr, "allOf", out, best)?;
        }
        if !o.any_of.is_empty() {
            self.any_of(&o.any_of, inst, ptr, out, best)?;
        }
        if !o.one_of.is_empty() {
            self.one_of(&o.one_of, inst, ptr, out, best)?;
        }
        if let Some(n) = o.not {
            self.not(n, inst, ptr, best)?;
        }
        if let Some(i) = o.if_ {
            self.conditional(o, i, inst, ptr, out, best)?;
        }
        if let CValue::Obj(m) = inst {
            for (name, id) in &o.dependent_schemas {
                if m.contains_key(name) {
                    self.same(*id, inst, ptr, "dependentSchemas", out, best)?;
                }
            }
        }
        match inst {
            CValue::Arr(a) => self.array_children(o, a, ptr, out, best),
            CValue::Obj(m) => self.object_children(o, m, ptr, out, best),
            _ => Ok(()),
        }
    }

    #[inline(never)]
    fn contains(
        &mut self,
        o: &Obj,
        c: NodeId,
        a: &[CValue],
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        let mut matched = 0u64;
        for (i, item) in a.iter().enumerate() {
            self.charge(ptr, 1)?;
            if self.node(c, item, &ptr.index(i), "contains")?.is_ok() {
                matched += 1;
                out.items.insert(i);
            }
        }
        if matched < o.min_contains.unwrap_or(1) {
            let kw = if o.min_contains.is_some() { "minContains" } else { "contains" };
            note(best, Fail::schema(ptr, kw, "too few items match contains"));
        }
        if o.max_contains.is_some_and(|m| matched > m) {
            note(best, Fail::schema(ptr, "maxContains", "too many items match contains"));
        }
        Ok(())
    }

    #[inline(never)]
    fn any_of(
        &mut self,
        ids: &[NodeId],
        inst: &CValue,
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        let mut any = false;
        for id in ids {
            if let Ok(x) = self.node(*id, inst, ptr, "anyOf")? {
                out.merge(x);
                any = true;
            }
        }
        if !any {
            note(best, Fail::schema(ptr, "anyOf", "value matches none of the alternatives"));
        }
        Ok(())
    }

    #[inline(never)]
    fn one_of(
        &mut self,
        ids: &[NodeId],
        inst: &CValue,
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        let mut passing = Vec::new();
        for id in ids {
            if let Ok(x) = self.node(*id, inst, ptr, "oneOf")? {
                passing.push(x);
            }
        }
        match (passing.pop(), passing.is_empty()) {
            (Some(x), true) => out.merge(x),
            _ => note(best, Fail::schema(ptr, "oneOf", "value does not match exactly one alternative")),
        }
        Ok(())
    }

    #[inline(never)]
    fn not(&mut self, n: NodeId, inst: &CValue, ptr: &JsonPointer, best: &mut Option<Fail>) -> Result<(), Abort> {
        if self.node(n, inst, ptr, "not")?.is_ok() {
            note(best, Fail::schema(ptr, "not", "value matches a forbidden schema"));
        }
        Ok(())
    }

    #[inline(never)]
    fn conditional(
        &mut self,
        o: &Obj,
        i: NodeId,
        inst: &CValue,
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        let next = match self.node(i, inst, ptr, "if")? {
            Ok(x) => {
                out.merge(x);
                o.then_.map(|t| (t, "then"))
            }
            Err(_) => o.else_.map(|e| (e, "else")),
        };
        if let Some((id, via)) = next {
            self.same(id, inst, ptr, via, out, best)?;
        }
        Ok(())
    }

    #[inline(never)]
    fn array_children(
        &mut self,
        o: &Obj,
        a: &[CValue],
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        for (i, (id, item)) in o.prefix_items.iter().zip(a.iter()).enumerate() {
            self.charge(ptr, 1)?;
            if let Some(f) = self.child(*id, item, &ptr.index(i), "prefixItems", out)? {
                note(best, f);
                break;
            }
            out.items.insert(i);
        }
        if let Some(id) = o.items {
            for (i, item) in a.iter().enumerate().skip(o.prefix_items.len()) {
                self.charge(ptr, 1)?;
                if let Some(f) = self.child(id, item, &ptr.index(i), "items", out)? {
                    note(best, f);
                    break;
                }
            }
            out.all_items = true;
        }
        if let (Some(id), None) = (o.unevaluated_items, best.as_ref()) {
            for (i, item) in a.iter().enumerate() {
                self.charge(ptr, 1)?;
                if out.all_items || out.items.contains(&i) {
                    continue;
                }
                if let Some(f) = self.child(id, item, &ptr.index(i), "unevaluatedItems", out)? {
                    note(best, f);
                    break;
                }
            }
            out.all_items = true;
        }
        Ok(())
    }

    #[inline(never)]
    fn object_children(
        &mut self,
        o: &Obj,
        m: &BTreeMap<String, CValue>,
        ptr: &JsonPointer,
        out: &mut Out,
        best: &mut Option<Fail>,
    ) -> Result<(), Abort> {
        let any_member_keyword = o.property_names.is_some()
            || !o.properties.is_empty()
            || !o.pattern_properties.is_empty()
            || o.additional_properties.is_some()
            || o.unevaluated_properties.is_some();
        if !any_member_keyword {
            return Ok(());
        }
        // Ordering the members costs one step per member; every keyword below then pays one
        // step per member it looks at, so a wide object cannot be walked for free.
        self.charge(ptr, m.len() as u64)?;
        let members = ordered(m);
        if let Some(id) = o.property_names {
            for (name, _) in &members {
                self.charge(ptr, 1)?;
                let at = ptr.key(name);
                if let Err(f) = self.node(id, &CValue::Str((*name).clone()), &at, "propertyNames")? {
                    note(best, Fail::schema(&at, "propertyNames", &f.message));
                    break;
                }
            }
        }
        if !o.properties.is_empty() {
            for (name, v) in &members {
                self.charge(ptr, 1)?;
                if let Some(id) = o.properties.get(*name) {
                    if let Some(f) = self.child(*id, v, &ptr.key(name), "properties", out)? {
                        note(best, f);
                        break;
                    }
                    out.props.insert((*name).clone());
                }
            }
        }
        if !o.pattern_properties.is_empty() {
            for (name, v) in &members {
                self.charge(ptr, 1)?;
                let mut first: Option<Fail> = None;
                for (pat, id) in &o.pattern_properties {
                    if self.matches(pat, name, ptr)? {
                        match self.child(*id, v, &ptr.key(name), "patternProperties", out)? {
                            Some(f) => note(&mut first, f),
                            None => {
                                out.props.insert((*name).clone());
                            }
                        }
                    }
                }
                if let Some(f) = first {
                    note(best, f);
                    break;
                }
            }
        }
        if let Some(id) = o.additional_properties {
            for (name, v) in &members {
                self.charge(ptr, 1)?;
                let mut patterned = false;
                for (p, _) in &o.pattern_properties {
                    if self.matches(p, name, ptr)? {
                        patterned = true;
                        break;
                    }
                }
                if o.properties.contains_key(*name) || patterned {
                    continue;
                }
                if let Some(f) = self.child(id, v, &ptr.key(name), "additionalProperties", out)? {
                    note(best, f);
                    break;
                }
                out.props.insert((*name).clone());
            }
        }
        if let (Some(id), None) = (o.unevaluated_properties, best.as_ref()) {
            for (name, v) in &members {
                self.charge(ptr, 1)?;
                if out.props.contains(*name) {
                    continue;
                }
                if let Some(f) = self.child(id, v, &ptr.key(name), "unevaluatedProperties", out)? {
                    note(best, f);
                    break;
                }
                out.props.insert((*name).clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn decimal_multiples() {
        let d = |x: f64| Dec::of(x).unwrap();
        assert!(d(0.3).is_multiple_of(d(0.1)));
        assert!(d(10.0).is_multiple_of(d(2.5)));
        assert!(!d(10.1).is_multiple_of(d(0.2)));
        assert!(!d(1e300).is_multiple_of(d(3.0)) && d(1e300).is_multiple_of(d(1e-5)));
        assert!(d(0.0).is_multiple_of(d(7.0)));
        assert!(!d(0.001).is_multiple_of(d(0.01)));
        assert!(d(-4.5).is_multiple_of(d(1.5)));
        assert!(d(1e20).is_multiple_of(d(1.0)) && d(1e20).is_multiple_of(d(4.0)));
        assert_eq!(d(1e20), Dec { coeff: 1, exp: 20 });
        assert_eq!(d(0.000_25), Dec { coeff: 25, exp: -5 });
    }

    #[test]
    fn rank_is_total_and_unique() {
        for (i, k) in RANK.iter().enumerate() {
            assert_eq!(rank(k), i, "{k}");
        }
    }
}
