//! Alignment parent chain, mirror frames in `GEO.FRAME.TREE`, composition to root and
//! invalidation. Spec node `ALN.MODEL.HIER.CHAIN` (`spec/aln/model/hier/chain/NODE.md`).
//!
//! - Every alignment has a mirror frame (`FrameKind::Alignment`) in [`fid_geo_frame::FrameTree`]
//!   whose parent is its parent alignment's mirror frame (or the root/world frame) and whose
//!   `to_parent` transform is the alignment's transform (REQ-001).
//! - [`compose_to_root`] resolves the alignment's mirror frame to the root frame via
//!   `FrameTree::resolve`, keeping GEO's fixed composition order (REQ-002).
//! - [`check_parent`] refuses cycles with `hierarchy_cycle{path}` (REQ-003) and chains deeper
//!   than [`MAX_HIERARCHY_DEPTH`] (64 alignments) with `hierarchy_too_deep{depth}` (REQ-004).
//! - [`set_parent`] (`align.set_parent@1`, W2, D0) atomically updates the alignment's parent
//!   reference, mirror frame parent, CORE.DEP parent edge, and marks the alignment and every
//!   transitive dependent `outdated` in the same commit, emitting one [`SetParentEvent`]
//!   (REQ-005, REQ-007). Errors change nothing and emit nothing.
//! - [`update_alignment_transform`] keeps an alignment's mirror frame synchronized with any
//!   transform change and marks every descendant alignment and dependent result `outdated`
//!   through CORE.DEP in the same commit (REQ-006).

use crate::command::{ID_PATTERN, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH};
use crate::element::{id_value, obj, parse_id, Alignment, AlignmentStatus, ParentRef};
use crate::error::AlignError;
use crate::model::{AlignmentModel, RecomputeEntry};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_dep::{is_markable, plan, EdgeError, EdgeIndex, EdgesSet, ElementStates, ElementsMarked, PropError};
use fid_geo_frame::{Frame, FrameError, FrameId, FrameKind, FrameTree, Transform};
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Maximum number of alignments in a chain from an alignment to the root (REQ-004).
pub const MAX_HIERARCHY_DEPTH: usize = 64;

/// Registry command ID and journal event type of `align.set_parent@1`.
pub const SET_PARENT: &str = "align.set_parent@1";
/// Registry command name without major version.
const SET_PARENT_NAME: &str = "align.set_parent";
/// CLI subcommand of `fid align` for setting an alignment's parent.
pub const CLI_SET_PARENT: &str = "set-parent";
/// MCP tool name for setting an alignment's parent.
pub const MCP_SET_PARENT: &str = "align_set_parent";

/// Most CLI arguments after `fid align set-parent`.
const MAX_CLI_ARGS: usize = 16;
/// Largest serialised [`SetParentEvent`] accepted by [`SetParentEvent::from_json`].
pub const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
/// Deepest nesting in a serialised [`SetParentEvent`].
const MAX_EVENT_DEPTH: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-22";

// ------------------------------------------------------------------ error

/// Errors returned by hierarchy validation, composition, re-parenting and transform sync.
/// On any error nothing in the model, mirror tree, edge index or element states is changed
/// and nothing is journaled.
#[derive(Debug, Clone, PartialEq)]
pub enum HierError {
    /// Setting the parent would make an alignment its own ancestor (REQ-003).
    /// `path` starts and ends at the alignment being reparented:
    /// `[alignment, proposed_parent, ..., alignment]`.
    HierarchyCycle { path: Vec<Uuid> },
    /// Resulting chain from an alignment (or one of its descendants) to the root would
    /// exceed [`MAX_HIERARCHY_DEPTH`] (64 alignments) (REQ-004).
    HierarchyTooDeep { depth: usize },
    /// Referenced alignment or parent does not exist.
    NotFound(Uuid),
    /// Malformed CLI/MCP/JSON parameters or event payload.
    BadParams(&'static str),
    /// Size limit exceeded.
    InputLimit(&'static str),
    /// Underlying alignment element/model error.
    Align(AlignError),
    /// Underlying `GEO.FRAME.TREE` error.
    Frame(FrameError),
    /// Underlying `CORE.DEP.EDGE` error.
    Edge(EdgeError),
    /// Underlying `CORE.DEP.PROP` error.
    Prop(PropError),
    /// Internal invariant fault.
    Internal(&'static str),
}

impl HierError {
    /// Stable error code.
    pub fn code(&self) -> &str {
        match self {
            HierError::HierarchyCycle { .. } => "hierarchy_cycle",
            HierError::HierarchyTooDeep { .. } => "hierarchy_too_deep",
            HierError::NotFound(_) => "not_found",
            HierError::BadParams(_) => "bad_params",
            HierError::InputLimit(_) => "input_limit",
            HierError::Align(e) => e.code(),
            HierError::Frame(_) => "frame_error",
            HierError::Edge(e) => e.code(),
            HierError::Prop(e) => e.code(),
            HierError::Internal(_) => "internal",
        }
    }

    /// Structured JSON error object for CLI `--json` / MCP error reporting.
    pub fn to_value(&self) -> CValue {
        match self {
            HierError::HierarchyCycle { path } => obj([
                ("code", CValue::Str(self.code().into())),
                ("path", CValue::Arr(path.iter().map(id_value).collect())),
            ]),
            HierError::HierarchyTooDeep { depth } => {
                obj([("code", CValue::Str(self.code().into())), ("depth", CValue::Num(*depth as f64))])
            }
            HierError::NotFound(id) => obj([("code", CValue::Str(self.code().into())), ("id", id_value(id))]),
            other => obj([("code", CValue::Str(other.code().into())), ("message", CValue::Str(other.to_string()))]),
        }
    }
}

impl fmt::Display for HierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            HierError::HierarchyCycle { path } => {
                f.write_str("{[")?;
                for (i, id) in path.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{id}")?;
                }
                f.write_str("]}")
            }
            HierError::HierarchyTooDeep { depth } => write!(f, "{{depth: {depth}}}"),
            HierError::NotFound(id) => write!(f, ": {id}"),
            HierError::BadParams(s) | HierError::InputLimit(s) | HierError::Internal(s) => write!(f, ": {s}"),
            HierError::Align(e) => write!(f, ": {e}"),
            HierError::Frame(e) => write!(f, ": {e}"),
            HierError::Edge(e) => write!(f, ": {e}"),
            HierError::Prop(e) => write!(f, ": {e}"),
        }
    }
}

