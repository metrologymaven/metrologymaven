//! Entity and result diff between branches (spec node `CORE.JRN.BR.DIFF`).
//!
//! [`diff_rows`] answers "what would promoting `source` into `target` change?" from the
//! journal's stored rows without re-executing anything. With feature `store`, [`diff_store`]
//! and `Journal::diff_branch` feed it from a project database, and [`command_defs`] declares
//! `journal.diff_branch@1` (R0, D0). The bus handler is in `fid-journal-cmd`.
//!
//! # Pinned rules
//!
//! - **Verified headers only.** Every row is untrusted. The rows are walked from sequence 1
//!   with the header checks and links of `CORE.JRN.CHAIN.VERIFY` (steps 1 to 14 of
//!   [`crate::verify`]): a row that fails refuses the call as `internal/invariant_violated`
//!   ([`DiffError::Corrupt`], carrying the located [`Failure`]). Payload and referenced blobs
//!   are never read, so the diff is the same with payloads removed (REQ-005). Only the
//!   verified header is read; the `branch`, `kind` and `hash` columns only have to agree with
//!   it, and the head cache (`heads`) is never read.
//! - **Limits of verification (#351).** Stored checkpoints are not authenticated, so they are
//!   not read. Without a trusted head a journal rewritten from some entry on (every later
//!   header re-hashed and re-linked) or cut at its tail cannot be told from the original;
//!   callers holding a trusted head pass it as [`DiffOptions::trusted`], which is checked as in
//!   verification (`sequence/extended`, `checkpoint/forked_head`, `checkpoint/truncated`).
//! - **Branches.** Both branches must be in the branch table (`CORE.JRN.BR.NAME`'s
//!   origin-bound fold, [`Branches`]), else `not_found/unknown_branch`; that table is the only
//!   source of a branch's existence, its `created_from` and its owner. A branch's head is the
//!   verified entry with the highest sequence whose header `branch` is the branch and whose
//!   kind is `command` or `head_move` (session headers and principal upgrades are not logical
//!   states); a branch with no such entry is at its `created_from`, which must name a verified
//!   entry (else `internal/invariant_violated`); a `created_from` naming a session header or
//!   principal upgrade resolves to the last state entry of that entry's branch at or before it
//!   (none: the empty state); `main` with neither has no head (the empty state). The table's own `head` column is not used.
//! - **Logical DAG.** Each verified entry's parent is its header `logical_parent`, which must
//!   name an entry verified before it (else `internal/invariant_violated`); so parents always
//!   have lower sequences, the DAG is a forest, and every walk ends. A `head_move` (undo,
//!   redo) is walked through its verified `head_move.to`, which must name an entry verified
//!   before it and equal its `logical_parent` (else `internal/invariant_violated`).
//! - **No writer yet.** No code on main sets `logical_parent` yet (`CORE.BUS.ROUTE` and
//!   `CORE.JRN.UNDO` will); until then every entry is a root, and two branches with different
//!   heads report `ancestor: null` with each side's own path folded. That is an obligation on
//!   ROUTE (commits and branch creation) and UNDO (head moves), recorded in the leaf. The ancestor is the first
//!   entry of the source head's parent path that is on the target head's parent path, or
//!   `null` when they do not meet (REQ-001).
//! - **Fold.** The base is the target path from the ancestor down to its root; each side is
//!   its path above the ancestor. Each is folded in sequence order over the entity-tagged
//!   output digests of `command` entries (an output without an entity is ignored; within one
//!   entry the last output for an entity wins); the tombstone digest means deleted. For every entity
//!   touched on either side, base, source and target values (`None` when absent or deleted);
//!   a side changed it when its value differs from the base. Neither changed: not listed.
//!   Only the source: `added` if the base is `None`, `removed` if the source value is `None`,
//!   else `changed`. Otherwise `changed`, `conflict` when the two values differ (REQ-002 to
//!   REQ-004). Each [`EntityDelta`] carries both values and, per side, the commands that
//!   output the entity since the ancestor in sequence order ([`CommandRef`]).
//! - **Agent or human.** A [`CommandRef`]'s principal kind is the verified header's (the
//!   entry's real principal as the bus recorded it) and a branch's owner comes from the
//!   origin-bound branch table; nothing is ever taken from an event body or a payload.
//! - **Determinism (D0).** Lists sorted by entity ID bytes, commands by sequence; the result
//!   value ([`BranchDiff::to_value`]) is canonical JSON.
//! - **Bounds.** At most [`DiffOptions::max_entries`] rows and an allocation budget of
//!   [`DiffOptions::max_bytes`] (every stored node, output, path step, fold entry and result
//!   item is charged at least its size), else `validation/input_limit`; one header in memory
//!   at a time; no recursion (`fid-canon`'s parser is depth-limited); no panics, checked
//!   arithmetic; the cancel flag is read once per row (`cancelled/requested`). The walk covers
//!   the whole stream from sequence 1 (verification needs the chain from genesis), so memory
//!   and time grow with the journal, not with the entries since the ancestor, and the spec's
//!   "10,000 entries since the ancestor in 200 ms" budget cannot hold on a long journal; with
//!   feature `store` the whole walk also runs under the store's connection mutex. A windowed
//!   walk anchored at a trusted head is a follow-up. Agents get lower bounds
//!   ([`DiffOptions::for_caller`]).

