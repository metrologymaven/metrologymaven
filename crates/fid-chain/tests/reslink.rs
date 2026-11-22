//! Acceptance criteria and edge-case tests for `TRUST.CHAIN.RESLINK` (AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_chain::reslink::{
    link_package, link_records, parse_verified_entry_line, parse_verified_segment, EntryKind, LinkResult, LinkStatus,
    MislinkReason, PackageError, PackageMemberReader, ParsedRecord, ReslinkError, VerifiedEntry, VerifiedSegment,
    SEGMENT_PATH,
};
use fid_pkg::reader::PackageReader;
use fid_trust_canon::{entry_hash_value, parse as canon_parse, Value};
use fid_types::Digest;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn pkg_goldens() -> PathBuf {
    repo_root().join("fixtures").join("pkg").join("golden")
}

fn reslink_fixtures() -> PathBuf {
    repo_root().join("fixtures").join("chain").join("reslink")
}

struct PkgAdapter<R> {
    reader: PackageReader<R>,
}

impl<R: std::io::Read + std::io::Seek> PackageMemberReader for PkgAdapter<R> {
    fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError> {
        self.reader
            .read_member(path, max_bytes)
            .map_err(|e| PackageError { code: e.code().to_owned(), detail: e.to_string() })
    }
}

/// Walks `journal/segment.jsonl` via `fid-trust-canon`, verifying hash and predecessor chain
/// links to detect the first failing index (zero-based) if any, then constructs a
/// `VerifiedSegment` containing only entries strictly before that failure (REQ-002).
fn walk_segment_and_link(zip_path: &Path) -> (Option<u64>, Vec<LinkResult>) {
    let zip_bytes = fs::read(zip_path).expect("read zip fixture");
    let reader = PackageReader::open(Cursor::new(zip_bytes)).expect("open package with PackageReader");
    let mut adapter = PkgAdapter { reader };

    let seg_bytes =
        adapter.read_member(SEGMENT_PATH, 16 * 1024 * 1024).expect("read segment").expect("segment present");

    let mut entries = Vec::new();
    let mut first_failing: Option<u64> = None;
    let mut prev_hash = Digest::from_bytes([0u8; 32]);

    for (idx, line) in seg_bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()).enumerate() {
        let index = idx as u64;
        let Ok(val) = canon_parse(line) else {
            first_failing = Some(index);
            break;
        };
        let Value::Object(ref root) = val else {
            first_failing = Some(index);
            break;
        };
        let Some((_, header_val)) = root.iter().find(|(k, _)| k == "header") else {
            first_failing = Some(index);
            break;
        };
        let Value::Object(ref header) = header_val else {
            first_failing = Some(index);
            break;
        };
        let Ok(recomputed) = entry_hash_value(header_val) else {
            first_failing = Some(index);
            break;
        };
        let stored_hash = header.iter().find(|(k, _)| k == "hash").and_then(|(_, v)| match v {
            Value::String(s) => Digest::parse(s).ok(),
            _ => None,
        });
        let stored_prev = header.iter().find(|(k, _)| k == "prev").and_then(|(_, v)| match v {
            Value::String(s) => Digest::parse(s).ok(),
            _ => None,
        });
        if stored_hash != Some(recomputed) || stored_prev != Some(prev_hash) {
            first_failing = Some(index);
            break;
        }
        prev_hash = recomputed;
        let entry = parse_verified_entry_line(line, index).expect("parse verified entry");
        entries.push(entry);
    }

    let verified = VerifiedSegment::new(entries, first_failing);
    let results = link_package(&mut adapter, &verified).expect("link_package");
    (first_failing, results)
}

