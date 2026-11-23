//! Deterministic parametric recompute with failure isolation and single-entry commit.
//! Spec node `CORE.DEP.RECOMP`.
//!
//! Takes a scope (`elements`, `all_outdated`, or `subtree`), orders it topologically with
//! Kahn's algorithm and a [`BTreeSet`] ready queue (ascending stable entity ID tie-break,
//! REQ-001), executes each element's recorded command against a scratch overlay layered over
//! the head state, isolates failures so transitive dependents stay `outdated` without running
//! (REQ-003), skips elements that are neither `outdated` nor `failed` (REQ-008), reports
//! monotonic progress and checks cancellation between elements (REQ-004, REQ-007), and emits
//! one deterministic `RecomputeCommitted` event for single-entry commit with `expected_head`
//! compare-and-swap (REQ-002, REQ-006, REQ-009).

#[cfg(feature = "bus")]
pub mod bus;

use crate::edge::EntityId;
use crate::prop::{DepGraph, ElementState, ElementStates, MAX_PASS_EDGES, MAX_PASS_ELEMENTS, RECOMPUTE_DEPENDENTS};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue, Digest};
use fid_registry::{CommandDef, DefError};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// Command name without `@major`.
pub const RECOMPUTE_COMMAND_NAME: &str = "project.recompute_dependents";
/// Event type emitted once per committed recompute (`REQ-CORE.DEP.RECOMP-002`).
pub const RECOMPUTE_COMMITTED_EVENT: &str = "RecomputeCommitted";
/// Event type recording an element's state transition during a recompute commit.
pub const ELEMENT_STATE_CHANGED_EVENT: &str = "ElementStateChanged";
const LIFECYCLE_SINCE: &str = "2026-11-23";
const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";

// ------------------------------------------------------------------ errors

