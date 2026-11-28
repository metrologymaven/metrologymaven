//! Scan edge analysis of Metrology Maven (spec node `SCAN.EDGE`). Pure Rust, `f64` metres, no
//! I/O, no `unsafe`; kernel graph, KNP-1 (no third-party dependencies).
//!
//! Implemented leaves:
//!
//! * `SCAN.EDGE.EST.EFIX`: the synthetic edge fixture generator and the named fixture set, in
//!   [`testkit`] behind the `testkit` feature (test support only; never enabled in a release
//!   build).
//!
//! The edge-estimation methods (`SCAN.EDGE.EST.TANG`, `SCAN.EDGE.EST.SILH`), their default
//! (`SCAN.EDGE.EST.DEFS`), the nominal edge chain (`SCAN.EDGE.NOMEDGE`) and the extraction
//! interface (`SCAN.EDGE.MESHBND`) are not built yet.

#[cfg(feature = "testkit")]
pub mod testkit;
