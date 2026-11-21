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
//! Boundary: triggers stop data changes (DML) only. Schema changes made outside this crate
//! (`DROP TRIGGER`, `ALTER`/`DROP TABLE`, an extra trigger, `writable_schema` edits, a file
//! written by `VACUUM INTO` or `ATTACH`, raw byte edits, a crafted `-wal`) are caught at every
//! open by the schema check: the file's `sqlite_master` must equal, object for object (type,
//! name, table, SQL text), what this build's DDL produces for the stored version, or the open
//! fails with `schema_invalid` before anything is written. Changed row contents are the
//! business of the `CORE.JRN` hash chain, not of this module.
//!
//! Versions: `schema_meta` holds one row per schema version the database has reached, exactly
//! the versions 1..=current. A new, empty database is version 0, and migration 0 -> 1 is the
//! v1 DDL ([`V1_SQL`]). A write open applies every pending migration and records each new
//! version inside one `BEGIN IMMEDIATE` transaction, so a failure leaves the database at its
//! previous version; it never migrates backwards and refuses a newer database with
//! `schema_too_new` (a read open does too: a newer schema cannot be checked).
//!
//! Every connection is hardened before it reads anything: `SQLITE_DBCONFIG_DEFENSIVE` on,
//! `trusted_schema` off, `cell_size_check` on, foreign keys on, `ATTACH` unable to create or
//! write files, and checkpoint-on-close off until the schema check has passed (so a refused
//! file, or its `-wal`, is never folded into `project.db`). Extension loading stays off: it is
//! off by default in SQLite, and this crate builds `rusqlite` without `load_extension`, so no
//! code path can turn it on. A write open then sets WAL journal mode and fails with
//! `wal_unavailable` if SQLite does not confirm it. A read open is read-only and leaves the
//! journal mode unchanged.
//!
//! Files: a write open needs the project's write lock ([`Locked`], `CORE.STO.LAYOUT`) and
//! checks that it is still held. `project.db` must be a regular file and is opened with
//! `SQLITE_OPEN_NOFOLLOW` below the canonical project root; `project.db-wal`, `-shm` and
//! `-journal`, when present, must be regular files, not symbolic links, and a non-empty
//! `-journal` (a WAL-mode project never has one) fails with `schema_invalid` rather than being
//! rolled back into the database. Paths: on Windows the database is opened through SQLite's
//! `win32-longpath` VFS with a `\\?\` path, so project roots up to
//! [`crate::layout::MAX_PATH_CHARS`] characters work. SQLite's Unix VFS limits a path to
//! [`UNIX_MAX_DB_PATH_BYTES`] bytes [unverified: SQLite `MAX_PATHNAME`]; a longer database path
//! fails with `path_too_long` before SQLite is called.
//!
//! Shared hardening: other crates that open a hostile SQLite file of their own (the results
//! catalog, `RPT.CATALOG.STORE.SCHEMA`) get a connection only through [`open_hardened`], the
//! single guarded door: it checks the file and its companions, opens with
//! `SQLITE_OPEN_NOFOLLOW` (never `SQLITE_OPEN_CREATE` or `SQLITE_OPEN_URI`), applies every
//! hardening setting above and sets the busy timeout. The raw open, the hardening step and the
//! `sqlite3_db_config` setter stay private, so no dependent can obtain an unhardened connection
//! from this crate (`CORE.BUS.LINT`); the only setting a caller may change afterwards is
//! checkpoint-on-close, through [`allow_checkpoint`] once its own checks have passed. The checks
//! [`check_companions_of`], [`check_root_pages`], [`expected_objects`], [`verify_schema`],
//! [`quick_check`], [`set_wal`] and the migration runner [`migrate_with`] (with
//! [`MigrationHooks`]) are public so those crates apply exactly the same checks instead of
//! copies. `project.db` uses the same door and runner ([`StoreHooks`]) with unchanged
//! behaviour.
//!
//! The raw open and the hardening step are not part of the public API:
//!
//! ```compile_fail
//! use fid_store::schema::open_path;
//! ```
//!
//! ```compile_fail
//! use fid_store::schema::harden;
//! ```
//!
//! ```compile_fail
//! use fid_store::schema::set_config;
//! ```

