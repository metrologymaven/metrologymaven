//! Core data model for 2D drawing revisions, sheets, coordinate systems and `SheetInfo`
//! (`RPT.BALLOON.SHEET.IMPORT`).
//!
//! Pure integer geometry (`0.01 mm`, top-left origin): zero floating-point types or operations
//! (RPT float rule, `RPT.MODEL.FMT` REQ-002).

use super::error::SheetError;
use crate::doc::model::{is_v7, obj, parse_canonical_uuid};
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_store::cas::BlobRef;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Maximum UTF-8 byte length for drawing metadata (`drawing_number`, `revision_label`, `title`, `REQ-004`).
pub const MAX_DRAWING_TEXT_BYTES: usize = 256;

/// Maximum positive sheet width/height in `0.01 mm` (100 metres = 10_000_000 cmm) to guard
/// arithmetic against hostile size parameters.
pub const MAX_SHEET_DIM_CMM: i64 = 10_000_000;

/// Maximum stated DPI accepted by `report.set_size_drawing_balloon@1`.
pub const MAX_DPI: u32 = 100_000;

macro_rules! v7_sheet_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Wraps an RFC 9562 UUIDv7; rejects any non-v7 UUID.
            pub fn new(id: Uuid) -> Option<$name> {
                is_v7(&id).then_some($name(id))
            }

            /// Parses a canonical lowercase hyphenated UUIDv7 string.
            pub fn parse(text: &str) -> Result<$name, SheetError> {
                let u = parse_canonical_uuid(text).ok_or(SheetError::BadParams("invalid uuid"))?;
                $name::new(u).ok_or(SheetError::BadParams("uuid must be v7"))
            }

            pub fn uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

v7_sheet_id!(
    /// Part revision identifier (UUIDv7, matching `fid_project::container::Revision::id`).
    RevisionId
);
v7_sheet_id!(
    /// Drawing revision identifier (UUIDv7, `REQ-001`).
    DrawingRevId
);
v7_sheet_id!(
    /// Drawing sheet identifier (UUIDv7, `REQ-001`).
    SheetId
);
v7_sheet_id!(
    /// Balloon identifier (UUIDv7, referenced by `DrawingRevision::carried_from`).
    BalloonId
);

/// Validates an untrusted drawing metadata string (at most 256 UTF-8 bytes, no NUL;
/// `require_non_empty` rejects empty strings for drawing number and revision label).
pub fn parse_drawing_text(s: &str, require_non_empty: bool) -> Result<Untrusted<Text>, SheetError> {
    if s.len() > MAX_DRAWING_TEXT_BYTES {
        return Err(SheetError::TextInvalid("exceeds 256 bytes"));
    }
    if s.contains('\0') {
        return Err(SheetError::TextInvalid("contains NUL"));
    }
    if require_non_empty && s.is_empty() {
        return Err(SheetError::TextInvalid("must not be empty"));
    }
    Untrusted::text(s).map_err(|_| SheetError::TextInvalid("invalid untrusted text"))
}

/// Warning emitted by `IO.REG.PLUGIN.DRAW` and retained on [`DrawingRevision`] (`REQ-007`, `AC-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderWarning {
    pub code: String,
    pub detail: String,
    pub byte_offset: Option<u64>,
}

impl ReaderWarning {
    pub fn from_draw(w: fid_io_draw::DrawWarning) -> Self {
        ReaderWarning { code: w.code, detail: w.detail, byte_offset: w.byte_offset }
    }

    pub fn to_value(&self) -> CValue {
        let off = match self.byte_offset {
            Some(n) => CValue::Str(n.to_string()),
            None => CValue::Null,
        };
        obj([
            ("byte_offset", off),
            ("code", CValue::Str(self.code.clone())),
            ("detail", CValue::Str(self.detail.clone())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, SheetError> {
        let CValue::Obj(m) = v else {
            return Err(SheetError::Store("reader warning is not an object".into()));
        };
        let code = req_str(m, "code")?.to_string();
        let detail = req_str(m, "detail")?.to_string();
        let byte_offset = match m.get("byte_offset") {
            Some(CValue::Null) | None => None,
            Some(CValue::Str(s)) => {
                Some(s.parse::<u64>().map_err(|_| SheetError::Store("invalid byte_offset".into()))?)
            }
            _ => return Err(SheetError::Store("invalid byte_offset field".into())),
        };
        Ok(ReaderWarning { code, detail, byte_offset })
    }
}

/// Readiness status of one [`DrawingSheet`] (`REQ-003`, `AC-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetStatus {
    /// Physical sheet dimensions in `0.01 mm` are known; ready for balloon placement.
    Ready,
    /// Raster page had no stated resolution; awaits `report.set_size_drawing_balloon@1`.
    NeedsSize,
}

impl SheetStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            SheetStatus::Ready => "ready",
            SheetStatus::NeedsSize => "needs_size",
        }
    }

