//! Independent-oracle cross-check recording and gating (spec node `UNC.RDL.GEN.XCHECK`).
//!
//! Merges an external oracle's `oracle-result/v1` file into generated tier-G records:
//! - Bounded RFC 8785 JSON parse (`MAX_ORACLE_FILE_BYTES = 16 MiB`, `MAX_ORACLE_DEPTH = 16`);
//! - Rejects any oracle whose ID equals the generator tool ID (`oracle_not_independent`);
//! - Compares every reference output in 256-bit [`Hp`] against the record's test uncertainty;
//! - Records `CrossCheck { oracle_id, oracle_version, status, max_dev_decimal }`;
//! - Fails atomically (mutating zero records) on `oracle_disagreement` or `oracle_missing`
//!   (when `require_all = true`).
//!
//! Deterministic (`D0`), pure (`R1` in-memory mutation of passed records), `BTreeMap` only,
//! zero panics.

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_rdl::{CrossOutcome, HexF64, RdlDataset};
use fid_types::Digest;

use crate::hp::{format_decimal_sig, Hp};

/// Wire schema tag for independent oracle result files.
pub const ORACLE_RESULT_SCHEMA: &str = "oracle-result/v1";
/// Built-in generator tool ID; an oracle claiming this ID is rejected as not independent.
pub const GENERATOR_TOOL_ID: &str = "fid-rdl-gen";
/// Largest accepted `oracle-result/v1` file, in bytes (16 MiB).
pub const MAX_ORACLE_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum JSON nesting depth accepted in an `oracle-result/v1` file.
pub const MAX_ORACLE_DEPTH: usize = 16;
/// Maximum number of record entries accepted in one oracle result file.
pub const MAX_ORACLE_ENTRIES: usize = 100_000;
/// Maximum number of outputs per record entry (matches `fid_rdl::record::MAX_OUTPUTS`).
pub const MAX_OUTPUTS_PER_ENTRY: usize = 256;
/// Maximum byte length for oracle IDs.
pub const MAX_ORACLE_ID_LEN: usize = 128;
/// Maximum byte length for oracle version strings and output names.
pub const MAX_NAME_LEN: usize = 64;
/// Maximum byte length for decimal strings in oracle entries.
pub const MAX_DECIMAL_LEN: usize = 256;
/// Significant decimal digits emitted in `max_dev_decimal`.
pub const MAX_DEV_SIG_DIGITS: usize = 16;

/// Cross-check status recorded on a generated dataset (REQ-UNC.RDL.GEN.XCHECK-001, -003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrossCheckStatus {
    Agreed,
    Disagreed,
    Pending,
}

impl CrossCheckStatus {
    /// Wire form (`agreed | disagreed | pending`).
    pub fn as_str(self) -> &'static str {
        match self {
            CrossCheckStatus::Agreed => "agreed",
            CrossCheckStatus::Disagreed => "disagreed",
            CrossCheckStatus::Pending => "pending",
        }
    }

    /// Parses the exact wire form.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "agreed" => Some(CrossCheckStatus::Agreed),
            "disagreed" => Some(CrossCheckStatus::Disagreed),
            "pending" => Some(CrossCheckStatus::Pending),
            _ => None,
        }
    }
}

