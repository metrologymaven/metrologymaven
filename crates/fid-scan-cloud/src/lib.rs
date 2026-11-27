//! Point-cloud operations of Metrology Maven (spec node `SCAN.CLOUD`). Pure Rust, `f64` metres,
//! no I/O, no `unsafe`; kernel layer, KNP-1 (no FMA, square roots only through `fid-num`, no
//! std `HashMap` or `HashSet`, total-order comparisons, NaN is an error).
//!
//! Implemented leaves: `SCAN.CLOUD.REG.CTGT` (the [`OrientedCloudTarget`] projection target,
//! [`overlap_fraction`] and [`stride_sample`], see [`reg::target`]). Pairwise and global
//! registration (`SCAN.CLOUD.REG.PAIR`, `SCAN.CLOUD.REG.GLOBAL`), the pair graph
//! (`SCAN.CLOUD.REG.GRAPH`) and the registered-copy writer (`SCAN.CLOUD.REG.RREP`) are not built
//! yet.

pub mod reg;

pub use reg::target::{
    overlap_fraction, overlap_fraction_with, stride_sample, Bitmask, CloudProjection, NoHitReason, NoNormalsReason,
    OrientedCloudTarget, Overlap, RegError, StrideSample,
};
