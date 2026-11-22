//! Groups, move, rename and visibility commands on the element tree.
//! Spec node `CORE.PRJ.TREE.GROUP` (CONSTITUTION C3, C6 untrusted text, §4 ordered collections,
//! §5 stable IDs, §8 no panics).
//!
//! Pure model-layer command logic reading through [`ContainerView`] and returning one
//! [`TreeEvent`] per call; the bus handlers, registry definitions and bus error mapping live
//! in `fid-project-cmd::tree_cmds`.
//!
//! Commands (all W2, D0, undoable by head move):
//! - [`create_group`] (`project.create_group@1`) -> `GroupCreated`
//! - [`move_element`] (`project.move_element@1`) -> `ElementsMoved`
//! - [`rename_element`] (`project.rename_element@1`) -> `Renamed`
//! - [`set_visibility`] (`project.set_visibility@1`) -> `VisibilitySet`

use std::collections::{BTreeMap, BTreeSet};

use fid_canon::CValue;

use crate::container::{ContainerKind, ContainerView};
use crate::id::{IdIndex, IdService, ObservationSource, Uuid};
use crate::stage::{STAGE_KIND, STAGE_LIST_KIND};
use crate::tree::{
    group_list_id, reserved_id, uuid_batch_field, ElementName, TreeError, TreeEvent, ELEMENTS_MOVED, GROUP_CREATED,
    GROUP_KIND, GROUP_LIST_KIND, MAX_GROUP_DEPTH, RENAMED, VISIBILITY_SET,
};

/// Command name without major: `project.create_group`.
pub const CREATE_GROUP: &str = "project.create_group";
/// Command name without major: `project.move_element`.
pub const MOVE_ELEMENT: &str = "project.move_element";
/// Command name without major: `project.rename_element`.
pub const RENAME_ELEMENT: &str = "project.rename_element";
/// Command name without major: `project.set_visibility`.
pub const SET_VISIBILITY: &str = "project.set_visibility";

struct ViewIndex<'a>(&'a dyn ContainerView);

impl IdIndex for ViewIndex<'_> {
    fn contains_id(&self, id: &Uuid) -> bool {
        self.0.entity(id).is_some()
    }
}

/// Reserved root group ID of `namespace` (matches `Tree::new(namespace, ..)`).
pub fn tree_root_id(namespace: Uuid) -> Result<Uuid, TreeError> {
    reserved_id(namespace, "project.tree_root")
}

/// `project.create_group@1` (REQ-CORE.PRJ.TREE.GROUP-001).
///
/// Params: `{"name": string, "parent"?: uuid, "id"?: uuid}`.
/// Defaults `parent` to the tree root of `namespace` when omitted or null.
pub fn create_group(
    view: &dyn ContainerView,
    params: &CValue,
    namespace: Uuid,
    source: &mut dyn ObservationSource,
) -> Result<TreeEvent, TreeError> {
    let o = params_obj(params, &["id", "name", "parent"])?;
    let name = ElementName::new(req_str(o, "name")?)?;
    let parent = match opt_uuid(o, "parent")? {
        Some(p) => p,
        None => tree_root_id(namespace)?,
    };
    let imported = opt_uuid(o, "id")?;

    ensure_group(view, parent, "parent")?;
    let depth = group_depth(view, parent)?;
    if depth + 1 > MAX_GROUP_DEPTH {
        return Err(TreeError::Hierarchy(format!("a group under {parent} would be deeper than {MAX_GROUP_DEPTH}")));
    }

    let idx = ViewIndex(view);
    let mut ids = IdService::new(namespace, source, &idx);
    let id = match imported {
        Some(given) => ids.import_uuid(given)?,
        None => ids.new_v7()?,
    };

    Ok(TreeEvent::new(
        GROUP_CREATED,
        obj([
            ("id", CValue::Str(id.to_string())),
            ("name", CValue::Str(name.as_untrusted_str().to_string())),
            ("parent", CValue::Str(parent.to_string())),
        ]),
    ))
}

