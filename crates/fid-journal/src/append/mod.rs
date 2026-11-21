//! Atomic durable append, sequence and immutability. Spec node `CORE.JRN.CHAIN.APPEND`.
//!
//! [`Journal::append`] is the single write path into a chained stream. It takes an
//! [`AppendRequest`] (an entry draft plus the blobs it references), assigns the next
//! sequence number and previous hash of the stream, builds and hashes the entry with
//! [`crate::entry::build`], and stores header, payload and blob references in one SQLite
//! transaction of the project database (`fid-store`). It returns the entry hash only after
//! the COMMIT has returned with `synchronous=FULL`. Every write goes through
//! [`fid_store::Txn`] (`CORE.STO.SQL.TXN`), the store's one commit path. There is no update or
//! delete API, and the database's `append_only` triggers refuse both (`CORE.STO.SQL.SCHEMA`).
//!
//! # Pinned rules
//!
//! - **Sequence and link (REQ-002).** One physical chain per stream over append order,
//!   whatever the branch: `seq` is the tail's plus one (1 for an empty chain) and `prev` the
//!   tail's hash ([`GENESIS_PREV`] for sequence 1). The tail is verified before every batch:
//!   its header hash, its `seq`, `stream`, `branch`, `kind` and `prev_hash` columns against
//!   the header, the genesis rule (zero `prev` exactly at sequence 1) and, above sequence 1,
//!   that its `prev` is the stored hash at sequence - 1 (no fork). A tail that fails is
//!   `chain_invalid` and nothing is appended.
//! - **Chain walk at open.** [`Journal::open`] walks the `prev_hash` and `hash` columns of
//!   the whole chain once, in sequence order, without re-hashing any header: sequence 1 must
//!   have the zero `prev_hash` and every later row the `hash` of the row below it. A fork at
//!   any depth is `chain_invalid`. The walk streams the rows (constant memory, one pass); the
//!   headers are hashed only at the tail.
//! - **One stream per database.** Schema v1 keys `entries` by `seq` alone, so a project
//!   database holds one chained stream, the one the [`Journal`] is opened for. A draft for
//!   another stream is `schema`; a database holding another stream is `chain_invalid`.
//! - **Time validity.** `time.utc` must be a real calendar instant (month 1-12, a day that
//!   exists in that month, hour 0-23, minute 0-59, second 0-59, second 60 only at 23:59:60);
//!   otherwise `schema`.
//! - **Time ahead.** An entry more than [`MAX_AHEAD_SECS`] after the injected [`Clock`]
//!   (strictly greater) is refused with `time_ahead`, so one far-future stamp cannot lock the
//!   chain against every later entry.
//! - **Time order.** An entry whose `time.utc` instant is earlier than the chain tail's is
//!   refused with `time_regressed`; an equal instant is accepted. Instants are compared as
//!   instants on the Unix time line ([`utc_order`]; 23:59:60 equals the next 00:00:00), not
//!   as text; `mono_ns` is not compared (it restarts with the host). The bus stamps a
//!   `principal_upgrade` at confirm time and commits it later, so the guard sits here, at
//!   the one place that knows the tail.
//! - **Concurrency.** Appends are serialised in arrival order. A request may carry
//!   `expected_prev`, the tail hash it was prepared against; if another append landed first
//!   it fails with `head_moved` (the bus catalogue's `conflict/head_moved`) and appends
//!   nothing. Without it the request is linked to whatever the tail is when its turn comes.
//! - **Group commit.** Requests that arrive while a commit is in flight are taken, up to
//!   [`MAX_BATCH`], in arrival order into the next transaction. Each request runs inside its
//!   own staging step and enters the transaction only once it is accepted, so one request's
//!   refusal does not affect the others. A transaction stops taking requests once its staged
//!   payloads and headers reach [`MAX_BATCH_BYTES`]; the rest of the batch goes into the next
//!   transaction, in the same order.
//! - **Atomicity and durability (REQ-003, REQ-004).** Each request checks its referenced
//!   blobs with [`fid_store::Txn::check_blob`] (`missing_blob`, naming the missing blob or
//!   chunk; a stored blob that does not hash to its address is `missing_blob` too) and stages
//!   its payload blob and its `entries` and `entry_blobs` rows into the batch's
//!   [`fid_store::Txn`]. [`fid_store::Txn::commit`] then checks every referenced blob again,
//!   puts the payload blobs with `durable: true`, flushes the already-stored referenced blobs
//!   (file, fan-out directories and `sha256/`), and runs `BEGIN IMMEDIATE`, the inserts and
//!   `COMMIT`. The store opens the connection with WAL, `synchronous=FULL`, `fullfsync=ON`
//!   and `checkpoint_fullfsync=ON` and reads them back ([`Journal::durability`] reports
//!   them). Every failure before the COMMIT rolls back, so after a crash or error the chain is
//!   at the old head; at worst an unreferenced payload blob remains. Nothing after a
//!   successful COMMIT can fail the call.
//! - **Failed COMMIT: outcome unknown.** A COMMIT that reports an error may still have
//!   written its commit frame (the WAL fsync can fail after the write), so the batch may
//!   reappear after a restart. A commit error from SQLite (`TxnError::Sqlite`) is taken as
//!   such, whatever statement reported it. Every request of the batch gets `append_failed`,
//!   and the journal is poisoned: every later `append` and `tail` returns `append_failed` until the
//!   project is reopened, which re-reads the durable tail. `tail` checks the flag again after
//!   taking the store lock, so a `tail` that waited for a batch whose COMMIT failed does not
//!   report the unknown state. Callers resolve the outcome after
//!   the reopen with `tail()` or by retrying with `expected_prev`.
//! - **Payload.** Schema v1 has no `payloads` table; the payload (salts and committed
//!   values, RFC 8785) is a content-addressed blob referenced from `entry_blobs` with role
//!   [`ROLE_PAYLOAD`], so it can be detached later without touching the header
//!   (`CORE.JRN.ENTRY` REQ-010). Referenced blobs get role [`ROLE_REF`].
//! - **Bounds.** At most [`MAX_QUEUE`] waiting requests (later callers wait), [`MAX_BATCH`]
//!   per batch, [`MAX_BATCH_BYTES`] of staged payloads and headers per transaction (plus at
//!   most one request), [`MAX_BLOBS`] referenced blobs per entry, [`MAX_HEADER_BYTES`] of
//!   header and [`CHUNK_SIZE`] of payload (`input_limit` beyond). No panics: lock poisoning
//!   is recovered, every arithmetic step is checked.
//! - **Borrowed store (REQ-008).** [`Journal::on_store`] appends on a [`Store`] the caller
//!   lends (for example the copy [`Store::copy_to`] hands its `finish`), through the same
//!   engine ([`Core`]) as [`Journal`]: same checks, rows, commit and poison rules, after
//!   proving the store is writable and holds its write lock ([`Store::claim_append`]). See
//!   module `borrowed`.

