//! Characteristic table block with column and row customisation
//! (spec node `RPT.MODEL.BLOCKS.TABLE`).
//!
//! Registers block kind `table` (payload schema v1), implements [`BlockKind`] and
//! [`BlockResolver`], resolves table headers and cells strictly through [`crate::fmt`]
//! (never holding float types or formatting numbers outside `fmt`), and exposes the
//! `report.set_table_block@1` (W2, D0) command with `--json` and CLI entry points.

use crate::bind::error::BindError;
use crate::bind::journal::Lookup;
use crate::bind::model::{Binding, ValueBinding, BINDING_KEY};
use crate::bind::resolve::{BlockResolver, ResolveContext, Resolved};
use crate::doc::defs::ID_PATTERN;
use crate::doc::kind::{BlockKind, PayloadError};
use crate::doc::model::parse_canonical_uuid;
use crate::doc::{
    BlockKindId, CanonicalJson, KindRegistry, Op, ReportBackend, ReportError, ReportEvent, ReportId, StoredReport,
    Text, TextFault, Untrusted, SET_TABLE_BLOCK,
};
use crate::fmt::{
    self as rpt_fmt, Characteristic, FmtContext, FmtError, Label, OutcomeText, UncertaintyText, ValueRecord, ValueText,
};
use crate::tmpl::slot::Outcome;
use fid_canon::CValue;
use fid_gdt::chars::record::EvaluationBasis;
use fid_kdec::Outcome as DecOutcome;
use fid_project::Uuid;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registered kind ID (`table`).
pub const TABLE_KIND_ID: &str = "table";
/// Schema marker written inside every `table` v1 payload.
pub const TABLE_SCHEMA: &str = "table_block@1";
/// Minimum columns per table block (REQ-RPT.MODEL.BLOCKS.TABLE-002).
pub const MIN_COLUMNS: usize = 1;
/// Maximum columns per table block (REQ-RPT.MODEL.BLOCKS.TABLE-002).
pub const MAX_COLUMNS: usize = 64;
/// Maximum UTF-8 byte length of a renamed column header (REQ-RPT.MODEL.BLOCKS.TABLE-002).
pub const MAX_HEADER_BYTES: usize = 128;
/// Maximum UTF-8 byte length of a filter tag ID.
pub const MAX_TAG_BYTES: usize = 128;
/// Maximum tags in one row filter.
pub const MAX_FILTER_TAGS: usize = 256;
/// Fixed display text for a missing value cell (REQ-RPT.MODEL.BLOCKS.TABLE-005).
pub const MISSING_TEXT: &str = "no result";
/// CLI subcommand (`fid report set-table-block`).
pub const CLI_SET_TABLE: &str = "set-table-block";

// ------------------------------------------------------------------ errors

/// Typed errors of the `table` block kind, resolver and `report.set_table_block@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    /// `report.column_unknown`: a column source is not in the closed list (REQ-003).
    ColumnUnknown(String),
    /// `report.invalid_block_payload`: payload violates v1 schema or bounds (REQ-001).
    InvalidBlockPayload(PayloadError),
    /// `report.not_found`: named report or block does not exist.
    NotFound(Uuid),
    /// `report.text_invalid`: renamed header or tag exceeds bounds or contains NUL.
    TextInvalid(TextFault),
    /// `report.bad_params`: malformed command or CLI arguments.
    BadParams(&'static str),
    /// Formatting error propagated from `RPT.MODEL.FMT`.
    Format(FmtError),
    /// Document/storage error propagated from `RPT.MODEL.DOC`.
    Report(ReportError),
}

impl TableError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            TableError::ColumnUnknown(_) => "report.column_unknown",
            TableError::InvalidBlockPayload(_) => "report.invalid_block_payload",
            TableError::NotFound(_) => "report.not_found",
            TableError::TextInvalid(_) => "report.text_invalid",
            TableError::BadParams(_) => "report.bad_params",
            TableError::Format(e) => e.code(),
            TableError::Report(e) => e.code(),
        }
    }

    /// Canonical `--json` / MCP error object.
    pub fn to_value(&self) -> CValue {
        match self {
            TableError::Report(e) => e.to_value(),
            other => {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(other.code().to_string()));
                match other {
                    TableError::ColumnUnknown(name) => {
                        let safe = PayloadError::new(name, "column_unknown");
                        o.insert("column".to_string(), CValue::Str(safe.field().to_string()));
                    }
                    TableError::InvalidBlockPayload(p) => {
                        o.insert("field".to_string(), CValue::Str(p.field().to_string()));
                        o.insert("reason".to_string(), CValue::Str(p.reason().to_string()));
                    }
                    TableError::NotFound(id) => {
                        o.insert("id".to_string(), CValue::Str(id.to_string()));
                    }
                    TableError::TextInvalid(f) => {
                        o.insert("reason".to_string(), CValue::Str(f.as_str().to_string()));
                    }
                    TableError::BadParams(r) => {
                        o.insert("reason".to_string(), CValue::Str((*r).to_string()));
                    }
                    TableError::Format(e) => {
                        o.insert("detail".to_string(), CValue::Str(e.to_string()));
                    }
                    TableError::Report(_) => {}
                }
                CValue::Obj(o)
            }
        }
    }
}

impl From<ReportError> for TableError {
    fn from(err: ReportError) -> Self {
        match err {
            ReportError::NotFound(id) => TableError::NotFound(id),
            ReportError::InvalidBlockPayload(p) => TableError::InvalidBlockPayload(p),
            ReportError::TextInvalid(f) => TableError::TextInvalid(f),
            other => TableError::Report(other),
        }
    }
}

