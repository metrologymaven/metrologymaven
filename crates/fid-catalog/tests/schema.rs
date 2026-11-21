//! Acceptance criteria of spec node RPT.CATALOG.STORE.SCHEMA (AC-1 .. AC-5) and its
//! requirements.
//!
//! The v1 fixture `fixtures/rpt/catalog/schema_v1.sqlite`, the rows a v2 build must find in it
//! (`schema_v1.json`) and the hand-labelled row cases (`schema_cases.json`) come from
//! `make_schema_fixtures.py` (Python's sqlite3), never from this crate. AC-5 is this file run
//! with the network cut (`crates/fid-catalog/offline-tests.sh`, listed in
//! `ci/fixture-checks.txt`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_catalog::schema::{read_version, validate_plan, TABLES, V1_SQL, V2_SQL};
use fid_catalog::{
    catalog_location, catalog_settings, configured_catalog_path, default_catalog_path, Availability, Catalog,
    CatalogEntryRow, CatalogError, CatalogText, DecisionOutcome, EntryStatus, EntryValue, EvaluationBasis,
    EvidenceLabels, Interpretation, Migration, MigrationPlan, Mode, OpenOptions, ProjectRow, RecordStatus, UecClass,
    CATALOG_PATH_KEY, LATEST, MIGRATIONS, PLAN,
};
use fid_project::{Bits, IdError, ObservationSource};
use fid_store::schema::expected_objects;
use fid_store::settings::{FileStore, Scope, Sources};
use fid_store::SchemaVersion;
use fid_types::Digest;
use fid_uuid::Uuid;
use rusqlite::types::Value;
use rusqlite::{Connection, ErrorCode};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/rpt/catalog");

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-cat-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

/// A fixed clock and fixed random bits, so the catalog ID is known.
struct Fixed {
    ms: u64,
    bits: u128,
}

impl ObservationSource for Fixed {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(self.ms)
    }
    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        Bits::new(n, self.bits & ((1u128 << n) - 1))
    }
}

/// A source that always fails: a catalog that already exists never needs it.
struct NoSource;

impl ObservationSource for NoSource {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Err(IdError::ObservationUnavailable)
    }
    fn random_bits(&mut self, _n: u32) -> Result<Bits, IdError> {
        Err(IdError::ObservationUnavailable)
    }
}

const MS: u64 = 1_763_700_000_000;

fn open_fixed(path: &Path, mode: Mode, plan: MigrationPlan) -> Result<Catalog, CatalogError> {
    let mut src = Fixed { ms: MS, bits: 0x2_0000_0000_0000_0000_1234 };
    Catalog::open_with(
        path,
        mode,
        OpenOptions { source: &mut src, owner: Some("lab"), plan, busy_timeout: Duration::from_millis(200) },
    )
}

fn open_existing(path: &Path, mode: Mode, plan: MigrationPlan) -> Result<Catalog, CatalogError> {
    let mut src = NoSource;
    Catalog::open_with(
        path,
        mode,
        OpenOptions { source: &mut src, owner: None, plan, busy_timeout: Duration::from_millis(200) },
    )
}

fn sha_file(p: &Path) -> Vec<u8> {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(std::fs::read(p).unwrap()).to_vec()
}

/// A private copy of the v1 fixture (SQLite would otherwise leave `-shm` files next to it).
fn fixture_copy(tag: &str) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let p = t.0.join("catalog.sqlite");
    std::fs::copy(Path::new(FIXTURES).join("schema_v1.sqlite"), &p).unwrap();
    (t, p)
}

