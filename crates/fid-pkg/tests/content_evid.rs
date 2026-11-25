//! Acceptance criteria tests for spec node `TRUST.PKG.CONTENT.EVID` (AC-1..AC-5).
#![cfg(feature = "export")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;

use fid_pkg::content::evid::{
    gather, BlobStore, GatheredKind, MemoryBlobStore, ResultRecord, StoredBlob, DEFAULT_POINTS_MEDIA_TYPE,
    REGISTER_NONE,
};
use fid_pkg::{EvidenceTier, PkgError, JSON_MEDIA_TYPE};
use fid_types::Digest;
use sha2::{Digest as Sha2Digest, Sha256};

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

fn shared_fixture() -> (MemoryBlobStore, Vec<ResultRecord>, [Digest; 3]) {
    let mut cas = MemoryBlobStore::new();
    let d1 = cas.insert(DEFAULT_POINTS_MEDIA_TYPE, b"FPTS1:seeded-point-set-alpha-0001".to_vec());
    let d2 = cas.insert(DEFAULT_POINTS_MEDIA_TYPE, b"FPTS1:seeded-point-set-beta-0002".to_vec());
    let d3 = cas.insert(DEFAULT_POINTS_MEDIA_TYPE, b"FPTS1:seeded-point-set-gamma-0003".to_vec());

    // 5 records referencing 3 point sets, with d1 shared by 3 records (AC-1).
    let records = vec![
        ResultRecord { id: "r1".into(), input_digests: vec![d1], interpretation_register: None },
        ResultRecord { id: "r2".into(), input_digests: vec![d1, d2], interpretation_register: None },
        ResultRecord { id: "r3".into(), input_digests: vec![d1], interpretation_register: None },
        ResultRecord { id: "r4".into(), input_digests: vec![d3], interpretation_register: None },
        ResultRecord { id: "r5".into(), input_digests: vec![d2, d3], interpretation_register: None },
    ];
    (cas, records, [d1, d2, d3])
}

#[test]
fn ac1_points_tier_deduplicates_and_sorts_by_path() {
    let (cas, records, [d1, d2, d3]) = shared_fixture();
    let out = gather(&cas, &records, EvidenceTier::Points).expect("AC-1 gather succeeds");
    assert_eq!(out.evidence_tier, EvidenceTier::Points);
    assert_eq!(out.point_members.len(), 3);
    assert!(out.interp_members.is_empty());

    let mut expected = vec![
        format!("points/{}.fpts", d1.hex()),
        format!("points/{}.fpts", d2.hex()),
        format!("points/{}.fpts", d3.hex()),
    ];
    expected.sort();

    let got_paths: Vec<String> = out.point_members.iter().map(|m| m.path.as_str().to_owned()).collect();
    assert_eq!(got_paths, expected);
    for m in &out.point_members {
        assert_eq!(m.kind, GatheredKind::Points);
        assert_eq!(m.ext, "fpts");
        assert_eq!(m.blob.digest, m.digest);
    }
}

#[test]
fn ac2_results_and_features_tiers_omit_points_and_record_tier() {
    let (cas, records, _) = shared_fixture();

    let res = gather(&cas, &records, EvidenceTier::Results).expect("results tier");
    assert_eq!(res.evidence_tier, EvidenceTier::Results);
    assert!(res.point_members.is_empty());

    let feat = gather(&cas, &records, EvidenceTier::Features).expect("features tier");
    assert_eq!(feat.evidence_tier, EvidenceTier::Features);
    assert!(feat.point_members.is_empty());
}

