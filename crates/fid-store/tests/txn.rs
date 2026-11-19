//! Acceptance criteria of spec node CORE.STO.SQL.TXN (AC-1 .. AC-5), its requirements, the
//! hand-labelled commit cases in `fixtures/core/sto/sql/txn/cases.json`, panics inside a
//! commit, the busy bound, and crash safety: a child process is killed mid-transaction (at
//! fixed steps and at varying instants) and the reopened store must hold every acknowledged
//! commit and no part of an unacknowledged one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_store::cas::address_of;
use fid_store::layout::DB_FILE;
use fid_store::schema::{is_append_only, TABLES};
use fid_store::txn::{CommitEvent, CommitProbe};
use fid_store::{OpenMode, ProjectLocation, Store, StoreError, TxnError, Value};
use fid_types::Digest;
use rusqlite::Connection;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/sql/txn");

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-txn-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

/// A new project (layout only), lock released.
fn new_project(tag: &str) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    let (_loc, lock) = ProjectLocation::create(&root).unwrap();
    drop(lock);
    (t, root)
}

/// Opens for writing, retrying while a just-killed process's OS lock is still being released.
fn open_write(root: &Path) -> Store {
    let start = Instant::now();
    loop {
        match Store::open(root, OpenMode::Write) {
            Ok(s) => return s,
            Err(TxnError::Store(StoreError::Locked(_))) if start.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("open for writing failed: {e}"),
        }
    }
}

fn digest_of(text: &str) -> String {
    let hex: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

fn count(store: &Store, table: &str) -> i64 {
    store.read(|c| c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))).unwrap()
}

fn entry_values<'a>(hash: &str, branch: &'a str) -> Vec<(&'a str, Value)> {
    vec![
        ("hash", Value::from(hash)),
        ("stream", Value::from("s")),
        ("branch", Value::from(branch)),
        ("kind", Value::from("k")),
        ("header", Value::Blob(b"header".to_vec())),
    ]
}

// ------------------------------------------------------------------ probe

/// Logs every commit step; can fail or panic at one step.
#[derive(Default)]
struct Probe {
    events: Mutex<Vec<CommitEvent>>,
    fail_at: Option<CommitEvent>,
    panic_at: Option<CommitEvent>,
}

