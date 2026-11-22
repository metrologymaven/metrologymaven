//! Acceptance and unit tests for `UNC.RDL.TRACIM.ARCH` (`fid_rdl::tracim::archive`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use fid_canon::{parse, CValue};
use fid_rdl::record::{BlobRef, HexF64, Kind, RefOutput, Tier, Unit, Visibility};
use fid_rdl::tracim::archive::{
    archive, fetched_test_data_to_record, ArchiveError, ArchiveIndex, ByteSpan, ContentStore, Direction, Exchange,
    FetchedDataRecordParams, MemoryContentStore, INDEX_SCHEMA,
};
use fid_rdl::view::public_view;
use fid_types::Digest;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/rdl/tracim/arch/exchange1.json")
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

fn str_field<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match m.get(k) {
        Some(CValue::Str(s)) => s.as_str(),
        other => panic!("expected string at {k}, got {other:?}"),
    }
}

fn spans_field(m: &BTreeMap<String, CValue>, k: &str) -> Vec<ByteSpan> {
    let Some(CValue::Arr(arr)) = m.get(k) else {
        panic!("expected array at {k}");
    };
    arr.iter()
        .map(|item| {
            let CValue::Arr(pair) = item else { panic!("span must be [start, end]") };
            let (CValue::Num(s), CValue::Num(e)) = (&pair[0], &pair[1]) else {
                panic!("span bounds must be numbers");
            };
            ByteSpan::new(*s as usize, *e as usize)
        })
        .collect()
}

fn load_exchange(m: &BTreeMap<String, CValue>) -> Exchange {
    Exchange {
        direction: Direction::parse(str_field(m, "direction")).unwrap(),
        timestamp: str_field(m, "timestamp").to_string(),
        destination: str_field(m, "destination").to_string(),
        bytes: str_field(m, "payload").as_bytes().to_vec(),
        secret_spans: spans_field(m, "secret_spans"),
    }
}

/// AC-1: archiving `exchange1.json` (request with marked key header + verbatim report)
/// replaces the key with `[REDACTED]`, stores report bytes identically, and lists both
/// in the `tracim-archive/v1` index.
#[test]
fn ac1_archive_redacted_request_and_verbatim_report() {
    let raw = fs::read(fixture_path()).expect("read exchange1.json");
    let root = parse(&raw).expect("parse fixture");
    let root_obj = obj(&root);
    let key = str_field(root_obj, "order_key").as_bytes();

    let req_obj = obj(root_obj.get("request").unwrap());
    let rep_obj = obj(root_obj.get("report").unwrap());

    let req = load_exchange(req_obj);
    let rep = load_exchange(rep_obj);

    let mut store = MemoryContentStore::new();
    let mut index = ArchiveIndex::new();

    let r1 = archive(&mut store, &mut index, &req, key).expect("archive request");
    let r2 = archive(&mut store, &mut index, &rep, key).expect("archive report");

    // Stored request has [REDACTED] in place of the key and matches independent SHA-256.
    let stored_req = store.get(&r1.digest).unwrap().expect("request blob stored");
    assert_eq!(stored_req, str_field(req_obj, "expected_redacted").as_bytes());
    assert!(!stored_req.windows(key.len()).any(|w| w == key));
    assert_eq!(r1.digest.as_str(), str_field(req_obj, "expected_digest"));
    assert_eq!(r1.entry.schema, INDEX_SCHEMA);
    assert_eq!(r1.entry.direction, Direction::Request);
    assert_eq!(r1.entry.redacted_spans, 1);

    // Report bytes are stored unmodified and match independent SHA-256.
    let stored_rep = store.get(&r2.digest).unwrap().expect("report blob stored");
    assert_eq!(stored_rep, rep.bytes);
    assert_eq!(r2.digest.as_str(), str_field(rep_obj, "expected_digest"));
    assert_eq!(r2.entry.direction, Direction::Report);
    assert_eq!(r2.entry.redacted_spans, 0);

    // Index lists both entries in order and serializes to canonical JSONL.
    assert_eq!(index.len(), 2);
    assert_eq!(index.entries()[0].digest, r1.digest);
    assert_eq!(index.entries()[1].digest, r2.digest);
    let jsonl = index.to_jsonl().unwrap();
    let lines: Vec<&[u8]> = jsonl.split(|&b| b == b'\n').filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);
}

/// AC-2 (error path): key appearing in an unmarked field fails with `secret_in_archive`
/// and stores nothing in either the content store or the index.
#[test]
fn ac2_unmarked_key_fails_with_secret_in_archive_and_stores_nothing() {
    let raw = fs::read(fixture_path()).expect("read exchange1.json");
    let root = parse(&raw).expect("parse fixture");
    let root_obj = obj(&root);
    let key = str_field(root_obj, "order_key").as_bytes();
    let leaked = load_exchange(obj(root_obj.get("leaked_request").unwrap()));

    let mut store = MemoryContentStore::new();
    let mut index = ArchiveIndex::new();

    let err = archive(&mut store, &mut index, &leaked, key).expect_err("unmarked key must fail");
    assert_eq!(err, ArchiveError::SecretInArchive);
    assert_eq!(err.code(), "secret_in_archive");
    assert!(store.is_empty(), "store must remain empty on secret_in_archive");
    assert!(index.is_empty(), "index must remain empty on secret_in_archive");
}

