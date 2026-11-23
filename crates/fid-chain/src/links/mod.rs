//! Streaming journal chain walker, anchor validation, findings and `verify.check_chain@1`
//! (spec node `TRUST.CHAIN.LINKS`).
//!
//! Walks `journal/segment.jsonl` inside a Result Package through `fid-pkg::reader`'s verified
//! streaming interface, recomputes every entry header hash via `fid-trust-canon`, verifies
//! predecessor-hash and sequence-number continuity, enforces first-entry anchoring to a
//! `session_header` or packaged checkpoint, and composes checkpoint verification
//! (`crate::ckpt`) and ResultRecord linkage (`crate::reslink`) into one [`ChainReport`].
//!
//! Memory is independent of segment length except for the compact sorted `(hash, index)`
//! vector (40 bytes per entry) and the verified metadata of entries referenced by
//! `results/records.json`. Everything is D0: no clock, no network, no `HashMap`/`HashSet`,
//! no panics, and no hashing or canonicalisation code shared with `fid-journal`/`fid-canon`
//! (REQ-TRUST.CHAIN-007).

pub mod command;

use crate::ckpt::{
    check_checkpoints, parse_checkpoint, Checkpoint, CheckpointInput, CheckpointResult, CheckpointStatus,
    SegmentHashes, CHECKPOINTS_PREFIX, ERR_MALFORMED as ERR_CKPT_MALFORMED, MAX_CHECKPOINT_BYTES,
};
use crate::reslink::{
    link_records, parse_records, parse_verified_entry_line, EntryKind, LinkResult, LinkStatus, ParsedRecord,
    ReslinkError, VerifiedEntry, VerifiedSegment, MAX_ENTRIES, MAX_LINE_BYTES, MAX_RECORDS_BYTES, RECORDS_PATH,
    SEGMENT_PATH,
};
use fid_pkg::reader::{PackageReader, ReadError};
use fid_registry::FidSchema;
use fid_sign_core::troot::{TrootError, TrustInputs};
use fid_trust_canon::{inspect_entry_line, CanonError, EntryScratch, InspectError, Limits, MAX_DEPTH};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read, Seek};

pub use command::{
    check_chain, check_chain_report, declarations, invoke_cli, invoke_mcp, ChainReportView, CheckChainParams,
    CheckpointReportView, CheckpointSigView, CheckpointStatusView, CliOutput, FindingView, LinkResultView,
    LinkStatusView, CLI_CHECK_CHAIN, CLI_GROUP, COMMAND_ID, MCP_CHECK_CHAIN,
};

/// Genesis predecessor hash (`sha256:` followed by 64 zeros), required at sequence 1.
pub const ZERO_DIGEST: Digest = Digest::from_bytes([0u8; 32]);
/// Maximum number of `journal/checkpoints/*.json` files accepted in one package.
pub const MAX_CHECKPOINT_FILES: usize = 1024;
/// Streaming read chunk size for [`walk_segment_reader`].
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Chain walk verdict (REQ-TRUST.CHAIN.LINKS-003, -004, -005).
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChainVerdict {
    #[fid(rename = "VALID")]
    Valid,
    #[fid(rename = "BROKEN")]
    Broken,
}

impl ChainVerdict {
    /// Wire string (`VALID` or `BROKEN`).
    pub fn as_str(self) -> &'static str {
        match self {
            ChainVerdict::Valid => "VALID",
            ChainVerdict::Broken => "BROKEN",
        }
    }
}

/// Classification of a chain-walk failure at an entry index.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKind {
    /// Stored `header.hash` is missing, malformed, or differs from the recomputed hash.
    #[fid(rename = "HASH_MISMATCH")]
    HashMismatch,
    /// `header.prev` differs from the preceding entry's hash (or breaks the genesis rule).
    #[fid(rename = "LINK_MISMATCH")]
    LinkMismatch,
    /// `header.seq` skips one or more sequence numbers (`seq > prev_seq + 1`).
    #[fid(rename = "SEQUENCE_GAP")]
    SequenceGap,
    /// `header.seq` repeats or goes backwards (`seq <= prev_seq`).
    #[fid(rename = "SEQUENCE_REPEAT")]
    SequenceRepeat,
    /// The last line of `journal/segment.jsonl` is incomplete or fails to parse.
    #[fid(rename = "TRUNCATED")]
    Truncated,
    /// Entry 0 is neither a `session_header` nor anchored by a matching packaged checkpoint.
    #[fid(rename = "UNANCHORED")]
    Unanchored,
}

