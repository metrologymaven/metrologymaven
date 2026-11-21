//! Transaction API, blob-before-row ordering and durability (spec node `CORE.STO.SQL.TXN`).
//!
//! [`Store`] is an open project: its location and write lock (`CORE.STO.LAYOUT`), its blob
//! store (`CORE.STO.CAS`) and its one `project.db` connection, opened only through
//! [`crate::schema::open_write`] / [`crate::schema::open_read`] (hardened connection, schema
//! check, WAL). This module never opens `project.db` itself.
//!
//! [`Store::transaction`] is the one way to commit. A [`Txn`] collects blob puts and row
//! operations in memory; [`Txn::commit`] then, holding the store's commit mutex:
//!
//! 1. refuses a read-only store (`read_only`) and malformed row operations (`invalid_row`);
//! 2. checks that every blob digest a row references is put in the same transaction or already
//!    stored, else fails with `dangling_blob`; an already-stored blob (and, for a chunked
//!    object, every chunk) is hash-checked, else the commit fails with `corrupt_blob`. Both
//!    checks run before anything is written (the spec's step 2, moved first so that "write
//!    nothing" holds for blobs too);
//! 3. puts every pending blob with `durable: true` (file and directories flushed), then flushes
//!    every already-stored referenced blob;
//! 4. runs `BEGIN IMMEDIATE`, the row statements and `COMMIT` on the connection, which runs
//!    with WAL and `synchronous=FULL` (plus `fullfsync` and `checkpoint_fullfsync`, which
//!    make macOS use `F_FULLFSYNC`; no-ops elsewhere), and returns only after SQLite reports
//!    the transaction committed.
//!
//! Isolation: SQLite transactions are serializable. `BEGIN IMMEDIATE` takes the write lock at
//! the start, so a commit never fails half-way with a lock upgrade error; in WAL mode
//! `BEGIN EXCLUSIVE` behaves the same, so IMMEDIATE (the spec's choice) is used. Busy: the
//! connection waits up to [`crate::schema::BUSY_TIMEOUT`] (5 s) for a lock held by another
//! connection, then the commit fails with `busy` and rolls back; there is no further retry
//! (the project write lock already keeps other Metrology Maven writers out, so only a foreign
//! connection can hold the lock).
//!
//! Rollback on every error path: every statement runs inside a `rusqlite::Transaction` whose
//! drop rolls back, so an early return, a failed `COMMIT` and an unwinding panic (from a
//! `CommitProbe` or a [`Store::read`] callback) all roll back. No `catch_unwind` is used:
//! the panic propagates to the caller unchanged. A panic poisons the commit mutex; the next
//! user of the store clears the poison and, before doing anything else, rolls back any
//! transaction still open and turns `query_only` off again, so a panic never leaves rows of a
//! transaction visible or the store unusable. With `panic = "abort"` the process dies and the
//! WAL discards the uncommitted transaction on the next open (REQ-005).

use crate::cas::{self, Address, BlobStore, CasError, PutOpts, Stored};
use crate::layout::{OpenMode, ProjectLocation, StoreError, VerifiedFolder, BLOBS_DIR};
use crate::schema::{self, Database, SchemaError, MUTABLE_TABLES, TABLES};
use crate::Locked;
use fid_types::Digest;
use rusqlite::types::{ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, ErrorCode, TransactionBehavior};
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::Path;
#[cfg(any(test, feature = "test-probe"))]
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};

/// Columns whose value is the digest of a blob in the project's blob store. A row in these
/// columns makes the commit check and flush the blob (REQ-CORE.STO.SQL.TXN-001/003).
/// `snapshots.content_digest` names cache content (`CORE.RPL.RECON.SNAP`), not a blob.
pub const BLOB_REF_COLUMNS: [(&str, &str); 1] = [("entry_blobs", "digest")];

/// Longest table or column name accepted in a row operation.
pub const MAX_IDENT_LEN: usize = 64;

