//! Command handler `report.export_balloon_pack@1` (`W1`, `fs_effect: write_root`), registry
//! `CommandDef`, root confinement, and export journal (`RPT.BALLOON.AUTO.EXPORT`).

use super::csv_form3::{render_form3_starter_csv, EvaluatedRowValue};
use super::pdf_overlay::{render_ballooned_pdf, BalloonOutcome};
use crate::balloon::auto_text::{extract_drawing_page, ExtractDrawingTextParams, SheetSegment};
use crate::balloon::char::CharBalloonStore;
use crate::balloon::register::BalloonRegister;
use crate::balloon::sheet::{digest_bytes, DrawingBlobBackend, DrawingRevId, DrawingStore, SheetId};
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_io_reg::{AtomicWriter, Principal, RootSet, WriteOptions};
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Canonical command / event ID for balloon pack export (`W1`, `write_root`).
pub const EXPORT_BALLOON_PACK: &str = "report.export_balloon_pack@1";

/// Parameters of `report.export_balloon_pack@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportBalloonPackParams {
    pub drawing_rev: DrawingRevId,
    /// Approved export-root directory handle (e.g. `root://exports/pack`).
    pub out_dir: String,
    pub outcome_marking: bool,
    pub include_unmeasured: bool,
    pub dry_run: bool,
}

/// Output of `report.export_balloon_pack@1` (`Design: Data structures`, `AC-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonPackResult {
    pub pdf_path: String,
    pub pdf_digest: Digest,
    pub csv_path: String,
    pub csv_digest: Digest,
    pub balloon_count: u32,
    pub committed: bool,
}

impl BalloonPackResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("balloon_count", CValue::Str(self.balloon_count.to_string())),
            ("committed", CValue::Bool(self.committed)),
            ("csv_digest", CValue::Str(self.csv_digest.to_string())),
            ("csv_path", CValue::Str(self.csv_path.clone())),
            ("pdf_digest", CValue::Str(self.pdf_digest.to_string())),
            ("pdf_path", CValue::Str(self.pdf_path.clone())),
        ])
    }
}

/// Typed errors of `report.export_balloon_pack@1` (`Design: Errors`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportPackError {
    /// `io.root_violation`: destination escapes or violates approved export roots (`REQ-004`, `AC-3`).
    RootViolation(String),
    /// `report.not_found`: drawing revision or sheet does not exist.
    NotFound(&'static str),
    /// `report.no_balloons`: drawing revision has zero balloons to export.
    NoBalloons,
    /// Atomic write or filesystem I/O failure.
    Io(String),
}

impl ExportPackError {
    pub fn code(&self) -> &'static str {
        match self {
            ExportPackError::RootViolation(_) => "io.root_violation",
            ExportPackError::NotFound(_) => "report.not_found",
            ExportPackError::NoBalloons => "report.no_balloons",
            ExportPackError::Io(_) => "io.write_failed",
        }
    }
}

impl fmt::Display for ExportPackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportPackError::RootViolation(d) => write!(f, "io.root_violation: {d}"),
            ExportPackError::NotFound(w) => write!(f, "report.not_found: {w}"),
            ExportPackError::NoBalloons => write!(f, "report.no_balloons: drawing revision has no balloons"),
            ExportPackError::Io(d) => write!(f, "io.write_failed: {d}"),
        }
    }
}

impl std::error::Error for ExportPackError {}

/// Canonical journal record emitted by `report.export_balloon_pack@1` (`AC-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportBalloonPackEvent {
    pub drawing_rev: DrawingRevId,
    pub result: BalloonPackResult,
}