impl CommitProbe for Probe {
    fn on(&self, event: &CommitEvent) -> std::io::Result<()> {
        self.events.lock().unwrap_or_else(|p| p.into_inner()).push(event.clone());
        if self.panic_at.as_ref() == Some(event) {
            panic!("probe panic at {event:?}");
        }
        if self.fail_at.as_ref() == Some(event) {
            return Err(std::io::Error::other(format!("injected fault at {event:?}")));
        }
        Ok(())
    }
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1: both blob flushes happen before BEGIN; commit returns after SQLite reports the
/// commit (WAL synced under synchronous=FULL).
#[test]
fn ac1_blobs_flushed_before_begin_and_commit_after_wal() {
    let (_t, root) = new_project("ac1");
    let mut store = open_write(&root);
    let probe = Arc::new(Probe::default());
    store.set_commit_probe(Some(probe.clone()));

    let h = digest_of("ac1");
    let mut txn = store.transaction();
    let a = txn.put_blob(b"first blob".to_vec()).unwrap();
    let b = txn.put_blob(b"second blob".to_vec()).unwrap();
    txn.insert("entries", &entry_values(&h, "main"));
    txn.insert(
        "entry_blobs",
        &[("entry_hash", Value::from(h.as_str())), ("role", Value::from("a")), ("digest", Value::from(a.digest()))],
    );
    txn.insert(
        "entry_blobs",
        &[("entry_hash", Value::from(h.as_str())), ("role", Value::from("b")), ("digest", Value::from(b.digest()))],
    );
    // Nothing is written before commit.
    assert!(!store.blobs().has_blob(&a.digest()).unwrap());
    let done = txn.commit().unwrap();
    let events = probe.events.lock().unwrap().clone();
    assert_eq!(done.rows, 3);
    assert_eq!(
        events,
        vec![
            CommitEvent::BlobDurable { index: 0, digest: a.digest() },
            CommitEvent::BlobDurable { index: 1, digest: b.digest() },
            CommitEvent::Begin,
            CommitEvent::BeforeCommit,
            CommitEvent::Committed,
        ]
    );
    assert!(store.blobs().has_blob(&a.digest()).unwrap() && store.blobs().has_blob(&b.digest()).unwrap());
    // Durability settings of the commit connection (REQ-004).
    let (sync, mode, full): (i64, String, i64) = store
        .read(|c| {
            Ok((
                c.query_row("PRAGMA synchronous", [], |r| r.get(0))?,
                c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?,
                c.query_row("PRAGMA fullfsync", [], |r| r.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!((sync, mode.as_str(), full), (2, "wal", 1));
    drop(store);
    let store = open_write(&root);
    assert_eq!((count(&store, "entries"), count(&store, "entry_blobs")), (1, 2));
}

/// AC-2: a row referencing sha256:00..00, which is not stored, fails with dangling_blob.
#[test]
fn ac2_dangling_blob() {
    let (_t, root) = new_project("ac2");
    let store = open_write(&root);
    let zero = format!("sha256:{}", "0".repeat(64));
    let h = digest_of("ac2");
    let mut txn = store.transaction();
    txn.insert("entries", &entry_values(&h, "main"));
    txn.insert(
        "entry_blobs",
        &[("entry_hash", Value::from(h.as_str())), ("role", Value::from("in")), ("digest", Value::from(zero.as_str()))],
    );
    let err = txn.commit().unwrap_err();
    assert_eq!(err.code(), "dangling_blob");
    assert!(matches!(err, TxnError::DanglingBlob(d) if d == digest(&zero)));
    assert_eq!((count(&store, "entries"), count(&store, "entry_blobs")), (0, 0));
}

/// AC-3: a fault on the second blob's flush fails the commit; after reopen no row is visible
/// and the first blob is present but unreferenced.
#[test]
fn ac3_fault_on_second_blob_flush() {
    let (_t, root) = new_project("ac3");
    let mut store = open_write(&root);
    let a = address_of(b"blob one").unwrap();
    let b = address_of(b"blob two").unwrap();
    let probe = Arc::new(Probe {
        fail_at: Some(CommitEvent::BlobDurable { index: 1, digest: b.digest() }),
        ..Probe::default()
    });
    store.set_commit_probe(Some(probe.clone()));
    let h = digest_of("ac3");
    let mut txn = store.transaction();
    txn.put_blob(b"blob one".to_vec()).unwrap();
    txn.put_blob(b"blob two".to_vec()).unwrap();
    txn.insert("entries", &entry_values(&h, "main"));
    for (role, d) in [("a", a.digest()), ("b", b.digest())] {
        txn.insert(
            "entry_blobs",
            &[("entry_hash", Value::from(h.as_str())), ("role", Value::from(role)), ("digest", Value::from(d))],
        );
    }
    let err = txn.commit().unwrap_err();
    assert_eq!(err.code(), "io");
    // The SQLite transaction never began.
    assert!(!probe.events.lock().unwrap().contains(&CommitEvent::Begin));
    drop(store);

    let store = open_write(&root);
    assert_eq!((count(&store, "entries"), count(&store, "entry_blobs")), (0, 0));
    assert!(store.blobs().has_blob(&a.digest()).unwrap(), "first blob was put durably");
    let refs: i64 = store
        .read(|c| {
            c.query_row("SELECT count(*) FROM entry_blobs WHERE digest = ?1", [a.digest().as_str()], |r| r.get(0))
        })
        .unwrap();
    assert_eq!(refs, 0);
}

/// AC-4: two threads commit 1,000 transactions each; all 2,000 are present in a serial order
/// and the rows of different transactions never interleave.
#[test]
fn ac4_two_threads_serialised() {
    const PER_THREAD: u32 = 1000;
    let (_t, root) = new_project("ac4");
    let store = Arc::new(open_write(&root));
    let mut names: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
    for t in 0..2u32 {
        for i in 0..PER_THREAD {
            for part in 0..2u32 {
                names.insert(digest_of(&format!("ac4-{t}-{i}-{part}")), (t, i, part));
            }
        }
    }
    let workers: Vec<_> = (0..2u32)
        .map(|t| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                let branch = format!("thread{t}");
                for i in 0..PER_THREAD {
                    let mut txn = store.transaction();
                    let h0 = digest_of(&format!("ac4-{t}-{i}-0"));
                    let h1 = digest_of(&format!("ac4-{t}-{i}-1"));
                    txn.insert("entries", &entry_values(&h0, &branch));
                    txn.insert("entries", &entry_values(&h1, &branch));
                    txn.upsert(
                        "heads",
                        &[("branch", Value::from(branch.as_str()))],
                        &[("entry_hash", Value::from(h1.as_str()))],
                    );
                    assert_eq!(txn.commit().unwrap().rows, 3);
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let hashes: Vec<String> = store
        .read(|c| {
            let mut st = c.prepare("SELECT hash FROM entries ORDER BY seq")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect()
        })
        .unwrap();
    assert_eq!(hashes.len(), 4 * PER_THREAD as usize);
    let mut next = [0u32; 2];
    for pair in hashes.chunks(2) {
        let (t0, i0, p0) = names[&pair[0]];
        let (t1, i1, p1) = names[&pair[1]];
        assert_eq!((t0, i0, p0), (t1, i1, 0), "rows of two transactions interleave");
        assert_eq!(p1, 1);
        assert_eq!(i0, next[t0 as usize], "thread {t0} commits out of call order");
        next[t0 as usize] += 1;
    }
    assert_eq!(next, [PER_THREAD, PER_THREAD]);
    assert_eq!(count(&store, "heads"), 2);
}

/// AC-5: a commit on a store opened read-only fails with read_only.
#[test]
fn ac5_read_only() {
    let (_t, root) = new_project("ac5");
    drop(open_write(&root));
    let store = Store::open(&root, OpenMode::Read).unwrap();
    let mut txn = store.transaction();
    txn.put_blob(b"never written".to_vec()).unwrap();
    txn.insert("entries", &entry_values(&digest_of("ac5"), "main"));
    let err = txn.commit().unwrap_err();
    assert_eq!(err.code(), "read_only");
    assert_eq!(count(&store, "entries"), 0);
    assert!(!store.blobs().has_blob(&address_of(b"never written").unwrap().digest()).unwrap());
}

// ------------------------------------------------------------------ hand-labelled cases

fn to_value(v: &CValue, put: &[Digest], stored: &[Digest]) -> Value {
    match v {
        CValue::Null => Value::Null,
        CValue::Str(s) => Value::Text(s.clone()),
        CValue::Num(n) => Value::Integer(*n as i64),
        CValue::Obj(o) => match o.iter().next() {
            Some((k, CValue::Str(s))) if k == "bytes" => Value::Blob(s.as_bytes().to_vec()),
            Some((k, CValue::Num(i))) if k == "blob" => Value::from(put[*i as usize]),
            Some((k, CValue::Num(i))) if k == "stored" => Value::from(stored[*i as usize]),
            _ => panic!("unknown value {v:?}"),
        },
        _ => panic!("unknown value {v:?}"),
    }
}

fn texts(v: Option<&CValue>) -> Vec<String> {
    match v {
        None => Vec::new(),
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                _ => panic!("expected a string"),
            })
            .collect(),
        _ => panic!("expected an array"),
    }
}

fn indices(v: Option<&CValue>) -> Vec<usize> {
    match v {
        None => Vec::new(),
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|x| match x {
                CValue::Num(n) => *n as usize,
                _ => panic!("expected a number"),
            })
            .collect(),
        _ => panic!("expected an array"),
    }
}

fn columns(v: Option<&CValue>, put: &[Digest], stored: &[Digest]) -> Vec<(String, Value)> {
    match v {
        None => Vec::new(),
        Some(CValue::Obj(o)) => o.iter().map(|(k, x)| (k.clone(), to_value(x, put, stored))).collect(),
        _ => panic!("expected an object"),
    }
}

#[test]
fn hand_labelled_cases() {
    let raw = std::fs::read(Path::new(FIXTURES).join("cases.json")).unwrap();
    let CValue::Obj(doc) = fid_canon::parse(&raw).unwrap() else { panic!("not an object") };
    let Some(CValue::Arr(cases)) = doc.get("cases") else { panic!("no cases") };
    assert!(cases.len() >= 15);
    for case in cases {
        let CValue::Obj(c) = case else { panic!("case is not an object") };
        let Some(CValue::Str(name)) = c.get("name") else { panic!("case without name") };
        let (_t, root) = new_project("case");
        let store = open_write(&root);
        let stored: Vec<Digest> = texts(c.get("stored"))
            .iter()
            .map(|s| store.blobs().put_bytes(s.as_bytes(), Default::default()).unwrap().digest())
            .collect();
        let mut txn = store.transaction();
        let put: Vec<Digest> =
            texts(c.get("blobs")).iter().map(|s| txn.put_blob(s.as_bytes().to_vec()).unwrap().digest()).collect();
        let Some(CValue::Arr(ops)) = c.get("ops") else { panic!("{name}: no ops") };
        for op in ops {
            let CValue::Obj(o) = op else { panic!("{name}: op") };
            let Some(CValue::Str(table)) = o.get("table") else { panic!("{name}: table") };
            let values = columns(o.get("values"), &put, &stored);
            let vals: Vec<(&str, Value)> = values.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            match o.get("op") {
                Some(CValue::Str(s)) if s == "insert" => {
                    txn.insert(table, &vals);
                }
                Some(CValue::Str(s)) if s == "upsert" => {
                    let key = columns(o.get("key"), &put, &stored);
                    let key: Vec<(&str, Value)> = key.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                    txn.upsert(table, &key, &vals);
                }
                other => panic!("{name}: op {other:?}"),
            }
        }
        let Some(CValue::Str(expect)) = c.get("expect") else { panic!("{name}: expect") };
        match txn.commit() {
            Ok(done) => {
                assert_eq!(expect, "ok", "{name}: committed, expected {expect}");
                let Some(CValue::Num(rows)) = c.get("rows") else { panic!("{name}: rows") };
                assert_eq!(done.rows, *rows as u64, "{name}: rows");
            }
            Err(e) => assert_eq!(e.code(), expect, "{name}: {e}"),
        }
        let Some(CValue::Obj(counts)) = c.get("counts") else { panic!("{name}: counts") };
        for table in TABLES.iter().filter(|t| **t != "schema_meta") {
            let want = match counts.get(*table) {
                Some(CValue::Num(n)) => *n as i64,
                None => 0,
                other => panic!("{name}: count {other:?}"),
            };
            assert_eq!(count(&store, table), want, "{name}: rows of {table}");
        }
        for i in indices(c.get("blobs_present")) {
            assert!(store.blobs().has_blob(&put[i]).unwrap(), "{name}: blob {i} missing");
        }
        for i in indices(c.get("blobs_absent")) {
            assert!(!store.blobs().has_blob(&put[i]).unwrap(), "{name}: blob {i} written");
        }
        if let Some(CValue::Arr(checks)) = c.get("checks") {
            for check in checks {
                let CValue::Obj(k) = check else { panic!("{name}: check") };
                let (Some(CValue::Str(sql)), Some(CValue::Str(text))) = (k.get("sql"), k.get("text")) else {
                    panic!("{name}: check fields")
                };
                let got: String = store.read(|c| c.query_row(sql, [], |r| r.get(0))).unwrap();
                assert_eq!(&got, text, "{name}: {sql}");
            }
        }
    }
}

// ------------------------------------------------------------------ error paths

/// A panic inside the SQLite transaction (from a probe callback) rolls back; the store stays
/// usable and the next commit succeeds.
#[test]
fn panic_inside_commit_rolls_back() {
    let (_t, root) = new_project("panic");
    let mut store = open_write(&root);
    store.set_commit_probe(Some(Arc::new(Probe { panic_at: Some(CommitEvent::BeforeCommit), ..Probe::default() })));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut txn = store.transaction();
        txn.insert("entries", &entry_values(&digest_of("p1"), "main"));
        txn.commit()
    }));
    assert!(result.is_err(), "the probe panic propagates");
    assert_eq!(count(&store, "entries"), 0, "rows of the panicked commit are visible");

    // A panic inside a read callback leaves query_only on until the next use resets it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.read(|_| -> rusqlite::Result<()> { panic!("read callback panic") });
    }));
    assert!(result.is_err());

    store.set_commit_probe(None);
    let mut txn = store.transaction();
    txn.insert("entries", &entry_values(&digest_of("p2"), "main"));
    txn.commit().unwrap();
    drop(store);
    let store = open_write(&root);
    let hashes: Vec<String> = store
        .read(|c| {
            let mut st = c.prepare("SELECT hash FROM entries")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect()
        })
        .unwrap();
    assert_eq!(hashes, vec![digest_of("p2")]);
}

