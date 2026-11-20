//! The synthetic chain and kill plan against the independent Python oracle
//! (`fixtures/core/sto/sql/crash/make_chain.py`), bounded parsing of stored rows, and the
//! mapping of failures to the v3 bus error catalogue.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_crash::chain::{blob_bytes, chain, MAX_ROW_BYTES};
use fid_crash::{CrashFailure, FailureKind, KillInstant, KillPlan, SyntheticRow};

/// REQ-001: every row, its canonical bytes and its hash equal the oracle's.
#[test]
fn chain_matches_the_python_oracle() {
    let fx = fixture();
    assert_eq!(text(field(&fx, "format")), fid_crash::chain::ROW_FORMAT);
    let want = arr(field(&fx, "rows"));
    let got = chain(want.len() as u64);
    assert_eq!(got.len(), 40);
    for (row, w) in got.iter().zip(want) {
        assert_eq!(row.seq, num(field(w, "seq")));
        assert_eq!(String::from_utf8(row.canonical()).unwrap(), text(field(w, "canonical")), "row {}", row.seq);
        assert_eq!(row.hash().as_str(), text(field(w, "hash")), "row {}", row.seq);
        assert_eq!(blob_bytes(row.seq).map_or(0, |b| b.len() as u64), num(field(w, "blob_len")));
    }
    // Each row links to the previous one's hash.
    for pair in got.windows(2) {
        assert_eq!(pair[1].prev_hash, Some(pair[0].hash()));
    }
    assert_eq!(got[0].prev_hash, None);
}

/// REQ-002 / REQ-005: the kill schedule is the oracle's, bounded, and instants round-trip.
#[test]
fn kill_plan_matches_the_oracle() {
    let fx = fixture();
    let kp = field(&fx, "kill_plan");
    let plan = KillPlan { seed: num(field(kp, "seed")), max_delay_us: num(field(kp, "max_delay_us")) };
    for (k, d) in arr(field(kp, "delays_us")).iter().enumerate() {
        assert_eq!(plan.instant(k as u64), KillInstant { kill: k as u64, delay_us: num(d) });
    }
    assert_eq!(fid_crash::plan::splitmix64(0), 0xe220_a839_7b1d_cdaf, "published SplitMix64 vector");
    let other = KillPlan { seed: 2, ..plan };
    assert_ne!(
        (0..16).map(|k| plan.instant(k)).collect::<Vec<_>>(),
        (0..16).map(|k| other.instant(k)).collect::<Vec<_>>()
    );
    for k in 0..10_000 {
        assert!(plan.instant(k).delay_us <= plan.max_delay_us);
    }
    let i = KillInstant { kill: 812, delay_us: 4711 };
    assert_eq!(KillInstant::parse(&i.to_string()), Some(i));
    for bad in ["", "1", "1:", ":2", "a:b", "1:2:3", "-1:2", "99999999999999999999:1"] {
        assert_eq!(KillInstant::parse(bad), None, "{bad:?}");
    }
    let edge = KillPlan { seed: 7, max_delay_us: u64::MAX };
    let _ = edge.instant(u64::MAX);
    assert_eq!(KillPlan { seed: 7, max_delay_us: 0 }.instant(5).delay_us, 0);
}

/// Stored rows are accepted only in their exact canonical form; anything else is refused
/// without panicking, and oversized input is refused before parsing.
#[test]
fn row_parsing_is_strict_and_bounded() {
    for row in chain(12) {
        assert_eq!(SyntheticRow::parse(&row.canonical()).unwrap(), row);
    }
    let r = SyntheticRow::new(1, None);
    let canon = String::from_utf8(r.canonical()).unwrap();
    let mut bad: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        vec![0xff, 0xfe, 0x00],
        canon.replace(',', ", ").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"01\"").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"0\"").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":1").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"99999999999999999999\"").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"1\",\"x\":null").into_bytes(),
        canon.replace("fid-crash-row@1", "fid-crash-row@2").into_bytes(),
        canon.replace("\"prev\":null", "\"prev\":\"sha256:00\"").into_bytes(),
        canon.replace("\"prev\":null", "\"prev\":{\"a\":[[[[]]]]}").into_bytes(),
        format!("{canon}{canon}").into_bytes(),
        "[".repeat(100_000).into_bytes(),
        vec![b' '; MAX_ROW_BYTES + 1],
    ];
    // Byte-for-byte mutations of a real row.
    for i in 0..canon.len() {
        let mut b = canon.clone().into_bytes();
        b[i] ^= 0x01;
        bad.push(b);
    }
    for b in &bad {
        if let Ok(row) = SyntheticRow::parse(b) {
            // A mutation may still be a canonical row (a flipped hex digit), but then it is a
            // different row, which the chain check refuses.
            assert_ne!(row, r, "mutation {:?} parsed as the original", String::from_utf8_lossy(b));
            assert_eq!(row.canonical(), *b);
        }
    }
}

/// Every failure maps to `internal` with a reason of the frozen v3 catalogue, never to a
/// success value.
#[test]
fn failures_map_to_v3_reasons() {
    for kind in [
        FailureKind::ChainBroken,
        FailureKind::AckMissing,
        FailureKind::BlobMissing,
        FailureKind::BlobCorrupt,
        FailureKind::StoreRefused,
    ] {
        let f = CrashFailure::new(kind, "detail").at(1, KillInstant { kill: 3, delay_us: 9 });
        assert_refused(&f, kind.code());
        let shown = f.to_string();
        assert!(shown.starts_with(kind.code()) && shown.contains("seed 1") && shown.contains("instant 3:9"), "{shown}");
        let json = String::from_utf8(f.to_bus_error().to_json().unwrap()).unwrap();
        assert!(json.contains("\"code\":\"internal\""), "{json}");
    }
}

/// Test hooks stay out of normal builds: no manifest in the workspace enables `fault-inject`
/// or fid-store's `test-probe` outside `[dev-dependencies]`.
#[test]
fn fault_injection_is_test_only() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut checked = 0;
    for e in std::fs::read_dir(&crates).unwrap() {
        let manifest = e.unwrap().path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else { continue };
        checked += 1;
        let mut section = String::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                section = t.to_string();
                continue;
            }
            if t.starts_with('#') || !section.contains("dependencies") || section.contains("dev-dependencies") {
                continue;
            }
            assert!(
                !t.contains("fault-inject") && !t.contains("test-probe"),
                "{}: {section} enables a test hook: {t}",
                manifest.display()
            );
        }
    }
    assert!(checked > 10);
}