/// Typed failures of `CORE.DEP.RECOMP`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecompError {
    /// Invalid or empty scope parameters (`validation`).
    Validation(&'static str),
    /// A scope or overlay referenced an element absent from the dependency graph (`validation`).
    UnknownElement(EntityId),
    /// The induced subgraph contains a cycle (`internal`).
    Cycle,
    /// Recompute was cancelled between elements; nothing was committed (`cancelled`).
    Cancelled,
    /// Branch head moved before the recompute entry committed (`conflict`).
    Conflict { expected: Digest, actual: Digest },
    /// Exceeded a graph traversal or scope size limit (`validation`).
    InputLimit(&'static str),
    /// Malformed `RecomputeCommitted` event during replay (`internal`).
    BadEvent(&'static str),
    /// Storage or bus integration error (`internal`).
    Internal(String),
}

impl RecompError {
    /// Stable error category code per spec Design (`conflict`, `cancelled`, `validation`, `internal`).
    pub fn code(&self) -> &'static str {
        match self {
            RecompError::Validation(_) | RecompError::UnknownElement(_) | RecompError::InputLimit(_) => "validation",
            RecompError::Cancelled => "cancelled",
            RecompError::Conflict { .. } => "conflict",
            RecompError::Cycle | RecompError::BadEvent(_) | RecompError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for RecompError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecompError::Validation(msg) => write!(f, "validation: {msg}"),
            RecompError::UnknownElement(id) => write!(f, "unknown element {id}"),
            RecompError::Cycle => write!(f, "dependency graph contains a cycle"),
            RecompError::Cancelled => write!(f, "recompute cancelled; partial results discarded"),
            RecompError::Conflict { expected, actual } => {
                write!(f, "conflict: branch head moved (expected {expected}, actual {actual})")
            }
            RecompError::InputLimit(what) => write!(f, "input limit exceeded: {what}"),
            RecompError::BadEvent(msg) => write!(f, "malformed recompute event: {msg}"),
            RecompError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for RecompError {}

// ------------------------------------------------------------------ scope & plan

/// Scope parameter of `project.recompute_dependents@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecomputeScope {
    /// Explicit list of elements to recompute.
    Elements(Vec<EntityId>),
    /// All elements in the graph whose current state is `outdated` or `failed`.
    AllOutdated,
    /// A root element and all its transitive dependents.
    Subtree(EntityId),
}

impl RecomputeScope {
    /// Parses `{ scope: "all_outdated" | { elements: [...] } | { subtree: "<id>" } }`.
    pub fn from_params(v: &CValue) -> Result<Self, RecompError> {
        let CValue::Obj(o) = v else { return Err(RecompError::Validation("params must be an object")) };
        if o.keys().any(|k| k != "scope") {
            return Err(RecompError::Validation("unknown parameter"));
        }
        let Some(scope) = o.get("scope") else {
            return Err(RecompError::Validation("missing scope"));
        };
        Self::from_scope_value(scope)
    }

    /// Parses the `scope` field value directly.
    pub fn from_scope_value(v: &CValue) -> Result<Self, RecompError> {
        match v {
            CValue::Str(s) if s == "all_outdated" => Ok(RecomputeScope::AllOutdated),
            CValue::Str(_) => Err(RecompError::Validation("unknown scope string")),
            CValue::Obj(o) => {
                if o.len() != 1 {
                    return Err(RecompError::Validation("scope object must have exactly one key"));
                }
                if let Some(CValue::Arr(arr)) = o.get("elements") {
                    if arr.is_empty() {
                        return Err(RecompError::Validation("elements scope must not be empty"));
                    }
                    if arr.len() > MAX_PASS_ELEMENTS {
                        return Err(RecompError::InputLimit("scope.elements"));
                    }
                    let mut ids = Vec::with_capacity(arr.len());
                    for item in arr {
                        let CValue::Str(s) = item else {
                            return Err(RecompError::Validation("element id must be a string"));
                        };
                        let id = Uuid::parse(s).map_err(|_| RecompError::Validation("invalid element uuid"))?;
                        ids.push(id);
                    }
                    Ok(RecomputeScope::Elements(ids))
                } else if let Some(CValue::Str(s)) = o.get("subtree") {
                    let id = Uuid::parse(s).map_err(|_| RecompError::Validation("invalid subtree uuid"))?;
                    Ok(RecomputeScope::Subtree(id))
                } else {
                    Err(RecompError::Validation("invalid scope object"))
                }
            }
            _ => Err(RecompError::Validation("scope must be 'all_outdated' or an object")),
        }
    }

    /// Canonical parameter object `{"scope": ...}`.
    pub fn to_params(&self) -> CValue {
        let scope_val = match self {
            RecomputeScope::AllOutdated => text("all_outdated"),
            RecomputeScope::Elements(ids) => {
                obj([("elements", CValue::Arr(ids.iter().map(|id| text(&id.to_string())).collect()))])
            }
            RecomputeScope::Subtree(root) => obj([("subtree", text(&root.to_string()))]),
        };
        obj([("scope", scope_val)])
    }
}

/// Deterministic topological processing order for a recompute pass (`REQ-001`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecomputePlan {
    /// Elements in topological order, ties broken by ascending stable entity ID.
    pub order: Vec<EntityId>,
}

/// Collects the scope and orders its induced subgraph with Kahn's algorithm using a
/// [`BTreeSet`] ready queue (ascending stable entity ID tie-break, `REQ-CORE.DEP.RECOMP-001`).
///
/// Runs in O((V + E) log V) using dense integer indices for nodes inside the scope while
/// ordering ready elements strictly by [`EntityId`].
pub fn plan_recompute<G: DepGraph + ?Sized>(
    graph: &G,
    states: &ElementStates,
    scope: &RecomputeScope,
) -> Result<RecomputePlan, RecompError> {
    let mut scope_set: BTreeSet<EntityId> = BTreeSet::new();
    match scope {
        RecomputeScope::AllOutdated => {
            for id in graph.elements_ascending() {
                if matches!(states.get(&id), Some(ElementState::Outdated | ElementState::Failed)) {
                    if scope_set.len() >= MAX_PASS_ELEMENTS {
                        return Err(RecompError::InputLimit("scope"));
                    }
                    scope_set.insert(id);
                }
            }
        }
        RecomputeScope::Elements(ids) => {
            if ids.is_empty() {
                return Err(RecompError::Validation("empty elements scope"));
            }
            for &id in ids {
                if !graph.contains(&id) {
                    return Err(RecompError::UnknownElement(id));
                }
                if scope_set.len() >= MAX_PASS_ELEMENTS && !scope_set.contains(&id) {
                    return Err(RecompError::InputLimit("scope"));
                }
                scope_set.insert(id);
            }
        }
        RecomputeScope::Subtree(root) => {
            if !graph.contains(root) {
                return Err(RecompError::UnknownElement(*root));
            }
            let mut queue = VecDeque::new();
            scope_set.insert(*root);
            queue.push_back(*root);
            let mut edges_seen: usize = 0;
            while let Some(u) = queue.pop_front() {
                if let Some(deps) = graph.dependents_of(&u) {
                    for &v in deps {
                        edges_seen = edges_seen.saturating_add(1);
                        if edges_seen > MAX_PASS_EDGES {
                            return Err(RecompError::InputLimit("edges"));
                        }
                        if scope_set.insert(v) {
                            if scope_set.len() > MAX_PASS_ELEMENTS {
                                return Err(RecompError::InputLimit("scope"));
                            }
                            queue.push_back(v);
                        }
                    }
                }
            }
        }
    }

    order_induced_subgraph(graph, &scope_set)
}

/// Orders `scope_set` topologically with a `BTreeSet<EntityId>` ready queue (`REQ-001`).
pub fn order_induced_subgraph<G: DepGraph + ?Sized>(
    graph: &G,
    scope_set: &BTreeSet<EntityId>,
) -> Result<RecomputePlan, RecompError> {
    if scope_set.is_empty() {
        return Ok(RecomputePlan { order: Vec::new() });
    }
    let nodes: Vec<EntityId> = scope_set.iter().copied().collect();
    let n = nodes.len();
    let mut indeg: Vec<usize> = vec![0; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut edge_count: usize = 0;

    for (u_idx, u_id) in nodes.iter().enumerate() {
        if let Some(deps) = graph.dependents_of(u_id) {
            for v_id in deps {
                if let Ok(v_idx) = nodes.binary_search(v_id) {
                    edge_count = edge_count.saturating_add(1);
                    if edge_count > MAX_PASS_EDGES {
                        return Err(RecompError::InputLimit("edges"));
                    }
                    adj[u_idx].push(v_idx);
                    indeg[v_idx] += 1;
                }
            }
        }
    }

    // `nodes` is strictly sorted by `EntityId`, so `usize` index order == ascending `EntityId` order!
    let mut ready: BTreeSet<usize> = BTreeSet::new();
    for (idx, &deg) in indeg.iter().enumerate() {
        if deg == 0 {
            ready.insert(idx);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(u_idx) = ready.pop_first() {
        order.push(nodes[u_idx]);
        for &v_idx in &adj[u_idx] {
            indeg[v_idx] -= 1;
            if indeg[v_idx] == 0 {
                ready.insert(v_idx);
            }
        }
    }

    if order.len() != n {
        return Err(RecompError::Cycle);
    }
    Ok(RecomputePlan { order })
}

/// Computes topological levels (each level is sorted by ascending `EntityId`) for testing
/// `REQ-CORE.DEP.RECOMP-006` (parallel-level equivalence with sequential runs).
pub fn topological_levels<G: DepGraph + ?Sized>(
    graph: &G,
    scope_set: &BTreeSet<EntityId>,
) -> Result<Vec<Vec<EntityId>>, RecompError> {
    if scope_set.is_empty() {
        return Ok(Vec::new());
    }
    let nodes: Vec<EntityId> = scope_set.iter().copied().collect();
    let n = nodes.len();
    let mut indeg: Vec<usize> = vec![0; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u_idx, u_id) in nodes.iter().enumerate() {
        if let Some(deps) = graph.dependents_of(u_id) {
            for v_id in deps {
                if let Ok(v_idx) = nodes.binary_search(v_id) {
                    adj[u_idx].push(v_idx);
                    indeg[v_idx] += 1;
                }
            }
        }
    }
    let mut cur: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut levels = Vec::new();
    let mut seen = 0usize;
    while !cur.is_empty() {
        levels.push(cur.iter().map(|&i| nodes[i]).collect());
        seen += cur.len();
        let mut next = BTreeSet::new();
        for u_idx in cur {
            for &v_idx in &adj[u_idx] {
                indeg[v_idx] -= 1;
                if indeg[v_idx] == 0 {
                    next.insert(v_idx);
                }
            }
        }
        cur = next.into_iter().collect();
    }
    if seen != n {
        return Err(RecompError::Cycle);
    }
    Ok(levels)
}

// ------------------------------------------------------------------ scratch overlay & events

/// An element's recorded command and canonical arguments used to recompute it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedCommand {
    /// Registered command ID with major version, e.g. `krn.fit_plane@1`.
    pub command: String,
    /// Canonical JSON arguments recorded when the element was defined/edited.
    pub args: CValue,
}

/// One event produced by an element handler during recompute.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayEvent {
    pub event_type: String,
    pub body: CValue,
}

impl OverlayEvent {
    pub fn to_envelope(&self) -> CValue {
        obj([("body", self.body.clone()), ("type", text(&self.event_type))])
    }
}

/// One entity record held in the head state or scratch overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayEntity {
    pub kind: String,
    pub body: CValue,
    pub state: ElementState,
    pub digest: Digest,
}

impl OverlayEntity {
    /// Builds an entity record, computing its canonical body digest.
    pub fn new(kind: impl Into<String>, body: CValue, state: ElementState) -> Result<Self, RecompError> {
        let bytes = to_canonical_bytes(&body).map_err(|e| RecompError::Internal(e.to_string()))?;
        let digest = digest_bytes(&bytes);
        Ok(Self { kind: kind.into(), body, state, digest })
    }
}

/// Scratch overlay state view layering recomputed outputs over the committed head state.
/// Never mutates the head snapshot; if cancelled or conflicted, dropping the overlay discards
/// all partial work (`REQ-CORE.DEP.RECOMP-004`).
#[derive(Debug, Clone)]
pub struct OverlayView {
    head: Digest,
    base: BTreeMap<EntityId, OverlayEntity>,
    staged: BTreeMap<EntityId, OverlayEntity>,
    settings: BTreeMap<String, CValue>,
}

impl OverlayView {
    pub fn new(head: Digest, base: BTreeMap<EntityId, OverlayEntity>) -> Self {
        Self { head, base, staged: BTreeMap::new(), settings: BTreeMap::new() }
    }

    pub fn with_settings(mut self, settings: BTreeMap<String, CValue>) -> Self {
        self.settings = settings;
        self
    }

    pub fn head(&self) -> Digest {
        self.head
    }

    /// Reads an entity from the overlay first, falling back to the base head state.
    pub fn get(&self, id: &EntityId) -> Option<&OverlayEntity> {
        self.staged.get(id).or_else(|| self.base.get(id))
    }

    pub fn state_of(&self, id: &EntityId) -> Option<ElementState> {
        self.get(id).map(|e| e.state)
    }

    pub fn setting(&self, key: &str) -> Option<&CValue> {
        self.settings.get(key)
    }

    /// Stages a recomputed element output into the scratch overlay (`ElementState::Current`).
    pub fn stage_recomputed(&mut self, id: EntityId, body: CValue, digest: Digest) -> Result<(), RecompError> {
        let kind = self.get(&id).ok_or(RecompError::UnknownElement(id))?.kind.clone();
        self.staged.insert(id, OverlayEntity { kind, body, state: ElementState::Current, digest });
        Ok(())
    }

    /// Stages an element failure (`ElementState::Failed`) in the scratch overlay.
    pub fn stage_failed(&mut self, id: EntityId) -> Result<(), RecompError> {
        let prev = self.get(&id).ok_or(RecompError::UnknownElement(id))?.clone();
        self.staged.insert(id, OverlayEntity { state: ElementState::Failed, ..prev });
        Ok(())
    }

    /// Snapshot of all current element states (base + staged).
    pub fn element_states(&self) -> ElementStates {
        let mut out = ElementStates::new();
        for (&id, e) in &self.base {
            out.insert(id, e.state);
        }
        for (&id, e) in &self.staged {
            out.insert(id, e.state);
        }
        out
    }

    /// Consumes the overlay and yields the merged final entities (only called after commit succeeds).
    pub fn into_merged(mut self) -> BTreeMap<EntityId, OverlayEntity> {
        for (id, e) in self.staged {
            self.base.insert(id, e);
        }
        self.base
    }
}

// ------------------------------------------------------------------ outcomes & committed event

/// Per-element outcome of one recompute pass.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementOutcome {
    Recomputed { digest: Digest, events: Vec<OverlayEvent> },
    Failed { code: String },
    Blocked,
    Skipped,
}

/// One successfully recomputed element entry in `RecomputeCommitted` and command result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecomputedEntry {
    pub id: EntityId,
    pub digest: Digest,
}

