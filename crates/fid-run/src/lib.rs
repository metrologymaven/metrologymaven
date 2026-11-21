//! Runs of Command Programs (spec node `AUTO.RUN`). Modules land one spec leaf at a time; see
//! `spec/auto/run/`.
//!
//! This crate holds `AUTO.RUN.BIND`: run binding validation and run records.
//!
//! - [`prepare_run`] (module [`prepare`]): lifecycle and principal rules, the binding check,
//!   input digests, and the [`RunPlan`] with its [`RunRecord`] (REQ-AUTO.RUN.BIND-001..005).
//! - [`record`]: the run record, its canonical JSON, its strict reader and its JSON Schema.
//! - [`ledger`]: the `RunStarted` event `AUTO.RUN.EXEC` commits and the origin-bound fold of
//!   run records (REQ-CORE.RPL.RECON.FOLD-008).
//!
//! No command is declared here: `program.run@1` and `program.run_batch@1` (`AUTO.RUN.EXEC`,
//! `AUTO.RUN.BATCH`) call this library. Nothing here panics, uses floating point in a
//! decision, or converts a unit.

pub mod ledger;
pub mod prepare;
pub mod record;

pub use ledger::{run_started, Applied, RunEntry, RunLedger, RUN_COMMAND, RUN_STARTED};
pub use prepare::{
    decided_lifecycle, prepare_run, BindCtx, BindingSource, InputSource, ProgramRef, RunError, RunFlags, RunIds,
    RunPlan,
};
pub use record::{record_schema, InputDigest, Outcome, RecordError, RunRecord, RECORD_KEYS, RECORD_SCHEMA};
