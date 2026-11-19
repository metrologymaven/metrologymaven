//! Atomic durable append, sequence and immutability. Spec node `CORE.JRN.CHAIN.APPEND`.
//!
//! [`Journal::append`] is the single write path into a chained stream. It takes an
//! [`AppendRequest`] (an entry draft plus the blobs it references), assigns the next
//! sequence number and previous hash of the stream, builds and hashes the entry with
//! [`crate::entry::build`], and stores header, payload and blob references in one SQLite
//! transaction of the project database (`fid-store`). It returns the entry hash only after
//! the COMMIT has returned with `synchronous=FULL`. There is no update or delete API, and
//! the database's `append_only` triggers refuse both (`CORE.STO.SQL.SCHEMA`).
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
//!   own SAVEPOINT, so one request's refusal does not affect the others.
//! - **Atomicity and durability (REQ-003, REQ-004).** Order inside one append: referenced
//!   blobs are checked present (`missing_blob`) and flushed (file, fan-out directories and
//!   `sha256/`); the payload is put as a content-addressed blob with `PutOpts::durable`;
//!   then `entries` and `entry_blobs` rows are inserted and the transaction commits. The
//!   connection runs WAL with `synchronous=FULL` (the WAL is flushed at every COMMIT) and
//!   `fullfsync=ON` / `checkpoint_fullfsync=ON` (`F_FULLFSYNC` on macOS, no effect
//!   elsewhere); [`Journal::open`] sets and reads back all three. Every failure before the
//!   COMMIT rolls back, so after a crash or error the chain is at the old head; at worst an
//!   unreferenced payload blob remains. Nothing after a successful COMMIT can fail the call.
//! - **Failed COMMIT: outcome unknown.** A COMMIT that reports an error may still have
//!   written its commit frame (the WAL fsync can fail after the write), so the batch may
//!   reappear after a restart. Every request of the batch gets `append_failed`, and the
//!   journal is poisoned: every later `append` and `tail` returns `append_failed` until the
//!   project is reopened, which re-reads the durable tail. Callers resolve the outcome after
//!   the reopen with `tail()` or by retrying with `expected_prev`.
//! - **Payload.** Schema v1 has no `payloads` table; the payload (salts and committed
//!   values, RFC 8785) is a content-addressed blob referenced from `entry_blobs` with role
//!   [`ROLE_PAYLOAD`], so it can be detached later without touching the header
//!   (`CORE.JRN.ENTRY` REQ-010). Referenced blobs get role [`ROLE_REF`].
//! - **Bounds.** At most [`MAX_QUEUE`] waiting requests (later callers wait), [`MAX_BATCH`]
//!   per transaction, [`MAX_BLOBS`] referenced blobs per entry, [`MAX_HEADER_BYTES`] of
//!   header and [`CHUNK_SIZE`] of payload (`input_limit` beyond). No panics: lock poisoning
//!   is recovered, every arithmetic step is checked.

mod blobs;

use crate::entry::{
    build, parse_digest, verify_header_value, EntryDraft, EntryError, OsSalts, RecordedSalts, FIRST_SEQ, GENESIS_PREV,
};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, Digest, MAX_DEPTH};
use fid_store::cas::{Address, BlobStore, PutOpts, CHUNK_SIZE};
use fid_store::schema::{open_write, Database};
use fid_store::{Locked, ProjectLocation, SchemaError};
use fid_types::Digest as StoreDigest;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The hash of an appended entry.
pub type EntryHash = Digest;

/// Most requests waiting for a commit; a caller beyond it waits for room.
pub const MAX_QUEUE: usize = 1024;
/// Most requests committed in one transaction (group commit).
pub const MAX_BATCH: usize = 256;
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

fn failed(what: &str, e: impl fmt::Display) -> JournalError {
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

struct Store {
    db: Database,
    lock: Locked,
}

/// The chained stream of one project database. `Send + Sync`: share it behind an `Arc`.
pub struct Journal {
    stream: String,
    clock: Arc<dyn Clock>,
    /// Set after a COMMIT whose outcome is unknown; cleared only by reopening.
    poisoned: AtomicBool,
    location: ProjectLocation,
    blobs: BlobStore,
    store: Mutex<Store>,
    queue: Mutex<Queue>,
    changed: Condvar,
    #[cfg(feature = "test-hooks")]
    hook: Mutex<Option<FaultHook>>,
}

impl fmt::Debug for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Journal").field("stream", &self.stream).field("root", &self.location.root()).finish()
    }
}

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
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

