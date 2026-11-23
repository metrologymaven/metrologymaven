//! Input replacement (`project.replace_input@1`). Spec node `CORE.DEP.REPL`.
//!
//! Swaps the content of an input element (`target`) for the content of a staged element
//! (`replacement`) of the exact same registered type (`kind`), keeping `target`'s ID, name,
//! group, tags and metadata (`REQ-001`), removing `replacement` from the tree in the same
//! commit (`REQ-003`), marking every transitive dependent of `target` `outdated` in the same
//! commit via [`crate::prop::propagate`] (`REQ-004`), and submitting one recompute when in
//! `auto` mode (`REQ-005`).

use crate::edge::{EdgeIndex, EntityId};
use crate::prop::{
    propagate, ElementState, ElementStates, ElementsMarked, PropError, RecomputeMode, RecomputeSubmission,
};
use crate::recompute::{OverlayEvent, OverlayView};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue, Digest};
use fid_registry::{CommandDef, DefError};
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;

/// Full command ID string (`project.replace_input@1`).
pub const REPLACE_INPUT: &str = "project.replace_input@1";
/// Command name without `@major`.
pub const REPLACE_INPUT_NAME: &str = "project.replace_input";
/// Event emitted when `target`'s content is swapped for `replacement`'s content.
pub const INPUT_REPLACED_EVENT: &str = "InputReplaced";
/// Event emitted when the consumed `replacement` element is removed from the tree.
pub const ELEMENT_REMOVED_EVENT: &str = "ElementRemoved";
const LIFECYCLE_SINCE: &str = "2026-11-23";
const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";

// ------------------------------------------------------------------ errors

