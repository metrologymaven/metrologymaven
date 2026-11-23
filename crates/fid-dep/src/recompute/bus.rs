//! Bus, job-scheduler and journal integration for `CORE.DEP.RECOMP` (enabled by feature `bus`).
//!
//! - [`BusSystemRunner`] runs each element's recorded command through
//!   [`fid_bus::pipeline::Bus::run_handlers_as_system`] as principal `system` with
//!   `on_behalf_of` the triggering caller.
//! - [`commit_recompute_entry`] appends exactly one `project.recompute_dependents@1` entry
//!   onto `fid_journal::append::Journal` with `expected_prev = Some(exec.expected_head)`,
//!   returning [`RecompError::Conflict`] if the branch head moved (`REQ-002`, `REQ-009`, `AC-6`).
//! - [`JobCancelAdapter`] and [`JobProgressAdapter`] bridge [`fid_jobs::CancelToken`] and
//!   [`fid_jobs::ProgressSink`] into [`execute_recompute`] (`REQ-004`, `REQ-007`, `AC-3`, `AC-5`).

use super::{
    execute_recompute, CancelCheck, ElementRunOutput, OverlayEvent, OverlayView, ProgressReport, RecompError,
    RecomputeExecution, RecomputeRunner, RecomputeScope, RecordedCommand, RECOMPUTE_COMMAND_NAME,
};
use crate::edge::EntityId;
use crate::prop::DepGraph;
use fid_bus::handler::{EntityId as BusEntityId, EntityRef, SettingKey, StateView};
use fid_bus::pipeline::{Bus, SystemCall};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, Principal, PrincipalId, PrincipalRef, SessionKey, SessionTable,
};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue, Digest};
use fid_jobs::{CancelToken, Execution, JobClass, JobId, JobOpts, JobService, ProgressSink};
use fid_journal::append::{AppendRequest, Journal, JournalError, JournalRef};
use fid_journal::entry::{
    AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OutputDigest, GENESIS_PREV,
};
use fid_registry::PredContext;
use fid_types::{CommandId, Determinism, SideEffect};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ bus StateView adapter

/// Read-only [`StateView`] exposing an [`OverlayView`] to `Bus::run_handlers_as_system`.
pub struct OverlayStateView {
    head: Digest,
    entities: BTreeMap<String, (BusEntityId, String, CValue)>,
    settings: BTreeMap<String, CValue>,
}

impl OverlayStateView {
    pub fn from_overlay(overlay: &OverlayView) -> Result<Self, RecompError> {
        let mut entities = BTreeMap::new();
        for (id, ent) in overlay.base.iter() {
            let cur = overlay.get(id).unwrap_or(ent);
            let s = id.to_string();
            let eid = BusEntityId::new(&s).map_err(|e| RecompError::Internal(e.to_string()))?;
            entities.insert(s, (eid, cur.kind.clone(), cur.body.clone()));
        }
        for (id, ent) in overlay.staged.iter() {
            let s = id.to_string();
            if let std::collections::btree_map::Entry::Vacant(slot) = entities.entry(s) {
                let eid = BusEntityId::new(slot.key()).map_err(|e| RecompError::Internal(e.to_string()))?;
                slot.insert((eid, ent.kind.clone(), ent.body.clone()));
            }
        }
        Ok(Self { head: overlay.head(), entities, settings: overlay.settings.clone() })
    }
}

impl StateView for OverlayStateView {
    fn entity(&self, id: &BusEntityId) -> Option<EntityRef<'_>> {
        let (eid, kind, body) = self.entities.get(id.as_str())?;
        Some(EntityRef { id: eid, kind: kind.as_str(), body })
    }

    fn head(&self) -> Digest {
        self.head
    }

    fn setting(&self, key: &SettingKey) -> Option<CValue> {
        self.settings.get(key.as_str()).cloned()
    }
}

// ------------------------------------------------------------------ internal system channel

/// Internal bus channel that derives principal `system` acting `on_behalf_of` the caller
/// (`CORE.DEP.RECOMP` Scope & Build prompt).
pub struct SystemRecomputeChannel<'a> {
    pub session: &'a SessionKey,
    pub on_behalf_of: PrincipalRef,
}