use crate::layout::{OpenMode, ProjectLocation, StoreError, DB_FILE};
use crate::Locked;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The v1 DDL: tables, indexes and append-only triggers (no transaction control).
pub const V1_SQL: &str = include_str!("v1.sql");

/// Newest schema version this build writes and the newest it can open.
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

/// Companion files SQLite keeps next to `project.db`.
pub const COMPANIONS: [&str; 3] = ["-wal", "-shm", "-journal"];

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

/// The migration list an open checks against and a write open applies, ending at `latest`.
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
    /// The database is newer than this build can open.
    SchemaTooNew { found: u32, supported: u32 },
    /// A migration failed; the transaction was rolled back.
    MigrationFailed { from: u32, to: u32, detail: String },
    /// WAL journal mode could not be set on a write open (SQLite's answer or error).
    WalUnavailable(String),
    /// The file is not a project database this build wrote: schema objects differ from the
    /// DDL of the stored version, `schema_meta` is not exactly 1..=version, tables without
    /// `schema_meta`, or a leftover rollback journal.
    SchemaInvalid(String),
    /// A connection safety setting could not be applied or read back.
    HardeningFailed(String),
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
            SchemaError::HardeningFailed(_) => "hardening_failed",
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
            SchemaError::HardeningFailed(why) => write!(f, "hardening_failed: {why}"),
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

/// An open project database at a known, checked schema version.
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

/// Opens `project.db` of a project for writing with this build's [`PLAN`]: checks the
/// schema, sets WAL, then migrates forward to [`LATEST`] (creating the v1 schema in a new
/// project).
pub fn open_write(loc: &ProjectLocation, lock: &Locked) -> Result<Database, SchemaError> {
    open_write_with(loc, lock, &PLAN)
}

/// [`open_write`] with an explicit migration plan (tests use plans with later versions).
pub fn open_write_with(loc: &ProjectLocation, lock: &Locked, plan: &MigrationPlan) -> Result<Database, SchemaError> {
    plan.validate()?;
    lock.verify()?;
    let (mut conn, root) = connect(loc, OpenMode::Write)?;
    // Nothing is written before the file has passed these checks.
    let found = checked_version(&conn, plan, true)?;
    // SQLite has now opened any -wal/-shm; check them again (see check_companions).
    check_companions(&root)?;
    set_wal(&conn)?;
    let version = if found.0 < plan.latest { migrate(&mut conn, plan)? } else { found };
    // Checked: a normal close may now checkpoint the WAL into project.db.
    allow_checkpoint(&conn)?;
    Ok(Database { conn, version, mode: OpenMode::Write })
}

/// Opens `project.db` read-only after the same checks as a write open; applies no migration
/// and leaves the journal mode unchanged.
pub fn open_read(loc: &ProjectLocation) -> Result<Database, SchemaError> {
    open_read_with(loc, &PLAN)
}

/// [`open_read`] checked against an explicit plan.
pub fn open_read_with(loc: &ProjectLocation, plan: &MigrationPlan) -> Result<Database, SchemaError> {
    plan.validate()?;
    let (conn, root) = connect(loc, OpenMode::Read)?;
    let version = checked_version(&conn, plan, true)?;
    check_companions(&root)?;
    Ok(Database { conn, version, mode: OpenMode::Read })
}

/// Opens `project.db` of a read-only project folder (`CORE.STO.LAYOUT` REQ-011, a folder
/// resolved in an import root) so that no file is created or written: SQLite's `immutable=1`
/// with `mode=ro`, which takes no lock and never touches a `-wal` or `-shm` file. Such a file
/// already present means the database cannot be read without the WAL, so the open fails with
/// `read_only_unavailable`. The same schema and integrity checks as [`open_read`] run.
pub fn open_read_immutable(loc: &ProjectLocation) -> Result<Database, SchemaError> {
    PLAN.validate()?;
    let root = std::fs::canonicalize(loc.root()).map_err(StoreError::Io)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        match std::fs::symlink_metadata(root.join(format!("{DB_FILE}{suffix}"))) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(StoreError::Io(e).into()),
            Ok(_) => return Err(StoreError::ReadOnlyUnavailable.into()),
        }
    }
    let path = root.join(DB_FILE);
    let meta = std::fs::symlink_metadata(&path).map_err(StoreError::Io)?;
    if meta.file_type().is_symlink() {
        return Err(StoreError::SymlinkRefused.into());
    }
    if !meta.is_file() {
        return Err(StoreError::NotRegularFile.into());
    }
    let uri = immutable_uri(&path).ok_or(StoreError::ReadOnlyUnavailable)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(uri, flags)?;
    harden(&conn)?;
    let version = checked_version(&conn, &PLAN, true)?;
    Ok(Database { conn, version, mode: OpenMode::Read })
}

