//! Degree-of-freedom accounting and the unconstrained-DoF policy (spec node `ALN.DATUM.DOF`,
//! REQ-ALN.DATUM.DOF-001 to -006).
//!
//! The bookkeeping that element-based and datum-system alignment share: the invariance subspace
//! of each nominal feature (the rigid motions that leave it unchanged), how many DoF each pair
//! removes after the higher-priority ones, which motions stay free and what the solver does with
//! them, and the projector that keeps a lower-priority pair from disturbing a higher-priority
//! one. Every rule below is pinned; the independent reference
//! `fixtures/aln/datum/make_dof_cases.py` recomputes the counts, the free subspaces and the
//! projectors in exact rational arithmetic and the names by the same documented rules.
//!
//! # Twists
//!
//! A twist is a 6-vector `(w, v)`: the angular part `w` (rad) and the linear part `v` (m) of the
//! velocity field `x -> v + w x x` about the alignment origin. A rotation about the unit
//! direction `a` through the point `p` is `(a, p x a)`; a translation along `t` is `(0, t)`.
//!
//! # Checks (pair by pair, in priority order; the first failure is returned)
//!
//! 1. No pairs is `empty_pairs`; more than [`MAX_DOF_PAIRS`] is `input_limit`.
//! 2. The feature type must have a row in the table below, else `unsupported_feature_type`
//!    (for example [`FeatureType::Torus`]).
//! 3. The location: the first NaN component is `nan_input`, the first infinite one
//!    `infinite_input`.
//! 4. The direction: required for a plane (its normal), line, cylinder, cone and 3D circle (the
//!    axis), else `missing_direction`; refused for a sphere and a point, which have none
//!    (`unexpected_direction`, a parameter that could not be applied); a NaN or infinite
//!    component as for the location; all zeros is `zero_direction`. The direction is
//!    normalised (`red::norm2`, one division per component); its sign does not matter.
//!
//! # Invariance table (REQ-001)
//!
//! | Type | Invariance basis | Constrained |
//! |---|---|---|
//! | plane (point `p`, normal `n`) | `(0, t1)`, `(0, t2)`, `(n, p x n)` | 3 |
//! | line, cylinder (point `p`, axis `a`) | `(0, a)`, `(a, p x a)` | 4 |
//! | cone (apex `p`), 3D circle (centre `p`), axis `a` | `(a, p x a)` | 5 |
//! | sphere (centre `p`), point (`p`) | `(e_i, p x e_i)`, `i = x, y, z` | 3 |
//!
//! `t1 = unit(n x e_k)` with `e_k` the frame axis of the smallest `|n_k|` (first on ties) and
//! `t2 = unit(n x t1)`. [`constrained_basis`] returns this basis in twist units with the
//! constrained count `6 - k`.
//!
//! # Metric
//!
//! The radius `R` is the largest `red::norm2(location)` over the pairs, recorded
//! ([`RadiusBasis::MaxLocation`]); when every location is the origin it is 1 m, recorded as
//! [`RadiusBasis::UnitFallback`]. The scaled coordinates of a twist are `(R w, v)`, so a scaled
//! rotation component is the arc length at the radius. "Orthonormal" and "orthogonal projector"
//! below are in these scaled coordinates. For the accounting each pair's basis is built with
//! `p / R` in place of `p` (the same span, entries of order 1).
//!
//! # Accumulation (REQ-002)
//!
//! `Q` is an orthonormal basis of the free subspace, starting as the 6 x 6 identity. For each
//! pair in priority order:
//!
//! 1. **Projector** (REQ-004): `P = Q Q^T` (each entry a `red::dot` over the columns of `Q`)
//!    is recorded as [`PairAccount::projector_before`]: the orthogonal projector onto the free
//!    subspace the higher-priority pairs leave. A solver restricting this pair's pose update to
//!    it keeps every higher-priority nominal feature invariant to first order.
//! 2. **Pair subspace:** `U` is the thin left factor of `fid_num::la::svd` of the pair's scaled
//!    basis `B` (6 x k). Its singular values are classified with the rank rule below; fewer than
//!    `k` kept is `degenerate_feature` (not reachable from a valid feature: the basis vectors
//!    are orthogonal-or-unit by construction).
//! 3. **Intersection:** `M = Q - U (U^T Q)` (6 x f; the part of each free direction outside the
//!    pair's subspace, every inner product a `red::dot`). The singular values `sigma` of `M`
//!    (`fid_num::la::svd`, descending) above the rank tolerance are the DoF this pair removes:
//!    the **effective** count (REQ-002, the drop in free dimension). The right singular vectors
//!    of the others span the intersection: the new `Q` is `Q V_null`. This is the intersection
//!    of the previous free subspace with the pair's invariance subspace (the Design builds it
//!    from the null space of `[A, -B]`; restricting `B`'s complement to `Q` gives the same
//!    subspace and keeps the count monotone, `0 <= effective <= nominal`, which is checked:
//!    `rank_inconsistent` otherwise). With no free DoF left, the effective count is 0 and no
//!    decision is made.
//! 4. **Redundancy** (REQ-006): an effective count of 0 adds [`DofWarning::RedundantPair`].
//!
//! **Rank rule:** a singular value counts when it is above [`DOF_RANK_TOL`] times the largest
//! singular value of the operator it belongs to. For `M` that operator is `I - U U^T`, whose
//! largest singular value is exactly 1, so the tolerance is the absolute `1e-9` (an absolute and
//! relative floor at once, far above rounding resolution). For `B` it is relative to `B`'s own
//! largest. The value 1e-9 is the spec's `[unverified]` default: every account records it with
//! [`RuleBasis::DefaultUnverified`] (D-045) and its `interpretation` is `provisional`. A
//! singular value in `(1e-9 / 1e3, 1e-9 * 1e3]` adds [`DofWarning::NearRankThreshold`] (the
//! band factor [`DOF_NEAR_FACTOR`] is also `[unverified]`), and each pair records the smallest
//! kept and largest dropped singular value, so a decision near the threshold is visible. A
//! block whose largest entry is subnormal (below `2^-1022`, which `fid_num::la::svd` refuses)
//! is below rounding resolution: it is taken as zero and [`DofWarning::BelowRoundingResolution`]
//! records its largest entry.
//!
//! # Free subspace and names (REQ-003)
//!
//! The final `Q` (f columns) is split by the same rank rule applied to the singular values of
//! its angular block `Q_w` (3 x f, padded with zero rows to f x f when f > 3): the right
//! singular vectors of the dropped values give the translations `T` (`w = 0`), the kept ones
//! their orthogonal complement `Rot`. Each part is then put in a canonical orthonormal basis,
//! independent of the SVD's choice of vectors: for the candidates `e_vx, e_vy, e_vz` (for `T`)
//! or `e_wx, e_wy, e_wz` (for `Rot`), in order, the candidate is projected onto the part, the
//! vectors already accepted are removed (modified Gram-Schmidt, twice) and the residual is
//! accepted (normalised) when its norm is above `0.5` (for `T`) or `0.5 * s` (for `Rot`, `s` the
//! smallest kept singular value of `Q_w`). Those bounds guarantee a full basis in exact
//! arithmetic (each remaining unit direction has squared candidate components summing to at
//! least 1, or `s^2`, over three candidates); should rounding leave the basis short, the
//! largest residual (first on ties) is taken and counted in
//! [`DofAccount::naming_fallbacks`]. The free basis is `T`'s vectors then `Rot`'s.
//!
//! **Signs:** each vector is negated when the largest-magnitude component (first on ties) of
//! its linear part (for `T`) or angular part (for `Rot`) is negative.
//!
//! **Names** (`NAME_TOL` is [`DOF_NAME_TOL`], `[unverified]`): a `T` vector is a translation
//! along `unit(v)`. A `Rot` vector is a rotation when `|w . v| <= NAME_TOL |w| |v|`, about
//! `a = unit(w)` through `q = R (w x v) / |w|^2` (the point of its axis nearest the origin), and a
//! generic twist otherwise (with its pitch `R (w . v) / |w|^2`, m/rad). A rotation is said to go
//! through the axis of the first axial pair (line, cylinder, cone, 3D circle) whose axis it
//! coincides with (`|a x a_i| <= NAME_TOL` and `|(p_i - q) x a| <= NAME_TOL R`), else the centre
//! of the first sphere, point, cone apex or circle centre on it (`|(p_i - q) x a| <= NAME_TOL R`),
//! else the alignment origin (`|q| <= NAME_TOL R`), else the point `q`. A direction within
//! `NAME_TOL` of a frame axis is written `+x` (and so on); any other direction or point is
//! written with nine decimals (`{:.9}`, `-0.000000000` written `0.000000000`). Pair indices in
//! names are 0-based priority positions.
//!
//! # Policy (REQ-005)
//!
//! With a non-empty free subspace, [`UnconstrainedPolicy::KeepParent`] gives
//! [`FreeDirective::HoldAtParent`]: the solver holds the free components at the parent
//! alignment's values. [`UnconstrainedPolicy::Reject`] fails with `underconstrained`, listing
//! the free DoF. With none free the directive is [`FreeDirective::FullyConstrained`].
//!
//! Nothing here panics; NaN is always an error; no FMA, no `f64::min`/`f64::max`, no hash
//! containers; every reduction is a `fid-num` fixed-order one, so the bases and projectors are
//! bit-identical on every platform (D0 under KNP-1).

