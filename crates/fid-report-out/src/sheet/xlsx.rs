//! Deterministic Office Open XML (`.xlsx`) spreadsheet workbook exporter and
//! `report.export_spreadsheet@1` command (spec node `RPT.OUTPUT.SHEET.XLSX`).
//!
//! - Emits one worksheet per resolved `table` block (`T01`, `T02`, ... or sanitised caption
//!   truncated to 31 characters) plus a final `meta` worksheet carrying the canonical
//!   `ExportMeta` with renderer `xlsx` (`REQ-RPT.OUTPUT.SHEET.XLSX-001`).
//! - Numeric cells write the canonical decimal string verbatim with an exact decimal-count
//!   OOXML `<numFmt>` style so spreadsheet viewers show the exact same digits as the PDF
//!   (`REQ-002`, `AC-2`, `AC-3`).
//! - Text cells use inline strings (`t="inlineStr"`) already formula-injection neutralised by
//!   `RPT.OUTPUT.SHEET.CELL` (C6).
//! - Deterministic ZIP container: fixed entry order, fixed DOS timestamp `1980-01-01 00:00:00`,
//!   zero extra fields, pure-Rust pinned raw DEFLATE, and no creation time or author
//!   properties (`REQ-003`, `AC-1`, `AC-5`).
//! - Enforces OOXML worksheet bounds (`1_048_576` rows, `16_384` columns) failing with
//!   `report.unsupported_block` naming the offending block before writing anything (`REQ-005`,
//!   `AC-4`).
//! - Zero floating-point types or literals anywhere in this module (CONSTITUTION C1 / RPT float rule).

use crate::meta::{ExportError, ExportMeta, ExportName, ExportReceipt, ExportSink, RendererVersion};
use crate::sheet::cell::{convert_report, Cell, SheetError, SheetTable};
use crate::sheet::csv::ResolvedLookup;
use fid_canon::CValue;
use fid_io_reg::Principal;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

/// Registry command identifier (`report.export_spreadsheet@1`).
pub const EXPORT_XLSX_CMD: &str = "report.export_spreadsheet@1";
/// CLI subcommand (`fid report export-spreadsheet`).
pub const CLI_EXPORT_XLSX: &str = "export-spreadsheet";
/// Renderer ID recorded in `ExportMeta` (`REQ-001`).
pub const XLSX_RENDERER_ID: &str = "xlsx";
/// Pinned XLSX exporter version.
pub const XLSX_RENDERER_VERSION: &str = "1.0.0";
/// OOXML maximum rows per worksheet (`REQ-005`, `AC-4`).
pub const MAX_XLSX_ROWS: usize = 1_048_576;
/// OOXML maximum columns per worksheet (`REQ-005`).
pub const MAX_XLSX_COLS: usize = 16_384;
/// Maximum worksheets in one exported workbook.
pub const MAX_XLSX_TABLES: usize = 256;
/// Maximum OOXML worksheet name length in UTF-16/characters (`REQ-001`).
pub const MAX_SHEET_NAME_CHARS: usize = 31;
/// Custom OOXML `numFmtId` base (IDs `>= 164` are application-defined per ECMA-376).
const CUSTOM_NUM_FMT_BASE: u32 = 164;
/// Fixed DOS modification time (`00:00:00`, `REQ-003`).
const ZIP_FIXED_MOD_TIME: u16 = 0x0000;
/// Fixed DOS modification date (`1980-01-01`, `REQ-003`).
const ZIP_FIXED_MOD_DATE: u16 = 0x0021;

// ------------------------------------------------------------------ errors

/// Typed errors of `report.export_spreadsheet@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XlsxExportError {
    /// `report.resolved_not_found`: no resolved report found under the given digest or ID.
    ResolvedNotFound(String),
    /// `report.bad_params`: malformed command or CLI arguments.
    BadParams(&'static str),
    /// Error from table-to-cell conversion or OOXML row/column limit (`report.unsupported_block`, `report.units_mismatch`).
    Sheet(SheetError),
    /// Error from export envelope or atomic local sink (`RPT.OUTPUT.META`).
    Export(ExportError),
}

impl XlsxExportError {
    pub fn code(&self) -> &'static str {
        match self {
            XlsxExportError::ResolvedNotFound(_) => "report.resolved_not_found",
            XlsxExportError::BadParams(_) => "report.bad_params",
            XlsxExportError::Sheet(e) => e.code(),
            XlsxExportError::Export(e) => e.code(),
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            XlsxExportError::Sheet(e) => e.to_value(),
            XlsxExportError::Export(e) => e.to_value(),
            XlsxExportError::ResolvedNotFound(id) => {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(self.code().to_string()));
                o.insert("resolved".to_string(), CValue::Str(id.clone()));
                CValue::Obj(o)
            }
            XlsxExportError::BadParams(reason) => {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(self.code().to_string()));
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
                CValue::Obj(o)
            }
        }
    }
}