#[cfg(feature = "store")]
mod store;

#[cfg(feature = "store")]
pub use store::{command_defs, diff_store, params_schema, result_schema, COMMAND_REF_PATTERN, MAX_LIST_ITEMS};

use crate::branch::{Branches, Owner};
use crate::entry::{parse_digest, PrincipalKind, GENESIS_PREV};
use crate::verify::{check_linked, Failure, FailureKind, ReadError, StoredEntry, TrustedHead};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::mem::size_of;
use std::sync::Arc;

/// Command ID of the diff command.
pub const DIFF_BRANCH: &str = "journal.diff_branch";
/// Default bound on rows walked.
pub const DEFAULT_MAX_ENTRIES: u64 = 10_000_000;
/// Default allocation budget, in bytes.
pub const DEFAULT_MAX_BYTES: usize = 512 * 1024 * 1024;
/// Lower bound on rows walked for a call from an agent (MCP).
pub const AGENT_MAX_ENTRIES: u64 = 1_000_000;
/// Lower allocation budget for a call from an agent (MCP), in bytes.
pub const AGENT_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Bytes charged for one entry of a `BTreeMap` or `BTreeSet` on top of its key and value
/// (node and pointer overhead, rounded up).
pub const MAP_ENTRY_OVERHEAD: usize = 64;

// ------------------------------------------------------------------ options and errors

/// A cancel flag the diff reads once per row.
pub type CancelFlag = Arc<dyn Fn() -> bool + Send + Sync>;

/// Bounds, trusted head and cancellation of one diff.
#[derive(Clone)]
pub struct DiffOptions {
    pub max_entries: u64,
    pub max_bytes: usize,
    /// A head the caller trusts (module docs, Limits of verification).
    pub trusted: Option<TrustedHead>,
    pub cancel: Option<CancelFlag>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions { max_entries: DEFAULT_MAX_ENTRIES, max_bytes: DEFAULT_MAX_BYTES, trusted: None, cancel: None }
    }
}

impl DiffOptions {
    /// The default bounds for a caller of this principal kind (as the bus derived it): an
    /// agent, whose calls arrive over MCP, gets [`AGENT_MAX_ENTRIES`] and [`AGENT_MAX_BYTES`];
    /// every other caller the defaults.
    pub fn for_caller(kind: PrincipalKind) -> DiffOptions {
        match kind {
            PrincipalKind::Agent => {
                DiffOptions { max_entries: AGENT_MAX_ENTRIES, max_bytes: AGENT_MAX_BYTES, ..DiffOptions::default() }
            }
            _ => DiffOptions::default(),
        }
    }
}

