//! Acceptance criteria of spec node CORE.JRN.BR.DIFF (AC-1 .. AC-4) and the pinned rules of
//! module `diff`: the diff from verified headers only, tamper and forgery cases, bounds,
//! determinism, refusals and their bus error mapping, and the published schema.
//!
//! Expected outcomes come from the independent Python reference
//! `fixtures/core/jrn/br/diff/make_fixtures.py` (its own RFC 8785 writer, hashing, header
//! check and diff). A tamper case is a list of mutations of the `fork` journal or its branch
//! table; this file applies them with the same meaning and checks that the Rust diff reaches
//! the oracle's outcome, in memory for every case and through a real project database for
//! AC-1 and AC-3. The bound and cancellation cases are hand-labelled from the spec rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_journal::branch::{BranchEvent, BranchName, Branches, EntryOrigin, EventSource, CREATE_BRANCH, MAIN};
use fid_journal::diff::{
    command_defs, diff_rows, diff_store, params_schema, result_schema, BranchDiff, DiffError, DiffOptions, DIFF_BRANCH,
};
use fid_journal::verify::{FailureKind, ReadError, StoredEntry, TrustedHead};
use fid_journal::{parse_digest, Journal, PrincipalKind};
use fid_store::{OpenMode, ProjectLocation, Store, Value};
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const STREAM: &str = "record";
const BUS_ERRORS_V3: &[u8] = include_bytes!("../../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

type Obj = BTreeMap<String, CValue>;

fn load(rel: &str) -> CValue {
    parse(&std::fs::read(Path::new(ROOT).join(rel)).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn get<'a>(o: &'a Obj, k: &str) -> &'a CValue {
    o.get(k).unwrap_or_else(|| panic!("missing {k}"))
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

fn int(v: &CValue) -> i64 {
    match v {
        CValue::Num(n) if n.fract() == 0.0 => *n as i64,
        other => panic!("integer expected, got {other:?}"),
    }
}

fn sha(b: &[u8]) -> String {
    digest_bytes(b).to_string()
}

// ------------------------------------------------------------------ the fixture model

#[derive(Clone)]
struct Model {
    rows: Vec<StoredEntry>,
    /// (name, created_from) in creation order.
    branches: Vec<(String, Option<String>)>,
}

fn fork() -> Model {
    let v = load("fixtures/core/jrn/br/diff/fork.json");
    let f = obj(&v);
    assert_eq!(st(f, "stream"), STREAM);
    let rows = arr(f, "entries")
        .iter()
        .map(|e| {
            let e = obj(e);
            let header = st(e, "header").as_bytes().to_vec();
            let refs = arr(e, "refs")
                .iter()
                .map(|r| match r {
                    CValue::Arr(p) => match (&p[0], &p[1]) {
                        (CValue::Str(a), CValue::Str(b)) => (a.clone(), b.clone()),
                        _ => panic!(),
                    },
                    _ => panic!(),
                })
                .collect();
            StoredEntry {
                seq: int(get(e, "seq")),
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
    let branches = arr(f, "branches")
        .iter()
        .map(|b| {
            let b = obj(b);
            let cf = match get(b, "created_from") {
                CValue::Null => None,
                CValue::Str(s) => Some(s.clone()),
                other => panic!("{other:?}"),
            };
            (st(b, "name").to_string(), cf)
        })
        .collect();
    Model { rows, branches }
}

/// The branch table as `CORE.JRN.BR.NAME`'s origin-bound fold builds it: `main` at genesis,
/// human branches from `journal.create_branch@1`'s handler, agent branches from the bus
/// routing module.
fn table(m: &Model) -> Branches {
    let first = Digest::parse(&m.rows[0].hash).unwrap_or(Digest::TOMBSTONE);
    let mut b = Branches::genesis(first);
    for (name, cf) in &m.branches {
        if name == MAIN {
            continue;
        }
        let from = Digest::parse(cf.as_deref().unwrap()).unwrap();
        let name = BranchName::parse(name).unwrap();
        let agent = name.as_str().starts_with("agent/");
        let origin = EntryOrigin {
            kind: if agent { PrincipalKind::Agent } else { PrincipalKind::Human },
            command: if agent { "feature.set_nominal".into() } else { CREATE_BRANCH.into() },
            major: 1,
            class: SideEffect::W2,
            branch: if agent { name.as_str().to_string() } else { MAIN.to_string() },
            source: if agent { EventSource::BusRoute } else { EventSource::Handler },
        };
        b.apply(&origin, &BranchEvent::Created { name, from }).unwrap();
    }
    b
}

fn row_at(m: &mut Model, seq: i64) -> &mut StoredEntry {
    m.rows.iter_mut().find(|r| r.seq == seq).unwrap()
}

fn header_of(r: &StoredEntry) -> Obj {
    obj(&parse(&r.header).unwrap()).clone()
}

fn set_path(h: &mut Obj, path: &[CValue], value: CValue) {
    let mut cur: &mut CValue = h
        .get_mut(match &path[0] {
            CValue::Str(s) => s.as_str(),
            _ => panic!(),
        })
        .unwrap();
    for k in &path[1..] {
        cur = match (cur, k) {
            (CValue::Obj(o), CValue::Str(s)) => o.get_mut(s).unwrap(),
            (CValue::Arr(a), CValue::Num(n)) => &mut a[*n as usize],
            _ => panic!("bad path"),
        };
    }
    *cur = value;
}

fn store_header(r: &mut StoredEntry, h: &Obj) {
    r.header = to_canonical_bytes(&CValue::Obj(h.clone())).unwrap();
    r.header_len = r.header.len() as u64;
}

/// Re-hashes the header of `seq` after `edit` and re-chains every later row on it (prev, and
/// any logical_parent naming a rewritten hash), as an editor of the file can.
fn rewrite(m: &mut Model, seq: i64, edit: impl Fn(&mut Obj)) {
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    m.rows.sort_by_key(|r| r.seq);
    for r in m.rows.iter_mut().filter(|r| r.seq >= seq) {
        let mut h = header_of(r);
        if r.seq == seq {
            edit(&mut h);
        } else {
            let prev = st(&h, "prev").to_string();
            if let Some(n) = renamed.get(&prev) {
                h.insert("prev".into(), CValue::Str(n.clone()));
            }
        }
        let renamed_parent = match h.get("logical_parent") {
            Some(CValue::Str(lp)) => renamed.get(lp).cloned(),
            _ => None,
        };
        if let Some(n) = renamed_parent {
            h.insert("logical_parent".into(), CValue::Str(n));
        }
        let CValue::Str(old) = h.remove("hash").unwrap() else { panic!() };
        let new = sha(&to_canonical_bytes(&CValue::Obj(h.clone())).unwrap());
        h.insert("hash".into(), CValue::Str(new.clone()));
        renamed.insert(old, new.clone());
        store_header(r, &h);
        r.hash = new;
        r.prev_hash = Some(st(&h, "prev").to_string());
    }
}

fn apply(base: &Model, muts: &[CValue]) -> Model {
    let mut m = base.clone();
    for mu in muts {
        let mu = obj(mu);
        match st(mu, "op") {
            "set_header_field" => {
                let r = row_at(&mut m, int(get(mu, "seq")));
                let mut h = header_of(r);
                set_path(&mut h, arr(mu, "path"), get(mu, "value").clone());
                store_header(r, &h);
            }
            "rewrite_field" => {
                let path = arr(mu, "path").to_vec();
                let value = get(mu, "value").clone();
                rewrite(&mut m, int(get(mu, "seq")), |h| set_path(h, &path, value.clone()));
            }
            "set_column" => {
                let r = row_at(&mut m, int(get(mu, "seq")));
                let v = st(mu, "value").to_string();
                match st(mu, "column") {
                    "branch" => r.branch = v,
                    "hash" => r.hash = v,
                    "kind" => r.kind = v,
                    "stream" => r.stream = v,
                    other => panic!("column {other}"),
                }
            }
            "drop" => {
                let seq = int(get(mu, "seq"));
                m.rows.retain(|r| r.seq != seq);
            }
            "set_refs" => {
                let refs: Vec<(String, String)> = arr(mu, "refs")
                    .iter()
                    .map(|r| match r {
                        CValue::Arr(p) => match (&p[0], &p[1]) {
                            (CValue::Str(a), CValue::Str(b)) => (a.clone(), b.clone()),
                            _ => panic!(),
                        },
                        _ => panic!(),
                    })
                    .collect();
                for r in &mut m.rows {
                    r.refs = refs.clone();
                }
            }
            "set_created_from" => {
                let name = st(mu, "branch");
                for b in &mut m.branches {
                    if b.0 == name {
                        b.1 = Some(st(mu, "value").to_string());
                    }
                }
            }
            other => panic!("unknown mutation {other}"),
        }
    }
    m
}

fn rows_of(m: &Model) -> impl Iterator<Item = Result<StoredEntry, ReadError>> + '_ {
    m.rows.iter().cloned().map(Ok)
}

fn run(m: &Model, source: &str, target: &str, opts: &DiffOptions) -> Result<BranchDiff, DiffError> {
    diff_rows(STREAM, rows_of(m), &table(m), source, target, opts)
}

fn trusted_of(c: &Obj) -> Option<TrustedHead> {
    let t = obj(c.get("trusted")?);
    Some(TrustedHead {
        seq: st(t, "seq").parse().unwrap(),
        head: parse_digest(st(t, "head")).unwrap(),
        exact: matches!(get(t, "exact"), CValue::Bool(true)),
    })
}

fn check(name: &str, got: Result<BranchDiff, DiffError>, expect: &Obj) {
    match (got, expect.get("result"), expect.get("error")) {
        (Ok(d), Some(want), None) => {
            assert_eq!(&d.to_value(), want, "{name}: result differs from the oracle");
            // Canonical and deterministic: the same bytes every time.
            let b = to_canonical_bytes(&d.to_value()).unwrap();
            assert_eq!(b, to_canonical_bytes(want).unwrap(), "{name}");
        }
        (Err(e), None, Some(want)) => {
            let want = obj(want);
            assert_eq!(e.bus_code(), (st(want, "code"), st(want, "reason")), "{name}: {e}");
            match (&e, want.get("failure")) {
                (DiffError::Corrupt(f), Some(wf)) => {
                    let wf = obj(wf);
                    assert_eq!(f.seq.map(|s| s.to_string()).as_deref(), Some(st(wf, "seq")), "{name}: {f}");
                    assert_eq!(f.kind.as_str(), st(wf, "kind"), "{name}: {f}");
                    assert_eq!(f.reason, st(wf, "reason"), "{name}: {f}");
                }
                (DiffError::Corrupt(f), None) => panic!("{name}: oracle has no located failure, got {f}"),
                (other, Some(_)) => panic!("{name}: oracle has a located failure, got {other}"),
                (_, None) => {}
            }
        }
        (got, _, _) => panic!("{name}: got {got:?}, expected {expect:?}"),
    }
}

fn cases() -> Obj {
    obj(&load("fixtures/core/jrn/br/diff/cases.json")).clone()
}

fn expect_of<'a>(c: &'a Obj, list: &str, name: &str) -> &'a Obj {
    let found = arr(c, list).iter().map(obj).find(|x| st(x, "name") == name).unwrap();
    obj(get(found, "expect"))
}

// ------------------------------------------------------------------ the oracle

#[test]
fn every_oracle_diff_reaches_the_oracle_outcome() {
    let c = cases();
    let m = fork();
    let mut n = 0;
    for d in arr(&c, "diffs") {
        let d = obj(d);
        let got = run(&m, st(d, "source"), st(d, "target"), &DiffOptions::default());
        check(st(d, "name"), got, obj(get(d, "expect")));
        n += 1;
    }
    assert!(n >= 11);
}

#[test]
fn every_tamper_and_forgery_case_reaches_the_oracle_outcome() {
    let c = cases();
    let base = fork();
    let mut n = 0;
    for t in arr(&c, "tampers") {
        let t = obj(t);
        let m = apply(&base, arr(t, "mutations"));
        let opts = DiffOptions { trusted: trusted_of(t), ..DiffOptions::default() };
        check(st(t, "name"), run(&m, st(t, "source"), st(t, "target"), &opts), obj(get(t, "expect")));
        n += 1;
    }
    assert!(n >= 13);
}

/// AC-1 as the spec states it: added [e6], removed [e2], changed [e3 (conflict), e4], with
/// digests and commands; the agent's commands on `s` read as agent and the human's as human,
/// from the verified headers.
#[test]
fn ac1_fork_lists_the_spec_sets() {
    let m = fork();
    let d = run(&m, "s", "main", &DiffOptions::default()).unwrap();
    let ids = |v: &[fid_journal::diff::EntityDelta]| v.iter().map(|x| x.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&d.added), ["e6"]);
    assert_eq!(ids(&d.removed), ["e2"]);
    assert_eq!(ids(&d.changed), ["e3", "e4"]);
    assert!(d.changed[0].conflict && !d.changed[1].conflict);
    assert_eq!(d.ancestor.unwrap().as_str(), m.rows[2].hash);
    let e3 = &d.changed[0];
    assert!(e3.source_digest.is_some() && e3.target_digest.is_some() && e3.source_digest != e3.target_digest);
    assert_eq!(e3.source_commands.iter().map(|c| c.principal).collect::<Vec<_>>(), [PrincipalKind::Agent]);
    assert_eq!(e3.target_commands.iter().map(|c| c.principal).collect::<Vec<_>>(), [PrincipalKind::Human]);
    assert!(d.removed[0].source_digest.is_none() && d.removed[0].target_digest.is_some());
    assert!(d.added[0].target_digest.is_none());
    // e5 changed and changed back on the source: no net change, not listed.
    assert!(!d.changed.iter().chain(&d.added).chain(&d.removed).any(|x| x.id == "e5"));
    // The result validates against the published schema (checked in the bus crate with
    // `Compiled`; here: it equals the oracle's value).
    let c = cases();
    assert_eq!(&d.to_value(), get(expect_of(&c, "diffs", "ac1_fork"), "result"));
}

/// AC-3: payloads gone or replaced, the result equals AC-1 (headers only).
#[test]
fn ac3_without_payloads_equals_ac1() {
    let c = cases();
    let want = expect_of(&c, "diffs", "ac1_fork");
    for name in ["ac3_payloads_removed", "payload_rows_forged"] {
        assert_eq!(expect_of(&c, "tampers", name), want, "{name}");
    }
}

/// AC-4: an unknown branch, either side, is not_found/unknown_branch before any row is read.
#[test]
fn ac4_unknown_branch_is_not_found_without_reading() {
    let m = fork();
    let poisoned = std::iter::once(Err(ReadError::Storage("must not be read".into())));
    for (s, t) in [("nope", "main"), ("main", "nope"), ("Bad Name", "main"), ("", "main")] {
        let e = diff_rows(STREAM, poisoned.clone(), &table(&m), s, t, &DiffOptions::default()).unwrap_err();
        assert_eq!(e.bus_code(), ("not_found", "unknown_branch"), "{s} {t}");
    }
}

// ------------------------------------------------------------------ bounds and refusals

#[test]
fn bounds_and_cancel_are_refusals() {
    let m = fork();
    let n = m.rows.len() as u64;
    let e = run(&m, "s", "main", &DiffOptions { max_entries: n - 1, ..DiffOptions::default() }).unwrap_err();
    assert_eq!(e.bus_code(), ("validation", "input_limit"));
    run(&m, "s", "main", &DiffOptions { max_entries: n, ..DiffOptions::default() }).unwrap();
    for bytes in [0, 100, 2_000] {
        let e = run(&m, "s", "main", &DiffOptions { max_bytes: bytes, ..DiffOptions::default() }).unwrap_err();
        assert_eq!(e.bus_code(), ("validation", "input_limit"), "{bytes}");
    }
    let flag: fid_journal::diff::CancelFlag = Arc::new(|| true);
    let e = run(&m, "s", "main", &DiffOptions { cancel: Some(flag), ..DiffOptions::default() }).unwrap_err();
    assert_eq!(e.bus_code(), ("cancelled", "requested"));
    // A read error is the read error's own refusal.
    let rows = std::iter::once(Err(ReadError::Storage("disk".into())));
    let e = diff_rows(STREAM, rows, &table(&m), "s", "main", &DiffOptions::default()).unwrap_err();
    assert_eq!(e.bus_code(), ("internal", "storage"));
}

/// Rows out of order, a foreign stream and an empty journal.
#[test]
fn reordered_foreign_and_empty_journals() {
    let m = fork();
    let mut swapped = m.clone();
    swapped.rows.swap(3, 4);
    let e = run(&swapped, "s", "main", &DiffOptions::default()).unwrap_err();
    let DiffError::Corrupt(f) = e else { panic!("{e}") };
    assert_eq!((f.seq, f.kind, f.reason), (Some(4), FailureKind::Sequence, "gap"));
    let e = diff_rows("other", rows_of(&m), &table(&m), "s", "main", &DiffOptions::default()).unwrap_err();
    let DiffError::Corrupt(f) = e else { panic!("{e}") };
    assert_eq!((f.seq, f.reason), (Some(1), "foreign_stream"));
    // An empty journal: main has no head; s's created_from names no verified entry.
    let empty = Model { rows: Vec::new(), branches: m.branches.clone() };
    let t = table(&m);
    let d = diff_rows(STREAM, rows_of(&empty), &t, "main", "main", &DiffOptions::default()).unwrap();
    assert!(d.ancestor.is_none() && d.source.head.is_none() && d.added.is_empty() && d.changed.is_empty());
    let e = diff_rows(STREAM, rows_of(&empty), &t, "s", "main", &DiffOptions::default()).unwrap_err();
    assert!(matches!(e, DiffError::Tampered(_)), "{e}");
}

/// Every refusal maps to a code and reason of the frozen bus catalogue v3.
#[test]
fn every_refusal_maps_into_bus_catalogue_v3() {
    let v = parse(BUS_ERRORS_V3).unwrap();
    let cat: BTreeMap<String, BTreeSet<String>> = obj(get(obj(&v), "codes"))
        .iter()
        .map(|(c, rs)| {
            let CValue::Arr(rs) = rs else { panic!() };
            (c.clone(), rs.iter().map(|r| if let CValue::Str(s) = r { s.clone() } else { panic!() }).collect())
        })
        .collect();
    let fail = fid_journal::Failure {
        stream: STREAM.into(),
        seq: Some(1),
        kind: FailureKind::Hash,
        reason: "hash_mismatch",
        checkpoint: None,
        detail: String::new(),
    };
    for e in [
        DiffError::UnknownBranch("x".into()),
        DiffError::Corrupt(fail.clone()),
        DiffError::Tampered("x".into()),
        DiffError::InputLimit("x".into()),
        DiffError::Cancelled,
        DiffError::Read(ReadError::Storage("x".into())),
        DiffError::Read(ReadError::BadParam("x".into())),
        DiffError::from(ReadError::Cancelled),
        DiffError::from(ReadError::Corrupt(fail)),
    ] {
        let (code, reason) = e.bus_code();
        assert!(cat.get(code).is_some_and(|rs| rs.contains(reason)), "{e:?} -> {code}/{reason}");
    }
}

// ------------------------------------------------------------------ definition and schema

#[test]
fn the_command_is_declared_r0_d0_on_cli_and_mcp() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    let v = defs[0].to_value();
    let o = obj(&v);
    assert_eq!(st(o, "id"), DIFF_BRANCH);
    assert_eq!(st(o, "side_effect"), "R0");
    assert_eq!(st(o, "determinism"), "D0");
    assert_eq!(arr(o, "effects"), &[] as &[CValue]);
    let exposure: BTreeSet<&str> =
        arr(o, "exposure").iter().map(|v| if let CValue::Str(s) = v { s.as_str() } else { panic!() }).collect();
    assert_eq!(exposure, BTreeSet::from(["cli", "mcp"]));
    assert_eq!(&defs[0].params_schema, &params_schema());
    assert_eq!(&defs[0].result_schema, &result_schema());
}

