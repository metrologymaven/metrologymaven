//! Golden-journal registration for upgrade OQ (spec node `QUAL.IQOQ.GOLDEN`).
//!
//! Lets a human principal register a branch entry range as a golden journal whose
//! `ResultRecord` digests define the expected outcome that OQ replays on every new
//! installation:
//! - Command `validate.register_golden@1` (`W2`, `D0`, `human_only: true`, MCP
//!   `request_only`; CLI `fid validate register-golden --branch --from --to --label
//!   [--supersedes] [--json]`).
//! - Any non-human principal (agent, script, service, device, system) is refused with
//!   `policy_refused` and nothing is journaled (REQ-QUAL.IQOQ.GOLDEN-002, D-031, AC-2).
//! - An entry range containing no committed `ResultRecord` fails with
//!   `validate.golden_empty` (REQ-QUAL.IQOQ.GOLDEN-003, AC-3).
//! - Registering with `supersedes: <golden_id>` retires the earlier golden so
//!   [`GoldenJournal::active_goldens`] excludes it from later OQ runs
//!   (REQ-QUAL.IQOQ.GOLDEN-004, AC-4); an unknown ID fails with `validate.golden_unknown`.

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_journal::entry::{
    build as build_entry, AiAssistance, CommandDraft, DraftBody, Entry, EntryDraft, EntryTime, Manifests, OutputDigest,
    PrincipalDraft, PrincipalKind, SaltSource,
};
use fid_registry::{CommandDef, DefError};
use fid_surface::{Envelope, ErrorCode, Outcome};
use fid_types::{Determinism, Digest, SideEffect};
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::record::CliOutput;

// ------------------------------------------------------------------ constants

/// Schema tag carried by every [`Golden`] record.
pub const GOLDEN_SCHEMA: &str = "qual-golden/1";
/// Project fixture format tag (`fixtures/qual/iqoq/golden/*.fid`).
pub const PROJECT_FORMAT: &str = "fid-qual-golden-project/1";

/// Unversioned registry command name for golden registration.
pub const REGISTER_GOLDEN_COMMAND: &str = "validate.register_golden";
/// Major version of `validate.register_golden`.
pub const REGISTER_GOLDEN_MAJOR: u32 = 1;
/// Versioned registry command ID (`validate.register_golden@1`).
pub const REGISTER_GOLDEN_ID: &str = "validate.register_golden@1";
/// CLI subcommand name under `fid validate`.
pub const CLI_REGISTER_GOLDEN: &str = "register-golden";
/// MCP tool name derived from `validate.register_golden@1`.
pub const MCP_REGISTER_GOLDEN: &str = "validate_register_golden";

/// Error code when the requested branch range has no committed `ResultRecord` (REQ-003, AC-3).
pub const ERR_GOLDEN_EMPTY: &str = "validate.golden_empty";
/// Error code when `supersedes` names an unknown golden ID.
pub const ERR_GOLDEN_UNKNOWN: &str = "validate.golden_unknown";
/// Error code when a non-human principal attempts `validate.register_golden@1` (REQ-002, AC-2).
pub const ERR_POLICY_REFUSED: &str = "policy_refused";

const LIFECYCLE_SINCE: &str = "2026-11-22";
const MAX_UNIX_MS: u64 = (1u64 << 48) - 1;
const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

/// Input byte budget for a `.fid` golden project journal (64 MiB).
pub const MAX_PROJECT_BYTES: usize = 64 * 1024 * 1024;
/// Input byte budget for command parameter JSON (16 KiB).
pub const MAX_PARAMS_BYTES: usize = 16 * 1024;
/// Maximum JSON nesting depth for project journals.
pub const MAX_JSON_DEPTH: usize = 64;
/// Maximum JSON nesting depth for command parameters.
pub const MAX_PARAMS_DEPTH: usize = 16;
/// Maximum entries allowed in a single project journal.
pub const MAX_PROJECT_ENTRIES: usize = 200_000;
/// Maximum registered goldens allowed in a single project journal.
pub const MAX_GOLDENS: usize = 10_000;
/// Maximum UTF-8 byte length for a golden label.
pub const MAX_LABEL_BYTES: usize = 256;
/// Maximum UTF-8 byte length for a branch name.
pub const MAX_BRANCH_BYTES: usize = 256;
/// Maximum CLI arguments accepted by [`invoke_cli`].
const MAX_CLI_ARGS: usize = 32;

// ------------------------------------------------------------------ untrusted wrapper

/// Taint-marked wrapper for untrusted user-supplied text (e.g. a golden's human label).
///
/// [`fmt::Display`] escapes ASCII control characters so untrusted labels cannot inject
/// terminal escapes or line breaks into logs or reports.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    /// Wraps a value as untrusted.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrows the inner value for explicit, audited handling.
    pub fn as_inner(&self) -> &T {
        &self.0
    }

    /// Consumes the wrapper and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Untrusted<String> {
    /// Validates and wraps an untrusted golden label (non-empty, bounded length, no control chars).
    pub fn label(raw: impl Into<String>) -> Result<Self, GoldenError> {
        let s = raw.into();
        if s.is_empty() {
            return Err(GoldenError::BadParams { field: "label".to_string(), reason: "must not be empty".to_string() });
        }
        if s.len() > MAX_LABEL_BYTES {
            return Err(GoldenError::BadParams {
                field: "label".to_string(),
                reason: format!("exceeds {MAX_LABEL_BYTES} bytes"),
            });
        }
        if s.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(GoldenError::BadParams {
                field: "label".to_string(),
                reason: "must not contain ASCII control characters".to_string(),
            });
        }
        Ok(Self(s))
    }

    /// Borrows the underlying label string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Untrusted<String> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            if ch.is_control() {
                write!(f, "\\u{{{:x}}}", u32::from(ch))?;
            } else {
                write!(f, "{ch}")?;
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ errors

/// Typed errors of the golden-journal registration module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoldenError {
    /// Non-human principal attempted a `human_only: true` command (`policy_refused`, REQ-002, AC-2).
    PolicyRefused { principal: PrincipalKind },
    /// Entry range contains no committed `ResultRecord` (`validate.golden_empty`, REQ-003, AC-3).
    GoldenEmpty { branch: String, from_seq: u64, to_seq: u64 },
    /// `supersedes` names an unknown golden ID (`validate.golden_unknown`).
    GoldenUnknown { id: Uuid },
    /// Invalid CLI, MCP or structural parameters.
    BadParams { field: String, reason: String },
    /// Malformed or tampered project journal.
    InvalidProject { field: String, reason: String },
    /// File read error when loading a project journal from disk.
    Io { reason: String },
    /// Internal canonicalisation or journal encoding failure.
    Internal { reason: String },
}