use crate::entry::{
    build, parse_digest, verify_header_value, EntryDraft, EntryError, OsSalts, RecordedSalts, FIRST_SEQ, GENESIS_PREV,
};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, Digest, MAX_DEPTH};
use fid_store::cas::{address_of, Address, BlobStore, CHUNK_SIZE};
use fid_store::{Locked, ProjectLocation, Store, Txn, TxnError, Value};
use fid_types::Digest as StoreDigest;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod borrowed;
pub use borrowed::JournalRef;

/// The hash of an appended entry.
pub type EntryHash = Digest;

/// Most requests waiting for a commit; a caller beyond it waits for room.
pub const MAX_QUEUE: usize = 1024;
/// Most requests taken from the queue as one batch (group commit).
pub const MAX_BATCH: usize = 256;
/// Staged payload and header bytes after which a transaction takes no further request; the
/// rest of the batch goes into the next transaction.
pub const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;
/// Most referenced blobs of one entry.
pub const MAX_BLOBS: usize = 4096;
/// Largest canonical header, in bytes.
pub const MAX_HEADER_BYTES: usize = 1024 * 1024;
/// `entry_blobs.role` of the payload blob.
pub const ROLE_PAYLOAD: &str = "payload";
/// `entry_blobs.role` of a blob the entry references.
pub const ROLE_REF: &str = "ref";
/// `PRAGMA synchronous` value for FULL.
pub const SYNCHRONOUS_FULL: i64 = 2;
/// Most seconds an entry's `time.utc` may lie after the host clock.
pub const MAX_AHEAD_SECS: u64 = 300;

// ------------------------------------------------------------------ errors

/// Typed errors with stable codes ([`JournalError::code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// A storage step failed (or a fault was injected); nothing of the entry is visible.
    AppendFailed(String),
    /// A referenced blob (or a chunk of it) is not in the blob store.
    MissingBlob(Digest),
    /// The draft or the request is invalid (entry schema, wrong stream, duplicate blob).
    Schema(String),
    /// `expected_prev` is not the tail hash: another append landed first.
    HeadMoved { expected: Digest, found: Digest },
    /// The entry's `time.utc` is earlier than the chain tail's.
    TimeRegressed { head: String, entry: String },
    /// The entry's `time.utc` is more than [`MAX_AHEAD_SECS`] after the host clock.
    TimeAhead { entry: String, now_unix: u64 },
    /// The stored chain does not verify at its tail, or holds another stream.
    ChainInvalid(String),
    /// The header, payload or blob list is over its bound.
    InputLimit(String),
    /// A checkpoint was asked for a stream with no entries (`CORE.JRN.CHAIN.CKPT`).
    EmptyStream(String),
    /// [`Journal::on_store`]: the lent store is not opened for writing, or no longer holds
    /// its project write lock.
    ReadOnly(String),
}

impl JournalError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            JournalError::AppendFailed(_) => "append_failed",
            JournalError::MissingBlob(_) => "missing_blob",
            JournalError::Schema(_) => "schema",
            JournalError::HeadMoved { .. } => "head_moved",
            JournalError::TimeRegressed { .. } => "time_regressed",
            JournalError::TimeAhead { .. } => "time_ahead",
            JournalError::ChainInvalid(_) => "chain_invalid",
            JournalError::InputLimit(_) => "input_limit",
            JournalError::EmptyStream(_) => "empty_stream",
            JournalError::ReadOnly(_) => "read_only",
        }
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::AppendFailed(m) => write!(f, "append_failed: {m}"),
            JournalError::MissingBlob(d) => write!(f, "missing_blob: {d}"),
            JournalError::Schema(m) => write!(f, "schema: {m}"),
            JournalError::HeadMoved { expected, found } => {
                write!(f, "head_moved: expected prev {expected}, chain head is {found}")
            }
            JournalError::TimeRegressed { head, entry } => {
                write!(f, "time_regressed: entry time {entry} is earlier than chain head time {head}")
            }
            JournalError::TimeAhead { entry, now_unix } => write!(
                f,
                "time_ahead: entry time {entry} is more than {MAX_AHEAD_SECS} s after the host clock (Unix {now_unix})"
            ),
            JournalError::ChainInvalid(m) => write!(f, "chain_invalid: {m}"),
            JournalError::InputLimit(m) => write!(f, "input_limit: {m}"),
            JournalError::EmptyStream(s) => write!(f, "empty_stream: stream {s:?} has no entries"),
            JournalError::ReadOnly(m) => write!(f, "read_only: {m}"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<EntryError> for JournalError {
    fn from(e: EntryError) -> Self {
        match e {
            EntryError::RngUnavailable => JournalError::AppendFailed(e.to_string()),
            other => JournalError::Schema(other.to_string()),
        }
    }
}

pub(crate) fn failed(what: &str, e: impl fmt::Display) -> JournalError {
    JournalError::AppendFailed(format!("{what}: {e}"))
}

// ------------------------------------------------------------------ requests and the trait

/// One append: the draft, the blobs it references and the optional concurrency guard.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendRequest {
    pub draft: EntryDraft,
    /// Digests of blobs the entry references; each must already be in the blob store.
    pub blobs: Vec<Digest>,
    /// If set, append only if the chain tail hash is this ([`GENESIS_PREV`] for an empty
    /// chain); otherwise `head_moved`.
    pub expected_prev: Option<Digest>,
    /// Recorded salts (replay); `None` draws fresh ones from the OS.
    pub salts: Option<RecordedSalts>,
}

