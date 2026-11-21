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
//! Every unit and measure instance is classified once (memoised by entity number), the parts
//! of each instance read are bounded ([`MAX_PARTS_READ`]), and authored number tokens are
//! interned and charged to a byte budget ([`MAX_NUMBER_TEXT_BYTES`]), so the work and the
//! memory stay linear in the file. There is no recursion and no panic; every refusal is a
//! typed [`HeaderError`].

use crate::{Instance, P21File, Param, StepText};
use fid_io_reg::prov::{LengthUnit, MAX_DECLARED, MAX_TEXT_BYTES};
use fid_io_reg::{
    resolve_unit, Code, IoError, Position, ProvError, ProvenanceBuilder, ReportBuilder, ReportError, ReportPos,
    UnitResolution, UnitSource, UntrustedText,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

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
/// Limit on the parts of one instance this module reads (a context, a unit or a measure).
/// Complex instances in practice have a handful; more is refused, not scanned.
pub const MAX_PARTS_READ: usize = 64;
/// Limit on one authored number token kept for provenance (an uncertainty or a factor).
pub const MAX_NUMBER_LEXEME_BYTES: usize = 128;
/// Budget for the distinct authored number tokens kept, each charged its length plus
/// [`LEXEME_ENTRY_COST`]. Equal tokens are shared, so this counts distinct tokens only.
pub const MAX_NUMBER_TEXT_BYTES: usize = 256 * 1024;
/// Nominal bytes charged per distinct authored token on top of its length: the `Arc`
/// header (two counters) and the interning set's entry.
pub const LEXEME_ENTRY_COST: usize = 64;
/// The import-report warning code for an uncertainty re-expressed in the override unit.
pub const UNCERTAINTY_REEXPRESSED: &str = "step_uncertainty_reexpressed";
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

/// A context's declared length uncertainty.
#[derive(Debug, Clone, PartialEq)]
pub struct Uncertainty {
    /// The value as parsed (a zero is always `+0.0`).
    pub value: f64,
    /// The authored token (a real lexeme or an integer), shared between contexts.
    pub authored: Arc<str>,
    /// The unit the value is expressed in: the file's, or the override unit when
    /// [`Uncertainty::reexpressed_from`] is set.
    pub unit: StepLengthUnit,
    /// The f64 nearest to that unit's exact factor to metres.
    pub length_factor_m: f64,
    /// The file's unit, when an override moved the uncertainty to the effective unit
    /// (the number is kept, as it is for the geometry).
    pub reexpressed_from: Option<StepLengthUnit>,
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

/// What a [`HeaderWarning`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarningDetail {
    /// A conversion-based unit's file factor was replaced by the exact one, written as a
    /// decimal (`0.0254`, `0.3048`) or `pi/180`.
    ExactFactor(&'static str),
    /// An override moved a context's uncertainty from the file's unit to the effective one.
    Reexpressed {
        /// The file's unit.
        from: StepLengthUnit,
        /// The effective (override) unit.
        to: StepLengthUnit,
    },
}

/// A warning for the import report: a conversion-based unit whose file factor differs from
/// the known exact one (the exact factor is used, REQ-IO.CAD.STEP.HDR-002), or an
/// uncertainty re-expressed in the override unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HeaderWarning {
    /// The conversion-based unit's entity number, or the context's for a re-expression.
    pub entity: u64,
    /// `length_factor_mismatch`, `angle_factor_mismatch` or [`UNCERTAINTY_REEXPRESSED`].
    pub code: &'static str,
    /// The authored factor or uncertainty token of the file.
    pub authored: Arc<str>,
    /// What happened.
    pub detail: WarningDetail,
    /// Byte offset of the unit (or context) instance.
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
    /// [`MAX_PARTS_READ`].
    Parts,
    /// [`MAX_NUMBER_LEXEME_BYTES`].
    NumberLexeme,
    /// [`MAX_NUMBER_TEXT_BYTES`].
    NumberText,
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
            HeaderBound::Parts => "parts",
            HeaderBound::NumberLexeme => "number_lexeme",
            HeaderBound::NumberText => "number_text",
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

/// Refuses an instance with more parts than this module reads ([`MAX_PARTS_READ`]).
fn bounded(inst: &Instance) -> Result<(), HeaderError> {
    if inst.parts.len() > MAX_PARTS_READ {
        return Err(limit(HeaderBound::Parts, MAX_PARTS_READ));
    }
    Ok(())
}

/// The parameters of part `name` of instance `n`, if present. A part this module reads that
/// appears twice is `malformed` (`duplicate_part`): there is no first-wins.
fn part<'i>(inst: &'i Instance, n: u64, name: &str) -> Result<Option<&'i [Param]>, HeaderError> {
    let mut found = None;
    for (p, params) in &inst.parts {
        if p.as_str() == name && found.replace(params.as_slice()).is_some() {
            return Err(bad(Some(n), "duplicate_part"));
        }
    }
    Ok(found)
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

/// The parts of a unit instance this module reads, each checked for repeats.
struct UnitParts<'i> {
    si: Option<&'i [Param]>,
    conv: Option<&'i [Param]>,
    is_length: bool,
    is_angle: bool,
}