impl fmt::Display for XlsxExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XlsxExportError::ResolvedNotFound(id) => write!(f, "report.resolved_not_found: {id}"),
            XlsxExportError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            XlsxExportError::Sheet(e) => e.fmt(f),
            XlsxExportError::Export(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for XlsxExportError {}

impl From<SheetError> for XlsxExportError {
    fn from(e: SheetError) -> XlsxExportError {
        XlsxExportError::Sheet(e)
    }
}

impl From<ExportError> for XlsxExportError {
    fn from(e: ExportError) -> XlsxExportError {
        XlsxExportError::Export(e)
    }
}

// ------------------------------------------------------------------ sheet naming & OOXML helpers

/// Sanitises a table name into a valid, unique OOXML worksheet name (`REQ-001`):
/// - Default names (`"table_01"`, `"table_02"`, ...) become `"T01"`, `"T02"`, ...
/// - Forbidden Excel sheet characters (`\ / ? * [ ] :`) are replaced with `_`.
/// - Truncated cleanly to 31 characters and deduplicated against `used` and `"meta"`.
pub fn format_sheet_name(raw_name: &str, ordinal: usize, used: &mut BTreeSet<String>) -> String {
    let default_name = format!("T{ordinal:02}");
    let base =
        if raw_name.is_empty() || raw_name.starts_with("table_") && raw_name[6..].bytes().all(|b| b.is_ascii_digit()) {
            default_name.clone()
        } else {
            let cleaned: String = raw_name
                .chars()
                .map(|c| match c {
                    '\\' | '/' | '?' | '*' | '[' | ']' | ':' => '_',
                    c if (c as u32) < 0x20 => '_',
                    c => c,
                })
                .collect();
            let trimmed = cleaned.trim().trim_matches('\'');
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("meta") {
                default_name.clone()
            } else {
                trimmed.chars().take(MAX_SHEET_NAME_CHARS).collect()
            }
        };

    if !used.contains(&base) && !base.eq_ignore_ascii_case("meta") {
        used.insert(base.clone());
        return base;
    }

    let mut candidate = default_name;
    let mut suffix = 1usize;
    while used.contains(&candidate) || candidate.eq_ignore_ascii_case("meta") {
        candidate = format!("T{ordinal:02}_{suffix}");
        suffix += 1;
    }
    used.insert(candidate.clone());
    candidate
}

/// Converts a 0-based column index into OOXML column letters (`0 -> "A"`, `25 -> "Z"`, `26 -> "AA"`).
fn col_letters(mut idx: usize) -> String {
    let mut bytes = Vec::with_capacity(4);
    loop {
        bytes.push(b'A' + (idx % 26) as u8);
        if idx < 26 {
            break;
        }
        idx = idx / 26 - 1;
    }
    bytes.reverse();
    String::from_utf8(bytes).unwrap_or_default()
}

fn push_xml_escaped(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {
                // XML 1.0 forbids C0 controls other than TAB/LF/CR; emit OOXML _x00XX_ escape.
                let _ = write!(out, "_x{:04X}_", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn format_code_for_decimals(decimals: u8) -> String {
    if decimals == 0 {
        "0".to_string()
    } else {
        let mut s = String::with_capacity(2 + usize::from(decimals));
        s.push_str("0.");
        for _ in 0..decimals {
            s.push('0');
        }
        s
    }
}

// ------------------------------------------------------------------ OOXML workbook builder

/// Renders `tables` and canonical `meta` into deterministic Office Open XML `.xlsx` bytes
/// (`REQ-RPT.OUTPUT.SHEET.XLSX-001`..`005`).
pub fn render_workbook_xlsx(tables: &[SheetTable], meta: &ExportMeta) -> Result<Vec<u8>, XlsxExportError> {
    if tables.len() > MAX_XLSX_TABLES {
        return Err(XlsxExportError::BadParams("too_many_tables"));
    }

    // REQ-005 / AC-4: check OOXML row and column limits before allocating any worksheet XML.
    let mut distinct_decimals: BTreeSet<u8> = BTreeSet::new();
    for table in tables {
        if table.header.len() > MAX_XLSX_COLS {
            return Err(XlsxExportError::Sheet(SheetError::UnsupportedBlock {
                block_id: Some(table.block_id),
                reason: "xlsx_column_limit_exceeded",
            }));
        }
        let total_rows = table.rows.len().saturating_add(1); // header row + data rows
        if total_rows > MAX_XLSX_ROWS {
            return Err(XlsxExportError::Sheet(SheetError::UnsupportedBlock {
                block_id: Some(table.block_id),
                reason: "xlsx_row_limit_exceeded",
            }));
        }
        for row in &table.rows {
            if row.len() > MAX_XLSX_COLS {
                return Err(XlsxExportError::Sheet(SheetError::UnsupportedBlock {
                    block_id: Some(table.block_id),
                    reason: "xlsx_column_limit_exceeded",
                }));
            }
            for cell in row {
                if let Cell::Number { decimals, .. } = cell {
                    distinct_decimals.insert(*decimals);
                }
            }
        }
    }

    // Style index mapping:
    //   0 : default general cell
    //   1 : header cell (bold)
    //   2 + k : numeric cell with decimal count `d` in sorted `distinct_decimals` order
    let mut decimal_to_style: BTreeMap<u8, u32> = BTreeMap::new();
    let mut num_fmts: Vec<(u32, String)> = Vec::with_capacity(distinct_decimals.len());
    for (idx, dec) in distinct_decimals.into_iter().enumerate() {
        let fmt_id = CUSTOM_NUM_FMT_BASE + idx as u32;
        let style_idx = 2 + idx as u32;
        decimal_to_style.insert(dec, style_idx);
        num_fmts.push((fmt_id, format_code_for_decimals(dec)));
    }

    // Build deterministic sheet list: T01..TNN + final `meta`.
    let mut used_names = BTreeSet::new();
    let mut sheet_names: Vec<String> = Vec::with_capacity(tables.len() + 1);
    for (i, t) in tables.iter().enumerate() {
        sheet_names.push(format_sheet_name(&t.name, i + 1, &mut used_names));
    }
    sheet_names.push("meta".to_string());

    let sheet_count = sheet_names.len();
    let meta_json_bytes = meta.to_canonical_bytes()?;
    let meta_json_str =
        std::str::from_utf8(&meta_json_bytes).map_err(|_| XlsxExportError::BadParams("non_utf8_export_meta"))?;

    // Assemble ZIP entries in strictly fixed order (REQ-003).
    let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(5 + sheet_count);
    entries.push(("[Content_Types].xml".to_string(), build_content_types_xml(sheet_count).into_bytes()));
    entries.push(("_rels/.rels".to_string(), build_root_rels_xml().into_bytes()));
    entries.push(("xl/workbook.xml".to_string(), build_workbook_xml(&sheet_names).into_bytes()));
    entries.push(("xl/_rels/workbook.xml.rels".to_string(), build_workbook_rels_xml(sheet_count).into_bytes()));
    entries.push(("xl/styles.xml".to_string(), build_styles_xml(&num_fmts).into_bytes()));

    for (i, table) in tables.iter().enumerate() {
        let path = format!("xl/worksheets/sheet{}.xml", i + 1);
        let xml = build_table_sheet_xml(table, &decimal_to_style);
        entries.push((path, xml.into_bytes()));
    }
    let meta_sheet_path = format!("xl/worksheets/sheet{sheet_count}.xml");
    entries.push((meta_sheet_path, build_meta_sheet_xml(meta, meta_json_str).into_bytes()));

    Ok(build_deterministic_zip(&entries))
}

fn build_content_types_xml(sheet_count: usize) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
         <Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>",
    );
    for i in 1..=sheet_count {
        let _ = write!(
            out,
            "<Override PartName=\"/xl/worksheets/sheet{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        );
    }
    out.push_str("</Types>");
    out
}

fn build_root_rels_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
     <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
     <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
     </Relationships>"
        .to_string()
}

fn build_workbook_xml(sheet_names: &[String]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <sheets>",
    );
    for (idx, name) in sheet_names.iter().enumerate() {
        let sheet_id = idx + 1;
        out.push_str("<sheet name=\"");
        push_xml_escaped(&mut out, name);
        let _ = write!(out, "\" sheetId=\"{sheet_id}\" r:id=\"rId{sheet_id}\"/>");
    }
    out.push_str("</sheets></workbook>");
    out
}

fn build_workbook_rels_xml(sheet_count: usize) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for i in 1..=sheet_count {
        let _ = write!(
            out,
            "<Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{i}.xml\"/>"
        );
    }
    let styles_rid = sheet_count + 1;
    let _ = write!(
        out,
        "<Relationship Id=\"rId{styles_rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>"
    );
    out.push_str("</Relationships>");
    out
}

