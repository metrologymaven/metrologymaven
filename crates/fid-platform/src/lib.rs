//! Metrology Maven platform support (spec node `PLAT`). Layer `tool` in `layers.toml`.
//!
//! - [`matrix`]: the support matrix schema, validator and tier lookup (`PLAT.MATRIX.FILE`).
//! - [`hw_profile`]: the hardware profile file schema and validator (`PLAT.HW.PROFILE`).
//! - [`bench_schema`]: the benchmark suite manifest and result document schemas, their
//!   validators and the result JSON Schema export (`PLAT.HW.SCHEMA`).

pub mod bench_schema;
pub mod hw_profile;
pub mod matrix;
