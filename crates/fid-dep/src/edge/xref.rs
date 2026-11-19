//! Collects the entity IDs a command's parameters reference through `x-ref` (REQ-001).
//!
//! The walker follows the parameter schema and the parameter value together, through
//! `properties` (objects) and `items` (arrays) only. A schema that holds an `x-ref`
//! anywhere else (`anyOf`, `$defs`, `additionalProperties`, a non-string type, ...) is
//! refused with `unsupported_schema`, so an edge can never be silently missed.

use super::{parse_id, EdgeError, EntityId, MAX_INPUTS, MAX_SCHEMA_DEPTH, MAX_WALK_NODES};
use crate::edge::error::kept;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};

const XREF: &str = "x-ref";

/// Counts every `x-ref` key anywhere in `schema`, bounded by depth and node budgets.
fn count_all(schema: &CValue) -> Result<usize, EdgeError> {
    let mut count = 0usize;
    let mut nodes = 1usize;
    let mut stack: Vec<(&CValue, usize)> = vec![(schema, 0)];
    while let Some((v, depth)) = stack.pop() {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(EdgeError::InputLimit("schema_depth"));
        }
        let children = match v {
            CValue::Obj(o) => {
                if o.contains_key(XREF) {
                    count += 1;
                }
                o.len()
            }
            CValue::Arr(a) => a.len(),
            _ => 0,
        };
        // Counted before pushing, so one huge object or array cannot grow the stack.
        nodes = nodes.saturating_add(children);
        if nodes > MAX_WALK_NODES {
            return Err(EdgeError::InputLimit("schema_nodes"));
        }
        match v {
            CValue::Obj(o) => stack.extend(o.values().map(|c| (c, depth + 1))),
            CValue::Arr(a) => stack.extend(a.iter().map(|c| (c, depth + 1))),
            _ => {}
        }
    }
    Ok(count)
}

/// Counts the `x-ref` keys reachable through `properties` and `items` only, and checks
/// that each such node is a plain string reference.
fn count_supported(schema: &CValue) -> Result<usize, EdgeError> {
    let mut count = 0usize;
    let mut stack: Vec<&CValue> = vec![schema];
    while let Some(v) = stack.pop() {
        let CValue::Obj(o) = v else { continue };
        if let Some(r) = o.get(XREF) {
            if !matches!(r, CValue::Str(_)) {
                return Err(EdgeError::UnsupportedSchema("x-ref value is not a string"));
            }
            match o.get("type") {
                None => {}
                Some(CValue::Str(t)) if t == "string" => {}
                Some(_) => return Err(EdgeError::UnsupportedSchema("x-ref on a type other than string")),
            }
            count += 1;
        }
        if let Some(CValue::Obj(props)) = o.get("properties") {
            stack.extend(props.values());
        }
        if let Some(items) = o.get("items") {
            if matches!(items, CValue::Obj(_)) {
                stack.push(items);
            }
        }
    }
    Ok(count)
}

/// Checks that every `x-ref` in `schema` is one the walker follows.
fn check_schema(schema: &CValue) -> Result<(), EdgeError> {
    // The full count runs first: it bounds depth and size, so the second walk is bounded too.
    let all = count_all(schema)?;
    let supported = count_supported(schema)?;
    if all != supported {
        return Err(EdgeError::UnsupportedSchema("x-ref under a keyword other than properties or items"));
    }
    Ok(())
}

fn props(o: &BTreeMap<String, CValue>) -> Option<&BTreeMap<String, CValue>> {
    match o.get("properties") {
        Some(CValue::Obj(p)) => Some(p),
        _ => None,
    }
}

/// Longest JSON path kept for error messages, in bytes; deeper segments are dropped.
const MAX_PATH: usize = 256;

fn child_path(path: &str, segment: &str) -> String {
    if path.len() + 1 + segment.len() > MAX_PATH {
        return path.to_string();
    }
    format!("{path}/{segment}")
}

/// The distinct entity IDs that `params` references through `x-ref` positions of
/// `schema`, in ascending order. A value at an `x-ref` position must be a canonical
/// lowercase UUID string (`bad_reference` otherwise). Parameters the schema does not
/// describe are ignored: validating params against the schema is the bus's job.
pub(crate) fn collect(schema: &CValue, params: &CValue) -> Result<BTreeSet<EntityId>, EdgeError> {
    check_schema(schema)?;
    let mut out = BTreeSet::new();
    let mut nodes = 1usize;
    let mut stack: Vec<(&CValue, &CValue, String, usize)> = vec![(schema, params, String::new(), 0)];
    while let Some((s, v, path, depth)) = stack.pop() {
        let children = match v {
            CValue::Obj(o) => o.len(),
            CValue::Arr(a) => a.len(),
            _ => 0,
        };
        // Counted before pushing, so one huge object or array cannot grow the stack.
        nodes = nodes.saturating_add(children);
        if nodes > MAX_WALK_NODES {
            return Err(EdgeError::InputLimit("param_nodes"));
        }
        if depth > MAX_SCHEMA_DEPTH {
            return Err(EdgeError::InputLimit("param_depth"));
        }
        let CValue::Obj(so) = s else { continue };
        if so.contains_key(XREF) {
            let id = match v {
                CValue::Str(text) => parse_id(text),
                _ => None,
            };
            let Some(id) = id else {
                return Err(EdgeError::BadReference(kept(&path)));
            };
            out.insert(id);
            if out.len() > MAX_INPUTS {
                return Err(EdgeError::InputLimit("inputs"));
            }
            continue;
        }
        match v {
            CValue::Obj(vo) => {
                if let Some(p) = props(so) {
                    for (k, child) in vo {
                        if let Some(sub) = p.get(k) {
                            stack.push((sub, child, child_path(&path, &kept(k)), depth + 1));
                        }
                    }
                }
            }
            CValue::Arr(items) => {
                if let Some(sub) = so.get("items") {
                    for (i, child) in items.iter().enumerate() {
                        stack.push((sub, child, child_path(&path, &i.to_string()), depth + 1));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}
