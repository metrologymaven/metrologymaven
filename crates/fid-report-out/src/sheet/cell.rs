//! Table-to-cell model, unit headers and formula-injection neutralisation
//! (spec node `RPT.OUTPUT.SHEET.CELL`).
//!
//! - Turns every `table` block of a [`ResolvedReport`] into a [`SheetTable`] in document order.
//! - Numeric cells carry the kernel formatter's canonical decimal string (`ValueText.canonical`)
//!   and its decimal digit count, with the unit symbol placed in the column header (`REQ-001`).
//! - Multi-unit columns split into one column per unit in order of first appearance (`REQ-002`).
//! - Requesting SI output on a report not generated with `UnitOverrideMode::Si` fails with
//!   `report.units_mismatch`; no value is ever converted here (`REQ-003`).
//! - Text cells starting with `=`, `+`, `-`, `@`, `\t`, `\r`, or full-width `＝`, `＋`, `－`, `＠`
//!   are prefixed with `'` while preserving the original untrusted text in `source` (`REQ-004`, C6).
//! - Values carrying markers (nonconformance / tie / undecidable) become text cells with the
//!   marker text included so the marker is never lost (`REQ-006`).
//! - Zero floats anywhere in this module (CONSTITUTION C1 / RPT float rule).

use fid_canon::CValue;
use fid_project::Uuid;
use fid_report::bind::{GeneratedBlock, ResolvedReport, UnitOverrideMode};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// ------------------------------------------------------------------ errors

/// Typed errors of the table-to-cell converter (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetError {
    /// `report.unsupported_block`: report has no `table` block, or a table block/cell cannot be
    /// represented (`REQ-005`).
    UnsupportedBlock { block_id: Option<Uuid>, reason: &'static str },
    /// `report.units_mismatch`: SI output was requested on a resolved report not generated with
    /// `units = si`; caller must regenerate with SI units (`REQ-003`).
    UnitsMismatch,
}

impl SheetError {
    pub fn code(&self) -> &'static str {
        match self {
            SheetError::UnsupportedBlock { .. } => "report.unsupported_block",
            SheetError::UnitsMismatch => "report.units_mismatch",
        }
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        match self {
            SheetError::UnsupportedBlock { block_id, reason } => {
                if let Some(id) = block_id {
                    o.insert("block_id".to_string(), CValue::Str(id.to_string()));
                }
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
            }
            SheetError::UnitsMismatch => {
                o.insert("reason".to_string(), CValue::Str("regenerate_with_si_units".to_string()));
            }
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for SheetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SheetError::UnsupportedBlock { block_id: Some(id), reason } => {
                write!(f, "report.unsupported_block: block {id} ({reason})")
            }
            SheetError::UnsupportedBlock { block_id: None, reason } => {
                write!(f, "report.unsupported_block: {reason}")
            }
            SheetError::UnitsMismatch => {
                f.write_str("report.units_mismatch: resolved report was not generated with SI units")
            }
        }
    }
}

impl std::error::Error for SheetError {}

// ------------------------------------------------------------------ data model

/// Formula-injection-neutralised text cell keeping the original untrusted source (`REQ-004`, C6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellText {
    /// Safe spreadsheet text (prefixed with `'` if `neutralised == true`).
    pub safe: String,
    /// Original untrusted text verbatim before neutralisation.
    pub source: String,
    /// Whether an apostrophe prefix was added for formula-injection neutralisation.
    pub neutralised: bool,
}

/// Neutralises a spreadsheet text value per `REQ-RPT.OUTPUT.SHEET.CELL-004` (C6).
///
/// Prefixes with `'` if `raw` begins with `=`, `+`, `-`, `@`, `\t` (U+0009), `\r` (U+000D),
/// or full-width `＝` (U+FF1D), `＋` (U+FF0B), `－` (U+FF0D), `＠` (U+FF20).
pub fn neutralise_text(raw: &str) -> CellText {
    let dangerous = raw.chars().next().is_some_and(|c| {
        matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r' | '\u{FF1D}' | '\u{FF0B}' | '\u{FF0D}' | '\u{FF20}')
    });
    if dangerous {
        CellText { safe: format!("'{raw}"), source: raw.to_string(), neutralised: true }
    } else {
        CellText { safe: raw.to_string(), source: raw.to_string(), neutralised: false }
    }
}

