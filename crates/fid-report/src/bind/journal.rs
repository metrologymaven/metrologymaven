//! Journal reads at an explicit head: the store seam ([`JournalReads`], [`MemoryJournal`])
//! and the typed [`Lookup`] over it ([`HeadLookup`]).
//!
//! The project store adapter (owned by the command bus, like [`crate::doc::ReportBackend`])
//! answers four questions: where a head sits in the journal, which head is current, and
//! for a stable ID or a statement digest, which project owns it, at which journal position
//! it was committed, what kind it is and its stored canonical bytes. [`HeadLookup`] turns
//! those answers into typed views as of exactly one head:
//!
//! 1. an entity of another project is [`LookupFault::CrossProject`], checked first, so
//!    nothing about the other project's journal (whether, when or as what the ID was
//!    committed) can be learned from the answer;
//! 2. an entity committed after the head is [`LookupFault::Unjournaled`];
//! 3. an absent entity, or one of another kind, is [`LookupFault::NotFound`].
//!
//! ResultRecords are read in their `fid.gdt-result/1` wire form (GDT.CHAR.RECORD): the
//! stored bytes must be canonical, and the record has one value field, `/value`, whose
//! uncertainty statement is the digest at `/uncertainty/statement` (`null` for UEC-0).
//! Record bytes never leave this module: a [`ResultView`] holds the record's ID, digest and
//! typed value fields only.

use super::error::BindError;
use super::model::{JournalHead, Quantity, QuantityUnit};
use crate::fmt::SiValue;
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Wire schema of a GDT ResultRecord.
pub const RESULT_RECORD_SCHEMA: &str = "fid.gdt-result/1";
/// The value field of a `fid.gdt-result/1` record.
pub const RECORD_VALUE_FIELD: &str = "/value";
/// Largest stored record or statement this module reads, in bytes.
pub const MAX_ENTITY_BYTES: usize = 1024 * 1024;
const MAX_ENTITY_DEPTH: usize = 32;

/// What a journaled entity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityKind {
    Result,
    Characteristic,
    NamedView,
    Series,
    Statement,
}

/// One journaled entity as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journaled {
    /// The owning project.
    pub project: Uuid,
    /// Journal position of the entry that committed it (0 is the first entry).
    pub seq: u64,
    pub kind: EntityKind,
    /// Stored canonical bytes.
    pub bytes: Vec<u8>,
}

/// Read access to the project store and its Record Journal.
pub trait JournalReads {
    /// Journal position of `head`, or `None` when it is no entry of this journal.
    fn head_seq(&self, head: &JournalHead) -> Option<u64>;
    /// The current head (`None` for an empty journal). Used by `report.bind_block@1` only:
    /// resolution always names its head.
    fn current_head(&self) -> Option<JournalHead>;
    /// The entity with this stable ID.
    fn entity(&self, id: &Uuid) -> Option<Journaled>;
    /// The stored uncertainty statement with this digest.
    fn statement(&self, digest: &Digest) -> Option<Journaled>;
}

/// An in-memory store and journal, for tests and tools. Each [`MemoryJournal::append`] is
/// one journal entry with its head.
#[derive(Debug, Clone, Default)]
pub struct MemoryJournal {
    heads: BTreeMap<JournalHead, u64>,
    order: Vec<JournalHead>,
    entities: BTreeMap<Uuid, Journaled>,
    statements: BTreeMap<Digest, Journaled>,
}

impl MemoryJournal {
    pub fn new() -> MemoryJournal {
        MemoryJournal::default()
    }

