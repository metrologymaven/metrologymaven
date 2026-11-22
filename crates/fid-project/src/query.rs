//! List and describe queries over the element tree at scale (spec node `CORE.PRJ.TREE.QUERY`).
//!
//! Pure model-layer read queries (side-effect class `R0`, determinism `D0`):
//! - [`list_elements_tree`] / [`list_elements`] (`project.list_elements@1`): filters by any
//!   combination of `type`, `state`, `group` (direct or `recursive`), and `stage`, sorted by
//!   pre-order tree order, paged at most [`MAX_PAGE_SIZE`] (5,000) per page with a
//!   head-bound [`PageToken`] (`bad_token` if malformed, `stale_token` if the head moved).
//! - [`describe_element_tree`] / [`describe_element`] (`project.describe_element@1`): returns
//!   an element's type, parent group, `group_path`, name, per-stage slots, visibility, tags
//!   and metadata, with every user-text field carrying the untrusted marker
//!   (`{"text": ..., "untrusted": true}` / `{"untrusted": true, "value": ...}`, CONSTITUTION C6).
//!
//! No `HashMap`/`HashSet`, no recursion, no panics (CONSTITUTION §4, §8).

use std::collections::BTreeMap;

use fid_canon::CValue;
use fid_types::Digest;

use crate::container::ContainerView;
use crate::id::Uuid;
use crate::stage::{STAGE_KIND, STAGE_LIST_KIND};
use crate::tree::{
    tree_index_id, Element, ElementState, Tree, TreeError, GROUP_KIND, GROUP_LIST_KIND, MAX_GROUP_DEPTH,
    TREE_INDEX_KIND,
};

/// Command name without major: `project.list_elements`.
pub const LIST_ELEMENTS: &str = "project.list_elements";
/// Command name without major: `project.describe_element`.
pub const DESCRIBE_ELEMENT: &str = "project.describe_element";
/// Maximum elements returned in one page (REQ-CORE.PRJ.TREE.QUERY-001).
pub const MAX_PAGE_SIZE: usize = 5_000;
/// Maximum accepted length of a serialized `PageToken`, in bytes.
pub const MAX_TOKEN_BYTES: usize = 128;

/// Prefix of version-1 continuation tokens.
const TOKEN_PREFIX: &str = "v1:";

/// Continuation token tying a list cursor to the branch head it was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageToken {
    /// Branch head digest when the token was issued.
    pub head: Digest,
    /// Zero-based index into the tree-order index where the next page starts (`>= 1`).
    pub offset: usize,
}

impl PageToken {
    /// Serialises the token in canonical form `v1:sha256:<64 hex>:<decimal offset>`.
    pub fn encode(&self) -> String {
        format!("{TOKEN_PREFIX}{}:{}", self.head, self.offset)
    }

    /// Parses a continuation token, failing closed with [`TreeError::BadToken`] on any
    /// syntactic deviation or overlong input.
    pub fn parse(raw: &str) -> Result<Self, TreeError> {
        if raw.is_empty() || raw.len() > MAX_TOKEN_BYTES {
            return Err(TreeError::BadToken("page token length out of range".to_owned()));
        }
        let Some(rest) = raw.strip_prefix(TOKEN_PREFIX) else {
            return Err(TreeError::BadToken("page token must start with v1:".to_owned()));
        };
        let Some(colon) = rest.rfind(':') else {
            return Err(TreeError::BadToken("page token missing offset separator".to_owned()));
        };
        let digest_str = &rest[..colon];
        let offset_str = &rest[colon + 1..];
        let head = Digest::parse(digest_str)
            .map_err(|_| TreeError::BadToken("page token has invalid head digest".to_owned()))?;
        if offset_str.is_empty()
            || !offset_str.bytes().all(|b| b.is_ascii_digit())
            || (offset_str.len() > 1 && offset_str.starts_with('0'))
        {
            return Err(TreeError::BadToken("page token has invalid offset".to_owned()));
        }
        let offset: usize =
            offset_str.parse().map_err(|_| TreeError::BadToken("page token offset overflow".to_owned()))?;
        if offset == 0 {
            return Err(TreeError::BadToken("page token offset must be positive".to_owned()));
        }
        Ok(PageToken { head, offset })
    }
}

