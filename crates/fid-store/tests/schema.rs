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

/// [`snapshot`] without `schema_meta`, which a migration extends by design.
fn data_snapshot(c: &Connection) -> Vec<(String, Vec<String>)> {
    snapshot(c).into_iter().filter(|(t, _)| t != "schema_meta").collect()
}

/// Like [`data_snapshot`], over the columns `v1` has (columns added later are left out).
fn data_snapshot_over(c: &Connection, v1: &Connection) -> Vec<(String, Vec<String>)> {
    TABLES.iter().filter(|t| **t != "schema_meta").map(|t| (t.to_string(), rows(c, t, &columns(v1, t)))).collect()
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
    Migration {
        from: 1,
        to: 2,
        sql: "ALTER TABLE entries ADD COLUMN note TEXT; INSERT INTO no_such_table VALUES (1);",
    },
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
    let meta: Vec<i64> = c
        .prepare("SELECT version FROM schema_meta")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
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
    let before = data_snapshot(&pristine);
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
        // Every v1 row of every data table is intact; schema_meta is checked above ([1, 2]).
        assert_eq!(data_snapshot_over(c, &pristine), before);
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
    // A database that has really reached 99: schema_meta holds 1..=99.
    raw(&root)
        .execute(
            "WITH RECURSIVE v(n) AS (SELECT 2 UNION ALL SELECT n + 1 FROM v WHERE n < 99) \
             INSERT INTO schema_meta (version) SELECT n FROM v",
            [],
        )
        .unwrap();
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
    // A newer schema cannot be checked, so a read open refuses it too.
    assert_eq!(open_read(&loc).unwrap_err().code(), "schema_too_new");
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
    // A valid v1 database in rollback-journal mode, with a raw reader holding a shared lock:
    // the schema check passes, but the switch to WAL cannot happen.
    let c = raw(&root);
    let mode: String = c.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "delete");
    c.execute_batch(V1_SQL).unwrap();
    c.execute_batch("INSERT INTO schema_meta (version) VALUES (1); BEGIN; SELECT count(*) FROM entries;").unwrap();
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

// ---------------------------------------------------------------- schema check (hostile files)

/// Applies `sql` to the fixture copy through a raw connection, then expects both opens to fail
/// with `schema_invalid` and `project.db` to stay byte-identical.
fn tampered_is_refused(tag: &str, sql: &str) {
    let (_t, root) = fixture_project(tag);
    raw(&root).execute_batch(sql).unwrap();
    let db = root.join(DB_FILE);
    let before = sha256_file(&db);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write(&loc, &lock.unwrap()).unwrap_err();
    assert_eq!(err.code(), "schema_invalid", "{sql}: {err}");
    assert_eq!(open_read(&loc).unwrap_err().code(), "schema_invalid", "{sql}");
    assert_eq!(sha256_file(&db), before, "{sql}");
}

#[test]
fn dropped_trigger_is_schema_invalid() {
    tampered_is_refused("droptrig", "DROP TRIGGER entries_no_update;");
    tampered_is_refused("droptrig2", "DROP TRIGGER snapshots_no_delete;");
}

#[test]
fn extra_trigger_is_schema_invalid() {
    tampered_is_refused(
        "extratrig",
        "CREATE TRIGGER evil AFTER INSERT ON entries BEGIN INSERT INTO heads VALUES ('x', NEW.hash); END;",
    );
}

#[test]
fn altered_or_extra_tables_and_indexes_are_schema_invalid() {
    tampered_is_refused("alter", "ALTER TABLE heads ADD COLUMN extra TEXT;");
    tampered_is_refused("droptable", "DROP TABLE idempotency;");
    tampered_is_refused("extratable", "CREATE TABLE side (x);");
    tampered_is_refused("extraindex", "CREATE INDEX side_idx ON heads (entry_hash);");
    // Same names, different text: a trigger that no longer aborts.
    tampered_is_refused(
        "swaptrig",
        "DROP TRIGGER entries_no_delete; \
         CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries BEGIN SELECT 1; END;",
    );
}