/// One typed spreadsheet cell (`Scope: Cell`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// Canonical ASCII decimal string from `ValueText.canonical` and its fraction digit count.
    Number { canonical: String, decimals: u8 },
    /// Neutralised text cell (strings, decisions, or marked values per `REQ-006`).
    Text(CellText),
    /// Empty cell (missing value or unused split-unit slot).
    Empty,
}

/// One column header cell carrying its display title (with `[unit]` when applicable) and optional unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderCell {
    /// Safe neutralised header text (e.g. `"Measured [mm]"`).
    pub text: CellText,
    /// Unit symbol attached to this column, if numeric.
    pub unit: Option<String>,
}

impl HeaderCell {
    pub fn plain(title: &str) -> HeaderCell {
        HeaderCell { text: neutralise_text(title), unit: None }
    }

    pub fn with_unit(title: &str, unit: &str) -> HeaderCell {
        let formatted = format!("{title} [{unit}]");
        HeaderCell { text: neutralise_text(&formatted), unit: Some(unit.to_string()) }
    }
}

/// One converted resolved `table` block ready for CSV or workbook serialization (`Scope: SheetTable`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetTable {
    /// Source block UUID from the resolved report.
    pub block_id: Uuid,
    /// Deterministic table name (e.g. `"table_01"` or block section title).
    pub name: String,
    /// Column headers in order (after multi-unit splitting).
    pub header: Vec<HeaderCell>,
    /// Data rows, each with `row.len() == header.len()`.
    pub rows: Vec<Vec<Cell>>,
}

// ------------------------------------------------------------------ conversion engine

/// Intermediate cell before multi-unit column splitting.
#[derive(Debug, Clone)]
enum RawCell {
    Number { canonical: String, decimals: u8, unit: String },
    Text(CellText),
    Empty,
}

/// Converts every `table` block of `resolved` into a [`SheetTable`] in document order
/// (`REQ-RPT.OUTPUT.SHEET.CELL-001`..`006`).
///
/// Fails with:
/// - [`SheetError::UnitsMismatch`] when `require_si` is true and `resolved.provenance.units` is
///   not `Some(UnitOverrideMode::Si)` (`REQ-003`).
/// - [`SheetError::UnsupportedBlock`] when the report contains no `table` block or an
///   unrepresentable table cell (`REQ-005`).
pub fn convert_report(resolved: &ResolvedReport, require_si: bool) -> Result<Vec<SheetTable>, SheetError> {
    if require_si && resolved.provenance.units != Some(UnitOverrideMode::Si) {
        return Err(SheetError::UnitsMismatch);
    }

    let mut tables = Vec::new();
    for page in &resolved.pages {
        for block in &page.blocks {
            if block.kind.as_str() != "table" {
                continue;
            }
            let ordinal = tables.len() + 1;
            let table = convert_table_block(block, ordinal)?;
            tables.push(table);
        }
    }

    if tables.is_empty() {
        let first_block = resolved.pages.first().and_then(|p| p.blocks.first()).map(|b| b.id);
        return Err(SheetError::UnsupportedBlock { block_id: first_block, reason: "no_table_block" });
    }
    Ok(tables)
}

fn convert_table_block(block: &GeneratedBlock, ordinal: usize) -> Result<SheetTable, SheetError> {
    let CValue::Obj(o) = &block.payload else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block.id), reason: "table_payload_not_object" });
    };
    let schema = match o.get("schema") {
        Some(CValue::Str(s)) => s.as_str(),
        _ => "",
    };
    let name = format!("table_{ordinal:02}");

    match schema {
        "resolved_column_group@1" => convert_column_group_table(block.id, name, o),
        "resolved_table@1" => {
            if o.contains_key("stages") && o.contains_key("columns") {
                convert_multistage_table(block.id, name, o)
            } else if o.contains_key("header") {
                convert_grid_table(block.id, name, o)
            } else {
                Err(SheetError::UnsupportedBlock { block_id: Some(block.id), reason: "unknown_table_layout" })
            }
        }
        _ => Err(SheetError::UnsupportedBlock { block_id: Some(block.id), reason: "unsupported_table_schema" }),
    }
}