/// Executes `report.export_balloon_pack@1` (`W1`, D0, `REQ-001..005`, `AC-1..AC-4`).
///
/// Validates export-root confinement for BOTH `ballooned.pdf` and `form3-starter.csv` BEFORE
/// writing any file, failing closed with `io.root_violation` on path traversal or unapproved roots.
#[allow(clippy::too_many_arguments)]
pub fn export_balloon_pack(
    drawing_store: &DrawingStore,
    blobs: &dyn DrawingBlobBackend,
    register: &BalloonRegister,
    char_store: &CharBalloonStore,
    evaluated: &BTreeMap<CharId, EvaluatedRowValue>,
    roots: &RootSet,
    principal: Principal,
    journal: &mut Vec<ExportBalloonPackEvent>,
    p: &ExportBalloonPackParams,
) -> Result<BalloonPackResult, ExportPackError> {
    // 1. Validate `out_dir` handle syntax and confine both outputs inside approved export root (`REQ-004`, `AC-3`).
    let dir_clean = p.out_dir.trim().trim_end_matches('/');
    if dir_clean.is_empty()
        || !dir_clean.starts_with("root://")
        || dir_clean.split('/').any(|seg| seg == ".." || seg == ".")
    {
        return Err(ExportPackError::RootViolation(format!("destination '{}' violates export root", p.out_dir)));
    }
    let pdf_handle = format!("{dir_clean}/ballooned.pdf");
    let csv_handle = format!("{dir_clean}/form3-starter.csv");

    let pdf_target = roots
        .resolve_export(principal, &pdf_handle)
        .map_err(|e| ExportPackError::RootViolation(e.code().to_string()))?;
    let csv_target = roots
        .resolve_export(principal, &csv_handle)
        .map_err(|e| ExportPackError::RootViolation(e.code().to_string()))?;

    // 2. Gather sheets and ascending balloons (`report.not_found` / `report.no_balloons`).
    let rev = drawing_store.get_revision(p.drawing_rev).ok_or(ExportPackError::NotFound("drawing_rev"))?;
    let balloons = register.list(p.drawing_rev);
    if balloons.is_empty() {
        return Err(ExportPackError::NoBalloons);
    }

    let mut sheets = Vec::with_capacity(rev.sheets.len());
    let mut segments_by_sheet: BTreeMap<SheetId, Vec<SheetSegment>> = BTreeMap::new();
    for &sid in &rev.sheets {
        let s = drawing_store.sheet(sid).ok_or(ExportPackError::NotFound("sheet"))?.clone();
        if let Ok(ex) = extract_drawing_page(
            drawing_store,
            blobs,
            &ExtractDrawingTextParams { sheet: sid, include_frame_boxes: true },
        ) {
            segments_by_sheet.insert(sid, ex.segments);
        }
        sheets.push(s);
    }

    // 3. Build deterministic D0 vector-overlaid PDF and Form 3 starter CSV bytes in memory.
    let outcomes: BTreeMap<CharId, BalloonOutcome> = evaluated.iter().map(|(k, v)| (k.clone(), v.outcome)).collect();
    let pdf_bytes = render_ballooned_pdf(&sheets, &balloons, &segments_by_sheet, &outcomes, p.outcome_marking);

    let chars_map: BTreeMap<CharId, _> = char_store.all().map(|c| (c.id.clone(), c.clone())).collect();
    let csv_bytes = render_form3_starter_csv(&balloons, &chars_map, evaluated, p.include_unmeasured);

    let pdf_digest = digest_bytes(&pdf_bytes);
    let csv_digest = digest_bytes(&csv_bytes);
    let pdf_path = pdf_target.path().to_string_lossy().into_owned();
    let csv_path = csv_target.path().to_string_lossy().into_owned();

    if p.dry_run {
        return Ok(BalloonPackResult {
            pdf_path,
            pdf_digest,
            csv_path,
            csv_digest,
            balloon_count: balloons.len() as u32,
            committed: false,
        });
    }

    // 4. Write both files atomically through fid-io-reg AtomicWriter (never following symlinks).
    let mut pdf_writer = AtomicWriter::create(pdf_target, WriteOptions::overwrite(true))
        .map_err(|e| ExportPackError::Io(e.to_string()))?;
    pdf_writer.write_bytes(&pdf_bytes).map_err(|e| ExportPackError::Io(e.to_string()))?;
    let mut csv_writer = AtomicWriter::create(csv_target, WriteOptions::overwrite(true))
        .map_err(|e| ExportPackError::Io(e.to_string()))?;
    csv_writer.write_bytes(&csv_bytes).map_err(|e| ExportPackError::Io(e.to_string()))?;

    pdf_writer.commit().map_err(|e| ExportPackError::Io(e.to_string()))?;
    csv_writer.commit().map_err(|e| ExportPackError::Io(e.to_string()))?;

    let result = BalloonPackResult {
        pdf_path,
        pdf_digest,
        csv_path,
        csv_digest,
        balloon_count: balloons.len() as u32,
        committed: true,
    };
    journal.push(ExportBalloonPackEvent { drawing_rev: p.drawing_rev, result: result.clone() });
    Ok(result)
}

/// Returns the registry [`CommandDef`] for `report.export_balloon_pack@1` (`W1`, `write_root`).
pub fn command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |xs: &[&str]| CValue::Arr(xs.iter().map(|s| text(s)).collect());
    let num = crate::json_num::num_usize;
    let object = |req: &[&str], props: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(req)),
            ("properties", props),
        ])
    };
    let id = |desc: &str| obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(desc))]);
    let digest = || obj([("type", text("string")), ("pattern", text(Digest::PATTERN))]);

    let params = object(
        &["drawing_rev", "out_dir"],
        obj([
            ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
            ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
            ("include_unmeasured", obj([("type", text("boolean")), ("default", CValue::Bool(true))])),
            ("out_dir", obj([("type", text("string")), ("minLength", num(1))])),
            ("outcome_marking", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
        ]),
    );
    let result = object(
        &["balloon_count", "committed", "csv_digest", "csv_path", "pdf_digest", "pdf_path"],
        obj([
            ("balloon_count", obj([("type", text("string"))])),
            ("committed", obj([("type", text("boolean"))])),
            ("csv_digest", digest()),
            ("csv_path", obj([("type", text("string"))])),
            ("pdf_digest", digest()),
            ("pdf_path", obj([("type", text("string"))])),
        ]),
    );

    CommandDef::from_value(obj([
        ("id", text("report.export_balloon_pack")),
        ("major", num(1)),
        ("title", text("Export vector-overlaid ballooned PDF and AS9102 Form 3 starter CSV")),
        (
            "description",
            text("Writes deterministic vector ballooned.pdf and formula-safe AS9102 form3-starter.csv into an approved local export root."),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("W1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("write_root")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[EXPORT_BALLOON_PACK])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text("2026-11-22"))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