/// `file:` URI of a canonical absolute path with `mode=ro&immutable=1`. Every byte but
/// unreserved characters and `/` (and the drive colon on Windows) is percent-encoded, so no
/// path text becomes a URI parameter. `None` for a path that is not UTF-8 or a Windows UNC path.
fn immutable_uri(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    #[cfg(windows)]
    let owned = {
        if s.starts_with(r"\\?\UNC\") || (s.starts_with(r"\\") && !s.starts_with(r"\\?\")) {
            return None;
        }
        format!("/{}", s.strip_prefix(r"\\?\").unwrap_or(s).replace('\\', "/"))
    };
    #[cfg(windows)]
    let s = owned.as_str();
    let mut out = String::from("file://");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_' | b'~') || (cfg!(windows) && b == b':') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out.push_str("?mode=ro&immutable=1");
    Some(out)
}

/// The checks every open runs before anything is written, reading only `sqlite_master` until
/// `schema_meta` is known to be the v1 table: root pages, then the version (whose
/// `schema_meta` definition [`read_version`] checks first), a newer version refused, then the
/// full object comparison, then (`integrity`) `PRAGMA quick_check`. A file SQLite reports as
/// corrupt or not a database is `schema_invalid` too.
fn checked_version(conn: &Connection, plan: &MigrationPlan, integrity: bool) -> Result<SchemaVersion, SchemaError> {
    let run = || -> Result<SchemaVersion, SchemaError> {
        check_root_pages(conn)?;
        let found = read_version(conn)?;
        if found.0 > plan.latest {
            return Err(SchemaError::SchemaTooNew { found: found.0, supported: plan.latest });
        }
        verify_schema(conn, plan, found)?;
        if integrity {
            quick_check(conn)?;
        }
        Ok(found)
    };
    run().map_err(|e| match e {
        SchemaError::Sqlite(ref inner)
            if matches!(inner.sqlite_error_code(), Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)) =>
        {
            SchemaError::SchemaInvalid(format!("database is corrupt: {inner}"))
        }
        other => other,
    })
}

/// Every table and index must have a root page (virtual tables and damaged rows have none).
pub fn check_root_pages(conn: &Connection) -> Result<(), SchemaError> {
    let mut st = conn.prepare("SELECT type, name, rootpage FROM sqlite_master WHERE type IN ('table', 'index')")?;
    let rows =
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?)))?;
    for row in rows {
        let (kind, name, root) = row?;
        if !matches!(root, Some(n) if n > 0) {
            return Err(SchemaError::SchemaInvalid(format!("{kind} {name} has no root page")));
        }
    }
    Ok(())
}

/// `PRAGMA quick_check` must answer exactly `ok`.
pub fn quick_check(conn: &Connection) -> Result<(), SchemaError> {
    let mut st = conn.prepare("PRAGMA quick_check")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?;
    let answers = rows.collect::<Result<Vec<String>, _>>()?;
    if answers != ["ok"] {
        let first = answers.first().cloned().unwrap_or_default();
        return Err(SchemaError::SchemaInvalid(format!("quick_check: {first}")));
    }
    Ok(())
}

/// Checks SQLite's answer to `PRAGMA journal_mode=WAL`.
pub fn check_journal_mode(answer: &str) -> Result<(), SchemaError> {
    if answer.eq_ignore_ascii_case("wal") {
        Ok(())
    } else {
        Err(SchemaError::WalUnavailable(format!("journal mode stayed {answer}")))
    }
}

