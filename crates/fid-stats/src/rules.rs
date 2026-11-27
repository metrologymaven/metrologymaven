//! Run-rule sets and violation detection (spec node `RPT.STATS.SPC.RULES`,
//! REQ-RPT.STATS.SPC.RULES-001 to -007).
//!
//! A rule set is named, versioned data: an ID, a version, a review status and an ordered list
//! of rules, each a rule ID with one parameterised pattern primitive (REQ-003). Rule sets are
//! loaded from canonical JSON with [`RuleSet::parse`] (see [`crate::rules_load`] for the file
//! format and the order of its checks) or built in code with [`RuleSet::new`]; either way an
//! invalid set is refused with `stats.ruleset_invalid` and no rule is evaluated (REQ-007).
//! [`evaluate_rules`] runs a set over a sequence of plotted points against a centre line and
//! control limits.
//!
//! # Shipped sets (REQ-004)
//!
//! | Set | Rules |
//! |---|---|
//! | `beyond_limits@1` (the default) | `BEYOND{3}`, the plain control-limit test |
//! | `western_electric@1` | `BEYOND{3}`, `K_OF_M_BEYOND{2,3,2}`, `K_OF_M_BEYOND{4,5,1}`, `RUN_SAME_SIDE{8}` |
//! | `nelson@1` | `BEYOND{3}`, `RUN_SAME_SIDE{9}`, `TREND{6}`, `ALTERNATING{14}`, `K_OF_M_BEYOND{2,3,2}`, `K_OF_M_BEYOND{4,5,1}`, `RUN_WITHIN{15,1}`, `RUN_OUTSIDE{8,1}` |
//!
//! The published sets are parameters only, written by the project after Western Electric,
//! *Statistical Quality Control Handbook*, 1956, and L. S. Nelson, *Journal of Quality
//! Technology*, 1984 [unverified citation details and parameters]; no text is copied. **Every
//! shipped set is `UNVERIFIED`**, `beyond_limits@1` included: its single rule is definitional,
//! but the boundary conventions of REQ-002 that it applies have not been reviewed by an expert
//! either. The review status travels with every result and every violation. A review claimed
//! by a loaded file is [`ReviewStatus::ClaimedByFile`], never [`ReviewStatus::ExpertReviewed`]
//! (D-045); only shipped data can carry the latter, and no shipped set does today.
//!
//! # Primitives (REQ-001, REQ-002)
//!
//! Sigma above the centre is `(UCL − centre) / 3` and below it `(centre − LCL) / 3`, so
//! asymmetric limits (an R chart with LCL clamped to 0) get their own sigma on each side. For a
//! zone parameter `z` the thresholds are `upper = centre + z · sigma_above` and
//! `lower = centre − z · sigma_below`, computed once per rule with plain IEEE 754 operations (two
//! roundings each, no FMA), except that `z = 3` uses `UCL` and `LCL` themselves, so `BEYOND{3}`
//! is exactly the control-limit test REQ-004 names whatever the rounding of `(UCL − centre) / 3`.
//!
//! A point is *above z* when `x > upper`, *below z* when `x < lower`, *beyond z* when either,
//! and *within z* when `lower ≤ x ≤ upper`; so a point exactly at z sigma is not beyond it.
//! It is *above the centre* when `x > centre`, *below* when `x < centre`, and *on the centre
//! line* otherwise. Comparisons are IEEE 754 totalOrder on values first normalised with `+ 0.0`
//! (so `-0.0` and `+0.0` are the same value, and a `-0.0` point sits on a `+0.0` centre).
//!
//! Each primitive fires at point `i` when the pattern it names is complete at `i`, with the
//! pattern's range `from..=i` (REQ-005). A pattern that stays complete fires again at every
//! later point that completes it (a run of 10 points on one side fires `RUN_SAME_SIDE{9}` at the
//! 9th and the 10th point): REQ-RPT.STATS.SPC-005 asks for every point that breaks a rule to be
//! flagged.
//!
//! - `BEYOND{z}`: point `i` is beyond z; range `i..=i`.
//! - `K_OF_M_BEYOND{k, m, z}`: point `i` is above z (or below z), and at least `k` of the points
//!   `max(0, i − m + 1)..=i` are above z (or below z), on the same side as `i`. The range starts
//!   at the k-th most recent such point. Fewer than `m` points at the start of the sequence are
//!   enough when `k` of them qualify.
//! - `RUN_SAME_SIDE{len}`: the `len` points `i − len + 1..=i` are all above the centre or all
//!   below it; a point on the centre line breaks the run.
//! - `TREND{len}`: the `len` points ending at `i` are strictly increasing or strictly
//!   decreasing; an equal consecutive value breaks the trend.
//! - `ALTERNATING{len}`: the `len − 1` successive differences of the `len` points ending at `i`
//!   are all non-zero and alternate in sign; an equal consecutive value breaks the alternation.
//! - `RUN_WITHIN{len, z}`: the `len` points ending at `i` are all within z.
//! - `RUN_OUTSIDE{len, z}`: the `len` points ending at `i` are all beyond z, on either side.
//!
//! # Order of checks and refusals
//!
//! 1. The lines (`stats.bad_lines{reason}`): every line finite; `UCL > centre > LCL`; both
//!    half-widths `UCL − centre` and `centre − LCL` finite and at least
//!    [`SIGMA_FLOOR_EPS`] `· ε · max(|centre|, |UCL|, |LCL|)`, the rounding-resolution floor.
//! 2. The points: none at all is `stats.insufficient_data`; more than `u32::MAX` is
//!    `stats.too_many_points`; the first NaN or infinite value is `stats.non_finite{index}`.
//! 3. Each rule's thresholds, in set order: a non-finite threshold is `stats.bad_lines`
//!    (`threshold_not_finite`), and a zone that rounds onto the centre line
//!    (`upper ≤ centre` or `lower ≥ centre`), or whose `z · sigma` on either side is below the
//!    same rounding-resolution floor as the half-widths (for `z ≠ 3`), is `stats.bad_lines`
//!    (`zone_below_resolution`).
//!
//! A rule that needs more points than the sequence has cannot fire; it is reported as
//! [`RuleStatus::TooFewPoints`] in [`RuleEvaluation::rules`], never silently as "no violation".
//!
//! # Determinism (D0)
//!
//! One pass over the points, every rule advanced at each point in set order; comparisons only,
//! apart from the thresholds above. Violations come out ordered by point index and then by rule
//! position (REQ-006), and are bit-identical on every target.

