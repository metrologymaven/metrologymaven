//! `fid-fixes`: the suggested-fix catalogue (spec node `FLOW.DEADEND.CATALOG`).
//!
//! One data file, `fix-catalogue@1`, maps every unresolved-state reason code
//! (`<domain>.<code>`) to one or more suggested fixes, each naming a registry command ID with
//! its major, an argument template and a UX label key. [`catalogue`] holds the format, the
//! bounded loader, the generated JSON Schema and the catalogue digest; [`shipped`] loads the
//! catalogue built into this crate.
//!
//! Pure data handling (side-effect class R0, determinism D0): ordered maps only, no clock.

pub mod catalogue;

pub use catalogue::{
    catalogue_schema, load, load_path, shipped, ArgTemplate, FixCatalogue, FixEntry, FixError, FixTemplate, StateKind,
    CATALOGUE_VERSION, MAX_BYTES, MAX_DEPTH, PLACEHOLDERS, SHIPPED_JSON,
};