impl GoldenError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            GoldenError::PolicyRefused { .. } => ERR_POLICY_REFUSED,
            GoldenError::GoldenEmpty { .. } => ERR_GOLDEN_EMPTY,
            GoldenError::GoldenUnknown { .. } => ERR_GOLDEN_UNKNOWN,
            GoldenError::BadParams { .. } => "usage_error",
            GoldenError::InvalidProject { .. } => "validate.invalid_project",
            GoldenError::Io { .. } => "validate.io_error",
            GoldenError::Internal { .. } => "internal_error",
        }
    }

    /// Offending field or identifier for diagnostics.
    pub fn field(&self) -> &str {
        match self {
            GoldenError::PolicyRefused { .. } => "principal",
            GoldenError::GoldenEmpty { .. } => "range",
            GoldenError::GoldenUnknown { .. } => "supersedes",
            GoldenError::BadParams { field, .. } | GoldenError::InvalidProject { field, .. } => field,
            GoldenError::Io { .. } => "path",
            GoldenError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for GoldenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoldenError::PolicyRefused { principal } => {
                write!(f, "{ERR_POLICY_REFUSED}: validate.register_golden@1 is human_only (got {})", principal.as_str())
            }
            GoldenError::GoldenEmpty { branch, from_seq, to_seq } => {
                write!(
                    f,
                    "{ERR_GOLDEN_EMPTY}: no committed ResultRecord on branch {branch:?} in [{from_seq}, {to_seq}]"
                )
            }
            GoldenError::GoldenUnknown { id } => {
                write!(f, "{ERR_GOLDEN_UNKNOWN}: golden {id} not found")
            }
            GoldenError::BadParams { field, reason } => write!(f, "usage_error: {field}: {reason}"),
            GoldenError::InvalidProject { field, reason } => {
                write!(f, "validate.invalid_project: {field}: {reason}")
            }
            GoldenError::Io { reason } => write!(f, "validate.io_error: {reason}"),
            GoldenError::Internal { reason } => write!(f, "internal_error: {reason}"),
        }
    }
}

impl std::error::Error for GoldenError {}

fn bad_params(field: impl Into<String>, reason: impl Into<String>) -> GoldenError {
    GoldenError::BadParams { field: field.into(), reason: reason.into() }
}

fn invalid_project(field: impl Into<String>, reason: impl Into<String>) -> GoldenError {
    GoldenError::InvalidProject { field: field.into(), reason: reason.into() }
}

fn internal(reason: impl Into<String>) -> GoldenError {
    GoldenError::Internal { reason: reason.into() }
}

// ------------------------------------------------------------------ UUIDv7 (RFC 9562 §5.7)

/// Constructs an RFC 9562 §5.7 UUIDv7 from a 48-bit Unix millisecond timestamp, 12-bit
/// `rand_a`, and 62-bit `rand_b`.
pub fn uuid_v7_from_parts(unix_ms: u64, rand_a: u16, rand_b: u64) -> Result<Uuid, GoldenError> {
    if unix_ms > MAX_UNIX_MS {
        return Err(bad_params("unix_ms", "exceeds 48-bit Unix millisecond range"));
    }
    if rand_a >= (1u16 << 12) {
        return Err(bad_params("rand_a", "exceeds 12-bit range"));
    }
    if rand_b >= (1u64 << 62) {
        return Err(bad_params("rand_b", "exceeds 62-bit range"));
    }
    let n = (u128::from(unix_ms) << 80)
        | (0x7u128 << 76)
        | (u128::from(rand_a) << 64)
        | (0b10u128 << 62)
        | u128::from(rand_b);
    Ok(Uuid::from_u128(n))
}

/// Deterministically derives an RFC 9562 §5.7 UUIDv7 from `unix_ms` and domain-separated
/// seed material (D0: no ambient OS randomness).
pub fn deterministic_uuid_v7(unix_ms: u64, seed_material: &[u8]) -> Result<Uuid, GoldenError> {
    let hash = digest_bytes(seed_material).0;
    let rand_a = (u16::from_be_bytes([hash[0], hash[1]]) >> 4) & 0x0fff;
    let rand_b = u64::from_be_bytes([hash[2], hash[3], hash[4], hash[5], hash[6], hash[7], hash[8], hash[9]])
        & ((1u64 << 62) - 1);
    uuid_v7_from_parts(unix_ms, rand_a, rand_b)
}

fn parse_uuid_v7(s: &str, field: &str) -> Result<Uuid, GoldenError> {
    let u = Uuid::parse(s).map_err(|_| bad_params(field, "invalid UUID"))?;
    if u.version() != 7 || u.variant_bits() != 0b10 {
        return Err(bad_params(field, "expected RFC 9562 UUIDv7"));
    }
    Ok(u)
}

// ------------------------------------------------------------------ Golden data model