/// Sets WAL journal mode and checks SQLite's answer ([`check_journal_mode`]).
pub fn set_wal(conn: &Connection) -> Result<(), SchemaError> {
    let answer: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| SchemaError::WalUnavailable(e.to_string()))?;
    check_journal_mode(&answer)
}

/// Refuses companion files that are links or not regular files, and a non-empty rollback
/// journal (SQLite would roll it back into the database on the first read). Runs before SQLite
/// opens the database and again once the checks have read it (by then SQLite holds the -wal
/// and -shm it found). A local process that swaps a file in between is not stopped (see the
/// leaf's Open questions); the write lock keeps other Metrology Maven writers out.
fn check_companions(dir: &Path) -> Result<(), SchemaError> {
    check_companions_of(dir, DB_FILE)
}

/// [`check_companions`] for the database file `name` in `dir` (shared with other SQLite files
/// that are hostile input, such as the results catalog of `RPT.CATALOG.STORE.SCHEMA`).
pub fn check_companions_of(dir: &Path, name: &str) -> Result<(), SchemaError> {
    for suffix in COMPANIONS {
        let p = dir.join(format!("{name}{suffix}"));
        let meta = match std::fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(StoreError::Io(e).into()),
        };
        if meta.file_type().is_symlink() {
            return Err(StoreError::SymlinkRefused.into());
        }
        if !meta.is_file() {
            return Err(StoreError::NotRegularFile.into());
        }
        if suffix == "-journal" && meta.len() > 0 {
            return Err(SchemaError::SchemaInvalid(format!("leftover rollback journal {name}{suffix}")));
        }
    }
    Ok(())
}

