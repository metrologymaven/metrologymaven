//! Project database schema, append-only triggers and forward migrations (spec node
//! `CORE.STO.SQL.SCHEMA`).
//!
//! `project.db` (created empty by [`crate::layout`]) holds the tables listed in
//! [`TABLES`]. The append-only tables ([`APPEND_ONLY_TABLES`], plus `schema_meta`) carry
//! `BEFORE UPDATE` and `BEFORE DELETE` triggers that abort with `append_only`, so the refusal
//! holds for any connection, including one that bypasses this crate. A third `BEFORE INSERT`
//! trigger refuses `INSERT OR REPLACE` over an existing key, which SQLite would otherwise carry
//! out as a delete without firing delete triggers. [`MUTABLE_TABLES`] are the only tables a
//! writer may change; each is rebuildable from `entries`.
//!
//! Versions: `schema_meta` holds one row per schema version the database has reached; the
//! current version is the largest. A new, empty database is version 0, and migration 0 -> 1
//! is the v1 DDL ([`V1_SQL`]). A write open applies every pending migration and records each
//! new version inside one `BEGIN IMMEDIATE` transaction, so a failure leaves the database at
//! its previous version; it never migrates backwards and refuses a newer database with
//! `schema_too_new`.
//!
//! Every open enables foreign keys and turns `trusted_schema` off (a project file is input
//! from outside). A write open sets WAL journal mode and fails with `wal_unavailable` if
//! SQLite does not confirm it. A read open is read-only and leaves the journal mode as found.
//!
//! Locking: a write open needs the project's write lock ([`Locked`], `CORE.STO.LAYOUT`) and
//! checks that it is still held. `project.db` must be a regular file, not a symbolic link.
//! Paths: on Windows the database is opened through SQLite's `win32-longpath` VFS with a
//! `\\?\` path, so project roots up to [`crate::layout::MAX_PATH_CHARS`] characters work as
//! they do for the rest of the layout. SQLite's Unix VFS limits a path to
//! [`UNIX_MAX_DB_PATH_BYTES`] bytes [unverified: SQLite `MAX_PATHNAME`]; a longer database
//! path fails with `path_too_long` before SQLite is called.

use crate::layout::{OpenMode, ProjectLocation, StoreError, DB_FILE};
use crate::Locked;
use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The v1 DDL: tables, indexes and append-only triggers (no transaction control).
pub const V1_SQL: &str = include_str!("v1.sql");

/// Newest schema version this build writes and the newest it can open for writing.
pub const LATEST: u32 = 1;

/// All tables of schema v1, in DDL order.
pub const TABLES: [&str; 9] = [
    "schema_meta",
    "entries",
    "entry_blobs",
    "heads",
    "snapshots",
    "checkpoints",
    "anchors",
    "idempotency",
    "settings_current",
];

/// Tables whose rows may never change or disappear (REQ-CORE.STO.SQL.SCHEMA-001).
pub const APPEND_ONLY_TABLES: [&str; 5] = ["entries", "entry_blobs", "snapshots", "checkpoints", "anchors"];

/// The only tables a writer may update or delete from (REQ-CORE.STO.SQL.SCHEMA-003).
pub const MUTABLE_TABLES: [&str; 3] = ["heads", "idempotency", "settings_current"];

/// Message of the trigger abort on an append-only table.
pub const APPEND_ONLY: &str = "append_only";

/// Longest database path, in bytes, passed to SQLite's Unix VFS [unverified: SQLite's
/// `MAX_PATHNAME` is 512 including the terminating NUL].
pub const UNIX_MAX_DB_PATH_BYTES: usize = 511;

/// How long a connection waits for a lock held by another connection.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// One forward migration: the SQL that takes the schema from `from` to `to` (`to == from + 1`).
/// The SQL must not contain transaction control; the runner owns the transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Migration {
    pub from: u32,
    pub to: u32,
    pub sql: &'static str,
}

/// A schema version as stored in `schema_meta` (0 = empty database).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion(pub u32);

