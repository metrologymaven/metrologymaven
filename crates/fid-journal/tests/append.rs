//! Acceptance criteria of spec node CORE.JRN.CHAIN.APPEND (AC-1 .. AC-5) and the pinned
//! rules of module `append` (time order, `head_moved`, atomicity under faults).
//!
//! Expected hashes, payload digests and time-order labels come from the independent Python
//! reference `fixtures/core/jrn/chain/append/make_fixtures.py`; the crash points' outcomes
//! are hand-labelled there. AC-3 uses raw SQL through `fid-store`'s connection, bypassing
//! the journal API.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue, Digest};
use fid_journal::{
    parse_digest, utc_order, verify_header_value, Append, AppendRequest, CrashPoint, DraftBody, EntryDraft, Field,
    Journal, JournalError, RecordedSalts, Salt, GENESIS_PREV, MAX_BATCH, MAX_BLOBS, MAX_QUEUE, ROLE_PAYLOAD, ROLE_REF,
    SYNCHRONOUS_FULL,
};
use fid_store::cas::{BlobStore, PutOpts};
use fid_store::schema::{is_append_only, open_read, open_write};
use fid_store::{OpenMode, ProjectLocation};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::{Arc, OnceLock};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/chain/append");
const STREAM: &str = "record";
/// Exit code of a crash child that reached its injection point.
const CRASH_EXIT: i32 = 86;

// ------------------------------------------------------------------ scaffolding

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name =
            format!("fid-jrn-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, AtomicOrdering::SeqCst));
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

/// A new project with an open journal.
fn new_journal(tag: &str) -> (TempDir, PathBuf, Journal) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let j = Journal::open(loc, lock, STREAM).unwrap();
    (t, root, j)
}

fn reopen(root: &Path) -> Journal {
    let (loc, lock) = ProjectLocation::open(root, OpenMode::Write).unwrap();
    Journal::open(loc, lock.unwrap(), STREAM).unwrap()
}

type Obj = BTreeMap<String, CValue>;

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("field {k} is {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("field {k} is {other:?}"),
    }
}

fn load(name: &str) -> CValue {
    parse(&std::fs::read(Path::new(FIXTURES).join(name)).unwrap()).unwrap()
}

