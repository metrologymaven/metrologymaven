//! Journal reads at an explicit head: the store seam ([`JournalReads`], [`MemoryJournal`])
//! and the typed [`Lookup`] over it ([`HeadLookup`]).
//!
//! The project store adapter (owned by the command bus, like [`crate::doc::ReportBackend`])
//! answers: where a head sits in the journal, which head is current, and for a stable ID
//! (or a project's statement digest) what the store held *as of a journal position*
//! ([`AtHead`]): the version committed at or before it, or the fact that the entity was
//! committed only later (with its owner and kind), or nothing. [`HeadLookup`] turns those
//! answers into typed views as of exactly one head:
//!
//! Order: ownership, then kind, then position.
//!
//! 1. ownership is checked for ResultRecords and characteristics only (the leaf's Open
//!    question: cross-project data enters reports only as statistics series, so saved views
//!    and series are not owner-checked here); an entity of another project is
//!    [`LookupFault::CrossProject`], checked before position, so nothing about the other
//!    project's journal (whether, when or as what the ID was committed) can be learned;
//! 2. an absent entity, or one of another kind (committed yet or not), is
//!    [`LookupFault::NotFound`];
//! 3. an entity committed only after the head is [`LookupFault::Unjournaled`].
//!
//! Statements are looked up by (project, digest): the same statement bytes may be stored by
//! two projects.
//!
//! ResultRecords are read in their `fid.gdt-result/1` wire form (GDT.CHAR.RECORD): the
//! stored bytes must be canonical, and the record has one value field, `/value`, whose
//! uncertainty statement is the digest at `/uncertainty/statement` (`null` for UEC-0). The
//! record's evidence facts (evaluation basis, decision, evidence class and method, and the
//! claimed / declared-input / uncharacterized-input facts) are read into [`Evidence`], so
//! no downstream kind ever has to invent them; the referenced statement is parsed
//! (`fid_kunc::UncertaintyStatement::parse_canonical_verified`) and its class, method and
//! claim facts must agree with the record's, else the record is `Invalid`. Record bytes never leave this module.

use super::error::BindError;
use super::model::{JournalHead, Quantity, QuantityUnit};
use crate::fmt::SiValue;
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_gdt::chars::record::EvaluationBasis;
use fid_kunc::uec::{RecordTable, SourceRecords};
use fid_kunc::UncertaintyStatement;
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Wire schema of a GDT ResultRecord without the interpretation block.
pub const RESULT_RECORD_SCHEMA: &str = "fid.gdt-result/1";
/// Wire schema of a GDT ResultRecord with the interpretation block (`GDT.CHAR.INTERP`). Its
/// value fields and evidence read exactly like `/1`; whether it needs the provisional label
/// is `fid_gdt::requires_provisional_label_bytes` (a `/1` record always does).
pub const RESULT_RECORD_SCHEMA_V2: &str = "fid.gdt-result/2";
/// The value field of a `fid.gdt-result/1` record.
pub const RECORD_VALUE_FIELD: &str = "/value";
/// Largest stored record or statement this module reads, in bytes.
pub const MAX_ENTITY_BYTES: usize = 1024 * 1024;
/// Most versions [`MemoryJournal`] keeps per entity.
pub const MAX_VERSIONS: usize = 1024;
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

/// One journaled version of an entity as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journaled {
    /// The owning project.
    pub project: Uuid,
    /// Journal position of the entry that committed this version (0 is the first entry).
    pub seq: u64,
    pub kind: EntityKind,
    /// Stored canonical bytes of this version.
    pub bytes: Vec<u8>,
}

/// What the store held for one key as of a journal position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtHead {
    /// Never committed.
    Absent,
    /// First committed after the position, by `project`, as `kind`.
    NotYet { project: Uuid, kind: EntityKind },
    /// The newest version committed at or before the position.
    Committed(Journaled),
}