impl RecomputedEntry {
    pub fn to_value(&self) -> CValue {
        obj([("digest", text(&self.digest.to_string())), ("id", text(&self.id.to_string()))])
    }

    pub fn from_value(v: &CValue) -> Result<Self, RecompError> {
        let CValue::Obj(o) = v else { return Err(RecompError::BadEvent("recomputed entry not object")) };
        if o.len() != 2 {
            return Err(RecompError::BadEvent("recomputed entry must have id and digest"));
        }
        let Some(CValue::Str(id_s)) = o.get("id") else {
            return Err(RecompError::BadEvent("missing recomputed id"));
        };
        let Some(CValue::Str(dig_s)) = o.get("digest") else {
            return Err(RecompError::BadEvent("missing recomputed digest"));
        };
        let id = Uuid::parse(id_s).map_err(|_| RecompError::BadEvent("bad recomputed id"))?;
        let digest = parse_digest_str(dig_s).ok_or(RecompError::BadEvent("bad recomputed digest"))?;
        Ok(Self { id, digest })
    }
}

/// One failed element entry in `RecomputeCommitted` and command result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedEntry {
    pub id: EntityId,
    pub code: String,
}

impl FailedEntry {
    pub fn to_value(&self) -> CValue {
        obj([("code", text(&self.code)), ("id", text(&self.id.to_string()))])
    }

