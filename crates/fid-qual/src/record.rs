//! Qualification record (`qual-record/1`), journal integration, status computation and the
//! `validate.show_record@1` command (spec node `QUAL.IQOQ.RECORD`).
//!
//! # Guarantees
//! - Every [`QualificationRecord`] serialises deterministically as RFC 8785 canonical JSON
//!   through `fid-canon`, and [`QualificationRecord::result_digest`] is the SHA-256 of those
//!   exact canonical bytes (REQ-QUAL.IQOQ.RECORD-002).
//! - Each IQ or OQ run commits **exactly one** journal event (`validate.run_iq@1` or
//!   `validate.run_oq@1`) whose body is the record and whose result digest equals
//!   [`QualificationRecord::result_digest`].
//! - `validate.show_record@1` is side-effect class `R0` and determinism class `D0`. It takes
//!   the Record Journal by immutable reference (`&RecordJournal`) so it can never mutate or
//!   append to the Record Journal, logging calls to the Activity Log only (D-031).
//! - Both CLI (`fid validate show-record [--id <id>] [--json]`) and MCP
//!   (`validate_show_record`) run through the single shared handler [`run`] and return
//!   byte-identical RFC 8785 canonical JSON envelopes.

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_journal::entry::{
    build as build_entry, verify_commitment, AiAssistance, CommandDraft, DraftBody, Entry, EntryDraft, EntryTime,
    Field, HeaderBody, Manifests, OutputDigest, PrincipalDraft, SaltSource,
};
use fid_registry::{CommandDef, DefError};
use fid_surface::{Envelope, ErrorCode};
use fid_types::{Determinism, Digest, SideEffect};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

// ------------------------------------------------------------------ constants

/// Schema tag carried by every [`QualificationRecord`].
pub const RECORD_SCHEMA: &str = "qual-record/1";
/// Synthetic journal fixture format tag (`fixtures/qual/iqoq/records/*.fid`).
pub const JOURNAL_FORMAT: &str = "fid-qual-journal/1";

/// Registry command name for an Installation Qualification run.
pub const RUN_IQ_COMMAND: &str = "validate.run_iq";
/// Major version for `validate.run_iq`.
pub const RUN_IQ_MAJOR: u32 = 1;
/// Full versioned command ID for an IQ run.
pub const RUN_IQ_ID: &str = "validate.run_iq@1";

/// Registry command name for an Operational Qualification run.
pub const RUN_OQ_COMMAND: &str = "validate.run_oq";
/// Major version for `validate.run_oq`.
pub const RUN_OQ_MAJOR: u32 = 1;
/// Full versioned command ID for an OQ run.
pub const RUN_OQ_ID: &str = "validate.run_oq@1";

/// Registry command name for `validate.show_record@1`.
pub const SHOW_RECORD_COMMAND: &str = "validate.show_record";
/// Full versioned command ID for `validate.show_record@1`.
pub const SHOW_RECORD_ID: &str = "validate.show_record@1";
/// CLI subcommand under `fid validate`.
pub const CLI_SHOW_RECORD: &str = "show-record";
/// MCP tool name derived from `validate.show_record@1`.
pub const MCP_SHOW_RECORD: &str = "validate_show_record";

/// Error code returned when a requested qualification record ID is absent (REQ-003, AC-4).
pub const ERR_RECORD_NOT_FOUND: &str = "validate.record_not_found";

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Input byte budget for a single `QualificationRecord` JSON document (4 MiB).
pub const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;
/// Input byte budget for a `.fid` qualification journal fixture (64 MiB).
pub const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;
/// Input byte budget for `validate.show_record@1` MCP/CLI parameter JSON (16 KiB).
pub const MAX_PARAMS_BYTES: usize = 16 * 1024;
/// Input byte budget for one Activity Log entry (32 KiB).
pub const MAX_ACTIVITY_BYTES: usize = 32 * 1024;
/// JSON nesting depth limit for records and journals.
pub const MAX_JSON_DEPTH: usize = 64;
/// JSON nesting depth limit for command parameters.
pub const MAX_PARAMS_DEPTH: usize = 16;
/// Maximum checks allowed in a single qualification record.
pub const MAX_CHECKS: usize = 100_000;
/// Maximum golden outcomes allowed in a single OQ record.
pub const MAX_GOLDENS: usize = 10_000;
/// Maximum exceedances allowed in a single golden outcome.
pub const MAX_EXCEEDANCES: usize = 10_000;
/// Maximum reasons allowed in a single golden outcome.
pub const MAX_REASONS: usize = 1_000;
/// Maximum events accepted in a single `.fid` journal.
pub const MAX_JOURNAL_EVENTS: usize = 200_000;
/// Maximum CLI arguments accepted by [`invoke_cli`].
const MAX_CLI_ARGS: usize = 32;

// ------------------------------------------------------------------ errors

/// Typed errors of the qualification record module. Every variant carries a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualError {
    /// Requested record ID does not exist in the Record Journal (`validate.record_not_found`).
    RecordNotFound { id: String },
    /// A `QualificationRecord` violates `qual-record/1` rules.
    InvalidRecord { field: String, reason: String },
    /// A journal or entry failed verification (digest mismatch, commitment mismatch, malformed shape).
    InvalidJournal { field: String, reason: String },
    /// Invalid CLI or MCP invocation parameters.
    BadParams { field: String, reason: String },
    /// File read error when loading a journal fixture from disk.
    Io { reason: String },
    /// Internal canonicalisation or envelope encoding failure.
    Internal { reason: String },
}

impl QualError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            QualError::RecordNotFound { .. } => ERR_RECORD_NOT_FOUND,
            QualError::InvalidRecord { .. } => "validate.invalid_record",
            QualError::InvalidJournal { .. } => "validate.invalid_journal",
            QualError::BadParams { .. } => "usage_error",
            QualError::Io { .. } => "validate.io_error",
            QualError::Internal { .. } => "internal_error",
        }
    }

    /// The offending field or identifier, for diagnostics.
    pub fn field(&self) -> &str {
        match self {
            QualError::RecordNotFound { .. } => "id",
            QualError::InvalidRecord { field, .. }
            | QualError::InvalidJournal { field, .. }
            | QualError::BadParams { field, .. } => field,
            QualError::Io { .. } => "path",
            QualError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for QualError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QualError::RecordNotFound { id } => write!(f, "{ERR_RECORD_NOT_FOUND}: record {id:?} not found"),
            QualError::InvalidRecord { field, reason } => write!(f, "validate.invalid_record: {field}: {reason}"),
            QualError::InvalidJournal { field, reason } => write!(f, "validate.invalid_journal: {field}: {reason}"),
            QualError::BadParams { field, reason } => write!(f, "usage_error: {field}: {reason}"),
            QualError::Io { reason } => write!(f, "validate.io_error: {reason}"),
            QualError::Internal { reason } => write!(f, "internal_error: {reason}"),
        }
    }
}

impl std::error::Error for QualError {}

fn invalid_record(field: impl Into<String>, reason: impl Into<String>) -> QualError {
    QualError::InvalidRecord { field: field.into(), reason: reason.into() }
}

