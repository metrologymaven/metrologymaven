//! Branch heads and checkpoints. Spec node `CORE.JRN.CHAIN.CKPT`.
//!
//! A [`Checkpoint`] is a small canonical record naming a stream, a sequence, the hash of the
//! entry at that sequence (the head) and the number of entries up to it, with the reason it
//! was made (`signature`, `export`, `close`), the time, and optionally the Access Ledger's
//! head. `TRUST` signs its canonical bytes ([`Checkpoint::canonical_bytes`]); this module
//! never signs. [`verify`] checks a checkpoint against a journal given as [`Link`]s.
//!
//! This part is storage-free (also built for wasm32 users such as an independent
//! verifier). With feature `store`, `Journal::checkpoint`, `Journal::head` and friends add
//! the derived head cache and the `checkpoints` table (submodule `store`).
//!
//! # Pinned rules
//!
//! - **Layout (`linear-v1`).** Exactly the keys `format`, `stream`, `seq`, `head`, `count`,
//!   `reason`, `time`, plus `ledger` `{seq, head}` when an Access Ledger head is given
//!   (absent, never `null`, otherwise). `format` is [`FORMAT_LINEAR_V1`]. `seq`, `count` and
//!   `ledger.seq` are decimal strings from 1 to 2^64-1 without leading zeros (JSON numbers
//!   are refused). `head` and `ledger.head` are `sha256:` + 64 lowercase hex digits.
//!   `stream` is `[a-z][a-z0-9_]*` of at most [`MAX_STREAM_BYTES`] bytes. `time` is
//!   `YYYY-MM-DDThh:mm:ssZ`, a real calendar instant with second 0-59 (whole seconds, no
//!   fraction, no leap second). Published as `schema/checkpoint.schema.json` (version 1).
//! - **Count.** In `linear-v1` sequences are gap-free from 1, so `count` equals `seq`; a
//!   checkpoint where they differ is `schema`. The field is kept so a later format whose
//!   count differs (a compacted or Merkle log) keeps the same name.
//! - **Format first.** Decoding reads `format` before anything else: a `format` string
//!   other than `linear-v1` is `unknown_format`, whatever the other keys hold, so a P1b
//!   Merkle checkpoint is refused cleanly by a P0 verifier. A missing or non-string
//!   `format` is `schema`.
//! - **Canonical bytes and digest (REQ-003).** The canonical bytes are the RFC 8785 form
//!   (`fid-canon`); the digest is SHA-256 over them. These bytes are what TRUST signs, with
//!   no prefix added here (domain separation belongs to the signature envelope).
//! - **Decoding.** [`Checkpoint::from_bytes`] takes at most [`MAX_CHECKPOINT_BYTES`]
//!   (`input_limit` beyond), refuses invalid UTF-8, invalid JSON and duplicate keys
//!   (`schema`), and requires the bytes to be exactly the canonical form of the value they
//!   decode to (`schema` otherwise), so one checkpoint has one byte string.
//! - **Time.** The host clock truncated to whole seconds, in UTC ([`format_time`]); an
//!   instant after 9999-12-31T23:59:59Z cannot be written.
//! - **Verification ([`verify`]).** Given a checkpoint, the stream name and the journal's
//!   links in the order they are stored: a stream other than the checkpoint's is
//!   `stream_mismatch`. Walking positions 1, 2, ...: a link whose `seq` is not its position
//!   is `reordered` (this also covers gaps and duplicates); a link whose `prev` is not the
//!   hash of the link before it (the zero digest at position 1) is `broken_link`; at the
//!   checkpoint's sequence a hash other than its head is `forked_head`. Fewer links than
//!   the checkpoint's sequence is `truncated`. With [`Coverage::Exact`] (an export or a
//!   closed project, which must end at the checkpoint) any link after it is `extended`;
//!   those links are counted, not checked. With [`Coverage::Prefix`] (a live journal that
//!   may have grown) links after the checkpoint are not read. Entry hashes are not
//!   recomputed here: that is `CORE.JRN.CHAIN.VERIFY`'s work over the headers.
//! - **Bounds.** Decoding is bounded by [`MAX_CHECKPOINT_BYTES`]; [`verify`] streams the
//!   links in constant memory. No panics; every arithmetic step is checked.

