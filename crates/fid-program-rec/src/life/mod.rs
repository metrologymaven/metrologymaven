//! Lifecycle states and content identity for Command Programs and scripts (spec node
//! `AUTO.REC.LIFE`, REQ-AUTO.REC.LIFE-001 .. 006, REQ-AUTO.REC-005/006/007/009).
//!
//! One state machine for both artifact kinds ([`ArtifactKind`]):
//!
//! ```text
//! draft --register--> registered --release--> released --deprecate--> deprecated
//!   ^                      |
//!   +-----unregister-------+
//! ```
//!
//! Every other transition is `illegal_transition` ([`transition_allowed`]).
//!
//! - [`LifeStore`]: the lifecycle state of one journal branch. Per artifact lineage (a program
//!   ID or a script ID) it keeps the versions, numbered 1, 2, ... and never reused, each with
//!   its state, its frozen canonical bytes, their SHA-256 digest (the version's identity) and
//!   the digest of the version its draft was copied from. A lineage also has a draft that is
//!   either open (editable) or closed (frozen by `register`).
//! - Query API for `AUTO.RUN` and `AUTO.SCRIPTED`: [`LifeStore::lifecycle`],
//!   [`LifeStore::versions`], [`LifeStore::by_digest`], [`LifeStore::run_gate`],
//!   [`LifeStore::frozen_bytes`].
//! - Edit API for `AUTO.PROG.EDIT.OPS`: [`LifeStore::guard_edit`] (`immutable` unless the
//!   lineage has an open draft). A new draft that copies a frozen version and records its
//!   digest as the source (REQ-005) is opened only by the journaled, authorised command
//!   `program.open_draft@1` / `script.open_draft@1`; the edit path then copies
//!   [`LifeStore::frozen_bytes`] into the project draft.
//! - [`command`]: the ten commands `program.{register,unregister,release,deprecate,open_draft}@1`
//!   and `script.{...}@1`, with their CLI and MCP entry points. `register`, `unregister` and
//!   `open_draft` are W2; `release` and `deprecate` are W3, `human_only` and request-only on
//!   MCP.
//! - [`event`]: the journal event of a transition; [`mod@replay`]: exact replay.
//! - [`bundle`]: the canonical form of a script bundle.
//!
//! **Who may do what.** The principal is the bus-derived [`fid_bus::principal::Principal`];
//! no parameter names or changes its kind. `release` and `deprecate` run only for a `human`
//! principal (the bus class check for W3, then the kind itself). A person writes lifecycle
//! state only off agent branches; every non-human principal (agent, script, service, device,
//! system) writes it only on an agent branch `agent/<client>/<n>`. Every refusal commits
//! nothing.
//!
//! **Durable content.** A `register` event carries the frozen canonical bytes themselves
//! ([`LifeEvent::content`]), so the journal holds every version's bytes and [`replay()`]
//! rebuilds the whole store, frozen bytes included, from the journal alone. The in-memory
//! [`LifeStore`] is a derived view of the journal.
//!
//! **One identity, one state.** A version cannot be registered again under a new number while
//! any identified (registered, released or deprecated) version of its lineage has the same
//! digest (`conflict`), so a new draft must actually change the content; a deprecation can
//! never be undone by re-registering the same bytes. Queries by digest also take the most
//! restrictive state if digests ever repeat (deprecated first).
//!
//! **Fail closed.** A transition changes the store only after its journal entry is committed.
//! A release is refused unless `program.check@1` passes and `AUTO.REC.ADOPT` reports no
//! unadopted work; without a wired checker ([`CheckerUnavailable`]) every program release is
//! `check_failed`.
//!
//! No floating point is used in decisions (D0), and nothing here panics.

pub mod bundle;
pub mod command;
pub mod event;
pub mod replay;

pub use bundle::ScriptBundle;
pub use command::{
    cli_params, declarations, invoke_cli, invoke_mcp, mcp_params, Drafts, LifeCall, LifeCtx, LifeJournal, LifeRequest,
    LifeRequests, LifeResult, McpReply, ProgramLifeParams, ScriptLifeParams, Verb,
};
pub use event::LifeEvent;
pub use replay::{replay, LifeReplayed, ReplayedLifecycle};

