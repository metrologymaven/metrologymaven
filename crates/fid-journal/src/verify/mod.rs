//! Journal verification and entry listing. Spec node `CORE.JRN.CHAIN.VERIFY`.
//!
//! The storage-free half: [`verify_rows`] walks untrusted stored rows ([`StoredEntry`],
//! [`CheckpointRow`]) with a [`BlobSource`] and returns a [`VerifyReport`]; [`list_rows`]
//! filters and pages entry headers. With feature `store`, submodule `store` feeds both from a
//! project database (`Journal::verify_chain`, `Journal::list_entries`, [`verify_store`],
//! [`list_store`]) and [`command_defs`] declares `journal.verify_chain@1` and
//! `journal.list_entries@1`.
//!
//! Every byte read from the journal is untrusted. Verification is fail-closed: anything it
//! cannot prove is a located [`Failure`], never success.
//!
//! # Pinned rules
//!
//! - **Checkpoint rows first**, in row-id order (at most [`MAX_CHECKPOINTS`]). A row id below
//!   1 (append never writes one) is `checkpoint/bad_id`. The body is
//!   decoded with [`Checkpoint::from_bytes`] (canonical bytes, `linear-v1` layout): an unknown
//!   `format` is `checkpoint/unknown_format`, any other decoding failure
//!   `checkpoint/malformed`. A row of another stream is `checkpoint/foreign_stream`; a body
//!   whose stream or head differs from the row's columns is `checkpoint/column_mismatch`.
//!   These failures carry the checkpoint row id and no sequence.
//! - **Blobs verified means the stored reference set.** The `entry_blobs` rows of an entry are
//!   not committed in its header (the entry model of `CORE.JRN.ENTRY` and the append of
//!   `CORE.JRN.CHAIN.APPEND` do not hash them), so the verifier checks the blobs the rows name
//!   and the payload against the header's commitments, but cannot tell a referenced-blob row
//!   that was removed, or one that was added, from the original set.
//! - **Entries in sequence order**, position p from the first sequence verified. Per row, in
//!   this order (the first failing check is reported, at sequence p):
//!   1. column `seq` != p: `sequence/gap` (above p: an entry is missing) or
//!      `sequence/reorder` (below p);
//!   2. header over [`MAX_HEADER_BYTES`]: `hash/oversize`;
//!   3. header not strict JSON (UTF-8, no duplicate key, depth): `hash/malformed`;
//!   4. header bytes not exactly their RFC 8785 form: `hash/non_canonical`;
//!   5. not an object, or `hash` missing or not a digest: `hash/schema`;
//!   6. SHA-256 of the canonical header without `hash` != `hash`: `hash/hash_mismatch`;
//!   7. `schema_version` is not the number 1 or 2: `hash/unknown_version`;
//!   8. the entry schema ([`crate::validate_header_value`]) fails: `hash/schema`;
//!   9. `hash` column != header hash: `hash/column_mismatch`;
//!   10. header `seq` != column `seq`: `sequence/header_seq`;
//!   11. header `stream` != the verified stream: `sequence/foreign_stream`;
//!   12. `stream`, `branch` or `kind` column != header: `hash/column_mismatch`;
//!   13. `prev_hash` column != header `prev`: `link/column_mismatch`;
//!   14. header `prev` != the hash of the entry before (the zero digest at sequence 1):
//!       `link/genesis` at sequence 1, else `link/broken`;
//!   15. `entry_blobs` rows: a role other than `payload`/`ref` is `blob/unknown_role`, a
//!       digest that is not `sha256:` + 64 hex `blob/bad_ref`, no payload row
//!       `blob/payload_ref_missing`, more than one `blob/payload_ref_duplicate`;
//!   16. the payload blob: absent from the blob store counts as an entry without payload
//!       (REQ-007, not a failure); stored bytes that do not hash to the digest are
//!       `blob/corrupt`; a payload that is not canonical JSON holding exactly `salts` and
//!       the header's committed fields, each salt 32 lowercase hex digits and each
//!       commitment reproduced, is `blob/payload_mismatch`;
//!   17. referenced blobs in digest order, each digest checked once per run: absent
//!       `blob/missing`, not hashing to its digest `blob/corrupt`;
//!   18. checkpoints at this sequence (row-id order) whose head is not this entry's hash:
//!       `checkpoint/forked_head`.
//! - **After the last row**, a checkpoint above it (lowest sequence first, then row id) is
//!   `checkpoint/truncated`: the journal lost its tail. Without a checkpoint above the end a
//!   truncated tail cannot be seen from the journal alone; callers holding a trusted head
//!   pass it as [`VerifyOptions::trusted`].
//! - **Start ([`resolve_start`]), fail closed.** By default from sequence 1 with the zero
//!   `prev` ([`Anchor::Genesis`]). `from_seq` = n > 1 needs the caller's [`TrustedHead`] at
//!   exactly n - 1 ([`Anchor::Trusted`]). Stored checkpoints are not authenticated (anyone
//!   who can edit the rows can insert one), so they never anchor until TRUST.VERIFY checks
//!   their signatures. Without a trusted head at n - 1, `from_seq` 0 or above the tail plus
//!   one is `validation/bad_range`. The anchoring row n - 1 must pass steps 2 to 13 and hash
//!   to the trusted head, else the refusal is `internal/invariant_violated` located at n - 1.
//!   The report carries `from_seq` and the anchor, so a partial verification is never
//!   mistaken for a full one. Checkpoints below n are not checked or counted.
//! - **Head cache ([`check_heads`]).** After a successful walk every `heads` row must name an
//!   entry of the journal (`link/head_past_chain`) on its own branch (`link/head_branch`).
//! - **Counts** on success: entries walked, distinct referenced blobs checked, checkpoints
//!   checked, entries whose payload blob is absent.
//! - **Bounds.** [`VerifyOptions::max_entries`] rows, [`VerifyOptions::max_blobs`] distinct
//!   referenced blobs, [`MAX_CHECKPOINTS`] checkpoints and [`MAX_ENTRY_REFS`] blob rows per
//!   entry (`validation/input_limit` beyond); one header and one payload in memory at a
//!   time; no recursion on input beyond `fid-canon`'s depth-limited parser; no panics,
//!   checked arithmetic.
//! - **Errors** ([`ReadError`]) are refusals of the call, mapped to bus error catalogue v3 by
//!   [`ReadError::bus_code`]. A verification [`Failure`] is a successful call whose report
//!   says `ok: false`; a caller that must refuse on it uses [`Failure::bus_code`]
//!   (`internal/invariant_violated`: the journal is corrupt).
//!
//! # Listing
//!
//! [`list_rows`] returns headers in sequence order filtered by stream, branch, sequence range,
//! command (`name@major`) and principal kind, at most [`MAX_PAGE`] per page. Every scanned
//! row is checked like steps 2 to 12 above (not the chain links: that is verification's job);
//! a row that fails is the refusal `internal/invariant_violated`, so a tampered header is
//! never listed as a journal entry. A page scans at most [`MAX_SCAN`] rows; a page that
//! stops early carries a continuation token even with fewer than [`MAX_PAGE`] headers. The
//! token is opaque text `jl1.<filter>.<seq>`: 16 hex digits of the SHA-256 of the canonical
//! query (without the token) and the next sequence. A token that does not parse is
//! `validation/bad_token`; one minted for another query `validation/stale_token`.