/// One registered golden journal range for upgrade OQ replay (REQ-QUAL.IQOQ.GOLDEN-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Golden {
    /// Immutable RFC 9562 UUIDv7 identifier.
    pub id: Uuid,
    /// Human-provided label, taint-marked as [`Untrusted`].
    pub label: Untrusted<String>,
    /// Branch whose entry range was registered.
    pub branch: String,
    /// Inclusive starting sequence number on `branch`.
    pub from_seq: u64,
    /// Inclusive ending sequence number on `branch`.
    pub to_seq: u64,
    /// Product version that recorded the golden range.
    pub product_version: String,
    /// Kernel manifest digest that recorded the golden range.
    pub kernel_manifest_digest: Digest,
    /// SHA-256 of the RFC 8785 canonical JSON array of `ResultRecord` digests in `[from_seq, to_seq]`.
    pub expected_digest: Digest,
    /// `true` once superseded by a newer golden registration (REQ-QUAL.IQOQ.GOLDEN-004).
    pub retired: bool,
    /// Optional ID of the earlier golden retired by this registration.
    pub supersedes: Option<Uuid>,
}

impl Golden {
    /// Validates internal invariants before serialisation or storage.
    pub fn validate(&self) -> Result<(), GoldenError> {
        if self.id.version() != 7 || self.id.variant_bits() != 0b10 {
            return Err(bad_params("id", "expected RFC 9562 UUIDv7"));
        }
        let _ = Untrusted::label(self.label.as_str())?;
        validate_branch(&self.branch)?;
        if self.from_seq == 0 || self.to_seq == 0 || self.from_seq > self.to_seq {
            return Err(bad_params("range", "requires 1 <= from_seq <= to_seq"));
        }
        if self.product_version.is_empty() {
            return Err(bad_params("product_version", "must not be empty"));
        }
        if let Some(sup) = self.supersedes {
            if sup == self.id {
                return Err(bad_params("supersedes", "golden cannot supersede itself"));
            }
        }
        Ok(())
    }

    /// Canonical JSON representation (`qual-golden/1`).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("branch".to_string(), text(&self.branch));
        m.insert("expected_digest".to_string(), text(self.expected_digest.as_str()));
        m.insert("from_seq".to_string(), CValue::Num(self.from_seq as f64));
        m.insert("id".to_string(), text(&self.id.to_string()));
        m.insert("kernel_manifest_digest".to_string(), text(self.kernel_manifest_digest.as_str()));
        m.insert("label".to_string(), text(self.label.as_str()));
        m.insert("product_version".to_string(), text(&self.product_version));
        m.insert("retired".to_string(), CValue::Bool(self.retired));
        m.insert("schema".to_string(), text(GOLDEN_SCHEMA));
        if let Some(sup) = self.supersedes {
            m.insert("supersedes".to_string(), text(&sup.to_string()));
        }
        m.insert("to_seq".to_string(), CValue::Num(self.to_seq as f64));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, GoldenError> {
        self.validate()?;
        to_canonical_bytes(&self.to_value()).map_err(|_| internal("canonical encoding of Golden failed"))
    }

    /// SHA-256 digest of [`Self::to_canonical_bytes`].
    pub fn result_digest(&self) -> Result<Digest, GoldenError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }

    /// Parses a [`Golden`] from a JSON object (`qual-golden/1`).
    pub fn from_value(v: &CValue) -> Result<Self, GoldenError> {
        let o = expect_obj(
            v,
            "golden",
            &[
                "branch",
                "expected_digest",
                "from_seq",
                "id",
                "kernel_manifest_digest",
                "label",
                "product_version",
                "retired",
                "schema",
                "supersedes",
                "to_seq",
            ],
        )?;
        let schema = req_str(o, "schema", "golden")?;
        if schema != GOLDEN_SCHEMA {
            return Err(invalid_project("golden.schema", format!("expected {GOLDEN_SCHEMA:?}")));
        }
        let id = parse_uuid_v7(req_str(o, "id", "golden")?, "golden.id")?;
        let label = Untrusted::label(req_str(o, "label", "golden")?)?;
        let branch = req_str(o, "branch", "golden")?.to_string();
        let from_seq = req_u64(o, "from_seq", "golden")?;
        let to_seq = req_u64(o, "to_seq", "golden")?;
        let product_version = req_str(o, "product_version", "golden")?.to_string();
        let kernel_manifest_digest = req_digest(o, "kernel_manifest_digest", "golden")?;
        let expected_digest = req_digest(o, "expected_digest", "golden")?;
        let retired = match o.get("retired") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(invalid_project("golden.retired", "expected boolean")),
        };
        let supersedes = match o.get("supersedes") {
            None => None,
            Some(CValue::Str(s)) => Some(parse_uuid_v7(s, "golden.supersedes")?),
            Some(_) => return Err(invalid_project("golden.supersedes", "expected UUID string")),
        };
        let golden = Golden {
            id,
            label,
            branch,
            from_seq,
            to_seq,
            product_version,
            kernel_manifest_digest,
            expected_digest,
            retired,
            supersedes,
        };
        golden.validate()?;
        Ok(golden)
    }
}

/// Computes `expected_digest` = SHA-256 of the RFC 8785 canonical JSON array of
/// `ResultRecord` digests in entry order.
pub fn compute_expected_digest(result_digests: &[Digest]) -> Result<Digest, GoldenError> {
    let arr = CValue::Arr(result_digests.iter().map(|d| text(d.as_str())).collect());
    let bytes = to_canonical_bytes(&arr).map_err(|_| internal("canonical encoding of result digest array failed"))?;
    Ok(Digest::from_bytes(digest_bytes(&bytes).0))
}

// ------------------------------------------------------------------ Project & journaled state

