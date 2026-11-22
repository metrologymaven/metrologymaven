//! Acceptance and unit tests for `IO.RES.TRANS.VIEW` (`spec/io/res/trans/view/NODE.md`):
//! AC-1..AC-4 plus edge cases and integration with `IO.RES.TRANS.MODEL`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use fid_canon::{parse, CValue};
use fid_io_reg::{IoError, Position};
use fid_io_res::trans::model::{transcribe, SourceInfo, SourceRow, SourceStatus, SourceText};
use fid_io_res::trans::view::{
    assert_marked, expected_from_transcription, Mark, MarkExtractor, ResultId, TranscriptionMark, ViewError,
    MAX_LABEL_PART_CHARS, TABLE_COLUMN_HEADERS,
};
use fid_project::{IdService, Observation, ReplaySource, Uuid};
use fid_types::Digest;

fn fixture_mark_json() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/io/res/trans/view/mark.json")
}

struct EmptyIds;
impl fid_project::IdIndex for EmptyIds {
    fn contains_id(&self, _id: &Uuid) -> bool {
        false
    }
}

fn sample_transcription(
    software: &str,
    version: Option<&str>,
    count: usize,
) -> (fid_io_res::trans::model::TranscriptionRecord, Vec<fid_io_res::trans::model::TranscribedResult>) {
    let ns = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000001").unwrap();
    let mut src = ReplaySource::new(Vec::<Observation>::new());
    let empty = EmptyIds;
    let ids = IdService::new(ns, &mut src, &empty);
    let info = SourceInfo {
        software: software.to_string(),
        version: version.map(str::to_string),
        file_sha256: Digest::from_bytes([0x42; 32]),
        imported_at: "2026-11-22T07:00:00Z".to_string(),
        format: "csv".to_string(),
    };
    let rows: Vec<SourceRow> = (0..count)
        .map(|i| SourceRow {
            characteristic: Some(format!("C{}", i + 1)),
            value: Some("10.000".to_string()),
            unit: Some("mm".to_string()),
            status: SourceStatus::Pass,
            status_text: Some("PASS".to_string()),
            ..SourceRow::default()
        })
        .collect();
    let t = transcribe(&rows, &info, &ids).expect("transcribe");
    (t.record().clone(), t.results().to_vec())
}

/// Simple JSON-lines test exporter extractor parsing `{"id", "record_status"?, "source_software"?, "source_version"?}`.
struct JsonLinesExtractor;

impl MarkExtractor for JsonLinesExtractor {
    fn marks(&self, output: &[u8]) -> Result<Vec<(ResultId, Mark)>, IoError> {
        let mut out = Vec::new();
        for (line_no, raw) in output.split(|&b| b == b'\n').enumerate() {
            if raw.is_empty() {
                continue;
            }
            let val = parse(raw).map_err(|_| IoError::Malformed {
                at: Position::line_col(0, (line_no + 1) as u32, 1),
                reason: "invalid_json",
            })?;
            let CValue::Obj(o) = val else {
                return Err(IoError::Malformed {
                    at: Position::line_col(0, (line_no + 1) as u32, 1),
                    reason: "row_not_object",
                });
            };
            let Some(CValue::Str(id_str)) = o.get("id") else {
                return Err(IoError::Malformed {
                    at: Position::line_col(0, (line_no + 1) as u32, 1),
                    reason: "missing_id",
                });
            };
            let id = Uuid::parse(id_str).map_err(|_| IoError::Malformed {
                at: Position::line_col(0, (line_no + 1) as u32, 1),
                reason: "bad_uuid",
            })?;
            let get_str = |k: &str| match o.get(k) {
                Some(CValue::Str(s)) => Some(s.clone()),
                _ => None,
            };
            out.push((
                id,
                Mark {
                    record_status: get_str("record_status"),
                    source_software: get_str("source_software"),
                    source_version: get_str("source_version"),
                },
            ));
        }
        Ok(out)
    }
}