#[cfg(feature = "store")]
mod store;

#[cfg(feature = "store")]
pub use store::{BranchHead, HeadsRebuilt, StoredCheckpoint, MAX_BRANCHES};

use crate::entry::{parse_digest, GENESIS_PREV};
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, Digest};
use std::collections::BTreeMap;
use std::fmt;

/// The P0 checkpoint format: a linear head.
pub const FORMAT_LINEAR_V1: &str = "linear-v1";
/// Largest accepted checkpoint, in bytes (a `linear-v1` checkpoint is about 300).
pub const MAX_CHECKPOINT_BYTES: usize = 4096;
/// Longest stream name, in bytes (the store's identifier bound).
pub const MAX_STREAM_BYTES: usize = 64;
/// Nesting depth accepted when decoding (root object and `ledger`).
const MAX_DECODE_DEPTH: usize = 2;
/// Last instant [`format_time`] can write: 9999-12-31T23:59:59Z.
pub const MAX_TIME_UNIX: u64 = 253_402_300_799;
/// Longest unknown `format` value echoed in an error.
const MAX_ECHO: usize = 64;

// ------------------------------------------------------------------ errors

/// Typed errors of decoding and verification, with stable codes ([`CheckpointError::code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointError {
    /// `format` names a layout this verifier does not know (truncated to 64 bytes).
    UnknownFormat(String),
    /// The checkpoint does not follow the `linear-v1` layout, or its bytes are not canonical.
    Schema(String),
    /// The encoded checkpoint is over [`MAX_CHECKPOINT_BYTES`].
    InputLimit(usize),
    /// The journal is another stream than the checkpoint's.
    StreamMismatch { checkpoint: String, journal: String },
    /// The journal ends before the checkpoint's sequence.
    Truncated { checkpoint_seq: u64, journal_count: u64 },
    /// The journal goes on after the checkpoint's sequence ([`Coverage::Exact`] only).
    Extended { checkpoint_seq: u64, journal_count: u64 },
    /// The link at `position` (1-based) carries sequence `seq`.
    Reordered { position: u64, seq: u64 },
    /// The link at `seq` does not name the hash of the link before it.
    BrokenLink { seq: u64 },
    /// The journal's entry at the checkpoint's sequence is not the checkpoint's head.
    ForkedHead { seq: u64, checkpoint: Digest, journal: Digest },
    /// Reading the journal failed.
    Storage(String),
}

impl CheckpointError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CheckpointError::UnknownFormat(_) => "unknown_format",
            CheckpointError::Schema(_) => "schema",
            CheckpointError::InputLimit(_) => "input_limit",
            CheckpointError::StreamMismatch { .. } => "stream_mismatch",
            CheckpointError::Truncated { .. } => "truncated",
            CheckpointError::Extended { .. } => "extended",
            CheckpointError::Reordered { .. } => "reordered",
            CheckpointError::BrokenLink { .. } => "broken_link",
            CheckpointError::ForkedHead { .. } => "forked_head",
            CheckpointError::Storage(_) => "storage",
        }
    }
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckpointError::UnknownFormat(s) => {
                write!(f, "unknown_format: {s:?} (this verifier knows {FORMAT_LINEAR_V1})")
            }
            CheckpointError::Schema(m) => write!(f, "schema: {m}"),
            CheckpointError::InputLimit(n) => {
                write!(f, "input_limit: checkpoint of {n} bytes, at most {MAX_CHECKPOINT_BYTES}")
            }
            CheckpointError::StreamMismatch { checkpoint, journal } => {
                write!(f, "stream_mismatch: checkpoint names stream {checkpoint:?}, journal is {journal:?}")
            }
            CheckpointError::Truncated { checkpoint_seq, journal_count } => {
                write!(f, "truncated: checkpoint at sequence {checkpoint_seq}, journal holds {journal_count} entries")
            }
            CheckpointError::Extended { checkpoint_seq, journal_count } => {
                write!(f, "extended: checkpoint at sequence {checkpoint_seq}, journal holds {journal_count} entries")
            }
            CheckpointError::Reordered { position, seq } => {
                write!(f, "reordered: entry at position {position} carries sequence {seq}")
            }
            CheckpointError::BrokenLink { seq } => {
                write!(f, "broken_link: entry {seq} does not link to the entry before it")
            }
            CheckpointError::ForkedHead { seq, checkpoint, journal } => {
                write!(
                    f,
                    "forked_head: at sequence {seq} the checkpoint names {checkpoint}, the journal holds {journal}"
                )
            }
            CheckpointError::Storage(m) => write!(f, "storage: {m}"),
        }
    }
}

