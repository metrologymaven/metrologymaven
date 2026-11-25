//! Original locked built-in templates for AS9102 FAI forms 1, 2 and 3
//! (`RPT.FORMS.LAYOUT.FAI`, `REQ-001`..`REQ-007`, `AC-1`..`AC-5`, D-015 option C).
//!
//! All three templates are original project-authored arrangements with catalog message keys
//! written in the project's own words, `locked = true`, `clearance = pending` (until D-015
//! counsel review is recorded), and citations restricted strictly to standard designator,
//! revision and clause numbers (`AS9102 Rev C`). Zero floating-point types or arithmetic.

use crate::layout::map::{
    resolve_form_field_block, Citation, FieldMap, FormFieldPayload, FormKind, FormLayoutInfo, FormLayoutRegistry,
    FormNumericFormatter, FormTemplateEntry, LayoutError, ResolvedFormFieldValue, FORM_FIELD_KIND,
};
use fid_canon::{digest_bytes as canon_sha256, parse_with_limits, to_canonical_bytes, CValue};
use fid_report::bind::{FieldId, FieldSource, FieldValue};
use fid_report::doc::BlockId;
use fid_report::tmpl::format::{parse_template, Template};
use fid_report::tmpl::slot::validate_template;
use fid_report::tmpl::ver::BuiltinRegistry;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Built-in FAI template version.
pub const FAI_TEMPLATE_VERSION: u32 = 1;

/// Logical names of the three built-in locked FAI templates (`REQ-001`, `AC-1`).
pub const FAI_FORM1_NAME: &str = "tmpl:fai-form1";
pub const FAI_FORM2_NAME: &str = "tmpl:fai-form2";
pub const FAI_FORM3_NAME: &str = "tmpl:fai-form3";

/// Embedded canonical JSON template documents (`REQ-001`, D0 static data).
pub const FAI_FORM1_JSON: &[u8] = include_bytes!("../../templates/fai/fai-form1.json");
pub const FAI_FORM2_JSON: &[u8] = include_bytes!("../../templates/fai/fai-form2.json");
pub const FAI_FORM3_JSON: &[u8] = include_bytes!("../../templates/fai/fai-form3.json");

/// Embedded canonical JSON field maps (`REQ-001`, sorted by `field_id`).
pub const FAI_FORM1_FIELDMAP_JSON: &[u8] = include_bytes!("../../templates/fai/fai-form1.fieldmap.json");
pub const FAI_FORM2_FIELDMAP_JSON: &[u8] = include_bytes!("../../templates/fai/fai-form2.fieldmap.json");
pub const FAI_FORM3_FIELDMAP_JSON: &[u8] = include_bytes!("../../templates/fai/fai-form3.fieldmap.json");

/// Embedded English source catalog strings written in the project's own words (`REQ-002`).
pub const FAI_LABELS_EN_JSON: &[u8] = include_bytes!("../../templates/fai/labels.en.json");

/// Default maximum data rows per Form 3 page during pagination (`AC-3`).
pub const DEFAULT_ROWS_PER_PAGE: usize = 25;

/// Descriptor for one built-in FAI template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaiTemplateSpec {
    pub form: FormKind,
    pub logical_name: &'static str,
    pub template_json: &'static [u8],
    pub field_map_json: &'static [u8],
}

/// The three built-in locked FAI templates in form order (`REQ-001`).
pub const FAI_TEMPLATES: [FaiTemplateSpec; 3] = [
    FaiTemplateSpec {
        form: FormKind::FaiForm1,
        logical_name: FAI_FORM1_NAME,
        template_json: FAI_FORM1_JSON,
        field_map_json: FAI_FORM1_FIELDMAP_JSON,
    },
    FaiTemplateSpec {
        form: FormKind::FaiForm2,
        logical_name: FAI_FORM2_NAME,
        template_json: FAI_FORM2_JSON,
        field_map_json: FAI_FORM2_FIELDMAP_JSON,
    },
    FaiTemplateSpec {
        form: FormKind::FaiForm3,
        logical_name: FAI_FORM3_NAME,
        template_json: FAI_FORM3_JSON,
        field_map_json: FAI_FORM3_FIELDMAP_JSON,
    },
];

