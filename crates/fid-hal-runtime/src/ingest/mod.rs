//! Data-plane ingest (spec node `HAL.ACQ.INGEST`). Modules land one spec leaf at a time.
//!
//! - [`credit`]: the per-stream credit ledger for flow control (`HAL.ACQ.INGEST.CREDIT`).

pub mod credit;
