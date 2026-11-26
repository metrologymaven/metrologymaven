//! `report.move_block_layout@1`: parameters, handler, result and the `--json` dispatcher
//! (`RPT.DESIGN.PLACE.FRAME`, `Design: Registry command`).
//!
//! The command checks everything that can fail before it commits, in this order: the
//! lengths (`x`, `y`, `w`, `h`, `dx`, `dy`, `dw`, `dh`; REQ-001), the grid pitch when
//! snapping is on (REQ-002), the report, the block and the target page (`report.not_found`),
//! the target frame, snapping (REQ-002) and the frame bounds (REQ-004). Then it commits the
//! new frame, and for a page move the report with the block moved, with one event (REQ-003).
//! A failed command commits nothing and journals nothing.
//!
//! The target is either absolute (`x`, `y`, `w`, `h`) or a delta (`dx`, `dy`, `dw`, `dh`)
//! added to the stored frame, never both. A coordinate the caller leaves out keeps its stored
//! value; a block that has no frame yet needs all four absolute coordinates, else the first
//! coordinate that needs the stored frame is `report.layout_invalid { <coordinate>, no_frame }`.
//! `x`, `y` and every delta may be negative (`-1.5mm`); `w` and `h` may not.

use super::backend::FrameBackend;
use super::event::{FrameEvent, MOVE_BLOCK_LAYOUT};
use super::model::{BlockFrame, ReportFrames};
use super::pitch::PitchSetting;
use crate::doc::model::{obj, parse_canonical_uuid};
use crate::doc::{BlockId, KindRegistry, PageId, ReportId, StoredReport};
use crate::layout::page::{parse_length, LayoutError, Length, LengthFault};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;

type Params = BTreeMap<String, CValue>;

/// The target frame of a move: absolute coordinates or a delta, each coordinate optional.
/// Lengths are text with a unit (`10.04mm`, `-0.5in`), checked by the handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameTarget {
    Absolute { x: Option<String>, y: Option<String>, w: Option<String>, h: Option<String> },
    Delta { dx: Option<String>, dy: Option<String>, dw: Option<String>, dh: Option<String> },
}

/// `report.move_block_layout@1` parameters: `{report, block, page?, snap?, x?, y?, w?, h?,
/// dx?, dy?, dw?, dh?}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveBlockLayout {
    pub report: Uuid,
    pub block: Uuid,
    /// The page to move the block to; `None` keeps its page.
    pub page: Option<Uuid>,
    pub to: FrameTarget,
    /// Snap the frame edges to the grid pitch (REQ-002). Default `false`.
    pub snap: bool,
}

const KEYS: [&str; 12] = ["block", "dh", "dw", "dx", "dy", "h", "page", "report", "snap", "w", "x", "y"];

fn params(v: &CValue) -> Result<&Params, LayoutError> {
    let CValue::Obj(m) = v else { return Err(LayoutError::BadParams("params is not an object")) };
    if m.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return Err(LayoutError::BadParams("unknown parameter"));
    }
    Ok(m)
}

/// `None` for an absent key or JSON null.
fn opt_str(m: &Params, k: &'static str) -> Result<Option<String>, LayoutError> {
    match m.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(LayoutError::BadParams(k)),
    }
}

fn id(m: &Params, k: &'static str) -> Result<Option<Uuid>, LayoutError> {
    opt_str(m, k)?.map(|s| parse_canonical_uuid(&s).ok_or(LayoutError::BadParams(k))).transpose()
}

