//! Uncertainty commands of Metrology Maven (layer L2, outside the kernel).
//!
//! - [`contrib`]: the `contributor/v1` contract, sub-graph expansion (`expand`),
//!   library loader, and read commands `unc.list_contributor@1` and
//!   `unc.show_contributor@1` (spec node `UNC.MODEL.CONTRIB.CONTRACT`).
//! - [`model_store`]: the project store of Uncertainty Model versions and the commands
//!   `unc.create_model@1`, `unc.validate_model@1` and `unc.show_model@1` (spec node
//!   `UNC.MODEL.GRAPH.STORE`), with version immutability, release-manifest provenance and
//!   [`model_store::resolve`] for engines.
//!
//! The kernel types, canonical form, digest and validation of a model are in `fid-kunc`
//! (`model`); this crate adds storage, contributors, the journal, the principal and the surfaces.

pub mod contrib;
pub mod model_store;
