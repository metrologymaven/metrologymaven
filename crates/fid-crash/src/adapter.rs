//! The payload adapter (REQ-CORE.STO.SQL.CRASH-006) and the adapter for `fid-store`.
//!
//! A [`CrashAdapter`] supplies the four things the harness cannot know: how to make an empty
//! store, how a writer opens it and commits the next row, and how to check a reopened store
//! against the acknowledged sequence numbers. [`StoreAdapter`] commits the synthetic chain
//! through `fid-store`'s [`Store::transaction`] and checks it with [`verify_store`].

use crate::chain::{blob_bytes, SyntheticRow, MAX_ROW_BYTES, ROW_FORMAT};
use crate::failure::{CrashFailure, FailureKind};
use fid_store::{Address, BlobRef, CasError, OpenMode, Store, StoreError, TxnError, Value};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(feature = "fault-inject")]
use fid_store::txn::{CommitEvent, CommitProbe};
#[cfg(feature = "fault-inject")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "fault-inject")]
use std::sync::Arc;

/// `entries.stream` of every synthetic row.
pub const STREAM: &str = "fid-crash";
/// `entries.branch` and the `heads` row the writer moves.
pub const BRANCH: &str = "main";
/// `entry_blobs.role` of a row's blob.
pub const BLOB_ROLE: &str = "blob";
/// How long an open waits for a just-killed writer's OS lock to be released.
pub const LOCK_WAIT: Duration = Duration::from_secs(10);

/// The writer's acknowledgement channel: one `ACK <seq>` line per acknowledged commit.
pub struct Ack<'a> {
    out: &'a mut dyn Write,
    last: Option<u64>,
}

impl<'a> Ack<'a> {
    pub fn new(out: &'a mut dyn Write) -> Ack<'a> {
        Ack { out, last: None }
    }

    /// Writes and flushes `ACK <seq>`.
    pub fn send(&mut self, seq: u64) -> io::Result<()> {
        writeln!(self.out, "ACK {seq}")?;
        self.out.flush()?;
        self.last = Some(seq);
        Ok(())
    }

    /// The last sequence number acknowledged.
    pub fn last(&self) -> Option<u64> {
        self.last
    }

    /// The underlying output, for extra report lines.
    pub fn out(&mut self) -> &mut dyn Write {
        self.out
    }
}

/// What the harness needs from a payload (spec Design: `trait CrashAdapter`).
pub trait CrashAdapter {
    /// Parent: makes a new, empty store at `root`.
    fn create(&self, root: &Path) -> Result<(), String>;
    /// Writer: opens the store at `root` and returns the next sequence number to commit.
    fn open_writer(&mut self, root: &Path) -> Result<u64, String>;
    /// Writer: commits row `seq` and returns only once the commit returned success. The
    /// harness acknowledges `seq` after this returns; a correct adapter never touches `ack`.
    fn commit_next(&mut self, seq: u64, ack: &mut Ack<'_>) -> Result<(), String>;
    /// Parent: reopens the store at `root` after a kill and checks it against every sequence
    /// number acknowledged since it was created. Returns the number of rows it holds.
    fn check(&self, root: &Path, acked: &[u64]) -> Result<u64, CrashFailure>;
}

/// How the [`StoreAdapter`] writer commits. The two broken modes exist for the harness's
/// self-tests (AC-2, AC-3) and are compiled only with the `fault-inject` feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterMode {
    /// Blob durable, then the row, through one `Txn::commit`; acknowledged after it returns.
    Correct,
    /// Acknowledges the row before committing it (AC-2).
    #[cfg(feature = "fault-inject")]
    AckBeforeCommit,
    /// Stores the blob without flushing it and commits the row without the `entry_blobs`
    /// reference that would make the commit flush it (AC-3).
    #[cfg(feature = "fault-inject")]
    RowBeforeBlobFlush,
}

impl WriterMode {
    /// Parses a mode name; broken modes are unknown without `fault-inject`.
    pub fn parse(s: &str) -> Option<WriterMode> {
        match s {
            "correct" => Some(WriterMode::Correct),
            #[cfg(feature = "fault-inject")]
            "ack-before-commit" => Some(WriterMode::AckBeforeCommit),
            #[cfg(feature = "fault-inject")]
            "row-before-blob-flush" => Some(WriterMode::RowBeforeBlobFlush),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            WriterMode::Correct => "correct",
            #[cfg(feature = "fault-inject")]
            WriterMode::AckBeforeCommit => "ack-before-commit",
            #[cfg(feature = "fault-inject")]
            WriterMode::RowBeforeBlobFlush => "row-before-blob-flush",
        }
    }

