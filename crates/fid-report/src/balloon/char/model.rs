//! Data structures, authored-literal SI conversion (`unit.authored_to_si@1`), and errors
//! for `RPT.BALLOON.CHAR` (`fid_report::balloon::char`).
//!
//! Pure integer / exact `fid_num::kdec::Decimal` arithmetic: zero floating-point types or ops.

use crate::balloon::auto_parse::DecimalString;
use crate::balloon::register::RegisterError;
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_gdt::chars::spec::{PlanId, SpecError};
use fid_num::kdec::{cmp_dec, Decimal};
use std::cmp::Ordering;
use std::fmt;

/// Versioned algorithm ID for authored-literal to SI conversion (`REQ-003`, CONSTITUTION §4).
pub const UNIT_CONVERSION_ALGO: &str = "unit.authored.si@1.0.0";

/// Maximum UTF-8 byte length for optional characteristic names and datum labels (C6).
pub const MAX_CHAR_TEXT_BYTES: usize = 256;

/// Explicit governing GD&T standard dialect required by `RPT.BALLOON.CHAR` (`REQ-004`, `AC-4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dialect {
    Asme,
    Iso,
}

impl Dialect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Dialect::Asme => "ASME",
            Dialect::Iso => "ISO",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "ASME" | "ASME_Y14_5" | "ASME Y14.5" => Some(Dialect::Asme),
            "ISO" | "ISO_GPS" | "ISO GPS" => Some(Dialect::Iso),
            _ => None,
        }
    }
}

/// Authored measurement unit for drawing callout quantities (`REQ-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthoredUnit {
    Mm,
    Inch,
    Um,
    M,
    Deg,
    Rad,
}

impl AuthoredUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthoredUnit::Mm => "mm",
            AuthoredUnit::Inch => "in",
            AuthoredUnit::Um => "um",
            AuthoredUnit::M => "m",
            AuthoredUnit::Deg => "deg",
            AuthoredUnit::Rad => "rad",
        }
    }

    pub const fn is_angular(self) -> bool {
        matches!(self, AuthoredUnit::Deg | AuthoredUnit::Rad)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mm" => Some(AuthoredUnit::Mm),
            "in" | "inch" | "\"" => Some(AuthoredUnit::Inch),
            "um" | "µm" | "μm" => Some(AuthoredUnit::Um),
            "m" => Some(AuthoredUnit::M),
            "deg" | "°" => Some(AuthoredUnit::Deg),
            "rad" => Some(AuthoredUnit::Rad),
            _ => None,
        }
    }

    /// Exact conversion factor to SI (`m` for length, `rad` for angle) as `Decimal`.
    fn to_si_factor(self) -> &'static str {
        match self {
            AuthoredUnit::Mm => "0.001",
            AuthoredUnit::Inch => "0.0254",
            AuthoredUnit::Um => "0.000001",
            AuthoredUnit::M => "1",
            AuthoredUnit::Deg => "0.017453292519943295",
            AuthoredUnit::Rad => "1",
        }
    }
}

/// Authored decimal quantity preserving exact verbatim authored text alongside its one-rounding
/// SI decimal value (`REQ-003`, `AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredQuantity {
    pub raw_decimal: DecimalString,
    pub unit: AuthoredUnit,
    /// Exact decimal value in SI (`m` or `rad`), computed once via `UNIT_CONVERSION_ALGO`.
    pub si_decimal: String,
}

impl AuthoredQuantity {
    /// Constructs an [`AuthoredQuantity`] from raw decimal text + unit (e.g. `"12.000"`, `Mm`).
    pub fn new(raw_decimal: &str, unit: AuthoredUnit) -> Result<Self, CharBalloonError> {
        let dec_str = DecimalString::parse(raw_decimal)
            .ok_or(CharBalloonError::GdtValidation("invalid_decimal", "nominal/tolerance is not a valid decimal"))?;
        let d = parse_kdec(dec_str.as_str())?;
        let factor = Decimal::parse(unit.to_si_factor())
            .map_err(|_| CharBalloonError::Store("invalid internal unit factor".into()))?;
        let si = d
            .mul(&factor)
            .map_err(|_| CharBalloonError::GdtValidation("numeric_overflow", "SI conversion overflow"))?;
        Ok(AuthoredQuantity { raw_decimal: dec_str, unit, si_decimal: si.to_string() })
    }