/// The migrations compiled into this build: migration `i` goes from version `i` to `i + 1`.
pub static MIGRATIONS: [Migration; 1] = [Migration { from: 0, to: 1, sql: V1_SQL }];

/// The migration list a write open applies, ending at `latest`.
#[derive(Clone, Copy, Debug)]
pub struct MigrationPlan {
    pub latest: u32,
    pub migrations: &'static [Migration],
}

/// This build's plan: [`MIGRATIONS`] up to [`LATEST`].
pub const PLAN: MigrationPlan = MigrationPlan { latest: LATEST, migrations: &MIGRATIONS };

impl MigrationPlan {
    /// Checks that the list is contiguous from version 0: migration `i` goes from `i` to
    /// `i + 1` and the last one reaches `latest`.
    pub fn validate(&self) -> Result<(), SchemaError> {
        let mut expected = 0u32;
        for m in self.migrations {
            let next = expected.checked_add(1);
            if m.from != expected || Some(m.to) != next {
                return Err(SchemaError::MigrationFailed {
                    from: m.from,
                    to: m.to,
                    detail: format!("plan is not contiguous: expected a migration from {expected}"),
                });
            }
            expected = m.to;
        }
        if expected != self.latest {
            return Err(SchemaError::MigrationFailed {
                from: expected,
                to: self.latest,
                detail: format!("plan ends at version {expected}, not {}", self.latest),
            });
        }
        Ok(())
    }
}

/// Errors of this module. [`SchemaError::code`] gives the spec's error name.
#[derive(Debug)]
#[non_exhaustive]
pub enum SchemaError {
    /// A trigger refused an UPDATE, DELETE or replacing INSERT on an append-only table.
    AppendOnly,
    /// The database is newer than this build can write.
    SchemaTooNew { found: u32, supported: u32 },
    /// A migration failed; the transaction was rolled back.
    MigrationFailed { from: u32, to: u32, detail: String },
    /// WAL journal mode could not be set on a write open (SQLite's answer or error).
    WalUnavailable(String),
    /// The file is not a project database this code understands (tables but no
    /// `schema_meta`, an empty `schema_meta`, a version out of range).
    SchemaInvalid(String),
    /// The database path cannot be given to SQLite (not valid Unicode).
    PathUnsupported(PathBuf),
    /// A layout-level failure (lock lost, symbolic link, path too long, ...).
    Store(StoreError),
    /// Any other SQLite error.
    Sqlite(rusqlite::Error),
}

impl SchemaError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            SchemaError::AppendOnly => APPEND_ONLY,
            SchemaError::SchemaTooNew { .. } => "schema_too_new",
            SchemaError::MigrationFailed { .. } => "migration_failed",
            SchemaError::WalUnavailable(_) => "wal_unavailable",
            SchemaError::SchemaInvalid(_) => "schema_invalid",
            SchemaError::PathUnsupported(_) => "path_unsupported",
            SchemaError::Store(e) => e.code(),
            SchemaError::Sqlite(_) => "sqlite",
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::AppendOnly => write!(f, "append_only: rows of this table cannot be changed or removed"),
            SchemaError::SchemaTooNew { found, supported } => {
                write!(f, "schema_too_new: database is at version {found}, this build supports up to {supported}")
            }
            SchemaError::MigrationFailed { from, to, detail } => {
                write!(f, "migration_failed: {from} -> {to}: {detail}")
            }
            SchemaError::WalUnavailable(why) => write!(f, "wal_unavailable: {why}"),
            SchemaError::SchemaInvalid(why) => write!(f, "schema_invalid: {why}"),
            SchemaError::PathUnsupported(p) => write!(f, "path_unsupported: {}", p.display()),
            SchemaError::Store(e) => write!(f, "{e}"),
            SchemaError::Sqlite(e) => write!(f, "sqlite: {e}"),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SchemaError::Store(e) => Some(e),
            SchemaError::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<StoreError> for SchemaError {
    fn from(e: StoreError) -> Self {
        SchemaError::Store(e)
    }
}

