//! Computed proposal impact from the branch diff: `agent.compute_impact@1` (spec node
//! `AGENT.PROPOSE.IMPACT`).
//!
//! [`compute_impact`] answers "what would this agent branch change?" from the branch diff
//! (`CORE.JRN.BR.DIFF`, [`fid_journal::diff::diff_rows`]) and the ResultRecords whose digests
//! the diff names, read from the object store (`CORE.STO`) through [`ObjectSource`]. It
//! takes only shared references, so it changes no persistent state (R1, REQ-001), and the
//! agent can influence the result only through the commands it actually ran.
//!
//! # Pinned rules
//!
//! - **Promotion target.** The branch table records no promotion target, so the target of
//!   branch `B` is the branch of the journal row whose hash is `B`'s `created_from` (the
//!   branch `B` was forked from). `B` not in the table (or not a valid name):
//!   `unknown_branch`. `B` without `created_from` (`main`), or a target that is `B`, in the
//!   `agent/` namespace (nothing promotes into an agent branch, `CORE.JRN.BR.PROMO`) or not
//!   in the table: `no_target`. A `created_from` naming no row: `invariant_violated`. The
//!   target comes from that row's `branch` column; the diff then verifies every row, so a
//!   result is only ever computed from verified headers.
//! - **Rows.** The diff of `B` against its target. Only an entity some command on `B` output
//!   since the ancestor counts (a non-empty `source_commands`): changes made on the target
//!   alone are not the proposal's. Old is the target's digest, new the source's (`None` when
//!   absent or deleted). Each present digest is read from the store: absent is
//!   `record_missing{digest}`; a store that reports the object corrupt, or bytes whose
//!   SHA-256 is not the digest, are `record_invalid` (the digest check runs before any size
//!   rule, so wrong oversize bytes are refused, never skipped); an unreadable store is
//!   `internal/storage`.
//! - **What is a ResultRecord.** An object over [`MAX_RECORD_BYTES`] is not one (the record
//!   builder's fields are bounded far below that, and `fid_gdt`'s label reader reads no
//!   more). An object is a ResultRecord when its bytes contain the canonical member
//!   [`RECORD_MARKER`], or when it parses (depth [`MAX_RECORD_DEPTH`], no duplicate keys) as a
//!   JSON object whose `schema` starts with `fid.gdt-result/`. Any other object (a feature, a
//!   mesh) is not a characteristic and is skipped. A ResultRecord must be RFC 8785 canonical,
//!   of schema `fid.gdt-result/1` or `/2`, and carry `characteristic` (non-empty),
//!   `value.value` (`0x` + 16 lowercase hex digits), `value.unit`, `uncertainty.class`
//!   (`UEC-0` .. `UEC-3`), the evidence facts `uncertainty.claimed`,
//!   `uncertainty.declared_input` and `uncertainty.uncharacterized_input` (booleans) and
//!   `decision.outcome` (one of the four), else `record_invalid`.
//!   If one side is a record, the other side must be a record naming the same
//!   characteristic, or absent; else `record_invalid`.
//! - **Listing (REQ-002).** A row is `added` (old absent), `removed` (new absent), or
//!   `changed` when the outcome, the UEC class, the value, the D-045 interpretation label or
//!   any evidence fact differs (maintainer decision: a label-only or evidence-only change is
//!   listed, so a claimed or provisional record never slips in unseen); nothing else is
//!   listed. Each side carries its facts as stored. `value_changed` compares the stored bits
//!   and unit, never a recomputed value (REQ-006); `interpretation_changed` and
//!   `evidence_changed` compare the label and the three facts. All three flags are true when
//!   a side is absent. Rows are sorted by entity ID bytes.
//! - **Moves (REQ-003).** A changed row whose outcome differs adds one at (old, new) of the
//!   4x4 matrix over [`Outcome::ALL`]; the diagonal stays zero. Added and removed rows are
//!   counted apart.
//! - **Entries (REQ-004).** `entry_count` counts the `command` rows on `B` whose sequence is
//!   above the `created_from` row's (the fork point). Each one's `command.id@major` is looked
//!   up in the registry ([`CommandClasses`], built from the manifest's definitions with
//!   [`class_table`]); a command the registry does not define, or a verified header whose
//!   `command.class` differs from the definition, is `invariant_violated`. `classes` is the
//!   set of the definitions' classes (as the Design says), in [`SideEffect`] order
//!   (`R0 R1 W1 W2 W3 X1 I1 D1 M1 S1 A0`). Session headers and head moves declare no class
//!   and are not counted.
//! - **Heads (REQ-005).** `source_head` and `target_head` are the heads the diff used.
//! - **Interpretation (D-045).** Every row side carries the record's interpretation label,
//!   decided by [`fid_gdt::requires_provisional_label_bytes`] on the stored bytes (so a `/1`
//!   record is `provisional`). The result's `interpretation` is `provisional` when any row
//!   side is, `signed_off` when there are rows and none is, and `none` without rows. No
//!   surface may drop it.
//! - **Output.** The CLI's only output form is the canonical JSON result: `--json` is
//!   accepted and is currently the default and only form (there is no text rendering yet).
//! - **Bounds.** The diff's own bounds ([`DiffOptions`]; MCP callers get the agent bounds),
//!   one object in memory at a time, checked counters, no panics, no `HashMap`.

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, CanonError, MAX_DEPTH};
use fid_journal::branch::Branches;
use fid_journal::branch::{Owner, NAME_PATTERN};
use fid_journal::diff::{diff_rows, DiffError, DiffOptions, EntityDelta};
use fid_journal::verify::{StoredEntry, MAX_HEADER_BYTES};
use fid_journal::PrincipalKind;
use fid_registry::{CommandDef, DefError};
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command ID of `agent.compute_impact@1`.
pub const COMPUTE_IMPACT: &str = "agent.compute_impact@1";
const COMPUTE_IMPACT_NAME: &str = "agent.compute_impact";
/// CLI subcommand under `fid agent`.
pub const CLI_COMPUTE_IMPACT: &str = "compute-impact";
/// MCP tool name.
pub const MCP_COMPUTE_IMPACT: &str = "agent_compute_impact";
/// Wire schema of a [`ProposalImpact`].
pub const IMPACT_SCHEMA: &str = "fid.proposal-impact/1";
/// Largest object read as a possible ResultRecord, in bytes (`fid_gdt`'s label reader bound).
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
/// Deepest JSON nesting of a ResultRecord.
pub const MAX_RECORD_DEPTH: usize = 8;
/// The canonical schema member every stored ResultRecord carries.
pub const RECORD_MARKER: &[u8] = b"\"schema\":\"fid.gdt-result/";
/// Largest MCP parameter payload, in bytes.
pub const MAX_IMPACT_PARAMS_BYTES: usize = 4096;
const MAX_PARAMS_DEPTH: usize = 4;
const MAX_CLI_ARGS: usize = 8;
const RECORD_SCHEMA_PREFIX: &str = "fid.gdt-result/";
const RECORD_SCHEMAS: [&str; 2] = ["fid.gdt-result/1", "fid.gdt-result/2"];
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
const BITS_PATTERN: &str = "^0x[0-9a-f]{16}$";
const LIFECYCLE_SINCE: &str = "2026-11-26";
/// Most rows a result schema admits (the diff's own list bound).
const MAX_ROWS: f64 = 1_000_000.0;

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn opt_text(s: Option<&str>) -> CValue {
    s.map_or(CValue::Null, text)
}