/// Opens a hardened connection to `project.db` of `loc`; also returns the canonical root.
fn connect(loc: &ProjectLocation, mode: OpenMode) -> Result<(Connection, PathBuf), SchemaError> {
    // The user's path to the root may contain links (CORE.STO.LAYOUT); below it nothing may.
    let root = std::fs::canonicalize(loc.root()).map_err(StoreError::Io)?;
    let path = root.join(DB_FILE);
    let meta = match std::fs::symlink_metadata(&path) {
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
    let conn = open_hardened(&root, DB_FILE, mode, BUSY_TIMEOUT)?;
    Ok((conn, root))
}

/// The single guarded door to a SQLite file: opens the existing database file `name` in the
/// absolute directory `dir` and returns a hardened connection. `name` must be a plain file
/// name. Pass `dir` canonicalised: `SQLITE_OPEN_NOFOLLOW` refuses a symbolic link anywhere in
/// the path on Unix. The file must be a regular file, not a symbolic link, and its companions must pass
/// [`check_companions_of`]; it is opened with `SQLITE_OPEN_NOFOLLOW` and without
/// `SQLITE_OPEN_CREATE` or `SQLITE_OPEN_URI` (the caller creates the file; a path is a path),
/// read-write for [`OpenMode::Write`] and read-only for [`OpenMode::Read`]. Every hardening
/// setting of this module is then applied and read back before anything is read, including
/// no checkpoint on close (turn it off with [`allow_checkpoint`] once the file has passed the
/// caller's checks), and the busy timeout is set to `busy`.
pub fn open_hardened(dir: &Path, name: &str, mode: OpenMode, busy: Duration) -> Result<Connection, SchemaError> {
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && Path::new(name).file_name().is_some_and(|n| n == name);
    if !dir.is_absolute() || !plain {
        return Err(SchemaError::PathUnsupported(dir.join(name)));
    }
    let path = dir.join(name);
    let meta = std::fs::symlink_metadata(&path).map_err(StoreError::Io)?;
    if meta.file_type().is_symlink() {
        return Err(StoreError::SymlinkRefused.into());
    }
    if !meta.is_file() {
        return Err(StoreError::NotRegularFile.into());
    }
    check_companions_of(dir, name)?;
    let access = match mode {
        OpenMode::Write => OpenFlags::SQLITE_OPEN_READ_WRITE,
        OpenMode::Read => OpenFlags::SQLITE_OPEN_READ_ONLY,
    };
    // No SQLITE_OPEN_CREATE (the caller creates the file) and no SQLITE_OPEN_URI (a path is a
    // path).
    let flags = access | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let conn = open_path(&path, flags)?;
    harden(&conn)?;
    conn.busy_timeout(busy)?;
    Ok(conn)
}

/// Lets a normal close checkpoint the WAL into the database file again
/// (`SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE` off, read back). [`open_hardened`] leaves it on so a
/// refused file, or its `-wal`, is never folded into the database; call this only once the
/// file has passed every check of the open.
pub fn allow_checkpoint(conn: &Connection) -> Result<(), SchemaError> {
    set_config(conn, DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false, "no_ckpt_on_close")
}

/// Sets one `sqlite3_db_config` flag and reads it back; `hardening_failed` if it did not take.
fn set_config(conn: &Connection, what: DbConfig, on: bool, name: &str) -> Result<(), SchemaError> {
    let now = conn.set_db_config(what, on).map_err(|e| SchemaError::HardeningFailed(format!("{name}: {e}")))?;
    if now != on || conn.db_config(what).map_err(|e| SchemaError::HardeningFailed(format!("{name}: {e}")))? != on {
        return Err(SchemaError::HardeningFailed(format!("{name} did not become {on}")));
    }
    Ok(())
}

/// Connection safety settings, applied before the connection reads the file and read back.
/// Leaves `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE` on: the caller turns it off with
/// [`allow_checkpoint`] once its checks have passed.
fn harden(conn: &Connection) -> Result<(), SchemaError> {
    set_config(conn, DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true, "defensive")?;
    set_config(conn, DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false, "trusted_schema")?;
    set_config(conn, DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA, false, "writable_schema")?;
    set_config(conn, DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_CREATE, false, "attach_create")?;
    set_config(conn, DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_WRITE, false, "attach_write")?;
    set_config(conn, DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, true, "enable_trigger")?;
    set_config(conn, DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true, "enable_fkey")?;
    // A refused file must not be checkpointed into project.db; a write open turns this back
    // off once the schema check has passed.
    set_config(conn, DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true, "no_ckpt_on_close")?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA cell_size_check=ON;")
        .map_err(|e| SchemaError::HardeningFailed(e.to_string()))?;
    for (pragma, want) in [("foreign_keys", 1i64), ("trusted_schema", 0), ("cell_size_check", 1)] {
        let got: i64 = conn
            .query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0))
            .map_err(|e| SchemaError::HardeningFailed(format!("{pragma}: {e}")))?;
        if got != want {
            return Err(SchemaError::HardeningFailed(format!("{pragma} is {got}, expected {want}")));
        }
    }
    Ok(())
}

/// Opens the database file at `path` (absolute) with `flags`: on Windows through the
/// `win32-longpath` VFS with a `\\?\` path; elsewhere refusing paths over
/// [`UNIX_MAX_DB_PATH_BYTES`] with `path_too_long` before SQLite is called.
#[cfg(windows)]
fn open_path(path: &Path, flags: OpenFlags) -> Result<Connection, SchemaError> {
    let Some(s) = path.to_str() else { return Err(SchemaError::PathUnsupported(path.to_path_buf())) };
    let verbatim = crate::layout::windows_verbatim(s);
    Ok(Connection::open_with_flags_and_vfs(verbatim, flags, "win32-longpath")?)
}

