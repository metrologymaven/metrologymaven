//! Tabular report export pipeline (`RPT.OUTPUT.SHEET`).
//!
//! - [`cell`]: format-neutral table-to-cell model, unit headers, multi-unit column splitting
//!   and formula-injection neutralisation (`RPT.OUTPUT.SHEET.CELL`).
//! - [`csv`]: RFC 4180 UTF-8 CSV export and `report.export_csv@1` command (`RPT.OUTPUT.SHEET.CSV`).

pub mod cell;
pub mod csv;

pub use cell::{convert_report, neutralise_text, Cell, CellText, HeaderCell, SheetError, SheetTable};
pub use csv::{
    csv_command_defs, export_csv, invoke_export_csv_cli, render_table_csv, CsvExportError, CsvExportJournal,
    CsvFileEntry, ExportCsvEnv, ExportCsvEvent, ExportCsvOutput, ExportCsvParams, ExportCsvResult, MemoryCsvJournal,
    MemoryResolvedLookup, ResolvedLookup, CSV_RENDERER_ID, CSV_RENDERER_VERSION, EXPORT_CSV_CMD, MAX_CSV_TABLES,
};
