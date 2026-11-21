//! Branch promotion and promotion requests (spec node `CORE.JRN.BR.PROMO`): the person/agent
//! gate, where work done on one branch (typically an agent's) is adopted onto a human branch.
//!
//! [`plan`] checks one call of `journal.promote_branch@1` (W2, `human_only`),
//! `journal.promote_protected_branch@1` (W3) or `journal.request_promote_branch@1` (W1) and
//! returns the one event to commit, or a [`PromoteError`] whose [`PromoteError::code`] and
//! [`PromoteError::reason`] are a code and reason of the bus error catalogue (v3).
//! [`Promotions::apply_entry`] folds committed promotion events and refuses any its entry's
//! origin could not have produced. With feature `store`, [`command_defs`] declares the three
//! commands. The bus handlers are in `fid-journal-cmd`; [`Rebaser`] (`CORE.RPL.RETGT`) and
//! [`AssertionCheck`] (`TRUST`, or `CORE.BUS.PRIN`'s `HumanPresence` in the Lean profile) are
//! wired by the app.
//!
//! # Pinned rules
//!
//! - **Who.** The caller is the [`Actor`] the bus derived from the call's principal; nothing in
//!   the parameters names it. Only a human promotes: `promote_branch` refuses anyone else
//!   with `policy/human_only` and `promote_protected_branch` with `policy/class_forbidden`.
//!   Over MCP the bus turns an agent's call to either into a pending request
//!   (`CORE.BUS.POL.PROJ`); an agent's own path is `request_promote_branch`, which moves no
//!   head.
//! - **Bound to one diff.** Every call names `source_head`, `target_head` (null for the empty
//!   state) and `diff`, the SHA-256 of the canonical bytes of the `journal.diff_branch@1`
//!   result it was approved against. The diff is recomputed from verified headers
//!   (`CORE.JRN.BR.DIFF`); if either head differs the call is `conflict/expected_head_mismatch`
//!   (compare-and-swap on both heads), if the digest differs
//!   `conflict/expected_digest_mismatch`. A promotion that landed moved the target head, so
//!   the same call again is refused: it cannot be replayed. (REQ-007 asked to promote the named
//!   source head even if the source moved; the maintainers chose refusal instead.)
//! - **Order of checks** (first refusal wins): caller; note length (requests, at most
//!   [`MAX_NOTE_CHARS`] characters, `validation/schema`); source, then target, in the branch
//!   table (`not_found/unknown_branch`); source equal to target or a target in the `agent/`
//!   namespace (`precondition/false`); protection (`promote_branch` into a protected target is
//!   `policy/target_protected`, `promote_protected_branch` into an unprotected one
//!   `precondition/false`); the diff, heads and digest; nothing to promote (equal heads, or the
//!   source head is the ancestor: `precondition/false`); then the request event, or the
//!   assertion (protected: none wired `precondition/capability_missing`, not confirmed over the
//!   pre-image `policy/no_assertion`); then fast-forward or rebase.
//! - **Fast-forward** when the diff's ancestor is the target head: mode `fast_forward`, no
//!   entries, the [`Rebaser`] is not called. **Otherwise** the Rebaser (none wired:
//!   `precondition/capability_missing`): no steps `precondition/false`, more than
//!   [`MAX_STEPS`] `validation/input_limit`, the first step not `IDENTICAL` or `EQUIVALENT`
//!   `precondition/divergent_step` naming it (1-based); else mode `rebased` with every step.
//! - **Whole or nothing.** A promotion is one event (`BranchPromoted` or
//!   `ProtectedBranchPromoted`) holding every re-executed step, committed in one entry: it
//!   lands whole or not at all. Splitting the steps into their own journal entries with
//!   `promoted_from` is the commit stage's job (not built).
//! - **Pre-image** of a protected promotion: SHA-256 of the canonical
//!   `{diff, source, source_head, target, target_head}`; the event carries it.
//! - **Protection** is never weakened: no promotion event changes a protected flag; the fold
//!   accepts `BranchPromoted` only into an unprotected target and `ProtectedBranchPromoted`
//!   only into a protected one, from a W3 entry carrying a verified human assertion.
//! - **Fold** ([`Promotions::apply_entry`]): an entry holds at most one promotion event;
//!   `BranchPromoted` only from `journal.promote_branch@1`'s handler, human, W2;
//!   `ProtectedBranchPromoted` only from `journal.promote_protected_branch@1`'s handler, human,
//!   W3, naming as `assertion` the verified entry header's `human_assertion`
//!   ([`EntryOrigin::human_assertion`]), with its pre-image recomputed from its binding and
//!   confirmed again by the wired [`AssertionCheck`] (none wired: refused, since it cannot be
//!   proved); either only on an entry of the target branch; `PromotionRequested` only from
//!   `journal.request_promote_branch@1`'s handler, W1, any principal, on an entry of the source
//!   branch (the requester is the entry's principal; its identity is the header's principal
//!   commitment, REQ-005, and [`FoldedRequest`] keeps only its kind). Every event: a known
//!   human-owned target other than the source, and `source_head`/`target_head` equal to the
//!   logical heads before its entry, which the caller gives from the verified journal. Bodies have exactly their keys; `fast_forward` has no
//!   entries, `rebased` 1 to [`MAX_STEPS`] numbered from 1, each `IDENTICAL` or `EQUIVALENT`.
//!   A second promotion of the same `(target, target_head, source_head)` is a replay. Anything
//!   else is `internal/invariant_violated`. The three event types are reserved in the
//!   registry (`fid_registry::RESERVED_EFFECTS`) to their commands.

