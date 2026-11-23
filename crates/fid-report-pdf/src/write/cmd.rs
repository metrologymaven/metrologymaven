//! Host export command `report.export_pdf@1` (W1, `fs_effect = write_root`), CLI
//! `fid report export-pdf --json` (with `--dry-run`), and MCP registration
//! (`RPT.OUTPUT.PDF.WRITE` REQ-004..REQ-006).

use super::{render_pdf_from_json, RenderedPdf, WriteError};
use crate::draw::{ImageStore, PainterRegistry};
use crate::layout::MeasureRegistry;
use crate::text::FontSet;
use fid_canon::CValue;
use fid_io_reg::Principal;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use fid_report::bind::ResolvedReport;
use fid_report_out::meta::{ExportName, ExportReceipt, ExportSink};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Registry command identifier (`report.export_pdf@1`).
pub const EXPORT_PDF_COMMAND: &str = "report.export_pdf@1";
/// CLI subcommand (`fid report export-pdf`).
pub const CLI_EXPORT_PDF: &str = "export-pdf";

// ------------------------------------------------------------------ journal & lookup seams

/// One journaled event emitted by `report.export_pdf@1` (`REQ-RPT.OUTPUT.PDF.WRITE-004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPdfEvent {
    pub resolved_digest: Digest,
    pub receipt: ExportReceipt,
}

impl ExportPdfEvent {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command".to_string(), CValue::Str(EXPORT_PDF_COMMAND.to_string()));
        o.insert("receipt".to_string(), self.receipt.to_value());
        o.insert("resolved_digest".to_string(), CValue::Str(self.resolved_digest.as_str().to_string()));
        CValue::Obj(o)
    }
}

/// Journal seam recording one W1 `report.export_pdf@1` event.
pub trait PdfExportJournal {
    fn record_pdf_export(&mut self, event: ExportPdfEvent) -> Result<(), WriteError>;
}

/// In-memory journal for tests and CLI execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryPdfJournal {
    events: Vec<ExportPdfEvent>,
}

impl MemoryPdfJournal {
    pub fn new() -> MemoryPdfJournal {
        MemoryPdfJournal::default()
    }

    pub fn events(&self) -> &[ExportPdfEvent] {
        &self.events
    }
}

impl PdfExportJournal for MemoryPdfJournal {
    fn record_pdf_export(&mut self, event: ExportPdfEvent) -> Result<(), WriteError> {
        self.events.push(event);
        Ok(())
    }
}

/// Lookup seam returning canonical `resolved_report@1` JSON bytes by digest or report ID.
pub trait PdfResolvedLookup {
    fn lookup_json(&self, key: &str) -> Option<Vec<u8>>;
}

/// In-memory lookup map keyed by resolved digest (`sha256:...`) and/or report UUID.
#[derive(Debug, Clone, Default)]
pub struct MemoryPdfResolvedLookup {
    entries: BTreeMap<String, Vec<u8>>,
}

impl MemoryPdfResolvedLookup {
    pub fn new() -> MemoryPdfResolvedLookup {
        MemoryPdfResolvedLookup::default()
    }

    /// Inserts raw canonical `resolved_report@1` JSON bytes under `key`.
    pub fn insert_json(&mut self, key: impl Into<String>, json: Vec<u8>) {
        self.entries.insert(key.into(), json);
    }

    /// Inserts a typed [`ResolvedReport`] under its canonical digest (`sha256:...`).
    pub fn insert_resolved(&mut self, resolved: &ResolvedReport) -> Result<(), WriteError> {
        let bytes = resolved.to_canonical_bytes().map_err(|_| WriteError::BadParams("canonical_resolved_failed"))?;
        self.entries.insert(resolved.digest.as_str().to_string(), bytes);
        Ok(())
    }
}

impl PdfResolvedLookup for MemoryPdfResolvedLookup {
    fn lookup_json(&self, key: &str) -> Option<Vec<u8>> {
        self.entries.get(key).cloned()
    }
}

// ------------------------------------------------------------------ command schema & handler

/// Wire parameters of `report.export_pdf@1` (`Design: Registry command`).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ExportPdfParams {
    #[fid(description = "Resolved report digest (sha256:...) or report ID.")]
    pub resolved: String,
    #[fid(description = "Approved local export root handle (e.g. root://exports).")]
    pub root: String,
    #[fid(description = "Export file name (1..=128 ASCII [A-Za-z0-9._-], e.g. bracket.pdf).")]
    pub name: String,
    #[fid(description = "If true, overwrite an existing file atomically.")]
    pub overwrite: bool,
    #[fid(description = "If true, render fully and return size and SHA-256 without writing or journaling.")]
    pub dry_run: bool,
}

/// Wire output of `report.export_pdf@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ExportPdfOutput {
    #[fid(description = "Resolved report digest exported.")]
    pub resolved_digest: String,
    #[fid(description = "Whether dry-run mode was active.")]
    pub dry_run: bool,
    #[fid(description = "Export file name.")]
    pub name: String,
    #[fid(description = "Rendered PDF size in bytes (as decimal string).")]
    pub size_bytes: String,
    #[fid(description = "SHA-256 digest of the rendered PDF bytes.")]
    pub sha256: String,
    #[fid(description = "Number of pages in the rendered PDF.")]
    pub page_count: u32,
}

/// Full typed result returned by [`export_pdf`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPdfResult {
    pub resolved_digest: Digest,
    pub dry_run: bool,
    pub name: ExportName,
    pub size_bytes: u64,
    pub sha256: Digest,
    pub page_count: u32,
    pub receipt: Option<ExportReceipt>,
}

