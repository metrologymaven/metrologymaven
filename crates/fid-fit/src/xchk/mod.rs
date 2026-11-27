//! Combinatorial cross-checks for the conic envelope fits (spec node `KRN.ENV.XCHK`).
//!
//! Solver-free algorithms whose answers the conic minimum circumscribed (`KRN.ENV.MCC`) and
//! maximum inscribed (`KRN.ENV.MIC`) fits are compared with in CI:
//!
//! - [`mcc_circle2d_welzl`] and [`mcc_sphere_welzl`]: the minimum enclosing 2D circle and
//!   sphere by Welzl's algorithm in its iterative move-to-front form (explicit frame stack, no
//!   recursion) over a shuffle drawn from the `fid-num` RNG with the caller's seed
//!   (REQ-KRN.ENV.XCHK-001). The problem is convex, so the answer is the global optimum, and it
//!   does not depend on the seed: the support set is made canonical and the radius is
//!   recomputed from it (REQ-002). The pinned rules are in `src/xchk/welzl.rs`.
//! - [`mic_circle2d_vor`]: the maximum inscribed 2D circle whose centre lies in a given convex
//!   polygon, from the nearest-point Voronoi diagram of `KRN.MZ.VOR` (REQ-003), with
//!   `on_region_boundary` when the optimum lies on the polygon's boundary (REQ-004).
//!   [`hull_region`] gives the convex hull of the points as a region and [`intersect_convex`]
//!   intersects two convex regions (for example the hull with a trust-region polygon). The
//!   pinned rules are in `src/xchk/mic.rs`.
//!
//! Algorithm IDs (spec cards in `cards/`): [`MCC_CIRCLE2D_ID`], [`MCC_SPHERE_ID`],
//! [`MIC_CIRCLE2D_ID`]. Constraints: KNP-1 (plain binary64 `+ - * /`, `fid_num::f` for square
//! roots and total order, no FMA, no `f64::min`/`max`, no hashing), D0, no `unsafe`, no panics,
//! no recursion.

mod mic;
mod welzl;

pub use mic::{hull_region, intersect_convex, mic_circle2d_vor, region_tol_floor, Inscribed};
pub use welzl::{mcc_circle2d_welzl, mcc_sphere_welzl, Enclosing, CANON_SEED, CANON_STREAM, SHUFFLE_STREAM};

use fid_kci::AlgorithmId;

/// `fit.circle2d.mcc_welzl@1.0.0`: minimum enclosing 2D circle (Welzl).
pub const MCC_CIRCLE2D_ID: &str = "fit.circle2d.mcc_welzl@1.0.0";
/// `fit.sphere.mcc_welzl@1.0.0`: minimum enclosing sphere (Welzl).
pub const MCC_SPHERE_ID: &str = "fit.sphere.mcc_welzl@1.0.0";
/// `fit.circle2d.mic_vor@1.0.0`: maximum inscribed 2D circle in a convex region (Voronoi).
pub const MIC_CIRCLE2D_ID: &str = "fit.circle2d.mic_vor@1.0.0";

fn id(feature: &str, method: &str) -> AlgorithmId {
    AlgorithmId {
        module: "fit".to_string(),
        feature: feature.to_string(),
        method: method.to_string(),
        version: (1, 0, 0),
    }
}

/// The parsed [`MCC_CIRCLE2D_ID`].
pub fn mcc_circle2d_id() -> AlgorithmId {
    id("circle2d", "mcc_welzl")
}

/// The parsed [`MCC_SPHERE_ID`].
pub fn mcc_sphere_id() -> AlgorithmId {
    id("sphere", "mcc_welzl")
}

/// The parsed [`MIC_CIRCLE2D_ID`].
pub fn mic_circle2d_id() -> AlgorithmId {
    id("circle2d", "mic_vor")
}

/// The three algorithm IDs of this module, in the order above (for the spec-card check).
pub fn algorithm_ids() -> [AlgorithmId; 3] {
    [mcc_circle2d_id(), mcc_sphere_id(), mic_circle2d_id()]
}