impl fmt::Debug for DiffOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiffOptions")
            .field("max_entries", &self.max_entries)
            .field("max_bytes", &self.max_bytes)
            .field("trusted", &self.trusted)
            .field("cancel", &self.cancel.is_some())
            .finish()
    }
}

/// A refusal of a diff, with its bus error (catalogue v3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffError {
    /// The branch is not in the branch table (`not_found/unknown_branch`).
    UnknownBranch(String),
    /// A row fails verification (`internal/invariant_violated`).
    Corrupt(Failure),
    /// Verified headers or the branch table contradict the DAG rules: a dangling
    /// `logical_parent` or `created_from` (`internal/invariant_violated`).
    Tampered(String),
    /// Over a bound (`validation/input_limit`).
    InputLimit(String),
    /// The caller cancelled (`cancelled/requested`).
    Cancelled,
    /// Reading the rows failed: the read error's own mapping.
    Read(ReadError),
}

impl DiffError {
    /// The bus error `(code, reason)` of catalogue v3.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            DiffError::UnknownBranch(_) => ("not_found", "unknown_branch"),
            DiffError::Corrupt(_) | DiffError::Tampered(_) => ("internal", "invariant_violated"),
            DiffError::InputLimit(_) => ("validation", "input_limit"),
            DiffError::Cancelled => ("cancelled", "requested"),
            DiffError::Read(e) => e.bus_code(),
        }
    }

    /// The detail text.
    pub fn detail(&self) -> String {
        match self {
            DiffError::UnknownBranch(b) => format!("no branch named {b:?}"),
            DiffError::Corrupt(f) => f.to_string(),
            DiffError::Tampered(d) | DiffError::InputLimit(d) => d.clone(),
            DiffError::Cancelled => "cancelled".to_string(),
            DiffError::Read(e) => e.to_string(),
        }
    }
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (code, reason) = self.bus_code();
        write!(f, "{code}/{reason}: {}", self.detail())
    }
}

impl std::error::Error for DiffError {}

impl From<ReadError> for DiffError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Cancelled => DiffError::Cancelled,
            ReadError::Corrupt(f) => DiffError::Corrupt(f),
            other => DiffError::Read(other),
        }
    }
}

fn tampered(why: impl Into<String>) -> DiffError {
    DiffError::Tampered(why.into())
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

// ------------------------------------------------------------------ result

/// One command that output an entity on a side since the ancestor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRef {
    /// The entry hash.
    pub entry: Digest,
    pub seq: u64,
    /// `id@major`.
    pub command: String,
    /// The verified header's principal kind.
    pub principal: PrincipalKind,
}

impl CommandRef {
    /// `{entry, seq, command, principal}`; `seq` is a decimal string.
    pub fn to_value(&self) -> CValue {
        obj([
            ("command", text(&self.command)),
            ("entry", text(self.entry.as_str())),
            ("principal", text(self.principal.as_str())),
            ("seq", CValue::Str(self.seq.to_string())),
        ])
    }
}

/// One entity in the diff (REQ-002, REQ-003, REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityDelta {
    /// The stable entity ID, as the headers name it.
    pub id: String,
    /// The target's value, `None` when absent or deleted.
    pub target_digest: Option<Digest>,
    /// The source's value, `None` when absent or deleted.
    pub source_digest: Option<Digest>,
    pub target_commands: Vec<CommandRef>,
    pub source_commands: Vec<CommandRef>,
    /// Both sides changed it, to different values.
    pub conflict: bool,
}

fn opt_digest(d: Option<Digest>) -> CValue {
    d.map_or(CValue::Null, |d| text(d.as_str()))
}

impl EntityDelta {
    pub fn to_value(&self) -> CValue {
        let refs = |v: &[CommandRef]| CValue::Arr(v.iter().map(CommandRef::to_value).collect());
        obj([
            ("conflict", CValue::Bool(self.conflict)),
            ("id", text(&self.id)),
            ("source_commands", refs(&self.source_commands)),
            ("source_digest", opt_digest(self.source_digest)),
            ("target_commands", refs(&self.target_commands)),
            ("target_digest", opt_digest(self.target_digest)),
        ])
    }
}

