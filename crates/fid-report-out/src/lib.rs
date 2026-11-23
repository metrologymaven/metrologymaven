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
    convert_report, csv_command_defs, export_csv, invoke_export_csv_cli, neutralise_text, render_table_csv, Cell,
    CellText, CsvExportError, CsvExportJournal, CsvFileEntry, ExportCsvEnv, ExportCsvEvent, ExportCsvOutput,
    ExportCsvParams, ExportCsvResult, HeaderCell, MemoryCsvJournal, MemoryResolvedLookup, ResolvedLookup, SheetError,
    SheetTable, CSV_RENDERER_ID, CSV_RENDERER_VERSION, EXPORT_CSV_CMD, MAX_CSV_TABLES,
};