use crate::adopt::ScriptId;
use fid_program::{Program, Uuid7};
use fid_registry::FidSchema;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Most versions one lineage may have.
pub const MAX_VERSIONS: u32 = 100_000;
/// Longest note, in bytes.
pub const MAX_NOTE_BYTES: usize = 1024;

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

// ------------------------------------------------------------------ kinds, states, IDs

/// The two artifact kinds that share the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    /// A Command Program (AUTO.PROG.FMT).
    Program,
    /// A script bundle ([`ScriptBundle`]).
    Script,
}

impl ArtifactKind {
    /// `program` or `script`: the command domain and CLI group.
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Program => "program",
            ArtifactKind::Script => "script",
        }
    }
}

/// A lifecycle state.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LifeState {
    /// Editable; not identified.
    Draft,
    /// Frozen and identified by its digest; usable, not yet released.
    Registered,
    /// Released by a person; immutable.
    Released,
    /// Deprecated by a person; bytes and run records unchanged.
    Deprecated,
}

impl LifeState {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            LifeState::Draft => "draft",
            LifeState::Registered => "registered",
            LifeState::Released => "released",
            LifeState::Deprecated => "deprecated",
        }
    }

    /// Reads a wire name.
    pub fn parse(s: &str) -> Option<LifeState> {
        match s {
            "draft" => Some(LifeState::Draft),
            "registered" => Some(LifeState::Registered),
            "released" => Some(LifeState::Released),
            "deprecated" => Some(LifeState::Deprecated),
            _ => None,
        }
    }
}

/// The transition table (REQ-001): draft → registered, registered → draft, registered →
/// released and released → deprecated. Everything else, including staying in a state, is
/// refused. One explicit arm per allowed pair; no other pair is allowed.
pub fn transition_allowed(from: LifeState, to: LifeState) -> bool {
    matches!(
        (from, to),
        (LifeState::Draft, LifeState::Registered)
            | (LifeState::Registered, LifeState::Draft)
            | (LifeState::Registered, LifeState::Released)
            | (LifeState::Released, LifeState::Deprecated)
    )
}

/// An artifact lineage: a program ID or a script ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactId {
    /// A program, by its `program_id`.
    Program(Uuid7),
    /// A script, by its script ID.
    Script(ScriptId),
}

impl ArtifactId {
    /// Reads an ID of `kind`; anything else is `bad_params`.
    pub fn parse(kind: ArtifactKind, s: &str) -> Result<ArtifactId, LifeError> {
        match kind {
            ArtifactKind::Program => {
                Uuid7::parse(s).map(ArtifactId::Program).ok_or(LifeError::bad("program is not a step-format UUIDv7"))
            }
            ArtifactKind::Script => {
                ScriptId::new(s).map(ArtifactId::Script).map_err(|_| LifeError::bad("script is not a valid script ID"))
            }
        }
    }

    /// The artifact kind.
    pub fn kind(&self) -> ArtifactKind {
        match self {
            ArtifactId::Program(_) => ArtifactKind::Program,
            ArtifactId::Script(_) => ArtifactKind::Script,
        }
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactId::Program(p) => write!(f, "{p}"),
            ArtifactId::Script(s) => f.write_str(s.as_str()),
        }
    }
}

/// The lifecycle of one version, as the query API reports it (spec Design `Lifecycle`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    /// The lineage (its kind is [`ArtifactId::kind`]).
    pub artifact_id: ArtifactId,
    /// The version's state. A version that was unregistered is `draft` again: its number is
    /// retired and it no longer identifies anything.
    pub state: LifeState,
    /// The version number in the lineage, from 1.
    pub version: u32,
    /// SHA-256 of the frozen canonical bytes.
    pub digest: Digest,
    /// The digest of the version the draft was copied from, if any (REQ-005).
    pub source_digest: Option<Digest>,
}

/// What a run of an artifact version may expect (for `AUTO.RUN.BIND` and `AUTO.SCRIPTED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunGate {
    /// A registered or released version.
    Runnable {
        /// The version number.
        version: u32,
        /// `registered` or `released`.
        state: LifeState,
    },
    /// A deprecated version: new runs are refused unless a person overrides (REQ-AUTO.REC-009,
    /// enforced by `AUTO.RUN.BIND`).
    Deprecated {
        /// The version number.
        version: u32,
    },
    /// No registered, released or deprecated version has this digest.
    NotIdentified,
}