/// Parsed filter and paging parameters for `project.list_elements@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListParams {
    pub type_id: Option<String>,
    pub state: Option<ElementState>,
    pub group: Option<Uuid>,
    pub recursive: bool,
    pub stage: Option<Uuid>,
    pub page_size: usize,
    pub page_token: Option<PageToken>,
}

impl ListParams {
    /// Validates and extracts parameters from a JSON object `CValue`.
    pub fn from_value(params: &CValue) -> Result<Self, TreeError> {
        let o = params_obj(params, &["group", "page_size", "page_token", "recursive", "stage", "state", "type"])?;
        let type_id = opt_str(o, "type")?.map(str::to_owned);
        let state = match opt_str(o, "state")? {
            Some(s) => Some(parse_state(s)?),
            None => None,
        };
        let group = opt_uuid(o, "group")?;
        let recursive = match o.get("recursive") {
            None | Some(CValue::Null) => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(TreeError::Schema("recursive must be a boolean".to_owned())),
        };
        let stage = opt_uuid(o, "stage")?;
        let page_size = match o.get("page_size") {
            None | Some(CValue::Null) => MAX_PAGE_SIZE,
            Some(CValue::Num(n)) => {
                if !n.is_finite() || n.fract() != 0.0 || *n < 1.0 || *n > MAX_PAGE_SIZE as f64 {
                    return Err(TreeError::Schema(format!("page_size must be an integer in 1..={MAX_PAGE_SIZE}")));
                }
                *n as usize
            }
            Some(_) => return Err(TreeError::Schema(format!("page_size must be an integer in 1..={MAX_PAGE_SIZE}"))),
        };
        let page_token = match o.get("page_token") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(PageToken::parse(s)?),
            Some(_) => return Err(TreeError::BadToken("page_token must be a string".to_owned())),
        };
        Ok(ListParams { type_id, state, group, recursive, stage, page_size, page_token })
    }
}

/// Executes `project.list_elements@1` directly over `&Tree` using its cached pre-order index
/// (REQ-001, REQ-003).
pub fn list_elements_tree(tree: &Tree, head: Digest, params: &CValue) -> Result<CValue, TreeError> {
    let p = ListParams::from_value(params)?;
    let start_offset = validate_token(p.page_token, head, tree.tree_order().len())?;

    let resolved_stage = match p.stage {
        Some(s) => {
            if !tree.stages().contains(&s) {
                return Err(TreeError::BadStage(s));
            }
            s
        }
        None => tree.active_stage(),
    };

    if let Some(gid) = p.group {
        if tree.group(&gid).is_none() {
            if tree.element(&gid).is_some() {
                return Err(TreeError::BadReference(format!("group {gid} is an element, not a group")));
            }
            return Err(TreeError::NotFound(format!("unknown group {gid}")));
        }
    }

    let order = tree.tree_order();
    let stage_str = resolved_stage.to_string();
    let mut elements = Vec::with_capacity(p.page_size.min(order.len().saturating_sub(start_offset)));
    let mut next_offset: Option<usize> = None;

    for (idx, eid) in order.iter().enumerate().skip(start_offset) {
        let Some(elem) = tree.element(eid) else {
            continue;
        };
        if !matches_tree_element(tree, elem, &p, resolved_stage) {
            continue;
        }
        if elements.len() == p.page_size {
            next_offset = Some(idx);
            break;
        }
        let state_str = elem.slots().get(&resolved_stage).map_or("not_computed", |s| s.state.as_str());
        elements.push(element_summary_value(
            &elem.id().to_string(),
            elem.type_id(),
            &elem.parent().to_string(),
            elem.name().as_untrusted_str(),
            state_str,
            &stage_str,
            elem.visible(),
        ));
    }

    let next = match next_offset {
        Some(offset) => CValue::Str(PageToken { head, offset }.encode()),
        None => CValue::Null,
    };
    Ok(obj([("elements", CValue::Arr(elements)), ("next", next)]))
}

fn matches_tree_element(tree: &Tree, elem: &Element, p: &ListParams, stage: Uuid) -> bool {
    if let Some(ref ty) = p.type_id {
        if elem.type_id() != ty {
            return false;
        }
    }
    if let Some(want_state) = p.state {
        let Some(slot) = elem.slots().get(&stage) else {
            return false;
        };
        if slot.state != want_state {
            return false;
        }
    }
    if let Some(gid) = p.group {
        if p.recursive {
            if !tree.is_in_group_subtree(elem.parent(), gid) {
                return false;
            }
        } else if elem.parent() != gid {
            return false;
        }
    }
    true
}

