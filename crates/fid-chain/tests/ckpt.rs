//! Acceptance criteria and error-path tests for `TRUST.CHAIN.CKPT` (AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_chain::{
    check_checkpoint, check_checkpoints, parse_checkpoint, CheckpointInput, CheckpointResult, CheckpointStatus,
    SegmentHashes, CHECKPOINTS_PREFIX, MAX_CHECKPOINT_BYTES,
};
use fid_pkg::reader::PackageReader;
use fid_sign_core::troot::{Keyring, Revocations, SignatureStatus, TrustInputs, TrustRoot, UtcTime};
use fid_trust_canon::{entry_hash, parse as canon_parse, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn ckpt_fixtures() -> PathBuf {
    repo_root().join("fixtures").join("chain").join("ckpt")
}

fn troot_fixtures() -> PathBuf {
    repo_root().join("fixtures").join("troot")
}

fn load_both_trust_root() -> TrustRoot {
    let bytes = fs::read(troot_fixtures().join("roots").join("both.json")).expect("read both.json");
    TrustRoot::parse(&bytes).expect("parse both.json")
}

/// Opens a Result Package ZIP through `fid-pkg::reader::PackageReader`, recomputes every
/// `journal/segment.jsonl` entry hash with `fid-trust-canon::entry_hash`, and checks all
/// `journal/checkpoints/*.json` members with `fid-chain::check_checkpoints`.
fn verify_package_checkpoints(zip_name: &str, trust: Option<&TrustInputs<'_>>) -> Vec<CheckpointResult> {
    let zip_bytes = fs::read(ckpt_fixtures().join(zip_name)).expect("read fixture zip");
    let mut reader = PackageReader::open(Cursor::new(zip_bytes)).expect("open package with PackageReader");

    let seg_bytes =
        reader.read_member("journal/segment.jsonl", 1024 * 1024).expect("read segment").expect("segment present");
    let seg_text = std::str::from_utf8(&seg_bytes).expect("utf-8 segment");

    let mut hashes = BTreeMap::new();
    for line in seg_text.lines().filter(|l| !l.trim().is_empty()) {
        let val = canon_parse(line.as_bytes()).expect("parse segment line");
        let seq = match &val {
            Value::Object(m) => m
                .iter()
                .find(|(k, _)| k == "seq")
                .and_then(|(_, v)| match v {
                    Value::String(s) => s.parse::<u64>().ok(),
                    _ => None,
                })
                .expect("seq decimal string"),
            _ => panic!("entry must be object"),
        };
        let digest = entry_hash(line.as_bytes()).expect("recompute entry hash");
        hashes.insert(seq, digest);
    }
    let segment = SegmentHashes::from_map(hashes);

    let ckpt_paths: Vec<String> = reader
        .manifest()
        .members
        .iter()
        .filter(|m| m.path.as_str().starts_with(CHECKPOINTS_PREFIX) && m.path.as_str().ends_with(".json"))
        .map(|m| m.path.as_str().to_string())
        .collect();

    let mut raw_ckpts: Vec<(String, Vec<u8>)> = Vec::new();
    for p in ckpt_paths {
        let data = reader
            .read_member(&p, MAX_CHECKPOINT_BYTES as u64)
            .expect("read checkpoint member")
            .expect("checkpoint member present");
        raw_ckpts.push((p, data));
    }
    let inputs: Vec<CheckpointInput<'_>> =
        raw_ckpts.iter().map(|(p, b)| CheckpointInput { path: p.as_str(), bytes: b.as_slice() }).collect();
    check_checkpoints(&inputs, &segment, trust)
}

#[test]
fn ac1_signed_p256_and_ed25519_checkpoints_match_and_valid_trusted() {
    let root = load_both_trust_root();
    let rev = Revocations::default();
    let ring = Keyring::default();
    let trust = TrustInputs { trust_root: &root, revocations: &rev, keyring: &ring, as_of: None };

    // P-256 DSSE-wrapped checkpoint.
    let res = verify_package_checkpoints("signed-ok.zip", Some(&trust));
    assert_eq!(res.len(), 1);
    let r = &res[0];
    assert_eq!(r.path, "journal/checkpoints/3.json");
    assert_eq!(r.index, Some(3));
    assert_eq!(r.status, CheckpointStatus::Match);
    assert_eq!(r.error_code, None);
    let sig = r.signature.as_ref().expect("signed checkpoint must report signature");
    assert_eq!(sig.status, SignatureStatus::ValidTrusted);
    assert_eq!(sig.fingerprint.as_deref(), Some("5a7a78cca4a0f420d9bc62bb669c3c2759e39f723d3ae10dcbe0f0815a07ecd4"));
    assert_eq!(sig.claimed_signed_at, "2026-06-15T12:00:00Z");

    // Ed25519 embedded-signature checkpoint.
    let res_ed = verify_package_checkpoints("signed-ed25519.zip", Some(&trust));
    assert_eq!(res_ed.len(), 1);
    let r_ed = &res_ed[0];
    assert_eq!(r_ed.status, CheckpointStatus::Match);
    let sig_ed = r_ed.signature.as_ref().expect("ed25519 signature present");
    assert_eq!(sig_ed.status, SignatureStatus::ValidTrusted);
    assert_eq!(sig_ed.fingerprint.as_deref(), Some("06e3fd8fda29bb60ab59557de61edb0aecdb231134be30e75b455f8e1b792fa9"));
}

#[test]
fn ac2_wrong_head_is_mismatch_and_far_index_is_out_of_range() {
    let wrong = verify_package_checkpoints("wrong-head.zip", None);
    assert_eq!(wrong.len(), 1);
    assert_eq!(wrong[0].index, Some(3));
    assert_eq!(wrong[0].status, CheckpointStatus::Mismatch);
    assert_eq!(wrong[0].error_code, None);
    assert!(wrong[0].signature.is_none());

    let far = verify_package_checkpoints("far-index.zip", None);
    assert_eq!(far.len(), 1);
    assert_eq!(far[0].index, Some(99));
    assert_eq!(far[0].status, CheckpointStatus::OutOfRange);
    assert_eq!(far[0].error_code, None);
}

#[test]
fn ac3_flipped_signature_byte_gives_match_with_invalid_signature() {
    let root = load_both_trust_root();
    let rev = Revocations::default();
    let ring = Keyring::default();
    let trust = TrustInputs { trust_root: &root, revocations: &rev, keyring: &ring, as_of: None };

    let res = verify_package_checkpoints("flipped-sig.zip", Some(&trust));
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].index, Some(3));
    assert_eq!(res[0].status, CheckpointStatus::Match);
    let sig = res[0].signature.as_ref().expect("signature result present");
    assert_eq!(sig.status, SignatureStatus::Invalid);
    assert_eq!(sig.detail.as_deref(), Some("signature_invalid"));
}

