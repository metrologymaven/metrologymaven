//! The alignment model: the wrapper that turns a solver output into an element, dependency
//! registration, and in-place recompute (REQ-002, REQ-004 to REQ-007).
//!
//! Creation is all or nothing: on any error the model, the edge index and the ID source
//! (except as noted on [`AlignmentModel::create`]) are unchanged and there is nothing to
//! journal (REQ-006). Recompute visits the changed elements and their transitive dependents
//! in one topological order (ties by ascending ID), re-solves each affected alignment with
//! the solver registered for exactly its stored algorithm ID and version, and reports the
//! whole pass as one [`RecomputeEntry`].

use crate::element::{
    check_inputs, id_value, obj, parse_id, Alignment, AlignmentStatus, ElementRef, InputRef, ParamSet, ParentRef,
    PrincipalKind, Provenance, MAX_INPUTS,
};
use crate::error::{kept, AlignError};
use crate::solver::{SolveStatus, SolverOutput, SolverRegistry};
use fid_canon::CValue;
use fid_dep::{EdgeError, EdgeIndex, EdgesSet};
use fid_kci::AlgorithmId;
use fid_project::IdService;
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};

/// Most alignments one model holds.
pub const MAX_ALIGNMENTS: usize = 65_536;
/// Most graph nodes (changed elements plus their transitive dependents) one recompute visits.
pub const MAX_RECOMPUTE_NODES: usize = 65_536;

/// Where the current content digest of a non-alignment element comes from (the project
/// store; CORE.PRJ). Alignments are looked up in the model first.
pub trait ElementLookup {
    fn digest_of(&self, id: &Uuid) -> Option<Digest>;
}

impl ElementLookup for BTreeMap<Uuid, Digest> {
    fn digest_of(&self, id: &Uuid) -> Option<Digest> {
        self.get(id).copied()
    }
}

/// What a solver command asks the wrapper to create.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateRequest {
    pub algorithm: AlgorithmId,
    /// Input element IDs in command order.
    pub inputs: Vec<Uuid>,
    pub params: ParamSet,
    pub parent: Option<Uuid>,
    /// The principal running the command.
    pub principal: String,
    pub kind: PrincipalKind,
}

/// A created alignment and the dependency event to journal with it in the same commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub alignment: Alignment,
    pub edges: EdgesSet,
}

/// One alignment a recompute touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecomputeItem {
    pub id: Uuid,
    pub status: AlignmentStatus,
    pub failure: Option<String>,
    pub digest_before: Digest,
    pub digest_after: Digest,
}

/// One recompute pass: the changed elements, ascending, and every alignment touched, in the
/// order visited.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecomputeEntry {
    pub changed: Vec<Uuid>,
    pub items: Vec<RecomputeItem>,
}

impl RecomputeEntry {
    /// `{"changed": [..], "items": [{"digest_after", "digest_before", "failure", "id", "status"}]}`.
    pub fn to_value(&self) -> CValue {
        let items = self
            .items
            .iter()
            .map(|i| {
                obj([
                    ("digest_after", CValue::Str(i.digest_after.as_str().into())),
                    ("digest_before", CValue::Str(i.digest_before.as_str().into())),
                    ("failure", i.failure.as_ref().map_or(CValue::Null, |f| CValue::Str(f.clone()))),
                    ("id", id_value(&i.id)),
                    ("status", CValue::Str(i.status.as_str().into())),
                ])
            })
            .collect();
        obj([("changed", CValue::Arr(self.changed.iter().map(id_value).collect())), ("items", CValue::Arr(items))])
    }
}

/// The alignments of a project, by ID.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlignmentModel {
    alignments: BTreeMap<Uuid, Alignment>,
}

fn status_of(s: SolveStatus) -> AlignmentStatus {
    match s {
        SolveStatus::Ok => AlignmentStatus::Ok,
        SolveStatus::NotConverged => AlignmentStatus::NotConverged,
        SolveStatus::Ambiguous => AlignmentStatus::Ambiguous,
    }
}

fn check_report(out: &SolverOutput) -> Result<(), AlignError> {
    if out.report.id == Uuid::NIL || out.report.id == Uuid::MAX {
        return Err(AlignError::BadElement("solver returned a nil or max report id"));
    }
    Ok(())
}

