//! Replay Equivalence Policy: table `rep-1@1.0.0` and classifier `rdl.rep.classify@1.0.0`.
//! Spec node `UNC.RDL.REP`.
//!
//! One table and one classifier, used by the journal replayer (`CORE.RPL`) and the
//! standalone verifier (`TRUST.REPLAY`), so the two never disagree on what `EQUIVALENT`
//! means. Pure Rust, no I/O, no `unsafe`, only `fid-num` (KNP-1), usable on wasm32.
//!
//! # Keying
//!
//! The table is keyed by **field kind** ([`FieldKind`]): one bound per kind. The
//! determinism class, kernel-manifest identity and platform class are **context rules**
//! applied by [`classify`] before the table is consulted ([`Ctx`]); they are not table keys.
//!
//! # Classification order (`rdl.rep.classify@1.0.0`)
//!
//! 1. A NaN on either side is the error `nan_input` (NaN is never a valid recorded value).
//! 2. `MARGINAL` is computed (REQ-UNC.RDL.REP-007); it never changes the outcome.
//! 3. Both values absent: `IDENTICAL`; exactly one absent: `DIVERGENT` (REQ-002, UEC-0).
//! 4. Bit-equal values: `IDENTICAL`.
//! 5. Kind `exact`: `DIVERGENT` (REQ-005).
//! 6. Same manifest digest and (D0, or D1/D2 on the same platform class): `DIVERGENT`,
//!    no table (REQ-003).
//! 7. Otherwise the table applies (REQ-004): no bound for the kind gives `DIVERGENT` with
//!    annotation `NO_TOLERANCE`; an infinite value gives `DIVERGENT`; else `EQUIVALENT`
//!    when `|recorded - replayed| <= bound`, decided exactly (REQ-006), and `DIVERGENT`
//!    beyond it. A relative bound is scaled by `max(|recorded|, |replayed|)`.

mod exact;

use core::cmp::Ordering;
use core::fmt;

use fid_num::kdec::Decimal;

use crate::exact::{abs_diff_le, Term};

/// Algorithm ID of the classifier.
pub const ALGORITHM_ID: &str = "rdl.rep.classify@1.0.0";
/// Version of the embedded table, recorded in every replay report (REQ-UNC.RDL.REP-009).
pub const TABLE_VERSION: &str = "rep-1@1.0.0";
/// Schema ID of the table document.
pub const TABLE_SCHEMA: &str = "rep-table@1";
/// Canonical JSON (RFC 8785) of table `rep-1@1.0.0`.
pub const TABLE_JSON: &str = include_str!("../table/rep-1@1.0.0.json");
/// SHA-256 of [`TABLE_JSON`], computed by `fixtures/rdl/rep/make_cases.py`.
pub const TABLE_DIGEST: &str = "sha256:4f79a3e5fb148c4f545d27ea974cc8cf21490bac35d933a7272191a72c00f40c";

/// Errors of this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepError {
    /// A field kind name that the classifier does not know.
    KindUnknown(String),
    /// A NaN recorded or replayed value.
    NanInput,
    /// A table that breaks the `rep-table@1` rules; the detail names the rule.
    TableInvalid(&'static str),
}

impl RepError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            RepError::KindUnknown(_) => "kind_unknown",
            RepError::NanInput => "nan_input",
            RepError::TableInvalid(_) => "table_invalid",
        }
    }
}

impl fmt::Display for RepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepError::KindUnknown(k) => write!(f, "kind_unknown: {k:?}"),
            RepError::NanInput => write!(f, "nan_input"),
            RepError::TableInvalid(d) => write!(f, "table_invalid: {d}"),
        }
    }
}

impl core::error::Error for RepError {}

/// Field kind: the key of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldKind {
    /// Length in metres.
    Length,
    /// Angle in radians.
    Angle,
    /// Dimensionless ratio.
    Dimensionless,
    /// Expanded uncertainty (absent under UEC-0).
    ExpandedUncertainty,
    /// Probability of conformance.
    PConformance,
    /// Counts, enums, outcomes, statuses, evidence class, index-set digests: equal only
    /// when identical.
    Exact,
}

