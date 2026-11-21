//! Storage seam, built-in registry and the project template index.
//!
//! - [`ObjectRead`] is the content-addressed store's **safe object read** (CORE.STO.CAS):
//!   the whole object whose SHA-256 is the digest, never a chunk manifest or a raw blob.
//!   Callers here still re-hash what they get, so a backend that serves anything else fails
//!   closed (`report.store_invalid`).
//! - [`TemplateBackend`] adds the two commits of the W2 commands, each atomic: object plus
//!   event (save), or object plus stored report plus event (apply). There is no call that
//!   replaces or deletes an object, so stored template versions are immutable (REQ-002).
//! - [`BuiltinRegistry`] holds the templates crates ship (starters and forms).
//! - [`TemplateIndex`] is the project index `BTreeMap<TemplateId, Vec<TemplateRef>>`, folded
//!   from the journal's save events (never stored separately, so it cannot disagree with
//!   the journal).

use super::error::VerError;
use super::event::TemplateEvent;
use crate::doc::{ReportBackend, ReportError, ReportEvent, ReportId, StoredReport, TemplateRef};
use crate::tmpl::format::{serialise, Template, TemplateId};
use fid_canon::{digest_bytes, CValue};
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Most template IDs one project index (or the built-in registry) holds.
pub const MAX_TEMPLATE_IDS: usize = 10_000;
/// Most versions of one template ID.
pub const MAX_VERSIONS: usize = 65_536;

