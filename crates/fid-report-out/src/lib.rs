//! Report export envelope and writers (spec node `RPT.OUTPUT`).
//!
//! - [`meta`]: canonical [`ExportMeta`] metadata record, validated [`ExportName`],
//!   principal-aware local [`ExportRootResolver`] and atomic [`ExportSink`] returning
//!   [`ExportReceipt`] (`RPT.OUTPUT.META`).
//! - [`sheet`]: table-to-cell model, unit headers and formula-injection neutralisation
//!   (`RPT.OUTPUT.SHEET.CELL`).

pub mod meta;
pub mod sheet;

pub use meta::{
    ExportError, ExportMeta, ExportName, ExportReceipt, ExportRootHandle, ExportRootResolver, ExportSink,
    LocalRootResolver, RendererVersion, ResolvedRoot, TemplateVersionRef, EXPORT_META_SCHEMA, MAX_EXPORT_NAME_BYTES,
};
pub use sheet::{
    convert_report, csv_command_defs, export_csv, export_spreadsheet, format_sheet_name, invoke_export_csv_cli,
    invoke_export_spreadsheet_cli, neutralise_text, render_table_csv, render_workbook_xlsx, xlsx_command_defs, Cell,
    CellText, CsvExportError, CsvExportJournal, CsvFileEntry, ExportCsvEnv, ExportCsvEvent, ExportCsvOutput,
    ExportCsvParams, ExportCsvResult, ExportXlsxEnv, ExportXlsxEvent, ExportXlsxOutput, ExportXlsxParams,
    ExportXlsxResult, HeaderCell, MemoryCsvJournal, MemoryResolvedLookup, MemoryXlsxJournal, ResolvedLookup,
    SheetError, SheetTable, XlsxExportError, XlsxExportJournal, CSV_RENDERER_ID, CSV_RENDERER_VERSION, EXPORT_CSV_CMD,
    EXPORT_XLSX_CMD, MAX_CSV_TABLES, MAX_XLSX_COLS, MAX_XLSX_ROWS, XLSX_RENDERER_ID, XLSX_RENDERER_VERSION,
};
