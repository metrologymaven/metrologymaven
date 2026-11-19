//! Change propagation and recompute mode. Spec node `CORE.DEP.PROP`.
//!
//! When a commit changes elements, every transitive dependent in state `current` or
//! `failed` becomes `outdated` in the same commit (REQ-001); `not_computed` elements are
//! left alone (REQ-002). In `auto` mode the commit then submits one recompute of exactly
//! the marked elements; in `manual` mode it submits nothing (REQ-004, REQ-005).
//!
//! The pass is planned on read-only inputs by [`plan`] and applied all or nothing by
//! [`propagate`]:
//!
//! 1. **Region.** Breadth-first from the changed elements (ascending ID) over the
//!    dependents, collecting the changed elements and all their transitive dependents.
//! 2. **Order.** Kahn's algorithm over the region with an ascending-ID ready queue: a
//!    topological order whose ties are broken by entity ID. Elements left over sit on or
//!    behind a cycle; the pass fails with `cycle` and changes nothing.
//! 3. **Staleness.** In that order, an element is *pending* (its value is stale) unless the
//!    commit itself computed it (a changed root, or a recomputed downstream element) from
//!    inputs that are all fresh: no pending input in the region and every input outside
//!    the region `current`. Changed roots get the same input check. Anything downstream of
//!    a pending element is pending. Propagation never makes anything `current`; it only
//!    decides what must become `outdated`.
//! 4. **Marking.** Every pending element in state `current` or `failed` is marked.
//!
//! Every traversal is iterative; inputs are bounded by [`MAX_CHANGED`],
//! [`MAX_PASS_ELEMENTS`] and [`MAX_PASS_EDGES`].

mod error;
mod mode;

pub use error::PropError;
pub use mode::{
    set_command_def, set_recompute_mode, RecomputeMode, RecomputeModeSet, SetRecomputeMode, RECOMPUTE_MODE_SET_EVENT,
    SET_RECOMPUTE_MODE,
};

use crate::edge::{EdgeIndex, EntityId, MAX_COMMIT_ELEMENTS};
use fid_canon::CValue;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// Journal event type of an [`ElementsMarked`] event.
pub const ELEMENTS_MARKED_EVENT: &str = "dep.elements_marked@1";
/// The command an `auto`-mode commit submits for the marked elements (CORE.DEP.RECOMP).
pub const RECOMPUTE_DEPENDENTS: &str = "project.recompute_dependents@1";
/// Most changed elements one pass starts from (the edge module's per-commit limit).
pub const MAX_CHANGED: usize = MAX_COMMIT_ELEMENTS;
/// Most elements one pass may touch: the region, the recomputed list and a marked event.
pub const MAX_PASS_ELEMENTS: usize = 1 << 20;
/// Most edges one pass may follow (each counted once per visit of its input).
pub const MAX_PASS_EDGES: usize = 1 << 24;

fn id_value(id: &EntityId) -> CValue {
    CValue::Str(id.to_string())
}

fn ids_value<'a>(ids: impl IntoIterator<Item = &'a EntityId>) -> CValue {
    CValue::Arr(ids.into_iter().map(id_value).collect())
}

// ------------------------------------------------------------------ element state

/// The state of an element (CORE.PRJ.TREE, REQ-CORE.PRJ-007). Propagation only ever moves
/// `current` or `failed` to `outdated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementState {
    Current,
    Outdated,
    Failed,
    NotComputed,
}

impl ElementState {
    pub fn as_str(self) -> &'static str {
        match self {
            ElementState::Current => "current",
            ElementState::Outdated => "outdated",
            ElementState::Failed => "failed",
            ElementState::NotComputed => "not_computed",
        }
    }

    /// Parses the exact lowercase text form; anything else is `None`.
    pub fn parse(text: &str) -> Option<ElementState> {
        match text {
            "current" => Some(ElementState::Current),
            "outdated" => Some(ElementState::Outdated),
            "failed" => Some(ElementState::Failed),
            "not_computed" => Some(ElementState::NotComputed),
            _ => None,
        }
    }

    /// Whether marking moves this state to `outdated` (REQ-001, REQ-002).
    pub fn is_markable(self) -> bool {
        matches!(self, ElementState::Current | ElementState::Failed)
    }
}