fn json(name: &str) -> CValue {
    fid_canon::parse(&std::fs::read(Path::new(FIXTURES).join(name)).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("array expected, got {other:?}"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("string expected, got {other:?}"),
    }
}

/// A typed fixture value as an SQLite value.
fn sql_value(v: &CValue) -> Value {
    let o = obj(v);
    match s(&o["t"]) {
        "null" => Value::Null,
        "text" => Value::Text(s(&o["v"]).to_string()),
        "int" => Value::Integer(s(&o["v"]).parse().unwrap()),
        "real" => Value::Real(s(&o["v"]).parse().unwrap()),
        "blob" => {
            let h = s(&o["v"]);
            Value::Blob((0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect())
        }
        t => panic!("type {t}"),
    }
}

fn insert_raw(conn: &Connection, columns: &[String], row: &[CValue]) -> rusqlite::Result<usize> {
    let marks: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
    let sql = format!("INSERT INTO entry ({}) VALUES ({})", columns.join(", "), marks.join(", "));
    conn.execute(&sql, rusqlite::params_from_iter(row.iter().map(sql_value)))
}

fn strings(v: &CValue) -> Vec<String> {
    arr(v).iter().map(|c| s(c).to_string()).collect()
}

const P1: &str = "01936b2e-7a00-7000-8000-000000000001";

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn digest(tag: &str) -> Digest {
    use sha2::{Digest as _, Sha256};
    let b: [u8; 32] = Sha256::digest(tag.as_bytes()).into();
    Digest::from_bytes(b)
}

fn project(id: &str, handle: &str) -> ProjectRow {
    ProjectRow {
        project_id: uuid(id),
        handle: CatalogText::new(handle),
        indexed: None,
        availability: Availability::Available,
    }
}

fn row(record: &str) -> CatalogEntryRow {
    CatalogEntryRow {
        project_id: uuid(P1),
        record_id: CatalogText::new(record),
        part: CatalogText::new("BRK-100"),
        revision: Some(CatalogText::new("B")),
        plan: CatalogText::new("plan-1"),
        run: CatalogText::new("run-1"),
        stage: CatalogText::new("released"),
        characteristic: CatalogText::new("flatness A"),
        timestamp: CatalogText::new("2026-11-01T08:00:00Z"),
        value: Some(EntryValue { literal: CatalogText::new("0.012"), unit: CatalogText::new("mm"), si: 1.2e-5 }),
        decision_outcome: DecisionOutcome::Conforms,
        rule_id: Some(CatalogText::new("unc.dec.simple@1")),
        evaluation_basis: EvaluationBasis::Features,
        uec_class: UecClass::Uec2,
        labels: EvidenceLabels { claimed: false, declared_input: false, uncharacterized_input: false },
        interpretation: Interpretation::SignedOff,
        record_status: RecordStatus::Active,
        journal_seq: 3,
        journal_entry_hash: digest(record),
        entry_status: EntryStatus::Ok,
    }
}

/// A raw connection to a catalog file, bypassing the crate (foreign keys on, as the crate has).
/// `Catalog::connection()` is crate-private, so the tests read and write the file this way.
fn raw_conn(p: &Path) -> Connection {
    let c = Connection::open(p).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    c
}

fn table_names(conn: &Connection) -> Vec<String> {
    let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
    let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

// ---------------------------------------------------------------- AC-1, REQ-005

/// AC-1: no catalog file; the location comes from a temporary user settings scope; a write
/// open creates an empty catalog at the current version with a UUIDv7 catalog ID.
#[test]
fn ac1_missing_catalog_is_created_at_the_configured_location() {
    let t = TempDir::new("ac1");
    let target = t.0.join("nested").join("dir").join("my-catalog.sqlite");
    let user_file = t.0.join("user").join("fiducial").join("settings.json");
    std::fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    let mut values = BTreeMap::new();
    values.insert(CATALOG_PATH_KEY.to_string(), CValue::Str(target.to_str().unwrap().to_string()));
    let mut doc = BTreeMap::new();
    doc.insert("format".to_string(), CValue::Str("fid-settings/1".into()));
    doc.insert("values".to_string(), CValue::Obj(values));
    std::fs::write(&user_file, fid_canon::to_canonical_bytes(&CValue::Obj(doc)).unwrap()).unwrap();
    let user = FileStore::new(Scope::User, user_file).unwrap();
    let machine = FileStore::new(Scope::Machine, t.0.join("machine").join("settings.json")).unwrap();
    let (path, warnings) = configured_catalog_path(&Sources { project: None, user: &user, machine: &machine }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(path, target);
    assert!(!path.exists());

    let cat = open_fixed(&path, Mode::ReadWrite, PLAN).unwrap();
    assert_eq!(cat.schema_version(), LATEST);
    assert_eq!(LATEST, 2);
    let id = cat.catalog_id();
    assert_eq!(id.version(), 7);
    assert_eq!(id.variant_bits(), 0b10);
    // RFC 9562 §5.7: the top 48 bits are the Unix milliseconds drawn from the source.
    assert_eq!(id.as_u128() >> 80, u128::from(MS));
    assert_eq!(cat.owner(), Some(&CatalogText::new("lab")));
    assert_eq!(cat.count_entries().unwrap(), 0);
    assert!(cat.projects().unwrap().is_empty());
    drop(cat);
    let conn = raw_conn(&path);
    assert_eq!(table_names(&conn), {
        let mut v: Vec<String> = TABLES.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    });
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    drop(conn);
    assert!(target.is_file());

    // Reopened: the same ID, no new draw (the source would fail), read-only works too.
    let again = open_existing(&target, Mode::ReadWrite, PLAN).unwrap();
    assert_eq!(again.catalog_id(), id);
    drop(again);
    let ro = open_existing(&target, Mode::ReadOnly, PLAN).unwrap();
    assert_eq!((ro.schema_version(), ro.catalog_id()), (LATEST, id));
}

#[test]
fn the_default_open_uses_the_system_source() {
    let t = TempDir::new("sys");
    let p = t.0.join("c.sqlite");
    let a = Catalog::open(&p, Mode::ReadWrite).unwrap();
    assert_eq!(a.catalog_id().version(), 7);
    assert_eq!(a.owner(), None);
}

/// The setting: user scope only, not admin, default "" meaning the per-user data directory.
#[test]
fn the_setting_is_declared_user_scope_with_an_empty_default() {
    let decls = catalog_settings();
    assert_eq!(decls.len(), 1);
    let d = decls[0];
    assert_eq!(
        (d.key, d.owner, d.default, d.scopes, d.admin),
        ("stats.catalog_path", "stats", "\"\"", &[Scope::User][..], false)
    );
    fid_store::settings::SettingsRegistry::build(&decls).unwrap();
}

/// Hand-labelled defaults per OS (Design: per-user data directory).
#[test]
fn default_location_per_os() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(*v))
    };
    let tail = ["fiducial", "catalog", "catalog.sqlite"];
    let join = |base: &str| tail.iter().fold(PathBuf::from(base), |p, s| p.join(s));
    assert_eq!(
        default_catalog_path("windows", env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")])),
        Some(join(r"C:\Users\u\AppData\Local"))
    );
    assert_eq!(default_catalog_path("windows", env(&[])), None);
    assert_eq!(
        default_catalog_path("macos", env(&[("HOME", "/Users/u")])),
        Some(join("/Users/u/Library/Application Support"))
    );
    assert_eq!(default_catalog_path("linux", env(&[("XDG_DATA_HOME", "/d"), ("HOME", "/h")])), Some(join("/d")));
    assert_eq!(
        default_catalog_path("linux", env(&[("XDG_DATA_HOME", "rel"), ("HOME", "/h")])),
        Some(join("/h/.local/share"))
    );
    assert_eq!(default_catalog_path("linux", env(&[("HOME", "relative")])), None);
}

#[test]
fn configured_values_are_checked() {
    let none = |_: &str| None;
    let home = |k: &str| {
        (k == "HOME" || k == "LOCALAPPDATA").then(|| OsString::from(if cfg!(windows) { r"C:\h" } else { "/h" }))
    };
    let os = std::env::consts::OS;
    assert!(catalog_location(&CValue::Str(String::new()), os, home).is_ok());
    let e = catalog_location(&CValue::Str(String::new()), os, none).unwrap_err();
    assert_eq!(e.code(), "stats.catalog_path_invalid");
    for bad in ["relative/catalog.sqlite", "catalog.sqlite", "/x\0y"] {
        assert_eq!(
            catalog_location(&CValue::Str(bad.into()), os, none).unwrap_err().code(),
            "stats.catalog_path_invalid",
            "{bad:?}"
        );
    }
    let long = format!("/{}", "a".repeat(4096));
    assert_eq!(catalog_location(&CValue::Str(long), os, none).unwrap_err().code(), "stats.catalog_path_invalid");
    assert_eq!(catalog_location(&CValue::Bool(true), os, none).unwrap_err().code(), "stats.catalog_path_invalid");
    let abs = std::env::temp_dir().join("c.sqlite");
    let abs = std::path::absolute(abs).unwrap();
    assert_eq!(catalog_location(&CValue::Str(abs.to_str().unwrap().into()), os, none).unwrap(), abs);
}

#[test]
fn a_relative_path_or_a_read_only_open_of_nothing_is_refused() {
    assert_eq!(
        Catalog::open(Path::new("rel.sqlite"), Mode::ReadWrite).unwrap_err().code(),
        "stats.catalog_path_invalid"
    );
    let t = TempDir::new("ro-missing");
    let p = t.0.join("none.sqlite");
    assert_eq!(Catalog::open(&p, Mode::ReadOnly).unwrap_err().code(), "stats.catalog_missing");
    assert!(!p.exists());
    let p = t.0.join("nodir").join("none.sqlite");
    assert_eq!(Catalog::open(&p, Mode::ReadOnly).unwrap_err().code(), "stats.catalog_missing");
    assert!(!p.parent().unwrap().exists());
    // An empty file (a creation that never migrated) is not a catalog to read.
    let p = t.0.join("empty.sqlite");
    std::fs::write(&p, b"").unwrap();
    assert_eq!(Catalog::open(&p, Mode::ReadOnly).unwrap_err().code(), "stats.catalog_missing");
    // ... but a write open initialises it.
    assert_eq!(open_fixed(&p, Mode::ReadWrite, PLAN).unwrap().schema_version(), LATEST);
}

// ---------------------------------------------------------------- AC-2, REQ-002

fn rows_of(conn: &Connection, sql: &str, n: usize) -> Vec<Vec<Value>> {
    let mut st = conn.prepare(sql).unwrap();
    let rows = st.query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect::<Result<Vec<_>, _>>()).unwrap();
    rows.map(Result::unwrap).collect()
}

/// AC-2: the v1 fixture opened by this v2 build migrates once, reads version 2 and keeps
/// every v1 row (expected rows from the Python plan).
#[test]
fn ac2_v1_fixture_migrates_once_and_keeps_every_row() {
    let (_t, p) = fixture_copy("ac2");
    let raw = Connection::open(&p).unwrap();
    assert_eq!(read_version(&raw).unwrap(), 1);
    assert!(!table_names(&raw).contains(&"entry_tag".to_string()));
    drop(raw);

    let want = json("schema_v1.json");
    let want = obj(&want);
    let cat = open_existing(&p, Mode::ReadWrite, PLAN).unwrap();
    assert_eq!(cat.schema_version(), 2);
    assert_eq!(cat.catalog_id().to_string(), s(&want["catalog_id"]));
    assert_eq!(cat.owner(), Some(&CatalogText::new(s(&want["owner"]))));

    let conn = raw_conn(&p);
    let columns = strings(&want["columns"]);
    let expected: Vec<Vec<Value>> =
        arr(&want["entries"]).iter().map(|r| arr(r).iter().map(sql_value).collect()).collect();
    let got = rows_of(
        &conn,
        &format!("SELECT {} FROM entry ORDER BY project_id, record_id", columns.join(", ")),
        columns.len(),
    );
    assert_eq!(got, expected);
    let projects: Vec<Vec<Value>> = arr(&want["projects"])
        .iter()
        .map(|p| {
            let o = obj(p);
            ["project_id", "handle", "last_seq", "head_hash", "availability"]
                .iter()
                .map(|k| sql_value(&o[*k]))
                .collect()
        })
        .collect();
    let got = rows_of(
        &conn,
        "SELECT project_id, handle, last_seq, head_hash, availability FROM project ORDER BY project_id",
        5,
    );
    assert_eq!(got, projects);
    // Every row also decodes through the typed API, in key order.
    let typed = cat.entries_after(None, 1000).unwrap();
    assert_eq!(typed.len(), expected.len());
    assert_eq!(cat.count_entries().unwrap(), expected.len() as u64);
    assert_eq!(cat.projects().unwrap().len(), 3);
    let claimed: Vec<bool> = typed.iter().map(CatalogEntryRow::is_claimed).collect();
    assert_eq!(claimed.iter().filter(|c| **c).count(), 1);
    // The v2 objects now exist.
    assert!(table_names(&conn).contains(&"entry_tag".to_string()));
    let n: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'entry_by_project_position'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    let n: i64 = conn.query_row("SELECT count(*) FROM entry_tag", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    drop(conn);
    drop(cat);

    // Once: a second write open finds version 2 and changes no row and no schema object.
    let before =
        rows_of(&Connection::open(&p).unwrap(), "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY name", 4);
    let again = open_existing(&p, Mode::ReadWrite, PLAN).unwrap();
    assert_eq!(again.schema_version(), 2);
    let conn = raw_conn(&p);
    let after = rows_of(&conn, "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY name", 4);
    assert_eq!(before, after);
    let v: i64 = conn.query_row("SELECT schema_version FROM catalog_meta", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 2);
}

/// A read-only open never migrates: an older file is refused and left as it was.
#[test]
fn a_read_only_open_of_an_older_catalog_is_refused_untouched() {
    let (_t, p) = fixture_copy("ro-old");
    let before = sha_file(&p);
    let e = open_existing(&p, Mode::ReadOnly, PLAN).unwrap_err();
    assert_eq!(e.code(), "stats.catalog_read_only");
    assert_eq!(sha_file(&p), before);
}

/// REQ-002: all pending migrations run in one transaction; a failing later step leaves the
/// file at its previous version with no object of the earlier steps.
#[test]
fn a_failing_migration_rolls_back_every_step() {
    static FAILING: [Migration; 3] = [
        Migration { from: 0, to: 1, sql: V1_SQL },
        Migration { from: 1, to: 2, sql: V2_SQL },
        Migration { from: 2, to: 3, sql: "CREATE TABLE entry (x INTEGER) STRICT;" },
    ];
    let (_t, p) = fixture_copy("rollback");
    let plan = MigrationPlan { latest: 3, migrations: &FAILING };
    match open_existing(&p, Mode::ReadWrite, plan).unwrap_err() {
        CatalogError::MigrationFailed { from: 2, to: 3, .. } => {}
        other => panic!("{other:?}"),
    }
    let raw = Connection::open(&p).unwrap();
    assert_eq!(read_version(&raw).unwrap(), 1);
    assert!(!table_names(&raw).contains(&"entry_tag".to_string()));
    drop(raw);
    // The same file still opens and migrates with this build's plan.
    assert_eq!(open_existing(&p, Mode::ReadWrite, PLAN).unwrap().schema_version(), 2);
}

#[test]
fn a_migration_waits_for_and_then_reports_a_held_lock() {
    let (_t, p) = fixture_copy("locked");
    // Put the fixture in a state where the migration needs the write lock someone else holds.
    let holder = Connection::open(&p).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let e = open_existing(&p, Mode::ReadWrite, PLAN).unwrap_err();
    assert_eq!(e.code(), "stats.catalog_locked");
    holder.execute_batch("ROLLBACK").unwrap();
    drop(holder);
    assert_eq!(open_existing(&p, Mode::ReadWrite, PLAN).unwrap().schema_version(), 2);
}

#[test]
fn the_plan_is_contiguous_and_starts_with_v1() {
    validate_plan(&PLAN).unwrap();
    assert_eq!(MIGRATIONS.len() as u32, LATEST);
    static GAP: [Migration; 2] = [Migration { from: 0, to: 1, sql: V1_SQL }, Migration { from: 2, to: 3, sql: V2_SQL }];
    assert_eq!(
        validate_plan(&MigrationPlan { latest: 3, migrations: &GAP }).unwrap_err().code(),
        "stats.catalog_migration_failed"
    );
    static NOT_V1: [Migration; 1] = [Migration { from: 0, to: 1, sql: V2_SQL }];
    assert!(validate_plan(&MigrationPlan { latest: 1, migrations: &NOT_V1 }).is_err());
    assert!(validate_plan(&MigrationPlan { latest: 3, migrations: &MIGRATIONS }).is_err());
    assert!(validate_plan(&MigrationPlan { latest: 0, migrations: &[] }).is_err());
    // The expected objects of each version are what the DDL builds.
    assert!(
        expected_objects(&PLAN, SchemaVersion(1)).unwrap().len()
            < expected_objects(&PLAN, SchemaVersion(2)).unwrap().len()
    );
}

// ---------------------------------------------------------------- AC-3

static FUTURE: [Migration; 3] = [
    Migration { from: 0, to: 1, sql: V1_SQL },
    Migration { from: 1, to: 2, sql: V2_SQL },
    Migration { from: 2, to: 3, sql: "CREATE TABLE future_thing (x INTEGER) STRICT;" },
];

/// AC-3: a catalog whose version is newer than the build: both opens fail with
/// `stats.catalog_too_new` and the file digest is unchanged.
#[test]
fn ac3_a_newer_catalog_is_refused_and_left_unchanged() {
    let t = TempDir::new("ac3");
    let p = t.0.join("catalog.sqlite");
    let future = MigrationPlan { latest: 3, migrations: &FUTURE };
    assert_eq!(open_fixed(&p, Mode::ReadWrite, future).unwrap().schema_version(), 3);
    let before = sha_file(&p);
    for mode in [Mode::ReadWrite, Mode::ReadOnly] {
        match open_existing(&p, mode, PLAN).unwrap_err() {
            CatalogError::TooNew { found: 3, supported: 2 } => {}
            other => panic!("{mode:?}: {other:?}"),
        }
        assert_eq!(sha_file(&p), before, "{mode:?}");
    }
    let e = Catalog::open(&p, Mode::ReadWrite).unwrap_err();
    assert_eq!(e.code(), "stats.catalog_too_new");
    assert_eq!(sha_file(&p), before);
}

// ---------------------------------------------------------------- AC-4, REQ-001

/// AC-4: two rows with the same (project ID, record ID) fail with a constraint error, both
/// through the API and through a raw connection to the file.
#[test]
fn ac4_duplicate_key_is_a_constraint_error() {
    let t = TempDir::new("ac4");
    let p = t.0.join("c.sqlite");
    let mut cat = open_fixed(&p, Mode::ReadWrite, PLAN).unwrap();
    cat.insert_project(&project(P1, "bracket-line")).unwrap();
    cat.insert_entry(&row("r-1")).unwrap();
    let e = cat.insert_entry(&row("r-1")).unwrap_err();
    assert_eq!(e.code(), "stats.catalog_constraint");
    let mut other = row("r-1");
    other.part = CatalogText::new("another part");
    assert_eq!(cat.insert_entry(&other).unwrap_err().code(), "stats.catalog_constraint");
    assert_eq!(cat.count_entries().unwrap(), 1);
    drop(cat);

    let raw = Connection::open(&p).unwrap();
    let err = raw.execute("INSERT INTO entry SELECT * FROM entry WHERE record_id = 'r-1'", []).unwrap_err();
    assert_eq!(err.sqlite_error_code(), Some(ErrorCode::ConstraintViolation));
}

/// REQ-001: every row carries its source journal entry hash; it reads back as written.
#[test]
fn entries_round_trip_with_their_journal_entry_hash() {
    let t = TempDir::new("rt");
    let p = t.0.join("c.sqlite");
    let mut cat = open_fixed(&p, Mode::ReadWrite, PLAN).unwrap();
    cat.insert_project(&project(P1, "bracket-line")).unwrap();
    let mut a = row("r-a");
    a.characteristic = CatalogText::new("\u{202e}evil\u{202c} \\ name");
    a.value = Some(EntryValue { literal: CatalogText::new("-0"), unit: CatalogText::new("mm"), si: -0.0 });
    let mut b = row("r-b");
    b.uec_class = UecClass::Uec1;
    b.labels = EvidenceLabels { claimed: true, declared_input: true, uncharacterized_input: false };
    b.interpretation = Interpretation::Provisional;
    let mut c = row("r-c");
    c.uec_class = UecClass::Uec0;
    c.value = None;
    c.decision_outcome = DecisionOutcome::NotDecidable;
    c.rule_id = None;
    c.revision = None;
    for r in [&c, &a, &b] {
        cat.insert_entry(r).unwrap();
    }
    let got = cat.entry(&uuid(P1), &CatalogText::new("r-b")).unwrap().unwrap();
    assert_eq!(got, b);
    assert!(got.is_claimed() && got.is_provisional());
    assert_eq!(got.journal_entry_hash, digest("r-b"));
    let back_a = cat.entry(&uuid(P1), &CatalogText::new("r-a")).unwrap().unwrap();
    // Negative zero is stored as zero.
    assert_eq!(back_a.value.as_ref().unwrap().si.to_bits(), 0.0f64.to_bits());
    assert_eq!(back_a.characteristic, a.characteristic);
    // Untrusted text is shown escaped.
    assert_eq!(back_a.characteristic.as_display().to_string(), "\\u{202e}evil\\u{202c} \\\\ name");
    assert_eq!(cat.entry(&uuid(P1), &CatalogText::new("nope")).unwrap(), None);
    // Key order, whatever the insertion order; pages continue after the last key.
    let all = cat.entries_after(None, 10).unwrap();
    let keys: Vec<String> = all.iter().map(|r| r.record_id.as_display().to_string()).collect();
    assert_eq!(keys, ["r-a", "r-b", "r-c"]);
    let page = cat.entries_after(Some((&uuid(P1), &CatalogText::new("r-a"))), 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].record_id, CatalogText::new("r-b"));
    // Tags (v2).
    cat.insert_tag(&uuid(P1), &CatalogText::new("r-a"), &CatalogText::new("golden")).unwrap();
    cat.insert_tag(&uuid(P1), &CatalogText::new("r-a"), &CatalogText::new("audit")).unwrap();
    assert_eq!(
        cat.insert_tag(&uuid(P1), &CatalogText::new("r-a"), &CatalogText::new("audit")).unwrap_err().code(),
        "stats.catalog_constraint"
    );
    assert_eq!(
        cat.insert_tag(&uuid(P1), &CatalogText::new("zz"), &CatalogText::new("x")).unwrap_err().code(),
        "stats.catalog_constraint"
    );
    assert_eq!(
        cat.tags(&uuid(P1), &CatalogText::new("r-a")).unwrap(),
        [CatalogText::new("audit"), CatalogText::new("golden")]
    );
    // Non-finite SI values never reach the file.
    let mut nan = row("r-nan");
    nan.value = Some(EntryValue { literal: CatalogText::new("1"), unit: CatalogText::new("mm"), si: f64::NAN });
    assert_eq!(cat.insert_entry(&nan).unwrap_err().code(), "stats.catalog_constraint");
}

#[test]
fn project_handles_are_unique_ignoring_ascii_case() {
    let t = TempDir::new("handles");
    let p = t.0.join("c.sqlite");
    let mut cat = open_fixed(&p, Mode::ReadWrite, PLAN).unwrap();
    cat.insert_project(&project(P1, "Bracket")).unwrap();
    let e = cat.insert_project(&project("01936b2e-7a00-7000-8000-000000000009", "bRACKET")).unwrap_err();
    assert_eq!(e.code(), "stats.catalog_constraint");
    let mut indexed = project("01936b2e-7a00-7000-8000-000000000002", "other");
    indexed.indexed = Some((7, digest("head")));
    cat.insert_project(&indexed).unwrap();
    assert_eq!(cat.project(&indexed.project_id).unwrap(), Some(indexed));
    let ro = open_existing(&p, Mode::ReadOnly, PLAN);
    drop(cat);
    let mut ro = ro.unwrap();
    assert_eq!(
        ro.insert_project(&project("01936b2e-7a00-7000-8000-000000000003", "x")).unwrap_err().code(),
        "stats.catalog_read_only"
    );
}

/// The hand-labelled cases of `schema_cases.json`, each in a transaction rolled back after.
#[test]
fn hand_labelled_row_cases() {
    let t = TempDir::new("cases");
    let p = t.0.join("c.sqlite");
    let mut cat = open_fixed(&p, Mode::ReadWrite, PLAN).unwrap();
    let doc = json("schema_cases.json");
    let doc = obj(&doc);
    let columns = strings(&doc["columns"]);
    for pr in arr(&doc["projects"]) {
        let f = arr(pr);
        let indexed = match (&f[2], &f[3]) {
            (CValue::Null, CValue::Null) => None,
            (n, CValue::Str(h)) => {
                let n = match n {
                    CValue::Num(x) => *x as u64,
                    CValue::U64(x) => *x,
                    CValue::I64(x) => *x as u64,
                    other => panic!("{other:?}"),
                };
                Some((n, Digest::parse(h).unwrap()))
            }
            other => panic!("{other:?}"),
        };
        let availability = Availability::parse(s(&f[4])).unwrap();
        cat.insert_project(&ProjectRow {
            project_id: uuid(s(&f[0])),
            handle: CatalogText::new(s(&f[1])),
            indexed,
            availability,
        })
        .unwrap();
    }
    let cases = arr(&doc["cases"]);
    assert!(cases.len() >= 50, "{}", cases.len());
    drop(cat);
    let mut conn = raw_conn(&p);
    for case in cases {
        let c = obj(case);
        let name = s(&c["name"]);
        let tx = conn.transaction().unwrap();
        for setup in arr(&c["setup"]) {
            insert_raw(&tx, &columns, arr(setup)).unwrap_or_else(|e| panic!("{name}: setup: {e}"));
        }
        let got = insert_raw(&tx, &columns, arr(&c["row"]));
        match s(&c["expect"]) {
            "accept" => assert_eq!(got.as_ref().map_err(ToString::to_string), Ok(&1), "{name}"),
            "refuse" => {
                let e = got.expect_err(name);
                assert_eq!(e.sqlite_error_code(), Some(ErrorCode::ConstraintViolation), "{name}: {e}");
            }
            other => panic!("{name}: {other}"),
        }
        tx.rollback().unwrap();
    }
}

// ---------------------------------------------------------------- hostile files

fn new_catalog(tag: &str) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let p = t.0.join("c.sqlite");
    let mut cat = open_fixed(&p, Mode::ReadWrite, PLAN).unwrap();
    cat.insert_project(&project(P1, "bracket-line")).unwrap();
    cat.insert_entry(&row("r-1")).unwrap();
    drop(cat);
    (t, p)
}

