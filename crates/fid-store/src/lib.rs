//! Local storage for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/sto/`.

pub mod cache;
pub mod cas;
pub mod layout;
pub mod schema;
pub mod txn;

pub use cache::{Cache, CacheConfig, CacheKey, CacheMode, CacheWarning};
pub use cas::{Address, BlobRef, BlobStore, CasError, ChunkManifest, ObjectRef, Probe, PutOpts};
pub use layout::{Area, HexName, LockInfo, Locked, OpenMode, ProjectLocation, StoreError, StoreWarning};
pub use schema::{Database, Migration, MigrationPlan, SchemaError, SchemaVersion};
pub use txn::{Committed, RowOp, Store, Txn, TxnError, Value};