impl FieldKind {
    /// Every kind, in table order.
    pub const ALL: [FieldKind; 6] = [
        FieldKind::Length,
        FieldKind::Angle,
        FieldKind::Dimensionless,
        FieldKind::ExpandedUncertainty,
        FieldKind::PConformance,
        FieldKind::Exact,
    ];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Length => "length",
            FieldKind::Angle => "angle",
            FieldKind::Dimensionless => "dimensionless",
            FieldKind::ExpandedUncertainty => "expanded_uncertainty",
            FieldKind::PConformance => "p_conformance",
            FieldKind::Exact => "exact",
        }
    }

    /// Parses a wire name (exact, case-sensitive); unknown names are `kind_unknown`.
    pub fn parse(s: &str) -> Result<FieldKind, RepError> {
        FieldKind::ALL.into_iter().find(|k| k.as_str() == s).ok_or_else(|| RepError::KindUnknown(s.to_string()))
    }
}

/// Determinism class of the step that produced the field (CONSTITUTION §4). The wire
/// names are those of `fid_types::Determinism`; this crate depends only on `fid-num`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Determinism {
    /// Bit-identical everywhere.
    D0,
    /// Bit-identical on one platform class.
    D1,
    /// Bit-identical on one platform class and configuration.
    D2,
    /// Reproducible within tolerance.
    D3,
    /// Non-deterministic; the replayer substitutes recorded values.
    N,
}

impl Determinism {
    /// Every class.
    pub const ALL: [Determinism; 5] =
        [Determinism::D0, Determinism::D1, Determinism::D2, Determinism::D3, Determinism::N];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Determinism::D0 => "D0",
            Determinism::D1 => "D1",
            Determinism::D2 => "D2",
            Determinism::D3 => "D3",
            Determinism::N => "N",
        }
    }

    /// Parses a wire name; `None` for anything else.
    pub fn parse(s: &str) -> Option<Determinism> {
        Determinism::ALL.into_iter().find(|d| d.as_str() == s)
    }
}

/// Comparison context: the context rules, not table keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ctx {
    /// The recording and replaying kernel-manifest digests are equal.
    pub same_manifest: bool,
    /// Determinism class of the step.
    pub determinism: Determinism,
    /// Recording and replaying hosts are in the same platform class.
    pub same_platform_class: bool,
    /// A decision limit near which the value is annotated `MARGINAL`.
    pub limit: Option<Decimal>,
}

impl Ctx {
    /// True when only bit identity is accepted (REQ-UNC.RDL.REP-003).
    pub fn bit_exact_required(&self) -> bool {
        self.same_manifest
            && match self.determinism {
                Determinism::D0 => true,
                Determinism::D1 | Determinism::D2 => self.same_platform_class,
                Determinism::D3 | Determinism::N => false,
            }
    }
}

/// Why a field or run could not be replayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotReplayableCause {
    /// A resource trap (memory, time or fuel budget) stopped the replay.
    Resource,
    /// Any other cause (missing input, nothing to replay).
    Other,
}

/// Replay outcome of a field, a step or a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Bit-equal.
    Identical,
    /// Within the table bound.
    Equivalent,
    /// Could not be replayed.
    NotReplayable(NotReplayableCause),
    /// Beyond the bound, or any `exact` difference.
    Divergent,
}

impl Outcome {
    /// Rank in the order `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` > `IDENTICAL`.
    pub fn rank(self) -> u8 {
        match self {
            Outcome::Identical => 0,
            Outcome::Equivalent => 1,
            Outcome::NotReplayable(_) => 2,
            Outcome::Divergent => 3,
        }
    }

    /// Wire name; a resource trap is `NOT_REPLAYABLE(resource)`.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Identical => "IDENTICAL",
            Outcome::Equivalent => "EQUIVALENT",
            Outcome::NotReplayable(NotReplayableCause::Resource) => "NOT_REPLAYABLE(resource)",
            Outcome::NotReplayable(NotReplayableCause::Other) => "NOT_REPLAYABLE",
            Outcome::Divergent => "DIVERGENT",
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Informative annotation of a field outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Annotation {
    /// The table has no bound for the field's kind; the outcome is `DIVERGENT`.
    NoTolerance,
    /// A value lies within its bound of the caller's decision limit.
    Marginal,
}

impl Annotation {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Annotation::NoTolerance => "NO_TOLERANCE",
            Annotation::Marginal => "MARGINAL",
        }
    }
}

/// Result of classifying one field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldOutcome {
    /// The outcome.
    pub outcome: Outcome,
    /// Annotations, sorted and without duplicates.
    pub annotations: Vec<Annotation>,
}

/// How a bound is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundMode {
    /// `|Δ| <= value`.
    Abs,
    /// `|Δ| <= value * max(|recorded|, |replayed|)`.
    Rel,
}

impl BoundMode {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            BoundMode::Abs => "abs",
            BoundMode::Rel => "rel",
        }
    }
}

