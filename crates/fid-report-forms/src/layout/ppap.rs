//! Original locked built-in template for the PPAP dimensional results report
//! (`RPT.FORMS.LAYOUT.PPAP`, `REQ-001`..`REQ-005`, `AC-1`..`AC-4`, D-015 option C).
//!
//! Original arrangement citing the AIAG PPAP manual by edition and section only (`AIAG PPAP
//! 4th Ed.`), `locked = true`, `clearance = pending` until D-015 counsel review is recorded.
//! Supports multi-part column groups (`max_columns_per_group = 6` by default, repeating
//! `balloon`, `spec`, and `outcome`) and multi-page continuation headers. Zero floats.

use crate::layout::fai::{register_fai_templates, validate_footer_citations};
use crate::layout::map::{
    resolve_form_field_block, FieldMap, FormFieldPayload, FormKind, FormLayoutInfo, FormLayoutRegistry,
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

/// Built-in PPAP template version.
pub const PPAP_TEMPLATE_VERSION: u32 = 1;
/// Logical name of the built-in locked PPAP dimensional template (`REQ-001`, `AC-1`).
pub const PPAP_DIM_NAME: &str = "tmpl:ppap-dim";
/// Default sample-part column limit per page group (`REQ-004`).
pub const DEFAULT_MAX_PARTS_PER_GROUP: usize = 6;
/// Default data rows per page during pagination (`AC-3`).
pub const DEFAULT_PPAP_ROWS_PER_PAGE: usize = 25;

/// Embedded canonical JSON template document for `tmpl:ppap-dim`.
pub const PPAP_DIM_JSON: &[u8] = include_bytes!("../../templates/ppap/ppap-dim.json");
/// Embedded canonical JSON field map for `tmpl:ppap-dim`.
pub const PPAP_DIM_FIELDMAP_JSON: &[u8] = include_bytes!("../../templates/ppap/ppap-dim.fieldmap.json");
/// Embedded English source catalog strings written in the project's own words (`REQ-002`).
pub const PPAP_LABELS_EN_JSON: &[u8] = include_bytes!("../../templates/ppap/labels.en.json");

/// Parses the embedded English source label catalog for PPAP (`labels.en.json`).
pub fn ppap_labels_en() -> Result<BTreeMap<String, String>, LayoutError> {
    let val = parse_with_limits(PPAP_LABELS_EN_JSON, 256 * 1024, 8)
        .map_err(|_| LayoutError::TemplateInvalid("malformed ppap labels.en.json".to_string()))?;
    let CValue::Obj(o) = val else {
        return Err(LayoutError::TemplateInvalid("ppap labels.en.json must be an object".to_string()));
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

/// Parses and validates the built-in `tmpl:ppap-dim` template, its slot, footer citation, and
/// field map.
pub fn load_ppap_template() -> Result<(Template, FieldMap), LayoutError> {
    let template =
        parse_template(PPAP_DIM_JSON).map_err(|e| LayoutError::TemplateInvalid(format!("{PPAP_DIM_NAME}: {e}")))?;
    validate_template(&template).map_err(|e| LayoutError::TemplateInvalid(format!("{PPAP_DIM_NAME}: {e}")))?;
    validate_footer_citations(&template)?;
    let field_map = FieldMap::from_json_bytes(PPAP_DIM_FIELDMAP_JSON)?;
    Ok((template, field_map))
}

/// Registers the locked built-in `tmpl:ppap-dim` template into `reg` and `builtins` (`AC-1`).
pub fn register_ppap_template(
    reg: &mut FormLayoutRegistry,
    builtins: &mut BuiltinRegistry,
) -> Result<FormLayoutInfo, LayoutError> {
    let labels = ppap_labels_en()?;
    let (template, field_map) = load_ppap_template()?;
    reg.register(
        FormTemplateEntry {
            form: FormKind::PpapDim,
            logical_name: PPAP_DIM_NAME.to_string(),
            template,
            version: PPAP_TEMPLATE_VERSION,
            field_map,
            labels,
        },
        builtins,
    )
}

/// Registers all four built-in locked industry form templates (AS9102 FAI Forms 1–3 and PPAP
/// Dimensional) into `reg` and `builtins`.
pub fn register_all_builtin_forms(
    reg: &mut FormLayoutRegistry,
    builtins: &mut BuiltinRegistry,
) -> Result<Vec<FormLayoutInfo>, LayoutError> {
    let mut out = register_fai_templates(reg, builtins)?;
    out.push(register_ppap_template(reg, builtins)?);
    Ok(out)
}

/// One column group of the PPAP dimensional table (`REQ-003`, `REQ-004`, `AC-3`).
///
/// Every group contains `[balloon, spec, <sample_parts...>, outcome]` so `balloon`, `spec`
/// and `outcome` repeat across every column group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpapColumnGroup {
    pub group_index: usize,
    pub sample_parts: Vec<String>,
    pub columns: Vec<String>,
    pub rows: Vec<BTreeMap<String, String>>,
}

/// One paginated page of a resolved PPAP dimensional report (`REQ-005`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpapPage {
    pub page_number: usize,
    pub total_pages: usize,
    pub column_group_index: usize,
    pub repeats_identifying_header: bool,
    pub identifying_header: BTreeMap<String, String>,
    pub repeats_table_header: bool,
    pub start_row: usize,
    pub end_row: usize,
}

/// Deterministically resolved PPAP dimensional report (`AC-2`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPpapReport {
    pub logical_name: String,
    pub table_block_id: BlockId,
    pub repeat_header_rows: u8,
    pub continuation_block: Option<BlockId>,
    pub fields: BTreeMap<String, String>,
    pub column_groups: Vec<PpapColumnGroup>,
    pub pages: Vec<PpapPage>,
    pub warnings: Vec<String>,
    pub canonical_digest: Digest,
}