impl AppendRequest {
    /// A request with no referenced blob, no guard and fresh salts.
    pub fn new(draft: EntryDraft) -> Self {
        AppendRequest { draft, blobs: Vec::new(), expected_prev: None, salts: None }
    }
}

/// The append seam the bus injects (`CORE.BUS.PRIN`, `CORE.BUS.PIPE`). `&self`, `Send` and
/// `Sync`, so one journal can serve concurrent callers behind an `Arc`. An error means
/// nothing was appended.
pub trait Append: Send + Sync {
    /// Appends one entry and returns its hash once it is durable.
    fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError>;

    /// The verified chain tail (`None` for an empty chain), from which a caller builds
    /// `expected_prev` ([`GENESIS_PREV`] for an empty chain).
    fn tail(&self) -> Result<Option<Tail>, JournalError>;
}

impl<T: Append + ?Sized> Append for Arc<T> {
    fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        (**self).append(req)
    }

    fn tail(&self) -> Result<Option<Tail>, JournalError> {
        (**self).tail()
    }
}

impl<T: Append + ?Sized> Append for &T {
    fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        (**self).append(req)
    }

    fn tail(&self) -> Result<Option<Tail>, JournalError> {
        (**self).tail()
    }
}

/// The host clock the time-ahead rule reads.
pub trait Clock: Send + Sync {
    /// Time since the Unix epoch.
    fn now(&self) -> Duration;
}

/// The system clock. A clock before the Unix epoch reads as the epoch, which refuses every
/// entry as `time_ahead` (fail closed).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO)
    }
}

// ------------------------------------------------------------------ fault injection points

macro_rules! points {
    ($($variant:ident => $n:literal, $s:literal;)+) => {
        /// The numbered points inside an append, in the order a single successful append
        /// passes them (fixture `fixtures/core/jrn/chain/append/crash/points.json`). The COMMIT
        /// ([`CrashPoint::Committed`]) is the only point at which an entry becomes visible.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum CrashPoint {
            $($variant,)+
        }

        impl CrashPoint {
            /// Every point, in order.
            pub const ALL: &'static [CrashPoint] = &[$(CrashPoint::$variant),+];

            /// The point's number, 1-based.
            pub fn number(self) -> u32 {
                match self {
                    $(CrashPoint::$variant => $n,)+
                }
            }

            /// The point's name in the fixture.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(CrashPoint::$variant => $s,)+
                }
            }
        }
    };
}

points! {
    Prechecked => 1, "prechecked";
    BatchTaken => 2, "batch_taken";
    LockVerified => 3, "lock_verified";
    Begun => 4, "begun";
    TailRead => 5, "tail_read";
    HeadChecked => 6, "head_checked";
    Built => 7, "built";
    BlobsChecked => 8, "blobs_checked";
    PayloadEncoded => 9, "payload_encoded";
    PayloadPut => 10, "payload_put";
    SavepointOpened => 11, "savepoint_opened";
    EntryInserted => 12, "entry_inserted";
    PayloadRefInserted => 13, "payload_ref_inserted";
    BlobRefsInserted => 14, "blob_refs_inserted";
    SavepointReleased => 15, "savepoint_released";
    BatchBuilt => 16, "batch_built";
    BeforeCommit => 17, "before_commit";
    Committed => 18, "committed";
    ResultsPublished => 19, "results_published";
    Returning => 20, "returning";
}

impl CrashPoint {
    /// True for points before the COMMIT, where an injected fault makes the append fail.
    pub fn before_commit(self) -> bool {
        self < CrashPoint::Committed
    }
}

/// A test hook called at every [`CrashPoint`]: `true` continues, `false` injects a failure.
/// Before the COMMIT the append fails and rolls back. At [`CrashPoint::Committed`] it
/// simulates a COMMIT that reported an error after writing (outcome unknown: the journal is
/// poisoned). Later refusals are ignored. A crash harness exits the process from it.
#[cfg(feature = "test-hooks")]
pub type FaultHook = Arc<dyn Fn(CrashPoint) -> bool + Send + Sync>;

// ------------------------------------------------------------------ time order