/// One committed entry in a project branch inspected by golden registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    /// Branch name (e.g. `"main"`).
    pub branch: String,
    /// Strictly positive sequence number on `branch`.
    pub seq: u64,
    /// Command ID that produced this entry (e.g. `"fit.circle@1"`).
    pub command_id: String,
    /// Product version that recorded this entry.
    pub product_version: String,
    /// Kernel manifest digest that recorded this entry.
    pub kernel_manifest_digest: Digest,
    /// Digest of the committed `ResultRecord`, if this entry produced one.
    pub result_record_digest: Option<Digest>,
}

/// One committed `validate.register_golden@1` journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenJournalEvent {
    /// Monotonic event sequence number within the golden journal.
    pub seq: u64,
    /// Command ID (`validate.register_golden@1`).
    pub command_id: String,
    /// SHA-256 of the canonical [`Golden`] JSON at registration time.
    pub result_digest: Digest,
    /// Journal entry hash linking this commitment into the journal chain.
    pub entry_hash: Digest,
    /// Snapshot of the registered [`Golden`] (with `retired = false` at commit time).
    pub golden: Golden,
}

/// Journaled state of project entries and registered goldens.
///
/// [`Self::active_goldens`] returns all non-retired goldens in deterministic registration
/// order for module `oq`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoldenJournal {
    entries: Vec<ProjectEntry>,
    goldens: BTreeMap<Uuid, Golden>,
    registration_order: Vec<Uuid>,
    committed_events: Vec<GoldenJournalEvent>,
}

impl GoldenJournal {
    /// Creates an empty project golden journal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one recorded project entry.
    pub fn push_entry(&mut self, entry: ProjectEntry) -> Result<(), GoldenError> {
        if self.entries.len() >= MAX_PROJECT_ENTRIES {
            return Err(invalid_project("entries", "exceeds MAX_PROJECT_ENTRIES"));
        }
        validate_branch(&entry.branch)?;
        if entry.seq == 0 {
            return Err(invalid_project("entry.seq", "sequence number must be >= 1"));
        }
        if entry.command_id.is_empty() {
            return Err(invalid_project("entry.command_id", "must not be empty"));
        }
        if entry.product_version.is_empty() {
            return Err(invalid_project("entry.product_version", "must not be empty"));
        }
        self.entries.push(entry);
        Ok(())
    }

    /// All project entries in journal order.
    pub fn entries(&self) -> &[ProjectEntry] {
        &self.entries
    }

    /// Committed `validate.register_golden@1` journal events in commit order.
    pub fn committed_events(&self) -> &[GoldenJournalEvent] {
        &self.committed_events
    }

    /// Looks up a registered golden (active or retired) by UUID.
    pub fn get_golden(&self, id: &Uuid) -> Option<&Golden> {
        self.goldens.get(id)
    }

    /// All registered goldens (active and retired) in deterministic registration order.
    pub fn all_goldens(&self) -> Vec<&Golden> {
        self.registration_order.iter().filter_map(|id| self.goldens.get(id)).collect()
    }

    /// Active (non-retired) goldens in deterministic registration order, consumed by OQ
    /// (`QUAL.IQOQ.OQ`, REQ-QUAL.IQOQ.GOLDEN-004).
    pub fn active_goldens(&self) -> Vec<&Golden> {
        self.registration_order.iter().filter_map(|id| self.goldens.get(id)).filter(|g| !g.retired).collect()
    }

    /// Collects `ResultRecord` digests on `branch` within `[from_seq, to_seq]` in entry
    /// order and returns `(expected_digest, product_version, kernel_manifest_digest)`.
    pub fn range_summary(
        &self,
        branch: &str,
        from_seq: u64,
        to_seq: u64,
    ) -> Result<(Digest, String, Digest), GoldenError> {
        validate_branch(branch)?;
        if from_seq == 0 || to_seq == 0 || from_seq > to_seq {
            return Err(bad_params("range", "requires 1 <= from_seq <= to_seq"));
        }
        let mut digests: Vec<Digest> = Vec::new();
        let mut recorded_meta: Option<(&str, Digest)> = None;
        for entry in &self.entries {
            if entry.branch != branch || entry.seq < from_seq || entry.seq > to_seq {
                continue;
            }
            if let Some(d) = entry.result_record_digest {
                digests.push(d);
                recorded_meta = Some((entry.product_version.as_str(), entry.kernel_manifest_digest));
            }
        }
        if digests.is_empty() {
            return Err(GoldenError::GoldenEmpty { branch: branch.to_string(), from_seq, to_seq });
        }
        let Some((product_version, kernel_manifest_digest)) = recorded_meta else {
            return Err(GoldenError::GoldenEmpty { branch: branch.to_string(), from_seq, to_seq });
        };
        let expected_digest = compute_expected_digest(&digests)?;
        Ok((expected_digest, product_version.to_string(), kernel_manifest_digest))
    }