use core::fmt;

use fid_num::la::{svd, Mat};
use fid_num::{f, red, NumError};

/// The relative singular-value threshold of every rank decision (REQ-ALN.DATUM.DOF-002),
/// `[unverified]` in the spec.
pub const DOF_RANK_TOL: f64 = 1e-9;
/// The factor around [`DOF_RANK_TOL`] within which a singular value is reported as near the
/// threshold, `[unverified]`.
pub const DOF_NEAR_FACTOR: f64 = 1e3;
/// The tolerance of the naming tests (axis labels, pitch, coincidence), `[unverified]`.
pub const DOF_NAME_TOL: f64 = 1e-9;
/// Most pairs [`accumulate`] accepts.
pub const MAX_DOF_PAIRS: usize = 4096;
/// The acceptance bound of the canonical basis of the translations.
const CANON_TAU: f64 = 0.5;

/// A twist: angular part (rad) then linear part (m) about the alignment origin.
pub type Twist = [f64; 6];

/// A nominal feature type. Only the types of the invariance table are supported; any other
/// kernel primitive is listed so that it can be refused by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeatureType {
    Plane,
    Line,
    Cylinder,
    Cone,
    Circle3d,
    Sphere,
    Point,
    /// Not in the table: `unsupported_feature_type`.
    Torus,
}

impl FeatureType {
    /// Every type, in declaration order.
    pub const ALL: [FeatureType; 8] = [
        FeatureType::Plane,
        FeatureType::Line,
        FeatureType::Cylinder,
        FeatureType::Cone,
        FeatureType::Circle3d,
        FeatureType::Sphere,
        FeatureType::Point,
        FeatureType::Torus,
    ];

    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureType::Plane => "plane",
            FeatureType::Line => "line",
            FeatureType::Cylinder => "cylinder",
            FeatureType::Cone => "cone",
            FeatureType::Circle3d => "circle3d",
            FeatureType::Sphere => "sphere",
            FeatureType::Point => "point",
            FeatureType::Torus => "torus",
        }
    }

    /// The type with this exact wire name, if any.
    pub fn from_name(name: &str) -> Option<FeatureType> {
        FeatureType::ALL.into_iter().find(|t| t.as_str() == name)
    }

    /// The constrained-DoF count of the table (REQ-001), `None` for an unsupported type.
    pub fn constrained_count(self) -> Option<usize> {
        match self {
            FeatureType::Plane | FeatureType::Sphere | FeatureType::Point => Some(3),
            FeatureType::Line | FeatureType::Cylinder => Some(4),
            FeatureType::Cone | FeatureType::Circle3d => Some(5),
            FeatureType::Torus => None,
        }
    }

    /// Whether the type takes a direction (a plane's normal or an axis).
    pub fn has_direction(self) -> bool {
        matches!(
            self,
            FeatureType::Plane | FeatureType::Line | FeatureType::Cylinder | FeatureType::Cone | FeatureType::Circle3d
        )
    }

    fn is_axial(self) -> bool {
        matches!(self, FeatureType::Line | FeatureType::Cylinder | FeatureType::Cone | FeatureType::Circle3d)
    }

    fn has_centre(self) -> bool {
        matches!(self, FeatureType::Sphere | FeatureType::Point | FeatureType::Cone | FeatureType::Circle3d)
    }
}

