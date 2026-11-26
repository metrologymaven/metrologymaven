//! The journal event of block placement (`RPT.DESIGN.PLACE.FRAME` REQ-003): one
//! `report.move_block_layout@1` event per successful command.
//!
//! ```text
//! report.move_block_layout@1  {type, report, block, from_page, page, frame,
//!                              frames_digest, report_digest}
//! ```
//!
//! `frame` is the resolved frame after snapping, so replay never re-derives it from the
//! command's lengths or the grid pitch setting. `from_page` and `page` are the block's page
//! before and after the command (equal unless it moved page). `frames_digest` is the SHA-256
//! of the report's frames after the command and `report_digest` the stored report's digest
//! after it (unchanged unless the block moved page); exact replay compares both. The `type`
//! key is the one `RPT.MODEL.DOC` and `RPT.DESIGN.PAGE` events use, so all three kinds share
//! one journal.

use super::model::BlockFrame;
use crate::doc::model::obj;
use crate::doc::{BlockId, PageId, ReportId};
use crate::layout::page::LayoutError;
use fid_canon::CValue;
use fid_types::Digest;

/// Command and event ID: set a block's frame, optionally moving it to another page (`W2`).
pub const MOVE_BLOCK_LAYOUT: &str = "report.move_block_layout@1";

/// One `report.move_block_layout@1` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameEvent {
    pub report: ReportId,
    pub block: BlockId,
    /// The page that held the block before the command.
    pub from_page: PageId,
    /// The page that holds the block after the command.
    pub page: PageId,
    pub frame: BlockFrame,
    /// Digest of the report's frames after the command ([`super::ReportFrames::digest`]).
    pub frames_digest: Digest,
    /// Digest of the stored report after the command.
    pub report_digest: Digest,
}

fn id_value(u: fid_project::Uuid) -> CValue {
    CValue::Str(u.to_string())
}

impl FrameEvent {
    /// Canonical journal form; keys sorted, integers as decimal strings.
    pub fn to_value(&self) -> CValue {
        obj([
            ("block", id_value(self.block.uuid())),
            ("frame", self.frame.to_value()),
            ("frames_digest", CValue::Str(self.frames_digest.as_str().to_string())),
            ("from_page", id_value(self.from_page.uuid())),
            ("page", id_value(self.page.uuid())),
            ("report", id_value(self.report.uuid())),
            ("report_digest", CValue::Str(self.report_digest.as_str().to_string())),
            ("type", CValue::Str(MOVE_BLOCK_LAYOUT.to_string())),
        ])
    }

    /// Reads an event back: exactly its keys, canonical UUIDv7 IDs, a stored frame form and
    /// `sha256:` digests. Anything else is `report.store_invalid`.
    pub fn from_value(v: &CValue) -> Result<FrameEvent, LayoutError> {
        let bad = || LayoutError::StoreInvalid("block frame event");
        let CValue::Obj(m) = v else { return Err(bad()) };
        let keys = ["block", "frame", "frames_digest", "from_page", "page", "report", "report_digest", "type"];
        if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
            return Err(bad());
        }
        if m.get("type") != Some(&CValue::Str(MOVE_BLOCK_LAYOUT.to_string())) {
            return Err(bad());
        }
        let field = |k: &str| m.get(k).unwrap_or(&CValue::Null);
        let digest = |k: &str| match field(k) {
            CValue::Str(s) => Digest::parse(s).map_err(|_| bad()),
            _ => Err(bad()),
        };
        Ok(FrameEvent {
            report: ReportId::from_value(field("report")).map_err(|_| bad())?,
            block: BlockId::from_value(field("block")).map_err(|_| bad())?,
            from_page: PageId::from_value(field("from_page")).map_err(|_| bad())?,
            page: PageId::from_value(field("page")).map_err(|_| bad())?,
            frame: BlockFrame::from_value(field("frame"))?,
            frames_digest: digest("frames_digest")?,
            report_digest: digest("report_digest")?,
        })
    }
}

/// True when `v` is a block placement event (its `type` is [`MOVE_BLOCK_LAYOUT`]).
pub fn is_frame_event(v: &CValue) -> bool {
    matches!(v, CValue::Obj(m) if matches!(m.get("type"), Some(CValue::Str(t)) if t == MOVE_BLOCK_LAYOUT))
}
