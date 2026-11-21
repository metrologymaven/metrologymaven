//! Replay outcome classification and aggregation. Spec node `TRUST.REPLAY.COMPARE`.
//!
//! Each recomputed result is compared field by field with its packaged ResultRecord and
//! classified `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE`; the result outcomes
//! are then folded into one package outcome, worst wins.
//!
//! There is no classifier here. Every field goes through `fid-rep`, algorithm
//! `rdl.rep.classify@1.0.0` (UNC.RDL.REP), with the REP-1 table loaded by [`Rep1`]:
//! numeric fields through [`fid_rep::classify_with`], enum and text fields through
//! [`fid_rep::classify_exact_bytes`], and the fold through [`fid_rep::combine`]. This module
//! only chooses the field kinds, keeps the field order and names the reason.
//!
//! # Fields, in order
//!
//! | Field path | Kind |
//! |---|---|
//! | `value` | the result's quantity kind (`length`, `angle`, ...) |
//! | `uncertainty.status` | `exact` (the UEC level) |
//! | `uncertainty.value` | `expanded_uncertainty` (absent under UEC-0) |
//! | `decision.outcome` | `exact` |
//! | `decision.rule_id` | `exact` |
//! | `status` | `exact` (characteristic status) |
//!
//! The result outcome is the worst field outcome. For a result that is not `IDENTICAL`,
//! `first_diff` is the first field, in this order, whose outcome equals the result outcome,
//! and `reason` is that field's reason. A decision, rule or status change is therefore always
//! `DIVERGENT` (REQ-003), and a D0 value that is not bit-equal is `DIVERGENT` with
//! `BIT_EXACT`, whatever the manifests (REQ-004; settled in UNC.RDL.REP).
//!
//! A quantity kind with no REP-1 row (a row missing from the table, or a kind `fid-rep` does
//! not know) gives `DIVERGENT` with `NO_TOLERANCE` for a value that is not bit-equal
//! (REQ-005); D0/D1/D2 bit-exact rules still come first. A NaN on either side is `DIVERGENT`
//! with `NAN_INPUT`: a package never makes the comparison fail open.

use std::collections::BTreeMap;

use fid_canon::CValue;
use fid_num::kdec::Decimal;
use fid_rep::{
    classify_exact_bytes, classify_with, combine, Annotation, Bound, BoundMode, Ctx, FieldKind, FieldOutcome, Outcome,
    RepError, Table,
};
pub use fid_types::Determinism;
use fid_types::Digest;

/// Largest REP-1 table file accepted, in bytes.
pub const MAX_TABLE_BYTES: usize = 64 * 1024;

/// Errors of this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareError {
    /// The REP-1 table file fails the `rep-table@1` schema; the detail names the rule.
    Rep1Malformed(&'static str),
}

impl CompareError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CompareError::Rep1Malformed(_) => "rep1.malformed",
        }
    }
}

impl std::fmt::Display for CompareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompareError::Rep1Malformed(d) => write!(f, "rep1.malformed: {d}"),
        }
    }
}

impl std::error::Error for CompareError {}

/// A loaded REP-1 table and the SHA-256 of the exact bytes it was loaded from (REQ-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rep1 {
    table: Table,
    digest: Digest,
}

fn malformed(detail: &'static str) -> CompareError {
    CompareError::Rep1Malformed(detail)
}

fn obj<'a>(v: &'a CValue, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, CompareError> {
    let CValue::Obj(m) = v else { return Err(malformed("not_an_object")) };
    if m.len() != keys.len() || !keys.iter().all(|k| m.contains_key(*k)) {
        return Err(malformed("bad_keys"));
    }
    Ok(m)
}

fn text<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, CompareError> {
    match m.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(malformed("not_a_string")),
    }
}

