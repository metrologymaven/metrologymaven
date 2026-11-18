//! Metrology Maven platform support (spec node `PLAT`). Layer `tool` in `layers.toml`.
//!
//! - [`matrix`]: the support matrix schema, validator and tier lookup (`PLAT.MATRIX.FILE`).
//! - [`hw_profile`]: the hardware profile file schema and validator (`PLAT.HW.PROFILE`).
//! - [`bench_schema`]: the benchmark suite manifest and result document schemas, their
//!   validators and the result JSON Schema export (`PLAT.HW.SCHEMA`).
//! - [`kiosk_display`]: the reference kiosk display file, its validator and derived pixel
//!   density (`PLAT.DEPLOY.KIOSK.DISPLAY`).
//! - [`gpu_policy`]: the GPU backend policy and known-bad driver list files, their validators
//!   and the known-bad lookup (`PLAT.GPU.POLICY`).

pub mod bench_schema;
pub mod gpu_policy;
pub mod hw_profile;
pub mod kiosk_display;
pub mod matrix;
