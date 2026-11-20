//! Structural validation of an Uncertainty Model. Spec node `UNC.MODEL.GRAPH.CHECK`.
//!
//! [`validate`] is the gate every model passes before an engine evaluates it: it returns a
//! [`ValidModel`], which nothing else can build, or every finding, never only the first.
//! It fails closed: anything it cannot resolve is a finding.
//!
//! # Passes (pinned order)
//!
//! An endpoint *resolves* when its node exists and has the named port in the right
//! direction: a source is an input node's implicit port `value` or an operator output
//! port; a target is an operator input port or an output node's implicit port `value`.
//! An edge is *resolved* when both ends resolve. Only resolved edges carry the graph used
//! by passes 2 and 6.
//!
//! 1. `dangling_edge`: an edge with an end that does not resolve (an unknown node, an
//!    unknown port, an edge out of an output node or into an input node).
//! 2. `cycle`: Kahn's topological sort over resolved edges, the ready set kept in node ID
//!    order. If nodes remain, each strongly connected component of the remainder that is a
//!    cycle (two or more nodes, or one node with an edge to itself) is reported once,
//!    naming its lowest node ID and listing its members.
//! 3. `unknown_operator`: an operator whose `algorithm_id@algorithm_version` is not an
//!    algorithm of the supplied kernel manifest (`KRN.MAN.MANI.SCHEMA`). Every manifest
//!    algorithm carries the digest of its `KRN.MAN.CARD` spec card, so membership also
//!    means the operator has a registered card.
//! 4. `unsourced_input`: an input without a source, or whose source digest is empty: the
//!    tombstone digest (64 zeros) or the SHA-256 of zero bytes.
//! 5. Ports: `arity_mismatch` when the number of edges whose target resolves to a port
//!    differs from its declared arity (an output node's port has arity 1), and
//!    `unit_mismatch` for a resolved edge whose ends differ in quantity kind or in unit.
//!    Units are compared as unit codes, exactly: `si:rad` and `si:1` are different units
//!    although both have dimension one, so an angle never flows silently into a ratio.
//! 6. `unreachable_output`: an output node that no input node reaches over resolved edges.
//! 7. `correlation_not_input`: a correlation group member that is not an input node.
//! 8. `invalid_correlation`: a group whose matrix fails `UNC.MODEL.DIST`'s
//!    [`validate_correlation`], with that check's reason (`shape`, `asymmetric`,
//!    `diagonal`, `off_diagonal_range`, `indefinite`, or the code of any other error).
//!
//! # Order of findings
//!
//! Sorted by (pass, node ID, then the remaining fields), all compared as bytes. The node
//! ID of a finding is the target node for `dangling_edge`, `arity_mismatch` and
//! `unit_mismatch`, the lowest member for `cycle`, and the group's first (smallest) input
//! ID for the correlation passes. The model is already in canonical order and the key is
//! total, so the list depends only on the model's content, never on the order of nodes,
//! edges or groups in the document. Every value is an integer or a string, so the result
//! is identical on every target, wasm32 included.

use std::collections::BTreeSet;
use std::fmt;

use fid_kci::manifest::KernelManifest;
use fid_types::Digest;

use super::{CorrelationGroup, Edge, Endpoint, UncertaintyModel};
use crate::dist::{validate_correlation, DistError, UnitCode};

/// The implicit port of input and output nodes.
pub const VALUE_PORT: &str = "value";

/// SHA-256 of zero bytes: a source digest that points at nothing.
const EMPTY_SHA256: &str = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// One finding of [`validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// Pass 1: an edge with an end that does not resolve.
    DanglingEdge { from: Endpoint, to: Endpoint },
    /// Pass 2: a cycle; `node` is the lowest ID on it, `members` the whole strongly
    /// connected component, ascending.
    Cycle { node: String, members: Vec<String> },
    /// Pass 3: the operator's algorithm is not in the kernel manifest.
    UnknownOperator { node: String, algorithm: String },
    /// Pass 4: an input without a usable source reference.
    UnsourcedInput { node: String },
    /// Pass 5: a port's connection count differs from its arity.
    ArityMismatch { node: String, port: String, arity: u32, connected: u64 },
    /// Pass 5: an edge between ports of different quantity kind or unit.
    UnitMismatch { node: String, port: String, from: Endpoint },
    /// Pass 6: no input reaches this output.
    UnreachableOutput { node: String },
    /// Pass 7: `node`, a member of the group named by its first input `group`, is not an
    /// input node.
    CorrelationNotInput { group: String, node: String },
    /// Pass 8: the group's matrix fails the `UNC.MODEL.DIST` check.
    InvalidCorrelation { group: String, reason: &'static str },
}