/// Parses the embedded English source label catalog (`labels.en.json`).
pub fn fai_labels_en() -> Result<BTreeMap<String, String>, LayoutError> {
    let val = parse_with_limits(FAI_LABELS_EN_JSON, 256 * 1024, 8)
        .map_err(|_| LayoutError::TemplateInvalid("malformed fai labels.en.json".to_string()))?;
    let CValue::Obj(o) = val else {
        return Err(LayoutError::TemplateInvalid("fai labels.en.json must be an object".to_string()));
    };
    let mut out = BTreeMap::new();
    for (k, v) in o {
        let CValue::Str(s) = v else {
            return Err(LayoutError::TemplateInvalid(format!("label {k} is not a string")));
        };
        out.insert(k, s);
    }
    Ok(out)
}

/// Parses and validates one built-in FAI template (`parse_template`, `validate_template`,
/// footer citation validation, and `FieldMap` validation).
pub fn load_fai_template(spec: FaiTemplateSpec) -> Result<(Template, FieldMap), LayoutError> {
    let template = parse_template(spec.template_json)
        .map_err(|e| LayoutError::TemplateInvalid(format!("{}: {e}", spec.logical_name)))?;
    validate_template(&template).map_err(|e| LayoutError::TemplateInvalid(format!("{}: {e}", spec.logical_name)))?;
    validate_footer_citations(&template)?;
    let field_map = FieldMap::from_json_bytes(spec.field_map_json)?;
    Ok((template, field_map))
}

/// Registers all three locked FAI templates (`tmpl:fai-form1`, `tmpl:fai-form2`,
/// `tmpl:fai-form3`) into `reg` and `builtins` (`REQ-001`..`REQ-006`, `AC-1`).
pub fn register_fai_templates(
    reg: &mut FormLayoutRegistry,
    builtins: &mut BuiltinRegistry,
) -> Result<Vec<FormLayoutInfo>, LayoutError> {
    let labels = fai_labels_en()?;
    let mut registered = Vec::with_capacity(FAI_TEMPLATES.len());
    for spec in FAI_TEMPLATES {
        let (template, field_map) = load_fai_template(spec)?;
        let info = reg.register(
            FormTemplateEntry {
                form: spec.form,
                logical_name: spec.logical_name.to_string(),
                template,
                version: FAI_TEMPLATE_VERSION,
                field_map,
                labels: labels.clone(),
            },
            builtins,
        )?;
        registered.push(info);
    }
    Ok(registered)
}

/// Validates that every `footer` block in `template` cites only a valid standard designator
/// and revision (no prose, <= 64 bytes; `REQ-003`, `AC-5`).
pub fn validate_footer_citations(template: &Template) -> Result<(), LayoutError> {
    for page in template.pages() {
        for block in page.blocks() {
            if block.kind().as_str() != "footer" {
                continue;
            }
            let CValue::Obj(o) = block.payload().value() else {
                return Err(LayoutError::CitationInvalid { detail: "footer payload must be an object".to_string() });
            };
            let des = match o.get("citation_designator") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => {
                    return Err(LayoutError::CitationInvalid {
                        detail: "footer missing citation_designator".to_string(),
                    })
                }
            };
            let rev = match o.get("citation_revision") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => {
                    return Err(LayoutError::CitationInvalid { detail: "footer missing citation_revision".to_string() })
                }
            };
            // Verify designator + revision strictly using Citation validator with a neutral clause.
            let _ = Citation::new(des, rev, "Form 1-3")?;
        }
    }
    Ok(())
}

