//! The journal event of page setup (`RPT.DESIGN.PAGE` REQ-007): one
//! `report.set_page_layout@1` event per successful command.
//!
//! ```text
//! report.set_page_layout@1  {type, report, page, layout, layout_digest}
//! ```
//!
//! `page` is `null` for the report default. `layout` is the full resolved layout (fields the
//! caller left out already filled from the layout in effect), so replay never re-derives
//! it; `layout_digest` is the SHA-256 of the report's whole page setup after the command,
//! which exact replay compares. The `type` key is the one `RPT.MODEL.DOC` events use, so
//! both kinds of events share one journal.

use super::error::LayoutError;
use super::model::PageLayout;
use crate::doc::model::obj;
use crate::doc::{PageId, ReportId};
use fid_canon::CValue;
use fid_types::Digest;

/// Command and event ID: set the report default or one page's override (`W2`).
pub const SET_PAGE_LAYOUT: &str = "report.set_page_layout@1";
/// Command ID: the effective layout of every page (`R0`, not journaled).
pub const SHOW_PAGE_LAYOUT: &str = "report.show_page_layout@1";

/// One `report.set_page_layout@1` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutEvent {
    pub report: ReportId,
    /// The page whose override was set; `None` for the report default (REQ-006).
    pub page: Option<PageId>,
    pub layout: PageLayout,
    /// Digest of the report's page setup after the command ([`super::ReportLayout::digest`]).
    pub layout_digest: Digest,
}

impl LayoutEvent {
    /// Canonical journal form; keys sorted, integers as decimal strings.
    pub fn to_value(&self) -> CValue {
        obj([
            ("layout", self.layout.to_value()),
            ("layout_digest", CValue::Str(self.layout_digest.as_str().to_string())),
            ("page", self.page.map_or(CValue::Null, |p| CValue::Str(p.uuid().to_string()))),
            ("report", CValue::Str(self.report.uuid().to_string())),
            ("type", CValue::Str(SET_PAGE_LAYOUT.to_string())),
        ])
    }

    /// Reads an event back: exactly its keys, canonical UUIDv7 IDs, a valid stored layout.
    /// Anything else is `report.store_invalid`.
    pub fn from_value(v: &CValue) -> Result<LayoutEvent, LayoutError> {
        let bad = || LayoutError::StoreInvalid("page layout event");
        let CValue::Obj(m) = v else { return Err(bad()) };
        let keys = ["layout", "layout_digest", "page", "report", "type"];
        if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
            return Err(bad());
        }
        if m.get("type") != Some(&CValue::Str(SET_PAGE_LAYOUT.to_string())) {
            return Err(bad());
        }
        let report = m.get("report").map(ReportId::from_value).transpose().ok().flatten().ok_or_else(bad)?;
        let page = match m.get("page") {
            Some(CValue::Null) => None,
            Some(p) => Some(PageId::from_value(p).map_err(|_| bad())?),
            None => return Err(bad()),
        };
        let layout_digest = match m.get("layout_digest") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| bad())?,
            _ => return Err(bad()),
        };
        let layout = PageLayout::from_value(m.get("layout").unwrap_or(&CValue::Null))?;
        Ok(LayoutEvent { report, page, layout, layout_digest })
    }
}

/// True when `v` is a page-setup event (its `type` is [`SET_PAGE_LAYOUT`]).
pub fn is_layout_event(v: &CValue) -> bool {
    matches!(v, CValue::Obj(m) if matches!(m.get("type"), Some(CValue::Str(t)) if t == SET_PAGE_LAYOUT))
}