impl From<FmtError> for TableError {
    fn from(err: FmtError) -> Self {
        TableError::Format(err)
    }
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::ColumnUnknown(c) => write!(f, "report.column_unknown: {c}"),
            TableError::InvalidBlockPayload(p) => write!(f, "report.invalid_block_payload: {p}"),
            TableError::NotFound(id) => write!(f, "report.not_found: {id}"),
            TableError::TextInvalid(t) => write!(f, "report.text_invalid: {}", t.as_str()),
            TableError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            TableError::Format(e) => write!(f, "{e}"),
            TableError::Report(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TableError {}

// ------------------------------------------------------------------ closed column list

/// Closed list of selectable column sources (REQ-RPT.MODEL.BLOCKS.TABLE-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColumnSource {
    /// Characteristic stable ID.
    Id,
    /// Characteristic display name.
    Name,
    /// Characteristic type token.
    CharType,
    /// Nominal value (formatted via `RPT.MODEL.FMT`).
    Nominal,
    /// Lower specification limit (formatted via `RPT.MODEL.FMT`).
    LowerLimit,
    /// Upper specification limit (formatted via `RPT.MODEL.FMT`).
    UpperLimit,
    /// Measured value (formatted via `RPT.MODEL.FMT`).
    Measured,
    /// Deviation `measured - nominal` (formatted via `RPT.MODEL.FMT`).
    Deviation,
    /// Decision outcome (formatted via `RPT.MODEL.FMT`).
    Decision,
    /// Decision rule ID.
    DecisionRule,
    /// Uncertainty status (UEC-0 shows `"not evaluated"`).
    Uncertainty,
    /// Evaluation basis (`MEASURED`, `RESULTS_ONLY`, `DIRECT_READING`, ...).
    EvaluationBasis,
    /// Source / provenance label (transcribed, gauge, claimed, declared input, ...).
    SourceLabel,
    /// Stage identifier or label.
    Stage,
    /// Run identifier or label.
    Run,
    /// Characteristic tags.
    Tag,
}

impl ColumnSource {
    /// Every legal column source, in declaration order.
    pub const ALL: [ColumnSource; 16] = [
        ColumnSource::Id,
        ColumnSource::Name,
        ColumnSource::CharType,
        ColumnSource::Nominal,
        ColumnSource::LowerLimit,
        ColumnSource::UpperLimit,
        ColumnSource::Measured,
        ColumnSource::Deviation,
        ColumnSource::Decision,
        ColumnSource::DecisionRule,
        ColumnSource::Uncertainty,
        ColumnSource::EvaluationBasis,
        ColumnSource::SourceLabel,
        ColumnSource::Stage,
        ColumnSource::Run,
        ColumnSource::Tag,
    ];

    /// Stable canonical wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            ColumnSource::Id => "id",
            ColumnSource::Name => "name",
            ColumnSource::CharType => "type",
            ColumnSource::Nominal => "nominal",
            ColumnSource::LowerLimit => "lower_limit",
            ColumnSource::UpperLimit => "upper_limit",
            ColumnSource::Measured => "measured",
            ColumnSource::Deviation => "deviation",
            ColumnSource::Decision => "decision",
            ColumnSource::DecisionRule => "decision_rule",
            ColumnSource::Uncertainty => "uncertainty",
            ColumnSource::EvaluationBasis => "evaluation_basis",
            ColumnSource::SourceLabel => "source_label",
            ColumnSource::Stage => "stage",
            ColumnSource::Run => "run",
            ColumnSource::Tag => "tag",
        }
    }

    /// Parses a wire string strictly against the closed list.
    pub fn parse(s: &str) -> Option<ColumnSource> {
        ColumnSource::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// Default English column header when no custom rename is stored.
    pub const fn default_header(self) -> &'static str {
        match self {
            ColumnSource::Id => "ID",
            ColumnSource::Name => "Name",
            ColumnSource::CharType => "Type",
            ColumnSource::Nominal => "Nominal",
            ColumnSource::LowerLimit => "Lower Limit",
            ColumnSource::UpperLimit => "Upper Limit",
            ColumnSource::Measured => "Measured",
            ColumnSource::Deviation => "Deviation",
            ColumnSource::Decision => "Decision",
            ColumnSource::DecisionRule => "Decision Rule",
            ColumnSource::Uncertainty => "Uncertainty",
            ColumnSource::EvaluationBasis => "Evaluation Basis",
            ColumnSource::SourceLabel => "Source Label",
            ColumnSource::Stage => "Stage",
            ColumnSource::Run => "Run",
            ColumnSource::Tag => "Tag",
        }
    }
}

// ------------------------------------------------------------------ payload model

/// Bounded untrusted tag identifier used in [`RowFilter::tags`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagId(String);

impl TagId {
    /// Checks 1..=[`MAX_TAG_BYTES`] bytes of UTF-8 with no NUL.
    pub fn new(s: &str) -> Result<TagId, TableError> {
        if s.is_empty() {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/tags", "empty_tag")));
        }
        if s.contains('\0') {
            return Err(TableError::TextInvalid(TextFault::Nul));
        }
        if s.len() > MAX_TAG_BYTES {
            return Err(TableError::TextInvalid(TextFault::TooLong));
        }
        Ok(TagId(s.to_string()))
    }

    /// Raw untrusted tag bytes.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validates a renamed column header (at most [`MAX_HEADER_BYTES`] bytes of UTF-8, no NUL).
pub fn parse_header(s: &str) -> Result<Untrusted<Text>, TableError> {
    if s.contains('\0') {
        return Err(TableError::TextInvalid(TextFault::Nul));
    }
    if s.len() > MAX_HEADER_BYTES {
        return Err(TableError::TextInvalid(TextFault::TooLong));
    }
    Untrusted::text(s).map_err(TableError::from)
}

/// One column specification in a `table` block payload (REQ-RPT.MODEL.BLOCKS.TABLE-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Closed column source.
    pub source: ColumnSource,
    /// Optional renamed header (at most 128 bytes of untrusted text).
    pub header: Option<Untrusted<Text>>,
}