fn build_styles_xml(num_fmts: &[(u32, String)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
    );
    if !num_fmts.is_empty() {
        let _ = write!(out, "<numFmts count=\"{}\">", num_fmts.len());
        for (id, code) in num_fmts {
            let _ = write!(out, "<numFmt numFmtId=\"{id}\" formatCode=\"{code}\"/>");
        }
        out.push_str("</numFmts>");
    }
    out.push_str(
        "<fonts count=\"2\">\
         <font><sz val=\"10\"/><name val=\"Calibri\"/></font>\
         <font><b/><sz val=\"10\"/><name val=\"Calibri\"/></font>\
         </fonts>\
         <fills count=\"2\">\
         <fill><patternFill patternType=\"none\"/></fill>\
         <fill><patternFill patternType=\"gray125\"/></fill>\
         </fills>\
         <borders count=\"1\">\
         <border><left/><right/><top/><bottom/><diagonal/></border>\
         </borders>\
         <cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>",
    );
    let xf_count = 2 + num_fmts.len();
    let _ = write!(
        out,
        "<cellXfs count=\"{xf_count}\">\
         <xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>\
         <xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/>"
    );
    for (id, _) in num_fmts {
        let _ = write!(
            out,
            "<xf numFmtId=\"{id}\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/>"
        );
    }
    out.push_str("</cellXfs></styleSheet>");
    out
}