/// Executes `project.describe_element@1` directly over `&Tree` (REQ-002, REQ-005).
pub fn describe_element_tree(tree: &Tree, params: &CValue) -> Result<CValue, TreeError> {
    let id = parse_describe_id(params)?;
    let Some(elem) = tree.element(&id) else {
        return Err(TreeError::NotFound(format!("unknown element {id}")));
    };
    let group_path: Vec<CValue> =
        tree.group_path(elem.parent()).into_iter().map(|n| untrusted_text(n.as_untrusted_str())).collect();
    let slots = elem.slots().iter().map(|(s, slot)| (s.to_string(), slot.to_value())).collect();
    let tags = extract_tags(elem.meta());
    Ok(element_detail_value(
        &elem.id().to_string(),
        elem.type_id(),
        &elem.parent().to_string(),
        group_path,
        elem.name().as_untrusted_str(),
        CValue::Obj(slots),
        elem.visible(),
        tags,
        elem.meta().clone(),
    ))
}

/// Executes `project.list_elements@1` through the generic [`ContainerView`] / bus `StateView`
/// boundary (REQ-001, REQ-004).
pub fn list_elements(
    view: &dyn ContainerView,
    head: Digest,
    namespace: Uuid,
    params: &CValue,
) -> Result<CValue, TreeError> {
    let p = ListParams::from_value(params)?;
    let idx_id = tree_index_id(namespace)?;
    let Some((TREE_INDEX_KIND, CValue::Obj(idx_body))) = view.entity(&idx_id) else {
        return Err(TreeError::Invariant("tree index entity missing from state view".to_owned()));
    };

    let Some(CValue::Arr(order_vals)) = idx_body.get("order") else {
        return Err(TreeError::Invariant("tree index missing order array".to_owned()));
    };
    let start_offset = validate_token(p.page_token, head, order_vals.len())?;

    let active_stage = match idx_body.get("active_stage") {
        Some(CValue::Str(s)) => Uuid::parse(s).map_err(|_| TreeError::Invariant("bad active_stage".to_owned()))?,
        _ => return Err(TreeError::Invariant("tree index missing active_stage".to_owned())),
    };
    let resolved_stage = match p.stage {
        Some(s) => {
            let known = match idx_body.get("stages") {
                Some(CValue::Arr(st)) => st.iter().any(|v| matches!(v, CValue::Str(x) if x == &s.to_string())),
                _ => false,
            };
            if !known {
                return Err(TreeError::BadStage(s));
            }
            s
        }
        None => active_stage,
    };

    if let Some(gid) = p.group {
        match view.entity(&gid) {
            Some((GROUP_KIND, _)) => {}
            Some((kind, _)) => {
                return Err(TreeError::BadReference(format!("group {gid} has kind {kind}, expected {GROUP_KIND}")));
            }
            None => return Err(TreeError::NotFound(format!("unknown group {gid}"))),
        }
    }

    let stage_str = resolved_stage.to_string();
    let mut elements = Vec::with_capacity(p.page_size.min(order_vals.len().saturating_sub(start_offset)));
    let mut next_offset: Option<usize> = None;

    for (idx, id_val) in order_vals.iter().enumerate().skip(start_offset) {
        let CValue::Str(id_str) = id_val else {
            return Err(TreeError::Invariant("non-string ID in tree index".to_owned()));
        };
        let Ok(eid) = Uuid::parse(id_str) else {
            return Err(TreeError::Invariant("bad UUID in tree index".to_owned()));
        };
        let Some((kind, CValue::Obj(body))) = view.entity(&eid) else {
            continue;
        };
        if is_non_element_kind(kind) {
            continue;
        }
        if let Some(ref ty) = p.type_id {
            if kind != ty {
                continue;
            }
        }
        let Some(CValue::Str(parent_str)) = body.get("parent") else {
            return Err(TreeError::Invariant(format!("element {eid} missing parent")));
        };
        let Ok(parent_uuid) = Uuid::parse(parent_str) else {
            return Err(TreeError::Invariant(format!("element {eid} has invalid parent")));
        };
        if let Some(gid) = p.group {
            if p.recursive {
                if !view_in_group_subtree(view, parent_uuid, gid) {
                    continue;
                }
            } else if parent_uuid != gid {
                continue;
            }
        }
        let state_str = match body.get("slots") {
            Some(CValue::Obj(slots)) => match slots.get(&stage_str) {
                Some(CValue::Obj(slot)) => match slot.get("state") {
                    Some(CValue::Str(st)) => st.as_str(),
                    _ => return Err(TreeError::Invariant(format!("element {eid} slot missing state"))),
                },
                _ => {
                    if p.state.is_some() {
                        continue;
                    }
                    "not_computed"
                }
            },
            _ => return Err(TreeError::Invariant(format!("element {eid} missing slots"))),
        };
        if let Some(want_state) = p.state {
            if state_str != want_state.as_str() {
                continue;
            }
        }
        if elements.len() == p.page_size {
            next_offset = Some(idx);
            break;
        }
        let name_str = match body.get("name") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => "",
        };
        let visible = matches!(body.get("visible"), Some(CValue::Bool(true)));
        elements.push(element_summary_value(id_str, kind, parent_str, name_str, state_str, &stage_str, visible));
    }

    let next = match next_offset {
        Some(offset) => CValue::Str(PageToken { head, offset }.encode()),
        None => CValue::Null,
    };
    Ok(obj([("elements", CValue::Arr(elements)), ("next", next)]))
}

