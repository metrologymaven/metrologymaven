//! Exact replay of a project journal holding report events (`RPT.MODEL.DOC`) and template
//! events (AC-6). Nothing is drawn and no file is read: IDs come from the events and
//! template bytes from the content-addressed store, re-hashed on every read.
//!
//! Per event, in journal order:
//!
//! - **Report events**: as `crate::doc::replay_exact` (kind registry and project checks,
//!   then the operation and its `report_digest`).
//! - **`report.save_template@1`**: the version must be the next one for that ID (latest of
//!   built-in and replayed project versions, plus one) and the ID must not be locked; the
//!   object must be in the store, hash to the digest, be a canonical template with that ID,
//!   carry the event's `locked` flag, pass every check of a save, and differ from the
//!   latest version (identical saves journal nothing). A save from a report must also
//!   rebuild, from the replayed report (whose digest must be the event's), to the same
//!   digest.
//! - **`report.apply_template@1`**: the event's `{id, version}` must resolve to the event's
//!   digest (so a built-in that changed under the same version is caught), the object must
//!   be stored, and the report rebuilt from it must have the event's `report_digest`.
//!
//! Outcomes: `NOT_REPLAYABLE` for an unreadable event or a block kind (or kind version) the
//! replaying registry does not have; `DIVERGENT` for anything else that does not match;
//! `IDENTICAL` when every event matches. Replay stops at the first event that is not
//! identical. A template edited after the fact therefore never makes a stored report
//! replay differently without detection: its bytes no longer hash to the recorded digest.

use super::convert::{check_template, open_stored, report_from_template, source_template, template_from_report};
use super::error::VerError;
use super::event::{SaveSource, TemplateEvent};
use super::store::{digest_of, read_object, BuiltinRegistry, ObjectRead, Origin, TemplateIndex};
use crate::doc::{
    KindRegistry, Op, Report, ReportError, ReportEvent, ReportId, StoredReport, TemplateRef, MAX_REPLAY_EVENTS,
};
use crate::tmpl::format::{serialise, TemplateId};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// The outcome of an exact replay, the replayed reports and the replayed project versions.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateReplayRun {
    pub outcome: ReplayOutcome,
    /// Index of the first event that was not identical, if any.
    pub first_mismatch: Option<usize>,
    /// Replayed reports after the last identical event, with their stored digests.
    pub reports: BTreeMap<ReportId, (Report, Digest)>,
    /// Replayed project template versions, in journal order.
    pub templates: Vec<TemplateRef>,
}

/// Why one event did not replay.
enum Miss {
    NotReplayable,
    Divergent,
}

impl From<ReportError> for Miss {
    fn from(e: ReportError) -> Miss {
        match e {
            ReportError::UnknownBlockKind(_) => Miss::NotReplayable,
            _ => Miss::Divergent,
        }
    }
}

impl From<VerError> for Miss {
    fn from(e: VerError) -> Miss {
        match e {
            VerError::Report(r) => r.into(),
            _ => Miss::Divergent,
        }
    }
}

struct State<'a> {
    store: &'a dyn ObjectRead,
    kinds: &'a KindRegistry,
    builtins: &'a BuiltinRegistry,
    project: Uuid,
    reports: BTreeMap<ReportId, (Report, Digest)>,
    index: TemplateIndex,
    templates: Vec<TemplateRef>,
}

