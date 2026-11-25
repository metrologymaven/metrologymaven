//! The two page-setup commands: parameters, handlers, results and the `--json` dispatcher
//! (`RPT.DESIGN.PAGE`, `Design: Registry commands`).
//!
//! `report.set_page_layout@1` checks everything that can fail before it commits: lengths
//! (REQ-003, REQ-004), the report and page, the custom-size range (REQ-002) and the body
//! area (REQ-005); then it commits the new page setup with one event (REQ-007). A failed
//! command commits nothing and journals nothing. Parameters the caller leaves out
//! (`orientation`, `margins`, `header`, `footer`) keep the value of the layout in effect for
//! the target: the page's effective layout for a page, the report default otherwise.

use super::backend::LayoutBackend;
use super::error::LayoutError;
use super::event::{LayoutEvent, SET_PAGE_LAYOUT, SHOW_PAGE_LAYOUT};
use super::length::{parse_length, Length};
use super::model::{LayoutSource, Margins, Orientation, PageLayout, PageSize, ReportLayout};
use crate::doc::model::{obj, parse_canonical_uuid};
use crate::doc::{KindRegistry, PageId, Report, ReportId};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;

type Params = BTreeMap<String, CValue>;

/// The `size` parameter: a preset, or `custom` with `width` and `height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeArg {
    Preset(PageSize),
    Custom,
}

/// `report.set_page_layout@1` parameters: `{report, page?, size, width?, height?,
/// orientation?, margins?, header?, footer?}`. Lengths are text with a unit (`15mm`,
/// `8.5in`), checked by the handler; `margins` is `top,right,bottom,left`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPageLayout {
    pub report: Uuid,
    /// `None` sets the report default; a page ID sets that page's override (REQ-006).
    pub page: Option<Uuid>,
    pub size: SizeArg,
    pub width: Option<String>,
    pub height: Option<String>,
    pub orientation: Option<Orientation>,
    pub margins: Option<String>,
    pub header: Option<String>,
    pub footer: Option<String>,
}

fn params<'a>(v: &'a CValue, allowed: &[&str]) -> Result<&'a Params, LayoutError> {
    let CValue::Obj(m) = v else { return Err(LayoutError::BadParams("params is not an object")) };
    if m.keys().any(|k| !allowed.contains(&k.as_str())) {
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

impl SetPageLayout {
    pub fn from_value(v: &CValue) -> Result<SetPageLayout, LayoutError> {
        let m =
            params(v, &["footer", "header", "height", "margins", "orientation", "page", "report", "size", "width"])?;
        let size = match opt_str(m, "size")?.as_deref() {
            Some("custom") => SizeArg::Custom,
            Some(name) => SizeArg::Preset(PageSize::preset(name).ok_or(LayoutError::BadParams("size"))?),
            None => return Err(LayoutError::BadParams("size")),
        };
        let orientation = match opt_str(m, "orientation")? {
            None => None,
            Some(o) => Some(Orientation::parse(&o).ok_or(LayoutError::BadParams("orientation"))?),
        };
        Ok(SetPageLayout {
            report: id(m, "report")?.ok_or(LayoutError::BadParams("report"))?,
            page: id(m, "page")?,
            size,
            width: opt_str(m, "width")?,
            height: opt_str(m, "height")?,
            orientation,
            margins: opt_str(m, "margins")?,
            header: opt_str(m, "header")?,
            footer: opt_str(m, "footer")?,
        })
    }
}

/// `report.show_page_layout@1` parameters: `{report}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowPageLayout {
    pub report: Uuid,
}

impl ShowPageLayout {
    pub fn from_value(v: &CValue) -> Result<ShowPageLayout, LayoutError> {
        let m = params(v, &["report"])?;
        Ok(ShowPageLayout { report: id(m, "report")?.ok_or(LayoutError::BadParams("report"))? })
    }
}

// ------------------------------------------------------------------ helpers

fn field_length(field: &'static str, text: &str) -> Result<Length, LayoutError> {
    parse_length(text).map_err(|f| LayoutError::LayoutInvalid { field, reason: f.reason() })
}

fn opt_length(field: &'static str, text: Option<&str>) -> Result<Option<Length>, LayoutError> {
    text.map(|t| field_length(field, t)).transpose()
}

/// `top,right,bottom,left`, exactly four lengths.
fn parse_margins(text: &str) -> Result<Margins, LayoutError> {
    let parts: Vec<&str> = text.split(',').collect();
    let [top, right, bottom, left] = parts.as_slice() else {
        return Err(LayoutError::LayoutInvalid { field: "margins", reason: "need_four_lengths" });
    };
    Ok(Margins {
        top: field_length("margin_top", top)?,
        right: field_length("margin_right", right)?,
        bottom: field_length("margin_bottom", bottom)?,
        left: field_length("margin_left", left)?,
    })
}

/// The page size of the parameters (REQ-001, REQ-002).
fn page_size(p: &SetPageLayout) -> Result<PageSize, LayoutError> {
    match p.size {
        SizeArg::Preset(size) => {
            for (field, given) in [("width", &p.width), ("height", &p.height)] {
                if given.is_some() {
                    return Err(LayoutError::LayoutInvalid { field, reason: "only_for_custom" });
                }
            }
            Ok(size)
        }
        SizeArg::Custom => {
            let need = |field: &'static str, t: &Option<String>| match t {
                Some(t) => field_length(field, t),
                None => Err(LayoutError::LayoutInvalid { field, reason: "required_for_custom" }),
            };
            Ok(PageSize::Custom { width: need("width", &p.width)?, height: need("height", &p.height)? })
        }
    }
}

