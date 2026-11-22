//! Storage seam, built-in registry and the project template index.
//!
//! - [`ObjectRead`] is the content-addressed store's **safe object read** (CORE.STO.CAS):
//!   the whole object whose SHA-256 is the digest, never a chunk manifest or a raw blob.
//!   Callers here still re-hash what they get, so a backend that serves anything else fails
//!   closed (`report.store_invalid`).
//! - [`TemplateBackend`] adds the two commits of the W2 commands, each atomic: object plus
//!   event (save, with a compare-and-set on the template ID's latest project version), or
//!   object plus stored report plus event (apply). There is no call that replaces or
//!   deletes an object, so stored template versions are immutable (REQ-002).
//! - [`BuiltinRegistry`] holds the templates crates ship (starters and forms), each at an
//!   explicit version: `builtin:<id>@<version>`.
//! - [`TemplateIndex`] is the project index `BTreeMap<TemplateId, Vec<TemplateRef>>`, folded
//!   from the journal's save events only (never stored separately, and independent of the
//!   built-ins, so a new release cannot invalidate it). Project versions are `<id>@<version>`,
//!   numbered 1, 2, 3, ... per ID with no gap.

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

/// The latest project version a save was prepared against: `(version, digest)`, or `None`
/// for a template ID with no project version yet.
pub type Expected = Option<(u32, Digest)>;

/// The project a template command runs against: reports, the content-addressed store and
/// the journal.
pub trait TemplateBackend: ReportBackend + ObjectRead {
    /// The template events of the journal, oldest first.
    fn template_events(&self) -> Result<Vec<TemplateEvent>, ReportError>;
    /// Stores `object` (idempotent: an object already stored is left as it is) and appends
    /// the save event, both or neither. The commit stage checks, atomically with the
    /// append, that the event's template ID still has `expected` as its latest project
    /// version; otherwise it commits nothing and fails with `conflict/head_moved`
    /// ([`VerError::HeadMoved`]), so two concurrent saves can never both become version n+1.
    fn commit_save(&mut self, object: Vec<u8>, expected: Expected, event: TemplateEvent) -> Result<(), VerError>;
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

    fn commit_save(&mut self, object: Vec<u8>, expected: Expected, event: TemplateEvent) -> Result<(), VerError> {
        let TemplateEvent::Save { template, .. } = &event else {
            return Err(ReportError::Backend("not a save event").into());
        };
        if digest_of(&object) != template.digest {
            return Err(ReportError::Backend("event does not match the object").into());
        }
        // The compare-and-set: the latest project version now, under the same exclusive
        // borrow as the append below.
        let id = TemplateId::new(template.id).ok_or(ReportError::StoreInvalid("template id"))?;
        let mut ix = TemplateIndex::fold(&self.template_events()?)?;
        let now = ix.latest(&id).map(|r| (r.version, r.digest));
        if now != expected {
            return Err(VerError::HeadMoved(id));
        }
        ix.push(*template)?;
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

/// The templates crates ship (starters and fixed forms): `builtin:<id>@<version>`, each
/// version given explicitly by the shipping crate, so the numbers do not depend on
/// registration order. Built-in versions are a number space of their own; project saves
/// never use or collide with them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltinRegistry {
    entries: BTreeMap<TemplateId, BTreeMap<u32, Builtin>>,
}

impl BuiltinRegistry {
    pub fn new() -> BuiltinRegistry {
        BuiltinRegistry::default()
    }

    /// A built-in registry pre-populated with the three unlocked starter templates at version 1
    /// (`RPT.MODEL.TMPL.STARTER`).
    pub fn with_starters() -> Result<BuiltinRegistry, VerError> {
        crate::tmpl::starter::starter_registry()
    }

    /// Registers `template` as `builtin:<its id>@<version>` and returns its reference.
    /// Registering the same bytes at the same version again returns the same reference;
    /// other bytes at a registered version, or version 0, are `report.bad_params`. Bounded
    /// by [`MAX_TEMPLATE_IDS`] and [`MAX_VERSIONS`] (`report.limit_exceeded`).
    pub fn register(&mut self, template: Template, version: u32) -> Result<TemplateRef, VerError> {
        if version == 0 {
            return Err(ReportError::BadParams("built-in version 0").into());
        }
        let bytes = serialise(&template)?.into_vec();
        let digest = digest_of(&bytes);
        let id = template.template_id();
        if !self.entries.contains_key(&id) && self.entries.len() >= MAX_TEMPLATE_IDS {
            return Err(ReportError::LimitExceeded("template ids").into());
        }
        let versions = self.entries.entry(id).or_default();
        if let Some(b) = versions.get(&version) {
            if b.r.digest == digest {
                return Ok(b.r);
            }
            return Err(ReportError::BadParams("built-in version already registered with other bytes").into());
        }
        if versions.len() >= MAX_VERSIONS {
            return Err(ReportError::LimitExceeded("template versions").into());
        }
        let r = TemplateRef { id: id.uuid(), version, digest, builtin: true };
        versions.insert(version, Builtin { r, locked: template.locked(), bytes });
        Ok(r)
    }