#[cfg(feature = "store")]
mod defs;

#[cfg(feature = "store")]
pub use defs::{command_defs, params_schema, result_schema, ASSERTION_PATTERN};

use crate::branch::{Actor, BranchError, BranchName, Branches, EntryOrigin, EventSource, Owner};
use crate::diff::{BranchDiff, DiffError};
use crate::entry::PrincipalKind;
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_types::{CommandId, Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Command IDs.
pub const PROMOTE_BRANCH: &str = "journal.promote_branch";
pub const PROMOTE_PROTECTED_BRANCH: &str = "journal.promote_protected_branch";
pub const REQUEST_PROMOTE_BRANCH: &str = "journal.request_promote_branch";
/// Event types.
pub const BRANCH_PROMOTED: &str = "BranchPromoted";
pub const PROTECTED_BRANCH_PROMOTED: &str = "ProtectedBranchPromoted";
pub const PROMOTION_REQUESTED: &str = "PromotionRequested";
/// Most re-executed steps in one promotion.
pub const MAX_STEPS: usize = 10_000;
/// Longest request note, in characters.
pub const MAX_NOTE_CHARS: usize = 2000;
/// Longest assertion reference, in bytes.
pub const MAX_ASSERTION_BYTES: usize = 4096;

// ------------------------------------------------------------------ errors

/// Why a call was refused, with its bus error (catalogue v3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoteError {
    /// Only a human may run this `human_only` command (`policy/human_only`).
    HumanOnly,
    /// Only a human may run this W3 command (`policy/class_forbidden`).
    ClassForbidden,
    /// A parameter is malformed (`validation/schema`).
    Schema(String),
    /// No such branch (`not_found/unknown_branch`).
    UnknownBranch(String),
    /// The call cannot promote anything (`precondition/false`).
    NotPromotable(String),
    /// `promote_branch` into a protected target (`policy/target_protected`).
    TargetProtected(String),
    /// A head moved since the diff was approved (`conflict/expected_head_mismatch`).
    HeadMismatch(String),
    /// The diff is not the approved one (`conflict/expected_digest_mismatch`).
    DigestMismatch,
    /// The Rebaser or AssertionCheck is not wired (`precondition/capability_missing`).
    CapabilityMissing(&'static str),
    /// The assertion is not confirmed over the pre-image (`policy/no_assertion`).
    NoAssertion,
    /// A re-executed step is not IDENTICAL or EQUIVALENT (`precondition/divergent_step`).
    DivergentStep { step: usize, outcome: StepOutcome },
    /// Over a bound (`validation/input_limit`).
    InputLimit(String),
    /// The Rebaser failed (`internal/handler_failed`).
    Rebase(String),
    /// The caller cancelled (`cancelled/requested`).
    Cancelled,
    /// The diff was refused: its own mapping.
    Diff(DiffError),
}

impl PromoteError {
    /// The bus error code.
    pub fn code(&self) -> &'static str {
        self.bus_code().0
    }

    /// The bus error reason.
    pub fn reason(&self) -> &'static str {
        self.bus_code().1
    }

    /// The bus error `(code, reason)` of catalogue v3.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            PromoteError::HumanOnly => ("policy", "human_only"),
            PromoteError::ClassForbidden => ("policy", "class_forbidden"),
            PromoteError::Schema(_) => ("validation", "schema"),
            PromoteError::UnknownBranch(_) => ("not_found", "unknown_branch"),
            PromoteError::NotPromotable(_) => ("precondition", "false"),
            PromoteError::TargetProtected(_) => ("policy", "target_protected"),
            PromoteError::HeadMismatch(_) => ("conflict", "expected_head_mismatch"),
            PromoteError::DigestMismatch => ("conflict", "expected_digest_mismatch"),
            PromoteError::CapabilityMissing(_) => ("precondition", "capability_missing"),
            PromoteError::NoAssertion => ("policy", "no_assertion"),
            PromoteError::DivergentStep { .. } => ("precondition", "divergent_step"),
            PromoteError::InputLimit(_) => ("validation", "input_limit"),
            PromoteError::Rebase(_) => ("internal", "handler_failed"),
            PromoteError::Cancelled => ("cancelled", "requested"),
            PromoteError::Diff(e) => e.bus_code(),
        }
    }

    /// The detail text.
    pub fn detail(&self) -> String {
        match self {
            PromoteError::HumanOnly => "only a human principal may promote a branch".into(),
            PromoteError::ClassForbidden => "only a human principal may promote into a protected branch (W3)".into(),
            PromoteError::Schema(d)
            | PromoteError::NotPromotable(d)
            | PromoteError::HeadMismatch(d)
            | PromoteError::InputLimit(d)
            | PromoteError::Rebase(d) => d.clone(),
            PromoteError::UnknownBranch(b) => format!("no branch named {b:?}"),
            PromoteError::TargetProtected(b) => {
                format!("{b} is protected; use journal.promote_protected_branch@1 with a human assertion")
            }
            PromoteError::DigestMismatch => "the diff differs from the one the promotion was approved against".into(),
            PromoteError::CapabilityMissing(what) => format!("no {what} is wired"),
            PromoteError::NoAssertion => "no confirmed human assertion over the promotion's pre-image".into(),
            PromoteError::DivergentStep { step, outcome } => format!("step {step} re-executed as {}", outcome.as_str()),
            PromoteError::Cancelled => "cancelled".into(),
            PromoteError::Diff(e) => e.detail(),
        }
    }
}

