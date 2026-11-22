//! Acceptance criteria of spec node CORE.JRN.CHAIN.VERIFY (AC-1 .. AC-4; AC-5 needs the bus
//! pipeline and is deferred) and the pinned rules of module `verify`: located, fail-closed
//! failures for every tamper case, bounds, refusals and their bus error mapping.
//!
//! Expected outcomes come from the independent Python reference
//! `fixtures/core/jrn/chain/verify/make_fixtures.py` (its own RFC 8785 writer, hashing and
//! verifier). A case is a list of mutations of the `good` journal; this file applies them with
//! the same meaning and checks that the Rust verifier reaches the oracle's outcome, in memory
//! for every case and through a real project database for a selection. The 2,500-match paging
//! test and the append round trip are hand-labelled from the spec rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue, Digest};
use fid_journal::checkpoint::{Reason, MAX_CHECKPOINT_BYTES};
use fid_journal::verify::{
    check_heads, list_rows, plan_list, resolve_start, verify_rows, Anchor, BlobSource, BlobState, CheckpointRow,
    FailureKind, ListQuery, ReadError, Start, StoredEntry, TrustedHead, VerifyOptions, VerifyReport, LIST_ENTRIES,
    MAX_CHECKPOINTS, MAX_ENTRY_REFS, MAX_PAGE, REASONS, VERIFY_CHAIN,
};
use fid_journal::{
    build, command_defs, parse_digest, verify_store, AiAssistance, AppendRequest, Clock, CommandDraft, DraftBody,
    EntryDraft, EntryTime, Field, Journal, Manifests, OsSalts, PrincipalDraft, PrincipalKind, RecordedSalts, Salt,
    GENESIS_PREV,
};
use fid_store::cas::PutOpts;
use fid_store::schema::open_write;
use fid_store::{OpenMode, ProjectLocation, Store, Value};
use fid_types::{Determinism, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const STREAM: &str = "record";
const BUS_ERRORS_V3: &[u8] = include_bytes!("../../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

// ------------------------------------------------------------------ scaffolding

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-verify-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

fn int(o: &Obj, k: &str) -> i64 {
    match o.get(k) {
        Some(CValue::Num(n)) if n.fract() == 0.0 => *n as i64,
        other => panic!("field {k} is {other:?}"),
    }
}

fn load(rel: &str) -> CValue {
    parse(&std::fs::read(Path::new(ROOT).join(rel)).unwrap()).unwrap()
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

// ------------------------------------------------------------------ the fixture model

#[derive(Clone)]
struct Model {
    heads: Vec<(String, String)>,
    rows: Vec<StoredEntry>,
    store: BTreeMap<[u8; 32], Vec<u8>>,
    checkpoints: Vec<CheckpointRow>,
}

fn good() -> Model {
    let v = load("fixtures/core/jrn/chain/verify/good.json");
    let g = obj(&v);
    assert_eq!(st(g, "stream"), STREAM);
    let mut store = BTreeMap::new();
    let rows = arr(g, "entries")
        .iter()
        .map(|e| {
            let e = obj(e);
            let header = st(e, "header").as_bytes().to_vec();
            store.insert(digest(st(e, "payload_digest")).0, st(e, "payload").as_bytes().to_vec());
            let mut refs = vec![("payload".to_string(), st(e, "payload_digest").to_string())];
            for r in arr(e, "refs") {
                let CValue::Str(r) = r else { panic!() };
                refs.push(("ref".to_string(), r.clone()));
            }
            StoredEntry {
                seq: int(e, "seq"),
                hash: st(e, "hash").into(),
                stream: st(e, "stream").into(),
                branch: st(e, "branch").into(),
                kind: st(e, "kind").into(),
                prev_hash: Some(st(e, "prev_hash").into()),
                header_len: header.len() as u64,
                header,
                refs,
            }
        })
        .collect();
    for b in arr(g, "blobs") {
        let b = obj(b);
        store.insert(digest(st(b, "digest")).0, st(b, "content").as_bytes().to_vec());
    }
    let checkpoints = arr(g, "checkpoints")
        .iter()
        .map(|c| {
            let c = obj(c);
            CheckpointRow {
                id: int(c, "id"),
                stream: st(c, "stream").into(),
                entry_hash: st(c, "entry_hash").into(),
                body: st(c, "body").as_bytes().to_vec(),
            }
        })
        .collect();
    Model { heads: Vec::new(), rows, store, checkpoints }
}

fn flip(b: &mut [u8], bit: i64) {
    let bit = bit as usize;
    b[bit / 8] ^= 1 << (bit % 8);
}

fn row_at(m: &mut Model, seq: i64) -> &mut StoredEntry {
    m.rows.iter_mut().find(|r| r.seq == seq).unwrap()
}

/// Applies the oracle's mutations (same meaning as `apply` in make_fixtures.py).
fn apply(base: &Model, muts: &[CValue]) -> Model {
    let mut m = base.clone();
    for mu in muts {
        let mu = obj(mu);
        match st(mu, "op") {
            "flip_header" => {
                let r = row_at(&mut m, int(mu, "seq"));
                flip(&mut r.header, int(mu, "bit"));
            }
            "set_header" => {
                let r = row_at(&mut m, int(mu, "seq"));
                r.header = st(mu, "header").as_bytes().to_vec();
                r.header_len = r.header.len() as u64;
            }
            "set_column" => {
                let r = row_at(&mut m, int(mu, "seq"));
                let v = st(mu, "value").to_string();
                match st(mu, "column") {
                    "hash" => r.hash = v,
                    "prev_hash" => r.prev_hash = Some(v),
                    "stream" => r.stream = v,
                    "branch" => r.branch = v,
                    "kind" => r.kind = v,
                    other => panic!("column {other}"),
                }
            }
            "flip_store" => flip(m.store.get_mut(&digest(st(mu, "digest")).0).unwrap(), int(mu, "bit")),
            "remove_store" => {
                m.store.remove(&digest(st(mu, "digest")).0).unwrap();
            }
            "remove_payloads" => {
                for r in &m.rows {
                    for (role, d) in &r.refs {
                        if role == "payload" {
                            m.store.remove(&digest(d).0);
                        }
                    }
                }
            }
            "put_store" => {
                m.store.insert(digest(st(mu, "digest")).0, st(mu, "content").as_bytes().to_vec());
            }
            "set_refs" => {
                let refs = arr(mu, "refs")
                    .iter()
                    .map(|p| match p {
                        CValue::Arr(p) => match (&p[0], &p[1]) {
                            (CValue::Str(a), CValue::Str(b)) => (a.clone(), b.clone()),
                            _ => panic!(),
                        },
                        _ => panic!(),
                    })
                    .collect();
                row_at(&mut m, int(mu, "seq")).refs = refs;
            }
            "drop" => {
                let s = int(mu, "seq");
                m.rows.retain(|r| r.seq != s);
            }
            "drop_renumber" => {
                let s = int(mu, "seq");
                m.rows.retain(|r| r.seq != s);
                for r in &mut m.rows {
                    if r.seq > s {
                        r.seq -= 1;
                    }
                }
            }
            "swap" => {
                let (a, b) = (int(mu, "a"), int(mu, "b"));
                let ia = m.rows.iter().position(|r| r.seq == a).unwrap();
                let ib = m.rows.iter().position(|r| r.seq == b).unwrap();
                let (sa, sb) = (m.rows[ia].seq, m.rows[ib].seq);
                m.rows.swap(ia, ib);
                m.rows[ia].seq = sa;
                m.rows[ib].seq = sb;
            }
            "truncate" => {
                let keep = int(mu, "keep");
                m.rows.retain(|r| r.seq <= keep);
            }
            "add_checkpoint" => m.checkpoints.push(CheckpointRow {
                id: int(mu, "id"),
                stream: st(mu, "stream").into(),
                entry_hash: st(mu, "entry_hash").into(),
                body: st(mu, "body").as_bytes().to_vec(),
            }),
            "flip_checkpoint" => {
                let id = int(mu, "id");
                let c = m.checkpoints.iter_mut().find(|c| c.id == id).unwrap();
                flip(&mut c.body, int(mu, "bit"));
            }
            "drop_checkpoints" => m.checkpoints.clear(),
            "add_head" => m.heads.push((st(mu, "branch").into(), st(mu, "entry_hash").into())),
            "relink_from" => relink_from(&mut m, int(mu, "seq"), st(mu, "fake_below")),
            other => panic!("unknown mutation {other}"),
        }
    }
    m.rows.sort_by_key(|r| r.seq);
    m.checkpoints.sort_by_key(|c| c.id);
    m
}

/// The `relink_from` mutation: entry n-1's stored hash becomes `fake`, and entries n.. are
/// re-hashed so each links to the one below (the reviewer's attack on an unanchored
/// `from_seq`). Same meaning as in make_fixtures.py.
fn relink_from(m: &mut Model, n: i64, fake: &str) {
    row_at(m, n - 1).hash = fake.to_string();
    let mut prev = fake.to_string();
    m.rows.sort_by_key(|r| r.seq);
    for r in m.rows.iter_mut().filter(|r| r.seq >= n) {
        let CValue::Obj(mut h) = parse(&r.header).unwrap() else { panic!() };
        h.insert("prev".into(), CValue::Str(prev.clone()));
        if !matches!(h.get("logical_parent"), Some(CValue::Null) | None) {
            h.insert("logical_parent".into(), CValue::Str(prev.clone()));
        }
        h.remove("hash");
        let hash = digest_bytes(&to_canonical_bytes(&CValue::Obj(h.clone())).unwrap()).to_string();
        h.insert("hash".into(), CValue::Str(hash.clone()));
        r.header = to_canonical_bytes(&CValue::Obj(h)).unwrap();
        r.header_len = r.header.len() as u64;
        r.hash = hash.clone();
        r.prev_hash = Some(prev);
        prev = hash;
    }
}

/// The blob store of a model, hashing on its own.
struct MemBlobs<'a>(&'a BTreeMap<[u8; 32], Vec<u8>>);

impl BlobSource for MemBlobs<'_> {
    fn check(&mut self, d: &Digest) -> Result<BlobState, ReadError> {
        Ok(match self.0.get(&d.0) {
            None => BlobState::Missing,
            Some(b) if digest_bytes(b) == *d => BlobState::Sound,
            Some(_) => BlobState::Corrupt,
        })
    }

    fn payload(&mut self, d: &Digest) -> Result<Option<Vec<u8>>, ReadError> {
        Ok(self.0.get(&d.0).cloned())
    }
}

/// The whole pipeline over a model, as `verify_store` runs it: anchor, walk, head cache.
fn verify_model(m: &Model, opts: &VerifyOptions) -> Result<VerifyReport, ReadError> {
    let from = opts.from_seq.unwrap_or(1);
    let tail = m.rows.iter().map(|r| r.seq).max().unwrap_or(0) as u64;
    let below = if from > 1 { m.rows.iter().find(|r| r.seq as u64 == from - 1) } else { None };
    let start = resolve_start(STREAM, from, tail, opts.trusted, below)?;
    let mut report = verify_rows(
        STREAM,
        start,
        opts,
        m.checkpoints.iter().cloned().map(Ok),
        // A full verification reads every row (a row below sequence 1 is a `sequence` failure).
        m.rows.iter().filter(|r| start.seq == 1 || r.seq >= start.seq as i64).cloned().map(Ok),
        &mut MemBlobs(&m.store),
    )?;
    let mut heads = m.heads.clone();
    heads.sort();
    check_heads(&mut report, heads.into_iter().map(Ok), |h| {
        Ok(m.rows.iter().find(|r| r.hash == h).map(|r| r.branch.clone()))
    })?;
    Ok(report)
}

/// The options of an oracle case (`from_seq`, `trusted`).
fn case_opts(case: &Obj) -> VerifyOptions {
    let trusted = case.get("trusted").map(|t| {
        let t = obj(t);
        TrustedHead {
            seq: st(t, "seq").parse().unwrap(),
            head: digest(st(t, "head")),
            exact: matches!(t.get("exact"), Some(CValue::Bool(true))),
        }
    });
    VerifyOptions {
        from_seq: case.get("from_seq").map(|_| st(case, "from_seq").parse().unwrap()),
        trusted,
        ..VerifyOptions::default()
    }
}

/// The oracle's outcome of a case: the expectation, or the refusal reason.
fn check_case(name: &str, got: Result<VerifyReport, ReadError>, expect: &Obj) {
    match expect.get("refused") {
        Some(CValue::Str(r)) => {
            let e = got.err().unwrap_or_else(|| panic!("{name}: expected the refusal {r}"));
            assert_eq!(e.bus_code().1, r.as_str(), "{name}: {e}");
            if let Some(want) = expect.get("failure") {
                let want = obj(want);
                let ReadError::Corrupt(f) = &e else { panic!("{name}: expected a located refusal, got {e}") };
                assert_eq!(
                    (f.seq, f.kind.as_str(), f.reason),
                    (Some(int(want, "seq") as u64), st(want, "kind"), st(want, "reason")),
                    "{name}: {f}"
                );
            }
        }
        _ => assert_expect(name, &got.unwrap_or_else(|e| panic!("{name}: refused {e}")), expect),
    }
}

fn cases() -> Obj {
    obj(&load("fixtures/core/jrn/chain/verify/cases.json")).clone()
}

/// Compares a report with the oracle's `expect`.
fn assert_expect(name: &str, got: &VerifyReport, expect: &Obj) {
    assert_eq!(got.from_seq.to_string(), st(expect, "from_seq"), "{name}: from_seq");
    assert_eq!(&got.anchor.to_value(), expect.get("anchor").unwrap(), "{name}: anchor");
    let ok = matches!(expect.get("ok"), Some(CValue::Bool(true)));
    if ok {
        assert!(got.ok(), "{name}: expected ok, got {:?}", got.failure);
        let c = obj(expect.get("checked").unwrap());
        let want = [int(c, "entries"), int(c, "blobs"), int(c, "checkpoints"), int(c, "without_payload")];
        let have = [got.checked.entries, got.checked.blobs, got.checked.checkpoints, got.checked.without_payload];
        assert_eq!(have.map(|n| n as i64), want, "{name}: counts");
    } else {
        let f = obj(expect.get("failure").unwrap());
        let got_f = got.failure.as_ref().unwrap_or_else(|| panic!("{name}: expected a failure, got ok"));
        let want_seq = match f.get("seq") {
            Some(CValue::Null) => None,
            _ => Some(int(f, "seq") as u64),
        };
        assert_eq!(
            (got_f.seq, got_f.kind.as_str(), got_f.reason),
            (want_seq, st(f, "kind"), st(f, "reason")),
            "{name}: {got_f}"
        );
        assert_eq!(got_f.stream, STREAM);
        if got_f.checkpoint.is_some() {
            assert_eq!(got_f.kind, FailureKind::Checkpoint, "{name}: a checkpoint id on a non-checkpoint failure");
        }
        assert!(REASONS.contains(&(got_f.kind, got_f.reason)), "{name}: reason {} not pinned", got_f.reason);
    }
}

// ------------------------------------------------------------------ every case in memory

#[test]
fn every_oracle_case_reaches_the_oracle_outcome() {
    let base = good();
    let c = cases();
    let all = arr(&c, "cases");
    assert!(all.len() >= 100, "the oracle writes over a hundred cases");
    let mut kinds = BTreeSet::new();
    for case in all {
        let case = obj(case);
        let name = st(case, "name");
        let m = apply(&base, arr(case, "mutations"));
        let got = verify_model(&m, &case_opts(case));
        if let Ok(Some(f)) = got.as_ref().map(|r| r.failure.as_ref()) {
            kinds.insert(f.kind);
        }
        check_case(name, got, obj(case.get("expect").unwrap()));
    }
    assert_eq!(kinds.len(), FailureKind::ALL.len(), "every failure kind is exercised");
}

/// AC-2: the named single-change fixtures report the expected kind and sequence.
#[test]
fn ac2_bad_fixtures_report_kind_and_sequence() {
    let base = good();
    let c = cases();
    let by_name: BTreeMap<&str, &Obj> = arr(&c, "cases").iter().map(|x| (st(obj(x), "name"), obj(x))).collect();
    for (name, kind, seq) in [
        ("bad_hash", "hash", Some(137)),
        ("bad_link", "link", Some(211)),
        ("bad_blob", "blob", Some(14)),
        ("bad_checkpoint", "checkpoint", None),
    ] {
        let case = by_name[name];
        let got = verify_model(&apply(&base, arr(case, "mutations")), &VerifyOptions::default()).unwrap();
        let f = got.failure.unwrap();
        assert_eq!((f.kind.as_str(), f.seq), (kind, seq), "{name}");
        // The result value carries stream, sequence and kind.
        let value = got_value(&f);
        let v = obj(&value);
        assert_eq!(st(v, "kind"), kind);
        assert_eq!(st(v, "stream"), STREAM);
    }
}

fn got_value(f: &fid_journal::Failure) -> CValue {
    f.to_value()
}

/// Every single-bit flip of a header (all 8 bits of every byte) never verifies: it is a
/// `hash` failure at that sequence. Each run verifies just that entry, linked to the stored
/// hash below it.
#[test]
fn a_flipped_bit_anywhere_in_a_header_is_a_hash_failure_at_that_sequence() {
    let base = good();
    for seq in [1usize, 2, 250, 500] {
        let row = &base.rows[seq - 1];
        let prev = if seq == 1 { GENESIS_PREV } else { digest(&base.rows[seq - 2].hash) };
        let anchor = if seq == 1 { Anchor::Genesis } else { Anchor::Trusted { seq: seq as u64 - 1 } };
        let start = Start { seq: seq as u64, prev, anchor };
        let clean = verify_rows(
            STREAM,
            start,
            &VerifyOptions::default(),
            Vec::new(),
            [Ok(row.clone())],
            &mut MemBlobs(&base.store),
        );
        assert!(clean.unwrap().ok());
        for bit in 0..row.header.len() * 8 {
            let mut r = row.clone();
            flip(&mut r.header, bit as i64);
            let got =
                verify_rows(STREAM, start, &VerifyOptions::default(), Vec::new(), [Ok(r)], &mut MemBlobs(&base.store));
            let f = got.unwrap().failure.unwrap();
            assert_eq!((f.kind, f.seq), (FailureKind::Hash, Some(seq as u64)), "seq {seq} bit {bit}: {f}");
        }
    }
}

// ------------------------------------------------------------------ bounds and refusals

#[test]
fn bounds_are_refusals_not_success() {
    let base = good();
    let opts = VerifyOptions { max_entries: 499, ..VerifyOptions::default() };
    assert!(matches!(verify_model(&base, &opts), Err(ReadError::InputLimit(_))));
    let opts = VerifyOptions { max_blobs: 19, ..VerifyOptions::default() };
    assert!(matches!(verify_model(&base, &opts), Err(ReadError::InputLimit(_))));
    let cancel = Arc::new(AtomicBool::new(true));
    let opts = VerifyOptions { cancel: Some(cancel), ..VerifyOptions::default() };
    assert_eq!(verify_model(&base, &opts), Err(ReadError::Cancelled));

    // A header declared over the bound is not parsed.
    let mut m = base.clone();
    m.rows[9].header_len = 2 * 1024 * 1024;
    let f = verify_model(&m, &VerifyOptions::default()).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Hash, "oversize", Some(10)));

    // Too many blob rows on one entry.
    let mut m = base.clone();
    m.rows[0].refs = (0..=MAX_ENTRY_REFS).map(|i| ("ref".to_string(), format!("sha256:{i:064x}"))).collect();
    assert!(matches!(verify_model(&m, &VerifyOptions::default()), Err(ReadError::InputLimit(_))));

    // Too many checkpoints.
    let mut m = base.clone();
    let one = m.checkpoints[0].clone();
    m.checkpoints = (1..=MAX_CHECKPOINTS as i64 + 1).map(|id| CheckpointRow { id, ..one.clone() }).collect();
    assert!(matches!(verify_model(&m, &VerifyOptions::default()), Err(ReadError::InputLimit(_))));

    // A blob digest column that is not a digest, a null prev_hash column, a negative sequence.
    let mut m = base.clone();
    m.rows[6].refs.push(("ref".into(), "sha256:XYZ".into()));
    let f = verify_model(&m, &VerifyOptions::default()).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Blob, "bad_ref", Some(7)));
    let mut m = base.clone();
    m.rows[3].prev_hash = None;
    let f = verify_model(&m, &VerifyOptions::default()).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Link, "column_mismatch", Some(4)));
    let mut m = base.clone();
    m.rows[0].seq = -1;
    let f = verify_model(&m, &VerifyOptions::default()).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Sequence, "reorder", Some(1)));
}