impl fmt::Display for CrossCheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Cross-check entry written onto a generated record (REQ-UNC.RDL.GEN.XCHECK-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossCheck {
    pub oracle_id: String,
    pub oracle_version: String,
    pub status: CrossCheckStatus,
    /// Maximum absolute deviation across outputs formatted as a canonical decimal string;
    /// `None` when [`CrossCheckStatus::Pending`].
    pub max_dev_decimal: Option<String>,
}

impl CrossCheck {
    /// Converts into canonical JSON (`CValue`).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        if let Some(dev) = &self.max_dev_decimal {
            m.insert("max_dev_decimal".to_string(), CValue::Str(dev.clone()));
        }
        m.insert("oracle_id".to_string(), CValue::Str(self.oracle_id.clone()));
        m.insert("oracle_version".to_string(), CValue::Str(self.oracle_version.clone()));
        m.insert("status".to_string(), CValue::Str(self.status.as_str().to_string()));
        CValue::Obj(m)
    }

    /// Converts a completed (`Agreed` or `Disagreed`) cross-check into a `fid_rdl::CrossCheck`
    /// for embedding into an `rdl-dataset/v1` record. Returns `Ok(None)` for `Pending`.
    pub fn to_rdl_cross_check(&self) -> Result<Option<fid_rdl::CrossCheck>, XCheckError> {
        let outcome = match self.status {
            CrossCheckStatus::Agreed => CrossOutcome::Agree,
            CrossCheckStatus::Disagreed => CrossOutcome::Disagree,
            CrossCheckStatus::Pending => return Ok(None),
        };
        let dev_str = self
            .max_dev_decimal
            .as_deref()
            .ok_or_else(|| XCheckError::OracleFileInvalid { detail: "missing max_dev_decimal".to_string() })?;
        let dev_hp = Hp::parse_decimal(dev_str)
            .map_err(|e| XCheckError::OracleFileInvalid { detail: format!("invalid max_dev_decimal: {e}") })?;
        let dev_f64 = dev_hp.abs().to_f64();
        let max_deviation_hex = HexF64::from_f64(if dev_f64 == 0.0 { 0.0 } else { dev_f64 }).ok_or_else(|| {
            XCheckError::OracleFileInvalid { detail: "max deviation overflows finite binary64".to_string() }
        })?;
        Ok(Some(fid_rdl::CrossCheck { oracle: self.oracle_id.clone(), outcome, max_deviation_hex }))
    }
}

/// One reference output on a generated record subject to cross-check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOutput {
    pub name: String,
    /// Reference value as a decimal string (e.g., >= 50 significant digits).
    pub value_dec: String,
    /// Declared test uncertainty as a non-negative decimal string.
    pub test_uncertainty_dec: String,
}

/// A generated record awaiting (or holding) oracle cross-check entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XCheckRecord {
    /// Dataset ID (e.g. `rdl:G/ls.plane.svd/seed1/0001`).
    pub id: String,
    /// Canonical record digest used to match entries in `oracle-result/v1`.
    pub digest: Digest,
    /// Generator tool identifier that produced this record.
    pub generator_id: String,
    /// Reference outputs to compare against the oracle.
    pub outputs: Vec<RecordOutput>,
    /// Recorded cross-checks (appended by [`finalize`]).
    pub cross_checks: Vec<CrossCheck>,
}

impl XCheckRecord {
    /// Adapts a validated `rdl-dataset/v1` [`RdlDataset`] into an [`XCheckRecord`].
    pub fn from_rdl_dataset(ds: &RdlDataset) -> Result<Self, XCheckError> {
        let generator_id = ds.generator().map(|g| g.id.clone()).unwrap_or_else(|| GENERATOR_TOOL_ID.to_string());
        let mut outputs = Vec::with_capacity(ds.reference().outputs.len());
        for out in &ds.reference().outputs {
            let value_dec = match &out.value_dec {
                Some(d) => d.clone(),
                None => {
                    let hp = Hp::from_f64(out.value_hex.value()).map_err(|e| XCheckError::OracleFileInvalid {
                        detail: format!("record {} output {}: {e}", ds.id(), out.name),
                    })?;
                    format_decimal_sig(&hp, 17).map_err(|e| XCheckError::OracleFileInvalid {
                        detail: format!("record {} output {}: {e}", ds.id(), out.name),
                    })?
                }
            };
            let tu_hp = Hp::from_f64(out.test_uncertainty_hex.value()).map_err(|e| XCheckError::OracleFileInvalid {
                detail: format!("record {} output {} test_uncertainty: {e}", ds.id(), out.name),
            })?;
            let test_uncertainty_dec = format_decimal_sig(&tu_hp, 17).map_err(|e| XCheckError::OracleFileInvalid {
                detail: format!("record {} output {} test_uncertainty: {e}", ds.id(), out.name),
            })?;
            outputs.push(RecordOutput { name: out.name.clone(), value_dec, test_uncertainty_dec });
        }
        Ok(Self { id: ds.id().to_string(), digest: ds.digest(), generator_id, outputs, cross_checks: Vec::new() })
    }
}

