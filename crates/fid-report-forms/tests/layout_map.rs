//! Integration tests for `RPT.FORMS.LAYOUT.MAP` (`AC-1`..`AC-6`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::disallowed_types)]

use fid_canon::{parse_with_limits, CValue};
use fid_io_reg::{Principal, RootSet};
use fid_num::fmt::{FormatterId, Unit};
use fid_project::Uuid;
use fid_report::bind::{FieldId, FieldSource, FieldValue, RowSource};
use fid_report::doc::kind::{BlockKind, KindRegistry};
use fid_report::doc::{CanonicalJson, Text, Untrusted};
use fid_report::fmt::{
    fmt_requirement, Catalog, Characteristic, DefaultDecimals, FmtContext, LocaleRules, SiValue, UnitOverride,
    ValueKind, ValueText,
};
use fid_report::tmpl::format::{parse_template, TemplateId};
use fid_report::tmpl::ver::{
    save_template, BuiltinRegistry, Env, MemoryTemplateBackend, SaveTemplate, Source, VerError,
};
use fid_report_forms::hdr::cleanroom_phrase_digest;
use fid_report_forms::layout::{
    check_cleanroom_labels, check_release_clearance, command_defs, resolve_template_fields, BuildProfile, Citation,
    CleanroomPhraseSet, FieldEntry, FieldMap, FormClearance, FormFieldKind, FormFieldResolver, FormKind,
    FormLayoutRegistry, FormNumericFormatter, FormTemplateEntry, LayoutError, ResolvedFormFieldValue, SourcePath,
    FIELD_MAP_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fid(s: &str) -> FieldId {
    FieldId::new(s).expect("valid field id")
}

fn sample_locked_template_bytes(template_id: &str, block_id: &str, field_id: &str, clearance_json: &str) -> Vec<u8> {
    format!(
        r#"{{"clearance":{clearance_json},"locked":true,"media_type":"application/vnd.fid.report-template+json","pages":[{{"blocks":[{{"continuation_block":null,"id":"{block_id}","kind":"form_field","kind_version":"1","payload":{{"untrusted":true,"value":{{"field_id":"{field_id}","show_label":true}}}},"repeat_header_rows":"0","slot":null}}],"id":"019a9f10-0001-7000-8000-000000000011"}}],"schema_version":"1","template_id":"{template_id}","title":{{"text":"report.forms.test.title","untrusted":true}}}}"#
    )
    .into_bytes()
}

type FieldSpec<'a> = (&'a str, &'a str, &'a str, Option<(&'a str, &'a str, &'a str)>);

fn make_entry(
    form: FormKind,
    logical_name: &str,
    template_id: &str,
    block_id: &str,
    field_ids: &[FieldSpec<'_>],
    clearance_json: &str,
) -> FormTemplateEntry {
    let first_field = field_ids[0].0;
    let tmpl = parse_template(&sample_locked_template_bytes(template_id, block_id, first_field, clearance_json))
        .expect("valid template");
    let mut fields = Vec::new();
    let mut labels = BTreeMap::new();
    for (id, lkey, src, cit) in field_ids {
        let citation = cit.map(|(d, r, c)| Citation::new(d, r, c).expect("valid citation"));
        fields.push(FieldEntry {
            field_id: fid(id),
            label_key: (*lkey).to_string(),
            source: SourcePath::new(src).expect("valid source"),
            citation,
        });
        labels.insert((*lkey).to_string(), format!("Project authored label for {id}"));
    }
    FormTemplateEntry {
        form,
        logical_name: logical_name.to_string(),
        template: tmpl,
        version: 1,
        field_map: FieldMap { schema_version: FIELD_MAP_SCHEMA_VERSION, form, fields },
        labels,
    }
}

fn populated_registry() -> (FormLayoutRegistry, BuiltinRegistry) {
    let mut reg = FormLayoutRegistry::new();
    let mut builtins = BuiltinRegistry::new();
    let pending = r#"{"state":"pending"}"#;

    reg.register(
        make_entry(
            FormKind::FaiForm1,
            "tmpl:fai-form1",
            "019a9f10-0001-7000-8000-000000000001",
            "019a9f10-0001-7000-8000-000000000101",
            &[
                (
                    "part_name",
                    "report.forms.fai.part_name",
                    "fai.header.part_name",
                    Some(("AS9102", "Rev C", "Form 1, Field 2")),
                ),
                (
                    "part_number",
                    "report.forms.fai.part_number",
                    "fai.header.part_number",
                    Some(("AS9102", "Rev C", "Form 1, Field 1")),
                ),
            ],
            pending,
        ),
        &mut builtins,
    )
    .expect("register form 1");

    reg.register(
        make_entry(
            FormKind::FaiForm2,
            "tmpl:fai-form2",
            "019a9f10-0002-7000-8000-000000000001",
            "019a9f10-0002-7000-8000-000000000101",
            &[(
                "material_spec",
                "report.forms.fai.material_spec",
                "fai.form2.material_spec",
                Some(("AS9102", "Rev C", "Form 2, Field 5")),
            )],
            pending,
        ),
        &mut builtins,
    )
    .expect("register form 2");

    reg.register(
        make_entry(
            FormKind::FaiForm3,
            "tmpl:fai-form3",
            "019a9f10-0003-7000-8000-000000000001",
            "019a9f10-0003-7000-8000-000000000101",
            &[(
                "balloon_number",
                "report.forms.fai.balloon_number",
                "fai.form3.balloon_number",
                Some(("AS9102", "Rev C", "Form 3, Field 5")),
            )],
            pending,
        ),
        &mut builtins,
    )
    .expect("register form 3");

    reg.register(
        make_entry(
            FormKind::PpapDim,
            "tmpl:ppap-dim",
            "019a9f10-0004-7000-8000-000000000001",
            "019a9f10-0004-7000-8000-000000000101",
            &[(
                "part_number",
                "report.forms.ppap.part_number",
                "ppap.header.part_number",
                Some(("AIAG PPAP", "4th Ed.", "Section 2.2.9")),
            )],
            pending,
        ),
        &mut builtins,
    )
    .expect("register ppap");

    (reg, builtins)
}

/// AC-1: `show-layout-fai` lists three templates (forms 1, 2, 3), each locked with sorted field
/// map, digest, and clearance state; `show-layout-ppap` lists one; CommandDefs validate cleanly.
#[test]
fn ac1_show_layout_fai_and_ppap_return_sorted_maps_digests_and_clearance() {
    let (reg, _builtins) = populated_registry();

    let fai = reg.show_layout_fai();
    assert_eq!(fai.len(), 3);
    assert_eq!(fai[0].form, FormKind::FaiForm1);
    assert_eq!(fai[1].form, FormKind::FaiForm2);
    assert_eq!(fai[2].form, FormKind::FaiForm3);
    for info in &fai {
        assert!(info.locked);
        assert!(info.template.builtin);
        assert_eq!(info.clearance, FormClearance::Pending);
        assert_eq!(info.field_map_digest, info.field_map.digest().unwrap());
        let ids: Vec<&str> = info.field_map.fields.iter().map(|e| e.field_id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    let ppap = reg.show_layout_ppap();
    assert_eq!(ppap.len(), 1);
    assert_eq!(ppap[0].form, FormKind::PpapDim);
    assert!(ppap[0].locked);

    // JSON view and CommandDef schema verification.
    let fai_json = reg.show_layout_fai_json();
    let CValue::Obj(o) = fai_json else { panic!("expected object") };
    let Some(CValue::Arr(arr)) = o.get("layouts") else { panic!("expected layouts array") };
    assert_eq!(arr.len(), 3);

    let defs = command_defs().expect("CommandDefs must validate");
    assert_eq!(defs.len(), 2);
    assert_eq!(defs[0].id, "report.show_layout_fai");
    assert_eq!(defs[1].id, "report.show_layout_ppap");
}

/// AC-2: Saving over a built-in locked form template ID via `RPT.MODEL.TMPL.VER` fails with
/// `report.template_locked` and appends nothing to the journal.
#[test]
fn ac2_save_over_locked_builtin_form_template_fails_template_locked_and_journals_nothing() {
    let (reg, builtins) = populated_registry();
    let form1_id = TemplateId::new(reg.show_layout_fai()[0].template.id).unwrap();

    let project_id = Uuid::parse("019a9f99-0001-7000-8000-000000000001").unwrap();
    let mut backend = MemoryTemplateBackend::new(project_id);
    let mut kinds = KindRegistry::new();
    kinds.register(Box::new(FormFieldKind::new().unwrap())).unwrap();
    let roots = RootSet::new();
    let env = Env { kinds: &kinds, builtins: &builtins, roots: &roots, principal: Principal::Human };
    let params = SaveTemplate {
        template: form1_id,
        source: Source::Report(Uuid::parse("019a9f99-0002-7000-8000-000000000001").unwrap()),
    };

    let err = save_template(&mut backend, &env, &params).expect_err("must refuse locked built-in");
    assert!(matches!(err, VerError::Locked(id) if id == form1_id));
    assert_eq!(err.code(), "report.template_locked");
    assert!(backend.journal().is_empty(), "must journal nothing on locked rejection");
}

/// AC-3: Fixture `fixtures/rpt/forms/bad_fieldmaps` rejects duplicate field IDs (reporting the
/// duplicate), prose/overlong citations (`report.citation_invalid`), and unknown `form_field`
/// block references (`report.field_unknown` naming the block).
#[test]
fn ac3_bad_fieldmaps_rejects_duplicate_prose_citation_and_unknown_field() {
    let dir = repo_root().join("fixtures/rpt/forms/bad_fieldmaps");

    // 1. Duplicate field ID
    let dup_bytes = std::fs::read(dir.join("duplicate_field.json")).unwrap();
    let dup_err = FieldMap::from_json_bytes(&dup_bytes).expect_err("duplicate must fail");
    assert_eq!(dup_err.code(), "report.fieldmap_invalid");
    assert!(
        matches!(&dup_err, LayoutError::DuplicateField { field_id } if field_id == "part_name"),
        "expected DuplicateField(part_name), got {dup_err:?}"
    );

    // 2. Prose citation & over-64-byte citation
    let prose_bytes = std::fs::read(dir.join("prose_citation.json")).unwrap();
    let prose_err = FieldMap::from_json_bytes(&prose_bytes).expect_err("prose citation must fail");
    assert_eq!(prose_err.code(), "report.citation_invalid");

    let long_bytes = std::fs::read(dir.join("too_long_citation.json")).unwrap();
    let long_err = FieldMap::from_json_bytes(&long_bytes).expect_err("overlong citation must fail");
    assert_eq!(long_err.code(), "report.citation_invalid");

    // 3. Unknown field in form_field block
    let unknown_tmpl_bytes = std::fs::read(dir.join("unknown_block_field.json")).unwrap();
    let unknown_tmpl = parse_template(&unknown_tmpl_bytes).expect("template syntax valid");
    let valid_map = FieldMap {
        schema_version: FIELD_MAP_SCHEMA_VERSION,
        form: FormKind::FaiForm1,
        fields: vec![FieldEntry {
            field_id: fid("part_number"),
            label_key: "report.forms.hdr.part_number".to_string(),
            source: SourcePath::new("fai.header.part_number").unwrap(),
            citation: Some(Citation::new("AS9102", "Rev C", "Form 1, Field 1").unwrap()),
        }],
    };
    let stub = StubSource::default();
    let fmt = StubFormatter::default();
    let err =
        resolve_template_fields(&unknown_tmpl, &valid_map, &[&stub], &fmt).expect_err("unknown field_id must fail");
    assert_eq!(err.code(), "report.field_unknown");
    assert!(
        matches!(&err, LayoutError::FieldUnknown { block, field_id }
            if block.uuid().to_string() == "019a9f10-bad1-7000-8000-000000000101"
            && field_id == "unmapped_ghost_field"),
        "expected FieldUnknown naming block & field, got {err:?}"
    );

    // Also verified at registration time via FormLayoutRegistry.
    let mut reg = FormLayoutRegistry::new();
    let mut builtins = BuiltinRegistry::new();
    let reg_err = reg
        .register(
            FormTemplateEntry {
                form: FormKind::FaiForm1,
                logical_name: "tmpl:bad-form".to_string(),
                template: unknown_tmpl,
                version: 1,
                field_map: valid_map,
                labels: BTreeMap::new(),
            },
            &mut builtins,
        )
        .expect_err("registration must reject unknown form_field");
    assert_eq!(reg_err.code(), "report.field_unknown");
}

#[derive(Default)]
struct StubSource {
    fields: BTreeMap<FieldId, FieldValue>,
}

impl FieldSource for StubSource {
    fn field(&self, id: &FieldId) -> Option<FieldValue> {
        self.fields.get(id).cloned()
    }

    fn rows(&self, _table: &FieldId) -> Vec<RowSource> {
        Vec::new()
    }
}

#[derive(Default)]
struct StubFormatter {
    records: BTreeMap<Uuid, (f64, Unit, u8)>,
}

impl FormNumericFormatter for StubFormatter {
    fn format_result(&self, record_id: &Uuid) -> Result<ValueText, LayoutError> {
        let Some(&(val_m, unit, decimals)) = self.records.get(record_id) else {
            return Err(LayoutError::FormatterFailed(format!("unknown record {record_id}")));
        };
        let ch = Characteristic {
            id: record_id.to_string(),
            kind: Some(ValueKind::Length),
            nominal: Some(SiValue::new(val_m)),
            unit,
            decimals: Some(decimals),
            limits: None,
        };
        let locale = LocaleRules::new("en", "test-1", '.', Some(','), 3, 3, "-").unwrap();
        let catalog = Catalog::english();
        let ctx = FmtContext {
            locale,
            units: UnitOverride::NONE,
            default_decimals: DefaultDecimals::initial(),
            catalog: &catalog,
        };
        fmt_requirement(&ch, &ctx).map_err(|e| LayoutError::FormatterFailed(e.to_string()))
    }
}

/// AC-4: Given `fixtures/rpt/forms/fai_complete` and a stub `FieldSource`, resolving a template
/// with `form_field` blocks for `part_number` (text) and `bore_diameter` (numeric) produces
/// `Untrusted<Text>` for text and a `ValueText` carrying a formatter ID from `RPT.MODEL.FMT`
/// (`fmt.length@1`) for the numeric field.
#[test]
fn ac4_resolve_text_as_untrusted_and_numeric_via_rpt_model_fmt() {
    let fix_bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/fai_complete/fai_complete.json")).unwrap();
    let CValue::Obj(root) = parse_with_limits(&fix_bytes, 64 * 1024, 16).unwrap() else {
        panic!("expected object");
    };
    let CValue::Obj(hdr) = root.get("header").unwrap() else { panic!("header") };
    let CValue::Str(part_number) = hdr.get("part_number").unwrap() else { panic!("part_number") };
    let CValue::Obj(nums) = root.get("numeric_fields").unwrap() else { panic!("numeric_fields") };
    let CValue::Obj(bore) = nums.get("bore_diameter_nominal_mm").unwrap() else { panic!("bore") };
    let CValue::Str(rec_str) = bore.get("record_id").unwrap() else { panic!("record_id") };
    let rec_uuid = Uuid::parse(rec_str).unwrap();

    let tmpl_json = r#"{
      "clearance": {"state": "pending"},
      "locked": true,
      "media_type": "application/vnd.fid.report-template+json",
      "pages": [{
        "id": "019a9f20-0001-7000-8000-000000000011",
        "blocks": [
          {
            "continuation_block": null,
            "id": "019a9f20-0001-7000-8000-000000000101",
            "kind": "form_field",
            "kind_version": "1",
            "payload": {"untrusted": true, "value": {"field_id": "part_number", "show_label": true}},
            "repeat_header_rows": "0",
            "slot": null
          },
          {
            "continuation_block": null,
            "id": "019a9f20-0001-7000-8000-000000000102",
            "kind": "form_field",
            "kind_version": "1",
            "payload": {"untrusted": true, "value": {"field_id": "bore_diameter", "show_label": true}},
            "repeat_header_rows": "0",
            "slot": null
          }
        ]
      }],
      "schema_version": "1",
      "template_id": "019a9f20-0001-7000-8000-000000000002",
      "title": {"text": "report.forms.fai.form1.title", "untrusted": true}
    }"#;
    let tmpl = parse_template(tmpl_json.as_bytes()).unwrap();

    let field_map = FieldMap {
        schema_version: FIELD_MAP_SCHEMA_VERSION,
        form: FormKind::FaiForm1,
        fields: vec![
            FieldEntry {
                field_id: fid("bore_diameter"),
                label_key: "report.forms.fai.bore_diameter".to_string(),
                source: SourcePath::new("fai.form3.bore_diameter").unwrap(),
                citation: Some(Citation::new("AS9102", "Rev C", "Form 3, Field 9").unwrap()),
            },
            FieldEntry {
                field_id: fid("part_number"),
                label_key: "report.forms.hdr.part_number".to_string(),
                source: SourcePath::new("fai.header.part_number").unwrap(),
                citation: Some(Citation::new("AS9102", "Rev C", "Form 1, Field 1").unwrap()),
            },
        ],
    };

    let mut source = StubSource::default();
    source.fields.insert(fid("part_number"), FieldValue::Text(Untrusted::new(Text::new(part_number).unwrap())));
    source.fields.insert(fid("bore_diameter"), FieldValue::Result(rec_uuid));

    let mut formatter = StubFormatter::default();
    formatter.records.insert(rec_uuid, (0.025, Unit::Mm, 3));

    let resolved = resolve_template_fields(&tmpl, &field_map, &[&source], &formatter).unwrap();
    assert_eq!(resolved.len(), 2);

    // Text field stays Untrusted<Text>.
    match &resolved[0].value {
        ResolvedFormFieldValue::Text(t) => assert_eq!(t.untrusted_value().as_str(), "MM-BRKT-1042"),
        other => panic!("expected Untrusted<Text>, got {other:?}"),
    }

    // Numeric field comes from RPT.MODEL.FMT with FormatterId::LENGTH_V1 (fmt.length@1).
    match &resolved[1].value {
        ResolvedFormFieldValue::Formatted(vt) => {
            assert_eq!(vt.formatter(), FormatterId::LENGTH_V1);
            assert_eq!(vt.text().display, "25.000");
            assert_eq!(vt.unit(), Unit::Mm);
        }
        other => panic!("expected Formatted(ValueText), got {other:?}"),
    }

    // Also verify FormFieldKind & FormFieldResolver construction.
    let kind = FormFieldKind::new().unwrap();
    assert_eq!(kind.id().as_str(), "form_field");
    assert_eq!(kind.version(), 1);
    let payload_val = parse_with_limits(br#"{"field_id":"part_number","show_label":true}"#, 1024, 4).unwrap();
    let canon = CanonicalJson::new(&payload_val).unwrap();
    kind.validate(&canon).unwrap();
    assert!(kind.value_fields(&canon).unwrap().is_empty());

    let resolver = FormFieldResolver::new(field_map).unwrap();
    assert_eq!(resolver.field_map().fields.len(), 2);
}

/// AC-5 (clearance gate): A pending form template fails under `BuildProfile::Release` naming
/// the template, and passes with a warning under `BuildProfile::Default`; a cleared template
/// passes both profiles with zero warnings.
#[test]
fn ac5_clearance_gate_blocks_release_and_warns_in_default() {
    let (reg, _builtins) = populated_registry();
    let all = reg.all();

    let rel_err = check_release_clearance(&all, BuildProfile::Release)
        .expect_err("release profile must reject pending form templates");
    assert_eq!(rel_err.code(), "report.clearance_pending");
    match &rel_err {
        LayoutError::ClearancePending { templates } => {
            assert!(templates.contains(&"tmpl:fai-form1".to_string()));
            assert!(templates.contains(&"tmpl:ppap-dim".to_string()));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let def_report =
        check_release_clearance(&all, BuildProfile::Default).expect("default profile passes with warnings");
    assert_eq!(def_report.pending_templates.len(), 4);
    assert_eq!(def_report.warnings.len(), 4);

    // Cleared template passes release cleanly.
    let mut cleared_reg = FormLayoutRegistry::new();
    let mut builtins = BuiltinRegistry::new();
    let cleared = r#"{"review_ref":{"text":"D-015-counsel-2026-11","untrusted":true},"state":"cleared"}"#;
    cleared_reg
        .register(
            make_entry(
                FormKind::PpapDim,
                "tmpl:ppap-dim-cleared",
                "019a9f10-0005-7000-8000-000000000001",
                "019a9f10-0005-7000-8000-000000000101",
                &[(
                    "part_number",
                    "report.forms.ppap.part_number",
                    "ppap.header.part_number",
                    Some(("AIAG PPAP", "4th Ed.", "Section 2.2.9")),
                )],
                cleared,
            ),
            &mut builtins,
        )
        .unwrap();
    let ok = check_release_clearance(&cleared_reg.all(), BuildProfile::Release).unwrap();
    assert!(ok.pending_templates.is_empty());
    assert!(ok.warnings.is_empty());
}

/// AC-6 (clean room): A label whose normalised SHA-256 appears in the forbidden phrase hash
/// set fails naming the offending label key; the maintained fixture file loads cleanly and
/// accepts our project-authored labels.
#[test]
fn ac6_cleanroom_label_check_catches_forbidden_hash_naming_label_key() {
    let fixture_bytes = std::fs::read(repo_root().join("fixtures/rpt/forms/cleanroom_phrases.json")).unwrap();
    let shipped_set = CleanroomPhraseSet::from_json_bytes(&fixture_bytes).unwrap();
    let (reg, _builtins) = populated_registry();
    check_cleanroom_labels(reg.labels().iter().map(|(k, v)| (k.as_str(), v.as_str())), &shipped_set)
        .expect("project-authored labels must pass maintained clean-room check");

    // Synthetic forbidden hash test (no standards text anywhere; pure synthetic phrase).
    let synthetic_forbidden = cleanroom_phrase_digest("Synthetic Prohibited Test Phrase");
    let test_set = CleanroomPhraseSet::from_digests([synthetic_forbidden]);

    let labels = [
        ("report.forms.ok_label", "Own Words Heading"),
        ("report.forms.bad_label", "  SYNTHETIC   prohibited, test-phrase! "),
    ];
    let err = check_cleanroom_labels(labels, &test_set).expect_err("must reject forbidden hash");
    assert_eq!(err.code(), "report.cleanroom_violation");
    assert!(
        matches!(&err, LayoutError::CleanroomViolation { label_key, digest }
            if label_key == "report.forms.bad_label" && *digest == synthetic_forbidden),
        "expected CleanroomViolation naming report.forms.bad_label, got {err:?}"
    );
}
