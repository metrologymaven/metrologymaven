//! Data structures and canonical JSON representations for `RPT.BALLOON.BLOCK`.
//!
//! Pure integer geometry (`0.01 mm`) and exact rationals: zero floating-point types or
//! operations (`RPT` float rule), `BTreeMap` only.

use crate::balloon::sheet::{digest_bytes, SheetId};
use crate::doc::kind::PayloadError;
use crate::doc::model::{obj, BlockKindId, CanonicalJson};
use fid_canon::{to_canonical_bytes, CValue};
use fid_store::cas::BlobRef;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registered `BlockKindId` string for ballooned-drawing report blocks (`RPT.BALLOON.BLOCK`).
pub const DRAWING_BLOCK_KIND: &str = "drawing";
/// Current payload schema version for `drawing` blocks.
pub const DRAWING_BLOCK_VERSION: u32 = 1;
/// Maximum JSON byte size when reading an optional `UX.REPORT` balloon style artifact.
pub const MAX_STYLE_BYTES: usize = 64 * 1024;

/// Integer axis-aligned crop rectangle in sheet coordinates (`0.01 mm`, top-left origin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetRect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl SheetRect {
    pub const fn new(x: i64, y: i64, width: i64, height: i64) -> Self {
        Self { x, y, width, height }
    }

    /// Returns `true` iff `self` has positive area and lies wholly inside `[0..=sheet_w] x [0..=sheet_h]`.
    pub fn fits_in_sheet(self, sheet_w: i64, sheet_h: i64) -> bool {
        if self.x < 0 || self.y < 0 || self.width <= 0 || self.height <= 0 {
            return false;
        }
        let Some(right) = self.x.checked_add(self.width) else { return false };
        let Some(bottom) = self.y.checked_add(self.height) else { return false };
        right <= sheet_w && bottom <= sheet_h
    }

    /// Returns `true` iff point `(px, py)` lies inside this rectangle (inclusive on all four edges).
    pub fn contains(self, px: i64, py: i64) -> bool {
        let Some(right) = self.x.checked_add(self.width) else { return false };
        let Some(bottom) = self.y.checked_add(self.height) else { return false };
        px >= self.x && px <= right && py >= self.y && py <= bottom
    }

    pub fn to_value(self) -> CValue {
        obj([
            ("height", CValue::Str(self.height.to_string())),
            ("width", CValue::Str(self.width.to_string())),
            ("x", CValue::Str(self.x.to_string())),
            ("y", CValue::Str(self.y.to_string())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, PayloadError> {
        let CValue::Obj(m) = v else {
            return Err(PayloadError::new("/crop", "must_be_object"));
        };
        if m.len() != 4 {
            return Err(PayloadError::new("/crop", "unexpected_keys"));
        }
        let x = req_i64(m, "x", "/crop/x")?;
        let y = req_i64(m, "y", "/crop/y")?;
        let width = req_i64(m, "width", "/crop/width")?;
        let height = req_i64(m, "height", "/crop/height")?;
        if x < 0 || y < 0 || width <= 0 || height <= 0 {
            return Err(PayloadError::new("/crop", "empty_or_negative"));
        }
        if x.checked_add(width).is_none() || y.checked_add(height).is_none() {
            return Err(PayloadError::new("/crop", "overflow"));
        }
        Ok(SheetRect { x, y, width, height })
    }
}

/// Sheet-to-block fit policy (`Design: Registry command`, `REQ-007`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Aspect-preserving scale to fit centred inside the block frame.
    ScaleToFit,
    /// Fixed rational scale `num / den` (`num >= 1`, `den >= 1`), centred in the block frame.
    FixedScale { num: u32, den: u32 },
}

impl FitMode {
    pub fn to_value(self) -> CValue {
        match self {
            FitMode::ScaleToFit => obj([("kind", CValue::Str("scale_to_fit".into()))]),
            FitMode::FixedScale { num, den } => obj([
                ("den", CValue::Str(den.to_string())),
                ("kind", CValue::Str("fixed_scale".into())),
                ("num", CValue::Str(num.to_string())),
            ]),
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, PayloadError> {
        let CValue::Obj(m) = v else {
            return Err(PayloadError::new("/fit", "must_be_object"));
        };
        let Some(CValue::Str(kind)) = m.get("kind") else {
            return Err(PayloadError::new("/fit/kind", "missing_or_not_string"));
        };
        match kind.as_str() {
            "scale_to_fit" => {
                if m.len() != 1 {
                    return Err(PayloadError::new("/fit", "unexpected_keys"));
                }
                Ok(FitMode::ScaleToFit)
            }
            "fixed_scale" => {
                if m.len() != 3 {
                    return Err(PayloadError::new("/fit", "unexpected_keys"));
                }
                let num = req_u32(m, "num", "/fit/num")?;
                let den = req_u32(m, "den", "/fit/den")?;
                if num == 0 || den == 0 {
                    return Err(PayloadError::new("/fit", "zero_scale"));
                }
                Ok(FitMode::FixedScale { num, den })
            }
            _ => Err(PayloadError::new("/fit/kind", "unknown_fit_kind")),
        }
    }
}

/// Stored payload of a `drawing` report block (version 1).
///
/// Stores ONLY references and styling options; balloon numbers and positions are never copied
/// into the payload (`RPT.BALLOON.BLOCK`, `REQ-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingPayload {
    pub sheet: SheetId,
    pub crop: Option<SheetRect>,
    pub fit: FitMode,
    pub outcome_marking: bool,
    pub show_leaders: bool,
}

impl DrawingPayload {
    pub fn to_value(&self) -> CValue {
        obj([
            ("crop", self.crop.map_or(CValue::Null, SheetRect::to_value)),
            ("fit", self.fit.to_value()),
            ("outcome_marking", CValue::Bool(self.outcome_marking)),
            ("sheet", CValue::Str(self.sheet.to_string())),
            ("show_leaders", CValue::Bool(self.show_leaders)),
        ])
    }

    pub fn to_canonical_json(&self) -> Result<CanonicalJson, DrawingBlockError> {
        CanonicalJson::new(&self.to_value())
            .ok_or_else(|| DrawingBlockError::Store("drawing payload failed canonical json check".into()))
    }

    pub fn from_value(v: &CValue) -> Result<Self, PayloadError> {
        let CValue::Obj(m) = v else {
            return Err(PayloadError::new("", "must_be_object"));
        };
        if m.len() != 5 {
            return Err(PayloadError::new("", "unexpected_keys"));
        }
        let Some(CValue::Str(sheet_s)) = m.get("sheet") else {
            return Err(PayloadError::new("/sheet", "missing_or_not_string"));
        };
        let sheet = SheetId::parse(sheet_s).map_err(|_| PayloadError::new("/sheet", "invalid_uuidv7"))?;
        let crop = match m.get("crop") {
            Some(CValue::Null) => None,
            Some(cv) => Some(SheetRect::from_value(cv)?),
            None => return Err(PayloadError::new("/crop", "missing")),
        };
        let fit_v = m.get("fit").ok_or_else(|| PayloadError::new("/fit", "missing"))?;
        let fit = FitMode::from_value(fit_v)?;
        let Some(CValue::Bool(outcome_marking)) = m.get("outcome_marking") else {
            return Err(PayloadError::new("/outcome_marking", "missing_or_not_bool"));
        };
        let Some(CValue::Bool(show_leaders)) = m.get("show_leaders") else {
            return Err(PayloadError::new("/show_leaders", "missing_or_not_bool"));
        };
        Ok(DrawingPayload { sheet, crop, fit, outcome_marking: *outcome_marking, show_leaders: *show_leaders })
    }

    pub fn from_canonical_json(c: &CanonicalJson) -> Result<Self, PayloadError> {
        Self::from_value(c.value())
    }
}

/// Returns the [`BlockKindId`] for `"drawing"`.
pub fn drawing_kind_id() -> Result<BlockKindId, DrawingBlockError> {
    BlockKindId::new(DRAWING_BLOCK_KIND).ok_or_else(|| DrawingBlockError::Store("invalid drawing block kind id".into()))
}

/// Target frame dimensions on the report page (`0.01 mm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFrame {
    pub width_cmm: i64,
    pub height_cmm: i64,
}

impl BlockFrame {
    pub const fn new(width_cmm: i64, height_cmm: i64) -> Self {
        Self { width_cmm, height_cmm }
    }
}

/// Integer coordinate inside the resolved block frame (`0.01 mm`, top-left origin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPoint {
    pub x: i64,
    pub y: i64,
}

impl BlockPoint {
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    pub fn to_value(self) -> CValue {
        obj([("x", CValue::Str(self.x.to_string())), ("y", CValue::Str(self.y.to_string()))])
    }
}

/// Exact integer rational isotropic scale + translation mapping sheet (or crop) coordinates
/// into block frame coordinates (`0.01 mm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Affine2Int {
    pub scale_num: i64,
    pub scale_den: i64,
    pub offset_x: i64,
    pub offset_y: i64,
}