#[test]
fn ac4_unknown_format_version_99_gives_ckpt_malformed_and_continues_checking_others() {
    let root = load_both_trust_root();
    let rev = Revocations::default();
    let ring = Keyring::default();
    let trust = TrustInputs { trust_root: &root, revocations: &rev, keyring: &ring, as_of: None };

    let res = verify_package_checkpoints("bad-version.zip", Some(&trust));
    assert_eq!(res.len(), 2);

    // First checkpoint (seq 2, format_version: 99) -> ckpt.malformed.
    assert_eq!(res[0].path, "journal/checkpoints/2.json");
    assert_eq!(res[0].status, CheckpointStatus::Malformed);
    assert_eq!(res[0].error_code, Some("ckpt.malformed"));
    assert_eq!(res[0].index, None);

    // Second checkpoint (seq 3, valid signed) -> still checked and passes!
    assert_eq!(res[1].path, "journal/checkpoints/3.json");
    assert_eq!(res[1].status, CheckpointStatus::Match);
    assert_eq!(res[1].error_code, None);
    assert_eq!(res[1].signature.as_ref().map(|s| s.status), Some(SignatureStatus::ValidTrusted));
}

#[test]
fn cross_check_canonical_bytes_agree_with_core_journal_checkpoint() {
    // Dev-only cross-check: a linear-v1 checkpoint decoded and re-encoded by CORE's fid-journal
    // matches fid-chain's independent fid-trust-canon canonical bytes byte-for-byte.
    let raw = format!(
        r#"{{"count":"7","format":"linear-v1","head":"sha256:{}","reason":"signature","seq":"7","stream":"main","time":"2026-06-15T12:00:00Z"}}"#,
        "ab".repeat(32)
    );
    let core_ckpt = fid_journal::checkpoint::Checkpoint::from_bytes(raw.as_bytes())
        .expect("CORE fid-journal must accept linear-v1 checkpoint");
    let core_bytes = core_ckpt.canonical_bytes().expect("core canonical bytes");

    let parsed = parse_checkpoint(&core_bytes).expect("fid-chain must parse CORE linear-v1 checkpoint");
    assert_eq!(parsed.seq, 7);
    assert_eq!(parsed.count, 7);
    assert_eq!(parsed.stream, "main");
    assert_eq!(parsed.head.as_str(), core_ckpt.head.to_string());
    assert_eq!(parsed.canonical_body, core_bytes);
}

