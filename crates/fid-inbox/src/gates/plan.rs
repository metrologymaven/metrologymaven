//! Gate plan selection from active governance profile, target protection and principal
//! (spec node `VIZ.INBOX.GATES.PLAN`, REQ-VIZ.INBOX.GATES.PLAN-001 .. -005).
//!
//! # Contract
//!
//! - Pure, D0 deterministic selection: [`plan`] is a total match on
//!   `(profile.name(), target_protected, command_count, principal, provider_present)`.
//! - **Non-human principals** (`agent`, `script`, `service`, `device`, `system`) always yield
//!   `GatePlan::Unavailable { reason: UnavailableReason::HumanRequired }` in every profile
//!   (REQ-004).
//! - **Protected targets** require a `Signature` step; when no [`CeremonyProvider`] is wired
//!   (`provider_present == false`), the plan is
//!   `GatePlan::Unavailable { reason: UnavailableReason::CeremonyUnavailable }` (REQ-001).
//! - **Lean** (and **Assistive-only**) on an unprotected target yield `[ConfirmCount { n }]`
//!   for batches (`command_count > 1`) and `[]` otherwise (REQ-002).
//! - **Regulated** adds `Reason` for every apply and `SecondPerson` when
//!   `program_release_by_other == always` (REQ-003), in addition to `ConfirmCount` for batches
//!   and `Signature` for protected targets.
//! - **All-or-nothing execution:** [`GateRunner::run`] calls the promotion callback only after
//!   every step in `GatePlan::Steps` reports [`StepOutcome::Passed`]; any cancellation, step
//!   failure or unavailable plan leaves the proposal pending and never invokes promotion
//!   (REQ-005).
//! - Gates never change side-effect classes and never sign on a human's behalf; signing happens
//!   out of band through a plugged-in [`CeremonyProvider`] (`VIZ.INBOX.GATES.REG` / `TRUST`).

#![deny(clippy::wildcard_enum_match_arm)]

use fid_canon::{to_canonical_bytes, CValue};
use fid_journal::promote::MAX_ASSERTION_BYTES;
use fid_journal::PrincipalKind;
use fid_profile::{CanonicalProfile, MeaningCode, ProfileDoc, ProfileError, ProfileName, ProgramReleaseByOther};
use fid_types::Digest;
use std::fmt;

/// Promotion pre-image digest passed to [`CeremonyProvider::run`].
pub type EntryHash = Digest;

/// Opaque human assertion reference returned by an out-of-band [`CeremonyProvider`] and
/// forwarded to `journal.promote_protected_branch@1`.
///
/// The Inbox never constructs an assertion on a human's behalf; only the plugged-in ceremony
/// provider produces one after interactive human verification.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssertionRef(String);

/// Why an assertion reference string was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertionRefError {
    Empty,
    TooLong { len: usize, max: usize },
    ControlChar,
}

impl fmt::Display for AssertionRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssertionRefError::Empty => f.write_str("assertion reference must not be empty"),
            AssertionRefError::TooLong { len, max } => {
                write!(f, "assertion reference is {len} bytes, at most {max} allowed")
            }
            AssertionRefError::ControlChar => f.write_str("assertion reference must not contain control characters"),
        }
    }
}

impl std::error::Error for AssertionRefError {}

impl AssertionRef {
    /// Validates and wraps a non-empty bounded assertion reference (`1..=MAX_ASSERTION_BYTES`
    /// bytes, no ASCII control characters).
    pub fn new(raw: impl Into<String>) -> Result<Self, AssertionRefError> {
        let s = raw.into();
        if s.is_empty() {
            return Err(AssertionRefError::Empty);
        }
        if s.len() > MAX_ASSERTION_BYTES {
            return Err(AssertionRefError::TooLong { len: s.len(), max: MAX_ASSERTION_BYTES });
        }
        if s.chars().any(char::is_control) {
            return Err(AssertionRefError::ControlChar);
        }
        Ok(Self(s))
    }