impl Affine2Int {
    pub fn to_value(self) -> CValue {
        obj([
            ("offset_x", CValue::Str(self.offset_x.to_string())),
            ("offset_y", CValue::Str(self.offset_y.to_string())),
            ("scale_den", CValue::Str(self.scale_den.to_string())),
            ("scale_num", CValue::Str(self.scale_num.to_string())),
        ])
    }
}

/// Vector shape of a balloon glyph from `UX.REPORT` balloon style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalloonShape {
    Circle,
    Pill,
    Square,
}

impl BalloonShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            BalloonShape::Circle => "circle",
            BalloonShape::Pill => "pill",
            BalloonShape::Square => "square",
        }
    }
}

/// Balloon glyph geometry parameters (`UX.REPORT` artifact or built-in defaults, `0.01 mm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BalloonStyle {
    pub shape: BalloonShape,
    /// Outer radius of circular balloon in `0.01 mm` (default `250` = 2.50 mm).
    pub radius_cmm: i64,
    /// Nominal numeral cap-height in `0.01 mm` (default `220` = 2.20 mm).
    pub number_size_cmm: i64,
    /// Leader stroke width in `0.01 mm` (default `25` = 0.25 mm).
    pub leader_width_cmm: i64,
}

impl Default for BalloonStyle {
    fn default() -> Self {
        Self { shape: BalloonShape::Circle, radius_cmm: 250, number_size_cmm: 220, leader_width_cmm: 25 }
    }
}