#[test]
fn schema_meta_must_be_exactly_one_to_max() {
    // Gap: versions 1 and 3.
    tampered_is_refused("gap", "INSERT INTO schema_meta (version) VALUES (3);");
    // Rows below 1, inserted past the CHECK constraint.
    tampered_is_refused("zero", "PRAGMA ignore_check_constraints=ON; INSERT INTO schema_meta (version) VALUES (0);");
    tampered_is_refused("neg", "PRAGMA ignore_check_constraints=ON; INSERT INTO schema_meta (version) VALUES (-5);");
    // A lone high version without its history.
    let (_t, root) = fixture_project("lone");
    raw(&root).execute("INSERT INTO schema_meta (version) VALUES (99)", []).unwrap();
    let (loc, _) = ProjectLocation::open(&root, OpenMode::Read).unwrap();
    assert_eq!(open_read(&loc).unwrap_err().code(), "schema_invalid");
}

#[test]
fn connections_are_hardened() {
    use rusqlite::config::DbConfig;
    let t = TempDir::new("harden");
    let (loc, lock) = ProjectLocation::create(&t.0.join("p")).unwrap();
    let w = open_write(&loc, &lock).unwrap();
    let r = open_read(&loc).unwrap();
    for db in [&w, &r] {
        let c = db.connection();
        assert!(c.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap());
        assert!(!c.db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA).unwrap());
        assert!(!c.db_config(DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA).unwrap());
        assert!(c.db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER).unwrap());
        for (pragma, want) in [("trusted_schema", 0i64), ("cell_size_check", 1), ("foreign_keys", 1)] {
            let got: i64 = c.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0)).unwrap();
            assert_eq!(got, want, "{pragma}");
        }
        // Extension loading is off (rusqlite is built without `load_extension`).
        let err = c.query_row("SELECT load_extension('fid_no_such_extension')", [], |row| row.get::<_, i64>(0));
        assert!(err.unwrap_err().to_string().contains("not authorized"));
        // ATTACH cannot create a new file.
        let target = t.0.join("attached.db");
        let sql = format!("ATTACH DATABASE '{}' AS side", target.to_str().unwrap().replace('\'', "''"));
        assert!(c.execute_batch(&sql).is_err());
        assert!(!target.exists());
    }
    // Defensive mode: writable_schema cannot be used to edit the schema table.
    let c = w.connection();
    let _ = c.execute_batch("PRAGMA writable_schema=ON;");
    assert!(c.execute("UPDATE sqlite_master SET sql = sql WHERE name = 'entries'", []).is_err());
}

#[test]
fn crafted_wal_that_adds_a_trigger_is_refused_and_not_checkpointed() {
    let (t, root) = fixture_project("craftwal");
    // The attacker's copy of the same database, with a change left uncheckpointed in its WAL.
    let src = t.0.join("src.db");
    std::fs::copy(Path::new(FIXTURES).join("v1.db"), &src).unwrap();
    let a = Connection::open(&src).unwrap();
    a.execute_batch(
        "PRAGMA wal_autocheckpoint=0; \
         CREATE TRIGGER evil AFTER INSERT ON entries BEGIN DELETE FROM heads; END;",
    )
    .unwrap();
    let wal = root.join(format!("{DB_FILE}-wal"));
    std::fs::copy(t.0.join("src.db-wal"), &wal).unwrap();
    drop(a);
    let db = root.join(DB_FILE);
    let before = sha256_file(&db);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write(&loc, &lock.unwrap()).unwrap_err();
    assert_eq!(err.code(), "schema_invalid", "{err}");
    assert!(err.to_string().contains("evil"), "{err}");
    // The refused WAL was not folded into project.db.
    assert_eq!(sha256_file(&db), before);
    assert_eq!(open_read(&loc).unwrap_err().code(), "schema_invalid");
}

#[test]
fn wal_with_a_different_page_size_is_schema_invalid() {
    let (t, root) = fixture_project("pagesize");
    // A WAL written for a 1024-byte-page database (the fixture uses 4096). Its page 1 frame
    // describes another database, so the file SQLite sees is not this project's schema (or is
    // corrupt); either way the open must refuse it as schema_invalid and leave project.db alone.
    let src = t.0.join("small.db");
    let a = Connection::open(&src).unwrap();
    a.execute_batch(
        "PRAGMA page_size=1024; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;          CREATE TABLE x (y); INSERT INTO x VALUES (randomblob(5000));",
    )
    .unwrap();
    std::fs::copy(t.0.join("small.db-wal"), root.join(format!("{DB_FILE}-wal"))).unwrap();
    drop(a);
    let db = root.join(DB_FILE);
    let before = sha256_file(&db);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write(&loc, &lock.unwrap()).unwrap_err();
    assert_eq!(err.code(), "schema_invalid", "{err}");
    assert_eq!(sha256_file(&db), before);
}

