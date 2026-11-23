//! CSV export of report tables and `report.export_csv@1` command
//! (spec node `RPT.OUTPUT.SHEET.CSV`).
//!
//! - Uses [`crate::sheet::cell::convert_report`] for table-to-cell conversion, unit headers,
//!   multi-unit column splitting and formula-injection neutralisation.
//! - Dialect (`REQ-002`, `REQ-003`): UTF-8 without BOM, `,` separator, `.` decimal point
//!   (from `ValueText.canonical`, unquoted), CRLF (`\r\n`) line endings, header row first,
//!   RFC 4180 quoting for any field containing `,`, `"`, `\r`, or `\n`.
//! - File naming (`REQ-001`): `<base>-01.csv`, `<base>-02.csv`, ... plus `<base>-meta.json`
//!   holding canonical [`ExportMeta`] with renderer `("csv", CSV_RENDERER_VERSION)`.
//! - Side-effect class W1 with `fs_effect: write_root` (`REQ-005`): writes all files through
//!   [`ExportSink`]; on ANY failure removes every file written earlier by this invocation and
//!   journals nothing; on success journals ONE `report.export_csv@1` event listing every
//!   [`ExportReceipt`]. Supports `--dry-run` (computes digests/sizes without writing or journaling).
//! - Zero floats anywhere in this module (CONSTITUTION C1 / RPT float rule).

use crate::meta::{ExportError, ExportMeta, ExportName, ExportReceipt, ExportSink, RendererVersion};
use crate::sheet::cell::{convert_report, Cell, SheetError, SheetTable};
use fid_canon::CValue;
use fid_io_reg::Principal;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use fid_report::bind::ResolvedReport;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID for CSV report export (W1, `fs_effect: write_root`, D0).
pub const EXPORT_CSV_CMD: &str = "report.export_csv@1";
/// Renderer ID recorded inside `<base>-meta.json` (`REQ-001`).
pub const CSV_RENDERER_ID: &str = "csv";
/// Renderer version recorded inside `<base>-meta.json`.
pub const CSV_RENDERER_VERSION: &str = "1.0.0";
/// Maximum table blocks supported by `<base>-NN.csv` (`01..=99`).
pub const MAX_CSV_TABLES: usize = 99;

// ------------------------------------------------------------------ errors

/// Typed errors of CSV export (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvExportError {
    /// Table-to-cell conversion error (`report.unsupported_block`, `report.units_mismatch`).
    Sheet(SheetError),
    /// Export envelope / sink error (`raw_path_forbidden`, `report.export_root_not_local`,
    /// `report.export_name_invalid`, `report.export_exists`, `report.export_io`, ...).
    Export(ExportError),
    /// `report.resolved_not_found`: no resolved report found for the requested digest or ID.
    ResolvedNotFound(String),
    /// `report.bad_params`: malformed command or CLI parameters.
    BadParams(&'static str),
}

impl From<SheetError> for CsvExportError {
    fn from(e: SheetError) -> CsvExportError {
        CsvExportError::Sheet(e)
    }
}

impl From<ExportError> for CsvExportError {
    fn from(e: ExportError) -> CsvExportError {
        CsvExportError::Export(e)
    }
}

impl CsvExportError {
    pub fn code(&self) -> &'static str {
        match self {
            CsvExportError::Sheet(e) => e.code(),
            CsvExportError::Export(e) => e.code(),
            CsvExportError::ResolvedNotFound(_) => "report.resolved_not_found",
            CsvExportError::BadParams(_) => "report.bad_params",
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            CsvExportError::Sheet(e) => e.to_value(),
            CsvExportError::Export(e) => e.to_value(),
            CsvExportError::ResolvedNotFound(id) => {
                obj([("code", CValue::Str(self.code().to_string())), ("id", CValue::Str(id.clone()))])
            }
            CsvExportError::BadParams(r) => {
                obj([("code", CValue::Str(self.code().to_string())), ("reason", CValue::Str((*r).to_string()))])
            }
        }
    }
}

impl fmt::Display for CsvExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CsvExportError::Sheet(e) => e.fmt(f),
            CsvExportError::Export(e) => e.fmt(f),
            CsvExportError::ResolvedNotFound(id) => write!(f, "report.resolved_not_found: {id}"),
            CsvExportError::BadParams(r) => write!(f, "report.bad_params: {r}"),
        }
    }
}

