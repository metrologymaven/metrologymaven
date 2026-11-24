//! Running a prepared plan (spec node `AUTO.RUN.EXEC`). Modules land one spec leaf at a time.
//!
//! - [`branch`] (`AUTO.RUN.EXEC.BRANCH`): the run's own journal branch, its Run entity, the
//!   [`branch::RunTarget`] every commit of the run goes through, and the one-time
//!   finalisation of the outcome.
//! - [`retarget`] (`AUTO.RUN.EXEC.RETGT`): the [`retarget::StepGate`] pre-step hook and its
//!   first gate, the [`retarget::RetargetGuard`] that stops a re-targeted run before a step
//!   marked `retargetable: false` (`not_retargetable`).

pub mod branch;
pub mod retarget;