#[test]
fn ac3_missing_and_corrupt_point_sets_fail_closed() {
    let (mut cas, records, [d1, d2, _]) = shared_fixture();

    // Remove d2 -> pkg.points_missing lists d2.
    assert!(cas.remove(&d2));
    let err = gather(&cas, &records, EvidenceTier::Points).expect_err("missing point set must fail");
    assert_eq!(err.code(), "pkg.points_missing");
    assert_eq!(err, PkgError::PointsMissing(vec![d2]));

    // Remove d1 too -> pkg.points_missing lists BOTH missing digests in sorted order.
    assert!(cas.remove(&d1));
    let err2 = gather(&cas, &records, EvidenceTier::Points).expect_err("two missing point sets");
    let mut want = vec![d1, d2];
    want.sort();
    assert_eq!(err2, PkgError::PointsMissing(want));

    // Flip one byte of a CAS blob -> pkg.points_corrupt names the digest.
    let (mut cas2, records2, [d1, _, _]) = shared_fixture();
    let mut corrupted = b"FPTS1:seeded-point-set-alpha-0001".to_vec();
    corrupted[0] ^= 0x01;
    cas2.insert_under_digest(d1, DEFAULT_POINTS_MEDIA_TYPE, corrupted);
    let err3 = gather(&cas2, &records2, EvidenceTier::Points).expect_err("corrupted blob must fail");
    assert_eq!(err3.code(), "pkg.points_corrupt");
    assert_eq!(err3, PkgError::PointsCorrupt(d1));
}

/// Zero-allocation deterministic repeating stream for AC-4 (streams 2 GiB with O(1) memory).
struct RepeatingStream {
    remaining: u64,
    byte: u8,
}

impl Read for RepeatingStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(self.remaining) as usize;
        buf[..n].fill(self.byte);
        self.remaining -= n as u64;
        Ok(n)
    }
}

struct SyntheticHugeStore {
    digest: Digest,
    length: u64,
    byte: u8,
}

impl BlobStore for SyntheticHugeStore {
    fn open(&self, digest: &Digest) -> Result<Option<StoredBlob<'_>>, PkgError> {
        if *digest != self.digest {
            return Ok(None);
        }
        Ok(Some(StoredBlob {
            media_type: DEFAULT_POINTS_MEDIA_TYPE.to_owned(),
            length: self.length,
            reader: Box::new(RepeatingStream { remaining: self.length, byte: self.byte }),
        }))
    }
}

#[cfg(target_os = "linux")]
fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let num = rest.split_whitespace().next()?;
            return num.parse::<u64>().ok();
        }
    }
    None
}

#[test]
fn ac4_streamed_synthetic_point_set_stays_under_200_mib_rss() {
    // In release builds stream the full 2 GiB (AC-4); in unoptimized debug builds stream
    // 256 MiB (strictly above the 200 MiB RSS ceiling so any whole-blob buffering fails)
    // to keep `cargo test --workspace` fast. Both expected SHA-256 digests were computed
    // independently with Python hashlib.sha256 over repeating 0x5A bytes.
    #[cfg(not(debug_assertions))]
    const SIZE: u64 = 2 * 1024 * 1024 * 1024;
    #[cfg(not(debug_assertions))]
    const EXPECTED: &str = "sha256:a39aeb34eba87f533e16a5a220ced624394244985b2b51e8351b8b3e0b8e7077";

    #[cfg(debug_assertions)]
    const SIZE: u64 = 256 * 1024 * 1024;
    #[cfg(debug_assertions)]
    const EXPECTED: &str = "sha256:d4e0d5a6082e9536f1ff4fbc69855d8b3e458328f27af8d72cb104d8e81b5bc2";

    const BYTE: u8 = 0x5A;

    let expected_digest = Digest::parse(EXPECTED).expect("valid constant digest");

    let store = SyntheticHugeStore { digest: expected_digest, length: SIZE, byte: BYTE };
    let records =
        vec![ResultRecord { id: "r-huge".into(), input_digests: vec![expected_digest], interpretation_register: None }];

    let out = gather(&store, &records, EvidenceTier::Points).expect("streamed gather succeeds");
    assert_eq!(out.point_members.len(), 1);
    assert_eq!(out.point_members[0].blob.length, SIZE);
    assert_eq!(out.point_members[0].digest, expected_digest);

    #[cfg(target_os = "linux")]
    if let Some(hwm_kib) = peak_rss_kib() {
        assert!(hwm_kib < 200 * 1024, "peak RSS {hwm_kib} KiB exceeded 200 MiB ceiling");
    }
}

