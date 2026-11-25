//! Undo, redo, undo tree and barriers (spec node `CORE.JRN.UNDO.MOVE`).
//!
//! Undo and redo never delete journal history (CONSTITUTION §6). `journal.undo@1` (W2) and
//! `journal.redo@1` (W2) plan a `HeadMoved` event committed as a `head_move` entry (`EntryKind::HeadMove`)
//! whose `logical_parent` equals `head_move.to` (consistent with `CORE.JRN.BR.DIFF`).
//!
//! # Pinned rules
//!
//! - **Undo tree.** Every committed `command` entry is a state node keyed by its verified entry hash,
//!   linked to its `logical_parent` (resolved through any `head_move` entry if needed). A command
//!   committed after an undo sets `logical_parent` to the branch's current logical head, becoming a
//!   sibling of the undone state (`REQ-004`).
//! - **Most recently visited child.** Each state records `visited_at`, initialised to its commit
//!   sequence and updated to the sequence of any `head_move` entry whose `to` is that state. Redo
//!   without a `child` parameter selects the child with the highest `visited_at` (`REQ-002`, `AC-3`).
//! - **Barriers (all-or-nothing, `REQ-005`, `REQ-006`, `REQ-008`).** Walking `steps` (`1..=100`)
//!   backwards from the current head checks every crossed command entry before committing anything:
//!   - A W3 entry returns `UndoError::ReleasedRecord` (`precondition/released_record`).
//!   - A command declared `not_undoable` returns `UndoError::NotUndoable` (`precondition/not_undoable`).
//! - **Tree listing (`REQ-007`).** `journal.show_undo_tree@1` (R0) walks from `from` (or the branch's
//!   root state) in deterministic breadth-first order up to `depth` (`1..=1_000`, default `1_000`) and
//!   at most [`MAX_TREE_NODES`] (`1_000`) nodes per call.

#[cfg(feature = "store")]
mod defs;

#[cfg(feature = "store")]
pub use defs::{command_defs, undo_store};

use crate::branch::NAME_PATTERN;
use crate::entry::{parse_digest, EntryHeader, HeadMove, HeaderBody, GENESIS_PREV};
use crate::verify::{check_linked, Failure, ReadError, StoredEntry};
use fid_canon::CValue;
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// Command ID for undo (major 1, W2).
pub const UNDO: &str = "journal.undo";
/// Command ID for redo (major 1, W2).
pub const REDO: &str = "journal.redo";
/// Command ID for showing the undo tree (major 1, R0).
pub const SHOW_UNDO_TREE: &str = "journal.show_undo_tree";
/// Event type emitted by `journal.undo@1` and `journal.redo@1`.
pub const HEAD_MOVED: &str = "HeadMoved";
/// Default branch name.
pub const DEFAULT_BRANCH: &str = "main";
/// Minimum `steps` for undo/redo (`REQ-008`).
pub const MIN_STEPS: u32 = 1;
/// Maximum `steps` for undo/redo (`REQ-008`).
pub const MAX_STEPS: u32 = 100;
/// Maximum nodes returned by `journal.show_undo_tree@1` per call (`REQ-007`).
pub const MAX_TREE_NODES: usize = 1_000;
/// Maximum `depth` argument for `journal.show_undo_tree@1` (`REQ-007`).
pub const MAX_TREE_DEPTH: u32 = 1_000;

/// Re-exported branch name pattern for parameter validation.
pub const BRANCH_NAME_PATTERN: &str = NAME_PATTERN;

// ------------------------------------------------------------------ errors

/// Why an undo/redo/show-undo-tree call or journal fold was refused, with its bus error
/// (catalogue v3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoError {
    /// Crossing a W3 released record is refused (`precondition/released_record`, `REQ-005`).
    ReleasedRecord { entry: Digest, command: String },
    /// Crossing a command declared `not_undoable` is refused (`precondition/not_undoable`, `REQ-006`).
    NotUndoable { entry: Digest, command: String },
    /// Redo on a head with no child state (`precondition/nothing_to_redo`, `REQ-003`).
    NothingToRedo(String),
    /// Undo past the initial state of the branch (`precondition/false`).
    NothingToUndo(String),
    /// Branch has no states (`not_found/unknown_branch`).
    UnknownBranch(String),
    /// Named state/child hash is not in the undo tree (`not_found/unknown_entity`).
    UnknownState(Digest),
    /// `steps` outside `1..=100` or `depth` outside `1..=1000` (`validation/bad_range`).
    BadRange(String),
    /// Malformed parameter (`validation/schema`).
    Schema(String),
    /// Storage read failure (`internal/storage`).
    Storage(String),
    /// Corrupt or forged journal entry (`internal/invariant_violated`).
    Tampered(String),
    /// Verification failure when scanning stored rows (`internal/invariant_violated`).
    Corrupt(Failure),
}