impl From<rusqlite::Error> for SchemaError {
    fn from(e: rusqlite::Error) -> Self {
        if is_append_only(&e) {
            SchemaError::AppendOnly
        } else {
            SchemaError::Sqlite(e)
        }
    }
}

/// True if `e` is the abort raised by an append-only trigger.
pub fn is_append_only(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, Some(msg))
        if f.code == ErrorCode::ConstraintViolation && msg == APPEND_ONLY)
}

/// An open project database at a known schema version.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
    version: SchemaVersion,
    mode: OpenMode,
}

impl Database {
    /// Schema version after open (and after migrations, for a write open).
    pub fn version(&self) -> SchemaVersion {
        self.version
    }

    /// Whether this is a write or a read-only open.
    pub fn mode(&self) -> OpenMode {
        self.mode
    }

    /// The connection, for `CORE.STO.SQL.TXN`. Raw statements on it still meet the triggers.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Mutable access to the connection (transactions need it).
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Opens `project.db` of a project for writing with this build's [`PLAN`]: sets WAL and
/// foreign keys, then migrates forward to [`LATEST`] (creating the v1 schema in a new project).
pub fn open_write(loc: &ProjectLocation, lock: &Locked) -> Result<Database, SchemaError> {
    open_write_with(loc, lock, &PLAN)
}

/// [`open_write`] with an explicit migration plan (tests use plans with later versions).
pub fn open_write_with(loc: &ProjectLocation, lock: &Locked, plan: &MigrationPlan) -> Result<Database, SchemaError> {
    plan.validate()?;
    lock.verify()?;
    let mut conn = connect(&loc.root().join(DB_FILE), OpenMode::Write)?;
    set_wal(&conn)?;
    let found = read_version(&conn)?;
    if found.0 > plan.latest {
        return Err(SchemaError::SchemaTooNew { found: found.0, supported: plan.latest });
    }
    let version = if found.0 < plan.latest { migrate(&mut conn, plan)? } else { found };
    Ok(Database { conn, version, mode: OpenMode::Write })
}

/// Opens `project.db` read-only. Applies no migration and accepts any version, including one
/// newer than this build; callers decide what they can read.
pub fn open_read(loc: &ProjectLocation) -> Result<Database, SchemaError> {
    let conn = connect(&loc.root().join(DB_FILE), OpenMode::Read)?;
    let version = read_version(&conn)?;
    Ok(Database { conn, version, mode: OpenMode::Read })
}

/// Checks SQLite's answer to `PRAGMA journal_mode=WAL`.
pub fn check_journal_mode(answer: &str) -> Result<(), SchemaError> {
    if answer.eq_ignore_ascii_case("wal") {
        Ok(())
    } else {
        Err(SchemaError::WalUnavailable(format!("journal mode stayed {answer}")))
    }
}

fn set_wal(conn: &Connection) -> Result<(), SchemaError> {
    let answer: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| SchemaError::WalUnavailable(e.to_string()))?;
    check_journal_mode(&answer)
}

/// Opens a connection with the per-open pragmas (foreign keys on, untrusted schema).
fn connect(path: &Path, mode: OpenMode) -> Result<Connection, SchemaError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(StoreError::NotAProject(format!("no {DB_FILE}")).into())
        }
        Err(e) => return Err(StoreError::Io(e).into()),
    };
    if meta.file_type().is_symlink() {
        return Err(StoreError::SymlinkRefused.into());
    }
    if !meta.is_file() {
        return Err(StoreError::NotRegularFile.into());
    }
    let access = match mode {
        OpenMode::Write => OpenFlags::SQLITE_OPEN_READ_WRITE,
        OpenMode::Read => OpenFlags::SQLITE_OPEN_READ_ONLY,
    };
    // No SQLITE_OPEN_CREATE (layout creates the file) and no SQLITE_OPEN_URI (a path is a path).
    let flags = access | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = open_path(path, flags)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk != 1 {
        return Err(SchemaError::SchemaInvalid("foreign keys could not be enabled".to_string()));
    }
    Ok(conn)
}

