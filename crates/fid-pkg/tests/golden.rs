//! Spec node TRUST.PKG.FIXTURE: the golden packages (REQ-002, REQ-004) read back through the
//! real bounded reader, and the conformance runner (AC-2) driving the reference reader binary.
//!
//! Every expected value (file digest, evidence tier, record IDs, UEC-0 records, signature
//! count) comes from `fixtures/pkg/golden/index.json`, written by the independent stdlib
//! generator `fixtures/pkg/make_golden.py`, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_pkg::hash_bytes;
use fid_pkg::reader::{inspect, PackageReader};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/pkg");

fn fix(rel: &str) -> PathBuf {
    Path::new(FIX).join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    let p = fix(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn st<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn int(o: &BTreeMap<String, CValue>, k: &str) -> u64 {
    match o.get(k) {
        Some(CValue::U64(n)) => *n,
        Some(CValue::I64(n)) => u64::try_from(*n).unwrap(),
        Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 0.0 => n.to_string().parse().unwrap(),
        other => panic!("{k}: expected integer, got {other:?}"),
    }
}

fn strs(v: &CValue) -> Vec<String> {
    arr(v)
        .iter()
        .map(|x| match x {
            CValue::Str(s) => s.clone(),
            other => panic!("expected string, got {other:?}"),
        })
        .collect()
}

fn index() -> Vec<BTreeMap<String, CValue>> {
    let v = fid_canon::parse(&read("golden/index.json")).unwrap();
    arr(&v).iter().map(|g| obj(g).clone()).collect()
}

#[test]
fn req002_golden_set_covers_every_tier_two_signatures_and_uec0() {
    let goldens = index();
    let tiers: std::collections::BTreeSet<&str> = goldens.iter().map(|g| st(g, "evidence_tier")).collect();
    assert_eq!(tiers.into_iter().collect::<Vec<_>>(), ["features", "points", "results"]);
    assert!(goldens.iter().any(|g| int(g, "signature_count") == 2));
    assert!(goldens.iter().any(|g| !arr(&g["uec0_record_ids"]).is_empty()));
    for name in ["points", "results"] {
        assert!(goldens.iter().any(|g| st(g, "name") == name), "{name}.zip is named by TRUST.REPLAY.SELECT");
    }
}

#[test]
fn every_golden_opens_verifies_and_matches_its_index_entry() {
    for g in index() {
        let name = st(&g, "name");
        let file = st(&g, "file");
        let bytes = read(&format!("golden/{file}"));

        // Pinned digest, in the index and in the sha256sum file.
        let digest = hash_bytes(&bytes);
        assert_eq!(digest.hex(), st(&g, "sha256"), "{name}");
        let pinned = String::from_utf8(read(&format!("golden/{file}.sha256"))).unwrap();
        assert_eq!(pinned, format!("{}  {file}\n", digest.hex()), "{name}");

        // The strict path every verifier takes.
        let mut reader = PackageReader::open(Cursor::new(&bytes[..])).unwrap_or_else(|e| panic!("{name}: {e}"));
        reader.verify_all().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reader.signature_count() as u64, int(&g, "signature_count"), "{name}");
        assert_eq!(reader.statement_sha256().unwrap().hex(), st(&g, "statement_sha256"), "{name}");
        let mut sigs: Vec<&str> = reader.entry_names().into_iter().filter(|n| n.starts_with("signatures/")).collect();
        sigs.sort_unstable();
        assert_eq!(sigs, strs(&g["signatures"]), "{name}");

        let inv = inspect(Cursor::new(&bytes[..])).unwrap();
        assert_eq!(inv.evidence_tier.as_str(), st(&g, "evidence_tier"), "{name}");
        assert_eq!(u64::from(inv.signature_count), int(&g, "signature_count"), "{name}");

        // The records: count and IDs, sorted by ID bytes.
        let records = reader.read_member("results/records.json", 1 << 20).unwrap().unwrap();
        let records = fid_canon::parse(&records).unwrap();
        let ids: Vec<String> = arr(&records).iter().map(|r| st(obj(r), "id").to_string()).collect();
        assert_eq!(ids.len() as u64, int(&g, "result_count"), "{name}");
        assert_eq!(ids, strs(&g["record_ids"]), "{name}");

        // UEC-0 (REQ-TRUST.PKG-006): exactly {"class":"UEC-0","status":"not_evaluated"}, never a value.
        let uec0 = strs(&g["uec0_record_ids"]);
        for r in arr(&records) {
            let r = obj(r);
            let u = obj(&r["uncertainty"]);
            if uec0.iter().any(|id| id == st(r, "id")) {
                assert_eq!(u.len(), 2, "{name}: {u:?}");
                assert_eq!((st(u, "class"), st(u, "status")), ("UEC-0", "not_evaluated"), "{name}");
            } else {
                assert_ne!(st(u, "class"), "UEC-0", "{name}");
            }
        }

        // Points only at tier `points`, each named by the digest of its bytes.
        let points: Vec<String> =
            reader.entry_names().into_iter().filter(|n| n.starts_with("points/")).map(str::to_string).collect();
        assert_eq!(!points.is_empty(), st(&g, "evidence_tier") == "points", "{name}");
        for p in points {
            let body = reader.read_member(&p, 1 << 20).unwrap().unwrap();
            let hex = p.strip_prefix("points/").unwrap().split('.').next().unwrap();
            assert_eq!(hash_bytes(&body).hex(), hex, "{name}: {p}");
        }
    }
}

/// The interpreters to try for the Python runner, in order.
fn pythons() -> &'static [&'static str] {
    if cfg!(windows) {
        &["python", "py", "python3"]
    } else {
        &["python3", "python"]
    }
}

#[test]
fn ac2_run_conformance_passes_with_the_reference_reader() {
    let runner = fix("run-conformance");
    let reader = env!("CARGO_BIN_EXE_fid-pkg-conformance");
    for py in pythons() {
        let Ok(out) = Command::new(py).arg(&runner).arg("--reasons").arg("--").arg(reader).output() else {
            continue;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() && !stdout.contains("conformance cases passed") {
            continue; // not a working interpreter (e.g. a launcher stub)
        }
        assert!(out.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
        let passed = stdout.lines().filter(|l| l.starts_with("ok   ")).count();
        assert!(passed >= 105 + 6, "{passed} cases:\n{stdout}");
        assert!(!stdout.contains("FAIL"), "{stdout}");
        return;
    }
    panic!("no Python interpreter found for fixtures/pkg/run-conformance (tried {:?})", pythons());
}

#[test]
fn reference_reader_reports_codes_as_json() {
    let reader = env!("CARGO_BIN_EXE_fid-pkg-conformance");
    let ok = Command::new(reader).arg(fix("golden/points.zip")).output().unwrap();
    assert!(ok.status.success());
    assert_eq!(String::from_utf8(ok.stdout).unwrap(), "{\"result\":\"accepted\"}\n");
    let bad = Command::new(reader).arg(fix("malformed/crc_mismatch/package.zip")).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(bad.stdout).unwrap(),
        "{\"error_code\":\"pkg.malformed\",\"reason\":\"crc\",\"result\":\"rejected\"}\n"
    );
    let usage = Command::new(reader).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
}