impl UndoError {
    /// The bus error `(code, reason)` of catalogue v3.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            UndoError::ReleasedRecord { .. } => ("precondition", "released_record"),
            UndoError::NotUndoable { .. } => ("precondition", "not_undoable"),
            UndoError::NothingToRedo(_) => ("precondition", "nothing_to_redo"),
            UndoError::NothingToUndo(_) => ("precondition", "false"),
            UndoError::UnknownBranch(_) => ("not_found", "unknown_branch"),
            UndoError::UnknownState(_) => ("not_found", "unknown_entity"),
            UndoError::BadRange(_) => ("validation", "bad_range"),
            UndoError::Schema(_) => ("validation", "schema"),
            UndoError::Storage(_) => ("internal", "storage"),
            UndoError::Tampered(_) | UndoError::Corrupt(_) => ("internal", "invariant_violated"),
        }
    }

    pub fn code(&self) -> &'static str {
        self.bus_code().0
    }

    pub fn reason(&self) -> &'static str {
        self.bus_code().1
    }

    pub fn detail(&self) -> String {
        match self {
            UndoError::ReleasedRecord { entry, command } => {
                format!("cannot undo across W3 released record {entry} ({command})")
            }
            UndoError::NotUndoable { entry, command } => {
                format!("cannot undo across not_undoable entry {entry} ({command})")
            }
            UndoError::NothingToRedo(m)
            | UndoError::NothingToUndo(m)
            | UndoError::BadRange(m)
            | UndoError::Schema(m)
            | UndoError::Storage(m)
            | UndoError::Tampered(m) => m.clone(),
            UndoError::UnknownBranch(b) => format!("unknown branch {:?}", truncate(b)),
            UndoError::UnknownState(d) => format!("unknown state {d}"),
            UndoError::Corrupt(f) => f.to_string(),
        }
    }
}

impl fmt::Display for UndoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.code(), self.reason(), self.detail())
    }
}

impl std::error::Error for UndoError {}

impl From<ReadError> for UndoError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::UnknownBranch(b) => UndoError::UnknownBranch(b),
            ReadError::BadRange(m) => UndoError::BadRange(m),
            ReadError::BadParam(m) => UndoError::Schema(m),
            ReadError::Corrupt(f) => UndoError::Corrupt(f),
            other => UndoError::Storage(other.to_string()),
        }
    }
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

// ------------------------------------------------------------------ entry projection

/// Verified entry kind for building [`UndoTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoEntryKind {
    Command {
        command: String,
        major: u32,
        class: SideEffect,
        /// `true` for `undo: "head_move"`, `false` for `undo: "not_undoable"`.
        undoable: bool,
    },
    HeadMove {
        from: Digest,
        to: Digest,
    },
    SessionHeader,
    PrincipalUpgrade,
}

/// One verified journal entry fed to [`UndoTree::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoEntry {
    pub seq: u64,
    pub hash: Digest,
    pub branch: String,
    pub logical_parent: Option<Digest>,
    pub kind: UndoEntryKind,
}

impl UndoEntry {
    /// Builds an [`UndoEntry`] from a verified [`EntryHeader`] and a registry lookup telling
    /// whether `(command_id, major)` allows `head_move` undo (`false` means `not_undoable`).
    pub fn from_header<F>(header: &EntryHeader, is_undoable: F) -> Self
    where
        F: Fn(&str, u32) -> bool,
    {
        let hash = Digest::from_bytes(header.hash.0);
        let logical_parent = header.logical_parent.map(|d| Digest::from_bytes(d.0));
        let kind = match &header.body {
            HeaderBody::Command(c) => UndoEntryKind::Command {
                command: c.id.clone(),
                major: c.major,
                class: c.class,
                undoable: is_undoable(&c.id, c.major),
            },
            HeaderBody::HeadMove(hm) => {
                UndoEntryKind::HeadMove { from: Digest::from_bytes(hm.from.0), to: Digest::from_bytes(hm.to.0) }
            }
            HeaderBody::SessionHeader(_) => UndoEntryKind::SessionHeader,
            HeaderBody::PrincipalUpgrade(_) => UndoEntryKind::PrincipalUpgrade,
        };
        UndoEntry { seq: header.seq, hash, branch: header.branch.clone(), logical_parent, kind }
    }
}