/// A fault after the rows are written but before COMMIT rolls every row back.
#[test]
fn fault_before_commit_rolls_back() {
    let (_t, root) = new_project("prefault");
    let mut store = open_write(&root);
    store.set_commit_probe(Some(Arc::new(Probe { fail_at: Some(CommitEvent::BeforeCommit), ..Probe::default() })));
    let mut txn = store.transaction();
    let a = txn.put_blob(b"kept".to_vec()).unwrap();
    let h = digest_of("prefault");
    txn.insert("entries", &entry_values(&h, "main"));
    txn.insert(
        "entry_blobs",
        &[("entry_hash", Value::from(h.as_str())), ("role", Value::from("a")), ("digest", Value::from(a.digest()))],
    );
    assert_eq!(txn.commit().unwrap_err().code(), "io");
    assert_eq!((count(&store, "entries"), count(&store, "entry_blobs")), (0, 0));
    assert!(store.blobs().has_blob(&a.digest()).unwrap());
}

/// The append-only triggers still hold for a raw connection after commits through Txn, and an
/// update through Txn is refused.
#[test]
fn append_only_still_holds() {
    let (_t, root) = new_project("ao");
    let store = open_write(&root);
    let h = digest_of("ao");
    let mut txn = store.transaction();
    txn.insert("entries", &entry_values(&h, "main"));
    txn.commit().unwrap();
    let mut txn = store.transaction();
    txn.upsert("entries", &[("hash", Value::from(h.as_str()))], &[("kind", Value::from("x"))]);
    assert_eq!(txn.commit().unwrap_err().code(), "invalid_row");
    let mut txn = store.transaction();
    txn.insert("entries", &entry_values(&h, "other"));
    assert_eq!(txn.commit().unwrap_err().code(), "append_only");
    drop(store);
    let raw = Connection::open(root.join(DB_FILE)).unwrap();
    assert!(is_append_only(&raw.execute("UPDATE entries SET kind = 'x'", []).unwrap_err()));
    assert!(is_append_only(&raw.execute("DELETE FROM entries", []).unwrap_err()));
}