/// One resolved table block inside a resolved FAI form report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFormTable {
    pub block_id: BlockId,
    pub table_id: String,
    pub columns: Vec<String>,
    pub repeat_header_rows: u8,
    pub continuation_block: Option<BlockId>,
    pub rows: Vec<BTreeMap<String, String>>,
}

/// One paginated page of a resolved FAI form report (`AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationPage {
    /// 1-based page number.
    pub page_number: usize,
    /// Total page count across the form.
    pub total_pages: usize,
    /// Whether the identifying continuation header block is repeated on this page.
    pub repeats_identifying_header: bool,
    /// Identifying header values shown on this page (`part_number`, `part_revision`,
    /// `fai_report_number`, `page_label`).
    pub identifying_header: BTreeMap<String, String>,
    /// Whether the table column header row is repeated on this page (`repeat_header_rows > 0`).
    pub repeats_table_header: bool,
    /// Row slice `[start_row..end_row)` shown on this page.
    pub start_row: usize,
    pub end_row: usize,
}

/// Deterministically resolved FAI form report (fields + expanded tables + pagination plan +
/// canonical SHA-256 digest; `REQ-007`, `AC-2`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFaiReport {
    pub form: FormKind,
    pub logical_name: String,
    pub fields: BTreeMap<String, String>,
    pub tables: Vec<ResolvedFormTable>,
    pub pages: Vec<ContinuationPage>,
    pub warnings: Vec<String>,
    pub canonical_digest: Digest,
}

/// Resolves an FAI form template against `sources` and `formatter`, expanding `form_field`
/// blocks and `field_table` row slots (sorting `form3_rows` in ascending balloon-number
/// order), building the continuation page plan, and computing the canonical report digest
/// (`REQ-004`, `REQ-005`, `REQ-007`, `AC-2`, `AC-3`).
pub fn resolve_fai_form(
    spec: FaiTemplateSpec,
    sources: &[&dyn FieldSource],
    formatter: &dyn FormNumericFormatter,
    rows_per_page: usize,
) -> Result<ResolvedFaiReport, LayoutError> {
    if rows_per_page == 0 {
        return Err(LayoutError::BadParams("rows_per_page must be >= 1"));
    }
    let (template, field_map) = load_fai_template(spec)?;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut tables: Vec<ResolvedFormTable> = Vec::new();
    let warnings: Vec<String> = Vec::new();

    for page in template.pages() {
        for block in page.blocks() {
            match block.kind().as_str() {
                FORM_FIELD_KIND => {
                    let payload = FormFieldPayload::from_value(block.payload().value())
                        .map_err(|e| LayoutError::TemplateInvalid(e.to_string()))?;
                    let rf = resolve_form_field_block(block.id(), &payload, &field_map, sources, formatter)?;
                    let rendered = match rf.value {
                        ResolvedFormFieldValue::Text(t) => t.untrusted_value().as_str().to_string(),
                        ResolvedFormFieldValue::Formatted(vt) => vt.text().canonical.to_string(),
                        ResolvedFormFieldValue::Missing => String::new(),
                    };
                    fields.insert(rf.field_id.as_str().to_string(), rendered);
                }
                "table" => {
                    let CValue::Obj(o) = block.payload().value() else {
                        return Err(LayoutError::TemplateInvalid("table payload must be object".to_string()));
                    };
                    let table_id = match o.get("table_id") {
                        Some(CValue::Str(s)) => s.clone(),
                        _ => return Err(LayoutError::TemplateInvalid("table missing table_id".to_string())),
                    };
                    let columns = extract_column_keys(o)?;
                    let table_fid = FieldId::new(&table_id)
                        .ok_or_else(|| LayoutError::TemplateInvalid(format!("bad table_id {table_id}")))?;
                    let mut raw_rows = Vec::new();
                    for src in sources {
                        let got = src.rows(&table_fid);
                        if !got.is_empty() {
                            raw_rows = got;
                            break;
                        }
                    }
                    let mut rows = Vec::with_capacity(raw_rows.len());
                    for row in raw_rows {
                        let mut out_row = BTreeMap::new();
                        for col in &columns {
                            let col_fid = FieldId::new(col)
                                .ok_or_else(|| LayoutError::TemplateInvalid(format!("bad column key {col}")))?;
                            let cell_str = match row.get(&col_fid) {
                                None => String::new(),
                                Some(FieldValue::Text(t)) => t.untrusted_value().as_str().to_string(),
                                Some(FieldValue::Result(id)) => {
                                    formatter.format_result(id)?.text().canonical.to_string()
                                }
                            };
                            out_row.insert(col.clone(), cell_str);
                        }
                        rows.push(out_row);
                    }
                    if table_id == "form3_rows" {
                        rows.sort_by(|a, b| {
                            let ba = a.get("balloon").map(String::as_str).unwrap_or("");
                            let bb = b.get("balloon").map(String::as_str).unwrap_or("");
                            cmp_balloon(ba, bb)
                        });
                    }
                    tables.push(ResolvedFormTable {
                        block_id: block.id(),
                        table_id,
                        columns,
                        repeat_header_rows: block.repeat_header_rows(),
                        continuation_block: block.continuation_block(),
                        rows,
                    });
                }
                _ => {}
            }
        }
    }

    let pages = paginate_form(&fields, &tables, rows_per_page);
    let canon_val = resolved_to_value(spec, &fields, &tables, &pages);
    let canon_bytes = to_canonical_bytes(&canon_val)
        .map_err(|_| LayoutError::TemplateInvalid("canonical encoding failed".to_string()))?;
    let canonical_digest = Digest::from_bytes(canon_sha256(&canon_bytes).0);

    Ok(ResolvedFaiReport {
        form: spec.form,
        logical_name: spec.logical_name.to_string(),
        fields,
        tables,
        pages,
        warnings,
        canonical_digest,
    })
}