fn convert_column_group_table(
    block_id: Uuid,
    name: String,
    o: &BTreeMap<String, CValue>,
) -> Result<SheetTable, SheetError> {
    let Some(CValue::Arr(cols)) = o.get("columns") else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "missing_columns" });
    };
    let header = vec![HeaderCell::plain("Column"), HeaderCell::plain("Kind"), HeaderCell::plain("ID")];
    let mut rows = Vec::with_capacity(cols.len());
    for (idx, c) in cols.iter().enumerate() {
        let CValue::Obj(co) = c else {
            return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "column_item_not_object" });
        };
        let id = str_field(co, "id").unwrap_or("");
        let kind = str_field(co, "kind").unwrap_or("");
        rows.push(vec![
            Cell::Text(neutralise_text(&format!("{}", idx + 1))),
            Cell::Text(neutralise_text(kind)),
            Cell::Text(neutralise_text(id)),
        ]);
    }
    Ok(SheetTable { block_id, name, header, rows })
}

fn convert_multistage_table(
    block_id: Uuid,
    name: String,
    o: &BTreeMap<String, CValue>,
) -> Result<SheetTable, SheetError> {
    let Some(CValue::Arr(stages)) = o.get("stages") else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "missing_stages" });
    };
    let Some(CValue::Arr(raw_rows)) = o.get("rows") else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "missing_rows" });
    };

    let stage_count = stages.len();
    let mut titles = Vec::with_capacity(3 + stage_count * 4);
    titles.push("Characteristic ID".to_string());
    titles.push("Name".to_string());
    titles.push("Nominal".to_string());
    for s_idx in 0..stage_count {
        let s_num = s_idx + 1;
        titles.push(format!("Stage {s_num} Measured"));
        titles.push(format!("Stage {s_num} Deviation"));
        titles.push(format!("Stage {s_num} Uncertainty"));
        titles.push(format!("Stage {s_num} Decision"));
    }

    let mut grid: Vec<Vec<RawCell>> = Vec::with_capacity(raw_rows.len());
    for row_val in raw_rows {
        let CValue::Obj(ro) = row_val else {
            return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "row_not_object" });
        };
        let mut row = Vec::with_capacity(titles.len());
        row.push(RawCell::Text(neutralise_text(str_field(ro, "characteristic_id").unwrap_or(""))));
        row.push(RawCell::Text(neutralise_text(str_field(ro, "name").unwrap_or(""))));
        row.push(match ro.get("nominal") {
            Some(CValue::Null) | None => RawCell::Empty,
            Some(nv) => parse_value_text_cell(block_id, nv)?,
        });

        let Some(CValue::Arr(cells)) = ro.get("cells") else {
            return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "row_cells_missing" });
        };
        for s_idx in 0..stage_count {
            match cells.get(s_idx) {
                Some(CValue::Obj(co)) => {
                    // Measured
                    row.push(match co.get("measured") {
                        Some(CValue::Null) | None => RawCell::Empty,
                        Some(mv) => parse_value_text_cell(block_id, mv)?,
                    });
                    // Deviation
                    row.push(match co.get("deviation") {
                        Some(CValue::Null) | None => RawCell::Empty,
                        Some(dv) => parse_value_text_cell(block_id, dv)?,
                    });
                    // Uncertainty
                    row.push(match co.get("uncertainty") {
                        Some(CValue::Null) | None => RawCell::Empty,
                        Some(uv) => parse_cell_envelope(block_id, uv)?,
                    });
                    // Decision
                    row.push(match co.get("decision") {
                        Some(CValue::Null) | None => RawCell::Empty,
                        Some(dv) => parse_outcome_cell(block_id, dv)?,
                    });
                }
                _ => {
                    row.push(RawCell::Empty);
                    row.push(RawCell::Empty);
                    row.push(RawCell::Empty);
                    row.push(RawCell::Empty);
                }
            }
        }
        grid.push(row);
    }

    Ok(finalize_columns_with_unit_split(block_id, name, &titles, &grid))
}

