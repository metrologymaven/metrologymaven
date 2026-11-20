//! The stored binding of a block and the value types resolution hands out.
//!
//! A bound block's payload holds, under the reserved key [`BINDING_KEY`], a `binding@1`:
//!
//! ```text
//! { "refs": [ { "id": <id>, "kind": "result" | "characteristic" | "named_view" | "series" | "field" }, ... ],
//!   "schema": "binding@1" }
//! ```
//!
//! Each reference has exactly the keys `id` and `kind`: an ID, never a value, a label or
//! formatted text. Values sit in the kind's declared value fields as `value_binding@1`
//! ([`ValueBinding`]), which hold a record ID and digests only. Neither type has a field
//! that could carry a number, so a block cannot hold a copied value by construction; the
//! command also refuses any JSON number anywhere in the payload it stores
//! (`report.binding_has_value`).

use super::error::BindError;
use crate::doc::kind::PayloadError;
use crate::doc::value::{check_value_binding, is_pointer, BindingFault, VALUE_BINDING_SCHEMA};
use crate::fmt::SiValue;
use fid_canon::CValue;
use fid_project::Uuid;
use fid_registry::FidSchema;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Schema tag of a stored binding.
pub const BINDING_SCHEMA: &str = "binding@1";
/// The payload key a binding is stored under.
pub const BINDING_KEY: &str = "binding";
/// Most references one binding holds.
pub const MAX_REFS: usize = 4096;
/// Longest field ID, in bytes.
pub const MAX_FIELD_ID_BYTES: usize = 64;

/// What a reference points at.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    /// A journaled ResultRecord.
    Result,
    /// A journaled characteristic.
    Characteristic,
    /// A saved view (resolved by its block kind).
    NamedView,
    /// A statistics series (resolved by its block kind).
    Series,
    /// A form field supplied by a [`super::FieldSource`].
    Field,
}

impl RefKind {
    /// Every kind, in wire order.
    pub const ALL: [RefKind; 5] =
        [RefKind::Result, RefKind::Characteristic, RefKind::NamedView, RefKind::Series, RefKind::Field];

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Result => "result",
            RefKind::Characteristic => "characteristic",
            RefKind::NamedView => "named_view",
            RefKind::Series => "series",
            RefKind::Field => "field",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<RefKind> {
        RefKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A form field or table ID: `^[a-z][a-z0-9_.]{0,63}$`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldId(String);

impl FieldId {
    /// Checks a field ID; `None` when it breaks the grammar.
    pub fn new(s: &str) -> Option<FieldId> {
        let b = s.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= MAX_FIELD_ID_BYTES
            && b.first().is_some_and(u8::is_ascii_lowercase)
            && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'.');
        ok.then(|| FieldId(s.to_string()))
    }

    /// The ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One typed stable-ID reference. It holds an ID and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ref {
    Result(Uuid),
    Characteristic(Uuid),
    NamedView(Uuid),
    Series(Uuid),
    Field(FieldId),
}

impl Ref {
    /// Builds a reference from its kind and ID text: a canonical lowercase UUID (not nil or
    /// max) for every kind but `field`, a [`FieldId`] for `field`.
    pub fn new(kind: RefKind, id: &str) -> Option<Ref> {
        if kind == RefKind::Field {
            return FieldId::new(id).map(Ref::Field);
        }
        let u = Uuid::parse(id).ok().filter(|u| u.to_string() == id && *u != Uuid::NIL && *u != Uuid::MAX)?;
        Some(match kind {
            RefKind::Result => Ref::Result(u),
            RefKind::Characteristic => Ref::Characteristic(u),
            RefKind::NamedView => Ref::NamedView(u),
            _ => Ref::Series(u),
        })
    }

    pub fn kind(&self) -> RefKind {
        match self {
            Ref::Result(_) => RefKind::Result,
            Ref::Characteristic(_) => RefKind::Characteristic,
            Ref::NamedView(_) => RefKind::NamedView,
            Ref::Series(_) => RefKind::Series,
            Ref::Field(_) => RefKind::Field,
        }
    }

    /// The ID as text.
    pub fn id_text(&self) -> String {
        match self {
            Ref::Result(u) | Ref::Characteristic(u) | Ref::NamedView(u) | Ref::Series(u) => u.to_string(),
            Ref::Field(f) => f.as_str().to_string(),
        }
    }

    /// The journaled entity ID (every kind but `field`).
    pub fn uuid(&self) -> Option<Uuid> {
        match self {
            Ref::Result(u) | Ref::Characteristic(u) | Ref::NamedView(u) | Ref::Series(u) => Some(*u),
            Ref::Field(_) => None,
        }
    }

    /// `{"id": ..., "kind": ...}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), CValue::Str(self.id_text()));
        o.insert("kind".to_string(), CValue::Str(self.kind().as_str().to_string()));
        CValue::Obj(o)
    }
}

/// The references a block shows, in the order given (order is kept; a repeat is refused).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Binding {
    refs: Vec<Ref>,
}

impl Binding {
    /// Checks the references: at most [`MAX_REFS`], no repeat (`report.bad_params`).
    pub fn new(refs: Vec<Ref>) -> Result<Binding, BindError> {
        if refs.len() > MAX_REFS {
            return Err(BindError::BadParams("too many refs"));
        }
        let mut seen = BTreeSet::new();
        if !refs.iter().all(|r| seen.insert(r.clone())) {
            return Err(BindError::BadParams("duplicate ref"));
        }
        Ok(Binding { refs })
    }

