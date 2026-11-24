//! Catalog integrity check and content digest (spec node `RPT.CATALOG.CHECK`).
//!
//! The catalog is only an index of its projects' journals. [`check_catalog`] compares every
//! entry with its source project and writes the derived entry status back: `ok` when the
//! record is present at the stored journal entry and that entry's hash equals the stored hash,
//! `stale` when the entry or the record is missing there or the hash differs, and `unavailable`
//! when the project (or that one journal entry) cannot be read. [`content_digest`] is the
//! catalog content digest of REQ-RPT.CATALOG.CHECK-006.
//!
//! **Partial build.** How a registered project's handle is resolved and opened
//! (`RPT.CATALOG.STORE.REG`) and how a ResultRecord is found inside a journal entry
//! (`RPT.CATALOG.STORE.INDEX`) belong to leaves that are not built yet, so the check reads
//! projects through the [`ProjectSources`] and [`RecordLookup`] traits. The journal-backed
//! implementation, `stats.rebuild_catalog@1` and the registry commands and CLI land with
//! those leaves (see the leaf's Open questions).
//!
//! **Digest.** SHA-256 of the RFC 8785 canonical JSON array of every entry, sorted by
//! (project ID, record ID) bytewise, each entry an object of its columns except
//! `entry_status` (the catalog holds no catalog-local timestamp; `timestamp` is the record's
//! own instant and is included), plus its tags as a sorted array. Project number rules of
//! `fid-canon` apply: `journal_seq` is a decimal string, `si` is the `"0x"` hex of its IEEE 754
//! bits. The digest is streamed page by page, so memory does not grow with the catalog.

use crate::schema::{
    read_tags, Catalog, CatalogEntryRow, CatalogError, CatalogText, EntryStatus, Mode, ProjectRow, MAX_PAGE,
};
use fid_canon::CValue;
use fid_types::Digest;
use fid_uuid::Uuid;
use rusqlite::{params, Statement};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// What a project's journal holds for one catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRecord {
    /// The journal entry at the stored position exists and carries the record; its hash.
    Present { entry_hash: Digest },
    /// No journal entry at the stored position, or that entry does not carry the record.
    Absent,
}

/// The journal entry could not be read (I/O failure, a damaged or locked project database).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceUnreadable;

/// Read-only lookups in one opened source project.
pub trait RecordLookup {
    /// The record `record` at journal position `seq` of the project's Record Journal.
    fn record_at(&mut self, seq: u64, record: &CatalogText) -> Result<SourceRecord, SourceUnreadable>;
}

/// Opens registered projects read-only. An implementation must never write to a project.
pub trait ProjectSources {
    /// Opens `project` read-only, or `None` when it cannot be reached (moved, deleted, not a
    /// project, or a different project at the handle).
    fn open(&mut self, project: &ProjectRow) -> Option<Box<dyn RecordLookup + '_>>;
}

/// The key of a catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryKey {
    pub project_id: Uuid,
    pub record_id: CatalogText,
}

/// Entry counts of one checked project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectCounts {
    pub project_id: Uuid,
    pub ok: u64,
    pub stale: u64,
    pub unavailable: u64,
}

/// The result of [`check_catalog`] (`CheckReport` of `stats.check_catalog@1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// One row per checked project, in project-ID order (projects without entries included).
    pub per_project: Vec<ProjectCounts>,
    /// Every entry found `stale`, in (project ID, record ID) order.
    pub stale: Vec<EntryKey>,
    /// The catalog content digest ([`content_digest`]) over the whole catalog.
    pub digest: Digest,
}

/// Errors of the check.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CheckError {
    /// The catalog could not be read or written.
    Catalog(CatalogError),
    /// The project filter names a project that is not registered.
    UnknownProject(Uuid),
    /// A canonical form could not be written (a non-finite SI value read from the file).
    Canon(String),
}

impl CheckError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CheckError::Catalog(e) => e.code(),
            CheckError::UnknownProject(_) => "stats.catalog_unknown_project",
            CheckError::Canon(_) => "stats.catalog_corrupt",
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Catalog(e) => e.fmt(f),
            CheckError::UnknownProject(p) => write!(f, "{}: project {p} is not registered", self.code()),
            CheckError::Canon(d) => write!(f, "{}: {d}", self.code()),
        }
    }
}

impl std::error::Error for CheckError {}

impl From<CatalogError> for CheckError {
    fn from(e: CatalogError) -> CheckError {
        CheckError::Catalog(e)
    }
}

impl From<rusqlite::Error> for CheckError {
    fn from(e: rusqlite::Error) -> CheckError {
        CheckError::Catalog(CatalogError::from(e))
    }
}