fn count(n: u32) -> CValue {
    CValue::Num(f64::from(n))
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

// ------------------------------------------------------------------ errors

/// A refusal of `agent.compute_impact@1`. On any error nothing is returned and nothing is
/// written (nothing ever is).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpactError {
    /// The branch is not in the branch table (`not_found/unknown_branch`).
    UnknownBranch(String),
    /// The branch has no promotion target (`precondition/false`, leaf code `no_target`).
    NoTarget(String),
    /// The store holds no object for a digest the diff names (`internal/invariant_violated`).
    RecordMissing { digest: Digest },
    /// A stored object does not match its digest, or is a ResultRecord that does not read
    /// (`internal/invariant_violated`).
    RecordInvalid { digest: Digest, reason: String },
    /// The diff refused, or a verified header contradicts the rules: the diff's bus error.
    Journal(DiffError),
    /// The object store could not be read (`internal/storage`).
    Storage(String),
    /// Malformed CLI or MCP parameters (`validation/schema`).
    BadParams(String),
    /// A counter or payload over its bound (`validation/input_limit`).
    InputLimit(&'static str),
    /// Internal encoding error (`internal/handler_failed`).
    Internal(&'static str),
}

impl ImpactError {
    /// The leaf error code.
    pub fn code(&self) -> &'static str {
        match self {
            ImpactError::UnknownBranch(_) => "unknown_branch",
            ImpactError::NoTarget(_) => "no_target",
            ImpactError::RecordMissing { .. } => "record_missing",
            ImpactError::RecordInvalid { .. } => "record_invalid",
            ImpactError::Journal(e) => e.bus_code().1,
            ImpactError::Storage(_) => "storage",
            ImpactError::BadParams(_) => "bad_params",
            ImpactError::InputLimit(_) => "input_limit",
            ImpactError::Internal(_) => "internal",
        }
    }

    /// The bus error `(code, reason)` of catalogue v3.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            ImpactError::UnknownBranch(_) => ("not_found", "unknown_branch"),
            ImpactError::NoTarget(_) => ("precondition", "false"),
            ImpactError::RecordMissing { .. } | ImpactError::RecordInvalid { .. } => ("internal", "invariant_violated"),
            ImpactError::Journal(e) => e.bus_code(),
            ImpactError::Storage(_) => ("internal", "storage"),
            ImpactError::BadParams(_) => ("validation", "schema"),
            ImpactError::InputLimit(_) => ("validation", "input_limit"),
            ImpactError::Internal(_) => ("internal", "handler_failed"),
        }
    }

    /// The digest a record error names.
    pub fn digest(&self) -> Option<Digest> {
        match self {
            ImpactError::RecordMissing { digest } | ImpactError::RecordInvalid { digest, .. } => Some(*digest),
            _ => None,
        }
    }
}

impl fmt::Display for ImpactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImpactError::UnknownBranch(b) => write!(f, "unknown_branch: no branch named {b:?}"),
            ImpactError::NoTarget(b) => write!(f, "no_target: branch {b:?} has no promotion target"),
            ImpactError::RecordMissing { digest } => write!(f, "record_missing: no stored object {digest}"),
            ImpactError::RecordInvalid { digest, reason } => write!(f, "record_invalid: {digest}: {reason}"),
            ImpactError::Journal(e) => write!(f, "{e}"),
            ImpactError::Storage(m) => write!(f, "storage: {m}"),
            ImpactError::BadParams(m) => write!(f, "bad_params: {m}"),
            ImpactError::InputLimit(m) => write!(f, "input_limit: {m}"),
            ImpactError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for ImpactError {}

fn invariant(why: impl Into<String>) -> ImpactError {
    ImpactError::Journal(DiffError::Tampered(why.into()))
}

