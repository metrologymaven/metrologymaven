//! Dependency edges, cycle rejection and dependency queries. Spec node `CORE.DEP.EDGE`.
//!
//! An edge `input -> element` says that `element` was computed from `input`. The
//! [`EdgeIndex`] keeps a forward map (input to dependents) and a reverse map (element to
//! inputs), both `BTreeMap<EntityId, BTreeSet<EntityId>>`, so every iteration and every
//! answer is in ascending ID order and nothing depends on hash order (D0).
//!
//! - [`EdgeIndex::commit_command`] is the bus commit hook: it derives one edge from each
//!   entity a command references through `x-ref` to each output element (REQ-001),
//!   replaces the incoming edges of updated elements (REQ-002), rejects cycles (REQ-003)
//!   and returns the [`EdgesSet`] events to append in the same commit.
//! - [`EdgeIndex::apply`] commits [`EdgesSet`]s directly; it is all-or-nothing.
//! - [`EdgeIndex::rebuild`] replays journaled events into an equal index (REQ-006).
//! - [`show_dependencies`] answers `project.show_dependencies@1` (REQ-004, REQ-005).
//!
//! Every traversal is iterative; inputs are bounded by [`MAX_INPUTS`],
//! [`MAX_COMMIT_ELEMENTS`], [`MAX_SCHEMA_DEPTH`] and [`MAX_WALK_NODES`].

mod command;
mod error;
mod xref;

pub use command::{command_def, show_dependencies, Dependencies, ShowDependencies, SHOW_DEPENDENCIES};
pub use error::EdgeError;

use fid_canon::CValue;
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

/// A stable entity ID (CONSTITUTION §5).
pub type EntityId = Uuid;

/// Journal event type of an [`EdgesSet`] event.
pub const EDGES_SET_EVENT: &str = "dep.edges_set@1";
/// Most inputs (distinct referenced entities) of one element.
pub const MAX_INPUTS: usize = 4096;
/// Most elements one commit may set.
pub const MAX_COMMIT_ELEMENTS: usize = 4096;
/// Deepest nesting the parameter and schema walkers follow.
pub const MAX_SCHEMA_DEPTH: usize = 64;
/// Most JSON nodes the parameter and schema walkers visit, each.
pub const MAX_WALK_NODES: usize = 65_536;

/// Parses a canonical entity ID: the 36-character lowercase hyphenated UUID form. Any
/// other spelling (uppercase, braces, no hyphens) is refused, so one entity has one text.
pub(crate) fn parse_id(text: &str) -> Option<EntityId> {
    let id = Uuid::parse(text).ok()?;
    (id.to_string() == text).then_some(id)
}

fn id_value(id: &EntityId) -> CValue {
    CValue::Str(id.to_string())
}

fn ids_value<'a>(ids: impl IntoIterator<Item = &'a EntityId>) -> CValue {
    CValue::Arr(ids.into_iter().map(id_value).collect())
}

// ------------------------------------------------------------------ event

/// The event that sets an element's incoming edges: `element` was computed from `inputs`.
/// `inputs` is strictly ascending (no duplicate edges) and at most [`MAX_INPUTS`] long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgesSet {
    element: EntityId,
    inputs: Vec<EntityId>,
}

impl EdgesSet {
    /// Builds an event. Refuses a repeated input (`duplicate_edge`), inputs out of
    /// ascending order (`unsorted_inputs`) and more than [`MAX_INPUTS`] inputs.
    pub fn new(element: EntityId, inputs: Vec<EntityId>) -> Result<EdgesSet, EdgeError> {
        if inputs.len() > MAX_INPUTS {
            return Err(EdgeError::InputLimit("inputs"));
        }
        for pair in inputs.windows(2) {
            if let [a, b] = pair {
                if a == b {
                    return Err(EdgeError::DuplicateEdge { element, input: *a });
                }
                if a > b {
                    return Err(EdgeError::UnsortedInputs(element));
                }
            }
        }
        Ok(EdgesSet { element, inputs })
    }

