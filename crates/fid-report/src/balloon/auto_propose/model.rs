//! Data structures and errors for `RPT.BALLOON.AUTO.PROPOSE` (`fid_report::balloon::auto_propose`).
//!
//! Pure integer geometry and exact verbatim decimal strings: zero floating-point types or ops.

use crate::balloon::auto_parse::{ConfidenceWord, ReasonCode};
use crate::balloon::auto_text::SheetRect;
use crate::balloon::char::{AuthoredUnit, CharBalloonError, Dialect, DrawingCharInput};
use crate::balloon::register::SheetPoint;
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId};
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_gdt::chars::spec::PlanId;
use fid_types::Digest;
use std::fmt;

/// Diagnostic code emitted when a sheet is raster-only and no OCR candidates were supplied (`REQ-003`, `AC-3`).
pub const DIAG_RASTER_NEEDS_OCR: &str = "report.raster_sheet_needs_ocr";

/// Maximum OCR candidates accepted per sheet (`REQ-004`).
pub const MAX_OCR_CANDIDATES: usize = 10_000;

/// Origin of a proposed balloon (`REQ-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalOrigin {
    VectorParse,
    OcrCandidate,
}

impl ProposalOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProposalOrigin::VectorParse => "vector_parse",
            ProposalOrigin::OcrCandidate => "ocr_candidate",
        }
    }
}

/// Configurable engineering drawing border grid for zone labels (e.g., rows A..D top-to-bottom,
/// columns 1..8 left-to-right -> `"B4"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneGridSpec {
    pub rows: u8,
    pub cols: u8,
}

impl Default for ZoneGridSpec {
    fn default() -> Self {
        ZoneGridSpec { rows: 4, cols: 8 }
    }
}

impl ZoneGridSpec {
    /// Computes the zone label (e.g. `"B4"`) for point `(x, y)` on a sheet `(w, h)`.
    pub fn zone_for(self, pt: SheetPoint, bounds: (i64, i64)) -> Option<String> {
        let (w, h) = bounds;
        if self.rows == 0 || self.cols == 0 || self.rows > 26 || w <= 0 || h <= 0 {
            return None;
        }
        let col_idx = ((i128::from(pt.x.clamp(0, w - 1)) * i128::from(self.cols)) / i128::from(w)) as u8;
        let row_idx = ((i128::from(pt.y.clamp(0, h - 1)) * i128::from(self.rows)) / i128::from(h)) as u8;
        let row_ch = (b'A' + row_idx.min(25)) as char;
        let col_num = col_idx.saturating_add(1);
        Some(format!("{row_ch}{col_num}"))
    }
}

/// Caller-supplied untrusted OCR text run candidate for scanned raster sheets (`REQ-004`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrTextRun {
    pub sheet: SheetId,
    pub text: String,
    pub bbox: SheetRect,
    pub font_h_cmm: i64,
}

/// Typed diagnostic returned alongside a [`BalloonProposalBatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub sheet: Option<SheetId>,
    pub detail: String,
}

impl Diagnostic {
    pub fn to_value(&self) -> CValue {
        obj([
            ("code", CValue::Str(self.code.clone())),
            ("detail", CValue::Str(self.detail.clone())),
            ("sheet", self.sheet.map_or(CValue::Null, |s| CValue::Str(s.to_string()))),
        ])
    }
}

/// One proposed balloon + characteristic staged on a W1 draft/agent branch (`REQ-002`, `Design`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonProposal {
    pub provisional_number: u32,
    pub sheet: SheetId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    pub source_bbox: SheetRect,
    pub zone: Option<Untrusted<Text>>,
    pub raw_text: Untrusted<Text>,
    pub char_input: Option<DrawingCharInput>,
    pub origin: ProposalOrigin,
    pub confidence: ConfidenceWord,
    pub reasons: Vec<ReasonCode>,
    pub needs_confirmation: bool,
}

impl BalloonProposal {
    pub fn to_value(&self) -> CValue {
        let char_v = match &self.char_input {
            Some(ci) => obj([
                ("datum_refs", CValue::Arr(ci.datum_refs.iter().map(Untrusted::to_value).collect())),
                ("dialect", ci.dialect.map_or(CValue::Null, |d| CValue::Str(d.as_str().into()))),
                ("kind", CValue::Str(ci.kind.as_str().into())),
                ("lower", ci.lower.to_value()),
                ("name", ci.name.as_ref().map_or(CValue::Null, Untrusted::to_value)),
                ("nominal", ci.nominal.to_value()),
                ("upper", ci.upper.to_value()),
            ]),
            None => CValue::Null,
        };
        obj([
            ("anchor", self.anchor.to_value()),
            ("char_input", char_v),
            ("confidence", CValue::Str(self.confidence.as_str().into())),
            ("label", self.label.to_value()),
            ("needs_confirmation", CValue::Bool(self.needs_confirmation)),
            ("origin", CValue::Str(self.origin.as_str().into())),
            ("provisional_number", CValue::Str(self.provisional_number.to_string())),
            ("raw_text", self.raw_text.to_value()),
            ("reasons", CValue::Arr(self.reasons.iter().map(|r| CValue::Str(r.as_str().into())).collect())),
            ("sheet", CValue::Str(self.sheet.to_string())),
            ("source_bbox", self.source_bbox.to_value()),
            ("zone", self.zone.as_ref().map_or(CValue::Null, Untrusted::to_value)),
        ])
    }
}

