//! Metrology Maven acceptance tooling (FLOW).
//!
//! - [`persona`]: the persona definitions file `acceptance/personas/personas.json`
//!   (`persona-set@1`) and its check (spec node FLOW.PERSONA.PERS).
//! - [`study`]: the study task list `acceptance/study/tasks.json` (`study-tasks@1`) and its
//!   check (spec node FLOW.STUDY.PROTO).

pub mod persona;
pub mod study;
