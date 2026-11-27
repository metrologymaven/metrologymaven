//! Metrology Maven statistics kernel (spec domain `RPT.STATS`, kernel graph, KNP-1).
//!
//! Pure functions over numbers the caller has already gathered: no I/O, no journal, no
//! third-party dependencies, NaN is always an error. The commands that feed them
//! (data sets, `stats.compute_capability@1`) live in higher-layer crates.
//!
//! - [`cap`]: the capability index function `stats.cap.index@1.0.0` (`RPT.STATS.CAP.INDEX`).
//! - [`spc`]: the control-chart constants table `spc.constants.normal@1.0.0`
//!   (`RPT.STATS.SPC.CONST`).
//! - [`hist`]: the histogram binning kernel with rules `sturges@1`, `fixed_count@1` and
//!   `fixed_width@1` (`RPT.STATS.TREND.HIST`).
//! - [`rules`]: run-rule sets (`beyond_limits@1`, `western_electric@1`, `nelson@1`) and
//!   violation detection `evaluate_rules` (`RPT.STATS.SPC.RULES`); [`rules_load`] is the
//!   rule-set file format.

pub mod cap;
pub mod hist;
pub mod rules;
pub mod rules_load;
pub mod spc;
mod spc_table;

pub use cap::{
    capability, BadInput, EstimatorId, Family, IndexPair, IndexValue, Side, StatsError, CAP_INDEX_ALGORITHM_ID,
    DEFAULT_MIN_N, MAX_ESTIMATOR_ID_BYTES,
};
pub use hist::{histogram, BinRule, BinningReason, Bins, RuleId, HISTOGRAM_MAX_BINS};
pub use rules::{
    default_rule_set, evaluate_rules, shipped_rule_set, Lines, LinesReason, Primitive, Rule, RuleEvaluation,
    RuleOutcome, RuleSet, RuleStatus, RuleViolation, RulesetReason, DEFAULT_RULE_SET, MAX_RULES, MAX_RULESET_BYTES,
    MAX_RULE_ID_BYTES, MAX_SET_ID_BYTES, MAX_VERSION, MAX_WINDOW, MAX_Z, RULESET_SCHEMA, SHIPPED_RULE_SETS,
    SIGMA_FLOOR_EPS,
};
pub use spc::{
    constants, spc_table, Citation, ReviewStatus, SpcConstants, SpcRow, StoredValue, SPC_MAX_N, SPC_MIN_N,
    SPC_TABLE_ID, SPC_TABLE_JSON, SPC_TABLE_VERSION,
};
