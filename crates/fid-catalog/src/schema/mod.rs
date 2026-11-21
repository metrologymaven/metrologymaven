//! The results catalog database: schema, forward migrations and the open function (spec node
//! `RPT.CATALOG.STORE.SCHEMA`).
//!
//! The catalog is one local SQLite file, separate from every project database, that indexes
//! ResultRecord summaries of registered projects. Tables: `catalog_meta` (one row: schema
//! version, catalog ID, owner), `project`, `entry` (one row per ResultRecord, unique on
//! (project ID, record ID), always carrying the source journal entry hash) and `entry_tag`.
//! Indexes: (part, characteristic, timestamp, record ID) and (project, journal position).
//!
//! **Untrusted file.** A catalog file may have been edited by anyone, so every open checks it
//! before reading a row: no symbolic link for the file or its `-wal`, `-shm`, `-journal`; no
//! non-empty rollback journal; every table and index has a root page; `catalog_meta` is
//! exactly the table the v1 DDL defines before it is queried; then the file's `sqlite_master`
//! must equal, object for object (type, name, table, SQL text), what this build's DDL produces
//! for the stored version. Anything else is `stats.catalog_corrupt` and nothing is written.
//! Connections come only from `fid-store`'s single guarded door, `fid_store::schema::open_hardened`
//! (file and companion checks, `SQLITE_OPEN_NOFOLLOW`, no `SQLITE_OPEN_CREATE` or
//! `SQLITE_OPEN_URI`; defensive mode, `trusted_schema` off, no `ATTACH` that creates or writes,
//! cell size checks, foreign keys on, no checkpoint on close until the checks pass and
//! `allow_checkpoint` is called), and the catalog uses `fid-store`'s own companion-file,
//! root-page, schema-object, `quick_check` and migration-runner code (`fid_store::schema`), so
//! the catalog and `project.db` get exactly the same checks. A write open also runs
//! `PRAGMA quick_check` before it sets WAL mode, and again under the migration's write
//! transaction before rows of an existing catalog are carried forward; a busy commit is
//! `stats.catalog_locked`. A read-only open relies on SQLite's corruption errors, which are
//! `stats.catalog_corrupt`. Tables are `STRICT` and every text
//! column from project content is bounded in bytes by a `CHECK`, so the bounds hold for any
//! connection; rows read back are checked again and wrapped as [`CatalogText`].
//!
//! **Versions.** A file with no schema object is version 0 (new). Migration `i` takes the
//! schema from `i` to `i + 1` ([`MIGRATIONS`]); a write open applies every pending migration
//! and records the version reached in `catalog_meta`, all inside one `BEGIN IMMEDIATE`
//! transaction, so a failure leaves the file at its previous version. A file newer than
//! [`LATEST`] is refused with `stats.catalog_too_new` before anything is written, by a write
//! and by a read-only open. A read-only open never migrates: an older file is refused with
//! `stats.catalog_read_only` (open it for writing first).
//!
//! **Creation.** A write open of a missing file creates its directory, creates the file
//! exclusively (never through a link) and migrates it from version 0, which writes the
//! `catalog_meta` row with a new UUIDv7 catalog ID drawn from the caller's
//! [`ObservationSource`] (REQ-005). Opening needs no network service and no account (REQ-004):
//! the only I/O is the local file.

pub mod row;

pub use row::{
    Availability, CatalogEntryRow, CatalogText, CatalogTextDisplay, DecisionOutcome, EntryStatus, EntryValue,
    EvaluationBasis, EvidenceLabels, Interpretation, ProjectRow, RecordStatus, UecClass,
};

use fid_project::{uuid_v7, ObservationSource, SystemSource};
pub use fid_store::schema::{Migration, MigrationPlan, SchemaObject};
use fid_types::Digest;
use fid_uuid::Uuid;
use row::escaped;

use fid_store::schema::{
    allow_checkpoint, check_companions_of, check_root_pages, migrate_with, open_hardened, quick_check, set_wal,
    verify_schema, MigrationHooks,
};
use fid_store::{OpenMode, SchemaError, SchemaVersion, StoreError};
use rusqlite::types::Value;
use rusqlite::{params, Connection, ErrorCode};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The v1 DDL (no transaction control).
pub const V1_SQL: &str = include_str!("v1.sql");
/// The v1 -> v2 migration (no transaction control).
pub const V2_SQL: &str = include_str!("v2.sql");

/// Newest schema version this build writes and the newest it opens.
pub const LATEST: u32 = 2;

/// The tables of the latest schema, in DDL order.
pub const TABLES: [&str; 4] = ["catalog_meta", "project", "entry", "entry_tag"];

/// Longest owner label, in bytes (as the `catalog_meta` check).
pub const MAX_OWNER_BYTES: usize = 256;

/// Most rows one [`Catalog::entries_after`] call returns.
pub const MAX_PAGE: usize = 10_000;