fn invalid(digest: Digest, reason: &str) -> ImpactError {
    ImpactError::RecordInvalid { digest, reason: reason.to_string() }
}

// ------------------------------------------------------------------ the object store

/// What reading one object gave. A store-backed source maps `fid_store`'s
/// `BlobStore::read_object` errors as follows: `CasError::NotFound` is [`ObjectRead::Missing`]
/// (`record_missing`); `CasError::OutOfRange` (the object is longer than `max_len`) is
/// [`ObjectRead::TooLarge`] (not a record); `CasError::Integrity` and
/// `CasError::ManifestInvalid` are [`ObjectRead::Corrupt`] (`record_invalid`); `CasError::Io`
/// is [`ObjectRead::Unavailable`] (`internal/storage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectRead {
    /// No object with that digest.
    Missing,
    /// The object is longer than the bound the caller passed (not read).
    TooLarge,
    /// The store holds something under the digest that is not the object (its bytes do not
    /// hash to it, or a chunk manifest is malformed).
    Corrupt(String),
    /// The store could not be read (I/O).
    Unavailable(String),
    /// The object's bytes.
    Bytes(Vec<u8>),
}

/// Read access to the project object store (`CORE.STO.CAS`). A store-backed source reads
/// with the safe object read, which never serves a chunk manifest as the object.
pub trait ObjectSource {
    /// The object stored under `digest`, or [`ObjectRead::TooLarge`] when it is longer than
    /// `max_len` bytes.
    fn read_object(&self, digest: &Digest, max_len: usize) -> ObjectRead;
}

/// An in-memory object store keyed by digest text (fixtures and tests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryObjects {
    objects: BTreeMap<String, Vec<u8>>,
}

impl MemoryObjects {
    pub fn new() -> MemoryObjects {
        MemoryObjects::default()
    }

    /// Stores `bytes` under `digest` as given (no check, so tests can store a wrong object).
    pub fn insert(&mut self, digest: Digest, bytes: Vec<u8>) {
        self.objects.insert(digest.as_str().to_string(), bytes);
    }

    /// Stores `bytes` under their own SHA-256 and returns it.
    pub fn put(&mut self, bytes: Vec<u8>) -> Digest {
        let d = Digest::from_bytes(digest_bytes(&bytes).0);
        self.insert(d, bytes);
        d
    }

    pub fn remove(&mut self, digest: &Digest) -> Option<Vec<u8>> {
        self.objects.remove(digest.as_str())
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

impl ObjectSource for MemoryObjects {
    fn read_object(&self, digest: &Digest, max_len: usize) -> ObjectRead {
        match self.objects.get(digest.as_str()) {
            None => ObjectRead::Missing,
            Some(b) if b.len() > max_len => ObjectRead::TooLarge,
            Some(b) => ObjectRead::Bytes(b.clone()),
        }
    }
}

/// The registry's side-effect class of each command, by `id@major` (`CORE.REG.MAN`).
pub trait CommandClasses {
    /// The class the registry defines for `command` (`id@major`), `None` when it defines none.
    fn class_of(&self, command: &str) -> Option<SideEffect>;
}

impl CommandClasses for BTreeMap<String, SideEffect> {
    fn class_of(&self, command: &str) -> Option<SideEffect> {
        self.get(command).copied()
    }
}

/// The `id@major` to class table of a set of registry definitions (for example the workspace
/// manifest's).
pub fn class_table(defs: &[CommandDef]) -> BTreeMap<String, SideEffect> {
    defs.iter().map(|d| (format!("{}@{}", d.id, d.major), d.side_effect)).collect()
}

/// What the command reads: one journal stream's stored rows (sequence order, from 1), the
/// origin-bound branch table (`CORE.JRN.BR.NAME`), the object store and the registry's
/// command classes.
#[derive(Clone, Copy)]
pub struct ImpactProject<'a> {
    pub stream: &'a str,
    pub rows: &'a [StoredEntry],
    pub branches: &'a Branches,
    pub objects: &'a dyn ObjectSource,
    pub commands: &'a dyn CommandClasses,
}

impl fmt::Debug for ImpactProject<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImpactProject")
            .field("stream", &self.stream)
            .field("rows", &self.rows.len())
            .field("branches", self.branches)
            .finish_non_exhaustive()
    }
}

// ------------------------------------------------------------------ record facts

/// A decision outcome (CONSTITUTION §5), in matrix order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    Conforms,
    Nonconforms,
    Indeterminate,
    NotDecidable,
}

impl Outcome {
    /// The matrix order of [`ProposalImpact::moves`].
    pub const ALL: [Outcome; 4] =
        [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate, Outcome::NotDecidable];

    pub const fn as_str(self) -> &'static str {
        match self {
            Outcome::Conforms => "CONFORMS",
            Outcome::Nonconforms => "NONCONFORMS",
            Outcome::Indeterminate => "INDETERMINATE",
            Outcome::NotDecidable => "NOT_DECIDABLE",
        }
    }

    pub fn parse(s: &str) -> Option<Outcome> {
        Outcome::ALL.into_iter().find(|o| o.as_str() == s)
    }

    const fn index(self) -> usize {
        match self {
            Outcome::Conforms => 0,
            Outcome::Nonconforms => 1,
            Outcome::Indeterminate => 2,
            Outcome::NotDecidable => 3,
        }
    }
}

/// An uncertainty evidence class (`UNC.UEC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Uec {
    Uec0,
    Uec1,
    Uec2,
    Uec3,
}