/// AC-1: given `fixtures/pkg/golden/points.zip`, every record is `LINKED`. Also confirms all
/// other golden Result Packages (`features.zip`, `results.zip`, `uec0.zip`, `two-signatures.zip`).
#[test]
fn ac1_golden_points_and_all_goldens_are_linked() {
    let (fail, results) = walk_segment_and_link(&pkg_goldens().join("points.zip"));
    assert_eq!(fail, None);
    assert_eq!(results.len(), 3);
    assert_eq!(
        results,
        vec![
            LinkResult { record_id: "hole-1".into(), status: LinkStatus::Linked, entry_index: Some(4), reason: None },
            LinkResult { record_id: "hole-2".into(), status: LinkStatus::Linked, entry_index: Some(5), reason: None },
            LinkResult { record_id: "hole-3".into(), status: LinkStatus::Linked, entry_index: Some(6), reason: None },
        ]
    );

    for golden in ["features.zip", "results.zip", "uec0.zip", "two-signatures.zip"] {
        let (f, res) = walk_segment_and_link(&pkg_goldens().join(golden));
        assert_eq!(f, None, "{golden} chain should be valid");
        assert!(!res.is_empty(), "{golden} should have records");
        for r in &res {
            assert_eq!(r.status, LinkStatus::Linked, "{golden}: record {} was {:?}", r.record_id, r);
            assert!(r.entry_index.is_some());
            assert_eq!(r.reason, None);
        }
    }
}

/// AC-2 (error path): given `fixtures/chain/reslink/foreign-hash.zip` (one record's hash
/// replaced by a random digest), exactly that record is `UNLINKED`.
#[test]
fn ac2_foreign_hash_marks_only_that_record_unlinked() {
    let (fail, results) = walk_segment_and_link(&reslink_fixtures().join("foreign-hash.zip"));
    assert_eq!(fail, None);
    assert_eq!(
        results,
        vec![
            LinkResult { record_id: "hole-1".into(), status: LinkStatus::Linked, entry_index: Some(4), reason: None },
            LinkResult { record_id: "hole-2".into(), status: LinkStatus::Unlinked, entry_index: None, reason: None },
            LinkResult { record_id: "hole-3".into(), status: LinkStatus::Linked, entry_index: Some(6), reason: None },
        ]
    );
}

/// AC-3 (error path): given `fixtures/chain/reslink/broken-at-5.zip` with a record linked to
/// entry 9 (after the break at index 5), that record is `UNLINKED`, while a record linked to
/// entry 4 (before the break) remains `LINKED` (REQ-TRUST.CHAIN.RESLINK-002).
#[test]
fn ac3_broken_at_5_unlinks_entry_9_and_keeps_entry_4() {
    let (fail, results) = walk_segment_and_link(&reslink_fixtures().join("broken-at-5.zip"));
    assert_eq!(fail, Some(5));
    assert_eq!(
        results,
        vec![
            LinkResult {
                record_id: "hole-early".into(),
                status: LinkStatus::Linked,
                entry_index: Some(4),
                reason: None,
            },
            LinkResult { record_id: "hole-late".into(), status: LinkStatus::Unlinked, entry_index: None, reason: None },
        ]
    );

    // Also verify parse_verified_segment directly enforces first_failing_index = Some(5).
    let zip_bytes = fs::read(reslink_fixtures().join("broken-at-5.zip")).unwrap();
    let mut reader = PackageReader::open(Cursor::new(zip_bytes)).unwrap();
    let seg_bytes = reader.read_member(SEGMENT_PATH, 1 << 20).unwrap().unwrap();
    let seg = parse_verified_segment(&seg_bytes, Some(5)).unwrap();
    assert_eq!(seg.len(), 5);
    assert_eq!(seg.first_failing_index(), Some(5));
}

/// AC-4 (error path): given `fixtures/chain/reslink/wrong-entry.zip` (record linked to a valid
/// but unrelated entry), reports `MISLINKED` with record ID and entry index.
#[test]
fn ac4_wrong_entry_reports_mislinked_with_index() {
    let (fail, results) = walk_segment_and_link(&reslink_fixtures().join("wrong-entry.zip"));
    assert_eq!(fail, None);
    assert_eq!(
        results,
        vec![
            LinkResult { record_id: "hole-1".into(), status: LinkStatus::Linked, entry_index: Some(4), reason: None },
            LinkResult {
                record_id: "hole-2".into(),
                status: LinkStatus::Mislinked,
                entry_index: Some(1),
                reason: Some(MislinkReason::NotResultProducingCommand),
            },
            LinkResult {
                record_id: "hole-3".into(),
                status: LinkStatus::Mislinked,
                entry_index: Some(4),
                reason: Some(MislinkReason::InputDigestMismatch),
            },
        ]
    );
}