    pub fn from_value(v: &CValue) -> Result<Self, RecompError> {
        let CValue::Obj(o) = v else { return Err(RecompError::BadEvent("failed entry not object")) };
        if o.len() != 2 {
            return Err(RecompError::BadEvent("failed entry must have id and code"));
        }
        let Some(CValue::Str(id_s)) = o.get("id") else {
            return Err(RecompError::BadEvent("missing failed id"));
        };
        let Some(CValue::Str(code)) = o.get("code") else {
            return Err(RecompError::BadEvent("missing failed code"));
        };
        if code.is_empty() {
            return Err(RecompError::BadEvent("empty error code"));
        }
        let id = Uuid::parse(id_s).map_err(|_| RecompError::BadEvent("bad failed id"))?;
        Ok(Self { id, code: code.clone() })
    }
}

/// The `RecomputeCommitted` event body (`REQ-002`, `REQ-006`, `REQ-008`).
/// Lists every recomputed element with its new output digest, every failed element with its
/// error code, and every skipped element, each in deterministic processing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecomputeCommitted {
    pub recomputed: Vec<RecomputedEntry>,
    pub failed: Vec<FailedEntry>,
    pub skipped: Vec<EntityId>,
}

impl RecomputeCommitted {
    pub fn to_value(&self) -> CValue {
        obj([
            ("failed", CValue::Arr(self.failed.iter().map(FailedEntry::to_value).collect())),
            ("recomputed", CValue::Arr(self.recomputed.iter().map(RecomputedEntry::to_value).collect())),
            ("skipped", CValue::Arr(self.skipped.iter().map(|id| text(&id.to_string())).collect())),
        ])
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RecompError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| RecompError::Internal(e.to_string()))
    }

    pub fn from_value(v: &CValue) -> Result<Self, RecompError> {
        let CValue::Obj(o) = v else { return Err(RecompError::BadEvent("RecomputeCommitted not object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "recomputed" | "failed" | "skipped")) {
            return Err(RecompError::BadEvent("unknown key in RecomputeCommitted"));
        }
        let Some(CValue::Arr(rec_arr)) = o.get("recomputed") else {
            return Err(RecompError::BadEvent("missing recomputed"));
        };
        let Some(CValue::Arr(fail_arr)) = o.get("failed") else {
            return Err(RecompError::BadEvent("missing failed"));
        };
        let Some(CValue::Arr(skip_arr)) = o.get("skipped") else {
            return Err(RecompError::BadEvent("missing skipped"));
        };
        let mut recomputed = Vec::with_capacity(rec_arr.len());
        for item in rec_arr {
            recomputed.push(RecomputedEntry::from_value(item)?);
        }
        let mut failed = Vec::with_capacity(fail_arr.len());
        for item in fail_arr {
            failed.push(FailedEntry::from_value(item)?);
        }
        let mut skipped = Vec::with_capacity(skip_arr.len());
        for item in skip_arr {
            let CValue::Str(s) = item else {
                return Err(RecompError::BadEvent("skipped id not string"));
            };
            skipped.push(Uuid::parse(s).map_err(|_| RecompError::BadEvent("bad skipped id"))?);
        }
        Ok(Self { recomputed, failed, skipped })
    }
}

/// Output of running one element's handler against the scratch overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementRunOutput {
    pub output_digest: Digest,
    pub events: Vec<OverlayEvent>,
    pub updated_body: CValue,
}