/// One branch of the diff: its name, the head the diff used, and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSide {
    pub branch: String,
    pub head: Option<Digest>,
    pub owner: Owner,
}

impl BranchSide {
    pub fn to_value(&self) -> CValue {
        obj([("branch", text(&self.branch)), ("head", opt_digest(self.head)), ("owner", text(self.owner.as_str()))])
    }
}

/// The `journal.diff_branch@1` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchDiff {
    /// The nearest common ancestor, `None` when the heads share none.
    pub ancestor: Option<Digest>,
    pub source: BranchSide,
    pub target: BranchSide,
    pub added: Vec<EntityDelta>,
    pub removed: Vec<EntityDelta>,
    pub changed: Vec<EntityDelta>,
}

impl BranchDiff {
    /// `{ancestor, source, target, added, removed, changed}` (`branch-diff.schema.json`).
    pub fn to_value(&self) -> CValue {
        let list = |v: &[EntityDelta]| CValue::Arr(v.iter().map(EntityDelta::to_value).collect());
        obj([
            ("added", list(&self.added)),
            ("ancestor", opt_digest(self.ancestor)),
            ("changed", list(&self.changed)),
            ("removed", list(&self.removed)),
            ("source", self.source.to_value()),
            ("target", self.target.to_value()),
        ])
    }
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ------------------------------------------------------------------ budget

/// The command part of a node: what the fold and the result need.
struct Cmd {
    /// `id@major`.
    id: String,
    principal: PrincipalKind,
    /// Entity-tagged outputs, one per entity (the last of the entry), sorted by entity.
    outputs: Vec<(String, Digest)>,
}

/// One verified entry on the logical DAG.
struct Node {
    seq: u64,
    hash: Digest,
    /// Index of the parent node (always lower).
    parent: Option<usize>,
    /// The logical state this entry stands for: itself for a `command` or `head_move`, else
    /// the last state entry of its branch at or before it.
    state: Option<usize>,
    command: Option<Cmd>,
}

/// What one side's fold keeps per entity.
struct Fold {
    digest: Digest,
    /// Nodes that output the entity, in sequence order.
    commands: Vec<usize>,
}

/// A node: its slot in the growing node vector (twice its size, for growth) and its entry in
/// the hash index.
const NODE_COST: usize = 2 * size_of::<Node>() + size_of::<([u8; 32], usize)>() + MAP_ENTRY_OVERHEAD;
/// An output kept on a node (plus its entity bytes), and its entry in the per-entry map that
/// keeps the last output of each entity.
const OUTPUT_COST: usize = 2 * size_of::<(String, Digest)>() + MAP_ENTRY_OVERHEAD;
/// A step of a parent path: the vector slot (doubled) and the membership set entry.
const PATH_COST: usize = 2 * size_of::<usize>() + size_of::<usize>() + MAP_ENTRY_OVERHEAD;
/// A fold map entry (plus its entity bytes).
const FOLD_COST: usize = size_of::<(String, Fold)>() + MAP_ENTRY_OVERHEAD;
/// A command index in a fold entry (doubled for growth).
const FOLD_REF_COST: usize = 2 * size_of::<usize>();
/// An entry of the set of touched entities (a reference and its node).
const TOUCHED_COST: usize = size_of::<&String>() + MAP_ENTRY_OVERHEAD;
/// A result delta (doubled for growth, plus its entity bytes).
const DELTA_COST: usize = 2 * size_of::<EntityDelta>();
/// A result command reference (doubled for growth, plus its command bytes).
const REF_COST: usize = 2 * size_of::<CommandRef>();

struct Budget {
    left: usize,
}

impl Budget {
    fn charge(&mut self, base: usize, extra: usize) -> Result<(), DiffError> {
        let n = base.checked_add(extra).ok_or_else(|| DiffError::InputLimit("allocation size overflow".into()))?;
        self.left = self.left.checked_sub(n).ok_or_else(|| {
            DiffError::InputLimit("the diff needs more memory than its allocation budget allows".into())
        })?;
        Ok(())
    }
}

// ------------------------------------------------------------------ headers

type Obj = BTreeMap<String, CValue>;

fn field<'a>(o: &'a Obj, k: &str) -> Result<&'a CValue, DiffError> {
    o.get(k).ok_or_else(|| tampered(format!("verified header has no {k}")))
}