    /// Parses a `.fid` project fixture (`fid-qual-golden-project/1`) with strict size and
    /// depth limits.
    pub fn from_json(bytes: &[u8]) -> Result<Self, GoldenError> {
        let v = parse_with_limits(bytes, MAX_PROJECT_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| invalid_project("project", "not acceptable JSON or exceeds size/depth limit"))?;
        let o = expect_obj(&v, "project", &["entries", "format", "goldens", "license", "title"])?;
        let fmt = req_str(o, "format", "project")?;
        if fmt != PROJECT_FORMAT {
            return Err(invalid_project("project.format", format!("expected {PROJECT_FORMAT:?}")));
        }
        let entries_arr = match o.get("entries") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(invalid_project("project.entries", "expected array")),
        };
        if entries_arr.len() > MAX_PROJECT_ENTRIES {
            return Err(invalid_project("project.entries", "exceeds MAX_PROJECT_ENTRIES"));
        }
        let mut journal = GoldenJournal::new();
        for (idx, item) in entries_arr.iter().enumerate() {
            let path = format!("project.entries[{idx}]");
            let eo = expect_obj(
                item,
                &path,
                &["branch", "command_id", "kernel_manifest_digest", "product_version", "result_record_digest", "seq"],
            )?;
            let branch = req_str(eo, "branch", &path)?.to_string();
            let seq = req_u64(eo, "seq", &path)?;
            let command_id = req_str(eo, "command_id", &path)?.to_string();
            let product_version = req_str(eo, "product_version", &path)?.to_string();
            let kernel_manifest_digest = req_digest(eo, "kernel_manifest_digest", &path)?;
            let result_record_digest = match eo.get("result_record_digest") {
                None | Some(CValue::Null) => None,
                Some(CValue::Str(s)) => Some(
                    Digest::parse(s)
                        .map_err(|_| invalid_project(format!("{path}.result_record_digest"), "invalid digest"))?,
                ),
                Some(_) => {
                    return Err(invalid_project(
                        format!("{path}.result_record_digest"),
                        "expected digest string or null",
                    ));
                }
            };
            journal.push_entry(ProjectEntry {
                branch,
                seq,
                command_id,
                product_version,
                kernel_manifest_digest,
                result_record_digest,
            })?;
        }
        if let Some(goldens_val) = o.get("goldens") {
            let CValue::Arr(g_arr) = goldens_val else {
                return Err(invalid_project("project.goldens", "expected array"));
            };
            if g_arr.len() > MAX_GOLDENS {
                return Err(invalid_project("project.goldens", "exceeds MAX_GOLDENS"));
            }
            for item in g_arr {
                let g = Golden::from_value(item)?;
                let id = g.id;
                if journal.goldens.contains_key(&id) {
                    return Err(invalid_project("project.goldens", "duplicate golden id"));
                }
                journal.goldens.insert(id, g);
                journal.registration_order.push(id);
            }
        }
        Ok(journal)
    }

    /// Loads a `.fid` project fixture from disk with a byte cap before reading.
    pub fn load_file(path: &Path) -> Result<Self, GoldenError> {
        let meta = std::fs::metadata(path).map_err(|e| GoldenError::Io { reason: e.to_string() })?;
        if meta.len() > MAX_PROJECT_BYTES as u64 {
            return Err(invalid_project("project", "file exceeds MAX_PROJECT_BYTES"));
        }
        let bytes = std::fs::read(path).map_err(|e| GoldenError::Io { reason: e.to_string() })?;
        Self::from_json(&bytes)
    }
}

// ------------------------------------------------------------------ validate.register_golden@1

/// Typed parameters of one `validate.register_golden@1` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterGoldenCall {
    pub branch: String,
    pub from_seq: u64,
    pub to_seq: u64,
    pub label: Untrusted<String>,
    pub supersedes: Option<Uuid>,
}

impl RegisterGoldenCall {
    /// Canonical parameter object matching `params_schema`.
    pub fn params(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("branch".to_string(), text(&self.branch));
        m.insert("from_seq".to_string(), CValue::Num(self.from_seq as f64));
        m.insert("label".to_string(), text(self.label.as_str()));
        if let Some(sup) = self.supersedes {
            m.insert("supersedes".to_string(), text(&sup.to_string()));
        }
        m.insert("to_seq".to_string(), CValue::Num(self.to_seq as f64));
        CValue::Obj(m)
    }

    /// Parses and validates `{ branch, from_seq, to_seq, label, supersedes? }`.
    pub fn from_value(v: &CValue) -> Result<Self, GoldenError> {
        let CValue::Obj(o) = v else {
            return Err(bad_params("params", "expected JSON object"));
        };
        for k in o.keys() {
            if !matches!(k.as_str(), "branch" | "from_seq" | "to_seq" | "label" | "supersedes") {
                return Err(bad_params(format!("params.{k}"), "unknown parameter"));
            }
        }
        let branch = match o.get("branch") {
            Some(CValue::Str(s)) => {
                validate_branch(s)?;
                s.clone()
            }
            _ => return Err(bad_params("branch", "expected non-empty string")),
        };
        let from_seq = parse_param_u64(o.get("from_seq"), "from_seq")?;
        let to_seq = parse_param_u64(o.get("to_seq"), "to_seq")?;
        if from_seq == 0 || to_seq == 0 || from_seq > to_seq {
            return Err(bad_params("range", "requires 1 <= from_seq <= to_seq"));
        }
        let label = match o.get("label") {
            Some(CValue::Str(s)) => Untrusted::label(s.as_str())?,
            _ => return Err(bad_params("label", "expected string")),
        };
        let supersedes = match o.get("supersedes") {
            None => None,
            Some(CValue::Str(s)) => Some(parse_uuid_v7(s, "supersedes")?),
            Some(_) => return Err(bad_params("supersedes", "expected UUIDv7 string")),
        };
        Ok(RegisterGoldenCall { branch, from_seq, to_seq, label, supersedes })
    }
}