impl Rep1 {
    /// Loads and validates a REP-1 table file (schema `rep-table@1`, published by UNC.RDL.REP).
    ///
    /// The file must be valid JSON with no duplicate keys and exactly the keys `algorithm`
    /// (`rdl.rep.classify@1.0.0`), `bounds`, `schema` (`rep-table@1`) and `version`; `bounds`
    /// holds one `{bound, kind, mode, unit}` row per numeric kind and exactly one
    /// `{"kind": "exact", "mode": "exact"}` row; the rows must pass [`Table::new`]; and the bytes
    /// must be the table's canonical JSON (RFC 8785), so that the recorded digest names the
    /// table and nothing else. Any failure is `rep1.malformed`.
    pub fn load(bytes: &[u8]) -> Result<Rep1, CompareError> {
        if bytes.len() > MAX_TABLE_BYTES {
            return Err(malformed("too_large"));
        }
        let doc = fid_canon::parse_with_limits(bytes, MAX_TABLE_BYTES, 8).map_err(|e| match e {
            fid_canon::CanonError::DuplicateKey(_) => malformed("duplicate_key"),
            _ => malformed("not_json"),
        })?;
        let top = obj(&doc, &["algorithm", "bounds", "schema", "version"])?;
        if text(top, "schema")? != fid_rep::TABLE_SCHEMA {
            return Err(malformed("schema"));
        }
        if text(top, "algorithm")? != fid_rep::ALGORITHM_ID {
            return Err(malformed("algorithm"));
        }
        let version = text(top, "version")?;
        let Some(CValue::Arr(rows)) = top.get("bounds") else { return Err(malformed("bounds_not_array")) };
        let mut bounds = Vec::new();
        let mut exact_rows = 0usize;
        for row in rows {
            let CValue::Obj(m) = row else { return Err(malformed("not_an_object")) };
            if text(m, "kind")? == "exact" {
                obj(row, &["kind", "mode"])?;
                if text(m, "mode")? != "exact" {
                    return Err(malformed("exact_mode"));
                }
                exact_rows += 1;
                continue;
            }
            let m = obj(row, &["bound", "kind", "mode", "unit"])?;
            let kind = FieldKind::parse(text(m, "kind")?).map_err(|_| malformed("kind_unknown"))?;
            let mode = match text(m, "mode")? {
                "abs" => BoundMode::Abs,
                "rel" => BoundMode::Rel,
                _ => return Err(malformed("mode")),
            };
            let value = Decimal::parse(text(m, "bound")?).map_err(|_| malformed("bound"))?;
            bounds.push(Bound { kind, mode, value, unit: text(m, "unit")?.to_string() });
        }
        if exact_rows != 1 {
            return Err(malformed("exact_row"));
        }
        let table = Table::new(version, bounds).map_err(|e| match e {
            RepError::TableInvalid(d) => malformed(d),
            _ => malformed("table"),
        })?;
        if table.canonical_json().as_bytes() != bytes {
            return Err(malformed("not_canonical"));
        }
        Ok(Rep1 { table, digest: Digest::from_bytes(fid_canon::digest_bytes(bytes).0) })
    }

    /// The production table `rep-1@1.0.0` embedded in `fid-rep`.
    pub fn embedded() -> Result<Rep1, CompareError> {
        Rep1::load(fid_rep::TABLE_JSON.as_bytes())
    }

    /// The table.
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// The table version, for the report.
    pub fn version(&self) -> &str {
        self.table.version()
    }

    /// SHA-256 of the table file, for the report (REQ-007).
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

/// Decision outcome of a ResultRecord (CONSTITUTION §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecisionOutcome {
    /// `CONFORMS`.
    Conforms,
    /// `NONCONFORMS`.
    Nonconforms,
    /// `INDETERMINATE`.
    Indeterminate,
    /// `NOT_DECIDABLE`.
    NotDecidable,
}

impl DecisionOutcome {
    /// Every outcome.
    pub const ALL: [DecisionOutcome; 4] = [
        DecisionOutcome::Conforms,
        DecisionOutcome::Nonconforms,
        DecisionOutcome::Indeterminate,
        DecisionOutcome::NotDecidable,
    ];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionOutcome::Conforms => "CONFORMS",
            DecisionOutcome::Nonconforms => "NONCONFORMS",
            DecisionOutcome::Indeterminate => "INDETERMINATE",
            DecisionOutcome::NotDecidable => "NOT_DECIDABLE",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<DecisionOutcome> {
        DecisionOutcome::ALL.into_iter().find(|d| d.as_str() == s)
    }
}

