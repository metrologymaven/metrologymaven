//! Closed factor catalogue and typed probe candidates (spec node `MIG.EXPLAIN.CAT`).
//!
//! - [`Factor`]: closed `#[repr(u8)]` enum in fixed catalogue order (`FIT_METHOD`, `FILTER`,
//!   `ALIGNMENT`, `DATUM_INTERPRETATION`, `POINT_SELECTION`, `ROUNDING`; REQ-001).
//! - [`FactorCatalogue`]: loads `mig-factors@1.json` and compiles each factor's setting JSON
//!   Schema via `fid-bus::validate::Compiled`.
//! - [`validate_candidate_set`]: validates every candidate against the catalogue (REQ-002),
//!   enforces `MAX_CANDIDATES = 64` (REQ-004), overrides `source`/`proposer`/`session` to
//!   [`CandidateSource::AgentProposed`] whenever the calling [`Principal`] is an agent
//!   (REQ-003), sorts by catalogue order then setting digest, and computes the SHA-256
//!   RFC 8785 canonical digest (REQ-005).

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::principal::{Principal, PrincipalKind};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Version identifier of the closed v1 factor catalogue.
pub const CATALOGUE_SCHEMA: &str = "mig-factors@1";
/// Maximum candidates permitted in one [`CandidateSet`] (REQ-004).
pub const MAX_CANDIDATES: usize = 64;
/// Maximum input JSON bytes accepted for a candidate set or catalogue (1 MiB).
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum JSON nesting depth accepted.
pub const MAX_INPUT_DEPTH: usize = 16;

/// Embedded canonical `mig-factors@1.json` catalogue data.
pub const MIG_FACTORS_V1_JSON: &str = include_str!("../../data/mig-factors@1.json");

// ------------------------------------------------------------------ Factor

/// Closed catalogue of the only discrepancy causes the explainer may name (REQ-001).
/// Numeric discriminant fixes the global tie-breaking order.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Factor {
    FitMethod = 0,
    Filter = 1,
    Alignment = 2,
    DatumInterpretation = 3,
    PointSelection = 4,
    Rounding = 5,
}

impl Factor {
    /// All six factors in fixed catalogue order (REQ-001).
    pub const ALL: [Factor; 6] = [
        Factor::FitMethod,
        Factor::Filter,
        Factor::Alignment,
        Factor::DatumInterpretation,
        Factor::PointSelection,
        Factor::Rounding,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Factor::FitMethod => "FIT_METHOD",
            Factor::Filter => "FILTER",
            Factor::Alignment => "ALIGNMENT",
            Factor::DatumInterpretation => "DATUM_INTERPRETATION",
            Factor::PointSelection => "POINT_SELECTION",
            Factor::Rounding => "ROUNDING",
        }
    }

    pub fn parse(s: &str) -> Option<Factor> {
        match s {
            "FIT_METHOD" => Some(Factor::FitMethod),
            "FILTER" => Some(Factor::Filter),
            "ALIGNMENT" => Some(Factor::Alignment),
            "DATUM_INTERPRETATION" => Some(Factor::DatumInterpretation),
            "POINT_SELECTION" => Some(Factor::PointSelection),
            "ROUNDING" => Some(Factor::Rounding),
            _ => None,
        }
    }
}

impl fmt::Display for Factor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ------------------------------------------------------------------ CandidateSource & Caller

/// Origin of a [`ProbeCandidate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CandidateSource {
    ExportStated,
    UserSelected,
    AgentProposed,
}

impl CandidateSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            CandidateSource::ExportStated => "ExportStated",
            CandidateSource::UserSelected => "UserSelected",
            CandidateSource::AgentProposed => "AgentProposed",
        }
    }

    pub fn parse(s: &str) -> Option<CandidateSource> {
        match s {
            "ExportStated" => Some(CandidateSource::ExportStated),
            "UserSelected" => Some(CandidateSource::UserSelected),
            "AgentProposed" => Some(CandidateSource::AgentProposed),
            _ => None,
        }
    }
}

/// Authenticated caller submitting a candidate set (derived from `CORE.BUS.PRIN`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerContext {
    pub kind: PrincipalKind,
    pub principal_id: String,
    pub session: Option<String>,
}

impl CallerContext {
    /// Builds a [`CallerContext`] directly from an authenticated bus [`Principal`] and
    /// optional session ID.
    pub fn from_principal(principal: &Principal, session: Option<String>) -> CallerContext {
        CallerContext { kind: principal.kind(), principal_id: principal.id().as_str().to_string(), session }
    }
}