/// Loads a report: `report.not_found` unless it is stored.
fn load_report(backend: &dyn LayoutBackend, kinds: &KindRegistry, id: &Uuid) -> Result<Report, LayoutError> {
    let rid = ReportId::new(*id).ok_or(LayoutError::NotFound(*id))?;
    let stored = backend.load(&rid)?.ok_or(LayoutError::NotFound(*id))?;
    Ok(stored.open(&rid, kinds)?)
}

/// The live page `id` of `report`.
pub(super) fn live_page(report: &Report, id: &Uuid) -> Option<PageId> {
    report.pages().iter().map(|p| p.id()).find(|p| p.uuid() == *id)
}

// ------------------------------------------------------------------ handlers

/// The outcome of a successful `report.set_page_layout@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetOutcome {
    pub report: ReportId,
    pub page: Option<PageId>,
    pub layout: PageLayout,
}

impl SetOutcome {
    /// Result: `{report, page, ...}` plus the effective layout of [`PageLayout::effective_map`].
    pub fn to_value(&self) -> CValue {
        let mut m = self.layout.effective_map();
        m.insert("page".to_string(), self.page.map_or(CValue::Null, |p| CValue::Str(p.uuid().to_string())));
        m.insert("report".to_string(), CValue::Str(self.report.uuid().to_string()));
        CValue::Obj(m)
    }
}

/// `report.set_page_layout@1` (W2): sets the report default, or with `page` that page's
/// override (REQ-006), and journals one event (REQ-007).
pub fn set_page_layout(
    backend: &mut dyn LayoutBackend,
    kinds: &KindRegistry,
    p: &SetPageLayout,
) -> Result<SetOutcome, LayoutError> {
    let size = page_size(p)?;
    let margins = p.margins.as_deref().map(parse_margins).transpose()?;
    let header = opt_length("header", p.header.as_deref())?;
    let footer = opt_length("footer", p.footer.as_deref())?;

    let report = load_report(backend, kinds, &p.report)?;
    let page = match &p.page {
        None => None,
        Some(id) => Some(live_page(&report, id).ok_or(LayoutError::NotFound(*id))?),
    };
    let current = backend.load_layout(&report.id())?.unwrap_or_default();
    let base = match &page {
        None => current.default_layout,
        Some(pg) => current.effective(pg).0,
    };
    let layout = PageLayout {
        size,
        orientation: p.orientation.unwrap_or(base.orientation),
        margins: margins.unwrap_or(base.margins),
        header: header.unwrap_or(base.header),
        footer: footer.unwrap_or(base.footer),
    };
    layout.check()?;

    let next = with_layout(&current, page, layout);
    let event = LayoutEvent { report: report.id(), page, layout, layout_digest: next.digest()? };
    backend.commit_layout(&report.id(), next, event)?;
    Ok(SetOutcome { report: report.id(), page, layout })
}

/// `current` with the default (`page` `None`) or `page`'s override replaced by `layout`.
pub(super) fn with_layout(current: &ReportLayout, page: Option<PageId>, layout: PageLayout) -> ReportLayout {
    let mut next = current.clone();
    match page {
        None => next.default_layout = layout,
        Some(pg) => {
            next.overrides.insert(pg, layout);
        }
    }
    next
}

/// One page of `report.show_page_layout@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePageLayout {
    pub page: PageId,
    pub source: LayoutSource,
    pub layout: PageLayout,
}

impl EffectivePageLayout {
    /// `{page_id, source, ...}` plus the effective layout of [`PageLayout::effective_map`].
    pub fn to_value(&self) -> CValue {
        let mut m = self.layout.effective_map();
        m.insert("page_id".to_string(), CValue::Str(self.page.uuid().to_string()));
        m.insert("source".to_string(), CValue::Str(self.source.name().to_string()));
        CValue::Obj(m)
    }
}

/// `report.show_page_layout@1` (R0): the effective layout of every live page, in page
/// order (REQ-008). Journals nothing.
pub fn show_page_layout(
    backend: &dyn LayoutBackend,
    kinds: &KindRegistry,
    p: &ShowPageLayout,
) -> Result<Vec<EffectivePageLayout>, LayoutError> {
    let report = load_report(backend, kinds, &p.report)?;
    let setup = backend.load_layout(&report.id())?.unwrap_or_default();
    Ok(report
        .pages()
        .iter()
        .map(|pg| {
            let (layout, source) = setup.effective(&pg.id());
            EffectivePageLayout { page: pg.id(), source, layout }
        })
        .collect())
}

/// The `report.show_page_layout@1` result: `{report, pages: [...]}`.
pub fn show_value(report: &Uuid, pages: &[EffectivePageLayout]) -> CValue {
    obj([
        ("pages", CValue::Arr(pages.iter().map(EffectivePageLayout::to_value).collect())),
        ("report", CValue::Str(report.to_string())),
    ])
}

/// Runs one page-setup command by its ID with `--json` parameters and returns its `--json`
/// result. An unknown command is `report.bad_params`.
pub fn execute(
    command: &str,
    params: &CValue,
    backend: &mut dyn LayoutBackend,
    kinds: &KindRegistry,
) -> Result<CValue, LayoutError> {
    match command {
        SET_PAGE_LAYOUT => Ok(set_page_layout(backend, kinds, &SetPageLayout::from_value(params)?)?.to_value()),
        SHOW_PAGE_LAYOUT => {
            let p = ShowPageLayout::from_value(params)?;
            Ok(show_value(&p.report, &show_page_layout(backend, kinds, &p)?))
        }
        _ => Err(LayoutError::BadParams("unknown command")),
    }
}