// ------------------------------------------------------------------ event & views

/// The `HeadMoved` event planned by [`UndoTree::plan_undo`] and [`UndoTree::plan_redo`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadMovedEvent {
    pub branch: String,
    pub from: Digest,
    pub to: Digest,
}

impl HeadMovedEvent {
    /// The event type name (`"HeadMoved"`).
    pub const fn type_id(&self) -> &'static str {
        HEAD_MOVED
    }

    /// The event body `{branch, from, to}`.
    pub fn body(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("branch".to_string(), text(&self.branch)),
            ("from".to_string(), text(self.from.as_str())),
            ("to".to_string(), text(self.to.as_str())),
        ]))
    }

    /// The command result `{from, to}`.
    pub fn result_value(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("from".to_string(), text(self.from.as_str())),
            ("to".to_string(), text(self.to.as_str())),
        ]))
    }

    /// Converts to the journal header [`HeadMove`] payload.
    pub fn to_head_move(&self) -> HeadMove {
        HeadMove { from: fid_canon::Digest(self.from.to_bytes()), to: fid_canon::Digest(self.to.to_bytes()) }
    }
}

/// One node of the undo tree (`REQ-007`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoNode {
    pub state: Digest,
    pub branch: String,
    pub parent: Option<Digest>,
    pub children: Vec<Digest>,
    pub visited_at: u64,
    pub command: String,
    pub class: SideEffect,
    pub undoable: bool,
    pub seq: u64,
}

impl UndoNode {
    /// Wire representation `{state, parent, children, visited_at, command}` for
    /// `journal.show_undo_tree@1`.
    pub fn to_view_value(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("children".to_string(), CValue::Arr(self.children.iter().map(|d| text(d.as_str())).collect())),
            ("command".to_string(), text(&self.command)),
            ("parent".to_string(), self.parent.map_or(CValue::Null, |d| text(d.as_str()))),
            ("state".to_string(), text(self.state.as_str())),
            ("visited_at".to_string(), text(&self.visited_at.to_string())),
        ]))
    }
}

/// Result of `journal.show_undo_tree@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTreeView {
    pub nodes: Vec<UndoNode>,
}

impl UndoTreeView {
    /// Wire result `{nodes: [{state, parent, children, visited_at, command}]}`.
    pub fn to_value(&self) -> CValue {
        CValue::Obj(BTreeMap::from([(
            "nodes".to_string(),
            CValue::Arr(self.nodes.iter().map(UndoNode::to_view_value).collect()),
        )]))
    }
}

// ------------------------------------------------------------------ UndoTree

/// Projected undo tree and logical heads across branches, derived solely from verified
/// `logical_parent` links and `head_move` entries (`REQ-001`..`REQ-008`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoTree {
    nodes: BTreeMap<Digest, UndoNode>,
    heads: BTreeMap<String, Digest>,
    /// Maps a `head_move` entry hash to its target logical state `to`, so a subsequent command
    /// whose `logical_parent` points at either `to` or the `head_move` entry resolves identically.
    head_move_targets: BTreeMap<Digest, Digest>,
    seen_entries: BTreeSet<Digest>,
    last_seq: u64,
}