#[test]
fn hostile_schema_meta_is_refused_before_it_is_read() {
    // A generated column named `version` whose evaluation fails: reading it would surface an
    // SQLite "integer overflow" error instead of schema_invalid. SQLite evaluates it on INSERT
    // too, so the row is written first and the column definition swapped in afterwards.
    let (_t, root) = new_project("genmeta");
    raw(&root)
        .execute_batch(
            "CREATE TABLE schema_meta (v0 INTEGER PRIMARY KEY); INSERT INTO schema_meta (v0) VALUES (1); \
             PRAGMA writable_schema=ON; \
             UPDATE sqlite_master SET sql = 'CREATE TABLE schema_meta (v0 INTEGER PRIMARY KEY, \
             version INTEGER AS (abs(-9223372036854775808)))' WHERE name = 'schema_meta'; \
             PRAGMA writable_schema=OFF;",
        )
        .unwrap();
    let c = raw(&root);
    let trap = c.query_row("SELECT max(version) FROM schema_meta", [], |r| r.get::<_, Option<i64>>(0));
    assert!(trap.unwrap_err().to_string().contains("overflow"), "the trap must be live");
    let err = read_version(&c).unwrap_err();
    assert_eq!(err.code(), "schema_invalid");
    assert!(err.to_string().contains("was not read"), "{err}");
    drop(c);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write(&loc, &lock.unwrap()).unwrap_err();
    assert_eq!(err.code(), "schema_invalid", "{err}");
    assert!(!err.to_string().contains("overflow"), "{err}");

    // Virtual tables: rtree and fts5 modules would run on any read of schema_meta.
    for module in ["rtree(version, lo, hi)", "fts5(version)"] {
        let (_t, root) = new_project("vtabmeta");
        raw(&root).execute_batch(&format!("CREATE VIRTUAL TABLE schema_meta USING {module};")).unwrap();
        let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
        let err = open_write(&loc, &lock.unwrap()).unwrap_err();
        assert_eq!(err.code(), "schema_invalid", "{module}: {err}");
        let msg = err.to_string();
        assert!(msg.contains("no root page") || msg.contains("was not read"), "{module}: {msg}");
        assert_eq!(open_read(&loc).unwrap_err().code(), "schema_invalid", "{module}");
    }
}

#[test]
fn zero_root_page_and_damaged_pages_are_schema_invalid() {
    tampered_is_refused(
        "rootpage",
        "PRAGMA writable_schema=ON; UPDATE sqlite_master SET rootpage = 0 WHERE name = 'heads';          PRAGMA writable_schema=OFF;",
    );
    // Garbage over the last page (a b-tree page of the data): quick_check refuses the file.
    let (_t, root) = fixture_project("damaged");
    let db = root.join(DB_FILE);
    let mut bytes = std::fs::read(&db).unwrap();
    let n = bytes.len();
    assert!(n >= 8192 && n % 4096 == 0);
    for b in &mut bytes[n - 4096..] {
        *b = 0x55;
    }
    std::fs::write(&db, &bytes).unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = open_write(&loc, &lock.unwrap()).unwrap_err();
    assert_eq!(err.code(), "schema_invalid", "{err}");
    assert_eq!(std::fs::read(&db).unwrap(), bytes);
}

#[test]
fn leftover_rollback_journal_is_refused_and_not_replayed() {
    let (_t, root) = fixture_project("journal");
    let db = root.join(DB_FILE);
    let journal = root.join(format!("{DB_FILE}-journal"));
    // An empty journal is harmless.
    std::fs::write(&journal, b"").unwrap();
    {
        let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
        assert_eq!(open_write(&loc, &lock.unwrap()).unwrap().version(), SchemaVersion(1));
    }
    let before = sha256_file(&db);
    // A non-empty one (hot-journal header plus junk) is refused before SQLite sees it.
    let mut bytes = vec![0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7];
    bytes.extend_from_slice(&[0xA5u8; 4096]);
    std::fs::write(&journal, &bytes).unwrap();
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    assert_eq!(open_write(&loc, &lock.unwrap()).unwrap_err().code(), "schema_invalid");
    assert_eq!(open_read(&loc).unwrap_err().code(), "schema_invalid");
    assert_eq!(sha256_file(&db), before);
    assert_eq!(std::fs::read(&journal).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn symlinked_companion_files_are_refused() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let (t, root) = fixture_project("complink");
        let elsewhere = t.0.join("elsewhere");
        std::fs::write(&elsewhere, b"").unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join(format!("{DB_FILE}{suffix}"))).unwrap();
        let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
        assert_eq!(open_write(&loc, &lock.unwrap()).unwrap_err().code(), "symlink_refused", "{suffix}");
        assert_eq!(open_read(&loc).unwrap_err().code(), "symlink_refused", "{suffix}");
        assert_eq!(std::fs::metadata(&elsewhere).unwrap().len(), 0);
    }
}

