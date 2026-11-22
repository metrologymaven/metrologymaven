//! ResultRecord to journal linkage (spec node `TRUST.CHAIN.RESLINK`).
//!
//! Reads `results/records.json` via the package reader abstraction ([`PackageMemberReader`])
//! and ties each ResultRecord to the verified journal segment ([`VerifiedSegment`]):
//!
//! - **REQ-TRUST.CHAIN.RESLINK-001**: if the record's `journal_entry` hash is absent from
//!   the verified segment, report [`LinkStatus::Unlinked`] with its `record_id`.
//! - **REQ-TRUST.CHAIN.RESLINK-002**: when the chain verdict is `BROKEN`, [`VerifiedSegment`]
//!   retains only entries strictly before `first_failing_index`, so records pointing at or
//!   after the break are reported [`LinkStatus::Unlinked`].
//! - **REQ-TRUST.CHAIN.RESLINK-003**: if the record links to a verified entry that is not a
//!   result-producing command or whose outputs do not match the record, report
//!   [`LinkStatus::Mislinked`] with `record_id`, `entry_index` and [`MislinkReason`].
//!
//! Zero CORE dependencies: uses `fid-trust-canon` and `fid-types` only. Determinism D0:
//! sorted vectors (`binary_search_by`) and `BTreeSet`, no `HashMap`/`HashSet`, no panics.

use fid_trust_canon::{digest_canonical, parse_with_limits, serialize, Limits, Value, MAX_DEPTH};
use fid_types::Digest;
use std::collections::BTreeSet;
use std::fmt;

/// Package member path for ResultRecords.
pub const RECORDS_PATH: &str = "results/records.json";
/// Package member path for the journal segment.
pub const SEGMENT_PATH: &str = "journal/segment.jsonl";
/// Largest accepted `results/records.json` member, in bytes (64 MiB).
pub const MAX_RECORDS_BYTES: u64 = 64 * 1024 * 1024;
/// Largest accepted `journal/segment.jsonl` member when parsed in-memory, in bytes (256 MiB).
pub const MAX_SEGMENT_BYTES: u64 = 256 * 1024 * 1024;
/// Largest accepted single line in `journal/segment.jsonl`, in bytes (16 MiB).
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum number of ResultRecords accepted in one package.
pub const MAX_RECORDS: usize = 1_000_000;
/// Maximum number of entries accepted in one segment.
pub const MAX_ENTRIES: usize = 1_000_000;
/// Maximum number of digests in a single record or entry list.
pub const MAX_DIGESTS_PER_ITEM: usize = 4096;

/// Stable error code for malformed JSON or schema violations in package members.
pub const ERR_MALFORMED: &str = "pkg.malformed";
/// Stable error code when a member or record count exceeds a bound.
pub const ERR_LIMIT: &str = "pkg.limit";

/// Passthrough error from the package reader (`pkg.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageError {
    /// Stable error code, e.g. `pkg.digest_mismatch`, `pkg.malformed`, `pkg.limit`.
    pub code: String,
    /// Human-readable detail.
    pub detail: String,
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for PackageError {}

/// Package reader boundary used by [`link_package`] (implemented over `fid-pkg::reader` by
/// callers, keeping `fid-chain` CORE-free).
pub trait PackageMemberReader {
    /// Returns the manifest-verified bytes of `path`, or `None` if absent. Fails if the member
    /// exceeds `max_bytes` or fails integrity verification.
    fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError>;
}

/// Errors returned while reading or parsing inputs for ResultRecord linkage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReslinkError {
    /// Error passed through unchanged from the package reader (`pkg.*`).
    Package(PackageError),
    /// A package member (`results/records.json` or `journal/segment.jsonl`) is malformed.
    Malformed { member: &'static str, reason: &'static str, detail: String },
    /// A package member or collection exceeds a configured safety limit.
    Limit { member: &'static str, what: &'static str },
}

impl ReslinkError {
    /// Stable machine-readable `pkg.*` error code.
    pub fn code(&self) -> &str {
        match self {
            ReslinkError::Package(e) => &e.code,
            ReslinkError::Malformed { .. } => ERR_MALFORMED,
            ReslinkError::Limit { .. } => ERR_LIMIT,
        }
    }