impl Uec {
    pub const ALL: [Uec; 4] = [Uec::Uec0, Uec::Uec1, Uec::Uec2, Uec::Uec3];

    pub const fn as_str(self) -> &'static str {
        match self {
            Uec::Uec0 => "UEC-0",
            Uec::Uec1 => "UEC-1",
            Uec::Uec2 => "UEC-2",
            Uec::Uec3 => "UEC-3",
        }
    }

    pub fn parse(s: &str) -> Option<Uec> {
        Uec::ALL.into_iter().find(|u| u.as_str() == s)
    }
}

/// The interpretation label of a record (D-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Label {
    Provisional,
    SignedOff,
}

impl Label {
    pub const fn as_str(self) -> &'static str {
        match self {
            Label::Provisional => "provisional",
            Label::SignedOff => "signed_off",
        }
    }
}

/// The stored value of a record: IEEE-754 bits and unit, exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredValue {
    pub bits: u64,
    pub unit: String,
}

impl StoredValue {
    fn to_value(&self) -> CValue {
        obj([("unit", text(&self.unit)), ("value", CValue::Str(format!("0x{:016x}", self.bits)))])
    }
}

/// The evidence facts of a record's uncertainty statement, as stored (`claimed values never
/// look measured`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evidence {
    pub claimed: bool,
    pub declared_input: bool,
    pub uncharacterized_input: bool,
}

impl Evidence {
    fn to_value(self) -> CValue {
        obj([
            ("claimed", CValue::Bool(self.claimed)),
            ("declared_input", CValue::Bool(self.declared_input)),
            ("uncharacterized_input", CValue::Bool(self.uncharacterized_input)),
        ])
    }
}

/// The facts the impact reads from one stored ResultRecord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFacts {
    pub digest: Digest,
    pub characteristic: String,
    pub outcome: Outcome,
    pub uec: Uec,
    pub value: StoredValue,
    pub evidence: Evidence,
    pub interpretation: Label,
}

type Obj = BTreeMap<String, CValue>;

fn member<'a>(o: &'a Obj, k: &str) -> Option<&'a CValue> {
    o.get(k)
}

fn str_at<'a>(o: &'a Obj, path: &[&str]) -> Option<&'a str> {
    let (last, init) = path.split_last()?;
    let mut cur = o;
    for k in init {
        match member(cur, k)? {
            CValue::Obj(m) => cur = m,
            _ => return None,
        }
    }
    match member(cur, last)? {
        CValue::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

fn str_obj<'a>(o: &'a Obj, k: &str) -> Option<&'a Obj> {
    match member(o, k)? {
        CValue::Obj(m) => Some(m),
        _ => None,
    }
}

fn bool_at(o: &Obj, path: &[&str]) -> Option<bool> {
    let (last, init) = path.split_last()?;
    let mut cur = o;
    for k in init {
        match member(cur, k)? {
            CValue::Obj(m) => cur = m,
            _ => return None,
        }
    }
    match member(cur, last)? {
        CValue::Bool(b) => Some(*b),
        _ => None,
    }
}

fn parse_bits(s: &str) -> Option<u64> {
    let hex = s.strip_prefix("0x")?;
    if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Reads the object under `digest`: `Ok(None)` when it is not a ResultRecord (module
/// documentation, What is a ResultRecord).
fn read_record(objects: &dyn ObjectSource, digest: Digest) -> Result<Option<RecordFacts>, ImpactError> {
    let bytes = match objects.read_object(&digest, MAX_RECORD_BYTES) {
        ObjectRead::Missing => return Err(ImpactError::RecordMissing { digest }),
        ObjectRead::TooLarge => return Ok(None),
        ObjectRead::Corrupt(why) => return Err(invalid(digest, &format!("store reports corruption: {why}"))),
        ObjectRead::Unavailable(why) => return Err(ImpactError::Storage(why)),
        ObjectRead::Bytes(b) => b,
    };
    // The digest check comes first: oversize bytes that are not the object are refused.
    if Digest::from_bytes(digest_bytes(&bytes).0) != digest {
        return Err(invalid(digest, "stored bytes do not hash to their digest"));
    }
    if bytes.len() > MAX_RECORD_BYTES {
        return Ok(None);
    }
    let marked = contains(&bytes, RECORD_MARKER);
    let not_record = |why: &str| if marked { Err(invalid(digest, why)) } else { Ok(None) };
    let v = match parse_with_limits(&bytes, MAX_RECORD_BYTES, MAX_RECORD_DEPTH) {
        Ok(v) => v,
        Err(_) => return not_record("a record that does not parse"),
    };
    let CValue::Obj(o) = &v else { return not_record("a record that is not an object") };
    let schema = match member(o, "schema") {
        Some(CValue::Str(s)) if s.starts_with(RECORD_SCHEMA_PREFIX) => s.as_str(),
        _ => return not_record("a record without a result schema"),
    };
    if !RECORD_SCHEMAS.contains(&schema) {
        return Err(invalid(digest, "unknown result schema version"));
    }
    match to_canonical_bytes(&v) {
        Ok(c) if c == bytes => {}
        _ => return Err(invalid(digest, "record is not RFC 8785 canonical")),
    }
    let characteristic = match str_at(o, &["characteristic"]) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => return Err(invalid(digest, "characteristic")),
    };
    let bits = str_at(o, &["value", "value"]).and_then(parse_bits).ok_or_else(|| invalid(digest, "value.value"))?;
    let unit = str_at(o, &["value", "unit"]).ok_or_else(|| invalid(digest, "value.unit"))?.to_string();
    let uec = str_at(o, &["uncertainty", "class"])
        .and_then(Uec::parse)
        .ok_or_else(|| invalid(digest, "uncertainty.class"))?;
    let outcome = str_at(o, &["decision", "outcome"])
        .and_then(Outcome::parse)
        .ok_or_else(|| invalid(digest, "decision.outcome"))?;
    let fact = |k: &'static str| bool_at(o, &["uncertainty", k]).ok_or_else(|| invalid(digest, k));
    let evidence = Evidence {
        claimed: fact("claimed")?,
        declared_input: fact("declared_input")?,
        uncharacterized_input: fact("uncharacterized_input")?,
    };
    let interpretation =
        if fid_gdt::requires_provisional_label_bytes(&bytes) { Label::Provisional } else { Label::SignedOff };
    Ok(Some(RecordFacts {
        digest,
        characteristic,
        outcome,
        uec,
        value: StoredValue { bits, unit },
        evidence,
        interpretation,
    }))
}

// ------------------------------------------------------------------ result

/// How a characteristic's record changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Changed,
}