impl ColumnSpec {
    /// Column with default header.
    pub fn new(source: ColumnSource) -> Self {
        ColumnSpec { source, header: None }
    }

    /// Column with a validated custom header.
    pub fn with_header(source: ColumnSource, header: &str) -> Result<Self, TableError> {
        Ok(ColumnSpec { source, header: Some(parse_header(header)?) })
    }

    /// Effective column header text.
    pub fn effective_header(&self) -> &str {
        match &self.header {
            Some(h) => h.untrusted_value().as_str(),
            None => self.source.default_header(),
        }
    }

    /// Canonical JSON representation.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("source".to_string(), CValue::Str(self.source.as_str().to_string()));
        if let Some(h) = &self.header {
            o.insert("header".to_string(), h.to_value());
        }
        CValue::Obj(o)
    }

    fn from_value(v: &CValue, path: &str) -> Result<ColumnSpec, TableError> {
        let CValue::Obj(o) = v else {
            return Err(TableError::InvalidBlockPayload(PayloadError::new(path, "not_object")));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "source" | "header")) {
            return Err(TableError::InvalidBlockPayload(PayloadError::new(path, "unexpected_keys")));
        }
        let source_str = match o.get("source") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => {
                return Err(TableError::InvalidBlockPayload(PayloadError::new(&format!("{path}/source"), "not_string")))
            }
        };
        let source =
            ColumnSource::parse(source_str).ok_or_else(|| TableError::ColumnUnknown(source_str.to_string()))?;
        let header = match o.get("header") {
            None | Some(CValue::Null) => None,
            Some(hv) => {
                let u = Untrusted::from_value(hv).map_err(|_| {
                    TableError::InvalidBlockPayload(PayloadError::new(&format!("{path}/header"), "bad_header"))
                })?;
                if u.untrusted_value().as_str().len() > MAX_HEADER_BYTES {
                    return Err(TableError::TextInvalid(TextFault::TooLong));
                }
                Some(u)
            }
        };
        Ok(ColumnSpec { source, header })
    }
}

/// Parses an outcome string (accepts both canonical uppercase `NONCONFORMS` and lowercase `nonconforms`).
pub fn parse_outcome(s: &str) -> Option<Outcome> {
    match s {
        "CONFORMS" | "conforms" => Some(Outcome::Conforms),
        "NONCONFORMS" | "nonconforms" => Some(Outcome::Nonconforms),
        "INDETERMINATE" | "indeterminate" => Some(Outcome::Indeterminate),
        "NOT_DECIDABLE" | "not_decidable" => Some(Outcome::NotDecidable),
        _ => None,
    }
}

/// Canonical uppercase wire token for an [`Outcome`].
pub const fn outcome_wire(o: Outcome) -> &'static str {
    match o {
        Outcome::Conforms => "CONFORMS",
        Outcome::Indeterminate => "INDETERMINATE",
        Outcome::Nonconforms => "NONCONFORMS",
        Outcome::NotDecidable => "NOT_DECIDABLE",
    }
}

/// Converts a kernel decision [`DecOutcome`] to the filter [`Outcome`].
pub const fn filter_outcome_of(o: DecOutcome) -> Outcome {
    match o {
        DecOutcome::Conforms => Outcome::Conforms,
        DecOutcome::Nonconforms => Outcome::Nonconforms,
        DecOutcome::Indeterminate => Outcome::Indeterminate,
        DecOutcome::NotDecidable(_) => Outcome::NotDecidable,
    }
}

/// Row filter applied at report generation (REQ-RPT.MODEL.BLOCKS.TABLE-004).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowFilter {
    /// Allowed decision outcomes (empty = do not filter by outcome).
    pub outcomes: BTreeSet<Outcome>,
    /// Allowed characteristic tags (empty = do not filter by tag).
    pub tags: BTreeSet<TagId>,
}

impl RowFilter {
    /// Whether this filter keeps a row with `outcome` and `row_tags` (REQ-004).
    /// When both `outcomes` and `tags` are non-empty, both must match.
    pub fn matches(&self, outcome: Option<Outcome>, row_tags: &[String]) -> bool {
        if !self.outcomes.is_empty() {
            match outcome {
                Some(o) if self.outcomes.contains(&o) => {}
                _ => return false,
            }
        }
        self.tags.is_empty() || row_tags.iter().any(|t| self.tags.iter().any(|want| want.as_str() == t.as_str()))
    }

    /// Canonical JSON representation (deterministic sorted arrays).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert(
            "outcomes".to_string(),
            CValue::Arr(self.outcomes.iter().map(|&x| CValue::Str(outcome_wire(x).to_string())).collect()),
        );
        o.insert(
            "tags".to_string(),
            CValue::Arr(self.tags.iter().map(|t| CValue::Str(t.as_str().to_string())).collect()),
        );
        CValue::Obj(o)
    }

    fn from_value(v: &CValue) -> Result<RowFilter, TableError> {
        let CValue::Obj(o) = v else {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter", "not_object")));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "outcomes" | "tags")) {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter", "unexpected_keys")));
        }
        let mut outcomes = BTreeSet::new();
        if let Some(ov) = o.get("outcomes") {
            let CValue::Arr(arr) = ov else {
                return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/outcomes", "not_array")));
            };
            for (i, item) in arr.iter().enumerate() {
                let CValue::Str(s) = item else {
                    return Err(TableError::InvalidBlockPayload(PayloadError::new(
                        &format!("/filter/outcomes/{i}"),
                        "not_string",
                    )));
                };
                let parsed = parse_outcome(s).ok_or_else(|| {
                    TableError::InvalidBlockPayload(PayloadError::new(
                        &format!("/filter/outcomes/{i}"),
                        "unknown_outcome",
                    ))
                })?;
                outcomes.insert(parsed);
            }
        }
        let mut tags = BTreeSet::new();
        if let Some(tv) = o.get("tags") {
            let CValue::Arr(arr) = tv else {
                return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/tags", "not_array")));
            };
            if arr.len() > MAX_FILTER_TAGS {
                return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/tags", "too_many_tags")));
            }
            for (i, item) in arr.iter().enumerate() {
                let CValue::Str(s) = item else {
                    return Err(TableError::InvalidBlockPayload(PayloadError::new(
                        &format!("/filter/tags/{i}"),
                        "not_string",
                    )));
                };
                tags.insert(TagId::new(s)?);
            }
        }
        Ok(RowFilter { outcomes, tags })
    }
}