/// Why a recompute could not solve an alignment.
enum Skip {
    Outdated,
    Failed(String),
}

impl AlignmentModel {
    pub fn new() -> AlignmentModel {
        AlignmentModel::default()
    }

    pub fn len(&self) -> usize {
        self.alignments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.alignments.is_empty()
    }

    pub fn get(&self, id: &Uuid) -> Option<&Alignment> {
        self.alignments.get(id)
    }

    /// Alignments in ascending ID order.
    pub fn iter(&self) -> impl Iterator<Item = &Alignment> {
        self.alignments.values()
    }

    /// Adds an alignment read back from storage (its edges are rebuilt from the journal by
    /// CORE.DEP). Refuses an ID already held (`id_collision`) and more than
    /// [`MAX_ALIGNMENTS`].
    pub fn insert_stored(&mut self, alignment: Alignment) -> Result<(), AlignError> {
        if self.alignments.contains_key(&alignment.id()) {
            return Err(AlignError::IdCollision(alignment.id()));
        }
        if self.alignments.len() >= MAX_ALIGNMENTS {
            return Err(AlignError::InputLimit("alignments"));
        }
        self.alignments.insert(alignment.id(), alignment);
        Ok(())
    }

    fn digest_of(&self, world: &dyn ElementLookup, id: &Uuid) -> Option<Digest> {
        match self.alignments.get(id) {
            Some(a) => Some(a.digest()),
            None => world.digest_of(id),
        }
    }

    /// Solves, wraps, digests and registers a new alignment (REQ-002, REQ-004, REQ-006).
    ///
    /// Order: budget; provenance; inputs (no repeats, each known to `world` and to `edges`,
    /// else `input_missing`); solver for exactly `req.algorithm` (else `method_unavailable`);
    /// parent (an alignment of this model, else `not_found`; status `ok`, else
    /// `parent_unusable`; an input that is an alignment must be `ok` too, else
    /// `parent_unusable`); solve (a solver error is returned unchanged); then a fresh UUIDv7
    /// from `ids` (the only step after the solve that draws an observation), the digest, one
    /// edge from each input and the parent, and insertion. A refusal before the ID is drawn
    /// draws nothing.
    pub fn create(
        &mut self,
        req: &CreateRequest,
        solvers: &SolverRegistry,
        world: &dyn ElementLookup,
        edges: &mut EdgeIndex,
        ids: &mut IdService<'_>,
    ) -> Result<Created, AlignError> {
        if self.alignments.len() >= MAX_ALIGNMENTS {
            return Err(AlignError::InputLimit("alignments"));
        }
        let provenance = Provenance::for_creation(&req.principal, req.kind)?;
        if req.inputs.len() > MAX_INPUTS {
            return Err(AlignError::InputLimit("inputs"));
        }
        let mut inputs: Vec<InputRef> = Vec::with_capacity(req.inputs.len());
        for id in &req.inputs {
            let digest = self.digest_of(world, id).ok_or(AlignError::InputMissing(*id))?;
            if !edges.contains(id) {
                return Err(AlignError::InputMissing(*id));
            }
            // The parent rule applies to an input that is itself an alignment.
            if self.alignments.get(id).is_some_and(|a| a.status() != AlignmentStatus::Ok) {
                return Err(AlignError::ParentUnusable(*id));
            }
            inputs.push(ElementRef { id: *id, digest });
        }
        check_inputs(&inputs)?;
        let solver = solvers
            .get(&req.algorithm)
            .ok_or_else(|| AlignError::MethodUnavailable(kept(&req.algorithm.to_string())))?;
        let (parent, pose) = match req.parent {
            None => (None, None),
            Some(pid) => {
                let p = self.alignments.get(&pid).ok_or(AlignError::NotFound(pid))?;
                if !edges.contains(&pid) {
                    return Err(AlignError::NotFound(pid));
                }
                if p.status() != AlignmentStatus::Ok {
                    return Err(AlignError::ParentUnusable(pid));
                }
                (Some(ParentRef { id: pid, digest: p.digest() }), Some(*p.transform()))
            }
        };
        let out = solver.solve(&inputs, &req.params, pose.as_ref()).map_err(AlignError::Solver)?;
        check_report(&out)?;
        let id = ids.new_v7().map_err(AlignError::Id)?;
        if parse_id(&id.to_string()).is_none() || self.alignments.contains_key(&id) || edges.contains(&id) {
            return Err(AlignError::IdCollision(id));
        }
        let mut from: BTreeSet<Uuid> = inputs.iter().map(|i| i.id).collect();
        if let Some(p) = &parent {
            from.insert(p.id);
        }
        let alignment = Alignment::assemble(
            id,
            req.algorithm.clone(),
            inputs,
            req.params.clone(),
            out.transform,
            parent,
            out.report,
            provenance,
            status_of(out.status),
            None,
        )?;
        let set = EdgesSet::from_set(id, &from).map_err(AlignError::Edge)?;
        edges.apply(std::slice::from_ref(&set)).map_err(|e| match e {
            EdgeError::DanglingInput { input, .. } => AlignError::InputMissing(input),
            other => AlignError::Edge(other),
        })?;
        self.alignments.insert(id, alignment.clone());
        Ok(Created { alignment, edges: set })
    }

