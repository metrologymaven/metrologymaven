//! Metrology Maven outreach tooling (domain OUTR).
//!
//! - [`metrics::def`]: adoption metric definitions, source list and no-telemetry guard (spec node
//!   `OUTR.COMM.METRIC.DEF`).
//! - [`launch_plan`]: the milestone criteria file (`launch/criteria.toml`), its parser and the
//!   `plan-check` rules (spec node `OUTR.LAUNCH.PLAN.FMT`). The gate and review checks reuse the
//!   parsed types.

pub mod launch_plan;
pub mod metrics;