impl Journal {
    /// Opens the chain of `stream` in a project opened for writing: opens `project.db`
    /// (`CORE.STO.SQL.SCHEMA`, migrating a new project), sets and reads back the durability
    /// pragmas, and checks the stored chain (sequences 1..n without gaps, one stream, a tail
    /// that verifies). The journal keeps the write lock for its lifetime.
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
        if !is_ident(stream) {
            return Err(JournalError::Schema(format!("stream {stream:?} is not a lowercase identifier")));
        }
        let db = open_write(&location, &lock).map_err(|e| failed("open project.db", e))?;
        let blobs = BlobStore::for_project(&location).map_err(|e| failed("open blob store", e))?;
        let conn = db.connection();
        conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")
            .map_err(|e| failed("durability pragmas", e))?;
        let journal = Journal {
            stream: stream.to_string(),
            clock,
            poisoned: AtomicBool::new(false),
            location,
            blobs,
            store: Mutex::new(Store { db, lock }),
            queue: Mutex::new(Queue::default()),
            changed: Condvar::new(),
            #[cfg(feature = "test-hooks")]
            hook: Mutex::new(None),
        };
        let d = journal.durability()?;
        if d != (Durability { synchronous: SYNCHRONOUS_FULL, fullfsync: 1, checkpoint_fullfsync: 1 }) {
            return Err(JournalError::AppendFailed(format!("durability pragmas did not take: {d:?}")));
        }
        {
            let store = guard(&journal.store);
            let conn = store.db.connection();
            let (count, min, max): (i64, i64, i64) = conn
                .query_row("SELECT count(*), coalesce(min(seq), 0), coalesce(max(seq), 0) FROM entries", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .map_err(|e| failed("read chain", e))?;
            if count != max || (count > 0 && min != 1) {
                return Err(JournalError::ChainInvalid(format!(
                    "sequences are not 1..{count} without gaps (min {min}, max {max})"
                )));
            }
            let others: i64 = conn
                .query_row("SELECT count(*) FROM entries WHERE stream <> ?1", [stream], |r| r.get(0))
                .map_err(|e| failed("read chain", e))?;
            if others != 0 {
                return Err(JournalError::ChainInvalid(format!(
                    "the database holds {others} entries of another stream than {stream:?}"
                )));
            }
            read_tail(&store.db, stream)?;
        }
        Ok(journal)
    }

    /// The stream this journal appends to.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// The project.
    pub fn location(&self) -> &ProjectLocation {
        &self.location
    }

    /// The project's blob store (callers put referenced blobs here before appending).
    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// The current chain tail, verified; `None` for an empty chain.
    pub fn tail(&self) -> Result<Option<Tail>, JournalError> {
        self.check_poison()?;
        let store = guard(&self.store);
        read_tail(&store.db, &self.stream)
    }