    pub fn parse(s: &str) -> Result<Self, SheetError> {
        match s {
            "ready" => Ok(SheetStatus::Ready),
            "needs_size" => Ok(SheetStatus::NeedsSize),
            _ => Err(SheetError::Store("invalid sheet status".into())),
        }
    }
}

/// Physical size specification for `report.set_size_drawing_balloon@1` (`REQ-003`, `Design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeSpec {
    /// Explicit sheet width in integer `0.01 mm`; height is computed from pixel aspect ratio.
    Width { cmm: i64 },
    /// Explicit horizontal and vertical resolution in dots per inch.
    Dpi { x: u32, y: u32 },
}

impl SizeSpec {
    pub fn to_value(self) -> CValue {
        match self {
            SizeSpec::Width { cmm } => {
                obj([("cmm", CValue::Str(cmm.to_string())), ("kind", CValue::Str("width".into()))])
            }
            SizeSpec::Dpi { x, y } => obj([
                ("kind", CValue::Str("dpi".into())),
                ("x", CValue::Str(x.to_string())),
                ("y", CValue::Str(y.to_string())),
            ]),
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, SheetError> {
        let CValue::Obj(m) = v else {
            return Err(SheetError::BadParams("size must be an object"));
        };
        match req_str_param(m, "kind")? {
            "width" => {
                let cmm = parse_i64_str(req_str_param(m, "cmm")?).ok_or(SheetError::SizeInvalid("invalid cmm"))?;
                Ok(SizeSpec::Width { cmm })
            }
            "dpi" => {
                let x = parse_u32_str(req_str_param(m, "x")?).ok_or(SheetError::SizeInvalid("invalid dpi x"))?;
                let y = parse_u32_str(req_str_param(m, "y")?).ok_or(SheetError::SizeInvalid("invalid dpi y"))?;
                Ok(SizeSpec::Dpi { x, y })
            }
            _ => Err(SheetError::SizeInvalid("size.kind must be width or dpi")),
        }
    }
}

/// One sheet of a [`DrawingRevision`] (`Design: Data structures`).
///
/// Coordinates are integer `0.01 mm` with the origin at the top-left corner (`REQ-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingSheet {
    pub id: SheetId,
    pub drawing_rev: DrawingRevId,
    pub index: u32,
    pub width_cmm: i64,
    pub height_cmm: i64,
    pub status: SheetStatus,
    pub image: Option<BlobRef>,
    pub px: Option<(u32, u32)>,
}

impl DrawingSheet {
    pub fn to_value(&self) -> CValue {
        let image = match self.image {
            Some(b) => obj([("digest", CValue::Str(b.digest.to_string())), ("size", CValue::Str(b.size.to_string()))]),
            None => CValue::Null,
        };
        let px = match self.px {
            Some((w, h)) => obj([("height", CValue::Str(h.to_string())), ("width", CValue::Str(w.to_string()))]),
            None => CValue::Null,
        };
        obj([
            ("drawing_rev", CValue::Str(self.drawing_rev.to_string())),
            ("height_cmm", CValue::Str(self.height_cmm.to_string())),
            ("id", CValue::Str(self.id.to_string())),
            ("image", image),
            ("index", CValue::Str(self.index.to_string())),
            ("px", px),
            ("status", CValue::Str(self.status.as_str().into())),
            ("width_cmm", CValue::Str(self.width_cmm.to_string())),
        ])
    }
}

/// One imported 2D drawing revision linked to a part revision (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingRevision {
    pub id: DrawingRevId,
    pub part_rev: RevisionId,
    pub drawing_number: Untrusted<Text>,
    pub revision_label: Untrusted<Text>,
    pub title: Option<Untrusted<Text>>,
    pub source_digest: Digest,
    pub sheets: Vec<SheetId>,
    pub warnings: Vec<ReaderWarning>,
    pub carried_from: BTreeMap<BalloonId, BalloonId>,
}