/// Expected answers of the durability pragmas of a write connection.
const DURABILITY: [(&str, &str, i64); 4] = [
    // FULL = 2: the WAL is synced at every commit.
    ("synchronous", "FULL", 2),
    ("fullfsync", "ON", 1),
    ("checkpoint_fullfsync", "ON", 1),
    // SQLite's default, set explicitly so a checkpoint is never disabled by an earlier setting.
    ("wal_autocheckpoint", "1000", 1000),
];

/// A value of one column. Floating point is deliberately absent: the project database stores
/// no floats (determinism, CONSTITUTION §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Integer(i64),
    Text(String),
    Blob(Vec<u8>),
}

impl ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Value::Null => ValueRef::Null,
            Value::Integer(i) => ValueRef::Integer(*i),
            Value::Text(s) => ValueRef::Text(s.as_bytes()),
            Value::Blob(b) => ValueRef::Blob(b),
        }))
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Text(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Text(s)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Integer(i)
    }
}

impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Value {
        Value::Blob(b)
    }
}

impl From<Digest> for Value {
    fn from(d: Digest) -> Value {
        Value::Text(d.as_str().to_string())
    }
}

/// One row operation of a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOp {
    /// `INSERT` into any table but `schema_meta`.
    Insert { table: String, values: Vec<(String, Value)> },
    /// Insert, or update the non-key columns if a row with this key exists. Mutable tables only.
    Upsert { table: String, key: Vec<(String, Value)>, values: Vec<(String, Value)> },
}

/// A blob waiting for the commit.
#[derive(Debug, Clone)]
pub struct PendingBlob {
    pub address: Address,
    bytes: Vec<u8>,
}

/// What a successful commit reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Committed {
    /// Row operations executed.
    pub rows: u64,
}

/// Errors of a commit. [`TxnError::code`] gives the stable code.
#[derive(Debug)]
#[non_exhaustive]
pub enum TxnError {
    /// A row references a blob that is neither put in the transaction nor stored.
    DanglingBlob(Digest),
    /// A referenced, already-stored blob (or one of its chunks) does not hash to its address.
    CorruptBlob(Digest),
    /// The store was opened read-only.
    ReadOnly,
    /// A row operation names an unknown table or a malformed column, upserts an append-only
    /// table, writes `schema_meta`, or repeats a column.
    InvalidRow(String),
    /// A trigger refused a change to an append-only table.
    AppendOnly,
    /// Another connection held the database lock for longer than the busy timeout.
    Busy,
    /// A blob put or flush failed (code of the blob error, `io` for I/O).
    Blob(CasError),
    /// An I/O failure outside the blob store (also what a `CommitProbe` fault injects).
    Io(io::Error),
    /// Opening the database or applying a connection setting failed.
    Schema(SchemaError),
    /// A layout failure, such as a lost write lock.
    Store(StoreError),
    /// Any other SQLite error; the transaction was rolled back.
    Sqlite(rusqlite::Error),
}

impl TxnError {
    /// Stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            TxnError::DanglingBlob(_) => "dangling_blob",
            TxnError::CorruptBlob(_) => "corrupt_blob",
            TxnError::ReadOnly => "read_only",
            TxnError::InvalidRow(_) => "invalid_row",
            TxnError::AppendOnly => schema::APPEND_ONLY,
            TxnError::Busy => "busy",
            TxnError::Blob(e) => e.code(),
            TxnError::Io(_) => "io",
            TxnError::Schema(e) => e.code(),
            TxnError::Store(e) => e.code(),
            TxnError::Sqlite(_) => "sqlite",
        }
    }
}

impl fmt::Display for TxnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxnError::DanglingBlob(d) => write!(f, "dangling_blob: {d} is neither in the transaction nor stored"),
            TxnError::CorruptBlob(d) => write!(f, "corrupt_blob: content stored under {d} does not match it"),
            TxnError::ReadOnly => write!(f, "read_only: the store was opened read-only"),
            TxnError::InvalidRow(why) => write!(f, "invalid_row: {why}"),
            TxnError::AppendOnly => write!(f, "append_only: rows of this table cannot be changed or removed"),
            TxnError::Busy => write!(f, "busy: the database stayed locked by another connection"),
            TxnError::Blob(e) => write!(f, "{e}"),
            TxnError::Io(e) => write!(f, "io: {e}"),
            TxnError::Schema(e) => write!(f, "{e}"),
            TxnError::Store(e) => write!(f, "{e}"),
            TxnError::Sqlite(e) => write!(f, "sqlite: {e}"),
        }
    }
}