/// Element states keyed by entity ID, the part of the project document propagation reads
/// and writes. Ordered, so iteration is in ascending ID order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementStates(BTreeMap<EntityId, ElementState>);

impl ElementStates {
    pub fn new() -> ElementStates {
        ElementStates::default()
    }

    /// Sets the state of `element`; returns the previous state.
    pub fn insert(&mut self, element: EntityId, state: ElementState) -> Option<ElementState> {
        self.0.insert(element, state)
    }

    pub fn get(&self, element: &EntityId) -> Option<ElementState> {
        self.0.get(element).copied()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every `(element, state)`, ascending by element.
    pub fn iter(&self) -> impl Iterator<Item = (&EntityId, &ElementState)> {
        self.0.iter()
    }

    /// Applies a marked event all or nothing: every listed element must exist and be
    /// `current` or `failed` (`not_found/missing_state`, `invalid_transition` otherwise);
    /// only then do all become `outdated`. Replay applies journaled events through this.
    pub fn apply_marked(&mut self, event: &ElementsMarked) -> Result<(), PropError> {
        for id in event.elements() {
            match self.get(id) {
                None => return Err(PropError::MissingState(*id)),
                Some(s) if !s.is_markable() => return Err(PropError::InvalidTransition { element: *id, from: s }),
                Some(_) => {}
            }
        }
        for id in event.elements() {
            self.0.insert(*id, ElementState::Outdated);
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ graph

/// The dependency graph as propagation reads it. An edge `input -> element` means
/// `element` was computed from `input`. Sets are ascending, so traversal is deterministic.
pub trait DepGraph {
    /// Whether `element` is a known element.
    fn contains(&self, element: &EntityId) -> bool;
    /// Direct dependents of `element`; `None` or empty if it has none.
    fn dependents_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>>;
    /// Direct inputs of `element`; `None` or empty if it has none.
    fn inputs_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>>;
    /// Every known element, strictly ascending. Propagation numbers elements by their
    /// position here, so its traversal uses plain vectors instead of per-element maps.
    fn elements_ascending(&self) -> Vec<EntityId>;
}

impl DepGraph for EdgeIndex {
    fn contains(&self, element: &EntityId) -> bool {
        EdgeIndex::contains(self, element)
    }
    fn dependents_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>> {
        EdgeIndex::dependents_of(self, element)
    }
    fn inputs_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>> {
        EdgeIndex::inputs_of(self, element)
    }
    fn elements_ascending(&self) -> Vec<EntityId> {
        self.elements().copied().collect()
    }
}

/// A plain adjacency graph with no invariants beyond its edges: unlike [`EdgeIndex`] it
/// may hold cycles. It stands for any graph read from outside the edge index (for example
/// one assembled by an importer), and is what propagation's cycle guard is tested against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Adjacency {
    forward: BTreeMap<EntityId, BTreeSet<EntityId>>,
    reverse: BTreeMap<EntityId, BTreeSet<EntityId>>,
}

impl Adjacency {
    pub fn new() -> Adjacency {
        Adjacency::default()
    }

    /// Makes `element` known (with no edges if it was new).
    pub fn add_element(&mut self, element: EntityId) {
        self.reverse.entry(element).or_default();
    }

    /// Adds the edge `input -> element`, making both known. A self-edge is kept.
    pub fn add_edge(&mut self, input: EntityId, element: EntityId) {
        self.add_element(input);
        self.reverse.entry(element).or_default().insert(input);
        self.forward.entry(input).or_default().insert(element);
    }
}

impl DepGraph for Adjacency {
    fn contains(&self, element: &EntityId) -> bool {
        self.reverse.contains_key(element)
    }
    fn dependents_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>> {
        self.forward.get(element)
    }
    fn inputs_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>> {
        self.reverse.get(element)
    }
    fn elements_ascending(&self) -> Vec<EntityId> {
        self.reverse.keys().copied().collect()
    }
}

// ------------------------------------------------------------------ event and submission

/// The event that marks elements `outdated`, appended in the same commit as the change.
/// `elements` is non-empty, strictly ascending and at most [`MAX_PASS_ELEMENTS`] long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementsMarked {
    elements: Vec<EntityId>,
}

impl ElementsMarked {
    /// Builds an event. Refuses an empty list, a repeated or out-of-order element and more
    /// than [`MAX_PASS_ELEMENTS`] elements (`bad_event` / `input_limit`).
    pub fn new(elements: Vec<EntityId>) -> Result<ElementsMarked, PropError> {
        if elements.len() > MAX_PASS_ELEMENTS {
            return Err(PropError::InputLimit("elements"));
        }
        if elements.is_empty() {
            return Err(PropError::BadEvent("no elements"));
        }
        for pair in elements.windows(2) {
            if let [a, b] = pair {
                if a >= b {
                    return Err(PropError::BadEvent("elements not strictly ascending"));
                }
            }
        }
        Ok(ElementsMarked { elements })
    }

    /// The marked elements, ascending.
    pub fn elements(&self) -> &[EntityId] {
        &self.elements
    }

    /// `{"elements": ["<id>", ...]}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("elements".to_string(), ids_value(&self.elements));
        CValue::Obj(o)
    }

    /// Reads an event written by [`ElementsMarked::to_value`]. Fails closed on any other
    /// key, a missing key, a non-canonical ID, an empty, repeated or unsorted list, or too
    /// many elements.
    pub fn from_value(v: &CValue) -> Result<ElementsMarked, PropError> {
        let CValue::Obj(o) = v else { return Err(PropError::BadEvent("not an object")) };
        if o.keys().any(|k| k != "elements") {
            return Err(PropError::BadEvent("unknown key"));
        }
        let list = match o.get("elements") {
            Some(CValue::Arr(a)) => a,
            Some(_) => return Err(PropError::BadEvent("elements is not an array")),
            None => return Err(PropError::BadEvent("missing elements")),
        };
        if list.len() > MAX_PASS_ELEMENTS {
            return Err(PropError::InputLimit("elements"));
        }
        let mut elements = Vec::with_capacity(list.len());
        for item in list {
            let CValue::Str(s) = item else { return Err(PropError::BadEvent("element is not a string")) };
            elements.push(crate::edge::parse_id(s).ok_or(PropError::BadEvent("element is not a canonical id"))?);
        }
        ElementsMarked::new(elements)
    }
}

/// The recompute an `auto`-mode commit submits after it completes (REQ-005): one
/// [`RECOMPUTE_DEPENDENTS`] call for exactly the marked elements, ascending. The bus runs
/// it as principal `system` on behalf of the committer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecomputeSubmission {
    elements: Vec<EntityId>,
}