/// A foreign connection holding the write lock makes the commit wait for the busy timeout
/// (5 s), then fail with busy and roll back; once the lock is free the commit succeeds.
#[test]
fn busy_is_bounded() {
    let (_t, root) = new_project("busy");
    let store = open_write(&root);
    let raw = Connection::open(root.join(DB_FILE)).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE").unwrap();
    let start = Instant::now();
    let mut txn = store.transaction();
    txn.insert("entries", &entry_values(&digest_of("busy"), "main"));
    let err = txn.commit().unwrap_err();
    let waited = start.elapsed();
    assert_eq!(err.code(), "busy", "{err}");
    assert!(waited >= Duration::from_secs(4) && waited < Duration::from_secs(60), "waited {waited:?}");
    raw.execute_batch("ROLLBACK").unwrap();
    let mut txn = store.transaction();
    txn.insert("entries", &entry_values(&digest_of("busy"), "main"));
    txn.commit().unwrap();
    assert_eq!(count(&store, "entries"), 1);
}

// ------------------------------------------------------------------ already-stored blobs

/// A commit referencing the stored blob `d` (plus one pending blob); returns the result.
fn commit_reference(store: &Store, d: Digest) -> Result<(), TxnError> {
    let h = digest_of("ref");
    let mut txn = store.transaction();
    let p = txn.put_blob(b"pending blob of the refused commit".to_vec())?;
    txn.insert("entries", &entry_values(&h, "main"));
    for (role, x) in [("stored", d), ("pending", p.digest())] {
        txn.insert(
            "entry_blobs",
            &[("entry_hash", Value::from(h.as_str())), ("role", Value::from(role)), ("digest", Value::from(x))],
        );
    }
    txn.commit().map(|_| ())
}

