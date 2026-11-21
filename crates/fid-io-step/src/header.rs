//! STEP header and unit-context resolution (spec node `IO.CAD.STEP.HDR`).
//!
//! [`resolve_header`] reads a parsed [`P21File`]: it checks `FILE_SCHEMA` against a fixed
//! table of accepted schemas, captures `FILE_NAME` and `FILE_DESCRIPTION` as untrusted
//! text, and resolves, for every geometric representation context, the length unit (to
//! one exact factor to metres), the plane-angle unit and the declared length uncertainty.
//!
//! Everything read from the file is untrusted. Header text is kept as [`StepText`] and is
//! only ever shown through [`fid_io_reg::UntrustedText::as_display`]; it is never
//! interpreted. The walk is bounded: the number of contexts, units and uncertainties per
//! context, and the total header text are limited ([`HeaderOptions`]), and unit references
//! are followed at most two levels (a conversion-based unit, its measure, the SI base unit).
//! There is no recursion and no panic; every refusal is a typed [`HeaderError`].

use crate::{Instance, P21File, Param, StepText};
use fid_io_reg::prov::LengthUnit;
use fid_io_reg::{
    resolve_unit, Code, IoError, Position, ProvError, ProvenanceBuilder, ReportBuilder, ReportError, ReportPos,
    UnitResolution, UnitSource, UntrustedText,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Default limit on the number of geometric representation contexts.
pub const DEFAULT_MAX_CONTEXTS: usize = 100_000;
/// Default limit on the total header text (all `FILE_NAME`, `FILE_DESCRIPTION` and
/// `FILE_SCHEMA` strings together), in UTF-8 bytes.
pub const DEFAULT_MAX_HEADER_TEXT_BYTES: usize = 64 * 1024;
/// Limit on the units listed by one `global_unit_assigned_context`.
pub const MAX_UNITS_PER_CONTEXT: usize = 64;
/// Limit on the uncertainties listed by one `global_uncertainty_assigned_context`.
pub const MAX_UNCERTAINTIES_PER_CONTEXT: usize = 64;
/// Limit on the items of one header string list (`author`, `organization`, `description`).
pub const MAX_HEADER_LIST_ITEMS: usize = 256;
/// Relative difference above which a conversion-based length unit's file factor is
/// reported (REQ-IO.CAD.STEP.HDR-002).
pub const LENGTH_FACTOR_TOLERANCE: f64 = 1e-12;
/// Relative difference above which a conversion-based degree's file factor is reported.
pub const ANGLE_FACTOR_TOLERANCE: f64 = 1e-6;
/// The f64 nearest to pi/180, the radians in one degree.
pub const DEGREE_IN_RADIANS: f64 = 0.017_453_292_519_943_295;

/// An accepted application-protocol schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StepSchema {
    /// AP242, managed model based 3D engineering.
    Ap242,
    /// AP214, automotive design.
    Ap214,
    /// AP203, configuration controlled design (both editions).
    Ap203,
}

impl StepSchema {
    /// The stable wire name: `ap242`, `ap214` or `ap203`.
    pub fn as_str(self) -> &'static str {
        match self {
            StepSchema::Ap242 => "ap242",
            StepSchema::Ap214 => "ap214",
            StepSchema::Ap203 => "ap203",
        }
    }
}

/// The fixed identifier table: an identifier is accepted when, trimmed of ASCII whitespace
/// and ASCII upper-cased, it starts with one of these prefixes and the next character is
/// the end, a space, `{` or `_` [unverified: identifiers to confirm against CAx-IF files].
pub const SCHEMA_TABLE: &[(&str, StepSchema)] = &[
    ("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING", StepSchema::Ap242),
    ("AUTOMOTIVE_DESIGN", StepSchema::Ap214),
    ("CONFIG_CONTROL_DESIGN", StepSchema::Ap203),
    ("AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES", StepSchema::Ap203),
];

/// Matches one `FILE_SCHEMA` identifier against [`SCHEMA_TABLE`].
pub fn match_schema(identifier: &str) -> Option<StepSchema> {
    let upper = identifier.trim_matches(|c: char| c.is_ascii_whitespace()).to_ascii_uppercase();
    SCHEMA_TABLE.iter().find_map(|(prefix, schema)| {
        let rest = upper.strip_prefix(prefix)?;
        match rest.as_bytes().first() {
            None | Some(b' ' | b'{' | b'_') => Some(*schema),
            Some(_) => None,
        }
    })
}

/// A length unit a context can resolve to. `Cm` is the one unit here that
/// `fid_geo_frame::LengthUnit` lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StepLengthUnit {
    /// Metre (SI, no prefix).
    M,
    /// Centimetre (SI, prefix centi).
    Cm,
    /// Millimetre (SI, prefix milli).
    Mm,
    /// Micrometre (SI, prefix micro).
    Um,
    /// Nanometre (SI, prefix nano).
    Nm,
    /// Inch (conversion based), exactly 0.0254 m.
    In,
    /// Foot (conversion based), exactly 0.3048 m.
    Ft,
    /// Thou, exactly 0.0000254 m (only as a caller unit).
    Thou,
}

