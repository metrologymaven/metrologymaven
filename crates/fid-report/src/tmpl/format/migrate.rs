//! Schema migration: pure functions from one schema version's value to the next.
//!
//! Chain: `0 -> 1`. Migrating from the current version is the identity; from a version
//! above it is `report.template_schema_unsupported`. Each step is a pure function of its
//! input value (no clock, no randomness, no ID drawing), so the same input always gives the
//! same canonical bytes.
//!
//! Schema version 0 (the hand-written pre-release form, pinned in the leaf's Design):
//!
//! ```text
//! { "format": "fid.report-template", "schema_version": 0, "id": <uuidv7>, "name": <string>,
//!   "locked": <bool, optional, default false>,
//!   "pages": [ { "id": <uuidv7>,
//!                "blocks": [ { "id": <uuidv7>, "kind": <kind id>, "version": <JSON integer>,
//!                              "data": <json>,
//!                              "slot": <optional, null or {"kind": <slot kind>, "rule": <object>}>,
//!                              "repeat_header": <optional JSON integer, default 0> } ] } ] }
//! ```
//!
//! Step `0 -> 1`: drop `format`; `id` becomes `template_id`; `name` becomes `title`
//! (`{"text": name, "untrusted": true}`); `locked` defaults to `false`; add `clearance:
//! null` and the media type; per block `version` becomes the decimal string
//! `kind_version`, `data` becomes `payload` (`{"untrusted": true, "value": data}`), a slot's
//! `rule` becomes `spec` (`{"untrusted": true, "value": rule}`), `repeat_header` becomes the
//! decimal string `repeat_header_rows`, and `continuation_block: null` is added;
//! `schema_version` becomes `"1"`. Values are moved, never reinterpreted; the v1 check
//! that follows judges them. Errors found during a step name the pointer in the input of
//! that step.

use super::error::{child, TemplateError};
use super::model::obj;
use super::read::{exact, get};
use super::{MEDIA_TYPE, SCHEMA_VERSION};
use fid_canon::CValue;

/// The `format` tag of schema version 0.
pub const V0_FORMAT: &str = "fid.report-template";

/// Migrates `value`, written in schema version `from`, to the current schema version.
pub fn migrate(value: &CValue, from: u32) -> Result<CValue, TemplateError> {
    if from > SCHEMA_VERSION {
        return Err(TemplateError::SchemaUnsupported { version: from });
    }
    let mut v = value.clone();
    let mut at = from;
    while at < SCHEMA_VERSION {
        v = match at {
            0 => v0_to_v1(&v)?,
            _ => return Err(TemplateError::SchemaUnsupported { version: from }),
        };
        at += 1;
    }
    Ok(v)
}

fn bad(at: &str, reason: &'static str) -> TemplateError {
    TemplateError::invalid(at, reason)
}

/// A JSON integer in `0..=u32::MAX` as a canonical decimal string.
fn int_string(v: &CValue, at: &str) -> Result<CValue, TemplateError> {
    match crate::json_num::as_u32(v) {
        Some(n) => Ok(CValue::Str(n.to_string())),
        None => Err(bad(at, "bad_integer")),
    }
}

/// The keys of `v` must be `required` plus any of `optional` (all in code-point order).
fn keys_v0<'a>(
    v: &'a CValue,
    at: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a std::collections::BTreeMap<String, CValue>, TemplateError> {
    let CValue::Obj(o) = v else { return Err(bad(at, "wrong_type")) };
    if let Some(k) = o.keys().find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str())) {
        return Err(bad(&child(at, k), "unknown_field"));
    }
    if let Some(k) = required.iter().find(|k| !o.contains_key(**k)) {
        return Err(bad(&child(at, k), "missing_field"));
    }
    Ok(o)
}

fn wrap(v: &CValue) -> CValue {
    obj([("untrusted", CValue::Bool(true)), ("value", v.clone())])
}

fn v0_to_v1(v: &CValue) -> Result<CValue, TemplateError> {
    let o = keys_v0(v, "", &["format", "id", "name", "pages", "schema_version"], &["locked"])?;
    let (f, fp) = get(o, "", "format")?;
    if f != &CValue::Str(V0_FORMAT.to_string()) {
        return Err(bad(&fp, "bad_format"));
    }
    let (ps, pp) = get(o, "", "pages")?;
    let CValue::Arr(pages_in) = ps else { return Err(bad(&pp, "wrong_type")) };
    let mut pages = Vec::with_capacity(pages_in.len());
    for (i, p) in pages_in.iter().enumerate() {
        let at = child(&pp, &i.to_string());
        let po = exact(p, &at, &["blocks", "id"])?;
        let (bs, bp) = get(po, &at, "blocks")?;
        let CValue::Arr(blocks_in) = bs else { return Err(bad(&bp, "wrong_type")) };
        let mut blocks = Vec::with_capacity(blocks_in.len());
        for (j, b) in blocks_in.iter().enumerate() {
            let bat = child(&bp, &j.to_string());
            blocks.push(block_v0(b, &bat)?);
        }
        let (id, _) = get(po, &at, "id")?;
        pages.push(obj([("blocks", CValue::Arr(blocks)), ("id", id.clone())]));
    }
    let (id, _) = get(o, "", "id")?;
    let (name, _) = get(o, "", "name")?;
    let locked = o.get("locked").cloned().unwrap_or(CValue::Bool(false));
    Ok(obj([
        ("clearance", CValue::Null),
        ("locked", locked),
        ("media_type", CValue::Str(MEDIA_TYPE.to_string())),
        ("pages", CValue::Arr(pages)),
        ("schema_version", CValue::Str(SCHEMA_VERSION.to_string())),
        ("template_id", id.clone()),
        ("title", obj([("text", name.clone()), ("untrusted", CValue::Bool(true))])),
    ]))
}

fn block_v0(b: &CValue, at: &str) -> Result<CValue, TemplateError> {
    let o = keys_v0(b, at, &["data", "id", "kind", "version"], &["repeat_header", "slot"])?;
    let (data, _) = get(o, at, "data")?;
    let (id, _) = get(o, at, "id")?;
    let (kind, _) = get(o, at, "kind")?;
    let (ver, verp) = get(o, at, "version")?;
    let kind_version = int_string(ver, &verp)?;
    let repeat = match o.get("repeat_header") {
        None => CValue::Str("0".to_string()),
        Some(r) => int_string(r, &child(at, "repeat_header"))?,
    };
    let slot = match o.get("slot") {
        None | Some(CValue::Null) => CValue::Null,
        Some(s) => {
            let sp = child(at, "slot");
            let so = exact(s, &sp, &["kind", "rule"])?;
            let (k, _) = get(so, &sp, "kind")?;
            let (rule, _) = get(so, &sp, "rule")?;
            obj([("kind", k.clone()), ("spec", wrap(rule))])
        }
    };
    Ok(obj([
        ("continuation_block", CValue::Null),
        ("id", id.clone()),
        ("kind", kind.clone()),
        ("kind_version", kind_version),
        ("payload", wrap(data)),
        ("repeat_header_rows", repeat),
        ("slot", slot),
    ]))
}