/// `schema/branch-diff.schema.json` is the result schema plus `$schema`, `$id` and `title`.
#[test]
fn the_published_schema_is_the_result_schema() {
    let mut file = obj(&load("schema/branch-diff.schema.json")).clone();
    assert!(st(&file, "$schema").contains("2020-12"));
    assert!(st(&file, "$id").ends_with("/schema/branch-diff.schema.json"));
    for k in ["$schema", "$id", "title"] {
        file.remove(k).unwrap();
    }
    assert_eq!(CValue::Obj(file), result_schema());
}

// ------------------------------------------------------------------ through the store

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-diff-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

/// A project whose `entries` are the model's rows; no `entry_blobs`, no blob files (every
/// payload removed, AC-3).
fn project_of(tag: &str, m: &Model) -> (TempDir, PathBuf) {
    let t = TempDir::new(tag);
    let root = t.0.join("p");
    let store = Store::create(&root).unwrap();
    let mut txn = store.transaction();
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
    }
    txn.commit().unwrap();
    (t, root)
}

fn snapshot(store: &Store) -> Vec<String> {
    store
        .read(|conn| {
            let mut out = Vec::new();
            for sql in [
                "SELECT group_concat(seq || hash, ',') FROM (SELECT * FROM entries ORDER BY seq)",
                "SELECT count(*) || '' FROM entry_blobs",
                "SELECT count(*) || '' FROM heads",
                "SELECT count(*) || '' FROM checkpoints",
            ] {
                out.push(conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0))?.unwrap_or_default());
            }
            Ok(out)
        })
        .unwrap()
}