fn text(t: &CatalogText) -> CValue {
    CValue::Str(t.raw().to_string())
}

fn opt_text(t: Option<&CatalogText>) -> CValue {
    t.map_or(CValue::Null, text)
}

/// The canonical JSON object of one entry in the content digest (no `entry_status`).
pub fn entry_value(e: &CatalogEntryRow, tags: &[CatalogText]) -> CValue {
    let mut o = BTreeMap::new();
    let mut put = |k: &str, v: CValue| {
        o.insert(k.to_string(), v);
    };
    put("project_id", CValue::Str(e.project_id.to_string()));
    put("record_id", text(&e.record_id));
    put("part", text(&e.part));
    put("revision", opt_text(e.revision.as_ref()));
    put("plan", text(&e.plan));
    put("run", text(&e.run));
    put("stage", text(&e.stage));
    put("characteristic", text(&e.characteristic));
    put("timestamp", text(&e.timestamp));
    let value = match &e.value {
        Some(v) => {
            let mut vo = BTreeMap::new();
            vo.insert("literal".to_string(), text(&v.literal));
            vo.insert("unit".to_string(), text(&v.unit));
            // One form for zero, as the catalog stores it.
            let si = if v.si == 0.0 { 0.0 } else { v.si };
            vo.insert("si".to_string(), CValue::F64Bits(si));
            CValue::Obj(vo)
        }
        None => CValue::Null,
    };
    put("value", value);
    put("decision_outcome", CValue::Str(e.decision_outcome.as_str().to_string()));
    put("rule_id", opt_text(e.rule_id.as_ref()));
    put("evaluation_basis", CValue::Str(e.evaluation_basis.as_str().to_string()));
    put("uec_class", CValue::Str(e.uec_class.as_str().to_string()));
    put("claimed", CValue::Bool(e.labels.claimed));
    put("declared_input", CValue::Bool(e.labels.declared_input));
    put("uncharacterized_input", CValue::Bool(e.labels.uncharacterized_input));
    put("interpretation", CValue::Str(e.interpretation.as_str().to_string()));
    put("record_status", CValue::Str(e.record_status.as_str().to_string()));
    put("journal_seq", CValue::U64(e.journal_seq));
    put("journal_entry_hash", CValue::Str(e.journal_entry_hash.as_str().to_string()));
    put("tags", CValue::Arr(tags.iter().map(text).collect()));
    CValue::Obj(o)
}

/// Streams the canonical JSON array of entries into a SHA-256.
struct DigestWriter {
    hasher: Sha256,
    first: bool,
}

impl DigestWriter {
    fn new() -> DigestWriter {
        let mut hasher = Sha256::new();
        hasher.update(b"[");
        DigestWriter { hasher, first: true }
    }

    fn push(&mut self, tag_st: &mut Statement<'_>, e: &CatalogEntryRow) -> Result<(), CheckError> {
        let tags = read_tags(tag_st, &e.project_id, &e.record_id)?;
        let bytes = fid_canon::to_canonical_bytes(&entry_value(e, &tags))
            .map_err(|err| CheckError::Canon(format!("entry cannot be written canonically: {err}")))?;
        if !self.first {
            self.hasher.update(b",");
        }
        self.first = false;
        self.hasher.update(&bytes);
        Ok(())
    }

    fn finish(mut self) -> Digest {
        self.hasher.update(b"]");
        Digest::from_bytes(self.hasher.finalize().into())
    }
}

/// Calls `f` on every entry in (project ID, record ID) order, one page at a time.
fn each_entry(
    catalog: &Catalog,
    mut f: impl FnMut(&CatalogEntryRow) -> Result<(), CheckError>,
) -> Result<(), CheckError> {
    let mut after: Option<(Uuid, CatalogText)> = None;
    loop {
        let page = catalog.entries_after(after.as_ref().map(|(p, r)| (p, r)), MAX_PAGE)?;
        let Some(last) = page.last() else { return Ok(()) };
        let next = (last.project_id, last.record_id.clone());
        for e in &page {
            f(e)?;
        }
        if page.len() < MAX_PAGE {
            return Ok(());
        }
        after = Some(next);
    }
}

/// The catalog content digest (REQ-RPT.CATALOG.CHECK-003, -006): independent of insertion
/// order and of every entry status.
pub fn content_digest(catalog: &Catalog) -> Result<Digest, CheckError> {
    // One prepared tag query for the whole pass.
    let mut tag_st = catalog.tag_statement()?;
    let mut w = DigestWriter::new();
    each_entry(catalog, |e| w.push(&mut tag_st, e))?;
    Ok(w.finish())
}