fn assert_refused_unchanged(p: &Path, code: &str) {
    let before = sha_file(p);
    for mode in [Mode::ReadWrite, Mode::ReadOnly] {
        let e = open_existing(p, mode, PLAN).unwrap_err();
        assert_eq!(e.code(), code, "{mode:?}: {e}");
        assert_eq!(sha_file(p), before, "{mode:?}");
    }
}

#[test]
fn an_extra_schema_object_is_refused() {
    let (_t, p) = new_catalog("trigger");
    let raw = Connection::open(&p).unwrap();
    raw.execute_batch("CREATE TRIGGER sneaky AFTER INSERT ON entry BEGIN DELETE FROM project; END;").unwrap();
    raw.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(raw);
    assert_refused_unchanged(&p, "stats.catalog_corrupt");
}

#[test]
fn a_replaced_catalog_meta_is_never_queried() {
    let t = TempDir::new("meta-view");
    let p = t.0.join("c.sqlite");
    let raw = Connection::open(&p).unwrap();
    raw.execute_batch(
        "CREATE TABLE x (a INTEGER); CREATE VIEW catalog_meta AS SELECT 1 AS singleton, 2 AS schema_version, \
         '01936b2e-7a00-7000-8000-00000000c0de' AS catalog_id, NULL AS owner;",
    )
    .unwrap();
    drop(raw);
    assert_refused_unchanged(&p, "stats.catalog_corrupt");
}