impl Change {
    pub const fn as_str(self) -> &'static str {
        match self {
            Change::Added => "added",
            Change::Removed => "removed",
            Change::Changed => "changed",
        }
    }
}

/// One listed characteristic (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactRow {
    /// The stable entity ID, as the journal headers name it.
    pub id: String,
    pub characteristic: String,
    pub change: Change,
    /// The target's record, `None` when added.
    pub old: Option<RecordFacts>,
    /// The source's record, `None` when removed.
    pub new: Option<RecordFacts>,
    /// Stored bits or unit differ (always true when added or removed).
    pub value_changed: bool,
    /// The D-045 label differs (always true when added or removed).
    pub interpretation_changed: bool,
    /// Any evidence fact differs (always true when added or removed).
    pub evidence_changed: bool,
}

impl ImpactRow {
    pub fn to_value(&self) -> CValue {
        let o = self.old.as_ref();
        let n = self.new.as_ref();
        let rec = |r: Option<&RecordFacts>| r.map_or(CValue::Null, |r| text(r.digest.as_str()));
        let outcome = |r: Option<&RecordFacts>| opt_text(r.map(|r| r.outcome.as_str()));
        let uec = |r: Option<&RecordFacts>| opt_text(r.map(|r| r.uec.as_str()));
        let value = |r: Option<&RecordFacts>| r.map_or(CValue::Null, |r| r.value.to_value());
        let label = |r: Option<&RecordFacts>| opt_text(r.map(|r| r.interpretation.as_str()));
        let evidence = |r: Option<&RecordFacts>| r.map_or(CValue::Null, |r| r.evidence.to_value());
        obj([
            ("change", text(self.change.as_str())),
            ("characteristic", text(&self.characteristic)),
            ("evidence_changed", CValue::Bool(self.evidence_changed)),
            ("id", text(&self.id)),
            ("interpretation_changed", CValue::Bool(self.interpretation_changed)),
            ("new_evidence", evidence(n)),
            ("new_interpretation", label(n)),
            ("new_outcome", outcome(n)),
            ("new_record", rec(n)),
            ("new_uec", uec(n)),
            ("new_value", value(n)),
            ("old_evidence", evidence(o)),
            ("old_interpretation", label(o)),
            ("old_outcome", outcome(o)),
            ("old_record", rec(o)),
            ("old_uec", uec(o)),
            ("old_value", value(o)),
            ("value_changed", CValue::Bool(self.value_changed)),
        ])
    }
}

/// The `agent.compute_impact@1` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalImpact {
    pub branch: String,
    /// The promotion target the diff ran against.
    pub target: String,
    pub source_head: Option<Digest>,
    pub target_head: Option<Digest>,
    pub characteristics: Vec<ImpactRow>,
    /// `moves[old][new]` over [`Outcome::ALL`].
    pub moves: [[u32; 4]; 4],
    pub added: u32,
    pub removed: u32,
    pub entry_count: u32,
    pub classes: BTreeSet<SideEffect>,
}

impl ProposalImpact {
    /// The D-045 label of the whole result: `provisional` when any row side is,
    /// `signed_off` when there are rows and none is, `None` without rows.
    pub fn interpretation(&self) -> Option<Label> {
        if self.characteristics.is_empty() {
            return None;
        }
        let any = self
            .characteristics
            .iter()
            .flat_map(|r| [r.old.as_ref(), r.new.as_ref()])
            .flatten()
            .any(|f| f.interpretation == Label::Provisional);
        Some(if any { Label::Provisional } else { Label::SignedOff })
    }

    /// The wire form (`fid.proposal-impact/1`).
    pub fn to_value(&self) -> CValue {
        let head = |d: Option<Digest>| d.map_or(CValue::Null, |d| text(d.as_str()));
        let moves =
            CValue::Arr(self.moves.iter().map(|r| CValue::Arr(r.iter().map(|n| count(*n)).collect())).collect());
        obj([
            ("added", count(self.added)),
            ("branch", text(&self.branch)),
            ("characteristics", CValue::Arr(self.characteristics.iter().map(ImpactRow::to_value).collect())),
            ("classes", CValue::Arr(self.classes.iter().map(|c| text(c.as_str())).collect())),
            ("entry_count", count(self.entry_count)),
            ("interpretation", text(self.interpretation().map_or("none", Label::as_str))),
            ("moves", moves),
            ("outcomes", CValue::Arr(Outcome::ALL.iter().map(|o| text(o.as_str())).collect())),
            ("removed", count(self.removed)),
            ("schema", text(IMPACT_SCHEMA)),
            ("source_head", head(self.source_head)),
            ("target", text(&self.target)),
            ("target_head", head(self.target_head)),
        ])
    }

