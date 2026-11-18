//! Metrology Maven acceptance tooling (FLOW).
//!
//! - [`package`]: the acceptance-job package `acceptance-job@1`, its validator and the suite
//!   index `acceptance/suite.json` (spec node FLOW.RUNNER.PKG).
//! - [`persona`]: the persona definitions file `acceptance/personas/personas.json`
//!   (`persona-set@1`) and its check (spec node FLOW.PERSONA.PERS).
//! - [`study`]: the study task list `acceptance/study/tasks.json` (`study-tasks@1`) and its
//!   check (spec node FLOW.STUDY.PROTO).

pub mod package;
pub mod persona;
pub mod study;