impl fmt::Display for FeatureType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A nominal feature in the alignment frame: its type, a location (a point of the plane, a
/// point of the axis, the cone apex, the circle or sphere centre, the point), metres, and its
/// direction (the plane normal or the axis; `None` for a sphere or a point). It is checked by
/// [`constrained_basis`] and [`accumulate`], not on construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NominalFeature {
    pub feature_type: FeatureType,
    pub location: [f64; 3],
    pub direction: Option<[f64; 3]>,
}

impl NominalFeature {
    pub fn new(feature_type: FeatureType, location: [f64; 3], direction: Option<[f64; 3]>) -> NominalFeature {
        NominalFeature { feature_type, location, direction }
    }
}

/// What the solver does with the free DoF (REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnconstrainedPolicy {
    /// Hold them at the parent alignment's values (`keep_parent`).
    KeepParent,
    /// Refuse the alignment (`reject`).
    Reject,
}

impl UnconstrainedPolicy {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            UnconstrainedPolicy::KeepParent => "keep_parent",
            UnconstrainedPolicy::Reject => "reject",
        }
    }
}

/// The solver directive for the free subspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FreeDirective {
    /// No DoF is free (`fully_constrained`).
    FullyConstrained,
    /// Hold the free components at the parent alignment's values (`hold_at_parent`).
    HoldAtParent,
}

impl FreeDirective {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            FreeDirective::FullyConstrained => "fully_constrained",
            FreeDirective::HoldAtParent => "hold_at_parent",
        }
    }
}

/// Where the radius of the metric comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RadiusBasis {
    /// The largest distance of a feature location from the origin (`max_location`).
    MaxLocation,
    /// Every location is the origin, so 1 m (`unit_fallback`).
    UnitFallback,
}

impl RadiusBasis {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            RadiusBasis::MaxLocation => "max_location",
            RadiusBasis::UnitFallback => "unit_fallback",
        }
    }
}

/// Where a rule constant comes from (D-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleBasis {
    /// The spec's default, not calibrated (`default_unverified`).
    DefaultUnverified,
}

impl RuleBasis {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            RuleBasis::DefaultUnverified => "default_unverified",
        }
    }
}

/// The invariance subspace of one feature in twist units (REQ-001), from [`constrained_basis`].
#[derive(Debug, Clone, PartialEq)]
pub struct TwistBasis {
    vectors: Vec<Twist>,
}

impl TwistBasis {
    /// The basis vectors, in the table's order (not orthonormal: unit directions, rotations
    /// through the feature location).
    pub fn vectors(&self) -> &[Twist] {
        &self.vectors
    }

    /// The invariance dimension.
    pub fn dim(&self) -> usize {
        self.vectors.len()
    }

    /// The constrained-DoF count, `6 - dim`.
    pub fn constrained(&self) -> usize {
        6 - self.vectors.len()
    }
}

/// The accounting of one pair.
#[derive(Debug, Clone, PartialEq)]
pub struct PairAccount {
    feature_type: FeatureType,
    nominal: usize,
    effective: usize,
    free_before: usize,
    free_after: usize,
    projector_before: [[f64; 6]; 6],
    kept_min: Option<f64>,
    dropped_max: Option<f64>,
}

impl PairAccount {
    pub fn feature_type(&self) -> FeatureType {
        self.feature_type
    }

    /// The constrained count of the feature type alone (REQ-001).
    pub fn nominal(&self) -> usize {
        self.nominal
    }

    /// The DoF this pair removes after the higher-priority ones (REQ-002).
    pub fn effective(&self) -> usize {
        self.effective
    }

    /// Free dimension before this pair.
    pub fn free_before(&self) -> usize {
        self.free_before
    }

    /// Free dimension after this pair.
    pub fn free_after(&self) -> usize {
        self.free_after
    }

    /// The orthogonal projector (scaled coordinates, row-major) onto the free subspace the
    /// higher-priority pairs leave (REQ-004); the identity for the first pair.
    pub fn projector_before(&self) -> &[[f64; 6]; 6] {
        &self.projector_before
    }

    /// The smallest singular value counted as constrained, `None` when none was.
    pub fn kept_min(&self) -> Option<f64> {
        self.kept_min
    }

    /// The largest singular value counted as still free, `None` when none was.
    pub fn dropped_max(&self) -> Option<f64> {
        self.dropped_max
    }
}

/// The kind of a free motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Motion {
    /// A translation along [`FreeDof::direction`].
    Translation,
    /// A rotation about [`FreeDof::direction`] through [`FreeDof::point`].
    Rotation,
    /// A screw motion with a pitch.
    Twist,
}

impl Motion {
    /// `translation`, `rotation` or `twist`.
    pub fn as_str(self) -> &'static str {
        match self {
            Motion::Translation => "translation",
            Motion::Rotation => "rotation",
            Motion::Twist => "twist",
        }
    }
}

/// What a rotation axis was found to go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Through {
    /// The axis of an axial pair.
    Axis { pair: usize, feature_type: FeatureType },
    /// The centre (apex) of a pair.
    Centre { pair: usize, feature_type: FeatureType },
    /// The alignment origin.
    Origin,
    /// None of these: the point [`FreeDof::point`].
    Point,
}

/// One vector of the free subspace's orthonormal basis, named (REQ-003).
#[derive(Debug, Clone, PartialEq)]
pub struct FreeDof {
    motion: Motion,
    scaled: Twist,
    twist: Twist,
    direction: [f64; 3],
    point: Option<[f64; 3]>,
    pitch: Option<f64>,
    through: Option<Through>,
    name: String,
}