fn build_table_sheet_xml(table: &SheetTable, decimal_to_style: &BTreeMap<u8, u32>) -> String {
    let est = (table.rows.len() + 1) * table.header.len().max(1) * 44 + 256;
    let mut out = String::with_capacity(est);
    out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
         <sheetData>",
    );

    // Precompute column letters once per worksheet for fast 100k-cell performance.
    let max_cols = table.header.len().max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    let cols: Vec<String> = (0..max_cols).map(col_letters).collect();

    // Row 1: header row (style 1 = bold, inline string).
    out.push_str("<row r=\"1\">");
    for (ci, h) in table.header.iter().enumerate() {
        let _ = write!(out, "<c r=\"{}1\" s=\"1\" t=\"inlineStr\"><is><t", cols[ci]);
        if needs_xml_space_preserve(&h.text.safe) {
            out.push_str(" xml:space=\"preserve\"");
        }
        out.push('>');
        push_xml_escaped(&mut out, &h.text.safe);
        out.push_str("</t></is></c>");
    }
    out.push_str("</row>");

    // Rows 2..: data rows.
    for (ri, row) in table.rows.iter().enumerate() {
        let rnum = ri + 2;
        let _ = write!(out, "<row r=\"{rnum}\">");
        for (ci, cell) in row.iter().enumerate() {
            match cell {
                Cell::Number { canonical, decimals } => {
                    let s_idx = decimal_to_style.get(decimals).copied().unwrap_or(0);
                    let _ = write!(out, "<c r=\"{}{rnum}\" s=\"{s_idx}\"><v>{canonical}</v></c>", cols[ci]);
                }
                Cell::Text(ct) => {
                    let _ = write!(out, "<c r=\"{}{rnum}\" t=\"inlineStr\"><is><t", cols[ci]);
                    if needs_xml_space_preserve(&ct.safe) {
                        out.push_str(" xml:space=\"preserve\"");
                    }
                    out.push('>');
                    push_xml_escaped(&mut out, &ct.safe);
                    out.push_str("</t></is></c>");
                }
                Cell::Empty => {}
            }
        }
        out.push_str("</row>");
    }

    out.push_str("</sheetData></worksheet>");
    out
}