impl std::error::Error for HierError {}

// ------------------------------------------------------------------ mirror frame & hierarchy

/// Mapping from an alignment ID to its mirror frame ID in `GEO.FRAME.TREE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MirrorFrame {
    pub alignment: Uuid,
    pub frame: FrameId,
}

/// Deterministic mirror frame ID for an alignment (bijective with [`Uuid`], never collides
/// with [`FrameId::WORLD`] because canonical alignment IDs are never [`Uuid::NIL`]).
pub fn mirror_frame_id(alignment: Uuid) -> FrameId {
    FrameId::from_bytes(*alignment.as_bytes())
}

/// Deterministic unique display name for an alignment's mirror frame.
fn mirror_frame_name(alignment: Uuid) -> String {
    format!("alignment:{alignment}")
}

/// Maintains the mirror of every alignment in a [`FrameTree`] (`GEO.FRAME.TREE`) using a
/// deterministic `BTreeMap` of [`MirrorFrame`] entries (REQ-001).
#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentHierarchy {
    mirrors: BTreeMap<Uuid, MirrorFrame>,
    tree: FrameTree,
}

impl Default for AlignmentHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

impl AlignmentHierarchy {
    /// An empty alignment hierarchy containing only the root (world) frame.
    pub fn new() -> AlignmentHierarchy {
        AlignmentHierarchy { mirrors: BTreeMap::new(), tree: FrameTree::new() }
    }

    /// Builds a mirror hierarchy for every alignment currently in `model`, checking that the
    /// parent graph is acyclic and has depth at most [`MAX_HIERARCHY_DEPTH`] (REQ-001, REQ-003,
    /// REQ-004).
    pub fn from_model(model: &AlignmentModel) -> Result<AlignmentHierarchy, HierError> {
        // Validate every alignment's chain to root before mutating anything.
        let mut depths: BTreeMap<Uuid, usize> = BTreeMap::new();
        for alignment in model.iter() {
            let d = chain_depth(model, alignment.id())?;
            depths.insert(alignment.id(), d);
        }
        // Insert parents before children: ascending (depth, id) is a topological order.
        let mut ordered: Vec<(usize, Uuid)> = depths.into_iter().map(|(id, d)| (d, id)).collect();
        ordered.sort_unstable();

        let mut hier = AlignmentHierarchy::new();
        for (_, id) in ordered {
            let alignment = model.get(&id).ok_or(HierError::NotFound(id))?;
            hier.insert_mirror(alignment)?;
        }
        Ok(hier)
    }

    /// Rebuilds the mirror frames from `model`, replacing this hierarchy only if `model` is
    /// valid.
    pub fn sync_from_model(&mut self, model: &AlignmentModel) -> Result<(), HierError> {
        let next = AlignmentHierarchy::from_model(model)?;
        *self = next;
        Ok(())
    }

    /// Adds a newly created alignment's mirror frame to the tree. Its parent's mirror frame
    /// (if any) must already be present, and depth must not exceed [`MAX_HIERARCHY_DEPTH`].
    pub fn register_alignment(&mut self, alignment: &Alignment) -> Result<MirrorFrame, HierError> {
        let parent_frame = match alignment.parent() {
            None => self.tree.world(),
            Some(p) => {
                let mf = self.mirrors.get(&p.id).ok_or(HierError::NotFound(p.id))?;
                let parent_depth = self.tree.depth(mf.frame).map_err(HierError::Frame)?;
                if parent_depth + 1 > MAX_HIERARCHY_DEPTH {
                    return Err(HierError::HierarchyTooDeep { depth: parent_depth + 1 });
                }
                mf.frame
            }
        };
        let frame_id = mirror_frame_id(alignment.id());
        let frame = Frame {
            id: frame_id,
            name: mirror_frame_name(alignment.id()),
            kind: FrameKind::Alignment,
            parent: Some(parent_frame),
            to_parent: *alignment.transform(),
        };
        self.tree.insert(frame).map_err(HierError::Frame)?;
        let mf = MirrorFrame { alignment: alignment.id(), frame: frame_id };
        self.mirrors.insert(alignment.id(), mf);
        Ok(mf)
    }

