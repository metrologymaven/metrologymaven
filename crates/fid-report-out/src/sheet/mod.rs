//! Tabular report export pipeline (`RPT.OUTPUT.SHEET`).
//!
//! - [`cell`]: format-neutral table-to-cell model, unit headers, multi-unit column splitting
//!   and formula-injection neutralisation (`RPT.OUTPUT.SHEET.CELL`).
//! - [`csv`]: RFC 4180 UTF-8 CSV export and `report.export_csv@1` command (`RPT.OUTPUT.SHEET.CSV`).
//! - [`xlsx`]: deterministic Office Open XML `.xlsx` workbook export and
//!   `report.export_spreadsheet@1` command (`RPT.OUTPUT.SHEET.XLSX`).

pub mod cell;
pub mod csv;
pub mod xlsx;

pub use cell::{convert_report, neutralise_text, Cell, CellText, HeaderCell, SheetError, SheetTable};
pub use csv::{
    csv_command_defs, export_csv, invoke_export_csv_cli, render_table_csv, CsvExportError, CsvExportJournal,
    CsvFileEntry, ExportCsvEnv, ExportCsvEvent, ExportCsvOutput, ExportCsvParams, ExportCsvResult, MemoryCsvJournal,
    MemoryResolvedLookup, ResolvedLookup, CSV_RENDERER_ID, CSV_RENDERER_VERSION, EXPORT_CSV_CMD, MAX_CSV_TABLES,
};
pub use xlsx::{
    command_defs as xlsx_command_defs, export_spreadsheet, format_sheet_name, invoke_export_spreadsheet_cli,
    render_workbook_xlsx, ExportXlsxEnv, ExportXlsxEvent, ExportXlsxOutput, ExportXlsxParams, ExportXlsxResult,
    MemoryXlsxJournal, XlsxExportError, XlsxExportJournal, EXPORT_XLSX_CMD, MAX_XLSX_COLS, MAX_XLSX_ROWS,
    XLSX_RENDERER_ID, XLSX_RENDERER_VERSION,
};