    /// Appends one entry committing an entity of `project`: `id` names it, except for a
    /// statement, which is keyed by the SHA-256 of `bytes`. A repeated head or ID is
    /// `report.store_invalid`.
    pub fn append(
        &mut self,
        head: JournalHead,
        project: Uuid,
        kind: EntityKind,
        id: Option<Uuid>,
        bytes: Vec<u8>,
    ) -> Result<(), BindError> {
        if self.heads.contains_key(&head) {
            return Err(BindError::StoreInvalid("repeated head"));
        }
        let seq = self.order.len() as u64;
        let entry = Journaled { project, seq, kind, bytes };
        match (kind, id) {
            (EntityKind::Statement, None) => {
                let d = Digest::from_bytes(digest_bytes(&entry.bytes).0);
                if self.statements.insert(d, entry).is_some() {
                    return Err(BindError::StoreInvalid("repeated statement"));
                }
            }
            (EntityKind::Statement, Some(_)) | (_, None) => return Err(BindError::StoreInvalid("entry key")),
            (_, Some(id)) => {
                if self.entities.contains_key(&id) {
                    return Err(BindError::StoreInvalid("repeated id"));
                }
                self.entities.insert(id, entry);
            }
        }
        self.heads.insert(head, seq);
        self.order.push(head);
        Ok(())
    }
}

impl JournalReads for MemoryJournal {
    fn head_seq(&self, head: &JournalHead) -> Option<u64> {
        self.heads.get(head).copied()
    }

    fn current_head(&self) -> Option<JournalHead> {
        self.order.last().copied()
    }

    fn entity(&self, id: &Uuid) -> Option<Journaled> {
        self.entities.get(id).cloned()
    }

    fn statement(&self, digest: &Digest) -> Option<Journaled> {
        self.statements.get(digest).cloned()
    }
}

// ------------------------------------------------------------------ typed lookup

/// Why a lookup failed. Callers add the block and the ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupFault {
    /// No such entity (or one of another kind) in this project.
    NotFound,
    /// Committed after the head.
    Unjournaled,
    /// Owned by this other project.
    CrossProject(Uuid),
    /// Stored bytes that are not a valid entity of the expected kind.
    Invalid(&'static str),
}

/// One value field of a record: its pointer, the typed quantity and the statement the
/// record references for it.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueField {
    pub field: &'static str,
    pub quantity: Quantity,
    pub statement: Option<Digest>,
}

/// A ResultRecord as of a head: its ID, the SHA-256 of its stored bytes, and its value
/// fields as typed quantities.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultView {
    pub id: Uuid,
    pub digest: Digest,
    pub values: Vec<ValueField>,
}

impl ResultView {
    /// The value field at `pointer`, if `pointer` names one.
    pub fn value_field(&self, pointer: &str) -> Option<&ValueField> {
        self.values.iter().find(|v| v.field == pointer)
    }
}

/// A journaled entity whose content its block kind reads (characteristic, saved view,
/// statistics series): its ID and the SHA-256 of its stored bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityView {
    pub id: Uuid,
    pub digest: Digest,
}

/// A stored uncertainty statement whose bytes hash to [`StatementBytes::digest`]. The bytes
/// are for `fid_kunc::UncertaintyStatement::parse_canonical_verified` (in `RPT.MODEL.FMT`);
/// nothing here reads a number from them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementBytes {
    digest: Digest,
    bytes: Vec<u8>,
}

