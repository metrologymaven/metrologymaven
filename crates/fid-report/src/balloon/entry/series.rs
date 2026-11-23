//! Multi-part reading series entry (`RPT.BALLOON.ENTRY.SERIES`, `REQ-001`..`REQ-005`,
//! `AC-1`..`AC-5`).
//!
//! `report.enter_series_reading@1` (`W2`, `human_only: true`, `--dry-run`, CLI
//! `fid report enter-series-reading`, MCP `report_enter_series_reading` projected
//! `request_only`): validates up to 500 `(stage, value)` pairs with the exact single-entry
//! validation rules of `RPT.BALLOON.ENTRY.RECORD`, collects EVERY failure index into
//! `report.series_invalid`, and commits one atomic `DIRECT_READING` series event if all pass.

use super::record::{
    validate_single_reading, DirectReadingEvaluator, EntryError, EntryTimestamp, EntryWarning, ManualReading,
    ManualReadingStore, PrincipalId, ReadingId, StageId,
};
use crate::balloon::gauge::GaugeRegister;
use crate::balloon::sheet::UuidV7Gen;
use crate::doc::model::obj;
use crate::fmt::GaugeId;
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_gdt::chars::record::{EvaluationBasis, RunId};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError, QuantityCatalogue};
use std::collections::BTreeSet;
use std::fmt;

/// Registry command / event ID for entering a multi-part reading series (`W2`, `human_only`).
pub const ENTER_SERIES_READING: &str = "report.enter_series_reading@1";

/// Maximum number of `(stage, value)` elements per series command (`REQ-005`, `AC-3`).
pub const MAX_SERIES_ELEMENTS: usize = 500;

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Read-only lookup confirming stage membership in a multi-part run (`CORE.PRJ.STAGE`, `REQ-003`).
pub trait RunStageLookup {
    /// Returns `true` iff `stage` belongs to `run`.
    fn contains_stage(&self, run: &RunId, stage: &StageId) -> bool;
}

/// Parameters of `report.enter_series_reading@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterSeriesParams {
    pub characteristic: CharId,
    pub run: RunId,
    pub gauge: Option<GaugeId>,
    /// Ordered `(stage, raw_value)` entries (at most [`MAX_SERIES_ELEMENTS`]).
    pub values: Vec<(StageId, String)>,
    pub operator: PrincipalId,
    pub entered_at: EntryTimestamp,
    pub dry_run: bool,
}

/// Result of `report.enter_series_reading@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterSeriesResult {
    pub readings: Vec<ManualReading>,
    /// Per-element `(index, warning)` pairs in input stage order (`REQ-004`).
    pub warnings: Vec<(usize, EntryWarning)>,
    pub committed: bool,
}

/// Typed errors of `RPT.BALLOON.ENTRY.SERIES` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeriesError {
    /// `core.human_only`: refused for non-human principals (`REQ-005`, `AC-4`).
    HumanOnly,
    /// `report.series_too_long`: more than 500 elements submitted (`REQ-005`, `AC-3`).
    SeriesTooLong { len: usize, max: usize },
    /// `report.series_invalid`: one or more elements failed validation, duplicate stage, or
    /// stage not in run; carries every failing `(index, error_code)` pair (`REQ-002`, `REQ-003`, `AC-2`, `AC-3`).
    SeriesInvalid { failures: Vec<(usize, &'static str)> },
    /// Series-level error (e.g. missing characteristic, missing gauge, retired/unknown gauge, or empty series).
    Entry(EntryError),
}

impl SeriesError {
    pub fn code(&self) -> &'static str {
        match self {
            SeriesError::HumanOnly => "core.human_only",
            SeriesError::SeriesTooLong { .. } => "report.series_too_long",
            SeriesError::SeriesInvalid { .. } => "report.series_invalid",
            SeriesError::Entry(e) => e.code(),
        }
    }
}