/// Executes `validate.register_golden@1` (W2, D0, `human_only: true`).
///
/// Checks principal first: any non-human principal is refused with `policy_refused` and
/// nothing is committed (REQ-QUAL.IQOQ.GOLDEN-002, AC-2).
pub fn register_golden(
    journal: &mut GoldenJournal,
    principal: PrincipalKind,
    call: &RegisterGoldenCall,
    unix_ms: u64,
) -> Result<(Golden, Digest), GoldenError> {
    // 1. D-031 human_only gate: refuse non-human principals before mutating anything.
    if principal != PrincipalKind::Human {
        return Err(GoldenError::PolicyRefused { principal });
    }
    if journal.goldens.len() >= MAX_GOLDENS {
        return Err(invalid_project("goldens", "exceeds MAX_GOLDENS"));
    }

    // 2. Validate parameters and supersedes target before mutating anything.
    validate_branch(&call.branch)?;
    if call.from_seq == 0 || call.to_seq == 0 || call.from_seq > call.to_seq {
        return Err(bad_params("range", "requires 1 <= from_seq <= to_seq"));
    }
    let _ = Untrusted::label(call.label.as_str())?;
    if let Some(old_id) = call.supersedes {
        if !journal.goldens.contains_key(&old_id) {
            return Err(GoldenError::GoldenUnknown { id: old_id });
        }
    }

    // 3. Compute expected_digest over committed ResultRecord digests in [from_seq, to_seq].
    let (expected_digest, product_version, kernel_manifest_digest) =
        journal.range_summary(&call.branch, call.from_seq, call.to_seq)?;

    // 4. Derive deterministic UUIDv7 (D0).
    let next_event_seq = (journal.committed_events.len() as u64) + 1;
    let seed_obj = obj([
        ("branch", text(&call.branch)),
        ("event_seq", CValue::Num(next_event_seq as f64)),
        ("expected_digest", text(expected_digest.as_str())),
        ("from_seq", CValue::Num(call.from_seq as f64)),
        ("label", text(call.label.as_str())),
        ("to_seq", CValue::Num(call.to_seq as f64)),
        ("unix_ms", CValue::Num(unix_ms as f64)),
    ]);
    let seed_bytes = to_canonical_bytes(&seed_obj).map_err(|_| internal("canonical encoding of UUIDv7 seed failed"))?;
    let id = deterministic_uuid_v7(unix_ms, &seed_bytes)?;

    let golden = Golden {
        id,
        label: call.label.clone(),
        branch: call.branch.clone(),
        from_seq: call.from_seq,
        to_seq: call.to_seq,
        product_version,
        kernel_manifest_digest,
        expected_digest,
        retired: false,
        supersedes: call.supersedes,
    };
    let result_digest = golden.result_digest()?;

    // 5. Commit atomically: build entry hash, retire superseded golden, insert new golden.
    let prev_hash = journal.committed_events.last().map_or("genesis", |ev| ev.entry_hash.as_str());
    let commit_obj = obj([
        ("command_id", text(REGISTER_GOLDEN_ID)),
        ("golden_digest", text(result_digest.as_str())),
        ("prev_hash", text(prev_hash)),
        ("seq", CValue::Num(next_event_seq as f64)),
    ]);
    let commit_bytes =
        to_canonical_bytes(&commit_obj).map_err(|_| internal("canonical encoding of golden commit failed"))?;
    let entry_hash = Digest::from_bytes(digest_bytes(&commit_bytes).0);

    if let Some(old_id) = call.supersedes {
        if let Some(old) = journal.goldens.get_mut(&old_id) {
            old.retired = true;
        }
    }
    journal.goldens.insert(id, golden.clone());
    journal.registration_order.push(id);
    journal.committed_events.push(GoldenJournalEvent {
        seq: next_event_seq,
        command_id: REGISTER_GOLDEN_ID.to_string(),
        result_digest,
        entry_hash,
        golden: golden.clone(),
    });
    Ok((golden, entry_hash))
}