impl FreeDof {
    pub fn motion(&self) -> Motion {
        self.motion
    }

    /// The basis vector in scaled coordinates `(R w, v)` (orthonormal with the others).
    pub fn scaled(&self) -> Twist {
        self.scaled
    }

    /// The same vector in twist units `(w, v)`, rad and m.
    pub fn twist(&self) -> Twist {
        self.twist
    }

    /// The unit translation direction or rotation (screw) axis direction.
    pub fn direction(&self) -> [f64; 3] {
        self.direction
    }

    /// The point of the rotation or screw axis nearest the origin, metres; `None` for a
    /// translation.
    pub fn point(&self) -> Option<[f64; 3]> {
        self.point
    }

    /// The pitch of a rotation or twist, m/rad; `None` for a translation.
    pub fn pitch(&self) -> Option<f64> {
        self.pitch
    }

    /// What a rotation goes through; `None` otherwise.
    pub fn through(&self) -> Option<Through> {
        self.through
    }

    /// The readable name, for example `rotation about +z through the axis of pair 1 (cylinder)`.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A warning attached to the account.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DofWarning {
    /// The pair removes no DoF (REQ-006).
    RedundantPair { pair: usize },
    /// A singular value within [`DOF_NEAR_FACTOR`] of the rank threshold; `pair` is `None` for
    /// the translation split of the free subspace.
    NearRankThreshold { pair: Option<usize>, sigma: f64 },
    /// A block whose largest entry is subnormal was taken as zero; `pair` as above.
    BelowRoundingResolution { pair: Option<usize>, max_entry: f64 },
}

impl DofWarning {
    /// `redundant_pair`, `near_rank_threshold` or `below_rounding_resolution`.
    pub fn code(&self) -> &'static str {
        match self {
            DofWarning::RedundantPair { .. } => "redundant_pair",
            DofWarning::NearRankThreshold { .. } => "near_rank_threshold",
            DofWarning::BelowRoundingResolution { .. } => "below_rounding_resolution",
        }
    }
}

/// The DoF account of an ordered list of pairs (REQ-002 to -006).
#[derive(Debug, Clone, PartialEq)]
pub struct DofAccount {
    radius: f64,
    radius_basis: RadiusBasis,
    per_pair: Vec<PairAccount>,
    free: Vec<FreeDof>,
    directive: FreeDirective,
    warnings: Vec<DofWarning>,
    rank_decisions: usize,
    naming_fallbacks: usize,
}

impl DofAccount {
    /// The radius `R` of the scaled metric, metres.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn radius_basis(&self) -> RadiusBasis {
        self.radius_basis
    }

    /// The rank tolerance used, [`DOF_RANK_TOL`].
    pub fn rank_tolerance(&self) -> f64 {
        DOF_RANK_TOL
    }

    /// The basis of the rank, band and naming constants: not calibrated (D-045).
    pub fn rule_basis(&self) -> RuleBasis {
        RuleBasis::DefaultUnverified
    }

    /// `provisional`: the counts rest on the uncalibrated rank threshold (D-045).
    pub fn interpretation(&self) -> &'static str {
        "provisional"
    }

    /// One entry per pair, in priority order.
    pub fn per_pair(&self) -> &[PairAccount] {
        &self.per_pair
    }

    /// The free subspace after all pairs, as named orthonormal vectors (REQ-003).
    pub fn free(&self) -> &[FreeDof] {
        &self.free
    }

    /// The free dimension.
    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    /// The names of [`DofAccount::free`].
    pub fn free_names(&self) -> Vec<&str> {
        self.free.iter().map(FreeDof::name).collect()
    }

    pub fn directive(&self) -> FreeDirective {
        self.directive
    }

    pub fn warnings(&self) -> &[DofWarning] {
        &self.warnings
    }

    /// How many singular values were classified by the rank rule (feature bases, the
    /// intersections and the translation split).
    pub fn rank_decisions(&self) -> usize {
        self.rank_decisions
    }

    /// How many canonical basis vectors needed the largest-residual fallback.
    pub fn naming_fallbacks(&self) -> usize {
        self.naming_fallbacks
    }

    /// Restricts the twist `(w, v)` (rad, m) to the free subspace left before `pair`
    /// (REQ-004): scales it, applies [`PairAccount::projector_before`] and scales back. `None`
    /// for a pair index out of range; an error for a non-finite input or result.
    pub fn project(&self, pair: usize, twist: &Twist) -> Option<Result<Twist, DofError>> {
        let p = self.per_pair.get(pair)?;
        Some(project_with(p.projector_before(), self.radius, twist))
    }
}

fn project_with(proj: &[[f64; 6]; 6], radius: f64, twist: &Twist) -> Result<Twist, DofError> {
    scan(None, "twist", twist)?;
    let mut s = *twist;
    for x in s.iter_mut().take(3) {
        *x *= radius;
    }
    let mut out = [0.0; 6];
    for (o, row) in out.iter_mut().zip(proj.iter()) {
        *o = dot(row, &s)?;
    }
    for x in out.iter_mut().take(3) {
        *x /= radius;
    }
    finite(&out, "project")?;
    Ok(out)
}