#[test]
fn a_changed_check_constraint_is_refused() {
    // Same table names, one CHECK loosened: the SQL text differs from the DDL.
    let t = TempDir::new("loose");
    let p = t.0.join("c.sqlite");
    let raw = Connection::open(&p).unwrap();
    raw.execute_batch(&V1_SQL.replace("CHECK (claimed = 0 OR uec_class = 'UEC-1')", "CHECK (1)")).unwrap();
    raw.execute("INSERT INTO catalog_meta VALUES (1, 1, '01936b2e-7a00-7000-8000-00000000c0de', NULL)", []).unwrap();
    drop(raw);
    assert_refused_unchanged(&p, "stats.catalog_corrupt");
}

#[test]
fn rows_that_bypass_the_checks_are_refused_on_read() {
    let (_t, p) = new_catalog("bypass");
    // Each edit breaks one table rule; the row is refused when read, whatever the file says.
    let edits = [
        "claimed = 1, uec_class = 'UEC-3'",
        "declared_input = 1",
        "uncharacterized_input = 1, uec_class = 'UEC-3'",
        "uec_class = 'UEC-0', value_literal = NULL, unit = NULL, si_value = NULL, uncharacterized_input = 1",
        "timestamp = '2026-11-01 08:00:00Z'",
        "timestamp = '2026-11-01T08:00:00+01:00'",
        "value_literal = '1.0mm'",
        "rule_id = NULL",
    ];
    for edit in edits {
        let (_t, p) = new_catalog("bypass");
        let raw = Connection::open(&p).unwrap();
        raw.execute_batch("PRAGMA ignore_check_constraints = ON").unwrap();
        assert_eq!(raw.execute(&format!("UPDATE entry SET {edit}"), []).unwrap(), 1, "{edit}");
        drop(raw);
        let cat = open_existing(&p, Mode::ReadOnly, PLAN).unwrap();
        let e = cat.entries_after(None, 10).unwrap_err();
        assert_eq!(e.code(), "stats.catalog_corrupt", "{edit}");
    }
}