/// Builds a full `CORE.JRN.ENTRY` (`fid_journal::entry::Entry`) committing one registered
/// [`Golden`] under `validate.register_golden@1` (`W2`, `D0`).
pub fn build_register_golden_entry(
    golden: &Golden,
    seq: u64,
    prev_hash: CanonDigest,
    time: EntryTime,
    human_identity: &str,
    registry_manifest: CanonDigest,
    salts: &mut dyn SaltSource,
) -> Result<Entry, GoldenError> {
    let result_digest = golden.result_digest()?;
    let mut args = BTreeMap::new();
    args.insert("branch".to_string(), text(&golden.branch));
    args.insert("from_seq".to_string(), CValue::Num(golden.from_seq as f64));
    args.insert("label".to_string(), text(golden.label.as_str()));
    if let Some(sup) = golden.supersedes {
        args.insert("supersedes".to_string(), text(&sup.to_string()));
    }
    args.insert("to_seq".to_string(), CValue::Num(golden.to_seq as f64));

    let draft = EntryDraft {
        stream: "record".to_string(),
        branch: golden.branch.clone(),
        logical_parent: None,
        time,
        principal: PrincipalDraft {
            kind: PrincipalKind::Human,
            identity: CValue::Str(human_identity.to_string()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: Manifests {
            registry: registry_manifest,
            kernel: CanonDigest(golden.kernel_manifest_digest.to_bytes()),
        },
        reason: Some(format!("register golden {}", golden.id)),
        human_assertion: None,
        body: DraftBody::Command(CommandDraft {
            id: REGISTER_GOLDEN_COMMAND.to_string(),
            major: REGISTER_GOLDEN_MAJOR,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: 0,
            args: CValue::Obj(args),
            events: vec![golden.to_value()],
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs: vec![OutputDigest {
                entity: Some(format!("golden:{}", golden.id)),
                digest: CanonDigest(result_digest.to_bytes()),
            }],
            algorithms: Vec::new(),
        }),
    };
    build_entry(&draft, seq, prev_hash, salts).map_err(|e| internal(format!("failed to build journal entry: {e}")))
}

// ------------------------------------------------------------------ Envelope, CLI & MCP surfaces

fn map_error_code(e: &GoldenError) -> ErrorCode {
    match e {
        GoldenError::PolicyRefused { .. } => ErrorCode::PolicyRefused,
        GoldenError::GoldenEmpty { .. } | GoldenError::InvalidProject { .. } => ErrorCode::ValidationError,
        GoldenError::GoldenUnknown { .. } => ErrorCode::NotFound,
        GoldenError::BadParams { .. } => ErrorCode::UsageError,
        GoldenError::Io { .. } | GoldenError::Internal { .. } => ErrorCode::InternalError,
    }
}

/// Builds a canonical `failed` [`Envelope`] carrying the domain error code in
/// `details.qual_error`.
pub fn failed_envelope(command: &str, e: &GoldenError) -> Envelope {
    let mut details = BTreeMap::new();
    details.insert("field".to_string(), text(e.field()));
    details.insert("qual_error".to_string(), text(e.code()));
    if let GoldenError::GoldenUnknown { id } = e {
        details.insert("supersedes".to_string(), text(&id.to_string()));
    }
    Envelope::failed(command, map_error_code(e), &e.to_string(), Some(CValue::Obj(details)))
}

/// Runs `validate.register_golden@1` and wraps the outcome in the shared `fid-surface`
/// [`Envelope`].
pub fn run(journal: &mut GoldenJournal, principal: PrincipalKind, call: &RegisterGoldenCall, unix_ms: u64) -> Envelope {
    match register_golden(journal, principal, call, unix_ms) {
        Ok((golden, entry_hash)) => Envelope {
            journal_entry_hash: Some(entry_hash.to_string()),
            result: Some(golden.to_value()),
            ..Envelope::bare(REGISTER_GOLDEN_ID, Outcome::Committed)
        },
        Err(e) => failed_envelope(REGISTER_GOLDEN_ID, &e),
    }
}

fn cli_output(env: Envelope, json: bool) -> CliOutput {
    let printed = if json {
        env.encode().map_err(|_| internal("envelope encoding failed"))
    } else {
        match &env.result {
            Some(r) => to_canonical_bytes(r).map_err(|_| internal("canonical encoding of result failed")),
            None => Ok(Vec::new()),
        }
    };
    match printed {
        Ok(stdout) => {
            let stderr = match (&env.error, json) {
                (Some(e), false) => format!("error: {}\n", e.message),
                _ => String::new(),
            };
            CliOutput { envelope: env, stdout, stderr }
        }
        Err(e) => {
            let env = failed_envelope(&env.command_id, &e);
            let stdout = if json { env.encode().unwrap_or_default() } else { Vec::new() };
            CliOutput { envelope: env, stdout, stderr: format!("error: {e}\n") }
        }
    }
}

fn parse_cli(args: &[&str]) -> Result<(RegisterGoldenCall, bool), (&'static str, GoldenError)> {
    if args.len() > MAX_CLI_ARGS {
        return Err(("validate", bad_params("args", "too many arguments")));
    }
    let (sub, rest) = args.split_first().ok_or(("validate", bad_params("args", "missing subcommand")))?;
    if *sub != CLI_REGISTER_GOLDEN {
        return Err(("validate", bad_params("args", "unknown subcommand")));
    }
    let mut branch: Option<String> = None;
    let mut from_seq: Option<u64> = None;
    let mut to_seq: Option<u64> = None;
    let mut label: Option<Untrusted<String>> = None;
    let mut supersedes: Option<Uuid> = None;
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--branch" if branch.is_none() => {
                let v = it.next().ok_or((REGISTER_GOLDEN_ID, bad_params("branch", "option needs a value")))?;
                validate_branch(v).map_err(|e| (REGISTER_GOLDEN_ID, e))?;
                branch = Some((*v).to_string());
            }
            "--from" if from_seq.is_none() => {
                let v = it.next().ok_or((REGISTER_GOLDEN_ID, bad_params("from_seq", "option needs a value")))?;
                let n: u64 =
                    v.parse().map_err(|_| (REGISTER_GOLDEN_ID, bad_params("from_seq", "expected positive integer")))?;
                from_seq = Some(n);
            }
            "--to" if to_seq.is_none() => {
                let v = it.next().ok_or((REGISTER_GOLDEN_ID, bad_params("to_seq", "option needs a value")))?;
                let n: u64 =
                    v.parse().map_err(|_| (REGISTER_GOLDEN_ID, bad_params("to_seq", "expected positive integer")))?;
                to_seq = Some(n);
            }
            "--label" if label.is_none() => {
                let v = it.next().ok_or((REGISTER_GOLDEN_ID, bad_params("label", "option needs a value")))?;
                let l = Untrusted::label(*v).map_err(|e| (REGISTER_GOLDEN_ID, e))?;
                label = Some(l);
            }
            "--supersedes" if supersedes.is_none() => {
                let v = it.next().ok_or((REGISTER_GOLDEN_ID, bad_params("supersedes", "option needs a value")))?;
                let u = parse_uuid_v7(v, "supersedes").map_err(|e| (REGISTER_GOLDEN_ID, e))?;
                supersedes = Some(u);
            }
            _ => return Err((REGISTER_GOLDEN_ID, bad_params("args", "unknown or repeated option"))),
        }
    }
    let branch = branch.ok_or((REGISTER_GOLDEN_ID, bad_params("branch", "missing --branch")))?;
    let from_seq = from_seq.ok_or((REGISTER_GOLDEN_ID, bad_params("from_seq", "missing --from")))?;
    let to_seq = to_seq.ok_or((REGISTER_GOLDEN_ID, bad_params("to_seq", "missing --to")))?;
    let label = label.ok_or((REGISTER_GOLDEN_ID, bad_params("label", "missing --label")))?;
    if from_seq == 0 || to_seq == 0 || from_seq > to_seq {
        return Err((REGISTER_GOLDEN_ID, bad_params("range", "requires 1 <= from_seq <= to_seq")));
    }
    Ok((RegisterGoldenCall { branch, from_seq, to_seq, label, supersedes }, json))
}

/// Runs CLI `fid validate register-golden --branch <b> --from <n> --to <m> --label <l>
/// [--supersedes <uuid>] [--json]`.
pub fn invoke_cli(journal: &mut GoldenJournal, principal: PrincipalKind, unix_ms: u64, args: &[&str]) -> CliOutput {
    let json = args.contains(&"--json");
    match parse_cli(args) {
        Ok((call, json)) => cli_output(run(journal, principal, &call, unix_ms), json),
        Err((command, e)) => {
            let id = if command == "validate" { "cli.invoke@1" } else { command };
            cli_output(failed_envelope(id, &e), json)
        }
    }
}