fn invalid_journal(field: impl Into<String>, reason: impl Into<String>) -> QualError {
    QualError::InvalidJournal { field: field.into(), reason: reason.into() }
}

fn bad_params(field: impl Into<String>, reason: impl Into<String>) -> QualError {
    QualError::BadParams { field: field.into(), reason: reason.into() }
}

fn internal(reason: impl Into<String>) -> QualError {
    QualError::Internal { reason: reason.into() }
}

// ------------------------------------------------------------------ core data types

/// Qualification run kind (`iq` or `oq`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    Iq,
    Oq,
}

impl RecordKind {
    /// Wire representation in `qual-record/1`.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Iq => "iq",
            RecordKind::Oq => "oq",
        }
    }

    /// Parses `"iq"` or `"oq"`.
    pub fn parse(s: &str) -> Result<Self, QualError> {
        match s {
            "iq" => Ok(RecordKind::Iq),
            "oq" => Ok(RecordKind::Oq),
            _ => Err(invalid_record("kind", "expected \"iq\" or \"oq\"")),
        }
    }

    /// Unversioned registry command name for committing this kind of run.
    pub fn command_name(self) -> &'static str {
        match self {
            RecordKind::Iq => RUN_IQ_COMMAND,
            RecordKind::Oq => RUN_OQ_COMMAND,
        }
    }

    /// Versioned command ID (`validate.run_iq@1` or `validate.run_oq@1`).
    pub fn command_id(self) -> &'static str {
        match self {
            RecordKind::Iq => RUN_IQ_ID,
            RecordKind::Oq => RUN_OQ_ID,
        }
    }

    /// Determinism class of the producing command (`N` for IQ, `D1` for OQ).
    pub fn determinism(self) -> Determinism {
        match self {
            RecordKind::Iq => Determinism::N,
            RecordKind::Oq => Determinism::D1,
        }
    }
}

impl fmt::Display for RecordKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Pass/fail verdict for a check or an overall qualification record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    /// Wire representation (`"PASS"` or `"FAIL"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
        }
    }

    /// Parses `"PASS"` or `"FAIL"`.
    pub fn parse(s: &str, field: &str) -> Result<Self, QualError> {
        match s {
            "PASS" => Ok(Verdict::Pass),
            "FAIL" => Ok(Verdict::Fail),
            _ => Err(invalid_record(field, "expected \"PASS\" or \"FAIL\"")),
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One individual check result recorded inside a [`QualificationRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub id: String,
    pub verdict: Verdict,
    pub detail: String,
}

impl CheckResult {
    /// Canonical JSON value representation.
    pub fn to_value(&self) -> CValue {
        obj([("detail", text(&self.detail)), ("id", text(&self.id)), ("verdict", text(self.verdict.as_str()))])
    }

    /// Parses a check result from a JSON object, rejecting unknown fields.
    pub fn from_value(v: &CValue, path: &str) -> Result<Self, QualError> {
        let o = expect_obj(v, path, &["id", "verdict", "detail"])?;
        let id = req_non_empty_str(o, "id", path)?;
        let verdict_s = req_str(o, "verdict", path)?;
        let verdict = Verdict::parse(verdict_s, &format!("{path}.verdict"))?;
        let detail = req_str(o, "detail", path)?.to_string();
        Ok(CheckResult { id, verdict, detail })
    }
}

/// REP-1 replay outcome for a golden journal in an OQ record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GoldenOutcome {
    Identical,
    Equivalent,
    Divergent,
    NotReplayable,
}

impl GoldenOutcome {
    /// Wire representation (`"IDENTICAL"`, `"EQUIVALENT"`, `"DIVERGENT"`, `"NOT_REPLAYABLE"`).
    pub fn as_str(self) -> &'static str {
        match self {
            GoldenOutcome::Identical => "IDENTICAL",
            GoldenOutcome::Equivalent => "EQUIVALENT",
            GoldenOutcome::Divergent => "DIVERGENT",
            GoldenOutcome::NotReplayable => "NOT_REPLAYABLE",
        }
    }

    /// Parses a REP-1 golden outcome string.
    pub fn parse(s: &str, field: &str) -> Result<Self, QualError> {
        match s {
            "IDENTICAL" => Ok(GoldenOutcome::Identical),
            "EQUIVALENT" => Ok(GoldenOutcome::Equivalent),
            "DIVERGENT" => Ok(GoldenOutcome::Divergent),
            "NOT_REPLAYABLE" => Ok(GoldenOutcome::NotReplayable),
            _ => Err(invalid_record(field, "unknown golden outcome")),
        }
    }
}

impl fmt::Display for GoldenOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One characteristic exceedance recorded for a golden replay in OQ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenExceedance {
    pub characteristic: String,
    pub delta: String,
    pub decision_changed: bool,
}

impl GoldenExceedance {
    /// Canonical JSON value representation.
    pub fn to_value(&self) -> CValue {
        obj([
            ("characteristic", text(&self.characteristic)),
            ("decision_changed", CValue::Bool(self.decision_changed)),
            ("delta", text(&self.delta)),
        ])
    }

    /// Parses one exceedance entry from a JSON object.
    pub fn from_value(v: &CValue, path: &str) -> Result<Self, QualError> {
        let o = expect_obj(v, path, &["characteristic", "delta", "decision_changed"])?;
        let characteristic = req_non_empty_str(o, "characteristic", path)?;
        let delta = req_str(o, "delta", path)?.to_string();
        let decision_changed = match o.get("decision_changed") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(invalid_record(format!("{path}.decision_changed"), "expected boolean")),
        };
        Ok(GoldenExceedance { characteristic, delta, decision_changed })
    }
}

/// One golden replay entry recorded in an OQ [`QualificationRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenResult {
    pub golden_id: String,
    pub outcome: GoldenOutcome,
    /// Optional human-readable reason codes (omitted on the wire when empty).
    pub reasons: Vec<String>,
    pub exceedances: Vec<GoldenExceedance>,
}

impl GoldenResult {
    /// Canonical JSON value representation.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert(
            "exceedances".to_string(),
            CValue::Arr(self.exceedances.iter().map(GoldenExceedance::to_value).collect()),
        );
        m.insert("golden_id".to_string(), text(&self.golden_id));
        m.insert("outcome".to_string(), text(self.outcome.as_str()));
        if !self.reasons.is_empty() {
            m.insert("reasons".to_string(), CValue::Arr(self.reasons.iter().map(|r| text(r)).collect()));
        }
        CValue::Obj(m)
    }

    /// Parses a golden replay entry from a JSON object.
    pub fn from_value(v: &CValue, path: &str) -> Result<Self, QualError> {
        let o = expect_obj(v, path, &["golden_id", "outcome", "reasons", "exceedances"])?;
        let golden_id = req_non_empty_str(o, "golden_id", path)?;
        let outcome = GoldenOutcome::parse(req_str(o, "outcome", path)?, &format!("{path}.outcome"))?;
        let reasons = match o.get("reasons") {
            None => Vec::new(),
            Some(CValue::Arr(arr)) => {
                if arr.len() > MAX_REASONS {
                    return Err(invalid_record(format!("{path}.reasons"), "too many reasons"));
                }
                let mut out = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    let CValue::Str(s) = item else {
                        return Err(invalid_record(format!("{path}.reasons[{i}]"), "expected string"));
                    };
                    out.push(s.clone());
                }
                out
            }
            Some(_) => return Err(invalid_record(format!("{path}.reasons"), "expected array")),
        };
        let exceedances = match o.get("exceedances") {
            Some(CValue::Arr(arr)) => {
                if arr.len() > MAX_EXCEEDANCES {
                    return Err(invalid_record(format!("{path}.exceedances"), "too many exceedances"));
                }
                let mut out = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    out.push(GoldenExceedance::from_value(item, &format!("{path}.exceedances[{i}]"))?);
                }
                out
            }
            _ => return Err(invalid_record(format!("{path}.exceedances"), "expected array")),
        };
        Ok(GoldenResult { golden_id, outcome, reasons, exceedances })
    }
}