/// Resolves `tmpl:ppap-dim` against `sources`, `formatter`, and ordered sample-part column
/// keys `sample_parts`, splitting sample parts into groups of at most `max_parts_per_group`
/// (each repeating `balloon`, `spec`, and `outcome`), sorting rows in ascending balloon-number
/// order, paginating across page sets with repeating headers, and computing the canonical
/// RFC 8785 SHA-256 digest (`REQ-003`..`REQ-005`, `AC-2`, `AC-3`).
pub fn resolve_ppap_form(
    sources: &[&dyn FieldSource],
    formatter: &dyn FormNumericFormatter,
    sample_parts: &[String],
    max_parts_per_group: usize,
    rows_per_page: usize,
) -> Result<ResolvedPpapReport, LayoutError> {
    if max_parts_per_group == 0 || rows_per_page == 0 {
        return Err(LayoutError::BadParams("max_parts_per_group and rows_per_page must be >= 1"));
    }
    let (template, field_map) = load_ppap_template()?;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut table_meta: Option<(BlockId, u8, Option<BlockId>)> = None;

    for page in template.pages() {
        for block in page.blocks() {
            match block.kind().as_str() {
                FORM_FIELD_KIND => {
                    let payload = FormFieldPayload::from_value(block.payload().value())
                        .map_err(|e| LayoutError::TemplateInvalid(e.to_string()))?;
                    let rf = resolve_form_field_block(block.id(), &payload, &field_map, sources, formatter)?;
                    let val = match rf.value {
                        ResolvedFormFieldValue::Text(t) => t.untrusted_value().as_str().to_string(),
                        ResolvedFormFieldValue::Formatted(vt) => vt.text().canonical.to_string(),
                        ResolvedFormFieldValue::Missing => String::new(),
                    };
                    fields.insert(rf.field_id.as_str().to_string(), val);
                }
                "table" => {
                    table_meta = Some((block.id(), block.repeat_header_rows(), block.continuation_block()));
                }
                _ => {}
            }
        }
    }

    let Some((table_block_id, repeat_header_rows, continuation_block)) = table_meta else {
        return Err(LayoutError::TemplateInvalid("ppap template missing table block".to_string()));
    };

    let table_fid =
        FieldId::new("ppap_rows").ok_or_else(|| LayoutError::TemplateInvalid("bad ppap_rows table id".to_string()))?;
    let mut raw_rows = Vec::new();
    for src in sources {
        let got = src.rows(&table_fid);
        if !got.is_empty() {
            raw_rows = got;
            break;
        }
    }

    // Collect full rows sorted ascending by balloon number (REQ-003).
    let mut all_rows: Vec<BTreeMap<String, String>> = Vec::with_capacity(raw_rows.len());
    for row in raw_rows {
        let mut out_row = BTreeMap::new();
        for (fid, fv) in row {
            let text = match fv {
                FieldValue::Text(t) => t.untrusted_value().as_str().to_string(),
                FieldValue::Result(id) => formatter.format_result(&id)?.text().canonical.to_string(),
            };
            out_row.insert(fid.as_str().to_string(), text);
        }
        all_rows.push(out_row);
    }
    all_rows.sort_by(|a, b| {
        let ba = a.get("balloon").map(String::as_str).unwrap_or("");
        let bb = b.get("balloon").map(String::as_str).unwrap_or("");
        cmp_balloon(ba, bb)
    });

    // Split sample parts into column groups of at most max_parts_per_group (REQ-004).
    let part_chunks: Vec<Vec<String>> = if sample_parts.is_empty() {
        vec![Vec::new()]
    } else {
        sample_parts.chunks(max_parts_per_group).map(|c| c.to_vec()).collect()
    };

    let mut column_groups = Vec::with_capacity(part_chunks.len());
    for (gidx, chunk) in part_chunks.into_iter().enumerate() {
        let mut columns = Vec::with_capacity(chunk.len() + 3);
        columns.push("balloon".to_string());
        columns.push("spec".to_string());
        columns.extend(chunk.iter().cloned());
        columns.push("outcome".to_string());

        let mut group_rows = Vec::with_capacity(all_rows.len());
        for row in &all_rows {
            let mut gr = BTreeMap::new();
            for col in &columns {
                gr.insert(col.clone(), row.get(col).cloned().unwrap_or_default());
            }
            group_rows.push(gr);
        }
        column_groups.push(PpapColumnGroup { group_index: gidx, sample_parts: chunk, columns, rows: group_rows });
    }

    // Build page sets across all column groups (REQ-004, REQ-005).
    let rows_per_group_pages = if all_rows.is_empty() { 1 } else { all_rows.len().div_ceil(rows_per_page) };
    let total_pages = column_groups.len() * rows_per_group_pages;
    let mut pages = Vec::with_capacity(total_pages);
    let mut page_idx = 0usize;
    for (gidx, _) in column_groups.iter().enumerate() {
        for rpage in 0..rows_per_group_pages {
            let page_number = page_idx + 1;
            let start_row = rpage * rows_per_page;
            let end_row = (start_row + rows_per_page).min(all_rows.len());
            let mut hdr = BTreeMap::new();
            hdr.insert("part_number".to_string(), fields.get("part_number").cloned().unwrap_or_default());
            hdr.insert("part_revision".to_string(), fields.get("part_revision").cloned().unwrap_or_default());
            hdr.insert("submission_id".to_string(), fields.get("submission_id").cloned().unwrap_or_default());
            hdr.insert("page_n_of_m".to_string(), format!("Page {page_number} of {total_pages}"));
            pages.push(PpapPage {
                page_number,
                total_pages,
                column_group_index: gidx,
                repeats_identifying_header: page_idx > 0 && continuation_block.is_some(),
                identifying_header: hdr,
                repeats_table_header: page_idx > 0 && repeat_header_rows > 0,
                start_row,
                end_row,
            });
            page_idx += 1;
        }
    }

    let canon_val = ppap_resolved_to_value(&fields, &column_groups, &pages);
    let canon_bytes = to_canonical_bytes(&canon_val)
        .map_err(|_| LayoutError::TemplateInvalid("canonical encoding failed".to_string()))?;
    let canonical_digest = Digest::from_bytes(canon_sha256(&canon_bytes).0);

    Ok(ResolvedPpapReport {
        logical_name: PPAP_DIM_NAME.to_string(),
        table_block_id,
        repeat_header_rows,
        continuation_block,
        fields,
        column_groups,
        pages,
        warnings: Vec::new(),
        canonical_digest,
    })
}

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