impl ModelError {
    /// Stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            ModelError::DanglingEdge { .. } => "dangling_edge",
            ModelError::Cycle { .. } => "cycle",
            ModelError::UnknownOperator { .. } => "unknown_operator",
            ModelError::UnsourcedInput { .. } => "unsourced_input",
            ModelError::ArityMismatch { .. } => "arity_mismatch",
            ModelError::UnitMismatch { .. } => "unit_mismatch",
            ModelError::UnreachableOutput { .. } => "unreachable_output",
            ModelError::CorrelationNotInput { .. } => "correlation_not_input",
            ModelError::InvalidCorrelation { .. } => "invalid_correlation",
        }
    }

    /// The pass that reports it, 1 to 8.
    pub const fn pass(&self) -> u8 {
        match self {
            ModelError::DanglingEdge { .. } => 1,
            ModelError::Cycle { .. } => 2,
            ModelError::UnknownOperator { .. } => 3,
            ModelError::UnsourcedInput { .. } => 4,
            ModelError::ArityMismatch { .. } | ModelError::UnitMismatch { .. } => 5,
            ModelError::UnreachableOutput { .. } => 6,
            ModelError::CorrelationNotInput { .. } => 7,
            ModelError::InvalidCorrelation { .. } => 8,
        }
    }

    /// The node ID the finding is sorted and reported by (see the module documentation).
    pub fn node(&self) -> &str {
        match self {
            ModelError::DanglingEdge { to, .. } => &to.node,
            ModelError::Cycle { node, .. }
            | ModelError::UnknownOperator { node, .. }
            | ModelError::UnsourcedInput { node }
            | ModelError::ArityMismatch { node, .. }
            | ModelError::UnitMismatch { node, .. }
            | ModelError::UnreachableOutput { node } => node,
            ModelError::CorrelationNotInput { group, .. } | ModelError::InvalidCorrelation { group, .. } => group,
        }
    }

    /// The total sort key: pass, node ID, then the remaining fields (decimal numbers are
    /// compared by value, everything else as bytes).
    fn key(&self) -> (u8, Vec<u8>, Vec<Vec<u8>>, (u64, u64)) {
        let b = |s: &str| s.as_bytes().to_vec();
        let (rest, nums) = match self {
            ModelError::DanglingEdge { from, to } => (vec![b(&to.port), b(&from.node), b(&from.port)], (0, 0)),
            ModelError::Cycle { members, .. } => (members.iter().map(|m| b(m)).collect(), (0, 0)),
            ModelError::UnknownOperator { algorithm, .. } => (vec![b(algorithm)], (0, 0)),
            ModelError::UnsourcedInput { .. } | ModelError::UnreachableOutput { .. } => (Vec::new(), (0, 0)),
            // Arity before unit on the same port: the code is part of the key.
            ModelError::ArityMismatch { port, arity, connected, .. } => {
                (vec![b(port), b("0")], (u64::from(*arity), *connected))
            }
            ModelError::UnitMismatch { port, from, .. } => {
                (vec![b(port), b("1"), b(&from.node), b(&from.port)], (0, 0))
            }
            ModelError::CorrelationNotInput { node, .. } => (vec![b(node)], (0, 0)),
            ModelError::InvalidCorrelation { reason, .. } => (vec![b(reason)], (0, 0)),
        };
        (self.pass(), b(self.node()), rest, nums)
    }
}

impl fmt::Display for ModelError {
    /// One line: the code, then the fields separated by single spaces. Every field passed
    /// the schema's printable-ASCII patterns, so the line needs no escaping.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ModelError::DanglingEdge { from, to } => {
                write!(f, "{code} {} {} {} {}", to.node, to.port, from.node, from.port)
            }
            ModelError::Cycle { node, members } => write!(f, "{code} {node} {}", members.join(",")),
            ModelError::UnknownOperator { node, algorithm } => write!(f, "{code} {node} {algorithm}"),
            ModelError::UnsourcedInput { node } | ModelError::UnreachableOutput { node } => write!(f, "{code} {node}"),
            ModelError::ArityMismatch { node, port, arity, connected } => {
                write!(f, "{code} {node} {port} {arity} {connected}")
            }
            ModelError::UnitMismatch { node, port, from } => {
                write!(f, "{code} {node} {port} {} {}", from.node, from.port)
            }
            ModelError::CorrelationNotInput { group, node } => write!(f, "{code} {group} {node}"),
            ModelError::InvalidCorrelation { group, reason } => write!(f, "{code} {group} {reason}"),
        }
    }
}