#[cfg(feature = "store")]
pub(crate) mod store;

#[cfg(feature = "store")]
pub use store::{command_defs, list_store, verify_store, LIST_ENTRIES, VERIFY_CHAIN};

use crate::checkpoint::{Checkpoint, CheckpointError};
use crate::entry::{
    commitment, parse_digest, validate_header_value, Salt, FIRST_SEQ, GENESIS_PREV, SCHEMA_V1, SCHEMA_V2,
};
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, Digest, MAX_DEPTH};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Largest stored header read, in bytes (the append bound).
pub const MAX_HEADER_BYTES: usize = 1024 * 1024;
/// Largest payload blob read, in bytes (the blob store's single-blob bound, 8 MiB).
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
/// Most checkpoint rows read.
pub const MAX_CHECKPOINTS: usize = 65_536;
/// Most `entry_blobs` rows of one entry: the payload plus the append bound of 4096 refs.
pub const MAX_ENTRY_REFS: usize = 4097;
/// Default bound on entries walked.
pub const DEFAULT_MAX_ENTRIES: u64 = 100_000_000;
/// Default bound on distinct referenced blobs remembered.
pub const DEFAULT_MAX_BLOBS: usize = 1_000_000;
/// Most headers of one list page (REQ-005).
pub const MAX_PAGE: usize = 1000;
/// Most rows one list page scans.
pub const MAX_SCAN: usize = 100_000;
/// Role of the payload blob in `entry_blobs`.
pub const ROLE_PAYLOAD: &str = "payload";
/// Role of a referenced blob in `entry_blobs`.
pub const ROLE_REF: &str = "ref";
/// Prefix of a list continuation token.
const TOKEN_PREFIX: &str = "jl1.";

// ------------------------------------------------------------------ failures

/// The kind of a verification failure (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FailureKind {
    Hash,
    Link,
    Sequence,
    Blob,
    Checkpoint,
}

impl FailureKind {
    /// Every kind.
    pub const ALL: [FailureKind; 5] =
        [FailureKind::Hash, FailureKind::Link, FailureKind::Sequence, FailureKind::Blob, FailureKind::Checkpoint];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            FailureKind::Hash => "hash",
            FailureKind::Link => "link",
            FailureKind::Sequence => "sequence",
            FailureKind::Blob => "blob",
            FailureKind::Checkpoint => "checkpoint",
        }
    }
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every failure reason, by kind (module documentation, Pinned rules).
pub const REASONS: [(FailureKind, &str); 31] = [
    (FailureKind::Hash, "oversize"),
    (FailureKind::Hash, "malformed"),
    (FailureKind::Hash, "non_canonical"),
    (FailureKind::Hash, "schema"),
    (FailureKind::Hash, "hash_mismatch"),
    (FailureKind::Hash, "unknown_version"),
    (FailureKind::Hash, "column_mismatch"),
    (FailureKind::Sequence, "gap"),
    (FailureKind::Sequence, "reorder"),
    (FailureKind::Sequence, "header_seq"),
    (FailureKind::Sequence, "foreign_stream"),
    (FailureKind::Sequence, "extended"),
    (FailureKind::Link, "column_mismatch"),
    (FailureKind::Link, "genesis"),
    (FailureKind::Link, "broken"),
    (FailureKind::Link, "head_past_chain"),
    (FailureKind::Link, "head_branch"),
    (FailureKind::Blob, "unknown_role"),
    (FailureKind::Blob, "bad_ref"),
    (FailureKind::Blob, "payload_ref_missing"),
    (FailureKind::Blob, "payload_ref_duplicate"),
    (FailureKind::Blob, "corrupt"),
    (FailureKind::Blob, "payload_mismatch"),
    (FailureKind::Blob, "missing"),
    (FailureKind::Checkpoint, "bad_id"),
    (FailureKind::Checkpoint, "malformed"),
    (FailureKind::Checkpoint, "unknown_format"),
    (FailureKind::Checkpoint, "foreign_stream"),
    (FailureKind::Checkpoint, "column_mismatch"),
    (FailureKind::Checkpoint, "forked_head"),
    (FailureKind::Checkpoint, "truncated"),
];

/// The first verification failure: where and what (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The verified stream.
    pub stream: String,
    /// The sequence at which the failure was found; `None` for a checkpoint row that fails on
    /// its own (malformed, unknown format, foreign stream, column mismatch).
    pub seq: Option<u64>,
    pub kind: FailureKind,
    /// A reason from [`REASONS`].
    pub reason: &'static str,
    /// The checkpoint row id, for a `checkpoint` failure.
    pub checkpoint: Option<i64>,
    /// Human-readable detail (untrusted content is quoted, never interpreted).
    pub detail: String,
}