    pub fn refs(&self) -> &[Ref] {
        &self.refs
    }

    /// The stored `binding@1` value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("refs".to_string(), CValue::Arr(self.refs.iter().map(Ref::to_value).collect()));
        o.insert("schema".to_string(), CValue::Str(BINDING_SCHEMA.to_string()));
        CValue::Obj(o)
    }

    /// Reads the binding of a stored payload: no [`BINDING_KEY`] is the empty binding; a
    /// malformed one is `report.store_invalid`.
    pub fn of_payload(payload: &CValue) -> Result<Binding, BindError> {
        let bad = BindError::StoreInvalid("binding");
        let CValue::Obj(p) = payload else { return Ok(Binding::default()) };
        let Some(b) = p.get(BINDING_KEY) else { return Ok(Binding::default()) };
        let CValue::Obj(o) = b else { return Err(bad) };
        if o.len() != 2 || o.get("schema") != Some(&CValue::Str(BINDING_SCHEMA.to_string())) {
            return Err(bad);
        }
        let Some(CValue::Arr(items)) = o.get("refs") else { return Err(bad) };
        let mut refs = Vec::with_capacity(items.len().min(MAX_REFS));
        for item in items {
            let CValue::Obj(r) = item else { return Err(bad) };
            let (Some(CValue::Str(kind)), Some(CValue::Str(id)), 2) = (r.get("kind"), r.get("id"), r.len()) else {
                return Err(bad);
            };
            refs.push(RefKind::parse(kind).and_then(|k| Ref::new(k, id)).ok_or(BindError::StoreInvalid("binding"))?);
        }
        Binding::new(refs).map_err(|_| BindError::StoreInvalid("binding"))
    }
}

/// A `value_binding@1` (shape pinned by `RPT.MODEL.DOC`): a record ID, the record's digest,
/// an RFC 6901 pointer to one of its value fields and the digest of the uncertainty
/// statement the record references for that field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueBinding {
    pub record: Uuid,
    pub record_digest: Digest,
    pub field: String,
    pub statement_digest: Digest,
}

impl ValueBinding {
    /// The stored value.
    pub fn to_value(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_string());
        let mut o = BTreeMap::new();
        o.insert("field".to_string(), s(&self.field));
        o.insert("record".to_string(), s(&self.record.to_string()));
        o.insert("record_digest".to_string(), s(self.record_digest.as_str()));
        o.insert("schema".to_string(), s(VALUE_BINDING_SCHEMA));
        o.insert("statement_digest".to_string(), s(self.statement_digest.as_str()));
        CValue::Obj(o)
    }

    /// Reads a declared value field at `pointer` (`report.value_literal_refused` or
    /// `report.value_binding_invalid`, as in `RPT.MODEL.DOC`).
    pub fn from_value(v: &CValue, pointer: &str) -> Result<ValueBinding, BindError> {
        use crate::doc::ReportError;
        check_value_binding(v).map_err(|e| match e {
            BindingFault::Literal => ReportError::ValueLiteralRefused(PayloadError::new(pointer, "literal")),
            BindingFault::Invalid(r) => ReportError::ValueBindingInvalid(PayloadError::new(pointer, r)),
        })?;
        let invalid = || BindError::from(ReportError::ValueBindingInvalid(PayloadError::new(pointer, "shape")));
        let CValue::Obj(o) = v else { return Err(invalid()) };
        let text = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let digest = |k: &str| text(k).and_then(|s| Digest::parse(s).ok());
        Ok(ValueBinding {
            record: text("record").and_then(|s| Uuid::parse(s).ok()).ok_or_else(invalid)?,
            record_digest: digest("record_digest").ok_or_else(invalid)?,
            field: text("field").filter(|f| is_pointer(f)).ok_or_else(invalid)?.to_string(),
            statement_digest: digest("statement_digest").ok_or_else(invalid)?,
        })
    }
}

/// An entry hash of the project's Record Journal (CORE.JRN), naming the journal as of that
/// entry. Resolution always names one explicitly; there is no "latest".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JournalHead(Digest);

impl JournalHead {
    pub fn new(d: Digest) -> JournalHead {
        JournalHead(d)
    }

    /// Parses `sha256:<64 hex>`.
    pub fn parse(s: &str) -> Option<JournalHead> {
        Digest::parse(s).ok().map(JournalHead)
    }

    pub fn digest(&self) -> Digest {
        self.0
    }
}

/// The unit of a resolved quantity (the uncertainty-statement units of `fid-kunc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuantityUnit {
    /// `si:m`.
    Metre,
    /// `si:rad`.
    Radian,
    /// `si:1`.
    One,
}

impl QuantityUnit {
    /// Parses the wire unit.
    pub fn parse(s: &str) -> Option<QuantityUnit> {
        match s {
            "si:m" => Some(QuantityUnit::Metre),
            "si:rad" => Some(QuantityUnit::Radian),
            "si:1" => Some(QuantityUnit::One),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            QuantityUnit::Metre => "si:m",
            QuantityUnit::Radian => "si:rad",
            QuantityUnit::One => "si:1",
        }
    }
}

/// A value of record as it reaches a block: a typed quantity. [`SiValue`] has no `Display`
/// and no float accessor outside `fid_report::fmt`, so a block can pass it to the formatter
/// but never turn it into text itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub value: SiValue,
    pub unit: QuantityUnit,
}
