//! Data model of page setup (`RPT.DESIGN.PAGE`, `Design: Data structures`).
//!
//! Integer geometry only: every length is a [`Length`] in `0.01 mm`, no float type or
//! operation (RPT float rule). Wire and stored forms write integers as decimal strings
//! (REQ-RPT.MODEL.BLOCKS-012).

use super::error::LayoutError;
use super::length::{Length, MAX_LENGTH};
use crate::doc::model::obj;
use crate::doc::PageId;
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Smallest custom page side: `50 mm` (REQ-002).
pub const MIN_CUSTOM_SIDE: Length = 5_000;
/// Largest custom page side: `2000 mm` (REQ-002).
pub const MAX_CUSTOM_SIDE: Length = 200_000;
/// Smallest body width and height: `20 mm` (REQ-005).
pub const MIN_BODY: Length = 2_000;

/// A page size: a preset (REQ-001) or a custom size (REQ-002). The dimensions are those of
/// the portrait form for a preset, and as given for a custom size; [`Orientation::Landscape`]
/// swaps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    /// ISO A4, `210 x 297 mm`.
    A4,
    /// ISO A3, `297 x 420 mm`.
    A3,
    /// US Letter, `215.9 x 279.4 mm`.
    Letter,
    /// A custom size, each side `50 mm` to `2000 mm`.
    Custom { width: Length, height: Length },
}

impl PageSize {
    /// Wire name: `A4`, `A3`, `Letter` or `custom`.
    pub const fn name(self) -> &'static str {
        match self {
            PageSize::A4 => "A4",
            PageSize::A3 => "A3",
            PageSize::Letter => "Letter",
            PageSize::Custom { .. } => "custom",
        }
    }

    /// The preset of a wire name; `None` for `custom` and anything else.
    pub fn preset(name: &str) -> Option<PageSize> {
        match name {
            "A4" => Some(PageSize::A4),
            "A3" => Some(PageSize::A3),
            "Letter" => Some(PageSize::Letter),
            _ => None,
        }
    }

    /// `(width, height)` before orientation, in `0.01 mm`.
    pub const fn dims(self) -> (Length, Length) {
        match self {
            PageSize::A4 => (21_000, 29_700),
            PageSize::A3 => (29_700, 42_000),
            PageSize::Letter => (21_590, 27_940),
            PageSize::Custom { width, height } => (width, height),
        }
    }
}

/// Page orientation; landscape swaps the width and height of the size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub const fn name(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }

    pub fn parse(name: &str) -> Option<Orientation> {
        match name {
            "portrait" => Some(Orientation::Portrait),
            "landscape" => Some(Orientation::Landscape),
            _ => None,
        }
    }
}

/// The four page margins, in `0.01 mm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margins {
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
}

/// An axis-aligned rectangle on the page, in `0.01 mm` from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: Length,
    pub y: Length,
    pub width: Length,
    pub height: Length,
}

impl Rect {
    pub fn to_value(self) -> CValue {
        obj([("height", int(self.height)), ("width", int(self.width)), ("x", int(self.x)), ("y", int(self.y))])
    }
}

/// One page format (`Design: Data structures`, `PageLayout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLayout {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margins: Margins,
    /// Height of the header area at the top, inside the top margin.
    pub header: Length,
    /// Height of the footer area at the bottom, inside the bottom margin.
    pub footer: Length,
}

/// The layout of a report that has never had one set: A4 portrait, 20 mm margins, no
/// header or footer area. The leaf names no default; this choice is stated in the PR.
pub const DEFAULT_LAYOUT: PageLayout = PageLayout {
    size: PageSize::A4,
    orientation: Orientation::Portrait,
    margins: Margins { top: 2_000, right: 2_000, bottom: 2_000, left: 2_000 },
    header: 0,
    footer: 0,
};

fn int(n: Length) -> CValue {
    CValue::Str(n.to_string())
}

fn invalid(field: &'static str, reason: &'static str) -> LayoutError {
    LayoutError::LayoutInvalid { field, reason }
}