    /// The built-in versions of `id`, ascending.
    pub(crate) fn versions(&self, id: &TemplateId) -> impl Iterator<Item = &Builtin> {
        self.entries.get(id).into_iter().flat_map(BTreeMap::values)
    }

    /// `builtin:<id>@<version>`, or the latest built-in version for `None`.
    pub(crate) fn get(&self, id: &TemplateId, version: Option<u32>) -> Option<&Builtin> {
        let v = self.entries.get(id)?;
        match version {
            None => v.values().next_back(),
            Some(n) => v.get(&n),
        }
    }

    /// Whether any built-in version of `id` is locked (REQ-003).
    pub(crate) fn is_locked(&self, id: &TemplateId) -> bool {
        self.versions(id).any(|b| b.locked)
    }

    /// Every built-in template ID, ascending.
    pub(crate) fn ids(&self) -> impl Iterator<Item = &TemplateId> {
        self.entries.keys()
    }
}

// ------------------------------------------------------------------ project index

/// Where a resolved version lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin<'a> {
    Builtin(&'a Builtin),
    Project(TemplateRef),
}

impl Origin<'_> {
    pub(crate) fn r(&self) -> TemplateRef {
        match self {
            Origin::Builtin(b) => b.r,
            Origin::Project(r) => *r,
        }
    }
}

/// The project index, folded from save events alone. Per ID, project versions are 1, 2,
/// 3, ... in journal order with no gap or repeat; anything else is `report.store_invalid`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateIndex {
    project: BTreeMap<TemplateId, Vec<TemplateRef>>,
}

impl TemplateIndex {
    /// Folds the save events of `events` (apply events are skipped).
    pub(crate) fn fold(events: &[TemplateEvent]) -> Result<TemplateIndex, ReportError> {
        let mut ix = TemplateIndex::default();
        for e in events {
            if let TemplateEvent::Save { template, .. } = e {
                ix.push(*template)?;
            }
        }
        Ok(ix)
    }

    /// The version a new save of `id` gets: the latest project version plus one, or 1.
    pub(crate) fn next_version(&self, id: &TemplateId) -> Result<u32, ReportError> {
        match self.latest(id) {
            None => Ok(1),
            Some(l) => l.version.checked_add(1).ok_or(ReportError::LimitExceeded("template versions")),
        }
    }

    /// Adds a project version (fold, commit check and replay).
    pub(crate) fn push(&mut self, r: TemplateRef) -> Result<(), ReportError> {
        let id = TemplateId::new(r.id).ok_or(ReportError::StoreInvalid("template id"))?;
        if r.builtin {
            return Err(ReportError::StoreInvalid("a save names a built-in version"));
        }
        if r.version != self.next_version(&id)? {
            return Err(ReportError::StoreInvalid("project versions are not consecutive"));
        }
        if !self.project.contains_key(&id) && self.project.len() >= MAX_TEMPLATE_IDS {
            return Err(ReportError::LimitExceeded("template ids"));
        }
        let versions = self.project.entry(id).or_default();
        if versions.len() >= MAX_VERSIONS {
            return Err(ReportError::LimitExceeded("template versions"));
        }
        versions.push(r);
        Ok(())
    }

    /// The latest project version of `id`.
    pub(crate) fn latest(&self, id: &TemplateId) -> Option<TemplateRef> {
        self.project.get(id).and_then(|v| v.last()).copied()
    }

    /// Project version `version` of `id`, or the latest for `None`.
    pub(crate) fn get(&self, id: &TemplateId, version: Option<u32>) -> Option<TemplateRef> {
        match version {
            None => self.latest(id),
            Some(n) => self.project.get(id)?.iter().find(|r| r.version == n).copied(),
        }
    }

    /// Every project version of `id`, ascending.
    pub(crate) fn versions(&self, id: &TemplateId) -> &[TemplateRef] {
        self.project.get(id).map_or(&[], Vec::as_slice)
    }

    /// Every template ID with a project version, ascending.
    pub(crate) fn ids(&self) -> impl Iterator<Item = &TemplateId> {
        self.project.keys()
    }

    /// Resolves `<id>@<version>` (project) or `builtin:<id>@<version>`; latest for `None`.
    pub(crate) fn resolve<'a>(
        &self,
        id: &TemplateId,
        version: Option<u32>,
        builtin: bool,
        builtins: &'a BuiltinRegistry,
    ) -> Option<Origin<'a>> {
        if builtin {
            builtins.get(id, version).map(Origin::Builtin)
        } else {
            self.get(id, version).map(Origin::Project)
        }
    }
}