impl RecomputeSubmission {
    /// The command to submit: [`RECOMPUTE_DEPENDENTS`].
    pub fn command(&self) -> &'static str {
        RECOMPUTE_DEPENDENTS
    }

    /// The elements to recompute, ascending.
    pub fn elements(&self) -> &[EntityId] {
        &self.elements
    }

    /// The parameters, in CORE.DEP.RECOMP's element-list scope:
    /// `{"scope": {"elements": ["<id>", ...]}}`.
    pub fn params(&self) -> CValue {
        let mut scope = BTreeMap::new();
        scope.insert("elements".to_string(), ids_value(&self.elements));
        let mut o = BTreeMap::new();
        o.insert("scope".to_string(), CValue::Obj(scope));
        CValue::Obj(o)
    }
}

// ------------------------------------------------------------------ plan

/// The outcome of planning one pass. Lists are in propagation order (topological, ties by
/// ascending entity ID) unless stated otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marking {
    order: Vec<EntityId>,
    pending: Vec<EntityId>,
    marked: Vec<EntityId>,
}

impl Marking {
    /// Every element of the region (changed elements and their transitive dependents) in
    /// propagation order.
    pub fn order(&self) -> &[EntityId] {
        &self.order
    }

    /// Elements whose value is stale after this commit, in propagation order: every
    /// downstream element the commit did not recompute against fresh inputs, whatever its
    /// state.
    pub fn pending(&self) -> &[EntityId] {
        &self.pending
    }