#[test]
fn a_trusted_head_catches_a_truncated_tail_without_checkpoints() {
    let base = good();
    let mut m = base.clone();
    m.checkpoints.clear();
    let tail = digest(&base.rows[499].hash);
    let exact = TrustedHead { seq: 500, head: tail, exact: true };
    let opts = VerifyOptions { trusted: Some(exact), ..VerifyOptions::default() };
    assert!(verify_model(&m, &opts).unwrap().ok());
    m.rows.truncate(490);
    let f = verify_model(&m, &opts).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Checkpoint, "truncated", Some(500)));
    // Exact: entries after the trusted head are refused.
    let at250 = TrustedHead { seq: 250, head: digest(&base.rows[249].hash), exact: true };
    let opts = VerifyOptions { trusted: Some(at250), ..VerifyOptions::default() };
    let mut m = base.clone();
    m.checkpoints.clear();
    let f = verify_model(&m, &opts).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Sequence, "extended", Some(251)));
    // A trusted head that is not the chain's.
    let forged = TrustedHead { seq: 250, head: digest_bytes(b"x"), exact: false };
    let opts = VerifyOptions { trusted: Some(forged), ..VerifyOptions::default() };
    let f = verify_model(&m, &opts).unwrap().failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Checkpoint, "forked_head", Some(250)));
}