    /// Builds an event from any input set (sorted and deduplicated by construction).
    pub fn from_set(element: EntityId, inputs: &BTreeSet<EntityId>) -> Result<EdgesSet, EdgeError> {
        EdgesSet::new(element, inputs.iter().copied().collect())
    }

    pub fn element(&self) -> EntityId {
        self.element
    }

    /// The inputs, ascending.
    pub fn inputs(&self) -> &[EntityId] {
        &self.inputs
    }

    /// `{"element": "<id>", "inputs": ["<id>", ...]}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("element".to_string(), id_value(&self.element));
        o.insert("inputs".to_string(), ids_value(&self.inputs));
        CValue::Obj(o)
    }

    /// Reads an event written by [`EdgesSet::to_value`]. Fails closed on any other key, a
    /// missing key, a non-canonical ID, a duplicate or unsorted input, or too many inputs.
    pub fn from_value(v: &CValue) -> Result<EdgesSet, EdgeError> {
        let CValue::Obj(o) = v else { return Err(EdgeError::BadEvent("not an object")) };
        if o.keys().any(|k| k != "element" && k != "inputs") {
            return Err(EdgeError::BadEvent("unknown key"));
        }
        let element = match o.get("element") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(EdgeError::BadEvent("element is not a canonical id"))?,
            Some(_) => return Err(EdgeError::BadEvent("element is not a string")),
            None => return Err(EdgeError::BadEvent("missing element")),
        };
        let list = match o.get("inputs") {
            Some(CValue::Arr(a)) => a,
            Some(_) => return Err(EdgeError::BadEvent("inputs is not an array")),
            None => return Err(EdgeError::BadEvent("missing inputs")),
        };
        if list.len() > MAX_INPUTS {
            return Err(EdgeError::InputLimit("inputs"));
        }
        let mut inputs = Vec::with_capacity(list.len());
        for item in list {
            let CValue::Str(s) = item else { return Err(EdgeError::BadEvent("input is not a string")) };
            inputs.push(parse_id(s).ok_or(EdgeError::BadEvent("input is not a canonical id"))?);
        }
        EdgesSet::new(element, inputs)
    }
}

// ------------------------------------------------------------------ commit hook input

/// Whether a command output creates a new element or updates an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    Create,
    Update,
}

/// One element a command creates or updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    pub element: EntityId,
    pub op: Op,
}

// ------------------------------------------------------------------ index

/// The in-memory edge index. Invariants: `reverse` has one entry per known element (its
/// inputs, possibly empty); `forward` has an entry only for an element with at least one
/// dependent; the two maps hold exactly the same edges. So two indexes with the same
/// edges and elements compare equal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EdgeIndex {
    forward: BTreeMap<EntityId, BTreeSet<EntityId>>,
    reverse: BTreeMap<EntityId, BTreeSet<EntityId>>,
}

/// What an element's incoming edges were before a commit touched it: `None` if the
/// element was new.
type Undo = Vec<(EntityId, Option<BTreeSet<EntityId>>)>;

impl EdgeIndex {
    pub fn new() -> EdgeIndex {
        EdgeIndex::default()
    }

    /// Number of known elements.
    pub fn len(&self) -> usize {
        self.reverse.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reverse.is_empty()
    }

    /// Number of edges.
    pub fn edge_count(&self) -> usize {
        self.reverse.values().map(BTreeSet::len).sum()
    }

    pub fn contains(&self, element: &EntityId) -> bool {
        self.reverse.contains_key(element)
    }

    /// Known elements, ascending.
    pub fn elements(&self) -> impl Iterator<Item = &EntityId> {
        self.reverse.keys()
    }