impl std::error::Error for ModelError {}

/// A model that passed every check of [`validate`] against one kernel manifest. The only
/// way to obtain one is [`validate`]; `UNC.PROP` accepts nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidModel {
    model: UncertaintyModel,
    manifest_digest: Digest,
    order: Vec<String>,
}

impl ValidModel {
    /// The validated model.
    pub fn model(&self) -> &UncertaintyModel {
        &self.model
    }

    /// Digest of the kernel manifest the operators were checked against.
    pub fn manifest_digest(&self) -> Digest {
        self.manifest_digest
    }

    /// Every node ID in Kahn order, the smallest ready ID first: an evaluation order.
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// The validated model, consuming the wrapper.
    pub fn into_model(self) -> UncertaintyModel {
        self.model
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Input,
    Operator,
    Output,
}

/// One node of the model: kind and index into its kind's (sorted) list.
#[derive(Debug, Clone, Copy)]
struct NodeRef<'a> {
    id: &'a str,
    kind: Kind,
    idx: usize,
}

/// What an edge compares on a resolved port.
struct Port<'a> {
    quantity_kind: &'a str,
    unit: UnitCode,
}

struct Graph<'a> {
    model: &'a UncertaintyModel,
    /// Every node, sorted by ID bytes; the position is the node's graph index.
    nodes: Vec<NodeRef<'a>>,
    /// First arity slot of each operator (by operator index); outputs follow all operators.
    op_slot: Vec<usize>,
    slots: usize,
}

