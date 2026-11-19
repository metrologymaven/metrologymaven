//! Value fields hold bindings, never values.
//!
//! The document model never formats, computes or trusts a value or its labels. A block kind
//! declares where its values sit ([`super::BlockKind::value_fields`], RFC 6901 pointers),
//! and each declared field must hold a `value_binding@1`: a reference to a stored record
//! and its stored uncertainty statement, by ID and digest. `RPT.MODEL.BIND` resolves the
//! binding against the stored record and statement (refusing a digest mismatch), and
//! `RPT.MODEL.FMT` alone turns it into value text with its own per-value labels and side.
//!
//! `value_binding@1` (exactly these keys):
//!
//! ```text
//! { "schema": "value_binding@1",
//!   "record": <canonical lowercase UUID, not nil or max>,
//!   "record_digest": "sha256:<64 hex>",
//!   "field": <RFC 6901 pointer into the record, "" or "/...", at most 256 bytes>,
//!   "statement_digest": "sha256:<64 hex>" }
//! ```
//!
//! Anything else in a declared value field is refused:
//!
//! - a number, string (number-like or not), boolean, null or array, an object without a
//!   `schema` key, or one whose `schema` is not `value_binding@1` (a hand-typed
//!   `value_text@1` included) is a literal: `report.value_literal_refused`;
//! - an object tagged `value_binding@1` that breaks the shape is
//!   `report.value_binding_invalid`, with the reason.
//!
//! Undeclared places are checked too: the whole payload is walked, and an object tagged
//! `value_text@1` anywhere is `report.value_literal_refused` (`value_text`), while an
//! object tagged `value_binding@1` at a pointer that is not a declared value field is
//! `report.value_binding_invalid` (`undeclared_binding`).

use super::error::ReportError;
use super::kind::{BlockKind, PayloadError};
use super::model::{parse_canonical_uuid, CanonicalJson};
use super::text::must_escape;
use fid_canon::CValue;
use fid_project::Uuid;

/// Schema tag of a value binding.
pub const VALUE_BINDING_SCHEMA: &str = "value_binding@1";
/// Schema tag of FMT value text, which a stored payload may never hold anywhere.
pub const VALUE_TEXT_SCHEMA: &str = "value_text@1";
/// Most value fields one block may declare.
pub const MAX_VALUE_FIELDS: usize = 4096;
/// Longest pointer (a declared value field, or a binding's `field`), in bytes.
pub const MAX_POINTER_BYTES: usize = 256;

/// Why a declared value field was refused. The field of the error is set by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingFault {
    /// Not a binding at all (`report.value_literal_refused`).
    Literal,
    /// Tagged as a binding but malformed (`report.value_binding_invalid`).
    Invalid(&'static str),
}

/// Whether `s` is a well-formed RFC 6901 pointer within the bounds: `""` or starting
/// with `/`, every `~` followed by `0` or `1`, at most [`MAX_POINTER_BYTES`] bytes, and no
/// character [`must_escape`] flags.
pub fn is_pointer(s: &str) -> bool {
    if s.len() > MAX_POINTER_BYTES || s.chars().any(must_escape) || !(s.is_empty() || s.starts_with('/')) {
        return false;
    }
    let b = s.as_bytes();
    b.iter().enumerate().all(|(i, c)| *c != b'~' || matches!(b.get(i + 1), Some(b'0' | b'1')))
}

fn digest_ok(v: Option<&CValue>) -> bool {
    matches!(v, Some(CValue::Str(s)) if fid_types::Digest::is_valid(s))
}