// ------------------------------------------------------------------ errors

/// Errors of the lifecycle commands. [`LifeError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeError {
    /// `bad_params{reason}`.
    BadParams {
        /// Static reason.
        reason: &'static str,
    },
    /// `policy_refused{reason}`: the principal or the branch may not run this command.
    PolicyRefused {
        /// Bus policy reason (`class_forbidden`, `human_only`) or `agent_branch` (a person on
        /// an agent branch) or `not_agent_branch` (a non-human off its agent branch).
        reason: &'static str,
    },
    /// `artifact_not_found{artifact}`.
    ArtifactNotFound {
        /// The artifact ID.
        artifact: String,
    },
    /// `illegal_transition{artifact, from, to}` (REQ-001).
    IllegalTransition {
        /// The artifact ID.
        artifact: String,
        /// The current state.
        from: LifeState,
        /// The requested state.
        to: LifeState,
    },
    /// `immutable{artifact}`: an edit aimed at a registered, released or deprecated artifact
    /// (REQ-005).
    Immutable {
        /// The artifact ID.
        artifact: String,
    },
    /// `check_failed{diagnostics}`: the program fails `program.check@1` (REQ-004).
    CheckFailed {
        /// The checker's diagnostics.
        diagnostics: Vec<String>,
    },
    /// `unadopted_steps{steps}`: agent-authored steps nobody adopted (REQ-004), in document
    /// order.
    UnadoptedSteps {
        /// The step IDs.
        steps: Vec<String>,
    },
    /// `unadopted_version{version}`: the script version is agent work nobody adopted.
    UnadoptedVersion {
        /// The script version digest.
        version: String,
    },
    /// `conflict{detail}`: the expected digest does not match, or a draft is already open.
    Conflict {
        /// What differs.
        detail: String,
    },
    /// `journal{reason}`: the transition could not be journaled; nothing changed.
    Journal {
        /// Display-safe reason from the journal seam.
        reason: String,
    },
    /// `request_failed{reason}`: an MCP request could not be filed.
    RequestFailed {
        /// Display-safe reason from the request seam.
        reason: String,
    },
    /// `internal{reason}`.
    Internal {
        /// Static reason.
        reason: &'static str,
    },
}

impl LifeError {
    pub(crate) fn bad(reason: &'static str) -> LifeError {
        LifeError::BadParams { reason }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            LifeError::BadParams { .. } => "bad_params",
            LifeError::PolicyRefused { .. } => "policy_refused",
            LifeError::ArtifactNotFound { .. } => "artifact_not_found",
            LifeError::IllegalTransition { .. } => "illegal_transition",
            LifeError::Immutable { .. } => "immutable",
            LifeError::CheckFailed { .. } => "check_failed",
            LifeError::UnadoptedSteps { .. } => "unadopted_steps",
            LifeError::UnadoptedVersion { .. } => "unadopted_version",
            LifeError::Conflict { .. } => "conflict",
            LifeError::Journal { .. } => "journal",
            LifeError::RequestFailed { .. } => "request_failed",
            LifeError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for LifeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LifeError::BadParams { reason } => write!(f, "bad_params: {reason}"),
            LifeError::PolicyRefused { reason } => write!(f, "policy_refused: {reason}"),
            LifeError::ArtifactNotFound { artifact } => write!(f, "artifact_not_found: {artifact}"),
            LifeError::IllegalTransition { artifact, from, to } => {
                write!(f, "illegal_transition: {artifact} is {}, cannot become {}", from.as_str(), to.as_str())
            }
            LifeError::Immutable { artifact } => write!(f, "immutable: {artifact}"),
            LifeError::CheckFailed { diagnostics } => write!(f, "check_failed: {}", diagnostics.join("; ")),
            LifeError::UnadoptedSteps { steps } => write!(f, "unadopted_steps: {}", steps.join(",")),
            LifeError::UnadoptedVersion { version } => write!(f, "unadopted_version: {version}"),
            LifeError::Conflict { detail } => write!(f, "conflict: {detail}"),
            LifeError::Journal { reason } => write!(f, "journal: {reason}"),
            LifeError::RequestFailed { reason } => write!(f, "request_failed: {reason}"),
            LifeError::Internal { reason } => write!(f, "internal: {reason}"),
        }
    }
}