    /// The opaque assertion reference text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssertionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Non-passing outcome of a [`CeremonyProvider::run`] invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeremonyOutcome {
    /// The human cancelled the signing ceremony.
    Cancelled,
    /// The signing ceremony failed (e.g. hardware token error or policy refusal).
    Failed { reason: String },
    /// No signing capability is available at runtime.
    Unavailable,
}

impl fmt::Display for CeremonyOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CeremonyOutcome::Cancelled => f.write_str("cancelled"),
            CeremonyOutcome::Failed { reason } => write!(f, "failed: {reason}"),
            CeremonyOutcome::Unavailable => f.write_str("ceremony_unavailable"),
        }
    }
}

/// Optional out-of-band signature ceremony provider registered at app start-up.
///
/// The Lean profile works with `None` registered (no signing crate linked); protected branch
/// promotion requires a registered provider (REQ-VIZ.INBOX.GATES.PLAN-001).
pub trait CeremonyProvider: Send + Sync {
    /// Runs the interactive signing ceremony for `pre_image` covering `meanings` and returns
    /// the resulting human assertion reference, or a non-passing [`CeremonyOutcome`].
    fn run(&self, pre_image: EntryHash, meanings: &[MeaningCode]) -> Result<AssertionRef, CeremonyOutcome>;
}

/// Why a gate plan cannot proceed (apply stays disabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnavailableReason {
    /// Principal is not `human` (REQ-VIZ.INBOX.GATES.PLAN-004).
    HumanRequired,
    /// Target branch is protected and no [`CeremonyProvider`] is wired
    /// (REQ-VIZ.INBOX.GATES.PLAN-001).
    CeremonyUnavailable,
}

impl UnavailableReason {
    /// Stable wire name (`human_required` or `ceremony_unavailable`).
    pub fn as_str(self) -> &'static str {
        match self {
            UnavailableReason::HumanRequired => "human_required",
            UnavailableReason::CeremonyUnavailable => "ceremony_unavailable",
        }
    }
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One human gate step that must pass before CORE promotion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateStep {
    /// Confirm the total command count `n` of a multi-proposal batch (REQ-002).
    ConfirmCount { n: usize },
    /// Enter a human reason text for the apply (REQ-003).
    Reason,
    /// Run the signature ceremony with the profile's required meanings (REQ-001).
    Signature { meanings: Vec<MeaningCode> },
    /// Obtain confirmation from a second, distinct human principal (REQ-003).
    SecondPerson,
}

/// Selected gate plan for an Inbox apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatePlan {
    /// Ordered human steps that must all pass before CORE promotion (empty means direct apply).
    Steps(Vec<GateStep>),
    /// Apply is unavailable and must remain disabled.
    Unavailable { reason: UnavailableReason },
}

impl GatePlan {
    /// Whether this plan can run (as opposed to [`GatePlan::Unavailable`]).
    pub fn is_available(&self) -> bool {
        match self {
            GatePlan::Steps(_) => true,
            GatePlan::Unavailable { .. } => false,
        }
    }
}