impl StepLengthUnit {
    /// The ASCII symbol.
    pub fn symbol(self) -> &'static str {
        match self {
            StepLengthUnit::M => "m",
            StepLengthUnit::Cm => "cm",
            StepLengthUnit::Mm => "mm",
            StepLengthUnit::Um => "um",
            StepLengthUnit::Nm => "nm",
            StepLengthUnit::In => "in",
            StepLengthUnit::Ft => "ft",
            StepLengthUnit::Thou => "thou",
        }
    }

    /// The f64 nearest to the exact factor to metres (each literal is correctly rounded).
    pub fn factor_m(self) -> f64 {
        match self {
            StepLengthUnit::M => 1.0,
            StepLengthUnit::Cm => 0.01,
            StepLengthUnit::Mm => 0.001,
            StepLengthUnit::Um => 0.000_001,
            StepLengthUnit::Nm => 0.000_000_001,
            StepLengthUnit::In => 0.0254,
            StepLengthUnit::Ft => 0.3048,
            StepLengthUnit::Thou => 0.000_025_4,
        }
    }

    /// The exact factor to metres as a terminating decimal.
    pub fn factor_decimal(self) -> &'static str {
        match self {
            StepLengthUnit::M => "1",
            StepLengthUnit::Cm => "0.01",
            StepLengthUnit::Mm => "0.001",
            StepLengthUnit::Um => "0.000001",
            StepLengthUnit::Nm => "0.000000001",
            StepLengthUnit::In => "0.0254",
            StepLengthUnit::Ft => "0.3048",
            StepLengthUnit::Thou => "0.0000254",
        }
    }

    /// The `IO.REG.PROV` unit, if there is one (`Cm` has none).
    pub fn to_length_unit(self) -> Option<LengthUnit> {
        match self {
            StepLengthUnit::M => Some(LengthUnit::M),
            StepLengthUnit::Cm => None,
            StepLengthUnit::Mm => Some(LengthUnit::Mm),
            StepLengthUnit::Um => Some(LengthUnit::Um),
            StepLengthUnit::Nm => Some(LengthUnit::Nm),
            StepLengthUnit::In => Some(LengthUnit::In),
            StepLengthUnit::Ft => Some(LengthUnit::Ft),
            StepLengthUnit::Thou => Some(LengthUnit::Thou),
        }
    }
}

impl From<LengthUnit> for StepLengthUnit {
    fn from(u: LengthUnit) -> StepLengthUnit {
        match u {
            LengthUnit::M => StepLengthUnit::M,
            LengthUnit::Mm => StepLengthUnit::Mm,
            LengthUnit::Um => StepLengthUnit::Um,
            LengthUnit::Nm => StepLengthUnit::Nm,
            LengthUnit::In => StepLengthUnit::In,
            LengthUnit::Ft => StepLengthUnit::Ft,
            LengthUnit::Thou => StepLengthUnit::Thou,
        }
    }
}

/// A plane-angle unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AngleUnit {
    /// SI radian.
    Radian,
    /// Conversion-based degree (pi/180 rad).
    Degree,
}

impl AngleUnit {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            AngleUnit::Radian => "radian",
            AngleUnit::Degree => "degree",
        }
    }

    /// The f64 nearest to the factor to radians.
    pub fn factor_rad(self) -> f64 {
        match self {
            AngleUnit::Radian => 1.0,
            AngleUnit::Degree => DEGREE_IN_RADIANS,
        }
    }
}

/// A context's declared length uncertainty, in the file's units.
#[derive(Debug, Clone, PartialEq)]
pub struct Uncertainty {
    /// The value as parsed.
    pub value: f64,
    /// The authored token (a real lexeme or an integer), for provenance.
    pub authored: String,
    /// The unit the value is expressed in.
    pub unit: StepLengthUnit,
    /// The f64 nearest to that unit's exact factor to metres.
    pub length_factor_m: f64,
}

/// The resolved units of one geometric representation context.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitContext {
    /// The context's entity number.
    pub context: u64,
    /// The effective length unit.
    pub length_unit: StepLengthUnit,
    /// The f64 nearest to the effective unit's exact factor to metres.
    pub length_factor_m: f64,
    /// The length unit the file declared, if any.
    pub declared: Option<StepLengthUnit>,
    /// The plane-angle unit the file declared, if any.
    pub angle: Option<AngleUnit>,
    /// The declared length uncertainty, if any.
    pub uncertainty: Option<Uncertainty>,
    /// Where the effective length unit came from.
    pub source: UnitSource,
    /// True when another instance refers to the context (it owns geometry).
    pub owns_geometry: bool,
}