impl std::error::Error for CheckpointError {}

fn schema(m: impl Into<String>) -> CheckpointError {
    CheckpointError::Schema(m.into())
}

// ------------------------------------------------------------------ the record

/// The layout of a checkpoint. P0 knows one; P1b Merkle checkpoints will add a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// [`FORMAT_LINEAR_V1`]: a signed linear head.
    LinearV1,
}

impl Format {
    /// The `format` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::LinearV1 => FORMAT_LINEAR_V1,
        }
    }
}

/// Why a checkpoint was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Signature,
    Export,
    Close,
}

impl Reason {
    /// Every reason, in schema order.
    pub const ALL: [Reason; 3] = [Reason::Signature, Reason::Export, Reason::Close];

    /// The `reason` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Signature => "signature",
            Reason::Export => "export",
            Reason::Close => "close",
        }
    }

    /// The reason named `s`.
    pub fn parse(s: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The Access Ledger's head (`CORE.JRN.STREAM.LEDGER` supplies it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerHead {
    pub seq: u64,
    pub head: Digest,
}

/// One checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub format: Format,
    pub stream: String,
    /// Sequence of the head entry.
    pub seq: u64,
    /// Hash of the entry at `seq`.
    pub head: Digest,
    /// Entries up to and including `seq` (equal to `seq` in `linear-v1`).
    pub count: u64,
    pub ledger: Option<LedgerHead>,
    pub reason: Reason,
    /// `YYYY-MM-DDThh:mm:ssZ`.
    pub time: String,
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