impl MoveBlockLayout {
    pub fn from_value(v: &CValue) -> Result<MoveBlockLayout, LayoutError> {
        let m = params(v)?;
        let snap = match m.get("snap") {
            None | Some(CValue::Null) => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(LayoutError::BadParams("snap")),
        };
        let absolute = [opt_str(m, "x")?, opt_str(m, "y")?, opt_str(m, "w")?, opt_str(m, "h")?];
        let delta = [opt_str(m, "dx")?, opt_str(m, "dy")?, opt_str(m, "dw")?, opt_str(m, "dh")?];
        let any = |a: &[Option<String>; 4]| a.iter().any(Option::is_some);
        let to = match (any(&absolute), any(&delta)) {
            (true, true) => return Err(LayoutError::BadParams("absolute and delta coordinates mixed")),
            (false, false) => return Err(LayoutError::BadParams("no frame coordinate")),
            (true, false) => {
                let [x, y, w, h] = absolute;
                FrameTarget::Absolute { x, y, w, h }
            }
            (false, true) => {
                let [dx, dy, dw, dh] = delta;
                FrameTarget::Delta { dx, dy, dw, dh }
            }
        };
        Ok(MoveBlockLayout {
            report: id(m, "report")?.ok_or(LayoutError::BadParams("report"))?,
            block: id(m, "block")?.ok_or(LayoutError::BadParams("block"))?,
            page: id(m, "page")?,
            to,
            snap,
        })
    }
}

// ------------------------------------------------------------------ helpers

/// A length that may carry one leading `-` (`-1.5mm`), otherwise as
/// [`parse_length`](crate::layout::page::parse_length).
pub fn parse_signed_length(text: &str) -> Result<Length, LengthFault> {
    match text.strip_prefix('-') {
        Some(rest) => parse_length(rest).map(|n| -n),
        None => parse_length(text),
    }
}

fn length(field: &'static str, text: Option<&String>, signed: bool) -> Result<Option<Length>, LayoutError> {
    let Some(text) = text else { return Ok(None) };
    let parsed = if signed { parse_signed_length(text) } else { parse_length(text) };
    parsed.map(Some).map_err(|f| LayoutError::LayoutInvalid { field, reason: f.reason() })
}

fn invalid(field: &'static str, reason: &'static str) -> LayoutError {
    LayoutError::LayoutInvalid { field, reason }
}

/// The parsed coordinates in `x`, `y`, `w`, `h` order and whether they are a delta.
fn coordinates(to: &FrameTarget) -> Result<([Option<Length>; 4], bool), LayoutError> {
    match to {
        FrameTarget::Absolute { x, y, w, h } => Ok((
            [
                length("x", x.as_ref(), true)?,
                length("y", y.as_ref(), true)?,
                length("w", w.as_ref(), false)?,
                length("h", h.as_ref(), false)?,
            ],
            false,
        )),
        FrameTarget::Delta { dx, dy, dw, dh } => Ok((
            [
                length("dx", dx.as_ref(), true)?,
                length("dy", dy.as_ref(), true)?,
                length("dw", dw.as_ref(), true)?,
                length("dh", dh.as_ref(), true)?,
            ],
            true,
        )),
    }
}

const COORDS: [&str; 4] = ["x", "y", "w", "h"];

/// The target frame before snapping: each coordinate absolute, stored plus delta, or kept.
fn target_frame(given: [Option<Length>; 4], delta: bool, base: Option<BlockFrame>) -> Result<BlockFrame, LayoutError> {
    let base = base.map(|f| [f.x, f.y, f.w, f.h]);
    let mut out: [Length; 4] = [0; 4];
    for (i, (slot, name)) in out.iter_mut().zip(COORDS).enumerate() {
        let g = given.get(i).copied().flatten();
        let b = base.and_then(|b| b.get(i).copied());
        *slot = match (g, delta, b) {
            (Some(v), false, _) => v,
            (_, _, None) => return Err(invalid(name, "no_frame")),
            (d, true, Some(b)) => b.checked_add(d.unwrap_or(0)).ok_or(invalid(name, "too_large"))?,
            (None, false, Some(b)) => b,
        };
    }
    let [x, y, w, h] = out;
    Ok(BlockFrame { x, y, w, h })
}

/// Everything a successful move commits.
struct Plan {
    report: ReportId,
    moved: Option<StoredReport>,
    frames: ReportFrames,
    event: FrameEvent,
}