/// Qualification record committed by one IQ or OQ run (`qual-record/1`, REQ-QUAL.IQOQ.RECORD-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationRecord {
    /// Must equal [`RECORD_SCHEMA`] (`"qual-record/1"`).
    pub schema: String,
    pub kind: RecordKind,
    pub product_version: String,
    pub kernel_manifest_digest: Digest,
    pub evidence_statement_digest: Digest,
    pub env_digest: Digest,
    pub checks: Vec<CheckResult>,
    pub verdict: Verdict,
    /// OQ only: SHA-256 digest of the bundled point-of-use RDL subset.
    pub rdl_subset_digest: Option<Digest>,
    /// OQ only: per-golden REP-1 replay outcomes.
    pub goldens: Option<Vec<GoldenResult>>,
}

impl QualificationRecord {
    /// Validates every `qual-record/1` structural and domain invariant.
    pub fn validate(&self) -> Result<(), QualError> {
        if self.schema != RECORD_SCHEMA {
            return Err(invalid_record("schema", format!("expected {RECORD_SCHEMA:?}")));
        }
        if self.product_version.is_empty() {
            return Err(invalid_record("product_version", "must not be empty"));
        }
        if self.checks.len() > MAX_CHECKS {
            return Err(invalid_record("checks", "exceeds maximum check count"));
        }
        for (i, check) in self.checks.iter().enumerate() {
            if check.id.is_empty() {
                return Err(invalid_record(format!("checks[{i}].id"), "must not be empty"));
            }
        }
        if self.kind == RecordKind::Iq {
            if self.rdl_subset_digest.is_some() {
                return Err(invalid_record("rdl_subset_digest", "allowed on OQ records only"));
            }
            if self.goldens.is_some() {
                return Err(invalid_record("goldens", "allowed on OQ records only"));
            }
        }
        if let Some(goldens) = &self.goldens {
            if goldens.len() > MAX_GOLDENS {
                return Err(invalid_record("goldens", "exceeds maximum golden count"));
            }
            for (i, g) in goldens.iter().enumerate() {
                if g.golden_id.is_empty() {
                    return Err(invalid_record(format!("goldens[{i}].golden_id"), "must not be empty"));
                }
                if g.reasons.len() > MAX_REASONS {
                    return Err(invalid_record(format!("goldens[{i}].reasons"), "too many reasons"));
                }
                if g.exceedances.len() > MAX_EXCEEDANCES {
                    return Err(invalid_record(format!("goldens[{i}].exceedances"), "too many exceedances"));
                }
                for (j, ex) in g.exceedances.iter().enumerate() {
                    if ex.characteristic.is_empty() {
                        return Err(invalid_record(
                            format!("goldens[{i}].exceedances[{j}].characteristic"),
                            "must not be empty",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Converts the record into a canonical `CValue` object (sorted keys by `BTreeMap`).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("checks".to_string(), CValue::Arr(self.checks.iter().map(CheckResult::to_value).collect()));
        m.insert("env_digest".to_string(), text(self.env_digest.as_str()));
        m.insert("evidence_statement_digest".to_string(), text(self.evidence_statement_digest.as_str()));
        if let Some(goldens) = &self.goldens {
            m.insert("goldens".to_string(), CValue::Arr(goldens.iter().map(GoldenResult::to_value).collect()));
        }
        m.insert("kernel_manifest_digest".to_string(), text(self.kernel_manifest_digest.as_str()));
        m.insert("kind".to_string(), text(self.kind.as_str()));
        m.insert("product_version".to_string(), text(&self.product_version));
        if let Some(subset) = &self.rdl_subset_digest {
            m.insert("rdl_subset_digest".to_string(), text(subset.as_str()));
        }
        m.insert("schema".to_string(), text(&self.schema));
        m.insert("verdict".to_string(), text(self.verdict.as_str()));
        CValue::Obj(m)
    }

    /// Serialises the validated record as RFC 8785 canonical JSON bytes.
    pub fn canonical_json(&self) -> Result<Vec<u8>, QualError> {
        self.validate()?;
        to_canonical_bytes(&self.to_value()).map_err(|_| internal("canonical JSON encoding failed"))
    }

    /// Computes SHA-256 of the record's RFC 8785 canonical JSON (REQ-QUAL.IQOQ.RECORD-002).
    pub fn result_digest(&self) -> Result<Digest, QualError> {
        let bytes = self.canonical_json()?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }

    /// Parses and validates a `QualificationRecord` from a `CValue`.
    pub fn from_value(v: &CValue) -> Result<Self, QualError> {
        let o = expect_obj(
            v,
            "record",
            &[
                "schema",
                "kind",
                "product_version",
                "kernel_manifest_digest",
                "evidence_statement_digest",
                "env_digest",
                "checks",
                "verdict",
                "rdl_subset_digest",
                "goldens",
            ],
        )?;
        let schema = req_str(o, "schema", "record")?.to_string();
        let kind = RecordKind::parse(req_str(o, "kind", "record")?)?;
        let product_version = req_non_empty_str(o, "product_version", "record")?;
        let kernel_manifest_digest = req_digest(o, "kernel_manifest_digest", "record")?;
        let evidence_statement_digest = req_digest(o, "evidence_statement_digest", "record")?;
        let env_digest = req_digest(o, "env_digest", "record")?;
        let checks = match o.get("checks") {
            Some(CValue::Arr(arr)) => {
                if arr.len() > MAX_CHECKS {
                    return Err(invalid_record("checks", "exceeds maximum check count"));
                }
                let mut out = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    out.push(CheckResult::from_value(item, &format!("checks[{i}]"))?);
                }
                out
            }
            _ => return Err(invalid_record("checks", "expected array")),
        };
        let verdict = Verdict::parse(req_str(o, "verdict", "record")?, "verdict")?;
        let rdl_subset_digest = match o.get("rdl_subset_digest") {
            None => None,
            Some(CValue::Str(s)) => {
                Some(Digest::parse(s).map_err(|_| invalid_record("rdl_subset_digest", "invalid sha256 digest"))?)
            }
            Some(_) => return Err(invalid_record("rdl_subset_digest", "expected sha256 digest string")),
        };
        let goldens = match o.get("goldens") {
            None => None,
            Some(CValue::Arr(arr)) => {
                if arr.len() > MAX_GOLDENS {
                    return Err(invalid_record("goldens", "exceeds maximum golden count"));
                }
                let mut out = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    out.push(GoldenResult::from_value(item, &format!("goldens[{i}]"))?);
                }
                Some(out)
            }
            Some(_) => return Err(invalid_record("goldens", "expected array")),
        };
        let rec = QualificationRecord {
            schema,
            kind,
            product_version,
            kernel_manifest_digest,
            evidence_statement_digest,
            env_digest,
            checks,
            verdict,
            rdl_subset_digest,
            goldens,
        };
        rec.validate()?;
        Ok(rec)
    }

    /// Parses and validates a `QualificationRecord` from bounded JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, QualError> {
        let v = parse_with_limits(bytes, MAX_RECORD_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| invalid_record("record", "not acceptable JSON"))?;
        Self::from_value(&v)
    }
}

/// Qualification status of an installed product version (REQ-QUAL.IQOQ.RECORD-004, 005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatusCode {
    /// Latest IQ and latest OQ for the installed product version both passed.
    Qualified,
    /// No IQ or OQ record exists for the installed product version, while records exist for other versions.
    UnqualifiedNewVersion,
    /// Neither qualified nor solely an upgraded version (no records, only one of IQ/OQ, or latest failed).
    NotQualified,
}

impl StatusCode {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            StatusCode::Qualified => "QUALIFIED",
            StatusCode::UnqualifiedNewVersion => "UNQUALIFIED_NEW_VERSION",
            StatusCode::NotQualified => "NOT_QUALIFIED",
        }
    }