#[cfg(windows)]
fn open_path(path: &Path, flags: OpenFlags) -> Result<Connection, SchemaError> {
    let Some(s) = path.to_str() else { return Err(SchemaError::PathUnsupported(path.to_path_buf())) };
    let verbatim = crate::layout::windows_verbatim(s);
    Ok(Connection::open_with_flags_and_vfs(verbatim, flags, "win32-longpath")?)
}

#[cfg(not(windows))]
fn open_path(path: &Path, flags: OpenFlags) -> Result<Connection, SchemaError> {
    let bytes = path.as_os_str().len();
    if bytes > UNIX_MAX_DB_PATH_BYTES {
        let chars = crate::layout::path_chars(&path.to_string_lossy());
        return Err(StoreError::PathTooLong { chars }.into());
    }
    Ok(Connection::open_with_flags(path, flags)?)
}

/// Reads the schema version: 0 for a database without any schema object, else the largest
/// version in `schema_meta`.
pub fn read_version(conn: &Connection) -> Result<SchemaVersion, SchemaError> {
    let objects: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\'", [], |r| {
            r.get(0)
        })?;
    if objects == 0 {
        return Ok(SchemaVersion(0));
    }
    let has_meta: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_meta'", [], |r| {
            r.get(0)
        })?;
    if has_meta == 0 {
        return Err(SchemaError::SchemaInvalid("database has tables but no schema_meta".to_string()));
    }
    let max: Option<i64> = conn.query_row("SELECT max(version) FROM schema_meta", [], |r| r.get(0))?;
    let Some(max) = max else { return Err(SchemaError::SchemaInvalid("schema_meta is empty".to_string())) };
    match u32::try_from(max) {
        Ok(v) if v >= 1 => Ok(SchemaVersion(v)),
        _ => Err(SchemaError::SchemaInvalid(format!("schema version {max} is out of range"))),
    }
}

/// Applies every migration after the current version in one `BEGIN IMMEDIATE` transaction,
/// recording each version reached; any failure rolls the whole transaction back.
fn migrate(conn: &mut Connection, plan: &MigrationPlan) -> Result<SchemaVersion, SchemaError> {
    let fail = |from: u32, to: u32, detail: String| SchemaError::MigrationFailed { from, to, detail };
    // Starting the write transaction is not a migration step; its failure is SQLite's.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(SchemaError::Sqlite)?;
    // Re-read under the write transaction: this is the version the migrations start from.
    let current = read_version(&tx)?;
    if current.0 > plan.latest {
        return Err(SchemaError::SchemaTooNew { found: current.0, supported: plan.latest });
    }
    for m in plan.migrations.iter().filter(|m| m.from >= current.0) {
        tx.execute_batch(m.sql).map_err(|e| fail(m.from, m.to, e.to_string()))?;
        if tx.is_autocommit() {
            return Err(fail(m.from, m.to, "migration SQL ended the transaction".to_string()));
        }
        tx.execute("INSERT INTO schema_meta (version) VALUES (?1)", [m.to])
            .map_err(|e| fail(m.from, m.to, format!("recording version {}: {e}", m.to)))?;
    }
    let violations = count_rows(&tx, "PRAGMA foreign_key_check")
        .map_err(|e| fail(current.0, plan.latest, format!("foreign key check: {e}")))?;
    if violations != 0 {
        return Err(fail(current.0, plan.latest, format!("{violations} foreign key violations")));
    }
    tx.commit().map_err(|e| fail(current.0, plan.latest, format!("commit: {e}")))?;
    Ok(SchemaVersion(plan.latest))
}

fn count_rows(conn: &Connection, sql: &str) -> Result<u64, rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    let mut n = 0u64;
    while rows.next()?.is_some() {
        n = n.saturating_add(1);
    }
    Ok(n)
}