impl FindingKind {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            FindingKind::HashMismatch => "HASH_MISMATCH",
            FindingKind::LinkMismatch => "LINK_MISMATCH",
            FindingKind::SequenceGap => "SEQUENCE_GAP",
            FindingKind::SequenceRepeat => "SEQUENCE_REPEAT",
            FindingKind::Truncated => "TRUNCATED",
            FindingKind::Unanchored => "UNANCHORED",
        }
    }
}

/// One typed finding emitted by the chain walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Zero-based line index of the failing entry in `journal/segment.jsonl`.
    pub index: u64,
    /// Classification of the failure.
    pub kind: FindingKind,
    /// Human-readable detail naming the expected and observed values.
    pub detail: String,
}

/// Result of walking `journal/segment.jsonl` alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainWalkResult {
    /// `VALID` iff every entry passed hash, anchor, link and sequence checks.
    pub verdict: ChainVerdict,
    /// Number of verified entries strictly before any failure (equals segment length when `VALID`).
    pub entries_verified: u64,
    /// Zero-based index of the first failing entry when `BROKEN`, or `None` when `VALID`.
    pub first_failing_index: Option<u64>,
    /// Findings at `first_failing_index` (empty when `VALID`).
    pub findings: Vec<Finding>,
    /// Sorted compact `(raw_sha256_bytes, zero_based_index)` vector of verified entries (40 bytes/entry).
    /// Byte order of raw SHA-256 digests matches canonical `sha256:<lower-hex>` order.
    pub entry_hashes: Vec<([u8; 32], u64)>,
    /// Recomputed hashes keyed by 1-based `seq` for checkpoint matching.
    pub checkpoint_hashes: SegmentHashes,
    /// Verified prefix metadata required for ResultRecord linkage (`RESLINK`).
    pub verified_segment: VerifiedSegment,
}

/// Composed chain verification report returned by [`check_package`] and `verify.check_chain@1`
/// (REQ-TRUST.CHAIN.LINKS-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReport {
    /// Chain walk verdict (`VALID` or `BROKEN`).
    pub verdict: ChainVerdict,
    /// Number of verified entries strictly before any failure.
    pub entries_verified: u64,
    /// Chain walk findings (empty when `verdict == ChainVerdict::Valid`).
    pub findings: Vec<Finding>,
    /// Checkpoint verification results in deterministic path order (`TRUST.CHAIN.CKPT`).
    pub checkpoints: Vec<CheckpointResult>,
    /// Per-record linkage outcomes in `results/records.json` order (`TRUST.CHAIN.RESLINK`).
    pub links: Vec<LinkResult>,
    /// Record IDs whose linkage status is `UNLINKED` (in `results/records.json` order).
    pub unlinked: Vec<String>,
    /// Record IDs whose linkage status is `MISLINKED` (in `results/records.json` order).
    pub mislinked: Vec<String>,
}

impl ChainReport {
    /// True iff the chain is `VALID`, no record is `UNLINKED` or `MISLINKED`, and no checkpoint
    /// failed head matching or parsing (signature trust is evaluated by callers / `TRUST.VERIFY.REPORT`).
    pub fn chain_and_links_ok(&self) -> bool {
        self.verdict == ChainVerdict::Valid
            && self.findings.is_empty()
            && self.unlinked.is_empty()
            && self.mislinked.is_empty()
            && self.checkpoints.iter().all(|c| c.status == CheckpointStatus::Match)
    }

    /// Converts to the schema-checked wire view.
    pub fn to_view(&self) -> ChainReportView {
        ChainReportView::from_report(self)
    }

    /// Canonical RFC 8785 JSON bytes (no trailing newline), encoded via `fid-trust-canon`.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        self.to_view().to_canonical_bytes()
    }
}