fn convert_grid_table(block_id: Uuid, name: String, o: &BTreeMap<String, CValue>) -> Result<SheetTable, SheetError> {
    let Some(CValue::Arr(raw_header)) = o.get("header") else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "missing_header" });
    };
    let Some(CValue::Arr(raw_rows)) = o.get("rows") else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "missing_rows" });
    };

    let mut titles = Vec::with_capacity(raw_header.len());
    for h in raw_header {
        let CValue::Str(s) = h else {
            return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "header_not_string" });
        };
        titles.push(s.clone());
    }

    let mut grid: Vec<Vec<RawCell>> = Vec::with_capacity(raw_rows.len());
    for r in raw_rows {
        let CValue::Arr(cells) = r else {
            return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "row_not_array" });
        };
        if cells.len() != titles.len() {
            return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "row_width_mismatch" });
        }
        let mut row = Vec::with_capacity(cells.len());
        for cell in cells {
            row.push(parse_cell_envelope(block_id, cell)?);
        }
        grid.push(row);
    }

    Ok(finalize_columns_with_unit_split(block_id, name, &titles, &grid))
}

/// Parses a `ValueText` JSON object (`{canonical, display, unit, markers, ...}`).
fn parse_value_text_cell(block_id: Uuid, v: &CValue) -> Result<RawCell, SheetError> {
    let CValue::Obj(o) = v else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "value_not_object" });
    };
    let canonical = str_field(o, "canonical")
        .ok_or(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "value_missing_canonical" })?;
    let display = str_field(o, "display").unwrap_or(canonical);
    let unit = str_field(o, "unit").unwrap_or("");

    // REQ-006: values with any marker (nonconformance / tie / undecidable) become text cells
    // carrying the display string + marker text so the marker is never lost.
    if let Some(CValue::Arr(markers)) = o.get("markers") {
        if !markers.is_empty() {
            let mut marker_texts = Vec::with_capacity(markers.len());
            for m in markers {
                if let CValue::Obj(mo) = m {
                    if let Some(t) = str_field(mo, "text") {
                        marker_texts.push(t.to_string());
                    } else if let Some(k) = str_field(mo, "key") {
                        marker_texts.push(k.to_string());
                    }
                } else if let CValue::Str(s) = m {
                    marker_texts.push(s.clone());
                }
            }
            let combined = if marker_texts.is_empty() {
                display.to_string()
            } else {
                format!("{display} [{}]", marker_texts.join("; "))
            };
            return Ok(RawCell::Text(neutralise_text(&combined)));
        }
    }

    // Count decimals strictly from canonical decimal string (no floats).
    let decimals = count_decimals(canonical);
    Ok(RawCell::Number { canonical: canonical.to_string(), decimals, unit: unit.to_string() })
}

fn parse_outcome_cell(block_id: Uuid, v: &CValue) -> Result<RawCell, SheetError> {
    let CValue::Obj(o) = v else {
        return Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "outcome_not_object" });
    };
    let text = str_field(o, "text")
        .ok_or(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "outcome_missing_text" })?;
    Ok(RawCell::Text(neutralise_text(text)))
}

fn parse_cell_envelope(block_id: Uuid, v: &CValue) -> Result<RawCell, SheetError> {
    match v {
        CValue::Null => Ok(RawCell::Empty),
        CValue::Str(s) => Ok(RawCell::Text(neutralise_text(s))),
        CValue::Obj(o) => match str_field(o, "kind") {
            Some("value") => {
                let inner = o
                    .get("value")
                    .ok_or(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "missing_value_inner" })?;
                parse_value_text_cell(block_id, inner)
            }
            Some("text") | Some("uncertainty_not_evaluated") | Some("uncertainty_class") => {
                let t = str_field(o, "text").unwrap_or("");
                Ok(RawCell::Text(neutralise_text(t)))
            }
            Some("missing") => Ok(RawCell::Empty),
            Some("outcome") => {
                let inner = o.get("outcome").ok_or(SheetError::UnsupportedBlock {
                    block_id: Some(block_id),
                    reason: "missing_outcome_inner",
                })?;
                parse_outcome_cell(block_id, inner)
            }
            Some("uncertainty") => {
                let inner = o.get("value").ok_or(SheetError::UnsupportedBlock {
                    block_id: Some(block_id),
                    reason: "missing_uncertainty_value",
                })?;
                parse_value_text_cell(block_id, inner)
            }
            Some(_) => {
                Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "unrepresentable_cell_kind" })
            }
            None => {
                // Plain ValueText object without envelope.
                if o.contains_key("canonical") {
                    parse_value_text_cell(block_id, v)
                } else {
                    Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "unknown_cell_object" })
                }
            }
        },
        _ => Err(SheetError::UnsupportedBlock { block_id: Some(block_id), reason: "unrepresentable_cell_type" }),
    }
}