impl std::error::Error for LifeError {}

// ------------------------------------------------------------------ program check seam

/// The verdict of `program.check@1` on a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckVerdict {
    /// No check errors.
    Passed,
    /// Check errors, as display text.
    Failed {
        /// The diagnostics.
        diagnostics: Vec<String>,
    },
}

/// The static program checker `program.check@1` (`AUTO.PROG.CHECK`), wired by the host.
pub trait ProgramChecker {
    /// Checks `program`.
    fn check(&self, program: &Program) -> CheckVerdict;
}

/// The checker to wire while `AUTO.PROG.CHECK` is not built: every program fails, so no
/// program can be released without a real check (fail closed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckerUnavailable;

impl ProgramChecker for CheckerUnavailable {
    fn check(&self, _program: &Program) -> CheckVerdict {
        CheckVerdict::Failed { diagnostics: vec!["program.check@1 is not available".to_owned()] }
    }
}

// ------------------------------------------------------------------ the store

/// One version of a lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    state: LifeState,
    digest: Digest,
    source: Option<Digest>,
    bytes: Vec<u8>,
    /// Journal entry that registered it.
    entry: u64,
}

/// The open draft of a lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Draft {
    source: Option<Digest>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Lineage {
    /// Index `i` is version `i + 1`.
    versions: Vec<Version>,
    /// `Some` while a draft is open (editable).
    draft: Option<Draft>,
}

/// The lifecycle state of one journal branch. A lineage with no record has an open draft and
/// no versions. Versions and their frozen bytes are never removed or rewritten; only a
/// version's state changes, and only along [`transition_allowed`], through a committed
/// command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LifeStore {
    lineages: BTreeMap<ArtifactId, Lineage>,
}

/// One change a committed command applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    Register { id: ArtifactId, digest: Digest, source: Option<Digest>, bytes: Vec<u8> },
    SetState { id: ArtifactId, version: u32, to: LifeState },
    OpenDraft { id: ArtifactId, source: Digest },
}

/// Restrictiveness of an identified state for runs: deprecated over released over registered.
/// A retired (`draft`) version identifies nothing.
fn restriction(state: LifeState) -> Option<u8> {
    match state {
        LifeState::Draft => None,
        LifeState::Registered => Some(1),
        LifeState::Released => Some(2),
        LifeState::Deprecated => Some(3),
    }
}

/// The identified version with `digest` whose state is the most restrictive (newest among
/// equals), with its index.
fn most_restrictive<'a>(versions: &'a [Version], digest: &Digest) -> Option<(usize, &'a Version)> {
    let mut best: Option<(u8, usize, &Version)> = None;
    for (i, v) in versions.iter().enumerate() {
        if v.digest != *digest {
            continue;
        }
        let Some(r) = restriction(v.state) else { continue };
        if best.map_or(true, |(b, _, _)| r >= b) {
            best = Some((r, i, v));
        }
    }
    best.map(|(_, i, v)| (i, v))
}

impl LifeStore {
    /// An empty store.
    pub fn new() -> LifeStore {
        LifeStore::default()
    }

    fn version_ref(&self, id: &ArtifactId, version: u32) -> Option<&Version> {
        let i = usize::try_from(version.checked_sub(1)?).ok()?;
        self.lineages.get(id)?.versions.get(i)
    }

    fn lifecycle_of(id: &ArtifactId, i: usize, v: &Version) -> Lifecycle {
        Lifecycle {
            artifact_id: id.clone(),
            state: v.state,
            version: u32::try_from(i + 1).unwrap_or(u32::MAX),
            digest: v.digest,
            source_digest: v.source,
        }
    }

    /// The lifecycle of version `version` of `id`.
    pub fn lifecycle(&self, id: &ArtifactId, version: u32) -> Option<Lifecycle> {
        let i = usize::try_from(version.checked_sub(1)?).ok()?;
        let v = self.lineages.get(id)?.versions.get(i)?;
        Some(LifeStore::lifecycle_of(id, i, v))
    }