/// Optional sort override on a `table` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSort {
    pub source: ColumnSource,
    pub descending: bool,
}

impl TableSort {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("descending".to_string(), CValue::Bool(self.descending));
        o.insert("source".to_string(), CValue::Str(self.source.as_str().to_string()));
        CValue::Obj(o)
    }

    fn from_value(v: &CValue) -> Result<TableSort, TableError> {
        let CValue::Obj(o) = v else {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/sort", "not_object")));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "source" | "descending")) {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/sort", "unexpected_keys")));
        }
        let source_str = match o.get("source") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(TableError::InvalidBlockPayload(PayloadError::new("/sort/source", "not_string"))),
        };
        let source =
            ColumnSource::parse(source_str).ok_or_else(|| TableError::ColumnUnknown(source_str.to_string()))?;
        let descending = match o.get("descending") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(TableError::InvalidBlockPayload(PayloadError::new("/sort/descending", "not_bool"))),
        };
        Ok(TableSort { source, descending })
    }
}

/// Payload schema v1 of a `table` block (REQ-RPT.MODEL.BLOCKS.TABLE-001, -002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePayload {
    /// Ordered column list (1..=64 columns).
    pub columns: Vec<ColumnSpec>,
    /// Row outcome and tag filters applied at generation.
    pub filter: RowFilter,
    /// Optional sort override.
    pub sort: Option<TableSort>,
}

impl TablePayload {
    /// Validates column count bounds and constructs a payload.
    pub fn new(columns: Vec<ColumnSpec>, filter: RowFilter, sort: Option<TableSort>) -> Result<Self, TableError> {
        if columns.len() < MIN_COLUMNS {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "too_few_columns")));
        }
        if columns.len() > MAX_COLUMNS {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "too_many_columns")));
        }
        Ok(TablePayload { columns, filter, sort })
    }

    /// Canonical JSON representation (`schema: "table_block@1"`, no literal values).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("columns".to_string(), CValue::Arr(self.columns.iter().map(ColumnSpec::to_value).collect()));
        o.insert("filter".to_string(), self.filter.to_value());
        o.insert("schema".to_string(), CValue::Str(TABLE_SCHEMA.to_string()));
        if let Some(sort) = &self.sort {
            o.insert("sort".to_string(), sort.to_value());
        }
        CValue::Obj(o)
    }

    /// Strict parser for `table` v1 payloads (ignores optional `binding` and `rows` keys
    /// managed by `RPT.MODEL.BIND.RESOLVE`).
    pub fn from_value(v: &CValue) -> Result<Self, TableError> {
        let CValue::Obj(o) = v else {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("", "not_object")));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "schema" | "columns" | "filter" | "sort" | BINDING_KEY | "rows")) {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("", "unexpected_keys")));
        }
        match o.get("schema") {
            Some(CValue::Str(s)) if s == TABLE_SCHEMA => {}
            _ => return Err(TableError::InvalidBlockPayload(PayloadError::new("/schema", "bad_schema"))),
        }
        let Some(CValue::Arr(cols_raw)) = o.get("columns") else {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "not_array")));
        };
        if cols_raw.len() < MIN_COLUMNS {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "too_few_columns")));
        }
        if cols_raw.len() > MAX_COLUMNS {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "too_many_columns")));
        }
        let mut columns = Vec::with_capacity(cols_raw.len());
        for (i, c) in cols_raw.iter().enumerate() {
            columns.push(ColumnSpec::from_value(c, &format!("/columns/{i}"))?);
        }
        let filter = match o.get("filter") {
            None => RowFilter::default(),
            Some(fv) => RowFilter::from_value(fv)?,
        };
        let sort = match o.get("sort") {
            None | Some(CValue::Null) => None,
            Some(sv) => Some(TableSort::from_value(sv)?),
        };
        if let Some(rows_val) = o.get("rows") {
            let CValue::Arr(rows) = rows_val else {
                return Err(TableError::InvalidBlockPayload(PayloadError::new("/rows", "not_array")));
            };
            for (i, r) in rows.iter().enumerate() {
                match r {
                    CValue::Obj(ro) if ro.len() == 1 && ro.contains_key("value") => {}
                    _ => {
                        return Err(TableError::InvalidBlockPayload(PayloadError::new(
                            &format!("/rows/{i}"),
                            "bad_row",
                        )))
                    }
                }
            }
        }
        TablePayload::new(columns, filter, sort)
    }

    /// Converts this payload into [`CanonicalJson`] for `RPT.MODEL.DOC`.
    pub fn to_canonical(&self) -> Result<CanonicalJson, TableError> {
        CanonicalJson::new(&self.to_value())
            .ok_or_else(|| TableError::InvalidBlockPayload(PayloadError::new("", "payload_bounds")))
    }
}

// ------------------------------------------------------------------ BlockKind & BlockResolver

/// Shipped `table` block kind (schema v1; REQ-RPT.MODEL.BLOCKS.TABLE-001).
#[derive(Debug, Clone, Copy, Default)]
pub struct TableBlockKind;