/// `FILE_NAME`, as untrusted text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileName {
    /// `name`.
    pub name: StepText,
    /// `time_stamp`.
    pub time_stamp: StepText,
    /// `author`.
    pub author: Vec<StepText>,
    /// `organization`.
    pub organization: Vec<StepText>,
    /// `preprocessor_version`.
    pub preprocessor_version: StepText,
    /// `authorization`.
    pub authorization: StepText,
}

/// `FILE_DESCRIPTION`, as untrusted text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDescription {
    /// `description`.
    pub description: Vec<StepText>,
    /// `implementation_level`.
    pub implementation_level: StepText,
}

/// A conversion-based unit whose file factor differs from the known exact one. The exact
/// factor is used (REQ-IO.CAD.STEP.HDR-002).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HeaderWarning {
    /// The conversion-based unit's entity number.
    pub entity: u64,
    /// `length_factor_mismatch` or `angle_factor_mismatch`.
    pub code: &'static str,
    /// The authored factor token of the file.
    pub authored: String,
    /// The exact factor used, as a decimal (`0.0254`, `0.3048`) or `pi/180`.
    pub exact: &'static str,
    /// Byte offset of the unit instance.
    pub offset: u64,
}

/// The resolved header.
#[derive(Debug, Clone, PartialEq)]
pub struct StepHeaderInfo {
    /// The accepted schema.
    pub schema: StepSchema,
    /// The `FILE_SCHEMA` identifier as written.
    pub raw_schema: StepText,
    /// `FILE_NAME.originating_system`.
    pub originating_system: StepText,
    /// The rest of `FILE_NAME`.
    pub file_name: FileName,
    /// `FILE_DESCRIPTION`.
    pub description: FileDescription,
    /// Every context with a length unit (declared or given), by entity number.
    pub contexts: BTreeMap<u64, UnitContext>,
    /// Contexts that declare no length unit, own no geometry, and got no caller unit.
    pub unitless: Vec<u64>,
    /// Factor warnings, sorted by entity and code, without repeats.
    pub warnings: Vec<HeaderWarning>,
}

/// Caller choices and bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderOptions {
    /// The caller's `unit`, used where a context declares none (REQ-IO.CAD.STEP.HDR-003).
    pub unit: Option<LengthUnit>,
    /// `override_unit` of `IO.REG.PROV`: the caller's unit replaces a declared one.
    pub override_unit: bool,
    /// Maximum number of geometric representation contexts.
    pub max_contexts: usize,
    /// Maximum total header text, in UTF-8 bytes.
    pub max_header_text_bytes: usize,
}

impl Default for HeaderOptions {
    fn default() -> Self {
        HeaderOptions {
            unit: None,
            override_unit: false,
            max_contexts: DEFAULT_MAX_CONTEXTS,
            max_header_text_bytes: DEFAULT_MAX_HEADER_TEXT_BYTES,
        }
    }
}

/// Which header bound a [`HeaderError::LimitExceeded`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeaderBound {
    /// [`HeaderOptions::max_contexts`].
    Contexts,
    /// [`HeaderOptions::max_header_text_bytes`].
    HeaderText,
    /// [`MAX_HEADER_LIST_ITEMS`].
    HeaderList,
    /// [`MAX_UNITS_PER_CONTEXT`].
    Units,
    /// [`MAX_UNCERTAINTIES_PER_CONTEXT`].
    Uncertainties,
}

impl HeaderBound {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            HeaderBound::Contexts => "contexts",
            HeaderBound::HeaderText => "header_text",
            HeaderBound::HeaderList => "header_list",
            HeaderBound::Units => "units",
            HeaderBound::Uncertainties => "uncertainties",
        }
    }
}

/// Every refusal of [`resolve_header`]. Reasons are static strings chosen here; file text
/// appears only as [`UntrustedText`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    /// `FILE_SCHEMA` names no accepted schema (REQ-IO.CAD.STEP.HDR-001).
    SchemaUnsupported {
        /// The identifier as written.
        raw: UntrustedText,
    },
    /// A context owning geometry declares no length unit and the caller gave none, or
    /// `override_unit` was set without a unit (REQ-IO.CAD.STEP.HDR-003).
    UnitRequired {
        /// The context, if one is concerned.
        context: Option<u64>,
    },
    /// The caller's unit differs from a declared one and `override_unit` is not set.
    UnitConflict {
        /// The context.
        context: u64,
    },
    /// A unit this reader does not resolve (an unknown conversion-based name, an SI prefix
    /// outside the table, or a centimetre context where `IO.REG.PROV` needs a unit).
    UnitUnsupported {
        /// The unit's (or context's) entity number.
        entity: u64,
    },
    /// The header or a unit structure breaks the rules this reader checks.
    Malformed {
        /// The instance concerned, if any.
        entity: Option<u64>,
        /// Static reason.
        reason: &'static str,
    },
    /// A bound was exceeded.
    LimitExceeded {
        /// The bound.
        bound: HeaderBound,
        /// Its value.
        value: u64,
    },
}