impl UndoTree {
    /// Creates an empty undo tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds an ordered sequence of verified entries into a new [`UndoTree`].
    pub fn fold<'a>(entries: impl IntoIterator<Item = &'a UndoEntry>) -> Result<Self, UndoError> {
        let mut tree = Self::new();
        for entry in entries {
            tree.apply(entry)?;
        }
        Ok(tree)
    }

    /// Resolves an entry hash that may be either a command state or a `head_move` entry to its
    /// underlying logical state hash.
    fn resolve_state(&self, h: Digest) -> Option<Digest> {
        if self.nodes.contains_key(&h) {
            Some(h)
        } else {
            self.head_move_targets.get(&h).copied()
        }
    }

    /// Folds one verified entry in sequence order.
    pub fn apply(&mut self, entry: &UndoEntry) -> Result<(), UndoError> {
        if entry.seq == 0 || entry.seq <= self.last_seq {
            return Err(UndoError::Tampered(format!(
                "entry {} has non-increasing sequence {} after {}",
                entry.hash, entry.seq, self.last_seq
            )));
        }
        if !self.seen_entries.insert(entry.hash) {
            return Err(UndoError::Tampered(format!("duplicate entry hash {}", entry.hash)));
        }
        self.last_seq = entry.seq;

        match &entry.kind {
            UndoEntryKind::SessionHeader | UndoEntryKind::PrincipalUpgrade => Ok(()),
            UndoEntryKind::Command { command, major, class, undoable } => {
                let parent = match entry.logical_parent {
                    None => None,
                    Some(p) => {
                        let resolved = self.resolve_state(p).ok_or_else(|| {
                            UndoError::Tampered(format!("entry {} logical_parent {p} names no prior state", entry.hash))
                        })?;
                        Some(resolved)
                    }
                };
                if let Some(p) = parent {
                    if let Some(pnode) = self.nodes.get_mut(&p) {
                        pnode.children.push(entry.hash);
                    }
                }
                let node = UndoNode {
                    state: entry.hash,
                    branch: entry.branch.clone(),
                    parent,
                    children: Vec::new(),
                    visited_at: entry.seq,
                    command: format!("{command}@{major}"),
                    class: *class,
                    undoable: *undoable,
                    seq: entry.seq,
                };
                self.nodes.insert(entry.hash, node);
                self.heads.insert(entry.branch.clone(), entry.hash);
                Ok(())
            }
            UndoEntryKind::HeadMove { from, to } => {
                let from_state = self.resolve_state(*from).ok_or_else(|| {
                    UndoError::Tampered(format!("head_move {} from {from} names no prior state", entry.hash))
                })?;
                let to_state = self.resolve_state(*to).ok_or_else(|| {
                    UndoError::Tampered(format!("head_move {} to {to} names no prior state", entry.hash))
                })?;
                if let Some(lp) = entry.logical_parent {
                    if self.resolve_state(lp) != Some(to_state) {
                        return Err(UndoError::Tampered(format!(
                            "head_move {} logical_parent {lp} disagrees with to {to}",
                            entry.hash
                        )));
                    }
                }
                if let Some(&cur_head) = self.heads.get(&entry.branch) {
                    if cur_head != from_state {
                        return Err(UndoError::Tampered(format!(
                            "head_move {} from {from_state} does not match branch {} head {cur_head}",
                            entry.hash, entry.branch
                        )));
                    }
                } else {
                    return Err(UndoError::Tampered(format!(
                        "head_move {} on branch {} with no logical head",
                        entry.hash, entry.branch
                    )));
                }
                if let Some(target) = self.nodes.get_mut(&to_state) {
                    target.visited_at = entry.seq;
                }
                self.head_move_targets.insert(entry.hash, to_state);
                self.heads.insert(entry.branch.clone(), to_state);
                Ok(())
            }
        }
    }

    /// Current logical head of `branch`, or `None` if no command has committed on it.
    pub fn head(&self, branch: &str) -> Option<Digest> {
        self.heads.get(branch).copied()
    }

    /// The `logical_parent` the next command committed on `branch` must carry (`REQ-004`).
    pub fn next_logical_parent(&self, branch: &str) -> Option<Digest> {
        self.head(branch)
    }

    /// Looks up a state node by hash.
    pub fn node(&self, state: &Digest) -> Option<&UndoNode> {
        self.nodes.get(state)
    }

    /// Number of command states in the undo tree.
    pub fn state_count(&self) -> usize {
        self.nodes.len()
    }

    /// Plans `journal.undo@1` on `branch` for `steps` (`1..=100`, default `1`).
    ///
    /// Checks every crossed state before returning (all-or-nothing, `REQ-005`, `REQ-006`, `REQ-008`).
    pub fn plan_undo(&self, branch: &str, steps: u32) -> Result<HeadMovedEvent, UndoError> {
        check_steps(steps)?;
        let start = self.head(branch).ok_or_else(|| UndoError::UnknownBranch(branch.to_string()))?;
        let mut cur = start;
        for _ in 0..steps {
            let node = self.nodes.get(&cur).ok_or(UndoError::UnknownState(cur))?;
            // Barrier checks (REQ-005, REQ-006): check W3 first, then not_undoable.
            if node.class == SideEffect::W3 {
                return Err(UndoError::ReleasedRecord { entry: cur, command: node.command.clone() });
            }
            if !node.undoable {
                return Err(UndoError::NotUndoable { entry: cur, command: node.command.clone() });
            }
            let Some(parent) = node.parent else {
                return Err(UndoError::NothingToUndo(format!(
                    "state {cur} on branch {:?} has no logical parent to undo to",
                    truncate(branch)
                )));
            };
            cur = parent;
        }
        Ok(HeadMovedEvent { branch: branch.to_string(), from: start, to: cur })
    }

    /// Plans `journal.redo@1` on `branch` to `child` (if given, for the first step) or the most
    /// recently visited child (`REQ-002`, `REQ-003`, `REQ-008`).
    pub fn plan_redo(&self, branch: &str, child: Option<Digest>, steps: u32) -> Result<HeadMovedEvent, UndoError> {
        check_steps(steps)?;
        let start = self.head(branch).ok_or_else(|| UndoError::UnknownBranch(branch.to_string()))?;
        let mut cur = start;
        for step in 0..steps {
            let node = self.nodes.get(&cur).ok_or(UndoError::UnknownState(cur))?;
            let next = if step == 0 {
                if let Some(want) = child {
                    if !self.nodes.contains_key(&want) {
                        return Err(UndoError::UnknownState(want));
                    }
                    if !node.children.contains(&want) {
                        return Err(UndoError::NothingToRedo(format!(
                            "state {want} is not a child of current head {cur}"
                        )));
                    }
                    want
                } else {
                    self.pick_most_recent_child(node)?
                }
            } else {
                self.pick_most_recent_child(node)?
            };
            cur = next;
        }
        Ok(HeadMovedEvent { branch: branch.to_string(), from: start, to: cur })
    }

    fn pick_most_recent_child(&self, node: &UndoNode) -> Result<Digest, UndoError> {
        if node.children.is_empty() {
            return Err(UndoError::NothingToRedo(format!("state {} has no child states", node.state)));
        }
        let mut best: Option<&UndoNode> = None;
        for ch in &node.children {
            let cn = self.nodes.get(ch).ok_or(UndoError::UnknownState(*ch))?;
            let better = match best {
                None => true,
                Some(b) => {
                    (cn.visited_at, cn.seq) > (b.visited_at, b.seq)
                        || ((cn.visited_at, cn.seq) == (b.visited_at, b.seq) && cn.state < b.state)
                }
            };
            if better {
                best = Some(cn);
            }
        }
        best.map(|n| n.state)
            .ok_or_else(|| UndoError::NothingToRedo(format!("state {} has no child states", node.state)))
    }

    /// Returns the undo tree from `from` (or the root of `branch`'s current head) up to `depth`
    /// (`1..=1_000`, default `1_000`) and at most [`MAX_TREE_NODES`] (`1_000`) nodes (`REQ-007`).
    pub fn show_tree(&self, branch: &str, from: Option<Digest>, depth: Option<u32>) -> Result<UndoTreeView, UndoError> {
        let max_depth = match depth {
            None => MAX_TREE_DEPTH,
            Some(d) if (1..=MAX_TREE_DEPTH).contains(&d) => d,
            Some(d) => {
                return Err(UndoError::BadRange(format!("depth {d} must be between 1 and {MAX_TREE_DEPTH}")));
            }
        };

        let root = match from {
            Some(h) => self.resolve_state(h).ok_or(UndoError::UnknownState(h))?,
            None => {
                let head = self.head(branch).ok_or_else(|| UndoError::UnknownBranch(branch.to_string()))?;
                self.root_of(head)?
            }
        };

        let mut out = Vec::new();
        let mut visited = BTreeSet::new();
        let mut queue: VecDeque<(Digest, u32)> = VecDeque::new();
        visited.insert(root);
        queue.push_back((root, 0));

        while let Some((state, d)) = queue.pop_front() {
            if out.len() >= MAX_TREE_NODES {
                break;
            }
            let node = self.nodes.get(&state).ok_or(UndoError::UnknownState(state))?;
            out.push(node.clone());
            if d < max_depth {
                for &child in &node.children {
                    if visited.insert(child) {
                        queue.push_back((child, d + 1));
                    }
                }
            }
        }

        Ok(UndoTreeView { nodes: out })
    }

    fn root_of(&self, start: Digest) -> Result<Digest, UndoError> {
        let mut cur = start;
        let mut guard = 0usize;
        while let Some(node) = self.nodes.get(&cur) {
            match node.parent {
                Some(p) => {
                    guard += 1;
                    if guard > self.nodes.len() {
                        return Err(UndoError::Tampered("cycle in logical_parent links".into()));
                    }
                    cur = p;
                }
                None => return Ok(cur),
            }
        }
        Err(UndoError::UnknownState(cur))
    }
}