    /// Pending elements in state `current` or `failed`, in propagation order: the ones
    /// this commit moves to `outdated`.
    pub fn marked(&self) -> &[EntityId] {
        &self.marked
    }

    /// The event to append in the same commit, or `None` if nothing is marked.
    pub fn event(&self) -> Result<Option<ElementsMarked>, PropError> {
        if self.marked.is_empty() {
            return Ok(None);
        }
        let mut ids = self.marked.clone();
        ids.sort_unstable();
        ElementsMarked::new(ids).map(Some)
    }

    /// The recompute to submit after the commit: `Some` only in `auto` mode with at least
    /// one marked element (REQ-004, REQ-005).
    pub fn submission(&self, mode: RecomputeMode) -> Option<RecomputeSubmission> {
        if mode != RecomputeMode::Auto || self.marked.is_empty() {
            return None;
        }
        let mut elements = self.marked.clone();
        elements.sort_unstable();
        Some(RecomputeSubmission { elements })
    }
}

/// Successor lists of the region in compressed rows: row `i` is `succ[start[i]..start[i + 1]]`.
struct Rows {
    start: Vec<usize>,
    succ: Vec<u32>,
}

impl Rows {
    fn row(&self, i: usize) -> &[u32] {
        match (self.start.get(i), self.start.get(i + 1)) {
            (Some(&a), Some(&b)) => self.succ.get(a..b).unwrap_or(&[]),
            _ => &[],
        }
    }
}

/// A dense index as `u32`; the pass never holds more than [`MAX_PASS_ELEMENTS`] elements.
fn dense_u32(i: usize) -> Result<u32, PropError> {
    u32::try_from(i).map_err(|_| PropError::InputLimit("elements"))
}

/// Sorts `list`, refusing any repeat (the smallest repeated ID is reported).
fn sorted_distinct(list: &[EntityId]) -> Result<Vec<EntityId>, PropError> {
    let mut v = list.to_vec();
    v.sort_unstable();
    for pair in v.windows(2) {
        if let [a, b] = pair {
            if a == b {
                return Err(PropError::DuplicateElement(*a));
            }
        }
    }
    Ok(v)
}