#[test]
fn revoked_expired_untrusted_unknown_and_oversized_edge_cases() {
    let zip_bytes = fs::read(ckpt_fixtures().join("signed-ok.zip")).expect("read signed-ok.zip");
    let mut reader = PackageReader::open(Cursor::new(zip_bytes)).expect("open");
    let ckpt_bytes =
        reader.read_member("journal/checkpoints/3.json", MAX_CHECKPOINT_BYTES as u64).expect("read").expect("present");
    let parsed = parse_checkpoint(&ckpt_bytes).expect("parse");
    let segment = SegmentHashes::from_contiguous(3, [parsed.head]);

    // 1. No trust root -> UNKNOWN_KEY (head still MATCH).
    let no_trust = check_checkpoint("journal/checkpoints/3.json", &ckpt_bytes, &segment, None);
    assert_eq!(no_trust.status, CheckpointStatus::Match);
    assert_eq!(no_trust.signature.as_ref().map(|s| s.status), Some(SignatureStatus::UnknownKey));

    // 2. Revoked BEFORE checkpoint time -> REVOKED.
    let root = load_both_trust_root();
    let rev_before = Revocations::parse(
        br#"{"entries":[{"fingerprint":"5a7a78cca4a0f420d9bc62bb669c3c2759e39f723d3ae10dcbe0f0815a07ecd4","reason":"compromised","revoked_at":"2026-06-01T00:00:00Z"}]}"#,
    )
    .expect("parse revocations");
    let ring = Keyring::default();
    let t_rev = TrustInputs { trust_root: &root, revocations: &rev_before, keyring: &ring, as_of: None };
    let r_rev = check_checkpoint("journal/checkpoints/3.json", &ckpt_bytes, &segment, Some(&t_rev));
    assert_eq!(r_rev.status, CheckpointStatus::Match);
    assert_eq!(r_rev.signature.as_ref().map(|s| s.status), Some(SignatureStatus::Revoked));

    // 3. Revoked AFTER checkpoint time -> VALID_TRUSTED with note `revoked_after_claimed_signing`.
    let rev_after = Revocations::parse(
        br#"{"entries":[{"fingerprint":"5a7a78cca4a0f420d9bc62bb669c3c2759e39f723d3ae10dcbe0f0815a07ecd4","reason":"rotated","revoked_at":"2026-07-01T00:00:00Z"}]}"#,
    )
    .expect("parse revocations");
    let as_of = UtcTime::parse("2027-01-01T00:00:00Z").expect("as_of");
    let t_after = TrustInputs { trust_root: &root, revocations: &rev_after, keyring: &ring, as_of: Some(&as_of) };
    let r_after = check_checkpoint("journal/checkpoints/3.json", &ckpt_bytes, &segment, Some(&t_after));
    let sig_after = r_after.signature.as_ref().expect("sig");
    assert_eq!(sig_after.status, SignatureStatus::ValidTrusted);
    assert!(sig_after.notes.iter().any(|n| n == "revoked_after_claimed_signing"));
    assert!(sig_after.notes.iter().any(|n| n == "key_validity_ended_as_of"));

    // 4. Expired key (fixtures/troot/roots/expired.json) -> EXPIRED.
    let expired_bytes = fs::read(troot_fixtures().join("roots").join("expired.json")).expect("read expired.json");
    let expired_root = TrustRoot::parse(&expired_bytes).expect("parse expired.json");
    let empty_rev = Revocations::default();
    let t_exp = TrustInputs { trust_root: &expired_root, revocations: &empty_rev, keyring: &ring, as_of: None };
    let r_exp = check_checkpoint("journal/checkpoints/3.json", &ckpt_bytes, &segment, Some(&t_exp));
    assert_eq!(r_exp.signature.as_ref().map(|s| s.status), Some(SignatureStatus::Expired));

    // 5. Keyring-only (known but untrusted) -> VALID_UNTRUSTED.
    let kr_bytes = fs::read(troot_fixtures().join("keyring").join("keyring.json")).expect("read keyring/keyring.json");
    let kr = Keyring::parse(&kr_bytes).expect("parse keyring");
    let empty_root = TrustRoot::default();
    let t_kr = TrustInputs { trust_root: &empty_root, revocations: &empty_rev, keyring: &kr, as_of: None };
    let r_kr = check_checkpoint("journal/checkpoints/3.json", &ckpt_bytes, &segment, Some(&t_kr));
    assert_eq!(r_kr.signature.as_ref().map(|s| s.status), Some(SignatureStatus::ValidUntrusted));

    // 6. Unknown `format` string (`merkle-v1`) and oversized (> 64 KiB) -> ckpt.malformed.
    let merkle = br#"{"format":"merkle-v1","seq":"1"}"#;
    let r_merkle = check_checkpoint("journal/checkpoints/1.json", merkle, &segment, None);
    assert_eq!(r_merkle.status, CheckpointStatus::Malformed);
    assert_eq!(r_merkle.error_code, Some("ckpt.malformed"));

    let huge = vec![b' '; MAX_CHECKPOINT_BYTES + 1];
    let r_huge = check_checkpoint("journal/checkpoints/huge.json", &huge, &segment, None);
    assert_eq!(r_huge.status, CheckpointStatus::Malformed);
    assert_eq!(r_huge.error_code, Some("ckpt.malformed"));
}
