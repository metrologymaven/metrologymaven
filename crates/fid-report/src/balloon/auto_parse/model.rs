//! Data structures for deterministic engineering callout parsing (`RPT.BALLOON.AUTO.PARSE`).
//!
//! Pure integer geometry and exact verbatim decimal strings validated through
//! `fid_num::kdec::Decimal::parse`: zero floating-point types or operations (RPT float rule).

use crate::balloon::auto_text::SheetRect;
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_num::kdec::Decimal;

/// Authored decimal string preserved verbatim (including sign and trailing zeros, `REQ-003`),
/// guaranteed valid under `fid_num::kdec::Decimal::parse` without binary float conversion.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecimalString(String);

impl DecimalString {
    /// Validates `s` as an exact base-10 decimal string (optional leading `+`/`-`) via
    /// `fid_num::kdec::Decimal::parse` while preserving the exact authored spelling.
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        // `Decimal::parse` accepts optional leading `-` or `+` (strip `+` if needed).
        let body = trimmed.strip_prefix('+').unwrap_or(trimmed);
        if body.is_empty() || body.starts_with('+') {
            return None;
        }
        // Reject scientific notation or underscores in drawing callouts; keep pure fixed decimals.
        if !body.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-') {
            return None;
        }
        Decimal::parse(body).ok()?;
        Some(DecimalString(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Semantic GD&T geometric characteristic symbol (`Scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GdtSymbol {
    Position,
    Flatness,
    Circularity,
    Cylindricity,
    Perpendicularity,
    Parallelism,
    Angularity,
    ProfileSurface,
    ProfileLine,
    CircularRunout,
    TotalRunout,
    Concentricity,
    Symmetry,
    Straightness,
}

impl GdtSymbol {
    pub const fn as_str(self) -> &'static str {
        match self {
            GdtSymbol::Position => "position",
            GdtSymbol::Flatness => "flatness",
            GdtSymbol::Circularity => "circularity",
            GdtSymbol::Cylindricity => "cylindricity",
            GdtSymbol::Perpendicularity => "perpendicularity",
            GdtSymbol::Parallelism => "parallelism",
            GdtSymbol::Angularity => "angularity",
            GdtSymbol::ProfileSurface => "profile_surface",
            GdtSymbol::ProfileLine => "profile_line",
            GdtSymbol::CircularRunout => "circular_runout",
            GdtSymbol::TotalRunout => "total_runout",
            GdtSymbol::Concentricity => "concentricity",
            GdtSymbol::Symmetry => "symmetry",
            GdtSymbol::Straightness => "straightness",
        }
    }
}

/// Top-level classification of a parsed drawing callout (`Scope`, `REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CalloutKind {
    Linear,
    Diameter,
    SphericalDiameter,
    Radius,
    SphericalRadius,
    Angle,
    Chamfer,
    Thread,
    SurfaceFinish,
    BasicDimension,
    ReferenceDimension,
    GdtFcf(GdtSymbol),
}

impl CalloutKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            CalloutKind::Linear => "linear",
            CalloutKind::Diameter => "diameter",
            CalloutKind::SphericalDiameter => "spherical_diameter",
            CalloutKind::Radius => "radius",
            CalloutKind::SphericalRadius => "spherical_radius",
            CalloutKind::Angle => "angle",
            CalloutKind::Chamfer => "chamfer",
            CalloutKind::Thread => "thread",
            CalloutKind::SurfaceFinish => "surface_finish",
            CalloutKind::BasicDimension => "basic_dimension",
            CalloutKind::ReferenceDimension => "reference_dimension",
            CalloutKind::GdtFcf(_) => "gdt_fcf",
        }
    }

    pub const fn gdt_symbol(self) -> Option<GdtSymbol> {
        match self {
            CalloutKind::GdtFcf(s) => Some(s),
            _ => None,
        }
    }
}

/// Material condition or tolerance zone modifier in a GD&T feature control frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    Mmc,
    Lmc,
    Projected,
    FreeState,
    TangentPlane,
}

impl Modifier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Modifier::Mmc => "mmc",
            Modifier::Lmc => "lmc",
            Modifier::Projected => "projected",
            Modifier::FreeState => "free_state",
            Modifier::TangentPlane => "tangent_plane",
        }
    }
}

/// Hinted physical unit from callout suffix or title-block metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitHint {
    Mm,
    Inch,
    Deg,
}

