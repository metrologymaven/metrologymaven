//! Acceptance criteria of spec node IO.RES.TRANS.MODEL (AC-1 .. AC-5) plus the pinned
//! Design rules. The golden canonical JSON comes from
//! `fixtures/io/res/trans/model/make_fixtures.py` (stdlib Python: its own implementation
//! of the rules, with the `decimal` and `uuid` modules); outcomes, sides, drop reasons and
//! refusal reasons below are hand-labelled from the spec and NODE.md Design.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_res::trans::model::{
    load_rows, transcribe, DropReason, Dropped, EvaluationBasis, Outcome, Side, SourceConflict, SourceInfo, SourceRow,
    SourceStatus, TransError, Transcription, DECISION_RULE, ELEMENT_TYPES, MAX_TEXT_BYTES,
};
use fid_project::{IdService, ReplaySource, Uuid};
use std::path::{Path, PathBuf};

/// The project namespace the fixture script uses.
const NAMESPACE: &str = "0b6f8c1e-3d2a-4f5b-9c7e-a1b2c3d4e5f6";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/res/trans/model")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn run(rows: &[SourceRow], source: &SourceInfo) -> Result<Transcription, TransError> {
    let mut replay = ReplaySource::new(Vec::new());
    let existing: Vec<Uuid> = Vec::new();
    let ids = IdService::new(Uuid::parse(NAMESPACE).unwrap(), &mut replay, &existing);
    transcribe(rows, source, &ids)
}

fn fixture(name: &str) -> Transcription {
    let (source, rows) = load_rows(&read(&format!("{name}.json"))).unwrap();
    run(&rows, &source).unwrap()
}

fn outcomes(t: &Transcription) -> Vec<Outcome> {
    t.results.iter().map(|r| r.outcome()).collect()
}

#[test]
fn ac1_one_record_unknown_version_and_mapped_outcomes() {
    let t = fixture("rows_basic");
    assert!(t.record.source_software().is("VendorX"));
    assert!(t.record.source_version().is("unknown"));
    assert_eq!(t.record.imported_at(), "2026-11-17T10:00:00Z");
    assert_eq!(t.record.format(), "csv");
    assert_eq!(
        outcomes(&t),
        [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate, Outcome::NotDecidable, Outcome::Conforms]
    );
    let bases: Vec<_> = t.results.iter().map(|r| r.evaluation_basis()).collect();
    assert_eq!(bases[..4], [EvaluationBasis::ResultsOnly; 4]);
    assert_eq!(bases[4], EvaluationBasis::Points);
    assert!(t.dropped.is_empty());
}