// ------------------------------------------------------------------ Errors

/// Errors returned by factor-catalogue loading and candidate-set validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatError {
    /// A candidate names an unknown factor or its setting fails the factor schema (REQ-002).
    CandidateInvalid { index: usize, factor: String, reason: String },
    /// Candidate set contains more than [`MAX_CANDIDATES`] (64) candidates (REQ-004).
    CandidateSetTooLarge { count: usize },
    /// Catalogue data file is malformed.
    CatalogueInvalid(String),
    /// Input JSON failed bounded parsing or top-level shape validation.
    BadInput(String),
    /// Internal canonical JSON error.
    Internal(&'static str),
}

impl CatError {
    pub const fn code(&self) -> &'static str {
        match self {
            CatError::CandidateInvalid { .. } => "candidate_invalid",
            CatError::CandidateSetTooLarge { .. } => "candidate_set_too_large",
            CatError::CatalogueInvalid(_) => "catalogue_invalid",
            CatError::BadInput(_) => "bad_input",
            CatError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for CatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatError::CandidateInvalid { index, factor, reason } => {
                write!(f, "candidate_invalid[index={index}, factor={factor:?}]: {reason}")
            }
            CatError::CandidateSetTooLarge { count } => {
                write!(f, "candidate_set_too_large: {count} exceeds limit {MAX_CANDIDATES}")
            }
            CatError::CatalogueInvalid(s) => write!(f, "catalogue_invalid: {s}"),
            CatError::BadInput(s) => write!(f, "bad_input: {s}"),
            CatError::Internal(s) => write!(f, "internal: {s}"),
        }
    }
}

impl std::error::Error for CatError {}

// ------------------------------------------------------------------ FactorCatalogue

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantity literals in factor schemas")))
    }

    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

/// Compiled `mig-factors@1` catalogue mapping each [`Factor`] to its strict JSON Schema.
pub struct FactorCatalogue {
    version: String,
    schemas: BTreeMap<Factor, Compiled>,
}

impl FactorCatalogue {
    /// Loads the embedded `mig-factors@1` catalogue.
    pub fn v1() -> Result<FactorCatalogue, CatError> {
        FactorCatalogue::from_json(MIG_FACTORS_V1_JSON.as_bytes())
    }

    /// Parses and compiles a `mig-factors@1` JSON document, verifying that it defines exactly
    /// the six closed [`Factor`] variants in exact catalogue order (REQ-001).
    pub fn from_json(bytes: &[u8]) -> Result<FactorCatalogue, CatError> {
        let v = parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_INPUT_DEPTH)
            .map_err(|e| CatError::CatalogueInvalid(format!("json parse: {e}")))?;
        let CValue::Obj(root) = v else {
            return Err(CatError::CatalogueInvalid("root must be an object".into()));
        };
        if root.len() != 2 || !root.contains_key("schema") || !root.contains_key("factors") {
            return Err(CatError::CatalogueInvalid("expected exact keys 'schema' and 'factors'".into()));
        }
        let version = match root.get("schema") {
            Some(CValue::Str(s)) if s == CATALOGUE_SCHEMA => s.clone(),
            other => {
                return Err(CatError::CatalogueInvalid(format!(
                    "unsupported catalogue schema {other:?} (expected {CATALOGUE_SCHEMA:?})"
                )))
            }
        };
        let Some(CValue::Arr(items)) = root.get("factors") else {
            return Err(CatError::CatalogueInvalid("'factors' must be an array".into()));
        };
        if items.len() != Factor::ALL.len() {
            return Err(CatError::CatalogueInvalid(format!(
                "expected {} factors, found {}",
                Factor::ALL.len(),
                items.len()
            )));
        }
        let mut schemas = BTreeMap::new();
        for (idx, (item, expected_factor)) in items.iter().zip(Factor::ALL).enumerate() {
            let CValue::Obj(fo) = item else {
                return Err(CatError::CatalogueInvalid(format!("factors[{idx}] must be an object")));
            };
            if fo.len() != 2 || !fo.contains_key("id") || !fo.contains_key("setting_schema") {
                return Err(CatError::CatalogueInvalid(format!(
                    "factors[{idx}] must have exact keys 'id' and 'setting_schema'"
                )));
            }
            let Some(CValue::Str(id_str)) = fo.get("id") else {
                return Err(CatError::CatalogueInvalid(format!("factors[{idx}].id must be a string")));
            };
            if id_str != expected_factor.as_str() {
                return Err(CatError::CatalogueInvalid(format!(
                    "factors[{idx}].id is {id_str:?}, expected {:?} (catalogue order is fixed)",
                    expected_factor.as_str()
                )));
            }
            let Some(schema_val) = fo.get("setting_schema") else {
                return Err(CatError::CatalogueInvalid(format!("factors[{idx}] missing setting_schema")));
            };
            let compiled = Compiled::compile(schema_val).map_err(|e| {
                CatError::CatalogueInvalid(format!("factors[{idx}] ({id_str}) schema compile error: {e}"))
            })?;
            schemas.insert(expected_factor, compiled);
        }
        Ok(FactorCatalogue { version, schemas })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    /// Validates one factor's setting object against its compiled schema.
    pub fn validate_setting(&self, factor: Factor, setting: &CValue) -> Result<(), String> {
        let Some(schema) = self.schemas.get(&factor) else {
            return Err("missing compiled schema".into());
        };
        schema
            .evaluate(setting, &NoHooks)
            .map(|_| ())
            .map_err(|fail| format!("{} at {}: {}", fail.keyword, fail.pointer, fail.message))
    }
}