fn unit_parts(inst: &Instance, n: u64) -> Result<UnitParts<'_>, HeaderError> {
    bounded(inst)?;
    Ok(UnitParts {
        si: part(inst, n, "SI_UNIT")?,
        conv: part(inst, n, "CONVERSION_BASED_UNIT")?,
        is_length: part(inst, n, "LENGTH_UNIT")?.is_some(),
        is_angle: part(inst, n, "PLANE_ANGLE_UNIT")?.is_some(),
    })
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

fn classify_si(n: u64, up: &UnitParts<'_>, params: &[Param]) -> Result<Kind, HeaderError> {
    let (prefix, name) = si_attrs(n, params)?;
    match name {
        "METRE" => {
            if up.is_angle {
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
            if up.is_length {
                return Err(bad(Some(n), "unit_kind_conflict"));
            }
            match prefix {
                None => Ok(Kind::Angle(AngleUnit::Radian)),
                Some(_) => Err(HeaderError::UnitUnsupported { entity: n }),
            }
        }
        _ if up.is_length || up.is_angle => Err(bad(Some(n), "unit_kind_conflict")),
        _ => Ok(Kind::Other),
    }
}

fn relative_difference(file_factor: f64, exact: f64) -> f64 {
    ((file_factor - exact) / exact).abs()
}

/// `(value, authored token, unit reference)` of a measure-with-unit instance.
type Measure = (f64, Arc<str>, u64);

/// The unit walk. Every unit and measure instance is evaluated once and memoised by entity
/// number, so a unit shared by many contexts (or listed many times) costs one evaluation.
/// Only successes are memoised: any refusal ends the resolution.
struct Walk<'f> {
    file: &'f P21File,
    warnings: BTreeSet<HeaderWarning>,
    kinds: BTreeMap<u64, Kind>,
    measures: BTreeMap<u64, Measure>,
    lexemes: BTreeSet<Arc<str>>,
    number_left: usize,
}

