//! Substitution inputs, procedure profiles and condition checks
//! (spec node `UNC.MODEL.SUBST.INPUT`, ISO 15530-3 calibrated-workpiece method).
//!
//! Validates every prerequisite before any substitution uncertainty calculation runs:
//! 1. Required calibrated-reference fields (`value`, `unit`, `u`, `k`, `source_kind` in
//!    `certificate | dcc`, `source_digest`) plus calibrated and production run sets
//!    (REQ-UNC.MODEL.SUBST.INPUT-001).
//! 2. Finite SI values (`si:m`, `si:rad`, `si:1`) consistent with the characteristic kind
//!    (REQ-UNC.MODEL.SUBST.INPUT-004).
//! 3. Exact equality of all six `Conditions` fields between calibrated-workpiece runs and
//!    production measurements; any mismatch fails with `conditions_mismatch` naming every
//!    differing field and marks the input ineligible for UEC-3
//!    (REQ-UNC.MODEL.SUBST.INPUT-003).
//! 4. Minimum run count and per-run repetition count from [`ProcedureProfile`] (default
//!    profile `"default"`: 20 runs of 3 repetitions [unverified: default minimums],
//!    overridable per site; REQ-UNC.MODEL.SUBST.INPUT-002).
//!
//! Determinism: D0 (`BTreeMap` only, zero `HashMap`/`HashSet`, zero panics/unwrap).

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_kunc::dist::UnitCode;
use fid_kunc::statement::SourceKind as StatementSourceKind;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

// ------------------------------------------------------------------ constants

/// Wire schema tag for substitution input JSON documents.
pub const SUBST_INPUT_SCHEMA: &str = "subst-input/1";
/// Default procedure profile identifier.
pub const DEFAULT_PROFILE_ID: &str = "default";
/// Default minimum calibrated-workpiece runs [unverified: default minimums].
pub const DEFAULT_MIN_RUNS: usize = 20;
/// Default minimum repetitions per calibrated-workpiece run [unverified: default minimums].
pub const DEFAULT_MIN_REPETITIONS_PER_RUN: usize = 3;

/// Input byte budget for a substitution input JSON document (16 MiB).
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// JSON nesting depth cap for substitution input parsing.
pub const MAX_JSON_DEPTH: usize = 32;
/// Maximum runs accepted in a single [`RunSet`].
pub const MAX_RUNS: usize = 10_000;
/// Maximum repetitions accepted in a single [`Run`].
pub const MAX_REPETITIONS_PER_RUN: usize = 10_000;
/// Maximum UTF-8 byte length for identifiers (`feature_ref`, `machine_id`, profile `id`, `run_id`).
pub const MAX_IDENT_BYTES: usize = 256;

// ------------------------------------------------------------------ enums & types

/// Characteristic kind evaluated by substitution, pinning its required SI unit
/// (REQ-UNC.MODEL.SUBST.INPUT-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CharacteristicKind {
    // Length characteristics (`si:m`)
    Diameter,
    Radius,
    Distance,
    Length,
    Flatness,
    Roundness,
    Cylindricity,
    Straightness,
    Position,
    Concentricity,
    Runout,
    Profile,
    Parallelism,
    Perpendicularity,
    // Angular characteristics (`si:rad`)
    Angle,
    Angularity,
    ConeAngle,
    // Dimensionless characteristics (`si:1`)
    Ratio,
    Count,
}

