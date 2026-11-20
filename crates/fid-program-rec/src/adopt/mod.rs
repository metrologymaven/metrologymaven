//! Agent-provenance gate on release (spec node `AUTO.REC.ADOPT`, REQ-AUTO.REC.ADOPT-001 ..
//! 007, CONSTITUTION C5, D-031).
//!
//! A person must adopt every agent-authored program step, and every agent-authored script
//! version as a whole, before `AUTO.REC.LIFE` may release it.
//!
//! - [`ProvenanceStore`]: two append-only record lists, authorship and adoption, keyed by
//!   [`Target`] (a program step or a script version). Records are never removed or rewritten;
//!   the current state of a target is derived from the order of its latest records.
//! - [`ProvenanceStore::record_authorship`]: called by every path that creates or changes a
//!   step or registers a script version. It takes the bus-derived
//!   [`fid_bus::principal::Principal`], never a kind from a request, so an agent cannot record
//!   itself as a person.
//! - [`ProvenanceStore::unadopted_steps`] and [`ProvenanceStore::unadopted_script`]: the
//!   predicates `AUTO.REC.LIFE` uses to refuse a release.
//! - [`command`]: `program.adopt_steps@1` and `script.adopt_version@1` (W2, `human_only:
//!   true`, request-only on MCP), declared with the registry macros, with their CLI and MCP
//!   entry points.
//! - [`event`]: the journal event of an adoption; [`mod@replay`]: exact replay of those events.
//!
//! **Fail closed.** A target is unadopted unless its latest authorship is by a `human`
//! principal, or an adoption was recorded after its latest authorship. So:
//! - a step or version with no authorship record at all (unknown provenance) is unadopted;
//! - every non-human author (agent, script, service, device, system) needs adoption, not only
//!   `agent`: none of them is a person accountable for the content;
//! - an authorship by a non-human after an adoption makes the target unadopted again
//!   (REQ-004), because the adoption is then older than the latest authorship.
//!
//! **Branches.** Adoption is human-only, and a person writes the working branch, so every
//! adoption event names the branch `working`. Authorship is recorded per target wherever the
//! step lives; a step merged from an agent branch keeps its agent authorship.
//!
//! No floating point is used (D0), and nothing here panics.

pub mod command;
pub mod event;
pub mod replay;

pub use command::{
    adopt_steps, adopt_version, declarations, invoke_cli, invoke_mcp, AdoptCtx, AdoptJournal, AdoptStepsParams,
    AdoptStepsResult, AdoptVersionParams, AdoptVersionResult, Programs, CLI_ADOPT_STEPS, CLI_ADOPT_VERSION,
    MCP_ADOPT_STEPS, MCP_ADOPT_VERSION,
};
pub use event::AdoptEvent;
pub use replay::{replay, AdoptReplayed};

use fid_bus::principal::{Principal, PrincipalKind};
use fid_program::{Program, Uuid7};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Command ID of the step adoption command.
pub const ADOPT_STEPS_COMMAND: &str = "program.adopt_steps@1";
/// Command ID of the script version adoption command.
pub const ADOPT_VERSION_COMMAND: &str = "script.adopt_version@1";
/// The branch every adoption is written to: adoption is human-only, and a person writes the
/// working branch.
pub const WORKING_BRANCH: &str = "working";
/// Longest script ID, in bytes.
pub const MAX_SCRIPT_ID_BYTES: usize = 128;
/// JSON Schema pattern of a script ID.
pub const SCRIPT_ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
/// JSON Schema pattern of a program or step ID (lowercase UUIDv7, AUTO.PROG.FMT).
pub const UUID7_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

// ------------------------------------------------------------------ identifiers

/// A script ID: 1 to 128 bytes, an ASCII letter or digit, then letters, digits and `._:-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScriptId(String);

impl ScriptId {
    /// Checks and wraps `s`; anything else is `bad_params`.
    pub fn new(s: &str) -> Result<ScriptId, AdoptError> {
        let b = s.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= MAX_SCRIPT_ID_BYTES
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(c));
        if ok {
            Ok(ScriptId(s.to_owned()))
        } else {
            Err(AdoptError::bad("script is not a valid script ID"))
        }
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What provenance is kept for: one program step, or one script version as a whole.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// A step of a Command Program.
    Step {
        /// The program.
        program: Uuid7,
        /// The step.
        step: Uuid7,
    },
    /// One version of a script, named by the digest of its canonical form.
    ScriptVersion {
        /// The script.
        script: ScriptId,
        /// The version digest.
        version: Digest,
    },
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Step { program, step } => write!(f, "{program}/{step}"),
            Target::ScriptVersion { script, version } => write!(f, "{}@{}", script.as_str(), version.as_str()),
        }
    }
}

