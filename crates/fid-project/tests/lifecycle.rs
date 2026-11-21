#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `CORE.PRJ.CONT.LIFE`, store-free half: the identity records and the identity read (AC-6,
//! AC-7 identity cases), with the journals of `fixtures/core/prj/cont/life/make_identity_cases.py`
//! (independent stdlib reference, hand-labelled outcomes). The commands (create, open, close,
//! save-as, list) and the save-as compare-and-swap are tested in `fid-project-cmd`.

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue, Digest};
use fid_journal::entry::parse_digest;
use fid_project::lifecycle::{
    read_identity, read_record, IdentityReader, IdentityRecord, JournalEntry, Record, RecordBlobs, Refusal,
    SavedAsRecord, SourceRef, IDENTITY_EFFECTS, MAX_RECORD_BYTES, PROJECT_COPIED_FROM, PROJECT_IDENTIFIED,
    PROJECT_SAVED_AS, SAVE_PROJECT_AS,
};
use fid_project::Uuid;
use std::collections::BTreeMap;

const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/prj/cont/life/identity_cases.json");

type Obj = BTreeMap<String, CValue>;
type Blobs = BTreeMap<[u8; 32], Vec<u8>>;

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn num(o: &Obj, k: &str) -> u64 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u64,
        other => panic!("{k}: expected number, got {other:?}"),
    }
}

fn strs(o: &Obj, k: &str) -> Vec<String> {
    match o.get(k) {
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|v| match v {
                CValue::Str(s) => s.clone(),
                other => panic!("{k}: {other:?}"),
            })
            .collect(),
        other => panic!("{k}: expected array, got {other:?}"),
    }
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

struct Case {
    name: String,
    rule: String,
    entries: Vec<JournalEntry>,
    blobs: Blobs,
    expect: Obj,
}