    /// Whether this mode writes the `entry_blobs` row of each blob.
    fn writes_entry_blobs(self) -> bool {
        match self {
            WriterMode::Correct => true,
            #[cfg(feature = "fault-inject")]
            WriterMode::AckBeforeCommit => true,
            #[cfg(feature = "fault-inject")]
            WriterMode::RowBeforeBlobFlush => false,
        }
    }
}

/// Bounds of [`verify_store`] on an untrusted store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckLimits {
    /// Most rows in `entries` (and twice that in `entry_blobs`) the check reads.
    pub max_rows: u64,
}

impl Default for CheckLimits {
    fn default() -> Self {
        CheckLimits { max_rows: 100_000 }
    }
}

/// Commits the synthetic chain through `fid-store`.
pub struct StoreAdapter {
    mode: WriterMode,
    limits: CheckLimits,
    store: Option<Store>,
    next: u64,
    prev: Option<Digest>,
    #[cfg(feature = "fault-inject")]
    probe: Option<Arc<dyn CommitProbe>>,
}

impl StoreAdapter {
    pub fn new(mode: WriterMode) -> StoreAdapter {
        StoreAdapter {
            mode,
            limits: CheckLimits::default(),
            store: None,
            next: 1,
            prev: None,
            #[cfg(feature = "fault-inject")]
            probe: None,
        }
    }

    pub fn with_limits(mut self, limits: CheckLimits) -> StoreAdapter {
        self.limits = limits;
        self
    }

    pub fn mode(&self) -> WriterMode {
        self.mode
    }

    /// Installs a commit probe on the writer's store (now, and when it is opened later).
    #[cfg(feature = "fault-inject")]
    pub fn set_probe(&mut self, probe: Arc<dyn CommitProbe>) {
        if let Some(store) = self.store.as_mut() {
            store.set_commit_probe(Some(Arc::clone(&probe)));
        }
        self.probe = Some(probe);
    }

    /// Closes the writer's store (the connection checkpoints and the lock is released).
    pub fn close(&mut self) {
        self.store = None;
    }

    fn commit_row(&self, store: &Store, row: &SyntheticRow, blob: Option<Vec<u8>>) -> Result<(), TxnError> {
        let hash = row.hash();
        let seq = i64::try_from(row.seq).map_err(|_| TxnError::InvalidRow("seq out of range".to_string()))?;
        let mut txn = store.transaction();
        if let Some(bytes) = blob {
            if self.mode.writes_entry_blobs() {
                txn.put_blob(bytes)?;
            } else {
                // Broken on purpose: the blob is stored without a flush, and the row below
                // carries no entry_blobs reference, so the commit neither checks nor flushes it.
                store.blobs().put_bytes(&bytes, fid_store::PutOpts { durable: false })?;
            }
        }
        txn.insert(
            "entries",
            &[
                ("seq", Value::Integer(seq)),
                ("hash", Value::from(hash)),
                ("stream", Value::from(STREAM)),
                ("branch", Value::from(BRANCH)),
                ("kind", Value::from(ROW_FORMAT)),
                ("prev_hash", row.prev_hash.map_or(Value::Null, Value::from)),
                ("header", Value::Blob(row.canonical())),
            ],
        );
        if let (Some(d), true) = (row.blob, self.mode.writes_entry_blobs()) {
            txn.insert(
                "entry_blobs",
                &[("entry_hash", Value::from(hash)), ("role", Value::from(BLOB_ROLE)), ("digest", Value::from(d))],
            );
        }
        txn.upsert("heads", &[("branch", Value::from(BRANCH))], &[("entry_hash", Value::from(hash))]);
        txn.commit().map(|_| ())
    }
}

