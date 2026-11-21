//! Appending on a borrowed store (`CORE.JRN.CHAIN.APPEND` REQ-008, `Journal::on_store`):
//! appends inside `Store::copy_to`'s `finish` give a chain that verifies; a read-only store,
//! a second claim and a stale head are refused; and the rows are byte-identical to the owned
//! path for the same entries.
//!
//! Expected hashes and payload digests come from the independent Python reference
//! `fixtures/core/jrn/chain/append/make_fixtures.py` (`chain.json`), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue, Digest};
use fid_journal::checkpoint::Reason;
use fid_journal::{
    parse_digest, AppendRequest, Clock, Coverage, EntryDraft, Field, Journal, JournalError, RecordedSalts, Salt,
    VerifyOptions, GENESIS_PREV,
};
use fid_store::cas::PutOpts;
use fid_store::{CopyTarget, FolderId, OpenMode, ProjectLocation, Store, StoreError, TxnError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/chain/append");
const STREAM: &str = "record";
/// 2030-01-01T00:00:00Z: after every fixture time.
const TEST_NOW: u64 = 1_893_456_000;

// ------------------------------------------------------------------ scaffolding

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-jrnb-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

struct FixedClock(u64);

impl Clock for FixedClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0)
    }
}

fn clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock(TEST_NOW))
}

struct Target {
    parent: PathBuf,
    id: FolderId,
    name: String,
}

impl Target {
    fn new(parent: &Path, name: &str) -> Target {
        Target { parent: parent.to_path_buf(), id: FolderId::of_path(parent).unwrap(), name: name.into() }
    }
}

impl CopyTarget for Target {
    fn parent_path(&self) -> &Path {
        &self.parent
    }
    fn parent_id(&self) -> FolderId {
        self.id
    }
    fn target_name(&self) -> &str {
        &self.name
    }
}

fn plain_publish(rename: &dyn Fn() -> Result<(), StoreError>) -> Result<(), TxnError> {
    rename().map_err(Into::into)
}