impl Failure {
    /// The bus error a caller that refuses on a failed verification reports: the journal is
    /// corrupt, which is neither the caller's mistake nor a passing fault
    /// (`internal/invariant_violated`, bus catalogue v3).
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        ("internal", "invariant_violated")
    }

    /// `{stream, seq, kind, reason}` (plus `checkpoint` when set); `seq` is a decimal string
    /// or `null`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("stream".to_string(), CValue::Str(self.stream.clone()));
        o.insert("seq".to_string(), self.seq.map_or(CValue::Null, |s| CValue::Str(s.to_string())));
        o.insert("kind".to_string(), CValue::Str(self.kind.as_str().to_string()));
        o.insert("reason".to_string(), CValue::Str(self.reason.to_string()));
        if let Some(id) = self.checkpoint {
            o.insert("checkpoint".to_string(), CValue::Str(id.to_string()));
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.seq {
            Some(s) => write!(f, "{}/{} at {} sequence {s}", self.kind, self.reason, self.stream)?,
            None => write!(f, "{}/{} in {}", self.kind, self.reason, self.stream)?,
        }
        if let Some(id) = self.checkpoint {
            write!(f, " (checkpoint {id})")?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ refusals

/// A refusal of a verify or list call, with its bus error (catalogue v3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The stream is not this journal's (`not_found/unknown_stream`).
    UnknownStream(String),
    /// No entry is on this branch (`not_found/unknown_branch`).
    UnknownBranch(String),
    /// A sequence bound is out of range (`validation/bad_range`).
    BadRange(String),
    /// A parameter is malformed (`validation/schema`).
    BadParam(String),
    /// The page token does not parse (`validation/bad_token`).
    BadToken,
    /// The page token belongs to another query (`validation/stale_token`).
    StaleToken,
    /// The journal is over a verification bound (`validation/input_limit`).
    InputLimit(String),
    /// The caller cancelled (`cancelled/requested`).
    Cancelled,
    /// Reading the store failed (`internal/storage`).
    Storage(String),
    /// A listed row does not verify (`internal/invariant_violated`).
    Corrupt(Failure),
}

impl ReadError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ReadError::UnknownStream(_) => "unknown_stream",
            ReadError::UnknownBranch(_) => "unknown_branch",
            ReadError::BadRange(_) => "bad_range",
            ReadError::BadParam(_) => "bad_param",
            ReadError::BadToken => "bad_token",
            ReadError::StaleToken => "stale_token",
            ReadError::InputLimit(_) => "input_limit",
            ReadError::Cancelled => "cancelled",
            ReadError::Storage(_) => "storage",
            ReadError::Corrupt(_) => "corrupt",
        }
    }

    /// The bus error `(code, reason)` of catalogue v3.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            ReadError::UnknownStream(_) => ("not_found", "unknown_stream"),
            ReadError::UnknownBranch(_) => ("not_found", "unknown_branch"),
            ReadError::BadRange(_) => ("validation", "bad_range"),
            ReadError::BadParam(_) => ("validation", "schema"),
            ReadError::BadToken => ("validation", "bad_token"),
            ReadError::StaleToken => ("validation", "stale_token"),
            ReadError::InputLimit(_) => ("validation", "input_limit"),
            ReadError::Cancelled => ("cancelled", "requested"),
            ReadError::Storage(_) => ("internal", "storage"),
            ReadError::Corrupt(_) => ("internal", "invariant_violated"),
        }
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::UnknownStream(s) => write!(f, "unknown_stream: {s:?}"),
            ReadError::UnknownBranch(b) => write!(f, "unknown_branch: {b:?}"),
            ReadError::BadRange(m) => write!(f, "bad_range: {m}"),
            ReadError::BadParam(m) => write!(f, "bad_param: {m}"),
            ReadError::InputLimit(m) => write!(f, "input_limit: {m}"),
            ReadError::Storage(m) => write!(f, "storage: {m}"),
            ReadError::Corrupt(fail) => write!(f, "corrupt: {fail}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for ReadError {}

// ------------------------------------------------------------------ inputs

/// One stored entry row as read from the journal, untrusted. Text columns that could not be
/// read as UTF-8 text are given as an empty string (which never matches a header).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredEntry {
    pub seq: i64,
    pub hash: String,
    pub stream: String,
    pub branch: String,
    pub kind: String,
    pub prev_hash: Option<String>,
    /// The stored header bytes; empty when [`StoredEntry::header_len`] is over the bound.
    pub header: Vec<u8>,
    /// The stored header length in bytes (the reader may skip loading an oversize header).
    pub header_len: u64,
    /// `(role, digest)` rows of `entry_blobs`.
    pub refs: Vec<(String, String)>,
}

/// One checkpoint row, untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckpointRow {
    pub id: i64,
    pub stream: String,
    pub entry_hash: String,
    /// The body; a reader may cut it after [`crate::checkpoint::MAX_CHECKPOINT_BYTES`] + 1 bytes.
    pub body: Vec<u8>,
}

/// State of a referenced blob in the blob store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobState {
    /// Stored and hashing to its digest (for a chunked object: manifest and every chunk).
    Sound,
    /// Not stored (or a chunk of it is not).
    Missing,
    /// Stored but not hashing to its digest.
    Corrupt,
}

/// The blob store as the verifier reads it.
pub trait BlobSource {
    /// Checks a referenced blob.
    fn check(&mut self, digest: &Digest) -> Result<BlobState, ReadError>;
    /// The stored bytes of a payload blob, at most [`MAX_PAYLOAD_BYTES`] + 1 of them;
    /// `None` if it is not stored.
    fn payload(&mut self, digest: &Digest) -> Result<Option<Vec<u8>>, ReadError>;
}

/// A trusted head the journal must reach (for example a signed checkpoint held outside the
/// journal): the entry at `seq` must have hash `head`; with `exact`, the journal must also
/// end there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedHead {
    pub seq: u64,
    pub head: Digest,
    pub exact: bool,
}

/// Parameters and bounds of a verification.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// The stream to verify; `None` for the journal's own.
    pub stream: Option<String>,
    /// First sequence to verify (default 1).
    pub from_seq: Option<u64>,
    /// A trusted head checked like a checkpoint (`checkpoint/forked_head`,
    /// `checkpoint/truncated`; with `exact`, entries after it are `sequence/extended`).
    pub trusted: Option<TrustedHead>,
    pub max_entries: u64,
    pub max_blobs: usize,
    /// Set to stop the walk with [`ReadError::Cancelled`].
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        VerifyOptions {
            stream: None,
            from_seq: None,
            trusted: None,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_blobs: DEFAULT_MAX_BLOBS,
            cancel: None,
        }
    }
}

/// What the first verified entry links to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Sequence 1 and the zero digest: a full verification.
    Genesis,
    /// The caller's [`TrustedHead`] at `seq`.
    Trusted { seq: u64 },
}

impl Anchor {
    /// `{kind, seq?, checkpoint?}` with decimal strings.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        let kind = match self {
            Anchor::Genesis => "genesis",
            Anchor::Trusted { seq } => {
                o.insert("seq".to_string(), CValue::Str(seq.to_string()));
                "trusted"
            }
        };
        o.insert("kind".to_string(), CValue::Str(kind.to_string()));
        CValue::Obj(o)
    }
}