fn cases() -> Vec<Case> {
    let doc = parse(&std::fs::read(FILE).unwrap()).unwrap();
    let CValue::Arr(cases) = &obj(&doc)["cases"] else { panic!("cases") };
    cases
        .iter()
        .map(|c| {
            let c = obj(c);
            let CValue::Arr(entries) = &c["entries"] else { panic!("entries") };
            let entries = entries
                .iter()
                .map(|e| {
                    let e = obj(e);
                    JournalEntry {
                        header: parse(st(e, "header").as_bytes()).unwrap(),
                        payload: match &e["payload"] {
                            CValue::Null => None,
                            CValue::Str(p) => Some(parse(p.as_bytes()).unwrap()),
                            other => panic!("payload {other:?}"),
                        },
                        promoted_from: match &e["promoted_from"] {
                            CValue::Null => None,
                            CValue::Str(d) => Some(digest(d)),
                            other => panic!("promoted_from {other:?}"),
                        },
                    }
                })
                .collect();
            let blobs = obj(&c["blobs"])
                .iter()
                .map(|(d, b)| {
                    let CValue::Str(b) = b else { panic!("blob") };
                    (digest(d).0, b.as_bytes().to_vec())
                })
                .collect();
            Case {
                name: st(c, "name").to_string(),
                rule: st(c, "rule").to_string(),
                entries,
                blobs,
                expect: obj(&c["expect"]).clone(),
            }
        })
        .collect()
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

/// AC-6, AC-7: every journal of the reference script reads to its hand-labelled outcome.
#[test]
fn oracle_cases_read_to_their_labels() {
    let all = cases();
    let forged = all.iter().filter(|c| c.expect.contains_key("refused")).count();
    assert!(forged >= 10, "the ten forged cases of AC-7 and more");
    assert!(all.len() > forged, "and valid journals");
    for c in &all {
        let got = read_identity(c.entries.clone(), &c.blobs);
        if let Some(want) = c.expect.get("refused") {
            let want = obj(want);
            let err = got.expect_err(&format!("{}: {} must be refused", c.name, c.rule));
            assert_eq!(err.code(), st(want, "code"), "{}: {err}", c.name);
            assert_eq!(err.seq, Some(num(want, "seq")), "{}: {err}", c.name);
            let hash = digest(st(obj(&c.entries[num(want, "seq") as usize - 1].header), "hash"));
            assert_eq!(err.entry, Some(hash), "{}: the refusal names the entry hash", c.name);
            assert!(err.to_string().contains(&format!("at entry {}", num(want, "seq"))), "{err}");
        } else {
            let want = obj(&c.expect["ok"]);
            let id = got.unwrap_or_else(|e| panic!("{}: {e}", c.name));
            assert_eq!(id.project(), uuid(st(want, "project")), "{}", c.name);
            assert_eq!(id.namespace(), uuid(st(want, "namespace")), "{}", c.name);
            assert_eq!(id.genesis.seq, num(want, "genesis_seq"), "{}", c.name);
            match &want["source"] {
                CValue::Null => assert_eq!(id.source(), None, "{}", c.name),
                src => {
                    let src = obj(src);
                    assert_eq!(
                        id.source(),
                        Some(SourceRef { project: uuid(st(src, "project")), head: digest(st(src, "head")) }),
                        "{}",
                        c.name
                    );
                }
            }
            let lineage: Vec<String> = id.lineage.iter().map(|g| g.record.project.to_string()).collect();
            assert_eq!(lineage, strs(want, "lineage"), "{}", c.name);
            let copies: Vec<String> = id.saved_as.iter().map(|s| s.record.copy.to_string()).collect();
            assert_eq!(copies, strs(want, "saved_as"), "{}", c.name);
            assert_eq!(id.head_seq, c.entries.len() as u64, "{}", c.name);
        }
    }
}

/// AC-6: the copy's genesis follows a session header whose previous entry is the source head,
/// and the source's `ProjectSavedAs` names the copy's project ID.
#[test]
fn copy_genesis_follows_its_session_header_on_the_source_head() {
    let c = case("copy_valid");
    let id = read_identity(c.entries.clone(), &c.blobs).unwrap();
    let src = id.source().unwrap();
    let session = obj(&c.entries[3].header);
    assert_eq!(st(session, "kind"), "session_header");
    assert_eq!(digest(st(session, "prev")), src.head);
    assert_eq!(digest(st(session, "hash")), id.genesis.session);
    assert_eq!(digest(st(obj(&c.entries[2].header), "hash")), src.head);
    assert_eq!(id.lineage[0].record.project, src.project);

    let s = case("source_after_save_as");
    let id = read_identity(s.entries.clone(), &s.blobs).unwrap();
    assert_eq!(id.saved_as.len(), 1);
    assert_eq!(id.saved_as[0].record.project, id.project());
    assert_eq!(id.saved_as[0].record.head, digest(st(obj(&s.entries[2].header), "hash")));
}

/// Mutations of the valid journals (the mutation is the input; the outcome is labelled from
/// the rules): a removed payload, a broken chain, events that do not reproduce the
/// commitment, a truncated journal.
#[test]
fn mutated_journals_fail_closed_naming_the_entry() {
    let c = case("create_valid");
    let read = |entries: Vec<JournalEntry>| read_identity(entries, &c.blobs);

    let mut e = c.entries.clone();
    e[1].payload = None;
    let err = read(e).unwrap_err();
    assert_eq!((err.refusal, err.seq), (Refusal::PayloadMissing, Some(2)));

    // A document command whose payload was removed is passed over.
    let mut e = c.entries.clone();
    e[2].payload = None;
    assert!(read(e).is_ok());

    let mut e = c.entries.clone();
    e.remove(2);
    let err = read(e).unwrap_err();
    assert_eq!((err.refusal, err.seq), (Refusal::Chain, Some(4)));

    let mut e = c.entries.clone();
    let CValue::Obj(p) = e[1].payload.as_mut().unwrap() else { panic!() };
    let CValue::Arr(evs) = p.get_mut("events").unwrap() else { panic!() };
    evs.clear();
    let err = read(e).unwrap_err();
    assert_eq!((err.refusal, err.seq), (Refusal::PayloadMismatch, Some(2)));

    let mut e = c.entries.clone();
    let CValue::Obj(h) = &mut e[1].header else { panic!() };
    h.insert("branch".into(), CValue::Str("side".into()));
    let err = read(e).unwrap_err();
    assert_eq!((err.refusal, err.seq, err.entry), (Refusal::Chain, Some(2), None), "hash no longer verifies");

    let err = read(c.entries[..1].to_vec()).unwrap_err();
    assert_eq!((err.refusal, err.seq), (Refusal::NoGenesis, Some(1)));
    let err = read(Vec::new()).unwrap_err();
    assert_eq!((err.refusal, err.seq, err.entry), (Refusal::NoGenesis, None, None));

    // Without the record blob the create entry has no record: one output, no record.
    let err = read_identity(c.entries.clone(), &Blobs::new()).unwrap_err();
    assert_eq!((err.refusal, err.seq), (Refusal::BadShape, Some(2)));
}

/// The reader stops at the first refusal and reports progress before it.
#[test]
fn reader_is_incremental() {
    let c = case("copy_valid");
    let mut r = IdentityReader::new(&c.blobs);
    assert!(r.current().is_none());
    for (i, e) in c.entries.iter().enumerate() {
        r.push(e).unwrap();
        if i == 1 {
            assert_eq!(r.current().unwrap().seq, 2);
        }
    }
    assert_eq!(r.head(), digest(st(obj(&c.entries[4].header), "hash")));
    assert_eq!(r.finish().unwrap().genesis.seq, 5);
}

const P1: &str = "018f3a10-0000-7000-8000-000000000001";
const P2: &str = "018f3a10-0000-7000-8000-000000000002";
const NS: &str = "6f1c2a4e-5b3d-4c2a-9e1f-00000000000a";

fn head() -> Digest {
    digest_bytes(b"head")
}

#[test]
fn records_round_trip_in_canonical_form() {
    let create = IdentityRecord::create(uuid(P1), uuid(NS)).unwrap();
    let copy = IdentityRecord::copy(uuid(P2), uuid(NS), SourceRef { project: uuid(P1), head: head() }).unwrap();
    let saved = SavedAsRecord::new(uuid(P1), uuid(P2), head()).unwrap();
    for r in [Record::Identity(create), Record::Identity(copy), Record::SavedAs(saved)] {
        let bytes = to_canonical_bytes(&r.to_value()).unwrap();
        assert_eq!(read_record(&bytes).unwrap(), Some(r));
    }
    assert_eq!(create.digest(), digest_bytes(&create.to_bytes()));
    assert_eq!(saved.digest(), digest_bytes(&saved.to_bytes()));
    assert_eq!(
        String::from_utf8(create.to_bytes()).unwrap(),
        format!(r#"{{"format":"fid-project-identity/1","namespace":"{NS}","project":"{P1}"}}"#)
    );
    assert_eq!(
        String::from_utf8(saved.to_bytes()).unwrap(),
        format!(r#"{{"copy":"{P2}","format":"fid-project-saved-as/1","head":"{}","project":"{P1}"}}"#, head())
    );
}

#[test]
fn record_constructors_and_reader_refuse_bad_ids() {
    let v4 = uuid(NS);
    assert!(IdentityRecord::create(v4, v4).is_err(), "project must be a UUIDv7");
    assert!(IdentityRecord::create(uuid(P1), Uuid::NIL).is_err(), "nil namespace");
    assert!(IdentityRecord::copy(uuid(P1), v4, SourceRef { project: uuid(P1), head: head() }).is_err());
    assert!(SavedAsRecord::new(uuid(P1), uuid(P1), head()).is_err());

    let not_records: [&[u8]; 5] = [b"", b"not json", b"[1]", br#"{"a":1}"#, br#"{"format":"other/1"}"#];
    for b in not_records {
        assert_eq!(read_record(b).unwrap(), None, "{:?}", String::from_utf8_lossy(b));
    }
    let big = vec![b' '; MAX_RECORD_BYTES + 1];
    assert_eq!(read_record(&big).unwrap(), None);

    let malformed = [
        format!(r#"{{"format":"fid-project-identity/1","namespace":"{NS}","project":"{}"}}"#, P1.to_uppercase()),
        format!(r#"{{"format":"fid-project-identity/1","namespace":"{NS}","project":"{P1}","x":1}}"#),
        format!(r#"{{"format":"fid-project-identity/1","namespace":"{NS}"}}"#),
        format!(r#"{{"namespace":"{NS}","format":"fid-project-identity/1","project":"{P1}"}}"#),
        format!(r#"{{"format":"fid-project-identity/2","namespace":"{NS}","project":"{P1}"}}"#),
        format!(r#"{{"format":"fid-project-saved-as/1","copy":"{P1}","head":"{}","project":"{P1}"}}"#, head()),
        format!(r#"{{"format":"fid-project-identity/1","namespace":"{NS}","project":"{P1}","source":"x"}}"#),
    ];
    for m in malformed {
        assert!(read_record(m.as_bytes()).is_err(), "{m}");
    }
}

#[test]
fn effects_and_owners() {
    assert_eq!(
        IDENTITY_EFFECTS,
        [
            (PROJECT_COPIED_FROM, SAVE_PROJECT_AS),
            (PROJECT_IDENTIFIED, "project.create_project"),
            (PROJECT_SAVED_AS, SAVE_PROJECT_AS)
        ]
    );
    assert_eq!(Refusal::StaleHead.code(), "stale_head");
}

/// A blob source that fails is `storage`, never success.
#[test]
fn unreadable_blob_store_fails_closed() {
    struct Broken;
    impl RecordBlobs for Broken {
        fn object(&self, _: &Digest, _: usize) -> Result<Option<Vec<u8>>, String> {
            Err("disk gone".into())
        }
    }
    let c = case("create_valid");
    let err = read_identity(c.entries, Broken).unwrap_err();
    assert_eq!((err.refusal, err.seq), (Refusal::Storage, Some(2)));
}