/// AC-3: archiving the same exchange twice produces identical blob and index-entry digests (D0).
#[test]
fn ac3_archiving_same_exchange_twice_gives_identical_digests() {
    let raw = fs::read(fixture_path()).expect("read exchange1.json");
    let root = parse(&raw).expect("parse fixture");
    let root_obj = obj(&root);
    let key = str_field(root_obj, "order_key").as_bytes();
    let req = load_exchange(obj(root_obj.get("request").unwrap()));

    let mut s1 = MemoryContentStore::new();
    let mut i1 = ArchiveIndex::new();
    let a1 = archive(&mut s1, &mut i1, &req, key).unwrap();

    let mut s2 = MemoryContentStore::new();
    let mut i2 = ArchiveIndex::new();
    let a2 = archive(&mut s2, &mut i2, &req, key).unwrap();

    assert_eq!(a1.digest, a2.digest);
    assert_eq!(a1.entry_digest, a2.entry_digest);
    assert_eq!(a1.entry, a2.entry);
}

/// Fetched test data converts into a validated Tier X `reference_only` RDL dataset record
/// whose public view redacts reference-only content.
#[test]
fn fetched_test_data_becomes_reference_only_tier_x_record() {
    let mut store = MemoryContentStore::new();
    let mut index = ArchiveIndex::new();
    let key = b"TRACIM-KEY-DEMO";
    let resp = Exchange {
        direction: Direction::Response,
        timestamp: "2026-11-22T06:00:01Z".to_string(),
        destination: "https://tracim.example.invalid/v1/orders/42/data".to_string(),
        bytes: b"0.0 0.0 0.0\n1.0 0.0 0.0\n".to_vec(),
        secret_spans: vec![],
    };
    let archived = archive(&mut store, &mut index, &resp, key).unwrap();

    let params = FetchedDataRecordParams {
        id: "rdl:X/fit.line3.ls/tracim_ord42/0001".to_string(),
        aim: "fit.line3.ls".to_string(),
        inputs: vec![BlobRef { role: "points".to_string(), digest: archived.digest, unit: Unit::Mm }],
        outputs: vec![RefOutput {
            name: "direction_x".to_string(),
            kind: Kind::Dimensionless,
            value_hex: HexF64::from_f64(1.0).unwrap(),
            value_dec: Some("1.0".to_string()),
            test_uncertainty_hex: HexF64::from_f64(1e-9).unwrap(),
        }],
        precision_digits: 16,
        source: format!("PTB TraCIM order ORD-42 (archive {})", archived.entry_digest),
        method: Some("TraCIM Gaussian line3 fetched test dataset".to_string()),
        licence: "LicenseRef-PTB-TraCIM-Reference-Only".to_string(),
    };

    let ds = fetched_test_data_to_record(&params).expect("valid Tier X reference_only record");
    assert_eq!(ds.tier(), Tier::X);
    assert_eq!(ds.visibility(), Visibility::ReferenceOnly);
    // Verify public_view redacts reference_only content (strips reference outputs, keeps digests).
    let pv = public_view(&ds);
    let Ok(CValue::Obj(pv_obj)) = pv else { panic!("public_view must be object") };
    assert!(!pv_obj.contains_key("reference"));
    assert!(pv_obj.contains_key("reference_digest"));
    assert_eq!(pv_obj.get("visibility"), Some(&CValue::Str("reference_only".to_string())));
}

/// Invalid and overlapping secret spans fail closed without storing anything.
#[test]
fn invalid_and_overlapping_spans_refused() {
    let mut store = MemoryContentStore::new();
    let mut index = ArchiveIndex::new();
    let key = b"SECRET";

    let bad_oob = Exchange {
        direction: Direction::Request,
        timestamp: "2026-11-22T06:00:00Z".to_string(),
        destination: "https://tracim.example.invalid".to_string(),
        bytes: b"hello".to_vec(),
        secret_spans: vec![ByteSpan::new(0, 99)],
    };
    assert_eq!(archive(&mut store, &mut index, &bad_oob, key).unwrap_err().code(), "invalid_span");

    let bad_overlap = Exchange {
        direction: Direction::Request,
        timestamp: "2026-11-22T06:00:00Z".to_string(),
        destination: "https://tracim.example.invalid".to_string(),
        bytes: b"0123456789".to_vec(),
        secret_spans: vec![ByteSpan::new(1, 5), ByteSpan::new(4, 8)],
    };
    assert_eq!(archive(&mut store, &mut index, &bad_overlap, key).unwrap_err().code(), "overlapping_spans");
    assert!(store.is_empty());
    assert!(index.is_empty());
}

/// Native `CORE.STO.CAS` (`fid_store::BlobStore`) round-trip test.
#[cfg(all(feature = "store", not(target_family = "wasm")))]
#[test]
fn blob_content_store_persists_to_cas() {
    use fid_rdl::tracim::archive::BlobContentStore;

    let tmp = std::env::temp_dir().join(format!("fid-rdl-tracim-arch-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let blobs = fid_store::BlobStore::open(&tmp).expect("open BlobStore");
    let mut cas = BlobContentStore::new(&blobs);
    let mut index = ArchiveIndex::new();

    let raw = fs::read(fixture_path()).expect("read exchange1.json");
    let root = parse(&raw).expect("parse fixture");
    let root_obj = obj(&root);
    let key = str_field(root_obj, "order_key").as_bytes();
    let req = load_exchange(obj(root_obj.get("request").unwrap()));
    let rep = load_exchange(obj(root_obj.get("report").unwrap()));

    let r1 = archive(&mut cas, &mut index, &req, key).unwrap();
    let r2 = archive(&mut cas, &mut index, &rep, key).unwrap();

    let got_req = cas.get(&r1.digest).unwrap().unwrap();
    let got_rep = cas.get(&r2.digest).unwrap().unwrap();
    assert_eq!(got_req, str_field(obj(root_obj.get("request").unwrap()), "expected_redacted").as_bytes());
    assert_eq!(got_rep, rep.bytes);
    assert_eq!(cas.get(&Digest::from_bytes([0u8; 32])).unwrap(), None);

    let _ = fs::remove_dir_all(&tmp);
}
