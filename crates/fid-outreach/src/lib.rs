//! Metrology Maven outreach tooling (spec node `OUTR`). Layer `tool` in `layers.toml`.
//!
//! - [`launch_plan`]: the milestone criteria file (`launch/criteria.toml`), its parser and the
//!   `plan-check` rules (`OUTR.LAUNCH.PLAN.FMT`). The gate and review checks reuse the parsed types.

pub mod launch_plan;
