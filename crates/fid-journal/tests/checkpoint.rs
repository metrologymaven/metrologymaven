//! Acceptance criteria of spec node CORE.JRN.CHAIN.CKPT (AC-1 .. AC-4) and the pinned rules
//! of module `checkpoint`: canonical encoding, decoding, the `format` gate, the time format,
//! the head cache and verification of a checkpoint against a journal.
//!
//! Expected bytes, digests, heads and verification outcomes come from the independent Python
//! reference `fixtures/core/jrn/chain/ckpt/make_fixtures.py`; the 50-entry test project is
//! the first 50 entries of `fixtures/core/jrn/chain/append/chain.json` (its own independent
//! reference). The head-cache tests change the `heads` table with raw SQL through
//! `fid-store`'s connection, bypassing the journal API.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, CValue, Digest};
use fid_journal::checkpoint::{
    format_time, is_time, verify_links, Format, Reason, FORMAT_LINEAR_V1, MAX_CHECKPOINT_BYTES, MAX_STREAM_BYTES,
    MAX_TIME_UNIX,
};
use fid_journal::{
    parse_digest, AppendRequest, Checkpoint, CheckpointError, Clock, Coverage, EntryDraft, Field, Journal,
    JournalError, LedgerHead, Link, RecordedSalts, Salt, DIGEST_PATTERN, GENESIS_PREV, SEQ_PATTERN,
};
use fid_store::cas::PutOpts;
use fid_store::schema::open_write;
use fid_store::{OpenMode, ProjectLocation};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const STREAM: &str = "record";

// ------------------------------------------------------------------ scaffolding

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-ckpt-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

/// A host clock fixed at a Unix instant.
struct FixedClock(Duration);