/// Applies `REQ-001` (unit in header) and `REQ-002` (split multi-unit columns in first-appearance order).
fn finalize_columns_with_unit_split(
    block_id: Uuid,
    name: String,
    titles: &[String],
    grid: &[Vec<RawCell>],
) -> SheetTable {
    enum ColPlan {
        Single,
        Split { units: Vec<String>, has_non_numeric: bool },
    }

    let mut plans = Vec::with_capacity(titles.len());
    let mut final_headers = Vec::new();

    for (col_idx, title) in titles.iter().enumerate() {
        let mut units_order: Vec<String> = Vec::new();
        let mut units_seen: BTreeSet<String> = BTreeSet::new();
        let mut has_text = false;

        for row in grid {
            match row.get(col_idx) {
                Some(RawCell::Number { unit, .. }) if !unit.is_empty() => {
                    if units_seen.insert(unit.clone()) {
                        units_order.push(unit.clone());
                    }
                }
                Some(RawCell::Text(_)) => {
                    has_text = true;
                }
                _ => {}
            }
        }

        if units_order.len() <= 1 {
            let unit = units_order.into_iter().next();
            match &unit {
                Some(u) => final_headers.push(HeaderCell::with_unit(title, u)),
                None => final_headers.push(HeaderCell::plain(title)),
            }
            plans.push(ColPlan::Single);
        } else {
            for u in &units_order {
                final_headers.push(HeaderCell::with_unit(title, u));
            }
            if has_text {
                final_headers.push(HeaderCell::plain(title));
            }
            plans.push(ColPlan::Split { units: units_order, has_non_numeric: has_text });
        }
    }

    let out_width = final_headers.len();
    let mut out_rows = Vec::with_capacity(grid.len());
    for row in grid {
        let mut out_row = Vec::with_capacity(out_width);
        for (col_idx, plan) in plans.iter().enumerate() {
            let raw = row.get(col_idx);
            match plan {
                ColPlan::Single => {
                    let cell = match raw {
                        Some(RawCell::Number { canonical, decimals, .. }) => {
                            Cell::Number { canonical: canonical.clone(), decimals: *decimals }
                        }
                        Some(RawCell::Text(t)) => Cell::Text(t.clone()),
                        Some(RawCell::Empty) | None => Cell::Empty,
                    };
                    out_row.push(cell);
                }
                ColPlan::Split { units, has_non_numeric } => {
                    for u in units {
                        match raw {
                            Some(RawCell::Number { canonical, decimals, unit }) if unit == u => {
                                out_row.push(Cell::Number { canonical: canonical.clone(), decimals: *decimals });
                            }
                            _ => out_row.push(Cell::Empty),
                        }
                    }
                    if *has_non_numeric {
                        match raw {
                            Some(RawCell::Text(t)) => out_row.push(Cell::Text(t.clone())),
                            _ => out_row.push(Cell::Empty),
                        }
                    }
                }
            }
        }
        out_rows.push(out_row);
    }

    SheetTable { block_id, name, header: final_headers, rows: out_rows }
}

fn count_decimals(canonical: &str) -> u8 {
    match canonical.split_once('.') {
        Some((_, frac)) => u8::try_from(frac.len()).unwrap_or(u8::MAX),
        None => 0,
    }
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Option<&'a str> {
    match o.get(k) {
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}