/// `project.move_element@1` (REQ-CORE.PRJ.TREE.GROUP-002, REQ-CORE.PRJ.TREE.GROUP-003).
///
/// Params: `{"ids": [uuid] (1..=10_000), "to": uuid}`.
/// Rejects any move that would make a group its own ancestor (`hierarchy`), exceed
/// [`MAX_GROUP_DEPTH`] including moved group subtrees, or move the root.
pub fn move_element(view: &dyn ContainerView, params: &CValue, namespace: Uuid) -> Result<TreeEvent, TreeError> {
    let o = params_obj(params, &["ids", "to"])?;
    let ids = uuid_batch_field(o, "ids")?;
    let to = req_uuid(o, "to")?;

    ensure_group(view, to, "to")?;
    let ancestors = group_ancestors(view, to)?;
    let to_depth = ancestors.len().saturating_sub(1);
    let mut group_children: Option<BTreeMap<Uuid, Vec<Uuid>>> = None;

    for id in &ids {
        let Some((kind, body)) = view.entity(id) else {
            return Err(TreeError::NotFound(format!("entity {id} not found")));
        };
        if kind == GROUP_KIND {
            let parent = group_parent_from_body(body, *id)?;
            if parent.is_none() {
                return Err(TreeError::Hierarchy(format!("root group {id} cannot be moved")));
            }
            if ancestors.contains(id) {
                return Err(TreeError::Hierarchy(format!(
                    "moving group {id} into {to} would make it its own ancestor"
                )));
            }
            let children_map = match group_children.as_ref() {
                Some(m) => m,
                None => {
                    let loaded = load_group_children(view, namespace)?;
                    group_children.insert(loaded)
                }
            };
            let h = subtree_height(children_map, *id)?;
            if to_depth + 1 + h > MAX_GROUP_DEPTH {
                return Err(TreeError::Hierarchy(format!(
                    "moving group {id} under {to} would exceed depth {MAX_GROUP_DEPTH}"
                )));
            }
        } else if !is_element_kind(kind) {
            return Err(TreeError::BadReference(format!("entity {id} ({kind}) is neither an element nor a group")));
        }
    }

    Ok(TreeEvent::new(
        ELEMENTS_MOVED,
        obj([
            ("ids", CValue::Arr(ids.iter().map(|id| CValue::Str(id.to_string())).collect())),
            ("to", CValue::Str(to.to_string())),
        ]),
    ))
}

/// `project.rename_element@1` (REQ-CORE.PRJ.TREE.GROUP-004).
///
/// Params: `{"id": uuid, "name": string}`.
/// Sets the element's or group's name to untrusted text of 0..=256 characters without checking
/// uniqueness.
pub fn rename_element(view: &dyn ContainerView, params: &CValue) -> Result<TreeEvent, TreeError> {
    let o = params_obj(params, &["id", "name"])?;
    let id = req_uuid(o, "id")?;
    let name = ElementName::new(req_str(o, "name")?)?;

    let Some((kind, _)) = view.entity(&id) else {
        return Err(TreeError::NotFound(format!("entity {id} not found")));
    };
    if kind != GROUP_KIND && !is_element_kind(kind) {
        return Err(TreeError::BadReference(format!("entity {id} ({kind}) is neither an element nor a group")));
    }

    Ok(TreeEvent::new(
        RENAMED,
        obj([("id", CValue::Str(id.to_string())), ("name", CValue::Str(name.as_untrusted_str().to_string()))]),
    ))
}

/// `project.set_visibility@1` (REQ-CORE.PRJ.TREE.GROUP-005).
///
/// Params: `{"ids": [uuid] (1..=10_000), "visible": bool}`.
pub fn set_visibility(view: &dyn ContainerView, params: &CValue) -> Result<TreeEvent, TreeError> {
    let o = params_obj(params, &["ids", "visible"])?;
    let ids = uuid_batch_field(o, "ids")?;
    let visible = match o.get("visible") {
        Some(CValue::Bool(b)) => *b,
        _ => return Err(TreeError::Schema("visible must be a boolean".to_string())),
    };

    for id in &ids {
        let Some((kind, _)) = view.entity(id) else {
            return Err(TreeError::NotFound(format!("element {id} not found")));
        };
        if !is_element_kind(kind) {
            return Err(TreeError::BadReference(format!("entity {id} ({kind}) is not an element")));
        }
    }

    Ok(TreeEvent::new(
        VISIBILITY_SET,
        obj([
            ("ids", CValue::Arr(ids.iter().map(|id| CValue::Str(id.to_string())).collect())),
            ("visible", CValue::Bool(visible)),
        ]),
    ))
}

// ------------------------------------------------------------------ helpers

fn is_element_kind(kind: &str) -> bool {
    kind != GROUP_KIND
        && kind != GROUP_LIST_KIND
        && kind != STAGE_KIND
        && kind != STAGE_LIST_KIND
        && ContainerKind::parse(kind).is_none()
}