    /// RFC 8785 canonical bytes of [`ProposalImpact::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, ImpactError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| ImpactError::Internal("result encoding"))
    }
}

// ------------------------------------------------------------------ the command

fn bump(n: &mut u32, what: &'static str) -> Result<(), ImpactError> {
    *n = n.checked_add(1).ok_or(ImpactError::InputLimit(what))?;
    Ok(())
}

/// The promotion target of `branch` and the fork-point sequence (module documentation).
fn target_of(project: &ImpactProject<'_>, branch: &str) -> Result<(String, i64), ImpactError> {
    let b = project.branches.get(branch).ok_or_else(|| ImpactError::UnknownBranch(truncate(branch)))?;
    let from = b.created_from.ok_or_else(|| ImpactError::NoTarget(truncate(branch)))?;
    let row = project
        .rows
        .iter()
        .find(|r| r.hash == from.as_str())
        .ok_or_else(|| invariant(format!("branch {branch} was created from {from}, which is no journal row")))?;
    let target = row.branch.as_str();
    let ok = target != branch && project.branches.get(target).is_some_and(|t| t.owner() != Owner::Agent);
    if !ok {
        return Err(ImpactError::NoTarget(truncate(branch)));
    }
    Ok((target.to_string(), row.seq))
}

/// `entry_count` and `classes` of `branch` above `fork_seq`, from the (already verified)
/// headers.
fn entries_of(
    project: &ImpactProject<'_>,
    branch: &str,
    fork_seq: i64,
) -> Result<(u32, BTreeSet<SideEffect>), ImpactError> {
    let mut n = 0u32;
    let mut classes = BTreeSet::new();
    for r in project.rows.iter().filter(|r| r.branch == branch && r.kind == "command" && r.seq > fork_seq) {
        let h = parse_with_limits(&r.header, MAX_HEADER_BYTES, MAX_DEPTH)
            .map_err(|e| invariant(format!("verified header {} does not parse: {}", r.seq, e.code())))?;
        let CValue::Obj(h) = h else { return Err(invariant("a verified header is not an object")) };
        let header_class = str_at(&h, &["command", "class"])
            .ok_or_else(|| invariant(format!("verified header {} has no command.class", r.seq)))?;
        let id = str_at(&h, &["command", "id"])
            .ok_or_else(|| invariant(format!("verified header {} has no command.id", r.seq)))?;
        let major = match str_obj(&h, "command").and_then(|c| c.get("major")) {
            Some(CValue::Num(m)) if m.fract() == 0.0 && *m >= 0.0 && *m <= f64::from(u32::MAX) => *m as u32,
            Some(CValue::U64(m)) => u32::try_from(*m)
                .map_err(|_| invariant(format!("verified header {} has a command.major out of range", r.seq)))?,
            _ => return Err(invariant(format!("verified header {} has no integer command.major", r.seq))),
        };
        let command = format!("{id}@{major}");
        let class = project.commands.class_of(&command).ok_or_else(|| {
            invariant(format!("entry {} runs {}, which the registry does not define", r.seq, truncate(&command)))
        })?;
        if class.as_str() != header_class {
            return Err(invariant(format!(
                "entry {} declares class {} but the registry defines {} as {}",
                r.seq,
                truncate(header_class),
                truncate(&command),
                class.as_str()
            )));
        }
        bump(&mut n, "entry count")?;
        classes.insert(class);
    }
    Ok((n, classes))
}

/// The row of one delta, `None` when it is not listed.
fn row_of(project: &ImpactProject<'_>, d: &EntityDelta) -> Result<Option<ImpactRow>, ImpactError> {
    if d.source_commands.is_empty() {
        return Ok(None);
    }
    let old = d.target_digest.map(|x| read_record(project.objects, x).map(|r| (x, r))).transpose()?;
    let new = d.source_digest.map(|x| read_record(project.objects, x).map(|r| (x, r))).transpose()?;
    let (old, new) = match (old, new) {
        (None | Some((_, None)), None | Some((_, None))) => return Ok(None),
        (Some((x, None)), Some((_, Some(_)))) | (Some((_, Some(_))), Some((x, None))) => {
            return Err(invalid(x, "a record replaced by an object that is not a record"))
        }
        (o, n) => (o.and_then(|(_, r)| r), n.and_then(|(_, r)| r)),
    };
    let (change, flags, characteristic) = match (&old, &new) {
        (None, Some(n)) => (Change::Added, (true, true, true), n.characteristic.clone()),
        (Some(o), None) => (Change::Removed, (true, true, true), o.characteristic.clone()),
        (Some(o), Some(n)) => {
            if o.characteristic != n.characteristic {
                return Err(invalid(n.digest, "record names another characteristic than the one it replaces"));
            }
            let flags = (o.value != n.value, o.interpretation != n.interpretation, o.evidence != n.evidence);
            if !(flags.0 || flags.1 || flags.2 || o.outcome != n.outcome || o.uec != n.uec) {
                return Ok(None);
            }
            (Change::Changed, flags, n.characteristic.clone())
        }
        (None, None) => return Ok(None),
    };
    let (value_changed, interpretation_changed, evidence_changed) = flags;
    Ok(Some(ImpactRow {
        id: d.id.clone(),
        characteristic,
        change,
        old,
        new,
        value_changed,
        interpretation_changed,
        evidence_changed,
    }))
}