impl fmt::Display for PromoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.code(), self.reason(), self.detail())
    }
}

impl std::error::Error for PromoteError {}

fn tampered(why: impl Into<String>) -> BranchError {
    BranchError::Tampered(why.into())
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

// ------------------------------------------------------------------ wiring

/// The replay outcome of one re-executed step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Identical,
    Equivalent,
    Divergent,
    Failed,
}

impl StepOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            StepOutcome::Identical => "IDENTICAL",
            StepOutcome::Equivalent => "EQUIVALENT",
            StepOutcome::Divergent => "DIVERGENT",
            StepOutcome::Failed => "FAILED",
        }
    }

    pub fn parse(s: &str) -> Option<StepOutcome> {
        match s {
            "IDENTICAL" => Some(StepOutcome::Identical),
            "EQUIVALENT" => Some(StepOutcome::Equivalent),
            "DIVERGENT" => Some(StepOutcome::Divergent),
            "FAILED" => Some(StepOutcome::Failed),
            _ => None,
        }
    }

    /// Whether a promotion may keep a step with this outcome.
    pub fn acceptable(self) -> bool {
        matches!(self, StepOutcome::Identical | StepOutcome::Equivalent)
    }
}

/// One source command re-executed on the target head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseStep {
    /// `id@major` of the re-executed command.
    pub command: String,
    /// Digest of the re-executed entry.
    pub digest: Digest,
    pub outcome: StepOutcome,
}

/// Re-executes the source's commands since the ancestor on the target head
/// (`CORE.RPL.RETGT` implements it; the app wires it).
pub trait Rebaser: Send + Sync {
    fn rebase(
        &self,
        source_head: &Digest,
        target_head: Option<&Digest>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<RebaseStep>, String>;
}

/// Confirms a human assertion over a promotion's pre-image (`TRUST`, or `CORE.BUS.PRIN`'s
/// `HumanPresence` in the Lean profile, D-035; the app wires it).
pub trait AssertionCheck: Send + Sync {
    fn confirm(&self, pre_image: &Digest, assertion: &str) -> bool;
}

// ------------------------------------------------------------------ calls and events

/// The three commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromoteOp {
    Promote,
    Protected,
    Request,
}