    /// Recomputes every alignment in `changed` or downstream of it, in place (REQ-002,
    /// REQ-005 to REQ-007). The pass is planned and run on a copy and committed only if it
    /// completes; it fails as a whole only on malformed input (`not_found` for a `changed`
    /// ID the edge index does not know, `input_limit`) or an internal fault.
    ///
    /// A non-alignment element in the pass that is not in `changed` is pending (CORE.DEP has
    /// not recomputed it), so it counts as outdated for everything downstream.
    ///
    /// Per alignment, in visiting order:
    /// - an upstream node failed, became outdated or is pending in this pass, or the parent's
    ///   status or the status of an input that is an alignment is not `ok`: `outdated`;
    /// - the parent alignment no longer exists, or an input digest cannot be read:
    ///   `failed` with `input_missing`;
    /// - no solver for exactly the stored algorithm ID and version: `failed` with
    ///   `method_unavailable` (no other version is tried);
    /// - the solver refuses: `failed` with the solver's code;
    /// - otherwise inputs (fresh digests), parent digest, transform, report, status and
    ///   digest are replaced; the ID, algorithm, parameters and provenance are kept.
    ///
    /// A `failed` or `outdated` alignment keeps its previous content and digest.
    pub fn recompute(
        &mut self,
        changed: &[Uuid],
        solvers: &SolverRegistry,
        world: &dyn ElementLookup,
        edges: &EdgeIndex,
    ) -> Result<RecomputeEntry, AlignError> {
        if changed.len() > MAX_RECOMPUTE_NODES {
            return Err(AlignError::InputLimit("recompute"));
        }
        let changed: BTreeSet<Uuid> = changed.iter().copied().collect();
        let nodes = downstream(&changed, edges)?;
        let order = topological(&nodes, edges)?;
        let mut working: BTreeMap<Uuid, Alignment> = BTreeMap::new();
        let mut bad: BTreeSet<Uuid> = BTreeSet::new();
        let mut items = Vec::new();
        for n in order {
            let upstream_bad = edges.dependencies(&n).map_err(AlignError::Edge)?.iter().any(|d| bad.contains(d));
            let Some(current) = self.alignments.get(&n) else {
                // Not an alignment and not itself changed: CORE.DEP has not recomputed it yet,
                // so its digest is stale. It is pending, and everything downstream of it is
                // outdated, never solved against the stale digest.
                if upstream_bad || !changed.contains(&n) {
                    bad.insert(n);
                }
                continue;
            };
            let next =
                if upstream_bad { Err(Skip::Outdated) } else { self.solve_again(current, &working, solvers, world) };
            let updated = match next {
                Ok(a) => a,
                Err(Skip::Outdated) => {
                    bad.insert(n);
                    current.with_status(AlignmentStatus::Outdated, None)?
                }
                Err(Skip::Failed(code)) => {
                    bad.insert(n);
                    current.with_status(AlignmentStatus::Failed, Some(code))?
                }
            };
            items.push(RecomputeItem {
                id: n,
                status: updated.status(),
                failure: updated.failure().map(str::to_string),
                digest_before: current.digest(),
                digest_after: updated.digest(),
            });
            working.insert(n, updated);
        }
        self.alignments.extend(working);
        Ok(RecomputeEntry { changed: changed.into_iter().collect(), items })
    }