fn assert_nothing_written(store: &Store) {
    assert_eq!((count(store, "entries"), count(store, "entry_blobs")), (0, 0));
    let p = address_of(b"pending blob of the refused commit").unwrap().digest();
    assert!(!store.blobs().has_blob(&p).unwrap(), "pending blob written by a refused commit");
}

/// A truncated or altered stored blob is refused with corrupt_blob before anything is written.
#[test]
fn corrupt_stored_blob_is_refused() {
    for how in ["truncate", "flip"] {
        let (_t, root) = new_project("corrupt");
        let store = open_write(&root);
        let content = b"stored earlier by a non-durable put".to_vec();
        let d = store.blobs().put_bytes(&content, Default::default()).unwrap().digest();
        let path = store.blobs().path_of(&d);
        let damaged = match how {
            "truncate" => content[..10].to_vec(),
            _ => content.iter().map(|b| b ^ 0x20).collect(),
        };
        std::fs::write(&path, damaged).unwrap();
        let err = commit_reference(&store, d).unwrap_err();
        assert_eq!(err.code(), "corrupt_blob", "{how}: {err}");
        assert!(matches!(err, TxnError::CorruptBlob(x) if x == d));
        assert_nothing_written(&store);
    }
}

/// A stored chunked object is checked chunk by chunk: a sound one commits, a damaged chunk is
/// corrupt_blob and a missing chunk dangling_blob, both before anything is written.
#[test]
fn stored_object_chunks_are_checked() {
    let big: Vec<u8> = (0..fid_store::cas::CHUNK_SIZE + 1000).map(|i| (i % 251) as u8).collect();
    for how in ["sound", "damage_chunk", "remove_chunk"] {
        let (_t, root) = new_project("object");
        let store = open_write(&root);
        let addr = store.blobs().put_bytes(&big, Default::default()).unwrap();
        let fid_store::Address::Object(o) = addr else { panic!("not chunked") };
        let chunks = store.blobs().manifest(&o).unwrap().chunks;
        assert_eq!(chunks.len(), 2);
        let last = store.blobs().path_of(&chunks[1]);
        match how {
            "damage_chunk" => std::fs::write(&last, vec![7u8; 1000]).unwrap(),
            "remove_chunk" => std::fs::remove_file(&last).unwrap(),
            _ => {}
        }
        let result = commit_reference(&store, o.manifest_digest);
        match how {
            "sound" => {
                result.unwrap();
                assert_eq!(count(&store, "entry_blobs"), 2);
            }
            "damage_chunk" => {
                let err = result.unwrap_err();
                assert!(matches!(err, TxnError::CorruptBlob(x) if x == chunks[1]), "{err}");
                assert_nothing_written(&store);
            }
            _ => {
                assert_eq!(result.unwrap_err().code(), "dangling_blob");
                assert_nothing_written(&store);
            }
        }
    }
}