impl PromoteOp {
    pub const ALL: [PromoteOp; 3] = [PromoteOp::Promote, PromoteOp::Protected, PromoteOp::Request];

    pub fn command(self) -> &'static str {
        match self {
            PromoteOp::Promote => PROMOTE_BRANCH,
            PromoteOp::Protected => PROMOTE_PROTECTED_BRANCH,
            PromoteOp::Request => REQUEST_PROMOTE_BRANCH,
        }
    }

    pub fn class(self) -> SideEffect {
        match self {
            PromoteOp::Promote => SideEffect::W2,
            PromoteOp::Protected => SideEffect::W3,
            PromoteOp::Request => SideEffect::W1,
        }
    }
}

/// The parameters of one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteParams {
    pub source: String,
    pub source_head: Digest,
    pub target: String,
    pub target_head: Option<Digest>,
    /// SHA-256 of the canonical bytes of the approved `journal.diff_branch@1` result.
    pub diff: Digest,
    /// `promote_protected_branch` only.
    pub assertion: Option<String>,
    /// `request_promote_branch` only; untrusted text.
    pub note: Option<String>,
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn opt_digest(d: Option<Digest>) -> CValue {
    d.map_or(CValue::Null, |d| text(d.as_str()))
}

fn to_digest(d: fid_canon::Digest) -> Digest {
    Digest::from_bytes(d.0)
}

/// SHA-256 of the canonical bytes of a diff result: the digest a promotion is bound to.
pub fn diff_digest(diff: &BranchDiff) -> Result<Digest, PromoteError> {
    let b = to_canonical_bytes(&diff.to_value()).map_err(|e| PromoteError::Schema(format!("diff: {e}")))?;
    Ok(to_digest(digest_bytes(&b)))
}

/// The core of a promotion or request: what it binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub source: BranchName,
    pub source_head: Digest,
    pub target: BranchName,
    pub target_head: Option<Digest>,
    pub diff: Digest,
}

impl Binding {
    fn fields(&self) -> BTreeMap<String, CValue> {
        BTreeMap::from([
            ("diff".to_string(), text(self.diff.as_str())),
            ("source".to_string(), text(self.source.as_str())),
            ("source_head".to_string(), text(self.source_head.as_str())),
            ("target".to_string(), text(self.target.as_str())),
            ("target_head".to_string(), opt_digest(self.target_head)),
        ])
    }

    /// The pre-image a protected promotion's human assertion signs.
    pub fn pre_image(&self) -> Digest {
        // The value has only strings and nulls: canonical serialisation cannot fail.
        let b = to_canonical_bytes(&CValue::Obj(self.fields())).unwrap_or_default();
        to_digest(digest_bytes(&b))
    }
}

/// How a promotion lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    FastForward,
    Rebased,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::FastForward => "fast_forward",
            Mode::Rebased => "rebased",
        }
    }
}

/// A promotion event's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promotion {
    /// `ProtectedBranchPromoted` (W3) rather than `BranchPromoted` (W2).
    pub protected: bool,
    pub binding: Binding,
    pub mode: Mode,
    pub entries: Vec<RebaseStep>,
    /// Protected promotions only.
    pub pre_image: Option<Digest>,
    /// Protected promotions only: the assertion reference, which must be the entry header's
    /// `human_assertion`.
    pub assertion: Option<String>,
}

/// A promotion request's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub binding: Binding,
    pub note: Option<String>,
}

/// The three events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoEvent {
    Promoted(Promotion),
    Requested(Request),
}