/// A conformance decision: rule ID and outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Decision rule ID.
    pub rule_id: String,
    /// Outcome.
    pub outcome: DecisionOutcome,
}

/// The compared fields of one side (packaged or recomputed) of a ResultRecord.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultFields {
    /// Value in SI units; `None` when the record has no value.
    pub value: Option<f64>,
    /// Uncertainty evaluation class, UEC-0..3.
    pub uec: u8,
    /// Expanded uncertainty in SI units; `None` when not evaluated (UEC-0).
    pub expanded_uncertainty: Option<f64>,
    /// Conformance decision, if any.
    pub decision: Option<Decision>,
    /// Characteristic status.
    pub status: String,
}

/// One replayed result: the packaged record and the recomputed one.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultPair {
    /// ResultRecord ID.
    pub record_id: String,
    /// Determinism class of the algorithm.
    pub determinism: Determinism,
    /// Quantity kind of the value, a REP-1 field kind name (`length`, `angle`, ...).
    pub quantity_kind: String,
    /// Optional decision limit, for the informative `MARGINAL` annotation.
    pub limit: Option<Decimal>,
    /// As packaged.
    pub packaged: ResultFields,
    /// As recomputed.
    pub recomputed: ResultFields,
}

/// Context of the replay as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayContext {
    /// The recomputing kernel's manifest digest equals the package's.
    pub same_manifest: bool,
    /// The recomputing host is in the packaging host's platform class.
    pub same_platform_class: bool,
}

/// One result of a package as SELECT hands it over.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultInput {
    /// Recomputed; compare it (boxed: a pair is far larger than the other variant).
    Replayed(Box<ResultPair>),
    /// Not replayable (TRUST.REPLAY.SELECT), with SELECT's reason code.
    NotReplayable {
        /// ResultRecord ID.
        record_id: String,
        /// Reason code from SELECT (for example `RESULTS_ONLY`).
        reason: String,
    },
}

/// A compared field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldPath {
    /// `value`.
    Value,
    /// `uncertainty.status`.
    UncertaintyStatus,
    /// `uncertainty.value`.
    UncertaintyValue,
    /// `decision.outcome`.
    DecisionOutcome,
    /// `decision.rule_id`.
    DecisionRuleId,
    /// `status`.
    Status,
}

impl FieldPath {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldPath::Value => "value",
            FieldPath::UncertaintyStatus => "uncertainty.status",
            FieldPath::UncertaintyValue => "uncertainty.value",
            FieldPath::DecisionOutcome => "decision.outcome",
            FieldPath::DecisionRuleId => "decision.rule_id",
            FieldPath::Status => "status",
        }
    }
}

/// Why a result is not `IDENTICAL` or `EQUIVALENT`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Reason {
    /// No REP-1 row for the field's kind (REQ-005).
    NoTolerance,
    /// Bit identity required (D0 always; D1/D2 on the same manifest and platform class).
    BitExact,
    /// Beyond the REP-1 bound.
    OutsideTolerance,
    /// An exact field (UEC level, decision, rule ID, status, `exact` kind) or a field's
    /// presence changed.
    FieldChanged,
    /// A NaN value.
    NanInput,
    /// Class N is not classified.
    ClassN,
    /// Not replayable, with SELECT's reason code.
    NotReplayable(String),
}

impl Reason {
    /// Wire form: `NO_TOLERANCE`, ..., or `NOT_REPLAYABLE:<code>`.
    pub fn to_wire(&self) -> String {
        match self {
            Reason::NoTolerance => "NO_TOLERANCE".to_string(),
            Reason::BitExact => "BIT_EXACT".to_string(),
            Reason::OutsideTolerance => "OUTSIDE_TOLERANCE".to_string(),
            Reason::FieldChanged => "FIELD_CHANGED".to_string(),
            Reason::NanInput => "NAN_INPUT".to_string(),
            Reason::ClassN => "CLASS_N".to_string(),
            Reason::NotReplayable(code) => format!("NOT_REPLAYABLE:{code}"),
        }
    }
}