impl Channel for SystemRecomputeChannel<'_> {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Internal
    }

    fn authenticated_identity(&self) -> Option<Identity> {
        let Ok(subject) = PrincipalId::new("recompute-engine") else {
            return None;
        };
        let mut id = Identity::new(subject);
        id.acting_for = Some(self.on_behalf_of.clone());
        Some(id)
    }

    fn session(&self) -> &SessionKey {
        self.session
    }
}

/// [`RecomputeRunner`] backed by [`Bus::run_handlers_as_system`] as principal `system`
/// with `on_behalf_of` the caller.
pub struct BusSystemRunner<'a> {
    pub bus: &'a Bus,
    pub sessions: &'a SessionTable,
    pub channel: &'a SystemRecomputeChannel<'a>,
    pub pred_ctx: &'a dyn PredContext,
}

impl RecomputeRunner for BusSystemRunner<'_> {
    fn run_element(
        &self,
        _id: EntityId,
        recorded: &RecordedCommand,
        overlay: &OverlayView,
    ) -> Result<ElementRunOutput, String> {
        let cmd_id = CommandId::parse(&recorded.command).map_err(|_| "invalid_command_id".to_owned())?;
        let view = OverlayStateView::from_overlay(overlay).map_err(|e| e.code().to_owned())?;
        let call = SystemCall { command: cmd_id, params: recorded.args.clone() };
        let mut checked = self
            .bus
            .run_handlers_as_system(self.sessions, self.channel, &[call], &view, self.pred_ctx)
            .map_err(|e| extract_handler_error_code(&e))?;
        let Some(first) = checked.pop() else {
            return Err("empty_handler_result".to_owned());
        };
        let raw_events = first.events();
        let mut out_events = Vec::with_capacity(raw_events.len());
        let mut updated_body = CValue::Null;
        for ev in raw_events {
            updated_body = ev.body().clone();
            out_events.push(OverlayEvent { event_type: ev.type_id().as_str().to_owned(), body: ev.body().clone() });
        }
        let body_bytes = to_canonical_bytes(&updated_body).map_err(|_| "canonical_encode_failed".to_owned())?;
        let output_digest = digest_bytes(&body_bytes);
        Ok(ElementRunOutput { output_digest, events: out_events, updated_body })
    }
}

// ------------------------------------------------------------------ fid-jobs adapters

/// Wraps a [`fid_jobs::CancelToken`] as a [`CancelCheck`].
pub struct JobCancelAdapter<'a>(pub &'a CancelToken);

impl CancelCheck for JobCancelAdapter<'_> {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// Wraps a [`fid_jobs::ProgressSink`] as a [`ProgressReport`].
pub struct JobProgressAdapter<'a>(pub &'a ProgressSink);

impl ProgressReport for JobProgressAdapter<'_> {
    fn report(&self, fraction: f64, stage: &str) {
        self.0.report(fraction, stage);
    }
}

/// Submits a recompute pass onto `fid_jobs::JobService`, reporting progress and checking
/// cancellation between elements (`REQ-004`, `REQ-007`, `AC-3`, `AC-5`).
#[allow(clippy::too_many_arguments)]
pub fn submit_recompute_job<G, R>(
    jobs: &JobService<Result<RecomputeExecution, RecompError>>,
    caller: &fid_jobs::Caller,
    graph: G,
    head_state: OverlayView,
    commands: BTreeMap<EntityId, RecordedCommand>,
    scope: RecomputeScope,
    runner: R,
) -> Result<JobId, RecompError>
where
    G: DepGraph + Send + 'static,
    R: RecomputeRunner + Send + 'static,
{
    let cmd_id = CommandId::parse(super::command_id()).map_err(|e| RecompError::Internal(e.to_string()))?;
    let exec = Execution::new(
        cmd_id,
        Box::new(move |sink: &ProgressSink, cancel: &CancelToken| {
            let cancel_ad = JobCancelAdapter(cancel);
            let prog_ad = JobProgressAdapter(sink);
            let res = execute_recompute(&graph, &head_state, &commands, &scope, &runner, &cancel_ad, &prog_ad);
            match res {
                Err(RecompError::Cancelled) => Err(fid_jobs::HandlerError::Cancelled),
                other => Ok(other),
            }
        }),
    );
    jobs.submit_as(caller, exec, JobOpts { class: JobClass::Batch, ..JobOpts::default() })
        .map_err(|e| RecompError::Internal(format!("job submit failed: {e:?}")))
}