impl ExportPdfResult {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("dry_run".to_string(), CValue::Bool(self.dry_run));
        o.insert("name".to_string(), CValue::Str(self.name.as_str().to_string()));
        o.insert("page_count".to_string(), CValue::Str(self.page_count.to_string()));
        o.insert("receipt".to_string(), self.receipt.as_ref().map_or(CValue::Null, ExportReceipt::to_value));
        o.insert("resolved_digest".to_string(), CValue::Str(self.resolved_digest.as_str().to_string()));
        o.insert("sha256".to_string(), CValue::Str(self.sha256.as_str().to_string()));
        o.insert("size_bytes".to_string(), CValue::Str(self.size_bytes.to_string()));
        CValue::Obj(o)
    }

    pub fn to_wire_output(&self) -> ExportPdfOutput {
        ExportPdfOutput {
            resolved_digest: self.resolved_digest.as_str().to_string(),
            dry_run: self.dry_run,
            name: self.name.as_str().to_string(),
            size_bytes: self.size_bytes.to_string(),
            sha256: self.sha256.as_str().to_string(),
            page_count: self.page_count,
        }
    }
}

/// Execution context for `report.export_pdf@1`.
pub struct ExportPdfEnv<'a> {
    pub principal: Principal,
    pub lookup: &'a dyn PdfResolvedLookup,
    pub fonts: &'a FontSet,
    pub images: &'a dyn ImageStore,
    pub measurers: &'a MeasureRegistry<'a>,
    pub painters: &'a PainterRegistry<'a>,
    pub sink: &'a ExportSink<'a>,
    pub journal: &'a mut dyn PdfExportJournal,
}

/// Executes `report.export_pdf@1` (`REQ-RPT.OUTPUT.PDF.WRITE-001`..`006`).
pub fn export_pdf(params: &ExportPdfParams, env: &mut ExportPdfEnv<'_>) -> Result<ExportPdfResult, WriteError> {
    let name = ExportName::parse(&params.name)?;
    let resolved_json = env
        .lookup
        .lookup_json(&params.resolved)
        .ok_or_else(|| WriteError::ResolvedNotFound(params.resolved.clone()))?;

    // Check painters up front, lay out, paint, and serialize PDF bytes before touching the sink.
    let rendered: RenderedPdf =
        render_pdf_from_json(&resolved_json, env.fonts, env.images, env.measurers, env.painters)?;

    if params.dry_run {
        return Ok(ExportPdfResult {
            resolved_digest: rendered.resolved_digest,
            dry_run: true,
            name,
            size_bytes: rendered.size_bytes,
            sha256: rendered.sha256,
            page_count: rendered.page_count,
            receipt: None,
        });
    }

    let receipt = env.sink.write(env.principal, &params.root, name.as_str(), &rendered.bytes, params.overwrite)?;
    let event = ExportPdfEvent { resolved_digest: rendered.resolved_digest, receipt: receipt.clone() };
    if let Err(err) = env.journal.record_pdf_export(event) {
        let _ = env.sink.remove_written(env.principal, &params.root, &name);
        return Err(err);
    }

    Ok(ExportPdfResult {
        resolved_digest: rendered.resolved_digest,
        dry_run: false,
        name,
        size_bytes: rendered.size_bytes,
        sha256: rendered.sha256,
        page_count: rendered.page_count,
        receipt: Some(receipt),
    })
}

#[fid_command(
    id = "report.export_pdf",
    major = 1,
    title = "Export resolved report as deterministic PDF",
    description = "Renders a resolved report to deterministic PDF 1.7 with embedded font subsets and canonical ExportMeta and writes it into an approved local export root.",
    side_effect = W1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = WriteRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [EXPORT_PDF_COMMAND],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-23"),
    web = Available,
)]
fn handle_export_pdf(env: &mut ExportPdfEnv<'_>, params: ExportPdfParams) -> Result<ExportPdfOutput, WriteError> {
    let res = export_pdf(&params, env)?;
    Ok(res.to_wire_output())
}

fid_commands![handle_export_pdf];

/// Registry command definitions published by `RPT.OUTPUT.PDF.WRITE`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

/// CLI entrypoint for `fid report export-pdf --resolved <d> --root <h> --name <file> [--overwrite] [--dry-run] --json`.
pub fn invoke_export_pdf_cli(args: &[&str], env: &mut ExportPdfEnv<'_>) -> Result<CValue, WriteError> {
    let mut resolved: Option<String> = None;
    let mut root: Option<String> = None;
    let mut name: Option<String> = None;
    let mut overwrite = false;
    let mut dry_run = false;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i] {
            "--resolved" => {
                i += 1;
                let v = *args.get(i).ok_or(WriteError::BadParams("missing_resolved_value"))?;
                resolved = Some(v.to_string());
            }
            "--root" => {
                i += 1;
                let v = *args.get(i).ok_or(WriteError::BadParams("missing_root_value"))?;
                root = Some(v.to_string());
            }
            "--name" => {
                i += 1;
                let v = *args.get(i).ok_or(WriteError::BadParams("missing_name_value"))?;
                name = Some(v.to_string());
            }
            "--overwrite" => overwrite = true,
            "--dry-run" => dry_run = true,
            "--json" => json_flag = true,
            _ => return Err(WriteError::BadParams("unknown_flag")),
        }
        i += 1;
    }

    if !json_flag {
        return Err(WriteError::BadParams("json_flag_required"));
    }

    let params = ExportPdfParams {
        resolved: resolved.ok_or(WriteError::BadParams("missing_resolved"))?,
        root: root.ok_or(WriteError::BadParams("missing_root"))?,
        name: name.ok_or(WriteError::BadParams("missing_name"))?,
        overwrite,
        dry_run,
    };
    let result = export_pdf(&params, env)?;
    Ok(result.to_value())
}