/// Checks one value field's content.
pub fn check_value_binding(v: &CValue) -> Result<(), BindingFault> {
    let CValue::Obj(o) = v else { return Err(BindingFault::Literal) };
    match o.get("schema") {
        Some(CValue::Str(s)) if s == VALUE_BINDING_SCHEMA => {}
        _ => return Err(BindingFault::Literal),
    }
    const KEYS: [&str; 5] = ["field", "record", "record_digest", "schema", "statement_digest"];
    if o.len() != KEYS.len() || KEYS.iter().any(|k| !o.contains_key(*k)) {
        return Err(BindingFault::Invalid("binding_keys"));
    }
    let record = match o.get("record") {
        Some(CValue::Str(s)) => parse_canonical_uuid(s),
        _ => None,
    };
    if !record.is_some_and(|r| r != Uuid::NIL && r != Uuid::MAX) {
        return Err(BindingFault::Invalid("record"));
    }
    if !digest_ok(o.get("record_digest")) {
        return Err(BindingFault::Invalid("record_digest"));
    }
    if !digest_ok(o.get("statement_digest")) {
        return Err(BindingFault::Invalid("statement_digest"));
    }
    match o.get("field") {
        Some(CValue::Str(f)) if is_pointer(f) => Ok(()),
        _ => Err(BindingFault::Invalid("field")),
    }
}