/// Errors returned by [`check_package`], [`check_reader`], and `verify.check_chain@1`.
/// Passes through `pkg.*`, `canon.*`, and `troot.malformed` codes unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckChainError {
    /// `pkg.*`: package container, limit, integrity, or IO error from `fid-pkg::reader`.
    Package(ReadError),
    /// `canon.invalid` or `canon.convention`: canonicalisation error on a non-final entry.
    Canon { index: u64, error: CanonError },
    /// `pkg.malformed` or `pkg.limit`: error while reading/parsing records or segment bounds.
    Reslink(ReslinkError),
    /// `troot.malformed`: invalid trust-root, revocation, or keyring file.
    Trust(TrootError),
    /// `params.invalid`: invalid command parameter or handle.
    Params(&'static str),
}

impl CheckChainError {
    /// Stable machine-readable error code (`pkg.*`, `canon.*`, `troot.malformed`, `params.invalid`).
    pub fn code(&self) -> &str {
        match self {
            CheckChainError::Package(e) => e.code(),
            CheckChainError::Canon { error, .. } => error.code(),
            CheckChainError::Reslink(e) => e.code(),
            CheckChainError::Trust(e) => e.code(),
            CheckChainError::Params(_) => "params.invalid",
        }
    }

    /// Stable reason detail, where the underlying error provides one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            CheckChainError::Package(e) => Some(e.reason()),
            CheckChainError::Canon { error, .. } => match error {
                CanonError::Invalid { kind, .. } => Some(kind.name()),
                CanonError::Convention { rule, .. } => Some(rule.name()),
            },
            CheckChainError::Reslink(e) => e.reason(),
            CheckChainError::Trust(e) => Some(e.reason),
            CheckChainError::Params(_) => None,
        }
    }
}

impl fmt::Display for CheckChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckChainError::Package(e) => write!(f, "{e}"),
            CheckChainError::Canon { index, error } => write!(f, "entry {index}: {error}"),
            CheckChainError::Reslink(e) => write!(f, "{e}"),
            CheckChainError::Trust(e) => write!(f, "{e}"),
            CheckChainError::Params(msg) => write!(f, "params.invalid: {msg}"),
        }
    }
}

impl std::error::Error for CheckChainError {}

impl From<ReadError> for CheckChainError {
    fn from(err: ReadError) -> Self {
        CheckChainError::Package(err)
    }
}

impl From<ReslinkError> for CheckChainError {
    fn from(err: ReslinkError) -> Self {
        CheckChainError::Reslink(err)
    }
}

impl From<TrootError> for CheckChainError {
    fn from(err: TrootError) -> Self {
        CheckChainError::Trust(err)
    }
}

// ------------------------------------------------------------------ anchor model

/// Validated checkpoint anchor candidate extracted before walking the segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorCheckpoint {
    pub seq: u64,
    pub head: Digest,
    pub stream: String,
}

impl AnchorCheckpoint {
    /// Builds an anchor candidate from a parsed [`Checkpoint`].
    pub fn from_checkpoint(ckpt: &Checkpoint) -> Self {
        AnchorCheckpoint { seq: ckpt.seq, head: ckpt.head, stream: ckpt.stream.clone() }
    }
}

// ------------------------------------------------------------------ package-level entry points

/// Opens `src` through `fid-pkg::reader::PackageReader` and runs the full chain, checkpoint,
/// and ResultRecord linkage verification.
pub fn check_package<R: Read + Seek>(src: R, trust: Option<&TrustInputs<'_>>) -> Result<ChainReport, CheckChainError> {
    let mut reader = PackageReader::open(src)?;
    check_reader(&mut reader, trust)
}