/// See the Windows variant.
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
/// version in `schema_meta`, whose rows must be exactly 1..=version.
///
/// `schema_meta` is queried only after its `sqlite_master` rows (the table and everything
/// attached to it) equal, byte for byte, those the v1 DDL creates, so a hostile file cannot
/// have a virtual table, a generated column or any other construct evaluated by this read.
/// `schema_meta`'s definition is therefore fixed for all versions.
pub fn read_version(conn: &Connection) -> Result<SchemaVersion, SchemaError> {
    let objects: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
    if objects == 0 {
        return Ok(SchemaVersion(0));
    }
    let meta_rows = |c: &Connection| -> Result<Vec<SchemaObject>, rusqlite::Error> {
        let mut st = c.prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master \
             WHERE name = 'schema_meta' OR tbl_name = 'schema_meta' ORDER BY type, name, tbl_name",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect()
    };
    let got = meta_rows(conn)?;
    if got.is_empty() {
        return Err(SchemaError::SchemaInvalid("database has schema objects but no schema_meta table".to_string()));
    }
    let mem = Connection::open_in_memory()?;
    mem.execute_batch(V1_SQL)?;
    if got != meta_rows(&mem)? {
        return Err(SchemaError::SchemaInvalid(
            "schema_meta is not the table the v1 DDL defines; it was not read".to_string(),
        ));
    }
    let (rows, distinct, min, max): (i64, i64, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT count(*), count(DISTINCT version), min(version), max(version) FROM schema_meta",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let (Some(min), Some(max)) = (min, max) else {
        return Err(SchemaError::SchemaInvalid("schema_meta is empty".to_string()));
    };
    if min != 1 || max != rows || distinct != rows {
        return Err(SchemaError::SchemaInvalid(format!(
            "schema_meta must hold exactly the versions 1..={max}: {rows} rows, {distinct} distinct, min {min}"
        )));
    }
    match u32::try_from(max) {
        Ok(v) => Ok(SchemaVersion(v)),
        Err(_) => Err(SchemaError::SchemaInvalid(format!("schema version {max} is out of range"))),
    }
}

/// One `sqlite_master` row: type, name, table, SQL text.
pub type SchemaObject = (String, String, String, Option<String>);

fn schema_objects(conn: &Connection) -> Result<Vec<SchemaObject>, rusqlite::Error> {
    let mut st = conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name, tbl_name")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect()
}

/// The schema objects the plan's DDL produces at `version`, built in a private in-memory
/// database.
pub fn expected_objects(plan: &MigrationPlan, version: SchemaVersion) -> Result<Vec<SchemaObject>, SchemaError> {
    let mem = Connection::open_in_memory()?;
    for m in plan.migrations.iter().filter(|m| m.to <= version.0) {
        mem.execute_batch(m.sql).map_err(|e| SchemaError::MigrationFailed {
            from: m.from,
            to: m.to,
            detail: format!("building the expected schema: {e}"),
        })?;
    }
    Ok(schema_objects(&mem)?)
}

/// Checks that the file's schema objects are exactly those of `version` under `plan`.
pub fn verify_schema(conn: &Connection, plan: &MigrationPlan, version: SchemaVersion) -> Result<(), SchemaError> {
    let want = expected_objects(plan, version)?;
    let got = schema_objects(conn)?;
    if got == want {
        return Ok(());
    }
    let describe = |o: &SchemaObject| format!("{} {} on {}", o.0, o.1, o.2);
    let detail = if let Some(o) = want.iter().find(|o| !got.iter().any(|g| g.0 == o.0 && g.1 == o.1)) {
        format!("missing {}", describe(o))
    } else if let Some(o) = got.iter().find(|o| !want.iter().any(|w| w.0 == o.0 && w.1 == o.1)) {
        format!("unexpected {}", describe(o))
    } else if let Some(o) = got.iter().find(|o| !want.contains(o)) {
        format!("changed {}", describe(o))
    } else {
        "schema objects differ".to_string()
    };
    Err(SchemaError::SchemaInvalid(format!("{detail} (expected the schema of version {})", version.0)))
}

/// What a database format supplies to [`migrate_with`]: how its version is checked, read and
/// recorded, and how a failed step is reported. `project.db` records versions in `schema_meta`
/// ([`StoreHooks`]); other formats (the results catalog) keep them elsewhere.
pub trait MigrationHooks {
    /// The error type of the format.
    type Error: From<rusqlite::Error> + From<SchemaError> + fmt::Display;
    /// Where the version is recorded, for error details (`schema_meta` for `project.db`).
    const VERSION_TABLE: &'static str;
    /// The checks an open runs, under the write transaction: the version migrations start from.
    fn checked_version(&mut self, conn: &Connection, plan: &MigrationPlan) -> Result<SchemaVersion, Self::Error>;
    /// The version recorded after the migrations.
    fn read_version(&mut self, conn: &Connection) -> Result<SchemaVersion, Self::Error>;
    /// Records, inside the transaction, that `m` was applied.
    fn record(&mut self, conn: &Connection, m: &Migration) -> Result<(), Self::Error>;
    /// The error of a failed step (`from` -> `to`).
    fn failed(&self, from: u32, to: u32, detail: String) -> Self::Error;
    /// The error of the failed SQLite call `what` of the run (`from` -> `to`), such as the
    /// commit. The default is [`MigrationHooks::failed`] with `"{what}: {e}"`; a format may map
    /// a busy or locked database to its own lock error instead.
    fn sqlite_failed(&self, from: u32, to: u32, what: &str, e: rusqlite::Error) -> Self::Error {
        self.failed(from, to, format!("{what}: {e}"))
    }
}

