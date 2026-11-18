//! Metrology Maven outreach tooling (domain OUTR).
//!
//! - [`metrics::def`]: adoption metric definitions, source list and no-telemetry guard (spec node
//!   `OUTR.COMM.METRIC.DEF`).
//! - [`qa::a11y`]: static accessibility gate for built pages (spec node `OUTR.SITE.QA.A11Y`).

pub mod metrics;
pub mod qa;