/// Read access to the project store and its Record Journal.
pub trait JournalReads {
    /// Journal position of `head`, or `None` when it is no entry of this journal.
    fn head_seq(&self, head: &JournalHead) -> Option<u64>;
    /// The current head (`None` for an empty journal). Used by `report.bind_block@1` only:
    /// resolution always names its head.
    fn current_head(&self) -> Option<JournalHead>;
    /// The entity with this stable ID as of journal position `seq`.
    fn entity(&self, id: &Uuid, seq: u64) -> AtHead;
    /// `project`'s stored uncertainty statement with this digest as of position `seq`.
    fn statement(&self, project: &Uuid, digest: &Digest, seq: u64) -> AtHead;
    /// The source records that confirm UEC-2 and UEC-3 statements (UNC.UEC.CLASS). Without
    /// them, a stored UEC-2/3 statement does not parse (`source_not_verified`) and its
    /// record is refused.
    fn source_records(&self) -> Option<&dyn SourceRecords> {
        None
    }
}

/// An in-memory store and journal, for tests and tools. Each [`MemoryJournal::append`] is
/// one journal entry with its head; appending an existing ID again commits a new version.
#[derive(Debug, Clone, Default)]
pub struct MemoryJournal {
    heads: BTreeMap<JournalHead, u64>,
    order: Vec<JournalHead>,
    entities: BTreeMap<Uuid, Vec<Journaled>>,
    statements: BTreeMap<(Uuid, Digest), Journaled>,
}

impl MemoryJournal {
    pub fn new() -> MemoryJournal {
        MemoryJournal::default()
    }

    /// Appends one entry committing an entity of `project`: `id` names it, except for a
    /// statement, which is keyed by (project, SHA-256 of `bytes`). A repeated head or
    /// statement, a new version of an ID under another project or kind, or more than
    /// [`MAX_VERSIONS`] versions is `report.store_invalid`.
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
                if self.statements.insert((project, d), entry).is_some() {
                    return Err(BindError::StoreInvalid("repeated statement"));
                }
            }
            (EntityKind::Statement, Some(_)) | (_, None) => return Err(BindError::StoreInvalid("entry key")),
            (_, Some(id)) => {
                let versions = self.entities.entry(id).or_default();
                if let Some(first) = versions.first() {
                    if first.project != project || first.kind != kind {
                        return Err(BindError::StoreInvalid("id reused"));
                    }
                }
                if versions.len() >= MAX_VERSIONS {
                    return Err(BindError::StoreInvalid("too many versions"));
                }
                versions.push(entry);
            }
        }
        self.heads.insert(head, seq);
        self.order.push(head);
        Ok(())
    }
}

fn at(versions: &[Journaled], seq: u64) -> AtHead {
    match (versions.iter().rev().find(|v| v.seq <= seq), versions.first()) {
        (Some(v), _) => AtHead::Committed(v.clone()),
        (None, Some(f)) => AtHead::NotYet { project: f.project, kind: f.kind },
        (None, None) => AtHead::Absent,
    }
}

impl JournalReads for MemoryJournal {
    fn head_seq(&self, head: &JournalHead) -> Option<u64> {
        self.heads.get(head).copied()
    }

    fn current_head(&self) -> Option<JournalHead> {
        self.order.last().copied()
    }

    fn entity(&self, id: &Uuid, seq: u64) -> AtHead {
        self.entities.get(id).map_or(AtHead::Absent, |v| at(v, seq))
    }

    fn statement(&self, project: &Uuid, digest: &Digest, seq: u64) -> AtHead {
        self.statements.get(&(*project, *digest)).map_or(AtHead::Absent, |v| at(std::slice::from_ref(v), seq))
    }
}

// ------------------------------------------------------------------ evidence

/// Decision outcome of a record (CONSTITUTION §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    Conforms,
    Nonconforms,
    Indeterminate,
    NotDecidable,
}

impl Outcome {
    pub fn parse(s: &str) -> Option<Outcome> {
        match s {
            "CONFORMS" => Some(Outcome::Conforms),
            "NONCONFORMS" => Some(Outcome::Nonconforms),
            "INDETERMINATE" => Some(Outcome::Indeterminate),
            "NOT_DECIDABLE" => Some(Outcome::NotDecidable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Conforms => "CONFORMS",
            Outcome::Nonconforms => "NONCONFORMS",
            Outcome::Indeterminate => "INDETERMINATE",
            Outcome::NotDecidable => "NOT_DECIDABLE",
        }
    }
}

/// Uncertainty evidence class (UNC.UEC): UEC-0 means "not evaluated".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceClass {
    Uec0,
    Uec1,
    Uec2,
    Uec3,
}