/// The UEC label rules (D-045, UNC.UEC.CLASS) through the typed API.
#[test]
fn uec_labels_must_agree_with_the_class() {
    let (_t, p) = new_catalog("labels");
    let mut cat = open_existing(&p, Mode::ReadWrite, PLAN).unwrap();
    let mut n = 0;
    let mut next = |f: &dyn Fn(&mut CatalogEntryRow)| {
        n += 1;
        let mut r = row(&format!("l-{n}"));
        f(&mut r);
        r
    };
    let refused = [
        next(&|r| r.labels.declared_input = true),
        next(&|r| {
            r.uec_class = UecClass::Uec3;
            r.labels.uncharacterized_input = true;
        }),
        next(&|r| {
            r.uec_class = UecClass::Uec0;
            r.value = None;
            r.labels.declared_input = true;
        }),
        next(&|r| r.labels.claimed = true),
    ];
    for r in &refused {
        assert_eq!(cat.insert_entry(r).unwrap_err().code(), "stats.catalog_constraint", "{r:?}");
    }
    let accepted = [
        next(&|r| {
            r.uec_class = UecClass::Uec1;
            r.labels.uncharacterized_input = true;
        }),
        next(&|r| {
            r.uec_class = UecClass::Uec1;
            r.labels = EvidenceLabels { claimed: true, declared_input: true, uncharacterized_input: true };
        }),
    ];
    for r in &accepted {
        cat.insert_entry(r).unwrap();
        assert_eq!(cat.entry(&r.project_id, &r.record_id).unwrap().as_ref(), Some(r));
    }
}