impl<'a> Graph<'a> {
    fn new(model: &'a UncertaintyModel) -> Graph<'a> {
        let mut nodes = Vec::with_capacity(model.inputs().len() + model.operators().len() + model.outputs().len());
        nodes.extend(model.inputs().iter().enumerate().map(|(idx, n)| NodeRef { id: &n.id, kind: Kind::Input, idx }));
        nodes.extend(model.operators().iter().enumerate().map(|(idx, n)| NodeRef {
            id: &n.id,
            kind: Kind::Operator,
            idx,
        }));
        nodes.extend(model.outputs().iter().enumerate().map(|(idx, n)| NodeRef { id: &n.id, kind: Kind::Output, idx }));
        // IDs are unique across kinds (the schema refuses `duplicate_node`), so this is total.
        nodes.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        let mut op_slot = Vec::with_capacity(model.operators().len());
        let mut slots = 0usize;
        for op in model.operators() {
            op_slot.push(slots);
            slots += op.ports_in.len();
        }
        slots += model.outputs().len();
        Graph { model, nodes, op_slot, slots }
    }

    fn find(&self, id: &str) -> Option<usize> {
        self.nodes.binary_search_by(|n| n.id.as_bytes().cmp(id.as_bytes())).ok()
    }

    /// The source port `e`, if it resolves: (graph index, port).
    fn source(&self, e: &Endpoint) -> Option<(usize, Port<'a>)> {
        let g = self.find(&e.node)?;
        let n = self.nodes[g];
        let port = match n.kind {
            Kind::Input => {
                let x = &self.model.inputs()[n.idx];
                (e.port == VALUE_PORT).then_some(Port { quantity_kind: &x.quantity_kind, unit: x.unit })
            }
            Kind::Operator => self.model.operators()[n.idx]
                .ports_out
                .iter()
                .find(|p| p.name == e.port)
                .map(|p| Port { quantity_kind: &p.quantity_kind, unit: p.unit }),
            Kind::Output => None,
        }?;
        Some((g, port))
    }

    /// The target port `e`, if it resolves: (graph index, port, arity slot, arity).
    fn target(&self, e: &Endpoint) -> Option<(usize, Port<'a>, usize, u32)> {
        let g = self.find(&e.node)?;
        let n = self.nodes[g];
        match n.kind {
            Kind::Input => None,
            Kind::Operator => {
                let op = &self.model.operators()[n.idx];
                let (i, p) = op.ports_in.iter().enumerate().find(|(_, p)| p.name == e.port)?;
                let port = Port { quantity_kind: &p.quantity_kind, unit: p.unit };
                Some((g, port, self.op_slot[n.idx] + i, p.arity))
            }
            Kind::Output => {
                let x = &self.model.outputs()[n.idx];
                let slot = self.slots - self.model.outputs().len() + n.idx;
                (e.port == VALUE_PORT).then_some((g, Port { quantity_kind: &x.quantity_kind, unit: x.unit }, slot, 1))
            }
        }
    }
}

/// Validates `model` against `manifest` (see the module documentation for the passes and
/// the order of findings).
pub fn validate(model: &UncertaintyModel, manifest: &KernelManifest) -> Result<ValidModel, Vec<ModelError>> {
    let graph = Graph::new(model);
    let n = graph.nodes.len();
    let mut errors = Vec::new();

    // Pass 1 (and the data for passes 2, 5, 6): resolve every edge.
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut connected = vec![0u64; graph.slots];
    for e in model.edges() {
        let source = graph.source(&e.from);
        let target = graph.target(&e.to);
        if let Some((_, _, slot, _)) = &target {
            connected[*slot] += 1;
        }
        match (source, target) {
            (Some((s, sp)), Some((t, tp, _, _))) => {
                succ[s].push(t);
                if sp.quantity_kind != tp.quantity_kind || sp.unit != tp.unit {
                    errors.push(unit_mismatch(e));
                }
            }
            _ => errors.push(ModelError::DanglingEdge { from: e.from.clone(), to: e.to.clone() }),
        }
    }

    // Pass 2: Kahn, then the cycles among what is left.
    let order = kahn(&succ);
    if order.len() < n {
        let mut done = vec![false; n];
        for &v in &order {
            done[v] = true;
        }
        for scc in cycles(&succ, &done) {
            let members: Vec<String> = scc.iter().map(|&v| graph.nodes[v].id.to_owned()).collect();
            let node = members.first().cloned().unwrap_or_default();
            errors.push(ModelError::Cycle { node, members });
        }
    }

    // Pass 3: operators in the kernel manifest.
    let algorithms = &manifest.core().algorithms;
    for op in model.operators() {
        let known = op.algorithm().is_some_and(|id| algorithms.binary_search_by(|a| a.id.cmp(&id)).is_ok());
        if !known {
            let algorithm = format!("{}@{}", op.algorithm_id, op.algorithm_version);
            errors.push(ModelError::UnknownOperator { node: op.id.clone(), algorithm });
        }
    }

    // Pass 4: sourced inputs.
    for x in model.inputs() {
        let sourced = x.source.as_ref().is_some_and(|s| !s.digest.is_tombstone() && s.digest.as_str() != EMPTY_SHA256);
        if !sourced {
            errors.push(ModelError::UnsourcedInput { node: x.id.clone() });
        }
    }

    // Pass 5: arity (unit mismatches were found while resolving).
    for (i, op) in model.operators().iter().enumerate() {
        for (j, p) in op.ports_in.iter().enumerate() {
            let c = connected[graph.op_slot[i] + j];
            if c != u64::from(p.arity) {
                errors.push(ModelError::ArityMismatch {
                    node: op.id.clone(),
                    port: p.name.clone(),
                    arity: p.arity,
                    connected: c,
                });
            }
        }
    }
    let first_output_slot = graph.slots - model.outputs().len();
    for (i, x) in model.outputs().iter().enumerate() {
        let c = connected[first_output_slot + i];
        if c != 1 {
            errors.push(ModelError::ArityMismatch {
                node: x.id.clone(),
                port: VALUE_PORT.to_owned(),
                arity: 1,
                connected: c,
            });
        }
    }

    // Pass 6: outputs reached from the inputs.
    let mut seen = vec![false; n];
    let mut stack: Vec<usize> = (0..n).filter(|&v| graph.nodes[v].kind == Kind::Input).collect();
    for &v in &stack {
        seen[v] = true;
    }
    while let Some(v) = stack.pop() {
        for &w in &succ[v] {
            if !seen[w] {
                seen[w] = true;
                stack.push(w);
            }
        }
    }
    for (v, node) in graph.nodes.iter().enumerate() {
        if node.kind == Kind::Output && !seen[v] {
            errors.push(ModelError::UnreachableOutput { node: node.id.to_owned() });
        }
    }

    // Passes 7 and 8: correlation groups.
    for g in model.correlations() {
        let group = group_name(g);
        for id in &g.inputs {
            let is_input = graph.find(id).is_some_and(|v| graph.nodes[v].kind == Kind::Input);
            if !is_input {
                errors.push(ModelError::CorrelationNotInput { group: group.clone(), node: id.clone() });
            }
        }
        let k = g.inputs.len();
        let data: Vec<f64> = g.matrix.iter().flatten().copied().collect();
        if let Err(e) = validate_correlation(k, &data) {
            errors.push(ModelError::InvalidCorrelation { group, reason: reason(&e) });
        }
    }

    if errors.is_empty() {
        let order = order.iter().map(|&v| graph.nodes[v].id.to_owned()).collect();
        return Ok(ValidModel { model: model.clone(), manifest_digest: manifest.manifest_digest(), order });
    }
    errors.sort_by_cached_key(ModelError::key);
    Err(errors)
}

fn unit_mismatch(e: &Edge) -> ModelError {
    ModelError::UnitMismatch { node: e.to.node.clone(), port: e.to.port.clone(), from: e.from.clone() }
}

/// A group is named by its smallest input ID (its first input in canonical order).
fn group_name(g: &CorrelationGroup) -> String {
    g.inputs.iter().min_by(|a, b| a.as_bytes().cmp(b.as_bytes())).cloned().unwrap_or_default()
}

fn reason(e: &DistError) -> &'static str {
    match e {
        DistError::InvalidCorrelation { reason } => reason,
        other => other.code(),
    }
}