impl EvidenceClass {
    pub fn parse(s: &str) -> Option<EvidenceClass> {
        match s {
            "UEC-0" => Some(EvidenceClass::Uec0),
            "UEC-1" => Some(EvidenceClass::Uec1),
            "UEC-2" => Some(EvidenceClass::Uec2),
            "UEC-3" => Some(EvidenceClass::Uec3),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceClass::Uec0 => "UEC-0",
            EvidenceClass::Uec1 => "UEC-1",
            EvidenceClass::Uec2 => "UEC-2",
            EvidenceClass::Uec3 => "UEC-3",
        }
    }
}

/// The evidence facts of a journaled ResultRecord, read from its stored bytes: how the
/// value was obtained and what its uncertainty and decision rest on. A claimed,
/// declared-input, uncharacterized-input, UEC-0, `RESULTS_ONLY` or `DIRECT_READING` value
/// therefore never looks measured ([`Evidence::looks_measured`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    basis: EvaluationBasis,
    class: EvidenceClass,
    method: Option<String>,
    claimed: bool,
    declared_input: bool,
    uncharacterized_input: bool,
    outcome: Outcome,
    decision_source: String,
    rule: Option<String>,
    reason: Option<String>,
    /// Whether the referenced statement was parsed and agrees with the record's facts.
    statement_checked: bool,
}

/// The measurement methods: an evaluation from data, not a claim (`DECLARED`,
/// `SPEC_DERIVED`).
pub const MEASUREMENT_METHODS: [&str; 4] = ["LPU", "MCM", "ISO15530-3", "ISO15530-4"];

impl Evidence {
    /// `POINTS`, `FEATURES`, `RESULTS_ONLY` or `DIRECT_READING`, exactly as journaled.
    pub fn basis(&self) -> EvaluationBasis {
        self.basis
    }
    pub fn class(&self) -> EvidenceClass {
        self.class
    }
    /// The uncertainty method (`LPU`, `DECLARED`, ...), absent for UEC-0.
    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }
    pub fn is_claimed(&self) -> bool {
        self.claimed
    }
    pub fn rests_on_declared_input(&self) -> bool {
        self.declared_input
    }
    pub fn rests_on_uncharacterized_input(&self) -> bool {
        self.uncharacterized_input
    }
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    /// `unc` or `gdt_not_decidable`.
    pub fn decision_source(&self) -> &str {
        &self.decision_source
    }
    /// The decision rule ID, if a rule was bound.
    pub fn rule(&self) -> Option<&str> {
        self.rule.as_deref()
    }
    /// The not-decidable reason, if any.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    /// Whether the referenced fid-kunc statement was parsed and its class, method and claim
    /// facts agree with the record's (false for a record without a statement).
    pub fn statement_checked(&self) -> bool {
        self.statement_checked
    }
    /// Whether the value may be shown as measured: evaluated from points or features, with an
    /// evaluated (UEC-1..3) uncertainty from a measurement method ([`MEASUREMENT_METHODS`]),
    /// neither claimed nor resting on declared or uncharacterized input, and confirmed by the
    /// parsed statement it references.
    pub fn looks_measured(&self) -> bool {
        matches!(self.basis, EvaluationBasis::Points | EvaluationBasis::Features)
            && self.statement_checked
            && self.method.as_deref().is_some_and(|m| MEASUREMENT_METHODS.contains(&m))
            && self.class != EvidenceClass::Uec0
            && !self.claimed
            && !self.declared_input
            && !self.uncharacterized_input
    }
}

// ------------------------------------------------------------------ typed lookup

/// Why a lookup failed. Callers add the block and the ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupFault {
    /// No such entity (or one of another kind).
    NotFound,
    /// Committed only after the head.
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
    field: &'static str,
    quantity: Quantity,
    statement: Option<Digest>,
}