/// Plans one propagation pass without changing anything.
///
/// - `changed`: the elements whose value this commit changed (at most [`MAX_CHANGED`],
///   distinct, known to `graph`).
/// - `recomputed`: the elements this commit recomputed against the values of this same
///   pass (a recompute commit lists them; an ordinary edit lists none). Distinct, known,
///   and each must be changed or downstream of a changed element (`not_in_pass`).
///
/// Errors are checked in a fixed order (limits, duplicates, unknown, region, cycle,
/// recomputed outside the pass, missing state), each scanning ascending IDs, so the
/// reported error does not depend on the order lists are given in.
pub fn plan<G: DepGraph + ?Sized>(
    graph: &G,
    states: &ElementStates,
    changed: &[EntityId],
    recomputed: &[EntityId],
) -> Result<Marking, PropError> {
    // Both limits first, then duplicates, then unknown elements.
    if changed.len() > MAX_CHANGED {
        return Err(PropError::InputLimit("changed"));
    }
    if recomputed.len() > MAX_PASS_ELEMENTS {
        return Err(PropError::InputLimit("recomputed"));
    }
    let changed = sorted_distinct(changed)?;
    let recomputed = sorted_distinct(recomputed)?;
    for id in changed.iter().chain(&recomputed) {
        if !graph.contains(id) {
            return Err(PropError::UnknownElement(*id));
        }
    }

    // 1. Region: breadth-first from the changed elements (ascending) over dependents
    // (ascending): the changed elements and all their transitive dependents. Elements are
    // numbered by their position in the graph's ascending element list (`all`), so the
    // visited check is a vector lookup; each edge is recorded once, in discovery numbers.
    let all = graph.elements_ascending();
    if all.windows(2).any(|w| matches!(w, [a, b] if a >= b)) {
        return Err(PropError::Internal("graph elements are not strictly ascending"));
    }
    let global = |id: &EntityId| all.binary_search(id).ok();
    const UNSEEN: u32 = u32::MAX;
    let mut local_of: Vec<u32> = vec![UNSEEN; all.len()];
    let mut found: Vec<u32> = Vec::with_capacity(changed.len());
    for id in &changed {
        let g = global(id).ok_or(PropError::UnknownElement(*id))?;
        let slot = local_of.get_mut(g).ok_or(PropError::Internal("dense index"))?;
        *slot = dense_u32(found.len())?;
        found.push(dense_u32(g)?);
    }
    let mut disc = Rows { start: Vec::with_capacity(changed.len() + 1), succ: Vec::new() };
    let mut next = 0usize;
    while let Some(g) = found.get(next).copied() {
        next += 1;
        disc.start.push(disc.succ.len());
        let node = all.get(g as usize).ok_or(PropError::Internal("dense index"))?;
        for d in graph.dependents_of(node).into_iter().flatten() {
            if disc.succ.len() >= MAX_PASS_EDGES {
                return Err(PropError::InputLimit("edges"));
            }
            let gd = global(d).ok_or(PropError::Internal("dependent is not a known element"))?;
            let slot = local_of.get_mut(gd).ok_or(PropError::Internal("dense index"))?;
            if *slot == UNSEEN {
                if found.len() >= MAX_PASS_ELEMENTS {
                    return Err(PropError::InputLimit("elements"));
                }
                *slot = dense_u32(found.len())?;
                found.push(dense_u32(gd)?);
            }
            disc.succ.push(*slot);
        }
    }
    disc.start.push(disc.succ.len());
    drop(local_of);

    // Renumber in ascending ID order, so "smaller index" means "smaller ID". Global
    // numbers are already in ID order, so sorting by them sorts by ID.
    let n = found.len();
    let mut by_id: Vec<u32> = (0..n).map(dense_u32).collect::<Result<_, _>>()?;
    by_id.sort_unstable_by_key(|k| found.get(*k as usize).copied());
    let mut rank: Vec<u32> = vec![0; n];
    for (r, k) in by_id.iter().enumerate() {
        if let Some(slot) = rank.get_mut(*k as usize) {
            *slot = dense_u32(r)?;
        }
    }
    let nodes: Vec<EntityId> =
        by_id.iter().filter_map(|k| found.get(*k as usize).and_then(|g| all.get(*g as usize)).copied()).collect();
    let dense = |id: &EntityId| nodes.binary_search(id).ok();

    // Successors in compressed rows, and in-degrees, both in ID order.
    let mut rows = Rows { start: Vec::with_capacity(n + 1), succ: Vec::with_capacity(disc.succ.len()) };
    let mut indeg: Vec<u32> = vec![0; n];
    for k in &by_id {
        rows.start.push(rows.succ.len());
        for d in disc.row(*k as usize) {
            let j = *rank.get(*d as usize).ok_or(PropError::Internal("dense index"))?;
            rows.succ.push(j);
            let slot = indeg.get_mut(j as usize).ok_or(PropError::Internal("dense index"))?;
            *slot = slot.saturating_add(1);
        }
    }
    rows.start.push(rows.succ.len());
    if nodes.len() != n {
        return Err(PropError::Internal("renumbering"));
    }

    // 2. Order: Kahn's algorithm, smallest ready ID first.
    let mut remaining = indeg;
    let mut ready: BinaryHeap<Reverse<usize>> =
        remaining.iter().enumerate().filter(|(_, d)| **d == 0).map(|(i, _)| Reverse(i)).collect();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for &j in rows.row(i) {
            let j = j as usize;
            let slot = remaining.get_mut(j).ok_or(PropError::Internal("dense index"))?;
            *slot = slot.saturating_sub(1);
            if *slot == 0 {
                ready.push(Reverse(j));
            }
        }
    }
    if order.len() < n {
        return Err(PropError::Cycle(find_cycle(&nodes, &remaining, &rows)?));
    }

    let mut is_recomputed = vec![false; n];
    for id in &recomputed {
        let i = dense(id).ok_or(PropError::NotInPass(*id))?;
        if let Some(flag) = is_recomputed.get_mut(i) {
            *flag = true;
        }
    }

    // States of the region, read in ascending ID order (cache-friendly); a missing state
    // is an error only for an element that turns out pending.
    let state_of: Vec<Option<ElementState>> = nodes.iter().map(|id| states.get(id)).collect();

    // 3. and 4. Staleness and marking, in propagation order.
    let mut has_input_in_region = vec![false; n];
    let mut pending_input = vec![false; n];
    let mut result = Marking { order: Vec::with_capacity(n), pending: Vec::new(), marked: Vec::new() };
    for &i in &order {
        let id = *nodes.get(i).ok_or(PropError::Internal("dense index"))?;
        let downstream = has_input_in_region.get(i).copied().unwrap_or(false);
        // Pending: a downstream element the commit did not recompute, anything with a
        // pending input in the region, and anything (changed roots included) that read an
        // input outside the region which is not `current`.
        let not_recomputed = downstream && !is_recomputed.get(i).copied().unwrap_or(false);
        let pending = if not_recomputed || pending_input.get(i).copied().unwrap_or(true) {
            true
        } else {
            stale_outside_input(graph, states, &id, &dense)?
        };
        if pending {
            let state = state_of.get(i).copied().flatten().ok_or(PropError::MissingState(id))?;
            if state.is_markable() {
                result.marked.push(id);
            }
            result.pending.push(id);
        }
        for &j in rows.row(i) {
            let j = j as usize;
            if let Some(flag) = has_input_in_region.get_mut(j) {
                *flag = true;
            }
            if pending {
                if let Some(flag) = pending_input.get_mut(j) {
                    *flag = true;
                }
            }
        }
        result.order.push(id);
    }
    Ok(result)
}