fn str_field<'a>(o: &'a Obj, k: &str) -> Result<&'a str, DiffError> {
    match field(o, k)? {
        CValue::Str(s) => Ok(s),
        _ => Err(tampered(format!("verified header.{k} is not a string"))),
    }
}

fn obj_field<'a>(o: &'a Obj, k: &str) -> Result<&'a Obj, DiffError> {
    match field(o, k)? {
        CValue::Obj(m) => Ok(m),
        _ => Err(tampered(format!("verified header.{k} is not an object"))),
    }
}

/// The command part of a verified `command` header.
fn command_of(o: &Obj, budget: &mut Budget) -> Result<Cmd, DiffError> {
    let principal = PrincipalKind::parse(str_field(obj_field(o, "principal")?, "kind")?)
        .map_err(|e| tampered(format!("verified header.principal.kind: {e}")))?;
    let c = obj_field(o, "command")?;
    let id = str_field(c, "id")?;
    let major = match field(c, "major")? {
        CValue::Num(f) if f.fract() == 0.0 && *f >= 0.0 && *f <= f64::from(u32::MAX) => *f as u32,
        CValue::U64(n) => u32::try_from(*n).map_err(|_| tampered("verified header.command.major is out of range"))?,
        _ => return Err(tampered("verified header.command.major is not an integer")),
    };
    let id = format!("{id}@{major}");
    budget.charge(0, id.len())?;
    let CValue::Arr(outs) = field(c, "outputs")? else {
        return Err(tampered("verified header.command.outputs is not an array"));
    };
    let mut last: BTreeMap<&str, Digest> = BTreeMap::new();
    for out in outs {
        let CValue::Obj(out) = out else { return Err(tampered("a verified output is not an object")) };
        let Some(entity) = out.get("entity") else { continue };
        let CValue::Str(entity) = entity else { return Err(tampered("a verified output entity is not a string")) };
        let d = parse_digest(str_field(out, "digest")?)
            .ok_or_else(|| tampered("a verified output digest is not a digest"))?;
        let d = Digest::from_bytes(d.0);
        match last.get_mut(entity.as_str()) {
            Some(slot) => *slot = d,
            None => {
                budget.charge(OUTPUT_COST, entity.len())?;
                last.insert(entity.as_str(), d);
            }
        }
    }
    let outputs = last.into_iter().map(|(e, d)| (e.to_string(), d)).collect();
    Ok(Cmd { id, principal, outputs })
}

// ------------------------------------------------------------------ the diff

/// What the walk keeps: every verified node, and the last state entry of every branch.
struct Walk {
    nodes: Vec<Node>,
    index: BTreeMap<[u8; 32], usize>,
    last_state: BTreeMap<String, usize>,
}

impl Walk {
    fn lookup(&self, d: &Digest) -> Option<usize> {
        self.index.get(&d.to_bytes()).copied()
    }

    fn node(&self, i: usize) -> Result<&Node, DiffError> {
        self.nodes.get(i).ok_or_else(|| tampered("node index out of range"))
    }

    /// The parent path of `head`, head first (strictly decreasing indices).
    fn path(&self, head: Option<usize>, budget: &mut Budget) -> Result<Vec<usize>, DiffError> {
        let mut out = Vec::new();
        let mut at = head;
        while let Some(i) = at {
            budget.charge(PATH_COST, 0)?;
            out.push(i);
            let next = self.node(i)?.parent;
            if next.is_some_and(|p| p >= i) {
                return Err(tampered("a logical parent is not below its child"));
            }
            at = next;
        }
        Ok(out)
    }