/// Runs the full chain walk, checkpoint verification, and ResultRecord linkage against an
/// already-opened [`PackageReader`].
pub fn check_reader<R: Read + Seek>(
    reader: &mut PackageReader<R>,
    trust: Option<&TrustInputs<'_>>,
) -> Result<ChainReport, CheckChainError> {
    // 1. Collect and read all `journal/checkpoints/*.json` members in sorted path order.
    let ckpt_paths: Vec<String> = reader
        .manifest()
        .members
        .iter()
        .filter(|m| {
            let p = m.path.as_str();
            p.starts_with(CHECKPOINTS_PREFIX) && p.ends_with(".json")
        })
        .map(|m| m.path.as_str().to_owned())
        .collect();

    if ckpt_paths.len() > MAX_CHECKPOINT_FILES {
        return Err(CheckChainError::Reslink(ReslinkError::Limit {
            member: "journal/checkpoints/",
            what: "checkpoint_count",
        }));
    }

    let mut raw_ckpts: Vec<(String, Option<Vec<u8>>)> = Vec::with_capacity(ckpt_paths.len());
    let mut anchors: Vec<AnchorCheckpoint> = Vec::new();
    let mut ckpt_seqs: BTreeSet<u64> = BTreeSet::new();

    for path in ckpt_paths {
        let cap = (MAX_CHECKPOINT_BYTES as u64).saturating_add(1);
        match reader.read_member(&path, cap) {
            Ok(Some(bytes)) => {
                if let Ok(parsed) = parse_checkpoint(&bytes) {
                    ckpt_seqs.insert(parsed.seq);
                    anchors.push(AnchorCheckpoint::from_checkpoint(&parsed));
                }
                raw_ckpts.push((path, Some(bytes)));
            }
            Ok(None) => {}
            Err(ReadError::Limit { reason: "buffer_cap", .. }) => {
                // Still verify the member's manifest integrity in O(1) memory, then report
                // `ckpt.malformed` for this oversized checkpoint without stopping others.
                let _ = reader.verify_member(&path)?;
                raw_ckpts.push((path, None));
            }
            Err(e) => return Err(CheckChainError::Package(e)),
        }
    }

    // 2. Read and parse `results/records.json` (if present) so the streaming walk only retains
    //    full `VerifiedEntry` metadata for hashes referenced by ResultRecords.
    let records: Vec<ParsedRecord> = match reader.read_member(RECORDS_PATH, MAX_RECORDS_BYTES)? {
        Some(bytes) => parse_records(&bytes)?,
        None => Vec::new(),
    };
    let mut needed_hashes: Vec<Digest> = records.iter().map(|r| r.journal_entry).collect();
    needed_hashes.sort_unstable();
    needed_hashes.dedup();

    // 3. Stream `journal/segment.jsonl` in a single checked pass (O(1) stream buffer).
    //    If manifest SHA-256, length, CRC-32 or ratio checks fail, `stream_member_checked`
    //    returns `Err` and `walker` is immediately dropped (fail closed).
    let mut walker = LineWalker::new(&anchors, Some(&ckpt_seqs), Some(&needed_hashes));
    let present = reader.stream_member_checked(SEGMENT_PATH, &mut |chunk| {
        walker.feed(chunk);
        Ok(())
    })?;
    if present.is_none() {
        return Err(CheckChainError::Package(ReadError::Malformed {
            reason: "missing_member",
            detail: SEGMENT_PATH.to_owned(),
        }));
    }
    let walk = walker.finish()?;

    // 4. Verify all checkpoints against verified segment hashes (preserving path order).
    let mut ckpt_inputs: Vec<CheckpointInput<'_>> = Vec::with_capacity(raw_ckpts.len());
    for (path, maybe_bytes) in &raw_ckpts {
        if let Some(bytes) = maybe_bytes {
            ckpt_inputs.push(CheckpointInput { path: path.as_str(), bytes: bytes.as_slice() });
        }
    }
    let mut checkpoints = check_checkpoints(&ckpt_inputs, &walk.checkpoint_hashes, trust);
    for (path, maybe_bytes) in &raw_ckpts {
        if maybe_bytes.is_none() {
            checkpoints.push(CheckpointResult {
                path: path.clone(),
                index: None,
                stream: None,
                head_sha256: None,
                status: CheckpointStatus::Malformed,
                error_code: Some(ERR_CKPT_MALFORMED),
                detail: Some(format!("checkpoint exceeds maximum size of {MAX_CHECKPOINT_BYTES} bytes")),
                signature: None,
                signatures: Vec::new(),
            });
        }
    }
    checkpoints.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    // 5. Link ResultRecords against the verified prefix (`index < first_failing_index`).
    let links = link_records(&records, &walk.verified_segment);
    let mut unlinked = Vec::new();
    let mut mislinked = Vec::new();
    for lr in &links {
        match lr.status {
            LinkStatus::Linked => {}
            LinkStatus::Unlinked => unlinked.push(lr.record_id.clone()),
            LinkStatus::Mislinked => mislinked.push(lr.record_id.clone()),
        }
    }

    Ok(ChainReport {
        verdict: walk.verdict,
        entries_verified: walk.entries_verified,
        findings: walk.findings,
        checkpoints,
        links,
        unlinked,
        mislinked,
    })
}

