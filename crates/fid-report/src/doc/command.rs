//! The six report commands: parameters, handlers, results and the `--json` dispatcher.
//!
//! Every handler checks everything that can fail before it draws an ID, builds one
//! [`Op`], applies it to a copy of the stored report and hands the new stored form and the
//! event to the backend in one commit. A failed command commits nothing, so it journals
//! nothing (REQ-005..007, REQ-010).

use super::backend::{ReportBackend, StoredReport};
use super::error::ReportError;
use super::event::{Op, ReportEvent, ADD_BLOCK, ADD_PAGE, CREATE_REPORT, REMOVE_BLOCK, REMOVE_PAGE, SHOW_REPORT};
use super::kind::{KindRegistry, PayloadError};
use super::model::{is_v7, obj, parse_canonical_uuid, BlockId, BlockKindId, CanonicalJson, PageId, Report, ReportId};
use super::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_project::{uuid_v7, ObservationSource, Uuid};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ params

/// `report.create_report@1` parameters: `{title: string, project: uuid}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateReport {
    pub title: Untrusted<Text>,
    pub project: Uuid,
}

/// `report.add_page@1` parameters: `{report, after?: page | null}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddPage {
    pub report: Uuid,
    pub after: Option<Uuid>,
}

/// `report.remove_page@1` parameters: `{report, page, force?: bool = false}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovePage {
    pub report: Uuid,
    pub page: Uuid,
    pub force: bool,
}

/// `report.add_block@1` parameters: `{report, page, after?: block | null, kind: string,
/// payload: json}`. A kind string that is not a well-formed kind ID is kept as `None` and
/// fails as `report.unknown_block_kind`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddBlock {
    pub report: Uuid,
    pub page: Uuid,
    pub after: Option<Uuid>,
    pub kind: Option<BlockKindId>,
    pub payload: CValue,
}

/// `report.remove_block@1` parameters: `{report, block}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveBlock {
    pub report: Uuid,
    pub block: Uuid,
}

/// `report.show_report@1` parameters: `{report}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowReport {
    pub report: Uuid,
}

fn params<'a>(v: &'a CValue, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, ReportError> {
    let CValue::Obj(o) = v else { return Err(ReportError::BadParams("params is not an object")) };
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(ReportError::BadParams("unknown parameter"));
    }
    Ok(o)
}

fn id_param(o: &BTreeMap<String, CValue>, key: &'static str) -> Result<Uuid, ReportError> {
    match o.get(key) {
        Some(CValue::Str(s)) => parse_canonical_uuid(s).ok_or(ReportError::BadParams("id is not a canonical uuid")),
        Some(_) => Err(ReportError::BadParams("id is not a string")),
        None => Err(ReportError::BadParams("missing id parameter")),
    }
}

fn opt_id_param(o: &BTreeMap<String, CValue>, key: &'static str) -> Result<Option<Uuid>, ReportError> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(_) => id_param(o, key).map(Some),
    }
}

impl CreateReport {
    pub fn from_value(v: &CValue) -> Result<CreateReport, ReportError> {
        let o = params(v, &["title", "project"])?;
        let title = match o.get("title") {
            Some(CValue::Str(s)) => Untrusted::text(s)?,
            Some(_) => return Err(ReportError::BadParams("title is not a string")),
            None => return Err(ReportError::BadParams("missing title")),
        };
        Ok(CreateReport { title, project: id_param(o, "project")? })
    }
}

impl AddPage {
    pub fn from_value(v: &CValue) -> Result<AddPage, ReportError> {
        let o = params(v, &["report", "after"])?;
        Ok(AddPage { report: id_param(o, "report")?, after: opt_id_param(o, "after")? })
    }
}

impl RemovePage {
    pub fn from_value(v: &CValue) -> Result<RemovePage, ReportError> {
        let o = params(v, &["report", "page", "force"])?;
        let force = match o.get("force") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(ReportError::BadParams("force is not a boolean")),
        };
        Ok(RemovePage { report: id_param(o, "report")?, page: id_param(o, "page")?, force })
    }
}

impl AddBlock {
    pub fn from_value(v: &CValue) -> Result<AddBlock, ReportError> {
        let o = params(v, &["report", "page", "after", "kind", "payload"])?;
        let kind = match o.get("kind") {
            Some(CValue::Str(s)) => BlockKindId::new(s),
            Some(_) => return Err(ReportError::BadParams("kind is not a string")),
            None => return Err(ReportError::BadParams("missing kind")),
        };
        let payload = o.get("payload").cloned().ok_or(ReportError::BadParams("missing payload"))?;
        Ok(AddBlock {
            report: id_param(o, "report")?,
            page: id_param(o, "page")?,
            after: opt_id_param(o, "after")?,
            kind,
            payload,
        })
    }
}