impl std::error::Error for CsvExportError {}

// ------------------------------------------------------------------ pure RFC 4180 serialiser

/// Serialises one [`SheetTable`] to deterministic UTF-8 CSV bytes (no BOM, `,` separator,
/// CRLF line ends, header row first, RFC 4180 quoting, unquoted canonical numbers;
/// `REQ-002`, `REQ-003`, `REQ-004`).
pub fn render_table_csv(table: &SheetTable) -> Vec<u8> {
    let estimated = (table.rows.len().saturating_add(1)).saturating_mul(table.header.len().saturating_mul(16).max(32));
    let mut out = String::with_capacity(estimated);

    // Header row first.
    for (idx, h) in table.header.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        write_rfc4180_field(&mut out, &h.text.safe);
    }
    out.push_str("\r\n");

    // Data rows.
    for row in &table.rows {
        for (idx, cell) in row.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            match cell {
                // REQ-003: numeric cells written as canonical decimal string, unquoted.
                Cell::Number { canonical, .. } => out.push_str(canonical),
                Cell::Text(t) => write_rfc4180_field(&mut out, &t.safe),
                Cell::Empty => {}
            }
        }
        out.push_str("\r\n");
    }

    out.into_bytes()
}

fn write_rfc4180_field(out: &mut String, field: &str) {
    let needs_quote = field.bytes().any(|b| matches!(b, b',' | b'"' | b'\r' | b'\n'));
    if !needs_quote {
        out.push_str(field);
        return;
    }
    out.push('"');
    for ch in field.chars() {
        if ch == '"' {
            out.push_str("\"\"");
        } else {
            out.push(ch);
        }
    }
    out.push('"');
}

// ------------------------------------------------------------------ event & receipt types

/// One file produced (or planned in `--dry-run`) by `report.export_csv@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvFileEntry {
    pub name: ExportName,
    pub bytes: u64,
    pub sha256: Digest,
}

impl CsvFileEntry {
    pub fn to_value(&self) -> CValue {
        obj([
            ("bytes", CValue::Str(self.bytes.to_string())),
            ("name", CValue::Str(self.name.as_str().to_string())),
            ("sha256", CValue::Str(self.sha256.as_str().to_string())),
        ])
    }
}

/// One W1 journal event emitted on successful (non-dry-run) `report.export_csv@1` (`REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportCsvEvent {
    pub resolved_digest: Digest,
    pub receipts: Vec<ExportReceipt>,
}

impl ExportCsvEvent {
    pub fn to_value(&self) -> CValue {
        obj([
            ("receipts", CValue::Arr(self.receipts.iter().map(ExportReceipt::to_value).collect())),
            ("resolved_digest", CValue::Str(self.resolved_digest.as_str().to_string())),
            ("type", CValue::Str(EXPORT_CSV_CMD.to_string())),
        ])
    }
}

/// Result of `report.export_csv@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportCsvResult {
    pub resolved_digest: Digest,
    pub dry_run: bool,
    pub files: Vec<CsvFileEntry>,
    pub receipts: Vec<ExportReceipt>,
}

impl ExportCsvResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("dry_run", CValue::Bool(self.dry_run)),
            ("files", CValue::Arr(self.files.iter().map(CsvFileEntry::to_value).collect())),
            ("receipts", CValue::Arr(self.receipts.iter().map(ExportReceipt::to_value).collect())),
            ("resolved_digest", CValue::Str(self.resolved_digest.as_str().to_string())),
        ])
    }
}

/// Journal seam for recording one W1 `report.export_csv@1` event.
pub trait CsvExportJournal {
    fn record_csv_export(&mut self, event: ExportCsvEvent) -> Result<(), CsvExportError>;
}

/// In-memory journal for tests and CLI execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryCsvJournal {
    events: Vec<ExportCsvEvent>,
}

impl MemoryCsvJournal {
    pub fn new() -> MemoryCsvJournal {
        MemoryCsvJournal::default()
    }

    pub fn events(&self) -> &[ExportCsvEvent] {
        &self.events
    }
}