// ------------------------------------------------------------------ standalone streaming walkers

/// Streams `journal/segment.jsonl` from any [`Read`] source, verifying hashes, anchor,
/// predecessor links, and sequence continuity.
///
/// Retains all verified entries in [`ChainWalkResult::verified_segment`] and all `(seq -> hash)`
/// pairs in [`ChainWalkResult::checkpoint_hashes`]. For million-entry packages, prefer
/// [`check_package`] / [`check_reader`], which filter metadata to referenced hashes and
/// checkpoints so memory stays O(N × 40 bytes).
pub fn walk_segment_reader<R: Read>(
    mut src: R,
    anchors: &[AnchorCheckpoint],
) -> Result<ChainWalkResult, CheckChainError> {
    let mut walker = LineWalker::new(anchors, None, None);
    let mut buf = vec![0u8; READ_CHUNK_BYTES];
    loop {
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(slice) = buf.get(..n) {
                    walker.feed(slice);
                }
                if walker.has_fatal_error() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CheckChainError::Package(ReadError::Io(e.to_string()))),
        }
    }
    walker.finish()
}

/// Walks an in-memory `journal/segment.jsonl` buffer.
pub fn walk_segment_bytes(bytes: &[u8], anchors: &[AnchorCheckpoint]) -> Result<ChainWalkResult, CheckChainError> {
    walk_segment_reader(bytes, anchors)
}

// ------------------------------------------------------------------ streaming line walker

enum PendingParseError {
    Canon(CanonError),
    Malformed { reason: &'static str, detail: String },
}

impl PendingParseError {
    fn to_detail(&self) -> String {
        match self {
            PendingParseError::Canon(e) => e.to_string(),
            PendingParseError::Malformed { reason, detail } => format!("{reason}: {detail}"),
        }
    }

    fn into_fatal(self, index: u64) -> CheckChainError {
        match self {
            PendingParseError::Canon(error) => CheckChainError::Canon { index, error },
            PendingParseError::Malformed { reason, detail } => {
                CheckChainError::Reslink(ReslinkError::Malformed { member: SEGMENT_PATH, reason, detail })
            }
        }
    }
}

struct LineWalker<'a> {
    anchors: &'a [AnchorCheckpoint],
    /// If `Some`, only record checkpoint hashes for sequences in this set; if `None`, record all.
    ckpt_seqs: Option<&'a BTreeSet<u64>>,
    /// If `Some`, only extract full `VerifiedEntry` for hashes in this sorted slice; if `None`, extract all.
    needed_hashes: Option<&'a [Digest]>,

    scratch: EntryScratch,
    line_buf: Vec<u8>,
    line_index: u64,
    prev_hash: Option<Digest>,
    prev_seq: Option<u64>,
    prev_stream: Option<String>,
    entries_verified: u64,
    first_failing_index: Option<u64>,
    findings: Vec<Finding>,
    entry_hashes: Vec<([u8; 32], u64)>,
    ckpt_hashes: BTreeMap<u64, Digest>,
    matched_entries: Vec<VerifiedEntry>,

    pending_parse_error: Option<(u64, PendingParseError)>,
    fatal_error: Option<CheckChainError>,
    stopped: bool,
}

impl<'a> LineWalker<'a> {
    fn new(
        anchors: &'a [AnchorCheckpoint],
        ckpt_seqs: Option<&'a BTreeSet<u64>>,
        needed_hashes: Option<&'a [Digest]>,
    ) -> Self {
        LineWalker {
            anchors,
            ckpt_seqs,
            needed_hashes,
            scratch: EntryScratch::new(),
            line_buf: Vec::with_capacity(2048),
            line_index: 0,
            prev_hash: None,
            prev_seq: None,
            prev_stream: None,
            entries_verified: 0,
            first_failing_index: None,
            findings: Vec::new(),
            entry_hashes: Vec::new(),
            ckpt_hashes: BTreeMap::new(),
            matched_entries: Vec::new(),
            pending_parse_error: None,
            fatal_error: None,
            stopped: false,
        }
    }

    fn has_fatal_error(&self) -> bool {
        self.fatal_error.is_some()
    }

