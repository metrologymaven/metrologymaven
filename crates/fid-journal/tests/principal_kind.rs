//! `fid_journal::PrincipalKind` is `fid_types::PrincipalKind` (one type, the six wire strings
//! of CORE.BUS.PRIN), and no path through the journal turns one principal kind into another:
//! in particular a `script` entry never reads back as `human`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::{
    build, session_start, AiAssistance, EntryDraft, EntryError, EntryHeader, EntryTime, Manifests, OsSalts,
    PrincipalDraft, PrincipalKind, SessionInfo, FIRST_SEQ, GENESIS_PREV, TOMBSTONE,
};
use std::any::TypeId;
use std::collections::BTreeMap;
use std::path::Path;

type Obj = BTreeMap<String, CValue>;

/// Hand-labelled from REQ-CORE.BUS.PRIN-001 and `schema/entry.schema.json`, in declaration order.
const WIRE: [&str; 6] = ["human", "agent", "script", "service", "device", "system"];

/// Near misses of a valid wire string; each must be refused, never mapped to a kind.
const BAD: [&str; 9] = ["", "Human", "HUMAN", "human ", " human", "user", "person", "Script", "scripts"];

fn kind_of(s: &str) -> PrincipalKind {
    PrincipalKind::ALL.iter().copied().find(|k| k.as_str() == s).unwrap()
}

fn header_for(kind: PrincipalKind) -> EntryHeader {
    let draft = session_start(
        "record",
        "main",
        EntryTime { utc: "2026-11-25T08:00:00Z".into(), mono_ns: 1, sync: "synced".into() },
        PrincipalDraft { kind, identity: CValue::Str("op-1".into()), on_behalf_of: None },
        AiAssistance::None,
        Manifests { registry: TOMBSTONE, kernel: TOMBSTONE },
        SessionInfo { session_id: "s-1".into(), app_version: "0.0.1".into() },
    );
    build(&draft, FIRST_SEQ, GENESIS_PREV, &mut OsSalts).unwrap().header
}

fn principal_mut(v: &mut CValue) -> &mut Obj {
    let CValue::Obj(o) = v else { panic!("object expected") };
    let Some(CValue::Obj(p)) = o.get_mut("principal") else { panic!("principal object expected") };
    p
}

/// The first fixture draft (a `session_header`), with `principal.kind` replaced.
fn fixture_draft(kind: &str) -> CValue {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/jrn/entry/vectors.json");
    let CValue::Arr(items) = parse(&std::fs::read(path).unwrap()).unwrap() else { panic!("array expected") };
    let CValue::Obj(first) = &items[0] else { panic!("object expected") };
    let mut draft = first["draft"].clone();
    principal_mut(&mut draft).insert("kind".into(), CValue::Str(kind.into()));
    draft
}

#[test]
fn journal_kind_is_the_shared_type() {
    assert_eq!(TypeId::of::<PrincipalKind>(), TypeId::of::<fid_types::PrincipalKind>());
    let shared: fid_types::PrincipalKind = PrincipalKind::Script;
    assert_eq!(shared, fid_types::PrincipalKind::Script);
}

#[test]
fn every_wire_string_round_trips() {
    let wire: Vec<&str> = PrincipalKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(wire, WIRE);
    for k in PrincipalKind::ALL {
        assert_eq!(PrincipalKind::parse(k.as_str()), Ok(*k));
        assert_eq!(k.to_string(), k.as_str());
    }
}

#[test]
fn every_kind_round_trips_through_a_header() {
    for &k in PrincipalKind::ALL {
        let header = header_for(k);
        let mut v = header.to_value();
        assert_eq!(principal_mut(&mut v)["kind"], CValue::Str(k.as_str().into()), "{k}");
        let back = EntryHeader::from_value(&v).unwrap();
        assert_eq!(back.principal.kind, k, "{k}");
        assert_eq!(back, header, "{k}");
        assert_eq!(to_canonical_bytes(&back.to_value()).unwrap(), to_canonical_bytes(&v).unwrap(), "{k}");
    }
}

#[test]
fn every_kind_round_trips_through_a_draft() {
    for s in WIRE {
        let draft = EntryDraft::from_value(&fixture_draft(s)).unwrap();
        assert_eq!(draft.principal.kind, kind_of(s));
        assert_eq!(draft.principal.kind.as_str(), s);
    }
}

#[test]
fn near_misses_are_schema_errors_with_the_old_message() {
    for bad in BAD {
        assert!(PrincipalKind::parse(bad).is_err(), "{bad:?}");
        let e = EntryDraft::from_value(&fixture_draft(bad)).unwrap_err();
        assert_eq!(e, EntryError::Schema(format!("principal.kind: unknown value {bad:?}")), "{bad:?}");
        let mut v = header_for(PrincipalKind::Script).to_value();
        principal_mut(&mut v).insert("kind".into(), CValue::Str(bad.into()));
        assert_eq!(EntryHeader::from_value(&v).unwrap_err().code(), "schema", "{bad:?}");
    }
}

#[test]
fn relabelling_a_recorded_kind_breaks_the_hash() {
    // No kind can be swapped for another in a recorded header, and above all no script
    // (or any other non-human kind) can be turned into `human`.
    for &from in PrincipalKind::ALL {
        let header = header_for(from);
        for &to in PrincipalKind::ALL.iter().filter(|t| **t != from) {
            let mut v = header.to_value();
            principal_mut(&mut v).insert("kind".into(), CValue::Str(to.as_str().into()));
            assert_eq!(EntryHeader::from_value(&v).unwrap_err(), EntryError::HashMismatch, "{from} -> {to}");
        }
    }
    let script = EntryHeader::from_value(&header_for(PrincipalKind::Script).to_value()).unwrap();
    assert_eq!(script.principal.kind, PrincipalKind::Script);
    assert_ne!(script.principal.kind, PrincipalKind::Human);
}