    /// Stable reason tag for `pkg.malformed` and `pkg.limit`, or `None` for passthrough errors.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ReslinkError::Package(_) => None,
            ReslinkError::Malformed { reason, .. } => Some(reason),
            ReslinkError::Limit { what, .. } => Some(what),
        }
    }
}

impl fmt::Display for ReslinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReslinkError::Package(e) => write!(f, "{e}"),
            ReslinkError::Malformed { member, reason, detail } => {
                write!(f, "{ERR_MALFORMED} ({member}, {reason}): {detail}")
            }
            ReslinkError::Limit { member, what } => {
                write!(f, "{ERR_LIMIT} ({member}, {what})")
            }
        }
    }
}

impl std::error::Error for ReslinkError {}

impl From<PackageError> for ReslinkError {
    fn from(err: PackageError) -> Self {
        ReslinkError::Package(err)
    }
}

/// Linkage verdict for one ResultRecord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkStatus {
    /// Record links to a verified result-producing journal entry whose outputs match the record.
    Linked,
    /// Record's `journal_entry` hash is absent from the verified prefix of the journal segment.
    Unlinked,
    /// Record's `journal_entry` hash exists in the verified segment, but the entry does not
    /// validly produce this record (not a command, not result-producing, or output/input mismatch).
    Mislinked,
}

impl LinkStatus {
    /// Wire string (`LINKED`, `UNLINKED`, `MISLINKED`).
    pub fn as_str(self) -> &'static str {
        match self {
            LinkStatus::Linked => "LINKED",
            LinkStatus::Unlinked => "UNLINKED",
            LinkStatus::Mislinked => "MISLINKED",
        }
    }
}

/// Specific reason why a ResultRecord was classified [`LinkStatus::Mislinked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MislinkReason {
    /// Linked journal entry is not a `command` entry (e.g., `session_header` or `checkpoint`).
    NotACommand,
    /// Linked command entry recorded no output digests.
    NoOutputs,
    /// Linked command ID is not a result-producing command for this record's evaluation basis.
    NotResultProducingCommand,
    /// Linked command entry does not list the record's algorithm.
    AlgorithmMismatch,
    /// Linked command entry's `outputs` do not include the record's digest.
    OutputDigestMissing,
    /// Linked command entry's `inputs` do not match the record's `input_digests`.
    InputDigestMismatch,
}

impl MislinkReason {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            MislinkReason::NotACommand => "not_a_command",
            MislinkReason::NoOutputs => "no_outputs",
            MislinkReason::NotResultProducingCommand => "not_result_producing_command",
            MislinkReason::AlgorithmMismatch => "algorithm_mismatch",
            MislinkReason::OutputDigestMissing => "output_digest_missing",
            MislinkReason::InputDigestMismatch => "input_digest_mismatch",
        }
    }
}

/// Linkage outcome for a single ResultRecord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkResult {
    /// Stable record ID from `results/records.json`.
    pub record_id: String,
    /// Linkage status (`LINKED`, `UNLINKED`, `MISLINKED`).
    pub status: LinkStatus,
    /// Zero-based index of the linked journal entry in the segment when found (`Some` for
    /// `LINKED` and `MISLINKED`, `None` for `UNLINKED`).
    pub entry_index: Option<u64>,
    /// Detailed reason when `status == LinkStatus::Mislinked`; `None` otherwise.
    pub reason: Option<MislinkReason>,
}

/// Kind of a journal entry header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    SessionHeader,
    Command,
    Checkpoint,
    Other,
}

impl EntryKind {
    /// Parses the journal entry `header.kind` wire string.
    pub fn parse(s: &str) -> Self {
        match s {
            "session_header" => EntryKind::SessionHeader,
            "command" => EntryKind::Command,
            "checkpoint" => EntryKind::Checkpoint,
            _ => EntryKind::Other,
        }
    }
}

/// Summary of one verified journal entry produced by the chain walker (`TRUST.CHAIN.LINKS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedEntry {
    /// Recomputed / verified entry header hash (`sha256:...`).
    pub hash: Digest,
    /// Zero-based line index of the entry in `journal/segment.jsonl`.
    pub index: u64,
    /// Entry header kind (`command`, `session_header`, `checkpoint`, ...).
    pub kind: EntryKind,
    /// Command identifier (`header.command.id`) when `kind == EntryKind::Command`.
    pub command_id: Option<String>,
    /// Algorithm IDs (`header.command.algorithms`) recorded on the entry.
    pub algorithms: Vec<String>,
    /// Input digests (`header.command.inputs[*].digest`) recorded on the entry.
    pub input_digests: Vec<Digest>,
    /// Output digests (`header.command.outputs[*].digest`) recorded on the entry.
    pub output_digests: Vec<Digest>,
}