    /// Every version of `id`, oldest first.
    pub fn versions(&self, id: &ArtifactId) -> Vec<Lifecycle> {
        self.lineages
            .get(id)
            .map(|l| l.versions.iter().enumerate().map(|(i, v)| LifeStore::lifecycle_of(id, i, v)).collect())
            .unwrap_or_default()
    }

    /// The identified version of `id` with this digest: a registered, released or deprecated
    /// one. If several have it, the most restrictive state wins (deprecated, then released,
    /// then registered; the newest among equals). A retired (unregistered) version identifies
    /// nothing.
    pub fn by_digest(&self, id: &ArtifactId, digest: &Digest) -> Option<Lifecycle> {
        let l = self.lineages.get(id)?;
        let (i, v) = most_restrictive(&l.versions, digest)?;
        Some(LifeStore::lifecycle_of(id, i, v))
    }

    /// What a run of `id` at `digest` may expect.
    pub fn run_gate(&self, id: &ArtifactId, digest: &Digest) -> RunGate {
        match self.by_digest(id, digest) {
            Some(Lifecycle { state: LifeState::Deprecated, version, .. }) => RunGate::Deprecated { version },
            Some(Lifecycle { state: s @ (LifeState::Registered | LifeState::Released), version, .. }) => {
                RunGate::Runnable { version, state: s }
            }
            Some(Lifecycle { state: LifeState::Draft, .. }) | None => RunGate::NotIdentified,
        }
    }

    /// The frozen canonical bytes of version `version` of `id`.
    pub fn frozen_bytes(&self, id: &ArtifactId, version: u32) -> Option<&[u8]> {
        self.version_ref(id, version).map(|v| v.bytes.as_slice())
    }

    /// Whether `id` has an open (editable) draft. A lineage with no record has one.
    pub fn draft_open(&self, id: &ArtifactId) -> bool {
        self.lineages.get(id).map_or(true, |l| l.draft.is_some())
    }

    /// The source digest of the open draft of `id`, if it was copied from a version.
    pub fn draft_source(&self, id: &ArtifactId) -> Option<Digest> {
        self.lineages.get(id).and_then(|l| l.draft).and_then(|d| d.source)
    }

    /// The edit guard every edit command calls before changing `id` (REQ-005): `immutable`
    /// unless `id` has an open draft. Registered, released and deprecated versions are never
    /// editable; editing starts from a new draft opened by `program.open_draft@1` /
    /// `script.open_draft@1`.
    pub fn guard_edit(&self, id: &ArtifactId) -> Result<(), LifeError> {
        if self.draft_open(id) {
            Ok(())
        } else {
            Err(LifeError::Immutable { artifact: id.to_string() })
        }
    }

    /// Applies a committed change. Crate-internal: only the commands reach it, after their
    /// journal entry is committed, and replay's comparison-only copy.
    pub(crate) fn apply(&mut self, change: &Change, entry: u64) {
        match change {
            Change::Register { id, digest, source, bytes } => {
                let l = self.lineages.entry(id.clone()).or_default();
                l.versions.push(Version {
                    state: LifeState::Registered,
                    digest: *digest,
                    source: *source,
                    bytes: bytes.clone(),
                    entry,
                });
                l.draft = None;
            }
            Change::OpenDraft { id, source } => {
                let l = self.lineages.entry(id.clone()).or_default();
                l.draft = Some(Draft { source: Some(*source) });
            }
            Change::SetState { id, version, to } => {
                let Some(l) = self.lineages.get_mut(id) else { return };
                let Some(i) = version.checked_sub(1).and_then(|i| usize::try_from(i).ok()) else { return };
                let Some(v) = l.versions.get_mut(i) else { return };
                v.state = *to;
                if *to == LifeState::Draft {
                    l.draft = Some(Draft { source: v.source });
                }
            }
        }
    }

    /// The journal entry that registered version `version` of `id`.
    pub fn registered_in(&self, id: &ArtifactId, version: u32) -> Option<u64> {
        self.version_ref(id, version).map(|v| v.entry)
    }