/// Where the walk starts: the first sequence, the hash it must link to and why that hash is
/// trusted. Build one with [`resolve_start`] (or [`Start::GENESIS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub seq: u64,
    pub prev: Digest,
    pub anchor: Anchor,
}

impl Start {
    /// Sequence 1, linking to the zero digest.
    pub const GENESIS: Start = Start { seq: FIRST_SEQ, prev: GENESIS_PREV, anchor: Anchor::Genesis };
}

/// The start of a verification from `from_seq` over a journal whose tail is `tail` (module
/// documentation, Start). `below` is the stored row of entry `from_seq - 1`. A partial
/// verification needs the caller's `trusted` head at exactly `from_seq - 1`; anything else is
/// `validation/bad_range`. Stored checkpoints are not authenticated (TRUST.VERIFY is not
/// built) and never anchor. The anchoring row itself must pass the row checks and hash to the
/// trusted head; if not, the refusal is [`ReadError::Corrupt`] located at `from_seq - 1`.
pub fn resolve_start(
    stream: &str,
    from_seq: u64,
    tail: u64,
    trusted: Option<TrustedHead>,
    below: Option<&StoredEntry>,
) -> Result<Start, ReadError> {
    if from_seq == 0 || from_seq > tail.saturating_add(1) {
        return Err(ReadError::BadRange(format!("from_seq {from_seq}, journal tail {tail}")));
    }
    if from_seq == FIRST_SEQ {
        return Ok(Start::GENESIS);
    }
    let at = from_seq - 1;
    let Some(t) = trusted.filter(|t| t.seq == at) else {
        return Err(ReadError::BadRange(format!(
            "from_seq {from_seq} needs a trusted head at sequence {at}; stored checkpoints are not authenticated"
        )));
    };
    let row = below
        .filter(|r| u64::try_from(r.seq).ok() == Some(at))
        .ok_or_else(|| ReadError::BadRange(format!("entry {at} below from_seq is not stored")))?;
    let corrupt = |f: Found| {
        ReadError::Corrupt(Failure {
            stream: stream.to_string(),
            seq: Some(at),
            kind: f.kind,
            reason: f.reason,
            checkpoint: None,
            detail: f.detail,
        })
    };
    let (_, hash) = check_row(row, at, stream).map_err(corrupt)?;
    if hash != t.head {
        return Err(corrupt(found(
            FailureKind::Checkpoint,
            "forked_head",
            format!("trusted head {}, entry hash {hash}", t.head),
        )));
    }
    Ok(Start { seq: from_seq, prev: t.head, anchor: Anchor::Trusted { seq: at } })
}

/// Most head-cache rows checked.
pub const MAX_HEADS: usize = 65_536;

/// After a successful walk, checks the derived head cache (`heads` rows `(branch,
/// entry_hash)`, in branch order): a row naming no entry of the journal is
/// `link/head_past_chain`, one naming an entry of another branch `link/head_branch` (both
/// without a sequence). `lookup` gives the branch column of the entry whose stored `hash`
/// column is the given text. A stale row naming an older entry of its branch is allowed
/// (`CORE.JRN.CHAIN.CKPT`: the journal wins). Does nothing on a failed report.
pub fn check_heads<H, L>(report: &mut VerifyReport, heads: H, mut lookup: L) -> Result<(), ReadError>
where
    H: IntoIterator<Item = Result<(String, String), ReadError>>,
    L: FnMut(&str) -> Result<Option<String>, ReadError>,
{
    if !report.ok() {
        return Ok(());
    }
    for (i, head) in heads.into_iter().enumerate() {
        if i >= MAX_HEADS {
            return Err(ReadError::InputLimit(format!("more than {MAX_HEADS} head rows")));
        }
        let (branch, entry_hash) = head?;
        let reason = match lookup(&entry_hash)? {
            None => "head_past_chain",
            Some(b) if b != branch => "head_branch",
            Some(_) => continue,
        };
        report.failure = Some(Failure {
            stream: report.stream.clone(),
            seq: None,
            kind: FailureKind::Link,
            reason,
            checkpoint: None,
            detail: format!("head of branch {:?} names {:?}", echo(&branch), echo(&entry_hash)),
        });
        return Ok(());
    }
    Ok(())
}

/// What a verification checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Checked {
    pub entries: u64,
    pub blobs: u64,
    pub checkpoints: u64,
    pub without_payload: u64,
}

/// The result of `journal.verify_chain@1` (REQ-002, REQ-003, REQ-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub stream: String,
    /// First sequence verified: 1 for a full verification.
    pub from_seq: u64,
    /// What the first verified entry was linked to.
    pub anchor: Anchor,
    /// `None` when the journal verified.
    pub failure: Option<Failure>,
    /// Counts up to the failure, or of the whole walk on success.
    pub checked: Checked,
}

impl VerifyReport {
    /// True when no failure was found.
    pub fn ok(&self) -> bool {
        self.failure.is_none()
    }

    /// The command result `{ok, from_seq, anchor, failure?, checked: {entries, blobs,
    /// checkpoints, without_payload}}`; sequences and counts are decimal strings.
    pub fn to_value(&self) -> CValue {
        let n = |v: u64| CValue::Str(v.to_string());
        let mut checked = BTreeMap::new();
        checked.insert("entries".to_string(), n(self.checked.entries));
        checked.insert("blobs".to_string(), n(self.checked.blobs));
        checked.insert("checkpoints".to_string(), n(self.checked.checkpoints));
        checked.insert("without_payload".to_string(), n(self.checked.without_payload));
        let mut o = BTreeMap::new();
        o.insert("ok".to_string(), CValue::Bool(self.ok()));
        o.insert("from_seq".to_string(), CValue::Str(self.from_seq.to_string()));
        o.insert("anchor".to_string(), self.anchor.to_value());
        o.insert("checked".to_string(), CValue::Obj(checked));
        if let Some(f) = &self.failure {
            o.insert("failure".to_string(), f.to_value());
        }
        CValue::Obj(o)
    }
}

// ------------------------------------------------------------------ header checks

type Obj = BTreeMap<String, CValue>;

/// A located failure before the stream name is attached.
struct Found {
    kind: FailureKind,
    reason: &'static str,
    checkpoint: Option<i64>,
    detail: String,
}

fn found(kind: FailureKind, reason: &'static str, detail: impl Into<String>) -> Found {
    Found { kind, reason, checkpoint: None, detail: detail.into() }
}