// ------------------------------------------------------------------ single-entry journal commit (CAS)

/// Appends exactly one `project.recompute_dependents@1` entry onto `journal` with
/// `expected_prev = Some(exec.expected_head)` (`REQ-002`, `REQ-009`, `AC-1`, `AC-6`).
///
/// If the branch head moved while the recompute ran, returns [`RecompError::Conflict`] and
/// appends nothing.
#[allow(clippy::too_many_arguments)]
pub fn commit_recompute_entry(
    journal: &JournalRef<'_>,
    branch: &str,
    principal: &Principal,
    time: EntryTime,
    manifests: Manifests,
    scope: &RecomputeScope,
    exec: &RecomputeExecution,
) -> Result<Digest, RecompError> {
    let mut outputs = Vec::with_capacity(exec.committed.recomputed.len());
    for rec in &exec.committed.recomputed {
        outputs.push(OutputDigest { entity: Some(rec.id.to_string()), digest: rec.digest });
    }
    let event_envelopes: Vec<CValue> = exec.events.iter().map(OverlayEvent::to_envelope).collect();

    let draft = EntryDraft {
        stream: "record".to_owned(),
        branch: branch.to_owned(),
        logical_parent: None,
        time,
        principal: principal.to_draft(),
        ai_assistance: AiAssistance::None,
        manifests,
        reason: None,
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::Command(CommandDraft {
            id: RECOMPUTE_COMMAND_NAME.to_owned(),
            major: 1,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: 0,
            args: scope.to_params(),
            events: event_envelopes,
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs,
            algorithms: Vec::new(),
            event_sources: Vec::new(),
        }),
    };

    let mut req = AppendRequest::new(draft);
    req.expected_prev = Some(exec.expected_head);
    journal.append(req).map_err(|e| map_journal_err(e, exec.expected_head))
}

/// Same as [`commit_recompute_entry`] for an owned [`Journal`].
#[allow(clippy::too_many_arguments)]
pub fn commit_recompute_entry_owned(
    journal: &Journal,
    branch: &str,
    principal: &Principal,
    time: EntryTime,
    manifests: Manifests,
    scope: &RecomputeScope,
    exec: &RecomputeExecution,
) -> Result<Digest, RecompError> {
    let mut outputs = Vec::with_capacity(exec.committed.recomputed.len());
    for rec in &exec.committed.recomputed {
        outputs.push(OutputDigest { entity: Some(rec.id.to_string()), digest: rec.digest });
    }
    let event_envelopes: Vec<CValue> = exec.events.iter().map(OverlayEvent::to_envelope).collect();

    let draft = EntryDraft {
        stream: "record".to_owned(),
        branch: branch.to_owned(),
        logical_parent: None,
        time,
        principal: principal.to_draft(),
        ai_assistance: AiAssistance::None,
        manifests,
        reason: None,
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::Command(CommandDraft {
            id: RECOMPUTE_COMMAND_NAME.to_owned(),
            major: 1,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: 0,
            args: scope.to_params(),
            events: event_envelopes,
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs,
            algorithms: Vec::new(),
            event_sources: Vec::new(),
        }),
    };

    let mut req = AppendRequest::new(draft);
    req.expected_prev = Some(exec.expected_head);
    journal.append(req).map_err(|e| map_journal_err(e, exec.expected_head))
}

fn map_journal_err(e: JournalError, expected_head: Digest) -> RecompError {
    match e {
        JournalError::HeadMoved { expected, found } => RecompError::Conflict { expected, actual: found },
        other => {
            let msg = other.to_string();
            if msg.contains("head") && msg.contains("moved") {
                RecompError::Conflict { expected: expected_head, actual: GENESIS_PREV }
            } else {
                RecompError::Internal(msg)
            }
        }
    }
}

fn extract_handler_error_code(e: &fid_bus::error::BusError) -> String {
    use fid_bus::error::{BusError, InternalReason, PreconditionReason};
    match e {
        BusError::Internal(InternalReason::HandlerFailed, d) | BusError::Precondition(PreconditionReason::False, d) => {
            let s = d.as_untrusted_str();
            if !s.is_empty()
                && s.len() <= 64
                && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-'))
            {
                return s.to_owned();
            }
            e.reason().to_owned()
        }
        other => other.reason().to_owned(),
    }
}