    fn insert_mirror(&mut self, alignment: &Alignment) -> Result<(), HierError> {
        let _ = self.register_alignment(alignment)?;
        Ok(())
    }

    /// The mirror frame entry for `alignment`, if present.
    pub fn mirror_of(&self, alignment: &Uuid) -> Option<&MirrorFrame> {
        self.mirrors.get(alignment)
    }

    /// All mirror frame entries, ascending by alignment ID.
    pub fn mirrors(&self) -> &BTreeMap<Uuid, MirrorFrame> {
        &self.mirrors
    }

    /// Read-only view of the underlying `GEO.FRAME.TREE`.
    pub fn frame_tree(&self) -> &FrameTree {
        &self.tree
    }

    /// Resolves `alignment`'s mirror frame to the root frame using `GEO.FRAME.TREE`'s pinned
    /// composition order (REQ-002).
    pub fn compose_to_root(&self, alignment: Uuid) -> Result<Transform, HierError> {
        let mf = self.mirrors.get(&alignment).ok_or(HierError::NotFound(alignment))?;
        self.tree.resolve(mf.frame, self.tree.world()).map_err(HierError::Frame)
    }

    /// Updates a mirror frame's parent and transform in place (crate-internal; callers validate
    /// with [`check_parent`] first).
    fn reparent_mirror(
        &mut self,
        alignment: Uuid,
        parent: Option<Uuid>,
        transform: Transform,
    ) -> Result<(), HierError> {
        let mf = *self.mirrors.get(&alignment).ok_or(HierError::NotFound(alignment))?;
        let new_parent_frame = match parent {
            None => self.tree.world(),
            Some(pid) => self.mirrors.get(&pid).ok_or(HierError::NotFound(pid))?.frame,
        };
        self.tree.reparent(mf.frame, new_parent_frame, transform).map_err(HierError::Frame)
    }

    /// Updates a mirror frame's transform in place (REQ-006).
    fn set_mirror_transform(&mut self, alignment: Uuid, transform: Transform) -> Result<(), HierError> {
        let mf = *self.mirrors.get(&alignment).ok_or(HierError::NotFound(alignment))?;
        self.tree.set_transform(mf.frame, transform).map_err(HierError::Frame)
    }
}

// ------------------------------------------------------------------ check_parent & compose_to_root

/// Counts the number of alignments on the chain from `id` to the root (`1` for a top-level
/// alignment directly under root). Refuses cycles with [`HierError::HierarchyCycle`], dangling
/// parents with [`HierError::NotFound`], and chains deeper than [`MAX_HIERARCHY_DEPTH`] with
/// [`HierError::HierarchyTooDeep`].
pub fn chain_depth(model: &AlignmentModel, id: Uuid) -> Result<usize, HierError> {
    if model.get(&id).is_none() {
        return Err(HierError::NotFound(id));
    }
    let mut visited: Vec<Uuid> = Vec::with_capacity(MAX_HIERARCHY_DEPTH + 1);
    let mut cur = id;
    loop {
        if let Some(pos) = visited.iter().position(|x| *x == cur) {
            let mut path = visited.get(pos..).unwrap_or(&[]).to_vec();
            path.push(cur);
            return Err(HierError::HierarchyCycle { path });
        }
        visited.push(cur);
        if visited.len() > MAX_HIERARCHY_DEPTH {
            return Err(HierError::HierarchyTooDeep { depth: visited.len() });
        }
        let a = model.get(&cur).ok_or(HierError::NotFound(cur))?;
        match a.parent() {
            None => return Ok(visited.len()),
            Some(p) => cur = p.id,
        }
    }
}

/// Computes the longest downward chain of alignments strictly below `id` (`0` if `id` has no
/// child alignments). Iterative and bounded by `model.len()`.
fn subtree_height(model: &AlignmentModel, id: Uuid) -> Result<usize, HierError> {
    let mut children: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for a in model.iter() {
        if let Some(p) = a.parent() {
            children.entry(p.id).or_default().push(a.id());
        }
    }
    let mut best = 0usize;
    let mut stack: Vec<(Uuid, usize)> = vec![(id, 0)];
    let mut seen: BTreeSet<Uuid> = BTreeSet::new();
    while let Some((cur, h)) = stack.pop() {
        if !seen.insert(cur) {
            return Err(HierError::HierarchyCycle { path: vec![id, cur, id] });
        }
        if h > best {
            best = h;
        }
        if let Some(kids) = children.get(&cur) {
            for &kid in kids {
                stack.push((kid, h + 1));
            }
        }
    }
    Ok(best)
}

