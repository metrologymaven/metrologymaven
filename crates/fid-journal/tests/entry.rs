//! Acceptance criteria of spec node CORE.JRN.ENTRY (AC-1 .. AC-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_journal::{
    build, commitment, parse_digest, session_start, validate_header_value, verify_commitment, verify_header_value,
    AiAssistance, CommitError, DraftBody, Entry, EntryDraft, EntryKind, EntryTime, Field, HeaderBody, Manifests,
    OsSalts, OutputDigest, PrincipalDraft, PrincipalKind, RecordedSalts, Salt, SessionInfo, TaggedDigest,
    R1_RESULT_TAG, TOMBSTONE,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/jrn/entry").join(name)
}

type Obj = BTreeMap<String, CValue>;

fn load(name: &str) -> Vec<Obj> {
    let CValue::Arr(items) = parse(&std::fs::read(fixture(name)).unwrap()).unwrap() else { panic!("array expected") };
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

fn ob<'a>(o: &'a Obj, k: &str) -> &'a Obj {
    match o.get(k) {
        Some(CValue::Obj(m)) => m,
        other => panic!("field {k} is {other:?}"),
    }
}

fn canon(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn recorded_salts(vec: &Obj) -> RecordedSalts {
    RecordedSalts(
        ob(vec, "salts")
            .iter()
            .map(|(k, v)| {
                let CValue::Str(hex) = v else { panic!("salt must be a string") };
                (Field::parse(k).unwrap(), Salt::from_hex(hex).unwrap())
            })
            .collect(),
    )
}

/// Rebuilds a fixture entry from its draft and recorded salts.
fn rebuild(vec: &Obj) -> Entry {
    let draft =
        EntryDraft::from_value(vec.get("draft").unwrap()).unwrap_or_else(|e| panic!("{}: {e}", st(vec, "name")));
    let seq: u64 = st(vec, "seq").parse().unwrap();
    let prev = match vec.get("prev") {
        Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(parse_digest(s).unwrap()),
        other => panic!("prev is {other:?}"),
    };
    build(&draft, seq, prev, &mut recorded_salts(vec)).unwrap_or_else(|e| panic!("{}: {e}", st(vec, "name")))
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

/// AC-1: every hash and commitment of the independent reference matches; every kind is present.
#[test]
fn ac1_vectors_match_reference() {
    let vectors = load("vectors.json");
    assert!(vectors.len() >= 30, "need at least 30 vectors, have {}", vectors.len());
    let mut kinds = Vec::new();
    for vec in &vectors {
        let name = st(vec, "name");
        let entry = rebuild(vec);
        assert_eq!(canon(&entry.header.to_value()), canon(vec.get("header").unwrap()), "{name}: header");
        assert_eq!(entry.header.hash.to_string(), st(vec, "hash"), "{name}: hash");
        assert_eq!(canon(&entry.payload.to_value()), canon(vec.get("payload").unwrap()), "{name}: payload");
        for (field, want) in ob(vec, "commitments") {
            let f = Field::parse(field).unwrap();
            let got = entry.header.commitment(f).unwrap_or_else(|| panic!("{name}: no commitment for {field}"));
            let CValue::Str(want) = want else { panic!() };
            assert_eq!(&got.to_string(), want, "{name}: commitment {field}");
        }
        assert_eq!(verify_header_value(vec.get("header").unwrap()).unwrap(), entry.header.hash, "{name}");
        kinds.push(entry.header.kind());
    }
    for k in EntryKind::ALL {
        assert!(kinds.contains(k), "no vector of kind {k}");
    }
}

/// AC-2: without its payload the header still verifies, and commitments fail only for lack of salt and value.
#[test]
fn ac2_payload_removed() {
    let vectors = load("vectors.json");
    for vec in &vectors {
        let name = st(vec, "name");
        // Keep the payload aside, as if it had been deleted from the store.
        let Entry { header, payload: recorded } = rebuild(vec);
        // The header alone, as stored, still verifies.
        header.verify_hash().unwrap();
        assert_eq!(verify_header_value(&header.to_value()).unwrap(), header.hash, "{name}");
        for f in Field::ALL {
            match header.commitment(*f) {
                None => assert_eq!(verify_commitment(&header, *f, None, None), Err(CommitError::NotCommitted)),
                Some(_) => {
                    assert_eq!(verify_commitment(&header, *f, None, None), Err(CommitError::PayloadMissing), "{name}");
                    // Only the missing salt and value were the problem.
                    let (salt, value) = (recorded.salts.get(f), recorded.values.get(f));
                    assert_eq!(verify_commitment(&header, *f, salt, value), Ok(()), "{name}: {f}");
                    let wrong = CValue::Str("tampered".into());
                    assert_eq!(verify_commitment(&header, *f, salt, Some(&wrong)), Err(CommitError::Mismatch));
                }
            }
        }
    }
}

/// AC-3: missing `ai_assistance` or the value `maybe` fails with `schema`, in drafts and in headers;
/// every hand-labelled invalid header is refused.
#[test]
fn ac3_schema_errors() {
    let vectors = load("vectors.json");
    let CValue::Obj(draft) = vectors[1].get("draft").unwrap().clone() else { panic!() };
    let mut missing = draft.clone();
    missing.remove("ai_assistance");
    assert_eq!(EntryDraft::from_value(&CValue::Obj(missing)).unwrap_err().code(), "schema");
    let mut maybe = draft;
    maybe.insert("ai_assistance".into(), CValue::Str("maybe".into()));
    assert_eq!(EntryDraft::from_value(&CValue::Obj(maybe)).unwrap_err().code(), "schema");

    let invalid = load("invalid_headers.json");
    assert!(invalid.len() >= 20);
    for case in &invalid {
        let e = validate_header_value(case.get("header").unwrap())
            .err()
            .unwrap_or_else(|| panic!("{} should not validate", st(case, "name")));
        assert_eq!(e.code(), "schema", "{}", st(case, "name"));
    }
    for name in ["missing_ai_assistance", "ai_assistance_maybe"] {
        assert!(invalid.iter().any(|c| st(c, "name") == name), "missing case {name}");
    }

    // A typed draft whose header breaks the schema fails to build.
    let mut bad = EntryDraft::from_value(vectors[1].get("draft").unwrap()).unwrap();
    bad.stream = "Record Journal".into();
    assert_eq!(build(&bad, 1, None, &mut OsSalts).unwrap_err().code(), "schema");
    // A non-finite committed value fails with the canon error.
    let mut nan = EntryDraft::from_value(vectors[1].get("draft").unwrap()).unwrap();
    nan.principal.identity = CValue::Num(f64::NAN);
    assert_eq!(build(&nan, 1, None, &mut OsSalts).unwrap_err().code(), "non_finite_number");
}

/// AC-4: the assertion pre-image is the hash of the same header with `human_assertion` empty.
#[test]
fn ac4_human_assertion_preimage() {
    let vectors = load("vectors.json");
    let mut seen = 0;
    for vec in &vectors {
        let entry = rebuild(vec);
        let pre = entry.header.assertion_preimage().unwrap();
        assert_eq!(pre.to_string(), st(vec, "assertion_preimage"), "{}", st(vec, "name"));
        if entry.header.human_assertion.is_some() {
            seen += 1;
            let mut bare = entry.header.clone();
            bare.human_assertion = None;
            assert_eq!(pre, bare.compute_hash().unwrap());
            assert_ne!(pre, entry.header.hash);
        } else {
            assert_eq!(pre, entry.header.hash);
        }
    }
    assert!(seen > 0, "no vector carries a human assertion");
}

/// AC-5: a session starts with a `session_header` entry holding the manifests and app version.
#[test]
fn ac5_session_header_first() {
    let manifests = Manifests { registry: digest(&format!("sha256:{}", "ab".repeat(32))), kernel: TOMBSTONE };
    let draft = session_start(
        "record",
        "main",
        EntryTime { utc: "2026-11-17T08:00:00Z".into(), mono_ns: 5, sync: "synced".into() },
        PrincipalDraft { kind: PrincipalKind::Script, identity: CValue::Str("op-17".into()), on_behalf_of: None },
        AiAssistance::None,
        manifests,
        SessionInfo { session_id: "s-1".into(), app_version: "0.0.1".into() },
    );
    let first = build(&draft, 0, None, &mut OsSalts).unwrap();
    assert_eq!(first.header.kind(), EntryKind::SessionHeader);
    assert_eq!(first.header.seq, 0);
    assert_eq!(first.header.prev, None);
    assert_eq!(first.header.manifests, manifests);
    let HeaderBody::SessionHeader(info) = &first.header.body else { panic!("session body expected") };
    assert_eq!(info.app_version, "0.0.1");
    let CValue::Obj(v) = first.header.to_value() else { panic!() };
    assert_eq!(st(ob(&v, "session"), "app_version"), "0.0.1");
    assert_eq!(st(ob(&v, "manifests"), "registry"), manifests.registry.to_string());
    // The fixture's chain also starts with one.
    let vectors = load("vectors.json");
    assert_eq!(st(ob(&vectors[0], "header"), "kind"), "session_header");
}

/// AC-6: consumed R1 result digests appear among the inputs tagged `r1_result`; deletions use the tombstone.
#[test]
fn ac6_consumed_r1_and_tombstone() {
    let vectors = load("vectors.json");
    let mut draft = None;
    for vec in &vectors {
        let d = EntryDraft::from_value(vec.get("draft").unwrap()).unwrap();
        if let DraftBody::Command(c) = &d.body {
            if c.class == fid_registry::SideEffect::W2 && !c.consumed_r1.is_empty() {
                draft = Some(d);
                break;
            }
        }
    }
    let mut draft = draft.expect("a W2 vector with consumed_r1");
    let r1 = digest(&format!("sha256:ab{}", "cd".repeat(31)));
    let DraftBody::Command(c) = &mut draft.body else { panic!() };
    c.consumed_r1 = vec![r1];
    c.outputs.push(OutputDigest::deleted("F-9999"));
    let entry = build(&draft, 7, None, &mut OsSalts).unwrap();
    let HeaderBody::Command(h) = &entry.header.body else { panic!() };
    assert!(h.inputs.contains(&TaggedDigest { tag: R1_RESULT_TAG.into(), digest: r1 }));
    assert!(h.outputs.contains(&OutputDigest { entity: Some("F-9999".into()), digest: TOMBSTONE }));
    assert_eq!(TOMBSTONE.to_string(), format!("sha256:{}", "0".repeat(64)));
}

/// AC-7: rebuilding a recorded entry from its draft and recorded salts gives the identical hash.
#[test]
fn ac7_replay_with_recorded_salts() {
    let vectors = load("vectors.json");
    for vec in &vectors {
        let draft = EntryDraft::from_value(vec.get("draft").unwrap()).unwrap();
        let recorded = build(&draft, 3, None, &mut OsSalts).unwrap();
        let replayed = build(&draft, 3, None, &mut recorded.payload.recorded_salts()).unwrap();
        assert_eq!(replayed.header.hash, recorded.header.hash, "{}", st(vec, "name"));
        assert_eq!(replayed, recorded);
        // Fresh salts give a different commitment, so salts are really drawn.
        let fresh = build(&draft, 3, None, &mut OsSalts).unwrap();
        assert_ne!(fresh.header.principal.commit, recorded.header.principal.commit);
        // Replay without the recorded salts is refused, not silently re-salted.
        assert_eq!(build(&draft, 3, None, &mut RecordedSalts::default()).unwrap_err().code(), "missing_salt");
    }
}

/// The commitment is SHA-256 over the 16 salt bytes then the canonical bytes (REQ-003).
#[test]
fn commitment_layout() {
    let salt = Salt([0x11; 16]);
    let value = CValue::Str("x".into());
    let mut bytes = vec![0x11u8; 16];
    bytes.extend_from_slice(b"\"x\"");
    assert_eq!(commitment(&salt, &value).unwrap(), fid_canon::digest_bytes(&bytes));
    assert_eq!(Salt::from_hex(&salt.to_hex()).unwrap(), salt);
    assert!(Salt::from_hex("11").is_err());
    assert!(Salt::from_hex(&"A".repeat(32)).is_err());
}