impl HeaderError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            HeaderError::SchemaUnsupported { .. } => "step_schema_unsupported",
            HeaderError::UnitRequired { .. } => "unit_required",
            HeaderError::UnitConflict { .. } => "unit_conflict",
            HeaderError::UnitUnsupported { .. } => "step_unit_unsupported",
            HeaderError::Malformed { .. } => "malformed",
            HeaderError::LimitExceeded { .. } => "limit_exceeded",
        }
    }

    /// The entity number concerned, if any.
    pub fn entity(&self) -> Option<u64> {
        match self {
            HeaderError::UnitRequired { context } => *context,
            HeaderError::UnitConflict { context } => Some(*context),
            HeaderError::UnitUnsupported { entity } => Some(*entity),
            HeaderError::Malformed { entity, .. } => *entity,
            HeaderError::SchemaUnsupported { .. } | HeaderError::LimitExceeded { .. } => None,
        }
    }
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            HeaderError::SchemaUnsupported { raw } => {
                let shown = raw.as_display().to_string();
                write!(f, ": \"{}\"", shown.replace('"', "\\\""))
            }
            HeaderError::LimitExceeded { bound, value } => write!(f, ": bound {} = {value}", bound.as_str()),
            HeaderError::Malformed { entity: Some(n), reason } => write!(f, ": {reason} (#{n})"),
            HeaderError::Malformed { entity: None, reason } => write!(f, ": {reason}"),
            other => match other.entity() {
                Some(n) => write!(f, ": #{n}"),
                None => Ok(()),
            },
        }
    }
}

impl std::error::Error for HeaderError {}

fn bad(entity: Option<u64>, reason: &'static str) -> HeaderError {
    HeaderError::Malformed { entity, reason }
}

fn limit(bound: HeaderBound, value: usize) -> HeaderError {
    HeaderError::LimitExceeded { bound, value: u64::try_from(value).unwrap_or(u64::MAX) }
}

// ------------------------------------------------------------------ header records

struct TextBudget {
    left: usize,
    max: usize,
}

impl TextBudget {
    fn take(&mut self, t: &StepText) -> Result<(), HeaderError> {
        match self.left.checked_sub(t.len()) {
            Some(rest) => {
                self.left = rest;
                Ok(())
            }
            None => Err(limit(HeaderBound::HeaderText, self.max)),
        }
    }
}

fn header_record<'f>(file: &'f P21File, name: &str, missing: &'static str) -> Result<&'f [Param], HeaderError> {
    let mut found: Option<&[Param]> = None;
    for inst in &file.header {
        for (n, params) in &inst.parts {
            if n.as_str() == name {
                if found.is_some() {
                    return Err(bad(None, "duplicate_header_record"));
                }
                found = Some(params);
            }
        }
    }
    found.ok_or_else(|| bad(None, missing))
}

fn text(p: &Param, budget: &mut TextBudget) -> Result<StepText, HeaderError> {
    match p {
        Param::Str(s) => {
            budget.take(s)?;
            Ok(s.clone())
        }
        Param::Unset => Ok(StepText::default()),
        _ => Err(bad(None, "header_string_expected")),
    }
}

fn text_list(p: &Param, budget: &mut TextBudget) -> Result<Vec<StepText>, HeaderError> {
    match p {
        Param::List(items) => {
            if items.len() > MAX_HEADER_LIST_ITEMS {
                return Err(limit(HeaderBound::HeaderList, MAX_HEADER_LIST_ITEMS));
            }
            items.iter().map(|i| text(i, budget)).collect()
        }
        Param::Unset => Ok(Vec::new()),
        _ => Err(bad(None, "header_list_expected")),
    }
}

// ------------------------------------------------------------------ units

fn part<'i>(inst: &'i Instance, name: &str) -> Option<&'i [Param]> {
    inst.parts.iter().find(|(n, _)| n.as_str() == name).map(|(_, p)| p.as_slice())
}

fn has(inst: &Instance, name: &str) -> bool {
    inst.parts.iter().any(|(n, _)| n.as_str() == name)
}