fn digest_str(d: &Digest) -> String {
    d.to_string()
}

/// Steps 2 to 8: the header bytes on their own. Returns the header object and its hash.
fn check_header(bytes: &[u8], len: u64) -> Result<(Obj, Digest), Found> {
    if len > MAX_HEADER_BYTES as u64 || bytes.len() > MAX_HEADER_BYTES {
        return Err(found(FailureKind::Hash, "oversize", format!("header of {len} bytes, at most {MAX_HEADER_BYTES}")));
    }
    let value = parse_with_limits(bytes, MAX_HEADER_BYTES, MAX_DEPTH)
        .map_err(|e| found(FailureKind::Hash, "malformed", e.to_string()))?;
    match to_canonical_bytes(&value) {
        Ok(c) if c == bytes => {}
        _ => return Err(found(FailureKind::Hash, "non_canonical", "header bytes are not their RFC 8785 form")),
    }
    let CValue::Obj(mut o) = value else {
        return Err(found(FailureKind::Hash, "schema", "header is not an object"));
    };
    let recorded = match o.remove("hash") {
        Some(CValue::Str(s)) => parse_digest(&s),
        _ => None,
    };
    let Some(recorded) = recorded else {
        return Err(found(FailureKind::Hash, "schema", "header.hash is missing or not a digest"));
    };
    let bare = CValue::Obj(o);
    let computed = to_canonical_bytes(&bare)
        .map(|b| digest_bytes(&b))
        .map_err(|e| found(FailureKind::Hash, "non_canonical", e.to_string()))?;
    if computed != recorded {
        return Err(found(
            FailureKind::Hash,
            "hash_mismatch",
            format!("header.hash {recorded}, recomputed {computed}"),
        ));
    }
    let CValue::Obj(mut o) = bare else {
        return Err(found(FailureKind::Hash, "schema", "header is not an object"));
    };
    match o.get("schema_version") {
        Some(CValue::Num(v)) if *v == f64::from(SCHEMA_V1) || *v == f64::from(SCHEMA_V2) => {}
        _ => return Err(found(FailureKind::Hash, "unknown_version", "schema_version is not 1 or 2")),
    }
    o.insert("hash".to_string(), CValue::Str(recorded.to_string()));
    let full = CValue::Obj(o);
    validate_header_value(&full).map_err(|e| found(FailureKind::Hash, "schema", e.to_string()))?;
    let CValue::Obj(o) = full else {
        return Err(found(FailureKind::Hash, "schema", "header is not an object"));
    };
    Ok((o, recorded))
}

fn header_text<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        _ => "",
    }
}

/// Steps 2 to 13 on one row at sequence `seq` of `stream`. Returns the header and its hash.
fn check_row(row: &StoredEntry, seq: u64, stream: &str) -> Result<(Obj, Digest), Found> {
    let (o, hash) = check_header(&row.header, row.header_len)?;
    if parse_digest(&row.hash) != Some(hash) {
        return Err(found(FailureKind::Hash, "column_mismatch", "hash column differs from the header hash"));
    }
    if header_text(&o, "seq") != seq.to_string() {
        return Err(found(
            FailureKind::Sequence,
            "header_seq",
            format!("header.seq {:?} at stored sequence {seq}", header_text(&o, "seq")),
        ));
    }
    if header_text(&o, "stream") != stream {
        return Err(found(
            FailureKind::Sequence,
            "foreign_stream",
            format!("header.stream {:?}", echo(header_text(&o, "stream"))),
        ));
    }
    for (col, key) in [(&row.stream, "stream"), (&row.branch, "branch"), (&row.kind, "kind")] {
        if col.as_str() != header_text(&o, key) {
            return Err(found(FailureKind::Hash, "column_mismatch", format!("{key} column differs from header.{key}")));
        }
    }
    let prev = parse_digest(header_text(&o, "prev"));
    if row.prev_hash.as_deref().and_then(parse_digest) != prev || prev.is_none() {
        return Err(found(FailureKind::Link, "column_mismatch", "prev_hash column differs from header.prev"));
    }
    Ok((o, hash))
}

/// Steps 1 to 14 on one row at position `p` of `stream` whose predecessor hashes to `prev`
/// (the zero digest at sequence 1), for readers that use headers only (module `diff`): the
/// same checks and the same located failures as [`verify_rows`], without the blob steps.
/// Returns the verified header object and its hash.
pub(crate) fn check_linked(row: &StoredEntry, p: u64, stream: &str, prev: &Digest) -> Result<(Obj, Digest), Failure> {
    let at = |f: Found| Failure {
        stream: stream.to_string(),
        seq: Some(p),
        kind: f.kind,
        reason: f.reason,
        checkpoint: f.checkpoint,
        detail: f.detail,
    };
    let col = u64::try_from(row.seq).ok();
    if col != Some(p) {
        let reason = if col.is_some_and(|c| c > p) { "gap" } else { "reorder" };
        return Err(at(found(FailureKind::Sequence, reason, format!("stored sequence {}", row.seq))));
    }
    let (o, hash) = check_row(row, p, stream).map_err(at)?;
    let header_prev = parse_digest(header_text(&o, "prev"));
    if header_prev != Some(*prev) {
        let reason = if p == FIRST_SEQ { "genesis" } else { "broken" };
        let detail = format!("prev {}, expected {}", header_text(&o, "prev"), digest_str(prev));
        return Err(at(found(FailureKind::Link, reason, detail)));
    }
    Ok((o, hash))
}

fn echo(s: &str) -> String {
    s.chars().take(64).collect()
}

/// The committed fields of a header: payload key and commitment.
fn commitments(o: &Obj) -> BTreeMap<&'static str, Digest> {
    const PATHS: [(&str, &str, Option<&str>); 6] = [
        ("identity", "principal", Some("commit")),
        ("on_behalf_of", "principal", Some("on_behalf_of_commit")),
        ("reason", "reason_commit", None),
        ("args", "command", Some("args_commit")),
        ("events", "command", Some("events_commit")),
        ("upgrade_identity", "upgrade", Some("identity_commit")),
    ];
    let mut out = BTreeMap::new();
    for (field, outer, inner) in PATHS {
        let v = match (o.get(outer), inner) {
            (Some(CValue::Obj(i)), Some(k)) => i.get(k),
            (v, None) => v,
            _ => None,
        };
        if let Some(CValue::Str(s)) = v {
            if let Some(d) = parse_digest(s) {
                out.insert(field, d);
            }
        }
    }
    out
}