/// Parsed `oracle-result/v1` file emitted by an independent external oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleResultFile {
    pub oracle_id: String,
    pub oracle_version: String,
    /// Map from record digest -> (output name -> oracle reference decimal string).
    pub entries: BTreeMap<Digest, BTreeMap<String, String>>,
}

/// Typed errors returned by oracle parsing and [`finalize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XCheckError {
    /// One or more records disagreed with the oracle beyond their test uncertainty.
    OracleDisagreement { ids: Vec<String> },
    /// One or more records had no oracle entry while `require_all = true`.
    OracleMissing { ids: Vec<String> },
    /// Oracle ID matched the generator's tool ID (REQ-UNC.RDL.GEN.XCHECK-004).
    OracleNotIndependent { oracle_id: String },
    /// Malformed, oversized, or schema-invalid `oracle-result/v1` file or record output.
    OracleFileInvalid { detail: String },
}

impl XCheckError {
    /// Stable error code matching `spec/unc/rdl/gen/xcheck/NODE.md`.
    pub fn code(&self) -> &'static str {
        match self {
            XCheckError::OracleDisagreement { .. } => "oracle_disagreement",
            XCheckError::OracleMissing { .. } => "oracle_missing",
            XCheckError::OracleNotIndependent { .. } => "oracle_not_independent",
            XCheckError::OracleFileInvalid { .. } => "oracle_file_invalid",
        }
    }
}

impl fmt::Display for XCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XCheckError::OracleDisagreement { ids } => {
                write!(f, "oracle_disagreement: [{}]", ids.join(", "))
            }
            XCheckError::OracleMissing { ids } => {
                write!(f, "oracle_missing: [{}]", ids.join(", "))
            }
            XCheckError::OracleNotIndependent { oracle_id } => {
                write!(f, "oracle_not_independent: {oracle_id}")
            }
            XCheckError::OracleFileInvalid { detail } => {
                write!(f, "oracle_file_invalid: {detail}")
            }
        }
    }
}

impl std::error::Error for XCheckError {}

// ------------------------------------------------------------------ validation helpers

fn oracle_id_ok(s: &str) -> bool {
    let mut b = s.bytes();
    !s.is_empty()
        && s.len() <= MAX_ORACLE_ID_LEN
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'))
}

fn version_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
}

fn output_name_ok(s: &str) -> bool {
    let mut b = s.bytes();
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn decimal_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_DECIMAL_LEN {
        return false;
    }
    let mut i = usize::from(b[0] == b'-');
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == start || (b[start] == b'0' && i - start > 1) {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == f {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return false;
        }
    }
    i == b.len()
}

fn invalid(detail: impl Into<String>) -> XCheckError {
    XCheckError::OracleFileInvalid { detail: detail.into() }
}

