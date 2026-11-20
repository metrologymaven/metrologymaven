//! Crash and power-loss harness for the project store (spec node `CORE.STO.SQL.CRASH`).
//!
//! A writer child process commits a synthetic hash chain ([`chain`]) through `fid-store` and
//! acknowledges each commit on its stdout only after the commit returned; the parent kills it
//! at seeded instants ([`plan`]), reopens the store and checks that the chain is intact, that
//! every acknowledged commit is there and that every referenced blob is stored and sound
//! ([`adapter::verify_store`]). A failed check is a [`CrashFailure`] naming the seed and the
//! kill instant, and (with the `bus-errors` feature) maps to a bus error of the published
//! catalogue (`CrashFailure::to_bus_error`). Other payloads plug in through [`CrashAdapter`];
//! without `bus-errors` the crate reaches only model-layer crates, so `fid-journal` tests can
//! use it as a dev-dependency.
//!
//! Test-only parts are behind the `fault-inject` feature, which only this crate's
//! dev-dependency on itself enables: the two deliberately broken writer modes of the
//! self-tests and [`adapter::StepProbe`], which uses `fid-store`'s commit probe (its
//! `test-probe` feature, enabled only through `fault-inject`) to hang or fail a writer at an
//! exact commit step. A normal build has neither.

pub mod adapter;
pub mod chain;
pub mod failure;
pub mod harness;
pub mod plan;
pub mod power;

pub use adapter::{verify_store, Ack, CheckLimits, CrashAdapter, StoreAdapter, WriterMode};
pub use chain::SyntheticRow;
pub use failure::{CrashFailure, FailureKind};
pub use harness::{run, run_writer, Config, Summary, Writer};
pub use plan::{KillInstant, KillPlan};
pub use power::PowerCut;