impl UnitHint {
    pub const fn as_str(self) -> &'static str {
        match self {
            UnitHint::Mm => "mm",
            UnitHint::Inch => "in",
            UnitHint::Deg => "deg",
        }
    }
}

/// Qualitative confidence word attached to every parsed candidate (`REQ-002`, `AC-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfidenceWord {
    High,
    Medium,
    Low,
}

impl ConfidenceWord {
    pub const fn as_str(self) -> &'static str {
        match self {
            ConfidenceWord::High => "high",
            ConfidenceWord::Medium => "medium",
            ConfidenceWord::Low => "low",
        }
    }
}

/// Typed diagnostic reason codes explaining confidence and parse decisions (`AC-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReasonCode {
    ExactMatch,
    StackedClusterMerged,
    MissingUnit,
    MissingTolerance,
    AmbiguousTolerance,
    PartialGdtFrame,
    IsoFitClass,
    NoteOnly,
}

impl ReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ReasonCode::ExactMatch => "exact_match",
            ReasonCode::StackedClusterMerged => "stacked_cluster_merged",
            ReasonCode::MissingUnit => "missing_unit",
            ReasonCode::MissingTolerance => "missing_tolerance",
            ReasonCode::AmbiguousTolerance => "ambiguous_tolerance",
            ReasonCode::PartialGdtFrame => "partial_gdt_frame",
            ReasonCode::IsoFitClass => "iso_fit_class",
            ReasonCode::NoteOnly => "note_only",
        }
    }
}

/// One parsed engineering drawing requirement candidate (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCallout {
    pub kind: CalloutKind,
    pub repeat_count: u32,
    pub nominal: Option<DecimalString>,
    pub upper: Option<DecimalString>,
    pub lower: Option<DecimalString>,
    pub unit_hint: Option<UnitHint>,
    pub gdt_zone_diameter: bool,
    pub datum_refs: Vec<Untrusted<Text>>,
    pub modifiers: Vec<Modifier>,
    pub raw_text: Untrusted<Text>,
    pub bbox: SheetRect,
    pub run_ids: Vec<u32>,
    pub confidence: ConfidenceWord,
    pub reasons: Vec<ReasonCode>,
}

impl ParsedCallout {
    pub fn to_value(&self) -> CValue {
        obj([
            ("bbox", self.bbox.to_value()),
            ("confidence", CValue::Str(self.confidence.as_str().into())),
            ("datum_refs", CValue::Arr(self.datum_refs.iter().map(Untrusted::to_value).collect())),
            ("gdt_symbol", self.kind.gdt_symbol().map_or(CValue::Null, |s| CValue::Str(s.as_str().into()))),
            ("gdt_zone_diameter", CValue::Bool(self.gdt_zone_diameter)),
            ("kind", CValue::Str(self.kind.as_str().into())),
            ("lower", self.lower.as_ref().map_or(CValue::Null, |d| CValue::Str(d.as_str().into()))),
            ("modifiers", CValue::Arr(self.modifiers.iter().map(|m| CValue::Str(m.as_str().into())).collect())),
            ("nominal", self.nominal.as_ref().map_or(CValue::Null, |d| CValue::Str(d.as_str().into()))),
            ("raw_text", self.raw_text.to_value()),
            ("reasons", CValue::Arr(self.reasons.iter().map(|r| CValue::Str(r.as_str().into())).collect())),
            ("repeat_count", CValue::Str(self.repeat_count.to_string())),
            ("run_ids", CValue::Arr(self.run_ids.iter().map(|id| CValue::Str(id.to_string())).collect())),
            ("unit_hint", self.unit_hint.map_or(CValue::Null, |u| CValue::Str(u.as_str().into()))),
            ("upper", self.upper.as_ref().map_or(CValue::Null, |d| CValue::Str(d.as_str().into()))),
        ])
    }
}

/// Title-block unit and default-tolerance hints extracted as untrusted metadata requiring human
/// confirmation (`Scope`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleBlockHints {
    pub unit_hint: Option<UnitHint>,
    pub default_tolerance_notes: Vec<Untrusted<Text>>,
}

/// Complete deterministic result of clustering and parsing a [`crate::balloon::auto_text::DrawingTextMap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDrawingResult {
    pub callouts: Vec<ParsedCallout>,
    pub excluded_run_ids: Vec<u32>,
    pub title_block_hints: TitleBlockHints,
}