impl PromoEvent {
    pub fn type_id(&self) -> &'static str {
        match self {
            PromoEvent::Promoted(p) if p.protected => PROTECTED_BRANCH_PROMOTED,
            PromoEvent::Promoted(_) => BRANCH_PROMOTED,
            PromoEvent::Requested(_) => PROMOTION_REQUESTED,
        }
    }

    pub fn body(&self) -> CValue {
        match self {
            PromoEvent::Promoted(p) => {
                let mut o = p.binding.fields();
                o.insert("mode".into(), text(p.mode.as_str()));
                let entries = p
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        CValue::Obj(BTreeMap::from([
                            ("command".to_string(), text(&s.command)),
                            ("digest".to_string(), text(s.digest.as_str())),
                            ("outcome".to_string(), text(s.outcome.as_str())),
                            ("step".to_string(), CValue::Num(step_number(i))),
                        ]))
                    })
                    .collect();
                o.insert("entries".into(), CValue::Arr(entries));
                if let Some(pi) = p.pre_image {
                    o.insert("pre_image".into(), text(pi.as_str()));
                }
                if let Some(a) = &p.assertion {
                    o.insert("assertion".into(), text(a));
                }
                CValue::Obj(o)
            }
            PromoEvent::Requested(r) => {
                let mut o = r.binding.fields();
                if let Some(n) = &r.note {
                    o.insert("note".into(), text(n));
                }
                CValue::Obj(o)
            }
        }
    }

    /// Parses a stored event strictly: a known type, exactly its keys, every value well
    /// formed. Anything else is [`BranchError::Tampered`].
    pub fn from_parts(type_id: &str, body: &CValue) -> Result<PromoEvent, BranchError> {
        let CValue::Obj(o) = body else { return Err(tampered(format!("{} body is not an object", truncate(type_id)))) };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        let want: &[&str] = match type_id {
            BRANCH_PROMOTED => &["diff", "entries", "mode", "source", "source_head", "target", "target_head"],
            PROTECTED_BRANCH_PROMOTED => {
                &["assertion", "diff", "entries", "mode", "pre_image", "source", "source_head", "target", "target_head"]
            }
            PROMOTION_REQUESTED if o.contains_key("note") => {
                &["diff", "note", "source", "source_head", "target", "target_head"]
            }
            PROMOTION_REQUESTED => &["diff", "source", "source_head", "target", "target_head"],
            other => return Err(tampered(format!("unknown promotion event type {:?}", truncate(other)))),
        };
        if keys != want {
            return Err(tampered(format!("{type_id} has keys {keys:?}, expected {want:?}")));
        }
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(v)) => Ok(v.as_str()),
            _ => Err(tampered(format!("{type_id}.{k} is not a string"))),
        };
        let digest = |k: &str| Digest::parse(s(k)?).map_err(|_| tampered(format!("{type_id}.{k} is not a digest")));
        let name = |k: &str| BranchName::parse(s(k)?).map_err(|e| tampered(format!("{type_id}.{k}: {}", e.detail())));
        let target_head = match o.get("target_head") {
            Some(CValue::Null) => None,
            _ => Some(digest("target_head")?),
        };
        let binding = Binding {
            source: name("source")?,
            source_head: digest("source_head")?,
            target: name("target")?,
            target_head,
            diff: digest("diff")?,
        };
        if type_id == PROMOTION_REQUESTED {
            let note = match o.get("note") {
                None => None,
                Some(CValue::Str(n)) => Some(n.clone()),
                Some(_) => return Err(tampered("PromotionRequested.note is not a string")),
            };
            return Ok(PromoEvent::Requested(Request { binding, note }));
        }
        let mode = match s("mode")? {
            "fast_forward" => Mode::FastForward,
            "rebased" => Mode::Rebased,
            _ => return Err(tampered(format!("{type_id}.mode is not fast_forward or rebased"))),
        };
        let Some(CValue::Arr(items)) = o.get("entries") else {
            return Err(tampered(format!("{type_id}.entries is not an array")));
        };
        if items.len() > MAX_STEPS {
            return Err(tampered(format!("{type_id} has more than {MAX_STEPS} entries")));
        }
        let mut entries = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let CValue::Obj(e) = item else { return Err(tampered(format!("{type_id}.entries[{i}] is not an object"))) };
            let ek: Vec<&str> = e.keys().map(String::as_str).collect();
            if ek != ["command", "digest", "outcome", "step"] {
                return Err(tampered(format!("{type_id}.entries[{i}] has keys {ek:?}")));
            }
            let es = |k: &str| match e.get(k) {
                Some(CValue::Str(v)) => Ok(v.as_str()),
                _ => Err(tampered(format!("{type_id}.entries[{i}].{k} is not a string"))),
            };
            if !matches!(e.get("step"), Some(CValue::Num(n)) if *n == step_number(i)) {
                return Err(tampered(format!("{type_id}.entries[{i}].step is not {}", i + 1)));
            }
            let command = es("command")?;
            CommandId::parse(command).map_err(|_| tampered(format!("{type_id}.entries[{i}].command")))?;
            entries.push(RebaseStep {
                command: command.to_string(),
                digest: Digest::parse(es("digest")?).map_err(|_| tampered(format!("{type_id}.entries[{i}].digest")))?,
                outcome: StepOutcome::parse(es("outcome")?)
                    .ok_or_else(|| tampered(format!("{type_id}.entries[{i}].outcome")))?,
            });
        }
        let pre_image = if type_id == PROTECTED_BRANCH_PROMOTED { Some(digest("pre_image")?) } else { None };
        let assertion = if type_id == PROTECTED_BRANCH_PROMOTED {
            let a = s("assertion")?;
            if a.is_empty() || a.len() > MAX_ASSERTION_BYTES {
                return Err(tampered("ProtectedBranchPromoted.assertion is empty or too long"));
            }
            Some(a.to_string())
        } else {
            None
        };
        Ok(PromoEvent::Promoted(Promotion {
            protected: type_id == PROTECTED_BRANCH_PROMOTED,
            binding,
            mode,
            entries,
            pre_image,
            assertion,
        }))
    }
}