/// `Txn::check_blob` runs the commit's blob check early and writes nothing: a pending blob and a
/// sound stored object pass, an absent blob is dangling_blob, a damaged chunk corrupt_blob and a
/// missing chunk dangling_blob naming that chunk.
#[test]
fn check_blob_runs_the_commit_check_early() {
    let big: Vec<u8> = (0..fid_store::cas::CHUNK_SIZE + 1000).map(|i| (i % 251) as u8).collect();
    let (_t, root) = new_project("check");
    let store = open_write(&root);
    let addr = store.blobs().put_bytes(&big, Default::default()).unwrap();
    let fid_store::Address::Object(o) = addr else { panic!("not chunked") };
    let chunks = store.blobs().manifest(&o).unwrap().chunks;
    let mut txn = store.transaction();
    let pending = txn.put_blob(b"pending only".to_vec()).unwrap().digest();
    txn.check_blob(&pending).unwrap();
    txn.check_blob(&o.manifest_digest).unwrap();
    let absent = address_of(b"never stored").unwrap().digest();
    assert!(matches!(txn.check_blob(&absent), Err(TxnError::DanglingBlob(x)) if x == absent));
    let last = store.blobs().path_of(&chunks[1]);
    std::fs::write(&last, vec![7u8; 1000]).unwrap();
    assert!(matches!(txn.check_blob(&o.manifest_digest), Err(TxnError::CorruptBlob(x)) if x == chunks[1]));
    std::fs::remove_file(&last).unwrap();
    assert!(matches!(txn.check_blob(&o.manifest_digest), Err(TxnError::DanglingBlob(x)) if x == chunks[1]));
    txn.abort();
    assert!(!store.blobs().has_blob(&pending).unwrap(), "check_blob or abort wrote a pending blob");
}

// ------------------------------------------------------------------ crash safety

const CRASH_ROOT_ENV: &str = "FID_TXN_CRASH_ROOT";
const CRASH_MODE_ENV: &str = "FID_TXN_CRASH_MODE";
/// Checkpoint rows per crash commit: makes the SQLite part of a commit long enough that kills
/// at varying instants land inside it.
const CRASH_ROWS: i64 = 200;

fn crash_blob(i: i64, part: &str) -> Vec<u8> {
    format!("crash-{i}-{part}").into_bytes()
}

/// Commit number `i` of the crash workload: two blobs, the entry with seq `i`, two
/// entry_blobs, CRASH_ROWS checkpoints and the head of main.
fn crash_commit(store: &Store, i: i64) -> Result<(), TxnError> {
    let h = digest_of(&format!("crash-{i}"));
    let mut txn = store.transaction();
    let a = txn.put_blob(crash_blob(i, "a"))?;
    let b = txn.put_blob(crash_blob(i, "b"))?;
    let mut entry = entry_values(&h, "main");
    entry.push(("seq", Value::Integer(i)));
    txn.insert("entries", &entry);
    for (role, d) in [("a", a.digest()), ("b", b.digest())] {
        txn.insert(
            "entry_blobs",
            &[("entry_hash", Value::from(h.as_str())), ("role", Value::from(role)), ("digest", Value::from(d))],
        );
    }
    for k in 0..CRASH_ROWS {
        txn.insert(
            "checkpoints",
            &[
                ("stream", Value::from("s")),
                ("entry_hash", Value::from(h.as_str())),
                ("body", Value::Blob(format!("{i}-{k}").repeat(8).into_bytes())),
            ],
        );
    }
    txn.upsert("heads", &[("branch", Value::from("main"))], &[("entry_hash", Value::from(h.as_str()))]);
    txn.commit().map(|_| ())
}

