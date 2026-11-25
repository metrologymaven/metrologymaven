//! `fid-platform-core`: the support matrix and hardware profile files of spec domain `PLAT`,
//! below the tool layer (layer `model` in `layers.toml`) so that `fid-doctor`
//! (`PLAT.DOCTOR.LOOKUP`, app layer) can read them: ECO.REPO.LAYERS forbids any crate from
//! depending on a `tool` crate such as `fid-platform`. Moved unchanged from `fid-platform`,
//! which re-exports both modules under the same paths, so there is still one parser and one
//! meaning of "tier" and "at least the minimum" (the same move `fid-gpu-policy` made for
//! `PLAT.GPU.DETECT`).
//!
//! - [`matrix`]: the support matrix schema, validator and tier lookup (`PLAT.MATRIX.FILE`).
//! - [`hw_profile`]: the hardware profile file schema and validator (`PLAT.HW.PROFILE`).
//!
//! Pure functions over bytes (D0); no I/O, no clock. Nothing here panics.

#![forbid(unsafe_code)]

pub mod hw_profile;
pub mod matrix;