/// Outcome of one result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultOutcome {
    /// ResultRecord ID.
    pub record_id: String,
    /// The outcome.
    pub outcome: Outcome,
    /// First differing field (see the module documentation); `None` when `IDENTICAL` or not
    /// replayed.
    pub first_diff: Option<FieldPath>,
    /// Reason, for an outcome other than `IDENTICAL` and `EQUIVALENT`.
    pub reason: Option<Reason>,
    /// The value is within its bound of the decision limit (informative).
    pub marginal: bool,
}

/// Outcome of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOutcome {
    /// Worst result outcome; `NOT_REPLAYABLE` for a package with no results (REQ-006).
    pub outcome: Outcome,
    /// Per result, in input order.
    pub results: Vec<ResultOutcome>,
    /// REP-1 table version used.
    pub rep1_version: String,
    /// SHA-256 of the REP-1 table file used (REQ-007).
    pub rep1_digest: Digest,
}

struct Field {
    outcome: Outcome,
    reason: Option<Reason>,
    marginal: bool,
}

fn numeric(
    table: &Table,
    kind_name: &str,
    a: Option<f64>,
    b: Option<f64>,
    det: Determinism,
    rc: &ReplayContext,
    limit: Option<Decimal>,
) -> Field {
    let known = FieldKind::parse(kind_name).ok();
    // A kind fid-rep does not know is classified as `exact` (bit identity only) and reported
    // as NO_TOLERANCE when it differs outside the bit-exact rules.
    let kind = known.unwrap_or(FieldKind::Exact);
    let ctx = Ctx {
        same_manifest: rc.same_manifest,
        determinism: det,
        same_platform_class: rc.same_platform_class,
        limit: known.and(limit),
    };
    let fo = match classify_with(table, kind, a, b, &ctx) {
        Ok(fo) => fo,
        Err(_) => return Field { outcome: Outcome::Divergent, reason: Some(Reason::NanInput), marginal: false },
    };
    let marginal = fo.annotations.contains(&Annotation::Marginal);
    let reason = match fo.outcome {
        Outcome::Identical | Outcome::Equivalent => None,
        Outcome::NotReplayable(_) => Some(Reason::ClassN),
        Outcome::Divergent => Some(if fo.annotations.contains(&Annotation::NoTolerance) {
            Reason::NoTolerance
        } else if a.is_none() || b.is_none() || known == Some(FieldKind::Exact) {
            Reason::FieldChanged
        } else if ctx.bit_exact_required() {
            Reason::BitExact
        } else if known.is_none() {
            Reason::NoTolerance
        } else {
            Reason::OutsideTolerance
        }),
    };
    Field { outcome: fo.outcome, reason, marginal }
}

fn tagged(v: Option<&[u8]>) -> Vec<u8> {
    match v {
        None => Vec::new(),
        Some(s) => {
            let mut out = Vec::with_capacity(s.len() + 1);
            out.push(b'+');
            out.extend_from_slice(s);
            out
        }
    }
}

fn exact(a: Option<&[u8]>, b: Option<&[u8]>) -> Field {
    let FieldOutcome { outcome, .. } = classify_exact_bytes(&tagged(a), &tagged(b));
    let reason = (outcome != Outcome::Identical).then_some(Reason::FieldChanged);
    Field { outcome, reason, marginal: false }
}

fn outcome_of(d: &Option<Decision>) -> Option<&[u8]> {
    d.as_ref().map(|d| d.outcome.as_str().as_bytes())
}

fn rule_of(d: &Option<Decision>) -> Option<&[u8]> {
    d.as_ref().map(|d| d.rule_id.as_bytes())
}