impl CsvExportJournal for MemoryCsvJournal {
    fn record_csv_export(&mut self, event: ExportCsvEvent) -> Result<(), CsvExportError> {
        self.events.push(event);
        Ok(())
    }
}

/// Lookup seam supplying a [`ResolvedReport`] by digest or report ID.
pub trait ResolvedLookup {
    fn lookup(&self, key: &str) -> Option<ResolvedReport>;
}

/// In-memory resolved report map keyed by digest string (`sha256:...`) and/or report UUID.
#[derive(Debug, Clone, Default)]
pub struct MemoryResolvedLookup {
    entries: BTreeMap<String, ResolvedReport>,
}

impl MemoryResolvedLookup {
    pub fn new() -> MemoryResolvedLookup {
        MemoryResolvedLookup::default()
    }

    pub fn insert(&mut self, resolved: ResolvedReport) {
        self.entries.insert(resolved.digest.as_str().to_string(), resolved);
    }

    pub fn insert_alias(&mut self, key: impl Into<String>, resolved: ResolvedReport) {
        self.entries.insert(key.into(), resolved);
    }
}

impl ResolvedLookup for MemoryResolvedLookup {
    fn lookup(&self, key: &str) -> Option<ResolvedReport> {
        self.entries.get(key).cloned()
    }
}

// ------------------------------------------------------------------ command schema & handler

/// Wire parameters of `report.export_csv@1` (`Design: Registry command`).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ExportCsvParams {
    #[fid(description = "Resolved report digest (sha256:...) or report ID.")]
    pub resolved: String,
    #[fid(description = "Approved local export root handle (e.g. root://exports).")]
    pub root: String,
    #[fid(description = "Base file name prefix (produces <base>-01.csv .. and <base>-meta.json).")]
    pub base: String,
    #[fid(description = "If true, overwrite existing export files atomically.")]
    pub overwrite: bool,
    #[fid(description = "If true, render and compute digests without writing files or journaling.")]
    pub dry_run: bool,
    #[fid(description = "If true, require the resolved report to have been generated with SI units.")]
    pub require_si: bool,
}

/// Wire output of `report.export_csv@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ExportCsvOutput {
    #[fid(description = "Resolved report digest that was exported.")]
    pub resolved_digest: String,
    #[fid(description = "Whether dry-run mode was active.")]
    pub dry_run: bool,
    #[fid(description = "Number of CSV + metadata files written (or planned in dry-run).")]
    pub file_count: u32,
}

/// Execution context for `report.export_csv@1`.
pub struct ExportCsvEnv<'a> {
    pub principal: Principal,
    pub lookup: &'a dyn ResolvedLookup,
    pub sink: &'a ExportSink<'a>,
    pub journal: &'a mut dyn CsvExportJournal,
}