    /// Parses a combined authored literal string such as `"12.000mm"`, `"+0.018mm"`, `"0mm"`,
    /// or `"45.0 deg"`.
    pub fn parse_literal(s: &str) -> Result<Self, CharBalloonError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(CharBalloonError::GdtValidation("empty_quantity", "empty authored quantity"));
        }
        // Split trailing unit letters/symbols from leading numeric portion.
        let split_at = trimmed
            .char_indices()
            .find(|&(_, c)| !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | ' ')))
            .map(|(idx, _)| idx)
            .ok_or(CharBalloonError::GdtValidation("missing_unit", "authored quantity requires explicit unit"))?;
        let num_part = trimmed[..split_at].trim();
        let unit_part = trimmed[split_at..].trim();
        let unit = AuthoredUnit::parse(unit_part)
            .ok_or(CharBalloonError::GdtValidation("unsupported_unit", "unknown unit suffix"))?;
        AuthoredQuantity::new(num_part, unit)
    }

    /// Human-readable verbatim authored literal (e.g. `"12.000 mm"`).
    pub fn authored_literal(&self) -> String {
        format!("{} {}", self.raw_decimal.as_str(), self.unit.as_str())
    }

    pub fn to_value(&self) -> CValue {
        obj([
            ("authored", CValue::Str(self.raw_decimal.as_str().into())),
            ("si", CValue::Str(self.si_decimal.clone())),
            ("unit", CValue::Str(self.unit.as_str().into())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, CharBalloonError> {
        let CValue::Obj(m) = v else {
            return Err(CharBalloonError::BadParams("quantity must be an object"));
        };
        let raw = match m.get("authored") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(CharBalloonError::BadParams("missing quantity.authored")),
        };
        let unit_s = match m.get("unit") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(CharBalloonError::BadParams("missing quantity.unit")),
        };
        let unit = AuthoredUnit::parse(unit_s).ok_or(CharBalloonError::BadParams("unsupported quantity.unit"))?;
        AuthoredQuantity::new(raw, unit)
    }

    pub fn cmp_si(&self, other: &Self) -> Result<Ordering, CharBalloonError> {
        if self.unit.is_angular() != other.unit.is_angular() {
            return Err(CharBalloonError::GdtValidation("unit_mismatch", "cannot compare length and angular units"));
        }
        let a = parse_kdec(&self.si_decimal)?;
        let b = parse_kdec(&other.si_decimal)?;
        Ok(cmp_dec(&a, &b))
    }
}

fn parse_kdec(s: &str) -> Result<Decimal, CharBalloonError> {
    let clean = s.trim().strip_prefix('+').unwrap_or(s.trim());
    Decimal::parse(clean).map_err(|_| CharBalloonError::GdtValidation("invalid_decimal", "decimal parse failed"))
}

/// Drawing-only subset of characteristic kinds supported by `GDT.CHAR.MODEL.TYPES` (`REQ-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DrawingCharKind {
    Linear,
    Diameter,
    Radius,
    Distance,
    Angle,
    Flatness,
    Straightness,
    Circularity,
    Cylindricity,
    Perpendicularity,
    Parallelism,
    Angularity,
    Position,
    Concentricity,
    Symmetry,
    ProfileSurface,
    ProfileLine,
    CircularRunout,
    TotalRunout,
}

impl DrawingCharKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            DrawingCharKind::Linear => "linear",
            DrawingCharKind::Diameter => "diameter",
            DrawingCharKind::Radius => "radius",
            DrawingCharKind::Distance => "distance",
            DrawingCharKind::Angle => "angle",
            DrawingCharKind::Flatness => "flatness",
            DrawingCharKind::Straightness => "straightness",
            DrawingCharKind::Circularity => "circularity",
            DrawingCharKind::Cylindricity => "cylindricity",
            DrawingCharKind::Perpendicularity => "perpendicularity",
            DrawingCharKind::Parallelism => "parallelism",
            DrawingCharKind::Angularity => "angularity",
            DrawingCharKind::Position => "position",
            DrawingCharKind::Concentricity => "concentricity",
            DrawingCharKind::Symmetry => "symmetry",
            DrawingCharKind::ProfileSurface => "profile_surface",
            DrawingCharKind::ProfileLine => "profile_line",
            DrawingCharKind::CircularRunout => "circular_runout",
            DrawingCharKind::TotalRunout => "total_runout",
        }
    }

    pub fn parse(s: &str) -> Result<Self, CharBalloonError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "linear" | "size" => Ok(DrawingCharKind::Linear),
            "diameter" => Ok(DrawingCharKind::Diameter),
            "radius" => Ok(DrawingCharKind::Radius),
            "distance" => Ok(DrawingCharKind::Distance),
            "angle" => Ok(DrawingCharKind::Angle),
            "flatness" => Ok(DrawingCharKind::Flatness),
            "straightness" => Ok(DrawingCharKind::Straightness),
            "circularity" => Ok(DrawingCharKind::Circularity),
            "cylindricity" => Ok(DrawingCharKind::Cylindricity),
            "perpendicularity" => Ok(DrawingCharKind::Perpendicularity),
            "parallelism" => Ok(DrawingCharKind::Parallelism),
            "angularity" => Ok(DrawingCharKind::Angularity),
            "position" => Ok(DrawingCharKind::Position),
            "concentricity" => Ok(DrawingCharKind::Concentricity),
            "symmetry" => Ok(DrawingCharKind::Symmetry),
            "profile_surface" => Ok(DrawingCharKind::ProfileSurface),
            "profile_line" => Ok(DrawingCharKind::ProfileLine),
            "circular_runout" => Ok(DrawingCharKind::CircularRunout),
            "total_runout" => Ok(DrawingCharKind::TotalRunout),
            other => Err(CharBalloonError::CharTypeUnsupported { kind: other.to_string() }),
        }
    }
}