use core::cmp::Ordering;
use core::fmt;
use std::collections::VecDeque;

use crate::rules_load;
use crate::spc::ReviewStatus;
use crate::StatsError;

/// The schema tag every rule-set file carries.
pub const RULESET_SCHEMA: &str = "fid.stats.ruleset/1";

/// The largest rule-set file [`RuleSet::parse`] reads, in bytes.
pub const MAX_RULESET_BYTES: usize = 65_536;

/// The most rules one set may hold.
pub const MAX_RULES: usize = 64;

/// The largest window, run length, `k` or `m` a primitive may name.
pub const MAX_WINDOW: u32 = 1_000;

/// The largest zone parameter `z` a primitive may name.
pub const MAX_Z: f64 = 1_000.0;

/// The largest rule-set version.
pub const MAX_VERSION: u32 = 999_999;

/// The rounding-resolution floor of the control-limit half-widths, in units of
/// `f64::EPSILON · max(|centre|, |UCL|, |LCL|)`.
pub const SIGMA_FLOOR_EPS: f64 = 16.0;

/// The longest rule-set ID, in bytes (`[a-z][a-z0-9_]*`).
pub const MAX_SET_ID_BYTES: usize = 64;

/// The longest rule ID, in bytes (`[a-z0-9][a-z0-9_]*`).
pub const MAX_RULE_ID_BYTES: usize = 32;

/// The ID and version of the default rule set, `beyond_limits@1` (REQ-004).
pub const DEFAULT_RULE_SET: (&str, u32) = ("beyond_limits", 1);

/// The shipped rule sets as embedded canonical JSON, `(id, version, file)` (REQ-004). The files
/// are written by `fixtures/rpt/stats/make_rule_cases.py` from the parameters it lists.
pub const SHIPPED_RULE_SETS: [(&str, u32, &[u8]); 3] = [
    ("beyond_limits", 1, include_bytes!("spc_rules/beyond_limits_v1.json")),
    ("nelson", 1, include_bytes!("spc_rules/nelson_v1.json")),
    ("western_electric", 1, include_bytes!("spc_rules/western_electric_v1.json")),
];