/// AC-1: Given a transcribed result from VendorX 4.2, the JSON fragment equals
/// `fixtures/io/res/trans/view/mark.json` and the label is `transcribed from VendorX 4.2`.
#[test]
fn ac1_vendorx_json_fragment_and_label_match_fixture() {
    let (record, results) = sample_transcription("VendorX", Some("4.2"), 1);
    let mark = TranscriptionMark::from_transcribed(&results[0], &record).expect("derive mark");

    let expected_raw = fs::read_to_string(fixture_mark_json()).expect("read mark.json");
    let got_json = String::from_utf8(mark.to_canonical_json().expect("canonical json")).unwrap();
    assert_eq!(got_json, expected_raw.trim_end_matches('\n'));

    assert_eq!(mark.one_line_label(), "transcribed from VendorX 4.2");

    let cols = mark.table_columns();
    assert_eq!(TABLE_COLUMN_HEADERS, ["record_status", "source_software", "source_version"]);
    assert_eq!(cols.record_status, "transcribed");
    assert_eq!(cols.source_software.as_deref(), Some("VendorX"));
    assert_eq!(cols.source_version.as_deref(), Some("4.2"));
}

/// AC-2: Given a software name of 300 characters containing a newline, the label holds
/// 64 escaped characters and no newline.
#[test]
fn ac2_long_software_name_with_newline_is_escaped_and_truncated_to_64_chars() {
    let mut name = String::from("Acme\nMeasure");
    while name.chars().count() < 300 {
        name.push('X');
    }
    assert_eq!(name.chars().count(), 300);
    assert!(name.contains('\n'));

    let mark = TranscriptionMark::transcribed(SourceText::new(&name), SourceText::new("1.0"));
    let label = mark.one_line_label();
    assert!(!label.contains('\n'), "label must never contain a raw newline");
    assert!(!label.contains('\r'), "label must never contain a raw carriage return");
    assert!(label.starts_with("transcribed from "));
    assert!(label.ends_with(" 1.0"));

    let sw_part = label.strip_prefix("transcribed from ").unwrap().strip_suffix(" 1.0").unwrap();
    assert_eq!(sw_part.chars().count(), MAX_LABEL_PART_CHARS);
    assert!(sw_part.contains("\\u{a}") || sw_part.contains("\\u{000a}"), "newline must be escaped: {sw_part}");
    assert!(sw_part.chars().all(|c| !c.is_control()), "no control chars in label");

    // Version is also escaped and truncated independently to 64 chars.
    let long_ver = format!("v\n{}", "9".repeat(100));
    let mark2 = TranscriptionMark::transcribed(SourceText::new("VendorX"), SourceText::new(&long_ver));
    let label2 = mark2.one_line_label();
    let ver_part = label2.strip_prefix("transcribed from VendorX ").unwrap();
    assert_eq!(ver_part.chars().count(), MAX_LABEL_PART_CHARS);
    assert!(!label2.contains('\n'));
}