/// REQ-TRUST.CHAIN.RESLINK-003: explicit `output_digest` on a ResultRecord must appear in the
/// linked entry's `outputs`; otherwise `MISLINKED` (`OutputDigestMissing`) is reported.
#[test]
fn explicit_output_digest_and_session_header_checks() {
    let h_session = Digest::from_bytes([0x01; 32]);
    let h_fit = Digest::from_bytes([0x02; 32]);
    let out_real = Digest::from_bytes([0xaa; 32]);
    let out_wrong = Digest::from_bytes([0xbb; 32]);
    let inp = Digest::from_bytes([0x10; 32]);

    let verified = VerifiedSegment::new(
        vec![
            VerifiedEntry {
                hash: h_session,
                index: 0,
                kind: EntryKind::SessionHeader,
                command_id: None,
                algorithms: Vec::new(),
                input_digests: Vec::new(),
                output_digests: Vec::new(),
            },
            VerifiedEntry {
                hash: h_fit,
                index: 1,
                kind: EntryKind::Command,
                command_id: Some("fit.ls_circle".into()),
                algorithms: vec!["fit.circle.ls@1.0.0".into()],
                input_digests: vec![inp],
                output_digests: vec![out_real],
            },
        ],
        None,
    );

    let records = vec![
        ParsedRecord {
            id: "rec-ok".into(),
            evaluation_basis: "POINTS".into(),
            algorithm: "fit.circle.ls@1.0.0".into(),
            journal_entry: h_fit,
            input_digests: vec![inp],
            output_digest: Some(out_real),
            canonical_digest: Digest::from_bytes([0u8; 32]),
            digest_without_journal_entry: Digest::from_bytes([0u8; 32]),
        },
        ParsedRecord {
            id: "rec-missing-output".into(),
            evaluation_basis: "POINTS".into(),
            algorithm: "fit.circle.ls@1.0.0".into(),
            journal_entry: h_fit,
            input_digests: vec![inp],
            output_digest: Some(out_wrong),
            canonical_digest: Digest::from_bytes([0u8; 32]),
            digest_without_journal_entry: Digest::from_bytes([0u8; 32]),
        },
        ParsedRecord {
            id: "rec-session-hdr".into(),
            evaluation_basis: "POINTS".into(),
            algorithm: "fit.circle.ls@1.0.0".into(),
            journal_entry: h_session,
            input_digests: vec![inp],
            output_digest: None,
            canonical_digest: Digest::from_bytes([0u8; 32]),
            digest_without_journal_entry: Digest::from_bytes([0u8; 32]),
        },
    ];

    let results = link_records(&records, &verified);
    assert_eq!(
        results[0],
        LinkResult { record_id: "rec-ok".into(), status: LinkStatus::Linked, entry_index: Some(1), reason: None }
    );
    assert_eq!(
        results[1],
        LinkResult {
            record_id: "rec-missing-output".into(),
            status: LinkStatus::Mislinked,
            entry_index: Some(1),
            reason: Some(MislinkReason::OutputDigestMissing),
        }
    );
    assert_eq!(
        results[2],
        LinkResult {
            record_id: "rec-session-hdr".into(),
            status: LinkStatus::Mislinked,
            entry_index: Some(0),
            reason: Some(MislinkReason::NotACommand),
        }
    );
}

/// Package reader errors pass through with their `pkg.*` code intact.
#[test]
fn package_errors_pass_through_unchanged() {
    struct FailingReader;
    impl PackageMemberReader for FailingReader {
        fn read_member(&mut self, _path: &str, _max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError> {
            Err(PackageError { code: "pkg.digest_mismatch".into(), detail: "tampered member".into() })
        }
    }
    let seg = VerifiedSegment::new(Vec::new(), None);
    let err = link_package(&mut FailingReader, &seg).unwrap_err();
    assert_eq!(err.code(), "pkg.digest_mismatch");
    assert!(matches!(err, ReslinkError::Package(_)));
}
