//! Test-only `table` kind and its resolver (feature `test-kinds`). Not shipped: the real
//! `table` kind belongs to `RPT.MODEL.BLOCKS`.
//!
//! Payload (version 1): `{"rows": [{"value": <value_binding@1>}, ...]}`, plus the optional
//! keys `binding` (the stored `binding@1`) and `note` (any JSON, for tests of the
//! no-number rule). Value fields: `/rows/<i>/value`. [`TableResolver::place`] writes one
//! row per value binding, in order, and keeps `note`.

use super::error::BindError;
use super::journal::Lookup;
use super::model::{Binding, ValueBinding, BINDING_KEY};
use super::resolve::{BlockResolver, ResolveContext, Resolved};
use crate::doc::kind::{BlockKind, PayloadError};
use crate::doc::{BlockKindId, CanonicalJson};
use fid_canon::CValue;
use std::collections::BTreeMap;

fn rows(payload: &CanonicalJson) -> Result<&Vec<CValue>, PayloadError> {
    let CValue::Obj(o) = payload.value() else { return Err(PayloadError::new("", "not_object")) };
    if o.keys().any(|k| !matches!(k.as_str(), "rows" | "note" | BINDING_KEY)) {
        return Err(PayloadError::new("", "unexpected_keys"));
    }
    match o.get("rows") {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(PayloadError::new("/rows", "not_array")),
    }
}

/// The `table` stub kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct TableStub;

impl BlockKind for TableStub {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal("table")
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        for (i, r) in rows(payload)?.iter().enumerate() {
            match r {
                CValue::Obj(o) if o.len() == 1 && o.contains_key("value") => {}
                _ => return Err(PayloadError::new(&format!("/rows/{i}"), "row")),
            }
        }
        Ok(())
    }

    fn value_fields(&self, payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok((0..rows(payload)?.len()).map(|i| format!("/rows/{i}/value")).collect())
    }
}

/// The resolver of the `table` stub kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct TableResolver;

impl BlockResolver for TableResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal("table")
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        let CValue::Obj(o) = payload.value() else { return Err(PayloadError::new("", "not_object")) };
        let mut out = BTreeMap::new();
        if let Some(note) = o.get("note") {
            out.insert("note".to_string(), note.clone());
        }
        let rows = values.iter().map(|v| CValue::Obj(BTreeMap::from([("value".to_string(), v.to_value())]))).collect();
        out.insert("rows".to_string(), CValue::Arr(rows));
        Ok(CValue::Obj(out))
    }

    fn resolve(&self, view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        let rows = view.values().iter().map(|v| CValue::Str(v.pointer().to_string())).collect();
        Ok(CValue::Obj(BTreeMap::from([("rows".to_string(), CValue::Arr(rows))])))
    }
}