#[test]
fn a_walk_from_a_later_sequence_links_to_its_anchor() {
    let base = good();
    let anchored = Start { seq: 301, prev: digest(&base.rows[299].hash), anchor: Anchor::Trusted { seq: 300 } };
    let got = verify_rows(
        STREAM,
        anchored,
        &VerifyOptions::default(),
        base.checkpoints.iter().cloned().map(Ok),
        base.rows[300..].iter().cloned().map(Ok),
        &mut MemBlobs(&base.store),
    )
    .unwrap();
    assert!(got.ok(), "{:?}", got.failure);
    // 200 entries; only the checkpoint at 500 is at or above the start.
    assert_eq!((got.checked.entries, got.checked.checkpoints, got.from_seq), (200, 1, 301));
    let bad = Start { seq: 301, prev: GENESIS_PREV, anchor: Anchor::Trusted { seq: 300 } };
    let got = verify_rows(
        STREAM,
        bad,
        &VerifyOptions::default(),
        Vec::new(),
        base.rows[300..].iter().cloned().map(Ok),
        &mut MemBlobs(&base.store),
    )
    .unwrap();
    let f = got.failure.unwrap();
    assert_eq!((f.kind, f.reason, f.seq), (FailureKind::Link, "broken", Some(301)));
}

fn catalogue_v3() -> BTreeMap<String, BTreeSet<String>> {
    let v = parse(BUS_ERRORS_V3).unwrap();
    obj(obj(&v).get("codes").unwrap())
        .iter()
        .map(|(c, rs)| {
            let CValue::Arr(rs) = rs else { panic!() };
            (c.clone(), rs.iter().map(|r| if let CValue::Str(s) = r { s.clone() } else { panic!() }).collect())
        })
        .collect()
}