/// Executes `project.describe_element@1` through the generic [`ContainerView`] / bus `StateView`
/// boundary (REQ-002, REQ-005).
pub fn describe_element(view: &dyn ContainerView, params: &CValue) -> Result<CValue, TreeError> {
    let id = parse_describe_id(params)?;
    let Some((kind, CValue::Obj(body))) = view.entity(&id) else {
        return Err(TreeError::NotFound(format!("unknown element {id}")));
    };
    if is_non_element_kind(kind) {
        return Err(TreeError::NotFound(format!("{id} is not an element (kind {kind})")));
    }

    let Some(CValue::Str(parent_str)) = body.get("parent") else {
        return Err(TreeError::Invariant(format!("element {id} missing parent")));
    };
    let Ok(parent_uuid) = Uuid::parse(parent_str) else {
        return Err(TreeError::Invariant(format!("element {id} has invalid parent")));
    };
    let name_str = match body.get("name") {
        Some(CValue::Str(s)) => s.as_str(),
        _ => return Err(TreeError::Invariant(format!("element {id} missing name"))),
    };
    let Some(slots) = body.get("slots").cloned() else {
        return Err(TreeError::Invariant(format!("element {id} missing slots")));
    };
    let visible = match body.get("visible") {
        Some(CValue::Bool(b)) => *b,
        _ => return Err(TreeError::Invariant(format!("element {id} missing visible"))),
    };
    let meta = body.get("meta").cloned().unwrap_or_else(|| CValue::Obj(BTreeMap::new()));
    let group_path = view_group_path(view, parent_uuid);
    let tags = extract_tags(&meta);

    Ok(element_detail_value(&id.to_string(), kind, parent_str, group_path, name_str, slots, visible, tags, meta))
}

// ------------------------------------------------------------------ helpers

fn validate_token(token: Option<PageToken>, head: Digest, order_len: usize) -> Result<usize, TreeError> {
    match token {
        None => Ok(0),
        Some(t) => {
            if t.head != head {
                return Err(TreeError::StaleToken(format!(
                    "page token head {} does not match current head {head}",
                    t.head
                )));
            }
            if t.offset > order_len {
                return Err(TreeError::BadToken(format!(
                    "page token offset {} exceeds element count {order_len}",
                    t.offset
                )));
            }
            Ok(t.offset)
        }
    }
}

fn is_non_element_kind(kind: &str) -> bool {
    matches!(kind, GROUP_KIND | GROUP_LIST_KIND | TREE_INDEX_KIND | STAGE_KIND | STAGE_LIST_KIND)
        || kind.starts_with("project.")
}

fn view_in_group_subtree(view: &dyn ContainerView, start: Uuid, target: Uuid) -> bool {
    let mut cur = start;
    for _ in 0..=MAX_GROUP_DEPTH {
        if cur == target {
            return true;
        }
        let Some((GROUP_KIND, CValue::Obj(g))) = view.entity(&cur) else {
            return false;
        };
        match g.get("parent") {
            Some(CValue::Str(p)) => match Uuid::parse(p) {
                Ok(pid) => cur = pid,
                Err(_) => return false,
            },
            _ => return false,
        }
    }
    false
}