/// The digest of `bytes` as a [`Digest`].
pub(crate) fn digest_of(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

/// The content-addressed store's safe object read.
pub trait ObjectRead {
    /// The whole object stored under `digest`, if any.
    fn object(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ReportError>;
}

impl ObjectRead for BTreeMap<Digest, Vec<u8>> {
    fn object(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ReportError> {
        Ok(self.get(digest).cloned())
    }
}

/// Reads `digest` and checks that the bytes hash to it (`report.store_invalid` otherwise);
/// `None` when the store does not hold it.
pub(crate) fn read_object<S: ObjectRead + ?Sized>(store: &S, digest: &Digest) -> Result<Option<Vec<u8>>, ReportError> {
    match store.object(digest)? {
        Some(b) if digest_of(&b) == *digest => Ok(Some(b)),
        Some(_) => Err(ReportError::StoreInvalid("object does not hash to its digest")),
        None => Ok(None),
    }
}

/// The project a template command runs against: reports, the content-addressed store and
/// the journal.
pub trait TemplateBackend: ReportBackend + ObjectRead {
    /// The template events of the journal, oldest first.
    fn template_events(&self) -> Result<Vec<TemplateEvent>, ReportError>;
    /// Stores `object` (idempotent: an object already stored is left as it is) and appends
    /// the save event, both or neither.
    fn commit_save(&mut self, object: Vec<u8>, event: TemplateEvent) -> Result<(), ReportError>;
    /// Stores `object` (idempotent), stores the new report under `id` and appends the apply
    /// event, all or nothing. The report must not exist yet.
    fn commit_apply(
        &mut self,
        object: Vec<u8>,
        id: &ReportId,
        stored: StoredReport,
        event: TemplateEvent,
    ) -> Result<(), ReportError>;
}

/// One journal entry of [`MemoryTemplateBackend`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalEntry {
    Report(ReportEvent),
    Template(TemplateEvent),
}

impl JournalEntry {
    pub fn to_value(&self) -> CValue {
        match self {
            JournalEntry::Report(e) => e.to_value(),
            JournalEntry::Template(e) => e.to_value(),
        }
    }
}

/// An in-memory project: stored reports, the content-addressed store and one journal of
/// report and template events in commit order. Used by tests and tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTemplateBackend {
    project: Uuid,
    docs: BTreeMap<ReportId, StoredReport>,
    objects: BTreeMap<Digest, Vec<u8>>,
    journal: Vec<JournalEntry>,
}

impl MemoryTemplateBackend {
    pub fn new(project: Uuid) -> MemoryTemplateBackend {
        MemoryTemplateBackend { project, docs: BTreeMap::new(), objects: BTreeMap::new(), journal: Vec::new() }
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// The journal as event values, oldest first (the input of [`super::replay_exact`]).
    pub fn journal_values(&self) -> Vec<CValue> {
        self.journal.iter().map(JournalEntry::to_value).collect()
    }

    /// The content-addressed store.
    pub fn objects(&self) -> &BTreeMap<Digest, Vec<u8>> {
        &self.objects
    }

    pub fn stored(&self, id: &ReportId) -> Option<&StoredReport> {
        self.docs.get(id)
    }

    fn put(&mut self, object: Vec<u8>) -> Result<(), ReportError> {
        let d = digest_of(&object);
        match self.objects.get(&d) {
            Some(existing) if *existing != object => Err(ReportError::Backend("object differs under its digest")),
            Some(_) => Ok(()),
            None => {
                self.objects.insert(d, object);
                Ok(())
            }
        }
    }
}

impl ReportBackend for MemoryTemplateBackend {
    fn project(&self) -> Uuid {
        self.project
    }

    fn load(&self, id: &ReportId) -> Result<Option<StoredReport>, ReportError> {
        Ok(self.docs.get(id).cloned())
    }

    fn commit(&mut self, id: &ReportId, stored: StoredReport, event: ReportEvent) -> Result<(), ReportError> {
        if event.report() != *id || event.report_digest() != stored.digest {
            return Err(ReportError::Backend("event does not match the stored report"));
        }
        self.journal.push(JournalEntry::Report(event));
        self.docs.insert(*id, stored);
        Ok(())
    }
}

impl ObjectRead for MemoryTemplateBackend {
    fn object(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ReportError> {
        Ok(self.objects.get(digest).cloned())
    }
}

impl TemplateBackend for MemoryTemplateBackend {
    fn template_events(&self) -> Result<Vec<TemplateEvent>, ReportError> {
        Ok(self
            .journal
            .iter()
            .filter_map(|e| match e {
                JournalEntry::Template(t) => Some(t.clone()),
                JournalEntry::Report(_) => None,
            })
            .collect())
    }

    fn commit_save(&mut self, object: Vec<u8>, event: TemplateEvent) -> Result<(), ReportError> {
        if !matches!(event, TemplateEvent::Save { .. }) || digest_of(&object) != event.template().digest {
            return Err(ReportError::Backend("event does not match the object"));
        }
        self.put(object)?;
        self.journal.push(JournalEntry::Template(event));
        Ok(())
    }

    fn commit_apply(
        &mut self,
        object: Vec<u8>,
        id: &ReportId,
        stored: StoredReport,
        event: TemplateEvent,
    ) -> Result<(), ReportError> {
        let TemplateEvent::Apply { report, report_digest, template, .. } = &event else {
            return Err(ReportError::Backend("not an apply event"));
        };
        if report != id || *report_digest != stored.digest || digest_of(&object) != template.digest {
            return Err(ReportError::Backend("event does not match the stored report"));
        }
        if self.docs.contains_key(id) {
            return Err(ReportError::IdCollision(id.uuid()));
        }
        self.put(object)?;
        self.docs.insert(*id, stored);
        self.journal.push(JournalEntry::Template(event));
        Ok(())
    }
}

// ------------------------------------------------------------------ built-ins

/// One registered built-in version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Builtin {
    pub(crate) r: TemplateRef,
    pub(crate) locked: bool,
    pub(crate) bytes: Vec<u8>,
}

/// The templates crates ship (starters and fixed forms), keyed by template ID. Registering
/// a template whose canonical bytes differ from that ID's latest registered version adds
/// the next version; identical bytes return the existing version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltinRegistry {
    entries: BTreeMap<TemplateId, Vec<Builtin>>,
}

impl BuiltinRegistry {
    pub fn new() -> BuiltinRegistry {
        BuiltinRegistry::default()
    }

    /// Registers `template` and returns its reference. Bounded by [`MAX_TEMPLATE_IDS`] and
    /// [`MAX_VERSIONS`] (`report.limit_exceeded`).
    pub fn register(&mut self, template: Template) -> Result<TemplateRef, VerError> {
        let bytes = serialise(&template)?.into_vec();
        let digest = digest_of(&bytes);
        let id = template.template_id();
        if !self.entries.contains_key(&id) && self.entries.len() >= MAX_TEMPLATE_IDS {
            return Err(ReportError::LimitExceeded("template ids").into());
        }
        let versions = self.entries.entry(id).or_default();
        if let Some(last) = versions.last() {
            if last.r.digest == digest {
                return Ok(last.r);
            }
        }
        if versions.len() >= MAX_VERSIONS {
            return Err(ReportError::LimitExceeded("template versions").into());
        }
        let version = u32::try_from(versions.len() + 1).map_err(|_| ReportError::LimitExceeded("template versions"))?;
        let r = TemplateRef { id: id.uuid(), version, digest };
        versions.push(Builtin { r, locked: template.locked(), bytes });
        Ok(r)
    }

    pub(crate) fn versions(&self, id: &TemplateId) -> &[Builtin] {
        self.entries.get(id).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Builtin> {
        self.entries.values().flatten()
    }
}

// ------------------------------------------------------------------ project index

/// One project version in the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectVersion {
    pub(crate) r: TemplateRef,
    pub(crate) locked: bool,
}

/// Where a resolved version lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin<'a> {
    Builtin(&'a Builtin),
    Project(ProjectVersion),
}

impl Origin<'_> {
    pub(crate) fn r(&self) -> TemplateRef {
        match self {
            Origin::Builtin(b) => b.r,
            Origin::Project(p) => p.r,
        }
    }

    pub(crate) fn locked(&self) -> bool {
        match self {
            Origin::Builtin(b) => b.locked,
            Origin::Project(p) => p.locked,
        }
    }
}