/// Verified prefix of a journal segment, indexed as a sorted vector of [`VerifiedEntry`]
/// for deterministic `O(log N)` binary search (no `HashMap`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSegment {
    /// Entries sorted by `(hash, index)`. Contains only entries with `index < first_failing_index`
    /// when `first_failing_index` is `Some` (REQ-TRUST.CHAIN.RESLINK-002).
    entries: Vec<VerifiedEntry>,
    /// Zero-based index of the first failing entry when the chain verdict is `BROKEN`, or `None`
    /// when the chain verdict is `VALID`.
    first_failing_index: Option<u64>,
}

impl VerifiedSegment {
    /// Builds a sorted [`VerifiedSegment`] from chain-walked entries.
    ///
    /// When `first_failing_index` is `Some(fail_idx)`, only entries with `entry.index < fail_idx`
    /// are retained (REQ-TRUST.CHAIN.RESLINK-002).
    pub fn new(entries: Vec<VerifiedEntry>, first_failing_index: Option<u64>) -> Self {
        let mut kept: Vec<VerifiedEntry> = match first_failing_index {
            Some(limit) => entries.into_iter().filter(|e| e.index < limit).collect(),
            None => entries,
        };
        kept.sort_by(|a, b| a.hash.cmp(&b.hash).then_with(|| a.index.cmp(&b.index)));
        VerifiedSegment { entries: kept, first_failing_index }
    }

    /// Number of verified entries indexed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no verified entries are present.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// First failing index if the underlying chain was `BROKEN`.
    pub fn first_failing_index(&self) -> Option<u64> {
        self.first_failing_index
    }

    /// Binary search for a verified entry by its `hash`.
    pub fn lookup(&self, hash: &Digest) -> Option<&VerifiedEntry> {
        let pos = self.entries.binary_search_by(|e| e.hash.cmp(hash)).ok()?;
        // If duplicate hashes ever existed, return the earliest index deterministically.
        let mut first = pos;
        while first > 0 {
            if let Some(prev) = self.entries.get(first - 1) {
                if prev.hash == *hash {
                    first -= 1;
                    continue;
                }
            }
            break;
        }
        self.entries.get(first)
    }

    /// Returns the sorted verified `(hash, index)` pairs.
    pub fn hash_index_pairs(&self) -> Vec<(Digest, u64)> {
        self.entries.iter().map(|e| (e.hash, e.index)).collect()
    }
}

/// Fields of a ResultRecord read from `results/records.json` for linkage verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecord {
    pub id: String,
    pub evaluation_basis: String,
    pub algorithm: String,
    pub journal_entry: Digest,
    pub input_digests: Vec<Digest>,
    /// Optional explicit output/record digest carried by the record (`output_digest`,
    /// `record_digest`, or `digest`).
    pub output_digest: Option<Digest>,
    /// SHA-256 of the RFC 8785 canonical JSON of the full record object.
    pub canonical_digest: Digest,
    /// SHA-256 of the RFC 8785 canonical JSON of the record object omitting `journal_entry`.
    pub digest_without_journal_entry: Digest,
}

/// Reads `results/records.json` from `pkg` and links every ResultRecord against `verified`.
///
/// If `results/records.json` is absent from the package, returns an empty `Vec`.
pub fn link_package(
    pkg: &mut impl PackageMemberReader,
    verified: &VerifiedSegment,
) -> Result<Vec<LinkResult>, ReslinkError> {
    let Some(bytes) = pkg.read_member(RECORDS_PATH, MAX_RECORDS_BYTES)? else {
        return Ok(Vec::new());
    };
    link_records_bytes(&bytes, verified)
}

/// Parses `results/records.json` bytes and links every ResultRecord against `verified`.
pub fn link_records_bytes(records_bytes: &[u8], verified: &VerifiedSegment) -> Result<Vec<LinkResult>, ReslinkError> {
    let records = parse_records(records_bytes)?;
    Ok(link_records(&records, verified))
}