/// Days from 1970-01-01 to the given proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `(Unix seconds, nanoseconds)` of a valid RFC 3339 UTC instant
/// `YYYY-MM-DDThh:mm:ss[.f]Z` with 1 to 9 fraction digits: month 1-12, a day that exists in
/// that month, hour 0-23, minute 0-59, second 0-59, and 60 only at 23:59:60 (a leap second,
/// which equals the next day's 00:00:00). `None` for anything else.
pub fn utc_instant(s: &str) -> Option<(i64, u32)> {
    let b = s.as_bytes();
    if b.len() < 20 || b.last() != Some(&b'Z') {
        return None;
    }
    let mut parts = [0i64; 6];
    let mut k = 0;
    for (i, c) in b"dddd-dd-ddTdd:dd:dd".iter().enumerate() {
        let got = *b.get(i)?;
        if *c != b'd' {
            if got != *c {
                return None;
            }
            k += 1;
        } else if got.is_ascii_digit() {
            let slot = parts.get_mut(k)?;
            *slot = *slot * 10 + i64::from(got - b'0');
        } else {
            return None;
        }
    }
    let [y, mo, d, h, mi, se] = parts;
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) {
        return None;
    }
    if h > 23 || mi > 59 || se > 60 || (se == 60 && (h, mi) != (23, 59)) {
        return None;
    }
    let frac = b.get(19..b.len() - 1)?;
    let mut nanos: u32 = 0;
    if !frac.is_empty() {
        let digits = frac.get(1..)?;
        if frac.first() != Some(&b'.') || digits.is_empty() || digits.len() > 9 {
            return None;
        }
        for i in 0..9 {
            let dg = match digits.get(i) {
                Some(c) if c.is_ascii_digit() => u32::from(c - b'0'),
                Some(_) => return None,
                None => 0,
            };
            nanos = nanos * 10 + dg;
        }
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se;
    Some((secs, nanos))
}

/// Orders two RFC 3339 UTC instants as instants (`...:00Z` equals `...:00.000Z` and is
/// earlier than `...:00.5Z`); `None` if either is not a valid instant ([`utc_instant`]).
pub fn utc_order(a: &str, b: &str) -> Option<Ordering> {
    Some(utc_instant(a)?.cmp(&utc_instant(b)?))
}

/// True if `entry` lies more than [`MAX_AHEAD_SECS`] after `now` (time since the epoch).
fn too_far_ahead(entry: (i64, u32), now: Duration) -> bool {
    let at = i128::from(entry.0) * 1_000_000_000 + i128::from(entry.1);
    let now = i128::try_from(now.as_nanos()).unwrap_or(i128::MAX);
    let limit = now.saturating_add(i128::from(MAX_AHEAD_SECS) * 1_000_000_000);
    at > limit
}

// ------------------------------------------------------------------ the journal

/// The chain tail of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    pub seq: u64,
    pub hash: Digest,
    /// `time.utc` of the tail entry.
    pub utc: String,
}

/// Durability settings of the journal's connection, as read back from SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Durability {
    pub synchronous: i64,
    pub fullfsync: i64,
    pub checkpoint_fullfsync: i64,
}

struct Pending {
    ticket: u64,
    req: AppendRequest,
}

type Outcome = Result<EntryHash, JournalError>;

#[derive(Default)]
struct Queue {
    waiting: VecDeque<Pending>,
    done: BTreeMap<u64, Outcome>,
    leader: bool,
    next: u64,
}

/// The chained stream of one project database. `Send + Sync`: share it behind an `Arc`.
pub struct Journal {
    pub(crate) stream: String,
    pub(crate) clock: Arc<dyn Clock>,
    /// Set after a COMMIT whose outcome is unknown; cleared only by reopening.
    poisoned: AtomicBool,
    /// The open project (`fid-store`): connection, write lock and blob store.
    pub(crate) store: Store,
    /// The store lock: held by a batch from its tail read to its last COMMIT, by `tail`, and
    /// by the checkpoint and head-cache writers (module `checkpoint`).
    pub(crate) gate: Mutex<()>,
    queue: Mutex<Queue>,
    changed: Condvar,
    #[cfg(feature = "test-hooks")]
    hook: Mutex<Option<FaultHook>>,
}

impl fmt::Debug for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Journal").field("stream", &self.stream).field("root", &self.store.location().root()).finish()
    }
}

pub(crate) fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn is_ident(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn to_store(d: &Digest) -> StoreDigest {
    StoreDigest::from_bytes(d.0)
}

fn from_store(d: &StoreDigest) -> Digest {
    Digest(d.to_bytes())
}

/// The append engine over one store, shared by [`Journal`] (owned store) and
/// [`JournalRef`] (borrowed store), so both paths run the same checks, staging, commit and
/// poison rules and write the same bytes.
pub(crate) struct Core<'a> {
    pub(crate) store: &'a Store,
    pub(crate) stream: &'a str,
    pub(crate) clock: &'a dyn Clock,
    pub(crate) poisoned: &'a AtomicBool,
    /// The store lock (see [`Journal`]'s `gate`).
    pub(crate) gate: &'a Mutex<()>,
    #[cfg(feature = "test-hooks")]
    pub(crate) hook: Option<&'a Mutex<Option<FaultHook>>>,
}