fn get(file: &P21File, n: u64) -> Result<&Instance, HeaderError> {
    file.get(n).ok_or_else(|| bad(Some(n), "dangling_unit_reference"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Length(StepLengthUnit),
    Angle(AngleUnit),
    Other,
}

/// The two attributes `(prefix, name)` of an `SI_UNIT` part: the complex-instance form has
/// exactly these, the simple-instance form starts with the derived dimensions.
fn si_attrs(n: u64, params: &[Param]) -> Result<(Option<&str>, &str), HeaderError> {
    let (prefix, name) = match params {
        [p, q] | [_, p, q] => (p, q),
        _ => return Err(bad(Some(n), "si_unit_arity")),
    };
    let prefix = match prefix {
        Param::Unset => None,
        Param::Enum(e) => Some(e.as_str()),
        _ => return Err(bad(Some(n), "si_unit_prefix_type")),
    };
    match name {
        Param::Enum(e) => Ok((prefix, e.as_str())),
        _ => Err(bad(Some(n), "si_unit_name_type")),
    }
}

fn classify_si(n: u64, inst: &Instance, params: &[Param]) -> Result<Kind, HeaderError> {
    let is_length = has(inst, "LENGTH_UNIT");
    let is_angle = has(inst, "PLANE_ANGLE_UNIT");
    let (prefix, name) = si_attrs(n, params)?;
    match name {
        "METRE" => {
            if is_angle {
                return Err(bad(Some(n), "unit_kind_conflict"));
            }
            let unit = match prefix {
                None => StepLengthUnit::M,
                Some("CENTI") => StepLengthUnit::Cm,
                Some("MILLI") => StepLengthUnit::Mm,
                Some("MICRO") => StepLengthUnit::Um,
                Some("NANO") => StepLengthUnit::Nm,
                Some(_) => return Err(HeaderError::UnitUnsupported { entity: n }),
            };
            Ok(Kind::Length(unit))
        }
        "RADIAN" => {
            if is_length {
                return Err(bad(Some(n), "unit_kind_conflict"));
            }
            match prefix {
                None => Ok(Kind::Angle(AngleUnit::Radian)),
                Some(_) => Err(HeaderError::UnitUnsupported { entity: n }),
            }
        }
        _ if is_length || is_angle => Err(bad(Some(n), "unit_kind_conflict")),
        _ => Ok(Kind::Other),
    }
}

/// `(value, authored token, unit reference)` of a measure-with-unit instance.
fn measure(file: &P21File, n: u64) -> Result<(f64, String, u64), HeaderError> {
    let inst = get(file, n)?;
    for (name, params) in &inst.parts {
        if !name.as_str().ends_with("MEASURE_WITH_UNIT") {
            continue;
        }
        let (value, unit) = match params.as_slice() {
            [v, Param::Ref(u), ..] => (v, *u),
            _ => continue,
        };
        let inner = match value {
            Param::Typed(t, inner) if t.as_str().ends_with("_MEASURE") => &**inner,
            other => other,
        };
        let (v, authored) = match inner {
            Param::Real(v, Some(lex)) => (*v, lex.as_str().to_owned()),
            Param::Real(v, None) => (*v, format!("{v:?}")),
            Param::Int(i) => (*i as f64, i.to_string()),
            _ => return Err(bad(Some(n), "measure_value_type")),
        };
        if !v.is_finite() {
            return Err(bad(Some(n), "measure_value_not_finite"));
        }
        return Ok((v, authored, unit));
    }
    Err(bad(Some(n), "measure_with_unit_expected"))
}

fn relative_difference(file_factor: f64, exact: f64) -> f64 {
    ((file_factor - exact) / exact).abs()
}

struct Walk<'f> {
    file: &'f P21File,
    warnings: BTreeSet<HeaderWarning>,
}

impl Walk<'_> {
    /// Classifies unit instance `n`. A conversion-based unit is followed to its measure and
    /// that measure's unit, which must be an SI unit (two levels, no recursion).
    fn classify(&mut self, n: u64) -> Result<Kind, HeaderError> {
        let file = self.file;
        let inst = get(file, n)?;
        let si = part(inst, "SI_UNIT");
        let conv = part(inst, "CONVERSION_BASED_UNIT");
        let is_length = has(inst, "LENGTH_UNIT");
        let is_angle = has(inst, "PLANE_ANGLE_UNIT");
        if is_length && is_angle {
            return Err(bad(Some(n), "unit_kind_conflict"));
        }
        match (si, conv) {
            (Some(_), Some(_)) => Err(bad(Some(n), "unit_kind_conflict")),
            (Some(params), None) => classify_si(n, inst, params),
            (None, Some(params)) => {
                if !is_length && !is_angle {
                    return Ok(Kind::Other);
                }
                let (name, factor) = match params {
                    [Param::Str(s), Param::Ref(r)] | [_, Param::Str(s), Param::Ref(r)] => (s, *r),
                    _ => return Err(bad(Some(n), "conversion_based_unit_attributes")),
                };
                let word = name.raw().to_ascii_uppercase();
                let (value, authored, base_n) = measure(file, factor)?;
                let base_inst = get(file, base_n)?;
                let base = match part(base_inst, "SI_UNIT") {
                    Some(p) if !has(base_inst, "CONVERSION_BASED_UNIT") => classify_si(base_n, base_inst, p)?,
                    _ => return Err(bad(Some(base_n), "conversion_base_not_si")),
                };
                if is_length {
                    let unit = match word.as_str() {
                        "INCH" => StepLengthUnit::In,
                        "FOOT" => StepLengthUnit::Ft,
                        _ => return Err(HeaderError::UnitUnsupported { entity: n }),
                    };
                    let base_unit = match base {
                        Kind::Length(u) => u,
                        _ => return Err(bad(Some(n), "conversion_base_kind")),
                    };
                    if value <= 0.0 {
                        return Err(bad(Some(factor), "conversion_factor_not_positive"));
                    }
                    let exact = unit.factor_m();
                    if relative_difference(value * base_unit.factor_m(), exact) > LENGTH_FACTOR_TOLERANCE {
                        self.warnings.insert(HeaderWarning {
                            entity: n,
                            code: "length_factor_mismatch",
                            authored,
                            exact: unit.factor_decimal(),
                            offset: inst.offset,
                        });
                    }
                    Ok(Kind::Length(unit))
                } else {
                    if word != "DEGREE" {
                        return Err(HeaderError::UnitUnsupported { entity: n });
                    }
                    if base != Kind::Angle(AngleUnit::Radian) {
                        return Err(bad(Some(n), "conversion_base_kind"));
                    }
                    if value <= 0.0 {
                        return Err(bad(Some(factor), "conversion_factor_not_positive"));
                    }
                    if relative_difference(value, DEGREE_IN_RADIANS) > ANGLE_FACTOR_TOLERANCE {
                        self.warnings.insert(HeaderWarning {
                            entity: n,
                            code: "angle_factor_mismatch",
                            authored,
                            exact: "pi/180",
                            offset: inst.offset,
                        });
                    }
                    Ok(Kind::Angle(AngleUnit::Degree))
                }
            }
            (None, None) if is_length || is_angle => Err(bad(Some(n), "unit_without_definition")),
            (None, None) => Ok(Kind::Other),
        }
    }
}