impl Checkpoint {
    /// The checkpoint as a canonical value (sequences as decimal strings).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("format".to_string(), text(self.format.as_str()));
        o.insert("stream".to_string(), text(&self.stream));
        o.insert("seq".to_string(), CValue::Str(self.seq.to_string()));
        o.insert("head".to_string(), CValue::Str(self.head.to_string()));
        o.insert("count".to_string(), CValue::Str(self.count.to_string()));
        if let Some(l) = &self.ledger {
            let mut lo = BTreeMap::new();
            lo.insert("seq".to_string(), CValue::Str(l.seq.to_string()));
            lo.insert("head".to_string(), CValue::Str(l.head.to_string()));
            o.insert("ledger".to_string(), CValue::Obj(lo));
        }
        o.insert("reason".to_string(), text(self.reason.as_str()));
        o.insert("time".to_string(), text(&self.time));
        CValue::Obj(o)
    }

    /// Checks the `linear-v1` rules on the fields.
    pub fn validate(&self) -> Result<(), CheckpointError> {
        if !is_stream(&self.stream) {
            return Err(schema(format!("stream {:?} is not [a-z][a-z0-9_]* of at most 64 bytes", self.stream)));
        }
        if self.seq == 0 {
            return Err(schema("seq must be at least 1"));
        }
        if self.count != self.seq {
            return Err(schema(format!("count {} differs from seq {} (linear-v1)", self.count, self.seq)));
        }
        if let Some(l) = &self.ledger {
            if l.seq == 0 {
                return Err(schema("ledger.seq must be at least 1"));
            }
        }
        if !is_time(&self.time) {
            return Err(schema(format!("time {:?} is not a whole-second UTC instant", self.time)));
        }
        Ok(())
    }

    /// The RFC 8785 bytes TRUST signs (REQ-003). Validates first.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CheckpointError> {
        self.validate()?;
        let bytes = to_canonical_bytes(&self.to_value()).map_err(|e| schema(e.to_string()))?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(CheckpointError::InputLimit(bytes.len()));
        }
        Ok(bytes)
    }

    /// SHA-256 of the canonical bytes (REQ-003).
    pub fn digest(&self) -> Result<Digest, CheckpointError> {
        self.canonical_bytes().map(|b| digest_bytes(&b))
    }

    /// Decodes a checkpoint value; `format` is read first.
    pub fn from_value(v: &CValue) -> Result<Checkpoint, CheckpointError> {
        let CValue::Obj(o) = v else { return Err(schema("checkpoint is not an object")) };
        let format = match o.get("format") {
            Some(CValue::Str(s)) if s == FORMAT_LINEAR_V1 => Format::LinearV1,
            Some(CValue::Str(s)) => return Err(CheckpointError::UnknownFormat(echo(s))),
            Some(_) => return Err(schema("format is not a string")),
            None => return Err(schema("format is missing")),
        };
        const KEYS: [&str; 8] = ["format", "stream", "seq", "head", "count", "ledger", "reason", "time"];
        if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(schema(format!("unknown key {:?}", echo(k))));
        }
        let field = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            Some(_) => Err(schema(format!("{k} is not a string"))),
            None => Err(schema(format!("{k} is missing"))),
        };
        let stream = field("stream")?;
        let seq = parse_seq(field("seq")?).ok_or_else(|| schema("seq is not a decimal sequence from 1"))?;
        let head = parse_digest(field("head")?).ok_or_else(|| schema("head is not a sha256 digest"))?;
        let count = parse_seq(field("count")?).ok_or_else(|| schema("count is not a decimal count from 1"))?;
        let reason =
            Reason::parse(field("reason")?).ok_or_else(|| schema("reason is not signature, export or close"))?;
        let time = field("time")?;
        let ledger = match o.get("ledger") {
            None => None,
            Some(CValue::Obj(l)) => {
                if l.len() != 2 {
                    return Err(schema("ledger must hold exactly seq and head"));
                }
                let lf = |k: &str| match l.get(k) {
                    Some(CValue::Str(s)) => Ok(s.as_str()),
                    _ => Err(schema(format!("ledger.{k} is missing or not a string"))),
                };
                Some(LedgerHead {
                    seq: parse_seq(lf("seq")?).ok_or_else(|| schema("ledger.seq is not a decimal sequence from 1"))?,
                    head: parse_digest(lf("head")?).ok_or_else(|| schema("ledger.head is not a sha256 digest"))?,
                })
            }
            Some(_) => return Err(schema("ledger is not an object")),
        };
        let cp =
            Checkpoint { format, stream: stream.to_string(), seq, head, count, ledger, reason, time: time.to_string() };
        cp.validate()?;
        Ok(cp)
    }

    /// Decodes canonical checkpoint bytes (module documentation, Decoding).
    pub fn from_bytes(bytes: &[u8]) -> Result<Checkpoint, CheckpointError> {
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(CheckpointError::InputLimit(bytes.len()));
        }
        let v = parse_with_limits(bytes, MAX_CHECKPOINT_BYTES, MAX_DECODE_DEPTH).map_err(|e| schema(e.to_string()))?;
        let cp = Checkpoint::from_value(&v)?;
        let canonical = to_canonical_bytes(&cp.to_value()).map_err(|e| schema(e.to_string()))?;
        if canonical != bytes {
            return Err(schema("bytes are not the canonical (RFC 8785) form"));
        }
        Ok(cp)
    }
}

fn echo(s: &str) -> String {
    s.chars().take(MAX_ECHO).collect()
}