/// Typed failures of `CORE.DEP.REPL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceError {
    /// Target or replacement entity does not exist (`not_found`).
    NotFound(EntityId),
    /// Replacement's registered element type differs from target's (`type_mismatch`, `REQ-002`).
    TypeMismatch { target: EntityId, target_kind: String, replacement: EntityId, replacement_kind: String },
    /// Replacement already has dependents of its own (`replacement_in_use`, `REQ-007`).
    ReplacementInUse { replacement: EntityId, dependents: Vec<EntityId> },
    /// Invalid parameters (e.g. target == replacement, malformed schema) (`validation`).
    Validation(&'static str),
    /// Propagation error (`PropError`).
    Propagation(PropError),
    /// Malformed event during replay (`bad_event`).
    BadEvent(&'static str),
    /// Canonical encoding / internal invariant error (`internal`).
    Internal(String),
}

impl ReplaceError {
    /// Stable error code (`type_mismatch`, `replacement_in_use`, `not_found`, `validation`, `internal`).
    pub fn code(&self) -> &'static str {
        match self {
            ReplaceError::NotFound(_) => "not_found",
            ReplaceError::TypeMismatch { .. } => "type_mismatch",
            ReplaceError::ReplacementInUse { .. } => "replacement_in_use",
            ReplaceError::Validation(_) => "validation",
            ReplaceError::Propagation(e) => e.code(),
            ReplaceError::BadEvent(_) | ReplaceError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for ReplaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplaceError::NotFound(id) => write!(f, "not_found: element {id} does not exist"),
            ReplaceError::TypeMismatch { target, target_kind, replacement, replacement_kind } => write!(
                f,
                "type_mismatch: target {target} is {target_kind}, replacement {replacement} is {replacement_kind}"
            ),
            ReplaceError::ReplacementInUse { replacement, dependents } => {
                write!(f, "replacement_in_use: replacement {replacement} already has {} dependent(s)", dependents.len())
            }
            ReplaceError::Validation(msg) => write!(f, "validation: {msg}"),
            ReplaceError::Propagation(e) => write!(f, "propagation: {e}"),
            ReplaceError::BadEvent(msg) => write!(f, "bad_event: {msg}"),
            ReplaceError::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl std::error::Error for ReplaceError {}

// ------------------------------------------------------------------ model

/// One tree element carrying identity, metadata (preserved on target across replacement, `REQ-001`)
/// and replaceable content (swapped by value/digest reference).
#[derive(Debug, Clone, PartialEq)]
pub struct TreeElement {
    pub id: EntityId,
    /// Registered element type (must match between target and replacement, `REQ-002`).
    pub kind: String,
    /// Preserved on target (`REQ-001`).
    pub name: String,
    /// Preserved on target (`REQ-001`).
    pub group: String,
    /// Preserved on target (`REQ-001`).
    pub tags: Vec<String>,
    /// Preserved on target (`REQ-001`).
    pub metadata: BTreeMap<String, CValue>,
    /// Current computation state (`current`, `outdated`, `failed`, `not_computed`).
    pub state: ElementState,
    /// Replaceable content (or digest reference for large blobs).
    pub content: CValue,
}

impl TreeElement {
    /// Canonical SHA-256 digest of `self.content`.
    pub fn content_digest(&self) -> Result<Digest, ReplaceError> {
        let bytes = to_canonical_bytes(&self.content).map_err(|e| ReplaceError::Internal(e.to_string()))?;
        Ok(digest_bytes(&bytes))
    }

    /// Canonical entity body exposed to `OverlayView` / handlers.
    pub fn to_overlay_body(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("content".to_owned(), self.content.clone());
        m.insert("group".to_owned(), text(&self.group));
        m.insert("id".to_owned(), text(&self.id.to_string()));
        m.insert("kind".to_owned(), text(&self.kind));
        m.insert("metadata".to_owned(), CValue::Obj(self.metadata.clone()));
        m.insert("name".to_owned(), text(&self.name));
        m.insert("tags".to_owned(), CValue::Arr(self.tags.iter().map(|t| text(t)).collect()));
        if let CValue::Obj(co) = &self.content {
            if let Some(v) = co.get("value") {
                m.insert("value".to_owned(), v.clone());
            }
        }
        CValue::Obj(m)
    }
}

/// Parameters of `project.replace_input@1`: `{ target: EntityRef, replacement: EntityRef }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceInputParams {
    pub target: EntityId,
    pub replacement: EntityId,
}

impl ReplaceInputParams {
    pub fn from_value(v: &CValue) -> Result<Self, ReplaceError> {
        let CValue::Obj(o) = v else { return Err(ReplaceError::Validation("params must be an object")) };
        if o.len() != 2 || !o.contains_key("target") || !o.contains_key("replacement") {
            return Err(ReplaceError::Validation("params must contain exactly target and replacement"));
        }
        let Some(CValue::Str(t_s)) = o.get("target") else {
            return Err(ReplaceError::Validation("target must be a string"));
        };
        let Some(CValue::Str(r_s)) = o.get("replacement") else {
            return Err(ReplaceError::Validation("replacement must be a string"));
        };
        let target = Uuid::parse(t_s).map_err(|_| ReplaceError::Validation("invalid target uuid"))?;
        let replacement = Uuid::parse(r_s).map_err(|_| ReplaceError::Validation("invalid replacement uuid"))?;
        if target == replacement {
            return Err(ReplaceError::Validation("target and replacement must be distinct elements"));
        }
        Ok(Self { target, replacement })
    }

    pub fn to_value(&self) -> CValue {
        obj([("replacement", text(&self.replacement.to_string())), ("target", text(&self.target.to_string()))])
    }
}

/// Event body of `InputReplaced` (`Design`: `{ target, old_content_digest, new_content_digest, replacement_removed }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputReplaced {
    pub target: EntityId,
    pub old_content_digest: Digest,
    pub new_content_digest: Digest,
    pub replacement_removed: EntityId,
}

impl InputReplaced {
    pub fn to_value(&self) -> CValue {
        obj([
            ("new_content_digest", text(&self.new_content_digest.to_string())),
            ("old_content_digest", text(&self.old_content_digest.to_string())),
            ("replacement_removed", text(&self.replacement_removed.to_string())),
            ("target", text(&self.target.to_string())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, ReplaceError> {
        let CValue::Obj(o) = v else { return Err(ReplaceError::BadEvent("InputReplaced not object")) };
        if o.len() != 4 {
            return Err(ReplaceError::BadEvent("InputReplaced must have 4 keys"));
        }
        let target = req_uuid(o, "target")?;
        let replacement_removed = req_uuid(o, "replacement_removed")?;
        let old_content_digest = req_digest(o, "old_content_digest")?;
        let new_content_digest = req_digest(o, "new_content_digest")?;
        Ok(Self { target, old_content_digest, new_content_digest, replacement_removed })
    }
}

/// Event body of `ElementRemoved` (`REQ-CORE.DEP.REPL-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementRemoved {
    pub element: EntityId,
}

impl ElementRemoved {
    pub fn to_value(&self) -> CValue {
        obj([("element", text(&self.element.to_string()))])
    }

    pub fn from_value(v: &CValue) -> Result<Self, ReplaceError> {
        let CValue::Obj(o) = v else { return Err(ReplaceError::BadEvent("ElementRemoved not object")) };
        if o.len() != 1 {
            return Err(ReplaceError::BadEvent("ElementRemoved must have 1 key"));
        }
        Ok(Self { element: req_uuid(o, "element")? })
    }
}

/// Outcome of one `project.replace_input@1` execution, ready to commit as ONE journal entry
/// plus an optional `auto`-mode [`RecomputeSubmission`] (`REQ-001`..`REQ-007`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaceOutcome {
    pub input_replaced: InputReplaced,
    pub element_removed: ElementRemoved,
    pub elements_marked: Option<ElementsMarked>,
    pub submission: Option<RecomputeSubmission>,
    /// All events emitted in this single commit (`InputReplaced`, `ElementRemoved`, and
    /// optionally `dep.elements_marked@1`).
    pub events: Vec<OverlayEvent>,
}

impl ReplaceOutcome {
    /// Command result JSON matching `result_schema`: `{ target, content_digest, marked }`.
    pub fn result_value(&self) -> CValue {
        let marked: Vec<CValue> = self
            .elements_marked
            .as_ref()
            .map(|m| m.elements().iter().map(|id| text(&id.to_string())).collect())
            .unwrap_or_default();
        obj([
            ("content_digest", text(&self.input_replaced.new_content_digest.to_string())),
            ("marked", CValue::Arr(marked)),
            ("target", text(&self.input_replaced.target.to_string())),
        ])
    }
}

/// Validates and executes `project.replace_input@1` atomically on `elements` and `graph`
/// (`REQ-001`..`REQ-007`).
///
/// On any refusal (`NotFound`, `TypeMismatch`, `ReplacementInUse`, `Validation`, `Propagation`),
/// neither `elements` nor `graph` is modified and nothing is emitted.
pub fn replace_input(
    elements: &mut BTreeMap<EntityId, TreeElement>,
    graph: &mut EdgeIndex,
    mode: RecomputeMode,
    params: ReplaceInputParams,
) -> Result<ReplaceOutcome, ReplaceError> {
    if params.target == params.replacement {
        return Err(ReplaceError::Validation("target and replacement must be distinct elements"));
    }

    let Some(target_el) = elements.get(&params.target) else {
        return Err(ReplaceError::NotFound(params.target));
    };
    let Some(repl_el) = elements.get(&params.replacement) else {
        return Err(ReplaceError::NotFound(params.replacement));
    };
    if !graph.contains(&params.target) {
        return Err(ReplaceError::NotFound(params.target));
    }
    if !graph.contains(&params.replacement) {
        return Err(ReplaceError::NotFound(params.replacement));
    }

    // REQ-002: exact registered type match required.
    if target_el.kind != repl_el.kind {
        return Err(ReplaceError::TypeMismatch {
            target: params.target,
            target_kind: target_el.kind.clone(),
            replacement: params.replacement,
            replacement_kind: repl_el.kind.clone(),
        });
    }

    // REQ-007: replacement must have no dependents of its own.
    if let Some(deps) = graph.dependents_of(&params.replacement) {
        if !deps.is_empty() {
            return Err(ReplaceError::ReplacementInUse {
                replacement: params.replacement,
                dependents: deps.iter().copied().collect(),
            });
        }
    }

    let old_digest = target_el.content_digest()?;
    let new_digest = repl_el.content_digest()?;
    let new_content = repl_el.content.clone();

    // Plan & apply propagation on a scratch ElementStates before mutating `elements` or `graph`
    // so failure leaves everything untouched.
    let mut states = ElementStates::new();
    for (&id, el) in elements.iter() {
        states.insert(id, el.state);
    }
    let prop_res = propagate(graph, &mut states, mode, &[params.target], &[]).map_err(ReplaceError::Propagation)?;

    // All checks succeeded -> apply atomically!
    // REQ-001: keep target's ID, name, group, tags, metadata; replace only its content.
    if let Some(t) = elements.get_mut(&params.target) {
        t.content = new_content;
        t.state = ElementState::Current;
    }

    // REQ-003: remove replacement from tree and dependency graph in the same commit.
    let removed_ok = graph
        .remove_leaf(&params.replacement)
        .map_err(|e| ReplaceError::Internal(format!("remove_leaf failed: {e}")))?;
    if !removed_ok {
        return Err(ReplaceError::ReplacementInUse { replacement: params.replacement, dependents: Vec::new() });
    }
    elements.remove(&params.replacement);

    // REQ-004: mark every transitive dependent outdated in the same commit.
    if let Some(marked) = &prop_res.event {
        for id in marked.elements() {
            if let Some(el) = elements.get_mut(id) {
                el.state = ElementState::Outdated;
            }
        }
    }

    let input_replaced = InputReplaced {
        target: params.target,
        old_content_digest: old_digest,
        new_content_digest: new_digest,
        replacement_removed: params.replacement,
    };
    let element_removed = ElementRemoved { element: params.replacement };

    let mut events = vec![
        OverlayEvent { event_type: INPUT_REPLACED_EVENT.to_owned(), body: input_replaced.to_value() },
        OverlayEvent { event_type: ELEMENT_REMOVED_EVENT.to_owned(), body: element_removed.to_value() },
    ];
    if let Some(marked) = &prop_res.event {
        events
            .push(OverlayEvent { event_type: crate::prop::ELEMENTS_MARKED_EVENT.to_owned(), body: marked.to_value() });
    }

    Ok(ReplaceOutcome {
        input_replaced,
        element_removed,
        elements_marked: prop_res.event,
        submission: prop_res.submission,
        events,
    })
}

/// Builds a read-only [`OverlayView`] from the current tree elements (ready to hand to
/// `recompute::execute_recompute` for `auto`-mode recompute, `REQ-005`).
pub fn overlay_from_elements(
    head: Digest,
    elements: &BTreeMap<EntityId, TreeElement>,
) -> Result<OverlayView, ReplaceError> {
    let mut base = BTreeMap::new();
    for (&id, el) in elements {
        let ent = crate::recompute::OverlayEntity::new(el.kind.clone(), el.to_overlay_body(), el.state)
            .map_err(|e| ReplaceError::Internal(e.to_string()))?;
        base.insert(id, ent);
    }
    Ok(OverlayView::new(head, base))
}

/// Applies a committed [`crate::recompute::RecomputeExecution`] back onto `elements`
/// (updating computed values & states while preserving metadata).
pub fn apply_recompute_to_elements(
    elements: &mut BTreeMap<EntityId, TreeElement>,
    exec: &crate::recompute::RecomputeExecution,
) {
    let merged = exec.overlay.clone().into_merged();
    for (id, ent) in merged {
        if let Some(el) = elements.get_mut(&id) {
            el.state = ent.state;
            if let CValue::Obj(o) = &ent.body {
                if let Some(v) = o.get("value") {
                    el.content = obj([("value", v.clone())]);
                } else if let Some(c) = o.get("content") {
                    el.content = c.clone();
                }
            }
        }
    }
}

/// Registry [`CommandDef`] for `project.replace_input@1` (W2, D0, undo `head_move`,
/// CLI `fid project replace-input`, MCP `project_replace_input`).
pub fn command_def() -> Result<CommandDef, DefError> {
    let uuid_schema =
        |desc: &str| obj([("description", text(desc)), ("pattern", text(UUID_PATTERN)), ("type", text("string"))]);
    let digest_schema =
        |desc: &str| obj([("description", text(desc)), ("pattern", text(DIGEST_PATTERN)), ("type", text("string"))]);

    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("replacement", uuid_schema("Staged replacement element UUID (consumed on commit).")),
                ("target", uuid_schema("Existing input element UUID whose content is replaced.")),
            ]),
        ),
        ("required", strs(&["replacement", "target"])),
        ("type", text("object")),
    ]);

    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("content_digest", digest_schema("SHA-256 digest of the target's new content.")),
                (
                    "marked",
                    obj([
                        ("description", text("Transitive dependent UUIDs marked outdated.")),
                        ("items", uuid_schema("Dependent element UUID.")),
                        ("type", text("array")),
                    ]),
                ),
                ("target", uuid_schema("Target element UUID (unchanged).")),
            ]),
        ),
        ("required", strs(&["content_digest", "marked", "target"])),
        ("type", text("object")),
    ]);

    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text(
                "Swaps the content of an input element for a staged replacement of the same type, preserving the \
                 target's identity and metadata, removing the replacement, and marking dependents outdated.",
            ),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[INPUT_REPLACED_EVENT, ELEMENT_REMOVED_EVENT, crate::prop::ELEMENTS_MARKED_EVENT])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text(REPLACE_INPUT_NAME)),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Replace input")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

