//! `fid-gpu-policy`: the GPU backend policy and known-bad driver list of spec node
//! `PLAT.GPU.POLICY`, below the tool layer (layer `model` in `layers.toml`) so that
//! `PLAT.GPU.DETECT` and the viewer can read them (maintainer decision on the layering of
//! `PLAT.GPU.DETECT`, 2026-11-21). Moved unchanged from `fid-platform`, which re-exports
//! everything here and keeps the support-matrix cross-check, so there is one validator.
//!
//! - [`policy`]: the schemas of `platform/gpu-policy.toml` and `platform/gpu-known-bad.toml`,
//!   their strict, bounded validators, the safe defaults and the known-bad lookup.
//! - [`Arch`], [`OsFamily`], [`Version`]: the platform identity types shared with the support
//!   matrix (`fid-platform::matrix` re-exports them).
//!
//! Pure functions over bytes (D0); no I/O, no clock. Nothing here panics.

pub mod policy;
mod types;

pub use types::{Arch, OsFamily, Version};