impl BalloonStyle {
    /// Parses a `UX.REPORT` balloon style JSON artifact when published; falls back safely on
    /// caller-checked error if malformed.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, DrawingBlockError> {
        if bytes.len() > MAX_STYLE_BYTES {
            return Err(DrawingBlockError::Store("balloon style file exceeds size budget".into()));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_STYLE_BYTES, fid_canon::MAX_DEPTH)
            .map_err(|_| DrawingBlockError::Store("balloon style is not valid JSON".into()))?;
        let CValue::Obj(m) = v else {
            return Err(DrawingBlockError::Store("balloon style must be an object".into()));
        };
        let shape = match m.get("shape") {
            Some(CValue::Str(s)) => match s.as_str() {
                "circle" => BalloonShape::Circle,
                "pill" => BalloonShape::Pill,
                "square" => BalloonShape::Square,
                _ => return Err(DrawingBlockError::Store("unknown balloon style shape".into())),
            },
            _ => BalloonShape::Circle,
        };
        let radius_cmm = opt_pos_i64(&m, "radius_cmm")?.unwrap_or(250);
        let number_size_cmm = opt_pos_i64(&m, "number_size_cmm")?.unwrap_or(220);
        let leader_width_cmm = opt_pos_i64(&m, "leader_width_cmm")?.unwrap_or(25);
        Ok(BalloonStyle { shape, radius_cmm, number_size_cmm, leader_width_cmm })
    }

    pub fn to_value(self) -> CValue {
        obj([
            ("leader_width_cmm", CValue::Str(self.leader_width_cmm.to_string())),
            ("number_size_cmm", CValue::Str(self.number_size_cmm.to_string())),
            ("radius_cmm", CValue::Str(self.radius_cmm.to_string())),
            ("shape", CValue::Str(self.shape.as_str().into())),
        ])
    }
}