/// One table row for a numeric field kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// The field kind (never [`FieldKind::Exact`]).
    pub kind: FieldKind,
    /// Absolute or relative.
    pub mode: BoundMode,
    /// The bound, an exact non-negative decimal.
    pub value: Decimal,
    /// Unit of the bound: an SI unit for an absolute bound, `1` for a relative one.
    pub unit: String,
}

/// A replay equivalence-tolerance table, keyed by field kind (schema `rep-table@1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    version: String,
    bounds: Vec<Bound>,
}

impl Table {
    /// Builds a table, checking the `rep-table@1` rules: a non-empty version; bounds for
    /// numeric kinds only, at most one per kind, non-negative, with a non-empty unit, `1`
    /// for a relative bound. Rows are kept in [`FieldKind`] order.
    pub fn new(version: &str, mut bounds: Vec<Bound>) -> Result<Table, RepError> {
        if version.is_empty() {
            return Err(RepError::TableInvalid("empty_version"));
        }
        bounds.sort_by_key(|b| b.kind);
        for (i, b) in bounds.iter().enumerate() {
            if b.kind == FieldKind::Exact {
                return Err(RepError::TableInvalid("bound_for_exact"));
            }
            if i > 0 && bounds.get(i - 1).map(|p| p.kind) == Some(b.kind) {
                return Err(RepError::TableInvalid("duplicate_kind"));
            }
            if b.value.is_negative() {
                return Err(RepError::TableInvalid("negative_bound"));
            }
            if b.unit.is_empty() || (b.mode == BoundMode::Rel && b.unit != "1") {
                return Err(RepError::TableInvalid("bad_unit"));
            }
        }
        Ok(Table { version: version.to_string(), bounds })
    }

    /// The embedded table `rep-1@1.0.0` (REQ-UNC.RDL.REP-001).
    pub fn rep1() -> Table {
        let row = |kind, mode, exp10: i32, unit: &str| Bound {
            kind,
            mode,
            // 1 * 10^exp10 with exp10 in -9..=-6 is always inside the Decimal range.
            value: Decimal::new(false, 1, exp10).unwrap_or(Decimal::ZERO),
            unit: unit.to_string(),
        };
        Table {
            version: TABLE_VERSION.to_string(),
            bounds: vec![
                row(FieldKind::Length, BoundMode::Abs, -9, "m"),
                row(FieldKind::Angle, BoundMode::Abs, -9, "rad"),
                row(FieldKind::Dimensionless, BoundMode::Rel, -9, "1"),
                row(FieldKind::ExpandedUncertainty, BoundMode::Rel, -6, "1"),
                row(FieldKind::PConformance, BoundMode::Abs, -6, "1"),
            ],
        }
    }

    /// The table version, for replay reports.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The rows, in [`FieldKind`] order.
    pub fn bounds(&self) -> &[Bound] {
        &self.bounds
    }

    /// The row for `kind`, if any (never for [`FieldKind::Exact`]).
    pub fn bound(&self, kind: FieldKind) -> Option<&Bound> {
        self.bounds.iter().find(|b| b.kind == kind)
    }

    /// Canonical JSON (RFC 8785) of the table; the caller hashes it for the report.
    /// Strings are written as-is: versions and units are expected to be plain ASCII
    /// without `"` or `\`; any other character is escaped as `\uXXXX`.
    pub fn canonical_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\"algorithm\":");
        push_json_str(&mut s, ALGORITHM_ID);
        s.push_str(",\"bounds\":[");
        for b in &self.bounds {
            s.push_str("{\"bound\":");
            push_json_str(&mut s, &b.value.to_string());
            s.push_str(",\"kind\":");
            push_json_str(&mut s, b.kind.as_str());
            s.push_str(",\"mode\":");
            push_json_str(&mut s, b.mode.as_str());
            s.push_str(",\"unit\":");
            push_json_str(&mut s, &b.unit);
            s.push_str("},");
        }
        s.push_str("{\"kind\":\"exact\",\"mode\":\"exact\"}],\"schema\":");
        push_json_str(&mut s, TABLE_SCHEMA);
        s.push_str(",\"version\":");
        push_json_str(&mut s, &self.version);
        s.push('}');
        s
    }
}

fn push_json_str(s: &mut String, v: &str) {
    s.push('"');
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            ' '..='~' => s.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    s.push_str(&format!("\\u{:04x}", u));
                }
            }
        }
    }
    s.push('"');
}