#[test]
fn every_refusal_and_failure_maps_into_bus_catalogue_v3() {
    let cat = catalogue_v3();
    let fail = fid_journal::Failure {
        stream: STREAM.into(),
        seq: Some(1),
        kind: FailureKind::Hash,
        reason: "hash_mismatch",
        checkpoint: None,
        detail: String::new(),
    };
    let all = [
        ReadError::UnknownStream("x".into()),
        ReadError::UnknownBranch("x".into()),
        ReadError::BadRange("x".into()),
        ReadError::BadParam("x".into()),
        ReadError::BadToken,
        ReadError::StaleToken,
        ReadError::InputLimit("x".into()),
        ReadError::Cancelled,
        ReadError::Storage("x".into()),
        ReadError::Corrupt(fail.clone()),
    ];
    for e in &all {
        let (code, reason) = e.bus_code();
        assert!(cat.get(code).is_some_and(|rs| rs.contains(reason)), "{e:?} -> {code}/{reason}");
        let caller_fault = matches!(code, "validation" | "not_found" | "cancelled");
        assert_eq!(caller_fault, !matches!(e, ReadError::Storage(_) | ReadError::Corrupt(_)), "{e:?}");
    }
    let (code, reason) = fail.bus_code();
    assert_eq!((code, reason), ("internal", "invariant_violated"));
    assert!(cat[code].contains(reason));
    // The pinned reasons are unique and cover every kind.
    let set: BTreeSet<_> = REASONS.iter().collect();
    assert_eq!(set.len(), REASONS.len());
    for k in FailureKind::ALL {
        assert!(REASONS.iter().any(|(kind, _)| *kind == k));
    }
}