fn ref_list(ctx: u64, params: &[Param], max: usize, bound: HeaderBound) -> Result<Vec<u64>, HeaderError> {
    let items = match params {
        [Param::List(items)] => items,
        _ => return Err(bad(Some(ctx), "context_list_expected")),
    };
    if items.len() > max {
        return Err(limit(bound, max));
    }
    items
        .iter()
        .map(|p| match p {
            Param::Ref(r) => Ok(*r),
            _ => Err(bad(Some(ctx), "context_reference_expected")),
        })
        .collect()
}

/// The entity numbers among `contexts` that some other instance refers to.
fn owned_contexts(file: &P21File, contexts: &BTreeSet<u64>) -> BTreeSet<u64> {
    let mut owned = BTreeSet::new();
    let mut stack: Vec<&Param> = Vec::new();
    for (n, inst) in &file.data {
        for (_, params) in &inst.parts {
            stack.extend(params.iter());
            while let Some(p) = stack.pop() {
                match p {
                    Param::Ref(r) if *r != *n && contexts.contains(r) => {
                        owned.insert(*r);
                    }
                    Param::List(items) => stack.extend(items.iter()),
                    Param::Typed(_, inner) => stack.push(inner),
                    _ => {}
                }
            }
        }
    }
    owned
}

// ------------------------------------------------------------------ resolution