/// How long a connection waits for a lock held by another connection, by default.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The migrations compiled into this build: migration `i` goes from version `i` to `i + 1`.
pub static MIGRATIONS: [Migration; 2] =
    [Migration { from: 0, to: 1, sql: V1_SQL }, Migration { from: 1, to: 2, sql: V2_SQL }];

/// This build's plan: [`MIGRATIONS`] up to [`LATEST`].
pub const PLAN: MigrationPlan = MigrationPlan { latest: LATEST, migrations: &MIGRATIONS };

/// Checks a plan for the catalog: contiguous from version 0 to a `latest` of at least 1
/// (`MigrationPlan::validate`), and migration 0 -> 1 is this crate's v1 DDL (it defines
/// `catalog_meta`, which every version reads the same way).
pub fn validate_plan(plan: &MigrationPlan) -> Result<(), CatalogError> {
    plan.validate()?;
    if plan.latest == 0 || plan.migrations.first().map(|m| m.sql) != Some(V1_SQL) {
        return Err(CatalogError::MigrationFailed {
            from: 0,
            to: 1,
            detail: "migration 0 -> 1 is not the v1 DDL".into(),
        });
    }
    Ok(())
}

/// How the catalog is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Read only; never migrates, never creates.
    ReadOnly,
    /// Read and write; creates a missing catalog and applies pending migrations.
    ReadWrite,
}

/// Errors of the catalog. [`CatalogError::code`] gives the stable code.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CatalogError {
    /// The file's schema is newer than this build supports (REQ-002).
    TooNew { found: u32, supported: u32 },
    /// The file is not a catalog this build wrote, or it is damaged.
    Corrupt(String),
    /// Another connection holds the lock the operation needs.
    Locked,
    /// A write was attempted on a read-only open, or a read-only open needs a migration.
    ReadOnly(String),
    /// A read-only open of a file that does not exist or was never initialised.
    Missing(String),
    /// The configured location cannot hold a catalog (relative, a link, not a file, ...).
    PathInvalid(String),
    /// A migration failed; its transaction was rolled back.
    MigrationFailed { from: u32, to: u32, detail: String },
    /// A row was refused by a table constraint (unique key, check, foreign key).
    Constraint(String),
    /// Any other I/O or SQLite failure.
    Io(String),
}

impl CatalogError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CatalogError::TooNew { .. } => "stats.catalog_too_new",
            CatalogError::Corrupt(_) => "stats.catalog_corrupt",
            CatalogError::Locked => "stats.catalog_locked",
            CatalogError::ReadOnly(_) => "stats.catalog_read_only",
            CatalogError::Missing(_) => "stats.catalog_missing",
            CatalogError::PathInvalid(_) => "stats.catalog_path_invalid",
            CatalogError::MigrationFailed { .. } => "stats.catalog_migration_failed",
            CatalogError::Constraint(_) => "stats.catalog_constraint",
            CatalogError::Io(_) => "stats.catalog_io",
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            CatalogError::TooNew { found, supported } => {
                write!(f, "{code}: catalog is at schema version {found}, this build supports up to {supported}")
            }
            CatalogError::Locked => write!(f, "{code}: another connection holds the catalog lock"),
            CatalogError::MigrationFailed { from, to, detail } => write!(f, "{code}: {from} -> {to}: {detail}"),
            CatalogError::Corrupt(d)
            | CatalogError::ReadOnly(d)
            | CatalogError::Missing(d)
            | CatalogError::PathInvalid(d)
            | CatalogError::Constraint(d)
            | CatalogError::Io(d) => write!(f, "{code}: {d}"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<rusqlite::Error> for CatalogError {
    fn from(e: rusqlite::Error) -> CatalogError {
        match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => CatalogError::Locked,
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
                CatalogError::Corrupt(format!("database is corrupt: {e}"))
            }
            Some(ErrorCode::ConstraintViolation) => CatalogError::Constraint(escaped(&e.to_string())),
            Some(ErrorCode::ReadOnly) => CatalogError::ReadOnly(e.to_string()),
            _ => CatalogError::Io(escaped(&e.to_string())),
        }
    }
}

impl From<SchemaError> for CatalogError {
    fn from(e: SchemaError) -> CatalogError {
        match e {
            SchemaError::SchemaTooNew { found, supported } => CatalogError::TooNew { found, supported },
            SchemaError::SchemaInvalid(d) => CatalogError::Corrupt(escaped(&d)),
            SchemaError::MigrationFailed { from, to, detail } => {
                CatalogError::MigrationFailed { from, to, detail: escaped(&detail) }
            }
            SchemaError::Sqlite(e) => CatalogError::from(e),
            SchemaError::AppendOnly => CatalogError::Constraint("append_only".into()),
            SchemaError::PathUnsupported(_) => {
                CatalogError::PathInvalid("the catalog path is not valid Unicode".into())
            }
            SchemaError::Store(e) => match e {
                StoreError::SymlinkRefused => {
                    CatalogError::PathInvalid("the catalog file or a companion file is a symbolic link".into())
                }
                StoreError::NotRegularFile => {
                    CatalogError::PathInvalid("the catalog file or a companion file is not a regular file".into())
                }
                StoreError::PathTooLong { .. } => CatalogError::PathInvalid("the catalog path is too long".into()),
                other => CatalogError::Io(escaped(&other.to_string())),
            },
            other => CatalogError::Io(escaped(&other.to_string())),
        }
    }
}