/// Step 16, content: the payload reproduces every commitment of the header.
fn payload_matches(o: &Obj, bytes: &[u8]) -> Result<(), String> {
    let v = parse_with_limits(bytes, MAX_PAYLOAD_BYTES, MAX_DEPTH).map_err(|e| format!("payload: {e}"))?;
    if to_canonical_bytes(&v).ok().as_deref() != Some(bytes) {
        return Err("payload bytes are not their RFC 8785 form".into());
    }
    let CValue::Obj(p) = v else { return Err("payload is not an object".into()) };
    let want = commitments(o);
    let Some(CValue::Obj(salts)) = p.get("salts") else { return Err("payload.salts is not an object".into()) };
    let keys_ok = p.len() == want.len() + 1
        && salts.len() == want.len()
        && want.keys().all(|k| p.contains_key(*k) && salts.contains_key(*k));
    if !keys_ok {
        return Err("payload fields differ from the header's committed fields".into());
    }
    for (field, c) in &want {
        let salt = match salts.get(*field) {
            Some(CValue::Str(s)) => Salt::from_hex(s).map_err(|_| format!("payload.salts.{field} is not a salt"))?,
            _ => return Err(format!("payload.salts.{field} is not a string")),
        };
        let value = p.get(*field).ok_or_else(|| format!("payload.{field} is missing"))?;
        match commitment(&salt, value) {
            Ok(got) if got == *c => {}
            _ => return Err(format!("payload.{field} does not reproduce its commitment")),
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ verification

struct Cp {
    id: i64,
    seq: u64,
    head: Digest,
}

fn decode_checkpoint(row: &CheckpointRow, stream: &str) -> Result<Cp, Found> {
    let at = |reason: &'static str, detail: String| Found {
        kind: FailureKind::Checkpoint,
        reason,
        checkpoint: Some(row.id),
        detail,
    };
    if row.id < 1 {
        return Err(at("bad_id", format!("row id {} (append writes ids from 1)", row.id)));
    }
    let cp = match Checkpoint::from_bytes(&row.body) {
        Ok(cp) => cp,
        Err(CheckpointError::UnknownFormat(f)) => {
            return Err(at("unknown_format", format!("format {f:?}")));
        }
        Err(e) => return Err(at("malformed", e.to_string())),
    };
    if row.stream != stream {
        return Err(at("foreign_stream", format!("row stream {:?}", echo(&row.stream))));
    }
    if cp.stream != row.stream || parse_digest(&row.entry_hash) != Some(cp.head) {
        return Err(at("column_mismatch", "row columns differ from the checkpoint body".into()));
    }
    Ok(Cp { id: row.id, seq: cp.seq, head: cp.head })
}

fn cancelled(opts: &VerifyOptions) -> bool {
    opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst))
}