impl std::error::Error for TxnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TxnError::Blob(e) => Some(e),
            TxnError::Io(e) => Some(e),
            TxnError::Schema(e) => Some(e),
            TxnError::Store(e) => Some(e),
            TxnError::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for TxnError {
    fn from(e: rusqlite::Error) -> Self {
        if schema::is_append_only(&e) {
            TxnError::AppendOnly
        } else if matches!(e.sqlite_error_code(), Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)) {
            TxnError::Busy
        } else {
            TxnError::Sqlite(e)
        }
    }
}

impl From<SchemaError> for TxnError {
    fn from(e: SchemaError) -> Self {
        match e {
            SchemaError::AppendOnly => TxnError::AppendOnly,
            other => TxnError::Schema(other),
        }
    }
}

impl From<StoreError> for TxnError {
    fn from(e: StoreError) -> Self {
        TxnError::Store(e)
    }
}

impl From<CasError> for TxnError {
    fn from(e: CasError) -> Self {
        TxnError::Blob(e)
    }
}

/// Steps of a commit reported to a `CommitProbe`, in the order they happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitEvent {
    /// The durable put of pending blob `index` returned: file and directories are flushed.
    BlobDurable { index: usize, digest: Digest },
    /// An already-stored blob the rows reference was flushed.
    ExistingBlobDurable { digest: Digest },
    /// About to run `BEGIN IMMEDIATE`.
    Begin,
    /// Every row statement ran; about to run `COMMIT`.
    BeforeCommit,
    /// SQLite reported the transaction committed (the WAL is synced under `synchronous=FULL`).
    Committed,
}

/// Test hook: observes the steps of every commit and can inject a fault (an `Err` fails the
/// commit at that step as if the step itself had failed) or panic. Compiled only for this
/// crate's tests and with the `test-probe` feature, which only the crate's dev-dependency on
/// itself enables; a normal build has no probe at all.
#[cfg(any(test, feature = "test-probe"))]
pub trait CommitProbe: Send + Sync {
    fn on(&self, event: &CommitEvent) -> io::Result<()>;
}

/// An open project with its database connection and blob store.
pub struct Store {
    // Fields drop in declaration order: the connection closes (and checkpoints) before the
    // write lock is released, so no other writer can open the project while it is still open.
    /// The connection. Holding this mutex is what serialises commits (and reads) in-process;
    /// the OS write lock keeps other processes out.
    db: Mutex<Database>,
    lock: Option<Locked>,
    location: ProjectLocation,
    blobs: BlobStore,
    mode: OpenMode,
    #[cfg(any(test, feature = "test-probe"))]
    probe: Option<Arc<dyn CommitProbe>>,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").field("root", &self.location.root()).field("mode", &self.mode).finish_non_exhaustive()
    }
}

impl Store {
    /// Creates a project in `path` (see [`ProjectLocation::create`]) and opens it for writing.
    pub fn create(path: &Path) -> Result<Store, TxnError> {
        let (location, lock) = ProjectLocation::create(path)?;
        Store::finish_open(location, Some(lock), OpenMode::Write)
    }

    /// Opens the project in `path`. `Write` takes the project's write lock and migrates the
    /// schema forward; `Read` opens the database read-only and refuses every commit.
    pub fn open(path: &Path, mode: OpenMode) -> Result<Store, TxnError> {
        let (location, lock) = ProjectLocation::open(path, mode)?;
        Store::finish_open(location, lock, mode)
    }

