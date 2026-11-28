//! Point-cloud operations of Metrology Maven (spec node `SCAN.CLOUD`). Pure Rust, `f64` metres,
//! no I/O, no `unsafe`; kernel layer, KNP-1 (no FMA, square roots only through `fid-num`, no
//! std `HashMap` or `HashSet`, total-order comparisons, NaN is an error).
//!
//! Implemented leaves: `SCAN.CLOUD.REG.CTGT` (the [`OrientedCloudTarget`] projection target,
//! [`overlap_fraction`] and [`stride_sample`], see [`reg::target`]) and `SCAN.CLOUD.REG.GRAPH`
//! (the overlap matrix and pair graph, [`pair_graph`], see [`reg::graph`]) and
//! `SCAN.CLOUD.REG.RREP` (the [`RegistrationReport`] and the registered-version writer
//! [`write_registered`], see [`reg::report`]). Pairwise and global registration
//! (`SCAN.CLOUD.REG.PAIR`, `SCAN.CLOUD.REG.GLOBAL`) are not built yet.

pub mod reg;

pub use reg::graph::{pair_graph, pair_graph_with, PairGraph, PairOverlap, ScanInput, DEFAULT_MIN_OVERLAP, MAX_SCANS};
pub use reg::report::{
    rigid_from_matrix4, write_registered, GraphPair, GraphPairStatus, GraphSummary, NewCloudVersion, PairResiduals,
    PairStat, RegStatus, RegisteredMarker, RegistrationReport, ScanPose, SolverSummary, ACCURACY_BASIS,
    GROUP_NAMESPACE, REPORT_SCHEMA, RESIDUAL_KIND,
};
pub use reg::target::{
    overlap_fraction, overlap_fraction_with, stride_sample, Bitmask, CloudProjection, NoHitReason, NoNormalsReason,
    OrientedCloudTarget, Overlap, RegError, StrideSample,
};