/// Every way [`constrained_basis`] and [`accumulate`] refuse. Codes are stable.
#[derive(Debug, Clone, PartialEq)]
pub enum DofError {
    /// No pairs.
    EmptyPairs,
    /// More than [`MAX_DOF_PAIRS`] pairs.
    InputLimit { count: usize },
    /// The feature type has no row in the invariance table.
    UnsupportedFeatureType { pair: usize, feature_type: FeatureType },
    /// A NaN; `field` is `location`, `direction` or `twist`, `index` the component.
    NanInput { pair: Option<usize>, field: &'static str, index: usize },
    /// An infinity; as [`DofError::NanInput`].
    InfiniteInput { pair: Option<usize>, field: &'static str, index: usize },
    /// A plane, line, cylinder, cone or 3D circle without a direction.
    MissingDirection { pair: usize },
    /// A sphere or point with a direction.
    UnexpectedDirection { pair: usize },
    /// An all-zero direction.
    ZeroDirection { pair: usize },
    /// A feature basis below full rank (not reachable from a valid feature).
    DegenerateFeature { pair: usize },
    /// The effective count exceeded the nominal count (not reachable in exact arithmetic).
    RankInconsistent { pair: usize },
    /// Free DoF remain and the policy is `reject` (REQ-005); the free DoF are listed.
    Underconstrained { free: Vec<FreeDof> },
    /// The canonical free basis could not be completed.
    NamingFailed,
    /// An intermediate or output value was not finite; `op` names the step.
    NonFinite { op: &'static str },
    /// A `fid-num` call refused.
    Numeric(NumError),
    /// A buffer could not be reserved.
    AllocationFailed,
}

impl DofError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            DofError::EmptyPairs => "empty_pairs",
            DofError::InputLimit { .. } => "input_limit",
            DofError::UnsupportedFeatureType { .. } => "unsupported_feature_type",
            DofError::NanInput { .. } => "nan_input",
            DofError::InfiniteInput { .. } => "infinite_input",
            DofError::MissingDirection { .. } => "missing_direction",
            DofError::UnexpectedDirection { .. } => "unexpected_direction",
            DofError::ZeroDirection { .. } => "zero_direction",
            DofError::DegenerateFeature { .. } => "degenerate_feature",
            DofError::RankInconsistent { .. } => "rank_inconsistent",
            DofError::Underconstrained { .. } => "underconstrained",
            DofError::NamingFailed => "naming_failed",
            DofError::NonFinite { .. } => "non_finite",
            DofError::Numeric(_) => "numeric",
            DofError::AllocationFailed => "allocation_failed",
        }
    }

    /// The pair the refusal names, if any.
    pub fn pair(&self) -> Option<usize> {
        match self {
            DofError::UnsupportedFeatureType { pair, .. }
            | DofError::MissingDirection { pair }
            | DofError::UnexpectedDirection { pair }
            | DofError::ZeroDirection { pair }
            | DofError::DegenerateFeature { pair }
            | DofError::RankInconsistent { pair } => Some(*pair),
            DofError::NanInput { pair, .. } | DofError::InfiniteInput { pair, .. } => *pair,
            _ => None,
        }
    }
}

impl fmt::Display for DofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            DofError::InputLimit { count } => write!(f, " ({count} pairs, at most {MAX_DOF_PAIRS})"),
            DofError::UnsupportedFeatureType { pair, feature_type } => write!(f, " (pair {pair}: {feature_type})"),
            DofError::NanInput { pair, field, index } | DofError::InfiniteInput { pair, field, index } => match pair {
                Some(p) => write!(f, " (pair {p} {field}[{index}])"),
                None => write!(f, " ({field}[{index}])"),
            },
            DofError::MissingDirection { pair }
            | DofError::UnexpectedDirection { pair }
            | DofError::ZeroDirection { pair }
            | DofError::DegenerateFeature { pair }
            | DofError::RankInconsistent { pair } => write!(f, " (pair {pair})"),
            DofError::Underconstrained { free } => {
                write!(f, " ({} free:", free.len())?;
                for (i, d) in free.iter().enumerate() {
                    write!(f, "{} {}", if i == 0 { "" } else { ";" }, d.name())?;
                }
                f.write_str(")")
            }
            DofError::NonFinite { op } => write!(f, " ({op})"),
            DofError::Numeric(e) => write!(f, " ({e})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for DofError {}

// ------------------------------------------------------------------ helpers

fn scan(pair: Option<usize>, field: &'static str, xs: &[f64]) -> Result<(), DofError> {
    for (index, x) in xs.iter().enumerate() {
        if x.is_nan() {
            return Err(DofError::NanInput { pair, field, index });
        }
        if x.is_infinite() {
            return Err(DofError::InfiniteInput { pair, field, index });
        }
    }
    Ok(())
}

fn finite(xs: &[f64], op: &'static str) -> Result<(), DofError> {
    if xs.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(DofError::NonFinite { op })
    }
}

fn buffer<T>(n: usize) -> Result<Vec<T>, DofError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| DofError::AllocationFailed)?;
    Ok(v)
}

fn dot(a: &[f64], b: &[f64]) -> Result<f64, DofError> {
    red::dot(a, b).map_err(DofError::Numeric)
}