impl ValueField {
    pub fn field(&self) -> &'static str {
        self.field
    }
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }
    pub fn statement(&self) -> Option<Digest> {
        self.statement
    }
}

/// A ResultRecord as of a head: its ID, the SHA-256 of its stored bytes, its value fields
/// as typed quantities and its evidence facts.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultView {
    id: Uuid,
    digest: Digest,
    values: Vec<ValueField>,
    evidence: Evidence,
}

impl ResultView {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn digest(&self) -> Digest {
        self.digest
    }
    pub fn values(&self) -> &[ValueField] {
        &self.values
    }
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
    /// The value field at `pointer`, if `pointer` names one.
    pub fn value_field(&self, pointer: &str) -> Option<&ValueField> {
        self.values.iter().find(|v| v.field == pointer)
    }
}

/// A journaled entity whose content its block kind reads (characteristic, saved view,
/// statistics series): its ID and the SHA-256 of its stored bytes at the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityView {
    id: Uuid,
    digest: Digest,
}

impl EntityView {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn digest(&self) -> Digest {
        self.digest
    }
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
    /// The project's statement with this digest; its stored bytes must hash to it.
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

    /// Ownership (results, characteristics, statements only), then position, then kind
    /// (see the module documentation).
    fn visible(&self, found: AtHead, kind: EntityKind) -> Result<Journaled, LookupFault> {
        let owned = matches!(kind, EntityKind::Result | EntityKind::Characteristic | EntityKind::Statement);
        let (project, found_kind) = match &found {
            AtHead::Absent => return Err(LookupFault::NotFound),
            AtHead::NotYet { project, kind } => (*project, *kind),
            AtHead::Committed(j) => (j.project, j.kind),
        };
        if owned && project != self.project {
            return Err(LookupFault::CrossProject(project));
        }
        if found_kind != kind {
            return Err(LookupFault::NotFound);
        }
        match found {
            AtHead::Committed(j) if j.seq <= self.seq => Ok(j),
            _ => Err(LookupFault::Unjournaled),
        }
    }

    fn entity_view(&self, id: &Uuid, kind: EntityKind) -> Result<EntityView, LookupFault> {
        let j = self.visible(self.journal.entity(id, self.seq), kind)?;
        Ok(EntityView { id: *id, digest: sha256(&j.bytes) })
    }
}

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

impl Lookup for HeadLookup<'_> {
    fn result(&self, id: &Uuid) -> Result<ResultView, LookupFault> {
        let j = self.visible(self.journal.entity(id, self.seq), EntityKind::Result)?;
        let (values, mut evidence) = read_record(&j.bytes)?;
        if let Some(d) = values.iter().find_map(|v| v.statement) {
            let stored = self.statement(&d)?;
            let empty = RecordTable::default();
            let records = self.journal.source_records().unwrap_or(&empty);
            let stmt = UncertaintyStatement::parse_canonical_verified(stored.as_bytes(), records)
                .map_err(|_| LookupFault::Invalid("statement"))?;
            let agrees = stmt.class().wire() == evidence.class.as_str()
                && stmt.method().map(|m| m.wire()) == evidence.method.as_deref()
                && stmt.is_claimed() == evidence.claimed
                && stmt.rests_on_declared_input() == evidence.declared_input
                && stmt.rests_on_uncharacterized_input() == evidence.uncharacterized_input;
            if !agrees {
                return Err(LookupFault::Invalid("record evidence"));
            }
            evidence.statement_checked = true;
        }
        Ok(ResultView { id: *id, digest: sha256(&j.bytes), values, evidence })
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
        let found = self.journal.statement(&self.project, digest, self.seq);
        let j = self.visible(found, EntityKind::Statement)?;
        if sha256(&j.bytes) != *digest {
            return Err(LookupFault::Invalid("statement digest"));
        }
        Ok(StatementBytes { digest: *digest, bytes: j.bytes })
    }
}

type Obj = BTreeMap<String, CValue>;