/// Executes a single element's recorded command against the read-only scratch overlay.
/// Production callers implement this via [`bus::BusSystemRunner`] calling
/// `Bus::run_handlers_as_system`.
pub trait RecomputeRunner {
    fn run_element(
        &self,
        id: EntityId,
        recorded: &RecordedCommand,
        overlay: &OverlayView,
    ) -> Result<ElementRunOutput, String>;
}

/// Cooperative cancellation probe checked between elements (`REQ-004`, `REQ-007`).
pub trait CancelCheck {
    fn is_cancelled(&self) -> bool;
}

/// Never-cancelled probe for synchronous callers.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverCancel;

impl CancelCheck for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Monotonic progress reporter (`0.0..=1.0`, `REQ-007`, `AC-5`).
pub trait ProgressReport {
    fn report(&self, fraction: f64, stage: &str);
}

/// No-op progress reporter for callers not wired to a job scheduler.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullProgress;

impl ProgressReport for NullProgress {
    fn report(&self, _fraction: f64, _stage: &str) {}
}

/// Full result of executing a recompute pass (prior to commit).
#[derive(Debug, Clone)]
pub struct RecomputeExecution {
    /// Processing order executed (`REQ-001`).
    pub order: Vec<EntityId>,
    /// The head digest this recompute was started against (`REQ-009`).
    pub expected_head: Digest,
    /// Per-element outcome keyed by entity ID.
    pub outcomes: BTreeMap<EntityId, ElementOutcome>,
    /// The single summary event (`REQ-002`).
    pub committed: RecomputeCommitted,
    /// All state-change and element output events to commit in the single journal entry,
    /// in deterministic processing order (`REQ-002`, `REQ-006`).
    pub events: Vec<OverlayEvent>,
    /// Updated scratch overlay ready to replace the in-memory state once the journal commit succeeds.
    pub overlay: OverlayView,
}