fn plan(
    backend: &dyn FrameBackend,
    kinds: &KindRegistry,
    pitch: &PitchSetting,
    p: &MoveBlockLayout,
) -> Result<Plan, LayoutError> {
    let (given, delta) = coordinates(&p.to)?;
    let pitch = if p.snap { Some(pitch.clone()?) } else { None };

    let rid = ReportId::new(p.report).ok_or(LayoutError::NotFound(p.report))?;
    let stored = backend.load(&rid)?.ok_or(LayoutError::NotFound(p.report))?;
    let report = stored.open(&rid, kinds)?;
    let block: BlockId = report.block(&p.block).map(|b| b.id()).ok_or(LayoutError::NotFound(p.block))?;
    let from_page = report.page_of_block(&p.block).ok_or(LayoutError::NotFound(p.block))?;
    let page: PageId = match &p.page {
        None => from_page,
        Some(id) => {
            report.pages().iter().map(|pg| pg.id()).find(|pg| pg.uuid() == *id).ok_or(LayoutError::NotFound(*id))?
        }
    };

    let current = backend.load_frames(&rid)?.unwrap_or_default();
    let mut frame = target_frame(given, delta, current.get(&block))?;
    if let Some(g) = pitch {
        frame = frame.snapped(g.units()).ok_or(invalid("x", "too_large"))?;
    }
    frame.check()?;

    let mut frames = current;
    frames.frames.insert(block, frame);
    let moved = if page == from_page {
        None
    } else {
        let mut r = report;
        r.move_block_to_page(&p.block, &page.uuid())?;
        Some(StoredReport::of(&r)?)
    };
    let report_digest = moved.as_ref().map_or(stored.digest, |s| s.digest);
    let event =
        FrameEvent { report: rid, block, from_page, page, frame, frames_digest: frames.digest()?, report_digest };
    Ok(Plan { report: rid, moved, frames, event })
}

// ------------------------------------------------------------------ handlers

/// The outcome of a successful `report.move_block_layout@1`: the block's page and frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveOutcome {
    pub report: ReportId,
    pub block: BlockId,
    pub page: PageId,
    pub frame: BlockFrame,
}

impl MoveOutcome {
    /// Result: `{block, h, page, report, w, x, y}`, integers as decimal strings.
    pub fn to_value(&self) -> CValue {
        let f = self.frame;
        let int = |n: Length| CValue::Str(n.to_string());
        obj([
            ("block", CValue::Str(self.block.uuid().to_string())),
            ("h", int(f.h)),
            ("page", CValue::Str(self.page.uuid().to_string())),
            ("report", CValue::Str(self.report.uuid().to_string())),
            ("w", int(f.w)),
            ("x", int(f.x)),
            ("y", int(f.y)),
        ])
    }
}

fn outcome(plan: &Plan) -> MoveOutcome {
    MoveOutcome { report: plan.report, block: plan.event.block, page: plan.event.page, frame: plan.event.frame }
}

/// `report.move_block_layout@1` (W2): sets a block's frame, optionally snapped to `pitch`
/// and optionally on another page, and journals one event (REQ-003). `pitch` is read only
/// when snapping is on.
pub fn move_block_layout(
    backend: &mut dyn FrameBackend,
    kinds: &KindRegistry,
    pitch: &PitchSetting,
    p: &MoveBlockLayout,
) -> Result<MoveOutcome, LayoutError> {
    let plan = plan(&*backend, kinds, pitch, p)?;
    let out = outcome(&plan);
    backend.commit_frames(&plan.report, plan.moved, plan.frames, plan.event)?;
    Ok(out)
}

/// The `--dry-run` form: every check and the resulting frame of
/// [`move_block_layout`], with nothing committed or journaled.
pub fn preview_move_block_layout(
    backend: &dyn FrameBackend,
    kinds: &KindRegistry,
    pitch: &PitchSetting,
    p: &MoveBlockLayout,
) -> Result<MoveOutcome, LayoutError> {
    plan(backend, kinds, pitch, p).map(|plan| outcome(&plan))
}

/// Runs `report.move_block_layout@1` with `--json` parameters and returns its `--json`
/// result. Any other command is `report.bad_params`.
pub fn execute(
    command: &str,
    params: &CValue,
    backend: &mut dyn FrameBackend,
    kinds: &KindRegistry,
    pitch: &PitchSetting,
) -> Result<CValue, LayoutError> {
    match command {
        MOVE_BLOCK_LAYOUT => {
            Ok(move_block_layout(backend, kinds, pitch, &MoveBlockLayout::from_value(params)?)?.to_value())
        }
        _ => Err(LayoutError::BadParams("unknown command")),
    }
}
