//! Metrology Maven acceptance tooling (FLOW).
//!
//! - [`journey`]: the journey-map format `journey-map@1` and its structural validator (spec
//!   node FLOW.PERSONA.SCHEMA).
//! - [`package`]: the acceptance-job package `acceptance-job@1`, its validator and the suite
//!   index `acceptance/suite.json` (spec node FLOW.RUNNER.PKG).
//! - [`persona`]: the persona definitions file `acceptance/personas/personas.json`
//!   (`persona-set@1`) and its check (spec node FLOW.PERSONA.PERS).
//! - [`study`]: the study task list `acceptance/study/tasks.json` (`study-tasks@1`) and its
//!   check (spec node FLOW.STUDY.PROTO).

pub mod journey;
pub mod package;
pub mod persona;
pub mod study;