/// Compares one recomputed result with its packaged record.
pub fn compare_result(rep1: &Rep1, rc: &ReplayContext, pair: &ResultPair) -> ResultOutcome {
    let (p, r) = (&pair.packaged, &pair.recomputed);
    let table = rep1.table();
    let det = pair.determinism;
    let fields = [
        (FieldPath::Value, numeric(table, &pair.quantity_kind, p.value, r.value, det, rc, pair.limit)),
        (FieldPath::UncertaintyStatus, exact(Some(std::slice::from_ref(&p.uec)), Some(std::slice::from_ref(&r.uec)))),
        (
            FieldPath::UncertaintyValue,
            numeric(table, "expanded_uncertainty", p.expanded_uncertainty, r.expanded_uncertainty, det, rc, None),
        ),
        (FieldPath::DecisionOutcome, exact(outcome_of(&p.decision), outcome_of(&r.decision))),
        (FieldPath::DecisionRuleId, exact(rule_of(&p.decision), rule_of(&r.decision))),
        (FieldPath::Status, exact(Some(p.status.as_bytes()), Some(r.status.as_bytes()))),
    ];
    let outcomes: Vec<Outcome> = fields.iter().map(|(_, f)| f.outcome).collect();
    let outcome = combine(&outcomes);
    let first = if outcome == Outcome::Identical {
        None
    } else {
        fields.iter().find(|(_, f)| f.outcome.rank() == outcome.rank())
    };
    ResultOutcome {
        record_id: pair.record_id.clone(),
        outcome,
        first_diff: first.map(|(path, _)| *path),
        reason: first.and_then(|(_, f)| f.reason.clone()),
        marginal: fields.first().is_some_and(|(_, f)| f.marginal),
    }
}

/// Worst outcome, `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` > `IDENTICAL`; an empty list
/// is `NOT_REPLAYABLE` (REQ-006). This is [`fid_rep::combine`].
pub fn aggregate(outcomes: &[Outcome]) -> Outcome {
    combine(outcomes)
}

/// Compares every result of a package and aggregates them (REQ-006, REQ-007).
pub fn compare_package(rep1: &Rep1, rc: &ReplayContext, inputs: &[ResultInput]) -> PackageOutcome {
    let results: Vec<ResultOutcome> = inputs
        .iter()
        .map(|i| match i {
            ResultInput::Replayed(pair) => compare_result(rep1, rc, pair),
            ResultInput::NotReplayable { record_id, reason } => ResultOutcome {
                record_id: record_id.clone(),
                outcome: Outcome::NotReplayable(fid_rep::NotReplayableCause::Other),
                first_diff: None,
                reason: Some(Reason::NotReplayable(reason.clone())),
                marginal: false,
            },
        })
        .collect();
    let outcomes: Vec<Outcome> = results.iter().map(|r| r.outcome).collect();
    PackageOutcome {
        outcome: aggregate(&outcomes),
        results,
        rep1_version: rep1.version().to_string(),
        rep1_digest: rep1.digest(),
    }
}

impl PackageOutcome {
    /// Canonical JSON (RFC 8785, through `fid-canon`) of the outcome: the bytes whose digest
    /// AC-H3 compares across targets.
    pub fn canonical_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        let s = |v: &str| CValue::Str(v.to_string());
        let results = self
            .results
            .iter()
            .map(|r| {
                let mut m = BTreeMap::new();
                m.insert("record_id".to_string(), s(&r.record_id));
                m.insert("outcome".to_string(), s(r.outcome.as_str()));
                m.insert("first_diff".to_string(), r.first_diff.map_or(CValue::Null, |f| s(f.as_str())));
                m.insert("reason".to_string(), r.reason.as_ref().map_or(CValue::Null, |x| CValue::Str(x.to_wire())));
                m.insert("marginal".to_string(), CValue::Bool(r.marginal));
                CValue::Obj(m)
            })
            .collect();
        let mut top = BTreeMap::new();
        top.insert("algorithm".to_string(), s(fid_rep::ALGORITHM_ID));
        top.insert("outcome".to_string(), s(self.outcome.as_str()));
        top.insert("rep1_digest".to_string(), s(self.rep1_digest.as_str()));
        top.insert("rep1_version".to_string(), s(&self.rep1_version));
        top.insert("results".to_string(), CValue::Arr(results));
        fid_canon::to_canonical_bytes(&CValue::Obj(top))
    }
}
