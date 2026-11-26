//! The reserved "no kernel attested" digest of spec node CORE.JRN.ENTRY (Design, "Reserved
//! no-kernel digest"): `NO_KERNEL_MANIFEST` reads as no attestation, verification accepts
//! and counts such entries without taking them for a real kernel, a relabelled kernel fails
//! the hash, and two unattested kernels are never the same kernel.
//!
//! Expected values come from the independent Python reference
//! `fixtures/core/jrn/entry/make_no_kernel.py` (its own canonical writer and hashlib).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, CValue, Digest};
use fid_journal::verify::{
    verify_rows, BlobSource, BlobState, FailureKind, ReadError, Start, StoredEntry, VerifyOptions,
};
use fid_journal::{parse_digest, same_attested_kernel, EntryHeader, KernelAttestation, Manifests, NO_KERNEL_MANIFEST};
use std::collections::BTreeMap;

const FIXTURE: &[u8] = include_bytes!("../../../fixtures/core/jrn/entry/no_kernel.json");

type Obj = BTreeMap<String, CValue>;

fn fixture() -> Obj {
    match parse(FIXTURE).unwrap() {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn obj<'a>(o: &'a Obj, k: &str) -> &'a Obj {
    match o.get(k) {
        Some(CValue::Obj(v)) => v,
        other => panic!("field {k} is {other:?}"),
    }
}

fn as_obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("field {k} is {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("field {k} is {other:?}"),
    }
}

fn int(o: &Obj, k: &str) -> u64 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u64,
        other => panic!("field {k} is {other:?}"),
    }
}

fn digest(s: &str) -> Digest {
    parse_digest(s).unwrap()
}

struct Payloads(BTreeMap<[u8; 32], Vec<u8>>);

impl BlobSource for Payloads {
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

fn rows(f: &Obj) -> (Vec<StoredEntry>, Payloads) {
    let mut store = BTreeMap::new();
    let rows = arr(f, "rows")
        .iter()
        .map(|r| {
            let r = as_obj(r);
            let header = st(r, "header").as_bytes().to_vec();
            store.insert(digest(st(r, "payload_digest")).0, st(r, "payload").as_bytes().to_vec());
            StoredEntry {
                seq: int(r, "seq") as i64,
                hash: st(r, "hash").into(),
                stream: st(r, "stream").into(),
                branch: st(r, "branch").into(),
                kind: st(r, "kind").into(),
                prev_hash: Some(st(r, "prev_hash").into()),
                header_len: header.len() as u64,
                header,
                refs: vec![("payload".to_string(), st(r, "payload_digest").to_string())],
            }
        })
        .collect();
    (rows, Payloads(store))
}

fn verify(stream: &str, rows: Vec<StoredEntry>, blobs: &mut Payloads) -> fid_journal::verify::VerifyReport {
    verify_rows(stream, Start::GENESIS, &VerifyOptions::default(), Vec::new(), rows.into_iter().map(Ok), blobs).unwrap()
}

#[test]
fn the_reserved_digest_is_the_sha256_of_nothing() {
    let f = fixture();
    assert_eq!(NO_KERNEL_MANIFEST, digest_bytes(b""));
    assert_eq!(NO_KERNEL_MANIFEST.to_string(), st(&f, "no_kernel_manifest"));
    assert_eq!(KernelAttestation::of(&NO_KERNEL_MANIFEST), KernelAttestation::Unattested);
    assert_eq!(KernelAttestation::Unattested.digest(), None);
    assert!(!KernelAttestation::Unattested.is_attested());
    let real = digest_bytes(b"some kernel manifest");
    assert_eq!(KernelAttestation::of(&real), KernelAttestation::Attested(real));
    assert!(KernelAttestation::of(&real).is_attested());
    let m = Manifests { registry: real, kernel: NO_KERNEL_MANIFEST };
    assert_eq!(m.attested_kernel(), None);
    assert_eq!(Manifests { registry: real, kernel: real }.attested_kernel(), Some(real));
}

#[test]
fn headers_read_back_with_the_oracle_attestation() {
    let f = fixture();
    let mut unattested = 0;
    for r in arr(&f, "rows") {
        let r = as_obj(r);
        let h = EntryHeader::from_value(&parse(st(r, "header").as_bytes()).unwrap()).unwrap();
        assert_eq!(h.hash.to_string(), st(r, "hash"));
        assert_eq!(h.manifests.kernel.to_string(), st(r, "kernel"), "seq {}", int(r, "seq"));
        let want = match r.get("attested_kernel") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(digest(s)),
            other => panic!("attested_kernel is {other:?}"),
        };
        assert_eq!(h.manifests.attested_kernel(), want, "seq {}", int(r, "seq"));
        assert_eq!(h.manifests.kernel_attestation().is_attested(), want.is_some());
        if want.is_none() {
            unattested += 1;
        }
    }
    assert_eq!(unattested, 4);
}