// ------------------------------------------------------------------ journal commit (bus feature)

#[cfg(feature = "bus")]
pub mod bus {
    use super::{ReplaceError, ReplaceInputParams, ReplaceOutcome, REPLACE_INPUT_NAME};
    use crate::recompute::OverlayEvent;
    use fid_bus::principal::Principal;
    use fid_canon::{CValue, Digest};
    use fid_journal::append::{AppendRequest, JournalRef};
    use fid_journal::entry::{AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OutputDigest};
    use fid_types::{Determinism, SideEffect};

    /// Appends exactly one `project.replace_input@1` entry onto `journal` (`AC-2`).
    #[allow(clippy::too_many_arguments)]
    pub fn commit_replace_entry(
        journal: &JournalRef<'_>,
        branch: &str,
        principal: &Principal,
        time: EntryTime,
        manifests: Manifests,
        params: ReplaceInputParams,
        outcome: &ReplaceOutcome,
        expected_prev: Option<Digest>,
    ) -> Result<Digest, ReplaceError> {
        let event_envelopes: Vec<CValue> = outcome.events.iter().map(OverlayEvent::to_envelope).collect();
        let draft = EntryDraft {
            stream: "record".to_owned(),
            branch: branch.to_owned(),
            logical_parent: None,
            time,
            principal: principal.to_draft(),
            ai_assistance: AiAssistance::None,
            manifests,
            reason: None,
            human_assertion: None,
            agent_coedited: false,
            body: DraftBody::Command(CommandDraft {
                id: REPLACE_INPUT_NAME.to_owned(),
                major: 1,
                class: SideEffect::W2,
                determinism: Determinism::D0,
                seed: 0,
                args: params.to_value(),
                events: event_envelopes,
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest {
                    entity: Some(outcome.input_replaced.target.to_string()),
                    digest: outcome.input_replaced.new_content_digest,
                }],
                algorithms: Vec::new(),
                event_sources: Vec::new(),
            }),
        };
        let mut req = AppendRequest::new(draft);
        req.expected_prev = expected_prev;
        journal.append(req).map_err(|e| ReplaceError::Internal(e.to_string()))
    }
}