// ------------------------------------------------------------------ ProbeCandidate & CandidateSet

/// One validated, typed discrepancy-explanation candidate (no free-form cause strings).
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeCandidate {
    pub factor: Factor,
    pub setting: CValue,
    pub setting_digest: Digest,
    pub source: CandidateSource,
    pub proposer: String,
    pub session: Option<String>,
}

impl ProbeCandidate {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("factor".into(), CValue::Str(self.factor.as_str().into()));
        o.insert("proposer".into(), CValue::Str(self.proposer.clone()));
        o.insert("session".into(), self.session.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone())));
        o.insert("setting".into(), self.setting.clone());
        o.insert("setting_digest".into(), CValue::Str(self.setting_digest.as_str().into()));
        o.insert("source".into(), CValue::Str(self.source.as_str().into()));
        CValue::Obj(o)
    }
}

/// A validated, canonically ordered set of at most 64 [`ProbeCandidate`]s and its SHA-256
/// RFC 8785 digest (REQ-004, REQ-005).
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateSet {
    pub catalogue: String,
    pub candidates: Vec<ProbeCandidate>,
    pub digest: Digest,
}

impl CandidateSet {
    /// Canonical [`CValue`] representation (candidates already sorted by factor catalogue order
    /// then setting digest).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("candidates".into(), CValue::Arr(self.candidates.iter().map(ProbeCandidate::to_value).collect()));
        o.insert("catalogue".into(), CValue::Str(self.catalogue.clone()));
        CValue::Obj(o)
    }

    pub fn to_canonical_json(&self) -> Result<Vec<u8>, CatError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| CatError::Internal("canonical json"))
    }
}

fn non_empty_id(v: Option<&CValue>, field: &str, index: usize, factor: &str) -> Result<String, CatError> {
    match v {
        Some(CValue::Str(s)) if !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_graphic()) => {
            Ok(s.clone())
        }
        _ => Err(CatError::CandidateInvalid {
            index,
            factor: factor.to_string(),
            reason: format!("'{field}' must be 1..=128 printable ASCII chars without spaces"),
        }),
    }
}

/// Validates raw candidate-set JSON bytes against `catalogue` under `caller` (REQ-001..005).
///
/// Accepts `{"candidates": [ { "factor", "setting", "source", "proposer", "session"? }, ... ]}`.
/// When `caller.kind == PrincipalKind::Agent`, every candidate's `source` is forced to
/// `CandidateSource::AgentProposed` with `caller.principal_id` and `caller.session`, ignoring
/// any caller-supplied source or proposer (REQ-003, AC-3).
pub fn validate_candidate_set_json(
    catalogue: &FactorCatalogue,
    caller: &CallerContext,
    bytes: &[u8],
) -> Result<CandidateSet, CatError> {
    let val = parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_INPUT_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => CatError::BadInput("input exceeds size or depth limit".into()),
        other => CatError::BadInput(format!("invalid JSON: {other}")),
    })?;
    validate_candidate_set_value(catalogue, caller, &val)
}