// ------------------------------------------------------------------ errors

/// Errors of the adoption commands. [`AdoptError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptError {
    /// `bad_params{reason}`: parameters break the schema or name a step twice.
    BadParams {
        /// Static reason.
        reason: &'static str,
    },
    /// `policy_refused{reason}`: the caller is not a `human` principal. `reason` is the bus
    /// policy reason (`human_only`, `class_forbidden`).
    PolicyRefused {
        /// Bus policy reason.
        reason: &'static str,
    },
    /// `program_not_found{program}`.
    ProgramNotFound {
        /// The program ID as given.
        program: String,
    },
    /// `stale_review{current}`: the program changed after the person reviewed it.
    StaleReview {
        /// The current program digest.
        current: String,
    },
    /// `unknown_step{step}`: the step is not in the program.
    UnknownStep {
        /// The step ID as given.
        step: String,
    },
    /// `unknown_version{script, version}`: no authorship of this script version is recorded.
    UnknownVersion {
        /// `script@version`.
        target: String,
    },
    /// `nothing_to_adopt{target}`: the target is not unadopted work (authored by a person, or
    /// already adopted since its latest authorship), or `all_unadopted` found nothing.
    NothingToAdopt {
        /// The step, script version or program named.
        target: String,
    },
    /// `journal{reason}`: the adoption could not be journaled; nothing was adopted.
    Journal {
        /// Display-safe reason from the journal seam.
        reason: String,
    },
    /// `internal{reason}`: the current program cannot be serialised.
    Internal {
        /// Static reason.
        reason: &'static str,
    },
}

impl AdoptError {
    fn bad(reason: &'static str) -> AdoptError {
        AdoptError::BadParams { reason }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            AdoptError::BadParams { .. } => "bad_params",
            AdoptError::PolicyRefused { .. } => "policy_refused",
            AdoptError::ProgramNotFound { .. } => "program_not_found",
            AdoptError::StaleReview { .. } => "stale_review",
            AdoptError::UnknownStep { .. } => "unknown_step",
            AdoptError::UnknownVersion { .. } => "unknown_version",
            AdoptError::NothingToAdopt { .. } => "nothing_to_adopt",
            AdoptError::Journal { .. } => "journal",
            AdoptError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for AdoptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdoptError::BadParams { reason } => write!(f, "bad_params: {reason}"),
            AdoptError::PolicyRefused { reason } => write!(f, "policy_refused: {reason}"),
            AdoptError::ProgramNotFound { program } => write!(f, "program_not_found: {program}"),
            AdoptError::StaleReview { current } => write!(f, "stale_review: the program is now {current}"),
            AdoptError::UnknownStep { step } => write!(f, "unknown_step: {step}"),
            AdoptError::UnknownVersion { target } => write!(f, "unknown_version: {target}"),
            AdoptError::NothingToAdopt { target } => write!(f, "nothing_to_adopt: {target}"),
            AdoptError::Journal { reason } => write!(f, "journal: {reason}"),
            AdoptError::Internal { reason } => write!(f, "internal: {reason}"),
        }
    }
}

impl std::error::Error for AdoptError {}

// ------------------------------------------------------------------ records

/// One authorship record: who last created or changed the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorship {
    /// The step or script version.
    pub target: Target,
    /// The bus-derived principal kind.
    pub principal_kind: PrincipalKind,
    /// The bus-derived principal identity.
    pub principal_id: String,
    /// Position of this record in the store's journal order.
    pub journal_entry: u64,
}

/// One adoption record: a person took responsibility for the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    /// The step or script version.
    pub target: Target,
    /// The adopting person's identity.
    pub human_id: String,
    /// The program digest the person reviewed, or the script version digest.
    pub digest: Digest,
    /// Position of this record in the store's journal order.
    pub journal_entry: u64,
}