#[test]
fn ac1_ac3_through_the_store_without_payloads_and_nothing_is_written() {
    let c = cases();
    let m = fork();
    let (_t, root) = project_of("fork", &m);
    let store = Store::open(&root, OpenMode::Read).unwrap();
    let before = snapshot(&store);
    let d = diff_store(&store, STREAM, &table(&m), "s", "main", &DiffOptions::default()).unwrap();
    check("store ac1", Ok(d), expect_of(&c, "diffs", "ac1_fork"));
    let e = diff_store(&store, STREAM, &table(&m), "nope", "main", &DiffOptions::default()).unwrap_err();
    assert_eq!(e.bus_code(), ("not_found", "unknown_branch"));
    let e = diff_store(&store, "Bad Stream", &table(&m), "s", "main", &DiffOptions::default()).unwrap_err();
    assert_eq!(e.bus_code().0, "validation");
    assert_eq!(snapshot(&store), before, "R0: nothing written");
    drop(store);
    // The same through an open journal.
    let (loc, lock) = ProjectLocation::open(&root, OpenMode::Write).unwrap();
    let j = Journal::open(loc, lock.unwrap(), STREAM).unwrap();
    let d = j.diff_branch(&table(&m), "s", "main", &DiffOptions::default()).unwrap();
    check("journal ac1", Ok(d), expect_of(&c, "diffs", "ac1_fork"));
}

#[test]
fn a_tampered_store_row_is_refused() {
    let c = cases();
    let m = apply(&fork(), &[parse(br#"{"op":"set_column","seq":8,"column":"branch","value":"main"}"#).unwrap()]);
    let (_t, root) = project_of("tampered", &m);
    let store = Store::open(&root, OpenMode::Read).unwrap();
    let got = diff_store(&store, STREAM, &table(&m), "s", "main", &DiffOptions::default());
    check("store branch column", got, expect_of(&c, "tampers", "branch_column_forged"));
}