fn build_meta_sheet_xml(meta: &ExportMeta, meta_json_str: &str) -> String {
    let mut out = String::with_capacity(meta_json_str.len() + 1024);
    out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
         <sheetData>",
    );

    let template_str = match meta.template {
        Some(t) => format!("{}@{}", t.id, t.version),
        None => String::new(),
    };
    let renderer_str = format!("{}@{}", meta.renderer.id, meta.renderer.version);
    let rows: [(&str, &str); 6] = [
        ("schema", "export_meta@1"),
        ("report_digest", meta.report_digest.as_str()),
        ("template", &template_str),
        ("kernel_version", &meta.kernel_version),
        ("renderer", &renderer_str),
        ("export_meta_json", meta_json_str),
    ];

    out.push_str(
        "<row r=\"1\"><c r=\"A1\" s=\"1\" t=\"inlineStr\"><is><t>Key</t></is></c>\
         <c r=\"B1\" s=\"1\" t=\"inlineStr\"><is><t>Value</t></is></c></row>",
    );
    for (idx, (k, v)) in rows.iter().enumerate() {
        let rnum = idx + 2;
        let _ = write!(
            out,
            "<row r=\"{rnum}\"><c r=\"A{rnum}\" t=\"inlineStr\"><is><t>{k}</t></is></c><c r=\"B{rnum}\" t=\"inlineStr\"><is><t"
        );
        if needs_xml_space_preserve(v) {
            out.push_str(" xml:space=\"preserve\"");
        }
        out.push('>');
        push_xml_escaped(&mut out, v);
        out.push_str("</t></is></c></row>");
    }

    out.push_str("</sheetData></worksheet>");
    out
}

fn needs_xml_space_preserve(s: &str) -> bool {
    s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) || s.contains('\n') || s.contains('\t')
}

// ------------------------------------------------------------------ deterministic ZIP + raw DEFLATE

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Deterministic raw RFC 1951 DEFLATE (pinned fixed Huffman LZ77, zero external crates, D0).
fn deflate_raw_deterministic(data: &[u8]) -> Vec<u8> {
    const HASH_BITS: usize = 12;
    const HASH_SIZE: usize = 1 << HASH_BITS;
    const WINDOW_SIZE: usize = 32_768;
    const WINDOW_MASK: usize = WINDOW_SIZE - 1;
    const MAX_MATCH: usize = 258;
    const MIN_MATCH: usize = 3;
    const MAX_CHAIN: usize = 12;
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227,
        258,
    ];
    const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
        6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] =
        [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

    struct Bw {
        out: Vec<u8>,
        buf: u32,
        nbits: u8,
    }
    impl Bw {
        fn bits(&mut self, v: u32, n: u8) {
            self.buf |= v << self.nbits;
            self.nbits += n;
            while self.nbits >= 8 {
                self.out.push((self.buf & 0xFF) as u8);
                self.buf >>= 8;
                self.nbits -= 8;
            }
        }
        fn huff(&mut self, code: u16, bits: u8) {
            let mut rev = 0u32;
            for i in 0..bits {
                if ((code >> (bits - 1 - i)) & 1) != 0 {
                    rev |= 1u32 << i;
                }
            }
            self.bits(rev, bits);
        }
        fn litlen(&mut self, sym: u16) {
            match sym {
                0..=143 => self.huff(0b0011_0000 + sym, 8),
                144..=255 => self.huff(0b1_1001_0000 + (sym - 144), 9),
                256..=279 => self.huff(sym - 256, 7),
                280..=287 => self.huff(0b1100_0000 + (sym - 280), 8),
                _ => {}
            }
        }
    }

    let hash3 = |pos: usize| -> usize {
        let v = (usize::from(data[pos]) << 16) | (usize::from(data[pos + 1]) << 8) | usize::from(data[pos + 2]);
        ((v.wrapping_mul(0x1E35_A7BD)) >> (32 - HASH_BITS)) & (HASH_SIZE - 1)
    };

    let mut bw = Bw { out: Vec::with_capacity(data.len() / 2 + 16), buf: 0, nbits: 0 };
    bw.bits(1, 1); // BFINAL=1
    bw.bits(1, 2); // BTYPE=01 (fixed Huffman)

    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut prev = vec![u32::MAX; WINDOW_SIZE];
    let n = data.len();
    let mut pos = 0usize;

    while pos < n {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;

        if pos + MIN_MATCH <= n {
            let h = hash3(pos);
            let mut cand = head[h];
            prev[pos & WINDOW_MASK] = cand;
            head[h] = pos as u32;

            let max_len = MAX_MATCH.min(n - pos);
            let mut steps = 0usize;
            while cand != u32::MAX && steps < MAX_CHAIN {
                let cpos = cand as usize;
                let dist = pos.saturating_sub(cpos);
                if dist == 0 || dist > WINDOW_SIZE {
                    break;
                }
                if data[cpos] == data[pos] && data[cpos + 1] == data[pos + 1] && data[cpos + 2] == data[pos + 2] {
                    let mut mlen = 3usize;
                    while mlen < max_len && data[cpos + mlen] == data[pos + mlen] {
                        mlen += 1;
                    }
                    if mlen > best_len {
                        best_len = mlen;
                        best_dist = dist;
                        if best_len == max_len {
                            break;
                        }
                    }
                }
                let nxt = prev[cpos & WINDOW_MASK];
                if nxt >= cand {
                    break;
                }
                cand = nxt;
                steps += 1;
            }
        }

        if best_len >= MIN_MATCH {
            let li = LEN_BASE.iter().rposition(|&b| best_len >= usize::from(b)).unwrap_or(0);
            bw.litlen(257 + li as u16);
            if LEN_EXTRA[li] > 0 {
                bw.bits((best_len - usize::from(LEN_BASE[li])) as u32, LEN_EXTRA[li]);
            }
            let di = DIST_BASE.iter().rposition(|&b| best_dist >= usize::from(b)).unwrap_or(0);
            bw.huff(di as u16, 5);
            if DIST_EXTRA[di] > 0 {
                bw.bits((best_dist - usize::from(DIST_BASE[di])) as u32, DIST_EXTRA[di]);
            }
            let end_skip = (pos + best_len).min(n.saturating_sub(MIN_MATCH - 1));
            for p in (pos + 1)..end_skip {
                let h = hash3(p);
                prev[p & WINDOW_MASK] = head[h];
                head[h] = p as u32;
            }
            pos += best_len;
        } else {
            bw.litlen(u16::from(data[pos]));
            pos += 1;
        }
    }

    bw.litlen(256);
    if bw.nbits > 0 {
        bw.out.push((bw.buf & 0xFF) as u8);
    }
    bw.out
}

