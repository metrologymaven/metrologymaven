//! Integration tests for `RPT.FORMS.LAYOUT.FAI` (`AC-1`..`AC-5`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::disallowed_types)]

use fid_canon::{parse_with_limits, CValue};
use fid_project::Uuid;
use fid_report::bind::{FieldId, FieldSource, FieldValue, RowSource};
use fid_report::doc::{Text, Untrusted};
use fid_report::fmt::ValueText;
use fid_report::tmpl::ver::BuiltinRegistry;
use fid_report_forms::layout::fai::{
    fai_labels_en, load_fai_template, register_fai_templates, resolve_fai_form, validate_footer_citations,
    DEFAULT_ROWS_PER_PAGE, FAI_FORM1_NAME, FAI_FORM2_NAME, FAI_FORM3_NAME, FAI_TEMPLATES,
};
use fid_report_forms::layout::{
    check_cleanroom_labels, CleanroomPhraseSet, FormClearance, FormLayoutRegistry, FormNumericFormatter, LayoutError,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct FixtureSource {
    fields: BTreeMap<FieldId, FieldValue>,
    tables: BTreeMap<FieldId, Vec<RowSource>>,
}

impl FixtureSource {
    fn from_json(bytes: &[u8]) -> FixtureSource {
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
        let mut tables = BTreeMap::new();
        let table_mappings =
            [("form1_assemblies", "form1_assemblies"), ("form2_items", "form2_items"), ("form3_rows", "form3_rows")];
        for (json_key, table_id) in table_mappings {
            if let Some(CValue::Arr(arr)) = root.get(json_key) {
                let mut rows = Vec::with_capacity(arr.len());
                for item in arr {
                    let CValue::Obj(ro) = item else { panic!("row must be object") };
                    let mut row = BTreeMap::new();
                    for (ck, cv) in ro {
                        if let CValue::Str(s) = cv {
                            row.insert(
                                FieldId::new(ck).unwrap(),
                                FieldValue::Text(Untrusted::new(Text::new(s).unwrap())),
                            );
                        }
                    }
                    rows.push(row);
                }
                tables.insert(FieldId::new(table_id).unwrap(), rows);
            }
        }
        FixtureSource { fields, tables }
    }
}

impl FieldSource for FixtureSource {
    fn field(&self, id: &FieldId) -> Option<FieldValue> {
        self.fields.get(id).cloned()
    }

    fn rows(&self, table: &FieldId) -> Vec<RowSource> {
        self.tables.get(table).cloned().unwrap_or_default()
    }
}

struct NoopFormatter;

impl FormNumericFormatter for NoopFormatter {
    fn format_result(&self, record_id: &Uuid) -> Result<ValueText, LayoutError> {
        Err(LayoutError::FormatterFailed(format!("unexpected result id {record_id}")))
    }
}

/// AC-1: `show-layout-fai` lists `tmpl:fai-form1`, `tmpl:fai-form2` and `tmpl:fai-form3`,
/// each locked, each with `clearance = pending` and a valid sorted field map.
#[test]
fn ac1_show_layout_fai_lists_three_locked_templates_with_valid_field_maps() {
    let mut reg = FormLayoutRegistry::new();
    let mut builtins = BuiltinRegistry::new();
    let registered = register_fai_templates(&mut reg, &mut builtins).expect("register all 3 FAI templates");
    assert_eq!(registered.len(), 3);

    let fai = reg.show_layout_fai();
    assert_eq!(fai.len(), 3);
    let names: Vec<&str> = fai.iter().map(|i| i.logical_name.as_str()).collect();
    assert_eq!(names, vec![FAI_FORM1_NAME, FAI_FORM2_NAME, FAI_FORM3_NAME]);

    for info in &fai {
        assert!(info.locked);
        assert!(info.template.builtin);
        assert_eq!(info.clearance, FormClearance::Pending);
        info.field_map.validate().expect("field map valid");
    }
}

/// AC-2: Resolving all three FAI templates against `fixtures/rpt/forms/fai_complete` matches
/// the committed golden digests in `fixtures/rpt/forms/golden/fai-form{1,2,3}.digest`.
#[test]
fn ac2_fai_complete_resolves_to_committed_golden_digests() {
    let fix_bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/fai_complete/fai_complete.json")).unwrap();
    let src = FixtureSource::from_json(&fix_bytes);
    let fmt = NoopFormatter;
    let golden_dir = repo_root().join("fixtures/rpt/forms/golden");

    let names = ["fai-form1.digest", "fai-form2.digest", "fai-form3.digest"];
    for (spec, golden_name) in FAI_TEMPLATES.iter().zip(names.iter()) {
        let rep1 = resolve_fai_form(*spec, &[&src], &fmt, DEFAULT_ROWS_PER_PAGE).unwrap();
        let rep2 = resolve_fai_form(*spec, &[&src], &fmt, DEFAULT_ROWS_PER_PAGE).unwrap();
        assert_eq!(rep1.canonical_digest, rep2.canonical_digest, "D0 determinism failed for {}", spec.logical_name);
        assert!(rep1.warnings.is_empty());

        let expected = std::fs::read_to_string(golden_dir.join(golden_name))
            .unwrap_or_else(|e| panic!("missing golden {golden_name}: {e}"));
        assert_eq!(rep1.canonical_digest.as_str(), expected.trim(), "golden digest mismatch for {}", spec.logical_name);
    }
}

/// AC-3: Given `fixtures/rpt/forms/fai_500rows` (500 balloons), Form 3 resolves in ascending
/// balloon-number order with zero warnings, and every continuation page after page 1 repeats
/// both the column header row (`repeat_header_rows == 1`) and the identifying header block
/// (`part_number`, `part_revision`, `fai_report_number`, `page n of m`).
#[test]
fn ac3_form3_500_rows_repeats_headers_on_every_continuation_page_without_warnings() {
    let fix_bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/fai_500rows/fai_500rows.json")).unwrap();
    let src = FixtureSource::from_json(&fix_bytes);
    let fmt = NoopFormatter;

    let rep = resolve_fai_form(FAI_TEMPLATES[2], &[&src], &fmt, DEFAULT_ROWS_PER_PAGE).unwrap();
    assert!(rep.warnings.is_empty(), "expected 0 warnings, got {:?}", rep.warnings);
    assert_eq!(rep.tables.len(), 1);
    let tbl = &rep.tables[0];
    assert_eq!(tbl.table_id, "form3_rows");
    assert_eq!(tbl.repeat_header_rows, 1);
    assert!(tbl.continuation_block.is_some());
    assert_eq!(tbl.rows.len(), 500);
    assert_eq!(tbl.columns, vec!["balloon", "requirement", "result", "decision", "basis", "producing_tool"]);
    assert_eq!(tbl.rows[0].get("balloon").map(String::as_str), Some("1"));
    assert_eq!(tbl.rows[9].get("balloon").map(String::as_str), Some("10"));
    assert_eq!(tbl.rows[499].get("balloon").map(String::as_str), Some("500"));

    // 500 rows / 25 per page = 20 pages.
    assert_eq!(rep.pages.len(), 20);
    assert!(!rep.pages[0].repeats_table_header);
    assert!(!rep.pages[0].repeats_identifying_header);
    for (idx, page) in rep.pages.iter().enumerate().skip(1) {
        assert_eq!(page.page_number, idx + 1);
        assert_eq!(page.total_pages, 20);
        assert!(page.repeats_table_header, "page {} must repeat table header", page.page_number);
        assert!(page.repeats_identifying_header, "page {} must repeat identifying header", page.page_number);
        assert_eq!(page.identifying_header.get("part_number").map(String::as_str), Some("MM-HOUSING-5000"));
        assert_eq!(page.identifying_header.get("part_revision").map(String::as_str), Some("A"));
        assert_eq!(page.identifying_header.get("fai_report_number").map(String::as_str), Some("FAI-2026-5000"));
        assert_eq!(
            page.identifying_header.get("page_n_of_m").map(String::as_str),
            Some(format!("Page {} of 20", page.page_number).as_str())
        );
    }
}

/// AC-4 & AC-5: Clean-room label check passes across all shipped FAI English labels, and
/// every citation in field maps and footers validates as designator + revision + clause only.
#[test]
fn ac4_and_ac5_cleanroom_labels_and_citations_pass() {
    let phrase_bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/cleanroom_phrases.json")).unwrap();
    let forbidden = CleanroomPhraseSet::from_json_bytes(&phrase_bytes).unwrap();
    let labels = fai_labels_en().unwrap();
    assert!(!labels.is_empty());
    check_cleanroom_labels(labels.iter().map(|(k, v)| (k.as_str(), v.as_str())), &forbidden)
        .expect("all shipped FAI labels must pass clean-room check");

    for spec in FAI_TEMPLATES {
        let (tmpl, fmap) = load_fai_template(spec).unwrap();
        validate_footer_citations(&tmpl).unwrap();
        fmap.validate().unwrap();
        for entry in &fmap.fields {
            if let Some(cit) = &entry.citation {
                cit.validate().unwrap();
                assert!(cit.rendered().len() <= 64);
                assert_eq!(cit.designator, "AS9102");
                assert_eq!(cit.revision, "Rev C");
            }
        }
    }
}