/// One request's rows and payload, staged before they enter the batch transaction.
struct Staged {
    entry: Vec<(&'static str, Value)>,
    refs: Vec<(&'static str, String)>,
    hash: String,
}

impl Journal {
    /// Opens the chain of `stream` in a project opened for writing: opens the project through
    /// [`fid_store::Store`] (`project.db`, migrating a new project, with the durability
    /// pragmas set and read back), and checks the stored chain: sequences 1..n without gaps,
    /// one stream, every `prev_hash` linking to the `hash` below it, and a tail that verifies.
    /// The journal keeps the write lock for its lifetime.
    pub fn open(location: ProjectLocation, lock: Locked, stream: &str) -> Result<Journal, JournalError> {
        Journal::open_with_clock(location, lock, stream, Arc::new(SystemClock))
    }

    /// [`Journal::open`] with an injected host clock for the time-ahead rule.
    pub fn open_with_clock(
        location: ProjectLocation,
        lock: Locked,
        stream: &str,
        clock: Arc<dyn Clock>,
    ) -> Result<Journal, JournalError> {
        check_stream_name(stream)?;
        let store = Store::open_locked(location, lock).map_err(|e| failed("open project", e))?;
        check_chain(&store, stream)?;
        Ok(Journal {
            stream: stream.to_string(),
            clock,
            poisoned: AtomicBool::new(false),
            store,
            gate: Mutex::new(()),
            queue: Mutex::new(Queue::default()),
            changed: Condvar::new(),
            #[cfg(feature = "test-hooks")]
            hook: Mutex::new(None),
        })
    }

    /// The shared append engine over this journal's store.
    pub(crate) fn core(&self) -> Core<'_> {
        Core {
            store: &self.store,
            stream: &self.stream,
            clock: &*self.clock,
            poisoned: &self.poisoned,
            gate: &self.gate,
            #[cfg(feature = "test-hooks")]
            hook: Some(&self.hook),
        }
    }

    /// The stream this journal appends to.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// The project.
    pub fn location(&self) -> &ProjectLocation {
        self.store.location()
    }

    /// The project's blob store (callers put referenced blobs here before appending).
    pub fn blobs(&self) -> &BlobStore {
        self.store.blobs()
    }

    /// The current chain tail, verified; `None` for an empty chain.
    pub fn tail(&self) -> Result<Option<Tail>, JournalError> {
        self.core().tail()
    }

    /// The connection's durability pragmas as SQLite reports them.
    pub fn durability(&self) -> Result<Durability, JournalError> {
        self.store
            .read(|conn| {
                let read = |p: &str| conn.query_row(&format!("PRAGMA {p}"), [], |r| r.get::<_, i64>(0));
                Ok(Durability {
                    synchronous: read("synchronous")?,
                    fullfsync: read("fullfsync")?,
                    checkpoint_fullfsync: read("checkpoint_fullfsync")?,
                })
            })
            .map_err(|e| failed("durability pragmas", e))
    }

    /// Installs (or removes) the fault hook called at every [`CrashPoint`].
    #[cfg(feature = "test-hooks")]
    pub fn set_fault_hook(&self, hook: Option<FaultHook>) {
        *guard(&self.hook) = hook;
    }

    /// True once a COMMIT's outcome is unknown; only a reopen clears it.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.load(AtomicOrdering::SeqCst)
    }

    pub(crate) fn check_poison(&self) -> Result<(), JournalError> {
        self.core().check_poison()
    }

    /// Appends one entry (see the module documentation for the pinned rules).
    pub fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        self.core().precheck(&req)?;
        self.core().at(CrashPoint::Prechecked)?;
        let mut q = guard(&self.queue);
        while q.waiting.len() >= MAX_QUEUE {
            q = self.changed.wait(q).unwrap_or_else(PoisonError::into_inner);
        }
        let ticket = q.next;
        q.next = q.next.wrapping_add(1);
        q.waiting.push_back(Pending { ticket, req });
        loop {
            if let Some(outcome) = q.done.remove(&ticket) {
                drop(q);
                if outcome.is_ok() {
                    let _ = self.core().at(CrashPoint::Returning);
                }
                return outcome;
            }
            if q.leader {
                q = self.changed.wait(q).unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            q.leader = true;
            let n = q.waiting.len().min(MAX_BATCH);
            let batch: Vec<Pending> = q.waiting.drain(..n).collect();
            drop(q);
            // Room in the queue for callers waiting to enqueue.
            self.changed.notify_all();
            let outcomes = self.commit_batch(&batch);
            let any_ok = outcomes.iter().any(|(_, o)| o.is_ok());
            q = guard(&self.queue);
            q.done.extend(outcomes);
            q.leader = false;
            self.changed.notify_all();
            if any_ok {
                let _ = self.core().at(CrashPoint::ResultsPublished);
            }
        }
    }

    /// Runs one batch in as few transactions as [`MAX_BATCH_BYTES`] allows (one, unless the
    /// payloads are large); one outcome per request, in arrival order.
    fn commit_batch(&self, batch: &[Pending]) -> Vec<(u64, Outcome)> {
        let core = self.core();
        let _gate = guard(core.gate);
        let mut outcomes = Vec::with_capacity(batch.len());
        let mut rest = batch;
        while !rest.is_empty() {
            match core.transaction(rest) {
                Ok((done, used)) => {
                    outcomes.extend(done);
                    rest = rest.get(used..).unwrap_or_default();
                }
                Err(e) => {
                    outcomes.extend(rest.iter().map(|p| (p.ticket, Err(e.clone()))));
                    rest = &[];
                }
            }
        }
        outcomes
    }
}