fn build_deterministic_zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    struct CdRecord<'a> {
        name: &'a [u8],
        crc: u32,
        comp_size: u32,
        uncomp_size: u32,
        local_offset: u32,
    }

    let mut out = Vec::new();
    let mut cd = Vec::with_capacity(entries.len());

    for (name, raw) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32(raw);
        let compressed = deflate_raw_deterministic(raw);
        let comp_size = compressed.len() as u32;
        let uncomp_size = raw.len() as u32;
        let local_offset = out.len() as u32;

        // Local file header (30 bytes + name, 0 extra fields, 1980-01-01 00:00:00, method=8 DEFLATE).
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
        out.extend_from_slice(&0u16.to_le_bytes()); // general purpose bit flag
        out.extend_from_slice(&8u16.to_le_bytes()); // compression method: DEFLATE
        out.extend_from_slice(&ZIP_FIXED_MOD_TIME.to_le_bytes());
        out.extend_from_slice(&ZIP_FIXED_MOD_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&comp_size.to_le_bytes());
        out.extend_from_slice(&uncomp_size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra field length = 0
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&compressed);

        cd.push(CdRecord { name: name_bytes, crc, comp_size, uncomp_size, local_offset });
    }

    let cd_offset = out.len() as u32;
    for rec in &cd {
        // Central directory header (46 bytes + name, 0 extra, 0 comment).
        out.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version made by: 2.0 MS-DOS
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&8u16.to_le_bytes()); // method: DEFLATE
        out.extend_from_slice(&ZIP_FIXED_MOD_TIME.to_le_bytes());
        out.extend_from_slice(&ZIP_FIXED_MOD_DATE.to_le_bytes());
        out.extend_from_slice(&rec.crc.to_le_bytes());
        out.extend_from_slice(&rec.comp_size.to_le_bytes());
        out.extend_from_slice(&rec.uncomp_size.to_le_bytes());
        out.extend_from_slice(&(rec.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length = 0
        out.extend_from_slice(&0u16.to_le_bytes()); // comment length = 0
        out.extend_from_slice(&0u16.to_le_bytes()); // disk number start = 0
        out.extend_from_slice(&0u16.to_le_bytes()); // internal file attributes
        out.extend_from_slice(&0u32.to_le_bytes()); // external file attributes
        out.extend_from_slice(&rec.local_offset.to_le_bytes());
        out.extend_from_slice(rec.name);
    }
    let cd_size = (out.len() as u32) - cd_offset;
    let count = cd.len() as u16;

    // End of central directory record (22 bytes, 0 comment).
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // disk number
    out.extend_from_slice(&0u16.to_le_bytes()); // start disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length = 0

    out
}

// ------------------------------------------------------------------ journal & command

/// One journaled event emitted by `report.export_spreadsheet@1` (`REQ-004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportXlsxEvent {
    pub resolved_digest: Digest,
    pub receipt: ExportReceipt,
}

impl ExportXlsxEvent {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command".to_string(), CValue::Str(EXPORT_XLSX_CMD.to_string()));
        o.insert("receipt".to_string(), self.receipt.to_value());
        o.insert("resolved_digest".to_string(), CValue::Str(self.resolved_digest.as_str().to_string()));
        CValue::Obj(o)
    }
}

