//! Selection model of Metrology Maven (spec node `GEO.SEL`). Pure Rust, no I/O, no
//! floating point, no `unsafe`; kernel graph, KNP-1 (no third-party dependencies).
//!
//! Implemented leaf: `GEO.SEL.MODEL` (see [`model`]): the [`IndexSet`] of sorted unique
//! indexes stored as coalesced half-open ranges over one [`Domain`] of an element, the
//! [`Selection`] that binds an index set (or an opaque [`SelectionQuery`]) to a target
//! element ID and that element's [`ElementDigest`](fid_geo_types::ElementDigest), the
//! canonical encoding and SHA-256 [`SelectionDigest`] of an index set, and [`apply`] with
//! the stale-selection check.
//!
//! Query language and evaluation (`GEO.SEL.QUERY`), pick and lasso resolution
//! (`GEO.SEL.RESOLVE`), set algebra (`GEO.SEL.ALGEBRA`) and commands (`GEO.SEL.CMD`) are
//! other leaves.

pub mod model;

pub use model::{
    apply, domain_count, Domain, EntityId, IndexSet, IndexSetBuilder, SelError, Selection, SelectionBody,
    SelectionDigest, SelectionQuery,
};