#[test]
fn literal_sql_bounds_match_the_constants() {
    // store.rs writes these as literals into its SQL.
    assert_eq!(MAX_ENTRY_REFS + 1, 4098);
    assert_eq!(MAX_CHECKPOINTS + 1, 65_537);
    assert_eq!(MAX_CHECKPOINT_BYTES + 1, 4097);
}

// ------------------------------------------------------------------ listing

fn query(q: &Obj) -> ListQuery {
    let get = |k: &str| q.get(k).map(|_| st(q, k).to_string());
    ListQuery {
        stream: None,
        branch: get("branch"),
        from_seq: get("from_seq").map(|s| s.parse().unwrap()),
        to_seq: get("to_seq").map(|s| s.parse().unwrap()),
        command: get("command"),
        principal_kind: get("principal_kind"),
        page_token: None,
    }
}

/// Every page of a query over `rows`.
fn all_pages(rows: &[StoredEntry], q: &ListQuery) -> Vec<Vec<u64>> {
    let tail = rows.last().map_or(0, |r| r.seq as u64);
    let mut q = q.clone();
    let mut pages = Vec::new();
    loop {
        let plan = plan_list(STREAM, tail, &q).unwrap();
        let page = list_rows(&plan, rows.iter().filter(|r| r.seq as u64 >= plan.start).cloned().map(Ok)).unwrap();
        assert!(page.headers.len() <= MAX_PAGE);
        pages.push(page.headers.iter().map(|h| h.seq).collect());
        match page.next {
            Some(t) => q.page_token = Some(t),
            None => return pages,
        }
    }
}