/// Why a rule set was refused (`stats.ruleset_invalid{reason}`, REQ-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulesetReason {
    /// The file is larger than [`MAX_RULESET_BYTES`].
    TooLarge,
    /// The file is not UTF-8 or not JSON (including NaN literals, string escapes, which the
    /// format never needs, and bytes after the value).
    Malformed,
    /// JSON nested deeper than the format allows.
    TooDeep,
    /// One object holds the same key twice.
    DuplicateKey,
    /// The top level is not an object.
    WrongType,
    /// An object holds a key the format does not define.
    UnknownKey,
    /// An object lacks a key the format requires.
    MissingKey,
    /// `schema` is not [`RULESET_SCHEMA`].
    BadSchema,
    /// The set ID is not `[a-z][a-z0-9_]*` of at most [`MAX_SET_ID_BYTES`] bytes.
    BadId,
    /// The version is not an integer in `1..=`[`MAX_VERSION`].
    BadVersion,
    /// The review status is not `UNVERIFIED`, `EXPERT_REVIEWED` or `CLAIMED_BY_FILE`, or a set
    /// built in code claims [`ReviewStatus::ExpertReviewed`].
    BadReview,
    /// `rules` is not an array.
    BadRules,
    /// The set has no rules.
    NoRules,
    /// The set has more than [`MAX_RULES`] rules.
    TooManyRules,
    /// A rule is not an object, or its `primitive` is not a string.
    BadRule,
    /// A rule names a primitive the engine does not know.
    UnknownPrimitive,
    /// A rule ID is not `[a-z0-9][a-z0-9_]*` of at most [`MAX_RULE_ID_BYTES`] bytes.
    BadRuleId,
    /// Two rules of the set share an ID.
    DuplicateRuleId,
    /// A primitive parameter is of the wrong type or out of range.
    BadParameter,
    /// No shipped rule set has this ID and version.
    UnknownRuleSet,
}

impl RulesetReason {
    /// The reason code, for example `unknown_primitive`.
    pub fn as_str(self) -> &'static str {
        match self {
            RulesetReason::TooLarge => "too_large",
            RulesetReason::Malformed => "malformed",
            RulesetReason::TooDeep => "too_deep",
            RulesetReason::DuplicateKey => "duplicate_key",
            RulesetReason::WrongType => "wrong_type",
            RulesetReason::UnknownKey => "unknown_key",
            RulesetReason::MissingKey => "missing_key",
            RulesetReason::BadSchema => "bad_schema",
            RulesetReason::BadId => "bad_id",
            RulesetReason::BadVersion => "bad_version",
            RulesetReason::BadReview => "bad_review",
            RulesetReason::BadRules => "bad_rules",
            RulesetReason::NoRules => "no_rules",
            RulesetReason::TooManyRules => "too_many_rules",
            RulesetReason::BadRule => "bad_rule",
            RulesetReason::UnknownPrimitive => "unknown_primitive",
            RulesetReason::BadRuleId => "bad_rule_id",
            RulesetReason::DuplicateRuleId => "duplicate_rule_id",
            RulesetReason::BadParameter => "bad_parameter",
            RulesetReason::UnknownRuleSet => "unknown_rule_set",
        }
    }
}