    /// Removes `element` when it has no dependents (`CORE.DEP.REPL` REQ-003).
    /// Returns `Ok(false)` if `element` still has dependents (leaving the index unchanged),
    /// `Ok(true)` if removed, or `Err(EdgeError::UnknownElement)` if absent.
    pub fn remove_leaf(&mut self, element: &EntityId) -> Result<bool, EdgeError> {
        if !self.reverse.contains_key(element) {
            return Err(EdgeError::UnknownElement(*element));
        }
        if self.forward.get(element).is_some_and(|deps| !deps.is_empty()) {
            return Ok(false);
        }
        if let Some(inputs) = self.reverse.remove(element) {
            for inp in inputs {
                if let Some(deps) = self.forward.get_mut(&inp) {
                    deps.remove(element);
                    if deps.is_empty() {
                        self.forward.remove(&inp);
                    }
                }
            }
        }
        self.forward.remove(element);
        Ok(true)
    }

    /// Every edge as `(input, element)`, ascending by input then element.
    pub fn edges(&self) -> impl Iterator<Item = (EntityId, EntityId)> + '_ {
        self.forward.iter().flat_map(|(input, deps)| deps.iter().map(move |d| (*input, *d)))
    }

    /// Direct inputs of `element`, ascending.
    pub fn dependencies(&self, element: &EntityId) -> Result<Vec<EntityId>, EdgeError> {
        let set = self.reverse.get(element).ok_or(EdgeError::UnknownElement(*element))?;
        Ok(set.iter().copied().collect())
    }

    /// Direct dependents of `element`, ascending.
    pub fn dependents(&self, element: &EntityId) -> Result<Vec<EntityId>, EdgeError> {
        if !self.contains(element) {
            return Err(EdgeError::UnknownElement(*element));
        }
        Ok(self.forward.get(element).map(|s| s.iter().copied().collect()).unwrap_or_default())
    }

    /// Direct inputs of `element` as a borrowed set, without allocating; `None` if unknown.
    pub fn inputs_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>> {
        self.reverse.get(element)
    }

    /// Direct dependents of `element` as a borrowed set, without allocating; `None` if it
    /// has none (or is unknown).
    pub fn dependents_of(&self, element: &EntityId) -> Option<&BTreeSet<EntityId>> {
        self.forward.get(element)
    }

    /// Every element `element` depends on, directly or not, ascending.
    pub fn transitive_dependencies(&self, element: &EntityId) -> Result<Vec<EntityId>, EdgeError> {
        self.closure(element, &self.reverse)
    }

    /// Every element that depends on `element`, directly or not, ascending.
    pub fn transitive_dependents(&self, element: &EntityId) -> Result<Vec<EntityId>, EdgeError> {
        self.closure(element, &self.forward)
    }

    fn closure(
        &self,
        element: &EntityId,
        map: &BTreeMap<EntityId, BTreeSet<EntityId>>,
    ) -> Result<Vec<EntityId>, EdgeError> {
        if !self.contains(element) {
            return Err(EdgeError::UnknownElement(*element));
        }
        let mut seen = BTreeSet::new();
        let mut stack = vec![*element];
        while let Some(n) = stack.pop() {
            for next in map.get(&n).into_iter().flatten() {
                if next != element && seen.insert(*next) {
                    stack.push(*next);
                }
            }
        }
        Ok(seen.into_iter().collect())
    }

    /// The canonical value of the index: `{"edges": [[input, element], ...], "elements":
    /// [...]}`, both lists ascending. Its digest identifies the index (AC-4).
    pub fn to_value(&self) -> CValue {
        let edges = self.edges().map(|(i, e)| CValue::Arr(vec![id_value(&i), id_value(&e)])).collect();
        let mut o = BTreeMap::new();
        o.insert("edges".to_string(), CValue::Arr(edges));
        o.insert("elements".to_string(), ids_value(self.reverse.keys()));
        CValue::Obj(o)
    }

    /// SHA-256 of the RFC 8785 canonical bytes of [`EdgeIndex::to_value`].
    pub fn digest(&self) -> Result<Digest, EdgeError> {
        let d = fid_canon::digest(&self.to_value()).map_err(|_| EdgeError::Internal("canonical json"))?;
        Ok(Digest::from_bytes(d.0))
    }

    /// Rebuilds an index by applying journaled events in journal order, each exactly as
    /// [`EdgeIndex::apply`] applied it. Fails closed on the first invalid event.
    pub fn rebuild<'a>(events: impl IntoIterator<Item = &'a EdgesSet>) -> Result<EdgeIndex, EdgeError> {
        let mut index = EdgeIndex::new();
        for event in events {
            index.apply_one(event)?;
        }
        Ok(index)
    }

    /// Commit hook (REQ-001..003). `schema` is the command's parameter schema, `params`
    /// its validated parameters, `outputs` the elements it creates or updates. Each output
    /// gets one edge from every distinct entity referenced through `x-ref`, except the
    /// command's own outputs (an `x-ref` naming an output names the target, not an
    /// input). On success the index holds the new edges and the returned events, in
    /// ascending element order, are what the bus appends in the same commit. On error
    /// the index is unchanged.
    pub fn commit_command(
        &mut self,
        schema: &CValue,
        params: &CValue,
        outputs: &[Output],
    ) -> Result<Vec<EdgesSet>, EdgeError> {
        if outputs.len() > MAX_COMMIT_ELEMENTS {
            return Err(EdgeError::InputLimit("elements"));
        }
        // Checked in ascending element order, so the reported error does not depend on the
        // order the caller listed the outputs in.
        let mut sorted: Vec<Output> = outputs.to_vec();
        sorted.sort_by_key(|o| (o.element, o.op));
        for pair in sorted.windows(2) {
            if let [a, b] = pair {
                if a.element == b.element {
                    return Err(EdgeError::DuplicateElement(a.element));
                }
            }
        }
        let targets: BTreeSet<EntityId> = sorted.iter().map(|o| o.element).collect();
        for out in &sorted {
            match out.op {
                Op::Create if self.contains(&out.element) => return Err(EdgeError::AlreadyExists(out.element)),
                Op::Update if !self.contains(&out.element) => return Err(EdgeError::UnknownElement(out.element)),
                _ => {}
            }
        }
        let refs = xref::collect(schema, params)?;
        let inputs: BTreeSet<EntityId> = refs.difference(&targets).copied().collect();
        if inputs.len() > MAX_INPUTS {
            return Err(EdgeError::InputLimit("inputs"));
        }
        let sets = targets.iter().map(|e| EdgesSet::from_set(*e, &inputs)).collect::<Result<Vec<_>, _>>()?;
        self.apply(&sets)?;
        Ok(sets)
    }

    /// Commits `sets` all or nothing. Each set replaces its element's incoming edges (and
    /// makes the element known). Sets are applied in ascending element order; an input
    /// must be known before its set is applied. On success returns the sets in the order
    /// applied: the events to journal. On error the index is unchanged.
    pub fn apply(&mut self, sets: &[EdgesSet]) -> Result<Vec<EdgesSet>, EdgeError> {
        if sets.len() > MAX_COMMIT_ELEMENTS {
            return Err(EdgeError::InputLimit("elements"));
        }
        let mut order: Vec<&EdgesSet> = sets.iter().collect();
        order.sort_by_key(|s| s.element);
        for pair in order.windows(2) {
            if let [a, b] = pair {
                if a.element == b.element {
                    return Err(EdgeError::DuplicateElement(a.element));
                }
            }
        }
        let mut undo: Undo = Vec::with_capacity(order.len());
        for set in &order {
            match self.apply_one(set) {
                Ok(prev) => undo.push((set.element, prev)),
                Err(e) => {
                    self.roll_back(undo);
                    return Err(e);
                }
            }
        }
        Ok(order.into_iter().cloned().collect())
    }

    /// Checks one set against the current index and, if valid, applies it. Returns the
    /// element's previous inputs (`None` if it was new). Changes nothing on error.
    fn apply_one(&mut self, set: &EdgesSet) -> Result<Option<BTreeSet<EntityId>>, EdgeError> {
        let x = set.element;
        let inputs: BTreeSet<EntityId> = set.inputs.iter().copied().collect();
        if inputs.contains(&x) {
            return Err(EdgeError::Cycle(vec![x]));
        }
        for input in &inputs {
            if !self.contains(input) {
                return Err(EdgeError::DanglingInput { element: x, input: *input });
            }
        }
        // Paths from x never use x's own incoming edges, so the check is the same before
        // and after the replacement; running it first keeps this step change-free.
        if let Some(path) = self.path_to_any(x, &inputs) {
            return Err(EdgeError::Cycle(rotate_to_min(path)));
        }
        Ok(self.set_inputs(x, inputs))
    }

    /// Iterative DFS over `forward` from `from`, children in ascending ID order. Returns
    /// the DFS path `[from, ..., t]` to the first node `t` in `targets` reached, if any.
    fn path_to_any(&self, from: EntityId, targets: &BTreeSet<EntityId>) -> Option<Vec<EntityId>> {
        if targets.is_empty() {
            return None;
        }
        let mut visited = BTreeSet::from([from]);
        // Each frame: a node and the last child already taken from it.
        let mut stack: Vec<(EntityId, Option<EntityId>)> = vec![(from, None)];
        while let Some(&(node, cursor)) = stack.last() {
            let next = self.forward.get(&node).and_then(|deps| match cursor {
                None => deps.iter().next().copied(),
                Some(c) => deps.range((Bound::Excluded(c), Bound::Unbounded)).next().copied(),
            });
            let Some(child) = next else {
                stack.pop();
                continue;
            };
            if let Some(top) = stack.last_mut() {
                top.1 = Some(child);
            }
            if targets.contains(&child) {
                let mut path: Vec<EntityId> = stack.iter().map(|(n, _)| *n).collect();
                path.push(child);
                return Some(path);
            }
            if visited.insert(child) {
                stack.push((child, None));
            }
        }
        None
    }

    /// Replaces `element`'s inputs, keeping both maps in step. Returns the old inputs.
    fn set_inputs(&mut self, element: EntityId, inputs: BTreeSet<EntityId>) -> Option<BTreeSet<EntityId>> {
        let old = self.reverse.insert(element, inputs.clone());
        for input in old.iter().flatten() {
            if let Some(deps) = self.forward.get_mut(input) {
                deps.remove(&element);
                if deps.is_empty() {
                    self.forward.remove(input);
                }
            }
        }
        for input in inputs {
            self.forward.entry(input).or_default().insert(element);
        }
        old
    }

    /// Undoes applied sets, last first, restoring the index exactly.
    fn roll_back(&mut self, undo: Undo) {
        for (element, prev) in undo.into_iter().rev() {
            match prev {
                Some(old) => {
                    self.set_inputs(element, old);
                }
                None => {
                    self.set_inputs(element, BTreeSet::new());
                    self.reverse.remove(&element);
                }
            }
        }
    }
}

/// Rotates a cycle so it starts at its smallest ID, keeping edge direction.
fn rotate_to_min(mut cycle: Vec<EntityId>) -> Vec<EntityId> {
    let start = cycle.iter().enumerate().min_by_key(|(_, id)| **id).map(|(i, _)| i).unwrap_or(0);
    cycle.rotate_left(start);
    cycle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> EntityId {
        Uuid::from_u128(n)
    }

    #[test]
    fn rotation_starts_at_the_smallest_id_and_keeps_direction() {
        assert_eq!(rotate_to_min(vec![id(3), id(1), id(2)]), vec![id(1), id(2), id(3)]);
        assert_eq!(rotate_to_min(vec![id(1)]), vec![id(1)]);
        assert_eq!(rotate_to_min(Vec::new()), Vec::<EntityId>::new());
    }

    #[test]
    fn canonical_ids_only() {
        assert!(parse_id("00000000-0000-7000-8000-00000000000a").is_some());
        assert!(parse_id("00000000-0000-7000-8000-00000000000A").is_none());
        assert!(parse_id("{00000000-0000-7000-8000-00000000000a}").is_none());
        assert!(parse_id("").is_none());
    }
}