#[test]
fn ac5_d045_gathers_deduplicated_register_and_roster_at_every_tier() {
    let mut cas = MemoryBlobStore::new();
    let roster_bytes = br#"{"reviewers":["eng:alice"],"schema":"interp-roster/1"}"#.to_vec();
    let roster_digest = cas.insert(JSON_MEDIA_TYPE, roster_bytes.clone());

    let reg_json =
        format!(r#"{{"entries":[],"roster_digest":"{}","schema":"interp-register/1"}}"#, roster_digest.as_str());
    let reg_bytes = reg_json.into_bytes();
    let reg_digest = cas.insert(JSON_MEDIA_TYPE, reg_bytes.clone());

    assert_eq!(ResultRecord::parse_register_ref(REGISTER_NONE).unwrap(), None);
    assert_eq!(ResultRecord::parse_register_ref(reg_digest.as_str()).unwrap(), Some(reg_digest));

    // Two records share the same interpretation_register digest, and one names `none`.
    let records = vec![
        ResultRecord { id: "r1".into(), input_digests: vec![], interpretation_register: Some(reg_digest) },
        ResultRecord { id: "r2".into(), input_digests: vec![], interpretation_register: Some(reg_digest) },
        ResultRecord { id: "r3".into(), input_digests: vec![], interpretation_register: None },
    ];

    let out = gather(&cas, &records, EvidenceTier::Results).expect("D-045 gather at results tier");
    assert_eq!(out.evidence_tier, EvidenceTier::Results);
    assert!(out.point_members.is_empty());
    assert_eq!(out.interp_members.len(), 2);
    assert_eq!(out.interp_members[0].kind, GatheredKind::InterpRegister);
    assert_eq!(out.interp_members[0].path.as_str(), format!("interp/registers/{}.json", reg_digest.hex()));
    assert_eq!(out.interp_members[0].digest, reg_digest);
    assert_eq!(out.interp_members[0].blob.length, reg_bytes.len() as u64);
    assert_eq!(out.interp_members[1].kind, GatheredKind::InterpRoster);
    assert_eq!(out.interp_members[1].path.as_str(), format!("interp/rosters/{}.json", roster_digest.hex()));
    assert_eq!(out.interp_members[1].digest, roster_digest);
    assert_eq!(out.interp_members[1].blob.length, roster_bytes.len() as u64);

    // Removing the register blob fails with pkg.register_missing naming it.
    let mut cas_missing_reg = cas.clone();
    cas_missing_reg.remove(&reg_digest);
    let err = gather(&cas_missing_reg, &records, EvidenceTier::Results).expect_err("missing register");
    assert_eq!(err.code(), "pkg.register_missing");
    assert_eq!(err, PkgError::RegisterMissing(reg_digest));

    // Corrupting the register blob also fails with pkg.register_missing naming it.
    let mut cas_corrupt_reg = cas.clone();
    cas_corrupt_reg.insert_under_digest(reg_digest, JSON_MEDIA_TYPE, b"{}".to_vec());
    let err2 = gather(&cas_corrupt_reg, &records, EvidenceTier::Results).expect_err("corrupt register");
    assert_eq!(err2, PkgError::RegisterMissing(reg_digest));

    // Removing the roster blob fails with pkg.register_missing naming the roster digest.
    let mut cas_missing_roster = cas;
    cas_missing_roster.remove(&roster_digest);
    let err3 = gather(&cas_missing_roster, &records, EvidenceTier::Results).expect_err("missing roster");
    assert_eq!(err3, PkgError::RegisterMissing(roster_digest));

    let _ = sha256(&roster_bytes);
}