impl fmt::Display for RulesetReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why the centre line and control limits were refused (`stats.bad_lines{reason}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinesReason {
    /// The centre line is NaN or infinite.
    NonFiniteCentre,
    /// The UCL is NaN or infinite.
    NonFiniteUcl,
    /// The LCL is NaN or infinite.
    NonFiniteLcl,
    /// `UCL ≤ centre`.
    UclNotAboveCentre,
    /// `LCL ≥ centre`.
    LclNotBelowCentre,
    /// `UCL − centre` or `centre − LCL` overflows.
    SpanNotFinite,
    /// A half-width is below the rounding-resolution floor (see [`SIGMA_FLOOR_EPS`]).
    SigmaBelowResolution,
    /// A rule's threshold `centre ± z · sigma` overflows.
    ThresholdNotFinite,
    /// A rule's zone rounds onto the centre line, or its `z · sigma` is below the
    /// rounding-resolution floor.
    ZoneBelowResolution,
}

impl LinesReason {
    /// The reason code, for example `ucl_not_above_centre`.
    pub fn as_str(self) -> &'static str {
        match self {
            LinesReason::NonFiniteCentre => "non_finite_centre",
            LinesReason::NonFiniteUcl => "non_finite_ucl",
            LinesReason::NonFiniteLcl => "non_finite_lcl",
            LinesReason::UclNotAboveCentre => "ucl_not_above_centre",
            LinesReason::LclNotBelowCentre => "lcl_not_below_centre",
            LinesReason::SpanNotFinite => "span_not_finite",
            LinesReason::SigmaBelowResolution => "sigma_below_resolution",
            LinesReason::ThresholdNotFinite => "threshold_not_finite",
            LinesReason::ZoneBelowResolution => "zone_below_resolution",
        }
    }
}

impl fmt::Display for LinesReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn invalid(reason: RulesetReason) -> StatsError {
    StatsError::RulesetInvalid { reason }
}

fn bad_lines(reason: LinesReason) -> StatsError {
    StatsError::BadLines { reason }
}

/// A pattern primitive with its parameters (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    /// One point more than `z` sigma from the centre.
    Beyond {
        /// Zone, in sigma.
        z: f64,
    },
    /// `k` of `m` consecutive points more than `z` sigma from the centre on the same side.
    KOfMBeyond {
        /// Points needed.
        k: u32,
        /// Window length.
        m: u32,
        /// Zone, in sigma.
        z: f64,
    },
    /// `len` consecutive points on the same side of the centre.
    RunSameSide {
        /// Run length, in points.
        len: u32,
    },
    /// `len` consecutive strictly increasing or strictly decreasing points.
    Trend {
        /// Run length, in points.
        len: u32,
    },
    /// `len` consecutive points alternating up and down.
    Alternating {
        /// Run length, in points.
        len: u32,
    },
    /// `len` consecutive points within `z` sigma on either side.
    RunWithin {
        /// Run length, in points.
        len: u32,
        /// Zone, in sigma.
        z: f64,
    },
    /// `len` consecutive points more than `z` sigma from the centre on either side.
    RunOutside {
        /// Run length, in points.
        len: u32,
        /// Zone, in sigma.
        z: f64,
    },
}

impl Primitive {
    /// The primitive's name in a rule-set file, for example `K_OF_M_BEYOND`.
    pub fn name(&self) -> &'static str {
        match self {
            Primitive::Beyond { .. } => "BEYOND",
            Primitive::KOfMBeyond { .. } => "K_OF_M_BEYOND",
            Primitive::RunSameSide { .. } => "RUN_SAME_SIDE",
            Primitive::Trend { .. } => "TREND",
            Primitive::Alternating { .. } => "ALTERNATING",
            Primitive::RunWithin { .. } => "RUN_WITHIN",
            Primitive::RunOutside { .. } => "RUN_OUTSIDE",
        }
    }

    /// The fewest points with which the primitive can fire.
    pub fn min_points(&self) -> u32 {
        match *self {
            Primitive::Beyond { .. } => 1,
            Primitive::KOfMBeyond { k, .. } => k,
            Primitive::RunSameSide { len }
            | Primitive::Trend { len }
            | Primitive::Alternating { len }
            | Primitive::RunWithin { len, .. }
            | Primitive::RunOutside { len, .. } => len,
        }
    }

    /// The zone parameter, for the primitives that have one.
    pub fn z(&self) -> Option<f64> {
        match *self {
            Primitive::Beyond { z }
            | Primitive::KOfMBeyond { z, .. }
            | Primitive::RunWithin { z, .. }
            | Primitive::RunOutside { z, .. } => Some(z),
            Primitive::RunSameSide { .. } | Primitive::Trend { .. } | Primitive::Alternating { .. } => None,
        }
    }

    /// Checks the parameter ranges (the same rules [`RuleSet::parse`] applies).
    fn validate(&self) -> Result<(), StatsError> {
        let ok = match *self {
            Primitive::Beyond { z } => z_ok(z),
            Primitive::KOfMBeyond { k, m, z } => {
                (1..=MAX_WINDOW).contains(&k) && (1..=MAX_WINDOW).contains(&m) && k <= m && z_ok(z)
            }
            Primitive::RunSameSide { len } => (2..=MAX_WINDOW).contains(&len),
            Primitive::Trend { len } | Primitive::Alternating { len } => (3..=MAX_WINDOW).contains(&len),
            Primitive::RunWithin { len, z } | Primitive::RunOutside { len, z } => {
                (2..=MAX_WINDOW).contains(&len) && z_ok(z)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(invalid(RulesetReason::BadParameter))
        }
    }
}

/// `z` is finite, above zero and at most [`MAX_Z`] (NaN fails every comparison).
pub(crate) fn z_ok(z: f64) -> bool {
    z.is_finite() && z > 0.0 && z <= MAX_Z
}

/// `[a-z][a-z0-9_]*`, 1 to [`MAX_SET_ID_BYTES`] bytes.
pub(crate) fn set_id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_SET_ID_BYTES
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// `[a-z0-9][a-z0-9_]*`, 1 to [`MAX_RULE_ID_BYTES`] bytes.
pub(crate) fn rule_id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_RULE_ID_BYTES
        && b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// One rule: an ID and a primitive (REQ-003).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    id: String,
    primitive: Primitive,
}

impl Rule {
    /// A rule, or `stats.ruleset_invalid` (`bad_rule_id`, `bad_parameter`).
    pub fn new(id: &str, primitive: Primitive) -> Result<Rule, StatsError> {
        if !rule_id_ok(id) {
            return Err(invalid(RulesetReason::BadRuleId));
        }
        primitive.validate()?;
        Ok(Rule { id: id.to_owned(), primitive })
    }

    /// The rule ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The primitive.
    pub fn primitive(&self) -> Primitive {
        self.primitive
    }
}

/// A named, versioned run-rule set (REQ-003). Only a valid set can exist.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSet {
    id: String,
    version: u32,
    review: ReviewStatus,
    rules: Vec<Rule>,
}

impl RuleSet {
    /// A rule set, or `stats.ruleset_invalid` (`bad_id`, `bad_version`, `bad_review`, `no_rules`,
    /// `too_many_rules`, `duplicate_rule_id`). A set built in code cannot claim
    /// [`ReviewStatus::ExpertReviewed`] (`bad_review`): only the shipped sets can (D-045).
    pub fn new(id: &str, version: u32, review: ReviewStatus, rules: Vec<Rule>) -> Result<RuleSet, StatsError> {
        RuleSet::build(id, version, review, rules, false)
    }

    pub(crate) fn build(
        id: &str,
        version: u32,
        review: ReviewStatus,
        rules: Vec<Rule>,
        shipped: bool,
    ) -> Result<RuleSet, StatsError> {
        if review == ReviewStatus::ExpertReviewed && !shipped {
            return Err(invalid(RulesetReason::BadReview));
        }
        if !set_id_ok(id) {
            return Err(invalid(RulesetReason::BadId));
        }
        if !(1..=MAX_VERSION).contains(&version) {
            return Err(invalid(RulesetReason::BadVersion));
        }
        if rules.is_empty() {
            return Err(invalid(RulesetReason::NoRules));
        }
        if rules.len() > MAX_RULES {
            return Err(invalid(RulesetReason::TooManyRules));
        }
        for (i, r) in rules.iter().enumerate() {
            if rules.iter().take(i).any(|p| p.id == r.id) {
                return Err(invalid(RulesetReason::DuplicateRuleId));
            }
            // A `Rule` is valid by construction; checked again so a set can never hold a bad one.
            r.primitive.validate()?;
            if !rule_id_ok(&r.id) {
                return Err(invalid(RulesetReason::BadRuleId));
            }
        }
        Ok(RuleSet { id: id.to_owned(), version, review, rules })
    }

    /// Loads a rule-set file (canonical JSON or any JSON of the same shape; see
    /// [`crate::rules_load`]), or `stats.ruleset_invalid{reason}`: no rule of a refused file is
    /// ever evaluated (REQ-007).
    ///
    /// A review the file claims (`EXPERT_REVIEWED` or `CLAIMED_BY_FILE`) is loaded as
    /// [`ReviewStatus::ClaimedByFile`], never as [`ReviewStatus::ExpertReviewed`] (D-045).
    pub fn parse(bytes: &[u8]) -> Result<RuleSet, StatsError> {
        rules_load::parse(bytes, rules_load::Origin::File).map_err(invalid)
    }

    /// The set in canonical JSON: sorted keys, no whitespace, integers without a fraction, and
    /// each `z` in the shortest decimal that reads back as the same `f64`. Two equal sets have
    /// the same encoding, and [`RuleSet::parse`] of it returns an equal set.
    pub fn to_canonical_json(&self) -> String {
        rules_load::encode(self)
    }

    /// The set ID, for example `nelson`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The set version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The review status the set carries.
    pub fn review(&self) -> ReviewStatus {
        self.review
    }

    /// The rules, in evaluation order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}

/// The shipped rule set `id@version` (REQ-004), or `stats.ruleset_invalid` (`unknown_rule_set`).
pub fn shipped_rule_set(id: &str, version: u32) -> Result<RuleSet, StatsError> {
    match SHIPPED_RULE_SETS.iter().find(|(i, v, _)| *i == id && *v == version) {
        Some((_, _, bytes)) => rules_load::parse(bytes, rules_load::Origin::Shipped).map_err(invalid),
        None => Err(invalid(RulesetReason::UnknownRuleSet)),
    }
}

/// The default rule set, `beyond_limits@1` (REQ-004).
pub fn default_rule_set() -> Result<RuleSet, StatsError> {
    shipped_rule_set(DEFAULT_RULE_SET.0, DEFAULT_RULE_SET.1)
}

/// The centre line and control limits the points are plotted against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lines {
    /// Centre line.
    pub centre: f64,
    /// Upper control limit.
    pub ucl: f64,
    /// Lower control limit (for example 0 on an R chart with small subgroups).
    pub lcl: f64,
}

/// One flagged pattern (REQ-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleViolation {
    /// The point that completed the pattern.
    pub index: u32,
    /// The rule-set ID.
    pub set_id: String,
    /// The rule-set version.
    pub set_version: u32,
    /// The rule ID.
    pub rule_id: String,
    /// The first point of the pattern.
    pub from: u32,
    /// The last point of the pattern (always [`RuleViolation::index`]).
    pub to: u32,
    /// The rule set's review status, repeated so no flag from an `UNVERIFIED` set can be read
    /// as settled.
    pub review: ReviewStatus,
}

/// Whether a rule could fire on this sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleStatus {
    /// The rule was evaluated over every point.
    Evaluated,
    /// The sequence is shorter than the rule's pattern: the rule could not fire, which is not
    /// the same as finding the process in control.
    TooFewPoints {
        /// The fewest points with which the rule can fire.
        needed: u32,
    },
}

/// The outcome of one rule of the set.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleOutcome {
    /// The rule ID.
    pub rule_id: String,
    /// The primitive evaluated.
    pub primitive: Primitive,
    /// Evaluated, or too few points.
    pub status: RuleStatus,
    /// Number of violations this rule raised.
    pub violations: u32,
    /// The upper and lower thresholds `(upper, lower)` of the rule's zone, for primitives with a
    /// `z`.
    pub zone: Option<(f64, f64)>,
}

/// The result of [`evaluate_rules`].
#[derive(Debug, Clone, PartialEq)]
pub struct RuleEvaluation {
    /// The rule-set ID.
    pub set_id: String,
    /// The rule-set version.
    pub set_version: u32,
    /// The rule set's review status.
    pub review: ReviewStatus,
    /// Number of points evaluated.
    pub n_points: u32,
    /// Every violation, ordered by point index and then by rule position (REQ-006).
    pub violations: Vec<RuleViolation>,
    /// One outcome per rule, in set order.
    pub rules: Vec<RuleOutcome>,
}

/// The validated lines with each side's sigma.
struct Zones {
    centre: f64,
    ucl: f64,
    lcl: f64,
    sigma_above: f64,
    sigma_below: f64,
    /// The rounding-resolution floor `SIGMA_FLOOR_EPS · ε · max|line|`.
    floor: f64,
}

/// `a > b` in IEEE 754 totalOrder (both already finite and normalised).
fn gt(a: f64, b: f64) -> bool {
    a.total_cmp(&b) == Ordering::Greater
}

/// `a < b` in IEEE 754 totalOrder (both already finite and normalised).
fn lt(a: f64, b: f64) -> bool {
    a.total_cmp(&b) == Ordering::Less
}

impl Zones {
    fn new(lines: &Lines) -> Result<Zones, StatsError> {
        if !lines.centre.is_finite() {
            return Err(bad_lines(LinesReason::NonFiniteCentre));
        }
        if !lines.ucl.is_finite() {
            return Err(bad_lines(LinesReason::NonFiniteUcl));
        }
        if !lines.lcl.is_finite() {
            return Err(bad_lines(LinesReason::NonFiniteLcl));
        }
        let centre = lines.centre + 0.0;
        let ucl = lines.ucl + 0.0;
        let lcl = lines.lcl + 0.0;
        if !gt(ucl, centre) {
            return Err(bad_lines(LinesReason::UclNotAboveCentre));
        }
        if !lt(lcl, centre) {
            return Err(bad_lines(LinesReason::LclNotBelowCentre));
        }
        let up = ucl - centre;
        let down = centre - lcl;
        if !up.is_finite() || !down.is_finite() {
            return Err(bad_lines(LinesReason::SpanNotFinite));
        }
        let scale = abs_max3(centre, ucl, lcl);
        let floor = SIGMA_FLOOR_EPS * f64::EPSILON * scale;
        if lt(up, floor) || lt(down, floor) {
            return Err(bad_lines(LinesReason::SigmaBelowResolution));
        }
        Ok(Zones { centre, ucl, lcl, sigma_above: up / 3.0, sigma_below: down / 3.0, floor })
    }

    /// `(upper, lower)` for zone `z`; `z = 3` is the control limits themselves.
    fn thresholds(&self, z: f64) -> Result<(f64, f64), StatsError> {
        // Exactly 3 (bit for bit): the control limits themselves, whose half-widths already
        // passed the floor in `Zones::new`.
        let three = z.to_bits() == 3.0_f64.to_bits();
        let (above, below) = (z * self.sigma_above, z * self.sigma_below);
        let (upper, lower) =
            if three { (self.ucl, self.lcl) } else { (self.centre + above + 0.0, self.centre - below + 0.0) };
        if !upper.is_finite() || !lower.is_finite() {
            return Err(bad_lines(LinesReason::ThresholdNotFinite));
        }
        // The same rounding-resolution floor as the half-widths applies to z · sigma, so a tiny z
        // cannot make a zone only an ulp or two wide.
        if (!three && (lt(above, self.floor) || lt(below, self.floor)))
            || !gt(upper, self.centre)
            || !lt(lower, self.centre)
        {
            return Err(bad_lines(LinesReason::ZoneBelowResolution));
        }
        Ok((upper, lower))
    }
}

/// `max(|a|, |b|, |c|)` of finite values, in total order.
fn abs_max3(a: f64, b: f64, c: f64) -> f64 {
    let mut m = a.abs();
    for x in [b.abs(), c.abs()] {
        if gt(x, m) {
            m = x;
        }
    }
    m
}

/// The sign of `b − a` in total order: +1 up, −1 down, 0 equal.
fn step_dir(a: f64, b: f64) -> i8 {
    match b.total_cmp(&a) {
        Ordering::Greater => 1,
        Ordering::Less => -1,
        Ordering::Equal => 0,
    }
}

/// The running state of one rule.
enum State {
    Beyond { upper: f64, lower: f64 },
    KOfM { k: usize, m: u32, upper: f64, lower: f64, above: VecDeque<u32>, below: VecDeque<u32> },
    SameSide { len: u32, centre: f64, side: i8, run: u32 },
    Trend { len: u32, dir: i8, run: u32 },
    Alternating { len: u32, last: i8, run: u32 },
    Within { len: u32, upper: f64, lower: f64, run: u32 },
    Outside { len: u32, upper: f64, lower: f64, run: u32 },
}

/// The start of a run of `len` points ending at `i`.
fn run_from(i: u32, len: u32) -> u32 {
    i.saturating_sub(len.saturating_sub(1))
}

impl State {
    fn new(p: Primitive, zones: &Zones) -> Result<(State, Option<(f64, f64)>), StatsError> {
        let zone = match p.z() {
            Some(z) => Some(zones.thresholds(z)?),
            None => None,
        };
        let (upper, lower) = zone.unwrap_or((zones.ucl, zones.lcl));
        let st = match p {
            Primitive::Beyond { .. } => State::Beyond { upper, lower },
            Primitive::KOfMBeyond { k, m, .. } => {
                State::KOfM { k: k as usize, m, upper, lower, above: VecDeque::new(), below: VecDeque::new() }
            }
            Primitive::RunSameSide { len } => State::SameSide { len, centre: zones.centre, side: 0, run: 0 },
            Primitive::Trend { len } => State::Trend { len, dir: 0, run: 0 },
            Primitive::Alternating { len } => State::Alternating { len, last: 0, run: 0 },
            Primitive::RunWithin { len, .. } => State::Within { len, upper, lower, run: 0 },
            Primitive::RunOutside { len, .. } => State::Outside { len, upper, lower, run: 0 },
        };
        Ok((st, zone))
    }

    /// Advances to point `i` with value `x` (`prev` is point `i − 1`); returns the pattern's
    /// first index when the pattern is complete at `i`.
    fn step(&mut self, i: u32, x: f64, prev: Option<f64>) -> Option<u32> {
        match self {
            State::Beyond { upper, lower } => (gt(x, *upper) || lt(x, *lower)).then_some(i),
            State::KOfM { k, m, upper, lower, above, below } => {
                let start = run_from(i, *m);
                for q in [&mut *above, &mut *below] {
                    while q.front().is_some_and(|&j| j < start) {
                        q.pop_front();
                    }
                }
                let q = if gt(x, *upper) {
                    above
                } else if lt(x, *lower) {
                    below
                } else {
                    return None;
                };
                q.push_back(i);
                if q.len() >= *k {
                    q.get(q.len() - *k).copied()
                } else {
                    None
                }
            }
            State::SameSide { len, centre, side, run } => {
                let s = if gt(x, *centre) {
                    1
                } else if lt(x, *centre) {
                    -1
                } else {
                    0
                };
                *run = if s == 0 {
                    0
                } else if s == *side {
                    run.saturating_add(1)
                } else {
                    1
                };
                *side = s;
                (*run >= *len).then_some(run_from(i, *len))
            }
            State::Trend { len, dir, run } => {
                match prev {
                    None => {
                        *run = 1;
                        *dir = 0;
                    }
                    Some(p) => {
                        let d = step_dir(p, x);
                        if d == 0 {
                            *run = 1;
                        } else if d == *dir {
                            *run = run.saturating_add(1);
                        } else {
                            *run = 2;
                        }
                        *dir = d;
                    }
                }
                (*run >= *len).then_some(run_from(i, *len))
            }
            State::Alternating { len, last, run } => {
                match prev {
                    None => {
                        *run = 1;
                        *last = 0;
                    }
                    Some(p) => {
                        let d = step_dir(p, x);
                        if d == 0 {
                            *run = 1;
                        } else if *last != 0 && d == -*last {
                            *run = run.saturating_add(1);
                        } else {
                            *run = 2;
                        }
                        *last = d;
                    }
                }
                (*run >= *len).then_some(run_from(i, *len))
            }
            State::Within { len, upper, lower, run } => {
                *run = if gt(x, *upper) || lt(x, *lower) { 0 } else { run.saturating_add(1) };
                (*run >= *len).then_some(run_from(i, *len))
            }
            State::Outside { len, upper, lower, run } => {
                *run = if gt(x, *upper) || lt(x, *lower) { run.saturating_add(1) } else { 0 };
                (*run >= *len).then_some(run_from(i, *len))
            }
        }
    }
}

/// Evaluates rule set `set` over `points` plotted against `lines` (REQ-005, REQ-006).
///
/// # Errors
///
/// In this order: `stats.bad_lines{reason}` for the lines, `stats.insufficient_data` for no
/// points, `stats.too_many_points`, `stats.non_finite{index}` for the first NaN or infinite
/// point, then `stats.bad_lines{reason}` for a rule's zone. Nothing is returned with an error.
pub fn evaluate_rules(points: &[f64], lines: &Lines, set: &RuleSet) -> Result<RuleEvaluation, StatsError> {
    let zones = Zones::new(lines)?;
    if points.is_empty() {
        return Err(StatsError::InsufficientData);
    }
    // Not covered by a test: reaching it needs a slice of more than u32::MAX f64 values (over
    // 32 GiB), which no CI runner can allocate, and wasm32 cannot even address it.
    let n_points = u32::try_from(points.len()).map_err(|_| StatsError::TooManyPoints)?;
    let mut xs = Vec::with_capacity(points.len());
    for (index, &x) in points.iter().enumerate() {
        if !x.is_finite() {
            return Err(StatsError::NonFinite { index });
        }
        xs.push(x + 0.0);
    }
    let mut states = Vec::with_capacity(set.rules.len());
    let mut rules = Vec::with_capacity(set.rules.len());
    for r in &set.rules {
        let (st, zone) = State::new(r.primitive, &zones)?;
        states.push(st);
        let needed = r.primitive.min_points();
        let status = if n_points < needed { RuleStatus::TooFewPoints { needed } } else { RuleStatus::Evaluated };
        rules.push(RuleOutcome { rule_id: r.id.clone(), primitive: r.primitive, status, violations: 0, zone });
    }
    let mut violations = Vec::new();
    let mut prev = None;
    for (i, &x) in (0..n_points).zip(xs.iter()) {
        for ((st, rule), outcome) in states.iter_mut().zip(&set.rules).zip(rules.iter_mut()) {
            if let Some(from) = st.step(i, x, prev) {
                outcome.violations = outcome.violations.saturating_add(1);
                violations.push(RuleViolation {
                    index: i,
                    set_id: set.id.clone(),
                    set_version: set.version,
                    rule_id: rule.id.clone(),
                    from,
                    to: i,
                    review: set.review,
                });
            }
        }
        prev = Some(x);
    }
    Ok(RuleEvaluation {
        set_id: set.id.clone(),
        set_version: set.version,
        review: set.review,
        n_points,
        violations,
        rules,
    })
}