#[test]
fn ac4_list_filters_return_exactly_the_matching_headers_in_order() {
    let base = good();
    let c = cases();
    for l in arr(&c, "lists") {
        let l = obj(l);
        let q = query(obj(l.get("query").unwrap()));
        let want: Vec<u64> =
            arr(l, "seqs").iter().map(|s| if let CValue::Num(n) = s { *n as u64 } else { panic!() }).collect();
        let pages = all_pages(&base.rows, &q);
        assert_eq!(pages.len(), 1, "{q:?}");
        assert_eq!(pages[0], want, "{q:?}");
    }
}

fn draft(seq: u64, command: &str) -> EntryDraft {
    let zero = Digest([7; 32]);
    EntryDraft {
        stream: STREAM.into(),
        branch: "main".into(),
        logical_parent: None,
        time: EntryTime { utc: "2026-11-20T10:00:00Z".into(), mono_ns: seq, sync: "synced".into() },
        principal: PrincipalDraft {
            kind: PrincipalKind::Script,
            identity: CValue::Str("ci".into()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: Manifests { registry: zero, kernel: zero },
        reason: None,
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::Command(CommandDraft {
            id: command.into(),
            major: 1,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: seq,
            args: CValue::Arr(Vec::new()),
            events: Vec::new(),
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs: Vec::new(),
            algorithms: Vec::new(),
            event_sources: Vec::new(),
        }),
    }
}

/// `n` valid chained entries built by `entry::build`; every 26th is `feature.set_nominal@1`,
/// the rest `project.set_tags@1`.
fn synthetic(n: u64) -> Vec<StoredEntry> {
    let mut rows = Vec::new();
    let mut prev = GENESIS_PREV;
    for seq in 1..=n {
        let cmd = if seq % 26 == 0 { "feature.set_nominal" } else { "project.set_tags" };
        let e = build(&draft(seq, cmd), seq, prev, &mut OsSalts).unwrap();
        let header = fid_canon::to_canonical_bytes(&e.header.to_value()).unwrap();
        rows.push(StoredEntry {
            seq: seq as i64,
            hash: e.header.hash.to_string(),
            stream: STREAM.into(),
            branch: "main".into(),
            kind: "command".into(),
            prev_hash: Some(prev.to_string()),
            header_len: header.len() as u64,
            header,
            refs: Vec::new(),
        });
        prev = e.header.hash;
    }
    rows
}

/// AC-4, paging: 2,500 matching entries among 2,600 come back as pages of 1,000, 1,000 and 500,
/// in sequence order, with tokens only on the first two (hand-labelled from REQ-005).
#[test]
fn ac4_2500_matches_come_back_in_three_pages() {
    let rows = synthetic(2600);
    let q =
        ListQuery { branch: Some("main".into()), command: Some("project.set_tags@1".into()), ..ListQuery::default() };
    let pages = all_pages(&rows, &q);
    assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), [1000, 1000, 500]);
    let flat: Vec<u64> = pages.concat();
    let want: Vec<u64> = (1..=2600).filter(|s| s % 26 != 0).collect();
    assert_eq!(flat, want);

    // Tokens are bound to their query and checked.
    let plan = plan_list(STREAM, 2600, &q).unwrap();
    let page = list_rows(&plan, rows.iter().cloned().map(Ok)).unwrap();
    let token = page.next.unwrap();
    let other = ListQuery { page_token: Some(token.clone()), principal_kind: Some("human".into()), ..q.clone() };
    assert_eq!(plan_list(STREAM, 2600, &other), Err(ReadError::StaleToken));
    for bad in ["", "jl1.", "jl1.zz.5", &format!("{token}0x"), "jl2.0000000000000000.1", "jl1.0123456789abcdef.0"] {
        let q2 = ListQuery { page_token: Some(bad.to_string()), ..q.clone() };
        assert!(matches!(plan_list(STREAM, 2600, &q2), Err(ReadError::BadToken | ReadError::StaleToken)), "{bad}");
    }

    // A tampered row is never listed: it is a refusal.
    let mut tampered = rows.clone();
    tampered[5].header[40] ^= 1;
    let err = list_rows(&plan, tampered.iter().cloned().map(Ok)).unwrap_err();
    assert!(matches!(&err, ReadError::Corrupt(f) if f.seq == Some(6)), "{err:?}");
    assert_eq!(err.bus_code(), ("internal", "invariant_violated"));
}

#[test]
fn list_parameters_are_checked() {
    let bad = [
        (ListQuery { stream: Some("ledger".into()), ..ListQuery::default() }, "unknown_stream"),
        (ListQuery { from_seq: Some(0), ..ListQuery::default() }, "bad_range"),
        (ListQuery { from_seq: Some(9), to_seq: Some(3), ..ListQuery::default() }, "bad_range"),
        (ListQuery { command: Some("project.set_tags".into()), ..ListQuery::default() }, "bad_param"),
        (ListQuery { command: Some("Project@1".into()), ..ListQuery::default() }, "bad_param"),
        (ListQuery { principal_kind: Some("robot".into()), ..ListQuery::default() }, "bad_param"),
        (ListQuery { branch: Some("-x".into()), ..ListQuery::default() }, "bad_param"),
    ];
    for (q, code) in bad {
        assert_eq!(plan_list(STREAM, 10, &q).unwrap_err().code(), code, "{q:?}");
    }
}

// ------------------------------------------------------------------ through the project database