/// Resolves the header and the unit contexts of `file` (REQ-IO.CAD.STEP.HDR-001..003).
///
/// Order of checks: `FILE_SCHEMA` (present, exactly one identifier, accepted),
/// `FILE_DESCRIPTION`, `FILE_NAME`, then the contexts in ascending entity number, each
/// context's units in list order.
pub fn resolve_header(file: &P21File, opts: &HeaderOptions) -> Result<StepHeaderInfo, HeaderError> {
    let mut budget = TextBudget { left: opts.max_header_text_bytes, max: opts.max_header_text_bytes };

    let schema_params = header_record(file, "FILE_SCHEMA", "missing_file_schema")?;
    let ids = match schema_params {
        [p] => text_list(p, &mut budget)?,
        _ => return Err(bad(None, "file_schema_arity")),
    };
    let raw_schema = match ids.as_slice() {
        [one] => one.clone(),
        _ => return Err(bad(None, "file_schema_not_single")),
    };
    let schema = match match_schema(raw_schema.raw()) {
        Some(s) => s,
        None => return Err(HeaderError::SchemaUnsupported { raw: raw_schema.to_untrusted() }),
    };

    let description = match header_record(file, "FILE_DESCRIPTION", "missing_file_description")? {
        [d, level] => {
            FileDescription { description: text_list(d, &mut budget)?, implementation_level: text(level, &mut budget)? }
        }
        _ => return Err(bad(None, "file_description_arity")),
    };
    let (file_name, originating_system) = match header_record(file, "FILE_NAME", "missing_file_name")? {
        [name, ts, author, org, pre, sys, auth] => (
            FileName {
                name: text(name, &mut budget)?,
                time_stamp: text(ts, &mut budget)?,
                author: text_list(author, &mut budget)?,
                organization: text_list(org, &mut budget)?,
                preprocessor_version: text(pre, &mut budget)?,
                authorization: text(auth, &mut budget)?,
            },
            text(sys, &mut budget)?,
        ),
        _ => return Err(bad(None, "file_name_arity")),
    };

    if opts.override_unit && opts.unit.is_none() {
        return Err(HeaderError::UnitRequired { context: None });
    }
    let given = opts.unit.map(StepLengthUnit::from);

    let mut context_ids = BTreeSet::new();
    for (n, inst) in &file.data {
        if has(inst, "GEOMETRIC_REPRESENTATION_CONTEXT") {
            if context_ids.len() >= opts.max_contexts {
                return Err(limit(HeaderBound::Contexts, opts.max_contexts));
            }
            context_ids.insert(*n);
        }
    }
    let owned = owned_contexts(file, &context_ids);

    let mut walk = Walk { file, warnings: BTreeSet::new() };
    let mut contexts = BTreeMap::new();
    let mut unitless = Vec::new();
    for &c in &context_ids {
        let inst = get(file, c)?;
        let mut length = None;
        let mut angle = None;
        if let Some(params) = part(inst, "GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            for u in ref_list(c, params, MAX_UNITS_PER_CONTEXT, HeaderBound::Units)? {
                match walk.classify(u)? {
                    Kind::Length(l) => {
                        if length.replace(l).is_some() {
                            return Err(bad(Some(c), "duplicate_length_unit"));
                        }
                    }
                    Kind::Angle(a) => {
                        if angle.replace(a).is_some() {
                            return Err(bad(Some(c), "duplicate_angle_unit"));
                        }
                    }
                    Kind::Other => {}
                }
            }
        }
        let mut uncertainty = None;
        if let Some(params) = part(inst, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT") {
            for m in ref_list(c, params, MAX_UNCERTAINTIES_PER_CONTEXT, HeaderBound::Uncertainties)? {
                let (value, authored, unit_n) = measure(file, m)?;
                if let Kind::Length(unit) = walk.classify(unit_n)? {
                    if value < 0.0 {
                        return Err(bad(Some(m), "uncertainty_negative"));
                    }
                    uncertainty = Some(Uncertainty { value, authored, unit, length_factor_m: unit.factor_m() });
                    break;
                }
            }
        }
        let owns_geometry = owned.contains(&c);
        let (effective, source) = match (length, given) {
            (None, None) if owns_geometry => return Err(HeaderError::UnitRequired { context: Some(c) }),
            (None, None) => {
                unitless.push(c);
                continue;
            }
            (None, Some(g)) => (g, UnitSource::Caller),
            (Some(d), None) => (d, UnitSource::Declared),
            (Some(d), Some(g)) if d == g => (d, UnitSource::Declared),
            (Some(_), Some(_)) if !opts.override_unit => return Err(HeaderError::UnitConflict { context: c }),
            (Some(_), Some(g)) => (g, UnitSource::Override),
        };
        contexts.insert(
            c,
            UnitContext {
                context: c,
                length_unit: effective,
                length_factor_m: effective.factor_m(),
                declared: length,
                angle,
                uncertainty,
                source,
                owns_geometry,
            },
        );
    }

    Ok(StepHeaderInfo {
        schema,
        raw_schema,
        originating_system,
        file_name,
        description,
        contexts,
        unitless,
        warnings: walk.warnings.into_iter().collect(),
    })
}

// ------------------------------------------------------------------ provenance

