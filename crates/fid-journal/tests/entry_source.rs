//! Schema version 2 of spec node CORE.JRN.ENTRY (REQ-013 .. REQ-015): the event source of
//! every event and `agent_coedited` in the hashed header, against the independent reference
//! `fixtures/core/jrn/entry/make_vectors.py`, and the branch fold reading the recorded source.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::branch::{BranchEvent, Branches, EntryOrigin, EventSource};
use fid_journal::{
    build, parse_digest, DraftBody, Entry, EntryDraft, EntryHeader, Field, HeaderBody, RecordedSalts, Salt, SCHEMA_V1,
    SCHEMA_V2,
};
use std::collections::BTreeMap;
use std::path::Path;

type Obj = BTreeMap<String, CValue>;

/// Vectors 0..36 are the version 1 vectors of the first build; version 2 cases follow.
const V1_VECTORS: usize = 36;

fn load(name: &str) -> Vec<Obj> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/jrn/entry").join(name);
    let CValue::Arr(items) = parse(&std::fs::read(path).unwrap()).unwrap() else { panic!("array expected") };
    items
        .into_iter()
        .map(|v| match v {
            CValue::Obj(o) => o,
            other => panic!("object expected, got {other:?}"),
        })
        .collect()
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("field {k} is {other:?}"),
    }
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

fn salts(vec: &Obj) -> RecordedSalts {
    let Some(CValue::Obj(s)) = vec.get("salts") else { panic!("salts") };
    RecordedSalts(
        s.iter()
            .map(|(k, v)| {
                let CValue::Str(hex) = v else { panic!("salt must be a string") };
                (Field::parse(k).unwrap(), Salt::from_hex(hex).unwrap())
            })
            .collect(),
    )
}

fn draft(vec: &Obj) -> EntryDraft {
    EntryDraft::from_value(vec.get("draft").unwrap()).unwrap_or_else(|e| panic!("{}: {e}", st(vec, "name")))
}

fn rebuild_from(vec: &Obj, d: &EntryDraft) -> Entry {
    let seq: u64 = st(vec, "seq").parse().unwrap();
    build(d, seq, parse_digest(st(vec, "prev")).unwrap(), &mut salts(vec)).unwrap()
}

fn header_obj(vec: &Obj) -> &Obj {
    let Some(CValue::Obj(h)) = vec.get("header") else { panic!("header") };
    h
}

fn events(d: &EntryDraft) -> Vec<CValue> {
    let DraftBody::Command(c) = &d.body else { panic!("command draft expected") };
    c.events.clone()
}

fn branch_event(v: &CValue) -> BranchEvent {
    let CValue::Obj(o) = v else { panic!("event envelope") };
    let Some(CValue::Str(t)) = o.get("type") else { panic!("type") };
    BranchEvent::from_parts(t, o.get("body").unwrap()).unwrap()
}

fn by_name<'a>(vectors: &'a [Obj], name: &str) -> &'a Obj {
    vectors.iter().find(|v| st(v, "name") == name).unwrap_or_else(|| panic!("no vector {name}"))
}

/// Old entries: the version 1 vectors are still version 1, carry neither new field, read back
/// byte for byte and record every event as the handler's, not co-edited.
#[test]
fn old_entries_verify_unchanged() {
    let vectors = load("vectors.json");
    for vec in &vectors[..V1_VECTORS] {
        let name = st(vec, "name");
        let h = header_obj(vec);
        assert_eq!(h.get("schema_version"), Some(&CValue::Num(1.0)), "{name}");
        assert!(!h.contains_key("agent_coedited"), "{name}");
        if let Some(CValue::Obj(c)) = h.get("command") {
            assert!(!c.contains_key("event_sources"), "{name}");
        }
        let header = EntryHeader::from_value(vec.get("header").unwrap()).unwrap();
        assert_eq!(canon(&header.to_value()), canon(vec.get("header").unwrap()), "{name}");
        assert_eq!(header.hash.to_string(), st(vec, "hash"), "{name}");
        assert_eq!(header.schema_version(), SCHEMA_V1, "{name}");
        assert!(!header.agent_coedited, "{name}");
        if let HeaderBody::Command(_) = header.body {
            let n = events(&draft(vec)).len();
            assert_eq!(header.event_sources(n).unwrap(), vec![EventSource::Handler; n], "{name}");
        }
        // Rebuilt from the old draft (no source, no flag), the header is the recorded one.
        assert_eq!(rebuild_from(vec, &draft(vec)).header, header, "{name}");
    }
}

/// New entries round-trip: draft -> build -> header value -> `EntryHeader::from_value` gives
/// the same header, the same bytes as the Python reference and the recorded sources.
#[test]
fn new_entries_round_trip() {
    let vectors = load("vectors.json");
    assert!(vectors.len() > V1_VECTORS);
    let mut v2 = 0;
    for vec in &vectors[V1_VECTORS..] {
        let name = st(vec, "name");
        let d = draft(vec);
        let built = rebuild_from(vec, &d).header;
        assert_eq!(canon(&built.to_value()), canon(vec.get("header").unwrap()), "{name}: header bytes");
        assert_eq!(built.hash.to_string(), st(vec, "hash"), "{name}: hash");
        let read = EntryHeader::from_value(&built.to_value()).unwrap();
        assert_eq!(read, built, "{name}: round trip");
        assert_eq!(read.agent_coedited, d.agent_coedited, "{name}");
        if let DraftBody::Command(c) = &d.body {
            let want = if c.event_sources.is_empty() {
                vec![EventSource::Handler; c.events.len()]
            } else {
                c.event_sources.clone()
            };
            assert_eq!(read.event_sources(c.events.len()).unwrap(), want, "{name}");
            if read.schema_version() == SCHEMA_V2 {
                assert!(read.event_sources(c.events.len() + 1).is_err(), "{name}: count is checked");
            }
        }
        if read.schema_version() == SCHEMA_V2 {
            v2 += 1;
            assert!(header_obj(vec).contains_key("agent_coedited"), "{name}");
        }
    }
    assert!(v2 >= 8, "need version 2 vectors of every shape, have {v2}");
    // A draft that names every source and all are the handler's is still version 1.
    let plain = by_name(&vectors, "44_v2_command");
    assert_eq!(header_obj(plain).get("schema_version"), Some(&CValue::Num(1.0)));
}