    /// Parses one of the three status strings.
    pub fn parse(s: &str) -> Result<Self, QualError> {
        match s {
            "QUALIFIED" => Ok(StatusCode::Qualified),
            "UNQUALIFIED_NEW_VERSION" => Ok(StatusCode::UnqualifiedNewVersion),
            "NOT_QUALIFIED" => Ok(StatusCode::NotQualified),
            _ => Err(invalid_record("status", "unknown qualification status")),
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Status result returned by `validate.show_record@1` when called without an ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationStatus {
    pub status: StatusCode,
    pub product_version: String,
    pub iq_record: Option<String>,
    pub oq_record: Option<String>,
}

impl QualificationStatus {
    /// Canonical JSON value representation (`iq_record` / `oq_record` omitted when absent).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        if let Some(iq) = &self.iq_record {
            m.insert("iq_record".to_string(), text(iq));
        }
        if let Some(oq) = &self.oq_record {
            m.insert("oq_record".to_string(), text(oq));
        }
        m.insert("product_version".to_string(), text(&self.product_version));
        m.insert("status".to_string(), text(self.status.as_str()));
        CValue::Obj(m)
    }

    /// Parses a `QualificationStatus` from a `CValue`.
    pub fn from_value(v: &CValue) -> Result<Self, QualError> {
        let o = expect_obj(v, "status", &["status", "product_version", "iq_record", "oq_record"])?;
        let status = StatusCode::parse(req_str(o, "status", "status")?)?;
        let product_version = req_non_empty_str(o, "product_version", "status")?;
        let iq_record = opt_non_empty_str(o, "iq_record", "status")?;
        let oq_record = opt_non_empty_str(o, "oq_record", "status")?;
        Ok(QualificationStatus { status, product_version, iq_record, oq_record })
    }
}

// ------------------------------------------------------------------ CORE.JRN.ENTRY bridge

/// Journal header metadata required to construct a real `CORE.JRN.ENTRY` for an IQ or OQ run.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalMeta {
    pub stream: String,
    pub branch: String,
    pub logical_parent: Option<CanonDigest>,
    pub time: EntryTime,
    pub principal: PrincipalDraft,
    pub ai_assistance: AiAssistance,
    pub manifests: Manifests,
    pub args: CValue,
    pub reason: Option<String>,
}

/// Builds **exactly one** hashed `CORE.JRN.ENTRY` for an IQ or OQ run (REQ-QUAL.IQOQ.RECORD-002).
///
/// The entry's `events` payload holds `[record.to_value()]` (length 1) and its single
/// `outputs` entry carries `record.result_digest()`, which equals SHA-256 of the record's
/// RFC 8785 canonical JSON. The returned entry's `header.hash` is the immutable record ID.
pub fn build_journal_entry(
    record: &QualificationRecord,
    meta: &JournalMeta,
    seq: u64,
    prev: CanonDigest,
    salts: &mut dyn SaltSource,
) -> Result<Entry, QualError> {
    let result_digest = record.result_digest()?;
    let output_digest = CanonDigest(result_digest.to_bytes());
    let draft = EntryDraft {
        stream: meta.stream.clone(),
        branch: meta.branch.clone(),
        logical_parent: meta.logical_parent,
        time: meta.time.clone(),
        principal: meta.principal.clone(),
        ai_assistance: meta.ai_assistance,
        manifests: meta.manifests,
        reason: meta.reason.clone(),
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::Command(CommandDraft {
            id: record.kind.command_name().to_string(),
            major: 1,
            class: SideEffect::W2,
            determinism: record.kind.determinism(),
            seed: 0,
            args: meta.args.clone(),
            events: vec![record.to_value()],
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs: vec![OutputDigest {
                entity: Some(format!("qual:{}", record.kind.as_str())),
                digest: output_digest,
            }],
            algorithms: Vec::new(),
            event_sources: Vec::new(),
        }),
    };
    build_entry(&draft, seq, prev, salts).map_err(|e| invalid_journal("entry", e.to_string()))
}

/// Extracts and verifies a [`QualificationEvent`] from a `CORE.JRN.ENTRY`, returning
/// `Ok(None)` when the entry is not a `validate.run_iq@1` or `validate.run_oq@1` command.
///
/// Verifies the entry header hash, the `Field::Events` salted commitment, that the run
/// committed exactly one event (REQ-002), that the record kind matches the command ID, and
/// that the entry's output result digest equals SHA-256 of the record's canonical JSON.
pub fn extract_from_entry(entry: &Entry) -> Result<Option<QualificationEvent>, QualError> {
    entry.header.verify_hash().map_err(|e| invalid_journal("header.hash", e.to_string()))?;
    let HeaderBody::Command(cmd) = &entry.header.body else {
        return Ok(None);
    };
    let expected_kind = match (cmd.id.as_str(), cmd.major) {
        (RUN_IQ_COMMAND, RUN_IQ_MAJOR) => RecordKind::Iq,
        (RUN_OQ_COMMAND, RUN_OQ_MAJOR) => RecordKind::Oq,
        _ => return Ok(None),
    };
    verify_commitment(
        &entry.header,
        Field::Events,
        entry.payload.salts.get(&Field::Events),
        entry.payload.values.get(&Field::Events),
    )
    .map_err(|e| invalid_journal("events_commit", e.to_string()))?;

    let Some(CValue::Arr(events)) = entry.payload.values.get(&Field::Events) else {
        return Err(invalid_journal("events", "missing events array in payload"));
    };
    if events.len() != 1 {
        return Err(invalid_journal("events", "qualification run must commit exactly one event"));
    }
    let record = QualificationRecord::from_value(&events[0])?;
    if record.kind != expected_kind {
        return Err(invalid_journal("kind", "record kind does not match command ID"));
    }
    let expected_digest = record.result_digest()?;
    let Some(first_output) = cmd.outputs.first() else {
        return Err(invalid_journal("outputs", "missing result digest output"));
    };
    let recorded_digest = Digest::from_bytes(first_output.digest.0);
    if recorded_digest != expected_digest {
        return Err(invalid_journal("result_digest", "event result digest does not match canonical record SHA-256"));
    }
    Ok(Some(QualificationEvent {
        id: entry.header.hash.to_string(),
        command_id: expected_kind.command_id().to_string(),
        result_digest: recorded_digest,
        record,
    }))
}