/// Links parsed ResultRecords against `verified`, preserving `results/records.json` order.
pub fn link_records(records: &[ParsedRecord], verified: &VerifiedSegment) -> Vec<LinkResult> {
    let mut results = Vec::with_capacity(records.len());
    for rec in records {
        results.push(link_one(rec, verified));
    }
    results
}

fn link_one(rec: &ParsedRecord, verified: &VerifiedSegment) -> LinkResult {
    let Some(entry) = verified.lookup(&rec.journal_entry) else {
        return LinkResult { record_id: rec.id.clone(), status: LinkStatus::Unlinked, entry_index: None, reason: None };
    };

    if let Some(reason) = check_entry_matches_record(rec, entry) {
        return LinkResult {
            record_id: rec.id.clone(),
            status: LinkStatus::Mislinked,
            entry_index: Some(entry.index),
            reason: Some(reason),
        };
    }

    LinkResult { record_id: rec.id.clone(), status: LinkStatus::Linked, entry_index: Some(entry.index), reason: None }
}

/// Returns `Some(MislinkReason)` if `entry` cannot validly have produced `rec`.
fn check_entry_matches_record(rec: &ParsedRecord, entry: &VerifiedEntry) -> Option<MislinkReason> {
    if entry.kind != EntryKind::Command {
        return Some(MislinkReason::NotACommand);
    }

    let Some(cmd_id) = entry.command_id.as_deref() else {
        return Some(MislinkReason::NotResultProducingCommand);
    };
    if cmd_id.is_empty() || !is_result_producing_command(cmd_id, &rec.evaluation_basis, &entry.algorithms) {
        return Some(MislinkReason::NotResultProducingCommand);
    }

    if entry.output_digests.is_empty() {
        return Some(MislinkReason::NoOutputs);
    }

    // REQ-TRUST.CHAIN.RESLINK-003: if the record declares an explicit output/record digest,
    // the linked entry's output digests MUST contain it.
    if let Some(expected_digest) = rec.output_digest {
        if !entry.output_digests.contains(&expected_digest) {
            return Some(MislinkReason::OutputDigestMissing);
        }
    }

    // Direct cryptographic digest match: if the entry explicitly lists the canonical digest
    // of the record (with or without `journal_entry`), accept the output linkage.
    let direct_record_digest_match = entry.output_digests.contains(&rec.canonical_digest)
        || entry.output_digests.contains(&rec.digest_without_journal_entry);

    if !direct_record_digest_match {
        match rec.evaluation_basis.as_str() {
            "RESULTS_ONLY" => {
                // Imported results: every imported-result digest on the record must be among the
                // import entry's recorded output digests.
                if rec.output_digest.is_none()
                    && !rec.input_digests.is_empty()
                    && !rec.input_digests.iter().all(|d| entry.output_digests.contains(d))
                {
                    return Some(MislinkReason::OutputDigestMissing);
                }
            }
            "POINTS" | "FEATURES" => {
                // Evaluated results: the linked command must list the record's algorithm and
                // consume the record's inputs.
                if !rec.algorithm.is_empty() && !entry.algorithms.iter().any(|a| a == &rec.algorithm) {
                    return Some(MislinkReason::AlgorithmMismatch);
                }
                if !rec.input_digests.is_empty() && !rec.input_digests.iter().all(|d| entry.input_digests.contains(d)) {
                    return Some(MislinkReason::InputDigestMismatch);
                }
            }
            _ => {
                // DIRECT_READING or future evaluation basis: require either explicit output digest
                // (already checked above) or matching inputs when both sides carry them.
                if !rec.input_digests.is_empty()
                    && !entry.input_digests.is_empty()
                    && !rec.input_digests.iter().all(|d| entry.input_digests.contains(d))
                {
                    return Some(MislinkReason::InputDigestMismatch);
                }
            }
        }
    }

    None
}