/// Verifies `rows` (stored order, starting at `start.seq`) of `stream` with their
/// `checkpoints` and blobs (module documentation, Pinned rules). `Ok` holds the report,
/// failed or not; `Err` is a refusal.
pub fn verify_rows<C, R, B>(
    stream: &str,
    start: Start,
    opts: &VerifyOptions,
    checkpoints: C,
    rows: R,
    blobs: &mut B,
) -> Result<VerifyReport, ReadError>
where
    C: IntoIterator<Item = Result<CheckpointRow, ReadError>>,
    R: IntoIterator<Item = Result<StoredEntry, ReadError>>,
    B: BlobSource + ?Sized,
{
    let mut report = VerifyReport {
        stream: stream.to_string(),
        from_seq: start.seq,
        anchor: start.anchor,
        failure: None,
        checked: Checked::default(),
    };
    let fail = |report: &mut VerifyReport, seq: Option<u64>, f: Found| {
        report.failure = Some(Failure {
            stream: stream.to_string(),
            seq,
            kind: f.kind,
            reason: f.reason,
            checkpoint: f.checkpoint,
            detail: f.detail,
        });
    };
    // Checkpoints at or above the start, by sequence then row id.
    let mut at_seq: BTreeMap<u64, Vec<(i64, Digest)>> = BTreeMap::new();
    let mut n_rows = 0usize;
    for row in checkpoints {
        let row = row?;
        n_rows += 1;
        if n_rows > MAX_CHECKPOINTS {
            return Err(ReadError::InputLimit(format!("more than {MAX_CHECKPOINTS} checkpoints")));
        }
        match decode_checkpoint(&row, stream) {
            Ok(cp) if cp.seq >= start.seq => {
                at_seq.entry(cp.seq).or_default().push((cp.id, cp.head));
                report.checked.checkpoints += 1;
            }
            Ok(_) => {}
            Err(f) => {
                fail(&mut report, None, f);
                return Ok(report);
            }
        }
    }
    for list in at_seq.values_mut() {
        list.sort_by_key(|(id, _)| *id);
    }
    let mut position = start.seq;
    let mut prev = start.prev;
    let mut walked: u64 = 0;
    let mut seen_blobs: BTreeSet<[u8; 32]> = BTreeSet::new();
    for row in rows {
        if cancelled(opts) {
            return Err(ReadError::Cancelled);
        }
        let row = row?;
        walked = walked.checked_add(1).ok_or_else(|| ReadError::InputLimit("entry count overflow".into()))?;
        if walked > opts.max_entries {
            return Err(ReadError::InputLimit(format!("more than {} entries", opts.max_entries)));
        }
        let p = position;
        if let Some(t) = opts.trusted.filter(|t| t.exact && p > t.seq) {
            fail(
                &mut report,
                Some(p),
                found(FailureKind::Sequence, "extended", format!("the trusted head ends the journal at {}", t.seq)),
            );
            return Ok(report);
        }
        // 1. column sequence.
        let col = u64::try_from(row.seq).ok();
        if col != Some(p) {
            let reason = if col.is_some_and(|c| c > p) { "gap" } else { "reorder" };
            fail(&mut report, Some(p), found(FailureKind::Sequence, reason, format!("stored sequence {}", row.seq)));
            return Ok(report);
        }
        // 2 to 13.
        let (o, hash) = match check_row(&row, p, stream) {
            Ok(x) => x,
            Err(f) => {
                fail(&mut report, Some(p), f);
                return Ok(report);
            }
        };
        // 14. link.
        let header_prev = parse_digest(header_text(&o, "prev"));
        if header_prev != Some(prev) {
            let reason = if p == FIRST_SEQ { "genesis" } else { "broken" };
            let detail = format!("prev {}, expected {}", header_text(&o, "prev"), digest_str(&prev));
            fail(&mut report, Some(p), found(FailureKind::Link, reason, detail));
            return Ok(report);
        }
        // 15. blob rows.
        if row.refs.len() > MAX_ENTRY_REFS {
            return Err(ReadError::InputLimit(format!("entry {p} has more than {MAX_ENTRY_REFS} blob rows")));
        }
        let mut payloads = Vec::new();
        let mut refs = Vec::new();
        for (role, d) in &row.refs {
            if role != ROLE_PAYLOAD && role != ROLE_REF {
                fail(&mut report, Some(p), found(FailureKind::Blob, "unknown_role", format!("role {:?}", echo(role))));
                return Ok(report);
            }
            let Some(d) = parse_digest(d) else {
                fail(&mut report, Some(p), found(FailureKind::Blob, "bad_ref", "blob digest is not sha256:<hex>"));
                return Ok(report);
            };
            if role == ROLE_PAYLOAD {
                payloads.push(d);
            } else {
                refs.push(d);
            }
        }
        let payload = match payloads.as_slice() {
            [one] => *one,
            [] => {
                fail(&mut report, Some(p), found(FailureKind::Blob, "payload_ref_missing", "no payload blob row"));
                return Ok(report);
            }
            _ => {
                fail(&mut report, Some(p), found(FailureKind::Blob, "payload_ref_duplicate", "several payload rows"));
                return Ok(report);
            }
        };
        // 16. payload.
        match blobs.payload(&payload)? {
            None => report.checked.without_payload += 1,
            Some(bytes) => {
                if bytes.len() > MAX_PAYLOAD_BYTES || digest_bytes(&bytes) != payload {
                    fail(&mut report, Some(p), found(FailureKind::Blob, "corrupt", format!("payload {payload}")));
                    return Ok(report);
                }
                if let Err(why) = payload_matches(&o, &bytes) {
                    fail(&mut report, Some(p), found(FailureKind::Blob, "payload_mismatch", why));
                    return Ok(report);
                }
            }
        }
        // 17. referenced blobs, each digest once per run.
        refs.sort_by_key(|d| d.0);
        refs.dedup();
        for d in refs {
            if seen_blobs.contains(&d.0) {
                continue;
            }
            match blobs.check(&d)? {
                BlobState::Sound => {}
                BlobState::Missing => {
                    fail(&mut report, Some(p), found(FailureKind::Blob, "missing", format!("blob {d}")));
                    return Ok(report);
                }
                BlobState::Corrupt => {
                    fail(&mut report, Some(p), found(FailureKind::Blob, "corrupt", format!("blob {d}")));
                    return Ok(report);
                }
            }
            if seen_blobs.len() >= opts.max_blobs {
                return Err(ReadError::InputLimit(format!("more than {} distinct blobs", opts.max_blobs)));
            }
            seen_blobs.insert(d.0);
            report.checked.blobs += 1;
        }
        // 18. checkpoints and the trusted head at this sequence.
        if let Some(list) = at_seq.get(&p) {
            if let Some((id, head)) = list.iter().find(|(_, head)| *head != hash) {
                let f = Found {
                    kind: FailureKind::Checkpoint,
                    reason: "forked_head",
                    checkpoint: Some(*id),
                    detail: format!("checkpoint head {head}, entry hash {hash}"),
                };
                fail(&mut report, Some(p), f);
                return Ok(report);
            }
        }
        if let Some(t) = opts.trusted.filter(|t| t.seq == p && t.head != hash) {
            let detail = format!("trusted head {}, entry hash {hash}", t.head);
            fail(&mut report, Some(p), found(FailureKind::Checkpoint, "forked_head", detail));
            return Ok(report);
        }
        report.checked.entries += 1;
        prev = hash;
        position = p.checked_add(1).ok_or_else(|| ReadError::InputLimit("sequence overflow".into()))?;
    }
    // After the last row: checkpoints and the trusted head above it.
    if let Some((seq, list)) = at_seq.range(position..).next() {
        let id = list.first().map(|(id, _)| *id);
        let f = Found {
            kind: FailureKind::Checkpoint,
            reason: "truncated",
            checkpoint: id,
            detail: format!("checkpoint at sequence {seq}, journal ends before {position}"),
        };
        fail(&mut report, Some(*seq), f);
        return Ok(report);
    }
    if let Some(t) = opts.trusted.filter(|t| t.seq >= position) {
        let detail = format!("trusted head at sequence {}, journal ends before {position}", t.seq);
        fail(&mut report, Some(t.seq), found(FailureKind::Checkpoint, "truncated", detail));
    }
    Ok(report)
}

// ------------------------------------------------------------------ listing

/// Parameters of `journal.list_entries@1` (REQ-005).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    pub stream: Option<String>,
    pub branch: Option<String>,
    pub from_seq: Option<u64>,
    pub to_seq: Option<u64>,
    /// `name@major`.
    pub command: Option<String>,
    pub principal_kind: Option<String>,
    pub page_token: Option<String>,
}

/// One listed entry: its sequence, hash and header (checked, never interpreted further).
#[derive(Debug, Clone, PartialEq)]
pub struct ListedEntry {
    pub seq: u64,
    pub hash: Digest,
    pub header: CValue,
}

/// One page of `journal.list_entries@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct ListPage {
    pub headers: Vec<ListedEntry>,
    pub next: Option<String>,
}

impl ListPage {
    /// The command result `{headers: [EntryHeader], next?}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("headers".to_string(), CValue::Arr(self.headers.iter().map(|h| h.header.clone()).collect()));
        if let Some(n) = &self.next {
            o.insert("next".to_string(), CValue::Str(n.clone()));
        }
        CValue::Obj(o)
    }
}

/// A checked list query: the sequence range and the filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPlan {
    pub stream: String,
    /// First sequence to scan (from the token, else `from_seq`, else 1).
    pub start: u64,
    /// Last sequence to scan, inclusive.
    pub end: u64,
    branch: Option<String>,
    command: Option<(String, u32)>,
    principal_kind: Option<String>,
    filter: String,
}

const PRINCIPAL_KINDS: [&str; 6] = ["human", "agent", "script", "service", "device", "system"];