#[cfg(unix)]
#[test]
fn symlink_in_the_path_to_the_root_still_opens() {
    let (t, root) = fixture_project("rootlink");
    let link = t.0.join("link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    let (loc, lock) = ProjectLocation::open(&link, OpenMode::Write).unwrap();
    assert_eq!(open_write(&loc, &lock.unwrap()).unwrap().version(), SchemaVersion(1));
}

/// The signature text of every `pub fn` in `src` (up to its body), with its name.
fn public_fns(src: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(public_fns(&path));
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("pub fn ") {
            let after = &rest[at + "pub fn ".len()..];
            let end = after.find(['{', ';']).unwrap_or(after.len());
            let sig = after[..end].split_whitespace().collect::<Vec<_>>().join(" ");
            let name = sig.split(['(', '<']).next().unwrap().trim().to_string();
            out.push((name, sig));
            rest = &after[end..];
        }
    }
    out
}

/// CORE.BUS.LINT: fid-store is a lint door for SQLite, so the only public function that hands
/// out an owned `Connection` is the guarded `open_hardened`; the raw open, the hardening step and
/// the `sqlite3_db_config` setter are private (the module docs also hold compile-fail checks).
#[test]
fn public_api_cannot_produce_an_unhardened_connection() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let fns = public_fns(&src);
    assert!(fns.iter().any(|(n, _)| n == "open_hardened"), "open_hardened must be public");
    let owned: Vec<&str> = fns
        .iter()
        .filter(|(_, sig)| {
            let ret = sig.split("->").nth(1).unwrap_or("");
            ret.replace("&mut Connection", "").replace("&Connection", "").contains("Connection")
        })
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(owned, ["open_hardened"], "public functions returning an owned Connection");
    for private in ["open_path", "harden", "set_config"] {
        assert!(!fns.iter().any(|(n, _)| n == private), "{private} must not be public");
    }
}

#[test]
fn open_hardened_returns_a_hardened_connection_and_never_creates() {
    use fid_store::schema::{allow_checkpoint, open_hardened, BUSY_TIMEOUT};
    use rusqlite::config::DbConfig;
    let t = TempDir::new("door");
    // A missing file is refused, not created.
    let err = open_hardened(&t.0, "other.db", OpenMode::Write, BUSY_TIMEOUT).unwrap_err();
    assert_eq!(err.code(), "io");
    assert!(!t.0.join("other.db").exists());
    std::fs::write(t.0.join("other.db"), b"").unwrap();
    // Not a plain file name, or a relative directory: refused before SQLite is called.
    for name in ["", ".", "..", "a/other.db", r"a\other.db"] {
        assert_eq!(open_hardened(&t.0, name, OpenMode::Read, BUSY_TIMEOUT).unwrap_err().code(), "path_unsupported");
    }
    let rel = Path::new("relative");
    assert_eq!(open_hardened(rel, "other.db", OpenMode::Read, BUSY_TIMEOUT).unwrap_err().code(), "path_unsupported");
    for mode in [OpenMode::Read, OpenMode::Write] {
        let conn = open_hardened(&t.0, "other.db", mode, BUSY_TIMEOUT).unwrap();
        assert!(conn.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap());
        assert!(!conn.db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA).unwrap());
        assert!(!conn.db_config(DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA).unwrap());
        assert!(!conn.db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_CREATE).unwrap());
        assert!(!conn.db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_WRITE).unwrap());
        assert!(conn.db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE).unwrap());
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        allow_checkpoint(&conn).unwrap();
        assert!(!conn.db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE).unwrap());
    }
}

#[cfg(unix)]
#[test]
fn open_hardened_refuses_a_symlinked_file() {
    use fid_store::schema::{open_hardened, BUSY_TIMEOUT};
    let t = TempDir::new("doorlink");
    std::fs::write(t.0.join("real.db"), b"").unwrap();
    std::os::unix::fs::symlink(t.0.join("real.db"), t.0.join("link.db")).unwrap();
    assert_eq!(open_hardened(&t.0, "link.db", OpenMode::Read, BUSY_TIMEOUT).unwrap_err().code(), "symlink_refused");
}