impl RemoveBlock {
    pub fn from_value(v: &CValue) -> Result<RemoveBlock, ReportError> {
        let o = params(v, &["report", "block"])?;
        Ok(RemoveBlock { report: id_param(o, "report")?, block: id_param(o, "block")? })
    }
}

impl ShowReport {
    pub fn from_value(v: &CValue) -> Result<ShowReport, ReportError> {
        let o = params(v, &["report"])?;
        Ok(ShowReport { report: id_param(o, "report")? })
    }
}

// ------------------------------------------------------------------ helpers

/// A fresh UUIDv7 from the observation source (recorded in live mode).
fn draw(ids: &mut dyn ObservationSource) -> Result<Uuid, ReportError> {
    let (ms, bits) = ids.draw_v7().map_err(|e| ReportError::IdUnavailable(e.code()))?;
    let id = uuid_v7(ms, &bits).map_err(|e| ReportError::IdUnavailable(e.code()))?;
    if !is_v7(&id) || id == Uuid::NIL || id == Uuid::MAX {
        return Err(ReportError::IdUnavailable("bad_id"));
    }
    Ok(id)
}

/// Loads a report named in params: `report.not_found` unless it is stored.
fn load(backend: &dyn ReportBackend, id: &Uuid) -> Result<Report, ReportError> {
    let rid = ReportId::new(*id).ok_or(ReportError::NotFound(*id))?;
    let stored = backend.load(&rid)?.ok_or(ReportError::NotFound(*id))?;
    stored.open(&rid)
}

/// Applies `op` and commits the new stored report with its event.
fn commit(
    backend: &mut dyn ReportBackend,
    kinds: &KindRegistry,
    state: Option<Report>,
    op: Op,
) -> Result<Report, ReportError> {
    let report = op.apply(state, kinds)?;
    let stored = StoredReport::of(&report)?;
    let event = ReportEvent::new(op, stored.digest);
    backend.commit(&report.id(), stored, event)?;
    Ok(report)
}

fn page_id(id: &Uuid) -> Result<PageId, ReportError> {
    PageId::new(*id).ok_or(ReportError::NotFound(*id))
}

fn block_id(id: &Uuid) -> Result<BlockId, ReportError> {
    BlockId::new(*id).ok_or(ReportError::NotFound(*id))
}

// ------------------------------------------------------------------ handlers

/// `report.create_report@1` (REQ-001): a new report with one empty page. Result: the
/// report summary.
pub fn create_report(
    backend: &mut dyn ReportBackend,
    kinds: &KindRegistry,
    ids: &mut dyn ObservationSource,
    p: &CreateReport,
) -> Result<Report, ReportError> {
    if p.project != backend.project() {
        return Err(ReportError::NotFound(p.project));
    }
    let report = ReportId::new(draw(ids)?).ok_or(ReportError::IdUnavailable("bad_id"))?;
    if backend.load(&report)?.is_some() || report.uuid() == p.project {
        return Err(ReportError::IdCollision(report.uuid()));
    }
    let page = PageId::new(draw(ids)?).ok_or(ReportError::IdUnavailable("bad_id"))?;
    if page.uuid() == p.project {
        return Err(ReportError::IdCollision(page.uuid()));
    }
    commit(backend, kinds, None, Op::CreateReport { report, page, project: p.project, title: p.title.clone() })
}

/// `report.add_page@1` (REQ-003): returns the new page ID.
pub fn add_page(
    backend: &mut dyn ReportBackend,
    kinds: &KindRegistry,
    ids: &mut dyn ObservationSource,
    p: &AddPage,
) -> Result<PageId, ReportError> {
    let r = load(backend, &p.report)?;
    let after = match &p.after {
        None => None,
        Some(a) if r.has_page(a) => Some(page_id(a)?),
        Some(a) => return Err(ReportError::NotFound(*a)),
    };
    let page = PageId::new(draw(ids)?).ok_or(ReportError::IdUnavailable("bad_id"))?;
    commit(backend, kinds, Some(r), Op::AddPage { report: report_id(&p.report)?, after, page })?;
    Ok(page)
}

/// `report.remove_page@1` (REQ-004, REQ-005).
pub fn remove_page(backend: &mut dyn ReportBackend, kinds: &KindRegistry, p: &RemovePage) -> Result<(), ReportError> {
    let r = load(backend, &p.report)?;
    if !r.has_page(&p.page) {
        return Err(ReportError::NotFound(p.page));
    }
    let op = Op::RemovePage { report: report_id(&p.report)?, page: page_id(&p.page)?, force: p.force };
    commit(backend, kinds, Some(r), op).map(|_| ())
}