/// The 1-based step number of index `i` (at most [`MAX_STEPS`], so exact in an f64).
fn step_number(i: usize) -> f64 {
    let n = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
    f64::from(n)
}

// ------------------------------------------------------------------ the planner

fn known<'a>(table: &'a Branches, name: &str) -> Result<&'a crate::branch::Branch, PromoteError> {
    table.get(name).ok_or_else(|| PromoteError::UnknownBranch(truncate(name)))
}

/// Checks one call and returns the event to commit (module documentation, Pinned rules).
/// `diff_of` computes the verified diff of source against target; it runs only once the
/// cheaper checks pass.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    op: PromoteOp,
    actor: Actor,
    table: &Branches,
    p: &PromoteParams,
    diff_of: &mut dyn FnMut() -> Result<BranchDiff, DiffError>,
    rebaser: Option<&dyn Rebaser>,
    check: Option<&dyn AssertionCheck>,
    cancel: &dyn Fn() -> bool,
) -> Result<PromoEvent, PromoteError> {
    match op {
        PromoteOp::Promote if actor != Actor::Human => return Err(PromoteError::HumanOnly),
        PromoteOp::Protected if actor != Actor::Human => return Err(PromoteError::ClassForbidden),
        _ => {}
    }
    if op == PromoteOp::Request {
        if let Some(n) = &p.note {
            if n.chars().count() > MAX_NOTE_CHARS {
                return Err(PromoteError::Schema(format!("note has more than {MAX_NOTE_CHARS} characters")));
            }
        }
    }
    let source = known(table, &p.source)?;
    let target = known(table, &p.target)?;
    if source.name == target.name {
        return Err(PromoteError::NotPromotable(format!("{} is both source and target", source.name)));
    }
    if target.owner() == Owner::Agent {
        return Err(PromoteError::NotPromotable(format!(
            "{}: promotion adopts work onto a human branch, not into an agent branch",
            target.name
        )));
    }
    match op {
        PromoteOp::Promote if target.protected => return Err(PromoteError::TargetProtected(target.name.to_string())),
        PromoteOp::Protected if !target.protected => {
            return Err(PromoteError::NotPromotable(format!(
                "{} is not protected; use journal.promote_branch@1",
                target.name
            )))
        }
        _ => {}
    }
    let d = diff_of().map_err(PromoteError::Diff)?;
    if d.source.head != Some(p.source_head) {
        return Err(PromoteError::HeadMismatch(format!(
            "source {} is at {}, not {}",
            source.name,
            d.source.head.map_or("the empty state".to_string(), |h| h.to_string()),
            p.source_head
        )));
    }
    if d.target.head != p.target_head {
        return Err(PromoteError::HeadMismatch(format!(
            "target {} is at {}, not {}",
            target.name,
            d.target.head.map_or("the empty state".to_string(), |h| h.to_string()),
            p.target_head.map_or("the empty state".to_string(), |h| h.to_string())
        )));
    }
    if diff_digest(&d)? != p.diff {
        return Err(PromoteError::DigestMismatch);
    }
    if p.target_head == Some(p.source_head) || d.ancestor == Some(p.source_head) {
        return Err(PromoteError::NotPromotable(format!("{} has nothing {} lacks", source.name, target.name)));
    }
    let binding = Binding {
        source: source.name.clone(),
        source_head: p.source_head,
        target: target.name.clone(),
        target_head: p.target_head,
        diff: p.diff,
    };
    if op == PromoteOp::Request {
        return Ok(PromoEvent::Requested(Request { binding, note: p.note.clone() }));
    }
    let (pre_image, assertion) = if op == PromoteOp::Protected {
        let check = check.ok_or(PromoteError::CapabilityMissing("AssertionCheck"))?;
        let assertion = p.assertion.as_deref().ok_or_else(|| PromoteError::Schema("missing assertion".into()))?;
        if assertion.is_empty() || assertion.len() > MAX_ASSERTION_BYTES {
            return Err(PromoteError::Schema(format!("assertion has 1 to {MAX_ASSERTION_BYTES} bytes")));
        }
        let pi = binding.pre_image();
        if !check.confirm(&pi, assertion) {
            return Err(PromoteError::NoAssertion);
        }
        (Some(pi), Some(assertion.to_string()))
    } else {
        (None, None)
    };
    let protected = op == PromoteOp::Protected;
    if d.ancestor == p.target_head {
        return Ok(PromoEvent::Promoted(Promotion {
            protected,
            binding,
            mode: Mode::FastForward,
            entries: Vec::new(),
            pre_image,
            assertion,
        }));
    }
    let rebaser = rebaser.ok_or(PromoteError::CapabilityMissing("Rebaser"))?;
    if cancel() {
        return Err(PromoteError::Cancelled);
    }
    let steps = rebaser.rebase(&p.source_head, p.target_head.as_ref(), cancel).map_err(PromoteError::Rebase)?;
    if cancel() {
        return Err(PromoteError::Cancelled);
    }
    if steps.is_empty() {
        return Err(PromoteError::NotPromotable("the Rebaser returned no steps".into()));
    }
    if steps.len() > MAX_STEPS {
        return Err(PromoteError::InputLimit(format!("more than {MAX_STEPS} re-executed steps")));
    }
    for (i, s) in steps.iter().enumerate() {
        if !s.outcome.acceptable() {
            return Err(PromoteError::DivergentStep { step: i.saturating_add(1), outcome: s.outcome });
        }
        if CommandId::parse(&s.command).is_err() {
            return Err(PromoteError::Rebase(format!("step {} names no command id@major", i.saturating_add(1))));
        }
    }
    Ok(PromoEvent::Promoted(Promotion {
        protected,
        binding,
        mode: Mode::Rebased,
        entries: steps,
        pre_image,
        assertion,
    }))
}