impl CharacteristicKind {
    /// Wire string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            CharacteristicKind::Diameter => "diameter",
            CharacteristicKind::Radius => "radius",
            CharacteristicKind::Distance => "distance",
            CharacteristicKind::Length => "length",
            CharacteristicKind::Flatness => "flatness",
            CharacteristicKind::Roundness => "roundness",
            CharacteristicKind::Cylindricity => "cylindricity",
            CharacteristicKind::Straightness => "straightness",
            CharacteristicKind::Position => "position",
            CharacteristicKind::Concentricity => "concentricity",
            CharacteristicKind::Runout => "runout",
            CharacteristicKind::Profile => "profile",
            CharacteristicKind::Parallelism => "parallelism",
            CharacteristicKind::Perpendicularity => "perpendicularity",
            CharacteristicKind::Angle => "angle",
            CharacteristicKind::Angularity => "angularity",
            CharacteristicKind::ConeAngle => "cone_angle",
            CharacteristicKind::Ratio => "ratio",
            CharacteristicKind::Count => "count",
        }
    }

    /// Parses a wire characteristic kind string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "diameter" => Some(CharacteristicKind::Diameter),
            "radius" => Some(CharacteristicKind::Radius),
            "distance" => Some(CharacteristicKind::Distance),
            "length" => Some(CharacteristicKind::Length),
            "flatness" => Some(CharacteristicKind::Flatness),
            "roundness" => Some(CharacteristicKind::Roundness),
            "cylindricity" => Some(CharacteristicKind::Cylindricity),
            "straightness" => Some(CharacteristicKind::Straightness),
            "position" => Some(CharacteristicKind::Position),
            "concentricity" => Some(CharacteristicKind::Concentricity),
            "runout" => Some(CharacteristicKind::Runout),
            "profile" => Some(CharacteristicKind::Profile),
            "parallelism" => Some(CharacteristicKind::Parallelism),
            "perpendicularity" => Some(CharacteristicKind::Perpendicularity),
            "angle" => Some(CharacteristicKind::Angle),
            "angularity" => Some(CharacteristicKind::Angularity),
            "cone_angle" => Some(CharacteristicKind::ConeAngle),
            "ratio" => Some(CharacteristicKind::Ratio),
            "count" => Some(CharacteristicKind::Count),
            _ => None,
        }
    }

    /// The SI unit required for values of this characteristic kind.
    pub const fn expected_unit(self) -> UnitCode {
        match self {
            CharacteristicKind::Diameter
            | CharacteristicKind::Radius
            | CharacteristicKind::Distance
            | CharacteristicKind::Length
            | CharacteristicKind::Flatness
            | CharacteristicKind::Roundness
            | CharacteristicKind::Cylindricity
            | CharacteristicKind::Straightness
            | CharacteristicKind::Position
            | CharacteristicKind::Concentricity
            | CharacteristicKind::Runout
            | CharacteristicKind::Profile
            | CharacteristicKind::Parallelism
            | CharacteristicKind::Perpendicularity => UnitCode::Metre,
            CharacteristicKind::Angle | CharacteristicKind::Angularity | CharacteristicKind::ConeAngle => {
                UnitCode::Radian
            }
            CharacteristicKind::Ratio | CharacteristicKind::Count => UnitCode::One,
        }
    }
}

impl fmt::Display for CharacteristicKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source document kind for the calibrated workpiece (`certificate | dcc`,
/// REQ-UNC.MODEL.SUBST.INPUT-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceSourceKind {
    Certificate,
    Dcc,
}

impl ReferenceSourceKind {
    /// Wire representation (`"certificate"` or `"dcc"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            ReferenceSourceKind::Certificate => "certificate",
            ReferenceSourceKind::Dcc => "dcc",
        }
    }

    /// Parses `"certificate"` or `"dcc"`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "certificate" => Some(ReferenceSourceKind::Certificate),
            "dcc" => Some(ReferenceSourceKind::Dcc),
            _ => None,
        }
    }

    /// Maps to the kernel statement source kind (characterized calibration record).
    pub const fn statement_source_kind(self) -> StatementSourceKind {
        StatementSourceKind::Dcc
    }
}

impl fmt::Display for ReferenceSourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Calibrated workpiece reference specification (optional fields allow callers and parsers
/// to construct partial inputs and receive structured `input_missing{field}` errors).
#[derive(Debug, Clone, PartialEq)]
pub struct CalibratedReference {
    /// Calibrated value in SI units (e.g. `0.0500012` for 50.0012 mm in `si:m`).
    pub value: Option<f64>,
    /// SI unit (`si:m`, `si:rad`, `si:1`).
    pub unit: Option<UnitCode>,
    /// Expanded calibration uncertainty `U` in SI units (`U > 0`, e.g. `6.0e-7` m for 0.6 µm).
    pub expanded_u: Option<f64>,
    /// Coverage factor `k` (`k > 0`, e.g. `2.0`).
    pub k: Option<f64>,
    /// Source kind (`certificate | dcc`).
    pub source_kind: Option<ReferenceSourceKind>,
    /// SHA-256 digest of the calibration certificate or DCC.
    pub source_digest: Option<Digest>,
}

/// Measurement conditions captured for a run set; every field must match between
/// calibrated-workpiece runs and production measurements (REQ-UNC.MODEL.SUBST.INPUT-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conditions {
    pub characteristic_kind: CharacteristicKind,
    pub feature_ref: String,
    pub strategy_digest: Digest,
    pub machine_id: String,
    pub probe_config_digest: Digest,
    pub program_digest: Digest,
}

