//! Integration tests for `RPT.FORMS.LAYOUT.PPAP` (`AC-1`..`AC-4`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::disallowed_types)]

use fid_canon::{parse_with_limits, CValue};
use fid_project::Uuid;
use fid_report::bind::{FieldId, FieldSource, FieldValue, RowSource};
use fid_report::doc::{Text, Untrusted};
use fid_report::fmt::ValueText;
use fid_report::tmpl::ver::BuiltinRegistry;
use fid_report_forms::layout::fai::validate_footer_citations;
use fid_report_forms::layout::ppap::{
    load_ppap_template, ppap_labels_en, register_all_builtin_forms, register_ppap_template, resolve_ppap_form,
    DEFAULT_MAX_PARTS_PER_GROUP, DEFAULT_PPAP_ROWS_PER_PAGE, PPAP_DIM_NAME,
};
use fid_report_forms::layout::{
    check_cleanroom_labels, CleanroomPhraseSet, FormClearance, FormLayoutRegistry, FormNumericFormatter, LayoutError,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct PpapFixtureSource {
    fields: BTreeMap<FieldId, FieldValue>,
    parts: Vec<String>,
    rows: Vec<RowSource>,
}

impl PpapFixtureSource {
    fn from_json(bytes: &[u8]) -> PpapFixtureSource {
        let CValue::Obj(root) = parse_with_limits(bytes, 4 * 1024 * 1024, 16).unwrap() else {
            panic!("fixture root must be object");
        };
        let mut fields = BTreeMap::new();
        if let Some(CValue::Obj(hdr)) = root.get("header") {
            for (k, v) in hdr {
                if let CValue::Str(s) = v {
                    fields.insert(FieldId::new(k).unwrap(), FieldValue::Text(Untrusted::new(Text::new(s).unwrap())));
                }
            }
        }
        let mut parts = Vec::new();
        if let Some(CValue::Arr(arr)) = root.get("parts") {
            for item in arr {
                if let CValue::Str(s) = item {
                    parts.push(s.clone());
                }
            }
        }
        let mut rows = Vec::new();
        if let Some(CValue::Arr(arr)) = root.get("ppap_rows") {
            for item in arr {
                let CValue::Obj(ro) = item else { panic!("row must be object") };
                let mut row = BTreeMap::new();
                for (ck, cv) in ro {
                    if let CValue::Str(s) = cv {
                        row.insert(FieldId::new(ck).unwrap(), FieldValue::Text(Untrusted::new(Text::new(s).unwrap())));
                    }
                }
                rows.push(row);
            }
        }
        PpapFixtureSource { fields, parts, rows }
    }
}

impl FieldSource for PpapFixtureSource {
    fn field(&self, id: &FieldId) -> Option<FieldValue> {
        self.fields.get(id).cloned()
    }

    fn rows(&self, table: &FieldId) -> Vec<RowSource> {
        if table.as_str() == "ppap_rows" {
            self.rows.clone()
        } else {
            Vec::new()
        }
    }
}

struct NoopFormatter;

impl FormNumericFormatter for NoopFormatter {
    fn format_result(&self, record_id: &Uuid) -> Result<ValueText, LayoutError> {
        Err(LayoutError::FormatterFailed(format!("unexpected result id {record_id}")))
    }
}

/// AC-1: `show-layout-ppap` lists `tmpl:ppap-dim`, locked, with `clearance = pending` and a
/// valid field map; `register_all_builtin_forms` registers all 3 FAI + 1 PPAP forms cleanly.
#[test]
fn ac1_show_layout_ppap_lists_locked_ppap_dim_with_valid_field_map() {
    let mut reg = FormLayoutRegistry::new();
    let mut builtins = BuiltinRegistry::new();
    let info = register_ppap_template(&mut reg, &mut builtins).expect("register ppap-dim");
    assert_eq!(info.logical_name, PPAP_DIM_NAME);
    assert!(info.locked);
    assert_eq!(info.clearance, FormClearance::Pending);
    info.field_map.validate().unwrap();

    let ppap = reg.show_layout_ppap();
    assert_eq!(ppap.len(), 1);
    assert_eq!(ppap[0].logical_name, PPAP_DIM_NAME);

    // Combined registration registers 3 FAI + 1 PPAP.
    let mut all_reg = FormLayoutRegistry::new();
    let mut all_builtins = BuiltinRegistry::new();
    let all = register_all_builtin_forms(&mut all_reg, &mut all_builtins).unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all_reg.show_layout_fai().len(), 3);
    assert_eq!(all_reg.show_layout_ppap().len(), 1);
}