impl DrawingRevision {
    pub fn to_value(&self) -> CValue {
        let title = match &self.title {
            Some(t) => t.to_value(),
            None => CValue::Null,
        };
        let sheets = CValue::Arr(self.sheets.iter().map(|s| CValue::Str(s.to_string())).collect());
        let warnings = CValue::Arr(self.warnings.iter().map(ReaderWarning::to_value).collect());
        let mut carried = BTreeMap::new();
        for (k, v) in &self.carried_from {
            carried.insert(k.to_string(), CValue::Str(v.to_string()));
        }
        obj([
            ("carried_from", CValue::Obj(carried)),
            ("drawing_number", self.drawing_number.to_value()),
            ("id", CValue::Str(self.id.to_string())),
            ("part_rev", CValue::Str(self.part_rev.to_string())),
            ("revision_label", self.revision_label.to_value()),
            ("sheets", sheets),
            ("source_digest", CValue::Str(self.source_digest.to_string())),
            ("title", title),
            ("warnings", warnings),
        ])
    }
}

/// Read-only sheet-metadata contract consumed by the balloon register and report drawing block
/// (`Design: Trait offered`, `REQ-003`).
pub trait SheetInfo {
    /// Physical sheet `(width_cmm, height_cmm)` in integer `0.01 mm`, or `None` if unknown or not ready.
    fn bounds(&self, s: SheetId) -> Option<(i64, i64)>;
    /// Owning drawing revision ID for sheet `s`.
    fn revision(&self, s: SheetId) -> Option<DrawingRevId>;
    /// Content-addressed raster blob reference, if the sheet has a decoded raster image.
    fn image(&self, s: SheetId) -> Option<BlobRef>;
    /// True when sheet `s` exists and has status [`SheetStatus::Ready`] (`REQ-003`, `AC-2`).
    fn ready(&self, s: SheetId) -> bool;
}

/// Computes `n / d` rounded half-to-even for `n >= 0, d > 0` using exact 128-bit integer arithmetic.
pub fn round_half_even_u128(n: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    let q = n / d;
    let r = n % d;
    match (2 * r).cmp(&d) {
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

/// Computes raster sheet `(width_cmm, height_cmm)` from pixel dimensions `(px_w, px_h)` and
/// a [`SizeSpec`] (`REQ-002`, `REQ-003`, `AC-2`).
pub fn compute_raster_bounds_cmm(px: (u32, u32), spec: SizeSpec) -> Result<(i64, i64), SheetError> {
    let (px_w, px_h) = px;
    if px_w == 0 || px_h == 0 {
        return Err(SheetError::SizeInvalid("raster sheet has zero pixel dimension"));
    }
    let (w, h) = match spec {
        SizeSpec::Dpi { x, y } => {
            if x == 0 || y == 0 || x > MAX_DPI || y > MAX_DPI {
                return Err(SheetError::SizeInvalid("dpi must be in 1..=100000"));
            }
            let w = round_half_even_u128(u128::from(px_w) * 2540, u128::from(x));
            let h = round_half_even_u128(u128::from(px_h) * 2540, u128::from(y));
            (w, h)
        }
        SizeSpec::Width { cmm } => {
            if cmm <= 0 || cmm > MAX_SHEET_DIM_CMM {
                return Err(SheetError::SizeInvalid("width cmm out of valid range"));
            }
            let w = cmm as u128;
            let h = round_half_even_u128(u128::from(px_h) * w, u128::from(px_w));
            (w, h)
        }
    };
    if w == 0 || h == 0 || w > MAX_SHEET_DIM_CMM as u128 || h > MAX_SHEET_DIM_CMM as u128 {
        return Err(SheetError::SizeInvalid("computed sheet bounds out of valid range"));
    }
    Ok((w as i64, h as i64))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, SheetError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(SheetError::Store("missing or non-string field".into())),
    }
}

fn req_str_param<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, SheetError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(SheetError::BadParams("missing or non-string param")),
    }
}

fn parse_i64_str(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

fn parse_u32_str(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}