#[test]
fn garbage_and_a_bad_meta_row_are_corrupt() {
    let t = TempDir::new("garbage");
    let p = t.0.join("c.sqlite");
    std::fs::write(&p, b"this is not an SQLite database, it only looks like a file....................").unwrap();
    assert_refused_unchanged(&p, "stats.catalog_corrupt");

    let (_t2, p) = new_catalog("meta-row");
    let raw = Connection::open(&p).unwrap();
    raw.execute("UPDATE catalog_meta SET catalog_id = '01936b2e-7a00-4000-8000-00000000c0de'", []).unwrap();
    raw.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(raw);
    // A catalog ID that is not a UUIDv7.
    assert_refused_unchanged(&p, "stats.catalog_corrupt");
}

#[test]
fn a_non_empty_rollback_journal_is_refused() {
    let (_t, p) = new_catalog("journal");
    let j = PathBuf::from(format!("{}-journal", p.display()));
    std::fs::write(&j, b"leftover").unwrap();
    assert_refused_unchanged(&p, "stats.catalog_corrupt");
}

#[test]
fn a_directory_at_the_path_is_refused() {
    let t = TempDir::new("dir");
    let p = t.0.join("c.sqlite");
    std::fs::create_dir_all(&p).unwrap();
    assert_eq!(Catalog::open(&p, Mode::ReadWrite).unwrap_err().code(), "stats.catalog_path_invalid");
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_refused() {
    let (t, p) = new_catalog("link");
    let link = t.0.join("link.sqlite");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    assert_eq!(Catalog::open(&link, Mode::ReadWrite).unwrap_err().code(), "stats.catalog_path_invalid");
    assert_eq!(Catalog::open(&link, Mode::ReadOnly).unwrap_err().code(), "stats.catalog_path_invalid");
    // A dangling link where a new catalog would be created is never followed.
    let dangling = t.0.join("new.sqlite");
    std::os::unix::fs::symlink(t.0.join("elsewhere.sqlite"), &dangling).unwrap();
    assert_eq!(Catalog::open(&dangling, Mode::ReadWrite).unwrap_err().code(), "stats.catalog_path_invalid");
    assert!(!t.0.join("elsewhere.sqlite").exists());
    // A linked companion file.
    let wal = PathBuf::from(format!("{}-wal", p.display()));
    let _ = std::fs::remove_file(&wal);
    std::os::unix::fs::symlink(t.0.join("x"), &wal).unwrap();
    assert_eq!(Catalog::open(&p, Mode::ReadOnly).unwrap_err().code(), "stats.catalog_path_invalid");
}

#[test]
fn error_codes_are_stable() {
    let all = [
        (CatalogError::TooNew { found: 3, supported: 2 }, "stats.catalog_too_new"),
        (CatalogError::Corrupt(String::new()), "stats.catalog_corrupt"),
        (CatalogError::Locked, "stats.catalog_locked"),
        (CatalogError::ReadOnly(String::new()), "stats.catalog_read_only"),
        (CatalogError::Missing(String::new()), "stats.catalog_missing"),
        (CatalogError::PathInvalid(String::new()), "stats.catalog_path_invalid"),
        (CatalogError::MigrationFailed { from: 1, to: 2, detail: String::new() }, "stats.catalog_migration_failed"),
        (CatalogError::Constraint(String::new()), "stats.catalog_constraint"),
        (CatalogError::Io(String::new()), "stats.catalog_io"),
    ];
    for (e, code) in all {
        assert_eq!(e.code(), code);
        assert!(e.to_string().starts_with(code), "{e}");
    }
}

#[test]
fn fixed_strings_round_trip() {
    for v in EntryStatus::ALL {
        assert_eq!(EntryStatus::parse(v.as_str()), Some(*v));
    }
    assert_eq!(
        EntryStatus::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
        ["ok", "unverified", "stale", "unavailable"]
    );
    assert_eq!(RecordStatus::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(), ["active", "voided", "superseded"]);
    assert_eq!(UecClass::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(), ["UEC-0", "UEC-1", "UEC-2", "UEC-3"]);
    assert_eq!(EntryStatus::parse("OK"), None);
    assert_eq!(Interpretation::parse("settled"), None);
}