impl BlockKind for TableBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(TABLE_KIND_ID)
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        TablePayload::from_value(payload.value()).map(|_| ()).map_err(|e| match e {
            TableError::InvalidBlockPayload(p) => p,
            TableError::ColumnUnknown(_) => PayloadError::new("/columns", "column_unknown"),
            TableError::TextInvalid(f) => PayloadError::new("/columns", f.as_str()),
            _ => PayloadError::new("", "invalid_table_payload"),
        })
    }

    fn value_fields(&self, payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        let CValue::Obj(o) = payload.value() else {
            return Err(PayloadError::new("", "not_object"));
        };
        let Some(rows_val) = o.get("rows") else {
            return Ok(Vec::new());
        };
        let CValue::Arr(rows) = rows_val else {
            return Err(PayloadError::new("/rows", "not_array"));
        };
        Ok((0..rows.len()).map(|i| format!("/rows/{i}/value")).collect())
    }
}

/// Shipped [`BlockResolver`] for the `table` block kind (`RPT.MODEL.BLOCKS.TABLE`).
#[derive(Debug, Clone, Copy, Default)]
pub struct TableBlockResolver;

impl BlockResolver for TableBlockResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal(TABLE_KIND_ID)
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        let parsed =
            TablePayload::from_value(payload.value()).map_err(|_| PayloadError::new("", "invalid_table_payload"))?;
        let CValue::Obj(mut out) = parsed.to_value() else {
            return Err(PayloadError::new("", "not_object"));
        };
        let rows = values.iter().map(|v| CValue::Obj(BTreeMap::from([("value".to_string(), v.to_value())]))).collect();
        out.insert("rows".to_string(), CValue::Arr(rows));
        Ok(CValue::Obj(out))
    }

    fn resolve(&self, view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        let rows = view.values().iter().map(|v| CValue::Str(v.pointer().to_string())).collect();
        Ok(CValue::Obj(BTreeMap::from([
            ("kind".to_string(), CValue::Str(TABLE_KIND_ID.to_string())),
            ("rows".to_string(), CValue::Arr(rows)),
        ])))
    }
}

// ------------------------------------------------------------------ table cell resolution

/// One resolved cell in a generated characteristic table (REQ-RPT.MODEL.BLOCKS.TABLE-005).
#[derive(Debug, Clone, PartialEq)]
pub enum CellText {
    /// Formatted numeric cell produced by `RPT.MODEL.FMT`, carrying its [`fid_num::fmt::FormatterId`].
    Value(ValueText),
    /// Formatted decision outcome cell produced by `RPT.MODEL.FMT`.
    Outcome(OutcomeText),
    /// Formatted uncertainty cell produced by `RPT.MODEL.FMT` (UEC-0 is `"not evaluated"`).
    Uncertainty(UncertaintyText),
    /// Plain or untrusted text cell (ID, name, type, rule ID, evaluation basis, source label, stage, run, tags).
    Text(String),
    /// Fixed text `"no result"` when the underlying value or record is absent (REQ-005).
    Missing,
}

impl CellText {
    /// Human-readable display text for this cell.
    pub fn display(&self) -> &str {
        match self {
            CellText::Value(v) => v.display.as_str(),
            CellText::Outcome(o) => o.text.as_str(),
            CellText::Uncertainty(u) => u.display(),
            CellText::Text(s) => s.as_str(),
            CellText::Missing => MISSING_TEXT,
        }
    }

    /// Underlying formatted [`ValueText`], if this is a numeric value cell (or expanded uncertainty).
    pub fn value_text(&self) -> Option<&ValueText> {
        match self {
            CellText::Value(v) => Some(v),
            CellText::Uncertainty(u) => u.value_text(),
            CellText::Outcome(_) | CellText::Text(_) | CellText::Missing => None,
        }
    }
}

/// Resolved table headers and cells produced at report generation.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCells {
    /// Column headers in payload order (custom renames applied).
    pub header: Vec<String>,
    /// Filtered rows, each with one [`CellText`] per column in payload order.
    pub rows: Vec<Vec<CellText>>,
}

/// One candidate row handed to [`resolve_table_cells`] during report generation.
pub struct TableRowInput<'a> {
    /// Characteristic stable ID text.
    pub characteristic_id: &'a str,
    /// Characteristic display name (untrusted text).
    pub characteristic_name: &'a str,
    /// Characteristic type token (e.g. `DiameterCharacteristic`).
    pub characteristic_type: &'a str,
    /// Characteristic formatting specification (unit, decimals, nominal, limits).
    pub characteristic: &'a Characteristic,
    /// Tags attached to this characteristic.
    pub tags: &'a [String],
    /// Stage label or ID, if bound to a stage.
    pub stage: Option<&'a str>,
    /// Run label or ID, if bound to a run.
    pub run: Option<&'a str>,
    /// Resolved `ValueRecord` at the pinned journal head, if a result exists.
    pub result: Option<&'a ValueRecord<'a>>,
}

fn basis_text(basis: EvaluationBasis) -> &'static str {
    match basis {
        EvaluationBasis::Points => "POINTS",
        EvaluationBasis::Features => "FEATURES",
        EvaluationBasis::DirectReading => "DIRECT_READING",
        EvaluationBasis::ResultsOnly => "RESULTS_ONLY",
    }
}

fn format_source_label(record: &ValueRecord<'_>, ctx: &FmtContext<'_>) -> Result<CellText, TableError> {
    let lbls = rpt_fmt::labels(record)?;
    let parts: Vec<String> = lbls
        .into_iter()
        .filter(|l| {
            matches!(
                l,
                Label::ProvisionalInterpretation
                    | Label::Transcribed { .. }
                    | Label::Gauge(_)
                    | Label::Claimed
                    | Label::DeclaredInput
                    | Label::UncharacterizedInput
            )
        })
        .map(|l| l.text(ctx.catalog))
        .collect();
    if parts.is_empty() {
        Ok(CellText::Text(basis_text(record.basis).to_string()))
    } else {
        Ok(CellText::Text(parts.join("; ")))
    }
}