/// RFC 6901 JSON pointer lookup (`""` is the whole value; `~0` is `~`, `~1` is `/`).
pub fn resolve_pointer<'a>(root: &'a CValue, pointer: &str) -> Option<&'a CValue> {
    if !is_pointer(pointer) {
        return None;
    }
    if pointer.is_empty() {
        return Some(root);
    }
    let rest = pointer.strip_prefix('/')?;
    let mut at = root;
    for raw in rest.split('/') {
        let token = raw.replace("~1", "/").replace("~0", "~");
        at = match at {
            CValue::Obj(o) => o.get(&token)?,
            CValue::Arr(a) => {
                let canonical = token == "0"
                    || (!token.is_empty() && !token.starts_with('0') && token.bytes().all(|c| c.is_ascii_digit()));
                if !canonical {
                    return None;
                }
                a.get(token.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(at)
}

/// The value rule for one block: every declared value field exists and holds a
/// `value_binding@1`, no `value_text@1` appears anywhere, and no binding sits outside a
/// declared value field.
pub(crate) fn check_block_values(kind: &dyn BlockKind, payload: &CanonicalJson) -> Result<(), ReportError> {
    let fields = kind.value_fields(payload).map_err(ReportError::InvalidBlockPayload)?;
    if fields.len() > MAX_VALUE_FIELDS {
        return Err(ReportError::InvalidBlockPayload(PayloadError::new("", "too_many_value_fields")));
    }
    let mut declared: Vec<&CValue> = Vec::with_capacity(fields.len());
    for f in &fields {
        if !is_pointer(f) {
            return Err(ReportError::InvalidBlockPayload(PayloadError::new(f, "bad_value_field_pointer")));
        }
        let v = resolve_pointer(payload.value(), f)
            .ok_or_else(|| ReportError::ValueBindingInvalid(PayloadError::new(f, "missing")))?;
        check_value_binding(v).map_err(|e| match e {
            BindingFault::Literal => ReportError::ValueLiteralRefused(PayloadError::new(f, "literal")),
            BindingFault::Invalid(r) => ReportError::ValueBindingInvalid(PayloadError::new(f, r)),
        })?;
        declared.push(v);
    }
    let mut walk = Walk::default();
    match walk.run(payload.value(), &declared) {
        Ok(()) => Ok(()),
        Err(Offence::ValueText(i)) => {
            Err(ReportError::ValueLiteralRefused(PayloadError::new(&walk.pointer(i), "value_text")))
        }
        Err(Offence::UndeclaredBinding(i)) => {
            Err(ReportError::ValueBindingInvalid(PayloadError::new(&walk.pointer(i), "undeclared_binding")))
        }
        Err(Offence::TooManyNodes) => Err(ReportError::InvalidBlockPayload(PayloadError::new("", "too_many_nodes"))),
    }
}

/// Most nodes the undeclared-place walk visits. A canonical payload of at most
/// [`super::MAX_PAYLOAD_BYTES`] bytes has fewer nodes than bytes, so this never refuses a
/// payload within bounds; it only makes the walk fail closed if that ever changed.
pub const MAX_WALK_NODES: usize = super::MAX_PAYLOAD_BYTES;

/// How a node was reached from its parent.
#[derive(Debug, Clone, Copy)]
enum Step<'a> {
    Root,
    Key(&'a str),
    Index(usize),
}

/// One visited node: the node, its parent's index in the trail and the step from it.
/// Borrowed only: no per-node string is built (review of #232: a 62 KB payload of one
/// 30 000-byte key over 16 000 array items cost about 480 MB with per-node pointers).
#[derive(Debug, Clone, Copy)]
struct Entry<'a> {
    node: &'a CValue,
    parent: usize,
    step: Step<'a>,
}

enum Offence {
    ValueText(usize),
    UndeclaredBinding(usize),
    TooManyNodes,
}

/// The undeclared-place walk. Memory is the trail (one [`Entry`] per visited node, at
/// most [`MAX_WALK_NODES`]) and a stack of trail indices; pointer text is built only for
/// the one node reported, and cut at [`super::kind::MAX_FIELD_BYTES`].
#[derive(Default)]
struct Walk<'a> {
    trail: Vec<Entry<'a>>,
    stack: Vec<usize>,
}

impl<'a> Walk<'a> {
    /// Visits objects in key order and arrays in index order (depth first), so the first
    /// offence reported is deterministic. An object whose `schema` is `value_text@1` is an
    /// offence anywhere; one whose `schema` is `value_binding@1` is an offence unless it is
    /// (by address) one of the `declared` nodes.
    fn run(&mut self, root: &'a CValue, declared: &[&CValue]) -> Result<(), Offence> {
        self.push(Entry { node: root, parent: usize::MAX, step: Step::Root })?;
        while let Some(i) = self.stack.pop() {
            let Some(e) = self.trail.get(i).copied() else { return Ok(()) };
            match e.node {
                CValue::Obj(o) => {
                    if let Some(CValue::Str(schema)) = o.get("schema") {
                        if schema == VALUE_TEXT_SCHEMA {
                            return Err(Offence::ValueText(i));
                        }
                        if schema == VALUE_BINDING_SCHEMA && !declared.iter().any(|d| std::ptr::eq(*d, e.node)) {
                            return Err(Offence::UndeclaredBinding(i));
                        }
                    }
                    for (k, c) in o.iter().rev() {
                        self.push(Entry { node: c, parent: i, step: Step::Key(k) })?;
                    }
                }
                CValue::Arr(a) => {
                    for (j, c) in a.iter().enumerate().rev() {
                        self.push(Entry { node: c, parent: i, step: Step::Index(j) })?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn push(&mut self, e: Entry<'a>) -> Result<(), Offence> {
        if self.trail.len() >= MAX_WALK_NODES {
            return Err(Offence::TooManyNodes);
        }
        self.stack.push(self.trail.len());
        self.trail.push(e);
        Ok(())
    }

    /// The RFC 6901 pointer of trail entry `i`, cut once it exceeds
    /// [`super::kind::MAX_FIELD_BYTES`] (the error field holds no more).
    fn pointer(&self, i: usize) -> String {
        let mut steps = Vec::new();
        let mut at = i;
        while let Some(e) = self.trail.get(at) {
            steps.push(e.step);
            at = e.parent;
        }
        let mut out = String::new();
        for step in steps.iter().rev() {
            if out.len() > super::kind::MAX_FIELD_BYTES {
                break;
            }
            match step {
                Step::Root => {}
                Step::Key(k) => {
                    out.push('/');
                    for c in k.chars() {
                        match c {
                            '~' => out.push_str("~0"),
                            '/' => out.push_str("~1"),
                            c => out.push(c),
                        }
                        if out.len() > super::kind::MAX_FIELD_BYTES {
                            break;
                        }
                    }
                }
                Step::Index(j) => {
                    out.push('/');
                    out.push_str(&j.to_string());
                }
            }
        }
        out
    }
}

/// Peak memory of the undeclared-place walk over `payload`, in bytes: trail and stack
/// capacity. For tests and audits: it is bounded by a constant times the number of
/// nodes, never by key lengths times nodes.
pub fn walk_peak_bytes(payload: &CanonicalJson) -> usize {
    let mut walk = Walk::default();
    let _ = walk.run(payload.value(), &[]);
    walk.trail.capacity() * std::mem::size_of::<Entry<'_>>() + walk.stack.capacity() * std::mem::size_of::<usize>()
}