fn extract_column_keys(o: &BTreeMap<String, CValue>) -> Result<Vec<String>, LayoutError> {
    let Some(CValue::Arr(cols)) = o.get("columns") else {
        return Err(LayoutError::TemplateInvalid("table missing columns array".to_string()));
    };
    let mut out = Vec::with_capacity(cols.len());
    for c in cols {
        let CValue::Obj(co) = c else {
            return Err(LayoutError::TemplateInvalid("column entry must be object".to_string()));
        };
        let Some(CValue::Str(key)) = co.get("key") else {
            return Err(LayoutError::TemplateInvalid("column missing key".to_string()));
        };
        out.push(key.clone());
    }
    Ok(out)
}

/// Numeric-aware ascending balloon comparison (`"2"` < `"10"`, `"10a"` < `"10b"`).
fn cmp_balloon(a: &str, b: &str) -> std::cmp::Ordering {
    fn split_prefix_num(s: &str) -> (Option<u64>, &str) {
        let idx = s.bytes().take_while(u8::is_ascii_digit).count();
        if idx == 0 {
            (None, s)
        } else {
            (s[..idx].parse::<u64>().ok(), &s[idx..])
        }
    }
    match (split_prefix_num(a), split_prefix_num(b)) {
        ((Some(na), ra), (Some(nb), rb)) => na.cmp(&nb).then_with(|| ra.cmp(rb)),
        _ => a.cmp(b),
    }
}