/// `agent.compute_impact@1` (module documentation, Pinned rules).
pub fn compute_impact(
    project: &ImpactProject<'_>,
    branch: &str,
    opts: &DiffOptions,
) -> Result<ProposalImpact, ImpactError> {
    let (target, fork_seq) = target_of(project, branch)?;
    let diff = diff_rows(project.stream, project.rows.iter().cloned().map(Ok), project.branches, branch, &target, opts)
        .map_err(ImpactError::Journal)?;
    let mut deltas: Vec<&EntityDelta> = diff.added.iter().chain(&diff.removed).chain(&diff.changed).collect();
    deltas.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));

    let mut characteristics = Vec::new();
    let mut moves = [[0u32; 4]; 4];
    let (mut added, mut removed) = (0u32, 0u32);
    for d in deltas {
        let Some(row) = row_of(project, d)? else { continue };
        match (&row.change, &row.old, &row.new) {
            (Change::Added, _, _) => bump(&mut added, "added count")?,
            (Change::Removed, _, _) => bump(&mut removed, "removed count")?,
            (Change::Changed, Some(o), Some(n)) if o.outcome != n.outcome => {
                let cell = moves
                    .get_mut(o.outcome.index())
                    .and_then(|r| r.get_mut(n.outcome.index()))
                    .ok_or(ImpactError::Internal("move matrix index"))?;
                bump(cell, "move count")?;
            }
            _ => {}
        }
        characteristics.push(row);
    }
    let (entry_count, classes) = entries_of(project, branch, fork_seq)?;
    Ok(ProposalImpact {
        branch: branch.to_string(),
        target,
        source_head: diff.source.head,
        target_head: diff.target.head,
        characteristics,
        moves,
        added,
        removed,
        entry_count,
        classes,
    })
}

// ------------------------------------------------------------------ CLI and MCP entry points

/// CLI handler for `fid agent compute-impact --branch <name> [--json]`. `caller` is the
/// principal kind the bus derived (never a parameter); it picks the diff bounds. Returns
/// the canonical JSON of the result: that is currently the only output form, so `--json` is
/// accepted but changes nothing (there is no text rendering yet).
pub fn invoke_compute_impact_cli(
    project: &ImpactProject<'_>,
    caller: PrincipalKind,
    args: &[&str],
) -> Result<Vec<u8>, ImpactError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(ImpactError::InputLimit("cli args"));
    }
    let mut branch: Option<&str> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(ImpactError::BadParams("repeated --json".into())),
            "--branch" if branch.is_none() => {
                branch = Some(*it.next().ok_or_else(|| ImpactError::BadParams("--branch needs a value".into()))?);
            }
            "--branch" => return Err(ImpactError::BadParams("repeated --branch".into())),
            other => return Err(ImpactError::BadParams(format!("unknown option {:?}", truncate(other)))),
        }
    }
    let branch = branch.ok_or_else(|| ImpactError::BadParams("missing --branch".into()))?;
    compute_impact(project, branch, &DiffOptions::for_caller(caller))?.to_json()
}

/// MCP handler for `agent_compute_impact` on bounded JSON `{"branch": "..."}`. MCP callers
/// are agents (D-035), so the diff runs with the agent bounds. Same logic and bytes as the
/// CLI.
pub fn invoke_compute_impact_mcp(project: &ImpactProject<'_>, params_json: &[u8]) -> Result<Vec<u8>, ImpactError> {
    let val = parse_with_limits(params_json, MAX_IMPACT_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => ImpactError::InputLimit("params"),
        CanonError::DuplicateKey(_) => ImpactError::BadParams("duplicate parameter key".into()),
        _ => ImpactError::BadParams("params are not valid JSON".into()),
    })?;
    let CValue::Obj(o) = &val else {
        return Err(ImpactError::BadParams("params must be a JSON object".into()));
    };
    if o.keys().any(|k| k != "branch") {
        return Err(ImpactError::BadParams("unknown parameter key".into()));
    }
    let Some(CValue::Str(branch)) = o.get("branch") else {
        return Err(ImpactError::BadParams("missing or non-string branch".into()));
    };
    compute_impact(project, branch, &DiffOptions::for_caller(PrincipalKind::Agent))?.to_json()
}

// ------------------------------------------------------------------ registry definition

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn described(mut v: CValue, description: &str) -> CValue {
    if let CValue::Obj(o) = &mut v {
        o.insert("description".to_string(), text(description));
    }
    v
}

fn or_null(v: CValue, description: &str) -> CValue {
    obj([("anyOf", CValue::Arr(vec![v, obj([("type", text("null"))])])), ("description", text(description))])
}

fn pattern(p: &str, max: f64) -> CValue {
    obj([("type", text("string")), ("pattern", text(p)), ("maxLength", CValue::Num(max))])
}

fn one_of(items: &[&str]) -> CValue {
    obj([("type", text("string")), ("enum", strs(items))])
}

fn untrusted(max: f64, description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("maxLength", CValue::Num(max)),
        ("x-untrusted", CValue::Bool(true)),
        ("description", text(description)),
    ])
}

fn counter(description: &str) -> CValue {
    obj([
        ("type", text("integer")),
        ("minimum", CValue::Num(0.0)),
        ("maximum", CValue::Num(f64::from(u32::MAX))),
        ("description", text(description)),
    ])
}

fn value_schema() -> CValue {
    object(
        &["unit", "value"],
        obj([("unit", untrusted(256.0, "Stored unit.")), ("value", pattern(BITS_PATTERN, 18.0))]),
    )
}

fn flag(description: &str) -> CValue {
    obj([("type", text("boolean")), ("description", text(description))])
}

fn evidence_schema() -> CValue {
    let b = || obj([("type", text("boolean"))]);
    object(
        &["claimed", "declared_input", "uncharacterized_input"],
        obj([("claimed", b()), ("declared_input", b()), ("uncharacterized_input", b())]),
    )
}