fn resolve_cell(source: ColumnSource, row: &TableRowInput<'_>, ctx: &FmtContext<'_>) -> Result<CellText, TableError> {
    match source {
        ColumnSource::Id => Ok(CellText::Text(row.characteristic_id.to_string())),
        ColumnSource::Name => Ok(CellText::Text(row.characteristic_name.to_string())),
        ColumnSource::CharType => Ok(CellText::Text(row.characteristic_type.to_string())),
        ColumnSource::Nominal => {
            if row.characteristic.nominal.is_none() {
                Ok(CellText::Missing)
            } else {
                Ok(CellText::Value(rpt_fmt::fmt_requirement(row.characteristic, ctx)?))
            }
        }
        ColumnSource::LowerLimit => match row.characteristic.limits.and_then(|ls| ls.lower.map(|l| (l, ls.unit))) {
            Some((lim, unit)) => Ok(CellText::Value(rpt_fmt::fmt_limit(row.characteristic, lim, unit, ctx)?)),
            None => Ok(CellText::Missing),
        },
        ColumnSource::UpperLimit => match row.characteristic.limits.and_then(|ls| ls.upper.map(|u| (u, ls.unit))) {
            Some((lim, unit)) => Ok(CellText::Value(rpt_fmt::fmt_limit(row.characteristic, lim, unit, ctx)?)),
            None => Ok(CellText::Missing),
        },
        ColumnSource::Measured => match row.result {
            Some(rec) => Ok(CellText::Value(rpt_fmt::fmt_value(rec, row.characteristic, ctx)?)),
            None => Ok(CellText::Missing),
        },
        ColumnSource::Deviation => match row.result {
            Some(rec) if row.characteristic.nominal.is_some() => {
                Ok(CellText::Value(rpt_fmt::fmt_deviation(rec, row.characteristic, ctx)?))
            }
            _ => Ok(CellText::Missing),
        },
        ColumnSource::Decision => match row.result {
            Some(rec) => Ok(CellText::Outcome(rpt_fmt::fmt_outcome(rec, ctx.catalog)?)),
            None => Ok(CellText::Missing),
        },
        ColumnSource::DecisionRule => match row.result {
            Some(rec) => {
                let out = rpt_fmt::fmt_outcome(rec, ctx.catalog)?;
                match out.rule_text {
                    Some(rt) => Ok(CellText::Text(rt)),
                    None => Ok(CellText::Missing),
                }
            }
            None => Ok(CellText::Missing),
        },
        ColumnSource::Uncertainty => match row.result {
            Some(rec) => Ok(CellText::Uncertainty(rpt_fmt::fmt_uncertainty(rec, row.characteristic, ctx)?)),
            None => Ok(CellText::Missing),
        },
        ColumnSource::EvaluationBasis => match row.result {
            Some(rec) => Ok(CellText::Text(basis_text(rec.basis).to_string())),
            None => Ok(CellText::Missing),
        },
        ColumnSource::SourceLabel => match row.result {
            Some(rec) => format_source_label(rec, ctx),
            None => Ok(CellText::Missing),
        },
        ColumnSource::Stage => match row.stage {
            Some(s) => Ok(CellText::Text(s.to_string())),
            None => Ok(CellText::Missing),
        },
        ColumnSource::Run => match row.run {
            Some(r) => Ok(CellText::Text(r.to_string())),
            None => Ok(CellText::Missing),
        },
        ColumnSource::Tag => {
            if row.tags.is_empty() {
                Ok(CellText::Missing)
            } else {
                Ok(CellText::Text(row.tags.join(", ")))
            }
        }
    }
}

/// Generates headers and filtered, formatted cells for `payload` over `rows`
/// (REQ-RPT.MODEL.BLOCKS.TABLE-004, -005).
pub fn resolve_table_cells(
    payload: &TablePayload,
    rows: &[TableRowInput<'_>],
    ctx: &FmtContext<'_>,
) -> Result<TableCells, TableError> {
    let header: Vec<String> = payload.columns.iter().map(|c| c.effective_header().to_string()).collect();
    let mut out_rows = Vec::new();
    for row in rows {
        let outcome = row.result.map(|rec| filter_outcome_of(rec.decision.outcome()));
        if !payload.filter.matches(outcome, row.tags) {
            continue;
        }
        let mut cells = Vec::with_capacity(payload.columns.len());
        for col in &payload.columns {
            cells.push(resolve_cell(col.source, row, ctx)?);
        }
        out_rows.push(cells);
    }
    Ok(TableCells { header, rows: out_rows })
}

// ------------------------------------------------------------------ command report.set_table_block@1

/// Execution context for [`set_table_block`].
pub struct TableCmdCtx<'a> {
    pub backend: &'a mut dyn ReportBackend,
    pub kinds: &'a KindRegistry,
}

/// Wire parameter for one column in `report.set_table_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpecParam {
    #[fid(
        description = "Column source from the closed list (id, name, type, nominal, lower_limit, upper_limit, measured, deviation, decision, decision_rule, uncertainty, evaluation_basis, source_label, stage, run, tag)."
    )]
    pub source: String,
    #[fid(description = "Optional renamed column header (at most 128 bytes of untrusted text).", max_length = MAX_HEADER_BYTES)]
    pub header: Option<String>,
}