/// A project holding the model's rows, blobs and checkpoints, written through `fid-store`;
/// then the model's blob-store differences applied to the files (corrupted or removed blobs).
fn project_of(tag: &str, base: &Model, m: &Model) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    {
        let store = Store::create(&root).unwrap();
        let mut txn = store.transaction();
        let mut put = BTreeSet::new();
        for (d, b) in base.store.iter().chain(m.store.iter()) {
            if digest_bytes(b).0 == *d && put.insert(*d) {
                txn.put_blob(b.clone()).unwrap();
            }
        }
        for r in &m.rows {
            txn.insert(
                "entries",
                &[
                    ("seq", Value::Integer(r.seq)),
                    ("hash", Value::Text(r.hash.clone())),
                    ("stream", Value::Text(r.stream.clone())),
                    ("branch", Value::Text(r.branch.clone())),
                    ("kind", Value::Text(r.kind.clone())),
                    ("prev_hash", r.prev_hash.clone().map_or(Value::Null, Value::Text)),
                    ("header", Value::Blob(r.header.clone())),
                ],
            );
            for (role, d) in &r.refs {
                txn.insert(
                    "entry_blobs",
                    &[
                        ("entry_hash", Value::Text(r.hash.clone())),
                        ("role", Value::Text(role.clone())),
                        ("digest", Value::Text(d.clone())),
                    ],
                );
            }
        }
        txn.commit().unwrap();
        // Blob files the model changed or removed.
        for d in &put {
            let path = store.blobs().path_of(&fid_types::Digest::from_bytes(*d));
            match m.store.get(d) {
                Some(b) if digest_bytes(b).0 == *d => {}
                other => {
                    let mut perm = std::fs::metadata(&path).unwrap().permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    std::fs::set_permissions(&path, perm).unwrap();
                    match other {
                        Some(b) => std::fs::write(&path, b).unwrap(),
                        None => std::fs::remove_file(&path).unwrap(),
                    }
                }
            }
        }
        // Blobs the model holds under a digest they do not hash to, never put above.
        for (d, b) in &m.store {
            if !put.contains(d) {
                let path = store.blobs().path_of(&fid_types::Digest::from_bytes(*d));
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, b).unwrap();
            }
        }
    }
    // Checkpoint rows with foreign keys off: a truncated or forked journal leaves rows whose
    // entry no longer exists, as an editor of the file could.
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let db = open_write(&loc, &lock.unwrap()).unwrap();
    let conn = db.connection();
    conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    for (branch, entry_hash) in &m.heads {
        conn.execute(
            "INSERT INTO heads (branch, entry_hash) VALUES (?1, ?2)",
            [Value::Text(branch.clone()), Value::Text(entry_hash.clone())],
        )
        .unwrap();
    }
    for c in &m.checkpoints {
        conn.execute(
            "INSERT INTO checkpoints (id, stream, entry_hash, body) VALUES (?1, ?2, ?3, ?4)",
            [
                Value::Integer(c.id),
                Value::Text(c.stream.clone()),
                Value::Text(c.entry_hash.clone()),
                Value::Blob(c.body.clone()),
            ],
        )
        .unwrap();
    }
    (t, root)
}

/// Every table's rows, for the read-only check.
fn snapshot(store: &Store) -> Vec<String> {
    store
        .read(|conn| {
            let mut out = Vec::new();
            for sql in [
                "SELECT group_concat(seq || hash, ',') FROM (SELECT * FROM entries ORDER BY seq)",
                "SELECT group_concat(entry_hash || role || digest, ',') FROM entry_blobs",
                "SELECT group_concat(id || entry_hash, ',') FROM checkpoints",
                "SELECT group_concat(branch || entry_hash, ',') FROM heads",
                "SELECT count(*) || '' FROM settings_current",
            ] {
                out.push(conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0))?.unwrap_or_default());
            }
            Ok(out)
        })
        .unwrap()
}

#[test]
fn ac1_ac3_good_journal_verifies_through_the_store_and_nothing_is_written() {
    let base = good();
    let (_t, root) = project_of("good", &base, &base);
    let store = Store::open(&root, OpenMode::Read).unwrap();
    let before = snapshot(&store);
    let got = verify_store(&store, STREAM, &VerifyOptions::default()).unwrap();
    assert!(got.ok(), "{:?}", got.failure);
    assert_eq!(
        (got.checked.entries, got.checked.blobs, got.checked.checkpoints, got.checked.without_payload),
        (500, 20, 2, 0)
    );
    let v = got.to_value();
    assert_eq!(obj(&v).get("ok"), Some(&CValue::Bool(true)));
    assert_eq!(st(obj(obj(&v).get("checked").unwrap()), "entries"), "500");
    // R0: nothing changed (REQ-006).
    assert_eq!(snapshot(&store), before);

    // The same through an open journal, from a later sequence, and a bad range.
    drop(store);
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let j = Journal::open(loc, lock.unwrap(), STREAM).unwrap();
    assert!(j.verify_chain(&VerifyOptions::default()).unwrap().ok());
    // A stored checkpoint at 250 does not anchor; a trusted head does.
    let from = VerifyOptions { from_seq: Some(251), ..VerifyOptions::default() };
    assert!(matches!(j.verify_chain(&from), Err(ReadError::BadRange(_))));
    let head = TrustedHead { seq: 250, head: digest(&base.rows[249].hash), exact: false };
    let from = VerifyOptions { from_seq: Some(251), trusted: Some(head), ..VerifyOptions::default() };
    let r = j.verify_chain(&from).unwrap();
    assert_eq!((r.ok(), r.checked.entries, r.checked.checkpoints), (true, 250, 1));
    for bad in [0, 502] {
        let o = VerifyOptions { from_seq: Some(bad), ..VerifyOptions::default() };
        assert!(matches!(j.verify_chain(&o), Err(ReadError::BadRange(_))), "{bad}");
    }
    let o = VerifyOptions { stream: Some("ledger".into()), ..VerifyOptions::default() };
    assert_eq!(j.verify_chain(&o).unwrap_err().bus_code(), ("not_found", "unknown_stream"));

    // AC-4 through the store.
    let q =
        ListQuery { branch: Some("main".into()), command: Some("project.set_tags@1".into()), ..ListQuery::default() };
    let page = j.list_entries(&q).unwrap();
    assert_eq!(page.headers.len(), 150);
    assert!(page.headers.windows(2).all(|w| w[0].seq < w[1].seq));
    let q = ListQuery { branch: Some("nope".into()), ..ListQuery::default() };
    assert_eq!(j.list_entries(&q).unwrap_err().bus_code(), ("not_found", "unknown_branch"));
    drop(j);

    // AC-3: all payloads removed.
    let c = cases();
    let no_payloads = arr(&c, "cases").iter().map(obj).find(|x| st(x, "name") == "no_payloads").unwrap();
    let m = apply(&base, arr(no_payloads, "mutations"));
    let (_t2, root2) = project_of("nopayload", &base, &m);
    let store = Store::open(&root2, OpenMode::Read).unwrap();
    let got = verify_store(&store, STREAM, &VerifyOptions::default()).unwrap();
    assert_expect("no_payloads (store)", &got, obj(no_payloads.get("expect").unwrap()));
    assert_eq!(got.checked.without_payload, 500);
}