    /// Folds the path slice `nodes` (head first, so strictly decreasing; folded from its end,
    /// in sequence order, without a copy) into entity -> last value, and with `refs` the
    /// commands that output each entity.
    fn fold(&self, nodes: &[usize], refs: bool, budget: &mut Budget) -> Result<BTreeMap<String, Fold>, DiffError> {
        if nodes.windows(2).any(|w| w[0] <= w[1]) {
            return Err(tampered("a path is not in decreasing order"));
        }
        let mut out: BTreeMap<String, Fold> = BTreeMap::new();
        for &i in nodes.iter().rev() {
            let Some(cmd) = &self.node(i)?.command else { continue };
            for (entity, d) in &cmd.outputs {
                match out.get_mut(entity) {
                    Some(f) => f.digest = *d,
                    None => {
                        budget.charge(FOLD_COST, entity.len())?;
                        out.insert(entity.clone(), Fold { digest: *d, commands: Vec::new() });
                    }
                }
                if refs {
                    budget.charge(FOLD_REF_COST, 0)?;
                    if let Some(f) = out.get_mut(entity) {
                        f.commands.push(i);
                    }
                }
            }
        }
        Ok(out)
    }

    fn refs(&self, f: Option<&Fold>, budget: &mut Budget) -> Result<Vec<CommandRef>, DiffError> {
        let mut out = Vec::new();
        for &i in f.map_or(&[][..], |f| &f.commands[..]) {
            let n = self.node(i)?;
            let Some(cmd) = &n.command else { return Err(tampered("a folded node is not a command")) };
            budget.charge(REF_COST, cmd.id.len())?;
            out.push(CommandRef { entry: n.hash, seq: n.seq, command: cmd.id.clone(), principal: cmd.principal });
        }
        Ok(out)
    }
}

fn live(d: Option<Digest>) -> Option<Digest> {
    d.filter(|d| !d.is_tombstone())
}

fn cancelled(opts: &DiffOptions) -> bool {
    opts.cancel.as_ref().is_some_and(|c| c())
}