/// Hangs (printing HANG) at one step of the first commit, so the parent kills it there.
struct HangAt(fn(&CommitEvent) -> bool);

impl CommitProbe for HangAt {
    fn on(&self, event: &CommitEvent) -> std::io::Result<()> {
        if (self.0)(event) {
            let mut out = std::io::stdout().lock();
            writeln!(out, "\nHANG")?;
            out.flush()?;
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Ok(())
    }
}

/// The child side of the crash tests (runs only when spawned by them).
#[test]
#[ignore]
fn crash_child() {
    let Some(root) = std::env::var_os(CRASH_ROOT_ENV) else { return };
    let mode = std::env::var(CRASH_MODE_ENV).unwrap();
    let mut store = Store::open(Path::new(&root), OpenMode::Write).unwrap();
    match mode.as_str() {
        "hang_after_first_blob" => {
            store.set_commit_probe(Some(Arc::new(HangAt(|e| matches!(e, CommitEvent::BlobDurable { index: 0, .. })))));
        }
        "hang_before_commit" => {
            store.set_commit_probe(Some(Arc::new(HangAt(|e| *e == CommitEvent::BeforeCommit))));
        }
        _ => {}
    }
    let start: i64 =
        store.read(|c| c.query_row("SELECT coalesce(max(seq), 0) FROM entries", [], |r| r.get(0))).unwrap();
    {
        // libtest has already written "test crash_child ... " without a line end: start a
        // fresh line so the marker begins one.
        let mut out = std::io::stdout().lock();
        writeln!(out, "\nREADY {start}").unwrap();
        out.flush().unwrap();
    }
    // Bounded: the parent kills it long before this; a stray child still ends.
    for i in start + 1..start + 5_000 {
        crash_commit(&store, i).unwrap();
        let mut out = std::io::stdout().lock();
        writeln!(out, "ACK {i}").unwrap();
        out.flush().unwrap();
    }
}

/// Longest wait for one expected line from a crash child, and for its stdout to close after
/// the kill. A child that stalls fails the test with what it printed instead of hanging CI.
const CHILD_WATCHDOG: Duration = Duration::from_secs(120);

/// A crash child. Its stdout is read by a helper thread into a channel, so every wait is
/// bounded by [`CHILD_WATCHDOG`]. Dropping it (also when a test fails) kills and reaps it.
struct Child {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
    seen: Vec<String>,
    acked: i64,
    started: Instant,
}

impl Child {
    fn spawn(root: &Path, mode: &str) -> Child {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["crash_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CRASH_ROOT_ENV, root)
            .env(CRASH_MODE_ENV, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in out.lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut c = Child { child, lines, seen: Vec::new(), acked: 0, started: Instant::now() };
        c.wait_for("READY");
        eprintln!("[{:?}] child {} ({mode}) ready", c.started.elapsed(), c.child.id());
        c
    }

    /// Reads lines until one starts with `prefix`; records every ACK on the way.
    fn wait_for(&mut self, prefix: &str) -> String {
        loop {
            match self.lines.recv_timeout(CHILD_WATCHDOG) {
                Ok(line) => {
                    self.note(&line);
                    // Markers start a line, but match anywhere in case another writer shares it.
                    if line.contains(prefix) {
                        return line;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("watchdog: no {prefix} from the child within {CHILD_WATCHDOG:?}; it printed {:?}", self.seen)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    let status = self.child.wait().ok();
                    panic!("child exited ({status:?}) before {prefix}; it printed {:?}", self.seen)
                }
            }
        }
    }

    fn note(&mut self, line: &str) {
        if let Some(at) = line.find("ACK ") {
            self.acked = self.acked.max(line[at + 4..].trim().parse().unwrap());
        }
        if self.seen.len() < 200 {
            self.seen.push(line.to_string());
        }
    }

    /// Kills the child (SIGKILL / TerminateProcess), reaps it and returns the last commit it
    /// acknowledged: every ACK line it wrote before dying counts.
    fn kill(mut self) -> i64 {
        self.child.kill().unwrap();
        let status = self.child.wait().unwrap();
        assert!(!status.success(), "child exited on its own");
        loop {
            match self.lines.recv_timeout(CHILD_WATCHDOG) {
                Ok(line) => self.note(&line),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("watchdog: the killed child's stdout stayed open for {CHILD_WATCHDOG:?}")
                }
            }
        }
        eprintln!("[{:?}] child {} killed after ACK {}", self.started.elapsed(), self.child.id(), self.acked);
        self.acked
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reopens the store and checks that it holds commits 1..=n whole (n >= acked) and nothing
/// of any other commit; returns n.
fn check_after_crash(root: &Path, acked: i64) -> i64 {
    let store = open_write(root);
    let seqs: Vec<i64> = store
        .read(|c| {
            let mut st = c.prepare("SELECT seq FROM entries ORDER BY seq")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect()
        })
        .unwrap();
    let n = seqs.len() as i64;
    assert_eq!(seqs, (1..=n).collect::<Vec<_>>(), "entries are not exactly 1..=n");
    assert!(n >= acked, "an acknowledged commit was lost: {n} entries, {acked} acknowledged");
    assert_eq!(count(&store, "entry_blobs"), 2 * n);
    assert_eq!(count(&store, "checkpoints"), CRASH_ROWS * n);
    for i in 1..=n {
        let h = digest_of(&format!("crash-{i}"));
        let (blobs, checkpoints): (i64, i64) = store
            .read(|c| {
                Ok((
                    c.query_row("SELECT count(*) FROM entry_blobs WHERE entry_hash = ?1", [&h], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM checkpoints WHERE entry_hash = ?1", [&h], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((blobs, checkpoints), (2, CRASH_ROWS), "commit {i} is partial");
        for part in ["a", "b"] {
            let addr = address_of(&crash_blob(i, part)).unwrap();
            assert_eq!(store.blobs().get(&addr).unwrap(), crash_blob(i, part), "blob {part} of commit {i}");
        }
    }
    if n > 0 {
        let head: String = store
            .read(|c| c.query_row("SELECT entry_hash FROM heads WHERE branch = 'main'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(head, digest_of(&format!("crash-{n}")));
    } else {
        assert_eq!(count(&store, "heads"), 0);
    }
    n
}

/// Killed at fixed steps of a commit: after the first blob is durable (no row yet), and with
/// every row written but before COMMIT. Neither commit may be visible after reopen; the blobs
/// already put may remain, unreferenced.
#[test]
fn crash_at_fixed_steps_is_all_or_nothing() {
    let (_t, root) = new_project("crash-fixed");
    drop(open_write(&root));

    let mut c = Child::spawn(&root, "hang_after_first_blob");
    c.wait_for("HANG");
    assert_eq!(c.kill(), 0);
    assert_eq!(check_after_crash(&root, 0), 0);
    let store = open_write(&root);
    assert!(store.blobs().has_blob(&address_of(&crash_blob(1, "a")).unwrap().digest()).unwrap());
    assert!(!store.blobs().has_blob(&address_of(&crash_blob(1, "b")).unwrap().digest()).unwrap());
    drop(store);

    let mut c = Child::spawn(&root, "hang_before_commit");
    c.wait_for("HANG");
    assert_eq!(c.kill(), 0);
    assert_eq!(check_after_crash(&root, 0), 0);
    let store = open_write(&root);
    assert!(store.blobs().has_blob(&address_of(&crash_blob(1, "b")).unwrap().digest()).unwrap());
    drop(store);

    // The store is still fully usable after both kills.
    let store = open_write(&root);
    crash_commit(&store, 1).unwrap();
    drop(store);
    assert_eq!(check_after_crash(&root, 1), 1);
}

/// Killed at varying instants while committing in a loop: after every reopen the store holds
/// every acknowledged commit, each one whole, and no part of any later one.
#[test]
fn crash_at_varying_instants_keeps_acknowledged_commits() {
    let (_t, root) = new_project("crash-loop");
    drop(open_write(&root));
    let mut total = 0;
    for round in 0..8u64 {
        let mut c = Child::spawn(&root, "loop");
        // Let it acknowledge a few commits, then kill after a delay that varies per round.
        let target = total + 2;
        while c.acked < target {
            c.wait_for("ACK");
        }
        std::thread::sleep(Duration::from_millis((round * 13) % 47));
        let acked = c.kill();
        assert!(acked >= target);
        let n = check_after_crash(&root, acked);
        assert!(n >= total);
        total = n;
    }
    assert!(total >= 16);
}
