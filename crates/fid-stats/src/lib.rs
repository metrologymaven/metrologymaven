//! Metrology Maven statistics kernel (spec domain `RPT.STATS`, kernel graph, KNP-1).
//!
//! Pure functions over numbers the caller has already gathered: no I/O, no journal, no
//! third-party dependencies, NaN is always an error. The commands that feed them
//! (data sets, `stats.compute_capability@1`) live in higher-layer crates.
//!
//! - [`cap`]: the capability index function `stats.cap.index@1.0.0` (`RPT.STATS.CAP.INDEX`).

pub mod cap;

pub use cap::{
    capability, BadInput, EstimatorId, Family, IndexPair, IndexValue, Side, StatsError, CAP_INDEX_ALGORITHM_ID,
    DEFAULT_MIN_N, MAX_ESTIMATOR_ID_BYTES,
};