impl PageLayout {
    /// `(width, height)` of the page after orientation.
    pub const fn page_dims(&self) -> (Length, Length) {
        let (w, h) = self.size.dims();
        match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        }
    }

    /// The body rectangle: the page minus the margins, minus the header area at the top and
    /// the footer area at the bottom (`Design: Algorithm`). Unchecked; see [`Self::check`].
    pub fn body(&self) -> Rect {
        let (w, h) = self.page_dims();
        let m = self.margins;
        Rect {
            x: m.left,
            y: m.top.saturating_add(self.header),
            width: w.saturating_sub(m.left).saturating_sub(m.right),
            height: h
                .saturating_sub(m.top)
                .saturating_sub(m.bottom)
                .saturating_sub(self.header)
                .saturating_sub(self.footer),
        }
    }

    /// Checks the layout and returns its body: every length `0..=2000 mm`, a custom size
    /// `50 mm` to `2000 mm` per side (REQ-002), a body at least `20 mm` wide and high
    /// (REQ-005). Fields are checked in this order: `width`, `height`, `margin_top`,
    /// `margin_right`, `margin_bottom`, `margin_left`, `header`, `footer`, then the body.
    pub fn check(&self) -> Result<Rect, LayoutError> {
        if let PageSize::Custom { width, height } = self.size {
            for (field, side) in [("width", width), ("height", height)] {
                if !(MIN_CUSTOM_SIDE..=MAX_CUSTOM_SIDE).contains(&side) {
                    return Err(invalid(field, "outside_custom_range"));
                }
            }
        }
        let m = self.margins;
        let lengths = [
            ("margin_top", m.top),
            ("margin_right", m.right),
            ("margin_bottom", m.bottom),
            ("margin_left", m.left),
            ("header", self.header),
            ("footer", self.footer),
        ];
        for (field, n) in lengths {
            if !(0..=MAX_LENGTH).contains(&n) {
                return Err(invalid(field, "too_large"));
            }
        }
        let body = self.body();
        if body.width < MIN_BODY {
            return Err(invalid("body_width", "body_too_narrow"));
        }
        if body.height < MIN_BODY {
            return Err(invalid("body_height", "body_too_short"));
        }
        Ok(body)
    }

    /// Stored and journaled form: `{footer, header, margins: {bottom, left, right, top},
    /// orientation, size, size_height, size_width}` (sizes before orientation).
    pub fn to_value(&self) -> CValue {
        let (w, h) = self.size.dims();
        let m = self.margins;
        obj([
            ("footer", int(self.footer)),
            ("header", int(self.header)),
            (
                "margins",
                obj([("bottom", int(m.bottom)), ("left", int(m.left)), ("right", int(m.right)), ("top", int(m.top))]),
            ),
            ("orientation", CValue::Str(self.orientation.name().to_string())),
            ("size", CValue::Str(self.size.name().to_string())),
            ("size_height", int(h)),
            ("size_width", int(w)),
        ])
    }

    /// The effective form `report.show_page_layout@1` returns (REQ-008): the stored form plus
    /// `page_width`, `page_height` (after orientation) and `body {x, y, width, height}`.
    pub fn effective_map(&self) -> BTreeMap<String, CValue> {
        let mut m = match self.to_value() {
            CValue::Obj(m) => m,
            _ => BTreeMap::new(),
        };
        let (w, h) = self.page_dims();
        m.insert("body".to_string(), self.body().to_value());
        m.insert("page_height".to_string(), int(h));
        m.insert("page_width".to_string(), int(w));
        m
    }

    /// Reads the stored form back: exactly its keys, lengths `0..=2000 mm` as decimal
    /// strings, preset sizes with their own dimensions. It does not run [`Self::check`].
    pub fn from_value(v: &CValue) -> Result<PageLayout, LayoutError> {
        let m = exact(v, &["footer", "header", "margins", "orientation", "size", "size_height", "size_width"])?;
        let mm = exact(m.get("margins").unwrap_or(&CValue::Null), &["bottom", "left", "right", "top"])?;
        let margins = Margins {
            top: length(mm.get("top"))?,
            right: length(mm.get("right"))?,
            bottom: length(mm.get("bottom"))?,
            left: length(mm.get("left"))?,
        };
        let orientation = Orientation::parse(text(m.get("orientation"))?).ok_or(store("orientation"))?;
        let (w, h) = (length(m.get("size_width"))?, length(m.get("size_height"))?);
        let size = match text(m.get("size"))? {
            "custom" => PageSize::Custom { width: w, height: h },
            name => {
                let p = PageSize::preset(name).ok_or(store("size"))?;
                if p.dims() != (w, h) {
                    return Err(store("preset dimensions"));
                }
                p
            }
        };
        Ok(PageLayout {
            size,
            orientation,
            margins,
            header: length(m.get("header"))?,
            footer: length(m.get("footer"))?,
        })
    }
}

fn store(what: &'static str) -> LayoutError {
    LayoutError::StoreInvalid(what)
}

fn exact<'a>(v: &'a CValue, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, LayoutError> {
    match v {
        CValue::Obj(m) if m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)) => Ok(m),
        _ => Err(store("keys")),
    }
}

fn text(v: Option<&CValue>) -> Result<&str, LayoutError> {
    match v {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(store("string")),
    }
}

/// A stored length: canonical decimal digits (no sign, no leading zero), `0..=2000 mm`.
fn length(v: Option<&CValue>) -> Result<Length, LayoutError> {
    let s = text(v)?;
    let canonical = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    match s.parse::<Length>() {
        Ok(n) if canonical && n <= MAX_LENGTH => Ok(n),
        _ => Err(store("length")),
    }
}

/// Where a page's effective layout comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutSource {
    /// The report default (no override for the page).
    Default,
    /// A per-page override (REQ-006).
    Override,
}

impl LayoutSource {
    pub const fn name(self) -> &'static str {
        match self {
            LayoutSource::Default => "default",
            LayoutSource::Override => "override",
        }
    }
}

/// The page setup of one report: a default layout and per-page overrides
/// (`Design: Data structures`; `BTreeMap`, no hash map).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLayout {
    pub default_layout: PageLayout,
    pub overrides: BTreeMap<PageId, PageLayout>,
}

impl Default for ReportLayout {
    fn default() -> ReportLayout {
        ReportLayout { default_layout: DEFAULT_LAYOUT, overrides: BTreeMap::new() }
    }
}

impl ReportLayout {
    /// The layout in effect for `page`: its override, else the default (REQ-006).
    pub fn effective(&self, page: &PageId) -> (PageLayout, LayoutSource) {
        match self.overrides.get(page) {
            Some(l) => (*l, LayoutSource::Override),
            None => (self.default_layout, LayoutSource::Default),
        }
    }

    /// Stored form: `{default, overrides: {<page id>: layout}}`.
    pub fn to_value(&self) -> CValue {
        let overrides = self.overrides.iter().map(|(p, l)| (p.uuid().to_string(), l.to_value())).collect();
        obj([("default", self.default_layout.to_value()), ("overrides", CValue::Obj(overrides))])
    }

    /// SHA-256 of the canonical (RFC 8785) bytes of [`Self::to_value`]; every
    /// `report.set_page_layout@1` event carries it, and exact replay compares it.
    pub fn digest(&self) -> Result<Digest, LayoutError> {
        let bytes = to_canonical_bytes(&self.to_value()).map_err(|_| LayoutError::StoreInvalid("canonical form"))?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}