fn generator_tool_matches(oracle_id: &str, generator_id: &str) -> bool {
    if oracle_id == GENERATOR_TOOL_ID || oracle_id == generator_id {
        return true;
    }
    // Also catch scoped generator IDs like `fid-rdl-gen:lsnull@p256d50`.
    if let Some((prefix, _)) = generator_id.split_once(':') {
        if oracle_id == prefix {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------ OracleResultFile

impl OracleResultFile {
    /// Strictly parses an `oracle-result/v1` JSON document with bounded size (16 MiB)
    /// and depth (`MAX_ORACLE_DEPTH`). Duplicate keys and unknown top-level keys are refused.
    pub fn from_json(bytes: &[u8]) -> Result<Self, XCheckError> {
        if bytes.len() > MAX_ORACLE_FILE_BYTES {
            return Err(invalid(format!("file exceeds {MAX_ORACLE_FILE_BYTES} bytes")));
        }
        let val = parse_with_limits(bytes, MAX_ORACLE_FILE_BYTES, MAX_ORACLE_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => invalid("input exceeds byte or nesting depth limit"),
            CanonError::DuplicateKey(k) => invalid(format!("duplicate key: {k}")),
            _ => invalid("malformed JSON"),
        })?;
        Self::from_value(&val)
    }

    /// Validates an `oracle-result/v1` JSON [`CValue`].
    pub fn from_value(v: &CValue) -> Result<Self, XCheckError> {
        let CValue::Obj(root) = v else {
            return Err(invalid("root must be a JSON object"));
        };
        for k in root.keys() {
            if !matches!(k.as_str(), "schema" | "oracle_id" | "oracle_version" | "entries") {
                return Err(invalid(format!("unknown root key: {k}")));
            }
        }
        match root.get("schema") {
            Some(CValue::Str(s)) if s == ORACLE_RESULT_SCHEMA => {}
            _ => return Err(invalid(format!("schema must be {ORACLE_RESULT_SCHEMA}"))),
        }
        let oracle_id = match root.get("oracle_id") {
            Some(CValue::Str(s)) if oracle_id_ok(s) => s.clone(),
            _ => return Err(invalid("oracle_id must match [a-z][a-z0-9_.-]* (1..=128 bytes)")),
        };
        let oracle_version = match root.get("oracle_version") {
            Some(CValue::Str(s)) if version_ok(s) => s.clone(),
            _ => return Err(invalid("oracle_version must be 1..=64 bytes of [A-Za-z0-9.+-]")),
        };
        let CValue::Obj(raw_entries) = root.get("entries").ok_or_else(|| invalid("missing required field: entries"))?
        else {
            return Err(invalid("entries must be a JSON object keyed by record digest"));
        };
        if raw_entries.len() > MAX_ORACLE_ENTRIES {
            return Err(invalid(format!("entries count exceeds {MAX_ORACLE_ENTRIES}")));
        }

        let mut entries = BTreeMap::new();
        for (digest_str, outputs_val) in raw_entries {
            let digest =
                Digest::parse(digest_str).map_err(|_| invalid(format!("invalid record digest key: {digest_str}")))?;
            let CValue::Obj(raw_outputs) = outputs_val else {
                return Err(invalid(format!("entry for {digest_str} must be an object of output decimals")));
            };
            if raw_outputs.is_empty() || raw_outputs.len() > MAX_OUTPUTS_PER_ENTRY {
                return Err(invalid(format!("entry for {digest_str} must have 1..={MAX_OUTPUTS_PER_ENTRY} outputs")));
            }
            let mut outputs = BTreeMap::new();
            for (out_name, dec_val) in raw_outputs {
                if !output_name_ok(out_name) {
                    return Err(invalid(format!("invalid output name {out_name} in {digest_str}")));
                }
                let CValue::Str(dec_str) = dec_val else {
                    return Err(invalid(format!("output {out_name} in {digest_str} must be a decimal string")));
                };
                if !decimal_ok(dec_str) {
                    return Err(invalid(format!("output {out_name} in {digest_str} is not a valid decimal string")));
                }
                // Verify it parses cleanly into finite 256-bit Hp.
                let hp = Hp::parse_decimal(dec_str)
                    .map_err(|e| invalid(format!("output {out_name} in {digest_str} out of Hp range: {e}")))?;
                hp.checked_finite().map_err(|_| invalid(format!("output {out_name} in {digest_str} is non-finite")))?;
                outputs.insert(out_name.clone(), dec_str.clone());
            }
            entries.insert(digest, outputs);
        }

        Ok(Self { oracle_id, oracle_version, entries })
    }

    /// Converts into canonical JSON (`CValue`).
    pub fn to_value(&self) -> CValue {
        let mut entries_obj = BTreeMap::new();
        for (digest, outs) in &self.entries {
            let mut out_obj = BTreeMap::new();
            for (k, v) in outs {
                out_obj.insert(k.clone(), CValue::Str(v.clone()));
            }
            entries_obj.insert(digest.as_str().to_string(), CValue::Obj(out_obj));
        }
        let mut root = BTreeMap::new();
        root.insert("entries".to_string(), CValue::Obj(entries_obj));
        root.insert("oracle_id".to_string(), CValue::Str(self.oracle_id.clone()));
        root.insert("oracle_version".to_string(), CValue::Str(self.oracle_version.clone()));
        root.insert("schema".to_string(), CValue::Str(ORACLE_RESULT_SCHEMA.to_string()));
        CValue::Obj(root)
    }

    /// Encodes as RFC 8785 canonical JSON bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, XCheckError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| invalid("failed to encode canonical JSON"))
    }
}

// ------------------------------------------------------------------ finalize

/// Evaluates one record against `oracle`, returning the computed [`CrossCheck`] without
/// mutating the record.
fn evaluate_record(rec: &XCheckRecord, oracle: &OracleResultFile) -> Result<CrossCheck, XCheckError> {
    if rec.outputs.is_empty() {
        return Err(invalid(format!("record {} has no reference outputs", rec.id)));
    }
    let Some(oracle_outputs) = oracle.entries.get(&rec.digest) else {
        return Ok(CrossCheck {
            oracle_id: oracle.oracle_id.clone(),
            oracle_version: oracle.oracle_version.clone(),
            status: CrossCheckStatus::Pending,
            max_dev_decimal: None,
        });
    };

    if oracle_outputs.len() != rec.outputs.len() {
        return Err(invalid(format!(
            "record {} ({}) has {} outputs, oracle provided {}",
            rec.id,
            rec.digest.as_str(),
            rec.outputs.len(),
            oracle_outputs.len()
        )));
    }

    let mut max_dev = Hp::ZERO;
    let mut agreed = true;

    for out in &rec.outputs {
        let Some(oracle_dec) = oracle_outputs.get(&out.name) else {
            return Err(invalid(format!("oracle entry for {} missing output {}", rec.digest.as_str(), out.name)));
        };
        let gen_hp = Hp::parse_decimal(&out.value_dec)
            .map_err(|e| invalid(format!("record {} output {} invalid value_dec: {e}", rec.id, out.name)))?;
        let ora_hp = Hp::parse_decimal(oracle_dec)
            .map_err(|e| invalid(format!("oracle {} output {} invalid decimal: {e}", rec.digest.as_str(), out.name)))?;
        let tol_hp = Hp::parse_decimal(&out.test_uncertainty_dec)
            .map_err(|e| invalid(format!("record {} output {} invalid test_uncertainty: {e}", rec.id, out.name)))?;
        if tol_hp.is_negative() {
            return Err(invalid(format!("record {} output {} has negative test_uncertainty", rec.id, out.name)));
        }

        let diff = gen_hp
            .try_sub(ora_hp)
            .map_err(|e| invalid(format!("record {} output {} Hp subtraction overflow: {e}", rec.id, out.name)))?
            .abs();
        if diff > max_dev {
            max_dev = diff;
        }
        if diff > tol_hp {
            agreed = false;
        }
    }

    let max_dev_decimal = format_decimal_sig(&max_dev, MAX_DEV_SIG_DIGITS)
        .map_err(|e| invalid(format!("record {} cannot format max_dev_decimal: {e}", rec.id)))?;

    Ok(CrossCheck {
        oracle_id: oracle.oracle_id.clone(),
        oracle_version: oracle.oracle_version.clone(),
        status: if agreed { CrossCheckStatus::Agreed } else { CrossCheckStatus::Disagreed },
        max_dev_decimal: Some(max_dev_decimal),
    })
}

/// Cross-checks every record in `records` against `oracle` and atomically writes one
/// [`CrossCheck`] entry per record (REQ-UNC.RDL.GEN.XCHECK-001..004).
///
/// Atomic guarantee:
/// - If `oracle.oracle_id` matches [`GENERATOR_TOOL_ID`] or any record's `generator_id`,
///   fails with [`XCheckError::OracleNotIndependent`] and modifies zero records.
/// - If any record disagrees beyond its test uncertainty, fails with
///   [`XCheckError::OracleDisagreement`] naming every disagreeing record ID and modifies zero
///   records.
/// - If `require_all == true` and any record lacks an oracle entry, fails with
///   [`XCheckError::OracleMissing`] naming every missing record ID and modifies zero records.
/// - If `require_all == false` and some records lack oracle entries, records `Pending` on those
///   records and `Agreed` on matched records.
pub fn finalize(records: &mut [XCheckRecord], oracle: &OracleResultFile, require_all: bool) -> Result<(), XCheckError> {
    if oracle.oracle_id == GENERATOR_TOOL_ID
        || records.iter().any(|r| generator_tool_matches(&oracle.oracle_id, &r.generator_id))
    {
        return Err(XCheckError::OracleNotIndependent { oracle_id: oracle.oracle_id.clone() });
    }

    let mut computed = Vec::with_capacity(records.len());
    let mut disagreed_ids = Vec::new();
    let mut missing_ids = Vec::new();

    for rec in records.iter() {
        let cc = evaluate_record(rec, oracle)?;
        match cc.status {
            CrossCheckStatus::Agreed => {}
            CrossCheckStatus::Disagreed => disagreed_ids.push(rec.id.clone()),
            CrossCheckStatus::Pending => missing_ids.push(rec.id.clone()),
        }
        computed.push(cc);
    }

    if !disagreed_ids.is_empty() {
        return Err(XCheckError::OracleDisagreement { ids: disagreed_ids });
    }
    if require_all && !missing_ids.is_empty() {
        return Err(XCheckError::OracleMissing { ids: missing_ids });
    }

    for (rec, cc) in records.iter_mut().zip(computed) {
        rec.cross_checks.push(cc);
    }
    Ok(())
}

/// Convenience wrapper that parses `oracle_bytes` (16 MiB bounded) and runs [`finalize`].
pub fn finalize_from_json(
    records: &mut [XCheckRecord],
    oracle_bytes: &[u8],
    require_all: bool,
) -> Result<(), XCheckError> {
    let oracle = OracleResultFile::from_json(oracle_bytes)?;
    finalize(records, &oracle, require_all)
}

/// Atomically cross-checks validated `rdl-dataset/v1` [`RdlDataset`] records against `oracle`.
///
/// On success, appends the corresponding `fid_rdl::CrossCheck` entry into each matched
/// dataset's `cross_check` array (re-validating and recomputing its digest) and returns the
/// full per-record [`CrossCheck`] list (including `Pending` entries when `require_all = false`).
/// On any error, `datasets` is left completely unchanged.
pub fn finalize_rdl_datasets(
    datasets: &mut [RdlDataset],
    oracle: &OracleResultFile,
    require_all: bool,
) -> Result<Vec<CrossCheck>, XCheckError> {
    let mut xrecs = Vec::with_capacity(datasets.len());
    for ds in datasets.iter() {
        xrecs.push(XCheckRecord::from_rdl_dataset(ds)?);
    }
    finalize(&mut xrecs, oracle, require_all)?;

    let mut updated = Vec::with_capacity(datasets.len());
    let mut results = Vec::with_capacity(datasets.len());
    for (ds, mut xrec) in datasets.iter().zip(xrecs) {
        let Some(cc) = xrec.cross_checks.pop() else {
            return Err(invalid("internal missing cross-check result"));
        };
        if let Some(rdl_cc) = cc.to_rdl_cross_check()? {
            let mut val = ds.to_value();
            if let CValue::Obj(ref mut root) = val {
                if let Some(CValue::Arr(ref mut arr)) = root.get_mut("cross_check") {
                    let mut entry = BTreeMap::new();
                    entry.insert("max_deviation_hex".to_string(), CValue::Str(rdl_cc.max_deviation_hex.to_wire()));
                    entry.insert("oracle".to_string(), CValue::Str(rdl_cc.oracle));
                    entry.insert("outcome".to_string(), CValue::Str(rdl_cc.outcome.as_str().to_string()));
                    arr.push(CValue::Obj(entry));
                }
            }
            let rebuilt = RdlDataset::from_value(&val)
                .map_err(|e| invalid(format!("failed to rebuild RdlDataset {}: {e}", ds.id())))?;
            updated.push(rebuilt);
        } else {
            updated.push(ds.clone());
        }
        results.push(cc);
    }

    for (slot, next) in datasets.iter_mut().zip(updated) {
        *slot = next;
    }
    Ok(results)
}