/// Pure, D0 deterministic gate plan selection (REQ-VIZ.INBOX.GATES.PLAN-001 .. -004).
///
/// Total match on `(profile.name(), target_protected, command_count, principal, provider_present)`.
/// Gates never alter command side-effect classes and never sign on a human's behalf.
pub fn plan(
    profile: &ProfileDoc,
    target_protected: bool,
    command_count: usize,
    principal: PrincipalKind,
    provider_present: bool,
) -> GatePlan {
    // REQ-004: gates are unavailable to every non-human principal in every profile.
    match principal {
        PrincipalKind::Human => {}
        PrincipalKind::Agent
        | PrincipalKind::Script
        | PrincipalKind::Service
        | PrincipalKind::Device
        | PrincipalKind::System => {
            return GatePlan::Unavailable { reason: UnavailableReason::HumanRequired };
        }
    }

    // REQ-001: protected target requires a Signature step and a wired CeremonyProvider.
    if target_protected && !provider_present {
        return GatePlan::Unavailable { reason: UnavailableReason::CeremonyUnavailable };
    }

    let is_batch = command_count > 1;
    let meanings = profile.params().required_meanings().to_vec();
    let mut steps = Vec::new();

    match (profile.name(), target_protected, is_batch) {
        (ProfileName::Lean | ProfileName::AssistiveOnly, false, false) => {}
        (ProfileName::Lean | ProfileName::AssistiveOnly, false, true) => {
            steps.push(GateStep::ConfirmCount { n: command_count });
        }
        (ProfileName::Lean | ProfileName::AssistiveOnly, true, false) => {
            steps.push(GateStep::Signature { meanings });
        }
        (ProfileName::Lean | ProfileName::AssistiveOnly, true, true) => {
            steps.push(GateStep::ConfirmCount { n: command_count });
            steps.push(GateStep::Signature { meanings });
        }
        (ProfileName::Regulated, protected, batch) => {
            if batch {
                steps.push(GateStep::ConfirmCount { n: command_count });
            }
            steps.push(GateStep::Reason);
            if protected {
                steps.push(GateStep::Signature { meanings });
            }
            match profile.params().program_release_by_other() {
                ProgramReleaseByOther::Always => steps.push(GateStep::SecondPerson),
                ProgramReleaseByOther::AgentAuthored => {}
            }
        }
    }

    GatePlan::Steps(steps)
}

/// Convenience wrapper for [`plan`] when the caller holds an `Option<&dyn CeremonyProvider>`.
pub fn plan_with_provider(
    profile: &ProfileDoc,
    target_protected: bool,
    command_count: usize,
    principal: PrincipalKind,
    provider: Option<&dyn CeremonyProvider>,
) -> GatePlan {
    plan(profile, target_protected, command_count, principal, provider.is_some())
}

// ------------------------------------------------------------------ sign.show_profile@1 reader

/// Errors decoding a `sign.show_profile@1` result (`{digest, document, name, origin}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowProfileError {
    Malformed(String),
    Profile(ProfileError),
}

impl fmt::Display for ShowProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShowProfileError::Malformed(msg) => write!(f, "sign.show_profile@1 malformed: {msg}"),
            ShowProfileError::Profile(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ShowProfileError {}

const SHOW_KEYS: [&str; 4] = ["digest", "document", "name", "origin"];

/// Reads and verifies the active [`CanonicalProfile`] from a `sign.show_profile@1` bus result
/// (`{digest, document, name, origin}`), failing closed on any mismatch or malformed document.
pub fn profile_from_show_result(value: &CValue) -> Result<CanonicalProfile, ShowProfileError> {
    let CValue::Obj(map) = value else {
        return Err(ShowProfileError::Malformed("result must be a JSON object".into()));
    };
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    if keys != SHOW_KEYS {
        return Err(ShowProfileError::Malformed(format!("result must have exactly the keys {}", SHOW_KEYS.join(", "))));
    }
    let Some(CValue::Str(digest)) = map.get("digest") else {
        return Err(ShowProfileError::Malformed("`digest` must be a string".into()));
    };
    let Some(CValue::Str(name)) = map.get("name") else {
        return Err(ShowProfileError::Malformed("`name` must be a string".into()));
    };
    let Some(CValue::Str(origin)) = map.get("origin") else {
        return Err(ShowProfileError::Malformed("`origin` must be a string".into()));
    };
    if origin != "default" && origin != "set" {
        return Err(ShowProfileError::Malformed("`origin` must be \"default\" or \"set\"".into()));
    }
    let Some(doc_val) = map.get("document") else {
        return Err(ShowProfileError::Malformed("`document` is missing".into()));
    };
    let bytes = to_canonical_bytes(doc_val)
        .map_err(|e| ShowProfileError::Malformed(format!("canonicalising `document`: {}", e.code())))?;
    let canonical = CanonicalProfile::read(&bytes).map_err(ShowProfileError::Profile)?;
    if canonical.digest().as_str() != digest.as_str() {
        return Err(ShowProfileError::Malformed("`digest` does not match `document`".into()));
    }
    if canonical.document().name().as_str() != name.as_str() {
        return Err(ShowProfileError::Malformed("`name` does not match `document.name`".into()));
    }
    Ok(canonical)
}

// ------------------------------------------------------------------ GateRunner

/// Outcome reported by one UI gate step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// The human completed the step. `Reason` sets `reason`; `Signature` sets `assertion`.
    Passed { reason: Option<String>, assertion: Option<AssertionRef> },
    /// The human cancelled or dismissed the step.
    Cancelled,
    /// The step failed (for example, signing ceremony error).
    Failed { reason: String },
}