// ------------------------------------------------------------------ RecordJournal

/// One verified qualification event in the Record Journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationEvent {
    /// Immutable record ID (entry hash `sha256:...` or fixture event ID).
    pub id: String,
    /// `validate.run_iq@1` or `validate.run_oq@1`.
    pub command_id: String,
    /// SHA-256 of the record's RFC 8785 canonical JSON.
    pub result_digest: Digest,
    /// The journaled qualification record body.
    pub record: QualificationRecord,
}

/// One event in a [`RecordJournal`], either a qualification record or an unrelated command
/// event skipped when computing qualification status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalEvent {
    Qualification(Box<QualificationEvent>),
    Other { id: String, command_id: String },
}

impl JournalEvent {
    /// The event's record ID.
    pub fn id(&self) -> &str {
        match self {
            JournalEvent::Qualification(q) => &q.id,
            JournalEvent::Other { id, .. } => id,
        }
    }

    /// The event's command ID.
    pub fn command_id(&self) -> &str {
        match self {
            JournalEvent::Qualification(q) => &q.command_id,
            JournalEvent::Other { command_id, .. } => command_id,
        }
    }
}

/// In-memory view of a Record Journal containing qualification events (and optionally other
/// events in journal order).
///
/// Read operations take `&RecordJournal` so `R0` commands such as `validate.show_record@1`
/// can never append to or mutate the Record Journal (D-031, AC-4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordJournal {
    events: Vec<JournalEvent>,
    /// Index from qualification record ID to position in `events` for O(log N) lookups.
    qual_index: BTreeMap<String, usize>,
}

impl RecordJournal {
    /// Creates an empty Record Journal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total number of events in the journal.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the journal holds zero events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// All events in journal order.
    pub fn events(&self) -> &[JournalEvent] {
        &self.events
    }

    /// Builds a [`RecordJournal`] from a slice of `CORE.JRN.ENTRY` records, verifying every
    /// qualification entry's hash, commitment, single-event rule and canonical result digest.
    pub fn from_entries(entries: &[Entry]) -> Result<Self, QualError> {
        if entries.len() > MAX_JOURNAL_EVENTS {
            return Err(invalid_journal("events", "exceeds maximum journal event count"));
        }
        let mut journal = Self::new();
        for entry in entries {
            match extract_from_entry(entry)? {
                Some(q) => journal.push_verified_qual(q)?,
                None => {
                    let command_id = match &entry.header.body {
                        HeaderBody::Command(c) => format!("{}@{}", c.id, c.major),
                        HeaderBody::SessionHeader(_) => "session_header".to_string(),
                        HeaderBody::PrincipalUpgrade(_) => "principal_upgrade".to_string(),
                        HeaderBody::HeadMove(_) => "head_move".to_string(),
                    };
                    journal.push_other(entry.header.hash.to_string(), command_id)?;
                }
            }
        }
        Ok(journal)
    }

    /// Commits one [`QualificationRecord`] as **exactly one** journal event (REQ-002).
    ///
    /// Computes the RFC 8785 canonical JSON and SHA-256 `result_digest`, derives a
    /// deterministic chain-linked event ID (`sha256:...`), and appends the event.
    pub fn commit_record(&mut self, record: QualificationRecord) -> Result<QualificationEvent, QualError> {
        if self.events.len() >= MAX_JOURNAL_EVENTS {
            return Err(invalid_journal("events", "exceeds maximum journal event count"));
        }
        let result_digest = record.result_digest()?;
        let prev_id = self
            .events
            .last()
            .map_or("sha256:0000000000000000000000000000000000000000000000000000000000000000", JournalEvent::id);
        let seq = (self.events.len() as u64).saturating_add(1);
        let command_id = record.kind.command_id().to_string();
        let id_preimage = obj([
            ("command_id", text(&command_id)),
            ("prev", text(prev_id)),
            ("result_digest", text(result_digest.as_str())),
            ("seq", CValue::U64(seq)),
        ]);
        let id_bytes = to_canonical_bytes(&id_preimage).map_err(|_| internal("event id encoding failed"))?;
        let id = Digest::from_bytes(digest_bytes(&id_bytes).0).as_str().to_string();
        let ev = QualificationEvent { id, command_id, result_digest, record };
        self.push_verified_qual(ev.clone())?;
        Ok(ev)
    }

    /// Appends an unrelated non-qualification event (useful for testing scanning over large journals).
    pub fn push_other(&mut self, id: String, command_id: String) -> Result<(), QualError> {
        if self.events.len() >= MAX_JOURNAL_EVENTS {
            return Err(invalid_journal("events", "exceeds maximum journal event count"));
        }
        if id.is_empty() {
            return Err(invalid_journal("id", "event id must not be empty"));
        }
        if command_id == RUN_IQ_ID || command_id == RUN_OQ_ID {
            return Err(invalid_journal("command_id", "qualification events must use commit_record"));
        }
        self.events.push(JournalEvent::Other { id, command_id });
        Ok(())
    }

    fn push_verified_qual(&mut self, ev: QualificationEvent) -> Result<(), QualError> {
        if ev.id.is_empty() {
            return Err(invalid_journal("id", "qualification event id must not be empty"));
        }
        if self.qual_index.contains_key(&ev.id) {
            return Err(invalid_journal("id", "duplicate qualification record id"));
        }
        let idx = self.events.len();
        self.qual_index.insert(ev.id.clone(), idx);
        self.events.push(JournalEvent::Qualification(Box::new(ev)));
        Ok(())
    }

    /// Looks up one journaled qualification event by ID, or returns `validate.record_not_found`.
    pub fn get_record(&self, id: &str) -> Result<&QualificationEvent, QualError> {
        let Some(&idx) = self.qual_index.get(id) else {
            return Err(QualError::RecordNotFound { id: id.to_string() });
        };
        match self.events.get(idx) {
            Some(JournalEvent::Qualification(ev)) => Ok(ev),
            _ => Err(QualError::RecordNotFound { id: id.to_string() }),
        }
    }