fn view_group_path(view: &dyn ContainerView, start: Uuid) -> Vec<CValue> {
    let mut rev = Vec::new();
    let mut cur = start;
    for _ in 0..=MAX_GROUP_DEPTH {
        let Some((GROUP_KIND, CValue::Obj(g))) = view.entity(&cur) else {
            break;
        };
        let Some(CValue::Str(p)) = g.get("parent") else {
            // Root group has `"parent": null` and is excluded from the display path.
            break;
        };
        let name = match g.get("name") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => "",
        };
        rev.push(untrusted_text(name));
        match Uuid::parse(p) {
            Ok(pid) => cur = pid,
            Err(_) => break,
        }
    }
    rev.reverse();
    rev
}

fn extract_tags(meta: &CValue) -> Vec<CValue> {
    let CValue::Obj(map) = meta else {
        return Vec::new();
    };
    let Some(CValue::Arr(arr)) = map.get("tags") else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        if let CValue::Str(s) = item {
            out.push(untrusted_text(s));
        }
    }
    out
}

fn untrusted_text(s: &str) -> CValue {
    obj([("text", CValue::Str(s.to_owned())), ("untrusted", CValue::Bool(true))])
}

fn element_summary_value(
    id: &str,
    type_id: &str,
    parent: &str,
    name: &str,
    state: &str,
    stage: &str,
    visible: bool,
) -> CValue {
    obj([
        ("id", CValue::Str(id.to_owned())),
        ("name", untrusted_text(name)),
        ("parent", CValue::Str(parent.to_owned())),
        ("stage", CValue::Str(stage.to_owned())),
        ("state", CValue::Str(state.to_owned())),
        ("type", CValue::Str(type_id.to_owned())),
        ("visible", CValue::Bool(visible)),
    ])
}

#[allow(clippy::too_many_arguments)]
fn element_detail_value(
    id: &str,
    type_id: &str,
    parent: &str,
    group_path: Vec<CValue>,
    name: &str,
    slots: CValue,
    visible: bool,
    tags: Vec<CValue>,
    metadata: CValue,
) -> CValue {
    obj([
        ("group_path", CValue::Arr(group_path)),
        ("id", CValue::Str(id.to_owned())),
        ("metadata", obj([("untrusted", CValue::Bool(true)), ("value", metadata)])),
        ("name", untrusted_text(name)),
        ("parent", CValue::Str(parent.to_owned())),
        ("slots", slots),
        ("tags", CValue::Arr(tags)),
        ("type", CValue::Str(type_id.to_owned())),
        ("visible", CValue::Bool(visible)),
    ])
}

fn parse_describe_id(params: &CValue) -> Result<Uuid, TreeError> {
    let o = params_obj(params, &["id"])?;
    match o.get("id") {
        Some(CValue::Str(s)) => {
            Uuid::parse(s).map_err(|_| TreeError::Schema(format!("id must be a canonical UUID, got {s:?}")))
        }
        _ => Err(TreeError::Schema("id is required and must be a canonical UUID".to_owned())),
    }
}

fn parse_state(s: &str) -> Result<ElementState, TreeError> {
    match s {
        "not_computed" => Ok(ElementState::NotComputed),
        "current" => Ok(ElementState::Current),
        "outdated" => Ok(ElementState::Outdated),
        "failed" => Ok(ElementState::Failed),
        _ => Err(TreeError::Schema(format!("unknown element state {s:?}"))),
    }
}

fn params_obj<'a>(params: &'a CValue, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, TreeError> {
    let CValue::Obj(o) = params else {
        return Err(TreeError::Schema("params must be an object".to_owned()));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(TreeError::Schema(format!("unknown parameter {k:?}")));
        }
    }
    Ok(o)
}

fn opt_str<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<Option<&'a str>, TreeError> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(TreeError::Schema(format!("{key} must be a string"))),
    }
}

fn opt_uuid(o: &BTreeMap<String, CValue>, key: &str) -> Result<Option<Uuid>, TreeError> {
    match opt_str(o, key)? {
        None => Ok(None),
        Some(s) => Uuid::parse(s)
            .map(Some)
            .map_err(|_| TreeError::Schema(format!("{key} must be a canonical UUID, got {s:?}"))),
    }
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
