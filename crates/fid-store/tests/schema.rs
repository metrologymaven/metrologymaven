//! Acceptance criteria of spec node CORE.STO.SQL.SCHEMA (AC-1 .. AC-5) and its requirements.
//!
//! The v1 fixture `fixtures/core/sto/sql/schema/v1.db` and its row counts `v1.json` come from
//! `make_fixture.py` (Python's sqlite3), never from this crate. AC-2 uses a raw `rusqlite`
//! connection that bypasses the store API.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_store::layout::{CREATED_ENTRIES, DB_COMPANIONS, DB_FILE};
use fid_store::schema::{
    check_journal_mode, is_append_only, open_read, open_write, open_write_with, read_version, APPEND_ONLY_TABLES,
    LATEST, MIGRATIONS, MUTABLE_TABLES, TABLES, V1_SQL,
};
use fid_store::{Migration, MigrationPlan, OpenMode, ProjectLocation, SchemaVersion};
use rusqlite::Connection;
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/sql/schema");

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-sql-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A new project (layout only; `project.db` empty), lock released.
fn new_project(tag: &str) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    let (_loc, lock) = ProjectLocation::create(&root).unwrap();
    drop(lock);
    (t, root)
}

/// A project whose `project.db` is a copy of the v1 fixture.
fn fixture_project(tag: &str) -> (TempDir, PathBuf) {
    let (t, root) = new_project(tag);
    std::fs::copy(Path::new(FIXTURES).join("v1.db"), root.join(DB_FILE)).unwrap();
    (t, root)
}

/// A read-only connection on a private copy of the fixture (SQLite would otherwise leave
/// `-shm` files next to the committed fixture).
fn pristine(tag: &str) -> (TempDir, Connection) {
    let t = TempDir::new(tag);
    let p = t.0.join("v1.db");
    std::fs::copy(Path::new(FIXTURES).join("v1.db"), &p).unwrap();
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let c = Connection::open_with_flags(&p, flags).unwrap();
    (t, c)
}