    /// Computes the qualification status for `installed_product_version` by scanning
    /// `validate.run_iq@1` and `validate.run_oq@1` events in journal order (REQ-004, REQ-005).
    ///
    /// - `QUALIFIED`: latest IQ and latest OQ for `installed_product_version` both `PASS`.
    /// - `UNQUALIFIED_NEW_VERSION`: no IQ or OQ record exists for `installed_product_version`,
    ///   and at least one IQ or OQ record exists for another product version.
    /// - `NOT_QUALIFIED`: otherwise.
    pub fn status(&self, installed_product_version: &str) -> QualificationStatus {
        let mut latest_iq: Option<(&str, Verdict)> = None;
        let mut latest_oq: Option<(&str, Verdict)> = None;
        let mut any_other_version = false;

        for ev in &self.events {
            let JournalEvent::Qualification(q) = ev else {
                continue;
            };
            if q.record.product_version == installed_product_version {
                match q.record.kind {
                    RecordKind::Iq => latest_iq = Some((q.id.as_str(), q.record.verdict)),
                    RecordKind::Oq => latest_oq = Some((q.id.as_str(), q.record.verdict)),
                }
            } else {
                any_other_version = true;
            }
        }

        let status = match (latest_iq, latest_oq) {
            (Some((_, Verdict::Pass)), Some((_, Verdict::Pass))) => StatusCode::Qualified,
            (None, None) if any_other_version => StatusCode::UnqualifiedNewVersion,
            _ => StatusCode::NotQualified,
        };

        QualificationStatus {
            status,
            product_version: installed_product_version.to_string(),
            iq_record: latest_iq.map(|(id, _)| id.to_string()),
            oq_record: latest_oq.map(|(id, _)| id.to_string()),
        }
    }

    /// Serialises the synthetic `.fid` journal as RFC 8785 canonical JSON bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, QualError> {
        let events: Vec<CValue> = self
            .events
            .iter()
            .map(|ev| match ev {
                JournalEvent::Qualification(q) => obj([
                    ("body", q.record.to_value()),
                    ("command_id", text(&q.command_id)),
                    ("id", text(&q.id)),
                    ("result_digest", text(q.result_digest.as_str())),
                ]),
                JournalEvent::Other { id, command_id } => obj([("command_id", text(command_id)), ("id", text(id))]),
            })
            .collect();
        let root = obj([("events", CValue::Arr(events)), ("format", text(JOURNAL_FORMAT))]);
        to_canonical_bytes(&root).map_err(|_| internal("canonical JSON encoding of journal failed"))
    }

    /// Parses a `.fid` qualification journal from bounded JSON bytes, verifying every
    /// qualification record and checking that each event's `result_digest` equals SHA-256 of
    /// its record's RFC 8785 canonical JSON (fail closed).
    pub fn from_json(bytes: &[u8]) -> Result<Self, QualError> {
        let v = parse_with_limits(bytes, MAX_JOURNAL_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| invalid_journal("journal", "not acceptable JSON"))?;
        let o = expect_journal_obj(&v, "journal", &["format", "events"])?;
        match o.get("format") {
            Some(CValue::Str(s)) if s == JOURNAL_FORMAT => {}
            _ => return Err(invalid_journal("format", format!("expected {JOURNAL_FORMAT:?}"))),
        }
        let Some(CValue::Arr(arr)) = o.get("events") else {
            return Err(invalid_journal("events", "expected array"));
        };
        if arr.len() > MAX_JOURNAL_EVENTS {
            return Err(invalid_journal("events", "exceeds maximum journal event count"));
        }
        let mut journal = Self::new();
        for (i, item) in arr.iter().enumerate() {
            let path = format!("events[{i}]");
            let eo = expect_journal_obj(item, &path, &["id", "command_id", "result_digest", "body"])?;
            let id = match eo.get("id") {
                Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
                _ => return Err(invalid_journal(format!("{path}.id"), "expected non-empty string")),
            };
            let command_id = match eo.get("command_id") {
                Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
                _ => return Err(invalid_journal(format!("{path}.command_id"), "expected non-empty string")),
            };
            match command_id.as_str() {
                RUN_IQ_ID | RUN_OQ_ID => {
                    let expected_kind = if command_id == RUN_IQ_ID { RecordKind::Iq } else { RecordKind::Oq };
                    let result_digest = match eo.get("result_digest") {
                        Some(CValue::Str(s)) => Digest::parse(s)
                            .map_err(|_| invalid_journal(format!("{path}.result_digest"), "invalid sha256 digest"))?,
                        _ => {
                            return Err(invalid_journal(
                                format!("{path}.result_digest"),
                                "expected sha256 digest string",
                            ))
                        }
                    };
                    let body =
                        eo.get("body").ok_or_else(|| invalid_journal(format!("{path}.body"), "missing record body"))?;
                    let record = QualificationRecord::from_value(body)?;
                    if record.kind != expected_kind {
                        return Err(invalid_journal(
                            format!("{path}.body.kind"),
                            "record kind does not match command_id",
                        ));
                    }
                    let computed = record.result_digest()?;
                    if computed != result_digest {
                        return Err(invalid_journal(
                            format!("{path}.result_digest"),
                            "result_digest does not match SHA-256 of canonical record JSON",
                        ));
                    }
                    journal.push_verified_qual(QualificationEvent { id, command_id, result_digest, record })?;
                }
                _ => {
                    journal.push_other(id, command_id)?;
                }
            }
        }
        Ok(journal)
    }

    /// Loads and verifies a `.fid` qualification journal fixture from disk with a byte cap.
    pub fn load_file(path: &Path) -> Result<Self, QualError> {
        let meta = std::fs::metadata(path).map_err(|e| QualError::Io { reason: e.to_string() })?;
        if meta.len() > MAX_JOURNAL_BYTES as u64 {
            return Err(invalid_journal("journal", "file exceeds MAX_JOURNAL_BYTES"));
        }
        let bytes = std::fs::read(path).map_err(|e| QualError::Io { reason: e.to_string() })?;
        Self::from_json(&bytes)
    }
}

// ------------------------------------------------------------------ validate.show_record@1 handler

/// Typed parameters of one `validate.show_record@1` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowRecordCall {
    pub id: Option<String>,
}

impl ShowRecordCall {
    /// Canonical parameter object (`{}` or `{"id": "<id>"}`).
    pub fn params(&self) -> CValue {
        let mut m = BTreeMap::new();
        if let Some(id) = &self.id {
            m.insert("id".to_string(), text(id));
        }
        CValue::Obj(m)
    }

    /// Parses and validates `{ id?: string }` parameters.
    pub fn from_value(v: &CValue) -> Result<Self, QualError> {
        let CValue::Obj(o) = v else {
            return Err(bad_params("params", "expected JSON object"));
        };
        if o.keys().any(|k| k != "id") {
            return Err(bad_params("params", "unknown parameter"));
        }
        let id = match o.get("id") {
            None => None,
            Some(CValue::Str(s)) if !s.is_empty() => Some(s.clone()),
            Some(CValue::Str(_)) => return Err(bad_params("id", "must not be empty")),
            Some(_) => return Err(bad_params("id", "expected string")),
        };
        Ok(ShowRecordCall { id })
    }
}

fn map_error_code(e: &QualError) -> ErrorCode {
    match e {
        QualError::RecordNotFound { .. } => ErrorCode::NotFound,
        QualError::BadParams { .. } => ErrorCode::UsageError,
        QualError::InvalidRecord { .. } | QualError::InvalidJournal { .. } => ErrorCode::ValidationError,
        QualError::Io { .. } | QualError::Internal { .. } => ErrorCode::InternalError,
    }
}

