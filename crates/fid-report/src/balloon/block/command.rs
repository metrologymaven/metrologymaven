//! Command `report.set_drawing_balloon@1` (`W2`, `--dry-run`, one journal event) and its
//! validated registry [`CommandDef`] (`RPT.BALLOON.BLOCK`, `REQ-006`, `AC-5`, `AC-6`).

use super::model::{DrawingBlockError, DrawingPayload, FitMode, SheetRect, DRAWING_BLOCK_KIND};
use crate::balloon::sheet::{SheetId, SheetInfo};
use crate::doc::backend::{ReportBackend, StoredReport};
use crate::doc::defs::ID_PATTERN;
use crate::doc::event::{Op, ReportEvent, SET_DRAWING_BALLOON};
use crate::doc::kind::KindRegistry;
use crate::doc::model::{obj, Block, BlockId, ReportId};
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-22";
const UINT_STR_PATTERN: &str = "^(0|[1-9][0-9]{0,18})$";
const POS_INT_STR_PATTERN: &str = "^[1-9][0-9]{0,18}$";

/// Parameters of `report.set_drawing_balloon@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDrawingBalloonParams {
    pub report: ReportId,
    pub block: BlockId,
    pub sheet: SheetId,
    pub crop: Option<SheetRect>,
    pub fit: FitMode,
    pub outcome_marking: bool,
    pub show_leaders: bool,
    pub dry_run: bool,
}

/// Validates and updates a `drawing` report block's payload (`report.set_drawing_balloon@1`, `W2`).
///
/// Fails with `report.invalid_block_payload` (or `report.not_found`) and journals nothing on
/// any validation error (`REQ-006`, `AC-5`). When `params.dry_run` is true, validates fully and
/// returns the updated preview [`Block`] without mutating `backend` or appending a journal event.
pub fn set_drawing_balloon(
    backend: &mut dyn ReportBackend,
    kinds: &KindRegistry,
    sheets: &dyn SheetInfo,
    params: &SetDrawingBalloonParams,
) -> Result<Block, DrawingBlockError> {
    let Some(stored) = backend.load(&params.report).map_err(|e| DrawingBlockError::Store(e.to_string()))? else {
        return Err(DrawingBlockError::NotFound("report"));
    };
    let report = stored.open(&params.report, kinds).map_err(|e| DrawingBlockError::Store(e.to_string()))?;
    let Some(existing_block) = report.block(&params.block.uuid()) else {
        return Err(DrawingBlockError::NotFound("block"));
    };
    if existing_block.kind().as_str() != DRAWING_BLOCK_KIND {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "block", reason: "not_a_drawing_block" });
    }

    // Validate sheet exists and is ready (`REQ-006`, `AC-5`).
    let Some((sheet_w, sheet_h)) = sheets.bounds(params.sheet) else {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "sheet", reason: "sheet_not_found_or_not_ready" });
    };
    if let Some(crop) = params.crop {
        if !crop.fits_in_sheet(sheet_w, sheet_h) {
            return Err(DrawingBlockError::InvalidBlockPayload {
                field: "crop",
                reason: "crop_empty_or_out_of_bounds",
            });
        }
    }
    if let FitMode::FixedScale { num, den } = params.fit {
        if num == 0 || den == 0 {
            return Err(DrawingBlockError::InvalidBlockPayload { field: "fit", reason: "zero_scale" });
        }
    }

    let payload_struct = DrawingPayload {
        sheet: params.sheet,
        crop: params.crop,
        fit: params.fit,
        outcome_marking: params.outcome_marking,
        show_leaders: params.show_leaders,
    };
    let payload = payload_struct.to_canonical_json()?;
    let op = Op::SetDrawingBalloon { report: params.report, block: params.block, payload };
    let next_report = op.apply(Some(report), kinds).map_err(|e| match e {
        crate::doc::ReportError::NotFound(_) => DrawingBlockError::NotFound("block"),
        crate::doc::ReportError::InvalidBlockPayload(_) => {
            DrawingBlockError::InvalidBlockPayload { field: "payload", reason: "schema_invalid" }
        }
        other => DrawingBlockError::Store(other.to_string()),
    })?;
    let updated_block = next_report.block(&params.block.uuid()).cloned().ok_or(DrawingBlockError::NotFound("block"))?;

    if params.dry_run {
        return Ok(updated_block);
    }

    let next_stored = StoredReport::of(&next_report).map_err(|e| DrawingBlockError::Store(e.to_string()))?;
    let event = ReportEvent::new(op, next_stored.digest);
    backend.commit(&params.report, next_stored, event).map_err(|e| DrawingBlockError::Store(e.to_string()))?;

    Ok(updated_block)
}

/// Returns the validated [`CommandDef`] for `report.set_drawing_balloon@1` (`W2`, `D0`,
/// CLI `fid report set-drawing-balloon`, MCP `report_set_drawing_balloon`).
pub fn set_drawing_balloon_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id = |description: &str| {
        obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
    };
    let object = |required: &[&str], properties: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(required)),
            ("properties", properties),
        ])
    };
    let crop_rect = object(
        &["height", "width", "x", "y"],
        obj([
            ("height", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("width", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("x", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))])),
            ("y", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))])),
        ]),
    );
    let fit_scale = object(&["kind"], obj([("kind", obj([("const", text("scale_to_fit"))]))]));
    let fit_fixed = object(
        &["den", "kind", "num"],
        obj([
            ("den", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("kind", obj([("const", text("fixed_scale"))])),
            ("num", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
        ]),
    );
    let fit_schema = obj([("anyOf", CValue::Arr(vec![fit_scale, fit_fixed]))]);

    CommandDef::from_value(obj([
        ("id", text("report.set_drawing_balloon")),
        ("major", crate::json_num::num_usize(1)),
        ("title", text("Configure a ballooned-drawing report block")),
        (
            "description",
            text("Updates a drawing report block's target sheet, optional crop, fit policy, and balloon display options."),
        ),
        (
            "params_schema",
            object(
                &["block", "fit", "outcome_marking", "report", "sheet", "show_leaders"],
                obj([
                    ("block", id("Target drawing block ID (UUIDv7).")),
                    ("crop", obj([("anyOf", CValue::Arr(vec![crop_rect, obj([("type", text("null"))])]))])),
                    ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                    ("fit", fit_schema),
                    ("outcome_marking", obj([("type", text("boolean"))])),
                    ("report", id("Target report ID (UUIDv7).")),
                    ("sheet", id("Target drawing sheet ID (UUIDv7).")),
                    ("show_leaders", obj([("type", text("boolean"))])),
                ]),
            ),
        ),
        (
            "result_schema",
            object(
                &["id", "kind", "kind_version", "payload"],
                obj([
                    ("id", id("Block ID (UUIDv7).")),
                    ("kind", obj([("type", text("string"))])),
                    ("kind_version", obj([("type", text("integer"))])),
                    ("payload", obj([("type", text("object"))])),
                ]),
            ),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[SET_DRAWING_BALLOON])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