fn ppap_resolved_to_value(fields: &BTreeMap<String, String>, groups: &[PpapColumnGroup], pages: &[PpapPage]) -> CValue {
    let mut fm = BTreeMap::new();
    for (k, v) in fields {
        fm.insert(k.clone(), CValue::Str(v.clone()));
    }
    let group_vals: Vec<CValue> = groups
        .iter()
        .map(|g| {
            let mut m = BTreeMap::new();
            m.insert("columns".to_string(), CValue::Arr(g.columns.iter().map(|c| CValue::Str(c.clone())).collect()));
            m.insert("group_index".to_string(), CValue::Str(g.group_index.to_string()));
            m.insert(
                "rows".to_string(),
                CValue::Arr(
                    g.rows
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
            m.insert(
                "sample_parts".to_string(),
                CValue::Arr(g.sample_parts.iter().map(|p| CValue::Str(p.clone())).collect()),
            );
            CValue::Obj(m)
        })
        .collect();
    let page_vals: Vec<CValue> = pages
        .iter()
        .map(|p| {
            let mut m = BTreeMap::new();
            m.insert("column_group_index".to_string(), CValue::Str(p.column_group_index.to_string()));
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
    root.insert("column_groups".to_string(), CValue::Arr(group_vals));
    root.insert("fields".to_string(), CValue::Obj(fm));
    root.insert("logical_name".to_string(), CValue::Str(PPAP_DIM_NAME.to_string()));
    root.insert("pages".to_string(), CValue::Arr(page_vals));
    CValue::Obj(root)
}