/// Builds a canonical `failed` [`Envelope`] carrying the domain error code (e.g.
/// `validate.record_not_found`) in `details.qual_error`.
pub fn failed_envelope(command: &str, e: &QualError) -> Envelope {
    let mut details = BTreeMap::new();
    details.insert("field".to_string(), text(e.field()));
    details.insert("qual_error".to_string(), text(e.code()));
    if let QualError::RecordNotFound { id } = e {
        details.insert("id".to_string(), text(id));
    }
    Envelope::failed(command, map_error_code(e), &e.to_string(), Some(CValue::Obj(details)))
}

/// The single pure `R0` / `D0` handler behind both CLI and MCP (REQ-003, REQ-004, REQ-005).
///
/// Takes `&RecordJournal` so it can never write to the Record Journal.
pub fn run(journal: &RecordJournal, installed_product_version: &str, call: &ShowRecordCall) -> Envelope {
    if installed_product_version.is_empty() {
        return failed_envelope(SHOW_RECORD_ID, &bad_params("product_version", "must not be empty"));
    }
    match &call.id {
        Some(id) => match journal.get_record(id) {
            Ok(ev) => Envelope::computed(SHOW_RECORD_ID, ev.record.to_value()),
            Err(e) => failed_envelope(SHOW_RECORD_ID, &e),
        },
        None => Envelope::computed(SHOW_RECORD_ID, journal.status(installed_product_version).to_value()),
    }
}

fn encode_envelope(env: &Envelope) -> Result<Vec<u8>, QualError> {
    env.encode().map_err(|_| internal("envelope encoding failed"))
}

fn envelope_digest(env: &Envelope) -> Result<Digest, QualError> {
    Ok(Digest::from_bytes(digest_bytes(&encode_envelope(env)?).0))
}

// ------------------------------------------------------------------ activity log (D-031)

/// Surface through which `validate.show_record@1` was invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Cli,
    Mcp,
}

impl Surface {
    /// Wire form (`"cli"` or `"mcp"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Cli => "cli",
            Surface::Mcp => "mcp",
        }
    }
}

/// One Activity Log record for an R0 `validate.show_record@1` invocation (D-031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub command: String,
    pub call: ShowRecordCall,
    pub surface: Surface,
    /// SHA-256 of the canonical [`Envelope`] bytes returned by the call.
    pub result_sha256: Digest,
}

impl ActivityEntry {
    /// `{command, params, result_sha256, surface}`.
    pub fn to_value(&self) -> CValue {
        obj([
            ("command", text(&self.command)),
            ("params", self.call.params()),
            ("result_sha256", text(self.result_sha256.as_str())),
            ("surface", text(self.surface.as_str())),
        ])
    }

    /// Canonical JSON bytes of [`ActivityEntry::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, QualError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| internal("canonical encoding of activity entry failed"))
    }

    /// Parses a record written by [`ActivityEntry::to_json`].
    pub fn from_json(bytes: &[u8]) -> Result<Self, QualError> {
        let v = parse_with_limits(bytes, MAX_ACTIVITY_BYTES, MAX_PARAMS_DEPTH)
            .map_err(|_| bad_params("entry", "not acceptable JSON"))?;
        let CValue::Obj(o) = v else {
            return Err(bad_params("entry", "expected JSON object"));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "command" | "params" | "result_sha256" | "surface")) {
            return Err(bad_params("entry", "unknown field"));
        }
        let command = match o.get("command") {
            Some(CValue::Str(s)) if s == SHOW_RECORD_ID => s.clone(),
            _ => return Err(bad_params("command", "expected validate.show_record@1")),
        };
        let call = ShowRecordCall::from_value(o.get("params").unwrap_or(&CValue::Null))?;
        let surface = match o.get("surface") {
            Some(CValue::Str(s)) if s == "cli" => Surface::Cli,
            Some(CValue::Str(s)) if s == "mcp" => Surface::Mcp,
            _ => return Err(bad_params("surface", "expected \"cli\" or \"mcp\"")),
        };
        let result_sha256 = match o.get("result_sha256") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| bad_params("result_sha256", "invalid digest"))?,
            _ => return Err(bad_params("result_sha256", "expected digest string")),
        };
        Ok(ActivityEntry { command, call, surface, result_sha256 })
    }
}

/// Executes one parsed `validate.show_record@1` call against `&RecordJournal` and appends
/// its record to the Activity Log only (never writes the Record Journal, D-031).
pub fn execute(
    journal: &RecordJournal,
    installed_product_version: &str,
    activity_log: &mut Vec<ActivityEntry>,
    call: ShowRecordCall,
    surface: Surface,
) -> Envelope {
    let env = run(journal, installed_product_version, &call);
    match envelope_digest(&env) {
        Ok(result_sha256) => {
            activity_log.push(ActivityEntry { command: SHOW_RECORD_ID.to_string(), call, surface, result_sha256 });
            env
        }
        Err(e) => failed_envelope(SHOW_RECORD_ID, &e),
    }
}

// ------------------------------------------------------------------ CLI and MCP surfaces

/// Output of one CLI call (`fid validate show-record [--id <id>] [--json]`).
#[derive(Debug, Clone, PartialEq)]
pub struct CliOutput {
    pub envelope: Envelope,
    /// With `--json`, the RFC 8785 canonical [`Envelope`] bytes; otherwise the canonical
    /// result bytes on success and empty on failure.
    pub stdout: Vec<u8>,
    /// Without `--json`, a single `error: ...` line on failure; empty with `--json`.
    pub stderr: String,
}

fn cli_output(env: Envelope, json: bool) -> CliOutput {
    let printed = if json {
        encode_envelope(&env)
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
            let stdout = if json { encode_envelope(&env).unwrap_or_default() } else { Vec::new() };
            CliOutput { envelope: env, stdout, stderr: format!("error: {e}\n") }
        }
    }
}

fn parse_cli(args: &[&str]) -> Result<(ShowRecordCall, bool), (&'static str, QualError)> {
    if args.len() > MAX_CLI_ARGS {
        return Err(("validate", bad_params("args", "too many arguments")));
    }
    let (sub, rest) = args.split_first().ok_or(("validate", bad_params("args", "missing subcommand")))?;
    if *sub != CLI_SHOW_RECORD {
        return Err(("validate", bad_params("args", "unknown subcommand")));
    }
    let mut id: Option<String> = None;
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--id" if id.is_none() => {
                let value = it.next().ok_or((SHOW_RECORD_ID, bad_params("id", "option needs a value")))?;
                if value.is_empty() {
                    return Err((SHOW_RECORD_ID, bad_params("id", "must not be empty")));
                }
                id = Some((*value).to_string());
            }
            _ => return Err((SHOW_RECORD_ID, bad_params("args", "unknown or repeated option"))),
        }
    }
    Ok((ShowRecordCall { id }, json))
}