/// Journal seam recording one W1 `report.export_spreadsheet@1` event.
pub trait XlsxExportJournal {
    fn record_xlsx_export(&mut self, event: ExportXlsxEvent) -> Result<(), XlsxExportError>;
}

/// In-memory journal for tests and CLI execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryXlsxJournal {
    events: Vec<ExportXlsxEvent>,
}

impl MemoryXlsxJournal {
    pub fn new() -> MemoryXlsxJournal {
        MemoryXlsxJournal::default()
    }

    pub fn events(&self) -> &[ExportXlsxEvent] {
        &self.events
    }
}

impl XlsxExportJournal for MemoryXlsxJournal {
    fn record_xlsx_export(&mut self, event: ExportXlsxEvent) -> Result<(), XlsxExportError> {
        self.events.push(event);
        Ok(())
    }
}

/// Wire parameters of `report.export_spreadsheet@1` (`Design: Registry command`).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ExportXlsxParams {
    #[fid(description = "Resolved report digest (sha256:...) or report ID.")]
    pub resolved: String,
    #[fid(description = "Approved local export root handle (e.g. root://exports).")]
    pub root: String,
    #[fid(description = "Export workbook file name (e.g. bracket.xlsx).")]
    pub name: String,
    #[fid(description = "If true, overwrite an existing file atomically.")]
    pub overwrite: bool,
    #[fid(description = "If true, render and compute SHA-256 without writing files or journaling.")]
    pub dry_run: bool,
    #[fid(description = "Units mode: authored (default) or si (requires resolved report generated with SI units).")]
    pub units: String,
}

/// Wire output of `report.export_spreadsheet@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ExportXlsxOutput {
    #[fid(description = "Resolved report digest exported.")]
    pub resolved_digest: String,
    #[fid(description = "Whether dry-run mode was active.")]
    pub dry_run: bool,
    #[fid(description = "Workbook file name.")]
    pub name: String,
    #[fid(description = "Rendered workbook byte size as decimal string.")]
    pub size_bytes: String,
    #[fid(description = "SHA-256 digest of the rendered .xlsx bytes.")]
    pub sha256: String,
    #[fid(description = "Total number of worksheets including meta.")]
    pub sheet_count: u32,
}

/// Typed result of [`export_spreadsheet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportXlsxResult {
    pub resolved_digest: Digest,
    pub dry_run: bool,
    pub name: ExportName,
    pub size_bytes: u64,
    pub sha256: Digest,
    pub sheet_count: u32,
    pub receipt: Option<ExportReceipt>,
}

impl ExportXlsxResult {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("dry_run".to_string(), CValue::Bool(self.dry_run));
        o.insert("name".to_string(), CValue::Str(self.name.as_str().to_string()));
        o.insert("receipt".to_string(), self.receipt.as_ref().map_or(CValue::Null, ExportReceipt::to_value));
        o.insert("resolved_digest".to_string(), CValue::Str(self.resolved_digest.as_str().to_string()));
        o.insert("sha256".to_string(), CValue::Str(self.sha256.as_str().to_string()));
        o.insert("sheet_count".to_string(), CValue::Str(self.sheet_count.to_string()));
        o.insert("size_bytes".to_string(), CValue::Str(self.size_bytes.to_string()));
        CValue::Obj(o)
    }

    pub fn to_wire_output(&self) -> ExportXlsxOutput {
        ExportXlsxOutput {
            resolved_digest: self.resolved_digest.as_str().to_string(),
            dry_run: self.dry_run,
            name: self.name.as_str().to_string(),
            size_bytes: self.size_bytes.to_string(),
            sha256: self.sha256.as_str().to_string(),
            sheet_count: self.sheet_count,
        }
    }
}

/// Execution context for `report.export_spreadsheet@1`.
pub struct ExportXlsxEnv<'a> {
    pub principal: Principal,
    pub lookup: &'a dyn ResolvedLookup,
    pub sink: &'a ExportSink<'a>,
    pub journal: &'a mut dyn XlsxExportJournal,
}