/// Kahn's algorithm; the ready set is ordered by graph index, which is node ID order.
/// Returns the sorted prefix (all nodes when the graph is acyclic).
fn kahn(succ: &[Vec<usize>]) -> Vec<usize> {
    let n = succ.len();
    let mut indegree = vec![0usize; n];
    for ws in succ {
        for &w in ws {
            indegree[w] += 1;
        }
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|&v| indegree[v] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(v) = ready.pop_first() {
        order.push(v);
        for &w in &succ[v] {
            indegree[w] -= 1;
            if indegree[w] == 0 {
                ready.insert(w);
            }
        }
    }
    order
}

/// The strongly connected components among the nodes not `done` that are cycles (two or
/// more members, or one member with an edge to itself), each sorted ascending, the list
/// sorted by first member. Iterative Tarjan, so a long chain cannot exhaust the stack.
fn cycles(succ: &[Vec<usize>], done: &[bool]) -> Vec<Vec<usize>> {
    const UNSEEN: usize = usize::MAX;
    let n = succ.len();
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut call: Vec<(usize, usize)> = Vec::new();
    let mut next = 0usize;
    let mut out = Vec::new();
    for root in 0..n {
        if done[root] || index[root] != UNSEEN {
            continue;
        }
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        call.push((root, 0));
        while let Some(top) = call.last_mut() {
            let (v, i) = *top;
            if let Some(&w) = succ[v].get(i) {
                top.1 += 1;
                if done[w] {
                    continue;
                }
                if index[w] == UNSEEN {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(u, _)) = call.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                let mut scc = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
                let is_cycle = scc.len() > 1 || succ[v].contains(&v);
                if is_cycle {
                    scc.sort_unstable();
                    out.push(scc);
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn kahn_takes_the_smallest_ready_node_first() {
        // 2 -> 0, 1 and 3 free: order 1, 2, 0, 3.
        let succ = vec![vec![], vec![], vec![0], vec![]];
        assert_eq!(kahn(&succ), vec![1, 2, 0, 3]);
    }

    #[test]
    fn cycles_are_components_with_a_loop() {
        // 0 -> 1 -> 2 -> 0, 3 -> 3, 4 -> 0 (downstream of nothing, upstream of the ring),
        // 5 -> 6 -> 5 behind 2.
        let succ = vec![vec![1], vec![2], vec![0, 5], vec![3], vec![0], vec![6], vec![5]];
        let done = vec![false; 7];
        assert_eq!(cycles(&succ, &done), vec![vec![0, 1, 2], vec![3], vec![5, 6]]);
        assert!(kahn(&succ).len() < 7);
    }

    #[test]
    fn a_long_ring_needs_no_recursion() {
        let n = 100_000;
        let succ: Vec<Vec<usize>> = (0..n).map(|v| vec![(v + 1) % n]).collect();
        let found = cycles(&succ, &vec![false; n]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].len(), n);
    }
}
