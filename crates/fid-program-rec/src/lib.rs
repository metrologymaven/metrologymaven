//! Record to program and the release lifecycle of Command Programs and scripts (spec node
//! `AUTO.REC`). Modules land one spec leaf at a time; see `spec/auto/rec/`.
//!
//! - [`adopt`]: the agent-provenance gate on release (`AUTO.REC.ADOPT`, CONSTITUTION C5).

pub mod adopt;