#[test]
fn verification_accepts_and_counts_unattested_entries() {
    let f = fixture();
    let stream = st(&f, "stream").to_string();
    let (rows, mut blobs) = rows(&f);
    let report = verify(&stream, rows, &mut blobs);
    assert!(report.ok(), "{:?}", report.failure);
    let want = obj(obj(&f, "expect"), "checked");
    assert_eq!(report.checked.entries, int(want, "entries"));
    assert_eq!(report.checked.without_kernel, int(want, "without_kernel"));
    assert_eq!(report.checked.without_payload, int(want, "without_payload"));
    // The command result keeps the shape CORE.JRN.CHAIN.VERIFY pins.
    let CValue::Obj(v) = report.to_value() else { panic!() };
    let keys: Vec<&str> = obj(&v, "checked").keys().map(String::as_str).collect();
    assert_eq!(keys, ["blobs", "checkpoints", "entries", "without_payload"]);
}

/// A relabelled kernel fails the hash; ordinary tampering of an unattested entry (a re-pointed
/// `prev`, corrupt payload bytes, another entry's payload) fails with its usual error. The
/// counts stop before the failing entry.
#[test]
fn tampering_fails_with_its_normal_error_and_counts_stop_before_it() {
    let f = fixture();
    let stream = st(&f, "stream").to_string();
    let cases = arr(&f, "tampered");
    let names: Vec<&str> = cases.iter().map(|c| st(as_obj(c), "name")).collect();
    assert_eq!(
        names,
        [
            "attested_to_none",
            "none_to_attested",
            "unattested_prev_relinked",
            "unattested_payload_corrupt",
            "unattested_payload_swapped"
        ]
    );
    for c in cases {
        let c = as_obj(c);
        let name = st(c, "name");
        let (mut rows, mut blobs) = rows(&f);
        let seq = int(c, "seq");
        let row = rows.iter_mut().find(|r| r.seq as u64 == seq).unwrap();
        for (k, v) in obj(c, "row") {
            let CValue::Str(v) = v else { panic!("{name}: row.{k} is {v:?}") };
            match k.as_str() {
                "header" => {
                    row.header = v.as_bytes().to_vec();
                    row.header_len = row.header.len() as u64;
                }
                "hash" => row.hash = v.clone(),
                "prev_hash" => row.prev_hash = Some(v.clone()),
                "payload_digest" => row.refs = vec![("payload".to_string(), v.clone())],
                other => panic!("{name}: unknown row field {other}"),
            }
        }
        for (d, v) in obj(c, "store") {
            let CValue::Str(v) = v else { panic!("{name}: store value is {v:?}") };
            blobs.0.insert(digest(d).0, v.as_bytes().to_vec());
        }
        let report = verify(&stream, rows, &mut blobs);
        let expect = obj(c, "expect");
        let want = obj(expect, "failure");
        let got = report.failure.as_ref().unwrap_or_else(|| panic!("{name} verified"));
        assert_eq!(got.seq, Some(int(want, "seq")), "{name}");
        assert_eq!(got.kind.as_str(), st(want, "kind"), "{name}");
        assert_eq!(got.reason, st(want, "reason"), "{name}");
        if name.contains("_to_") {
            assert_eq!(got.kind, FailureKind::Hash, "{name}");
        }
        let counts = obj(expect, "checked");
        assert_eq!(report.checked.entries, int(counts, "entries"), "{name}");
        assert_eq!(report.checked.without_kernel, int(counts, "without_kernel"), "{name}");
    }
}

#[test]
fn an_unattested_kernel_is_never_the_same_kernel() {
    let f = fixture();
    let pairs = arr(&f, "same_attested_kernel");
    assert_eq!(pairs.len(), 16);
    for p in pairs {
        let p = as_obj(p);
        let (a, b) = (digest(st(p, "recorded")), digest(st(p, "replayed")));
        let want = matches!(p.get("same"), Some(CValue::Bool(true)));
        assert_eq!(same_attested_kernel(&a, &b), want, "{a} vs {b}");
    }
    assert!(!same_attested_kernel(&NO_KERNEL_MANIFEST, &NO_KERNEL_MANIFEST));
}