fn check_steps(steps: u32) -> Result<(), UndoError> {
    if !(MIN_STEPS..=MAX_STEPS).contains(&steps) {
        return Err(UndoError::BadRange(format!("steps {steps} must be between {MIN_STEPS} and {MAX_STEPS}")));
    }
    Ok(())
}

// ------------------------------------------------------------------ verified row fold

/// Walks and verifies stored journal rows (in `seq` order from sequence 1) of `stream` and builds
/// their [`UndoTree`], checking every entry hash and chain link via [`check_linked`].
pub fn undo_rows<R, F>(stream: &str, rows: R, is_undoable: F) -> Result<UndoTree, UndoError>
where
    R: IntoIterator<Item = Result<StoredEntry, ReadError>>,
    F: Fn(&str, u32) -> bool,
{
    let mut tree = UndoTree::new();
    let mut prev = GENESIS_PREV;
    let mut seq: u64 = 1;
    for row in rows {
        let row = row?;
        let (o, hash) = check_linked(&row, seq, stream, &prev).map_err(UndoError::Corrupt)?;
        let branch = str_field(&o, "branch")?.to_string();
        let logical_parent = match o.get("logical_parent") {
            Some(CValue::Null) | None => None,
            Some(CValue::Str(s)) => {
                let d = parse_digest(s).ok_or_else(|| UndoError::Tampered("bad logical_parent digest".into()))?;
                Some(Digest::from_bytes(d.0))
            }
            _ => return Err(UndoError::Tampered("logical_parent is not string or null".into())),
        };
        let kind_str = str_field(&o, "kind")?;
        let kind = match kind_str {
            "command" => {
                let c = obj_field(&o, "command")?;
                let id = str_field(c, "id")?.to_string();
                let major = match c.get("major") {
                    Some(CValue::Num(n)) if *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX) => *n as u32,
                    _ => return Err(UndoError::Tampered("command.major is not a u32".into())),
                };
                let class = SideEffect::parse(str_field(c, "class")?)
                    .map_err(|_| UndoError::Tampered("unknown command.class".into()))?;
                let undoable = is_undoable(&id, major);
                UndoEntryKind::Command { command: id, major, class, undoable }
            }
            "head_move" => {
                let hm = obj_field(&o, "head_move")?;
                let from = parse_digest(str_field(hm, "from")?)
                    .ok_or_else(|| UndoError::Tampered("bad head_move.from".into()))?;
                let to =
                    parse_digest(str_field(hm, "to")?).ok_or_else(|| UndoError::Tampered("bad head_move.to".into()))?;
                UndoEntryKind::HeadMove { from: Digest::from_bytes(from.0), to: Digest::from_bytes(to.0) }
            }
            "session_header" => UndoEntryKind::SessionHeader,
            "principal_upgrade" => UndoEntryKind::PrincipalUpgrade,
            other => return Err(UndoError::Tampered(format!("unknown entry kind {other:?}"))),
        };

        let entry = UndoEntry { seq, hash: Digest::from_bytes(hash.0), branch, logical_parent, kind };
        tree.apply(&entry)?;
        prev = hash;
        seq = seq.checked_add(1).ok_or_else(|| UndoError::Tampered("sequence overflow".into()))?;
    }
    Ok(tree)
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, UndoError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(UndoError::Tampered(format!("verified header missing string field {k}"))),
    }
}

fn obj_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a BTreeMap<String, CValue>, UndoError> {
    match o.get(k) {
        Some(CValue::Obj(m)) => Ok(m),
        _ => Err(UndoError::Tampered(format!("verified header missing object field {k}"))),
    }
}