/// Non-colour + colour-role decision mark attached to a balloon glyph when outcome marking is on
/// (`REQ-003`, `AC-3`, `design/tokens/status.json`, `RPT.MODEL.FMT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeMark {
    /// Semantic status colour role (e.g. `status.conforms`, `status.nonconforms`, `status.not-evaluated`).
    pub colour_role: String,
    /// Non-colour cue glyph ID from `design/tokens/status.json` (e.g. `state.conforms`).
    pub symbol: String,
    /// Catalog text from `RPT.MODEL.FMT` (`conforms`, `nonconforms`, `indeterminate`, `not decidable`, `not evaluated`).
    pub label: String,
}

impl OutcomeMark {
    pub fn to_value(&self) -> CValue {
        obj([
            ("colour_role", CValue::Str(self.colour_role.clone())),
            ("label", CValue::Str(self.label.clone())),
            ("symbol", CValue::Str(self.symbol.clone())),
        ])
    }
}

/// One resolved vector balloon glyph (never baked into the sheet raster image, `REQ-004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonGlyph {
    pub number: u32,
    pub anchor: BlockPoint,
    pub label: BlockPoint,
    /// Optional vector leader segment `(anchor, label)` when `show_leaders` is enabled.
    pub leader: Option<(BlockPoint, BlockPoint)>,
    pub style: BalloonStyle,
    pub outcome: Option<OutcomeMark>,
}

impl BalloonGlyph {
    pub fn to_value(&self) -> CValue {
        let leader = match self.leader {
            Some((a, b)) => obj([("from", a.to_value()), ("to", b.to_value())]),
            None => CValue::Null,
        };
        obj([
            ("anchor", self.anchor.to_value()),
            ("label", self.label.to_value()),
            ("leader", leader),
            ("number", CValue::Str(self.number.to_string())),
            ("outcome", self.outcome.as_ref().map_or(CValue::Null, OutcomeMark::to_value)),
            ("style", self.style.to_value()),
        ])
    }
}

/// Resolved content of a `drawing` report block (`Design: Data structures`).
///
/// Sheet image reference and vector balloon glyphs remain separate so PDF and preview
/// renderers draw balloons as crisp vectors (`REQ-004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingBlockContent {
    pub sheet_id: SheetId,
    pub sheet_image: Option<BlobRef>,
    pub transform: Affine2Int,
    pub glyphs: Vec<BalloonGlyph>,
    /// Balloon numbers on this sheet whose anchors fell outside the active crop region (`REQ-005`).
    pub excluded: Vec<u32>,
}

impl DrawingBlockContent {
    pub fn to_value(&self) -> CValue {
        let sheet_image = match self.sheet_image {
            Some(b) => obj([("digest", CValue::Str(b.digest.to_string())), ("size", CValue::Str(b.size.to_string()))]),
            None => CValue::Null,
        };
        obj([
            ("excluded", CValue::Arr(self.excluded.iter().map(|n| CValue::Str(n.to_string())).collect())),
            ("glyphs", CValue::Arr(self.glyphs.iter().map(BalloonGlyph::to_value).collect())),
            ("sheet_id", CValue::Str(self.sheet_id.to_string())),
            ("sheet_image", sheet_image),
            ("transform", self.transform.to_value()),
        ])
    }