// ------------------------------------------------------------------ the fold

/// A folded promotion request, with its requester taken from the entry's principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedRequest {
    pub requester: PrincipalKind,
    pub request: Request,
}

/// The promotions and requests of a journal, folded from their events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Promotions {
    seen: BTreeSet<(String, Option<Digest>, Digest)>,
    pub promotions: Vec<Promotion>,
    pub requests: Vec<FoldedRequest>,
}

impl Promotions {
    fn handler(origin: &EntryOrigin, command: &str, class: SideEffect) -> bool {
        origin.source == EventSource::Handler && origin.command == command && origin.major == 1 && origin.class == class
    }

    /// Folds the promotion events of one committed entry with this origin. `heads` are the
    /// logical heads (`CORE.JRN.BR.DIFF`'s rule; `None` for the empty state) of the branches
    /// before this entry, as the replay of the verified journal gives them; `check` is the
    /// wired AssertionCheck. Refuses, as `internal/invariant_violated`, anything its origin
    /// could not have produced (module documentation, Fold), and folds nothing of the entry
    /// then.
    pub fn apply_entry(
        &mut self,
        table: &Branches,
        heads: &BTreeMap<String, Option<Digest>>,
        check: Option<&dyn AssertionCheck>,
        origin: &EntryOrigin,
        events: &[PromoEvent],
    ) -> Result<(), BranchError> {
        let n = events.iter().filter(|e| matches!(e, PromoEvent::Promoted(_))).count();
        if n > 1 {
            return Err(tampered("an entry holds more than one promotion"));
        }
        let mut next = self.clone();
        for e in events {
            let binding = match e {
                PromoEvent::Requested(r) => &r.binding,
                PromoEvent::Promoted(p) => &p.binding,
            };
            let target = check_pair(table, binding)?;
            if target.owner() == Owner::Agent {
                return Err(tampered(format!("{} into agent branch {}", e.type_id(), target.name)));
            }
            check_heads(heads, binding)?;
            match e {
                PromoEvent::Requested(r) => {
                    if !Self::handler(origin, REQUEST_PROMOTE_BRANCH, SideEffect::W1)
                        || origin.branch != r.binding.source.as_str()
                    {
                        return Err(tampered(format!("PromotionRequested from {}", describe(origin))));
                    }
                    if r.note.as_ref().is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS) {
                        return Err(tampered("PromotionRequested.note is too long"));
                    }
                    next.requests.push(FoldedRequest { requester: origin.kind, request: r.clone() });
                }
                PromoEvent::Promoted(p) => {
                    let ok = if p.protected {
                        Self::handler(origin, PROMOTE_PROTECTED_BRANCH, SideEffect::W3)
                            && origin.kind == PrincipalKind::Human
                            && asserted_by(origin, p, check)
                    } else {
                        Self::handler(origin, PROMOTE_BRANCH, SideEffect::W2) && origin.kind == PrincipalKind::Human
                    };
                    if !ok || origin.branch != p.binding.target.as_str() {
                        return Err(tampered(format!("{} from {}", e.type_id(), describe(origin))));
                    }
                    if target.protected != p.protected {
                        return Err(tampered(format!(
                            "{} into {} (protected: {})",
                            e.type_id(),
                            target.name,
                            target.protected
                        )));
                    }
                    match p.mode {
                        Mode::FastForward if !p.entries.is_empty() => {
                            return Err(tampered("a fast-forward carries re-executed entries"));
                        }
                        Mode::Rebased if p.entries.is_empty() || p.entries.len() > MAX_STEPS => {
                            return Err(tampered("a rebased promotion has no or too many entries"));
                        }
                        _ => {}
                    }
                    if p.entries.iter().any(|s| !s.outcome.acceptable()) {
                        return Err(tampered("a promotion keeps a step that is not IDENTICAL or EQUIVALENT"));
                    }
                    let key = (p.binding.target.to_string(), p.binding.target_head, p.binding.source_head);
                    if !next.seen.insert(key) {
                        return Err(tampered(format!("promotion into {} replayed", p.binding.target)));
                    }
                    next.promotions.push(p.clone());
                }
            }
        }
        *self = next;
        Ok(())
    }
}