impl Conditions {
    /// Returns the names of every field that differs from `other`, in declaration order.
    pub fn diff_fields(&self, other: &Conditions) -> Vec<&'static str> {
        let mut diffs = Vec::new();
        if self.characteristic_kind != other.characteristic_kind {
            diffs.push("characteristic_kind");
        }
        if self.feature_ref != other.feature_ref {
            diffs.push("feature_ref");
        }
        if self.strategy_digest != other.strategy_digest {
            diffs.push("strategy_digest");
        }
        if self.machine_id != other.machine_id {
            diffs.push("machine_id");
        }
        if self.probe_config_digest != other.probe_config_digest {
            diffs.push("probe_config_digest");
        }
        if self.program_digest != other.program_digest {
            diffs.push("program_digest");
        }
        diffs
    }

    /// Canonical JSON representation.
    pub fn to_value(&self) -> CValue {
        obj([
            ("characteristic_kind", text(self.characteristic_kind.as_str())),
            ("feature_ref", text(&self.feature_ref)),
            ("machine_id", text(&self.machine_id)),
            ("probe_config_digest", text(self.probe_config_digest.as_str())),
            ("program_digest", text(self.program_digest.as_str())),
            ("strategy_digest", text(self.strategy_digest.as_str())),
        ])
    }

    /// Parses [`Conditions`] from a JSON object.
    pub fn from_value(v: &CValue, path: &str) -> Result<Self, SubstError> {
        let o = expect_obj(
            v,
            path,
            &[
                "characteristic_kind",
                "feature_ref",
                "machine_id",
                "probe_config_digest",
                "program_digest",
                "strategy_digest",
            ],
        )?;
        let kind_str = req_str_or_missing(o, "characteristic_kind", path)?;
        let characteristic_kind = CharacteristicKind::parse(kind_str).ok_or_else(|| {
            invalid_value(format!("{path}.characteristic_kind"), format!("unknown characteristic kind {kind_str:?}"))
        })?;
        let feature_ref = req_str_or_missing(o, "feature_ref", path)?.to_string();
        let machine_id = req_str_or_missing(o, "machine_id", path)?.to_string();
        let strategy_digest = req_digest_or_missing(o, "strategy_digest", path)?;
        let probe_config_digest = req_digest_or_missing(o, "probe_config_digest", path)?;
        let program_digest = req_digest_or_missing(o, "program_digest", path)?;
        Ok(Conditions {
            characteristic_kind,
            feature_ref,
            strategy_digest,
            machine_id,
            probe_config_digest,
            program_digest,
        })
    }
}

/// One measurement run containing repeated observations in SI units.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Human/system identifier for this run (e.g. `"run-01"`).
    pub run_id: String,
    /// Repeated measurement values in SI units.
    pub values: Vec<f64>,
}

/// A set of runs taken under one shared [`Conditions`] and SI unit.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSet {
    /// SI unit of all values in [`Self::runs`].
    pub unit: Option<UnitCode>,
    /// Measurement conditions under which this set was acquired.
    pub conditions: Conditions,
    /// Repeated runs.
    pub runs: Vec<Run>,
}

/// Unchecked input bundle to the substitution method (`subst-input/1`).
#[derive(Debug, Clone, PartialEq)]
pub struct SubstitutionInput {
    /// Schema tag (`subst-input/1`).
    pub schema: String,
    /// Calibrated workpiece reference.
    pub reference: CalibratedReference,
    /// Repeated runs measured on the calibrated workpiece.
    pub calibrated_runs: Option<RunSet>,
    /// Production measurement runs evaluated by substitution.
    pub production_runs: Option<RunSet>,
}

/// Procedure profile setting minimum calibrated-workpiece run and repetition counts
/// (project-defined defaults, overridable per site; REQ-UNC.MODEL.SUBST.INPUT-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureProfile {
    pub id: String,
    pub min_runs: usize,
    pub min_repetitions_per_run: usize,
}

impl ProcedureProfile {
    /// Project default profile: `"default"`, 20 runs of 3 repetitions [unverified: default minimums].
    pub fn default_profile() -> Self {
        ProcedureProfile {
            id: DEFAULT_PROFILE_ID.to_string(),
            min_runs: DEFAULT_MIN_RUNS,
            min_repetitions_per_run: DEFAULT_MIN_REPETITIONS_PER_RUN,
        }
    }

    /// Creates a site-overridden procedure profile, rejecting empty IDs or zero minimums.
    pub fn custom(id: impl Into<String>, min_runs: usize, min_repetitions_per_run: usize) -> Result<Self, SubstError> {
        let id = id.into();
        validate_ident(&id, "profile.id")?;
        if min_runs == 0 || min_runs > MAX_RUNS {
            return Err(invalid_value("profile.min_runs", format!("must be in 1..={MAX_RUNS}")));
        }
        if min_repetitions_per_run == 0 || min_repetitions_per_run > MAX_REPETITIONS_PER_RUN {
            return Err(invalid_value(
                "profile.min_repetitions_per_run",
                format!("must be in 1..={MAX_REPETITIONS_PER_RUN}"),
            ));
        }
        Ok(ProcedureProfile { id, min_runs, min_repetitions_per_run })
    }
}