    /// Computes the canonical RFC 8785 SHA-256 digest of this resolved content (`AC-6`).
    pub fn canonical_digest(&self) -> Result<Digest, DrawingBlockError> {
        let bytes = to_canonical_bytes(&self.to_value())
            .map_err(|e| DrawingBlockError::Store(format!("canonicalise drawing block content: {e}")))?;
        Ok(digest_bytes(&bytes))
    }
}

/// Typed errors of `RPT.BALLOON.BLOCK` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawingBlockError {
    /// `report.invalid_block_payload`: missing/not-ready sheet, invalid crop, invalid fit, or
    /// target block is not kind `drawing` (`REQ-006`, `AC-5`).
    InvalidBlockPayload { field: &'static str, reason: &'static str },
    /// `report.not_found`: named report or block does not exist.
    NotFound(&'static str),
    /// Internal storage or canonicalisation error.
    Store(String),
}

impl DrawingBlockError {
    pub fn code(&self) -> &'static str {
        match self {
            DrawingBlockError::InvalidBlockPayload { .. } => "report.invalid_block_payload",
            DrawingBlockError::NotFound(_) => "report.not_found",
            DrawingBlockError::Store(_) => "report.store_invalid",
        }
    }
}

impl fmt::Display for DrawingBlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrawingBlockError::InvalidBlockPayload { field, reason } => {
                write!(f, "report.invalid_block_payload ({field}): {reason}")
            }
            DrawingBlockError::NotFound(what) => write!(f, "report.not_found: {what}"),
            DrawingBlockError::Store(detail) => write!(f, "report.store_invalid: {detail}"),
        }
    }
}

impl std::error::Error for DrawingBlockError {}

fn req_i64(m: &BTreeMap<String, CValue>, k: &str, ptr: &str) -> Result<i64, PayloadError> {
    let Some(v) = m.get(k) else {
        return Err(PayloadError::new(ptr, "missing_or_not_integer"));
    };
    if let Some(u) = crate::json_num::as_u32(v) {
        return Ok(i64::from(u));
    }
    match v {
        CValue::Str(s) => s.parse::<i64>().map_err(|_| PayloadError::new(ptr, "invalid_integer")),
        CValue::I64(n) => Ok(*n),
        CValue::U64(n) => i64::try_from(*n).map_err(|_| PayloadError::new(ptr, "integer_overflow")),
        _ => Err(PayloadError::new(ptr, "missing_or_not_integer")),
    }
}

fn req_u32(m: &BTreeMap<String, CValue>, k: &str, ptr: &str) -> Result<u32, PayloadError> {
    let Some(v) = m.get(k) else {
        return Err(PayloadError::new(ptr, "missing_or_not_u32"));
    };
    if let Some(u) = crate::json_num::as_u32(v) {
        return Ok(u);
    }
    match v {
        CValue::Str(s) => s.parse::<u32>().map_err(|_| PayloadError::new(ptr, "invalid_u32")),
        CValue::U64(n) => u32::try_from(*n).map_err(|_| PayloadError::new(ptr, "u32_overflow")),
        _ => Err(PayloadError::new(ptr, "missing_or_not_u32")),
    }
}

fn opt_pos_i64(m: &BTreeMap<String, CValue>, k: &str) -> Result<Option<i64>, DrawingBlockError> {
    let Some(v) = m.get(k) else {
        return Ok(None);
    };
    let n = if let Some(u) = crate::json_num::as_u32(v) {
        i64::from(u)
    } else {
        match v {
            CValue::Str(s) => {
                s.parse::<i64>().map_err(|_| DrawingBlockError::Store(format!("balloon style {k} invalid integer")))?
            }
            CValue::I64(n) => *n,
            CValue::U64(n) => {
                i64::try_from(*n).map_err(|_| DrawingBlockError::Store(format!("balloon style {k} overflow")))?
            }
            _ => return Err(DrawingBlockError::Store(format!("balloon style {k} must be integer"))),
        }
    };
    if n <= 0 {
        return Err(DrawingBlockError::Store(format!("balloon style {k} must be positive")));
    }
    Ok(Some(n))
}