/// `report.add_block@1` (REQ-002, REQ-007): returns the new block ID. Checks, in order:
/// report, page, `after` on that page, kind, payload bounds, payload check.
pub fn add_block(
    backend: &mut dyn ReportBackend,
    kinds: &KindRegistry,
    ids: &mut dyn ObservationSource,
    p: &AddBlock,
) -> Result<BlockId, ReportError> {
    let r = load(backend, &p.report)?;
    if !r.has_page(&p.page) {
        return Err(ReportError::NotFound(p.page));
    }
    let after = match &p.after {
        None => None,
        Some(a) if r.page_has_block(&p.page, a) => Some(block_id(a)?),
        Some(a) => return Err(ReportError::NotFound(*a)),
    };
    let kind = p.kind.clone().ok_or(ReportError::UnknownBlockKind(None))?;
    let (kind_version, k) = kinds.get(&kind).ok_or_else(|| ReportError::UnknownBlockKind(Some(kind.clone())))?;
    let payload = CanonicalJson::new(&p.payload)
        .ok_or_else(|| ReportError::InvalidBlockPayload(PayloadError::new("", "payload_bounds")))?;
    k.validate(&payload).map_err(ReportError::InvalidBlockPayload)?;
    let block = BlockId::new(draw(ids)?).ok_or(ReportError::IdUnavailable("bad_id"))?;
    let op = Op::AddBlock {
        report: report_id(&p.report)?,
        page: page_id(&p.page)?,
        after,
        block,
        kind,
        kind_version,
        payload,
    };
    commit(backend, kinds, Some(r), op)?;
    Ok(block)
}

/// `report.remove_block@1` (REQ-004).
pub fn remove_block(backend: &mut dyn ReportBackend, kinds: &KindRegistry, p: &RemoveBlock) -> Result<(), ReportError> {
    let r = load(backend, &p.report)?;
    let block = BlockId::new(p.block).filter(|b| r.pages().iter().any(|pg| pg.blocks().iter().any(|x| x.id() == *b)));
    let block = block.ok_or(ReportError::NotFound(p.block))?;
    commit(backend, kinds, Some(r), Op::RemoveBlock { report: report_id(&p.report)?, block }).map(|_| ())
}

/// `report.show_report@1` (R0): the stored report.
pub fn show_report(backend: &dyn ReportBackend, p: &ShowReport) -> Result<Report, ReportError> {
    load(backend, &p.report)
}

fn report_id(id: &Uuid) -> Result<ReportId, ReportError> {
    ReportId::new(*id).ok_or(ReportError::NotFound(*id))
}

// ------------------------------------------------------------------ results

/// The `report.create_report@1` result:
/// `{"report_id", "title": {text, untrusted}, "pages": [{"page_id", "block_count"}],
/// "report_digest"}` (`block_count` a decimal string).
pub fn summary_value(r: &Report) -> Result<CValue, ReportError> {
    let stored = StoredReport::of(r)?;
    let pages = r
        .pages()
        .iter()
        .map(|p| {
            obj([
                ("block_count", CValue::U64(p.blocks().len() as u64)),
                ("page_id", CValue::Str(p.id().uuid().to_string())),
            ])
        })
        .collect();
    Ok(obj([
        ("pages", CValue::Arr(pages)),
        ("report_digest", CValue::Str(stored.digest.as_str().to_string())),
        ("report_id", CValue::Str(r.id().uuid().to_string())),
        ("title", r.title().to_value()),
    ]))
}

/// Runs one report command from its ID and `--json` parameters and returns its `--json`
/// result: the summary (`create_report`), `{"page_id"}`, `{"block_id"}`, `{}` (removals)
/// or the report itself (`show_report`). An unknown command is `report.bad_params`.
pub fn execute(
    command: &str,
    params: &CValue,
    backend: &mut dyn ReportBackend,
    kinds: &KindRegistry,
    ids: &mut dyn ObservationSource,
) -> Result<CValue, ReportError> {
    let id = |k: &str, u: Uuid| obj([(k, CValue::Str(u.to_string()))]);
    match command {
        CREATE_REPORT => summary_value(&create_report(backend, kinds, ids, &CreateReport::from_value(params)?)?),
        ADD_PAGE => Ok(id("page_id", add_page(backend, kinds, ids, &AddPage::from_value(params)?)?.uuid())),
        REMOVE_PAGE => remove_page(backend, kinds, &RemovePage::from_value(params)?).map(|()| obj([])),
        ADD_BLOCK => Ok(id("block_id", add_block(backend, kinds, ids, &AddBlock::from_value(params)?)?.uuid())),
        REMOVE_BLOCK => remove_block(backend, kinds, &RemoveBlock::from_value(params)?).map(|()| obj([])),
        SHOW_REPORT => show_report(backend, &ShowReport::from_value(params)?).map(|r| r.to_value()),
        _ => Err(ReportError::BadParams("unknown command")),
    }
}