/// Walks and verifies `rows` (stored order, from sequence 1) of `stream`, keeping the DAG.
fn walk<R>(stream: &str, rows: R, opts: &DiffOptions, budget: &mut Budget) -> Result<Walk, DiffError>
where
    R: IntoIterator<Item = Result<StoredEntry, ReadError>>,
{
    let mut w = Walk { nodes: Vec::new(), index: BTreeMap::new(), last_state: BTreeMap::new() };
    let mut prev = GENESIS_PREV;
    let mut p: u64 = 1;
    let mut walked: u64 = 0;
    for row in rows {
        if cancelled(opts) {
            return Err(DiffError::Cancelled);
        }
        let row = row?;
        walked = walked.checked_add(1).ok_or_else(|| DiffError::InputLimit("entry count overflow".into()))?;
        if walked > opts.max_entries {
            return Err(DiffError::InputLimit(format!("more than {} entries", opts.max_entries)));
        }
        if let Some(t) = opts.trusted.filter(|t| t.exact && p > t.seq) {
            let detail = format!("the trusted head ends the journal at {}", t.seq);
            return Err(DiffError::Corrupt(located(stream, p, FailureKind::Sequence, "extended", detail)));
        }
        let (o, hash) = check_linked(&row, p, stream, &prev).map_err(DiffError::Corrupt)?;
        drop(row);
        if let Some(t) = opts.trusted.filter(|t| t.seq == p && t.head != hash) {
            let detail = format!("trusted head {}, entry hash {hash}", t.head);
            return Err(DiffError::Corrupt(located(stream, p, FailureKind::Checkpoint, "forked_head", detail)));
        }
        let parent = match field(&o, "logical_parent")? {
            CValue::Null => None,
            CValue::Str(s) => {
                let d = parse_digest(s).ok_or_else(|| tampered("verified logical_parent is not a digest"))?;
                let i = w.lookup(&Digest::from_bytes(d.0)).ok_or_else(|| {
                    tampered(format!("entry {p}: logical_parent {d} names no entry verified before it"))
                })?;
                Some(i)
            }
            _ => return Err(tampered("verified logical_parent is not a digest or null")),
        };
        let kind = str_field(&o, "kind")?;
        if kind == "head_move" {
            // Undo and redo walk through the verified `to`, which must be the logical parent.
            let to = parse_digest(str_field(obj_field(&o, "head_move")?, "to")?)
                .ok_or_else(|| tampered("verified head_move.to is not a digest"))?;
            let to = w
                .lookup(&Digest::from_bytes(to.0))
                .ok_or_else(|| tampered(format!("entry {p}: head_move.to {to} names no entry verified before it")))?;
            if parent != Some(to) {
                return Err(tampered(format!("entry {p}: head_move logical_parent disagrees with head_move.to")));
            }
        }
        budget.charge(NODE_COST, 0)?;
        let command = if kind == "command" { Some(command_of(&o, budget)?) } else { None };
        let i = w.nodes.len();
        let branch = str_field(&o, "branch")?;
        let is_state = kind == "command" || kind == "head_move";
        let state = if is_state {
            match w.last_state.get_mut(branch) {
                Some(slot) => *slot = i,
                None => {
                    budget.charge(FOLD_COST, branch.len())?;
                    w.last_state.insert(branch.to_string(), i);
                }
            }
            Some(i)
        } else {
            w.last_state.get(branch).copied()
        };
        let hash = Digest::from_bytes(hash.0);
        w.nodes.push(Node { seq: p, hash, parent, state, command });
        w.index.insert(hash.to_bytes(), i);
        prev = fid_canon::Digest(hash.to_bytes());
        p = p.checked_add(1).ok_or_else(|| DiffError::InputLimit("sequence overflow".into()))?;
    }
    if let Some(t) = opts.trusted.filter(|t| t.seq >= p) {
        let detail = format!("trusted head at sequence {}, journal ends before {p}", t.seq);
        return Err(DiffError::Corrupt(located(stream, t.seq, FailureKind::Checkpoint, "truncated", detail)));
    }
    Ok(w)
}

fn located(stream: &str, seq: u64, kind: FailureKind, reason: &'static str, detail: String) -> Failure {
    Failure { stream: stream.to_string(), seq: Some(seq), kind, reason, checkpoint: None, detail }
}