/// Runs `fid validate <args>`: `show-record [--id <id>] [--json]`.
///
/// Logs parsed calls to `activity_log` only; never writes to `journal` (D-031, AC-4).
pub fn invoke_cli(
    journal: &RecordJournal,
    installed_product_version: &str,
    activity_log: &mut Vec<ActivityEntry>,
    args: &[&str],
) -> CliOutput {
    let json = args.contains(&"--json");
    match parse_cli(args) {
        Ok((call, json)) => {
            cli_output(execute(journal, installed_product_version, activity_log, call, Surface::Cli), json)
        }
        Err((command, e)) => {
            let id = if command == "validate" { "cli.invoke@1" } else { command };
            cli_output(failed_envelope(id, &e), json)
        }
    }
}

/// Runs MCP tool `validate_show_record` on raw JSON parameters.
///
/// Uses the exact same handler [`run`] as [`invoke_cli`], producing byte-identical canonical
/// [`Envelope`] JSON (AC-5). Logs parsed calls to `activity_log` only (D-031).
pub fn invoke_mcp(
    journal: &RecordJournal,
    installed_product_version: &str,
    activity_log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
) -> Envelope {
    if tool != MCP_SHOW_RECORD {
        return failed_envelope("mcp.invoke@1", &bad_params("tool", "unknown tool"));
    }
    let parsed = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| bad_params("params", "not acceptable JSON"))
        .and_then(|v| ShowRecordCall::from_value(&v));
    match parsed {
        Ok(call) => execute(journal, installed_product_version, activity_log, call, Surface::Mcp),
        Err(e) => failed_envelope(SHOW_RECORD_ID, &e),
    }
}

// ------------------------------------------------------------------ registry definitions

/// Registry command definitions exported by `fid-qual`: `validate.show_record@1` (R0, D0,
/// exposed on CLI and MCP).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let digest_schema = || obj([("pattern", text(Digest::PATTERN)), ("type", text("string"))]);
    let non_empty_string = || obj([("minLength", CValue::Num(1.0)), ("type", text("string"))]);
    let verdict_schema = || obj([("enum", strs(&["PASS", "FAIL"])), ("type", text("string"))]);

    let check_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("detail", obj([("type", text("string"))])),
                ("id", non_empty_string()),
                ("verdict", verdict_schema()),
            ]),
        ),
        ("required", strs(&["detail", "id", "verdict"])),
        ("type", text("object")),
    ]);

    let exceedance_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("characteristic", non_empty_string()),
                ("decision_changed", obj([("type", text("boolean"))])),
                ("delta", obj([("type", text("string"))])),
            ]),
        ),
        ("required", strs(&["characteristic", "decision_changed", "delta"])),
        ("type", text("object")),
    ]);

    let golden_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("exceedances", obj([("items", exceedance_schema), ("type", text("array"))])),
                ("golden_id", non_empty_string()),
                (
                    "outcome",
                    obj([
                        ("enum", strs(&["IDENTICAL", "EQUIVALENT", "DIVERGENT", "NOT_REPLAYABLE"])),
                        ("type", text("string")),
                    ]),
                ),
                ("reasons", obj([("items", obj([("type", text("string"))])), ("type", text("array"))])),
            ]),
        ),
        ("required", strs(&["exceedances", "golden_id", "outcome"])),
        ("type", text("object")),
    ]);

    let record_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("checks", obj([("items", check_schema), ("type", text("array"))])),
                ("env_digest", digest_schema()),
                ("evidence_statement_digest", digest_schema()),
                ("goldens", obj([("items", golden_schema), ("type", text("array"))])),
                ("kernel_manifest_digest", digest_schema()),
                ("kind", obj([("enum", strs(&["iq", "oq"])), ("type", text("string"))])),
                ("product_version", non_empty_string()),
                ("rdl_subset_digest", digest_schema()),
                ("schema", obj([("const", text(RECORD_SCHEMA)), ("type", text("string"))])),
                ("verdict", verdict_schema()),
            ]),
        ),
        (
            "required",
            strs(&[
                "checks",
                "env_digest",
                "evidence_statement_digest",
                "kernel_manifest_digest",
                "kind",
                "product_version",
                "schema",
                "verdict",
            ]),
        ),
        ("type", text("object")),
    ]);

    let status_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("iq_record", non_empty_string()),
                ("oq_record", non_empty_string()),
                ("product_version", non_empty_string()),
                (
                    "status",
                    obj([
                        ("enum", strs(&["QUALIFIED", "UNQUALIFIED_NEW_VERSION", "NOT_QUALIFIED"])),
                        ("type", text("string")),
                    ]),
                ),
            ]),
        ),
        ("required", strs(&["product_version", "status"])),
        ("type", text("object")),
    ]);

    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([(
                "id",
                obj([
                    (
                        "description",
                        text(
                            "Optional qualification record ID. When omitted, returns the qualification status \
                             for the installed product version.",
                        ),
                    ),
                    ("minLength", CValue::Num(1.0)),
                    ("type", text("string")),
                ]),
            )]),
        ),
        ("type", text("object")),
    ]);

    let result_schema = obj([("oneOf", CValue::Arr(vec![record_schema, status_schema]))]);

    let show_record = CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text(
                "Shows one journaled qualification record by ID, or the qualification status \
                 (QUALIFIED, UNQUALIFIED_NEW_VERSION or NOT_QUALIFIED) of the installed product \
                 version with the record IDs used.",
            ),
        ),
        ("determinism", text("D0")),
        ("effects", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text(SHOW_RECORD_COMMAND)),
        ("idempotency", text("natural")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("R0")),
        ("title", text("Show qualification record or status")),
        ("undo", text("not_undoable")),
        ("web", text("available")),
    ]))?;

    Ok(vec![show_record])
}

// ------------------------------------------------------------------ JSON helpers

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn expect_obj<'a>(v: &'a CValue, path: &str, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, QualError> {
    let CValue::Obj(o) = v else {
        return Err(invalid_record(path, "expected JSON object"));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid_record(format!("{path}.{k}"), "unknown field"));
        }
    }
    Ok(o)
}

fn expect_journal_obj<'a>(
    v: &'a CValue,
    path: &str,
    allowed: &[&str],
) -> Result<&'a BTreeMap<String, CValue>, QualError> {
    let CValue::Obj(o) = v else {
        return Err(invalid_journal(path, "expected JSON object"));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid_journal(format!("{path}.{k}"), "unknown field"));
        }
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, key: &str, path: &str) -> Result<&'a str, QualError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(invalid_record(format!("{path}.{key}"), "expected string")),
    }
}

fn req_non_empty_str(o: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<String, QualError> {
    let s = req_str(o, key, path)?;
    if s.is_empty() {
        return Err(invalid_record(format!("{path}.{key}"), "must not be empty"));
    }
    Ok(s.to_string())
}

fn opt_non_empty_str(o: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<Option<String>, QualError> {
    match o.get(key) {
        None => Ok(None),
        Some(CValue::Str(s)) if !s.is_empty() => Ok(Some(s.clone())),
        Some(CValue::Str(_)) => Err(invalid_record(format!("{path}.{key}"), "must not be empty")),
        Some(_) => Err(invalid_record(format!("{path}.{key}"), "expected string")),
    }
}

fn req_digest(o: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<Digest, QualError> {
    let s = req_str(o, key, path)?;
    Digest::parse(s).map_err(|_| invalid_record(format!("{path}.{key}"), "invalid sha256 digest"))
}
