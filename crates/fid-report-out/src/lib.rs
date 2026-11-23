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
pub use sheet::{convert_report, neutralise_text, Cell, CellText, HeaderCell, SheetError, SheetTable};