impl fmt::Display for SeriesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeriesError::HumanOnly => write!(f, "core.human_only: series entry requires a human principal"),
            SeriesError::SeriesTooLong { len, max } => {
                write!(f, "report.series_too_long: {len} elements exceeds limit {max}")
            }
            SeriesError::SeriesInvalid { failures } => {
                write!(f, "report.series_invalid: {} failing element(s)", failures.len())
            }
            SeriesError::Entry(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SeriesError {}

/// Executes `report.enter_series_reading@1` (`W2`, `human_only: true`, `--dry-run`, `REQ-001`..`REQ-005`).
///
/// Validates EVERY element and collects ALL failures before mutating anything; on any failure,
/// journals nothing (`REQ-002`, `REQ-003`).
#[allow(clippy::too_many_arguments)]
pub fn enter_series_reading(
    store: &mut ManualReadingStore,
    principal: Principal,
    catalogue: &dyn QuantityCatalogue,
    gauge_reg: &dyn GaugeRegister,
    stages: &dyn RunStageLookup,
    evaluator: &dyn DirectReadingEvaluator,
    ids: &mut dyn UuidV7Gen,
    params: &EnterSeriesParams,
) -> Result<EnterSeriesResult, SeriesError> {
    if principal != Principal::Human {
        return Err(SeriesError::HumanOnly);
    }
    if params.values.is_empty() {
        return Err(SeriesError::Entry(EntryError::BadParams("series must contain at least one element")));
    }
    if params.values.len() > MAX_SERIES_ELEMENTS {
        return Err(SeriesError::SeriesTooLong { len: params.values.len(), max: MAX_SERIES_ELEMENTS });
    }
    let expected_kind = evaluator
        .expected_quantity_kind(&params.characteristic)
        .ok_or(SeriesError::Entry(EntryError::NotFound("characteristic")))?;

    let mut seen_stages: BTreeSet<&StageId> = BTreeSet::new();
    let mut failures: Vec<(usize, &'static str)> = Vec::new();
    let mut validated_items = Vec::with_capacity(params.values.len());

    for (idx, (stage, raw_value)) in params.values.iter().enumerate() {
        if !stages.contains_stage(&params.run, stage) {
            failures.push((idx, "report.stage_not_in_run"));
            continue;
        }
        if !seen_stages.insert(stage) {
            failures.push((idx, "report.duplicate_stage"));
            continue;
        }
        match validate_single_reading(
            raw_value,
            expected_kind,
            catalogue,
            gauge_reg,
            params.gauge.as_ref(),
            params.entered_at.utc_date(),
        ) {
            Ok(v) => validated_items.push((idx, stage, v)),
            Err(e) => failures.push((idx, e.code())),
        }
    }

    if !failures.is_empty() {
        return Err(SeriesError::SeriesInvalid { failures });
    }

    let mut readings = Vec::with_capacity(validated_items.len());
    let mut warnings = Vec::new();
    let base_pos = store.next_pos();

    for (offset, (idx, stage, val)) in validated_items.into_iter().enumerate() {
        let result_ref = evaluator
            .build_direct_reading_record(
                &params.characteristic,
                &params.run,
                Some(stage),
                &val.authored,
                &val.gauge,
                &val.uncertainty,
            )
            .map_err(SeriesError::Entry)?;
        if result_ref.basis != EvaluationBasis::DirectReading {
            return Err(SeriesError::Entry(EntryError::Store("evaluator did not return DIRECT_READING basis".into())));
        }
        let id = ReadingId::new(ids.next_v7().map_err(|e| SeriesError::Entry(EntryError::Store(e.to_string())))?)
            .ok_or_else(|| SeriesError::Entry(EntryError::Store("id generator did not yield UUIDv7".into())))?;
        for w in val.warnings {
            warnings.push((idx, w));
        }
        readings.push(ManualReading {
            id,
            characteristic: params.characteristic.clone(),
            run: params.run.clone(),
            stage: Some(stage.clone()),
            literal: val.authored,
            gauge: val.gauge,
            flags: val.flags,
            operator: params.operator.clone(),
            entered_at: params.entered_at.clone(),
            note: None,
            result: result_ref,
            void_info: None,
            journal_pos: base_pos + offset as u64,
        });
    }

    if params.dry_run {
        return Ok(EnterSeriesResult { readings, warnings, committed: false });
    }

    let committed = store.commit_series(readings).map_err(SeriesError::Entry)?;
    Ok(EnterSeriesResult { readings: committed, warnings, committed: true })
}

/// Returns the validated [`CommandDef`] for `report.enter_series_reading@1` (`W2`,
/// `human_only: true`, `mcp_projection: "request_only"`, CLI `fid report enter-series-reading`).
pub fn enter_series_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let num = |n: usize| crate::json_num::num_usize(n);
    let object = |required: &[&str], properties: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(required)),
            ("properties", properties),
        ])
    };
    let pair_schema = object(
        &["stage", "value"],
        obj([
            ("stage", obj([("type", text("string")), ("minLength", num(1))])),
            (
                "value",
                obj([
                    ("type", text("string")),
                    ("minLength", num(1)),
                    ("maxLength", num(super::record::MAX_VALUE_BYTES)),
                    ("x-untrusted", CValue::Bool(true)),
                ]),
            ),
        ]),
    );

    CommandDef::from_value(obj([
        ("id", text("report.enter_series_reading")),
        ("major", num(1)),
        ("title", text("Enter a multi-part manual reading series")),
        (
            "description",
            text("Validates and commits an ordered series of up to 500 human hand-gauge readings across run stages in one atomic journal event."),
        ),
        (
            "params_schema",
            object(
                &["char", "gauge", "run", "values"],
                obj([
                    ("char", obj([("type", text("string")), ("minLength", num(1))])),
                    ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                    ("gauge", obj([("type", text("string")), ("minLength", num(1))])),
                    ("run", obj([("type", text("string")), ("minLength", num(1))])),
                    (
                        "values",
                        obj([
                            ("type", text("array")),
                            ("minItems", num(1)),
                            ("maxItems", num(MAX_SERIES_ELEMENTS)),
                            ("items", pair_schema),
                        ]),
                    ),
                ]),
            ),
        ),
        (
            "result_schema",
            object(
                &["committed", "readings", "warnings"],
                obj([
                    ("committed", obj([("type", text("boolean"))])),
                    ("readings", obj([("type", text("array")), ("items", obj([("type", text("object"))]))])),
                    ("warnings", obj([("type", text("array")), ("items", obj([("type", text("object"))]))])),
                ]),
            ),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("request_only")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[ENTER_SERIES_READING])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(true)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