impl RecomputeExecution {
    /// Command result JSON matching `result_schema`: `{ recomputed, failed, skipped }`.
    pub fn result_value(&self) -> CValue {
        self.committed.to_value()
    }

    /// Canonical bytes of the entire event payload (summary + state-change events) for
    /// `REQ-CORE.DEP.RECOMP-006` byte-identity verification.
    pub fn canonical_events_bytes(&self) -> Result<Vec<u8>, RecompError> {
        let arr = CValue::Arr(self.events.iter().map(OverlayEvent::to_envelope).collect());
        to_canonical_bytes(&arr).map_err(|e| RecompError::Internal(e.to_string()))
    }
}

/// Executes a recompute pass sequentially in deterministic Kahn + `BTreeSet` order
/// (`REQ-001`..`REQ-008`). Does not mutate the caller's committed state: on `Err(Cancelled)`,
/// partial results are discarded and nothing is returned to commit.
pub fn execute_recompute<G: DepGraph + ?Sized>(
    graph: &G,
    head_state: &OverlayView,
    commands: &BTreeMap<EntityId, RecordedCommand>,
    scope: &RecomputeScope,
    runner: &dyn RecomputeRunner,
    cancel: &dyn CancelCheck,
    progress: &dyn ProgressReport,
) -> Result<RecomputeExecution, RecompError> {
    let states = head_state.element_states();
    let plan = plan_recompute(graph, &states, scope)?;
    execute_planned_recompute(graph, head_state, commands, plan, runner, cancel, progress)
}