/// Complete W1 draft-branch proposal batch (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonProposalBatch {
    pub branch: String,
    pub plan: PlanId,
    pub drawing_rev: DrawingRevId,
    pub proposals: Vec<BalloonProposal>,
    pub skipped_runs: u32,
    pub diagnostics: Vec<Diagnostic>,
    pub branch_digest: Digest,
}

impl BalloonProposalBatch {
    pub fn to_value(&self) -> CValue {
        obj([
            ("branch", CValue::Str(self.branch.clone())),
            ("branch_digest", CValue::Str(self.branch_digest.to_string())),
            ("diagnostics", CValue::Arr(self.diagnostics.iter().map(Diagnostic::to_value).collect())),
            ("drawing_rev", CValue::Str(self.drawing_rev.to_string())),
            ("plan", CValue::Str(self.plan.as_str().into())),
            ("proposals", CValue::Arr(self.proposals.iter().map(BalloonProposal::to_value).collect())),
            ("skipped_runs", CValue::Str(self.skipped_runs.to_string())),
        ])
    }
}

/// Parameters of `report.propose_balloons@1` (`W1`, `Design`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposeBalloonsParams {
    pub plan: PlanId,
    pub drawing_rev: DrawingRevId,
    pub sheets: Option<Vec<SheetId>>,
    pub dialect: Option<Dialect>,
    pub default_unit: Option<AuthoredUnit>,
    pub zone_grid: ZoneGridSpec,
    pub ocr_candidates: Vec<OcrTextRun>,
    pub branch: String,
    pub dry_run: bool,
}

/// Human decision for one proposal during H2 batch adoption (`AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptDecision {
    pub provisional_number: u32,
    /// Optional human-edited characteristic input overriding the proposed one (`AC-2`).
    pub edited_char_input: Option<DrawingCharInput>,
    /// Optional human-edited label position.
    pub edited_label: Option<SheetPoint>,
    /// Optional human-edited zone label.
    pub edited_zone: Option<String>,
}

/// Provenance link retained on every H2-adopted characteristic/balloon (`REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptedProvenance {
    pub characteristic_id: CharId,
    pub balloon_id: BalloonId,
    pub balloon_number: u32,
    pub source_sheet: SheetId,
    pub source_bbox: SheetRect,
    pub raw_text: Untrusted<Text>,
    pub origin: ProposalOrigin,
}

/// Result of H2 batch adoption (`AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptBatchResult {
    pub branch: String,
    pub adopted: Vec<AdoptedProvenance>,
}

/// Typed errors of `RPT.BALLOON.AUTO.PROPOSE` (`Design: Errors`, `REQ-004`, `AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposeError {
    NotFound(&'static str),
    BalloonOffSheet {
        x: i64,
        y: i64,
        width_cmm: i64,
        height_cmm: i64,
    },
    LimitExceeded {
        limit: &'static str,
        detail: String,
    },
    TextInvalid(&'static str),
    /// Non-human principal attempted H2 adoption (`AC-2`, CONSTITUTION C5).
    HumanOnly,
    /// Proposal lacks explicit characteristic input / dialect required for W2 adoption.
    IncompleteProposal {
        provisional_number: u32,
        detail: &'static str,
    },
    CharBalloon(CharBalloonError),
    BadParams(&'static str),
    Store(String),
}

impl ProposeError {
    pub fn code(&self) -> &'static str {
        match self {
            ProposeError::NotFound(_) => "report.not_found",
            ProposeError::BalloonOffSheet { .. } => "report.balloon_off_sheet",
            ProposeError::LimitExceeded { .. } => "report.limit_exceeded",
            ProposeError::TextInvalid(_) => "report.text_invalid",
            ProposeError::HumanOnly => "core.human_only",
            ProposeError::IncompleteProposal { .. } => "report.incomplete_proposal",
            ProposeError::CharBalloon(e) => e.code(),
            ProposeError::BadParams(_) => "report.bad_params",
            ProposeError::Store(_) => "report.store_invalid",
        }
    }
}

impl From<CharBalloonError> for ProposeError {
    fn from(e: CharBalloonError) -> Self {
        ProposeError::CharBalloon(e)
    }
}

impl fmt::Display for ProposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProposeError::NotFound(w) => write!(f, "report.not_found: {w}"),
            ProposeError::BalloonOffSheet { x, y, width_cmm, height_cmm } => {
                write!(f, "report.balloon_off_sheet: ({x},{y}) outside {width_cmm}x{height_cmm}")
            }
            ProposeError::LimitExceeded { limit, detail } => write!(f, "report.limit_exceeded ({limit}): {detail}"),
            ProposeError::TextInvalid(r) => write!(f, "report.text_invalid: {r}"),
            ProposeError::HumanOnly => write!(f, "core.human_only: H2 adoption requires a human principal"),
            ProposeError::IncompleteProposal { provisional_number, detail } => {
                write!(f, "report.incomplete_proposal #{provisional_number}: {detail}")
            }
            ProposeError::CharBalloon(e) => write!(f, "{e}"),
            ProposeError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            ProposeError::Store(d) => write!(f, "report.store_invalid: {d}"),
        }
    }
}

impl std::error::Error for ProposeError {}