impl Core<'_> {
    /// The current chain tail, verified; `None` for an empty chain (see [`Journal::tail`]).
    pub(crate) fn tail(&self) -> Result<Option<Tail>, JournalError> {
        self.check_poison()?;
        let _gate = guard(self.gate);
        // Again under the lock: a batch that held it may have poisoned the journal meanwhile,
        // and the tail it leaves is not known to be the durable one.
        self.check_poison()?;
        read_tail(self.store, self.stream)
    }

    /// `append_failed` while poisoned (a COMMIT's outcome is unknown).
    pub(crate) fn check_poison(&self) -> Result<(), JournalError> {
        if self.poisoned.load(AtomicOrdering::SeqCst) {
            return Err(JournalError::AppendFailed(
                "journal poisoned: an earlier COMMIT failed with an unknown outcome; reopen the project".into(),
            ));
        }
        Ok(())
    }

    /// Calls the hook at `p`: `false` means the hook refused.
    fn hook_refuses(&self, p: CrashPoint) -> bool {
        #[cfg(feature = "test-hooks")]
        {
            let hook = self.hook.and_then(|h| guard(h).clone());
            if let Some(h) = hook {
                return !h(p);
            }
        }
        #[cfg(not(feature = "test-hooks"))]
        let _ = p;
        false
    }

    /// Calls the hook at `p`; a refusal before the COMMIT is an injected `append_failed`.
    fn at(&self, p: CrashPoint) -> Result<(), JournalError> {
        if self.hook_refuses(p) && p.before_commit() {
            return Err(JournalError::AppendFailed(format!("injected fault at {}", p.as_str())));
        }
        Ok(())
    }

    /// Checks done before queueing: stream, blob list.
    pub(crate) fn precheck(&self, req: &AppendRequest) -> Result<(), JournalError> {
        if req.draft.stream != self.stream {
            return Err(JournalError::Schema(format!(
                "draft.stream {:?} is not this journal's stream {:?}",
                req.draft.stream, self.stream
            )));
        }
        if req.blobs.len() > MAX_BLOBS {
            return Err(JournalError::InputLimit(format!("{} referenced blobs, at most {MAX_BLOBS}", req.blobs.len())));
        }
        let mut seen = BTreeSet::new();
        for d in &req.blobs {
            if !seen.insert(d.0) {
                return Err(JournalError::Schema(format!("blob {d} is listed twice")));
            }
        }
        Ok(())
    }

    /// One transaction over a prefix of `rest`: the outcomes of the requests it took and how
    /// many it took (at least one). `Err` fails every request of `rest`; nothing of them is
    /// visible unless the error says the outcome is unknown.
    pub(crate) fn transaction(&self, rest: &[Pending]) -> Result<(Vec<(u64, Outcome)>, usize), JournalError> {
        self.check_poison()?;
        self.at(CrashPoint::BatchTaken)?;
        // The write lock is verified by `Txn::commit`, under the store's commit mutex.
        self.at(CrashPoint::LockVerified)?;
        let mut txn = self.store.transaction();
        self.at(CrashPoint::Begun)?;
        let mut tail = read_tail(self.store, self.stream)?;
        self.at(CrashPoint::TailRead)?;
        let mut outcomes = Vec::with_capacity(rest.len());
        let mut staged_bytes: usize = 0;
        for p in rest {
            if staged_bytes >= MAX_BATCH_BYTES {
                break;
            }
            match self.one(&mut txn, tail.as_ref(), &p.req)? {
                Ok((next, bytes)) => {
                    outcomes.push((p.ticket, Ok(next.hash)));
                    tail = Some(next);
                    staged_bytes = staged_bytes.saturating_add(bytes);
                }
                Err(e) => outcomes.push((p.ticket, Err(e))),
            }
        }
        let used = outcomes.len();
        self.at(CrashPoint::BatchBuilt)?;
        if !outcomes.iter().any(|(_, o)| o.is_ok()) {
            txn.abort();
            return Ok((outcomes, used));
        }
        self.at(CrashPoint::BeforeCommit)?;
        let committed = txn.commit();
        // A refusal at `Committed` simulates a COMMIT that wrote its frame and then reported
        // an error (test hook).
        let injected = self.hook_refuses(CrashPoint::Committed);
        let why = match committed {
            Ok(_) if !injected => return Ok((outcomes, used)),
            Ok(_) => "injected fault at committed".to_string(),
            Err(TxnError::Sqlite(e)) => e.to_string(),
            Err(e) => return Err(commit_error(e)),
        };
        // Outcome unknown: the batch may be durable. Poison until a reopen.
        self.poisoned.store(true, AtomicOrdering::SeqCst);
        Err(JournalError::AppendFailed(format!("commit failed, outcome unknown until the project is reopened: {why}")))
    }

    /// One request: checks, builds and stages it into `txn`. Outer `Err`: abort the
    /// transaction. Inner `Err`: this request is refused and staged nothing. On success the
    /// new tail and the bytes staged for it (payload and header).
    fn one(
        &self,
        txn: &mut Txn<'_>,
        tail: Option<&Tail>,
        req: &AppendRequest,
    ) -> Result<Result<(Tail, usize), JournalError>, JournalError> {
        let (seq, prev) = match tail {
            None => (FIRST_SEQ, GENESIS_PREV),
            Some(t) => match t.seq.checked_add(1) {
                Some(s) if s <= i64::MAX as u64 => (s, t.hash),
                _ => return Ok(Err(JournalError::AppendFailed("sequence numbers exhausted".into()))),
            },
        };
        if let Some(expected) = req.expected_prev {
            if expected != prev {
                return Ok(Err(JournalError::HeadMoved { expected, found: prev }));
            }
        }
        let Some(at) = utc_instant(&req.draft.time.utc) else {
            return Ok(Err(JournalError::Schema(format!(
                "draft.time.utc {:?} is not a valid RFC 3339 UTC instant",
                req.draft.time.utc
            ))));
        };
        let now = self.clock.now();
        if too_far_ahead(at, now) {
            return Ok(Err(JournalError::TimeAhead { entry: req.draft.time.utc.clone(), now_unix: now.as_secs() }));
        }
        if let Some(t) = tail {
            match utc_order(&req.draft.time.utc, &t.utc) {
                None => return Ok(Err(JournalError::ChainInvalid(format!("tail time {:?} is not valid", t.utc)))),
                Some(Ordering::Less) => {
                    return Ok(Err(JournalError::TimeRegressed {
                        head: t.utc.clone(),
                        entry: req.draft.time.utc.clone(),
                    }))
                }
                Some(_) => {}
            }
        }
        if let Err(e) = self.at(CrashPoint::HeadChecked) {
            return Ok(Err(e));
        }
        let built = match &req.salts {
            Some(s) => build(&req.draft, seq, prev, &mut s.clone()),
            None => build(&req.draft, seq, prev, &mut OsSalts),
        };
        let entry = match built {
            Ok(e) => e,
            Err(e) => return Ok(Err(e.into())),
        };
        let header = match to_canonical_bytes(&entry.header.to_value()) {
            Ok(h) => h,
            Err(e) => return Ok(Err(JournalError::Schema(e.to_string()))),
        };
        if header.len() > MAX_HEADER_BYTES {
            return Ok(Err(JournalError::InputLimit(format!(
                "header of {} bytes, at most {MAX_HEADER_BYTES}",
                header.len()
            ))));
        }
        if let Err(e) = self.at(CrashPoint::Built) {
            return Ok(Err(e));
        }
        // Present and sound now; `Txn::commit` checks again and flushes them.
        for d in &req.blobs {
            match txn.check_blob(&to_store(d)) {
                Ok(()) => {}
                Err(TxnError::DanglingBlob(m) | TxnError::CorruptBlob(m)) => {
                    return Ok(Err(JournalError::MissingBlob(from_store(&m))))
                }
                Err(e) => return Ok(Err(failed(&format!("check blob {d}"), e))),
            }
        }
        if let Err(e) = self.at(CrashPoint::BlobsChecked) {
            return Ok(Err(e));
        }
        let payload = match to_canonical_bytes(&entry.payload.to_value()) {
            Ok(p) => p,
            Err(e) => return Ok(Err(JournalError::Schema(e.to_string()))),
        };
        if payload.len() as u64 > CHUNK_SIZE {
            return Ok(Err(JournalError::InputLimit(format!(
                "payload of {} bytes, at most {CHUNK_SIZE}",
                payload.len()
            ))));
        }
        if let Err(e) = self.at(CrashPoint::PayloadEncoded) {
            return Ok(Err(e));
        }
        let payload_digest = match address_of(&payload) {
            Ok(Address::Blob(b)) => b.digest,
            Ok(Address::Object(_)) => {
                return Ok(Err(JournalError::AppendFailed("payload would be stored as a chunked object".into())))
            }
            Err(e) => return Ok(Err(failed("address payload", e))),
        };
        if let Err(e) = self.at(CrashPoint::PayloadPut) {
            return Ok(Err(e));
        }
        let hash = entry.header.hash;
        let stage = || -> Result<Staged, JournalError> {
            self.at(CrashPoint::SavepointOpened)?;
            let seq_i = i64::try_from(seq).map_err(|_| JournalError::AppendFailed("sequence out of range".into()))?;
            let entry_row = vec![
                ("seq", Value::Integer(seq_i)),
                ("hash", Value::Text(hash.to_string())),
                ("stream", Value::Text(entry.header.stream.clone())),
                ("branch", Value::Text(entry.header.branch.clone())),
                ("kind", Value::from(entry.header.kind().as_str())),
                ("prev_hash", Value::Text(prev.to_string())),
                ("header", Value::Blob(header.clone())),
            ];
            self.at(CrashPoint::EntryInserted)?;
            let mut refs = Vec::with_capacity(req.blobs.len().saturating_add(1));
            refs.push((ROLE_PAYLOAD, payload_digest.as_str().to_string()));
            self.at(CrashPoint::PayloadRefInserted)?;
            refs.extend(req.blobs.iter().map(|d| (ROLE_REF, d.to_string())));
            self.at(CrashPoint::BlobRefsInserted)?;
            Ok(Staged { entry: entry_row, refs, hash: hash.to_string() })
        };
        let staged = match stage() {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let bytes = payload.len().saturating_add(header.len());
        // Into the transaction: from here on the rows belong to the batch, and a fault aborts it.
        let put = txn.put_blob(payload).map_err(|e| failed("stage payload", e))?;
        if put.digest() != payload_digest {
            return Err(JournalError::AppendFailed("payload address changed while staging".into()));
        }
        txn.insert("entries", &staged.entry);
        for (role, digest) in staged.refs {
            txn.insert(
                "entry_blobs",
                &[
                    ("entry_hash", Value::Text(staged.hash.clone())),
                    ("role", Value::from(role)),
                    ("digest", Value::Text(digest)),
                ],
            );
        }
        self.at(CrashPoint::SavepointReleased)?;
        Ok(Ok((Tail { seq, hash, utc: req.draft.time.utc.clone() }, bytes)))
    }
}