    /// Opens the project database for writing with a write lock the caller already holds
    /// (from [`ProjectLocation::create`] or [`ProjectLocation::open`] in `Write` mode), for
    /// callers such as `fid-journal` whose API hands over the location and the lock.
    pub fn open_locked(location: ProjectLocation, lock: Locked) -> Result<Store, TxnError> {
        Store::finish_open(location, Some(lock), OpenMode::Write)
    }

    /// [`Store::create`] in a folder `CORE.BUS.POL.FS` just created (`FolderTarget::create_new`):
    /// the folder's identity is checked before the layout is written and again after the
    /// database is opened (`CORE.STO.LAYOUT` REQ-011).
    pub fn create_at(folder: &dyn VerifiedFolder) -> Result<Store, TxnError> {
        let (location, lock) = ProjectLocation::create_at(folder)?;
        let store = Store::finish_open(location, Some(lock), OpenMode::Write)?;
        store.location.check_identity(folder.folder_id())?;
        Ok(store)
    }

    /// [`Store::open`] of a folder `CORE.BUS.POL.FS` resolved (`FolderTarget::open_project`),
    /// with the identity checked when the folder is opened and again after the database is
    /// opened. A read-only folder (import root) opens only with [`OpenMode::Read`], takes no
    /// lock and writes no file: the database is opened `immutable` and the blob store without
    /// creating anything (`read_only_unavailable` when that is not possible).
    pub fn open_at(folder: &dyn VerifiedFolder, mode: OpenMode) -> Result<Store, TxnError> {
        let (location, lock) = ProjectLocation::open_at(folder, mode)?;
        let store = if folder.read_only() {
            let db = schema::open_read_immutable(&location)?;
            let blobs = BlobStore::open_existing(&location.root().join(BLOBS_DIR))?;
            Store {
                location,
                lock: None,
                blobs,
                mode: OpenMode::Read,
                db: Mutex::new(db),
                #[cfg(any(test, feature = "test-probe"))]
                probe: None,
            }
        } else {
            Store::finish_open(location, lock, mode)?
        };
        store.location.check_identity(folder.folder_id())?;
        Ok(store)
    }

    fn finish_open(location: ProjectLocation, lock: Option<Locked>, mode: OpenMode) -> Result<Store, TxnError> {
        let db = match (mode, lock.as_ref()) {
            (OpenMode::Write, Some(l)) => schema::open_write(&location, l)?,
            (OpenMode::Write, None) => return Err(StoreError::LockLost.into()),
            (OpenMode::Read, _) => schema::open_read(&location)?,
        };
        if mode == OpenMode::Write {
            set_durability(db.connection())?;
        }
        let blobs = BlobStore::for_project(&location)?;
        Ok(Store {
            location,
            lock,
            blobs,
            mode,
            db: Mutex::new(db),
            #[cfg(any(test, feature = "test-probe"))]
            probe: None,
        })
    }

    /// Whether the store was opened for writing.
    pub fn mode(&self) -> OpenMode {
        self.mode
    }

    /// The project folder.
    pub fn location(&self) -> &ProjectLocation {
        &self.location
    }