/// Writes `items` as a JSON array of strings (RFC 8259 escaping), so a list of untrusted
/// strings stays one unambiguous value.
fn json_list(items: &[StepText]) -> String {
    let mut out = String::from("[");
    for (i, t) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in t.raw().chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

fn mine(s: &str) -> UntrustedText {
    UntrustedText::new(s)
}

impl UnitContext {
    /// The provenance summary of this context's units (everything but the entity number).
    /// Built only from enumerated names, decimal constants and validated number tokens.
    pub fn signature(&self) -> String {
        let uncertainty = match &self.uncertainty {
            Some(u) => format!("{} {}", u.authored, u.unit.symbol()),
            None => "none".to_owned(),
        };
        format!(
            "length={} factor_m={} source={} angle={} uncertainty={} geometry={}",
            self.length_unit.symbol(),
            self.length_unit.factor_decimal(),
            self.source.as_str(),
            self.angle.map_or("none", AngleUnit::as_str),
            uncertainty,
            self.owns_geometry,
        )
    }
}

impl StepHeaderInfo {
    /// The context whose unit the provenance record carries: the lowest-numbered context
    /// that owns geometry, else the lowest-numbered context.
    pub fn primary_context(&self) -> Option<&UnitContext> {
        self.contexts.values().find(|c| c.owns_geometry).or_else(|| self.contexts.values().next())
    }

    /// The import's effective unit for [`fid_io_reg::SourceProvenance`], from the primary
    /// context (or, when there is no context, from the caller's unit alone). A centimetre
    /// context fails `step_unit_unsupported`, because `IO.REG.PROV` has no centimetre.
    pub fn provenance_unit(&self, opts: &HeaderOptions) -> Result<UnitResolution, HeaderError> {
        let to_prov = |c: &UnitContext, u: StepLengthUnit| {
            u.to_length_unit().ok_or(HeaderError::UnitUnsupported { entity: c.context })
        };
        let (declared, given, override_unit) = match self.primary_context() {
            None => (None, opts.unit, opts.override_unit),
            Some(c) => match c.source {
                UnitSource::Declared => (Some(to_prov(c, c.length_unit)?), None, false),
                UnitSource::Caller => (None, Some(to_prov(c, c.length_unit)?), false),
                UnitSource::Override => {
                    let d = c.declared.ok_or(HeaderError::UnitRequired { context: Some(c.context) })?;
                    (Some(to_prov(c, d)?), Some(to_prov(c, c.length_unit)?), true)
                }
            },
        };
        resolve_unit(declared, given, override_unit).map_err(|e| match e {
            IoError::UnitConflict => {
                HeaderError::UnitConflict { context: self.primary_context().map_or(0, |c| c.context) }
            }
            _ => HeaderError::UnitRequired { context: self.primary_context().map(|c| c.context) },
        })
    }

    /// Records the declared schema, the header metadata, and every context's unit and
    /// uncertainty as `declared` items of the provenance (REQ-IO.CAD.STEP.HDR-004).
    ///
    /// Contexts are recorded grouped by [`UnitContext::signature`]: one item
    /// `step.units.group_<k>` per distinct signature (in order of its first context), with
    /// the count and the first context number, plus `step.contexts` (the total) and
    /// `step.units.mixed` when geometry contexts use more than one length unit. Lists of
    /// header strings are JSON arrays. Fails as `IO.REG.PROV` does on a value over its
    /// size limit or on too many items; nothing is truncated.
    pub fn record_provenance(&self, b: &mut ProvenanceBuilder) -> Result<(), ProvError> {
        b.declare("step.schema", mine(self.schema.as_str()))?;
        b.declare("step.schema_identifier", self.raw_schema.to_untrusted())?;
        b.declare("step.originating_system", self.originating_system.to_untrusted())?;
        b.declare("step.file_name", self.file_name.name.to_untrusted())?;
        b.declare("step.time_stamp", self.file_name.time_stamp.to_untrusted())?;
        b.declare("step.author", UntrustedText::from_string(json_list(&self.file_name.author)))?;
        b.declare("step.organization", UntrustedText::from_string(json_list(&self.file_name.organization)))?;
        b.declare("step.preprocessor_version", self.file_name.preprocessor_version.to_untrusted())?;
        b.declare("step.authorization", self.file_name.authorization.to_untrusted())?;
        b.declare("step.description", UntrustedText::from_string(json_list(&self.description.description)))?;
        b.declare("step.implementation_level", self.description.implementation_level.to_untrusted())?;
        b.declare("step.contexts", mine(&self.contexts.len().to_string()))?;
        if !self.unitless.is_empty() {
            b.declare("step.unitless_contexts", mine(&self.unitless.len().to_string()))?;
        }
        let geometry_units: BTreeSet<StepLengthUnit> =
            self.contexts.values().filter(|c| c.owns_geometry).map(|c| c.length_unit).collect();
        if geometry_units.len() > 1 {
            b.declare("step.units.mixed", mine("true"))?;
        }
        for (k, (signature, count, first)) in self.unit_groups().iter().enumerate() {
            let value = format!("{signature} contexts={count} first=#{first}");
            b.declare(&format!("step.units.group_{}", k + 1), mine(&value))?;
        }
        Ok(())
    }

    /// `(signature, context count, first context)` per distinct signature, in order of the
    /// first context.
    pub fn unit_groups(&self) -> Vec<(String, u64, u64)> {
        let mut groups: Vec<(String, u64, u64)> = Vec::new();
        let mut index: BTreeMap<String, usize> = BTreeMap::new();
        for c in self.contexts.values() {
            let sig = c.signature();
            match index.get(&sig) {
                Some(&i) => {
                    if let Some(g) = groups.get_mut(i) {
                        g.1 = g.1.saturating_add(1);
                    }
                }
                None => {
                    index.insert(sig.clone(), groups.len());
                    groups.push((sig, 1, c.context));
                }
            }
        }
        groups
    }

    /// Adds every factor warning to the import report.
    pub fn record_warnings(&self, report: &mut ReportBuilder) -> Result<(), ReportError> {
        for w in &self.warnings {
            let code = Code::new(w.code)?;
            let pos = ReportPos::new(Position::byte(w.offset)).ok();
            let detail = format!("#{}: file factor {} replaced by exact {}", w.entity, w.authored, w.exact);
            report.warn(&code, pos, &mine(&detail));
        }
        Ok(())
    }
}
