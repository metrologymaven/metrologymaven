//! The `project.show_dependencies@1` query (R0, D0): parameters, result and registry
//! definition (REQ-004, REQ-005).

use super::{ids_value, parse_id, EdgeError, EdgeIndex, EntityId};
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// Command ID of the dependency query.
pub const SHOW_DEPENDENCIES: &str = "project.show_dependencies@1";
const NAME: &str = "project.show_dependencies";
const LIFECYCLE_SINCE: &str = "2026-11-19";
/// Pattern of a canonical entity ID in the published schemas.
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// Parameters: `{ element: EntityRef, transitive: bool = false }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowDependencies {
    pub element: EntityId,
    pub transitive: bool,
}

impl ShowDependencies {
    /// Reads the parameter object. Fails closed (`bad_params`) on an unknown key, a
    /// missing `element`, a non-canonical ID or a non-boolean `transitive`.
    pub fn from_value(v: &CValue) -> Result<ShowDependencies, EdgeError> {
        let CValue::Obj(o) = v else { return Err(EdgeError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "element" && k != "transitive") {
            return Err(EdgeError::BadParams("unknown parameter"));
        }
        let element = match o.get("element") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(EdgeError::BadParams("element is not a canonical id"))?,
            Some(_) => return Err(EdgeError::BadParams("element is not a string")),
            None => return Err(EdgeError::BadParams("missing element")),
        };
        let transitive = match o.get("transitive") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(EdgeError::BadParams("transitive is not a boolean")),
        };
        Ok(ShowDependencies { element, transitive })
    }
}

/// Result: `{ dependencies: [EntityId], dependents: [EntityId] }`, each ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependencies {
    pub dependencies: Vec<EntityId>,
    pub dependents: Vec<EntityId>,
}

impl Dependencies {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("dependencies".to_string(), ids_value(&self.dependencies));
        o.insert("dependents".to_string(), ids_value(&self.dependents));
        CValue::Obj(o)
    }
}

/// Runs `project.show_dependencies@1` on `index`: direct dependencies and dependents, or
/// with `transitive` all of them, each sorted by entity ID. `not_found` for an unknown
/// element.
pub fn show_dependencies(index: &EdgeIndex, params: &ShowDependencies) -> Result<Dependencies, EdgeError> {
    let e = &params.element;
    if params.transitive {
        Ok(Dependencies {
            dependencies: index.transitive_dependencies(e)?,
            dependents: index.transitive_dependents(e)?,
        })
    } else {
        Ok(Dependencies { dependencies: index.dependencies(e)?, dependents: index.dependents(e)? })
    }
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn id_list(description: &str) -> CValue {
    obj([
        ("type", text("array")),
        ("items", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
        ("description", text(description)),
    ])
}

fn params_schema() -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element"])),
        (
            "properties",
            obj([
                (
                    "element",
                    obj([
                        ("type", text("string")),
                        ("x-ref", text("element")),
                        ("pattern", text(ID_PATTERN)),
                        ("description", text("The element whose dependencies and dependents are listed.")),
                    ]),
                ),
                (
                    "transitive",
                    obj([
                        ("type", text("boolean")),
                        ("default", CValue::Bool(false)),
                        ("description", text("List all transitive dependencies and dependents, not only direct ones.")),
                    ]),
                ),
            ]),
        ),
    ])
}

fn result_schema() -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["dependencies", "dependents"])),
        (
            "properties",
            obj([
                ("dependencies", id_list("Elements the element was computed from, ascending by entity ID.")),
                ("dependents", id_list("Elements computed from the element, ascending by entity ID.")),
            ]),
        ),
    ])
}

/// The registry definition of `project.show_dependencies@1`, built through
/// [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn command_def() -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(NAME)),
        ("major", CValue::Num(1.0)),
        ("title", text("Show dependencies")),
        (
            "description",
            text("Lists an element's dependencies and dependents, direct or transitive, each sorted by entity ID."),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