    /// The project's blob store (reads; writes belong in a transaction).
    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// Starts collecting a transaction. Nothing is written before [`Txn::commit`].
    pub fn transaction(&self) -> Txn<'_> {
        Txn { store: self, blobs: Vec::new(), rows: Vec::new() }
    }

    /// Runs `f` on the connection inside a read transaction with `query_only` on, under the
    /// commit mutex. Afterwards (also if `f` panics, at the next use of the store) the read
    /// transaction is ended and `query_only` turned off. Not a security boundary: `f` is
    /// trusted code of this process; the append-only triggers hold whatever it does.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, TxnError> {
        let guard = self.lock_db()?;
        let conn = guard.connection();
        conn.execute_batch("PRAGMA query_only=ON; BEGIN DEFERRED")?;
        let out = f(conn);
        reset(conn, self.mode)?;
        Ok(out?)
    }

    /// Installs or removes a `CommitProbe` (tests only).
    #[cfg(any(test, feature = "test-probe"))]
    pub fn set_commit_probe(&mut self, probe: Option<Arc<dyn CommitProbe>>) {
        self.probe = probe;
    }

    /// Takes the commit mutex. A panic of an earlier holder poisoned it: the poison is cleared
    /// and any transaction it left open rolled back before the connection is used again.
    fn lock_db(&self) -> Result<MutexGuard<'_, Database>, TxnError> {
        let guard = match self.db.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                let g = poisoned.into_inner();
                self.db.clear_poison();
                g
            }
        };
        reset(guard.connection(), self.mode)?;
        Ok(guard)
    }

    #[cfg(any(test, feature = "test-probe"))]
    fn report(&self, event: &CommitEvent) -> Result<(), TxnError> {
        match &self.probe {
            Some(p) => p.on(event).map_err(TxnError::Io),
            None => Ok(()),
        }
    }

    /// Without the `test-probe` feature there is no probe: every step proceeds.
    #[cfg(not(any(test, feature = "test-probe")))]
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn report(&self, _event: &CommitEvent) -> Result<(), TxnError> {
        Ok(())
    }
}

/// Ends any transaction left open and, on a write connection, turns `query_only` off.
fn reset(conn: &Connection, mode: OpenMode) -> Result<(), TxnError> {
    if !conn.is_autocommit() {
        conn.execute_batch("ROLLBACK")?;
    }
    if mode == OpenMode::Write {
        conn.execute_batch("PRAGMA query_only=OFF")?;
    }
    Ok(())
}

/// Applies and reads back the durability settings of a write connection.
fn set_durability(conn: &Connection) -> Result<(), TxnError> {
    for (pragma, value, want) in DURABILITY {
        let fail = |why: String| TxnError::Schema(SchemaError::HardeningFailed(format!("{pragma}: {why}")));
        // Some setters answer with a row (wal_autocheckpoint); step through whatever comes back.
        let set = || -> rusqlite::Result<()> {
            let mut st = conn.prepare(&format!("PRAGMA {pragma}={value}"))?;
            let mut rows = st.query([])?;
            while rows.next()?.is_some() {}
            Ok(())
        };
        set().map_err(|e| fail(e.to_string()))?;
        let got: i64 =
            conn.query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0)).map_err(|e| fail(e.to_string()))?;
        if got != want {
            return Err(fail(format!("is {got}, expected {want}")));
        }
    }
    Ok(())
}

/// A transaction being collected. Dropping it, or [`Txn::abort`], discards it unwritten.
pub struct Txn<'s> {
    store: &'s Store,
    blobs: Vec<PendingBlob>,
    rows: Vec<RowOp>,
}

impl fmt::Debug for Txn<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Txn").field("blobs", &self.blobs.len()).field("rows", &self.rows).finish_non_exhaustive()
    }
}

fn owned(values: &[(&str, Value)]) -> Vec<(String, Value)> {
    values.iter().map(|(c, v)| ((*c).to_string(), v.clone())).collect()
}