/// Executes an already-planned [`RecomputePlan`].
pub fn execute_planned_recompute<G: DepGraph + ?Sized>(
    graph: &G,
    head_state: &OverlayView,
    commands: &BTreeMap<EntityId, RecordedCommand>,
    plan: RecomputePlan,
    runner: &dyn RecomputeRunner,
    cancel: &dyn CancelCheck,
    progress: &dyn ProgressReport,
) -> Result<RecomputeExecution, RecompError> {
    let expected_head = head_state.head();
    let mut overlay = head_state.clone();
    let mut outcomes: BTreeMap<EntityId, ElementOutcome> = BTreeMap::new();
    let mut recomputed: Vec<RecomputedEntry> = Vec::new();
    let mut failed: Vec<FailedEntry> = Vec::new();
    let mut skipped: Vec<EntityId> = Vec::new();
    let mut failed_or_blocked: BTreeSet<EntityId> = BTreeSet::new();
    let mut detail_events: Vec<OverlayEvent> = Vec::new();

    let total = plan.order.len();
    if total == 0 {
        progress.report(1.0, "recompute complete");
    }

    for (idx, &id) in plan.order.iter().enumerate() {
        // REQ-004 & REQ-007: check cancellation between elements; discard everything if cancelled.
        if cancel.is_cancelled() {
            return Err(RecompError::Cancelled);
        }

        let cur_state = overlay.state_of(&id).ok_or(RecompError::UnknownElement(id))?;
        // REQ-008: skip elements that are neither `outdated` nor `failed`.
        if !matches!(cur_state, ElementState::Outdated | ElementState::Failed) {
            skipped.push(id);
            outcomes.insert(id, ElementOutcome::Skipped);
            progress.report(((idx + 1) as f64) / (total as f64), "recomputing");
            continue;
        }

        // REQ-003: if any direct input failed or was blocked, leave this element `outdated`
        // without running its handler.
        let blocked_by_input = graph.inputs_of(&id).is_some_and(|inputs| {
            inputs
                .iter()
                .any(|inp| failed_or_blocked.contains(inp) || overlay.state_of(inp) == Some(ElementState::Failed))
        });
        if blocked_by_input {
            failed_or_blocked.insert(id);
            outcomes.insert(id, ElementOutcome::Blocked);
            progress.report(((idx + 1) as f64) / (total as f64), "recomputing");
            continue;
        }

        let Some(recorded) = commands.get(&id) else {
            let code = "missing_command".to_owned();
            overlay.stage_failed(id)?;
            failed_or_blocked.insert(id);
            failed.push(FailedEntry { id, code: code.clone() });
            outcomes.insert(id, ElementOutcome::Failed { code: code.clone() });
            detail_events.push(state_changed_event(id, cur_state, ElementState::Failed, Some(&code)));
            progress.report(((idx + 1) as f64) / (total as f64), "recomputing");
            continue;
        };

        match runner.run_element(id, recorded, &overlay) {
            Ok(out) => {
                overlay.stage_recomputed(id, out.updated_body, out.output_digest)?;
                recomputed.push(RecomputedEntry { id, digest: out.output_digest });
                for ev in &out.events {
                    detail_events.push(ev.clone());
                }
                detail_events.push(state_changed_event(id, cur_state, ElementState::Current, None));
                outcomes.insert(id, ElementOutcome::Recomputed { digest: out.output_digest, events: out.events });
            }
            Err(code) => {
                let norm_code = if code.is_empty() { "handler_failed".to_owned() } else { code };
                overlay.stage_failed(id)?;
                failed_or_blocked.insert(id);
                failed.push(FailedEntry { id, code: norm_code.clone() });
                outcomes.insert(id, ElementOutcome::Failed { code: norm_code.clone() });
                detail_events.push(state_changed_event(id, cur_state, ElementState::Failed, Some(&norm_code)));
            }
        }

        progress.report(((idx + 1) as f64) / (total as f64), "recomputing");
    }

    // Final cancellation check before publishing outcomes (REQ-004).
    if cancel.is_cancelled() {
        return Err(RecompError::Cancelled);
    }

    let committed = RecomputeCommitted { recomputed, failed, skipped };
    let mut events = Vec::with_capacity(1 + detail_events.len());
    events.push(OverlayEvent { event_type: RECOMPUTE_COMMITTED_EVENT.to_owned(), body: committed.to_value() });
    events.extend(detail_events);

    Ok(RecomputeExecution { order: plan.order, expected_head, outcomes, committed, events, overlay })
}

fn state_changed_event(id: EntityId, from: ElementState, to: ElementState, error_code: Option<&str>) -> OverlayEvent {
    let mut m = BTreeMap::new();
    m.insert("element".to_owned(), text(&id.to_string()));
    m.insert("from".to_owned(), text(from.as_str()));
    m.insert("to".to_owned(), text(to.as_str()));
    if let Some(code) = error_code {
        m.insert("error_code".to_owned(), text(code));
    }
    OverlayEvent { event_type: ELEMENT_STATE_CHANGED_EVENT.to_owned(), body: CValue::Obj(m) }
}

// ------------------------------------------------------------------ exact replay (AC-4)

/// Outcome of replaying a recorded `project.recompute_dependents@1` entry in exact mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayOutcome {
    Identical,
    Diverged { reason: String },
}