fn io_err(what: &str, e: std::io::Error) -> CatalogError {
    CatalogError::Io(format!("{what}: {e}"))
}

/// Everything an open may take besides the path and mode.
pub struct OpenOptions<'a> {
    /// Time and randomness of the catalog ID of a new catalog.
    pub source: &'a mut dyn ObservationSource,
    /// Owner label written into a new catalog (`None`: no owner recorded). Ignored when the
    /// catalog exists.
    pub owner: Option<&'a str>,
    /// The migrations to check against and apply.
    pub plan: MigrationPlan,
    /// How long to wait for another connection's lock.
    pub busy_timeout: Duration,
}

impl fmt::Debug for OpenOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenOptions")
            .field("owner", &self.owner)
            .field("plan", &self.plan)
            .field("busy_timeout", &self.busy_timeout)
            .finish_non_exhaustive()
    }
}

/// An open catalog at a checked schema version.
#[derive(Debug)]
pub struct Catalog {
    conn: Connection,
    path: PathBuf,
    mode: Mode,
    version: u32,
    catalog_id: Uuid,
    owner: Option<CatalogText>,
}

/// One `catalog_meta` row, checked.
struct Meta {
    version: u32,
    catalog_id: Uuid,
    owner: Option<CatalogText>,
}

impl Catalog {
    /// Opens the catalog at `path` with this build's [`PLAN`], the system clock and random
    /// source for a new catalog ID, no owner label and [`BUSY_TIMEOUT`].
    pub fn open(path: &Path, mode: Mode) -> Result<Catalog, CatalogError> {
        let mut source = SystemSource;
        Catalog::open_with(
            path,
            mode,
            OpenOptions { source: &mut source, owner: None, plan: PLAN, busy_timeout: BUSY_TIMEOUT },
        )
    }

    /// [`Catalog::open`] with explicit options.
    pub fn open_with(path: &Path, mode: Mode, opts: OpenOptions<'_>) -> Result<Catalog, CatalogError> {
        let OpenOptions { source, owner, plan, busy_timeout } = opts;
        validate_plan(&plan)?;
        if let Some(o) = owner {
            if o.is_empty() || o.len() > MAX_OWNER_BYTES {
                return Err(CatalogError::Constraint(format!("owner label must be 1 to {MAX_OWNER_BYTES} bytes")));
            }
        }
        let file = locate(path, mode)?;
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = file_name(&file)?;
        let access = match mode {
            Mode::ReadWrite => OpenMode::Write,
            Mode::ReadOnly => OpenMode::Read,
        };
        // fid-store's single guarded door: file and companion checks, NOFOLLOW (no CREATE: locate
        // creates the file; no URI), every hardening setting, the busy timeout.
        let mut conn = open_hardened(&dir, &name, access, busy_timeout)?;
        // Nothing is written before the file has passed these checks.
        let found = checked_version(&conn, &plan)?;
        // SQLite has now opened any -wal/-shm; check them again.
        check_companions_of(&dir, &name)?;
        let version = match mode {
            Mode::ReadOnly => {
                if found == 0 {
                    return Err(CatalogError::Missing("the catalog file was never initialised".into()));
                }
                if found < plan.latest {
                    return Err(CatalogError::ReadOnly(format!(
                        "catalog is at schema version {found} and needs a migration to {}; open it for writing",
                        plan.latest
                    )));
                }
                found
            }
            Mode::ReadWrite => {
                // The whole file must be sound before WAL is set or a row is carried forward.
                quick_check(&conn)?;
                set_wal(&conn)?;
                let v = if found < plan.latest {
                    let mut hooks = CatalogHooks { source, owner };
                    migrate_with(&mut conn, &plan, &mut hooks)?.0
                } else {
                    found
                };
                // Checked: a normal close may now checkpoint the WAL into the catalog file.
                allow_checkpoint(&conn)?;
                v
            }
        };
        let meta = read_meta(&conn)?;
        if meta.version != version {
            return Err(CatalogError::Corrupt(format!(
                "catalog_meta reads version {}, expected {version}",
                meta.version
            )));
        }
        Ok(Catalog { conn, path: file, mode, version, catalog_id: meta.catalog_id, owner: meta.owner })
    }

    /// Schema version after open (and after migrations, for a write open).
    pub fn schema_version(&self) -> u32 {
        self.version
    }