impl Default for ProcedureProfile {
    fn default() -> Self {
        Self::default_profile()
    }
}

// ------------------------------------------------------------------ errors

/// Which count failed a minimum requirement in [`SubstError::InsufficientRuns`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsufficientScope {
    CalibratedRunCount,
    CalibratedRepetitions,
    ProductionRunCount,
    ProductionRepetitions,
}

impl InsufficientScope {
    /// Short wire label for diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            InsufficientScope::CalibratedRunCount => "calibrated_runs",
            InsufficientScope::CalibratedRepetitions => "calibrated_repetitions",
            InsufficientScope::ProductionRunCount => "production_runs",
            InsufficientScope::ProductionRepetitions => "production_repetitions",
        }
    }
}

/// Typed errors returned by [`check`] and [`SubstitutionInput::from_json`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubstError {
    /// Required field is missing (`input_missing{field}`, REQ-001, AC-4).
    InputMissing { field: String },
    /// Run or repetition count is below the procedure profile minimum (`insufficient_runs`, REQ-002, AC-2).
    InsufficientRuns { scope: InsufficientScope, required: usize, supplied: usize, run_index: Option<usize> },
    /// One or more `Conditions` fields differ between calibrated and production sets
    /// (`conditions_mismatch{fields}`, REQ-003, AC-3; never eligible for UEC-3).
    ConditionsMismatch { fields: Vec<&'static str>, uec3_eligible: bool },
    /// Non-finite value, non-positive uncertainty/k, non-SI unit, or unit inconsistent with
    /// characteristic kind (`invalid_value`, REQ-004).
    InvalidValue { field: String, reason: String },
    /// I/O error reading a fixture file.
    Io { reason: String },
}

impl SubstError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            SubstError::InputMissing { .. } => "input_missing",
            SubstError::InsufficientRuns { .. } => "insufficient_runs",
            SubstError::ConditionsMismatch { .. } => "conditions_mismatch",
            SubstError::InvalidValue { .. } => "invalid_value",
            SubstError::Io { .. } => "io_error",
        }
    }

    /// Whether an input that produced this error is eligible for UEC-3 (always `false` on error).
    pub const fn uec3_eligible(&self) -> bool {
        false
    }
}

impl fmt::Display for SubstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubstError::InputMissing { field } => write!(f, "input_missing{{{field}}}"),
            SubstError::InsufficientRuns { scope, required, supplied, run_index } => match run_index {
                Some(idx) => write!(
                    f,
                    "insufficient_runs{{scope: {}, run_index: {idx}, required: {required}, supplied: {supplied}}}",
                    scope.as_str()
                ),
                None => write!(f, "insufficient_runs{{required: {required}, supplied: {supplied}}}"),
            },
            SubstError::ConditionsMismatch { fields, .. } => {
                write!(f, "conditions_mismatch{{{}}}", fields.join(","))
            }
            SubstError::InvalidValue { field, reason } => write!(f, "invalid_value{{{field}: {reason}}}"),
            SubstError::Io { reason } => write!(f, "io_error: {reason}"),
        }
    }
}

impl std::error::Error for SubstError {}

fn input_missing(field: impl Into<String>) -> SubstError {
    SubstError::InputMissing { field: field.into() }
}

fn invalid_value(field: impl Into<String>, reason: impl Into<String>) -> SubstError {
    SubstError::InvalidValue { field: field.into(), reason: reason.into() }
}

// ------------------------------------------------------------------ CheckedInput (sealed)

/// Validated calibrated reference with guaranteed finite SI values and positive `U`, `k`.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedReference {
    value: f64,
    unit: UnitCode,
    expanded_u: f64,
    k: f64,
    source_kind: ReferenceSourceKind,
    source_digest: Digest,
}

impl CheckedReference {
    pub const fn value(&self) -> f64 {
        self.value
    }
    pub const fn unit(&self) -> UnitCode {
        self.unit
    }
    pub const fn expanded_u(&self) -> f64 {
        self.expanded_u
    }
    pub const fn k(&self) -> f64 {
        self.k
    }
    pub const fn source_kind(&self) -> ReferenceSourceKind {
        self.source_kind
    }
    pub const fn source_digest(&self) -> Digest {
        self.source_digest
    }
}

/// Validated run set with guaranteed SI unit and finite observations.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedRunSet {
    unit: UnitCode,
    conditions: Conditions,
    runs: Vec<Run>,
}

impl CheckedRunSet {
    pub const fn unit(&self) -> UnitCode {
        self.unit
    }
    pub fn conditions(&self) -> &Conditions {
        &self.conditions
    }
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }
    pub fn total_observations(&self) -> usize {
        self.runs.iter().map(|r| r.values.len()).sum()
    }
}