impl<'f> Walk<'f> {
    fn new(file: &'f P21File) -> Walk<'f> {
        Walk {
            file,
            warnings: BTreeSet::new(),
            kinds: BTreeMap::new(),
            measures: BTreeMap::new(),
            lexemes: BTreeSet::new(),
            number_left: MAX_NUMBER_TEXT_BYTES,
        }
    }

    /// One shared copy per distinct authored token, charged to the number-text budget.
    fn intern(&mut self, s: &str) -> Result<Arc<str>, HeaderError> {
        if s.len() > MAX_NUMBER_LEXEME_BYTES {
            return Err(limit(HeaderBound::NumberLexeme, MAX_NUMBER_LEXEME_BYTES));
        }
        if let Some(a) = self.lexemes.get(s) {
            return Ok(Arc::clone(a));
        }
        let cost = s.len().saturating_add(LEXEME_ENTRY_COST);
        self.number_left = match self.number_left.checked_sub(cost) {
            Some(rest) => rest,
            None => return Err(limit(HeaderBound::NumberText, MAX_NUMBER_TEXT_BYTES)),
        };
        let a: Arc<str> = Arc::from(s);
        self.lexemes.insert(Arc::clone(&a));
        Ok(a)
    }

    /// The measure-with-unit instance `n`. Exactly one of its `*MEASURE_WITH_UNIT` parts
    /// may carry the value and unit; a repeated part name or a second value-carrying part is
    /// `duplicate_part`.
    fn measure(&mut self, n: u64) -> Result<Measure, HeaderError> {
        if let Some(m) = self.measures.get(&n) {
            return Ok(m.clone());
        }
        let inst = get(self.file, n)?;
        bounded(inst)?;
        let mut names: BTreeSet<&str> = BTreeSet::new();
        let mut found: Option<(&Param, u64)> = None;
        for (name, params) in &inst.parts {
            if !name.as_str().ends_with("MEASURE_WITH_UNIT") {
                continue;
            }
            if !names.insert(name.as_str()) {
                return Err(bad(Some(n), "duplicate_part"));
            }
            if let [v, Param::Ref(u), ..] = params.as_slice() {
                if found.replace((v, *u)).is_some() {
                    return Err(bad(Some(n), "duplicate_part"));
                }
            }
        }
        let (value, unit) = found.ok_or_else(|| bad(Some(n), "measure_with_unit_expected"))?;
        let inner = match value {
            Param::Typed(t, inner) if t.as_str().ends_with("_MEASURE") => &**inner,
            other => other,
        };
        let (v, authored) = match inner {
            Param::Real(v, Some(lex)) => (*v, self.intern(lex.as_str())?),
            Param::Real(v, None) => (*v, self.intern(&format!("{v:?}"))?),
            Param::Int(i) => (*i as f64, self.intern(&i.to_string())?),
            _ => return Err(bad(Some(n), "measure_value_type")),
        };
        if !v.is_finite() {
            return Err(bad(Some(n), "measure_value_not_finite"));
        }
        // -0.0 is read as +0.0, so a zero has one value (and one provenance group).
        let v = if v == 0.0 { 0.0 } else { v };
        let m = (v, authored, unit);
        self.measures.insert(n, m.clone());
        Ok(m)
    }

    /// Classifies unit instance `n` (memoised).
    fn classify(&mut self, n: u64) -> Result<Kind, HeaderError> {
        if let Some(k) = self.kinds.get(&n) {
            return Ok(*k);
        }
        let k = self.classify_uncached(n)?;
        self.kinds.insert(n, k);
        Ok(k)
    }

    /// A conversion-based unit is followed to its measure and that measure's unit, which
    /// must be an SI unit (two levels, no recursion).
    fn classify_uncached(&mut self, n: u64) -> Result<Kind, HeaderError> {
        let file = self.file;
        let inst = get(file, n)?;
        let up = unit_parts(inst, n)?;
        if up.is_length && up.is_angle {
            return Err(bad(Some(n), "unit_kind_conflict"));
        }
        match (up.si, up.conv) {
            (Some(_), Some(_)) => Err(bad(Some(n), "unit_kind_conflict")),
            (Some(params), None) => classify_si(n, &up, params),
            (None, Some(params)) => {
                if !up.is_length && !up.is_angle {
                    return Ok(Kind::Other);
                }
                let (name, factor) = match params {
                    [Param::Str(s), Param::Ref(r)] | [_, Param::Str(s), Param::Ref(r)] => (s, *r),
                    _ => return Err(bad(Some(n), "conversion_based_unit_attributes")),
                };
                let word = name.raw();
                let (value, authored, base_n) = self.measure(factor)?;
                let base_inst = get(file, base_n)?;
                let base_up = unit_parts(base_inst, base_n)?;
                let base = match base_up.si {
                    Some(p) if base_up.conv.is_none() => classify_si(base_n, &base_up, p)?,
                    _ => return Err(bad(Some(base_n), "conversion_base_not_si")),
                };
                if up.is_length {
                    let unit = if word.eq_ignore_ascii_case("INCH") {
                        StepLengthUnit::In
                    } else if word.eq_ignore_ascii_case("FOOT") {
                        StepLengthUnit::Ft
                    } else {
                        return Err(HeaderError::UnitUnsupported { entity: n });
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
                            detail: WarningDetail::ExactFactor(unit.factor_decimal()),
                            offset: inst.offset,
                        });
                    }
                    Ok(Kind::Length(unit))
                } else {
                    if !word.eq_ignore_ascii_case("DEGREE") {
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
                            detail: WarningDetail::ExactFactor("pi/180"),
                            offset: inst.offset,
                        });
                    }
                    Ok(Kind::Angle(AngleUnit::Degree))
                }
            }
            (None, None) if up.is_length || up.is_angle => Err(bad(Some(n), "unit_without_definition")),
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
        let mut marks = 0usize;
        for (p, _) in &inst.parts {
            if p.as_str() == "GEOMETRIC_REPRESENTATION_CONTEXT" {
                marks += 1;
            }
        }
        match marks {
            0 => {}
            1 => {
                bounded(inst)?;
                if context_ids.len() >= opts.max_contexts {
                    return Err(limit(HeaderBound::Contexts, opts.max_contexts));
                }
                context_ids.insert(*n);
            }
            _ => return Err(bad(Some(*n), "duplicate_part")),
        }
    }
    let owned = owned_contexts(file, &context_ids);

    let mut walk = Walk::new(file);
    let mut contexts = BTreeMap::new();
    let mut unitless = Vec::new();
    for &c in &context_ids {
        let inst = get(file, c)?;
        let unit_part = part(inst, c, "GLOBAL_UNIT_ASSIGNED_CONTEXT")?;
        let uncertainty_part = part(inst, c, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")?;
        let mut length = None;
        let mut angle = None;
        if let Some(params) = unit_part {
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
        if let Some(params) = uncertainty_part {
            for m in ref_list(c, params, MAX_UNCERTAINTIES_PER_CONTEXT, HeaderBound::Uncertainties)? {
                let (value, authored, unit_n) = walk.measure(m)?;
                if let Kind::Length(unit) = walk.classify(unit_n)? {
                    if value < 0.0 {
                        return Err(bad(Some(m), "uncertainty_negative"));
                    }
                    uncertainty = Some(Uncertainty {
                        value,
                        authored,
                        unit,
                        length_factor_m: unit.factor_m(),
                        reexpressed_from: None,
                    });
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
        // An override says the file's length unit is wrong. An uncertainty written in that
        // same (declared) unit is wrong in the same way, so it moves with the geometry: the
        // number is kept and its unit becomes the effective one, with a report warning.
        if source == UnitSource::Override {
            if let (Some(u), Some(d)) = (uncertainty.as_mut(), length) {
                if u.unit == d {
                    u.reexpressed_from = Some(d);
                    u.unit = effective;
                    u.length_factor_m = effective.factor_m();
                    walk.warnings.insert(HeaderWarning {
                        entity: c,
                        code: UNCERTAINTY_REEXPRESSED,
                        authored: Arc::clone(&u.authored),
                        detail: WarningDetail::Reexpressed { from: d, to: effective },
                        offset: inst.offset,
                    });
                }
            }
        }
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

/// The `declared` items [`StepHeaderInfo::record_provenance`] always writes for the header
/// records (`step.schema` .. `step.implementation_level`) plus `step.contexts`.
const FIXED_ITEMS: usize = 12;

/// What groups contexts in the provenance: the unit summary, with the uncertainty as its
/// parsed value and unit (never the authored token, so `1.E-6` and `1.0E-6` share a group).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    length: StepLengthUnit,
    source: UnitSource,
    angle: Option<AngleUnit>,
    uncertainty: Option<(u64, StepLengthUnit)>,
    geometry: bool,
}

impl UnitContext {
    fn group_key(&self) -> GroupKey {
        GroupKey {
            length: self.length_unit,
            source: self.source,
            angle: self.angle,
            uncertainty: self.uncertainty.as_ref().map(|u| (u.value.to_bits(), u.unit)),
            geometry: self.owns_geometry,
        }
    }

    /// The provenance summary of this context's units (everything but the entity number).
    /// Built only from enumerated names, decimal constants and the parsed uncertainty value
    /// (Rust's shortest round-trip form of the f64, e.g. `1e-6`), never from file text.
    pub fn signature(&self) -> String {
        let uncertainty = match &self.uncertainty {
            Some(u) => format!("{:?} {}", u.value, u.unit.symbol()),
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
    /// The context whose unit the provenance record carries when every relevant context's
    /// unit is declared: the lowest-numbered context that owns geometry, else the
    /// lowest-numbered context.
    pub fn primary_context(&self) -> Option<&UnitContext> {
        self.contexts.values().find(|c| c.owns_geometry).or_else(|| self.contexts.values().next())
    }

    /// The contexts whose effective length unit is not the one the file declared (source
    /// `caller` or `override`), among those that own geometry plus the primary context, in
    /// ascending entity number. Any such context makes the import `unit_overridden`.
    pub fn not_declared(&self) -> Vec<&UnitContext> {
        let primary = self.primary_context().map(|c| c.context);
        self.contexts
            .values()
            .filter(|c| c.source != UnitSource::Declared && (c.owns_geometry || Some(c.context) == primary))
            .collect()
    }

    /// The import's effective unit for [`fid_io_reg::SourceProvenance`].
    ///
    /// When any context of [`StepHeaderInfo::not_declared`] exists, the caller's unit
    /// applied somewhere, and every context then has that unit as its effective unit: the
    /// record carries it with source `override` if any of them was overridden, else
    /// `caller`, so `unit_overridden` is true. Otherwise the unit comes from the primary
    /// context (declared), or, when there is no context, from the caller's unit alone. A
    /// centimetre unit fails `step_unit_unsupported`, because `IO.REG.PROV` has none.
    pub fn provenance_unit(&self, opts: &HeaderOptions) -> Result<UnitResolution, HeaderError> {
        let to_prov =
            |c: &UnitContext| c.length_unit.to_length_unit().ok_or(HeaderError::UnitUnsupported { entity: c.context });
        let affected = self.not_declared();
        if let Some(first) = affected.first() {
            let g = to_prov(first)?;
            if let Some(c) = affected.iter().find(|c| c.length_unit != first.length_unit) {
                return Err(HeaderError::UnitConflict { context: c.context });
            }
            let resolved = match affected.iter().find(|c| c.source == UnitSource::Override) {
                // The override row of the IO.REG.PROV table needs only "declared differs
                // from given"; the record carries the effective unit, not the replaced one
                // (which may be a centimetre, absent from IO.REG.PROV).
                Some(c) => {
                    let stand_in = if g == LengthUnit::M { LengthUnit::Mm } else { LengthUnit::M };
                    let d = c.declared.and_then(|d| d.to_length_unit()).filter(|d| *d != g).unwrap_or(stand_in);
                    resolve_unit(Some(d), Some(g), true)
                }
                None => resolve_unit(None, Some(g), false),
            };
            return resolved.map_err(|_| HeaderError::UnitConflict { context: first.context });
        }
        let (declared, given, override_unit) = match self.primary_context() {
            None => (None, opts.unit, opts.override_unit),
            // Not in `affected`, so its unit is declared.
            Some(c) => (Some(to_prov(c)?), None, false),
        };
        resolve_unit(declared, given, override_unit).map_err(|e| match e {
            IoError::UnitConflict => {
                HeaderError::UnitConflict { context: self.primary_context().map_or(0, |c| c.context) }
            }
            _ => HeaderError::UnitRequired { context: self.primary_context().map(|c| c.context) },
        })
    }

    /// The contexts of [`StepHeaderInfo::not_declared`] as runs over the context table: a
    /// run `#a..#b` (or `#a`) stands for every context of [`StepHeaderInfo::contexts`]
    /// numbered from `a` to `b`. Runs are joined by `,` into pieces of at most
    /// [`MAX_TEXT_BYTES`] bytes each.
    pub fn not_declared_runs(&self) -> Vec<String> {
        let affected: BTreeSet<u64> = self.not_declared().iter().map(|c| c.context).collect();
        let mut runs: Vec<(u64, u64)> = Vec::new();
        let mut open = false;
        for &c in self.contexts.keys() {
            if affected.contains(&c) {
                match runs.last_mut() {
                    Some(last) if open => last.1 = c,
                    _ => runs.push((c, c)),
                }
                open = true;
            } else {
                open = false;
            }
        }
        let mut pieces: Vec<String> = Vec::new();
        let mut cur = String::new();
        for (a, b) in runs {
            let run = if a == b { format!("#{a}") } else { format!("#{a}..#{b}") };
            if !cur.is_empty() && cur.len() + 1 + run.len() > MAX_TEXT_BYTES {
                pieces.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(',');
            }
            cur.push_str(&run);
        }
        if !cur.is_empty() {
            pieces.push(cur);
        }
        pieces
    }

    /// Records the declared schema, the header metadata, and every context's unit and
    /// uncertainty as `declared` items of the provenance (REQ-IO.CAD.STEP.HDR-004).
    ///
    /// Contexts are recorded grouped by [`UnitContext::signature`]: one item
    /// `step.units.group_<k>` per distinct signature (in order of its first context), with
    /// the count and the first context number, plus `step.contexts` (the total) and
    /// `step.units.mixed` when geometry contexts use more than one length unit. When the
    /// unit of any context was not declared ([`StepHeaderInfo::not_declared`]),
    /// `step.units.not_declared` gives the counts and `step.units.not_declared_<k>` the
    /// contexts ([`StepHeaderInfo::not_declared_runs`]). Lists of header strings are JSON
    /// arrays.
    ///
    /// The items are counted first: more than `IO.REG.PROV`'s [`MAX_DECLARED`] fails
    /// `too_large` with field `step.units`, and nothing is declared. A value over
    /// `IO.REG.PROV`'s size limit fails as it does there; nothing is truncated.
    pub fn record_provenance(&self, b: &mut ProvenanceBuilder) -> Result<(), ProvError> {
        let groups = self.unit_groups();
        let affected = self.not_declared();
        let runs = if affected.is_empty() { Vec::new() } else { self.not_declared_runs() };
        let geometry_units: BTreeSet<StepLengthUnit> =
            self.contexts.values().filter(|c| c.owns_geometry).map(|c| c.length_unit).collect();
        let mixed = geometry_units.len() > 1;
        let planned = FIXED_ITEMS
            + usize::from(!self.unitless.is_empty())
            + usize::from(mixed)
            + groups.len()
            + if affected.is_empty() { 0 } else { 1 + runs.len() };
        if planned > MAX_DECLARED {
            return Err(ProvError::TooLarge { field: "step.units", limit: MAX_DECLARED as u64 });
        }
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
        if mixed {
            b.declare("step.units.mixed", mine("true"))?;
        }
        for (k, (signature, count, first)) in groups.iter().enumerate() {
            let value = format!("{signature} contexts={count} first=#{first}");
            b.declare(&format!("step.units.group_{}", k + 1), mine(&value))?;
        }
        if !affected.is_empty() {
            let overridden = affected.iter().filter(|c| c.source == UnitSource::Override).count();
            let summary =
                format!("contexts={} override={} caller={}", affected.len(), overridden, affected.len() - overridden);
            b.declare("step.units.not_declared", mine(&summary))?;
            for (k, piece) in runs.iter().enumerate() {
                b.declare(&format!("step.units.not_declared_{}", k + 1), mine(piece))?;
            }
        }
        Ok(())
    }

    /// `(signature, context count, first context)` per distinct signature, in order of the
    /// first context. Contexts are grouped on parsed values ([`UnitContext::signature`]).
    pub fn unit_groups(&self) -> Vec<(String, u64, u64)> {
        let mut groups: Vec<(String, u64, u64)> = Vec::new();
        let mut index: BTreeMap<GroupKey, usize> = BTreeMap::new();
        for c in self.contexts.values() {
            let key = c.group_key();
            match index.get(&key) {
                Some(&i) => {
                    if let Some(g) = groups.get_mut(i) {
                        g.1 = g.1.saturating_add(1);
                    }
                }
                None => {
                    index.insert(key, groups.len());
                    groups.push((c.signature(), 1, c.context));
                }
            }
        }
        groups
    }

    /// Adds every warning (factor replaced, uncertainty re-expressed) to the import report.
    pub fn record_warnings(&self, report: &mut ReportBuilder) -> Result<(), ReportError> {
        for w in &self.warnings {
            let code = Code::new(w.code)?;
            let pos = ReportPos::new(Position::byte(w.offset)).ok();
            let detail = match w.detail {
                WarningDetail::ExactFactor(exact) => {
                    format!("#{}: file factor {} replaced by exact {}", w.entity, w.authored, exact)
                }
                WarningDetail::Reexpressed { from, to } => format!(
                    "#{}: uncertainty {} re-expressed from {} to {} (unit override)",
                    w.entity,
                    w.authored,
                    from.symbol(),
                    to.symbol()
                ),
            };
            report.warn(&code, pos, &mine(&detail));
        }
        Ok(())
    }
}