/// Refuses a stream name that is not a lowercase identifier (`schema`).
pub(crate) fn check_stream_name(stream: &str) -> Result<(), JournalError> {
    if !is_ident(stream) {
        return Err(JournalError::Schema(format!("stream {stream:?} is not a lowercase identifier")));
    }
    Ok(())
}

/// The chain checks of [`Journal::open`] on an opened store: sequences 1..n without gaps,
/// one stream, every `prev_hash` linking to the `hash` below it, and a tail that verifies.
pub(crate) fn check_chain(store: &Store, stream: &str) -> Result<(), JournalError> {
    let (count, min, max, others) = store
        .read(|conn| {
            let (count, min, max): (i64, i64, i64) = conn.query_row(
                "SELECT count(*), coalesce(min(seq), 0), coalesce(max(seq), 0) FROM entries",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let others: i64 =
                conn.query_row("SELECT count(*) FROM entries WHERE stream <> ?1", [stream], |r| r.get(0))?;
            Ok((count, min, max, others))
        })
        .map_err(|e| failed("read chain", e))?;
    if count != max || (count > 0 && min != 1) {
        return Err(JournalError::ChainInvalid(format!(
            "sequences are not 1..{count} without gaps (min {min}, max {max})"
        )));
    }
    if others != 0 {
        return Err(JournalError::ChainInvalid(format!(
            "the database holds {others} entries of another stream than {stream:?}"
        )));
    }
    walk_chain(store)?;
    read_tail(store, stream)?;
    Ok(())
}

/// A commit error whose outcome is known: the transaction rolled back, nothing is visible.
fn commit_error(e: TxnError) -> JournalError {
    match e {
        // A referenced blob vanished or went bad between the request's check and the commit.
        TxnError::DanglingBlob(d) | TxnError::CorruptBlob(d) => JournalError::MissingBlob(from_store(&d)),
        TxnError::AppendOnly => JournalError::AppendFailed("commit: refused as append_only (row exists)".into()),
        other => failed("commit", other),
    }
}

impl Append for Journal {
    fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        Journal::append(self, req)
    }

    fn tail(&self) -> Result<Option<Tail>, JournalError> {
        Journal::tail(self)
    }
}