    /// The connection's durability pragmas as SQLite reports them.
    pub fn durability(&self) -> Result<Durability, JournalError> {
        let store = guard(&self.store);
        let conn = store.db.connection();
        let read = |p: &str| -> Result<i64, JournalError> {
            conn.query_row(&format!("PRAGMA {p}"), [], |r| r.get(0)).map_err(|e| failed(p, e))
        };
        Ok(Durability {
            synchronous: read("synchronous")?,
            fullfsync: read("fullfsync")?,
            checkpoint_fullfsync: read("checkpoint_fullfsync")?,
        })
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

    fn check_poison(&self) -> Result<(), JournalError> {
        if self.is_poisoned() {
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
            let hook = guard(&self.hook).clone();
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
    fn precheck(&self, req: &AppendRequest) -> Result<(), JournalError> {
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

    /// Appends one entry (see the module documentation for the pinned rules).
    pub fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        self.precheck(&req)?;
        self.at(CrashPoint::Prechecked)?;
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
                    let _ = self.at(CrashPoint::Returning);
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
            let outcomes = self.commit_batch(batch);
            let any_ok = outcomes.iter().any(|(_, o)| o.is_ok());
            q = guard(&self.queue);
            q.done.extend(outcomes);
            q.leader = false;
            self.changed.notify_all();
            if any_ok {
                let _ = self.at(CrashPoint::ResultsPublished);
            }
        }
    }

    /// Runs one batch in one transaction; one outcome per request, in arrival order.
    fn commit_batch(&self, batch: Vec<Pending>) -> Vec<(u64, Outcome)> {
        let fail_all = |batch: &[Pending], e: &JournalError| -> Vec<(u64, Outcome)> {
            batch.iter().map(|p| (p.ticket, Err(e.clone()))).collect()
        };
        let store = guard(&self.store);
        if let Err(e) = self.check_poison() {
            return fail_all(&batch, &e);
        }
        if let Err(e) = self.at(CrashPoint::BatchTaken) {
            return fail_all(&batch, &e);
        }
        if let Err(e) = store.lock.verify() {
            return fail_all(&batch, &failed("write lock", e));
        }
        if let Err(e) = self.at(CrashPoint::LockVerified) {
            return fail_all(&batch, &e);
        }
        let conn = store.db.connection();
        if !conn.is_autocommit() {
            // A transaction left open by a failed ROLLBACK: discard it before starting anew.
            let _ = conn.execute_batch("ROLLBACK");
        }
        if let Err(e) = conn.execute_batch("BEGIN IMMEDIATE") {
            return fail_all(&batch, &failed("begin", e));
        }
        match self.in_transaction(&store.db, &batch) {
            Ok(outcomes) => outcomes,
            Err(e) => {
                // Rolls back everything of this batch; if even that fails, the connection
                // still holds an uncommitted transaction that the next BEGIN refuses, and
                // SQLite discards it on close or crash.
                let _ = conn.execute_batch("ROLLBACK");
                fail_all(&batch, &e)
            }
        }
    }

    /// The body of a batch transaction. `Err` aborts the whole batch (the caller rolls
    /// back); per-request refusals are `Ok` outcomes.
    fn in_transaction(&self, db: &Database, batch: &[Pending]) -> Result<Vec<(u64, Outcome)>, JournalError> {
        let conn = db.connection();
        self.at(CrashPoint::Begun)?;
        let mut tail = read_tail(db, &self.stream)?;
        self.at(CrashPoint::TailRead)?;
        let mut outcomes = Vec::with_capacity(batch.len());
        for p in batch {
            match self.one(db, tail.as_ref(), &p.req)? {
                Ok(next) => {
                    outcomes.push((p.ticket, Ok(next.hash)));
                    tail = Some(next);
                }
                Err(e) => outcomes.push((p.ticket, Err(e))),
            }
        }
        self.at(CrashPoint::BatchBuilt)?;
        if !outcomes.iter().any(|(_, o)| o.is_ok()) {
            conn.execute_batch("ROLLBACK").map_err(|e| failed("rollback", e))?;
            return Ok(outcomes);
        }
        self.at(CrashPoint::BeforeCommit)?;
        let committed = conn.execute_batch("COMMIT");
        // A refusal at `Committed` simulates a COMMIT that wrote its frame and then reported
        // an error (test hook).
        let injected = self.hook_refuses(CrashPoint::Committed);
        match committed {
            Ok(()) if !injected => Ok(outcomes),
            other => {
                // Outcome unknown: the batch may be durable. Poison until a reopen.
                self.poisoned.store(true, AtomicOrdering::SeqCst);
                let why = match other {
                    Err(e) => e.to_string(),
                    Ok(()) => "injected fault at committed".to_string(),
                };
                Err(JournalError::AppendFailed(format!(
                    "commit failed, outcome unknown until the project is reopened: {why}"
                )))
            }
        }
    }

    /// One request inside the batch transaction. Outer `Err`: abort the batch. Inner
    /// `Err`: this request is refused and left no row.
    fn one(
        &self,
        db: &Database,
        tail: Option<&Tail>,
        req: &AppendRequest,
    ) -> Result<Result<Tail, JournalError>, JournalError> {
        let conn = db.connection();
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
        for d in &req.blobs {
            let files = match blobs::check(&self.blobs, &to_store(d)) {
                Ok(blobs::Found::Present(files)) => files,
                Ok(blobs::Found::Missing(m)) => return Ok(Err(JournalError::MissingBlob(Digest(m.to_bytes())))),
                Err(e) => return Ok(Err(failed(&format!("check blob {d}"), e))),
            };
            for f in &files {
                if let Err(e) = blobs::flush(&self.blobs, f) {
                    return Ok(Err(failed(&format!("flush blob {f}"), e)));
                }
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
        let payload_digest = match self.blobs.put_bytes(&payload, PutOpts { durable: true }) {
            Ok(Address::Blob(b)) => b.digest,
            Ok(Address::Object(_)) => {
                return Ok(Err(JournalError::AppendFailed("payload was stored as a chunked object".into())))
            }
            Err(e) => return Ok(Err(failed("put payload", e))),
        };
        if let Err(e) = self.at(CrashPoint::PayloadPut) {
            return Ok(Err(e));
        }
        conn.execute_batch("SAVEPOINT fid_append").map_err(|e| failed("savepoint", e))?;
        let hash = entry.header.hash;
        let rows = || -> Result<(), JournalError> {
            self.at(CrashPoint::SavepointOpened)?;
            let seq_i = i64::try_from(seq).map_err(|_| JournalError::AppendFailed("sequence out of range".into()))?;
            let hash_s = hash.to_string();
            conn.execute(
                "INSERT INTO entries (seq, hash, stream, branch, kind, prev_hash, header) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    seq_i,
                    hash_s.as_str(),
                    entry.header.stream.as_str(),
                    entry.header.branch.as_str(),
                    entry.header.kind().as_str(),
                    prev.to_string(),
                    header.as_slice(),
                ),
            )
            .map_err(|e| insert_error("entries", e.into()))?;
            self.at(CrashPoint::EntryInserted)?;
            let insert_ref = |role: &str, digest: &str| -> Result<(), JournalError> {
                conn.execute(
                    "INSERT INTO entry_blobs (entry_hash, role, digest) VALUES (?1, ?2, ?3)",
                    (hash_s.as_str(), role, digest),
                )
                .map(|_| ())
                .map_err(|e| insert_error("entry_blobs", e.into()))
            };
            insert_ref(ROLE_PAYLOAD, payload_digest.as_str())?;
            self.at(CrashPoint::PayloadRefInserted)?;
            for d in &req.blobs {
                insert_ref(ROLE_REF, &d.to_string())?;
            }
            self.at(CrashPoint::BlobRefsInserted)?;
            Ok(())
        };
        if let Err(e) = rows() {
            conn.execute_batch("ROLLBACK TO fid_append; RELEASE fid_append").map_err(|e| failed("rollback to", e))?;
            return Ok(Err(e));
        }
        conn.execute_batch("RELEASE fid_append").map_err(|e| failed("release", e))?;
        // Past the savepoint the rows belong to the batch: a fault here aborts the batch.
        self.at(CrashPoint::SavepointReleased)?;
        Ok(Ok(Tail { seq, hash, utc: req.draft.time.utc.clone() }))
    }
}

fn insert_error(table: &str, e: SchemaError) -> JournalError {
    match e {
        SchemaError::AppendOnly => {
            JournalError::AppendFailed(format!("insert into {table}: refused as append_only (row exists)"))
        }
        other => failed(&format!("insert into {table}"), other),
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

/// Reads and verifies the chain tail: the row's columns against its header, the header's
/// hash, the genesis rule, and the stream.
fn read_tail(db: &Database, stream: &str) -> Result<Option<Tail>, JournalError> {
    let conn = db.connection();
    let mut st = conn
        .prepare("SELECT seq, hash, stream, branch, kind, prev_hash, header FROM entries ORDER BY seq DESC LIMIT 1")
        .map_err(|e| failed("read tail", e))?;
    let mut rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Vec<u8>>(6)?,
            ))
        })
        .map_err(|e| failed("read tail", e))?;
    let Some(row) = rows.next() else { return Ok(None) };
    let (seq, hash, row_stream, row_branch, row_kind, prev, header) = row.map_err(|e| failed("read tail", e))?;
    drop(rows);
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
    if seq_u > FIRST_SEQ {
        // No fork: the tail links to the stored entry just below it (one indexed lookup).
        let mut below_st =
            conn.prepare("SELECT hash FROM entries WHERE seq = ?1").map_err(|e| failed("read tail", e))?;
        let below: Option<String> = below_st
            .query_map([seq - 1], |r| r.get::<_, String>(0))
            .map_err(|e| failed("read tail", e))?
            .next()
            .transpose()
            .map_err(|e| failed("read tail", e))?;
        if below.as_deref().and_then(parse_digest) != Some(header_prev) {
            return Err(bad(format!("prev does not link to the stored entry {}", seq - 1)));
        }
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