    /// One alignment's re-solve against the state of the pass so far.
    fn solve_again(
        &self,
        current: &Alignment,
        working: &BTreeMap<Uuid, Alignment>,
        solvers: &SolverRegistry,
        world: &dyn ElementLookup,
    ) -> Result<Alignment, Skip> {
        let latest = |id: &Uuid| working.get(id).or_else(|| self.alignments.get(id));
        let missing = || Skip::Failed("input_missing".to_string());
        let (parent, pose) = match current.parent() {
            None => (None, None),
            Some(p) => {
                let pa = latest(&p.id).ok_or_else(missing)?;
                if pa.status() != AlignmentStatus::Ok {
                    return Err(Skip::Outdated);
                }
                (Some(ParentRef { id: p.id, digest: pa.digest() }), Some(*pa.transform()))
            }
        };
        if current.inputs().iter().any(|i| latest(&i.id).is_some_and(|a| a.status() != AlignmentStatus::Ok)) {
            return Err(Skip::Outdated);
        }
        let solver = solvers.get(current.algorithm()).ok_or_else(|| Skip::Failed("method_unavailable".to_string()))?;
        let mut inputs = Vec::with_capacity(current.inputs().len());
        for i in current.inputs() {
            let digest = match latest(&i.id) {
                Some(a) => a.digest(),
                None => world.digest_of(&i.id).ok_or_else(missing)?,
            };
            inputs.push(ElementRef { id: i.id, digest });
        }
        let out =
            solver.solve(&inputs, current.params(), pose.as_ref()).map_err(|e| Skip::Failed(e.code().to_string()))?;
        check_report(&out).map_err(|e| Skip::Failed(e.code().to_string()))?;
        Alignment::assemble(
            current.id(),
            current.algorithm().clone(),
            inputs,
            current.params().clone(),
            out.transform,
            parent,
            out.report,
            current.provenance().clone(),
            status_of(out.status),
            None,
        )
        .map_err(|e| Skip::Failed(e.code().to_string()))
    }
}

/// `changed` (each must be known to `edges`, else `not_found`) and every element downstream
/// of any of them: one iterative walk over the union, refused with `input_limit` as soon as it
/// exceeds [`MAX_RECOMPUTE_NODES`].
fn downstream(changed: &BTreeSet<Uuid>, edges: &EdgeIndex) -> Result<BTreeSet<Uuid>, AlignError> {
    if changed.len() > MAX_RECOMPUTE_NODES {
        return Err(AlignError::InputLimit("recompute"));
    }
    for c in changed {
        if !edges.contains(c) {
            return Err(AlignError::NotFound(*c));
        }
    }
    let mut nodes = changed.clone();
    let mut stack: Vec<Uuid> = changed.iter().copied().collect();
    while let Some(n) = stack.pop() {
        for m in edges.dependents(&n).map_err(AlignError::Edge)? {
            if nodes.insert(m) {
                if nodes.len() > MAX_RECOMPUTE_NODES {
                    return Err(AlignError::InputLimit("recompute"));
                }
                stack.push(m);
            }
        }
    }
    Ok(nodes)
}

/// Kahn's algorithm over the subgraph induced by `nodes`; among ready nodes the smallest ID
/// goes first, so the order is a pure function of the graph.
fn topological(nodes: &BTreeSet<Uuid>, edges: &EdgeIndex) -> Result<Vec<Uuid>, AlignError> {
    let mut indegree: BTreeMap<Uuid, usize> = BTreeMap::new();
    for n in nodes {
        let deps = edges.dependencies(n).map_err(AlignError::Edge)?;
        indegree.insert(*n, deps.iter().filter(|d| nodes.contains(d)).count());
    }
    let mut ready: BTreeSet<Uuid> = indegree.iter().filter(|(_, d)| **d == 0).map(|(n, _)| *n).collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(n) = ready.pop_first() {
        order.push(n);
        for m in edges.dependents(&n).map_err(AlignError::Edge)? {
            if let Some(d) = indegree.get_mut(&m) {
                *d = d.saturating_sub(1);
                if *d == 0 {
                    ready.insert(m);
                }
            }
        }
    }
    if order.len() != nodes.len() {
        return Err(AlignError::Internal("the edge index holds a cycle"));
    }
    Ok(order)
}