fn row_schema() -> CValue {
    let outcomes: Vec<&str> = Outcome::ALL.iter().map(|o| o.as_str()).collect();
    let uecs: Vec<&str> = Uec::ALL.iter().map(|u| u.as_str()).collect();
    let labels = ["provisional", "signed_off"];
    object(
        &[
            "change",
            "characteristic",
            "evidence_changed",
            "id",
            "interpretation_changed",
            "new_evidence",
            "new_interpretation",
            "new_outcome",
            "new_record",
            "new_uec",
            "new_value",
            "old_evidence",
            "old_interpretation",
            "old_outcome",
            "old_record",
            "old_uec",
            "old_value",
            "value_changed",
        ],
        obj([
            ("change", one_of(&["added", "changed", "removed"])),
            ("characteristic", untrusted(1024.0, "Characteristic ID the stored record names.")),
            ("evidence_changed", flag("Any evidence fact differs; true when a side is absent.")),
            ("id", untrusted(1_048_576.0, "Stable entity ID, as the journal headers name it.")),
            ("interpretation_changed", flag("The D-045 label differs; true when a side is absent.")),
            ("new_evidence", or_null(evidence_schema(), "Stored evidence facts of the new record.")),
            ("new_interpretation", or_null(one_of(&labels), "D-045 label of the new record; null when removed.")),
            ("new_outcome", or_null(one_of(&outcomes), "Stored decision outcome of the new record.")),
            ("new_record", or_null(pattern(DIGEST_PATTERN, 71.0), "Digest of the new record.")),
            ("new_uec", or_null(one_of(&uecs), "Stored UEC class of the new record.")),
            ("new_value", or_null(value_schema(), "Stored value of the new record.")),
            ("old_evidence", or_null(evidence_schema(), "Stored evidence facts of the old record.")),
            ("old_interpretation", or_null(one_of(&labels), "D-045 label of the old record; null when added.")),
            ("old_outcome", or_null(one_of(&outcomes), "Stored decision outcome of the old record.")),
            ("old_record", or_null(pattern(DIGEST_PATTERN, 71.0), "Digest of the old record.")),
            ("old_uec", or_null(one_of(&uecs), "Stored UEC class of the old record.")),
            ("old_value", or_null(value_schema(), "Stored value of the old record.")),
            (
                "value_changed",
                obj([
                    ("type", text("boolean")),
                    ("description", text("Stored bits or unit differ; never a recomputed value.")),
                ]),
            ),
        ]),
    )
}

/// The params schema of `agent.compute_impact@1`.
pub fn params_schema() -> CValue {
    object(
        &["branch"],
        obj([("branch", described(pattern(NAME_PATTERN, 100.0), "Branch whose proposal impact is computed."))]),
    )
}

/// The result schema of `agent.compute_impact@1`.
pub fn result_schema() -> CValue {
    let classes: Vec<&str> = SideEffect::ALL.iter().map(|c| c.as_str()).collect();
    let outcomes: Vec<&str> = Outcome::ALL.iter().map(|o| o.as_str()).collect();
    let array = |items: CValue, max: f64, description: &str| {
        obj([
            ("type", text("array")),
            ("maxItems", CValue::Num(max)),
            ("items", items),
            ("description", text(description)),
        ])
    };
    let matrix_row = array(counter("Moves from this row's outcome to the column's."), 4.0, "One row of the matrix.");
    object(
        &[
            "added",
            "branch",
            "characteristics",
            "classes",
            "entry_count",
            "interpretation",
            "moves",
            "outcomes",
            "removed",
            "schema",
            "source_head",
            "target",
            "target_head",
        ],
        obj([
            ("added", counter("Records added on the branch.")),
            ("branch", described(pattern(NAME_PATTERN, 100.0), "The branch.")),
            ("characteristics", array(row_schema(), MAX_ROWS, "Changed characteristics, sorted by entity ID.")),
            ("classes", array(one_of(&classes), 11.0, "Side-effect classes of the branch's commands, in class order.")),
            ("entry_count", counter("Command entries on the branch since its fork point.")),
            (
                "interpretation",
                described(
                    one_of(&["none", "provisional", "signed_off"]),
                    "provisional when any listed record is (D-045); none without rows.",
                ),
            ),
            ("moves", array(matrix_row, 4.0, "moves[old][new] over the outcomes list.")),
            ("outcomes", array(one_of(&outcomes), 4.0, "Matrix order.")),
            ("removed", counter("Records removed on the branch.")),
            ("schema", one_of(&[IMPACT_SCHEMA])),
            ("source_head", or_null(pattern(DIGEST_PATTERN, 71.0), "Source head the diff used.")),
            ("target", described(pattern(NAME_PATTERN, 100.0), "Promotion target the diff ran against.")),
            ("target_head", or_null(pattern(DIGEST_PATTERN, 71.0), "Target head the diff used.")),
        ]),
    )
}

/// The definition of `agent.compute_impact@1` (R1, D0, CLI and MCP).
pub fn compute_impact_command_def() -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(COMPUTE_IMPACT_NAME)),
        ("major", CValue::Num(1.0)),
        ("title", text("Compute proposal impact")),
        (
            "description",
            text(
                "Computes from the branch diff against its promotion target which characteristics an agent \
                 branch would change in value, decision outcome or uncertainty class, the decision-move matrix, \
                 the command count and side-effect classes since the fork point, and both heads. Reports stored \
                 values only and changes nothing.",
            ),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
        ("side_effect", text("R1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
