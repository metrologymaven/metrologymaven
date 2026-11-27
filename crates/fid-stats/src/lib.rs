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

pub mod cap;
pub mod hist;
pub mod spc;
mod spc_table;

pub use cap::{
    capability, BadInput, EstimatorId, Family, IndexPair, IndexValue, Side, StatsError, CAP_INDEX_ALGORITHM_ID,
    DEFAULT_MIN_N, MAX_ESTIMATOR_ID_BYTES,
};
pub use hist::{histogram, BinRule, BinningReason, Bins, RuleId, HISTOGRAM_MAX_BINS};
pub use spc::{
    constants, spc_table, Citation, ReviewStatus, SpcConstants, SpcRow, StoredValue, SPC_MAX_N, SPC_MIN_N,
    SPC_TABLE_ID, SPC_TABLE_JSON, SPC_TABLE_VERSION,
};