/// A protected promotion is bound to its entry: the event names the verified header's
/// `human_assertion`, its pre-image is the one its binding gives, and the wired
/// AssertionCheck confirms that assertion over it again (none wired: not provable).
fn asserted_by(origin: &EntryOrigin, p: &Promotion, check: Option<&dyn AssertionCheck>) -> bool {
    let pre = p.binding.pre_image();
    match (&origin.human_assertion, &p.assertion, p.pre_image, check) {
        (Some(header), Some(body), Some(pi), Some(check)) => header == body && pi == pre && check.confirm(&pi, body),
        _ => false,
    }
}

/// The event's heads are the heads before its entry: a replay, or a made-up or stale head,
/// is refused.
fn check_heads(heads: &BTreeMap<String, Option<Digest>>, b: &Binding) -> Result<(), BranchError> {
    let at = |name: &BranchName| {
        heads.get(name.as_str()).copied().ok_or_else(|| tampered(format!("no head given for {name}")))
    };
    if at(&b.source)? != Some(b.source_head) || at(&b.target)? != b.target_head {
        return Err(tampered(format!("promotion bound to heads of {} and {} that are not theirs", b.source, b.target)));
    }
    Ok(())
}

/// Source and target are known, distinct branches; returns the target.
fn check_pair<'a>(table: &'a Branches, b: &Binding) -> Result<&'a crate::branch::Branch, BranchError> {
    let s = table.get(b.source.as_str()).ok_or_else(|| tampered(format!("unknown source branch {}", b.source)))?;
    let t = table.get(b.target.as_str()).ok_or_else(|| tampered(format!("unknown target branch {}", b.target)))?;
    if s.name == t.name {
        return Err(tampered(format!("{} is both source and target", s.name)));
    }
    Ok(t)
}

fn describe(o: &EntryOrigin) -> String {
    format!(
        "{}@{} ({:?}, principal {}, {:?} on {})",
        truncate(&o.command),
        o.major,
        o.source,
        o.kind.as_str(),
        o.class,
        truncate(&o.branch)
    )
}