    /// The catalog ID (UUIDv7) written when the catalog was created.
    pub fn catalog_id(&self) -> Uuid {
        self.catalog_id
    }

    /// The owner label written when the catalog was created, if any.
    pub fn owner(&self) -> Option<&CatalogText> {
        self.owner.as_ref()
    }

    /// The opened file (its directory canonicalised).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How the catalog was opened.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The connection, for the sibling leaves of this crate (registration, indexing, query,
    /// check). Raw statements still meet the table constraints.
    #[allow(dead_code)]
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Mutable access to the connection (transactions need it).
    #[allow(dead_code)]
    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    fn writable(&self) -> Result<(), CatalogError> {
        match self.mode {
            Mode::ReadWrite => Ok(()),
            Mode::ReadOnly => Err(CatalogError::ReadOnly("the catalog was opened read-only".into())),
        }
    }

    /// Inserts a `project` row. A second row with the same project ID or handle (ignoring
    /// ASCII case) is `stats.catalog_constraint`.
    pub fn insert_project(&mut self, p: &ProjectRow) -> Result<(), CatalogError> {
        self.writable()?;
        let (seq, head) = match &p.indexed {
            Some((s, h)) => (Some(seq_to_sql(*s)?), Some(h.as_str())),
            None => (None, None),
        };
        self.conn.execute(
            "INSERT INTO project (project_id, handle, last_seq, head_hash, availability) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.project_id.to_string(), p.handle.raw(), seq, head, p.availability.as_str()],
        )?;
        Ok(())
    }

    /// Inserts an `entry` row. A second row with the same (project ID, record ID), a project
    /// that is not registered, or a row a table check refuses is `stats.catalog_constraint`.
    pub fn insert_entry(&mut self, e: &CatalogEntryRow) -> Result<(), CatalogError> {
        self.writable()?;
        let (literal, unit, si) = match &e.value {
            Some(v) => {
                if !v.si.is_finite() {
                    return Err(CatalogError::Constraint("si_value must be finite".into()));
                }
                // One stored form for zero.
                let si = if v.si == 0.0 { 0.0 } else { v.si };
                (Some(v.literal.raw()), Some(v.unit.raw()), Some(si))
            }
            None => (None, None, None),
        };
        self.conn.execute(
            "INSERT INTO entry (project_id, record_id, part, revision, plan, run, stage, characteristic, timestamp, \
             value_literal, unit, si_value, decision_outcome, rule_id, evaluation_basis, uec_class, claimed, \
             declared_input, uncharacterized_input, interpretation, record_status, journal_seq, journal_entry_hash, \
             entry_status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, \
             ?22, ?23, ?24)",
            params![
                e.project_id.to_string(),
                e.record_id.raw(),
                e.part.raw(),
                e.revision.as_ref().map(CatalogText::raw),
                e.plan.raw(),
                e.run.raw(),
                e.stage.raw(),
                e.characteristic.raw(),
                e.timestamp.raw(),
                literal,
                unit,
                si,
                e.decision_outcome.as_str(),
                e.rule_id.as_ref().map(CatalogText::raw),
                e.evaluation_basis.as_str(),
                e.uec_class.as_str(),
                i64::from(e.labels.claimed),
                i64::from(e.labels.declared_input),
                i64::from(e.labels.uncharacterized_input),
                e.interpretation.as_str(),
                e.record_status.as_str(),
                seq_to_sql(e.journal_seq)?,
                e.journal_entry_hash.as_str(),
                e.entry_status.as_str(),
            ],
        )?;
        Ok(())
    }

    /// The `project` row with this ID.
    pub fn project(&self, id: &Uuid) -> Result<Option<ProjectRow>, CatalogError> {
        let mut st = self.conn.prepare(
            "SELECT project_id, handle, last_seq, head_hash, availability FROM project WHERE project_id = ?1",
        )?;
        let mut rows = st.query([id.to_string()])?;
        match rows.next()? {
            Some(r) => Ok(Some(decode_project(r)?)),
            None => Ok(None),
        }
    }

