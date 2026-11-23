//! Journal events of the report commands (REQ-001..003, REQ-011).
//!
//! One event per successful W2 command. Every ID the command created is in the event, so
//! replay reads IDs back instead of drawing new ones, and every event carries the SHA-256
//! of the stored report after the command (`report_digest`), which exact replay compares.
//!
//! Event forms (all keys required; `after` is `null` or an ID):
//!
//! ```text
//! report.create_report@1  {type, report, page, project, title, report_digest}
//! report.add_page@1       {type, report, after, page, report_digest}
//! report.remove_page@1    {type, report, page, force, report_digest}
//! report.add_block@1      {type, report, page, after, block, kind, kind_version, payload, report_digest}
//! report.remove_block@1   {type, report, block, report_digest}
//! report.bind_block@1     {type, report, block, payload, report_digest}
//! ```
//!
//! `report.bind_block@1` (spec node `RPT.MODEL.BIND.RESOLVE`) holds the block's whole new
//! payload: its `binding@1` and its `value_binding@1` fields, IDs and digests only.

use super::error::ReportError;
use super::kind::KindRegistry;
use super::model::{
    check_block, exact_obj, field, parse_canonical_uuid, payload_from_value, payload_value, u32_value, Block, BlockId,
    BlockKindId, CanonicalJson, PageId, Report, ReportId, MAX_REPORT_BYTES,
};
use super::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::BTreeMap;

pub const CREATE_REPORT: &str = "report.create_report@1";
pub const ADD_PAGE: &str = "report.add_page@1";
pub const REMOVE_PAGE: &str = "report.remove_page@1";
pub const ADD_BLOCK: &str = "report.add_block@1";
pub const REMOVE_BLOCK: &str = "report.remove_block@1";
/// `report.bind_block@1` (`RPT.MODEL.BIND.RESOLVE`).
pub const BIND_BLOCK: &str = "report.bind_block@1";
/// `report.set_table_block@1` (`RPT.MODEL.BLOCKS.TABLE`).
pub const SET_TABLE_BLOCK: &str = "report.set_table_block@1";
/// `report.set_text_block@1` (`RPT.MODEL.BLOCKS.TEXT`).
pub const SET_TEXT_BLOCK: &str = "report.set_text_block@1";
/// `report.set_image_block@1` (`RPT.MODEL.BLOCKS.TEXT`).
pub const SET_IMAGE_BLOCK: &str = "report.set_image_block@1";
/// `report.set_drawing_balloon@1` (`RPT.BALLOON.BLOCK`).
pub const SET_DRAWING_BALLOON: &str = "report.set_drawing_balloon@1";
/// The read-only command; it journals nothing.
pub const SHOW_REPORT: &str = "report.show_report@1";

/// What a W2 command did, with every ID it created.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    CreateReport {
        report: ReportId,
        page: PageId,
        project: Uuid,
        title: Untrusted<Text>,
    },
    AddPage {
        report: ReportId,
        after: Option<PageId>,
        page: PageId,
    },
    RemovePage {
        report: ReportId,
        page: PageId,
        force: bool,
    },
    AddBlock {
        report: ReportId,
        page: PageId,
        after: Option<BlockId>,
        block: BlockId,
        kind: BlockKindId,
        kind_version: u32,
        payload: CanonicalJson,
    },
    RemoveBlock {
        report: ReportId,
        block: BlockId,
    },
    /// Replaces a block's payload with its bound form (`report.bind_block@1`).
    BindBlock {
        report: ReportId,
        block: BlockId,
        payload: CanonicalJson,
    },
    /// Replaces a `table` block's column list, headers and filters (`report.set_table_block@1`).
    SetTableBlock {
        report: ReportId,
        block: BlockId,
        payload: CanonicalJson,
    },
    /// Replaces a `text`, `header` or `footer` block's runs (`report.set_text_block@1`).
    SetTextBlock {
        report: ReportId,
        block: BlockId,
        payload: CanonicalJson,
    },
    /// Replaces an `image` block's CAS digest, dimensions and caption (`report.set_image_block@1`).
    SetImageBlock {
        report: ReportId,
        block: BlockId,
        payload: CanonicalJson,
    },
    /// Updates a `drawing` block's payload (`report.set_drawing_balloon@1`, `RPT.BALLOON.BLOCK`).
    SetDrawingBalloon {
        report: ReportId,
        block: BlockId,
        payload: CanonicalJson,
    },
}