/// Whether an element computed in this commit (a changed root or a recomputed element) read
/// any input from outside the region that is not `current`: such a value was computed on a
/// stale input, so the element is pending. An input with no state is `missing_state`.
fn stale_outside_input<G: DepGraph + ?Sized>(
    graph: &G,
    states: &ElementStates,
    id: &EntityId,
    dense: &dyn Fn(&EntityId) -> Option<usize>,
) -> Result<bool, PropError> {
    for input in graph.inputs_of(id).into_iter().flatten() {
        if dense(input).is_some() {
            continue;
        }
        match states.get(input) {
            None => return Err(PropError::MissingState(*input)),
            Some(ElementState::Current) => {}
            Some(_) => return Ok(true),
        }
    }
    Ok(false)
}

/// Finds one cycle among the elements Kahn's algorithm could not order (`remaining > 0`).
/// Each of them has an unordered input, so walking from the smallest one to its smallest
/// unordered input must revisit an element within as many steps as there are elements.
/// Returns the cycle in edge direction, starting from its smallest ID.
fn find_cycle(nodes: &[EntityId], remaining: &[u32], rows: &Rows) -> Result<Vec<EntityId>, PropError> {
    let left = |i: usize| remaining.get(i).is_some_and(|d| *d > 0);
    // Smallest unordered input of each unordered element; inputs are scanned ascending.
    let mut smallest_input: BTreeMap<usize, usize> = BTreeMap::new();
    for i in (0..nodes.len()).filter(|i| left(*i)) {
        for &j in rows.row(i) {
            let j = j as usize;
            if left(j) {
                smallest_input.entry(j).or_insert(i);
            }
        }
    }
    let start = (0..nodes.len()).find(|i| left(*i)).ok_or(PropError::Internal("no cycle left"))?;
    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    let mut walk: Vec<usize> = Vec::new();
    let mut cur = start;
    let begin = loop {
        if let Some(&pos) = seen.get(&cur) {
            break pos;
        }
        if walk.len() > nodes.len() {
            return Err(PropError::Internal("cycle walk"));
        }
        seen.insert(cur, walk.len());
        walk.push(cur);
        cur = *smallest_input.get(&cur).ok_or(PropError::Internal("unordered element without input"))?;
    };
    // The walk goes against edge direction; reverse it, then start at the smallest ID.
    let mut cycle: Vec<EntityId> =
        walk.get(begin..).unwrap_or(&[]).iter().rev().filter_map(|i| nodes.get(*i).copied()).collect();
    let min = cycle.iter().enumerate().min_by_key(|(_, id)| **id).map(|(i, _)| i).unwrap_or(0);
    cycle.rotate_left(min);
    Ok(cycle)
}