/// Validates parenting `id` under `parent` (`None` = root) without mutating anything
/// (REQ-003, REQ-004).
///
/// Called by every command that sets an alignment's parent:
/// 1. `id` must exist in `model` (`not_found`).
/// 2. If `parent` is `Some(pid)`:
///    - `pid` must exist in `model` (`not_found`).
///    - Walking `pid` upward to root must never reach `id` (`hierarchy_cycle` with
///      `path = [id, pid, ..., id]`).
///    - The resulting deepest chain length `depth(pid) + 1 + subtree_height(id)` must be at
///      most [`MAX_HIERARCHY_DEPTH`] (64) (`hierarchy_too_deep{depth}`).
/// 3. If `parent` is `None` (root), `1 + subtree_height(id)` must be at most
///    [`MAX_HIERARCHY_DEPTH`].
pub fn check_parent(model: &AlignmentModel, id: Uuid, parent: Option<Uuid>) -> Result<(), HierError> {
    if model.get(&id).is_none() {
        return Err(HierError::NotFound(id));
    }
    let parent_depth = match parent {
        None => 0,
        Some(pid) => {
            if model.get(&pid).is_none() {
                return Err(HierError::NotFound(pid));
            }
            // Walk pid's ancestors looking for id (cycle) or any pre-existing cycle.
            let mut path: Vec<Uuid> = Vec::with_capacity(MAX_HIERARCHY_DEPTH + 2);
            path.push(id);
            let mut cur = pid;
            let mut steps = 0usize;
            loop {
                path.push(cur);
                if cur == id {
                    return Err(HierError::HierarchyCycle { path });
                }
                steps += 1;
                if steps > MAX_HIERARCHY_DEPTH {
                    // Check whether an ancestor further up actually cycles back to id within
                    // model.len() before reporting depth, so a cycle is never misreported.
                    let mut extra = 0usize;
                    let limit = model.len();
                    let mut probe = cur;
                    let mut seen: BTreeSet<Uuid> = path.iter().copied().collect();
                    while extra <= limit {
                        let a = model.get(&probe).ok_or(HierError::NotFound(probe))?;
                        let Some(next) = a.parent() else { break };
                        probe = next.id;
                        path.push(probe);
                        if probe == id {
                            return Err(HierError::HierarchyCycle { path });
                        }
                        if !seen.insert(probe) {
                            return Err(HierError::HierarchyCycle { path });
                        }
                        extra += 1;
                    }
                    return Err(HierError::HierarchyTooDeep { depth: steps + 1 });
                }
                let a = model.get(&cur).ok_or(HierError::NotFound(cur))?;
                match a.parent() {
                    None => break,
                    Some(p) => {
                        if path.iter().skip(1).any(|x| *x == p.id) {
                            path.push(p.id);
                            return Err(HierError::HierarchyCycle { path });
                        }
                        cur = p.id;
                    }
                }
            }
            steps
        }
    };
    let height = subtree_height(model, id)?;
    let resulting_depth = parent_depth + 1 + height;
    if resulting_depth > MAX_HIERARCHY_DEPTH {
        return Err(HierError::HierarchyTooDeep { depth: resulting_depth });
    }
    Ok(())
}

/// Composes `id` to the root frame via `GEO.FRAME.TREE`'s `resolve(mirror, root)` (REQ-002).
///
/// Builds only `id`'s ancestor chain (at most [`MAX_HIERARCHY_DEPTH`] frames) in a
/// [`FrameTree`] and calls `FrameTree::resolve`, guaranteeing bit-identical results with
/// [`AlignmentHierarchy::compose_to_root`] while staying O(depth).
pub fn compose_to_root(model: &AlignmentModel, id: Uuid) -> Result<Transform, HierError> {
    if model.get(&id).is_none() {
        return Err(HierError::NotFound(id));
    }
    let mut chain: Vec<&Alignment> = Vec::with_capacity(MAX_HIERARCHY_DEPTH);
    let mut seen: Vec<Uuid> = Vec::with_capacity(MAX_HIERARCHY_DEPTH + 1);
    let mut cur = id;
    loop {
        if let Some(pos) = seen.iter().position(|x| *x == cur) {
            let mut path = seen.get(pos..).unwrap_or(&[]).to_vec();
            path.push(cur);
            return Err(HierError::HierarchyCycle { path });
        }
        seen.push(cur);
        if seen.len() > MAX_HIERARCHY_DEPTH {
            return Err(HierError::HierarchyTooDeep { depth: seen.len() });
        }
        let a = model.get(&cur).ok_or(HierError::NotFound(cur))?;
        chain.push(a);
        match a.parent() {
            None => break,
            Some(p) => cur = p.id,
        }
    }
    let mut tree = FrameTree::new();
    let mut parent_frame = tree.world();
    for a in chain.into_iter().rev() {
        let fid = mirror_frame_id(a.id());
        let frame = Frame {
            id: fid,
            name: mirror_frame_name(a.id()),
            kind: FrameKind::Alignment,
            parent: Some(parent_frame),
            to_parent: *a.transform(),
        };
        tree.insert(frame).map_err(HierError::Frame)?;
        parent_frame = fid;
    }
    tree.resolve(mirror_frame_id(id), tree.world()).map_err(HierError::Frame)
}

// ------------------------------------------------------------------ set_parent command

/// Target parent in `align.set_parent@1`: either `root` or another alignment ID (REQ-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParentTarget {
    Root,
    Alignment(Uuid),
}

impl ParentTarget {
    pub fn as_option(self) -> Option<Uuid> {
        match self {
            ParentTarget::Root => None,
            ParentTarget::Alignment(id) => Some(id),
        }
    }

    pub fn to_value(self) -> CValue {
        match self {
            ParentTarget::Root => CValue::Str("root".into()),
            ParentTarget::Alignment(id) => id_value(&id),
        }
    }

    pub fn parse_str(s: &str) -> Result<ParentTarget, HierError> {
        if s == "root" {
            Ok(ParentTarget::Root)
        } else {
            parse_id(s)
                .map(ParentTarget::Alignment)
                .ok_or(HierError::BadParams("parent must be 'root' or canonical id"))
        }
    }