    fn feed(&mut self, mut chunk: &[u8]) {
        if self.stopped {
            return;
        }
        while !chunk.is_empty() {
            if self.stopped {
                return;
            }
            if let Some(nl_pos) = chunk.iter().position(|&b| b == b'\n') {
                let piece = &chunk[..nl_pos];
                if self.line_buf.is_empty() {
                    if piece.len() > MAX_LINE_BYTES {
                        self.fatal_error = Some(CheckChainError::Reslink(ReslinkError::Limit {
                            member: SEGMENT_PATH,
                            what: "line_bytes",
                        }));
                        self.stopped = true;
                        return;
                    }
                    self.process_line(piece, false);
                } else {
                    if self.line_buf.len().saturating_add(piece.len()) > MAX_LINE_BYTES {
                        self.fatal_error = Some(CheckChainError::Reslink(ReslinkError::Limit {
                            member: SEGMENT_PATH,
                            what: "line_bytes",
                        }));
                        self.stopped = true;
                        return;
                    }
                    self.line_buf.extend_from_slice(piece);
                    let line = std::mem::take(&mut self.line_buf);
                    self.process_line(&line, false);
                    self.line_buf = line;
                    self.line_buf.clear();
                }
                chunk = &chunk[nl_pos + 1..];
            } else {
                if self.line_buf.len().saturating_add(chunk.len()) > MAX_LINE_BYTES {
                    self.fatal_error = Some(CheckChainError::Reslink(ReslinkError::Limit {
                        member: SEGMENT_PATH,
                        what: "line_bytes",
                    }));
                    self.stopped = true;
                    return;
                }
                self.line_buf.extend_from_slice(chunk);
                break;
            }
        }
    }

    fn finish(mut self) -> Result<ChainWalkResult, CheckChainError> {
        if let Some(err) = self.fatal_error.take() {
            return Err(err);
        }

        if !self.stopped && !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.process_line(&line, true);
        }

        if let Some(err) = self.fatal_error.take() {
            return Err(err);
        }

        // If the very last line (terminated by `\n`) failed to parse and no further lines
        // followed, classify it as `TRUNCATED` (REQ-TRUST.CHAIN.LINKS-005).
        if let Some((idx, pending)) = self.pending_parse_error.take() {
            self.record_finding(
                idx,
                FindingKind::Truncated,
                format!("truncated or unparseable last line: {}", pending.to_detail()),
            );
        }

        // An empty segment has no anchor (REQ-TRUST.CHAIN.LINKS-004).
        if self.entries_verified == 0 && self.findings.is_empty() {
            self.record_finding(0, FindingKind::Unanchored, "journal segment is empty (no anchor)".to_owned());
        }

        self.entry_hashes.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        let verdict = if self.findings.is_empty() { ChainVerdict::Valid } else { ChainVerdict::Broken };
        let verified_segment = VerifiedSegment::new(self.matched_entries, self.first_failing_index);
        let checkpoint_hashes = SegmentHashes::from_map(self.ckpt_hashes);