impl Txn<'_> {
    /// Adds a blob and returns its address (computed now; written durably at commit). The same
    /// content put twice is kept once.
    pub fn put_blob(&mut self, bytes: Vec<u8>) -> Result<Address, TxnError> {
        let address = cas::address_of(&bytes)?;
        if !self.blobs.iter().any(|b| b.address == address) {
            self.blobs.push(PendingBlob { address, bytes });
        }
        Ok(address)
    }

    /// Adds an `INSERT` of `values` (column, value) into `table`.
    pub fn insert(&mut self, table: &str, values: &[(&str, Value)]) -> &mut Self {
        self.rows.push(RowOp::Insert { table: table.to_string(), values: owned(values) });
        self
    }

    /// Adds an insert-or-update on a mutable table: `key` are the primary-key columns, `values`
    /// the columns set on insert and replaced on update.
    pub fn upsert(&mut self, table: &str, key: &[(&str, Value)], values: &[(&str, Value)]) -> &mut Self {
        self.rows.push(RowOp::Upsert { table: table.to_string(), key: owned(key), values: owned(values) });
        self
    }

    /// The pending blobs, in put order.
    pub fn pending_blobs(&self) -> &[PendingBlob] {
        &self.blobs
    }

    /// The row operations, in call order.
    pub fn rows(&self) -> &[RowOp] {
        &self.rows
    }

    /// Checks, writing nothing, that `digest` can be referenced by a row of this transaction:
    /// it is pending in it, or stored whole and sound. This is the check [`Txn::commit`] runs
    /// on every referenced blob, offered so a caller can refuse one row early instead of the
    /// whole commit: `dangling_blob` names the missing digest (the blob, or the first missing
    /// chunk of the object it names), `corrupt_blob` the first one that does not hash to its
    /// address. The commit checks again and flushes.
    pub fn check_blob(&self, digest: &Digest) -> Result<(), TxnError> {
        if self.blobs.iter().any(|b| b.address.digest() == *digest) {
            return Ok(());
        }
        match self.store.blobs.check_stored(digest)? {
            Stored::Missing(m) => Err(TxnError::DanglingBlob(m)),
            Stored::Corrupt(bad) => Err(TxnError::CorruptBlob(bad)),
            Stored::Sound(_) => Ok(()),
        }
    }

    /// Discards the transaction; nothing was written.
    pub fn abort(self) {}

    /// Commits: blobs durable first, then all rows in one SQLite transaction (module docs).
    /// On any error nothing of the rows is visible; blobs already put stay, unreferenced.
    pub fn commit(self) -> Result<Committed, TxnError> {
        let store = self.store;
        if store.mode != OpenMode::Write {
            return Err(TxnError::ReadOnly);
        }
        let statements = self.rows.iter().map(statement).collect::<Result<Vec<_>, _>>()?;
        let referenced = referenced_blobs(&self.rows)?;
        let pending: BTreeSet<Digest> = self.blobs.iter().map(|b| b.address.digest()).collect();

        let mut guard = store.lock_db()?;
        if let Some(lock) = &store.lock {
            lock.verify()?;
        }
        // Referenced blobs must be pending, or stored whole and sound (content and every chunk
        // hash-checked), before anything is written.
        let mut existing: Vec<(Digest, Vec<Digest>)> = Vec::new();
        for d in referenced.difference(&pending) {
            match store.blobs.check_stored(d)? {
                Stored::Missing(_) => return Err(TxnError::DanglingBlob(*d)),
                Stored::Corrupt(bad) => return Err(TxnError::CorruptBlob(bad)),
                Stored::Sound(flush) => existing.push((*d, flush)),
            }
        }
        // (1) Every pending blob durable.
        let opts = PutOpts { durable: true };
        for (index, b) in self.blobs.iter().enumerate() {
            let got = store.blobs.put_bytes(&b.bytes, opts)?;
            if got != b.address {
                return Err(TxnError::Io(io::Error::other(format!(
                    "blob store returned {} for content addressed {}",
                    got.digest(),
                    b.address.digest()
                ))));
            }
            store.report(&CommitEvent::BlobDurable { index, digest: got.digest() })?;
        }
        // (2) Already-stored referenced blobs durable too (an earlier put may not have been).
        for (d, flush) in &existing {
            for f in flush {
                store.blobs.flush_stored(f)?;
            }
            store.report(&CommitEvent::ExistingBlobDurable { digest: *d })?;
        }
        // (3) One write transaction. `tx` rolls back when dropped without commit: on every
        // `?` below and while unwinding from a panic.
        store.report(&CommitEvent::Begin)?;
        let tx = guard.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (sql, params) in &statements {
            tx.execute(sql, rusqlite::params_from_iter(params.iter()))?;
        }
        store.report(&CommitEvent::BeforeCommit)?;
        tx.commit()?;
        // Committed and durable: a probe can no longer fail the commit.
        let _ = store.report(&CommitEvent::Committed);
        Ok(Committed { rows: statements.len() as u64 })
    }
}