/// Replays a recorded `RecomputeCommitted` event against `head_state` by re-running the
/// recorded scope and verifying byte-identical event payloads and element digests (`REQ-005`, `AC-4`).
pub fn replay_exact<G: DepGraph + ?Sized>(
    graph: &G,
    head_state: &OverlayView,
    commands: &BTreeMap<EntityId, RecordedCommand>,
    scope: &RecomputeScope,
    recorded: &RecomputeCommitted,
    runner: &dyn RecomputeRunner,
) -> Result<(ReplayOutcome, RecomputeExecution), RecompError> {
    let exec = execute_recompute(graph, head_state, commands, scope, runner, &NeverCancel, &NullProgress)?;
    if exec.committed == *recorded && exec.committed.canonical_bytes()? == recorded.canonical_bytes()? {
        Ok((ReplayOutcome::Identical, exec))
    } else {
        Ok((
            ReplayOutcome::Diverged {
                reason: format!(
                    "recompute replay mismatch: recorded {} recomputed / {} failed vs replayed {} recomputed / {} failed",
                    recorded.recomputed.len(),
                    recorded.failed.len(),
                    exec.committed.recomputed.len(),
                    exec.committed.failed.len(),
                ),
            },
            exec,
        ))
    }
}

// ------------------------------------------------------------------ registry command definition

/// Builds the registry [`CommandDef`] for `project.recompute_dependents@1`
/// (W2, D0, undo `head_move`, CLI `fid project recompute-dependents`, MCP `project_recompute_dependents`).
pub fn command_def() -> Result<CommandDef, DefError> {
    let uuid_schema =
        |desc: &str| obj([("description", text(desc)), ("pattern", text(UUID_PATTERN)), ("type", text("string"))]);
    let digest_schema =
        |desc: &str| obj([("description", text(desc)), ("pattern", text(DIGEST_PATTERN)), ("type", text("string"))]);

    let scope_schema = obj([
        ("description", text("Elements to recompute: explicit list, 'all_outdated', or subtree root.")),
        (
            "oneOf",
            CValue::Arr(vec![
                obj([("enum", strs(&["all_outdated"])), ("type", text("string"))]),
                obj([
                    ("additionalProperties", CValue::Bool(false)),
                    (
                        "properties",
                        obj([(
                            "elements",
                            obj([
                                ("description", text("Explicit element IDs in scope.")),
                                ("items", uuid_schema("Element entity UUID.")),
                                ("minItems", CValue::Num(1.0)),
                                ("type", text("array")),
                            ]),
                        )]),
                    ),
                    ("required", strs(&["elements"])),
                    ("type", text("object")),
                ]),
                obj([
                    ("additionalProperties", CValue::Bool(false)),
                    (
                        "properties",
                        obj([("subtree", uuid_schema("Root element whose transitive dependents are recomputed."))]),
                    ),
                    ("required", strs(&["subtree"])),
                    ("type", text("object")),
                ]),
            ]),
        ),
    ]);

    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj([("scope", scope_schema)])),
        ("required", strs(&["scope"])),
        ("type", text("object")),
    ]);

    let recomputed_item = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("digest", digest_schema("SHA-256 digest of the recomputed output.")),
                ("id", uuid_schema("Recomputed element UUID.")),
            ]),
        ),
        ("required", strs(&["digest", "id"])),
        ("type", text("object")),
    ]);

    let failed_item = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                (
                    "code",
                    obj([
                        ("description", text("Typed failure code.")),
                        ("minLength", CValue::Num(1.0)),
                        ("type", text("string")),
                    ]),
                ),
                ("id", uuid_schema("Failed element UUID.")),
            ]),
        ),
        ("required", strs(&["code", "id"])),
        ("type", text("object")),
    ]);

    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("failed", obj([("items", failed_item), ("type", text("array"))])),
                ("recomputed", obj([("items", recomputed_item), ("type", text("array"))])),
                ("skipped", obj([("items", uuid_schema("Skipped element UUID.")), ("type", text("array"))])),
            ]),
        ),
        ("required", strs(&["failed", "recomputed", "skipped"])),
        ("type", text("object")),
    ]);

    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text(
                "Recomputes outdated or failed elements in scope in deterministic topological order with failure \
                 isolation and commits the result as a single journal entry.",
            ),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[RECOMPUTE_COMMITTED_EVENT, ELEMENT_STATE_CHANGED_EVENT])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text(RECOMPUTE_COMMAND_NAME)),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Recompute dependents")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

/// Full command ID string `"project.recompute_dependents@1"`.
pub const fn command_id() -> &'static str {
    RECOMPUTE_DEPENDENTS
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

fn parse_digest_str(s: &str) -> Option<Digest> {
    let hex = s.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, slot) in bytes.iter_mut().enumerate() {
        let hi = hex_nibble(hex.as_bytes()[2 * i])?;
        let lo = hex_nibble(hex.as_bytes()[2 * i + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(Digest(bytes))
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}