    pub fn from_value(v: &CValue) -> Result<ParentTarget, HierError> {
        let CValue::Str(s) = v else { return Err(HierError::BadParams("parent must be a string")) };
        ParentTarget::parse_str(s)
    }
}

/// Parameters of `align.set_parent@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetParentParams {
    pub alignment: Uuid,
    pub parent: ParentTarget,
}

impl SetParentParams {
    pub fn to_value(&self) -> CValue {
        obj([("alignment", id_value(&self.alignment)), ("parent", self.parent.to_value())])
    }

    /// Reads `{ "alignment": "<uuid>", "parent": "root" | "<uuid>" }`; fails closed on any
    /// unknown key or non-canonical value.
    pub fn from_value(v: &CValue) -> Result<SetParentParams, HierError> {
        let CValue::Obj(o) = v else { return Err(HierError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "alignment" && k != "parent") {
            return Err(HierError::BadParams("unknown parameter"));
        }
        let alignment = match o.get("alignment") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(HierError::BadParams("alignment is not a canonical id"))?,
            Some(_) => return Err(HierError::BadParams("alignment is not a string")),
            None => return Err(HierError::BadParams("missing alignment")),
        };
        let parent = match o.get("parent") {
            Some(p) => ParentTarget::from_value(p)?,
            None => return Err(HierError::BadParams("missing parent")),
        };
        Ok(SetParentParams { alignment, parent })
    }

    /// Parses CLI arguments after `fid align set-parent`:
    /// `--alignment <id> --parent <id|root> [--json]`.
    pub fn from_cli_args(args: &[&str]) -> Result<SetParentParams, HierError> {
        if args.len() > MAX_CLI_ARGS {
            return Err(HierError::InputLimit("args"));
        }
        let mut alignment: Option<&str> = None;
        let mut parent: Option<&str> = None;
        let mut json = false;
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            match *arg {
                "--json" if !json => {
                    json = true;
                }
                "--json" => return Err(HierError::BadParams("repeated --json")),
                "--alignment" => {
                    if alignment.is_some() {
                        return Err(HierError::BadParams("repeated --alignment"));
                    }
                    alignment = Some(*it.next().ok_or(HierError::BadParams("--alignment needs a value"))?);
                }
                "--parent" => {
                    if parent.is_some() {
                        return Err(HierError::BadParams("repeated --parent"));
                    }
                    parent = Some(*it.next().ok_or(HierError::BadParams("--parent needs a value"))?);
                }
                _ => return Err(HierError::BadParams("unknown option or argument")),
            }
        }
        let alignment = parse_id(alignment.ok_or(HierError::BadParams("missing --alignment"))?)
            .ok_or(HierError::BadParams("alignment is not a canonical id"))?;
        let parent = ParentTarget::parse_str(parent.ok_or(HierError::BadParams("missing --parent"))?)?;
        Ok(SetParentParams { alignment, parent })
    }
}

/// Result of `align.set_parent@1`: `{ alignment, parent, marked_outdated }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetParentResult {
    pub alignment: Uuid,
    pub parent: ParentTarget,
    pub marked_outdated: u64,
}

impl SetParentResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("alignment", id_value(&self.alignment)),
            ("marked_outdated", CValue::Num(self.marked_outdated as f64)),
            ("parent", self.parent.to_value()),
        ])
    }

    pub fn to_json(&self) -> Result<Vec<u8>, HierError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| HierError::Internal("canonical json"))
    }
}

/// The single journal event written by `align.set_parent@1` (AC-4, AC-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetParentEvent {
    pub alignment: Uuid,
    pub parent: ParentTarget,
    pub parent_digest: Option<Digest>,
    pub edges: EdgesSet,
    pub marked: Option<ElementsMarked>,
}

impl SetParentEvent {
    pub fn to_value(&self) -> CValue {
        obj([
            ("alignment", id_value(&self.alignment)),
            ("edges", self.edges.to_value()),
            ("event", CValue::Str(SET_PARENT.into())),
            ("marked", self.marked.as_ref().map_or(CValue::Null, ElementsMarked::to_value)),
            ("parent", self.parent.to_value()),
            ("parent_digest", self.parent_digest.map_or(CValue::Null, |d| CValue::Str(d.as_str().into()))),
        ])
    }

