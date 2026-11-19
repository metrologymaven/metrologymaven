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
//! report.add_block@1      {type, report, page, after, block, kind, kind_version, labels, payload, report_digest}
//! report.remove_block@1   {type, report, block, report_digest}
//! ```

use super::error::ReportError;
use super::kind::KindRegistry;
use super::model::{
    check_block, exact_obj, field, parse_canonical_uuid, payload_from_value, payload_value, u32_value, Block, BlockId,
    BlockKindId, CanonicalJson, PageId, Report, ReportId, MAX_REPORT_BYTES,
};
use super::text::{Text, Untrusted};
use super::value::{labels_from_value, labels_value, ValueLabel};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

pub const CREATE_REPORT: &str = "report.create_report@1";
pub const ADD_PAGE: &str = "report.add_page@1";
pub const REMOVE_PAGE: &str = "report.remove_page@1";
pub const ADD_BLOCK: &str = "report.add_block@1";
pub const REMOVE_BLOCK: &str = "report.remove_block@1";
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
        labels: BTreeSet<ValueLabel>,
        payload: CanonicalJson,
    },
    RemoveBlock {
        report: ReportId,
        block: BlockId,
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
        }
    }

    pub fn report(&self) -> ReportId {
        match self {
            Op::CreateReport { report, .. }
            | Op::AddPage { report, .. }
            | Op::RemovePage { report, .. }
            | Op::AddBlock { report, .. }
            | Op::RemoveBlock { report, .. } => *report,
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
            Op::AddBlock { page, after, block, kind, kind_version, labels, payload, .. } => {
                check_block(kinds, kind, *kind_version, payload, labels)?;
                let b = Block::new(*block, kind.clone(), *kind_version, labels.clone(), payload.clone());
                r.insert_block(&page.uuid(), after.map(|a| a.uuid()).as_ref(), b)?;
            }
            Op::RemoveBlock { block, .. } => r.remove_block(&block.uuid())?,
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
            Op::AddBlock { page, after, block, kind, kind_version, labels, payload, .. } => {
                put("labels", labels_value(labels));
                put("page", id(page.uuid()));
                put("after", opt(after.map(|a| a.uuid())));
                put("block", id(block.uuid()));
                put("kind", CValue::Str(kind.as_str().to_string()));
                put("kind_version", CValue::U64(u64::from(*kind_version)));
                put("payload", payload_value(payload));
            }
            Op::RemoveBlock { block, .. } => put("block", id(block.uuid())),
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
            ADD_BLOCK => &[
                "type",
                "report",
                "page",
                "after",
                "block",
                "kind",
                "kind_version",
                "labels",
                "payload",
                "report_digest",
            ],
            REMOVE_BLOCK => &["type", "report", "block", "report_digest"],
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
                    labels: labels_from_value(field(o, "labels")?).ok_or(ReportError::StoreInvalid("labels"))?,
                    payload: payload_from_value(field(o, "payload")?)?,
                }
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