    /// Every `project` row, sorted by project ID.
    pub fn projects(&self) -> Result<Vec<ProjectRow>, CatalogError> {
        let mut st = self
            .conn
            .prepare("SELECT project_id, handle, last_seq, head_hash, availability FROM project ORDER BY project_id")?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(decode_project(r)?);
        }
        Ok(out)
    }

    /// The `entry` row with this key.
    pub fn entry(&self, project: &Uuid, record: &CatalogText) -> Result<Option<CatalogEntryRow>, CatalogError> {
        let sql = format!("SELECT {ENTRY_COLUMNS} FROM entry WHERE project_id = ?1 AND record_id = ?2");
        let mut st = self.conn.prepare(&sql)?;
        let mut rows = st.query(params![project.to_string(), record.raw()])?;
        match rows.next()? {
            Some(r) => Ok(Some(decode_entry(r)?)),
            None => Ok(None),
        }
    }

    /// Up to `limit` (at most [`MAX_PAGE`]) entries in (project ID, record ID) order, both
    /// compared bytewise, starting after `after` (exclusive) or at the first entry. The order
    /// does not depend on insertion order.
    pub fn entries_after(
        &self,
        after: Option<(&Uuid, &CatalogText)>,
        limit: usize,
    ) -> Result<Vec<CatalogEntryRow>, CatalogError> {
        let limit = i64::try_from(limit.min(MAX_PAGE)).unwrap_or(0);
        let (p, r) = match after {
            Some((p, r)) => (Value::Text(p.to_string()), Value::Text(r.raw().to_string())),
            None => (Value::Null, Value::Null),
        };
        let sql = format!(
            "SELECT {ENTRY_COLUMNS} FROM entry \
             WHERE ?1 IS NULL OR project_id > ?1 OR (project_id = ?1 AND record_id > ?2) \
             ORDER BY project_id, record_id LIMIT ?3"
        );
        let mut st = self.conn.prepare(&sql)?;
        let mut rows = st.query(params![p, r, limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(decode_entry(row)?);
        }
        Ok(out)
    }

    /// Number of `entry` rows.
    pub fn count_entries(&self) -> Result<u64, CatalogError> {
        let n: i64 = self.conn.query_row("SELECT count(*) FROM entry", [], |r| r.get(0))?;
        u64::try_from(n).map_err(|_| CatalogError::Corrupt("negative row count".into()))
    }

    /// Adds `tag` to an entry. A tag already present is `stats.catalog_constraint`.
    pub fn insert_tag(&mut self, project: &Uuid, record: &CatalogText, tag: &CatalogText) -> Result<(), CatalogError> {
        self.writable()?;
        self.conn.execute(
            "INSERT INTO entry_tag (project_id, record_id, tag) VALUES (?1, ?2, ?3)",
            params![project.to_string(), record.raw(), tag.raw()],
        )?;
        Ok(())
    }

    /// The tags of an entry, sorted bytewise.
    pub fn tags(&self, project: &Uuid, record: &CatalogText) -> Result<Vec<CatalogText>, CatalogError> {
        let mut st =
            self.conn.prepare("SELECT tag FROM entry_tag WHERE project_id = ?1 AND record_id = ?2 ORDER BY tag")?;
        let mut rows = st.query(params![project.to_string(), record.raw()])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(CatalogText::from_string(text(r, 0)?));
        }
        Ok(out)
    }
}

/// The `entry` columns in DDL order, as decoded by [`decode_entry`].
const ENTRY_COLUMNS: &str = "project_id, record_id, part, revision, plan, run, stage, characteristic, timestamp, \
     value_literal, unit, si_value, decision_outcome, rule_id, evaluation_basis, uec_class, claimed, declared_input, \
     uncharacterized_input, interpretation, record_status, journal_seq, journal_entry_hash, entry_status";

fn seq_to_sql(seq: u64) -> Result<i64, CatalogError> {
    i64::try_from(seq).map_err(|_| CatalogError::Constraint("journal sequence out of range".into()))
}

fn corrupt_value(column: &str) -> CatalogError {
    CatalogError::Corrupt(format!("column {column} holds a value this build does not accept"))
}

fn text(r: &rusqlite::Row<'_>, i: usize) -> Result<String, CatalogError> {
    match r.get_ref(i)? {
        rusqlite::types::ValueRef::Text(b) => {
            String::from_utf8(b.to_vec()).map_err(|_| CatalogError::Corrupt(format!("column {i} is not UTF-8")))
        }
        _ => Err(CatalogError::Corrupt(format!("column {i} is not text"))),
    }
}

fn opt_text(r: &rusqlite::Row<'_>, i: usize) -> Result<Option<String>, CatalogError> {
    match r.get_ref(i)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        _ => text(r, i).map(Some),
    }
}

fn int(r: &rusqlite::Row<'_>, i: usize) -> Result<i64, CatalogError> {
    match r.get_ref(i)? {
        rusqlite::types::ValueRef::Integer(n) => Ok(n),
        _ => Err(CatalogError::Corrupt(format!("column {i} is not an integer"))),
    }
}

fn flag(r: &rusqlite::Row<'_>, i: usize, name: &str) -> Result<bool, CatalogError> {
    match int(r, i)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(corrupt_value(name)),
    }
}

fn seq(n: i64, name: &str) -> Result<u64, CatalogError> {
    u64::try_from(n).ok().filter(|v| *v >= 1).ok_or_else(|| corrupt_value(name))
}

fn uuid(s: &str, name: &str) -> Result<Uuid, CatalogError> {
    let u = Uuid::parse(s).map_err(|_| corrupt_value(name))?;
    // Only the lowercase form is stored.
    if u.to_string() != s {
        return Err(corrupt_value(name));
    }
    Ok(u)
}