    pub fn to_json(&self) -> Result<Vec<u8>, HierError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| HierError::Internal("canonical json"))
    }

    pub fn from_value(v: &CValue) -> Result<SetParentEvent, HierError> {
        let CValue::Obj(o) = v else { return Err(HierError::BadParams("event is not an object")) };
        const KEYS: [&str; 6] = ["alignment", "edges", "event", "marked", "parent", "parent_digest"];
        if o.len() != KEYS.len() || !KEYS.iter().all(|k| o.contains_key(*k)) {
            return Err(HierError::BadParams("unexpected event keys"));
        }
        match o.get("event") {
            Some(CValue::Str(s)) if s == SET_PARENT => {}
            _ => return Err(HierError::BadParams("unexpected event type")),
        }
        let alignment = match o.get("alignment") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(HierError::BadParams("bad alignment id"))?,
            _ => return Err(HierError::BadParams("bad alignment id")),
        };
        let parent = match o.get("parent") {
            Some(p) => ParentTarget::from_value(p)?,
            None => return Err(HierError::BadParams("missing parent")),
        };
        let parent_digest = match o.get("parent_digest") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(Digest::parse(s).map_err(|_| HierError::BadParams("bad parent_digest"))?),
            _ => return Err(HierError::BadParams("bad parent_digest")),
        };
        match (parent, parent_digest) {
            (ParentTarget::Root, None) | (ParentTarget::Alignment(_), Some(_)) => {}
            _ => return Err(HierError::BadParams("parent and parent_digest mismatch")),
        }
        let edges = match o.get("edges") {
            Some(ev) => EdgesSet::from_value(ev).map_err(HierError::Edge)?,
            None => return Err(HierError::BadParams("missing edges")),
        };
        if edges.element() != alignment {
            return Err(HierError::BadParams("edges element does not match alignment"));
        }
        let marked = match o.get("marked") {
            Some(CValue::Null) => None,
            Some(mv) => Some(ElementsMarked::from_value(mv).map_err(HierError::Prop)?),
            None => return Err(HierError::BadParams("missing marked")),
        };
        Ok(SetParentEvent { alignment, parent, parent_digest, edges, marked })
    }

    pub fn from_json(bytes: &[u8]) -> Result<SetParentEvent, HierError> {
        let v = parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => HierError::InputLimit("event"),
            CanonError::DuplicateKey(_) => HierError::BadParams("duplicate key"),
            _ => HierError::BadParams("event is not valid JSON"),
        })?;
        SetParentEvent::from_value(&v)
    }
}

/// Output of a committed `align.set_parent@1`: the command result and the one journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetParentCommit {
    pub result: SetParentResult,
    pub event: SetParentEvent,
}

/// Runs `align.set_parent@1` atomically (REQ-003, REQ-004, REQ-005, REQ-007).
///
/// Validates everything on staged copies first:
/// 1. `check_parent(model, alignment, parent)` (exists, acyclic, depth <= 64).
/// 2. Stages replacing `alignment`'s parent input edge in `edges` (keeping all geometric
///    inputs from `alignment.inputs()`).
/// 3. Plans downstream invalidation through `fid_dep::prop::plan` on the updated edges, and
///    includes `alignment` itself whenever its state is markable (`current` or `failed`) so
///    `alignment` and every transitive dependent become `outdated` in the same commit.
/// 4. Stages reparenting `alignment`'s mirror frame in `hier` and updating the affected
///    alignments in `model`.
/// 5. Only if every step succeeds does it commit all four structures together and return
///    the command result and one [`SetParentEvent`].
pub fn set_parent(
    model: &mut AlignmentModel,
    hier: &mut AlignmentHierarchy,
    edges: &mut EdgeIndex,
    states: &mut ElementStates,
    params: &SetParentParams,
) -> Result<SetParentCommit, HierError> {
    let id = params.alignment;
    let parent_opt = params.parent.as_option();

    // 1. Hierarchy checks (exists, cycle, depth <= 64).
    check_parent(model, id, parent_opt)?;

    let current = model.get(&id).ok_or(HierError::NotFound(id))?.clone();
    let new_parent_ref = match parent_opt {
        None => None,
        Some(pid) => {
            let p = model.get(&pid).ok_or(HierError::NotFound(pid))?;
            Some(ParentRef { id: pid, digest: p.digest() })
        }
    };

    // 2. Stage dependency edge replacement in CORE.DEP.
    let mut next_inputs: BTreeSet<Uuid> = current.inputs().iter().map(|i| i.id).collect();
    if let Some(p) = &new_parent_ref {
        next_inputs.insert(p.id);
    }
    let edges_set = EdgesSet::from_set(id, &next_inputs).map_err(HierError::Edge)?;
    let mut staged_edges = edges.clone();
    staged_edges.apply(std::slice::from_ref(&edges_set)).map_err(HierError::Edge)?;

    // 3. Plan invalidation of alignment + every transitive dependent via CORE.DEP.PROP.
    let self_state = states.get(&id).ok_or(HierError::Prop(PropError::MissingState(id)))?;
    let marking = plan(&staged_edges, states, &[id], &[]).map_err(HierError::Prop)?;
    let mut marked_set: BTreeSet<Uuid> = marking.marked().iter().copied().collect();
    if is_markable(self_state) {
        marked_set.insert(id);
    }
    let marked_event = if marked_set.is_empty() {
        None
    } else {
        Some(ElementsMarked::new(marked_set.iter().copied().collect()).map_err(HierError::Prop)?)
    };

    let mut staged_states = states.clone();
    if let Some(ev) = &marked_event {
        staged_states.apply_marked(ev).map_err(HierError::Prop)?;
    }

    // 4. Stage alignment updates and mirror frame reparenting.
    let updated_self = current.with_parent(new_parent_ref, AlignmentStatus::Outdated).map_err(HierError::Align)?;
    let mut staged_model = model.clone();
    staged_model.replace_alignment(updated_self).map_err(HierError::Align)?;
    for mid in &marked_set {
        if *mid == id {
            continue;
        }
        if let Some(dep_align) = staged_model.get(mid).cloned() {
            let outdated = dep_align.with_status(AlignmentStatus::Outdated, None).map_err(HierError::Align)?;
            staged_model.replace_alignment(outdated).map_err(HierError::Align)?;
        }
    }

    let mut staged_hier = hier.clone();
    staged_hier.reparent_mirror(id, parent_opt, *current.transform())?;

    // 5. Commit all four structures atomically.
    *model = staged_model;
    *hier = staged_hier;
    *edges = staged_edges;
    *states = staged_states;

    let marked_outdated = marked_set.len() as u64;
    let result = SetParentResult { alignment: id, parent: params.parent, marked_outdated };
    let event = SetParentEvent {
        alignment: id,
        parent: params.parent,
        parent_digest: new_parent_ref.map(|p| p.digest),
        edges: edges_set,
        marked: marked_event,
    };
    Ok(SetParentCommit { result, event })
}