/// Validates a parsed [`CValue`] candidate set against `catalogue` under `caller`.
pub fn validate_candidate_set_value(
    catalogue: &FactorCatalogue,
    caller: &CallerContext,
    val: &CValue,
) -> Result<CandidateSet, CatError> {
    let CValue::Obj(root) = val else {
        return Err(CatError::BadInput("candidate set must be a JSON object".into()));
    };
    if root.keys().any(|k| k != "candidates") {
        return Err(CatError::BadInput("unknown top-level key (expected only 'candidates')".into()));
    }
    let Some(CValue::Arr(raw_list)) = root.get("candidates") else {
        return Err(CatError::BadInput("missing 'candidates' array".into()));
    };
    if raw_list.len() > MAX_CANDIDATES {
        return Err(CatError::CandidateSetTooLarge { count: raw_list.len() });
    }

    let mut validated: Vec<ProbeCandidate> = Vec::with_capacity(raw_list.len());
    for (index, item) in raw_list.iter().enumerate() {
        let CValue::Obj(co) = item else {
            return Err(CatError::CandidateInvalid {
                index,
                factor: "<none>".into(),
                reason: "candidate must be an object".into(),
            });
        };
        let factor_str = match co.get("factor") {
            Some(CValue::Str(s)) => s.clone(),
            _ => {
                return Err(CatError::CandidateInvalid {
                    index,
                    factor: "<missing>".into(),
                    reason: "missing string 'factor'".into(),
                })
            }
        };
        // Reject unknown candidate keys (no free-form cause fields allowed).
        for k in co.keys() {
            if !matches!(k.as_str(), "factor" | "proposer" | "session" | "setting" | "source") {
                return Err(CatError::CandidateInvalid {
                    index,
                    factor: factor_str,
                    reason: format!("unknown candidate key '{k}'"),
                });
            }
        }
        let Some(factor) = Factor::parse(&factor_str) else {
            return Err(CatError::CandidateInvalid {
                index,
                factor: factor_str,
                reason: "factor is outside the closed mig-factors@1 catalogue".into(),
            });
        };
        let Some(setting) = co.get("setting") else {
            return Err(CatError::CandidateInvalid { index, factor: factor_str, reason: "missing 'setting'".into() });
        };
        if let Err(reason) = catalogue.validate_setting(factor, setting) {
            return Err(CatError::CandidateInvalid { index, factor: factor_str, reason });
        }
        let setting_d = fid_canon::digest(setting).map_err(|_| CatError::Internal("canonical setting json"))?;
        let setting_digest = Digest::from_bytes(setting_d.0);

        // Validate declared source syntax even if agent override applies later.
        let declared_source = match co.get("source") {
            Some(CValue::Str(s)) => CandidateSource::parse(s).ok_or_else(|| CatError::CandidateInvalid {
                index,
                factor: factor_str.clone(),
                reason: format!("unknown source {s:?}"),
            })?,
            _ => {
                return Err(CatError::CandidateInvalid {
                    index,
                    factor: factor_str,
                    reason: "missing string 'source'".into(),
                })
            }
        };

        // REQ-003: Agent principal always forces AgentProposed + principal ID + session ID.
        let (source, proposer, session) = if caller.kind == PrincipalKind::Agent {
            let sess = caller.session.clone().filter(|s| !s.is_empty()).ok_or_else(|| CatError::CandidateInvalid {
                index,
                factor: factor_str.clone(),
                reason: "agent principal requires a non-empty session ID".into(),
            })?;
            (CandidateSource::AgentProposed, caller.principal_id.clone(), Some(sess))
        } else {
            let proposer = match co.get("proposer") {
                Some(_) => non_empty_id(co.get("proposer"), "proposer", index, &factor_str)?,
                None => caller.principal_id.clone(),
            };
            let session = match co.get("session") {
                None | Some(CValue::Null) => caller.session.clone(),
                Some(v) => Some(non_empty_id(Some(v), "session", index, &factor_str)?),
            };
            (declared_source, proposer, session)
        };

        validated.push(ProbeCandidate { factor, setting: setting.clone(), setting_digest, source, proposer, session });
    }

    // REQ-005: Sort by factor catalogue order, then setting digest (then provenance tie-break).
    validated.sort_by(|a, b| {
        (a.factor, a.setting_digest.as_str(), a.source, a.proposer.as_str(), a.session.as_deref()).cmp(&(
            b.factor,
            b.setting_digest.as_str(),
            b.source,
            b.proposer.as_str(),
            b.session.as_deref(),
        ))
    });

    let mut set = CandidateSet {
        catalogue: catalogue.version().to_string(),
        candidates: validated,
        digest: Digest::from_bytes([0u8; 32]),
    };
    let d = fid_canon::digest(&set.to_value()).map_err(|_| CatError::Internal("canonical candidate set json"))?;
    set.digest = Digest::from_bytes(d.0);
    Ok(set)
}
