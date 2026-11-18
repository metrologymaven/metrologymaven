//! Local storage for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/sto/`.

pub mod cas;
pub mod layout;

pub use cas::{Address, BlobRef, BlobStore, CasError, ChunkManifest, ObjectRef, Probe, PutOpts};
pub use layout::{Area, HexName, LockInfo, Locked, OpenMode, ProjectLocation, StoreError, StoreWarning};