impl StatementBytes {
    pub fn digest(&self) -> Digest {
        self.digest
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Typed reads as of one journal head.
pub trait Lookup {
    fn result(&self, id: &Uuid) -> Result<ResultView, LookupFault>;
    fn characteristic(&self, id: &Uuid) -> Result<EntityView, LookupFault>;
    fn named_view(&self, id: &Uuid) -> Result<EntityView, LookupFault>;
    fn series(&self, id: &Uuid) -> Result<EntityView, LookupFault>;
    /// The statement with this digest; its stored bytes must hash to it.
    fn statement(&self, digest: &Digest) -> Result<StatementBytes, LookupFault>;
}

/// [`Lookup`] over [`JournalReads`] at exactly one head of one project.
pub struct HeadLookup<'a> {
    journal: &'a dyn JournalReads,
    project: Uuid,
    seq: u64,
}

impl<'a> HeadLookup<'a> {
    /// `report.not_found` (naming the head) when `head` is no entry of the journal.
    pub fn new(journal: &'a dyn JournalReads, project: Uuid, head: &JournalHead) -> Result<HeadLookup<'a>, BindError> {
        let seq = journal.head_seq(head).ok_or_else(|| BindError::NotFound(head.digest().as_str().to_string()))?;
        Ok(HeadLookup { journal, project, seq })
    }

    /// Project first, then position, then kind (see the module documentation).
    fn visible(&self, found: Option<Journaled>, kind: EntityKind) -> Result<Journaled, LookupFault> {
        let j = found.ok_or(LookupFault::NotFound)?;
        if j.project != self.project {
            return Err(LookupFault::CrossProject(j.project));
        }
        if j.seq > self.seq {
            return Err(LookupFault::Unjournaled);
        }
        if j.kind != kind {
            return Err(LookupFault::NotFound);
        }
        Ok(j)
    }

    fn entity_view(&self, id: &Uuid, kind: EntityKind) -> Result<EntityView, LookupFault> {
        let j = self.visible(self.journal.entity(id), kind)?;
        Ok(EntityView { id: *id, digest: sha256(&j.bytes) })
    }
}

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

impl Lookup for HeadLookup<'_> {
    fn result(&self, id: &Uuid) -> Result<ResultView, LookupFault> {
        let j = self.visible(self.journal.entity(id), EntityKind::Result)?;
        let values = read_record(&j.bytes)?;
        Ok(ResultView { id: *id, digest: sha256(&j.bytes), values })
    }

    fn characteristic(&self, id: &Uuid) -> Result<EntityView, LookupFault> {
        self.entity_view(id, EntityKind::Characteristic)
    }

    fn named_view(&self, id: &Uuid) -> Result<EntityView, LookupFault> {
        self.entity_view(id, EntityKind::NamedView)
    }

    fn series(&self, id: &Uuid) -> Result<EntityView, LookupFault> {
        self.entity_view(id, EntityKind::Series)
    }

    fn statement(&self, digest: &Digest) -> Result<StatementBytes, LookupFault> {
        let j = self.visible(self.journal.statement(digest), EntityKind::Statement)?;
        if sha256(&j.bytes) != *digest {
            return Err(LookupFault::Invalid("statement digest"));
        }
        Ok(StatementBytes { digest: *digest, bytes: j.bytes })
    }
}

/// The value fields of a stored `fid.gdt-result/1` record (canonical bytes required).
fn read_record(bytes: &[u8]) -> Result<Vec<ValueField>, LookupFault> {
    let bad = LookupFault::Invalid("record");
    let v = parse_with_limits(bytes, MAX_ENTITY_BYTES, MAX_ENTITY_DEPTH).map_err(|_| bad)?;
    if to_canonical_bytes(&v).map_err(|_| bad)? != bytes {
        return Err(LookupFault::Invalid("record not canonical"));
    }
    let CValue::Obj(o) = &v else { return Err(bad) };
    if o.get("schema") != Some(&CValue::Str(RESULT_RECORD_SCHEMA.to_string())) {
        return Err(LookupFault::Invalid("record schema"));
    }
    let Some(CValue::Obj(value)) = o.get("value") else { return Err(bad) };
    let (Some(CValue::Str(bits)), Some(CValue::Str(unit)), 2) = (value.get("value"), value.get("unit"), value.len())
    else {
        return Err(bad);
    };
    let unit = QuantityUnit::parse(unit).ok_or(bad)?;
    let bits = bits
        .strip_prefix("0x")
        .filter(|h| h.len() == 16 && h.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    let bits = bits.and_then(|h| u64::from_str_radix(h, 16).ok()).ok_or(bad)?;
    let Some(CValue::Obj(unc)) = o.get("uncertainty") else { return Err(bad) };
    let statement = match unc.get("statement") {
        Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(Digest::parse(s).map_err(|_| bad)?),
        _ => return Err(bad),
    };
    Ok(vec![ValueField {
        field: RECORD_VALUE_FIELD,
        quantity: Quantity { value: SiValue::from_bits(bits), unit },
        statement,
    }])
}