fn paginate_form(
    fields: &BTreeMap<String, String>,
    tables: &[ResolvedFormTable],
    rows_per_page: usize,
) -> Vec<ContinuationPage> {
    let primary = tables.first();
    let total_rows = primary.map_or(0, |t| t.rows.len());
    let total_pages = if total_rows == 0 { 1 } else { total_rows.div_ceil(rows_per_page) };
    let repeat_header = primary.is_some_and(|t| t.repeat_header_rows > 0);
    let has_cont_block = primary.is_some_and(|t| t.continuation_block.is_some());

    let mut pages = Vec::with_capacity(total_pages);
    for idx in 0..total_pages {
        let page_number = idx + 1;
        let start_row = idx * rows_per_page;
        let end_row = (start_row + rows_per_page).min(total_rows);
        let mut hdr = BTreeMap::new();
        hdr.insert("part_number".to_string(), fields.get("part_number").cloned().unwrap_or_default());
        hdr.insert("part_revision".to_string(), fields.get("part_revision").cloned().unwrap_or_default());
        hdr.insert("fai_report_number".to_string(), fields.get("fai_report_number").cloned().unwrap_or_default());
        hdr.insert("page_n_of_m".to_string(), format!("Page {page_number} of {total_pages}"));
        pages.push(ContinuationPage {
            page_number,
            total_pages,
            repeats_identifying_header: idx > 0 && has_cont_block,
            identifying_header: hdr,
            repeats_table_header: idx > 0 && repeat_header,
            start_row,
            end_row,
        });
    }
    pages
}

fn resolved_to_value(
    spec: FaiTemplateSpec,
    fields: &BTreeMap<String, String>,
    tables: &[ResolvedFormTable],
    pages: &[ContinuationPage],
) -> CValue {
    let mut fm = BTreeMap::new();
    for (k, v) in fields {
        fm.insert(k.clone(), CValue::Str(v.clone()));
    }
    let table_vals: Vec<CValue> = tables
        .iter()
        .map(|t| {
            let mut m = BTreeMap::new();
            m.insert("columns".to_string(), CValue::Arr(t.columns.iter().map(|c| CValue::Str(c.clone())).collect()));
            m.insert(
                "continuation_block".to_string(),
                t.continuation_block.map_or(CValue::Null, |id| CValue::Str(id.uuid().to_string())),
            );
            m.insert("repeat_header_rows".to_string(), CValue::Str(t.repeat_header_rows.to_string()));
            m.insert(
                "rows".to_string(),
                CValue::Arr(
                    t.rows
                        .iter()
                        .map(|r| {
                            let mut rm = BTreeMap::new();
                            for (ck, cv) in r {
                                rm.insert(ck.clone(), CValue::Str(cv.clone()));
                            }
                            CValue::Obj(rm)
                        })
                        .collect(),
                ),
            );
            m.insert("table_id".to_string(), CValue::Str(t.table_id.clone()));
            CValue::Obj(m)
        })
        .collect();
    let page_vals: Vec<CValue> = pages
        .iter()
        .map(|p| {
            let mut m = BTreeMap::new();
            m.insert("end_row".to_string(), CValue::Str(p.end_row.to_string()));
            let mut hm = BTreeMap::new();
            for (k, v) in &p.identifying_header {
                hm.insert(k.clone(), CValue::Str(v.clone()));
            }
            m.insert("identifying_header".to_string(), CValue::Obj(hm));
            m.insert("page_number".to_string(), CValue::Str(p.page_number.to_string()));
            m.insert("repeats_identifying_header".to_string(), CValue::Bool(p.repeats_identifying_header));
            m.insert("repeats_table_header".to_string(), CValue::Bool(p.repeats_table_header));
            m.insert("start_row".to_string(), CValue::Str(p.start_row.to_string()));
            m.insert("total_pages".to_string(), CValue::Str(p.total_pages.to_string()));
            CValue::Obj(m)
        })
        .collect();
    let mut root = BTreeMap::new();
    root.insert("fields".to_string(), CValue::Obj(fm));
    root.insert("form".to_string(), CValue::Str(spec.form.as_str().to_string()));
    root.insert("logical_name".to_string(), CValue::Str(spec.logical_name.to_string()));
    root.insert("pages".to_string(), CValue::Arr(page_vals));
    root.insert("tables".to_string(), CValue::Arr(table_vals));
    CValue::Obj(root)
}