/// Building refuses a source list that is not one per event; reading refuses a tampered source.
#[test]
fn sources_are_bound_to_events_and_hash() {
    let vectors = load("vectors.json");
    let routed = by_name(&vectors, "36_v2_command");
    let mut d = draft(routed);
    let DraftBody::Command(c) = &mut d.body else { panic!() };
    c.event_sources.push(EventSource::BusRoute);
    assert_eq!(
        build(&d, 1, parse_digest(st(routed, "prev")).unwrap(), &mut salts(routed)).unwrap_err().code(),
        "schema"
    );

    // Rewriting a recorded source without rehashing is a hash mismatch.
    let mut h = header_obj(routed).clone();
    let Some(CValue::Obj(cmd)) = h.get_mut("command") else { panic!() };
    cmd.insert(
        "event_sources".into(),
        CValue::Arr(vec![CValue::Str("bus_route".into()), CValue::Str("bus_route".into())]),
    );
    assert_eq!(EntryHeader::from_value(&CValue::Obj(h)).unwrap_err().code(), "hash_mismatch");
}

/// The fold applies the recorded source: an agent `BranchCreated` folds only when the header
/// records `bus_route` for it. The same event in an entry that records it as the handler's
/// (a handler claiming to be the route stage) is refused.
#[test]
fn forged_source_is_refused_by_the_fold() {
    let vectors = load("vectors.json");
    let routed = by_name(&vectors, "36_v2_command");
    let header = EntryHeader::from_value(routed.get("header").unwrap()).unwrap();
    let evs = events(&draft(routed));
    let origins = EntryOrigin::of_entry(&header, evs.len()).unwrap();
    assert_eq!(origins[1].source, EventSource::BusRoute);
    assert_eq!(origins[0].source, EventSource::Handler);
    let created = branch_event(&evs[1]);
    let genesis = Branches::genesis(fid_types::Digest::parse(st(routed, "prev")).unwrap());
    let mut b = genesis.clone();
    b.apply(&origins[1], &created).unwrap();
    assert!(b.get("agent/claude-code/9").is_some());

    // Forged: the same events, recorded as the handler's (a version 1 entry).
    let mut forged = draft(routed);
    let DraftBody::Command(c) = &mut forged.body else { panic!() };
    c.event_sources.clear();
    let forged = rebuild_from(routed, &forged).header;
    assert_eq!(forged.schema_version(), SCHEMA_V1);
    let origin = EntryOrigin::of_event(&forged, 1).unwrap();
    assert_eq!(origin.source, EventSource::Handler);
    let e = genesis.clone().apply(&origin, &created).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("internal", "invariant_violated"));
    // Index 0 (the handler's own event) is the handler's in both versions.
    assert_eq!(EntryOrigin::of_event(&header, 0).unwrap().source, EventSource::Handler);
    assert!(EntryOrigin::of_event(&header, 2).is_err());
}

/// Release protection (CORE.JRN.BR.NAME REQ-006) folds only from a recorded `bus_release`
/// source on a W3 entry on `main`.
#[test]
fn release_protection_uses_the_recorded_source() {
    let vectors = load("vectors.json");
    let release = by_name(&vectors, "38_v2_command");
    let header = EntryHeader::from_value(release.get("header").unwrap()).unwrap();
    let evs = events(&draft(release));
    let protect = branch_event(&evs[1]);
    let origin = EntryOrigin::of_event(&header, 1).unwrap();
    assert_eq!(origin.source, EventSource::BusRelease);
    let genesis = Branches::genesis(fid_types::Digest::parse(st(release, "prev")).unwrap());
    let mut b = genesis.clone();
    b.apply(&origin, &protect).unwrap();
    assert_eq!(b.is_protected("main"), Some(true));
    // The handler cannot add it.
    let handler = EntryOrigin::of_event(&header, 0).unwrap();
    assert_eq!(handler.source, EventSource::Handler);
    assert!(genesis.clone().apply(&handler, &protect).is_err());
}

/// `agent_coedited` is in the hashed header of version 2 and false in version 1.
#[test]
fn agent_coedited_is_recorded() {
    let vectors = load("vectors.json");
    for name in ["37_v2_command", "40_v2_command", "41_v2_command", "42_v2_session_header", "43_v2_head_move"] {
        let vec = by_name(&vectors, name);
        let header = EntryHeader::from_value(vec.get("header").unwrap()).unwrap();
        assert!(header.agent_coedited, "{name}");
        assert_eq!(header.schema_version(), SCHEMA_V2, "{name}");
        // Flipping the flag changes the entry hash.
        let mut flipped = header.clone();
        flipped.agent_coedited = false;
        assert_ne!(flipped.compute_hash().unwrap(), header.hash, "{name}");
    }
    // Non-command entries carry no events.
    let ses = by_name(&vectors, "42_v2_session_header");
    let header = EntryHeader::from_value(ses.get("header").unwrap()).unwrap();
    assert_eq!(header.event_source(0).unwrap_err().code(), "schema");
}