/// Executes `report.export_spreadsheet@1` (`REQ-RPT.OUTPUT.SHEET.XLSX-001`..`005`).
pub fn export_spreadsheet(
    params: &ExportXlsxParams,
    env: &mut ExportXlsxEnv<'_>,
) -> Result<ExportXlsxResult, XlsxExportError> {
    let name = ExportName::parse(&params.name)?;
    let require_si = match params.units.as_str() {
        "" | "authored" => false,
        "si" => true,
        _ => return Err(XlsxExportError::BadParams("invalid_units_mode")),
    };
    let resolved = env
        .lookup
        .lookup(&params.resolved)
        .ok_or_else(|| XlsxExportError::ResolvedNotFound(params.resolved.clone()))?;

    let tables = convert_report(&resolved, require_si)?;
    let renderer = RendererVersion::new(XLSX_RENDERER_ID, XLSX_RENDERER_VERSION)?;
    let meta = ExportMeta::from_resolved(&resolved, renderer);
    let bytes = render_workbook_xlsx(&tables, &meta)?;
    let sha256 = Digest::from_bytes(Sha256::digest(&bytes).into());
    let size_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let sheet_count = u32::try_from(tables.len() + 1).unwrap_or(u32::MAX);

    if params.dry_run {
        return Ok(ExportXlsxResult {
            resolved_digest: resolved.digest,
            dry_run: true,
            name,
            size_bytes,
            sha256,
            sheet_count,
            receipt: None,
        });
    }

    let receipt = env.sink.write(env.principal, &params.root, name.as_str(), &bytes, params.overwrite)?;
    let event = ExportXlsxEvent { resolved_digest: resolved.digest, receipt: receipt.clone() };
    if let Err(err) = env.journal.record_xlsx_export(event) {
        let _ = env.sink.remove_written(env.principal, &params.root, &name);
        return Err(err);
    }

    Ok(ExportXlsxResult {
        resolved_digest: resolved.digest,
        dry_run: false,
        name,
        size_bytes,
        sha256,
        sheet_count,
        receipt: Some(receipt),
    })
}

#[fid_command(
    id = "report.export_spreadsheet",
    major = 1,
    title = "Export report tables as deterministic XLSX workbook",
    description = "Exports each table block of a resolved report as an Office Open XML workbook (.xlsx) with one worksheet per table plus a canonical ExportMeta worksheet into an approved local export root.",
    side_effect = W1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = WriteRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [EXPORT_XLSX_CMD],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-23"),
    web = Available,
)]
fn export_spreadsheet_decl(
    env: &mut ExportXlsxEnv<'_>,
    params: ExportXlsxParams,
) -> Result<ExportXlsxOutput, XlsxExportError> {
    let res = export_spreadsheet(&params, env)?;
    Ok(res.to_wire_output())
}

fid_commands![export_spreadsheet_decl];

/// Registry command definitions published by `RPT.OUTPUT.SHEET.XLSX`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

/// CLI entrypoint for `fid report export-spreadsheet --resolved <d> --root <h> --name <file> [--units authored|si] [--overwrite] [--dry-run] --json`.
pub fn invoke_export_spreadsheet_cli(args: &[&str], env: &mut ExportXlsxEnv<'_>) -> Result<CValue, XlsxExportError> {
    let mut resolved: Option<String> = None;
    let mut root: Option<String> = None;
    let mut name: Option<String> = None;
    let mut units = String::from("authored");
    let mut overwrite = false;
    let mut dry_run = false;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i] {
            "--resolved" => {
                i += 1;
                resolved = Some((*args.get(i).ok_or(XlsxExportError::BadParams("missing_resolved"))?).to_string());
            }
            "--root" => {
                i += 1;
                root = Some((*args.get(i).ok_or(XlsxExportError::BadParams("missing_root"))?).to_string());
            }
            "--name" => {
                i += 1;
                name = Some((*args.get(i).ok_or(XlsxExportError::BadParams("missing_name"))?).to_string());
            }
            "--units" => {
                i += 1;
                units = (*args.get(i).ok_or(XlsxExportError::BadParams("missing_units"))?).to_string();
            }
            "--overwrite" => overwrite = true,
            "--dry-run" => dry_run = true,
            "--json" => json_flag = true,
            _ => return Err(XlsxExportError::BadParams("unknown_flag")),
        }
        i += 1;
    }

    if !json_flag {
        return Err(XlsxExportError::BadParams("json_flag_required"));
    }

    let params = ExportXlsxParams {
        resolved: resolved.ok_or(XlsxExportError::BadParams("missing_resolved"))?,
        root: root.ok_or(XlsxExportError::BadParams("missing_root"))?,
        name: name.ok_or(XlsxExportError::BadParams("missing_name"))?,
        overwrite,
        dry_run,
        units,
    };
    let result = export_spreadsheet(&params, env)?;
    Ok(result.to_value())
}