// ------------------------------------------------------------------ helpers

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn req_uuid(o: &BTreeMap<String, CValue>, key: &str) -> Result<EntityId, ReplaceError> {
    let Some(CValue::Str(s)) = o.get(key) else {
        return Err(ReplaceError::BadEvent("missing uuid field"));
    };
    Uuid::parse(s).map_err(|_| ReplaceError::BadEvent("invalid uuid"))
}

fn req_digest(o: &BTreeMap<String, CValue>, key: &str) -> Result<Digest, ReplaceError> {
    let Some(CValue::Str(s)) = o.get(key) else {
        return Err(ReplaceError::BadEvent("missing digest field"));
    };
    let Some(hex) = s.strip_prefix("sha256:") else {
        return Err(ReplaceError::BadEvent("digest missing sha256: prefix"));
    };
    if hex.len() != 64 {
        return Err(ReplaceError::BadEvent("digest wrong length"));
    }
    let mut bytes = [0u8; 32];
    for (i, slot) in bytes.iter_mut().enumerate() {
        let hi = hex_nibble(hex.as_bytes()[2 * i]).ok_or(ReplaceError::BadEvent("bad hex"))?;
        let lo = hex_nibble(hex.as_bytes()[2 * i + 1]).ok_or(ReplaceError::BadEvent("bad hex"))?;
        *slot = (hi << 4) | lo;
    }
    Ok(Digest(bytes))
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}