/// The diff of `source` against `target` over `rows` of `stream` (stored order, from sequence
/// 1), with the branch table `branches` (module documentation, Pinned rules). `Err` is a
/// refusal.
pub fn diff_rows<R>(
    stream: &str,
    rows: R,
    branches: &Branches,
    source: &str,
    target: &str,
    opts: &DiffOptions,
) -> Result<BranchDiff, DiffError>
where
    R: IntoIterator<Item = Result<StoredEntry, ReadError>>,
{
    let src = branches.get(source).ok_or_else(|| DiffError::UnknownBranch(truncate(source)))?;
    let tgt = branches.get(target).ok_or_else(|| DiffError::UnknownBranch(truncate(target)))?;
    let mut budget = Budget { left: opts.max_bytes };
    let w = walk(stream, rows, opts, &mut budget)?;

    let head_of = |b: &crate::branch::Branch| -> Result<Option<usize>, DiffError> {
        if let Some(i) = w.last_state.get(b.name.as_str()) {
            return Ok(Some(*i));
        }
        match b.created_from {
            None => Ok(None),
            Some(d) => {
                let i = w.lookup(&d).ok_or_else(|| {
                    tampered(format!("branch {} was created from {d}, which is no verified entry", b.name))
                })?;
                // A session header or principal upgrade stands for the last state of its branch.
                Ok(w.node(i)?.state)
            }
        }
    };
    let (sh, th) = (head_of(src)?, head_of(tgt)?);

    let tpath = w.path(th, &mut budget)?;
    let tset: BTreeSet<usize> = tpath.iter().copied().collect();
    let mut spath = Vec::new();
    let mut ancestor = None;
    for i in w.path(sh, &mut budget)? {
        if tset.contains(&i) {
            ancestor = Some(i);
            break;
        }
        spath.push(i);
    }
    drop(tset);
    let (tside, base) = match ancestor {
        // The target path is head first: everything above the ancestor is the target side.
        Some(a) => {
            let k = tpath.iter().position(|&i| i == a).ok_or_else(|| tampered("ancestor is off the target path"))?;
            tpath.split_at(k)
        }
        None => (&tpath[..], &[][..]),
    };

    let b = w.fold(base, false, &mut budget)?;
    let s = w.fold(&spath, true, &mut budget)?;
    let t = w.fold(tside, true, &mut budget)?;
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut touched: BTreeSet<&String> = BTreeSet::new();
    for k in s.keys().chain(t.keys()) {
        if touched.insert(k) {
            budget.charge(TOUCHED_COST, 0)?;
        }
    }
    for entity in touched {
        let bv = live(b.get(entity).map(|f| f.digest));
        let sf = s.get(entity);
        let tf = t.get(entity);
        let sv = sf.map_or(bv, |f| live(Some(f.digest)));
        let tv = tf.map_or(bv, |f| live(Some(f.digest)));
        let (sc, tc) = (sv != bv, tv != bv);
        if !sc && !tc {
            continue;
        }
        budget.charge(DELTA_COST, entity.len())?;
        let delta = EntityDelta {
            id: entity.clone(),
            target_digest: tv,
            source_digest: sv,
            target_commands: w.refs(tf, &mut budget)?,
            source_commands: w.refs(sf, &mut budget)?,
            conflict: sc && tc && sv != tv,
        };
        if sc && !tc && bv.is_none() {
            added.push(delta);
        } else if sc && !tc && sv.is_none() {
            removed.push(delta);
        } else {
            changed.push(delta);
        }
    }
    let hash_of =
        |i: Option<usize>| -> Result<Option<Digest>, DiffError> { i.map(|i| w.node(i).map(|n| n.hash)).transpose() };
    Ok(BranchDiff {
        ancestor: hash_of(ancestor)?,
        source: BranchSide { branch: src.name.as_str().to_string(), head: hash_of(sh)?, owner: src.owner() },
        target: BranchSide { branch: tgt.name.as_str().to_string(), head: hash_of(th)?, owner: tgt.owner() },
        added,
        removed,
        changed,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Every charged cost covers at least the size of what it stands for (the brief's
    /// allocation-budget rule; reviews of #177 and #180).
    #[test]
    fn costs_cover_the_real_sizes() {
        let pairs = [
            ("node", NODE_COST, 2 * size_of::<Node>() + size_of::<([u8; 32], usize)>()),
            ("output", OUTPUT_COST, size_of::<(String, Digest)>()),
            ("path", PATH_COST, 2 * size_of::<usize>()),
            ("fold", FOLD_COST, size_of::<(String, Fold)>()),
            ("fold ref", FOLD_REF_COST, size_of::<usize>()),
            ("touched", TOUCHED_COST, size_of::<&String>()),
            ("delta", DELTA_COST, size_of::<EntityDelta>()),
            ("ref", REF_COST, size_of::<CommandRef>()),
            // A node's command part is inline in the node, so the node cost covers it.
            ("command", size_of::<Node>(), size_of::<Cmd>()),
        ];
        for (what, cost, size) in pairs {
            assert!(cost >= size, "{what}: cost {cost} below size {size}");
        }
    }

    #[test]
    fn budget_refuses_past_its_limit() {
        let mut b = Budget { left: 10 };
        b.charge(4, 6).unwrap();
        assert_eq!(b.charge(1, 0).unwrap_err().bus_code(), ("validation", "input_limit"));
        assert_eq!(b.charge(usize::MAX, 1).unwrap_err().bus_code(), ("validation", "input_limit"));
    }
}