    /// Read access for prepare: the lineage of `id`, if any.
    pub(crate) fn lineage_view(&self, id: &ArtifactId) -> LineageView<'_> {
        LineageView { lineage: self.lineages.get(id) }
    }
}

/// Read-only view of one lineage, for the command checks.
pub(crate) struct LineageView<'a> {
    lineage: Option<&'a Lineage>,
}

impl LineageView<'_> {
    pub(crate) fn exists(&self) -> bool {
        self.lineage.is_some_and(|l| !l.versions.is_empty())
    }

    pub(crate) fn draft_open(&self) -> bool {
        self.lineage.map_or(true, |l| l.draft.is_some())
    }

    pub(crate) fn draft_source(&self) -> Option<Digest> {
        self.lineage.and_then(|l| l.draft).and_then(|d| d.source)
    }

    pub(crate) fn next_version(&self) -> Option<u32> {
        let n = self.lineage.map_or(0, |l| l.versions.len());
        u32::try_from(n).ok().and_then(|n| n.checked_add(1)).filter(|n| *n <= MAX_VERSIONS)
    }

    /// The newest version's state (for an `illegal_transition` on register).
    pub(crate) fn head_state(&self) -> Option<LifeState> {
        self.lineage.and_then(|l| l.versions.iter().rev().find(|v| v.state != LifeState::Draft)).map(|v| v.state)
    }

    /// The identified version with this digest (most restrictive state): (number, state).
    pub(crate) fn identified(&self, digest: &Digest) -> Option<(u32, LifeState)> {
        let (i, v) = most_restrictive(&self.lineage?.versions, digest)?;
        Some((u32::try_from(i + 1).ok()?, v.state))
    }

    /// The newest version with this digest, any state: (number, state, source, bytes).
    pub(crate) fn find(&self, digest: &Digest) -> Option<(u32, LifeState, Option<Digest>, &[u8])> {
        let l = self.lineage?;
        let (i, v) = l.versions.iter().enumerate().rev().find(|(_, v)| v.digest == *digest)?;
        Some((u32::try_from(i + 1).ok()?, v.state, v.source, v.bytes.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn d(b: u8) -> Digest {
        Digest::from_bytes([b; 32])
    }

    /// If two identified versions ever share a digest, queries by digest return the most
    /// restrictive state: deprecated wins over a later registered or released copy.
    #[test]
    fn repeated_digest_takes_the_most_restrictive_state() {
        let id = ArtifactId::Program(Uuid7::from_parts(1, [7; 10]));
        let mut s = LifeStore::new();
        let reg = |s: &mut LifeStore, e| {
            s.apply(&Change::Register { id: id.clone(), digest: d(1), source: None, bytes: vec![1] }, e)
        };
        reg(&mut s, 1);
        s.apply(&Change::SetState { id: id.clone(), version: 1, to: LifeState::Released }, 2);
        s.apply(&Change::SetState { id: id.clone(), version: 1, to: LifeState::Deprecated }, 3);
        reg(&mut s, 4);
        s.apply(&Change::SetState { id: id.clone(), version: 2, to: LifeState::Released }, 5);
        reg(&mut s, 6);
        assert_eq!(s.run_gate(&id, &d(1)), RunGate::Deprecated { version: 1 });
        assert_eq!(s.by_digest(&id, &d(1)).unwrap().state, LifeState::Deprecated);
        assert_eq!(s.lineage_view(&id).identified(&d(1)), Some((1, LifeState::Deprecated)));
        // Without the deprecated one, released wins over a newer registered copy.
        let mut t = LifeStore::new();
        let reg2 = |t: &mut LifeStore, e| {
            t.apply(&Change::Register { id: id.clone(), digest: d(2), source: None, bytes: vec![2] }, e)
        };
        reg2(&mut t, 1);
        t.apply(&Change::SetState { id: id.clone(), version: 1, to: LifeState::Released }, 2);
        reg2(&mut t, 3);
        assert_eq!(t.run_gate(&id, &d(2)), RunGate::Runnable { version: 1, state: LifeState::Released });
        // A retired version identifies nothing.
        t.apply(&Change::SetState { id: id.clone(), version: 2, to: LifeState::Draft }, 4);
        assert_eq!(t.by_digest(&id, &d(2)).unwrap().version, 1);
    }
}