/// Checks whether `command_id` is permitted to produce a ResultRecord for `evaluation_basis`.
fn is_result_producing_command(command_id: &str, evaluation_basis: &str, algorithms: &[String]) -> bool {
    // Administrative, UI, storage, verification and non-result import commands never produce
    // ResultRecords directly.
    const NON_RESULT_PREFIXES: &[&str] = &[
        "project.",
        "settings.",
        "branch.",
        "session.",
        "view.",
        "overlay.",
        "report.",
        "verify.",
        "sign.",
        "profile.",
        "job.",
        "undo.",
        "redo.",
    ];
    for prefix in NON_RESULT_PREFIXES {
        if command_id.starts_with(prefix) {
            return false;
        }
    }

    if command_id.starts_with("io.") {
        // Only result-import commands may link to RESULTS_ONLY records; geometry/point/CAD
        // import commands (e.g. `io.points.import`) never produce ResultRecords.
        return evaluation_basis == "RESULTS_ONLY"
            && (command_id.starts_with("io.results.") || command_id.starts_with("io.res."));
    }

    match evaluation_basis {
        "RESULTS_ONLY" => {
            !algorithms.is_empty()
                || command_id.starts_with("io.results.")
                || command_id.starts_with("io.res.")
                || command_id.starts_with("mig.")
        }
        "DIRECT_READING" => {
            !algorithms.is_empty()
                || command_id.starts_with("hal.")
                || command_id.starts_with("cmm.")
                || command_id.starts_with("manual.")
                || command_id.starts_with("measure.")
                || command_id.starts_with("reading.")
        }
        // POINTS / FEATURES require an algorithm-bearing evaluation/fit/construction command.
        _ => !algorithms.is_empty(),
    }
}

/// Parses `results/records.json` with bounded RFC 8259/8785 parsing via `fid-trust-canon`.
pub fn parse_records(bytes: &[u8]) -> Result<Vec<ParsedRecord>, ReslinkError> {
    if bytes.len() as u64 > MAX_RECORDS_BYTES {
        return Err(ReslinkError::Limit { member: RECORDS_PATH, what: "records_bytes" });
    }
    let val = parse_with_limits(bytes, Limits { max_bytes: MAX_RECORDS_BYTES as usize, max_depth: MAX_DEPTH })
        .map_err(|e| ReslinkError::Malformed { member: RECORDS_PATH, reason: "invalid_json", detail: e.to_string() })?;
    let Value::Array(items) = val else {
        return Err(ReslinkError::Malformed {
            member: RECORDS_PATH,
            reason: "not_an_array",
            detail: "results/records.json must be a JSON array".to_owned(),
        });
    };
    if items.len() > MAX_RECORDS {
        return Err(ReslinkError::Limit { member: RECORDS_PATH, what: "record_count" });
    }

    let mut seen_ids = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let rec = parse_one_record(item, idx)?;
        if !seen_ids.insert(rec.id.clone()) {
            return Err(ReslinkError::Malformed {
                member: RECORDS_PATH,
                reason: "duplicate_record_id",
                detail: format!("duplicate record id {:?} at index {idx}", rec.id),
            });
        }
        out.push(rec);
    }
    Ok(out)
}

fn parse_one_record(item: &Value, idx: usize) -> Result<ParsedRecord, ReslinkError> {
    let Value::Object(fields) = item else {
        return Err(ReslinkError::Malformed {
            member: RECORDS_PATH,
            reason: "record_not_object",
            detail: format!("record[{idx}] must be a JSON object"),
        });
    };

    let id = req_string(fields, "id", RECORDS_PATH, idx)?;
    if id.is_empty() {
        return Err(ReslinkError::Malformed {
            member: RECORDS_PATH,
            reason: "empty_record_id",
            detail: format!("record[{idx}].id must be non-empty"),
        });
    }
    let evaluation_basis = req_string(fields, "evaluation_basis", RECORDS_PATH, idx)?;
    let algorithm = opt_string(fields, "algorithm", RECORDS_PATH, idx)?.unwrap_or_default();
    let journal_entry_str = req_string(fields, "journal_entry", RECORDS_PATH, idx)?;
    let journal_entry = parse_digest(&journal_entry_str, RECORDS_PATH, "bad_journal_entry", idx)?;

    let mut input_digests = Vec::new();
    if let Some(v) = find_field(fields, "input_digests") {
        let Value::Array(arr) = v else {
            return Err(ReslinkError::Malformed {
                member: RECORDS_PATH,
                reason: "bad_input_digests",
                detail: format!("record[{idx}].input_digests must be an array"),
            });
        };
        if arr.len() > MAX_DIGESTS_PER_ITEM {
            return Err(ReslinkError::Limit { member: RECORDS_PATH, what: "input_digests" });
        }
        for (j, elem) in arr.iter().enumerate() {
            let Value::String(s) = elem else {
                return Err(ReslinkError::Malformed {
                    member: RECORDS_PATH,
                    reason: "bad_input_digests",
                    detail: format!("record[{idx}].input_digests[{j}] must be a string"),
                });
            };
            input_digests.push(parse_digest(s, RECORDS_PATH, "bad_input_digests", idx)?);
        }
    }

    // Check optional explicit digest field (`output_digest`, `record_digest`, or `digest`).
    let mut output_digest = None;
    for key in ["output_digest", "record_digest", "digest"] {
        if let Some(s) = opt_string(fields, key, RECORDS_PATH, idx)? {
            output_digest = Some(parse_digest(&s, RECORDS_PATH, "bad_output_digest", idx)?);
            break;
        }
    }

    let canon_full = serialize(item).map_err(|e| ReslinkError::Malformed {
        member: RECORDS_PATH,
        reason: "canonicalize_failed",
        detail: format!("record[{idx}]: {e}"),
    })?;
    let canonical_digest = digest_canonical(&canon_full);

    let without_je = Value::Object(fields.iter().filter(|(k, _)| k != "journal_entry").cloned().collect());
    let canon_without_je = serialize(&without_je).map_err(|e| ReslinkError::Malformed {
        member: RECORDS_PATH,
        reason: "canonicalize_failed",
        detail: format!("record[{idx}]: {e}"),
    })?;
    let digest_without_journal_entry = digest_canonical(&canon_without_je);

    Ok(ParsedRecord {
        id,
        evaluation_basis,
        algorithm,
        journal_entry,
        input_digests,
        output_digest,
        canonical_digest,
        digest_without_journal_entry,
    })
}