impl StepOutcome {
    /// A step that passed with no attached data (`ConfirmCount`, `SecondPerson`).
    pub fn passed() -> Self {
        Self::Passed { reason: None, assertion: None }
    }

    /// A `Reason` step that passed with the human's text unchanged.
    pub fn passed_with_reason(reason: impl Into<String>) -> Self {
        Self::Passed { reason: Some(reason.into()), assertion: None }
    }

    /// A `Signature` step that passed with the assertion returned by a [`CeremonyProvider`].
    pub fn passed_with_assertion(assertion: AssertionRef) -> Self {
        Self::Passed { reason: None, assertion: Some(assertion) }
    }
}

/// Interactive step runner supplied by the GUI layer (`VIZ.INBOX.GATES.LEAN` / `.REG`).
pub trait GateUi {
    /// Presents `step` to the human reviewer and returns its [`StepOutcome`].
    fn run_step(&mut self, step: &GateStep) -> StepOutcome;
}

/// Values collected across all passing gate steps and handed to the promotion callback.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GateResult {
    /// Human-entered reason text from a `Reason` step, if any.
    pub reason: Option<String>,
    /// Human assertion reference from a `Signature` step, if any.
    pub assertion: Option<AssertionRef>,
}

/// Outcome of [`GateRunner::run`] (all-or-nothing contract, REQ-VIZ.INBOX.GATES.PLAN-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome<T, E> {
    /// Every step passed and the promotion callback succeeded.
    Promoted(T),
    /// Every step passed, but the promotion callback returned an error.
    PromotionFailed(E),
    /// The plan was unavailable; promotion was not called and the proposal stays pending.
    Unavailable { reason: UnavailableReason },
    /// A step was cancelled; promotion was not called and the proposal stays pending.
    Cancelled { at_step: GateStep },
    /// A step failed; promotion was not called and the proposal stays pending.
    StepFailed { at_step: GateStep, reason: String },
}

/// All-or-nothing runner that executes every step of a [`GatePlan`] in order and calls the
/// promotion callback if and only if every step reported [`StepOutcome::Passed`] (REQ-005).
pub struct GateRunner;

impl GateRunner {
    /// Runs `plan` through `ui`, calling `on_all_passed` only after every step reports
    /// [`StepOutcome::Passed`]. If `plan` is [`GatePlan::Unavailable`], or if any step returns
    /// [`StepOutcome::Cancelled`] or [`StepOutcome::Failed`], `on_all_passed` is never called
    /// and the proposal stays pending.
    pub fn run<U, F, T, E>(plan: &GatePlan, ui: &mut U, on_all_passed: F) -> RunOutcome<T, E>
    where
        U: GateUi + ?Sized,
        F: FnOnce(GateResult) -> Result<T, E>,
    {
        let steps = match plan {
            GatePlan::Unavailable { reason } => return RunOutcome::Unavailable { reason: *reason },
            GatePlan::Steps(s) => s,
        };

        let mut collected = GateResult::default();
        for step in steps {
            match ui.run_step(step) {
                StepOutcome::Passed { reason, assertion } => {
                    if let Some(r) = reason {
                        collected.reason = Some(r);
                    }
                    if let Some(a) = assertion {
                        collected.assertion = Some(a);
                    }
                }
                StepOutcome::Cancelled => return RunOutcome::Cancelled { at_step: step.clone() },
                StepOutcome::Failed { reason } => {
                    return RunOutcome::StepFailed { at_step: step.clone(), reason };
                }
            }
        }

        match on_all_passed(collected) {
            Ok(v) => RunOutcome::Promoted(v),
            Err(e) => RunOutcome::PromotionFailed(e),
        }
    }