impl Clock for FixedClock {
    fn now(&self) -> Duration {
        self.0
    }
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

/// A decimal-string field.
fn num(o: &Obj, k: &str) -> u64 {
    st(o, k).parse().unwrap()
}

/// A JSON-number field.
fn int(o: &Obj, k: &str) -> usize {
    match o.get(k) {
        Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 0.0 => *n as usize,
        other => panic!("field {k} is {other:?}"),
    }
}

fn load(rel: &str) -> CValue {
    parse(&std::fs::read(Path::new(ROOT).join(rel)).unwrap()).unwrap()
}

fn expected() -> Obj {
    obj(&load("fixtures/core/jrn/chain/ckpt/expected.json")).clone()
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

/// The entries of the APPEND fixture chain.
fn chain_fixture() -> Vec<Obj> {
    let v = load("fixtures/core/jrn/chain/append/chain.json");
    let o = obj(&v);
    assert_eq!(st(o, "stream"), STREAM);
    arr(o, "entries").iter().map(|e| obj(e).clone()).collect()
}

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

fn open_journal(root: &Path, now: u64) -> Journal {
    let (loc, lock) = ProjectLocation::open(root, OpenMode::Write).unwrap();
    Journal::open_with_clock(loc, lock.unwrap(), STREAM, Arc::new(FixedClock(Duration::from_secs(now)))).unwrap()
}

/// A new project holding the first `n` fixture entries, with the clock at `now`.
fn project(tag: &str, n: usize, now: u64) -> (TempDir, PathBuf, Journal) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let j = Journal::open_with_clock(loc, lock, STREAM, Arc::new(FixedClock(Duration::from_secs(now)))).unwrap();
    for e in chain_fixture().iter().take(n) {
        for b in arr(e, "blobs") {
            j.blobs().put_bytes(st(obj(b), "content").as_bytes(), PutOpts { durable: true }).unwrap();
        }
        let h = j.append(request(e)).unwrap();
        assert_eq!(h.to_string(), st(e, "hash"));
    }
    (t, root, j)
}

/// Runs raw SQL on a closed project.
fn raw(root: &Path, sql: &str) {
    let (loc, lock) = ProjectLocation::open(root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    let db = open_write(&loc, &lock).unwrap();
    db.connection().execute_batch(sql).unwrap();
}

/// The `heads` table of a closed project.
fn cache(root: &Path) -> BTreeMap<String, String> {
    let (loc, lock) = ProjectLocation::open(root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    let db = open_write(&loc, &lock).unwrap();
    let c = db.connection();
    let mut s = c.prepare("SELECT branch, entry_hash FROM heads ORDER BY branch").unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
}

/// The `checkpoints` rows of a closed project: (stream, entry_hash, body).
fn checkpoint_rows(root: &Path) -> Vec<(String, String, Vec<u8>)> {
    let (loc, lock) = ProjectLocation::open(root, OpenMode::Write).unwrap();
    let lock = lock.unwrap();
    let db = open_write(&loc, &lock).unwrap();
    let c = db.connection();
    let mut s = c.prepare("SELECT stream, entry_hash, body FROM checkpoints ORDER BY id").unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_close_checkpoint_of_fifty_entries() {
    let x = expected();
    let ac1 = obj(x.get("ac1").unwrap());
    let now = num(ac1, "clock_unix");
    let n = int(ac1, "entries");
    let (_t, root, j) = project("ac1", n, now);
    let cp = j.checkpoint(Reason::Close).unwrap();
    let fx = chain_fixture();
    assert_eq!(cp.format, Format::LinearV1);
    assert_eq!(cp.stream, STREAM);
    assert_eq!(cp.seq, 50);
    assert_eq!(cp.count, 50);
    assert_eq!(cp.head.to_string(), st(&fx[49], "hash"));
    assert_eq!(cp.ledger, None);
    assert_eq!(cp.reason, Reason::Close);
    let bytes = cp.canonical_bytes().unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), st(ac1, "canonical"));
    assert_eq!(cp.digest().unwrap().to_string(), st(ac1, "digest"));
    assert_eq!(cp.to_value(), *ac1.get("value").unwrap());
    // Stored, and read back as it was made.
    let stored = j.last_checkpoint().unwrap().unwrap();
    assert_eq!(stored.checkpoint, cp);
    assert_eq!(stored.digest.to_string(), st(ac1, "digest"));
    // It verifies against the journal it was made from.
    assert_eq!(j.verify_checkpoint(&cp, Coverage::Exact).unwrap().checked, 50);
    assert_eq!(j.verify_checkpoint(&cp, Coverage::Prefix).unwrap().checked, 50);
    drop(j);
    let rows = checkpoint_rows(&root);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, STREAM);
    assert_eq!(rows[0].1, st(&fx[49], "hash"));
    assert_eq!(rows[0].2, bytes);
}

#[test]
fn checkpoint_after_more_entries_is_extended_exactly_but_a_valid_prefix() {
    let x = expected();
    let ac1 = obj(x.get("ac1").unwrap());
    let now = num(ac1, "clock_unix");
    let (_t, root, j) = project("ext", 50, now);
    let cp = j.checkpoint(Reason::Export).unwrap();
    drop(j);
    let j = open_journal(&root, now);
    let e = &chain_fixture()[50];
    for b in arr(e, "blobs") {
        j.blobs().put_bytes(st(obj(b), "content").as_bytes(), PutOpts { durable: true }).unwrap();
    }
    j.append(request(e)).unwrap();
    assert_eq!(j.verify_checkpoint(&cp, Coverage::Prefix).unwrap().checked, 50);
    assert_eq!(
        j.verify_checkpoint(&cp, Coverage::Exact),
        Err(CheckpointError::Extended { checkpoint_seq: 50, journal_count: 51 })
    );
    // A checkpoint naming another head at sequence 50 is a fork.
    let mut forked = cp.clone();
    forked.head = digest(st(&chain_fixture()[48], "hash"));
    assert!(matches!(j.verify_checkpoint(&forked, Coverage::Prefix), Err(CheckpointError::ForkedHead { seq: 50, .. })));
    // One past the end is truncated.
    let mut ahead = cp.clone();
    ahead.seq = 52;
    ahead.count = 52;
    assert_eq!(
        j.verify_checkpoint(&ahead, Coverage::Prefix),
        Err(CheckpointError::Truncated { checkpoint_seq: 52, journal_count: 51 })
    );
    // A second checkpoint is stored after the first.
    let cp2 = j.checkpoint(Reason::Signature).unwrap();
    assert_eq!(cp2.seq, 51);
    assert_eq!(j.last_checkpoint().unwrap().unwrap().checkpoint, cp2);
}

// ------------------------------------------------------------------ AC-2

fn expected_heads() -> BTreeMap<String, (u64, String)> {
    let x = expected();
    let ac1 = obj(x.get("ac1").unwrap());
    obj(ac1.get("heads").unwrap())
        .iter()
        .map(|(b, h)| (b.clone(), (num(obj(h), "seq"), st(obj(h), "hash").to_string())))
        .collect()
}

#[test]
fn ac2_missing_cache_is_rebuilt() {
    let want = expected_heads();
    let (_t, root, j) = project("ac2", 50, 1_893_456_000);
    // A fresh project has no cache rows: head(main) rebuilds them.
    let h = j.head("main").unwrap().unwrap();
    assert_eq!((h.seq, h.hash.to_string()), want["main"].clone());
    drop(j);
    let rebuilt = cache(&root);
    let want_cache: BTreeMap<String, String> = want.iter().map(|(b, (_, h))| (b.clone(), h.clone())).collect();
    assert_eq!(rebuilt, want_cache);
    // The cache deleted: rebuilt again before answering.
    raw(&root, "DELETE FROM heads");
    assert!(cache(&root).is_empty());
    let j = open_journal(&root, 1_893_456_000);
    let h = j.head("main").unwrap().unwrap();
    assert_eq!(h.branch, "main");
    assert_eq!((h.seq, h.hash.to_string()), want["main"].clone());
    drop(j);
    assert_eq!(cache(&root), rebuilt);
}

#[test]
fn stale_cache_row_loses_to_the_journal() {
    let want = expected_heads();
    let (_t, root, j) = project("stale", 50, 1_893_456_000);
    j.rebuild_heads().unwrap();
    drop(j);
    // Point main's row at another (real) entry.
    let other = &want["agent/claude-code/3"].1;
    raw(&root, &format!("UPDATE heads SET entry_hash = '{other}' WHERE branch = 'main'"));
    let j = open_journal(&root, 1_893_456_000);
    let h = j.head("main").unwrap().unwrap();
    assert_eq!(h.hash.to_string(), want["main"].1);
    drop(j);
    assert_eq!(cache(&root)["main"], want["main"].1);
    // heads(): every branch, journal wins over a stale row too.
    raw(&root, &format!("UPDATE heads SET entry_hash = '{other}' WHERE branch = 'feature/bore-check'"));
    let j = open_journal(&root, 1_893_456_000);
    let all = j.heads().unwrap();
    let got: BTreeMap<String, (u64, String)> =
        all.into_iter().map(|h| (h.branch, (h.seq, h.hash.to_string()))).collect();
    assert_eq!(got, want);
    drop(j);
    assert_eq!(cache(&root)["feature/bore-check"], want["feature/bore-check"].1);
}

#[test]
fn unknown_branch_has_no_head() {
    let (_t, root, j) = project("nobranch", 5, 1_893_456_000);
    assert_eq!(j.head("does-not-exist").unwrap(), None);
    drop(j);
    // A cache row for a branch that has no entry is never answered.
    let any = st(&chain_fixture()[0], "hash").to_string();
    raw(&root, &format!("INSERT INTO heads (branch, entry_hash) VALUES ('ghost', '{any}')"));
    let j = open_journal(&root, 1_893_456_000);
    assert_eq!(j.head("ghost").unwrap(), None);
    assert!(j.heads().unwrap().iter().all(|h| h.branch != "ghost"));
}

#[test]
fn heads_follow_appends() {
    let (_t, root, j) = project("follow", 3, 1_893_456_000);
    let fx = chain_fixture();
    assert_eq!(j.head("main").unwrap().unwrap().hash.to_string(), st(&fx[0], "hash"));
    drop(j);
    let j = open_journal(&root, 1_893_456_000);
    for e in fx.iter().skip(3).take(1) {
        for b in arr(e, "blobs") {
            j.blobs().put_bytes(st(obj(b), "content").as_bytes(), PutOpts { durable: true }).unwrap();
        }
        j.append(request(e)).unwrap();
    }
    // Entry 4 (index 3) is on main: the cached row is now stale and the journal wins.
    assert_eq!(j.head("main").unwrap().unwrap().hash.to_string(), st(&fx[3], "hash"));
    assert_eq!(j.head("main").unwrap().unwrap().seq, 4);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_empty_stream() {
    let (_t, root, j) = project("empty", 0, 1_893_456_000);
    let e = j.checkpoint(Reason::Close).unwrap_err();
    assert_eq!(e, JournalError::EmptyStream(STREAM.into()));
    assert_eq!(e.code(), "empty_stream");
    assert!(e.to_string().starts_with("empty_stream"));
    assert_eq!(j.last_checkpoint().unwrap(), None);
    assert!(j.heads().unwrap().is_empty());
    drop(j);
    assert!(checkpoint_rows(&root).is_empty());
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_ledger_head_is_included() {
    let x = expected();
    let ac4 = obj(x.get("ac4").unwrap());
    let now = num(ac4, "clock_unix");
    let value = obj(ac4.get("value").unwrap());
    let led = obj(value.get("ledger").unwrap());
    let ledger = LedgerHead { seq: num(led, "seq"), head: digest(st(led, "head")) };
    let (_t, _root, j) = project("ac4", 50, now);
    let cp = j.checkpoint_with_ledger(Reason::Signature, Some(ledger)).unwrap();
    assert_eq!(cp.ledger, Some(ledger));
    assert_eq!(cp.head.to_string(), st(&chain_fixture()[49], "hash"));
    assert_eq!(String::from_utf8(cp.canonical_bytes().unwrap()).unwrap(), st(ac4, "canonical"));
    assert_eq!(cp.digest().unwrap().to_string(), st(ac4, "digest"));
    assert_eq!(j.last_checkpoint().unwrap().unwrap().checkpoint, cp);
    // A ledger sequence of 0 is refused and nothing is stored.
    let bad = j.checkpoint_with_ledger(Reason::Signature, Some(LedgerHead { seq: 0, head: GENESIS_PREV }));
    assert_eq!(bad.unwrap_err().code(), "schema");
    assert_eq!(j.last_checkpoint().unwrap().unwrap().checkpoint, cp);
}

// ------------------------------------------------------------------ encoding and decoding

#[test]
fn encode_vectors_match_the_reference() {
    let x = expected();
    let mut all: Vec<Obj> = vec![obj(x.get("ac1").unwrap()).clone(), obj(x.get("ac4").unwrap()).clone()];
    all.extend(arr(&x, "encode").iter().map(|v| obj(v).clone()));
    for v in &all {
        let canonical = st(v, "canonical");
        let cp = Checkpoint::from_value(v.get("value").unwrap()).unwrap();
        assert_eq!(cp.to_value(), *v.get("value").unwrap());
        assert_eq!(String::from_utf8(cp.canonical_bytes().unwrap()).unwrap(), canonical);
        assert_eq!(cp.digest().unwrap().to_string(), st(v, "digest"));
        assert_eq!(digest_bytes(canonical.as_bytes()).to_string(), st(v, "digest"));
        assert_eq!(Checkpoint::from_bytes(canonical.as_bytes()).unwrap(), cp);
    }
    assert_eq!(st(&x, "format"), FORMAT_LINEAR_V1);
    assert_eq!(int(&x, "max_bytes"), MAX_CHECKPOINT_BYTES);
}

#[test]
fn time_vectors_match_the_reference() {
    for v in arr(&expected(), "time") {
        let v = obj(v);
        let secs = num(v, "unix_secs");
        let got = format_time(secs).unwrap_or_else(|| "out_of_range".into());
        assert_eq!(got, st(v, "time"), "{}", st(v, "name"));
        if got != "out_of_range" {
            assert!(is_time(&got), "{got}");
        }
    }
    assert_eq!(format_time(MAX_TIME_UNIX).unwrap(), "9999-12-31T23:59:59Z");
    assert_eq!(format_time(u64::MAX), None);
}

#[test]
fn checkpoint_time_truncates_the_clock() {
    let t = TempDir::new("trunc");
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    // 2030-01-01T00:00:00.999999999Z
    let clock = Arc::new(FixedClock(Duration::new(1_893_456_000, 999_999_999)));
    let j = Journal::open_with_clock(loc, lock, STREAM, clock).unwrap();
    let e = &chain_fixture()[0];
    for b in arr(e, "blobs") {
        j.blobs().put_bytes(st(obj(b), "content").as_bytes(), PutOpts { durable: true }).unwrap();
    }
    j.append(request(e)).unwrap();
    assert_eq!(j.checkpoint(Reason::Close).unwrap().time, "2030-01-01T00:00:00Z");
}

#[test]
fn decode_cases_match_the_reference() {
    for c in arr(&expected(), "decode") {
        let c = obj(c);
        let name = st(c, "name");
        let got = Checkpoint::from_bytes(st(c, "bytes").as_bytes());
        match c.get("ok") {
            Some(CValue::Bool(true)) => {
                assert!(got.is_ok(), "{name}: {got:?}");
            }
            Some(CValue::Bool(false)) => {
                let e = got.expect_err(name);
                assert_eq!(e.code(), st(c, "code"), "{name}: {e}");
            }
            other => panic!("{name}: ok is {other:?}"),
        }
    }
}

#[test]
fn unknown_format_is_refused_first() {
    let mut o = Obj::new();
    o.insert("format".into(), CValue::Str("merkle-v1".into()));
    o.insert("tree_size".into(), CValue::U64(50));
    let e = Checkpoint::from_value(&CValue::Obj(o)).unwrap_err();
    assert_eq!(e, CheckpointError::UnknownFormat("merkle-v1".into()));
    // The echoed value is bounded.
    let mut o = Obj::new();
    o.insert("format".into(), CValue::Str("x".repeat(10_000)));
    let CheckpointError::UnknownFormat(s) = Checkpoint::from_value(&CValue::Obj(o)).unwrap_err() else {
        panic!("unknown_format expected")
    };
    assert_eq!(s.len(), 64);
}

#[test]
fn field_rules_are_enforced_on_encode() {
    let base = Checkpoint {
        format: Format::LinearV1,
        stream: STREAM.into(),
        seq: 3,
        head: GENESIS_PREV,
        count: 3,
        ledger: None,
        reason: Reason::Export,
        time: "2026-11-19T12:00:00Z".into(),
    };
    assert!(base.canonical_bytes().is_ok());
    let bad = [
        Checkpoint { seq: 0, count: 0, ..base.clone() },
        Checkpoint { count: 2, ..base.clone() },
        Checkpoint { stream: "Record".into(), ..base.clone() },
        Checkpoint { stream: "r".repeat(MAX_STREAM_BYTES + 1), ..base.clone() },
        Checkpoint { stream: String::new(), ..base.clone() },
        Checkpoint { time: "2026-11-19T12:00:00.5Z".into(), ..base.clone() },
        Checkpoint { time: "2026-02-29T12:00:00Z".into(), ..base.clone() },
        Checkpoint { time: "2016-12-31T23:59:60Z".into(), ..base.clone() },
        Checkpoint { ledger: Some(LedgerHead { seq: 0, head: GENESIS_PREV }), ..base.clone() },
    ];
    for cp in bad {
        assert_eq!(cp.canonical_bytes().unwrap_err().code(), "schema", "{cp:?}");
    }
    assert!(Checkpoint { stream: "r".repeat(MAX_STREAM_BYTES), ..base.clone() }.canonical_bytes().is_ok());
    for r in Reason::ALL {
        assert_eq!(Reason::parse(r.as_str()), Some(r));
    }
}

// ------------------------------------------------------------------ verification

fn link(v: &CValue) -> Link {
    let CValue::Arr(a) = v else { panic!("link must be an array") };
    let s = |i: usize| match &a[i] {
        CValue::Str(s) => s.clone(),
        other => panic!("link field {other:?}"),
    };
    Link { seq: s(0).parse().unwrap(), prev: digest(&s(1)), hash: digest(&s(2)) }
}

fn expected_error(e: &Obj) -> CheckpointError {
    let n = |k: &str| num(e, k);
    match st(e, "code") {
        "truncated" => {
            CheckpointError::Truncated { checkpoint_seq: n("checkpoint_seq"), journal_count: n("journal_count") }
        }
        "extended" => {
            CheckpointError::Extended { checkpoint_seq: n("checkpoint_seq"), journal_count: n("journal_count") }
        }
        "reordered" => CheckpointError::Reordered { position: n("position"), seq: n("seq") },
        "broken_link" => CheckpointError::BrokenLink { seq: n("seq") },
        "forked_head" => CheckpointError::ForkedHead {
            seq: n("seq"),
            checkpoint: digest(st(e, "checkpoint")),
            journal: digest(st(e, "journal")),
        },
        other => panic!("no fields for {other}"),
    }
}

#[test]
fn verify_cases_match_the_reference() {
    for c in arr(&expected(), "verify") {
        let c = obj(c);
        let name = st(c, "name");
        let coverage = match st(c, "coverage") {
            "exact" => Coverage::Exact,
            "prefix" => Coverage::Prefix,
            other => panic!("{other}"),
        };
        let links: Vec<Link> = arr(c, "links").iter().map(link).collect();
        let want = obj(c.get("expect").unwrap());
        let got = Checkpoint::from_bytes(st(c, "checkpoint").as_bytes())
            .and_then(|cp| verify_links(&cp, st(c, "stream"), links, coverage));
        match st(want, "code") {
            "ok" => assert_eq!(got.unwrap().checked, num(want, "checked"), "{name}"),
            code @ ("unknown_format" | "schema" | "stream_mismatch") => {
                assert_eq!(got.unwrap_err().code(), code, "{name}")
            }
            _ => assert_eq!(got.unwrap_err(), expected_error(want), "{name}"),
        }
    }
}

#[test]
fn prefix_verification_stops_reading_at_the_checkpoint() {
    let x = expected();
    let case = arr(&x, "verify").iter().map(obj).find(|c| st(c, "name") == "prefix_longer_journal").unwrap();
    let cp = Checkpoint::from_bytes(st(case, "checkpoint").as_bytes()).unwrap();
    let links: Vec<Link> = arr(case, "links").iter().map(link).collect();
    let read = std::cell::Cell::new(0usize);
    let counted = links.iter().map(|l| {
        read.set(read.get() + 1);
        *l
    });
    verify_links(&cp, STREAM, counted, Coverage::Prefix).unwrap();
    assert_eq!(read.get(), cp.seq as usize);
    // A read error is passed through.
    let failing = vec![Ok(links[0]), Err(CheckpointError::Storage("disk".into()))];
    let e = fid_journal::checkpoint::verify(&cp, STREAM, failing, Coverage::Prefix).unwrap_err();
    assert_eq!(e.code(), "storage");
}

// ------------------------------------------------------------------ schema and codes

fn at<'a>(v: &'a CValue, path: &[&str]) -> &'a CValue {
    path.iter().fold(v, |v, k| obj(v).get(*k).unwrap_or_else(|| panic!("schema has no {k}")))
}

fn strings(v: &CValue) -> Vec<String> {
    let CValue::Arr(a) = v else { panic!("array expected") };
    a.iter()
        .map(|x| match x {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn schema_matches_rust_constants() {
    let s = load("schema/checkpoint.schema.json");
    let required = strings(at(&s, &["required"]));
    assert_eq!(required, ["format", "stream", "seq", "head", "count", "reason", "time"]);
    let mut props: Vec<&String> = obj(at(&s, &["properties"])).keys().collect();
    props.sort();
    assert_eq!(props, ["count", "format", "head", "ledger", "reason", "seq", "stream", "time"]);
    assert_eq!(at(&s, &["additionalProperties"]), &CValue::Bool(false));
    assert_eq!(at(&s, &["properties", "format", "const"]), &CValue::Str(FORMAT_LINEAR_V1.into()));
    let reasons: Vec<String> = Reason::ALL.iter().map(|r| r.as_str().to_string()).collect();
    assert_eq!(strings(at(&s, &["properties", "reason", "enum"])), reasons);
    assert_eq!(strings(at(&s, &["properties", "ledger", "required"])), ["seq", "head"]);
    assert_eq!(at(&s, &["properties", "ledger", "additionalProperties"]), &CValue::Bool(false));
    let pat = |d: &str| match at(&s, &["$defs", d, "pattern"]) {
        CValue::Str(p) => p.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(pat("digest"), DIGEST_PATTERN);
    assert_eq!(pat("seq"), SEQ_PATTERN);
    assert_eq!(pat("stream"), format!("^[a-z][a-z0-9_]{{0,{}}}$", MAX_STREAM_BYTES - 1));
    assert_eq!(pat("time"), "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$");
}

#[test]
fn error_codes_are_stable() {
    let d = GENESIS_PREV;
    let all = [
        (CheckpointError::UnknownFormat(String::new()), "unknown_format"),
        (CheckpointError::Schema(String::new()), "schema"),
        (CheckpointError::InputLimit(0), "input_limit"),
        (CheckpointError::StreamMismatch { checkpoint: String::new(), journal: String::new() }, "stream_mismatch"),
        (CheckpointError::Truncated { checkpoint_seq: 1, journal_count: 0 }, "truncated"),
        (CheckpointError::Extended { checkpoint_seq: 1, journal_count: 2 }, "extended"),
        (CheckpointError::Reordered { position: 1, seq: 2 }, "reordered"),
        (CheckpointError::BrokenLink { seq: 1 }, "broken_link"),
        (CheckpointError::ForkedHead { seq: 1, checkpoint: d, journal: d }, "forked_head"),
        (CheckpointError::Storage(String::new()), "storage"),
    ];
    for (e, code) in all {
        assert_eq!(e.code(), code);
        assert!(e.to_string().starts_with(code), "{e}");
    }
    let e = JournalError::EmptyStream(STREAM.into());
    assert_eq!(e.code(), "empty_stream");
    assert!(e.to_string().starts_with("empty_stream"));
}