impl Op {
    /// The journal event type.
    pub fn event_type(&self) -> &'static str {
        match self {
            Op::CreateReport { .. } => CREATE_REPORT,
            Op::AddPage { .. } => ADD_PAGE,
            Op::RemovePage { .. } => REMOVE_PAGE,
            Op::AddBlock { .. } => ADD_BLOCK,
            Op::RemoveBlock { .. } => REMOVE_BLOCK,
            Op::BindBlock { .. } => BIND_BLOCK,
            Op::SetTableBlock { .. } => SET_TABLE_BLOCK,
            Op::SetTextBlock { .. } => SET_TEXT_BLOCK,
            Op::SetImageBlock { .. } => SET_IMAGE_BLOCK,
            Op::SetDrawingBalloon { .. } => SET_DRAWING_BALLOON,
        }
    }

    pub fn report(&self) -> ReportId {
        match self {
            Op::CreateReport { report, .. }
            | Op::AddPage { report, .. }
            | Op::RemovePage { report, .. }
            | Op::AddBlock { report, .. }
            | Op::RemoveBlock { report, .. }
            | Op::BindBlock { report, .. }
            | Op::SetTableBlock { report, .. }
            | Op::SetTextBlock { report, .. }
            | Op::SetImageBlock { report, .. }
            | Op::SetDrawingBalloon { report, .. } => *report,
        }
    }

    /// Applies the operation to the report's current state (`None` before creation). Uses
    /// only IDs held in the operation. `kinds` re-checks an added block's payload.
    pub fn apply(&self, state: Option<Report>, kinds: &KindRegistry) -> Result<Report, ReportError> {
        if let Op::CreateReport { report, page, title, .. } = self {
            if state.is_some() {
                return Err(ReportError::IdCollision(report.uuid()));
            }
            return Report::new(*report, *page, title.clone());
        }
        let mut r = state.ok_or(ReportError::NotFound(self.report().uuid()))?;
        match self {
            Op::CreateReport { .. } => {}
            Op::AddPage { after, page, .. } => r.insert_page(after.map(|a| a.uuid()).as_ref(), *page)?,
            Op::RemovePage { page, force, .. } => r.remove_page(&page.uuid(), *force)?,
            Op::AddBlock { page, after, block, kind, kind_version, payload, .. } => {
                check_block(kinds, kind, *kind_version, payload)?;
                let b = Block::new(*block, kind.clone(), *kind_version, payload.clone());
                r.insert_block(&page.uuid(), after.map(|a| a.uuid()).as_ref(), b)?;
            }
            Op::RemoveBlock { block, .. } => r.remove_block(&block.uuid())?,
            Op::BindBlock { block, payload, .. } | Op::SetDrawingBalloon { block, payload, .. } => {
                let b = r.block(&block.uuid()).ok_or(ReportError::NotFound(block.uuid()))?;
                check_block(kinds, b.kind(), b.kind_version(), payload)?;
                r.set_block_payload(&block.uuid(), payload.clone())?;
            }
            Op::SetTableBlock { block, payload, .. } => {
                let b = r.block(&block.uuid()).ok_or(ReportError::NotFound(block.uuid()))?;
                if b.kind().as_str() != "table" {
                    return Err(ReportError::InvalidBlockPayload(super::kind::PayloadError::new(
                        "/kind",
                        "not_table_block",
                    )));
                }
                check_block(kinds, b.kind(), b.kind_version(), payload)?;
                r.set_block_payload(&block.uuid(), payload.clone())?;
            }
            Op::SetTextBlock { block, payload, .. } => {
                let b = r.block(&block.uuid()).ok_or(ReportError::NotFound(block.uuid()))?;
                if !matches!(b.kind().as_str(), "text" | "header" | "footer") {
                    return Err(ReportError::InvalidBlockPayload(super::kind::PayloadError::new(
                        "/kind",
                        "not_text_block",
                    )));
                }
                check_block(kinds, b.kind(), b.kind_version(), payload)?;
                r.set_block_payload(&block.uuid(), payload.clone())?;
            }
            Op::SetImageBlock { block, payload, .. } => {
                let b = r.block(&block.uuid()).ok_or(ReportError::NotFound(block.uuid()))?;
                if b.kind().as_str() != "image" {
                    return Err(ReportError::InvalidBlockPayload(super::kind::PayloadError::new(
                        "/kind",
                        "not_image_block",
                    )));
                }
                check_block(kinds, b.kind(), b.kind_version(), payload)?;
                r.set_block_payload(&block.uuid(), payload.clone())?;
            }
        }
        Ok(r)
    }
}