/// AC-2: Fixture `ppap_6parts_complete` resolves deterministically to committed golden digest
/// `fixtures/rpt/forms/golden/ppap-dim.digest`.
#[test]
fn ac2_ppap_6parts_complete_matches_golden_digest() {
    let bytes =
        std::fs::read(repo_root().join("fixtures/rpt/forms/ppap_6parts_complete/ppap_6parts_complete.json")).unwrap();
    let src = PpapFixtureSource::from_json(&bytes);
    let fmt = NoopFormatter;

    let rep1 =
        resolve_ppap_form(&[&src], &fmt, &src.parts, DEFAULT_MAX_PARTS_PER_GROUP, DEFAULT_PPAP_ROWS_PER_PAGE).unwrap();
    let rep2 =
        resolve_ppap_form(&[&src], &fmt, &src.parts, DEFAULT_MAX_PARTS_PER_GROUP, DEFAULT_PPAP_ROWS_PER_PAGE).unwrap();
    assert_eq!(rep1.canonical_digest, rep2.canonical_digest);
    assert_eq!(rep1.column_groups.len(), 1);
    assert_eq!(
        rep1.column_groups[0].columns,
        vec!["balloon", "spec", "part_1", "part_2", "part_3", "part_4", "part_5", "part_6", "outcome"]
    );

    let golden_path = repo_root().join("fixtures/rpt/forms/golden/ppap-dim.digest");
    let expected =
        std::fs::read_to_string(&golden_path).unwrap_or_else(|e| panic!("missing golden ppap-dim.digest: {e}"));
    assert_eq!(rep1.canonical_digest.as_str(), expected.trim());
}

/// AC-3: Fixture `ppap_10parts` (10 sample parts, 80 rows) splits parts 1–6 and 7–10 into two
/// column groups each repeating `balloon`, `spec`, and `outcome`, and repeats the column header
/// and identifying header block on every continuation page.
#[test]
fn ac3_ppap_10parts_80rows_splits_into_two_column_groups_with_repeating_headers() {
    let bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/ppap_10parts/ppap_10parts.json")).unwrap();
    let src = PpapFixtureSource::from_json(&bytes);
    let fmt = NoopFormatter;

    let rep =
        resolve_ppap_form(&[&src], &fmt, &src.parts, DEFAULT_MAX_PARTS_PER_GROUP, DEFAULT_PPAP_ROWS_PER_PAGE).unwrap();
    assert!(rep.warnings.is_empty());
    assert_eq!(rep.repeat_header_rows, 1);
    assert!(rep.continuation_block.is_some());

    // Two column groups: parts 1..=6 and 7..=10, each repeating balloon, spec, outcome.
    assert_eq!(rep.column_groups.len(), 2);
    assert_eq!(
        rep.column_groups[0].columns,
        vec!["balloon", "spec", "part_1", "part_2", "part_3", "part_4", "part_5", "part_6", "outcome"]
    );
    assert_eq!(
        rep.column_groups[1].columns,
        vec!["balloon", "spec", "part_7", "part_8", "part_9", "part_10", "outcome"]
    );
    assert_eq!(rep.column_groups[0].rows.len(), 80);
    assert_eq!(rep.column_groups[1].rows.len(), 80);
    assert_eq!(rep.column_groups[0].rows[0].get("balloon").map(String::as_str), Some("1"));
    assert_eq!(rep.column_groups[0].rows[9].get("balloon").map(String::as_str), Some("10"));
    assert_eq!(rep.column_groups[0].rows[79].get("balloon").map(String::as_str), Some("80"));

    // 80 rows / 25 per page = 4 pages per group * 2 groups = 8 total pages.
    assert_eq!(rep.pages.len(), 8);
    assert!(!rep.pages[0].repeats_table_header);
    assert!(!rep.pages[0].repeats_identifying_header);
    for (idx, page) in rep.pages.iter().enumerate().skip(1) {
        assert_eq!(page.page_number, idx + 1);
        assert_eq!(page.total_pages, 8);
        assert!(page.repeats_table_header);
        assert!(page.repeats_identifying_header);
        assert_eq!(page.identifying_header.get("part_number").map(String::as_str), Some("MM-FLG-210"));
        assert_eq!(page.identifying_header.get("part_revision").map(String::as_str), Some("B"));
        assert_eq!(page.identifying_header.get("submission_id").map(String::as_str), Some("PPAP-2026-010"));
        assert_eq!(
            page.identifying_header.get("page_n_of_m").map(String::as_str),
            Some(format!("Page {} of 8", page.page_number).as_str())
        );
    }
}

/// AC-4 (clean room): All shipped PPAP labels pass clean-room verification and every citation
/// names the AIAG PPAP manual by edition and section only.
#[test]
fn ac4_ppap_cleanroom_labels_and_citations_pass() {
    let phrase_bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/cleanroom_phrases.json")).unwrap();
    let forbidden = CleanroomPhraseSet::from_json_bytes(&phrase_bytes).unwrap();
    let labels = ppap_labels_en().unwrap();
    assert!(!labels.is_empty());
    check_cleanroom_labels(labels.iter().map(|(k, v)| (k.as_str(), v.as_str())), &forbidden).unwrap();

    let (tmpl, fmap) = load_ppap_template().unwrap();
    validate_footer_citations(&tmpl).unwrap();
    fmap.validate().unwrap();
    for entry in &fmap.fields {
        let cit = entry.citation.as_ref().expect("ppap field citation");
        cit.validate().unwrap();
        assert_eq!(cit.designator, "AIAG PPAP");
        assert_eq!(cit.revision, "4th Ed.");
        assert!(cit.clause.starts_with("Section "));
    }
}
