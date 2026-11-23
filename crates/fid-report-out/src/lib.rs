//! Report export envelope and writers (spec node `RPT.OUTPUT`).
//!
//! - [`meta`]: canonical [`ExportMeta`] metadata record, validated [`ExportName`],
//!   principal-aware local [`ExportRootResolver`] and atomic [`ExportSink`] returning
//!   [`ExportReceipt`] (`RPT.OUTPUT.META`).

pub mod meta;

pub use meta::{
    ExportError, ExportMeta, ExportName, ExportReceipt, ExportRootHandle, ExportRootResolver, ExportSink,
    LocalRootResolver, RendererVersion, ResolvedRoot, TemplateVersionRef, EXPORT_META_SCHEMA, MAX_EXPORT_NAME_BYTES,
};