/// Derived state of one target: its latest authorship and latest adoption positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetState {
    author_pos: u64,
    author_is_human: bool,
    adoption_pos: Option<u64>,
}

impl TargetState {
    fn unadopted(&self) -> bool {
        match self.adoption_pos {
            _ if self.author_is_human => false,
            None => true,
            Some(a) => a < self.author_pos,
        }
    }
}

/// The provenance store: append-only authorship and adoption records with the derived
/// per-target state. There is no API that removes, rewrites or resets a record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProvenanceStore {
    authorships: Vec<Authorship>,
    adoptions: Vec<Adoption>,
    next: u64,
    state: BTreeMap<Target, TargetState>,
}

impl ProvenanceStore {
    /// An empty store.
    pub fn new() -> ProvenanceStore {
        ProvenanceStore::default()
    }

    fn position(&mut self) -> u64 {
        let p = self.next;
        self.next = self.next.saturating_add(1);
        p
    }

    /// Records `principal` as the latest author of `target` and returns the record's
    /// position. The kind and identity come from the bus-derived principal only. An author
    /// that is not `human` after an adoption makes the target unadopted again (REQ-004).
    pub fn record_authorship(&mut self, target: Target, principal: &Principal) -> u64 {
        let pos = self.position();
        let kind = principal.kind();
        let st = TargetState { author_pos: pos, author_is_human: is_person(kind), adoption_pos: None };
        self.state
            .entry(target.clone())
            .and_modify(|s| {
                s.author_pos = pos;
                s.author_is_human = st.author_is_human;
            })
            .or_insert(st);
        self.authorships.push(Authorship {
            target,
            principal_kind: kind,
            principal_id: principal.id().as_str().to_owned(),
            journal_entry: pos,
        });
        pos
    }

    /// Appends an adoption. Crate-internal: only the human-only commands and replay reach it.
    fn record_adoption(&mut self, target: Target, human_id: &str, digest: Digest) {
        let pos = self.position();
        // A target with no authorship (unknown provenance) gets a state whose adoption is as
        // recent as its (absent) authorship, so any later authorship supersedes the adoption.
        let st = TargetState { author_pos: pos, author_is_human: false, adoption_pos: None };
        self.state.entry(target.clone()).or_insert(st).adoption_pos = Some(pos);
        self.adoptions.push(Adoption { target, human_id: human_id.to_owned(), digest, journal_entry: pos });
    }

    /// Whether `target` is unadopted: its latest authorship is by a non-human with no later
    /// adoption, or nothing is known about it (fail closed).
    pub fn is_unadopted(&self, target: &Target) -> bool {
        match self.state.get(target) {
            Some(s) => s.unadopted(),
            None => true,
        }
    }

    /// Whether any authorship of `target` is recorded.
    pub fn knows(&self, target: &Target) -> bool {
        self.authorships.iter().any(|a| &a.target == target)
    }

    /// Every step of `program`, in document order (AUTO.PROG.FMT: main, then subroutines,
    /// depth first), that is unadopted (REQ-005). Steps with no provenance record count as
    /// unadopted.
    pub fn unadopted_steps(&self, program: &Program) -> Vec<Uuid7> {
        let id = program.program_id;
        program.step_ids().into_iter().filter(|s| self.is_unadopted(&Target::Step { program: id, step: *s })).collect()
    }

    /// Whether the script version is unadopted (REQ-006): registered or last changed by a
    /// non-human with no later `script.adopt_version@1`, or with no provenance at all.
    pub fn unadopted_script(&self, script: &ScriptId, version: &Digest) -> bool {
        self.is_unadopted(&Target::ScriptVersion { script: script.clone(), version: *version })
    }

    /// The authorship records, in order.
    pub fn authorships(&self) -> &[Authorship] {
        &self.authorships
    }

    /// The adoption records, in order.
    pub fn adoptions(&self) -> &[Adoption] {
        &self.adoptions
    }
}

/// Whether an author of this kind is a person accountable for the content. Exhaustive, so a
/// new principal kind fails to build until it is classified.
fn is_person(kind: PrincipalKind) -> bool {
    match kind {
        PrincipalKind::Human => true,
        PrincipalKind::Agent
        | PrincipalKind::Script
        | PrincipalKind::Service
        | PrincipalKind::Device
        | PrincipalKind::System => false,
    }
}