/// Parses a single JSONL line from `journal/segment.jsonl` into a [`VerifiedEntry`].
///
/// Recomputes the entry hash using `fid_trust_canon::entry_hash_value` so the returned
/// `VerifiedEntry::hash` is the verifier-computed digest (never blindly copied from the line).
pub fn parse_verified_entry_line(line: &[u8], index: u64) -> Result<VerifiedEntry, ReslinkError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ReslinkError::Limit { member: SEGMENT_PATH, what: "line_bytes" });
    }
    let val = parse_with_limits(line, Limits { max_bytes: MAX_LINE_BYTES, max_depth: MAX_DEPTH }).map_err(|e| {
        ReslinkError::Malformed { member: SEGMENT_PATH, reason: "invalid_json", detail: format!("line {index}: {e}") }
    })?;
    let Value::Object(root) = &val else {
        return Err(ReslinkError::Malformed {
            member: SEGMENT_PATH,
            reason: "entry_not_object",
            detail: format!("line {index} must be a JSON object"),
        });
    };
    let Some(header_val) = find_field(root, "header") else {
        return Err(ReslinkError::Malformed {
            member: SEGMENT_PATH,
            reason: "missing_header",
            detail: format!("line {index} missing header"),
        });
    };
    let Value::Object(header) = header_val else {
        return Err(ReslinkError::Malformed {
            member: SEGMENT_PATH,
            reason: "header_not_object",
            detail: format!("line {index}.header must be an object"),
        });
    };

    let recomputed = fid_trust_canon::entry_hash_value(header_val).map_err(|e| ReslinkError::Malformed {
        member: SEGMENT_PATH,
        reason: "bad_entry_header",
        detail: format!("line {index}: {e}"),
    })?;

    let kind_str = req_string(header, "kind", SEGMENT_PATH, index as usize)?;
    let kind = EntryKind::parse(&kind_str);

    let mut command_id = None;
    let mut algorithms = Vec::new();
    let mut input_digests = Vec::new();
    let mut output_digests = Vec::new();

    if kind == EntryKind::Command {
        let Some(Value::Object(cmd)) = find_field(header, "command") else {
            return Err(ReslinkError::Malformed {
                member: SEGMENT_PATH,
                reason: "missing_command",
                detail: format!("line {index} command entry missing command object"),
            });
        };
        command_id = Some(req_string(cmd, "id", SEGMENT_PATH, index as usize)?);

        if let Some(Value::Array(algs)) = find_field(cmd, "algorithms") {
            if algs.len() > MAX_DIGESTS_PER_ITEM {
                return Err(ReslinkError::Limit { member: SEGMENT_PATH, what: "algorithms" });
            }
            for (j, a) in algs.iter().enumerate() {
                let Value::String(s) = a else {
                    return Err(ReslinkError::Malformed {
                        member: SEGMENT_PATH,
                        reason: "bad_algorithms",
                        detail: format!("line {index}.command.algorithms[{j}] must be a string"),
                    });
                };
                algorithms.push(s.clone());
            }
        }

        if let Some(Value::Array(inps)) = find_field(cmd, "inputs") {
            if inps.len() > MAX_DIGESTS_PER_ITEM {
                return Err(ReslinkError::Limit { member: SEGMENT_PATH, what: "inputs" });
            }
            for (j, inp) in inps.iter().enumerate() {
                let Value::Object(obj) = inp else {
                    return Err(ReslinkError::Malformed {
                        member: SEGMENT_PATH,
                        reason: "bad_inputs",
                        detail: format!("line {index}.command.inputs[{j}] must be an object"),
                    });
                };
                let ds = req_string(obj, "digest", SEGMENT_PATH, index as usize)?;
                input_digests.push(parse_digest(&ds, SEGMENT_PATH, "bad_inputs", index as usize)?);
            }
        }

        if let Some(Value::Array(outs)) = find_field(cmd, "outputs") {
            if outs.len() > MAX_DIGESTS_PER_ITEM {
                return Err(ReslinkError::Limit { member: SEGMENT_PATH, what: "outputs" });
            }
            for (j, out) in outs.iter().enumerate() {
                let Value::Object(obj) = out else {
                    return Err(ReslinkError::Malformed {
                        member: SEGMENT_PATH,
                        reason: "bad_outputs",
                        detail: format!("line {index}.command.outputs[{j}] must be an object"),
                    });
                };
                let ds = req_string(obj, "digest", SEGMENT_PATH, index as usize)?;
                output_digests.push(parse_digest(&ds, SEGMENT_PATH, "bad_outputs", index as usize)?);
            }
        }
    }

    Ok(VerifiedEntry { hash: recomputed, index, kind, command_id, algorithms, input_digests, output_digests })
}

