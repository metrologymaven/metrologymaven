//! CAD assemblies and instance placement (spec node `GEO.CAD.ASSY`).
//!
//! - [`model`]: [`Instance`], [`InstancePath`], per-instance entity IDs, the sorted
//!   [`InstanceIndex`] and the [`InstanceView`] (`GEO.CAD.ASSY.MODEL`).
//!
//! Flattening an assembly tree (`GEO.CAD.ASSY.FLAT`) and the commands (`GEO.CAD.ASSY.CMD`)
//! are other leaves.

pub mod model;

pub use model::{
    AssyModelError, BodyId, Instance, InstanceEntityId, InstanceId, InstanceIndex, InstanceIx, InstancePath,
    InstanceView, ASSY_NAMESPACE, MAX_INSTANCE_ENTITIES, MAX_PATH_DEPTH,
};