#[test]
fn ac2_fixed_fields_and_golden_canonical_json() {
    for name in ["rows_basic", "rows_edge"] {
        let t = fixture(name);
        let digest = t.record.digest().unwrap();
        for r in &t.results {
            assert_eq!(r.status(), "transcribed");
            assert_eq!(r.uec(), "UEC-0");
            assert_eq!(r.decision_rule(), DECISION_RULE);
            assert_eq!(r.decision_rule(), "dr:as-reported-by-source@1");
            // Referenced by record ID and digest, and by the file digest.
            assert_eq!(r.record(), t.record.id());
            assert_eq!(r.record_digest(), digest);
            assert_eq!(r.input_digests(), [t.record.file_sha256()]);
            // Claimed, declared-input and uncharacterized-input facts on every result.
            assert!(r.is_claimed() && r.rests_on_declared_input() && r.rests_on_uncharacterized_input());
            let json = String::from_utf8(r.to_canonical_json().unwrap()).unwrap();
            assert!(json.contains(r#""algorithm":null"#) && json.contains(r#""kernel_version":null"#));
            assert!(json.contains(r#""uncertainty":{"uec":"UEC-0","uec0_reason":"u_missing"}"#));
            assert!(json.contains(r#""claim":{"claimed":true,"declared_input":true,"uncharacterized_input":true}"#));
        }
        assert_eq!(t.to_canonical_json().unwrap(), read(&format!("{name}.out.json")), "{name}");
    }
}

#[test]
fn ac3_authored_decimal_text_and_unit_kept() {
    let t = fixture("rows_edge");
    let r = &t.results[0];
    assert!(r.characteristic().is("E1"));
    assert!(r.value().is("0.0100"));
    assert!(r.unit().unwrap().is("mm"));
    assert!(r.as_reported().value.is("0.0100"));
    // Exponent and sign text is kept as written too.
    let e6 = t.results.iter().find(|r| r.characteristic().is("E6")).unwrap();
    assert!(e6.value().is("-0.5e-1"));
}

#[test]
fn ac4_source_uncertainty_stays_as_reported_text() {
    let t = fixture("rows_edge");
    let r = t.results.iter().find(|r| r.characteristic().is("E2")).unwrap();
    assert_eq!(r.uec(), "UEC-0");
    let a = r.as_reported();
    assert!(a.uncertainty.as_ref().unwrap().is("U=0.002 mm"));
    assert!(a.decision_rule.as_ref().unwrap().is("ISO 14253-1 simple"));
    assert!(a.outcome.as_ref().unwrap().is("in tolerance"));
    assert_eq!(r.decision_rule(), DECISION_RULE);
    let json = String::from_utf8(r.to_canonical_json().unwrap()).unwrap();
    assert!(!json.contains(r#""U":"#) && !json.contains("u_std"));
}

#[test]
fn ac5_incomplete_rows_dropped_and_the_rest_mapped() {
    let t = fixture("rows_edge");
    let d = |row, reason| Dropped { row, reason };
    assert_eq!(
        t.dropped,
        [
            d(2, DropReason::RowIncomplete),
            d(3, DropReason::RowIncomplete),
            d(4, DropReason::RowIncomplete),
            d(14, DropReason::DuplicateRowKey),
            d(16, DropReason::DuplicateRowKey),
            d(17, DropReason::ValueNotDecimal),
            d(18, DropReason::ValueNotDecimal),
            d(19, DropReason::FieldTooLong),
        ]
    );
    let rows: Vec<usize> = t.results.iter().map(|r| r.source_row()).collect();
    assert_eq!(rows, [0, 1, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 20]);
    assert_eq!(DropReason::RowIncomplete.as_str(), "row_incomplete");
}

#[test]
fn pinned_outcome_side_and_basis_rules() {
    let t = fixture("rows_edge");
    let by_row = |n: usize| t.results.iter().find(|r| r.source_row() == n).unwrap();
    let expect = [
        (0, Outcome::Conforms, Side::Inside, None, EvaluationBasis::ResultsOnly),
        (1, Outcome::Conforms, Side::Undecidable, None, EvaluationBasis::ResultsOnly),
        (5, Outcome::NotDecidable, Side::Undecidable, None, EvaluationBasis::Features),
        (6, Outcome::NotDecidable, Side::Undecidable, None, EvaluationBasis::Points),
        (7, Outcome::NotDecidable, Side::Undecidable, None, EvaluationBasis::ResultsOnly),
        (
            8,
            Outcome::NotDecidable,
            Side::AboveUpper,
            Some(SourceConflict::PassOutsideLimit),
            EvaluationBasis::ResultsOnly,
        ),
        (
            9,
            Outcome::NotDecidable,
            Side::BelowLower,
            Some(SourceConflict::PassOutsideLimit),
            EvaluationBasis::ResultsOnly,
        ),
        (10, Outcome::Nonconforms, Side::Inside, None, EvaluationBasis::ResultsOnly),
        (
            11,
            Outcome::NotDecidable,
            Side::AboveUpper,
            Some(SourceConflict::PassOutsideLimit),
            EvaluationBasis::ResultsOnly,
        ),
        (
            12,
            Outcome::NotDecidable,
            Side::BelowLower,
            Some(SourceConflict::PassOutsideLimit),
            EvaluationBasis::ResultsOnly,
        ),
        (13, Outcome::Conforms, Side::Undecidable, None, EvaluationBasis::ResultsOnly),
        (15, Outcome::NotDecidable, Side::Undecidable, None, EvaluationBasis::ResultsOnly),
        (20, Outcome::Conforms, Side::Undecidable, None, EvaluationBasis::ResultsOnly),
    ];
    for (row, outcome, side, conflict, basis) in expect {
        let r = by_row(row);
        assert_eq!(
            (r.outcome(), r.side(), r.source_conflict(), r.evaluation_basis()),
            (outcome, side, conflict, basis)
        );
    }
    // A violated limit is never shown with CONFORMS; an undecidable side is never decided.
    for r in &t.results {
        assert!(!r.side().is_violation() || r.outcome() != Outcome::Conforms);
    }
}

#[test]
fn ids_are_stable_uuidv5_and_independent_of_row_order() {
    let (source, mut rows) = load_rows(&read("rows_basic.json")).unwrap();
    let a = run(&rows, &source).unwrap();
    let again = run(&rows, &source).unwrap();
    assert_eq!(a, again);
    rows.reverse();
    let b = run(&rows, &source).unwrap();
    assert_eq!(a.record.id(), b.record.id());
    for r in &a.results {
        assert_eq!(r.id().version(), 5);
        let other = b.results.iter().find(|x| x.characteristic() == r.characteristic()).unwrap();
        assert_eq!(r.id(), other.id());
    }
    // Another file digest gives other IDs.
    let mut s2 = source.clone();
    s2.file_sha256 = fid_types::Digest::parse(&format!("sha256:{}", "1".repeat(64))).unwrap();
    let c = run(&rows, &s2).unwrap();
    assert_ne!(a.record.id(), c.record.id());
}

#[test]
fn rows_file_refusals() {
    let cases = [
        ("bad_dup_key.json", "duplicate_key", None),
        ("bad_case_dup_key.json", "case_duplicate_key", Some(0)),
        ("bad_number_value.json", "wrong_type", Some(0)),
        ("bad_status.json", "bad_status", Some(0)),
    ];
    for (file, reason, row) in cases {
        assert_eq!(load_rows(&read(file)).unwrap_err(), TransError::RowsInvalid { reason, row }, "{file}");
    }
    let src = r#""source":{"software":"S","file_sha256":"sha256:0000000000000000000000000000000000000000000000000000000000000000","imported_at":"2026-11-17T10:00:00Z","format":"csv"}"#;
    let doc = |body: &str| format!(r#"{{"schema":"transcription-rows@1",{src},{body}}}"#);
    let err = |s: String| load_rows(s.as_bytes()).unwrap_err();
    assert_eq!(err(doc(r#""rows":[],"Rows":[]"#)), TransError::RowsInvalid { reason: "case_duplicate_key", row: None });
    assert_eq!(err(doc(r#""rows":[],"extra":1"#)), TransError::RowsInvalid { reason: "unknown_key", row: None });
    assert_eq!(
        err(doc(r#""rows":[{"has_points":"yes"}]"#)),
        TransError::RowsInvalid { reason: "wrong_type", row: Some(0) }
    );
    assert_eq!(err(doc(r#""rows":{}"#)), TransError::RowsInvalid { reason: "wrong_type", row: None });
    let bad_schema = r#"{"schema":"transcription-rows@2","source":{},"rows":[]}"#;
    assert_eq!(
        load_rows(bad_schema.as_bytes()).unwrap_err(),
        TransError::RowsInvalid { reason: "unsupported_schema", row: None }
    );
    let bom = [&[0xEF, 0xBB, 0xBF][..], doc(r#""rows":[]"#).as_bytes()].concat();
    assert!(load_rows(&bom).is_err());
}

#[test]
fn bad_source_is_a_caller_error() {
    let (source, rows) = load_rows(&read("rows_basic.json")).unwrap();
    let with = |f: &dyn Fn(&mut SourceInfo)| {
        let mut s = source.clone();
        f(&mut s);
        run(&rows, &s).unwrap_err()
    };
    assert_eq!(with(&|s| s.software = " ".into()), TransError::BadSource("software"));
    assert_eq!(with(&|s| s.software = "x".repeat(MAX_TEXT_BYTES + 1)), TransError::BadSource("software"));
    assert_eq!(with(&|s| s.version = Some("x".repeat(MAX_TEXT_BYTES + 1))), TransError::BadSource("version"));
    assert_eq!(with(&|s| s.imported_at = "2026-11-17 10:00".into()), TransError::BadSource("imported_at"));
    assert_eq!(with(&|s| s.format = "CSV".into()), TransError::BadSource("format"));
    assert_eq!(TransError::BadSource("format").to_string(), "transcription_source_invalid{format}");
}

#[test]
fn version_text_is_kept_when_stated() {
    let (mut source, rows) = load_rows(&read("rows_basic.json")).unwrap();
    source.version = Some("4.2.1".into());
    assert!(run(&rows, &source).unwrap().record.source_version().is("4.2.1"));
}

#[test]
fn hostile_text_stays_typed_and_escaped() {
    let t = fixture("rows_edge");
    let r = t.results.iter().find(|r| r.source_row() == 20).unwrap();
    assert!(r.characteristic().is("E20\u{0}\u{202e}"));
    let shown = r.characteristic().escaped();
    assert!(!shown.chars().any(|c| c.is_control() || c == '\u{202e}'), "{shown}");
    let status = r.as_reported().status.as_ref().unwrap().escaped();
    assert!(!status.contains('\n') && !status.contains('\r'), "{status}");
    let dbg = format!("{:?}", t.record.source_software());
    assert!(!dbg.contains('\u{202e}') && !dbg.contains('\u{7}'), "{dbg}");
    // The canonical JSON escapes control characters.
    let json = r.to_canonical_json().unwrap();
    assert!(!json.iter().any(|b| *b < 0x20));
}

#[test]
fn element_types_registered() {
    let names: Vec<_> = ELEMENT_TYPES.iter().map(|e| (e.name, e.schema)).collect();
    assert_eq!(
        names,
        [("transcription_record", "transcription-record@1"), ("transcribed_result", "transcribed-result@1")]
    );
}

/// Bounded and panic-free on arbitrary rows and arbitrary rows-file bytes (deterministic
/// pseudo-random inputs).
#[test]
fn never_panics_on_hostile_input() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let alphabet = ["", " ", "0", "1", "-", "+", ".", "e", "E", "9", "\u{0}", "\u{202e}", "C", "c", "é", "1e999999999"];
    let mut pick = |n: usize| -> Option<String> {
        if next() % 5 == 0 {
            return None;
        }
        Some((0..n).map(|_| alphabet[(next() % alphabet.len() as u64) as usize]).collect())
    };
    let (source, _) = load_rows(&read("rows_basic.json")).unwrap();
    let statuses = [
        SourceStatus::Pass,
        SourceStatus::Fail,
        SourceStatus::Undecided,
        SourceStatus::Unrecognized,
        SourceStatus::Absent,
    ];
    let rows: Vec<SourceRow> = (0..2000)
        .map(|i| SourceRow {
            characteristic: pick(2),
            part: pick(1),
            value: pick(4),
            lower_limit: pick(3),
            upper_limit: pick(3),
            status: statuses[i % statuses.len()],
            has_points: i % 3 == 0,
            has_features: i % 2 == 0,
            ..SourceRow::default()
        })
        .collect();
    let t = run(&rows, &source).unwrap();
    assert_eq!(t.results.len() + t.dropped.len(), rows.len());
    for r in &t.results {
        assert!(!r.side().is_violation() || r.outcome() != Outcome::Conforms);
    }
    t.to_canonical_json().unwrap();
    let base = read("rows_edge.json");
    let mut next2 = {
        let mut s = 0x2545_f491_4f6c_dd1du64;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }
    };
    for _ in 0..300 {
        let mut b = base.clone();
        for _ in 0..4 {
            let i = (next2() % b.len() as u64) as usize;
            b[i] = (next2() & 0xff) as u8;
        }
        if let Ok((s, rows)) = load_rows(&b) {
            let _ = run(&rows, &s);
        }
    }
}