fn load_group_children(view: &dyn ContainerView, namespace: Uuid) -> Result<BTreeMap<Uuid, Vec<Uuid>>, TreeError> {
    let list_id = group_list_id(namespace)?;
    let Some((kind, body)) = view.entity(&list_id) else {
        return Ok(BTreeMap::new());
    };
    if kind != GROUP_LIST_KIND {
        return Err(TreeError::Invariant(format!("group list {list_id} has unexpected kind {kind}")));
    }
    let CValue::Obj(o) = body else {
        return Err(TreeError::Invariant("group list body is not an object".to_string()));
    };
    let Some(CValue::Obj(map)) = o.get("children") else {
        return Err(TreeError::Invariant("group list body missing children map".to_string()));
    };
    let mut out = BTreeMap::new();
    for (k, v) in map {
        let parent = parse_uuid(k, "group_list key")
            .map_err(|_| TreeError::Invariant(format!("group list has malformed key {k}")))?;
        let CValue::Arr(arr) = v else {
            return Err(TreeError::Invariant(format!("group list children for {parent} is not an array")));
        };
        let mut kids = Vec::with_capacity(arr.len());
        for item in arr {
            let CValue::Str(s) = item else {
                return Err(TreeError::Invariant(format!("group list child for {parent} is not a string")));
            };
            let kid = parse_uuid(s, "group_list child")
                .map_err(|_| TreeError::Invariant(format!("group list has malformed child {s}")))?;
            kids.push(kid);
        }
        out.insert(parent, kids);
    }
    Ok(out)
}

fn subtree_height(children: &BTreeMap<Uuid, Vec<Uuid>>, group: Uuid) -> Result<usize, TreeError> {
    let mut max_h = 0usize;
    let mut visited = BTreeSet::new();
    let mut stack: Vec<(Uuid, usize)> = vec![(group, 0)];
    while let Some((cur, h)) = stack.pop() {
        if !visited.insert(cur) {
            return Err(TreeError::Invariant(format!("cycle in group hierarchy at {cur}")));
        }
        if h > max_h {
            max_h = h;
        }
        if let Some(kids) = children.get(&cur) {
            for kid in kids {
                stack.push((*kid, h + 1));
            }
        }
    }
    Ok(max_h)
}

fn ensure_group(view: &dyn ContainerView, id: Uuid, field: &str) -> Result<(), TreeError> {
    match view.entity(&id) {
        Some((GROUP_KIND, _)) => Ok(()),
        _ => Err(TreeError::BadReference(format!("{field} {id} is not a group of this project"))),
    }
}

fn group_parent_from_body(body: &CValue, id: Uuid) -> Result<Option<Uuid>, TreeError> {
    let CValue::Obj(o) = body else {
        return Err(TreeError::Invariant(format!("group {id} body is not an object")));
    };
    match o.get("parent") {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => match Uuid::parse(s) {
            Ok(p) if p.to_string() == *s => Ok(Some(p)),
            _ => Err(TreeError::Invariant(format!("group {id} has malformed parent"))),
        },
        _ => Err(TreeError::Invariant(format!("group {id} missing parent field"))),
    }
}

/// Walks parents of `group` up to the root, returning the set of all ancestors including `group`.
fn group_ancestors(view: &dyn ContainerView, group: Uuid) -> Result<BTreeSet<Uuid>, TreeError> {
    let mut ancestors = BTreeSet::new();
    let mut cur = group;
    let mut steps = 0usize;
    loop {
        if !ancestors.insert(cur) {
            return Err(TreeError::Invariant(format!("cycle in group hierarchy at {cur}")));
        }
        let Some((GROUP_KIND, body)) = view.entity(&cur) else {
            return Err(TreeError::Invariant(format!("group {cur} is an orphan")));
        };
        match group_parent_from_body(body, cur)? {
            None => return Ok(ancestors),
            Some(parent) => {
                steps += 1;
                if steps > MAX_GROUP_DEPTH {
                    return Err(TreeError::Hierarchy(format!("group {group} is deeper than {MAX_GROUP_DEPTH}")));
                }
                cur = parent;
            }
        }
    }
}

fn group_depth(view: &dyn ContainerView, group: Uuid) -> Result<usize, TreeError> {
    Ok(group_ancestors(view, group)?.len().saturating_sub(1))
}

fn params_obj<'a>(params: &'a CValue, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, TreeError> {
    let CValue::Obj(o) = params else {
        return Err(TreeError::Schema("params must be an object".to_string()));
    };
    for k in o.keys() {
        if !allowed.iter().any(|a| a == k) {
            return Err(TreeError::Schema(format!("unexpected parameter `{k}`")));
        }
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, TreeError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(TreeError::Schema(format!("{key} must be a string"))),
    }
}

fn parse_uuid(s: &str, label: &str) -> Result<Uuid, TreeError> {
    match Uuid::parse(s) {
        Ok(id) if id.to_string() == s => Ok(id),
        _ => Err(TreeError::Schema(format!("{label} must be a canonical lowercase UUID"))),
    }
}

fn req_uuid(o: &BTreeMap<String, CValue>, key: &str) -> Result<Uuid, TreeError> {
    parse_uuid(req_str(o, key)?, key)
}

fn opt_uuid(o: &BTreeMap<String, CValue>, key: &str) -> Result<Option<Uuid>, TreeError> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(parse_uuid(s, key)?)),
        _ => Err(TreeError::Schema(format!("{key} must be a canonical lowercase UUID"))),
    }
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