fn is_stream(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_STREAM_BYTES
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// A decimal string from 1 to 2^64-1 without leading zeros.
fn parse_seq(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || s.starts_with('0') || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<u64>().ok()
}

// ------------------------------------------------------------------ time

fn days_in_month(y: u64, m: u64) -> u64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// True for `YYYY-MM-DDThh:mm:ssZ` naming a real instant with second 0-59.
pub fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 20 {
        return false;
    }
    let mut parts = [0u64; 6];
    let mut k = 0usize;
    for (i, c) in b"dddd-dd-ddTdd:dd:ddZ".iter().enumerate() {
        let Some(&got) = b.get(i) else { return false };
        if *c == b'd' {
            if !got.is_ascii_digit() {
                return false;
            }
            let Some(slot) = parts.get_mut(k) else { return false };
            *slot = *slot * 10 + u64::from(got - b'0');
        } else if got != *c {
            return false;
        } else {
            k += 1;
        }
    }
    let [y, mo, d, h, mi, se] = parts;
    (1..=12).contains(&mo) && d >= 1 && d <= days_in_month(y, mo) && h <= 23 && mi <= 59 && se <= 59
}

/// The checkpoint time of an instant `unix_secs` seconds after the Unix epoch:
/// `YYYY-MM-DDThh:mm:ssZ`, or `None` after [`MAX_TIME_UNIX`].
pub fn format_time(unix_secs: u64) -> Option<String> {
    if unix_secs > MAX_TIME_UNIX {
        return None;
    }
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    // H. Hinnant's civil_from_days, for days >= 0 (the era is non-negative).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    Some(format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60))
}

// ------------------------------------------------------------------ verification

/// One journal entry as the verifier sees it: its sequence, previous hash and hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub seq: u64,
    pub prev: Digest,
    pub hash: Digest,
}

/// How much of the journal the checkpoint must cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// The journal must end at the checkpoint (an export, a closed project).
    Exact,
    /// The journal may go on after the checkpoint (a live journal).
    Prefix,
}

/// A successful verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    /// Links checked: the checkpoint's sequence.
    pub checked: u64,
}

/// Checks `cp` against the journal of `stream`, given as `links` in stored order (module
/// documentation, Verification). Constant memory; reads links only as far as needed.
pub fn verify<I, E>(cp: &Checkpoint, stream: &str, links: I, coverage: Coverage) -> Result<Verified, CheckpointError>
where
    I: IntoIterator<Item = Result<Link, E>>,
    E: Into<CheckpointError>,
{
    cp.validate()?;
    if cp.stream != stream {
        return Err(CheckpointError::StreamMismatch { checkpoint: cp.stream.clone(), journal: echo(stream) });
    }
    let target = cp.seq;
    let mut position: u64 = 0;
    let mut prev = GENESIS_PREV;
    let mut links = links.into_iter();
    loop {
        if position >= target && coverage == Coverage::Prefix {
            break;
        }
        let Some(link) = links.next() else { break };
        let link = link.map_err(Into::into)?;
        if position >= target {
            position = position.checked_add(1).ok_or_else(|| schema("journal longer than 2^64-1 entries"))?;
            continue;
        }
        position += 1;
        if link.seq != position {
            return Err(CheckpointError::Reordered { position, seq: link.seq });
        }
        if link.prev != prev {
            return Err(CheckpointError::BrokenLink { seq: link.seq });
        }
        if position == target && link.hash != cp.head {
            return Err(CheckpointError::ForkedHead { seq: target, checkpoint: cp.head, journal: link.hash });
        }
        prev = link.hash;
    }
    if position < target {
        return Err(CheckpointError::Truncated { checkpoint_seq: target, journal_count: position });
    }
    if position > target {
        return Err(CheckpointError::Extended { checkpoint_seq: target, journal_count: position });
    }
    Ok(Verified { checked: target })
}

impl From<std::convert::Infallible> for CheckpointError {
    fn from(e: std::convert::Infallible) -> Self {
        match e {}
    }
}

/// [`verify`] over links that cannot fail to read.
pub fn verify_links<I: IntoIterator<Item = Link>>(
    cp: &Checkpoint,
    stream: &str,
    links: I,
    coverage: Coverage,
) -> Result<Verified, CheckpointError> {
    verify(cp, stream, links.into_iter().map(Ok::<Link, std::convert::Infallible>), coverage)
}