/// Input specification for creating a characteristic from a drawing callout (`Design`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingCharInput {
    pub kind: DrawingCharKind,
    pub nominal: AuthoredQuantity,
    pub upper: AuthoredQuantity,
    pub lower: AuthoredQuantity,
    /// Explicit governing dialect; `None` fails with `report.dialect_missing` (`REQ-004`, `AC-4`).
    pub dialect: Option<Dialect>,
    pub datum_refs: Vec<Untrusted<Text>>,
    pub name: Option<Untrusted<Text>>,
}

/// Created characteristic stored by `RPT.BALLOON.CHAR` with verbatim authored literals (`AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingCharacteristic {
    pub id: CharId,
    pub plan: PlanId,
    pub kind: DrawingCharKind,
    pub nominal: AuthoredQuantity,
    pub upper: AuthoredQuantity,
    pub lower: AuthoredQuantity,
    pub dialect: Dialect,
    pub datum_refs: Vec<Untrusted<Text>>,
    pub name: Option<Untrusted<Text>>,
    pub unit_algo: String,
}

impl DrawingCharacteristic {
    pub fn to_value(&self) -> CValue {
        obj([
            ("datum_refs", CValue::Arr(self.datum_refs.iter().map(Untrusted::to_value).collect())),
            ("dialect", CValue::Str(self.dialect.as_str().into())),
            ("id", CValue::Str(self.id.as_str().into())),
            ("kind", CValue::Str(self.kind.as_str().into())),
            ("lower", self.lower.to_value()),
            ("name", self.name.as_ref().map_or(CValue::Null, Untrusted::to_value)),
            ("nominal", self.nominal.to_value()),
            ("plan", CValue::Str(self.plan.as_str().into())),
            ("unit_algo", CValue::Str(self.unit_algo.clone())),
            ("upper", self.upper.to_value()),
        ])
    }
}

/// Typed errors of `report.add_char_balloon@1` (`Design: Errors`, `REQ-002`, `REQ-004`, `REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharBalloonError {
    /// `report.dialect_missing`: dialect was omitted (`REQ-004`, `AC-4`).
    DialectMissing,
    /// `report.char_type_unsupported`: requested kind is outside the drawing-only GDT subset (`REQ-005`).
    CharTypeUnsupported { kind: String },
    /// GDT characteristic validation failure passed through unchanged (`REQ-002`, `AC-3`).
    GdtValidation(&'static str, &'static str),
    /// GDT `SpecState` error passed through unchanged (`REQ-002`).
    GdtSpec(SpecError),
    /// Balloon register error passed through unchanged (`REQ-002`, `AC-4`).
    Register(RegisterError),
    /// Invalid text (C6).
    TextInvalid(&'static str),
    /// Malformed params.
    BadParams(&'static str),
    /// Store or ID failure.
    Store(String),
}

impl CharBalloonError {
    pub fn code(&self) -> &'static str {
        match self {
            CharBalloonError::DialectMissing => "report.dialect_missing",
            CharBalloonError::CharTypeUnsupported { .. } => "report.char_type_unsupported",
            CharBalloonError::GdtValidation(code, _) => code,
            CharBalloonError::GdtSpec(e) => e.code(),
            CharBalloonError::Register(e) => e.code(),
            CharBalloonError::TextInvalid(_) => "report.text_invalid",
            CharBalloonError::BadParams(_) => "report.bad_params",
            CharBalloonError::Store(_) => "report.store_invalid",
        }
    }
}

impl From<RegisterError> for CharBalloonError {
    fn from(e: RegisterError) -> Self {
        CharBalloonError::Register(e)
    }
}

impl From<SpecError> for CharBalloonError {
    fn from(e: SpecError) -> Self {
        CharBalloonError::GdtSpec(e)
    }
}

impl fmt::Display for CharBalloonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CharBalloonError::DialectMissing => {
                write!(f, "report.dialect_missing: explicit dialect (ASME or ISO) required")
            }
            CharBalloonError::CharTypeUnsupported { kind } => {
                write!(f, "report.char_type_unsupported: unsupported drawing characteristic kind '{kind}'")
            }
            CharBalloonError::GdtValidation(code, detail) => write!(f, "gdt.{code}: {detail}"),
            CharBalloonError::GdtSpec(e) => write!(f, "{e}"),
            CharBalloonError::Register(e) => write!(f, "{e}"),
            CharBalloonError::TextInvalid(r) => write!(f, "report.text_invalid: {r}"),
            CharBalloonError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            CharBalloonError::Store(d) => write!(f, "report.store_invalid: {d}"),
        }
    }
}

impl std::error::Error for CharBalloonError {}