fn digest(s: &str, name: &str) -> Result<Digest, CatalogError> {
    Digest::parse(s).map_err(|_| corrupt_value(name))
}

fn bounded(s: String, max: usize, name: &str) -> Result<CatalogText, CatalogError> {
    if s.is_empty() || s.len() > max {
        return Err(corrupt_value(name));
    }
    Ok(CatalogText::from_string(s))
}

fn parse_enum<T>(s: &str, parse: fn(&str) -> Option<T>, name: &str) -> Result<T, CatalogError> {
    parse(s).ok_or_else(|| corrupt_value(name))
}

/// The `timestamp` shape of the table check: `YYYY-MM-DDTHH:MM:SS`, then digits, `T`, `Z`,
/// `:`, `.` or `-`, ending in `Z`; 20 to 40 bytes.
fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    (20..=40).contains(&b.len())
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18].iter().all(|i| digit(*i))
        && at(4, b'-')
        && at(7, b'-')
        && at(10, b'T')
        && at(13, b':')
        && at(16, b':')
        && b.last() == Some(&b'Z')
        && b.iter().all(|c| c.is_ascii_digit() || b"TZ:.-".contains(c))
}

/// The `value_literal` shape of the table check: 1 to 64 of `0-9 . e E + -`.
fn is_value_literal(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit() || b".eE+-".contains(&c))
}

fn decode_project(r: &rusqlite::Row<'_>) -> Result<ProjectRow, CatalogError> {
    let project_id = uuid(&text(r, 0)?, "project_id")?;
    let handle = bounded(text(r, 1)?, 256, "handle")?;
    let last = match r.get_ref(2)? {
        rusqlite::types::ValueRef::Null => None,
        _ => Some(seq(int(r, 2)?, "last_seq")?),
    };
    let head = opt_text(r, 3)?.map(|h| digest(&h, "head_hash")).transpose()?;
    let indexed = match (last, head) {
        (Some(s), Some(h)) => Some((s, h)),
        (None, None) => None,
        _ => return Err(corrupt_value("last_seq")),
    };
    let availability = parse_enum(&text(r, 4)?, Availability::parse, "availability")?;
    Ok(ProjectRow { project_id, handle, indexed, availability })
}

fn decode_entry(r: &rusqlite::Row<'_>) -> Result<CatalogEntryRow, CatalogError> {
    let t = |i: usize, max: usize, name: &str| -> Result<CatalogText, CatalogError> { bounded(text(r, i)?, max, name) };
    let ot = |i: usize, max: usize, name: &str| -> Result<Option<CatalogText>, CatalogError> {
        opt_text(r, i)?.map(|s| bounded(s, max, name)).transpose()
    };
    let literal = ot(9, 64, "value_literal")?;
    let unit = ot(10, 64, "unit")?;
    let si = match r.get_ref(11)? {
        rusqlite::types::ValueRef::Null => None,
        rusqlite::types::ValueRef::Real(f) if f.is_finite() => Some(f),
        _ => return Err(corrupt_value("si_value")),
    };
    let value = match (literal, unit, si) {
        (Some(literal), Some(unit), Some(si)) => Some(EntryValue { literal, unit, si }),
        (None, None, None) => None,
        _ => return Err(corrupt_value("value_literal")),
    };
    let row = CatalogEntryRow {
        project_id: uuid(&text(r, 0)?, "project_id")?,
        record_id: t(1, 256, "record_id")?,
        part: t(2, 1024, "part")?,
        revision: ot(3, 256, "revision")?,
        plan: t(4, 1024, "plan")?,
        run: t(5, 256, "run")?,
        stage: t(6, 256, "stage")?,
        characteristic: t(7, 1024, "characteristic")?,
        timestamp: t(8, 40, "timestamp")?,
        value,
        decision_outcome: parse_enum(&text(r, 12)?, DecisionOutcome::parse, "decision_outcome")?,
        rule_id: ot(13, 256, "rule_id")?,
        evaluation_basis: parse_enum(&text(r, 14)?, EvaluationBasis::parse, "evaluation_basis")?,
        uec_class: parse_enum(&text(r, 15)?, UecClass::parse, "uec_class")?,
        labels: EvidenceLabels {
            claimed: flag(r, 16, "claimed")?,
            declared_input: flag(r, 17, "declared_input")?,
            uncharacterized_input: flag(r, 18, "uncharacterized_input")?,
        },
        interpretation: parse_enum(&text(r, 19)?, Interpretation::parse, "interpretation")?,
        record_status: parse_enum(&text(r, 20)?, RecordStatus::parse, "record_status")?,
        journal_seq: seq(int(r, 21)?, "journal_seq")?,
        journal_entry_hash: digest(&text(r, 22)?, "journal_entry_hash")?,
        entry_status: parse_enum(&text(r, 23)?, EntryStatus::parse, "entry_status")?,
    };
    // The row-level rules of the table, checked again on read (a file may bypass checks).
    if !is_timestamp(row.timestamp.raw()) {
        return Err(corrupt_value("timestamp"));
    }
    if row.value.as_ref().is_some_and(|v| !is_value_literal(v.literal.raw())) {
        return Err(corrupt_value("value_literal"));
    }
    let any_label = row.labels.claimed || row.labels.declared_input || row.labels.uncharacterized_input;
    if row.uec_class == UecClass::Uec0 && (row.value.is_some() || any_label) {
        return Err(corrupt_value("uec_class"));
    }
    if matches!(row.uec_class, UecClass::Uec2 | UecClass::Uec3)
        && (row.labels.declared_input || row.labels.uncharacterized_input)
    {
        return Err(corrupt_value("uec_class"));
    }
    if row.labels.claimed && row.uec_class != UecClass::Uec1 {
        return Err(corrupt_value("claimed"));
    }
    // UNC.UEC.CLASS: a UEC-1 entry is UEC-1 for exactly one reason, a claimed value or an
    // uncharacterized contributor (a declared input alone does not make it UEC-1).
    if row.uec_class == UecClass::Uec1 && row.labels.claimed == row.labels.uncharacterized_input {
        return Err(corrupt_value("uec_class"));
    }
    if row.decision_outcome != DecisionOutcome::NotDecidable && row.rule_id.is_none() {
        return Err(corrupt_value("rule_id"));
    }
    Ok(row)
}