/// A journal error carried out of `copy_to`'s `finish`.
fn to_txn(e: JournalError) -> TxnError {
    TxnError::Io(std::io::Error::other(e.to_string()))
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

/// The entries of `chain.json` (Python reference).
fn chain() -> Vec<Obj> {
    let v = parse(&std::fs::read(Path::new(FIXTURES).join("chain.json")).unwrap()).unwrap();
    let o = obj(&v);
    assert_eq!(st(o, "stream"), STREAM);
    arr(o, "entries").iter().map(|e| obj(e).clone()).collect()
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

/// The fixture entry as a request with its recorded salts and referenced blobs, guarded by
/// the fixture's `prev`.
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
    AppendRequest { draft, blobs, expected_prev: Some(digest(st(e, "prev"))), salts: Some(salts) }
}

fn put_blobs(blobs: &fid_store::BlobStore, e: &Obj) {
    for b in arr(e, "blobs") {
        let b = obj(b);
        let addr = blobs.put_bytes(st(b, "content").as_bytes(), PutOpts { durable: true }).unwrap();
        assert_eq!(addr.digest().as_str(), st(b, "digest"));
    }
}

/// `(seq, hash, prev_hash, stream, branch, kind, header)` of every entry.
type EntryRow = (i64, String, Option<String>, String, String, String, Vec<u8>);

/// Every stored row of the journal tables and the bytes of every referenced blob, in a
/// fixed order: what "byte-identical" compares.
#[derive(Debug, PartialEq, Eq)]
struct Dump {
    entries: Vec<EntryRow>,
    refs: Vec<(String, String, String)>,
    blobs: Vec<(String, Vec<u8>)>,
}

fn dump(store: &Store) -> Dump {
    let entries: Vec<EntryRow> = store
        .read(|c| {
            let mut s =
                c.prepare("SELECT seq, hash, prev_hash, stream, branch, kind, header FROM entries ORDER BY seq")?;
            let rows =
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?;
            rows.collect()
        })
        .unwrap();
    let refs: Vec<(String, String, String)> = store
        .read(|c| {
            let mut s =
                c.prepare("SELECT entry_hash, role, digest FROM entry_blobs ORDER BY entry_hash, role, digest")?;
            let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect()
        })
        .unwrap();
    let mut blobs: Vec<(String, Vec<u8>)> = refs
        .iter()
        .map(|(_, _, d)| {
            let d = fid_types::Digest::parse(d).unwrap();
            (d.as_str().to_string(), std::fs::read(store.blobs().path_of(&d)).unwrap())
        })
        .collect();
    blobs.sort();
    blobs.dedup();
    Dump { entries, refs, blobs }
}

/// Appends fixture entries `range` through an owned journal on a new project at `root`.
fn owned_project(root: &Path, entries: &[Obj]) {
    let (loc, lock) = ProjectLocation::create(root).unwrap();
    let j = Journal::open_with_clock(loc, lock, STREAM, clock()).unwrap();
    for e in entries {
        put_blobs(j.blobs(), e);
        assert_eq!(j.append(request(e)).unwrap(), digest(st(e, "hash")), "{}", st(e, "name"));
    }
}

// ------------------------------------------------------------------ copy_to's finish

#[test]
fn appends_in_copy_finish_give_a_chain_that_verifies() {
    let c = chain();
    let t = TempDir::new("copy");
    let src_root = t.0.join("src");
    {
        // The source: five entries and a close checkpoint, through the owned path.
        let (loc, lock) = ProjectLocation::create(&src_root).unwrap();
        let j = Journal::open_with_clock(loc, lock, STREAM, clock()).unwrap();
        for e in &c[..5] {
            put_blobs(j.blobs(), e);
            j.append(request(e)).unwrap();
        }
        j.checkpoint(Reason::Close).unwrap();
    }
    let out = t.0.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let src = Store::open(&src_root, OpenMode::Write).unwrap();
    let copy = src
        .copy_to(
            &Target::new(&out, "p_copy"),
            |copy| {
                let jr = Journal::on_store_with_clock(copy, STREAM, clock()).map_err(to_txn)?;
                // The copy starts at the source's head (the backup snapshot).
                assert_eq!(jr.tail().unwrap().unwrap().hash, digest(st(&c[4], "hash")));
                for e in &c[5..8] {
                    put_blobs(jr.blobs(), e);
                    let h = jr.append(request(e)).map_err(to_txn)?;
                    assert_eq!(h, digest(st(e, "hash")), "{}", st(e, "name"));
                }
                let report = jr.verify(&VerifyOptions::default()).unwrap();
                assert!(report.ok(), "{report:?}");
                assert_eq!(report.checked.entries, 8);
                let branch = c[7].get("draft").map(|d| st(obj(d), "branch").to_string()).unwrap();
                assert_eq!(jr.head(&branch).unwrap().unwrap().hash, digest(st(&c[7], "hash")));
                Ok(())
            },
            plain_publish,
        )
        .unwrap();
    // The returned copy verifies through the plain store API too.
    let report = fid_journal::verify_store(&copy, STREAM, &VerifyOptions::default()).unwrap();
    assert!(report.ok(), "{report:?}");
    assert_eq!(report.checked.entries, 8);
    drop(copy);

    // Reopened with the owned path: same chain, and the source's checkpoint still verifies
    // as a prefix of the extended chain.
    let (loc, lock) = ProjectLocation::open(&out.join("p_copy"), OpenMode::Write).unwrap();
    let j = Journal::open_with_clock(loc, lock.unwrap(), STREAM, clock()).unwrap();
    let tail = j.tail().unwrap().unwrap();
    assert_eq!((tail.seq, tail.hash), (8, digest(st(&c[7], "hash"))));
    assert!(j.verify_chain(&VerifyOptions::default()).unwrap().ok());
    let cp = j.last_checkpoint().unwrap().unwrap().checkpoint;
    assert_eq!(cp.seq, 5);
    j.verify_checkpoint(&cp, Coverage::Prefix).unwrap();
    // The source is untouched.
    let src_tail = fid_journal::verify_store(&src, STREAM, &VerifyOptions::default()).unwrap();
    assert!(src_tail.ok());
    assert_eq!(src_tail.checked.entries, 5);
}

#[test]
fn a_failed_append_in_finish_publishes_nothing() {
    let c = chain();
    let t = TempDir::new("fail");
    let src_root = t.0.join("src");
    owned_project(&src_root, &c[..2]);
    let out = t.0.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let src = Store::open(&src_root, OpenMode::Write).unwrap();
    let e = src
        .copy_to(
            &Target::new(&out, "p_copy"),
            |copy| {
                let jr = Journal::on_store_with_clock(copy, STREAM, clock()).map_err(to_txn)?;
                // Entry 4 against a tail at 2: its expected prev is stale.
                put_blobs(jr.blobs(), &c[3]);
                jr.append(request(&c[3])).map_err(to_txn)?;
                Ok(())
            },
            plain_publish,
        )
        .unwrap_err();
    assert!(e.to_string().contains("head_moved"), "{e}");
    assert!(std::fs::read_dir(&out).unwrap().next().is_none(), "nothing may be left at the target");
}

// ------------------------------------------------------------------ refusals

#[test]
fn a_read_only_store_is_refused() {
    let c = chain();
    let t = TempDir::new("ro");
    let root = t.0.join("p");
    owned_project(&root, &c[..2]);
    let ro = Store::open(&root, OpenMode::Read).unwrap();
    let e = Journal::on_store_with_clock(&ro, STREAM, clock()).unwrap_err();
    assert_eq!(e.code(), "read_only", "{e}");
    assert!(matches!(e, JournalError::ReadOnly(_)));
}

#[test]
fn a_second_claim_on_the_same_store_is_refused_until_the_first_is_dropped() {
    let t = TempDir::new("claim");
    let store = Store::create(&t.0.join("p")).unwrap();
    let first = Journal::on_store_with_clock(&store, STREAM, clock()).unwrap();
    let e = Journal::on_store_with_clock(&store, STREAM, clock()).unwrap_err();
    assert_eq!(e.code(), "append_failed", "{e}");
    assert!(e.to_string().contains("claimed"), "{e}");
    drop(first);
    Journal::on_store_with_clock(&store, STREAM, clock()).unwrap();
}

#[test]
fn a_store_of_another_stream_or_a_bad_name_is_refused() {
    let c = chain();
    let t = TempDir::new("stream");
    let root = t.0.join("p");
    owned_project(&root, &c[..1]);
    let store = Store::open(&root, OpenMode::Write).unwrap();
    assert_eq!(Journal::on_store(&store, "ledger").unwrap_err().code(), "chain_invalid");
    assert_eq!(Journal::on_store(&store, "Bad-Name").unwrap_err().code(), "schema");
    // The refused opens released the claim.
    Journal::on_store_with_clock(&store, STREAM, clock()).unwrap();
}

#[test]
fn a_stale_head_is_refused_and_appends_nothing() {
    let c = chain();
    let t = TempDir::new("stale");
    let store = Store::create(&t.0.join("p")).unwrap();
    let jr = Journal::on_store_with_clock(&store, STREAM, clock()).unwrap();
    put_blobs(jr.blobs(), &c[0]);
    put_blobs(jr.blobs(), &c[1]);
    let h0 = jr.append(request(&c[0])).unwrap();
    assert_eq!(h0, digest(st(&c[0], "hash")));
    // Prepared against the empty chain, but entry 1 landed first.
    let mut stale = request(&c[1]);
    stale.expected_prev = Some(GENESIS_PREV);
    match jr.append(stale).unwrap_err() {
        JournalError::HeadMoved { expected, found } => {
            assert_eq!(expected, GENESIS_PREV);
            assert_eq!(found, h0);
        }
        other => panic!("head_moved expected, got {other}"),
    }
    let tail = jr.tail().unwrap().unwrap();
    assert_eq!((tail.seq, tail.hash), (1, h0));
    assert!(!jr.is_poisoned());
    // Prepared against the real head: accepted.
    assert_eq!(jr.append(request(&c[1])).unwrap(), digest(st(&c[1], "hash")));
}

// ------------------------------------------------------------------ same bytes as the owned path

#[test]
fn rows_are_byte_identical_to_the_owned_path() {
    let c = chain();
    let n = 12;
    let t = TempDir::new("bytes");
    let owned_root = t.0.join("owned");
    owned_project(&owned_root, &c[..n]);
    let owned = Store::open(&owned_root, OpenMode::Read).unwrap();

    let lent = Store::create(&t.0.join("lent")).unwrap();
    {
        let jr = Journal::on_store_with_clock(&lent, STREAM, clock()).unwrap();
        for e in &c[..n] {
            put_blobs(jr.blobs(), e);
            assert_eq!(jr.append(request(e)).unwrap(), digest(st(e, "hash")), "{}", st(e, "name"));
        }
    }
    let (a, b) = (dump(&owned), dump(&lent));
    assert_eq!(a.entries.len(), n);
    assert_eq!(a, b);
    // And both match the Python reference: hashes, links and payload digests.
    for (row, e) in b.entries.iter().zip(&c[..n]) {
        assert_eq!(row.0.to_string(), st(e, "seq"));
        assert_eq!(row.1, st(e, "hash"));
        assert_eq!(row.2.as_deref(), Some(st(e, "prev")));
        let payload: Vec<&str> =
            b.refs.iter().filter(|(h, role, _)| h == &row.1 && role == "payload").map(|(_, _, d)| d.as_str()).collect();
        assert_eq!(payload, [st(e, "payload_digest")], "{}", st(e, "name"));
    }
}