/// Parses a `journal/segment.jsonl` buffer into a [`VerifiedSegment`], truncating at
/// `first_failing_index` when the chain is `BROKEN` (REQ-TRUST.CHAIN.RESLINK-002).
///
/// Only lines strictly before `first_failing_index` are parsed and included; lines at or after
/// `first_failing_index` are ignored (since a broken segment may be tampered or truncated there).
pub fn parse_verified_segment(
    segment_bytes: &[u8],
    first_failing_index: Option<u64>,
) -> Result<VerifiedSegment, ReslinkError> {
    if segment_bytes.len() as u64 > MAX_SEGMENT_BYTES {
        return Err(ReslinkError::Limit { member: SEGMENT_PATH, what: "segment_bytes" });
    }
    let mut entries = Vec::new();
    let mut idx: u64 = 0;
    for raw_line in segment_bytes.split(|&b| b == b'\n') {
        if raw_line.is_empty() {
            continue;
        }
        if let Some(limit) = first_failing_index {
            if idx >= limit {
                break;
            }
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(ReslinkError::Limit { member: SEGMENT_PATH, what: "entry_count" });
        }
        entries.push(parse_verified_entry_line(raw_line, idx)?);
        idx = idx.saturating_add(1);
    }
    Ok(VerifiedSegment::new(entries, first_failing_index))
}

fn find_field<'a>(fields: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn req_string(
    fields: &[(String, Value)],
    name: &'static str,
    member: &'static str,
    idx: usize,
) -> Result<String, ReslinkError> {
    match find_field(fields, name) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(ReslinkError::Malformed {
            member,
            reason: "missing_or_bad_field",
            detail: format!("item[{idx}].{name} must be a string"),
        }),
    }
}

fn opt_string(
    fields: &[(String, Value)],
    name: &'static str,
    member: &'static str,
    idx: usize,
) -> Result<Option<String>, ReslinkError> {
    match find_field(fields, name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(ReslinkError::Malformed {
            member,
            reason: "bad_field_type",
            detail: format!("item[{idx}].{name} must be a string or null"),
        }),
    }
}

fn parse_digest(s: &str, member: &'static str, reason: &'static str, idx: usize) -> Result<Digest, ReslinkError> {
    Digest::parse(s).map_err(|_| ReslinkError::Malformed {
        member,
        reason,
        detail: format!("item[{idx}] invalid sha256 digest {s:?}"),
    })
}