/// The project index, folded from save events. Per ID, project versions strictly increase
/// and never reuse a built-in version of that ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateIndex {
    project: BTreeMap<TemplateId, Vec<ProjectVersion>>,
}

impl TemplateIndex {
    /// Folds the save events of `events` (apply events are skipped). A version that does
    /// not increase, or that a built-in of the same ID already uses, is
    /// `report.store_invalid`.
    pub(crate) fn fold(events: &[TemplateEvent], builtins: &BuiltinRegistry) -> Result<TemplateIndex, ReportError> {
        let mut ix = TemplateIndex::default();
        for e in events {
            if let TemplateEvent::Save { template, locked, .. } = e {
                ix.push(*template, *locked, builtins)?;
            }
        }
        Ok(ix)
    }

    /// Adds a project version (fold and replay).
    pub(crate) fn push(&mut self, r: TemplateRef, locked: bool, builtins: &BuiltinRegistry) -> Result<(), ReportError> {
        let id = TemplateId::new(r.id).ok_or(ReportError::StoreInvalid("template id"))?;
        if builtins.versions(&id).iter().any(|b| b.r.version == r.version) {
            return Err(ReportError::StoreInvalid("project version reuses a built-in version"));
        }
        if !self.project.contains_key(&id) && self.project.len() >= MAX_TEMPLATE_IDS {
            return Err(ReportError::LimitExceeded("template ids"));
        }
        let versions = self.project.entry(id).or_default();
        if versions.last().is_some_and(|l| l.r.version >= r.version) {
            return Err(ReportError::StoreInvalid("template versions do not increase"));
        }
        if versions.len() >= MAX_VERSIONS {
            return Err(ReportError::LimitExceeded("template versions"));
        }
        versions.push(ProjectVersion { r, locked });
        Ok(())
    }

    /// Every version of `id`, built-in and project, ascending by version.
    pub(crate) fn all<'a>(&self, id: &TemplateId, builtins: &'a BuiltinRegistry) -> Vec<Origin<'a>> {
        let mut out: Vec<Origin<'a>> = builtins.versions(id).iter().map(Origin::Builtin).collect();
        out.extend(self.project.get(id).into_iter().flatten().map(|p| Origin::Project(*p)));
        out.sort_by_key(|o| o.r().version);
        out
    }

    /// The latest version of `id`.
    pub(crate) fn latest<'a>(&self, id: &TemplateId, builtins: &'a BuiltinRegistry) -> Option<Origin<'a>> {
        self.all(id, builtins).pop()
    }

    /// Version `version` of `id`, or the latest for `None`.
    pub(crate) fn get<'a>(
        &self,
        id: &TemplateId,
        version: Option<u32>,
        builtins: &'a BuiltinRegistry,
    ) -> Option<Origin<'a>> {
        match version {
            None => self.latest(id, builtins),
            Some(v) => self.all(id, builtins).into_iter().find(|o| o.r().version == v),
        }
    }

    /// Every template ID with a project version, ascending.
    pub(crate) fn project_ids(&self) -> impl Iterator<Item = &TemplateId> {
        self.project.keys()
    }
}