        Ok(ChainWalkResult {
            verdict,
            entries_verified: self.entries_verified,
            first_failing_index: self.first_failing_index,
            findings: self.findings,
            entry_hashes: self.entry_hashes,
            checkpoint_hashes,
            verified_segment,
        })
    }

    fn record_finding(&mut self, index: u64, kind: FindingKind, detail: String) {
        if self.first_failing_index.is_none() {
            self.first_failing_index = Some(index);
        }
        self.findings.push(Finding { index, kind, detail });
    }

    fn handle_parse_err(&mut self, idx: u64, is_final: bool, pending: PendingParseError) {
        if is_final {
            self.record_finding(
                idx,
                FindingKind::Truncated,
                format!("truncated or unparseable last line: {}", pending.to_detail()),
            );
            self.stopped = true;
        } else {
            self.pending_parse_error = Some((idx, pending));
        }
    }

    fn process_line(&mut self, raw_line: &[u8], is_final: bool) {
        if self.stopped {
            return;
        }

        // If the previous line had a parse error and another line exists after it, the previous
        // line was NOT the last line — fail closed with its underlying `canon.*` / `pkg.malformed`.
        if let Some((prev_idx, pending)) = self.pending_parse_error.take() {
            self.fatal_error = Some(pending.into_fatal(prev_idx));
            self.stopped = true;
            return;
        }

        let idx = self.line_index;
        if idx >= MAX_ENTRIES as u64 {
            self.fatal_error =
                Some(CheckChainError::Reslink(ReslinkError::Limit { member: SEGMENT_PATH, what: "entry_count" }));
            self.stopped = true;
            return;
        }
        self.line_index = idx.saturating_add(1);

        let limits = Limits { max_bytes: MAX_LINE_BYTES, max_depth: MAX_DEPTH };
        let inspected = match inspect_entry_line(raw_line, limits, &mut self.scratch) {
            Ok(i) => i,
            Err(InspectError::Canon(err)) => {
                self.handle_parse_err(idx, is_final, PendingParseError::Canon(err));
                return;
            }
            Err(InspectError::Structure { reason, detail }) => {
                self.handle_parse_err(
                    idx,
                    is_final,
                    PendingParseError::Malformed { reason, detail: format!("line {idx}: {detail}") },
                );
                return;
            }
        };

        let recomputed = inspected.recomputed_hash;
        let recomputed_bytes = inspected.recomputed_bytes;

        // Parse required sequence number (`seq` decimal string, >= 1).
        let seq = match inspected.seq.as_deref().and_then(parse_u64_decimal) {
            Some(n) if n >= 1 => n,
            _ => {
                let reason =
                    if inspected.seq.is_none() && !inspected.seq_non_string { "missing_seq" } else { "bad_seq" };
                let detail = format!("line {idx}.header.seq must be a positive decimal u64");
                self.handle_parse_err(idx, is_final, PendingParseError::Malformed { reason, detail });
                return;
            }
        };

        // Parse required `prev` digest (fast path: exact string match against `prev_hash`).
        let stored_prev = match (self.prev_hash, inspected.stored_prev.as_deref()) {
            (Some(ph), Some(s)) if s == ph.as_str() => ph,
            (_, s_opt) => match s_opt.and_then(|s| Digest::parse(s).ok()) {
                Some(d) => d,
                None => {
                    let reason = if inspected.stored_prev.is_none() && !inspected.prev_non_string {
                        "missing_prev"
                    } else {
                        "bad_prev"
                    };
                    let detail = format!("line {idx}.header.prev must be a valid sha256 digest");
                    self.handle_parse_err(idx, is_final, PendingParseError::Malformed { reason, detail });
                    return;
                }
            },
        };

        let kind_raw = inspected.kind.as_deref();
        let kind = kind_raw.map(EntryKind::parse).unwrap_or(EntryKind::Other);
        let stream_eq_prev = match (self.prev_stream.as_deref(), inspected.stream.as_deref()) {
            (Some(p), Some(c)) => p == c,
            _ => true,
        };
        let stream_mismatch_msg = if !stream_eq_prev {
            Some(format!(
                "stream changed from {:?} to {:?}",
                self.prev_stream.as_deref().unwrap_or(""),
                inspected.stream.as_deref().unwrap_or("")
            ))
        } else {
            None
        };
        let new_stream = if self.prev_stream.is_none() { inspected.stream.as_deref().map(str::to_owned) } else { None };

        // 1. REQ-TRUST.CHAIN.LINKS-001: stored hash must equal recomputed entry hash.
        //    `recomputed.as_str()` is already canonical lowercase `sha256:<64 hex>`, so exact
        //    string equality is equivalent to `Digest::parse(s) == Ok(recomputed)`.
        if inspected.stored_hash.as_deref() != Some(recomputed.as_str()) {
            let got_desc = if let Some(s) = inspected.stored_hash.as_deref() {
                s.to_owned()
            } else if inspected.hash_non_string {
                "<non-string>".to_owned()
            } else {
                "<missing>".to_owned()
            };
            drop(inspected);
            self.record_finding(
                idx,
                FindingKind::HashMismatch,
                format!("recomputed hash {recomputed} differs from stored hash {got_desc}"),
            );
            self.stopped = true;
            return;
        }

        // Compute entry-0 anchor state while `inspected` is still alive, then release `self.scratch`.
        let unanchored_msg = if idx == 0 {
            let anchored_by_session = kind == EntryKind::SessionHeader;
            let cur_stream = inspected.stream.as_deref();
            let anchored_by_ckpt = self
                .anchors
                .iter()
                .any(|a| a.seq == seq && a.head == recomputed && cur_stream.map_or(true, |st| st == a.stream));
            if !anchored_by_session && !anchored_by_ckpt {
                Some(format!(
                    "first entry (seq {seq}, kind {:?}) is neither a session_header nor anchored by a matching checkpoint",
                    kind_raw.unwrap_or("<none>")
                ))
            } else {
                None
            }
        } else {
            None
        };
        drop(inspected);

        // 2. Entry 0 anchor and genesis checks (REQ-TRUST.CHAIN.LINKS-004).
        if idx == 0 {
            if let Some(msg) = unanchored_msg {
                self.record_finding(0, FindingKind::Unanchored, msg);
                self.stopped = true;
                return;
            }

            if seq == 1 && stored_prev != ZERO_DIGEST {
                let msg = format!("sequence 1 entry must link to zero digest, found {stored_prev}");
                self.record_finding(0, FindingKind::LinkMismatch, msg);
                self.stopped = true;
                return;
            }
            if seq > 1 && stored_prev == ZERO_DIGEST {
                let msg = format!("sequence {seq} entry must not link to zero digest");
                self.record_finding(0, FindingKind::LinkMismatch, msg);
                self.stopped = true;
                return;
            }
        } else {
            // 3. REQ-TRUST.CHAIN.LINKS-002: predecessor-hash and sequence continuity.
            let prev_h = match self.prev_hash {
                Some(h) => h,
                None => {
                    self.record_finding(idx, FindingKind::LinkMismatch, "missing predecessor hash state".to_owned());
                    self.stopped = true;
                    return;
                }
            };
            let prev_s = match self.prev_seq {
                Some(s) => s,
                None => {
                    self.record_finding(idx, FindingKind::SequenceGap, "missing predecessor sequence state".to_owned());
                    self.stopped = true;
                    return;
                }
            };

            let mut failed = false;
            if stored_prev != prev_h {
                let msg = format!("expected prev {prev_h}, found {stored_prev}");
                self.record_finding(idx, FindingKind::LinkMismatch, msg);
                failed = true;
            }
            if let Some(msg) = stream_mismatch_msg {
                if !failed {
                    self.record_finding(idx, FindingKind::LinkMismatch, msg);
                    failed = true;
                }
            }

            match prev_s.checked_add(1) {
                Some(expected) if seq == expected => {}
                Some(expected) if seq > expected => {
                    self.record_finding(idx, FindingKind::SequenceGap, format!("expected seq {expected}, found {seq}"));
                    failed = true;
                }
                Some(expected) => {
                    self.record_finding(
                        idx,
                        FindingKind::SequenceRepeat,
                        format!("expected seq {expected}, found {seq} (<= previous {prev_s})"),
                    );
                    failed = true;
                }
                None => {
                    self.record_finding(
                        idx,
                        FindingKind::SequenceGap,
                        format!("sequence overflow after {prev_s}, found {seq}"),
                    );
                    failed = true;
                }
            }

            if failed {
                self.stopped = true;
                return;
            }
        }

        // Entry passed all checks: update state and compact indexes.
        self.prev_hash = Some(recomputed);
        self.prev_seq = Some(seq);
        if self.prev_stream.is_none() {
            self.prev_stream = new_stream;
        }
        self.entries_verified = self.entries_verified.saturating_add(1);
        self.entry_hashes.push((recomputed_bytes, idx));

        if self.ckpt_seqs.map_or(true, |set| set.contains(&seq)) {
            self.ckpt_hashes.insert(seq, recomputed);
        }

        let need_metadata = match self.needed_hashes {
            Some(list) => list.binary_search(&recomputed).is_ok(),
            None => true,
        };
        if need_metadata {
            match parse_verified_entry_line(raw_line, idx) {
                Ok(entry) => self.matched_entries.push(entry),
                Err(ReslinkError::Malformed { reason: "missing_header", .. }) => {
                    // Bare-header entry line (no wrapper): cannot be a ResultRecord command entry.
                    self.matched_entries.push(VerifiedEntry {
                        hash: recomputed,
                        index: idx,
                        kind,
                        command_id: None,
                        algorithms: Vec::new(),
                        input_digests: Vec::new(),
                        output_digests: Vec::new(),
                    });
                }
                Err(err) => {
                    self.fatal_error = Some(CheckChainError::Reslink(err));
                    self.stopped = true;
                }
            }
        }
    }
}

fn parse_u64_decimal(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse::<u64>().ok()
}