/// Runs MCP tool `validate_register_golden` on raw JSON parameters.
///
/// Refuses non-human principals (e.g. `PrincipalKind::Agent` or `PrincipalKind::Script`)
/// with `policy_refused` and commits nothing (REQ-QUAL.IQOQ.GOLDEN-002, AC-2).
pub fn invoke_mcp(
    journal: &mut GoldenJournal,
    principal: PrincipalKind,
    unix_ms: u64,
    tool: &str,
    params: &[u8],
) -> Envelope {
    if tool != MCP_REGISTER_GOLDEN {
        return failed_envelope("mcp.invoke@1", &bad_params("tool", "unknown tool"));
    }
    if principal != PrincipalKind::Human {
        return failed_envelope(REGISTER_GOLDEN_ID, &GoldenError::PolicyRefused { principal });
    }
    let parsed = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| bad_params("params", "not acceptable JSON"))
        .and_then(|v| RegisterGoldenCall::from_value(&v));
    match parsed {
        Ok(call) => run(journal, principal, &call, unix_ms),
        Err(e) => failed_envelope(REGISTER_GOLDEN_ID, &e),
    }
}

// ------------------------------------------------------------------ registry definition

/// Registry command definition for `validate.register_golden@1` (W2, D0, `human_only: true`,
/// MCP `request_only`, REQ-QUAL.IQOQ.GOLDEN-002).
pub fn command_def() -> Result<CommandDef, DefError> {
    let digest_schema = || obj([("pattern", text(Digest::PATTERN)), ("type", text("string"))]);
    let non_empty_string = || obj([("minLength", CValue::Num(1.0)), ("type", text("string"))]);
    let uuid_schema = || obj([("pattern", text(UUID_PATTERN)), ("type", text("string"))]);
    let pos_int = || obj([("minimum", CValue::Num(1.0)), ("type", text("integer"))]);

    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("branch", non_empty_string()),
                ("from_seq", pos_int()),
                ("label", non_empty_string()),
                ("supersedes", uuid_schema()),
                ("to_seq", pos_int()),
            ]),
        ),
        ("required", strs(&["branch", "from_seq", "label", "to_seq"])),
        ("type", text("object")),
    ]);

    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("branch", non_empty_string()),
                ("expected_digest", digest_schema()),
                ("from_seq", pos_int()),
                ("id", uuid_schema()),
                ("kernel_manifest_digest", digest_schema()),
                ("label", non_empty_string()),
                ("product_version", non_empty_string()),
                ("retired", obj([("type", text("boolean"))])),
                ("schema", obj([("const", text(GOLDEN_SCHEMA)), ("type", text("string"))])),
                ("supersedes", uuid_schema()),
                ("to_seq", pos_int()),
            ]),
        ),
        (
            "required",
            strs(&[
                "branch",
                "expected_digest",
                "from_seq",
                "id",
                "kernel_manifest_digest",
                "label",
                "product_version",
                "retired",
                "schema",
                "to_seq",
            ]),
        ),
        ("type", text("object")),
    ]);

    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text(
                "Registers a committed branch entry range as a golden journal baseline for upgrade OQ. \
                 Human-only (D-031): computes the expected outcome digest from ResultRecord digests in \
                 the range and optionally retires a superseded golden.",
            ),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&["validate.golden_registered"])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(true)),
        ("id", text(REGISTER_GOLDEN_COMMAND)),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(f64::from(REGISTER_GOLDEN_MAJOR))),
        ("mcp_projection", text("request_only")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Register a golden journal baseline for upgrade OQ")),
        ("undo", text("not_undoable")),
        ("web", text("available")),
    ]))
}

/// Registry command definitions exported by `golden`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![command_def()?])
}

// ------------------------------------------------------------------ JSON helpers

fn validate_branch(branch: &str) -> Result<(), GoldenError> {
    if branch.is_empty() {
        return Err(bad_params("branch", "must not be empty"));
    }
    if branch.len() > MAX_BRANCH_BYTES {
        return Err(bad_params("branch", format!("exceeds {MAX_BRANCH_BYTES} bytes")));
    }
    if branch.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return Err(bad_params("branch", "must not contain whitespace or control characters"));
    }
    Ok(())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn expect_obj<'a>(v: &'a CValue, path: &str, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, GoldenError> {
    let CValue::Obj(o) = v else {
        return Err(invalid_project(path, "expected JSON object"));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid_project(format!("{path}.{k}"), "unknown field"));
        }
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, key: &str, path: &str) -> Result<&'a str, GoldenError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(invalid_project(format!("{path}.{key}"), "expected string")),
    }
}

fn req_digest(o: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<Digest, GoldenError> {
    let s = req_str(o, key, path)?;
    Digest::parse(s).map_err(|_| invalid_project(format!("{path}.{key}"), "invalid sha256 digest"))
}

fn cvalue_to_u64(v: &CValue) -> Option<u64> {
    let CValue::Num(n) = v else {
        return None;
    };
    if !n.is_finite() || *n < 0.0 || n.fract() != 0.0 || *n > 9_007_199_254_740_991.0 {
        return None;
    }
    Some(*n as u64)
}

fn req_u64(o: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<u64, GoldenError> {
    o.get(key)
        .and_then(cvalue_to_u64)
        .ok_or_else(|| invalid_project(format!("{path}.{key}"), "expected non-negative integer"))
}

fn parse_param_u64(v: Option<&CValue>, field: &str) -> Result<u64, GoldenError> {
    v.and_then(cvalue_to_u64).ok_or_else(|| bad_params(field, "expected positive integer"))
}