/// Replays one journaled [`SetParentEvent`] deterministically (AC-5).
pub fn replay_set_parent(
    model: &mut AlignmentModel,
    hier: &mut AlignmentHierarchy,
    edges: &mut EdgeIndex,
    states: &mut ElementStates,
    event: &SetParentEvent,
) -> Result<SetParentResult, HierError> {
    let id = event.alignment;
    let parent_opt = event.parent.as_option();
    check_parent(model, id, parent_opt)?;

    let current = model.get(&id).ok_or(HierError::NotFound(id))?.clone();
    let new_parent_ref = match (parent_opt, event.parent_digest) {
        (None, None) => None,
        (Some(pid), Some(digest)) => {
            let p = model.get(&pid).ok_or(HierError::NotFound(pid))?;
            if p.digest() != digest {
                return Err(HierError::Align(AlignError::DigestMismatch));
            }
            Some(ParentRef { id: pid, digest })
        }
        _ => return Err(HierError::BadParams("parent and parent_digest mismatch")),
    };

    let mut staged_edges = edges.clone();
    staged_edges.apply(std::slice::from_ref(&event.edges)).map_err(HierError::Edge)?;

    let mut staged_states = states.clone();
    if let Some(marked) = &event.marked {
        staged_states.apply_marked(marked).map_err(HierError::Prop)?;
    }

    let updated_self = current.with_parent(new_parent_ref, AlignmentStatus::Outdated).map_err(HierError::Align)?;
    let mut staged_model = model.clone();
    staged_model.replace_alignment(updated_self).map_err(HierError::Align)?;
    if let Some(marked) = &event.marked {
        for mid in marked.elements() {
            if *mid == id {
                continue;
            }
            if let Some(dep_align) = staged_model.get(mid).cloned() {
                let outdated = dep_align.with_status(AlignmentStatus::Outdated, None).map_err(HierError::Align)?;
                staged_model.replace_alignment(outdated).map_err(HierError::Align)?;
            }
        }
    }

    let mut staged_hier = hier.clone();
    staged_hier.reparent_mirror(id, parent_opt, *current.transform())?;

    *model = staged_model;
    *hier = staged_hier;
    *edges = staged_edges;
    *states = staged_states;

    let marked_outdated = event.marked.as_ref().map_or(0, |m| m.elements().len() as u64);
    Ok(SetParentResult { alignment: id, parent: event.parent, marked_outdated })
}

/// CLI entry point for `fid align set-parent --alignment <id> --parent <id|root> [--json]`.
/// Returns canonical JSON result bytes and the single journal event; on error changes
/// nothing and returns no event.
pub fn invoke_set_parent_cli(
    model: &mut AlignmentModel,
    hier: &mut AlignmentHierarchy,
    edges: &mut EdgeIndex,
    states: &mut ElementStates,
    args: &[&str],
) -> Result<(Vec<u8>, SetParentEvent), HierError> {
    let params = SetParentParams::from_cli_args(args)?;
    let commit = set_parent(model, hier, edges, states, &params)?;
    Ok((commit.result.to_json()?, commit.event))
}

/// MCP entry point for `align_set_parent`. Parses bounded JSON parameters and runs the exact
/// same [`set_parent`] handler as [`invoke_set_parent_cli`], returning identical canonical
/// bytes.
pub fn invoke_set_parent_mcp(
    model: &mut AlignmentModel,
    hier: &mut AlignmentHierarchy,
    edges: &mut EdgeIndex,
    states: &mut ElementStates,
    params_json: &[u8],
) -> Result<(Vec<u8>, SetParentEvent), HierError> {
    let value = parse_with_limits(params_json, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => HierError::InputLimit("params"),
        CanonError::DuplicateKey(_) => HierError::BadParams("duplicate key"),
        _ => HierError::BadParams("params are not valid JSON"),
    })?;
    let params = SetParentParams::from_value(&value)?;
    let commit = set_parent(model, hier, edges, states, &params)?;
    Ok((commit.result.to_json()?, commit.event))
}

// ------------------------------------------------------------------ transform synchronization (REQ-006)

/// Outcome of updating an alignment's transform and invalidating its descendants (REQ-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformUpdated {
    pub alignment: Uuid,
    pub digest: Digest,
    pub marked: Option<ElementsMarked>,
    pub marked_outdated: u64,
}