/// Executes `report.export_csv@1` (`REQ-RPT.OUTPUT.SHEET.CSV-001`..`005`).
pub fn export_csv(params: &ExportCsvParams, env: &mut ExportCsvEnv<'_>) -> Result<ExportCsvResult, CsvExportError> {
    // Validate base prefix up front.
    let base = ExportName::parse(&params.base)?;
    let resolved =
        env.lookup.lookup(&params.resolved).ok_or_else(|| CsvExportError::ResolvedNotFound(params.resolved.clone()))?;

    let tables = convert_report(&resolved, params.require_si)?;
    if tables.len() > MAX_CSV_TABLES {
        return Err(CsvExportError::BadParams("too_many_tables"));
    }

    // Build all planned file names and payloads deterministically BEFORE touching the root.
    let mut planned: Vec<(ExportName, Vec<u8>)> = Vec::with_capacity(tables.len() + 1);
    for (idx, table) in tables.iter().enumerate() {
        let ordinal = idx + 1;
        let fname = ExportName::parse(&format!("{}-{ordinal:02}.csv", base.as_str()))?;
        let bytes = render_table_csv(table);
        planned.push((fname, bytes));
    }

    let renderer = RendererVersion::new(CSV_RENDERER_ID, CSV_RENDERER_VERSION)?;
    let meta = ExportMeta::from_resolved(&resolved, renderer);
    let meta_name = ExportName::parse(&format!("{}-meta.json", base.as_str()))?;
    let meta_bytes = meta.to_canonical_bytes()?;
    planned.push((meta_name, meta_bytes));

    let files: Vec<CsvFileEntry> = planned
        .iter()
        .map(|(name, bytes)| CsvFileEntry {
            name: name.clone(),
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            sha256: Digest::from_bytes(Sha256::digest(bytes).into()),
        })
        .collect();

    if params.dry_run {
        return Ok(ExportCsvResult { resolved_digest: resolved.digest, dry_run: true, files, receipts: Vec::new() });
    }

    // Write every file through ExportSink; on any failure roll back all files written by this call (REQ-005, AC-4).
    let mut receipts = Vec::with_capacity(planned.len());
    let mut written_names: Vec<ExportName> = Vec::with_capacity(planned.len());
    for (name, bytes) in &planned {
        match env.sink.write(env.principal, &params.root, name.as_str(), bytes, params.overwrite) {
            Ok(receipt) => {
                written_names.push(name.clone());
                receipts.push(receipt);
            }
            Err(err) => {
                for prev in written_names.iter().rev() {
                    let _ = env.sink.remove_written(env.principal, &params.root, prev);
                }
                return Err(CsvExportError::Export(err));
            }
        }
    }

    let event = ExportCsvEvent { resolved_digest: resolved.digest, receipts: receipts.clone() };
    if let Err(err) = env.journal.record_csv_export(event) {
        for prev in written_names.iter().rev() {
            let _ = env.sink.remove_written(env.principal, &params.root, prev);
        }
        return Err(err);
    }

    Ok(ExportCsvResult { resolved_digest: resolved.digest, dry_run: false, files, receipts })
}

#[fid_command(
    id = "report.export_csv",
    major = 1,
    title = "Export report tables as CSV",
    description = "Exports each table block of a resolved report as RFC 4180 UTF-8 CSV files plus canonical ExportMeta into an approved local export root.",
    side_effect = W1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = WriteRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [EXPORT_CSV_CMD],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
fn export_csv_decl(env: &mut ExportCsvEnv<'_>, params: ExportCsvParams) -> Result<ExportCsvOutput, CsvExportError> {
    let res = export_csv(&params, env)?;
    Ok(ExportCsvOutput {
        resolved_digest: res.resolved_digest.as_str().to_string(),
        dry_run: res.dry_run,
        file_count: u32::try_from(res.files.len()).unwrap_or(u32::MAX),
    })
}

fid_commands![export_csv_decl];

/// Registry definitions for `report.export_csv@1`.
pub fn csv_command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

// ------------------------------------------------------------------ CLI dispatcher

/// Parses CLI arguments for `fid report export-csv` (after `export-csv`) and executes it.
pub fn invoke_export_csv_cli(
    args: &[&str],
    env: &mut ExportCsvEnv<'_>,
) -> Result<(ExportCsvResult, bool), CsvExportError> {
    let mut resolved: Option<String> = None;
    let mut root: Option<String> = None;
    let mut base: Option<String> = None;
    let mut overwrite = false;
    let mut dry_run = false;
    let mut require_si = false;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i] {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--overwrite" => {
                overwrite = true;
                i += 1;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            "--si" => {
                require_si = true;
                i += 1;
            }
            "--resolved" => {
                let v = args.get(i + 1).ok_or(CsvExportError::BadParams("missing_resolved"))?;
                resolved = Some((*v).to_string());
                i += 2;
            }
            "--root" => {
                let v = args.get(i + 1).ok_or(CsvExportError::BadParams("missing_root"))?;
                root = Some((*v).to_string());
                i += 2;
            }
            "--base" => {
                let v = args.get(i + 1).ok_or(CsvExportError::BadParams("missing_base"))?;
                base = Some((*v).to_string());
                i += 2;
            }
            _ => return Err(CsvExportError::BadParams("unknown_cli_flag")),
        }
    }

    let params = ExportCsvParams {
        resolved: resolved.ok_or(CsvExportError::BadParams("missing_resolved"))?,
        root: root.ok_or(CsvExportError::BadParams("missing_root"))?,
        base: base.ok_or(CsvExportError::BadParams("missing_base"))?,
        overwrite,
        dry_run,
        require_si,
    };
    let result = export_csv(&params, env)?;
    Ok((result, json_flag))
}

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    CValue::Obj(m)
}