/// The 100 entries of `chain.json`.
fn chain_fixture() -> Vec<Obj> {
    let v = load("chain.json");
    let o = obj(&v);
    assert_eq!(st(o, "stream"), STREAM);
    arr(o, "entries").iter().map(|e| obj(e).clone()).collect()
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

/// The fixture entry as a request: its draft, recorded salts and referenced blobs.
fn request(e: &Obj) -> AppendRequest {
    let draft = EntryDraft::from_value(e.get("draft").unwrap()).unwrap();
    let salts = RecordedSalts(
        obj(e.get("salts").unwrap())
            .iter()
            .map(|(k, v)| {
                let CValue::Str(hex) = v else { panic!("salt must be a string") };
                (Field::parse(k).unwrap(), Salt::from_hex(hex).unwrap())
            })
            .collect(),
    );
    let blobs = arr(e, "blobs").iter().map(|b| digest(st(obj(b), "digest"))).collect();
    AppendRequest { draft, blobs, expected_prev: None, salts: Some(salts) }
}

/// Puts the fixture entry's referenced blobs into the project's blob store.
fn put_blobs(j: &Journal, e: &Obj) {
    for b in arr(e, "blobs") {
        let b = obj(b);
        let addr = j.blobs().put_bytes(st(b, "content").as_bytes(), PutOpts { durable: true }).unwrap();
        assert_eq!(addr.digest().as_str(), st(b, "digest"));
    }
}

fn append_fixture(j: &Journal, e: &Obj) -> Digest {
    put_blobs(j, e);
    let h = j.append(request(e)).unwrap();
    assert_eq!(h.to_string(), st(e, "hash"), "{}", st(e, "name"));
    h
}

/// One stored entry, read through a raw read-only connection.
#[derive(Debug)]
struct Row {
    seq: i64,
    hash: String,
    prev: Option<String>,
    stream: String,
    header: Vec<u8>,
    refs: Vec<(String, String)>,
}

fn rows(root: &Path) -> Vec<Row> {
    let (loc, _) = ProjectLocation::open(root, OpenMode::Read).unwrap();
    let db = open_read(&loc).unwrap();
    let c = db.connection();
    let mut st = c.prepare("SELECT seq, hash, prev_hash, stream, header FROM entries ORDER BY seq").unwrap();
    let mut out: Vec<Row> = st
        .query_map([], |r| {
            Ok(Row { seq: r.get(0)?, hash: r.get(1)?, prev: r.get(2)?, stream: r.get(3)?, header: r.get(4)?, refs: vec![] })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut refs = c.prepare("SELECT role, digest FROM entry_blobs WHERE entry_hash = ?1 ORDER BY role, digest").unwrap();
    for row in &mut out {
        row.refs = refs
            .query_map([row.hash.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
    }
    let orphans: i64 = c
        .query_row("SELECT count(*) FROM entry_blobs WHERE entry_hash NOT IN (SELECT hash FROM entries)", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(orphans, 0, "entry_blobs rows without an entry");
    out
}

/// Checks the whole stored chain: sequences 1..n, links, header hashes, one stream, one
/// payload reference per entry, and every referenced blob present in the blob store.
fn verify_chain(root: &Path) -> Vec<Row> {
    let out = rows(root);
    let blobs = BlobStore::open(&root.join("blobs")).unwrap();
    let mut prev = GENESIS_PREV.to_string();
    for (i, r) in out.iter().enumerate() {
        assert_eq!(r.seq, i as i64 + 1);
        assert_eq!(r.stream, STREAM);
        assert_eq!(r.prev.as_deref(), Some(prev.as_str()), "prev of seq {}", r.seq);
        let h = verify_header_value(&parse(&r.header).unwrap()).unwrap();
        assert_eq!(h.to_string(), r.hash);
        assert_eq!(r.refs.iter().filter(|(role, _)| role == ROLE_PAYLOAD).count(), 1, "seq {}", r.seq);
        for (_, d) in &r.refs {
            let d = fid_types::Digest::parse(d).unwrap();
            assert!(blobs.has_blob(&d).unwrap(), "referenced blob {d} is missing");
        }
        prev = r.hash.clone();
    }
    out
}

fn tail_hash(j: &Journal) -> Option<Digest> {
    j.tail().unwrap().map(|t| t.hash)
}

/// A W2 draft derived from fixture entry 0 with another branch, time and argument.
fn w2(branch: &str, utc: &str, n: u64, pad: usize) -> AppendRequest {
    static BASE: OnceLock<EntryDraft> = OnceLock::new();
    let base = BASE.get_or_init(|| EntryDraft::from_value(chain_fixture()[0].get("draft").unwrap()).unwrap());
    let mut draft = base.clone();
    draft.branch = branch.to_string();
    draft.time.utc = utc.to_string();
    if let DraftBody::Command(c) = &mut draft.body {
        let mut args = Obj::new();
        args.insert("n".into(), CValue::U64(n));
        args.insert("pad".into(), CValue::Str("x".repeat(pad)));
        c.args = CValue::Obj(args);
        c.outputs.clear();
    }
    AppendRequest::new(draft)
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_hundred_w2_entries_chain_from_one() {
    let (_t, root, j) = new_journal("ac1");
    let fx = chain_fixture();
    assert_eq!(fx.len(), 100);
    for e in &fx {
        append_fixture(&j, e);
    }
    let stored = verify_chain(&root);
    assert_eq!(stored.len(), 100);
    assert_eq!(stored[0].prev.as_deref(), Some(format!("sha256:{}", "0".repeat(64)).as_str()));
    for (r, e) in stored.iter().zip(&fx) {
        assert_eq!(r.seq.to_string(), st(e, "seq"));
        assert_eq!(r.prev.as_deref(), Some(st(e, "prev")));
        assert_eq!(r.hash, st(e, "hash"));
        let mut want = vec![(ROLE_PAYLOAD.to_string(), st(e, "payload_digest").to_string())];
        want.extend(arr(e, "blobs").iter().map(|b| (ROLE_REF.to_string(), st(obj(b), "digest").to_string())));
        want.sort();
        assert_eq!(r.refs, want, "{}", st(e, "name"));
    }
    let t = j.tail().unwrap().unwrap();
    assert_eq!((t.seq, t.hash.to_string()), (100, st(&fx[99], "hash").to_string()));
    // The chain survives a reopen and continues from its tail.
    drop(j);
    let j = reopen(&root);
    assert_eq!(j.tail().unwrap().unwrap().seq, 100);
}

#[test]
fn durability_pragmas_are_full() {
    let (_t, _root, j) = new_journal("pragmas");
    let d = j.durability().unwrap();
    assert_eq!(d.synchronous, SYNCHRONOUS_FULL);
    assert_eq!((d.fullfsync, d.checkpoint_fullfsync), (1, 1));
}

// ------------------------------------------------------------------ AC-2

fn crash_points() -> (usize, usize, Vec<(u32, String, bool)>) {
    let v = load("crash/points.json");
    let o = obj(&v);
    let num = |k: &str| match o.get(k) {
        Some(CValue::Num(n)) => *n as usize,
        other => panic!("{k} is {other:?}"),
    };
    let pts = arr(o, "points")
        .iter()
        .map(|p| {
            let p = obj(p);
            let n = match p.get("n") {
                Some(CValue::Num(n)) => *n as u32,
                other => panic!("n is {other:?}"),
            };
            let present = match st(p, "after_restart") {
                "present" => true,
                "absent" => false,
                other => panic!("after_restart {other}"),
            };
            (n, st(p, "name").to_string(), present)
        })
        .collect();
    (num("prechain"), num("target"), pts)
}

#[test]
fn crash_points_match_the_fixture() {
    let (_, _, pts) = crash_points();
    assert_eq!(pts.len(), 20);
    assert_eq!(CrashPoint::ALL.len(), 20);
    for ((n, name, present), p) in pts.iter().zip(CrashPoint::ALL) {
        assert_eq!((*n, name.as_str()), (p.number(), p.as_str()));
        assert_eq!(*present, !p.before_commit(), "{name}");
    }
}

/// A project holding the first `prechain` fixture entries, journal closed.
fn prechain_project(tag: &str, prechain: usize) -> (TempDir, PathBuf) {
    let (t, root, j) = new_journal(tag);
    for e in &chain_fixture()[..prechain] {
        append_fixture(&j, e);
    }
    drop(j);
    (t, root)
}

/// Child process of [`ac2_crash_harness`]: appends the target entry and exits the process
/// (no destructors, no SQLite close) at the injection point given in the environment.
#[test]
#[ignore = "run by ac2_crash_harness as a child process"]
fn ac2_crash_child() {
    let (Ok(root), Ok(point)) = (std::env::var("FID_JRN_CRASH_ROOT"), std::env::var("FID_JRN_CRASH_POINT")) else {
        return;
    };
    let n: u32 = point.parse().unwrap();
    let (_, target, _) = crash_points();
    let e = &chain_fixture()[target];
    let j = reopen(Path::new(&root));
    put_blobs(&j, e);
    j.set_fault_hook(Some(Arc::new(move |p: CrashPoint| {
        if p.number() == n {
            std::process::exit(CRASH_EXIT);
        }
        true
    })));
    let _ = j.append(request(e));
    // Only reached if the point was never passed.
    std::process::exit(0);
}

#[test]
fn ac2_crash_harness() {
    let (prechain, target, pts) = crash_points();
    let fx = chain_fixture();
    let exe = std::env::current_exe().unwrap();
    for (n, name, present) in pts {
        let (_t, root) = prechain_project(&format!("crash{n}"), prechain);
        let status = std::process::Command::new(&exe)
            .args(["ac2_crash_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env("FID_JRN_CRASH_ROOT", &root)
            .env("FID_JRN_CRASH_POINT", n.to_string())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(CRASH_EXIT), "point {n} ({name}) was not reached");
        let j = reopen(&root);
        let stored = verify_chain(&root);
        if present {
            assert_eq!(stored.len(), prechain + 1, "point {n} ({name}): entry must be present");
            assert_eq!(stored[target].hash, st(&fx[target], "hash"));
        } else {
            assert_eq!(stored.len(), prechain, "point {n} ({name}): entry must be absent");
            assert_eq!(tail_hash(&j).unwrap().to_string(), st(&fx[prechain - 1], "hash"));
            // The chain continues where it was: the same entry appends with the same hash.
            append_fixture(&j, &fx[target]);
            assert_eq!(verify_chain(&root).len(), prechain + 1);
        }
    }
}

#[test]
fn injected_failures_before_commit_leave_the_old_head() {
    let (_t, root, j) = new_journal("faults");
    let fx = chain_fixture();
    append_fixture(&j, &fx[0]);
    let mut next = 1;
    for p in CrashPoint::ALL.iter().copied().filter(|p| p.before_commit()) {
        let before = tail_hash(&j);
        put_blobs(&j, &fx[next]);
        j.set_fault_hook(Some(Arc::new(move |q: CrashPoint| q != p)));
        let err = j.append(request(&fx[next])).unwrap_err();
        assert_eq!(err.code(), "append_failed", "{}", p.as_str());
        assert!(err.to_string().contains(p.as_str()), "{err}");
        j.set_fault_hook(None);
        assert_eq!(tail_hash(&j), before, "{}", p.as_str());
        assert_eq!(verify_chain(&root).len(), next);
        // The journal stays usable, and the same entry appends with the reference hash.
        append_fixture(&j, &fx[next]);
        next += 1;
    }
    // A refusal by the hook after the COMMIT is ignored: the entry is durable and reported.
    j.set_fault_hook(Some(Arc::new(|q: CrashPoint| q.before_commit())));
    put_blobs(&j, &fx[next]);
    assert_eq!(j.append(request(&fx[next])).unwrap().to_string(), st(&fx[next], "hash"));
    assert_eq!(verify_chain(&root).len(), next + 1);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_raw_update_and_delete_are_refused() {
    let (_t, root, j) = new_journal("ac3");
    let fx = chain_fixture();
    for e in &fx[..3] {
        append_fixture(&j, e);
    }
    drop(j);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    let db = open_write(&loc, &lock).unwrap();
    let c = db.connection();
    for sql in [
        "UPDATE entries SET branch = 'main' WHERE seq = 2",
        "DELETE FROM entries WHERE seq = 3",
        "UPDATE entry_blobs SET role = 'ref'",
        "DELETE FROM entry_blobs",
        "INSERT OR REPLACE INTO entries SELECT * FROM entries WHERE seq = 1",
    ] {
        let err = c.execute(sql, []).expect_err(sql);
        assert!(is_append_only(&err), "{sql}: {err}");
    }
    drop(db);
    drop(lock);
    assert_eq!(verify_chain(&root).len(), 3);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_missing_blob_appends_nothing() {
    let (_t, root, j) = new_journal("ac4");
    let fx = chain_fixture();
    append_fixture(&j, &fx[0]);
    let before = tail_hash(&j);
    // Entry 1's blob is not put.
    let err = j.append(request(&fx[1])).unwrap_err();
    assert_eq!(err.code(), "missing_blob");
    assert_eq!(err, JournalError::MissingBlob(digest(st(obj(&arr(&fx[1], "blobs")[0]), "digest"))));
    assert_eq!(tail_hash(&j), before);
    assert_eq!(verify_chain(&root).len(), 1);
    // The tombstone digest is never a stored blob.
    let mut req = request(&fx[1]);
    put_blobs(&j, &fx[1]);
    req.blobs.push(Digest([0; 32]));
    assert_eq!(j.append(req).unwrap_err().code(), "missing_blob");
    assert_eq!(tail_hash(&j), before);
    append_fixture(&j, &fx[1]);
}

#[test]
fn missing_chunk_of_a_referenced_object_is_missing_blob() {
    let (_t, _root, j) = new_journal("chunk");
    // A canonical fid-chunks@1 manifest listing a chunk that is not stored.
    let absent = format!("sha256:{}", "ab".repeat(32));
    let manifest = format!(
        "{{\"chunk_size\":\"8388608\",\"chunks\":[\"{absent}\"],\"format\":\"fid-chunks@1\",\"size\":\"100\"}}"
    );
    let m = j.blobs().put_bytes(manifest.as_bytes(), PutOpts { durable: true }).unwrap();
    let mut req = w2("main", "2026-11-19T10:00:00Z", 1, 0);
    req.blobs.push(Digest(m.digest().to_bytes()));
    assert_eq!(j.append(req).unwrap_err(), JournalError::MissingBlob(digest(&absent)));
    assert_eq!(j.tail().unwrap(), None);
}

// ------------------------------------------------------------------ AC-5 and concurrency

fn concurrent_appends(tag: &str, writers: usize, per_writer: usize, pad: usize) -> (TempDir, PathBuf, f64) {
    let (t, root, j) = new_journal(tag);
    let j = Arc::new(j);
    let start = std::time::Instant::now();
    let handles: Vec<_> = (0..writers)
        .map(|w| {
            let j = Arc::clone(&j);
            std::thread::spawn(move || {
                // Equal instants: concurrent writers race for arrival order, and an equal time
                // is accepted by the time-order rule.
                (0..per_writer)
                    .map(|i| j.append(w2(&format!("writer/{w}"), "2026-11-19T10:00:00Z", i as u64, pad)).unwrap())
                    .collect::<Vec<Digest>>()
            })
        })
        .collect();
    let per: Vec<Vec<Digest>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let secs = start.elapsed().as_secs_f64();
    let stored = verify_chain(&root);
    assert_eq!(stored.len(), writers * per_writer);
    // Every writer's entries are stored, in the order that writer appended them.
    let seq_of: BTreeMap<String, i64> = stored.iter().map(|r| (r.hash.clone(), r.seq)).collect();
    for hashes in &per {
        let seqs: Vec<i64> = hashes.iter().map(|h| seq_of[&h.to_string()]).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    }
    drop(j);
    (t, root, (writers * per_writer) as f64 / secs)
}

#[test]
fn concurrent_writers_on_different_branches_form_one_chain() {
    concurrent_appends("conc", 4, 40, 64);
}

/// AC-5 `bench_append`: 10,000 W2 entries of about 2 KiB, 4 concurrent writers on different
/// branches. Run with `cargo test -p fid-journal --release -- --ignored bench_append
/// --nocapture`; the 1,000 entries/s budget is [unverified until measured] on reference
/// hardware, so CI does not assert it.
#[test]
#[ignore = "benchmark (AC-5); run on reference hardware"]
fn bench_append() {
    let (_t, _root, rate) = concurrent_appends("bench", 4, 2500, 2048);
    println!("bench_append: {rate:.0} entries/s (budget 1000, REQ-007)");
}

// ------------------------------------------------------------------ pinned rules

#[test]
fn time_order_fixture() {
    let v = load("time_order.json");
    let CValue::Arr(cases) = &v else { panic!("array expected") };
    assert!(cases.len() >= 15);
    for c in cases {
        let c = obj(c);
        let accept = matches!(c.get("accept"), Some(CValue::Bool(true)));
        let order = utc_order(st(c, "entry"), st(c, "head")).unwrap();
        assert_eq!(order != Ordering::Less, accept, "{}", st(c, "name"));
    }
    for bad in ["2026-11-19T10:00:00", "2026-11-19T10:00:00.Z", "2026-11-19T10:00:00.1234567890Z", "2026-11-19 10:00:00Z"]
    {
        assert_eq!(utc_order(bad, "2026-11-19T10:00:00Z"), None, "{bad}");
    }
}

#[test]
fn earlier_time_than_the_head_is_time_regressed() {
    let v = load("time_order.json");
    let CValue::Arr(cases) = &v else { panic!("array expected") };
    for c in cases {
        let c = obj(c);
        let name = st(c, "name");
        let (_t, root, j) = new_journal("time");
        let head = j.append(w2("main", st(c, "head"), 0, 0)).unwrap();
        let got = j.append(w2("feature/x", st(c, "entry"), 1, 0));
        if matches!(c.get("accept"), Some(CValue::Bool(true))) {
            assert!(got.is_ok(), "{name}: {got:?}");
            assert_eq!(verify_chain(&root).len(), 2);
        } else {
            let err = got.unwrap_err();
            assert_eq!(err.code(), "time_regressed", "{name}");
            assert_eq!(
                err,
                JournalError::TimeRegressed { head: st(c, "head").to_string(), entry: st(c, "entry").to_string() }
            );
            assert_eq!(tail_hash(&j), Some(head));
            assert_eq!(verify_chain(&root).len(), 1);
        }
    }
}

#[test]
fn expected_prev_mismatch_is_head_moved() {
    let (_t, root, j) = new_journal("moved");
    // On an empty chain the expected prev is the genesis digest.
    let mut first = w2("main", "2026-11-19T10:00:00Z", 0, 0);
    first.expected_prev = Some(GENESIS_PREV);
    let h1 = j.append(first).unwrap();
    // Prepared against the empty chain, but entry 1 landed first.
    let mut stale = w2("main", "2026-11-19T10:00:01Z", 1, 0);
    stale.expected_prev = Some(GENESIS_PREV);
    let err = j.append(stale).unwrap_err();
    assert_eq!(err.code(), "head_moved");
    assert_eq!(err, JournalError::HeadMoved { expected: GENESIS_PREV, found: h1 });
    assert_eq!(verify_chain(&root).len(), 1);
    let mut fresh = w2("main", "2026-11-19T10:00:01Z", 1, 0);
    fresh.expected_prev = Some(h1);
    j.append(fresh).unwrap();
    assert_eq!(verify_chain(&root).len(), 2);
}

#[test]
fn request_refusals_are_typed_and_append_nothing() {
    let (_t, root, j) = new_journal("refuse");
    // Another stream.
    let mut other = w2("main", "2026-11-19T10:00:00Z", 0, 0);
    other.draft.stream = "activity".into();
    assert_eq!(j.append(other).unwrap_err().code(), "schema");
    // A blob listed twice.
    let b = j.blobs().put_bytes(b"x", PutOpts { durable: false }).unwrap();
    let d = Digest(b.digest().to_bytes());
    let mut twice = w2("main", "2026-11-19T10:00:00Z", 0, 0);
    twice.blobs = vec![d, d];
    assert_eq!(j.append(twice).unwrap_err().code(), "schema");
    // Too many blobs.
    let mut many = w2("main", "2026-11-19T10:00:00Z", 0, 0);
    many.blobs = (0..=MAX_BLOBS).map(|i| Digest(fid_canon::digest_bytes(&i.to_le_bytes()).0)).collect();
    assert_eq!(j.append(many).unwrap_err().code(), "input_limit");
    // A malformed time is a schema error once there is a head to compare with.
    j.append(w2("main", "2026-11-19T10:00:00Z", 0, 0)).unwrap();
    assert_eq!(j.append(w2("main", "2026-11-19 10:00:01Z", 1, 0)).unwrap_err().code(), "schema");
    // Missing recorded salt.
    let mut no_salt = w2("main", "2026-11-19T10:00:01Z", 1, 0);
    no_salt.salts = Some(RecordedSalts::default());
    assert_eq!(j.append(no_salt).unwrap_err().code(), "schema");
    assert_eq!(verify_chain(&root).len(), 1);
    // A referenced blob put without durability is flushed and accepted.
    let mut with_blob = w2("main", "2026-11-19T10:00:01Z", 1, 0);
    with_blob.blobs = vec![d];
    j.append(with_blob).unwrap();
    assert_eq!(verify_chain(&root).len(), 2);
}

#[test]
fn open_refuses_another_stream_and_bad_names() {
    let (_t, root, j) = new_journal("streams");
    j.append(w2("main", "2026-11-19T10:00:00Z", 0, 0)).unwrap();
    drop(j);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let err = Journal::open(loc, lock.unwrap(), "ledger").unwrap_err();
    assert_eq!(err.code(), "chain_invalid");
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    assert_eq!(Journal::open(loc, lock.unwrap(), "Record").unwrap_err().code(), "schema");
}

#[test]
fn error_codes_are_stable() {
    let d = GENESIS_PREV;
    let all = [
        (JournalError::AppendFailed(String::new()), "append_failed"),
        (JournalError::MissingBlob(d), "missing_blob"),
        (JournalError::Schema(String::new()), "schema"),
        (JournalError::HeadMoved { expected: d, found: d }, "head_moved"),
        (JournalError::TimeRegressed { head: String::new(), entry: String::new() }, "time_regressed"),
        (JournalError::ChainInvalid(String::new()), "chain_invalid"),
        (JournalError::InputLimit(String::new()), "input_limit"),
    ];
    for (e, code) in all {
        assert_eq!(e.code(), code);
        assert!(e.to_string().starts_with(code));
    }
    assert!(MAX_BATCH <= MAX_QUEUE);
}

// ------------------------------------------------------------------ the bus seam

/// The shape of `fid-bus`'s `JournalAppend` (CORE.BUS.PRIN): `&mut self`, a borrowed draft,
/// no hash returned. An adapter over [`Append`] is a few lines; it lives in `fid-bus`.
trait BusJournalAppend {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), String>;
}

struct BusAdapter<A: Append>(A);

impl<A: Append> BusJournalAppend for BusAdapter<A> {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), String> {
        self.0.append(AppendRequest::new(draft.clone())).map(|_| ()).map_err(|e| e.code().to_string())
    }
}

#[test]
fn append_trait_serves_the_bus_shape() {
    let (_t, root, j) = new_journal("seam");
    let j: Arc<dyn Append> = Arc::new(j);
    let mut bus = BusAdapter(Arc::clone(&j));
    let req = w2("main", "2026-11-19T10:00:05Z", 0, 0);
    bus.append(&req.draft).unwrap();
    // An upgrade stamped at confirm time, committed after a later entry: refused here.
    let late = w2("main", "2026-11-19T10:00:04Z", 1, 0);
    assert_eq!(bus.append(&late.draft).unwrap_err(), "time_regressed");
    assert_eq!(verify_chain(&root).len(), 1);
}