/// Wire parameter for the row filter in `report.set_table_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq, Default)]
pub struct RowFilterParam {
    #[fid(
        description = "Keep only rows whose decision outcome is in this set (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE)."
    )]
    pub outcomes: Vec<String>,
    #[fid(description = "Keep only rows whose characteristic carries at least one of these tags.", max_items = MAX_FILTER_TAGS)]
    pub tags: Vec<String>,
}

/// Parameters of `report.set_table_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetTableBlockParams {
    #[fid(description = "Report ID (UUIDv7).", pattern = ID_PATTERN)]
    pub report: String,
    #[fid(description = "Table block ID (UUIDv7).", pattern = ID_PATTERN)]
    pub block: String,
    #[fid(description = "Ordered list of 1 to 64 column specifications.", max_items = MAX_COLUMNS)]
    pub columns: Vec<ColumnSpecParam>,
    #[fid(description = "Row outcome and tag filter applied at generation.")]
    pub filter: RowFilterParam,
}

impl SetTableBlockParams {
    /// Strict `--json` / MCP parameter parser.
    pub fn from_value(v: &CValue) -> Result<SetTableBlockParams, TableError> {
        let CValue::Obj(o) = v else {
            return Err(TableError::BadParams("params is not an object"));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "report" | "block" | "columns" | "filter")) {
            return Err(TableError::BadParams("unknown parameter"));
        }
        let get_str = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.clone()),
            _ => Err(TableError::BadParams("report and block must be uuid strings")),
        };
        let report = get_str("report")?;
        let block = get_str("block")?;
        let Some(CValue::Arr(cols_raw)) = o.get("columns") else {
            return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "not_array")));
        };
        let mut columns = Vec::with_capacity(cols_raw.len());
        for (i, item) in cols_raw.iter().enumerate() {
            let CValue::Obj(co) = item else {
                return Err(TableError::InvalidBlockPayload(PayloadError::new(&format!("/columns/{i}"), "not_object")));
            };
            if co.keys().any(|k| !matches!(k.as_str(), "source" | "header")) {
                return Err(TableError::InvalidBlockPayload(PayloadError::new(
                    &format!("/columns/{i}"),
                    "unexpected_keys",
                )));
            }
            let source = match co.get("source") {
                Some(CValue::Str(s)) => s.clone(),
                _ => {
                    return Err(TableError::InvalidBlockPayload(PayloadError::new(
                        &format!("/columns/{i}/source"),
                        "not_string",
                    )))
                }
            };
            let header = match co.get("header") {
                None | Some(CValue::Null) => None,
                Some(CValue::Str(s)) => Some(s.clone()),
                Some(_) => {
                    return Err(TableError::InvalidBlockPayload(PayloadError::new(
                        &format!("/columns/{i}/header"),
                        "not_string",
                    )))
                }
            };
            columns.push(ColumnSpecParam { source, header });
        }
        let mut filter = RowFilterParam::default();
        if let Some(fv) = o.get("filter") {
            let CValue::Obj(fo) = fv else {
                return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter", "not_object")));
            };
            if fo.keys().any(|k| !matches!(k.as_str(), "outcomes" | "tags")) {
                return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter", "unexpected_keys")));
            }
            if let Some(ov) = fo.get("outcomes") {
                let CValue::Arr(arr) = ov else {
                    return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/outcomes", "not_array")));
                };
                for (i, item) in arr.iter().enumerate() {
                    let CValue::Str(s) = item else {
                        return Err(TableError::InvalidBlockPayload(PayloadError::new(
                            &format!("/filter/outcomes/{i}"),
                            "not_string",
                        )));
                    };
                    filter.outcomes.push(s.clone());
                }
            }
            if let Some(tv) = fo.get("tags") {
                let CValue::Arr(arr) = tv else {
                    return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/tags", "not_array")));
                };
                for (i, item) in arr.iter().enumerate() {
                    let CValue::Str(s) = item else {
                        return Err(TableError::InvalidBlockPayload(PayloadError::new(
                            &format!("/filter/tags/{i}"),
                            "not_string",
                        )));
                    };
                    filter.tags.push(s.clone());
                }
            }
        }
        Ok(SetTableBlockParams { report, block, columns, filter })
    }
}

/// Result of `report.set_table_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetTableBlockResult {
    #[fid(description = "Updated table block ID (UUIDv7).", pattern = ID_PATTERN)]
    pub block_id: String,
    #[fid(description = "Number of columns stored.")]
    pub column_count: u32,
    #[fid(description = "SHA-256 digest of the stored report after the update.")]
    pub report_digest: String,
}

impl SetTableBlockResult {
    /// Canonical JSON representation.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("block_id".to_string(), CValue::Str(self.block_id.clone()));
        o.insert("column_count".to_string(), CValue::U64(u64::from(self.column_count)));
        o.insert("report_digest".to_string(), CValue::Str(self.report_digest.clone()));
        CValue::Obj(o)
    }
}