/// Updates `id`'s transform in `model` and its mirror frame in `hier` in the same commit,
/// and marks every descendant alignment and dependent result `outdated` through CORE.DEP
/// (REQ-006). On any error, nothing is changed.
pub fn update_alignment_transform(
    model: &mut AlignmentModel,
    hier: &mut AlignmentHierarchy,
    edges: &EdgeIndex,
    states: &mut ElementStates,
    id: Uuid,
    transform: Transform,
) -> Result<TransformUpdated, HierError> {
    let current = model.get(&id).ok_or(HierError::NotFound(id))?.clone();
    let updated = current.with_transform(transform).map_err(HierError::Align)?;

    // Plan descendant and dependent invalidation via CORE.DEP.PROP.
    let marking = plan(edges, states, &[id], &[]).map_err(HierError::Prop)?;
    let marked_event = marking.event().map_err(HierError::Prop)?;

    let mut staged_states = states.clone();
    if let Some(ev) = &marked_event {
        staged_states.apply_marked(ev).map_err(HierError::Prop)?;
    }

    let mut staged_model = model.clone();
    staged_model.replace_alignment(updated.clone()).map_err(HierError::Align)?;
    if let Some(ev) = &marked_event {
        for mid in ev.elements() {
            if let Some(dep_align) = staged_model.get(mid).cloned() {
                let outdated = dep_align.with_status(AlignmentStatus::Outdated, None).map_err(HierError::Align)?;
                staged_model.replace_alignment(outdated).map_err(HierError::Align)?;
            }
        }
    }

    let mut staged_hier = hier.clone();
    staged_hier.set_mirror_transform(id, *updated.transform())?;

    *model = staged_model;
    *hier = staged_hier;
    *states = staged_states;

    let marked_outdated = marked_event.as_ref().map_or(0, |m| m.elements().len() as u64);
    Ok(TransformUpdated { alignment: id, digest: updated.digest(), marked: marked_event, marked_outdated })
}

/// Synchronizes mirror frame transforms in `hier` after an [`AlignmentModel::recompute`]
/// pass and propagates `outdated` to any dependent elements not re-solved in that pass
/// (REQ-006).
pub fn sync_recompute_entry(
    model: &AlignmentModel,
    hier: &mut AlignmentHierarchy,
    edges: &EdgeIndex,
    states: &mut ElementStates,
    entry: &RecomputeEntry,
) -> Result<Option<ElementsMarked>, HierError> {
    let mut staged_hier = hier.clone();
    let mut changed: Vec<Uuid> = Vec::new();
    let mut recomputed: Vec<Uuid> = Vec::new();
    for item in &entry.items {
        let a = model.get(&item.id).ok_or(HierError::NotFound(item.id))?;
        if item.digest_after != item.digest_before {
            staged_hier.set_mirror_transform(item.id, *a.transform())?;
            changed.push(item.id);
            recomputed.push(item.id);
        }
    }
    if changed.is_empty() {
        *hier = staged_hier;
        return Ok(None);
    }
    let marking = plan(edges, states, &changed, &recomputed).map_err(HierError::Prop)?;
    let marked = marking.event().map_err(HierError::Prop)?;
    let mut staged_states = states.clone();
    if let Some(ev) = &marked {
        staged_states.apply_marked(ev).map_err(HierError::Prop)?;
    }
    *hier = staged_hier;
    *states = staged_states;
    Ok(marked)
}

// ------------------------------------------------------------------ registry command definition

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn parent_schema(with_ref: bool) -> CValue {
    let id_variant = if with_ref {
        obj([
            ("type", text("string")),
            ("x-ref", text("alignment")),
            ("pattern", text(ID_PATTERN)),
            ("description", text("The parent alignment ID.")),
        ])
    } else {
        obj([
            ("type", text("string")),
            ("pattern", text(ID_PATTERN)),
            ("description", text("The parent alignment ID.")),
        ])
    };
    obj([
        (
            "oneOf",
            CValue::Arr(vec![
                obj([
                    ("type", text("string")),
                    ("enum", strs(&["root"])),
                    ("description", text("Make the alignment a top-level alignment under the root frame.")),
                ]),
                id_variant,
            ]),
        ),
        ("description", text("The parent alignment ID, or \"root\" for a top-level alignment.")),
    ])
}

/// The registry definition of `align.set_parent@1` (W2, D0, undo `head_move`), built through
/// [`CommandDef::from_value`].
pub fn set_parent_command_def() -> Result<CommandDef, DefError> {
    let params_schema = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["alignment", "parent"])),
        (
            "properties",
            obj([
                (
                    "alignment",
                    obj([
                        ("type", text("string")),
                        ("x-ref", text("alignment")),
                        ("pattern", text(ID_PATTERN)),
                        ("description", text("The alignment whose parent is changed.")),
                    ]),
                ),
                ("parent", parent_schema(true)),
            ]),
        ),
    ]);
    let result_schema = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["alignment", "marked_outdated", "parent"])),
        (
            "properties",
            obj([
                ("alignment", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
                (
                    "marked_outdated",
                    obj([
                        ("type", text("integer")),
                        ("minimum", CValue::Num(0.0)),
                        ("description", text("Count of elements marked outdated in this commit.")),
                    ]),
                ),
                ("parent", parent_schema(false)),
            ]),
        ),
    ]);
    CommandDef::from_value(obj([
        ("id", text(SET_PARENT_NAME)),
        ("major", CValue::Num(1.0)),
        ("title", text("Set alignment parent")),
        (
            "description",
            text(
                "Sets an alignment's parent to another alignment or root, rejecting cycles and chains deeper than 64, \
                 replacing its parent edge in CORE.DEP and marking the alignment and all transitive dependents \
                 outdated in the same commit.",
            ),
        ),
        ("params_schema", params_schema),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[SET_PARENT])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