fn norm(a: &[f64]) -> Result<f64, DofError> {
    red::norm2(a).map_err(DofError::Numeric)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn twist(w: [f64; 3], v: [f64; 3]) -> Twist {
    [w[0], w[1], w[2], v[0], v[1], v[2]]
}

fn angular(t: &Twist) -> [f64; 3] {
    [t[0], t[1], t[2]]
}

fn linear(t: &Twist) -> [f64; 3] {
    [t[3], t[4], t[5]]
}

fn unit(d: [f64; 3], op: &'static str) -> Result<[f64; 3], DofError> {
    let n = norm(&d)?;
    if n == 0.0 {
        return Err(DofError::NonFinite { op });
    }
    let u = d.map(|x| x / n);
    finite(&u, op)?;
    Ok(u)
}

/// `sum_j coeff[j] * cols[j]`, each component one `red::dot`.
fn combine(cols: &[Twist], coeff: &[f64]) -> Result<Twist, DofError> {
    let mut out = [0.0; 6];
    let mut comp = buffer(cols.len())?;
    for (i, o) in out.iter_mut().enumerate() {
        comp.clear();
        comp.extend(cols.iter().map(|c| c[i]));
        *o = dot(&comp, coeff)?;
    }
    Ok(out)
}

/// The index of the largest `|x|` (first on ties).
fn largest(xs: &[f64]) -> usize {
    let mut best = 0usize;
    let mut top = 0.0_f64;
    for (j, x) in xs.iter().enumerate() {
        if j == 0 || x.abs() > top {
            best = j;
            top = x.abs();
        }
    }
    best
}

/// A checked feature: its type, location and unit direction (if any).
struct Valid {
    ty: FeatureType,
    p: [f64; 3],
    a: Option<[f64; 3]>,
}

fn validate(feature: &NominalFeature, pair: usize) -> Result<Valid, DofError> {
    let ty = feature.feature_type;
    if ty.constrained_count().is_none() {
        return Err(DofError::UnsupportedFeatureType { pair, feature_type: ty });
    }
    scan(Some(pair), "location", &feature.location)?;
    let a = match (ty.has_direction(), feature.direction) {
        (true, None) => return Err(DofError::MissingDirection { pair }),
        (false, Some(_)) => return Err(DofError::UnexpectedDirection { pair }),
        (false, None) => None,
        (true, Some(d)) => {
            scan(Some(pair), "direction", &d)?;
            if d.iter().all(|&x| x == 0.0) {
                return Err(DofError::ZeroDirection { pair });
            }
            Some(unit(d, "direction")?)
        }
    };
    Ok(Valid { ty, p: feature.location, a })
}

/// The table's basis with `p / scale` in place of `p`.
fn basis_of(v: &Valid, scale: f64, pair: usize) -> Result<Vec<Twist>, DofError> {
    let p = v.p.map(|x| x / scale);
    finite(&p, "scale")?;
    let z = [0.0; 3];
    let e = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let out = match (v.ty, v.a) {
        (FeatureType::Plane, Some(n)) => {
            let k = {
                let mut k = 0usize;
                for (j, x) in n.iter().enumerate().skip(1) {
                    if x.abs() < n[k].abs() {
                        k = j;
                    }
                }
                k
            };
            let ek = e.get(k).copied().unwrap_or(z);
            let t1 = unit(cross(n, ek), "plane_basis")?;
            let t2 = unit(cross(n, t1), "plane_basis")?;
            vec![twist(z, t1), twist(z, t2), twist(n, cross(p, n))]
        }
        (FeatureType::Line | FeatureType::Cylinder, Some(a)) => vec![twist(z, a), twist(a, cross(p, a))],
        (FeatureType::Cone | FeatureType::Circle3d, Some(a)) => vec![twist(a, cross(p, a))],
        (FeatureType::Sphere | FeatureType::Point, None) => e.iter().map(|&ei| twist(ei, cross(p, ei))).collect(),
        _ => return Err(DofError::DegenerateFeature { pair }),
    };
    for t in &out {
        finite(t, "basis")?;
    }
    Ok(out)
}

/// The invariance subspace of one nominal feature as a twist basis about the alignment origin,
/// with its constrained count (REQ-ALN.DATUM.DOF-001; the Design's `constrained_basis`). The
/// checks are those of [`accumulate`] with pair index 0.
pub fn constrained_basis(feature: &NominalFeature) -> Result<TwistBasis, DofError> {
    let v = validate(feature, 0)?;
    Ok(TwistBasis { vectors: basis_of(&v, 1.0, 0)? })
}

/// Singular values and right singular vectors (as columns) of a row-major `rows x cols`
/// matrix with `rows >= cols >= 1`; a matrix whose largest entry is subnormal is taken as zero
/// and that entry returned.
struct Spectrum {
    sigma: Vec<f64>,
    v: Vec<Vec<f64>>,
    u: Vec<Vec<f64>>,
    flushed: Option<f64>,
}

fn spectrum(rows: usize, cols: usize, mut data: Vec<f64>) -> Result<Spectrum, DofError> {
    let mut mx = 0.0_f64;
    for &x in &data {
        mx = f::total_max(mx, x.abs()).map_err(DofError::Numeric)?;
    }
    let flushed = if mx != 0.0 && mx < f64::MIN_POSITIVE {
        data.iter_mut().for_each(|x| *x = 0.0);
        Some(mx)
    } else {
        None
    };
    let s = svd(&Mat::new(rows, cols, data).map_err(DofError::Numeric)?, 0.0).map_err(DofError::Numeric)?;
    let column = |m: &Mat, j: usize| -> Result<Vec<f64>, DofError> {
        (0..m.rows()).map(|i| m.get(i, j).map_err(DofError::Numeric)).collect()
    };
    let mut v = buffer(cols)?;
    let mut u = buffer(cols)?;
    for j in 0..s.v().cols() {
        v.push(column(s.v(), j)?);
        u.push(column(s.u(), j)?);
    }
    Ok(Spectrum { sigma: s.sigma().to_vec(), v, u, flushed })
}

fn to_twist(xs: &[f64]) -> Twist {
    let mut t = [0.0; 6];
    for (o, x) in t.iter_mut().zip(xs) {
        *o = *x;
    }
    t
}

fn projector_of(q: &[Twist]) -> Result<[[f64; 6]; 6], DofError> {
    let mut rows: [Vec<f64>; 6] = Default::default();
    for (i, r) in rows.iter_mut().enumerate() {
        *r = buffer(q.len())?;
        r.extend(q.iter().map(|t| t[i]));
    }
    let mut p = [[0.0; 6]; 6];
    for (pr, ri) in p.iter_mut().zip(rows.iter()) {
        for (x, rj) in pr.iter_mut().zip(rows.iter()) {
            *x = dot(ri, rj)?;
        }
    }
    Ok(p)
}

/// Records the near-threshold singular values of one decision.
fn near(pair: Option<usize>, sigma: &[f64], warnings: &mut Vec<DofWarning>) {
    let lo = DOF_RANK_TOL / DOF_NEAR_FACTOR;
    let hi = DOF_RANK_TOL * DOF_NEAR_FACTOR;
    for &s in sigma {
        if s > lo && s <= hi {
            warnings.push(DofWarning::NearRankThreshold { pair, sigma: s });
        }
    }
}

/// Degree-of-freedom accounting of `pairs` in priority order under `policy`, by the rules of
/// the module docs (REQ-ALN.DATUM.DOF-002 to -006).
pub fn accumulate(pairs: &[NominalFeature], policy: UnconstrainedPolicy) -> Result<DofAccount, DofError> {
    if pairs.is_empty() {
        return Err(DofError::EmptyPairs);
    }
    if pairs.len() > MAX_DOF_PAIRS {
        return Err(DofError::InputLimit { count: pairs.len() });
    }
    let mut valid = buffer(pairs.len())?;
    for (i, p) in pairs.iter().enumerate() {
        valid.push(validate(p, i)?);
    }
    let mut radius = 0.0_f64;
    for v in &valid {
        radius = f::total_max(radius, norm(&v.p)?).map_err(DofError::Numeric)?;
    }
    let (radius, radius_basis) =
        if radius == 0.0 { (1.0, RadiusBasis::UnitFallback) } else { (radius, RadiusBasis::MaxLocation) };

    let mut q: Vec<Twist> = buffer(6)?;
    for i in 0..6 {
        let mut t = [0.0; 6];
        if let Some(x) = t.get_mut(i) {
            *x = 1.0;
        }
        q.push(t);
    }
    let mut per_pair = buffer(valid.len())?;
    let mut warnings = Vec::new();
    let mut decisions = 0usize;

    for (i, v) in valid.iter().enumerate() {
        let projector_before = projector_of(&q)?;
        let nominal =
            v.ty.constrained_count().ok_or(DofError::UnsupportedFeatureType { pair: i, feature_type: v.ty })?;
        // The pair's subspace.
        let b = basis_of(v, radius, i)?;
        let k = b.len();
        let mut data = buffer(6 * k)?;
        for r in 0..6 {
            data.extend(b.iter().map(|t| t[r]));
        }
        let sb = spectrum(6, k, data)?;
        decisions += k;
        let smax = sb.sigma.first().copied().unwrap_or(0.0);
        if sb.flushed.is_some() || sb.sigma.iter().filter(|&&s| s > DOF_RANK_TOL * smax).count() != k {
            return Err(DofError::DegenerateFeature { pair: i });
        }
        let u: Vec<Twist> = sb.u.iter().map(|c| to_twist(c)).collect();

        let f_before = q.len();
        let (effective, kept_min, dropped_max) = if f_before == 0 {
            (0, None, None)
        } else {
            // M = Q - U (U^T Q), row-major 6 x f.
            let mut cols = buffer(f_before)?;
            for col in &q {
                let mut c = buffer(k)?;
                for ul in &u {
                    c.push(dot(ul, col)?);
                }
                let inside = combine(&u, &c)?;
                let mut m = *col;
                for (x, y) in m.iter_mut().zip(inside.iter()) {
                    *x -= *y;
                }
                cols.push(m);
            }
            let mut data = buffer(6 * f_before)?;
            for r in 0..6 {
                data.extend(cols.iter().map(|t: &Twist| t[r]));
            }
            let s = spectrum(6, f_before, data)?;
            if let Some(max_entry) = s.flushed {
                warnings.push(DofWarning::BelowRoundingResolution { pair: Some(i), max_entry });
            }
            decisions += s.sigma.len();
            near(Some(i), &s.sigma, &mut warnings);
            let eff = s.sigma.iter().filter(|&&x| x > DOF_RANK_TOL).count();
            let kept_min = eff.checked_sub(1).and_then(|j| s.sigma.get(j)).copied();
            let dropped_max = s.sigma.get(eff).copied();
            let mut next = buffer(f_before - eff)?;
            for vj in s.v.iter().skip(eff) {
                next.push(combine(&q, vj)?);
            }
            q = next;
            (eff, kept_min, dropped_max)
        };
        if effective > nominal {
            return Err(DofError::RankInconsistent { pair: i });
        }
        if effective == 0 {
            warnings.push(DofWarning::RedundantPair { pair: i });
        }
        per_pair.push(PairAccount {
            feature_type: v.ty,
            nominal,
            effective,
            free_before: f_before,
            free_after: q.len(),
            projector_before,
            kept_min,
            dropped_max,
        });
    }

    let mut naming_fallbacks = 0usize;
    let free = name_free(&q, &valid, radius, &mut warnings, &mut decisions, &mut naming_fallbacks)?;
    let directive = if free.is_empty() {
        FreeDirective::FullyConstrained
    } else {
        match policy {
            UnconstrainedPolicy::KeepParent => FreeDirective::HoldAtParent,
            UnconstrainedPolicy::Reject => return Err(DofError::Underconstrained { free }),
        }
    };
    Ok(DofAccount {
        radius,
        radius_basis,
        per_pair,
        free,
        directive,
        warnings,
        rank_decisions: decisions,
        naming_fallbacks,
    })
}

/// The canonical orthonormal basis of the part spanned by `basis` (orthonormal), from the
/// candidate unit vectors `cands`, with acceptance bound `tau`.
fn canonical(basis: &[Twist], cands: [usize; 3], tau: f64, fallbacks: &mut usize) -> Result<Vec<Twist>, DofError> {
    let d = basis.len();
    let mut out: Vec<Twist> = buffer(d)?;
    let mut used = [false; 3];
    let residual = |c: usize, acc: &[Twist]| -> Result<(Twist, f64), DofError> {
        let coeff: Vec<f64> = basis.iter().map(|b| b[c]).collect();
        let mut r = combine(basis, &coeff)?;
        for _ in 0..2 {
            for a in acc {
                let g = dot(a, &r)?;
                for (x, y) in r.iter_mut().zip(a.iter()) {
                    *x -= g * *y;
                }
            }
        }
        let n = norm(&r)?;
        Ok((r, n))
    };
    for (slot, &c) in used.iter_mut().zip(cands.iter()) {
        if out.len() == d {
            break;
        }
        let (r, n) = residual(c, &out)?;
        if n > tau {
            out.push(r.map(|x| x / n));
            *slot = true;
        }
    }
    while out.len() < d {
        let mut best: Option<(usize, Twist, f64)> = None;
        for (k, &c) in cands.iter().enumerate() {
            if used.get(k).copied().unwrap_or(true) {
                continue;
            }
            let (r, n) = residual(c, &out)?;
            if best.as_ref().map_or(true, |b| n > b.2) {
                best = Some((k, r, n));
            }
        }
        match best {
            Some((k, r, n)) if n > 0.0 => {
                out.push(r.map(|x| x / n));
                if let Some(slot) = used.get_mut(k) {
                    *slot = true;
                }
                *fallbacks += 1;
            }
            _ => return Err(DofError::NamingFailed),
        }
    }
    for t in &out {
        finite(t, "canonical")?;
    }
    Ok(out)
}

fn fmt9(x: f64) -> String {
    let s = format!("{x:.9}");
    if s == "-0.000000000" {
        "0.000000000".to_string()
    } else {
        s
    }
}

fn label(d: [f64; 3]) -> String {
    for (k, axis) in ["x", "y", "z"].iter().enumerate() {
        let on = d.get(k).map_or(false, |x| x.abs() >= 1.0 - DOF_NAME_TOL);
        let off = d.iter().enumerate().all(|(j, x)| j == k || x.abs() <= DOF_NAME_TOL);
        if on && off {
            let sign = if d.get(k).map_or(false, |&x| x > 0.0) { "+" } else { "-" };
            return format!("{sign}{axis}");
        }
    }
    format!("({}, {}, {})", fmt9(d[0]), fmt9(d[1]), fmt9(d[2]))
}

/// Negates `t` when the largest-magnitude component of `part` is negative.
fn sign_fix(t: Twist, part: [f64; 3]) -> Twist {
    let k = largest(&part);
    if part.get(k).map_or(false, |&x| x < 0.0) {
        t.map(|x| -x)
    } else {
        t
    }
}

fn through(a: [f64; 3], q: [f64; 3], radius: f64, valid: &[Valid]) -> Result<Through, DofError> {
    let lim = DOF_NAME_TOL * radius;
    for (pair, v) in valid.iter().enumerate() {
        if let (true, Some(ai)) = (v.ty.is_axial(), v.a) {
            if norm(&cross(a, ai))? <= DOF_NAME_TOL && norm(&cross(sub(v.p, q), a))? <= lim {
                return Ok(Through::Axis { pair, feature_type: v.ty });
            }
        }
    }
    for (pair, v) in valid.iter().enumerate() {
        if v.ty.has_centre() && norm(&cross(sub(v.p, q), a))? <= lim {
            return Ok(Through::Centre { pair, feature_type: v.ty });
        }
    }
    if norm(&q)? <= lim {
        return Ok(Through::Origin);
    }
    Ok(Through::Point)
}

fn name_free(
    q: &[Twist],
    valid: &[Valid],
    radius: f64,
    warnings: &mut Vec<DofWarning>,
    decisions: &mut usize,
    fallbacks: &mut usize,
) -> Result<Vec<FreeDof>, DofError> {
    let fdim = q.len();
    if fdim == 0 {
        return Ok(Vec::new());
    }
    // Translation split: the angular block, padded to at least f rows.
    let rows = if fdim > 3 { fdim } else { 3 };
    let mut data = buffer(rows * fdim)?;
    for r in 0..rows {
        data.extend(q.iter().map(|t| if r < 3 { t[r] } else { 0.0 }));
    }
    let s = spectrum(rows, fdim, data)?;
    if let Some(max_entry) = s.flushed {
        warnings.push(DofWarning::BelowRoundingResolution { pair: None, max_entry });
    }
    *decisions += s.sigma.len();
    near(None, &s.sigma, warnings);
    let kept = s.sigma.iter().filter(|&&x| x > DOF_RANK_TOL).count();
    let mut rot = buffer(kept)?;
    let mut tra = buffer(fdim - kept)?;
    for (j, vj) in s.v.iter().enumerate() {
        let t = combine(q, vj)?;
        if j < kept {
            rot.push(t);
        } else {
            tra.push(t);
        }
    }
    let smin = kept.checked_sub(1).and_then(|j| s.sigma.get(j)).copied().unwrap_or(0.0);

    let mut out = buffer(fdim)?;
    for t in canonical(&tra, [3, 4, 5], CANON_TAU, fallbacks)? {
        let t = sign_fix(t, linear(&t));
        let direction = unit(linear(&t), "translation")?;
        out.push(FreeDof {
            motion: Motion::Translation,
            scaled: t,
            twist: unscale(&t, radius)?,
            direction,
            point: None,
            pitch: None,
            through: None,
            name: format!("translation along {}", label(direction)),
        });
    }
    for t in canonical(&rot, [0, 1, 2], CANON_TAU * smin, fallbacks)? {
        let t = sign_fix(t, angular(&t));
        let (w, v) = (angular(&t), linear(&t));
        let (nw, nv) = (norm(&w)?, norm(&v)?);
        let nw2 = dot(&w, &w)?;
        if nw == 0.0 || nw2 == 0.0 {
            return Err(DofError::NamingFailed);
        }
        let h = dot(&w, &v)?;
        let direction = w.map(|x| x / nw);
        let point = cross(w, v).map(|x| (x / nw2) * radius);
        let pitch = (h / nw2) * radius;
        finite(&direction, "rotation")?;
        finite(&point, "rotation")?;
        if !pitch.is_finite() {
            return Err(DofError::NonFinite { op: "rotation" });
        }
        let (motion, thr, name) = if h.abs() <= DOF_NAME_TOL * nw * nv {
            let thr = through(direction, point, radius, valid)?;
            let at = match thr {
                Through::Axis { pair, feature_type } => format!("the axis of pair {pair} ({feature_type})"),
                Through::Centre { pair, feature_type } => format!("the centre of pair {pair} ({feature_type})"),
                Through::Origin => "the alignment origin".to_string(),
                Through::Point => format!("({}, {}, {}) m", fmt9(point[0]), fmt9(point[1]), fmt9(point[2])),
            };
            (Motion::Rotation, Some(thr), format!("rotation about {} through {at}", label(direction)))
        } else {
            (Motion::Twist, None, "generic twist".to_string())
        };
        out.push(FreeDof {
            motion,
            scaled: t,
            twist: unscale(&t, radius)?,
            direction,
            point: Some(point),
            pitch: Some(pitch),
            through: thr,
            name,
        });
    }
    if out.len() != fdim {
        return Err(DofError::NamingFailed);
    }
    Ok(out)
}

fn unscale(t: &Twist, radius: f64) -> Result<Twist, DofError> {
    let mut u = *t;
    for x in u.iter_mut().take(3) {
        *x /= radius;
    }
    finite(&u, "unscale")?;
    Ok(u)
}