impl State<'_> {
    fn report_event(&mut self, v: &CValue) -> Result<(), Miss> {
        let ev = ReportEvent::from_value(v).map_err(|_| Miss::NotReplayable)?;
        if let Op::AddBlock { kind, kind_version, .. } = ev.op() {
            if self.kinds.get(kind).map(|(v, _)| v) != Some(*kind_version) {
                return Err(Miss::NotReplayable);
            }
        }
        if let Op::CreateReport { project, .. } = ev.op() {
            if *project != self.project {
                return Err(Miss::Divergent);
            }
        }
        let id = ev.report();
        let state = self.reports.get(&id).map(|(r, _)| r.clone());
        let r = ev.op().apply(state, self.kinds)?;
        let d = StoredReport::of(&r)?.digest;
        if d != ev.report_digest() {
            return Err(Miss::Divergent);
        }
        self.reports.insert(id, (r, d));
        Ok(())
    }

    fn object(&self, digest: &Digest) -> Result<Vec<u8>, Miss> {
        read_object(self.store, digest)?.ok_or(Miss::Divergent)
    }

    fn save(&mut self, template: TemplateRef, locked: bool, source: &SaveSource) -> Result<(), Miss> {
        let id = TemplateId::new(template.id).ok_or(Miss::Divergent)?;
        let latest = self.index.latest(&id, self.builtins);
        let builtin_locked = self.builtins.versions(&id).iter().any(|b| b.locked);
        if builtin_locked || latest.as_ref().is_some_and(Origin::locked) {
            return Err(Miss::Divergent);
        }
        let expected = latest.as_ref().map_or(Some(1), |l| l.r().version.checked_add(1));
        if expected != Some(template.version) || latest.is_some_and(|l| l.r().digest == template.digest) {
            return Err(Miss::Divergent);
        }
        let bytes = self.object(&template.digest)?;
        let t = open_stored(&bytes, &template)?;
        if t.locked() != locked {
            return Err(Miss::Divergent);
        }
        check_template(&t, self.kinds)?;
        if let SaveSource::Report { report, report_digest } = source {
            let (r, d) = self.reports.get(report).ok_or(Miss::Divergent)?;
            if d != report_digest {
                return Err(Miss::Divergent);
            }
            let from = source_template(r, self.store)?;
            let rebuilt = template_from_report(r, id, from.as_ref())?;
            let rebuilt = serialise(&rebuilt).map_err(|_| Miss::Divergent)?;
            if digest_of(rebuilt.as_bytes()) != template.digest {
                return Err(Miss::Divergent);
            }
        }
        self.index.push(template, locked, self.builtins)?;
        self.templates.push(template);
        Ok(())
    }

    fn apply(
        &mut self,
        report: ReportId,
        title: &crate::doc::Untrusted<crate::doc::Text>,
        template: TemplateRef,
        report_digest: Digest,
    ) -> Result<(), Miss> {
        let id = TemplateId::new(template.id).ok_or(Miss::Divergent)?;
        let known = self.index.get(&id, Some(template.version), self.builtins).ok_or(Miss::Divergent)?;
        if known.r() != template || self.reports.contains_key(&report) || report.uuid() == self.project {
            return Err(Miss::Divergent);
        }
        let bytes = self.object(&template.digest)?;
        let t = open_stored(&bytes, &template)?;
        check_template(&t, self.kinds)?;
        let r = report_from_template(&t, template, report, title, self.kinds)?;
        let d = StoredReport::of(&r)?.digest;
        if d != report_digest {
            return Err(Miss::Divergent);
        }
        self.reports.insert(report, (r, d));
        Ok(())
    }

    fn event(&mut self, v: &CValue) -> Result<(), Miss> {
        let ty = match v {
            CValue::Obj(o) => match o.get("type") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => return Err(Miss::NotReplayable),
            },
            _ => return Err(Miss::NotReplayable),
        };
        if !TemplateEvent::is_template_type(ty) {
            return self.report_event(v);
        }
        match TemplateEvent::from_value(v).map_err(|_| Miss::NotReplayable)? {
            TemplateEvent::Save { template, locked, source } => self.save(template, locked, &source),
            TemplateEvent::Apply { report, title, template, report_digest } => {
                self.apply(report, &title, template, report_digest)
            }
        }
    }
}

/// Replays `events` (oldest first) of project `project` in exact mode against the
/// content-addressed store `store`.
pub fn replay_exact(
    events: &[CValue],
    store: &dyn ObjectRead,
    kinds: &KindRegistry,
    builtins: &BuiltinRegistry,
    project: Uuid,
) -> TemplateReplayRun {
    let mut st = State {
        store,
        kinds,
        builtins,
        project,
        reports: BTreeMap::new(),
        index: TemplateIndex::default(),
        templates: Vec::new(),
    };
    for (i, v) in events.iter().enumerate() {
        let res = if i >= MAX_REPLAY_EVENTS { Err(Miss::NotReplayable) } else { st.event(v) };
        if let Err(m) = res {
            let outcome = match m {
                Miss::NotReplayable => ReplayOutcome::NotReplayable,
                Miss::Divergent => ReplayOutcome::Divergent,
            };
            return TemplateReplayRun {
                outcome,
                first_mismatch: Some(i),
                reports: st.reports,
                templates: st.templates,
            };
        }
    }
    TemplateReplayRun {
        outcome: ReplayOutcome::Identical,
        first_mismatch: None,
        reports: st.reports,
        templates: st.templates,
    }
}