/// Opens `root` for writing, waiting up to [`LOCK_WAIT`] while a killed writer's OS lock is
/// being released.
pub fn open_write(root: &Path) -> Result<Store, TxnError> {
    let start = Instant::now();
    loop {
        match Store::open(root, OpenMode::Write) {
            Err(TxnError::Store(StoreError::Locked(_))) if start.elapsed() < LOCK_WAIT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

impl CrashAdapter for StoreAdapter {
    fn create(&self, root: &Path) -> Result<(), String> {
        Store::create(root).map(drop).map_err(|e| format!("create {}: {e}", root.display()))
    }

    fn open_writer(&mut self, root: &Path) -> Result<u64, String> {
        #[allow(unused_mut)]
        let mut store = open_write(root).map_err(|e| format!("open: {e}"))?;
        #[cfg(feature = "fault-inject")]
        if let Some(p) = &self.probe {
            store.set_commit_probe(Some(Arc::clone(p)));
        }
        let last: Option<(i64, String)> = store
            .read(|c| {
                let mut st = c.prepare("SELECT seq, hash FROM entries ORDER BY seq DESC LIMIT 1")?;
                let mut rows = st.query([])?;
                match rows.next()? {
                    Some(r) => Ok(Some((r.get(0)?, r.get(1)?))),
                    None => Ok(None),
                }
            })
            .map_err(|e| format!("read last row: {e}"))?;
        (self.next, self.prev) = match last {
            None => (1, None),
            Some((seq, hash)) => {
                let seq = u64::try_from(seq).map_err(|_| "negative seq".to_string())?;
                let prev = Digest::parse(&hash).map_err(|_| "last hash is not a digest".to_string())?;
                (seq.checked_add(1).ok_or("seq overflow")?, Some(prev))
            }
        };
        self.store = Some(store);
        Ok(self.next)
    }

    fn commit_next(&mut self, seq: u64, ack: &mut Ack<'_>) -> Result<(), String> {
        if seq != self.next {
            return Err(format!("asked to commit {seq}, next is {}", self.next));
        }
        let store = self.store.as_ref().ok_or("writer store is not open")?;
        let row = SyntheticRow::new(seq, self.prev);
        #[cfg(feature = "fault-inject")]
        if self.mode == WriterMode::AckBeforeCommit {
            ack.send(seq).map_err(|e| format!("ack: {e}"))?;
        }
        #[cfg(not(feature = "fault-inject"))]
        let _ = &ack;
        self.commit_row(store, &row, blob_bytes(seq)).map_err(|e| format!("commit {seq}: {e}"))?;
        self.prev = Some(row.hash());
        self.next = seq.saturating_add(1);
        Ok(())
    }

    fn check(&self, root: &Path, acked: &[u64]) -> Result<u64, CrashFailure> {
        verify_store(root, acked, self.limits, self.mode.writes_entry_blobs())
    }
}

/// One `entries` row as read back, every column untrusted.
struct EntryRow {
    seq: i64,
    hash: Option<String>,
    stream: Option<String>,
    branch: Option<String>,
    kind: Option<String>,
    prev_hash: Option<String>,
    /// `None` when the header is not a BLOB of at most [`MAX_ROW_BYTES`] bytes.
    header: Option<Vec<u8>>,
}

fn refused(detail: impl Into<String>) -> CrashFailure {
    CrashFailure::new(FailureKind::StoreRefused, detail)
}

fn broken(detail: impl Into<String>) -> CrashFailure {
    CrashFailure::new(FailureKind::ChainBroken, detail)
}

/// Reopens the store at `root` and checks it (REQ-CORE.STO.SQL.CRASH-003). Fails closed:
/// anything it cannot read, parse or match is a [`CrashFailure`], never a pass.
///
/// Checks, in order: the store opens; SQLite's `quick_check` says `ok`; `entries` holds at
/// most `limits.max_rows` rows, numbered exactly `1..=n`; each row's header is the canonical
/// form of exactly the row the chain defines at that position (which links it to its
/// predecessor's hash), and its other columns agree; each row's blob is stored and hashes to
/// its digest; with `entry_blobs`, each blob has exactly its one reference row and no other
/// reference exists; `heads` holds only `main` at row `n` (nothing when `n = 0`); and every
/// acknowledged sequence number is at most `n`. Returns `n`.
///
/// Bounded on an untrusted store: rows past the limit are not read, headers longer than
/// [`MAX_ROW_BYTES`] are not fetched, and each blob is read once with its expected length.
pub fn verify_store(root: &Path, acked: &[u64], limits: CheckLimits, entry_blobs: bool) -> Result<u64, CrashFailure> {
    let store = open_write(root).map_err(|e| refused(format!("open: {e}")))?;
    let max = i64::try_from(limits.max_rows).unwrap_or(i64::MAX);
    let quick: Vec<String> = store
        .read(|c| {
            let mut st = c.prepare("PRAGMA quick_check(8)")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
        .map_err(|e| refused(format!("quick_check: {e}")))?;
    if quick != ["ok"] {
        return Err(refused(format!("quick_check: {quick:?}")));
    }
    let (count, ref_count): (i64, i64) = store
        .read(|c| {
            Ok((
                c.query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?,
                c.query_row("SELECT count(*) FROM entry_blobs", [], |r| r.get(0))?,
            ))
        })
        .map_err(|e| refused(format!("count rows: {e}")))?;
    if count > max || ref_count > max.saturating_mul(2) {
        return Err(refused(format!("{count} entries and {ref_count} blob references exceed the limit {max}")));
    }
    let limit = max.saturating_add(1);
    let rows: Vec<EntryRow> = store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT seq, hash, stream, branch, kind, prev_hash, \
                 CASE WHEN typeof(header) = 'blob' AND length(header) <= ?2 THEN header END \
                 FROM entries ORDER BY seq LIMIT ?1",
            )?;
            let rows = st.query_map([limit, MAX_ROW_BYTES as i64], |r| {
                Ok(EntryRow {
                    seq: r.get(0)?,
                    hash: r.get(1)?,
                    stream: r.get(2)?,
                    branch: r.get(3)?,
                    kind: r.get(4)?,
                    prev_hash: r.get(5)?,
                    header: r.get(6)?,
                })
            })?;
            rows.collect()
        })
        .map_err(|e| broken(format!("read entries: {e}")))?;
    if rows.len() as u64 > limits.max_rows {
        return Err(refused(format!("more than {} entries", limits.max_rows)));
    }
    let mut refs: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    if entry_blobs {
        let all: Vec<(String, String, String)> = store
            .read(|c| {
                let mut st = c.prepare("SELECT entry_hash, role, digest FROM entry_blobs LIMIT ?1")?;
                let rows =
                    st.query_map([max.saturating_mul(2).saturating_add(1)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect()
            })
            .map_err(|e| broken(format!("read entry_blobs: {e}")))?;
        for (h, role, d) in all {
            refs.entry(h).or_default().push((role, d));
        }
    }

    let mut prev: Option<Digest> = None;
    let mut with_blob: u64 = 0;
    for (i, e) in rows.iter().enumerate() {
        let seq = i as u64 + 1;
        if u64::try_from(e.seq).ok() != Some(seq) {
            return Err(broken(format!("entry {} has seq {}; entries are not 1..n", i + 1, e.seq)));
        }
        let header = e.header.as_deref().ok_or_else(|| broken(format!("row {seq}: header missing or too long")))?;
        let row = SyntheticRow::parse(header).map_err(|why| broken(format!("row {seq}: {why}")))?;
        let want = SyntheticRow::new(seq, prev);
        if row != want {
            return Err(broken(format!("row {seq} is not the chain's row {seq} (prev, payload or blob differs)")));
        }
        let hash = row.hash();
        let columns_ok = e.hash.as_deref() == Some(hash.as_str())
            && e.prev_hash.as_deref() == want.prev_hash.as_ref().map(Digest::as_str)
            && e.stream.as_deref() == Some(STREAM)
            && e.branch.as_deref() == Some(BRANCH)
            && e.kind.as_deref() == Some(ROW_FORMAT);
        if !columns_ok {
            return Err(broken(format!("row {seq}: hash, prev_hash, stream, branch or kind column disagrees")));
        }
        if let Some(d) = row.blob {
            with_blob += 1;
            check_blob(&store, seq, d)?;
        }
        if entry_blobs {
            let want_refs: Vec<(String, String)> =
                row.blob.iter().map(|d| (BLOB_ROLE.to_string(), d.as_str().to_string())).collect();
            let got = refs.remove(hash.as_str()).unwrap_or_default();
            if got != want_refs {
                return Err(broken(format!("row {seq}: entry_blobs are {got:?}, expected {want_refs:?}")));
            }
        }
        prev = Some(hash);
    }
    if entry_blobs && (!refs.is_empty() || ref_count as u64 != with_blob) {
        return Err(broken(format!("{} entry_blobs rows reference no row of the chain", refs.len())));
    }
    let heads: Vec<(String, String)> = store
        .read(|c| {
            let mut st = c.prepare("SELECT branch, entry_hash FROM heads LIMIT 3")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
        .map_err(|e| broken(format!("read heads: {e}")))?;
    let want_heads: Vec<(String, String)> = prev.iter().map(|h| (BRANCH.to_string(), h.as_str().to_string())).collect();
    if heads != want_heads {
        return Err(broken(format!("heads are {heads:?}, expected {want_heads:?}")));
    }
    let n = rows.len() as u64;
    if let Some(lost) = acked.iter().find(|&&a| a == 0 || a > n) {
        return Err(CrashFailure::new(
            FailureKind::AckMissing,
            format!("seq {lost} was acknowledged but the store holds rows 1..={n}"),
        ));
    }
    Ok(n)
}

/// The blob of row `seq` must be stored and hash to `d` (read once, with its expected length).
fn check_blob(store: &Store, seq: u64, d: Digest) -> Result<(), CrashFailure> {
    let len = blob_bytes(seq).map_or(0, |b| b.len() as u64);
    match store.blobs().has_blob(&d) {
        Ok(true) => {}
        Ok(false) => return Err(CrashFailure::new(FailureKind::BlobMissing, format!("row {seq}: blob {d} missing"))),
        Err(e) => return Err(refused(format!("row {seq}: blob {d}: {e}"))),
    }
    match store.blobs().get(&Address::Blob(BlobRef { digest: d, size: len })) {
        Ok(_) => Ok(()),
        Err(CasError::NotFound(_)) => {
            Err(CrashFailure::new(FailureKind::BlobMissing, format!("row {seq}: blob {d} missing")))
        }
        Err(e) => Err(CrashFailure::new(FailureKind::BlobCorrupt, format!("row {seq}: blob {d}: {e}"))),
    }
}

/// A commit probe that counts the steps of every commit and, at step `hang_at`, prints
/// `HANG <step>` and blocks (so the harness can kill the writer exactly there), or at step
/// `fail_at` returns an injected I/O error. Compiled only with `fault-inject`.
#[cfg(feature = "fault-inject")]
pub struct StepProbe {
    steps: AtomicU64,
    hang_at: Option<u64>,
    fail_at: Option<u64>,
}

#[cfg(feature = "fault-inject")]
impl StepProbe {
    pub fn new(hang_at: Option<u64>, fail_at: Option<u64>) -> StepProbe {
        StepProbe { steps: AtomicU64::new(0), hang_at, fail_at }
    }

    /// Steps reported so far.
    pub fn steps(&self) -> u64 {
        self.steps.load(Ordering::SeqCst)
    }
}

#[cfg(feature = "fault-inject")]
impl CommitProbe for StepProbe {
    fn on(&self, event: &CommitEvent) -> io::Result<()> {
        let n = self.steps.fetch_add(1, Ordering::SeqCst) + 1;
        if self.hang_at == Some(n) {
            let mut out = io::stdout().lock();
            writeln!(out, "HANG {n} {event:?}")?;
            out.flush()?;
            // Bounded: the harness kills the writer long before this ends.
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(600) {
                std::thread::sleep(Duration::from_millis(100));
            }
            return Err(io::Error::other("hang timed out"));
        }
        if self.fail_at == Some(n) {
            return Err(io::Error::other(format!("injected fault at step {n}")));
        }
        Ok(())
    }
}