/// The status one entry gets from its source (`None`: the project could not be opened).
fn judge(lookup: Option<&mut (dyn RecordLookup + '_)>, e: &CatalogEntryRow) -> EntryStatus {
    let Some(l) = lookup else { return EntryStatus::Unavailable };
    match l.record_at(e.journal_seq, &e.record_id) {
        Ok(SourceRecord::Present { entry_hash }) if entry_hash == e.journal_entry_hash => EntryStatus::Ok,
        Ok(_) => EntryStatus::Stale,
        Err(SourceUnreadable) => EntryStatus::Unavailable,
    }
}

/// Checks every entry of every registered project (or of `project` only), writes the entry
/// statuses back and reports the counts, the stale entries and the content digest
/// (REQ-RPT.CATALOG.CHECK-001, -002).
///
/// Projects are opened through `sources`, read-only, once each, in project-ID order. The whole
/// check runs in one `BEGIN IMMEDIATE` transaction, so no indexing pass can change an entry
/// between its comparison and its status write; a held lock is `stats.catalog_locked`, and a
/// failure leaves every status as it was. Only the derived `entry_status` column changes (it is
/// not part of the digest). A read-only open is refused with `stats.catalog_read_only` before
/// anything is read.
pub fn check_catalog(
    catalog: &mut Catalog,
    sources: &mut dyn ProjectSources,
    project: Option<&Uuid>,
) -> Result<CheckReport, CheckError> {
    if catalog.mode() != Mode::ReadWrite {
        return Err(
            CatalogError::ReadOnly("the catalog check writes entry statuses; open it for writing".into()).into()
        );
    }
    catalog.connection().execute_batch("BEGIN IMMEDIATE")?;
    let result = run_check(catalog, sources, project);
    let end = if result.is_ok() { "COMMIT" } else { "ROLLBACK" };
    match catalog.connection().execute_batch(end) {
        Ok(()) => result,
        Err(e) => {
            let _ = catalog.connection().execute_batch("ROLLBACK");
            // The check's own error, if any, says more than the failed rollback.
            result.and(Err(e.into()))
        }
    }
}

/// Refuses a catalog holding entries of an unregistered project (a file edited outside this
/// crate with foreign keys off): such an entry belongs to no checked project, so it would keep
/// whatever status it had, `ok` included.
fn refuse_orphans(catalog: &Catalog) -> Result<(), CheckError> {
    let mut st = catalog.connection().prepare("PRAGMA foreign_key_check(entry)")?;
    let mut rows = st.query([])?;
    if rows.next()?.is_some() {
        return Err(
            CatalogError::Corrupt("the catalog holds entries of a project that is not registered".into()).into()
        );
    }
    Ok(())
}

fn run_check(
    catalog: &Catalog,
    sources: &mut dyn ProjectSources,
    project: Option<&Uuid>,
) -> Result<CheckReport, CheckError> {
    refuse_orphans(catalog)?;
    let projects = catalog.projects()?;
    if let Some(p) = project {
        if !projects.iter().any(|r| r.project_id == *p) {
            return Err(CheckError::UnknownProject(*p));
        }
    }
    let mut update =
        catalog.connection().prepare("UPDATE entry SET entry_status = ?1 WHERE project_id = ?2 AND record_id = ?3")?;
    let mut per_project = Vec::new();
    let mut stale = Vec::new();
    for row in projects.iter().filter(|r| project.map_or(true, |p| r.project_id == *p)) {
        let mut counts = ProjectCounts { project_id: row.project_id, ok: 0, stale: 0, unavailable: 0 };
        let mut lookup = sources.open(row);
        let mut after: Option<CatalogText> = None;
        loop {
            let page = catalog.project_entries_after(&row.project_id, after.as_ref(), MAX_PAGE)?;
            for e in &page {
                let status = judge(lookup.as_deref_mut(), e);
                match status {
                    EntryStatus::Ok => counts.ok += 1,
                    EntryStatus::Stale => {
                        counts.stale += 1;
                        stale.push(EntryKey { project_id: e.project_id, record_id: e.record_id.clone() });
                    }
                    _ => counts.unavailable += 1,
                }
                // Written page by page, inside the caller's transaction.
                if status != e.entry_status {
                    let n = update.execute(params![status.as_str(), e.project_id.to_string(), e.record_id.raw()])?;
                    if n != 1 {
                        return Err(
                            CatalogError::Corrupt(format!("entry status update changed {n} rows, expected 1")).into()
                        );
                    }
                }
            }
            match page.last() {
                Some(last) if page.len() == MAX_PAGE => after = Some(last.record_id.clone()),
                _ => break,
            }
        }
        per_project.push(counts);
    }
    drop(update);
    let digest = content_digest(catalog)?;
    Ok(CheckReport { per_project, stale, digest })
}