fn is_ident(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn parse_command(s: &str) -> Option<(String, u32)> {
    let (name, major) = s.split_once('@')?;
    let ok_name = fid_types::CommandName::is_valid(name);
    let ok_major = !major.is_empty() && major.len() <= 5 && major.bytes().all(|c| c.is_ascii_digit());
    if !ok_name || !ok_major || (major.len() > 1 && major.starts_with('0')) {
        return None;
    }
    Some((name.to_string(), major.parse().ok()?))
}

fn filter_digest(stream: &str, q: &ListQuery) -> String {
    let opt = |v: &Option<String>| v.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()));
    let opt_n = |v: Option<u64>| v.map_or(CValue::Null, |n| CValue::Str(n.to_string()));
    let mut o = BTreeMap::new();
    o.insert("stream".to_string(), CValue::Str(stream.to_string()));
    o.insert("branch".to_string(), opt(&q.branch));
    o.insert("from_seq".to_string(), opt_n(q.from_seq));
    o.insert("to_seq".to_string(), opt_n(q.to_seq));
    o.insert("command".to_string(), opt(&q.command));
    o.insert("principal_kind".to_string(), opt(&q.principal_kind));
    let bytes = to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default();
    let d = digest_bytes(&bytes).to_string();
    d.get(7..23).unwrap_or_default().to_string()
}

fn parse_token(token: &str, filter: &str) -> Result<u64, ReadError> {
    let rest = token.strip_prefix(TOKEN_PREFIX).ok_or(ReadError::BadToken)?;
    let (f, seq) = rest.split_once('.').ok_or(ReadError::BadToken)?;
    let hex_ok = f.len() == 16 && f.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    let seq_ok = !seq.is_empty() && seq.len() <= 20 && !seq.starts_with('0') && seq.bytes().all(|c| c.is_ascii_digit());
    if !hex_ok || !seq_ok {
        return Err(ReadError::BadToken);
    }
    let seq: u64 = seq.parse().map_err(|_| ReadError::BadToken)?;
    if f != filter {
        return Err(ReadError::StaleToken);
    }
    Ok(seq)
}

/// Checks a list query against the journal's `stream` and its `tail` sequence (0 when
/// empty): parameters, range and token.
pub fn plan_list(stream: &str, tail: u64, q: &ListQuery) -> Result<ListPlan, ReadError> {
    if let Some(s) = &q.stream {
        if s != stream {
            return Err(ReadError::UnknownStream(echo(s)));
        }
    }
    if let Some(b) = &q.branch {
        let ok = !b.is_empty()
            && b.len() <= 256
            && b.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && b.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'/' | b'-'));
        if !ok {
            return Err(ReadError::BadParam(format!("branch {:?} is not a branch name", echo(b))));
        }
    }
    let command = match &q.command {
        None => None,
        Some(c) => Some(
            parse_command(c).ok_or_else(|| ReadError::BadParam(format!("command {:?} is not name@major", echo(c))))?,
        ),
    };
    if let Some(k) = &q.principal_kind {
        if !PRINCIPAL_KINDS.contains(&k.as_str()) {
            return Err(ReadError::BadParam(format!("principal_kind {:?} is unknown", echo(k))));
        }
    }
    let from = q.from_seq.unwrap_or(FIRST_SEQ);
    let to = q.to_seq.unwrap_or(u64::MAX);
    if from == 0 || to == 0 || from > to {
        return Err(ReadError::BadRange(format!("from_seq {from}, to_seq {to}")));
    }
    let filter = filter_digest(stream, q);
    let start = match &q.page_token {
        None => from,
        Some(t) => {
            let s = parse_token(t, &filter)?;
            if s < from || s > to || s > tail.saturating_add(1) {
                return Err(ReadError::BadToken);
            }
            s
        }
    };
    Ok(ListPlan {
        stream: stream.to_string(),
        start,
        end: to,
        branch: q.branch.clone(),
        command,
        principal_kind: q.principal_kind.clone(),
        filter,
    })
}

impl ListPlan {
    /// The continuation token for sequence `next`.
    pub fn token(&self, next: u64) -> String {
        format!("{TOKEN_PREFIX}{}.{next}", self.filter)
    }

    /// The branch filter, if any.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    fn matches(&self, o: &Obj) -> bool {
        if let Some(b) = &self.branch {
            if header_text(o, "branch") != b {
                return false;
            }
        }
        if let Some(k) = &self.principal_kind {
            match o.get("principal") {
                Some(CValue::Obj(p)) if header_text(p, "kind") == k => {}
                _ => return false,
            }
        }
        if let Some((name, major)) = &self.command {
            match o.get("command") {
                Some(CValue::Obj(c)) => {
                    let m = match c.get("major") {
                        Some(CValue::Num(m)) => *m,
                        _ => return false,
                    };
                    if header_text(c, "id") != name || m != f64::from(*major) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

/// One page of headers from `rows` (stored order, starting at or after `plan.start`, at most
/// [`MAX_SCAN`] + 1 of them are read). A row that does not verify on its own is the refusal
/// [`ReadError::Corrupt`].
pub fn list_rows<R>(plan: &ListPlan, rows: R) -> Result<ListPage, ReadError>
where
    R: IntoIterator<Item = Result<StoredEntry, ReadError>>,
{
    let mut headers = Vec::new();
    let mut last: Option<u64> = None;
    for (scanned, row) in rows.into_iter().enumerate() {
        let row = row?;
        let seq = u64::try_from(row.seq).map_err(|_| ReadError::Storage(format!("negative sequence {}", row.seq)))?;
        if seq < plan.start || last.is_some_and(|l| seq <= l) {
            return Err(ReadError::Storage(format!("rows out of order at sequence {seq}")));
        }
        if seq > plan.end {
            break;
        }
        if headers.len() >= MAX_PAGE || scanned >= MAX_SCAN {
            return Ok(ListPage { headers, next: Some(plan.token(seq)) });
        }
        last = Some(seq);
        let (o, hash) = check_row(&row, seq, &plan.stream).map_err(|f| {
            ReadError::Corrupt(Failure {
                stream: plan.stream.clone(),
                seq: Some(seq),
                kind: f.kind,
                reason: f.reason,
                checkpoint: None,
                detail: f.detail,
            })
        })?;
        if plan.matches(&o) {
            headers.push(ListedEntry { seq, hash, header: CValue::Obj(o) });
        }
    }
    Ok(ListPage { headers, next: None })
}

/// Checks that `s` names a stream identifier.
pub fn is_stream_name(s: &str) -> bool {
    s.len() <= 64 && is_ident(s)
}