/// `|x|` larger of two finite values, compared by bit pattern (no `f64::max`, KNP-1).
fn larger_abs(a: f64, b: f64) -> f64 {
    let (ma, mb) = (a.to_bits() & !(1u64 << 63), b.to_bits() & !(1u64 << 63));
    f64::from_bits(if ma >= mb { ma } else { mb })
}

/// The bound as an exact term, scaled for a relative bound.
fn bound_term(b: &Bound, scale: f64) -> Option<Term> {
    let d = Term::from_decimal(&b.value);
    match b.mode {
        BoundMode::Abs => Some(d),
        BoundMode::Rel => Term::from_f64(scale).map(|s| d.mul(&s.abs())),
    }
}

fn is_marginal(table: &Table, kind: FieldKind, recorded: Option<f64>, replayed: Option<f64>, ctx: &Ctx) -> bool {
    let (Some(limit), Some(b)) = (ctx.limit, table.bound(kind)) else { return false };
    let vals: Vec<f64> = [recorded, replayed].into_iter().flatten().filter(|v| v.is_finite()).collect();
    let scale = vals.iter().fold(0.0, |acc, &v| larger_abs(acc, v));
    let Some(bound) = bound_term(b, scale) else { return false };
    let lim = Term::from_decimal(&limit);
    vals.iter().any(|&v| Term::from_f64(v).is_some_and(|t| abs_diff_le(&t, &lim, &bound)))
}

/// Classifies one field against the embedded table `rep-1@1.0.0`. `None` is an absent
/// value (for example an expanded uncertainty under UEC-0).
pub fn classify(
    kind: FieldKind,
    recorded: Option<f64>,
    replayed: Option<f64>,
    ctx: &Ctx,
) -> Result<FieldOutcome, RepError> {
    classify_with(&Table::rep1(), kind, recorded, replayed, ctx)
}

/// Classifies one field against `table` (see the crate documentation for the order).
pub fn classify_with(
    table: &Table,
    kind: FieldKind,
    recorded: Option<f64>,
    replayed: Option<f64>,
    ctx: &Ctx,
) -> Result<FieldOutcome, RepError> {
    if recorded.is_some_and(f64::is_nan) || replayed.is_some_and(f64::is_nan) {
        return Err(RepError::NanInput);
    }
    let mut annotations = Vec::new();
    if kind != FieldKind::Exact && is_marginal(table, kind, recorded, replayed, ctx) {
        annotations.push(Annotation::Marginal);
    }
    let outcome = match (recorded, replayed) {
        (None, None) => Outcome::Identical,
        (Some(a), Some(b)) => {
            if a.to_bits() == b.to_bits() {
                Outcome::Identical
            } else if kind == FieldKind::Exact || ctx.bit_exact_required() {
                Outcome::Divergent
            } else {
                match table.bound(kind) {
                    None => {
                        annotations.insert(0, Annotation::NoTolerance);
                        Outcome::Divergent
                    }
                    Some(bound) => {
                        let terms = (Term::from_f64(a), Term::from_f64(b), bound_term(bound, larger_abs(a, b)));
                        match terms {
                            (Some(x), Some(y), Some(t)) if abs_diff_le(&x, &y, &t) => Outcome::Equivalent,
                            _ => Outcome::Divergent,
                        }
                    }
                }
            }
        }
        _ => Outcome::Divergent,
    };
    Ok(FieldOutcome { outcome, annotations })
}

/// Classifies a field of kind `exact` given as bytes (an enum name, a status, a digest):
/// `IDENTICAL` when equal, else `DIVERGENT` (REQ-UNC.RDL.REP-005).
pub fn classify_exact_bytes(recorded: &[u8], replayed: &[u8]) -> FieldOutcome {
    let outcome = if recorded == replayed { Outcome::Identical } else { Outcome::Divergent };
    FieldOutcome { outcome, annotations: Vec::new() }
}

/// Worst outcome, in the order `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` >
/// `IDENTICAL` (REQ-UNC.RDL.REP-008). Among several `NOT_REPLAYABLE`, the first one wins.
/// An empty list is `NOT_REPLAYABLE` (nothing was replayed).
pub fn combine(outcomes: &[Outcome]) -> Outcome {
    let mut worst: Option<Outcome> = None;
    for &o in outcomes {
        worst = match worst {
            Some(w) if w.rank().cmp(&o.rank()) != Ordering::Less => Some(w),
            _ => Some(o),
        };
    }
    worst.unwrap_or(Outcome::NotReplayable(NotReplayableCause::Other))
}