/// One journal event: the operation and the digest of the stored report after it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportEvent {
    op: Op,
    report_digest: Digest,
}

impl Eq for ReportEvent {}

impl ReportEvent {
    pub fn new(op: Op, report_digest: Digest) -> ReportEvent {
        ReportEvent { op, report_digest }
    }

    pub fn op(&self) -> &Op {
        &self.op
    }

    pub fn report(&self) -> ReportId {
        self.op.report()
    }

    pub fn report_digest(&self) -> Digest {
        self.report_digest
    }

    pub fn event_type(&self) -> &'static str {
        self.op.event_type()
    }

    /// The journal payload (canonical JSON value).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        let mut put = |k: &str, v: CValue| {
            o.insert(k.to_string(), v);
        };
        put("type", CValue::Str(self.op.event_type().to_string()));
        put("report", CValue::Str(self.op.report().uuid().to_string()));
        put("report_digest", CValue::Str(self.report_digest.as_str().to_string()));
        let id = |u: Uuid| CValue::Str(u.to_string());
        let opt = |u: Option<Uuid>| u.map_or(CValue::Null, id);
        match &self.op {
            Op::CreateReport { page, project, title, .. } => {
                put("page", id(page.uuid()));
                put("project", id(*project));
                put("title", title.to_value());
            }
            Op::AddPage { after, page, .. } => {
                put("after", opt(after.map(|a| a.uuid())));
                put("page", id(page.uuid()));
            }
            Op::RemovePage { page, force, .. } => {
                put("page", id(page.uuid()));
                put("force", CValue::Bool(*force));
            }
            Op::AddBlock { page, after, block, kind, kind_version, payload, .. } => {
                put("page", id(page.uuid()));
                put("after", opt(after.map(|a| a.uuid())));
                put("block", id(block.uuid()));
                put("kind", CValue::Str(kind.as_str().to_string()));
                put("kind_version", CValue::U64(u64::from(*kind_version)));
                put("payload", payload_value(payload));
            }
            Op::RemoveBlock { block, .. } => put("block", id(block.uuid())),
            Op::BindBlock { block, payload, .. }
            | Op::SetTableBlock { block, payload, .. }
            | Op::SetTextBlock { block, payload, .. }
            | Op::SetImageBlock { block, payload, .. }
            | Op::SetDrawingBalloon { block, payload, .. } => {
                put("block", id(block.uuid()));
                put("payload", payload_value(payload));
            }
        }
        CValue::Obj(o)
    }

    /// Reads an event from JSON bytes: bounded parse (duplicate keys refused), then
    /// [`ReportEvent::from_value`]. Any failure is `report.store_invalid`.
    pub fn from_json(bytes: &[u8]) -> Result<ReportEvent, ReportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_REPORT_BYTES, fid_canon::MAX_DEPTH)
            .map_err(|_| ReportError::StoreInvalid("event is not json"))?;
        ReportEvent::from_value(&v)
    }

    /// Strict reader of [`ReportEvent::to_value`] (`report.store_invalid` on any deviation).
    pub fn from_value(v: &CValue) -> Result<ReportEvent, ReportError> {
        let CValue::Obj(raw) = v else { return Err(ReportError::StoreInvalid("event is not an object")) };
        let ty = match raw.get("type") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(ReportError::StoreInvalid("event type")),
        };
        let keys: &[&str] = match ty {
            CREATE_REPORT => &["type", "report", "page", "project", "title", "report_digest"],
            ADD_PAGE => &["type", "report", "after", "page", "report_digest"],
            REMOVE_PAGE => &["type", "report", "page", "force", "report_digest"],
            ADD_BLOCK => {
                &["type", "report", "page", "after", "block", "kind", "kind_version", "payload", "report_digest"]
            }
            REMOVE_BLOCK => &["type", "report", "block", "report_digest"],
            BIND_BLOCK | SET_TABLE_BLOCK | SET_TEXT_BLOCK | SET_IMAGE_BLOCK | SET_DRAWING_BALLOON => {
                &["type", "report", "block", "payload", "report_digest"]
            }
            _ => return Err(ReportError::StoreInvalid("unknown event type")),
        };
        let o = exact_obj(v, keys)?;
        let report = ReportId::from_value(field(o, "report")?)?;
        let report_digest = match field(o, "report_digest")? {
            CValue::Str(s) => Digest::parse(s).map_err(|_| ReportError::StoreInvalid("report_digest"))?,
            _ => return Err(ReportError::StoreInvalid("report_digest")),
        };
        let page = || PageId::from_value(field(o, "page")?);
        let block = || BlockId::from_value(field(o, "block")?);
        let op = match ty {
            CREATE_REPORT => {
                let project = match field(o, "project")? {
                    CValue::Str(s) => parse_canonical_uuid(s).ok_or(ReportError::StoreInvalid("project id"))?,
                    _ => return Err(ReportError::StoreInvalid("project id")),
                };
                Op::CreateReport { report, page: page()?, project, title: Untrusted::from_value(field(o, "title")?)? }
            }
            ADD_PAGE => Op::AddPage { report, after: opt_id(field(o, "after")?, PageId::from_value)?, page: page()? },
            REMOVE_PAGE => {
                let force = match field(o, "force")? {
                    CValue::Bool(b) => *b,
                    _ => return Err(ReportError::StoreInvalid("force")),
                };
                Op::RemovePage { report, page: page()?, force }
            }
            ADD_BLOCK => {
                let kind = match field(o, "kind")? {
                    CValue::Str(s) => BlockKindId::new(s).ok_or(ReportError::StoreInvalid("kind id"))?,
                    _ => return Err(ReportError::StoreInvalid("kind id")),
                };
                Op::AddBlock {
                    report,
                    page: page()?,
                    after: opt_id(field(o, "after")?, BlockId::from_value)?,
                    block: block()?,
                    kind,
                    kind_version: u32_value(field(o, "kind_version")?)?,
                    payload: payload_from_value(field(o, "payload")?)?,
                }
            }
            BIND_BLOCK => Op::BindBlock { report, block: block()?, payload: payload_from_value(field(o, "payload")?)? },
            SET_TABLE_BLOCK => {
                Op::SetTableBlock { report, block: block()?, payload: payload_from_value(field(o, "payload")?)? }
            }
            SET_TEXT_BLOCK => {
                Op::SetTextBlock { report, block: block()?, payload: payload_from_value(field(o, "payload")?)? }
            }
            SET_IMAGE_BLOCK => {
                Op::SetImageBlock { report, block: block()?, payload: payload_from_value(field(o, "payload")?)? }
            }
            SET_DRAWING_BALLOON => {
                Op::SetDrawingBalloon { report, block: block()?, payload: payload_from_value(field(o, "payload")?)? }
            }
            _ => Op::RemoveBlock { report, block: block()? },
        };
        Ok(ReportEvent { op, report_digest })
    }
}

fn opt_id<T>(v: &CValue, read: fn(&CValue) -> Result<T, ReportError>) -> Result<Option<T>, ReportError> {
    match v {
        CValue::Null => Ok(None),
        other => read(other).map(Some),
    }
}
