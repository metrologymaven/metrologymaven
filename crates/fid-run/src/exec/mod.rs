//! Running a prepared plan (spec node `AUTO.RUN.EXEC`). Modules land one spec leaf at a time.
//!
//! - [`branch`] (`AUTO.RUN.EXEC.BRANCH`): the run's own journal branch, its Run entity, the
//!   [`branch::RunTarget`] every commit of the run goes through, and the one-time
//!   finalisation of the outcome.

pub mod branch;