fn sha256_file(p: &Path) -> String {
    let bytes = std::fs::read(p).unwrap();
    Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn raw(root: &Path) -> Connection {
    Connection::open(root.join(DB_FILE)).unwrap()
}

fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

fn columns(c: &Connection, table: &str) -> Vec<String> {
    let mut st = c.prepare(&format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid")).unwrap();
    let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

/// Every row of `table` restricted to `cols`, rendered and sorted.
fn rows(c: &Connection, table: &str, cols: &[String]) -> Vec<String> {
    let mut st = c.prepare(&format!("SELECT {} FROM {table}", cols.join(", "))).unwrap();
    let n = cols.len();
    let mapped = st
        .query_map([], |r| {
            let mut out = Vec::new();
            for i in 0..n {
                out.push(format!("{:?}", r.get::<_, rusqlite::types::Value>(i)?));
            }
            Ok(out.join("|"))
        })
        .unwrap();
    let mut v: Vec<String> = mapped.map(Result::unwrap).collect();
    v.sort();
    v
}

/// All rows of all v1 tables, keyed by table, over the v1 columns.
fn snapshot(c: &Connection) -> Vec<(String, Vec<String>)> {
    TABLES.iter().map(|t| (t.to_string(), rows(c, t, &columns(c, t)))).collect()
}

/// Like [`snapshot`], over the columns `v1` has (columns added later are left out).
fn snapshot_over(c: &Connection, v1: &Connection) -> Vec<(String, Vec<String>)> {
    TABLES.iter().map(|t| (t.to_string(), rows(c, t, &columns(v1, t)))).collect()
}

fn expected_counts() -> Vec<(String, i64)> {
    let raw = std::fs::read(Path::new(FIXTURES).join("v1.json")).unwrap();
    let doc = fid_canon::parse(&raw).unwrap();
    let CValue::Obj(top) = doc else { panic!("v1.json is not an object") };
    let Some(CValue::Obj(counts)) = top.get("counts") else { panic!("counts") };
    let total = match top.get("total") {
        Some(CValue::Num(n)) => *n as i64,
        other => panic!("total: {other:?}"),
    };
    let out: Vec<(String, i64)> = counts
        .iter()
        .map(|(k, v)| match v {
            CValue::Num(n) => (k.clone(), *n as i64),
            other => panic!("{k}: {other:?}"),
        })
        .collect();
    assert_eq!(out.iter().map(|(_, n)| n).sum::<i64>(), total);
    assert_eq!(total, 100);
    out
}

fn triggers_on(c: &Connection, table: &str) -> Vec<(String, String)> {
    let mut st = c
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1 ORDER BY name")
        .unwrap();
    let rows = st.query_map([table], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

const V2_ADD_COLUMN: Migration = Migration { from: 1, to: 2, sql: "ALTER TABLE entries ADD COLUMN note TEXT;" };
static PLAN_V2: [Migration; 2] = [Migration { from: 0, to: 1, sql: V1_SQL }, V2_ADD_COLUMN];

/// v2 whose second statement fails (no such table).
static PLAN_V2_FAILING: [Migration; 2] = [
    Migration { from: 0, to: 1, sql: V1_SQL },
    Migration { from: 1, to: 2, sql: "ALTER TABLE entries ADD COLUMN note TEXT; INSERT INTO no_such_table VALUES (1);" },
];

fn plan(migrations: &'static [Migration]) -> MigrationPlan {
    MigrationPlan { latest: migrations.len() as u32, migrations }
}

// ---------------------------------------------------------------- AC-1

#[test]
fn ac1_new_project_is_version_1_with_two_triggers_on_each_append_only_table() {
    let t = TempDir::new("ac1");
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let db = open_write(&loc, &lock).unwrap();
    assert_eq!(db.version(), SchemaVersion(1));
    assert_eq!(db.mode(), OpenMode::Write);
    let c = db.connection();
    let meta: Vec<i64> =
        c.prepare("SELECT version FROM schema_meta").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(meta, [1]);
    for table in TABLES {
        assert!(count(c, table) >= 0, "{table} exists");
    }
    for table in APPEND_ONLY_TABLES {
        let trig = triggers_on(c, table);
        for suffix in ["no_update", "no_delete"] {
            let name = format!("{table}_{suffix}");
            let (_, sql) = trig.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("{name} missing"));
            let kind = if suffix == "no_update" { "BEFORE UPDATE" } else { "BEFORE DELETE" };
            assert!(sql.contains(kind), "{sql}");
            assert!(sql.contains("RAISE(ABORT, 'append_only')"), "{sql}");
        }
    }
    for table in MUTABLE_TABLES {
        assert!(triggers_on(c, table).is_empty(), "{table} must stay mutable");
    }
    // REQ-007: WAL and foreign keys at open.
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(fk, 1);
    drop(db);
    // A second write open finds version 1 and applies nothing.
    let db = open_write(&loc, &lock).unwrap();
    assert_eq!(db.version(), SchemaVersion(1));
    assert_eq!(count(db.connection(), "schema_meta"), 1);
}

#[test]
fn the_v1_fixture_schema_matches_the_compiled_ddl() {
    let (_t, root) = new_project("same");
    {
        let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
        open_write(&loc, &lock.unwrap()).unwrap();
    }
    let ours = raw(&root);
    let (_t2, theirs) = pristine("same-fx");
    let objects = |c: &Connection| -> Vec<String> {
        let mut st = c.prepare("SELECT type || ' ' || name || ' ' || coalesce(sql, '') FROM sqlite_master").unwrap();
        let mut v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        v.sort();
        v
    };
    assert_eq!(objects(&ours), objects(&theirs));
    for (table, n) in expected_counts() {
        assert_eq!(count(&theirs, &table), n, "{table}");
    }
}

// ---------------------------------------------------------------- AC-2

#[test]
fn ac2_raw_connection_update_and_delete_fail_with_append_only() {
    let (_t, root) = fixture_project("ac2");
    let c = raw(&root);
    let before: Vec<i64> = TABLES.iter().map(|t| count(&c, t)).collect();
    let snap = snapshot(&c);
    let mut statements = vec!["UPDATE entries SET kind = 'tampered'".to_string(), "DELETE FROM snapshots".to_string()];
    for table in APPEND_ONLY_TABLES.iter().chain(["schema_meta"].iter()) {
        let col = &columns(&c, table)[0];
        statements.push(format!("UPDATE {table} SET {col} = {col}"));
        statements.push(format!("DELETE FROM {table}"));
        statements.push(format!("DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} LIMIT 1)"));
    }
    // INSERT OR REPLACE over an existing key would delete the old row without delete triggers.
    let h1 = "sha256:".to_string() + &hex_sha("entry-1");
    statements.push(format!(
        "INSERT OR REPLACE INTO entries (seq, hash, stream, branch, kind, header) VALUES (1, '{h1}', 'r', 'main', 'x', x'00')"
    ));
    statements.push(format!(
        "INSERT OR REPLACE INTO entry_blobs (entry_hash, role, digest) VALUES ('{h1}', 'payload', 'sha256:{}')",
        hex_sha("blob-1")
    ));
    statements.push(
        "INSERT OR REPLACE INTO snapshots (id, entry_hash, format_version, state_digest, content_digest, size) \
         SELECT id, entry_hash, 9, state_digest, content_digest, 0 FROM snapshots WHERE id = 1"
            .into(),
    );
    statements.push(
        "INSERT OR REPLACE INTO checkpoints (id, stream, entry_hash, body) \
         SELECT id, stream, entry_hash, x'00' FROM checkpoints WHERE id = 1"
            .into(),
    );
    statements.push("INSERT OR REPLACE INTO anchors (id, checkpoint_id, kind, body) VALUES (1, 1, 'x', x'00')".into());
    statements.push("INSERT OR REPLACE INTO schema_meta (version) VALUES (1)".into());
    statements.push(format!(
        "INSERT INTO entries (seq, hash, stream, branch, kind, header) VALUES (999, '{h1}', 'r', 'main', 'x', x'00') \
         ON CONFLICT (hash) DO UPDATE SET kind = 'upserted'"
    ));
    for sql in &statements {
        // WITHOUT ROWID tables have no rowid; that statement is not applicable there.
        if sql.contains("entry_blobs WHERE rowid") {
            continue;
        }
        let err = c.execute(sql, []).expect_err(sql);
        assert!(is_append_only(&err), "{sql}: {err}");
        assert!(err.to_string().contains("append_only"), "{sql}: {err}");
        assert_eq!(fid_store::SchemaError::from(err).code(), "append_only");
    }
    let after: Vec<i64> = TABLES.iter().map(|t| count(&c, t)).collect();
    assert_eq!(before, after);
    assert_eq!(snap, snapshot(&c));
    // REQ-003: the mutable tables accept updates and deletes.
    c.execute("UPDATE heads SET entry_hash = (SELECT hash FROM entries WHERE seq = 2) WHERE branch = 'main'", [])
        .unwrap();
    c.execute("DELETE FROM idempotency WHERE key = 'key-1'", []).unwrap();
    c.execute("UPDATE settings_current SET value = x'00' WHERE key = 'setting.1'", []).unwrap();
    // Appending still works.
    c.execute(
        "INSERT INTO entries (hash, stream, branch, kind, header) VALUES (?1, 'record', 'main', 'command', x'7b7d')",
        [format!("sha256:{}", hex_sha("entry-41"))],
    )
    .unwrap();
    assert_eq!(count(&c, "entries"), before[1] + 1);
}

fn hex_sha(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- AC-3

#[test]
fn ac3_v1_fixture_migrates_to_v2_with_every_row_intact() {
    let (_t, root) = fixture_project("ac3");
    let (_t2, pristine) = pristine("ac3-fx");
    let before = snapshot(&pristine);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    {
        let db = open_write_with(&loc, &lock, &plan(&PLAN_V2)).unwrap();
        assert_eq!(db.version(), SchemaVersion(2));
        let c = db.connection();
        assert!(columns(c, "entries").contains(&"note".to_string()));
        let meta: Vec<i64> = c
            .prepare("SELECT version FROM schema_meta ORDER BY version")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(meta, [1, 2]);
        assert_eq!(snapshot_over(c, &pristine), before);
        for (table, n) in expected_counts() {
            assert_eq!(count(c, &table), n, "{table}");
        }
        // The triggers survive the migration.
        let err = c.execute("UPDATE entries SET note = 'x'", []).unwrap_err();
        assert!(is_append_only(&err), "{err}");
    }
    // Reopening at v2 applies nothing; this build's own plan (latest 1) refuses the file.
    let db = open_write_with(&loc, &lock, &plan(&PLAN_V2)).unwrap();
    assert_eq!(db.version(), SchemaVersion(2));
    drop(db);
    let err = open_write(&loc, &lock).unwrap_err();
    assert_eq!(err.code(), "schema_too_new", "{err}");
}

#[test]
fn a_new_project_runs_every_migration_in_one_go() {
    let t = TempDir::new("fresh2");
    let (loc, lock) = ProjectLocation::create(&t.0.join("p")).unwrap();
    let db = open_write_with(&loc, &lock, &plan(&PLAN_V2)).unwrap();
    assert_eq!(db.version(), SchemaVersion(2));
    assert_eq!(count(db.connection(), "schema_meta"), 2);
}

// ---------------------------------------------------------------- AC-4

#[test]
fn ac4_failed_migration_leaves_version_1_and_identical_bytes() {
    let (_t, root) = fixture_project("ac4");
    let db_path = root.join(DB_FILE);
    let before = sha256_file(&db_path);
    assert_eq!(before, sha256_file(&Path::new(FIXTURES).join("v1.db")));
    {
        let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
        let err = open_write_with(&loc, &lock.unwrap(), &plan(&PLAN_V2_FAILING)).unwrap_err();
        assert_eq!(err.code(), "migration_failed", "{err}");
        match err {
            fid_store::SchemaError::MigrationFailed { from, to, detail } => {
                assert_eq!((from, to), (1, 2));
                assert!(detail.contains("no_such_table"), "{detail}");
            }
            other => panic!("{other:?}"),
        }
    }
    let (loc, none) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    assert!(none.is_none());
    let db = open_read(&loc).unwrap();
    assert_eq!(db.version(), SchemaVersion(1));
    assert_eq!(db.mode(), OpenMode::Read);
    assert!(!columns(db.connection(), "entries").contains(&"note".to_string()));
    assert_eq!(sha256_file(&db_path), before);
    // A read-only open cannot write.
    let err = db.connection().execute("INSERT INTO schema_meta (version) VALUES (5)", []).unwrap_err();
    assert!(err.to_string().contains("readonly") || err.to_string().contains("read-only"), "{err}");
}

#[test]
fn migration_that_ends_the_transaction_is_refused() {
    static PLAN: [Migration; 2] =
        [Migration { from: 0, to: 1, sql: V1_SQL }, Migration { from: 1, to: 2, sql: "COMMIT;" }];
    let (_t, root) = fixture_project("commit");
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write_with(&loc, &lock.unwrap(), &plan(&PLAN)).unwrap_err();
    assert_eq!(err.code(), "migration_failed", "{err}");
    let db = open_read(&loc).unwrap();
    assert_eq!(db.version(), SchemaVersion(1));
}

#[test]
fn plans_must_be_contiguous_from_zero() {
    static GAP: [Migration; 2] = [Migration { from: 0, to: 1, sql: V1_SQL }, Migration { from: 2, to: 3, sql: "" }];
    static JUMP: [Migration; 1] = [Migration { from: 0, to: 2, sql: V1_SQL }];
    assert!(plan(&MIGRATIONS).validate().is_ok());
    assert_eq!(MIGRATIONS.len() as u32, LATEST);
    for p in [plan(&GAP), plan(&JUMP), MigrationPlan { latest: 2, migrations: &MIGRATIONS }] {
        assert_eq!(p.validate().unwrap_err().code(), "migration_failed");
    }
}

// ---------------------------------------------------------------- AC-5

#[test]
fn ac5_version_99_fails_write_open_with_schema_too_new() {
    let (_t, root) = fixture_project("ac5");
    raw(&root).execute("INSERT INTO schema_meta (version) VALUES (99)", []).unwrap();
    let before = sha256_file(&root.join(DB_FILE));
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    for p in [fid_store::schema::PLAN, plan(&PLAN_V2)] {
        match open_write_with(&loc, &lock, &p).unwrap_err() {
            fid_store::SchemaError::SchemaTooNew { found, supported } => {
                assert_eq!((found, supported), (99, p.latest));
            }
            other => panic!("{other:?}"),
        }
    }
    drop(lock);
    assert_eq!(sha256_file(&root.join(DB_FILE)), before);
    // A read open reports the version and leaves the decision to the caller.
    assert_eq!(open_read(&loc).unwrap().version(), SchemaVersion(99));
}

// ---------------------------------------------------------------- REQ-007 and hostile files

#[test]
fn journal_mode_answers_other_than_wal_are_wal_unavailable() {
    assert!(check_journal_mode("wal").is_ok());
    assert!(check_journal_mode("WAL").is_ok());
    for answer in ["delete", "memory", "off", "truncate", "persist", ""] {
        assert_eq!(check_journal_mode(answer).unwrap_err().code(), "wal_unavailable", "{answer}");
    }
}

#[test]
fn wal_that_cannot_be_set_fails_the_write_open() {
    let (_t, root) = new_project("wal");
    // A raw rollback-journal reader holds a shared lock, so the switch to WAL cannot happen.
    let c = raw(&root);
    let mode: String = c.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "delete");
    c.execute_batch("CREATE TABLE t (x); BEGIN; SELECT count(*) FROM t;").unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write(&loc, &lock.unwrap()).unwrap_err();
    assert_eq!(err.code(), "wal_unavailable", "{err}");
    c.execute_batch("COMMIT;").unwrap();
}

#[test]
fn foreign_database_and_empty_schema_meta_are_schema_invalid() {
    let (_t, root) = new_project("foreign");
    raw(&root).execute_batch("CREATE TABLE other (x);").unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    assert_eq!(open_write(&loc, &lock).unwrap_err().code(), "schema_invalid");
    assert_eq!(open_read(&loc).unwrap_err().code(), "schema_invalid");

    let (_t2, root2) = new_project("emptymeta");
    raw(&root2).execute_batch(V1_SQL).unwrap();
    let (loc2, _) = ProjectLocation::open(&root2, OpenMode::Read).unwrap();
    assert_eq!(open_read(&loc2).unwrap_err().code(), "schema_invalid");
    let c = raw(&root2);
    assert_eq!(read_version(&c).unwrap_err().code(), "schema_invalid");
}

#[test]
fn read_open_of_a_new_project_is_version_0_and_writes_nothing() {
    let (_t, root) = new_project("read0");
    let (loc, _) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    let db = open_read(&loc).unwrap();
    assert_eq!(db.version(), SchemaVersion(0));
    drop(db);
    assert_eq!(std::fs::metadata(root.join(DB_FILE)).unwrap().len(), 0);
}

// ---------------------------------------------------------------- CORE.STO.LAYOUT behaviour

#[test]
fn only_layout_entries_appear_and_a_second_writer_is_locked_out() {
    let t = TempDir::new("layout");
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let db = open_write(&loc, &lock).unwrap();
    db.connection()
        .execute(
            "INSERT INTO entries (hash, stream, branch, kind, header) VALUES (?1, 'record', 'main', 'command', x'7b7d')",
            [format!("sha256:{}", hex_sha("x"))],
        )
        .unwrap();
    // A second writer, in this process or another, cannot take the project.
    assert_eq!(ProjectLocation::open(&root, OpenMode::Write).unwrap_err().code(), "locked");
    // A reader can open the database next to the writer and sees the committed row.
    let (ro, _) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    let reader = open_read(&ro).unwrap();
    assert_eq!(reader.version(), SchemaVersion(1));
    assert_eq!(count(reader.connection(), "entries"), 1);
    let allowed: Vec<&str> = CREATED_ENTRIES.iter().chain(DB_COMPANIONS.iter()).copied().collect();
    for name in loc.list().unwrap() {
        let top = name.split('/').next().unwrap();
        assert!(allowed.contains(&top), "unexpected entry {name}");
    }
    drop(reader);
    drop(db);
    drop(lock);
}

#[cfg(unix)]
#[test]
fn symlinked_database_is_refused() {
    let (t, root) = new_project("dblink");
    let elsewhere = t.0.join("elsewhere.db");
    std::fs::copy(Path::new(FIXTURES).join("v1.db"), &elsewhere).unwrap();
    std::fs::remove_file(root.join(DB_FILE)).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join(DB_FILE)).unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    assert_eq!(open_write(&loc, &lock.unwrap()).unwrap_err().code(), "symlink_refused");
    assert_eq!(open_read(&loc).unwrap_err().code(), "symlink_refused");
}

#[test]
fn database_that_is_a_directory_is_refused() {
    let (_t, root) = new_project("dbdir");
    std::fs::remove_file(root.join(DB_FILE)).unwrap();
    std::fs::create_dir(root.join(DB_FILE)).unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    assert_eq!(open_write(&loc, &lock.unwrap()).unwrap_err().code(), "not_regular_file");
}

/// An absolute path of exactly `total` characters below `base` (as in the layout tests).
fn path_of_len(base: &Path, total: usize) -> PathBuf {
    let mut s = base.to_str().unwrap().trim_end_matches(std::path::MAIN_SEPARATOR).to_string();
    let mut rem = total - s.chars().count();
    assert!(rem >= 2);
    while rem > 0 {
        let take = if rem > 101 {
            if rem - 101 >= 2 {
                100
            } else {
                98
            }
        } else {
            rem - 1
        };
        s.push(std::path::MAIN_SEPARATOR);
        s.push_str(&"d".repeat(take));
        rem -= take + 1;
    }
    PathBuf::from(s)
}

#[cfg(windows)]
#[test]
fn windows_project_at_a_1024_character_path_opens_its_database() {
    let t = TempDir::new("long");
    let root = path_of_len(&t.0, fid_store::layout::MAX_PATH_CHARS);
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let db = open_write(&loc, &lock).unwrap();
    assert_eq!(db.version(), SchemaVersion(1));
    drop(db);
    assert_eq!(open_read(&loc).unwrap().version(), SchemaVersion(1));
}

#[cfg(unix)]
#[test]
fn unix_database_path_limit_is_reported_as_path_too_long() {
    use fid_store::schema::UNIX_MAX_DB_PATH_BYTES;
    let t = TempDir::new("long");
    // Fits: the database path stays below SQLite's limit.
    let ok = path_of_len(&t.0, 440);
    let (loc, lock) = ProjectLocation::create(&ok).unwrap();
    assert_eq!(open_write(&loc, &lock).unwrap().version(), SchemaVersion(1));
    drop(lock);
    // Too long for SQLite's Unix VFS, though fine for the layout.
    let long = path_of_len(&t.0, UNIX_MAX_DB_PATH_BYTES);
    let (loc, lock) = ProjectLocation::create(&long).unwrap();
    assert_eq!(open_write(&loc, &lock).unwrap_err().code(), "path_too_long");
}