#[fid_command(
    id = "report.set_table_block",
    major = 1,
    title = "Set table block",
    description = "Replaces a characteristic table block's column list, renamed headers and row filters and journals one event.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SET_TABLE_BLOCK],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn set_table_block(
    ctx: &mut TableCmdCtx<'_>,
    params: SetTableBlockParams,
) -> Result<SetTableBlockResult, TableError> {
    let report = parse_canonical_uuid(&params.report).ok_or(TableError::BadParams("report is not a canonical uuid"))?;
    let block = parse_canonical_uuid(&params.block).ok_or(TableError::BadParams("block is not a canonical uuid"))?;

    // Check column count and sources before touching storage (REQ-002, REQ-003).
    if params.columns.len() < MIN_COLUMNS {
        return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "too_few_columns")));
    }
    if params.columns.len() > MAX_COLUMNS {
        return Err(TableError::InvalidBlockPayload(PayloadError::new("/columns", "too_many_columns")));
    }
    let mut columns = Vec::with_capacity(params.columns.len());
    for col in &params.columns {
        let source = ColumnSource::parse(&col.source).ok_or_else(|| TableError::ColumnUnknown(col.source.clone()))?;
        let header = match &col.header {
            Some(h) => Some(parse_header(h)?),
            None => None,
        };
        columns.push(ColumnSpec { source, header });
    }

    let mut outcomes = BTreeSet::new();
    for (i, o_str) in params.filter.outcomes.iter().enumerate() {
        let parsed = parse_outcome(o_str).ok_or_else(|| {
            TableError::InvalidBlockPayload(PayloadError::new(&format!("/filter/outcomes/{i}"), "unknown_outcome"))
        })?;
        outcomes.insert(parsed);
    }
    if params.filter.tags.len() > MAX_FILTER_TAGS {
        return Err(TableError::InvalidBlockPayload(PayloadError::new("/filter/tags", "too_many_tags")));
    }
    let mut tags = BTreeSet::new();
    for t_str in &params.filter.tags {
        tags.insert(TagId::new(t_str)?);
    }

    let new_payload = TablePayload::new(columns, RowFilter { outcomes, tags }, None)?;

    let rid = ReportId::new(report).ok_or(TableError::NotFound(report))?;
    let stored = ctx.backend.load(&rid)?.ok_or(TableError::NotFound(report))?;
    let current = stored.open(&rid, ctx.kinds)?;
    let existing = current.block(&block).ok_or(TableError::NotFound(block))?;
    if existing.kind().as_str() != TABLE_KIND_ID {
        return Err(TableError::InvalidBlockPayload(PayloadError::new("/kind", "not_table_block")));
    }
    let bid = existing.id();

    // Preserve any existing binding / placed value rows while replacing columns and filters.
    let CValue::Obj(mut merged) = new_payload.to_value() else {
        return Err(TableError::InvalidBlockPayload(PayloadError::new("", "not_object")));
    };
    if let CValue::Obj(prev) = existing.payload().value() {
        if let Some(binding) = prev.get(BINDING_KEY) {
            merged.insert(BINDING_KEY.to_string(), binding.clone());
        }
        if let Some(rows) = prev.get("rows") {
            merged.insert("rows".to_string(), rows.clone());
        }
    }
    let payload = CanonicalJson::new(&CValue::Obj(merged))
        .ok_or_else(|| TableError::InvalidBlockPayload(PayloadError::new("", "payload_bounds")))?;

    let op = Op::SetTableBlock { report: rid, block: bid, payload };
    let next = op.apply(Some(current), ctx.kinds)?;
    let stored_next = StoredReport::of(&next)?;
    let digest = stored_next.digest;
    ctx.backend.commit(&rid, stored_next, ReportEvent::new(op, digest))?;

    Ok(SetTableBlockResult {
        block_id: bid.uuid().to_string(),
        column_count: u32::try_from(new_payload.columns.len()).unwrap_or(u32::MAX),
        report_digest: digest.as_str().to_string(),
    })
}

fid_commands![set_table_block];

/// Registry command definitions exported by `RPT.MODEL.BLOCKS.TABLE`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

/// `--json` / MCP entry point for `report.set_table_block@1`.
pub fn execute_set_table_block(ctx: &mut TableCmdCtx<'_>, params: &CValue) -> Result<CValue, TableError> {
    set_table_block(ctx, SetTableBlockParams::from_value(params)?).map(|r| r.to_value())
}

/// CLI entry point (`fid report set-table-block --report <r> --block <b> --columns c1,c2
/// [--rename col=Label ...] [--outcome OUTCOME ...] [--tag TAG ...] [--json]`).
pub fn invoke_cli(ctx: &mut TableCmdCtx<'_>, args: &[&str]) -> Result<CValue, TableError> {
    let Some((&sub, mut rest)) = args.split_first() else {
        return Err(TableError::BadParams("missing subcommand"));
    };
    if sub != CLI_SET_TABLE {
        return Err(TableError::BadParams("unknown subcommand"));
    }
    let mut report: Option<String> = None;
    let mut block: Option<String> = None;
    let mut raw_cols: Vec<String> = Vec::new();
    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    let mut outcomes: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();

    while let Some((&opt, tail)) = rest.split_first() {
        rest = tail;
        if opt == "--json" {
            continue;
        }
        let Some((&val, tail)) = rest.split_first() else {
            return Err(TableError::BadParams("option without value"));
        };
        rest = tail;
        match opt {
            "--report" if report.is_none() => report = Some(val.to_string()),
            "--block" if block.is_none() => block = Some(val.to_string()),
            "--columns" => {
                for c in val.split(',') {
                    if !c.is_empty() {
                        raw_cols.push(c.to_string());
                    }
                }
            }
            "--rename" => {
                let Some((col, label)) = val.split_once('=') else {
                    return Err(TableError::BadParams("--rename expects <column>=<label>"));
                };
                renames.insert(col.to_string(), label.to_string());
            }
            "--outcome" => {
                for o in val.split(',') {
                    if !o.is_empty() {
                        outcomes.push(o.to_string());
                    }
                }
            }
            "--tag" => {
                for t in val.split(',') {
                    if !t.is_empty() {
                        tags.push(t.to_string());
                    }
                }
            }
            _ => return Err(TableError::BadParams("unknown or repeated option")),
        }
    }

    let (Some(report), Some(block)) = (report, block) else {
        return Err(TableError::BadParams("--report and --block are required"));
    };
    let columns = raw_cols
        .into_iter()
        .map(|source| {
            let header = renames.get(&source).cloned();
            ColumnSpecParam { source, header }
        })
        .collect();
    let params = SetTableBlockParams { report, block, columns, filter: RowFilterParam { outcomes, tags } };
    set_table_block(ctx, params).map(|r| r.to_value())
}