// ------------------------------------------------------------------ files

fn file_name(file: &Path) -> Result<String, CatalogError> {
    file.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| CatalogError::PathInvalid("the catalog path has no usable file name".into()))
}

/// Checks `path`, creates its directory and the file for a write open of a missing catalog,
/// and returns the path with its directory canonicalised. The file itself must be a regular
/// file, never a symbolic link.
fn locate(path: &Path, mode: Mode) -> Result<PathBuf, CatalogError> {
    if !path.is_absolute() {
        return Err(CatalogError::PathInvalid("the catalog path must be absolute".into()));
    }
    let name = file_name(path)?;
    if name == "." || name == ".." {
        return Err(CatalogError::PathInvalid("the catalog path has no usable file name".into()));
    }
    let Some(parent) = path.parent() else {
        return Err(CatalogError::PathInvalid("the catalog path has no directory".into()));
    };
    if mode == Mode::ReadWrite {
        std::fs::create_dir_all(parent).map_err(|e| io_err("creating the catalog directory", e))?;
    }
    let dir = match std::fs::canonicalize(parent) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CatalogError::Missing("the catalog directory does not exist".into()))
        }
        Err(e) => return Err(io_err("resolving the catalog directory", e)),
    };
    let file = dir.join(&name);
    match std::fs::symlink_metadata(&file) {
        Ok(m) => {
            if m.file_type().is_symlink() {
                return Err(CatalogError::PathInvalid("the catalog file is a symbolic link".into()));
            }
            if !m.is_file() {
                return Err(CatalogError::PathInvalid("the catalog path is not a regular file".into()));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if mode == Mode::ReadOnly {
                return Err(CatalogError::Missing("no catalog file at the configured location".into()));
            }
            check_companions_of(&dir, &name)?;
            // Exclusive create: never follows a link planted at the path. A file that appeared
            // in between is opened and checked like any other.
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&file) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return locate(path, Mode::ReadOnly),
                Err(e) => return Err(io_err("creating the catalog file", e)),
            }
        }
        Err(e) => return Err(io_err("reading the catalog file", e)),
    }
    check_companions_of(&dir, &name)?;
    Ok(file)
}

// ------------------------------------------------------------------ schema checks

fn schema_objects(conn: &Connection, filter: &str) -> Result<Vec<SchemaObject>, rusqlite::Error> {
    let sql = format!("SELECT type, name, tbl_name, sql FROM sqlite_master {filter} ORDER BY type, name, tbl_name");
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect()
}

const META_FILTER: &str = "WHERE name = 'catalog_meta' OR tbl_name = 'catalog_meta'";

/// The checks every open runs before anything is written: root pages, the version (read
/// only after `catalog_meta` is known to be the v1 table), a newer version refused, then the
/// full object comparison. A file SQLite reports as corrupt is `stats.catalog_corrupt`.
fn checked_version(conn: &Connection, plan: &MigrationPlan) -> Result<u32, CatalogError> {
    check_root_pages(conn)?;
    let found = read_version(conn)?;
    if found > plan.latest {
        return Err(CatalogError::TooNew { found, supported: plan.latest });
    }
    verify_schema(conn, plan, SchemaVersion(found))?;
    Ok(found)
}