/// Validated substitution input produced exclusively by [`check`].
///
/// Guarantees:
/// - All required reference fields are present and valid (`U > 0`, `k > 0`, finite).
/// - All values are finite and expressed in the SI unit matching `characteristic_kind`.
/// - Calibrated-workpiece and production [`Conditions`] match on every field (`uec3_eligible = true`).
/// - Run and repetition counts satisfy the [`ProcedureProfile`] minimums.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedInput {
    profile: ProcedureProfile,
    reference: CheckedReference,
    calibrated_runs: CheckedRunSet,
    production_runs: CheckedRunSet,
    uec3_eligible: bool,
}

impl CheckedInput {
    pub fn profile(&self) -> &ProcedureProfile {
        &self.profile
    }
    pub fn reference(&self) -> &CheckedReference {
        &self.reference
    }
    pub fn calibrated_runs(&self) -> &CheckedRunSet {
        &self.calibrated_runs
    }
    pub fn production_runs(&self) -> &CheckedRunSet {
        &self.production_runs
    }
    /// Always `true` for a [`CheckedInput`] (since mismatched conditions fail `check`).
    pub const fn uec3_eligible(&self) -> bool {
        self.uec3_eligible
    }

    /// Canonical JSON representation (with IEEE-754 hex bit encodings for D0 determinism).
    pub fn to_canonical_value(&self) -> CValue {
        let hex = |x: f64| text(&format!("0x{:016x}", x.to_bits()));
        let encode_runs = |runs: &[Run]| {
            CValue::Arr(
                runs.iter()
                    .map(|r| {
                        obj([
                            ("run_id", text(&r.run_id)),
                            ("values_hex", CValue::Arr(r.values.iter().map(|v| hex(*v)).collect())),
                        ])
                    })
                    .collect(),
            )
        };
        obj([
            (
                "calibrated_runs",
                obj([
                    ("conditions", self.calibrated_runs.conditions.to_value()),
                    ("runs", encode_runs(&self.calibrated_runs.runs)),
                    ("unit", text(self.calibrated_runs.unit.as_str())),
                ]),
            ),
            (
                "production_runs",
                obj([
                    ("conditions", self.production_runs.conditions.to_value()),
                    ("runs", encode_runs(&self.production_runs.runs)),
                    ("unit", text(self.production_runs.unit.as_str())),
                ]),
            ),
            (
                "profile",
                obj([
                    ("id", text(&self.profile.id)),
                    ("min_repetitions_per_run", CValue::Num(self.profile.min_repetitions_per_run as f64)),
                    ("min_runs", CValue::Num(self.profile.min_runs as f64)),
                ]),
            ),
            (
                "reference",
                obj([
                    ("k_hex", hex(self.reference.k)),
                    ("source_digest", text(self.reference.source_digest.as_str())),
                    ("source_kind", text(self.reference.source_kind.as_str())),
                    ("u_hex", hex(self.reference.expanded_u)),
                    ("unit", text(self.reference.unit.as_str())),
                    ("value_hex", hex(self.reference.value)),
                ]),
            ),
            ("schema", text(SUBST_INPUT_SCHEMA)),
            ("uec3_eligible", CValue::Bool(self.uec3_eligible)),
        ])
    }