fn opt_str(o: &Obj, k: &str) -> Result<Option<String>, LookupFault> {
    match o.get(k) {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(LookupFault::Invalid("record")),
    }
}

fn flag(o: &Obj, k: &str) -> Result<bool, LookupFault> {
    match o.get(k) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(LookupFault::Invalid("record")),
    }
}

/// The value fields and evidence of a stored `fid.gdt-result/1` record (canonical bytes
/// required). The decision's evidence basis, when present, must state the same class,
/// method, facts and statement as the uncertainty (`record evidence`).
fn read_record(bytes: &[u8]) -> Result<(Vec<ValueField>, Evidence), LookupFault> {
    let bad = LookupFault::Invalid("record");
    let v = parse_with_limits(bytes, MAX_ENTITY_BYTES, MAX_ENTITY_DEPTH).map_err(|_| bad)?;
    if to_canonical_bytes(&v).map_err(|_| bad)? != bytes {
        return Err(LookupFault::Invalid("record not canonical"));
    }
    let CValue::Obj(o) = &v else { return Err(bad) };
    match o.get("schema") {
        Some(CValue::Str(s)) if s == RESULT_RECORD_SCHEMA || s == RESULT_RECORD_SCHEMA_V2 => {}
        _ => return Err(LookupFault::Invalid("record schema")),
    }
    let Some(CValue::Obj(value)) = o.get("value") else { return Err(bad) };
    let (Some(CValue::Str(bits)), Some(CValue::Str(unit)), 2) = (value.get("value"), value.get("unit"), value.len())
    else {
        return Err(bad);
    };
    let unit = QuantityUnit::parse(unit).ok_or(bad)?;
    let hex = bits
        .strip_prefix("0x")
        .filter(|h| h.len() == 16 && h.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    let bits = hex.and_then(|h| u64::from_str_radix(h, 16).ok()).ok_or(bad)?;
    let basis = match o.get("evaluation_basis") {
        Some(CValue::Str(s)) => EvaluationBasis::from_wire(s).ok_or(bad)?,
        _ => return Err(bad),
    };
    let Some(CValue::Obj(unc)) = o.get("uncertainty") else { return Err(bad) };
    let statement = opt_str(unc, "statement")?.map(|s| Digest::parse(&s)).transpose().map_err(|_| bad)?;
    let class = opt_str(unc, "class")?.and_then(|c| EvidenceClass::parse(&c)).ok_or(bad)?;
    let method = opt_str(unc, "method")?;
    let (claimed, declared_input, uncharacterized_input) =
        (flag(unc, "claimed")?, flag(unc, "declared_input")?, flag(unc, "uncharacterized_input")?);
    let Some(CValue::Obj(dec)) = o.get("decision") else { return Err(bad) };
    let outcome = opt_str(dec, "outcome")?.and_then(|s| Outcome::parse(&s)).ok_or(bad)?;
    let decision_source = opt_str(dec, "source")?.ok_or(bad)?;
    if !matches!(decision_source.as_str(), "unc" | "gdt_not_decidable") {
        return Err(bad);
    }
    match dec.get("basis") {
        Some(CValue::Null) => {}
        Some(CValue::Obj(b)) => {
            let same = opt_str(b, "class")? == Some(class.as_str().to_string())
                && opt_str(b, "method")? == method
                && flag(b, "claimed")? == claimed
                && flag(b, "declared_input")? == declared_input
                && flag(b, "uncharacterized_input")? == uncharacterized_input
                && opt_str(b, "statement")? == statement.map(|d| d.as_str().to_string());
            if !same {
                return Err(LookupFault::Invalid("record evidence"));
            }
        }
        _ => return Err(bad),
    }
    let evidence = Evidence {
        basis,
        class,
        method,
        claimed,
        declared_input,
        uncharacterized_input,
        outcome,
        decision_source,
        rule: opt_str(dec, "rule")?,
        reason: opt_str(dec, "reason")?,
        statement_checked: false,
    };
    let field =
        ValueField { field: RECORD_VALUE_FIELD, quantity: Quantity::new(SiValue::from_bits(bits), unit), statement };
    Ok((vec![field], evidence))
}