/// Reads the schema version: 0 for a file without any schema object, else the version in
/// the single `catalog_meta` row. `catalog_meta` is queried only after its `sqlite_master`
/// rows equal, byte for byte, those the v1 DDL creates, so a hostile file cannot have a view,
/// virtual table or other construct evaluated by this read.
pub fn read_version(conn: &Connection) -> Result<u32, CatalogError> {
    let objects: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
    if objects == 0 {
        return Ok(0);
    }
    let got = schema_objects(conn, META_FILTER)?;
    if got.is_empty() {
        return Err(CatalogError::Corrupt("file has schema objects but no catalog_meta table".into()));
    }
    let mem = Connection::open_in_memory()?;
    mem.execute_batch(V1_SQL)?;
    if got != schema_objects(&mem, META_FILTER)? {
        return Err(CatalogError::Corrupt("catalog_meta is not the table the v1 DDL defines; it was not read".into()));
    }
    Ok(read_meta(conn)?.version)
}

fn read_meta(conn: &Connection) -> Result<Meta, CatalogError> {
    let rows: i64 = conn.query_row("SELECT count(*) FROM catalog_meta", [], |r| r.get(0))?;
    if rows != 1 {
        return Err(CatalogError::Corrupt(format!("catalog_meta must hold exactly one row, not {rows}")));
    }
    let mut st = conn.prepare("SELECT singleton, schema_version, catalog_id, owner FROM catalog_meta")?;
    let mut q = st.query([])?;
    let Some(r) = q.next()? else { return Err(CatalogError::Corrupt("catalog_meta is empty".into())) };
    if int(r, 0)? != 1 {
        return Err(corrupt_value("singleton"));
    }
    let version = u32::try_from(int(r, 1)?).ok().filter(|v| *v >= 1).ok_or_else(|| corrupt_value("schema_version"))?;
    let catalog_id = uuid(&text(r, 2)?, "catalog_id")?;
    if catalog_id.version() != 7 || catalog_id.variant_bits() != 0b10 {
        return Err(corrupt_value("catalog_id"));
    }
    let owner = opt_text(r, 3)?.map(|o| bounded(o, MAX_OWNER_BYTES, "owner")).transpose()?;
    Ok(Meta { version, catalog_id, owner })
}

/// The migration hooks of the catalog: the version lives in the single `catalog_meta` row,
/// which migration 0 -> 1 writes with a new UUIDv7 catalog ID.
struct CatalogHooks<'a> {
    source: &'a mut dyn ObservationSource,
    owner: Option<&'a str>,
}

impl MigrationHooks for CatalogHooks<'_> {
    type Error = CatalogError;
    const VERSION_TABLE: &'static str = "catalog_meta";

    fn checked_version(&mut self, conn: &Connection, plan: &MigrationPlan) -> Result<SchemaVersion, CatalogError> {
        let current = checked_version(conn, plan)?;
        if current > 0 {
            // Existing rows are about to be carried forward: the file must be sound now, under
            // the write transaction, not only at the quick_check the open ran before it.
            quick_check(conn)?;
        }
        Ok(SchemaVersion(current))
    }

    fn read_version(&mut self, conn: &Connection) -> Result<SchemaVersion, CatalogError> {
        read_version(conn).map(SchemaVersion)
    }

    fn record(&mut self, conn: &Connection, m: &Migration) -> Result<(), CatalogError> {
        if m.from == 0 {
            let (ms, bits) = self.source.draw_v7().map_err(|e| self.failed(0, 1, format!("catalog ID: {e}")))?;
            let id = uuid_v7(ms, &bits).map_err(|e| self.failed(0, 1, format!("catalog ID: {e}")))?;
            conn.execute(
                "INSERT INTO catalog_meta (singleton, schema_version, catalog_id, owner) VALUES (1, ?1, ?2, ?3)",
                params![m.to, id.to_string(), self.owner],
            )
            .map_err(|e| self.failed(0, 1, format!("writing catalog_meta: {e}")))?;
            return Ok(());
        }
        let n = conn
            .execute("UPDATE catalog_meta SET schema_version = ?1 WHERE singleton = 1", params![m.to])
            .map_err(|e| self.failed(m.from, m.to, format!("recording version {}: {e}", m.to)))?;
        if n != 1 {
            return Err(self.failed(m.from, m.to, format!("recording version {}: {n} rows changed", m.to)));
        }
        Ok(())
    }

    fn failed(&self, from: u32, to: u32, detail: String) -> CatalogError {
        CatalogError::MigrationFailed { from, to, detail: escaped(&detail) }
    }

    /// A busy or locked commit stays `stats.catalog_locked`; anything else is the failed step.
    fn sqlite_failed(&self, from: u32, to: u32, what: &str, e: rusqlite::Error) -> CatalogError {
        match CatalogError::from(e) {
            CatalogError::Locked => CatalogError::Locked,
            other => self.failed(from, to, format!("{what}: {other}")),
        }
    }
}