/// The hooks of `project.db`: versions in `schema_meta`, one row per version reached.
#[derive(Debug, Clone, Copy, Default)]
pub struct StoreHooks;

impl MigrationHooks for StoreHooks {
    type Error = SchemaError;
    const VERSION_TABLE: &'static str = "schema_meta";

    fn checked_version(&mut self, conn: &Connection, plan: &MigrationPlan) -> Result<SchemaVersion, SchemaError> {
        checked_version(conn, plan, false)
    }

    fn read_version(&mut self, conn: &Connection) -> Result<SchemaVersion, SchemaError> {
        read_version(conn)
    }

    fn record(&mut self, conn: &Connection, m: &Migration) -> Result<(), SchemaError> {
        conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [m.to])
            .map(|_| ())
            .map_err(|e| self.failed(m.from, m.to, format!("recording version {}: {e}", m.to)))
    }

    fn failed(&self, from: u32, to: u32, detail: String) -> SchemaError {
        SchemaError::MigrationFailed { from, to, detail }
    }
}

/// Applies every migration after the current version in one `BEGIN IMMEDIATE` transaction,
/// recording each version reached; any failure rolls the whole transaction back.
fn migrate(conn: &mut Connection, plan: &MigrationPlan) -> Result<SchemaVersion, SchemaError> {
    migrate_with(conn, plan, &mut StoreHooks)
}

/// The migration runner shared by every SQLite format: in one `BEGIN IMMEDIATE` transaction,
/// re-checks the version ([`MigrationHooks::checked_version`]), applies each pending migration
/// and records it, checks foreign keys, then requires the recorded version to be
/// `plan.latest` and the schema objects to be exactly that version's ([`verify_schema`]).
/// Any failure rolls the whole transaction back.
pub fn migrate_with<H: MigrationHooks>(
    conn: &mut Connection,
    plan: &MigrationPlan,
    hooks: &mut H,
) -> Result<SchemaVersion, H::Error> {
    // Starting the write transaction is not a migration step; its failure is SQLite's.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(H::Error::from)?;
    // Re-read under the write transaction: this is the version the migrations start from.
    let current = hooks.checked_version(&tx, plan)?;
    for m in plan.migrations.iter().filter(|m| m.from >= current.0) {
        tx.execute_batch(m.sql).map_err(|e| hooks.failed(m.from, m.to, e.to_string()))?;
        if tx.is_autocommit() {
            return Err(hooks.failed(m.from, m.to, "migration SQL ended the transaction".to_string()));
        }
        hooks.record(&tx, m)?;
    }
    let violations = count_rows(&tx, "PRAGMA foreign_key_check")
        .map_err(|e| hooks.failed(current.0, plan.latest, format!("foreign key check: {e}")))?;
    if violations != 0 {
        return Err(hooks.failed(current.0, plan.latest, format!("{violations} foreign key violations")));
    }
    // The result must be exactly the schema of the new version.
    let reached = match hooks.read_version(&tx) {
        Ok(v) => v,
        Err(e) => return Err(hooks.failed(current.0, plan.latest, e.to_string())),
    };
    if reached.0 != plan.latest {
        return Err(hooks.failed(current.0, plan.latest, format!("{} reached {}", H::VERSION_TABLE, reached.0)));
    }
    verify_schema(&tx, plan, reached).map_err(|e| hooks.failed(current.0, plan.latest, e.to_string()))?;
    tx.commit().map_err(|e| hooks.sqlite_failed(current.0, plan.latest, "commit", e))?;
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