/// AC-3 (error path): Given a test exporter output where one of three transcribed results
/// lacks the marking column, `assert_marked` fails with `transcription_unmarked` naming
/// that result's ID.
#[test]
fn ac3_missing_mark_on_one_of_three_transcribed_results_fails_with_result_id() {
    let (record, results) = sample_transcription("VendorX", Some("4.2"), 3);
    let expected = expected_from_transcription(&record, &results).expect("expected marks");
    assert_eq!(expected.len(), 3);

    // Build a fully marked output -> passes.
    let mut ok_buf = String::new();
    for (id, mark) in &expected {
        let cols = mark.table_columns();
        ok_buf.push_str(&format!(
            r#"{{"id":"{id}","record_status":"{}","source_software":"{}","source_version":"{}"}}"#,
            cols.record_status,
            cols.source_software.as_deref().unwrap(),
            cols.source_version.as_deref().unwrap(),
        ));
        ok_buf.push('\n');
    }
    assert_marked(&JsonLinesExtractor, ok_buf.as_bytes(), &expected).expect("all 3 marked passes");

    // Drop the marking columns on the second result -> fails naming results[1].id().
    let mut bad_buf = String::new();
    for (idx, (id, mark)) in expected.iter().enumerate() {
        if idx == 1 {
            bad_buf.push_str(&format!(r#"{{"id":"{id}"}}"#));
        } else {
            let cols = mark.table_columns();
            bad_buf.push_str(&format!(
                r#"{{"id":"{id}","record_status":"{}","source_software":"{}","source_version":"{}"}}"#,
                cols.record_status,
                cols.source_software.as_deref().unwrap(),
                cols.source_version.as_deref().unwrap(),
            ));
        }
        bad_buf.push('\n');
    }
    let err = assert_marked(&JsonLinesExtractor, bad_buf.as_bytes(), &expected)
        .expect_err("unmarked second result must fail");
    assert_eq!(err, ViewError::TranscriptionUnmarked { result: results[1].id() });
    assert_eq!(err.code(), "transcription_unmarked");
    assert!(err.to_string().contains(&results[1].id().to_string()));
}

/// AC-4: Given an evaluated (not transcribed) result, no renderer output contains
/// `transcribed`, and the conformance kit accepts it and rejects accidental `transcribed`
/// tagging.
#[test]
fn ac4_non_transcribed_result_never_emits_transcribed() {
    let mark = TranscriptionMark::non_transcribed("evaluated").expect("evaluated status");
    assert!(!mark.is_transcribed());
    assert_eq!(mark.record_status(), "evaluated");

    let json = String::from_utf8(mark.to_canonical_json().unwrap()).unwrap();
    assert_eq!(json, r#"{"record_status":"evaluated"}"#);
    assert!(!json.to_ascii_lowercase().contains("transcribed"));

    let cols = mark.table_columns();
    assert_eq!(cols.record_status, "evaluated");
    assert_eq!(cols.source_software, None);
    assert_eq!(cols.source_version, None);

    let label = mark.one_line_label();
    assert_eq!(label, "evaluated");
    assert!(!label.to_ascii_lowercase().contains("transcribed"));

    // Constructing a non-transcribed mark with "transcribed" fails closed.
    assert_eq!(TranscriptionMark::non_transcribed("transcribed").unwrap_err().code(), "invalid_record_status");
    assert_eq!(TranscriptionMark::non_transcribed("TRANSCRIBED_COPY").unwrap_err().code(), "invalid_record_status");

    // Conformance kit accepts evaluated output and rejects if mis-tagged as transcribed.
    let eval_id = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000100").unwrap();
    let expected = vec![(eval_id, mark)];
    let ok_out = format!(r#"{{"id":"{eval_id}","record_status":"evaluated"}}"#);
    assert_marked(&JsonLinesExtractor, ok_out.as_bytes(), &expected).expect("evaluated passes");

    let bad_out = format!(r#"{{"id":"{eval_id}","record_status":"transcribed"}}"#);
    let err = assert_marked(&JsonLinesExtractor, bad_out.as_bytes(), &expected).unwrap_err();
    assert_eq!(err, ViewError::UnexpectedTranscriptionMark { result: eval_id });
}

/// Record ID mismatch between a `TranscribedResult` and `TranscriptionRecord` fails closed.
#[test]
fn mismatched_record_refused() {
    let (_, results_a) = sample_transcription("VendorA", Some("1.0"), 1);
    let (record_b, _) = sample_transcription("VendorB", Some("2.0"), 1);
    // Different file_sha256 needed for different record IDs; build second with different digest.
    let ns = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000001").unwrap();
    let mut src = ReplaySource::new(Vec::<Observation>::new());
    let empty = EmptyIds;
    let ids = IdService::new(ns, &mut src, &empty);
    let info_c = SourceInfo {
        software: "VendorC".to_string(),
        version: Some("3.0".to_string()),
        file_sha256: Digest::from_bytes([0x99; 32]),
        imported_at: "2026-11-22T07:00:00Z".to_string(),
        format: "csv".to_string(),
    };
    let rows = vec![SourceRow {
        characteristic: Some("C1".to_string()),
        value: Some("1.0".to_string()),
        ..SourceRow::default()
    }];
    let t_c = transcribe(&rows, &info_c, &ids).unwrap();
    assert_ne!(record_b.id(), t_c.record().id());
    let err = TranscriptionMark::from_transcribed(&results_a[0], t_c.record()).unwrap_err();
    assert_eq!(err.code(), "record_mismatch");
}