    /// SHA-256 digest of the RFC 8785 canonical JSON of this validated input (D0).
    pub fn digest(&self) -> Result<Digest, SubstError> {
        let bytes = to_canonical_bytes(&self.to_canonical_value())
            .map_err(|_| invalid_value("checked_input", "canonical JSON encoding failed"))?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

// ------------------------------------------------------------------ check()

/// Validates a [`SubstitutionInput`] against a [`ProcedureProfile`], returning a sealed
/// [`CheckedInput`] or a typed [`SubstError`] (REQ-UNC.MODEL.SUBST.INPUT-001..004).
pub fn check(input: &SubstitutionInput, profile: &ProcedureProfile) -> Result<CheckedInput, SubstError> {
    // 0. Validate profile itself.
    let validated_profile =
        ProcedureProfile::custom(profile.id.clone(), profile.min_runs, profile.min_repetitions_per_run)?;
    if input.schema != SUBST_INPUT_SCHEMA {
        return Err(invalid_value("schema", format!("expected {SUBST_INPUT_SCHEMA:?}")));
    }

    // 1. Required fields (REQ-001, AC-4).
    let value = input.reference.value.ok_or_else(|| input_missing("value"))?;
    let ref_unit = input.reference.unit.ok_or_else(|| input_missing("unit"))?;
    let expanded_u = input.reference.expanded_u.ok_or_else(|| input_missing("u"))?;
    let k = input.reference.k.ok_or_else(|| input_missing("k"))?;
    let source_kind = input.reference.source_kind.ok_or_else(|| input_missing("source_kind"))?;
    let source_digest = input.reference.source_digest.ok_or_else(|| input_missing("source_digest"))?;
    let cal = input.calibrated_runs.as_ref().ok_or_else(|| input_missing("calibrated_runs"))?;
    let prod = input.production_runs.as_ref().ok_or_else(|| input_missing("production_runs"))?;
    let cal_unit = cal.unit.ok_or_else(|| input_missing("calibrated_runs.unit"))?;
    let prod_unit = prod.unit.ok_or_else(|| input_missing("production_runs.unit"))?;

    if cal.conditions.feature_ref.is_empty() {
        return Err(input_missing("calibrated_runs.conditions.feature_ref"));
    }
    if cal.conditions.machine_id.is_empty() {
        return Err(input_missing("calibrated_runs.conditions.machine_id"));
    }
    if prod.conditions.feature_ref.is_empty() {
        return Err(input_missing("production_runs.conditions.feature_ref"));
    }
    if prod.conditions.machine_id.is_empty() {
        return Err(input_missing("production_runs.conditions.machine_id"));
    }
    validate_ident(&cal.conditions.feature_ref, "calibrated_runs.conditions.feature_ref")?;
    validate_ident(&cal.conditions.machine_id, "calibrated_runs.conditions.machine_id")?;
    validate_ident(&prod.conditions.feature_ref, "production_runs.conditions.feature_ref")?;
    validate_ident(&prod.conditions.machine_id, "production_runs.conditions.machine_id")?;

    // 2. Numeric finiteness, positivity, and SI unit consistency (REQ-004).
    if !value.is_finite() {
        return Err(invalid_value("reference.value", "must be finite"));
    }
    if !expanded_u.is_finite() || expanded_u <= 0.0 {
        return Err(invalid_value("reference.u", "must be finite and > 0"));
    }
    if !k.is_finite() || k <= 0.0 {
        return Err(invalid_value("reference.k", "must be finite and > 0"));
    }

    let cal_expected_unit = cal.conditions.characteristic_kind.expected_unit();
    if ref_unit != cal_expected_unit {
        return Err(invalid_value(
            "reference.unit",
            format!(
                "unit {} inconsistent with characteristic kind {} (expected {})",
                ref_unit.as_str(),
                cal.conditions.characteristic_kind.as_str(),
                cal_expected_unit.as_str()
            ),
        ));
    }
    if cal_unit != cal_expected_unit {
        return Err(invalid_value(
            "calibrated_runs.unit",
            format!(
                "unit {} inconsistent with characteristic kind {} (expected {})",
                cal_unit.as_str(),
                cal.conditions.characteristic_kind.as_str(),
                cal_expected_unit.as_str()
            ),
        ));
    }
    let prod_expected_unit = prod.conditions.characteristic_kind.expected_unit();
    if prod_unit != prod_expected_unit {
        return Err(invalid_value(
            "production_runs.unit",
            format!(
                "unit {} inconsistent with characteristic kind {} (expected {})",
                prod_unit.as_str(),
                prod.conditions.characteristic_kind.as_str(),
                prod_expected_unit.as_str()
            ),
        ));
    }

    validate_run_values(&cal.runs, "calibrated_runs")?;
    validate_run_values(&prod.runs, "production_runs")?;

    // 3. Conditions equivalence (REQ-003, AC-3).
    let mismatched = cal.conditions.diff_fields(&prod.conditions);
    if !mismatched.is_empty() {
        return Err(SubstError::ConditionsMismatch { fields: mismatched, uec3_eligible: false });
    }

    // 4. Minimum run & repetition counts (REQ-002, AC-2).
    if cal.runs.len() < validated_profile.min_runs {
        return Err(SubstError::InsufficientRuns {
            scope: InsufficientScope::CalibratedRunCount,
            required: validated_profile.min_runs,
            supplied: cal.runs.len(),
            run_index: None,
        });
    }
    for (idx, run) in cal.runs.iter().enumerate() {
        if run.values.len() < validated_profile.min_repetitions_per_run {
            return Err(SubstError::InsufficientRuns {
                scope: InsufficientScope::CalibratedRepetitions,
                required: validated_profile.min_repetitions_per_run,
                supplied: run.values.len(),
                run_index: Some(idx),
            });
        }
    }
    if prod.runs.is_empty() {
        return Err(SubstError::InsufficientRuns {
            scope: InsufficientScope::ProductionRunCount,
            required: 1,
            supplied: 0,
            run_index: None,
        });
    }
    for (idx, run) in prod.runs.iter().enumerate() {
        if run.values.is_empty() {
            return Err(SubstError::InsufficientRuns {
                scope: InsufficientScope::ProductionRepetitions,
                required: 1,
                supplied: 0,
                run_index: Some(idx),
            });
        }
    }

    Ok(CheckedInput {
        profile: validated_profile,
        reference: CheckedReference { value, unit: ref_unit, expanded_u, k, source_kind, source_digest },
        calibrated_runs: CheckedRunSet { unit: cal_unit, conditions: cal.conditions.clone(), runs: cal.runs.clone() },
        production_runs: CheckedRunSet {
            unit: prod_unit,
            conditions: prod.conditions.clone(),
            runs: prod.runs.clone(),
        },
        uec3_eligible: true,
    })
}

fn validate_run_values(runs: &[Run], label: &str) -> Result<(), SubstError> {
    if runs.len() > MAX_RUNS {
        return Err(invalid_value(label, format!("exceeds MAX_RUNS ({MAX_RUNS})")));
    }
    for (r_idx, run) in runs.iter().enumerate() {
        validate_ident(&run.run_id, &format!("{label}.runs[{r_idx}].run_id"))?;
        if run.values.len() > MAX_REPETITIONS_PER_RUN {
            return Err(invalid_value(
                format!("{label}.runs[{r_idx}].values"),
                format!("exceeds MAX_REPETITIONS_PER_RUN ({MAX_REPETITIONS_PER_RUN})"),
            ));
        }
        for (v_idx, &v) in run.values.iter().enumerate() {
            if !v.is_finite() {
                return Err(invalid_value(format!("{label}.runs[{r_idx}].values[{v_idx}]"), "must be finite"));
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ bounded JSON parsing

impl SubstitutionInput {
    /// Parses a `subst-input/1` JSON document with strict byte, depth and array limits.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SubstError> {
        let v = parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| invalid_value("input", "not acceptable JSON or exceeds size/depth limit"))?;
        Self::from_value(&v)
    }

    /// Loads and parses a `subst-input/1` JSON file with a byte cap before reading.
    pub fn load_file(path: &Path) -> Result<Self, SubstError> {
        let meta = std::fs::metadata(path).map_err(|e| SubstError::Io { reason: e.to_string() })?;
        if meta.len() > MAX_INPUT_BYTES as u64 {
            return Err(invalid_value("input", "file exceeds MAX_INPUT_BYTES"));
        }
        let bytes = std::fs::read(path).map_err(|e| SubstError::Io { reason: e.to_string() })?;
        Self::from_json(&bytes)
    }

    /// Parses a [`SubstitutionInput`] from a [`CValue`].
    pub fn from_value(v: &CValue) -> Result<Self, SubstError> {
        let o =
            expect_obj(v, "input", &["calibrated_runs", "license", "production_runs", "reference", "schema", "title"])?;
        let schema = match o.get("schema") {
            Some(CValue::Str(s)) => s.clone(),
            None => return Err(input_missing("schema")),
            Some(_) => return Err(invalid_value("schema", "expected string")),
        };
        let reference = match o.get("reference") {
            Some(rv) => parse_reference(rv)?,
            None => return Err(input_missing("reference")),
        };
        let calibrated_runs = match o.get("calibrated_runs") {
            None | Some(CValue::Null) => None,
            Some(cv) => Some(parse_run_set(cv, "calibrated_runs")?),
        };
        let production_runs = match o.get("production_runs") {
            None | Some(CValue::Null) => None,
            Some(pv) => Some(parse_run_set(pv, "production_runs")?),
        };
        Ok(SubstitutionInput { schema, reference, calibrated_runs, production_runs })
    }
}

fn parse_reference(v: &CValue) -> Result<CalibratedReference, SubstError> {
    let o = expect_obj(v, "reference", &["k", "source_digest", "source_kind", "u", "unit", "value"])?;
    let value = opt_f64(o.get("value"), "reference.value")?;
    let unit = opt_si_unit(o.get("unit"), "reference.unit")?;
    let expanded_u = opt_f64(o.get("u"), "reference.u")?;
    let k = opt_f64(o.get("k"), "reference.k")?;
    let source_kind = match o.get("source_kind") {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(
            ReferenceSourceKind::parse(s)
                .ok_or_else(|| invalid_value("reference.source_kind", "expected \"certificate\" or \"dcc\""))?,
        ),
        Some(_) => return Err(invalid_value("reference.source_kind", "expected string")),
    };
    let source_digest = match o.get("source_digest") {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => {
            Some(Digest::parse(s).map_err(|_| invalid_value("reference.source_digest", "invalid sha256 digest"))?)
        }
        Some(_) => return Err(invalid_value("reference.source_digest", "expected digest string")),
    };
    Ok(CalibratedReference { value, unit, expanded_u, k, source_kind, source_digest })
}

fn parse_run_set(v: &CValue, path: &str) -> Result<RunSet, SubstError> {
    let o = expect_obj(v, path, &["conditions", "runs", "unit"])?;
    let unit = opt_si_unit(o.get("unit"), &format!("{path}.unit"))?;
    let conditions = match o.get("conditions") {
        Some(cv) => Conditions::from_value(cv, &format!("{path}.conditions"))?,
        None => return Err(input_missing(format!("{path}.conditions"))),
    };
    let runs_arr = match o.get("runs") {
        Some(CValue::Arr(a)) => a,
        None => return Err(input_missing(format!("{path}.runs"))),
        Some(_) => return Err(invalid_value(format!("{path}.runs"), "expected array")),
    };
    if runs_arr.len() > MAX_RUNS {
        return Err(invalid_value(format!("{path}.runs"), format!("exceeds MAX_RUNS ({MAX_RUNS})")));
    }
    let mut runs = Vec::with_capacity(runs_arr.len());
    for (idx, item) in runs_arr.iter().enumerate() {
        let rpath = format!("{path}.runs[{idx}]");
        let ro = expect_obj(item, &rpath, &["run_id", "values"])?;
        let run_id = match ro.get("run_id") {
            Some(CValue::Str(s)) => s.clone(),
            None => format!("run-{:04}", idx + 1),
            Some(_) => return Err(invalid_value(format!("{rpath}.run_id"), "expected string")),
        };
        let vals_arr = match ro.get("values") {
            Some(CValue::Arr(a)) => a,
            None => return Err(input_missing(format!("{rpath}.values"))),
            Some(_) => return Err(invalid_value(format!("{rpath}.values"), "expected array")),
        };
        if vals_arr.len() > MAX_REPETITIONS_PER_RUN {
            return Err(invalid_value(
                format!("{rpath}.values"),
                format!("exceeds MAX_REPETITIONS_PER_RUN ({MAX_REPETITIONS_PER_RUN})"),
            ));
        }
        let mut values = Vec::with_capacity(vals_arr.len());
        for (v_idx, val) in vals_arr.iter().enumerate() {
            let CValue::Num(x) = val else {
                return Err(invalid_value(format!("{rpath}.values[{v_idx}]"), "expected finite number"));
            };
            values.push(*x);
        }
        runs.push(Run { run_id, values });
    }
    Ok(RunSet { unit, conditions, runs })
}

// ------------------------------------------------------------------ JSON & validation helpers

fn validate_ident(s: &str, field: &str) -> Result<(), SubstError> {
    if s.is_empty() {
        return Err(invalid_value(field, "must not be empty"));
    }
    if s.len() > MAX_IDENT_BYTES {
        return Err(invalid_value(field, format!("exceeds {MAX_IDENT_BYTES} bytes")));
    }
    if s.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(invalid_value(field, "must not contain control characters"));
    }
    Ok(())
}

fn opt_f64(v: Option<&CValue>, field: &str) -> Result<Option<f64>, SubstError> {
    match v {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Num(x)) => Ok(Some(*x)),
        Some(_) => Err(invalid_value(field, "expected number")),
    }
}

fn opt_si_unit(v: Option<&CValue>, field: &str) -> Result<Option<UnitCode>, SubstError> {
    match v {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => {
            let u = UnitCode::parse(s)
                .ok_or_else(|| invalid_value(field, format!("non-SI unit {s:?}; expected si:m, si:rad, or si:1")))?;
            Ok(Some(u))
        }
        Some(_) => Err(invalid_value(field, "expected SI unit string")),
    }
}

fn expect_obj<'a>(v: &'a CValue, path: &str, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, SubstError> {
    let CValue::Obj(o) = v else {
        return Err(invalid_value(path, "expected JSON object"));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid_value(format!("{path}.{k}"), "unknown field"));
        }
    }
    Ok(o)
}

fn req_str_or_missing<'a>(o: &'a BTreeMap<String, CValue>, key: &str, path: &str) -> Result<&'a str, SubstError> {
    match o.get(key) {
        None | Some(CValue::Null) => Err(input_missing(format!("{path}.{key}"))),
        Some(CValue::Str(s)) => Ok(s.as_str()),
        Some(_) => Err(invalid_value(format!("{path}.{key}"), "expected string")),
    }
}

fn req_digest_or_missing(o: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<Digest, SubstError> {
    let s = req_str_or_missing(o, key, path)?;
    Digest::parse(s).map_err(|_| invalid_value(format!("{path}.{key}"), "invalid sha256 digest"))
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
