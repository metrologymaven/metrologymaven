//! Metrology Maven acceptance tooling (FLOW).
//!
//! - [`journey`]: the journey-map format `journey-map@1` and its structural validator (spec
//!   node FLOW.PERSONA.SCHEMA).
//! - [`package`]: the acceptance-job package `acceptance-job@1`, its validator and the suite
//!   index `acceptance/suite.json` (spec node FLOW.RUNNER.PKG).
//! - [`persona`]: the persona definitions file `acceptance/personas/personas.json`
//!   (`persona-set@1`) and its check (spec node FLOW.PERSONA.PERS).
//! - [`report`]: the nightly acceptance report `acceptance-report@1`, append-only trend
//!   history and `accept.show_report@1` (spec node FLOW.RUNNER.VERDICT).
//! - [`study`]: the study task list `acceptance/study/tasks.json` (`study-tasks@1`) and its
//!   check (spec node FLOW.STUDY.PROTO).
//! - [`unassist`]: unassisted-run principal check over golden journals and Activity Logs
//!   (spec node FLOW.RUNNER.UNASSIST).

pub mod journey;
pub mod package;
pub mod persona;
pub mod report;
pub mod study;
pub mod unassist;