/// Walks the `prev_hash` and `hash` columns of the whole chain once, in sequence order,
/// without re-hashing: sequence 1 links to [`GENESIS_PREV`], every later row to the row
/// below it. A fork at any depth is `chain_invalid`. Streams the rows: constant memory.
fn walk_chain(store: &Store) -> Result<(), JournalError> {
    store
        .read(|conn| {
            let mut st = conn.prepare("SELECT seq, hash, prev_hash FROM entries ORDER BY seq")?;
            let mut rows = st.query([])?;
            let mut want_seq: i64 = 1;
            let mut below = GENESIS_PREV;
            while let Some(r) = rows.next()? {
                let seq: i64 = r.get(0)?;
                let hash: String = r.get(1)?;
                let prev: Option<String> = r.get(2)?;
                let bad = |why: String| Err(JournalError::ChainInvalid(format!("entry {seq}: {why}")));
                if seq != want_seq {
                    return Ok(bad(format!("expected sequence {want_seq}")));
                }
                let Some(prev) = prev.as_deref().and_then(parse_digest) else {
                    return Ok(bad("prev_hash column is not a digest".into()));
                };
                if prev != below {
                    return Ok(if seq == 1 {
                        bad("prev_hash is not the genesis prev".into())
                    } else {
                        bad(format!("prev_hash does not link to the hash of entry {}", seq - 1))
                    });
                }
                let Some(hash) = parse_digest(&hash) else {
                    return Ok(bad("hash column is not a digest".into()));
                };
                below = hash;
                want_seq = match want_seq.checked_add(1) {
                    Some(n) => n,
                    None => return Ok(bad("sequence numbers exhausted".into())),
                };
            }
            Ok(Ok(()))
        })
        .map_err(|e| failed("walk chain", e))?
}

/// The tail row: `seq, hash, stream, branch, kind, prev_hash, header`.
type TailRow = (i64, String, String, String, String, Option<String>, Vec<u8>);

/// Reads and verifies the chain tail: the row's columns against its header, the header's
/// hash, the genesis rule, the link to the entry below, and the stream.
pub(crate) fn read_tail(store: &Store, stream: &str) -> Result<Option<Tail>, JournalError> {
    let read = store
        .read(|conn| {
            let mut st = conn.prepare(
                "SELECT seq, hash, stream, branch, kind, prev_hash, header FROM entries ORDER BY seq DESC LIMIT 1",
            )?;
            let mut rows = st.query_map([], |r| -> Result<TailRow, _> {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            })?;
            let Some(row) = rows.next().transpose()? else { return Ok(None) };
            // The stored entry just below the tail (one indexed lookup), for the fork check.
            let below: Option<String> = if row.0 > 1 {
                let mut below_st = conn.prepare("SELECT hash FROM entries WHERE seq = ?1")?;
                let mut found = below_st.query_map([row.0 - 1], |r| r.get::<_, String>(0))?;
                found.next().transpose()?
            } else {
                None
            };
            Ok(Some((row, below)))
        })
        .map_err(|e| failed("read tail", e))?;
    let Some(((seq, hash, row_stream, row_branch, row_kind, prev, header), below)) = read else { return Ok(None) };
    let bad = |why: String| JournalError::ChainInvalid(format!("tail entry {seq}: {why}"));
    let seq_u = u64::try_from(seq).map_err(|_| bad("negative sequence".into()))?;
    if row_stream != stream {
        return Err(bad(format!("stream {row_stream:?}, expected {stream:?}")));
    }
    if header.len() > MAX_HEADER_BYTES {
        return Err(bad("header over the size bound".into()));
    }
    let value = parse_with_limits(&header, MAX_HEADER_BYTES, MAX_DEPTH).map_err(|e| bad(format!("header: {e}")))?;
    let recomputed = verify_header_value(&value).map_err(|e| bad(format!("header: {e}")))?;
    let column_hash = parse_digest(&hash).ok_or_else(|| bad("hash column is not a digest".into()))?;
    if recomputed != column_hash {
        return Err(bad("hash column differs from the header hash".into()));
    }
    let CValue::Obj(o) = &value else { return Err(bad("header is not an object".into())) };
    let text = |k: &str| match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.clone()),
        _ => Err(bad(format!("header.{k} is not a string"))),
    };
    if text("seq")? != seq_u.to_string() {
        return Err(bad("header.seq differs from the row".into()));
    }
    if text("stream")? != stream {
        return Err(bad("header.stream differs from the row".into()));
    }
    if text("branch")? != row_branch {
        return Err(bad("branch column differs from header.branch".into()));
    }
    if text("kind")? != row_kind {
        return Err(bad("kind column differs from header.kind".into()));
    }
    let header_prev = parse_digest(&text("prev")?).ok_or_else(|| bad("header.prev is not a digest".into()))?;
    if prev.as_deref().and_then(parse_digest) != Some(header_prev) {
        return Err(bad("prev_hash column differs from header.prev".into()));
    }
    if (seq_u == FIRST_SEQ) != (header_prev == GENESIS_PREV) {
        return Err(bad("a zero prev must be exactly at sequence 1".into()));
    }
    if seq_u > FIRST_SEQ && below.as_deref().and_then(parse_digest) != Some(header_prev) {
        return Err(bad(format!("prev does not link to the stored entry {}", seq - 1)));
    }
    let utc = match o.get("time") {
        Some(CValue::Obj(t)) => match t.get("utc") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(bad("header.time.utc is not a string".into())),
        },
        _ => return Err(bad("header.time is not an object".into())),
    };
    Ok(Some(Tail { seq: seq_u, hash: column_hash, utc }))
}