    /// Runs `plan` by dispatching [`GateStep::Signature`] steps to `provider` over `pre_image`
    /// and every other step to `ui`. If a `Signature` step is reached while `provider` is
    /// `None`, the runner fails closed without invoking `on_all_passed`.
    pub fn run_with_ceremony<U, F, T, E>(
        plan: &GatePlan,
        pre_image: EntryHash,
        provider: Option<&dyn CeremonyProvider>,
        ui: &mut U,
        on_all_passed: F,
    ) -> RunOutcome<T, E>
    where
        U: GateUi + ?Sized,
        F: FnOnce(GateResult) -> Result<T, E>,
    {
        struct CeremonyDispatch<'a, U: ?Sized> {
            pre_image: EntryHash,
            provider: Option<&'a dyn CeremonyProvider>,
            ui: &'a mut U,
        }

        impl<U: GateUi + ?Sized> GateUi for CeremonyDispatch<'_, U> {
            fn run_step(&mut self, step: &GateStep) -> StepOutcome {
                match step {
                    GateStep::Signature { meanings } => match self.provider {
                        None => StepOutcome::Failed { reason: UnavailableReason::CeremonyUnavailable.as_str().into() },
                        Some(p) => match p.run(self.pre_image, meanings) {
                            Ok(assertion) => StepOutcome::passed_with_assertion(assertion),
                            Err(CeremonyOutcome::Cancelled) => StepOutcome::Cancelled,
                            Err(CeremonyOutcome::Failed { reason }) => StepOutcome::Failed { reason },
                            Err(CeremonyOutcome::Unavailable) => {
                                StepOutcome::Failed { reason: UnavailableReason::CeremonyUnavailable.as_str().into() }
                            }
                        },
                    },
                    GateStep::ConfirmCount { .. } | GateStep::Reason | GateStep::SecondPerson => self.ui.run_step(step),
                }
            }
        }

        let mut dispatch = CeremonyDispatch { pre_image, provider, ui };
        Self::run(plan, &mut dispatch, on_all_passed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assertion_ref_bounds() {
        assert_eq!(AssertionRef::new(""), Err(AssertionRefError::Empty));
        assert_eq!(AssertionRef::new("ok\nline"), Err(AssertionRefError::ControlChar));
        let too_long = "a".repeat(MAX_ASSERTION_BYTES + 1);
        assert_eq!(
            AssertionRef::new(too_long),
            Err(AssertionRefError::TooLong { len: MAX_ASSERTION_BYTES + 1, max: MAX_ASSERTION_BYTES })
        );
        let ok = AssertionRef::new("esig:urn:sha256:abc");
        assert!(ok.is_ok());
    }

    #[test]
    fn every_non_human_principal_is_human_required() {
        let non_humans = [
            PrincipalKind::Agent,
            PrincipalKind::Script,
            PrincipalKind::Service,
            PrincipalKind::Device,
            PrincipalKind::System,
        ];
        for pname in ProfileName::ALL {
            let doc = ProfileDoc::builtin(pname);
            for pk in non_humans {
                for protected in [false, true] {
                    for count in [1usize, 5] {
                        for provider in [false, true] {
                            assert_eq!(
                                plan(&doc, protected, count, pk, provider),
                                GatePlan::Unavailable { reason: UnavailableReason::HumanRequired }
                            );
                        }
                    }
                }
            }
        }
    }
}