// ------------------------------------------------------------------ commit hook

/// What one propagation pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Propagation {
    /// The plan that was applied.
    pub marking: Marking,
    /// The event to append in the same commit as the change (`None` if nothing was marked).
    pub event: Option<ElementsMarked>,
    /// The recompute to submit after the commit completes (`auto` mode only).
    pub submission: Option<RecomputeSubmission>,
}

/// The commit hook (REQ-001, REQ-002, REQ-004, REQ-005): runs after the edge hook, plans
/// the pass with [`plan`] and applies the marks to `states`. All or nothing: on error
/// `states` is unchanged and nothing is to be journaled or submitted.
pub fn propagate<G: DepGraph + ?Sized>(
    graph: &G,
    states: &mut ElementStates,
    mode: RecomputeMode,
    changed: &[EntityId],
    recomputed: &[EntityId],
) -> Result<Propagation, PropError> {
    let marking = plan(graph, states, changed, recomputed)?;
    let event = marking.event()?;
    if let Some(e) = &event {
        states.apply_marked(e)?;
    }
    let submission = marking.submission(mode);
    Ok(Propagation { marking, event, submission })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fid_uuid::Uuid;

    fn id(n: u128) -> EntityId {
        Uuid::from_u128(n)
    }

    #[test]
    fn state_text_round_trips_and_is_exact() {
        for s in [ElementState::Current, ElementState::Outdated, ElementState::Failed, ElementState::NotComputed] {
            assert_eq!(ElementState::parse(s.as_str()), Some(s));
        }
        assert_eq!(ElementState::parse("Current"), None);
        assert_eq!(ElementState::parse("stale"), None);
    }

    #[test]
    fn cycle_walk_reports_the_smallest_rotation_in_edge_direction() {
        // 1 -> 2 -> 3 -> 1, all unordered.
        let mut g = Adjacency::new();
        g.add_edge(id(1), id(2));
        g.add_edge(id(2), id(3));
        g.add_edge(id(3), id(1));
        let mut st = ElementStates::new();
        for i in 1..=3 {
            st.insert(id(i), ElementState::Current);
        }
        let got = plan(&g, &st, &[id(2)], &[]);
        assert_eq!(got, Err(PropError::Cycle(vec![id(1), id(2), id(3)])));
        assert_eq!(st.get(&id(1)), Some(ElementState::Current));
    }
}