/// True for `[a-z][a-z0-9_]*` of at most [`MAX_IDENT_LEN`] bytes.
fn is_ident(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && s.len() <= MAX_IDENT_LEN
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn check_columns<'a>(table: &str, cols: impl Iterator<Item = &'a str>) -> Result<Vec<&'a str>, TxnError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for c in cols {
        if !is_ident(c) {
            return Err(TxnError::InvalidRow(format!("column name {c:?} of {table} is not allowed")));
        }
        if !seen.insert(c) {
            return Err(TxnError::InvalidRow(format!("column {c} of {table} given twice")));
        }
        out.push(c);
    }
    if out.is_empty() {
        return Err(TxnError::InvalidRow(format!("row of {table} has no columns")));
    }
    Ok(out)
}

fn check_table(table: &str) -> Result<(), TxnError> {
    if table == "schema_meta" {
        return Err(TxnError::InvalidRow("schema_meta is written only by migrations".to_string()));
    }
    if !TABLES.contains(&table) {
        return Err(TxnError::InvalidRow(format!("unknown table {table:?}")));
    }
    Ok(())
}

/// The SQL text and parameters of one row operation. Identifiers are checked against the
/// schema's table list and `[a-z][a-z0-9_]*` and quoted; values are always bound.
fn statement(op: &RowOp) -> Result<(String, Vec<&Value>), TxnError> {
    let quote = |c: &str| format!("\"{c}\"");
    let placeholders = |n: usize| (1..=n).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    match op {
        RowOp::Insert { table, values } => {
            check_table(table)?;
            let cols = check_columns(table, values.iter().map(|(c, _)| c.as_str()))?;
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({})",
                quote(table),
                cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
                placeholders(cols.len())
            );
            Ok((sql, values.iter().map(|(_, v)| v).collect()))
        }
        RowOp::Upsert { table, key, values } => {
            check_table(table)?;
            if !MUTABLE_TABLES.contains(&table.as_str()) {
                return Err(TxnError::InvalidRow(format!("upsert on {table}, which is not a mutable table")));
            }
            if key.is_empty() {
                return Err(TxnError::InvalidRow(format!("upsert on {table} without key columns")));
            }
            let cols = check_columns(table, key.iter().chain(values.iter()).map(|(c, _)| c.as_str()))?;
            let key_cols: Vec<String> = key.iter().map(|(c, _)| quote(c)).collect();
            let action = if values.is_empty() {
                "NOTHING".to_string()
            } else {
                let sets: Vec<String> = values.iter().map(|(c, _)| format!("{0} = excluded.{0}", quote(c))).collect();
                format!("UPDATE SET {}", sets.join(", "))
            };
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO {action}",
                quote(table),
                cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
                placeholders(cols.len()),
                key_cols.join(", ")
            );
            Ok((sql, key.iter().chain(values.iter()).map(|(_, v)| v).collect()))
        }
    }
}

/// Blob digests the row operations reference through [`BLOB_REF_COLUMNS`]. A value there that
/// is not a valid digest is refused as `invalid_row`.
fn referenced_blobs(rows: &[RowOp]) -> Result<BTreeSet<Digest>, TxnError> {
    let mut out = BTreeSet::new();
    for op in rows {
        let (table, cols): (&str, Vec<&(String, Value)>) = match op {
            RowOp::Insert { table, values } => (table.as_str(), values.iter().collect()),
            RowOp::Upsert { table, key, values } => (table.as_str(), key.iter().chain(values.iter()).collect()),
        };
        for (col, value) in cols {
            if !BLOB_REF_COLUMNS.contains(&(table, col.as_str())) {
                continue;
            }
            let Value::Text(s) = value else {
                return Err(TxnError::InvalidRow(format!("{table}.{col} must be a sha256: digest")));
            };
            let d = Digest::parse(s)
                .map_err(|_| TxnError::InvalidRow(format!("{table}.{col} must be a sha256: digest, got {s:?}")))?;
            out.insert(d);
        }
    }
    Ok(out)
}