/// Tamper cases through a real project database: the stored rows, blob files and checkpoint
/// rows as the oracle mutated them.
#[test]
fn tamper_cases_through_the_store_reach_the_oracle_outcome() {
    let base = good();
    let c = cases();
    // Each case writes a 500-entry project with its blobs durably; a selection keeps CI time
    // reasonable (every case runs in memory above).
    let selected = [
        "bad_hash",
        "bad_link",
        "bad_blob",
        "bad_checkpoint",
        "truncate_tail",
        "fork_tail",
        "payload_forged",
        "blob_missing",
        "checkpoint_id_minus5",
        "head_past_chain",
        "from_attack_unanchored",
        "from_forged_checkpoint_attack",
        "from_trusted_anchor",
        "from_trusted_anchor_attack",
    ];
    for case in arr(&c, "cases").iter().map(obj) {
        let name = st(case, "name");
        if !selected.contains(&name) {
            continue;
        }
        let m = apply(&base, arr(case, "mutations"));
        let (_t, root) = project_of(name, &base, &m);
        let store = Store::open(&root, OpenMode::Read).unwrap();
        let got = verify_store(&store, STREAM, &case_opts(case));
        check_case(&format!("{name} (store)"), got, obj(case.get("expect").unwrap()));
    }
}

// ------------------------------------------------------------------ appended journals verify

struct FixedClock(Duration);

impl Clock for FixedClock {
    fn now(&self) -> Duration {
        self.0
    }
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

/// A journal written by `Journal::append` and checkpointed by `Journal::checkpoint` verifies,
/// with one blob row per appended referenced blob (hand-labelled: the APPEND fixture chain).
#[test]
fn an_appended_and_checkpointed_journal_verifies() {
    let v = load("fixtures/core/jrn/chain/append/chain.json");
    let entries: Vec<Obj> = arr(obj(&v), "entries").iter().map(|e| obj(e).clone()).collect();
    let t = TempDir::new("append");
    let root = t.0.join("p");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let clock = Arc::new(FixedClock(Duration::from_secs(1_900_000_000)));
    let j = Journal::open_with_clock(loc, lock, STREAM, clock).unwrap();
    let n = entries.len().min(40);
    let mut blobs = BTreeSet::new();
    for e in entries.iter().take(n) {
        for b in arr(e, "blobs") {
            let b = obj(b);
            j.blobs().put_bytes(st(b, "content").as_bytes(), PutOpts { durable: true }).unwrap();
            blobs.insert(st(b, "digest").to_string());
        }
        j.append(request(e)).unwrap();
    }
    j.checkpoint(Reason::Export).unwrap();
    let r = j.verify_chain(&VerifyOptions::default()).unwrap();
    assert!(r.ok(), "{:?}", r.failure);
    assert_eq!(
        (r.checked.entries, r.checked.blobs, r.checked.checkpoints, r.checked.without_payload),
        (n as u64, blobs.len() as u64, 1, 0)
    );
}

// ------------------------------------------------------------------ registry definitions

#[test]
fn both_commands_are_declared_r0_d0_on_cli_and_mcp() {
    let defs = command_defs().unwrap();
    let ids: Vec<String> = defs.iter().map(|d| d.id.clone()).collect();
    assert_eq!(ids, [VERIFY_CHAIN, LIST_ENTRIES]);
    assert_eq!((VERIFY_CHAIN, LIST_ENTRIES), ("journal.verify_chain", "journal.list_entries"));
    for d in &defs {
        let v = d.to_value();
        let o = obj(&v);
        assert_eq!(st(o, "side_effect"), "R0");
        assert_eq!(st(o, "determinism"), "D0");
        assert_eq!(arr(o, "effects").len(), 0);
    }
}

#[test]
fn draft_helper_builds_valid_entries() {
    // Guards the paging test's draft: it must hash and validate.
    let e = build(&draft(1, "project.set_tags"), 1, GENESIS_PREV, &mut OsSalts).unwrap();
    assert!(e.header.verify_hash().is_ok());
}
