//! Degree-of-freedom locks and weights for best-fit (spec node `ALN.BFIT.CONSTR`).
//!
//! The constrained-best-fit building block the loops `ALN.BFIT.ICP` and `ALN.BFIT.FEAT` build
//! their Jacobians through. [`validate`] checks the locks, resolves the lock frame, combines
//! the weights and returns a [`ConstraintSet`] with both digests; [`linearise`] turns the free
//! parameters into the pose and its Jacobian; [`reduced_jacobian`] keeps the free columns;
//! [`verify_locks`] certifies that a pose honours the locks. Everything below is pinned so that
//! an independent reference (`fixtures/aln/bfit/make_constr_fixtures.py`) reproduces every bit.
//!
//! # Parameterisation (REQ-ALN.BFIT.CONSTR-001, -002)
//!
//! The six parameters, in the fixed order `tx, ty, tz, a, b, c` ([`Dof::ALL`]), describe a
//! rigid motion `D` of the lock frame relative to the start pose: `D(q) = R q + t` with
//! `t = (tx, ty, tz)` (metres) and `R = Rz(c) Ry(b) Rx(a)` (radians, the fixed `ZYX` sequence).
//! A locked parameter is not an unknown: it is the constant `+0.0`. With `L` the lock frame
//! (lock-frame coordinates to the parent alignment's frame) and `P0` the start pose (data
//! coordinates to the parent frame), the pose is the lock-frame conjugation
//!
//! ```text
//! P = L ∘ (D ∘ M)        with   M = L⁻¹ ∘ P0   (computed once by `validate`)
//! ```
//!
//! using `fid-geo-frame`'s pinned `compose` and `inverse` (`dot3(a, b) = (a0 b0 + a1 b1) + a2 b2`,
//! `C = A B` by `dot3` along rows and columns, `t = 1.0 * (Ra tb) + ta`, `L⁻¹ = (Lᵀ, -(1.0 * (Lᵀ t)))`).
//! `sin` and `cos` come from `fid_num::f`. The elementary matrices are written with their
//! zeros: `Rx = [[1,0,0],[0,ca,-sa],[0,sa,ca]]`, `Ry = [[cb,0,sb],[0,1,0],[-sb,0,cb]]`,
//! `Rz = [[cc,-sc,0],[sc,cc,0],[0,0,1]]`, and `R = Rz (Ry Rx)`.
//!
//! Locked components cannot drift in the parameters (they are never touched), and the pose is
//! certified, not assumed: every [`linearise`] and [`pose_from_params`] call evaluates the
//! components of the produced pose in the lock frame relative to the start pose (see
//! [`verify_locks`]) and refuses with `lock_drift` if a locked one differs from 0 by more than
//! [`LOCK_TOL`] (`1e-12` m or rad). Rounding in the conjugation is about `1e-16` times the
//! magnitudes of the frames' translations, so the refusal happens only for absurd frame sizes;
//! it is never relaxed.
//!
//! # Jacobian (REQ-ALN.BFIT.CONSTR-003)
//!
//! For a data point `x`, with `q = M(x)` (`apply_point`), `v1 = Rx q`, `v2 = Ry v1` and `RL` the
//! lock frame's rotation, the derivative of `P(x)` by each parameter is:
//!
//! - `tx, ty, tz`: column `k` of `RL` (copied, no arithmetic);
//! - `a`: `RL (Rz (Ry (dRx q)))`; `b`: `RL (Rz (dRy v1))`; `c`: `RL (dRz v2)`;
//!
//! with `dRx = [[0,0,0],[0,-sa,-ca],[0,ca,-sa]]`, `dRy = [[-sb,0,cb],[0,0,0],[-cb,0,-sb]]`,
//! `dRz = [[-sc,-cc,0],[cc,-sc,0],[0,0,0]]`, every product by `fid-geo-frame`'s `mat_vec`.
//! [`Linearisation::jacobian_rows`] returns the three rows (one per coordinate) with all six
//! columns; [`reduced_jacobian`] keeps the free columns in the order `tx, ty, tz, a, b, c`.
//!
//! # Gimbal guard (REQ-ALN.BFIT.CONSTR-005)
//!
//! A free `b` with `|b| > B_MAX` (80 degrees, the binary64 nearest `4 pi / 9`) is refused with
//! `lock_parameterisation_singular` [unverified: margin to gimbal lock]; `|b| = B_MAX` is
//! accepted.
//!
//! # Weights (REQ-ALN.BFIT.CONSTR-006, -007)
//!
//! Regions arrive resolved ([`RegionWeight`]: strictly increasing point indices; selector
//! resolution is `ALN.BFIT.LOCAL` / `GEO.SEL`). An input weight must be `+0`, `-0` (read as
//! `+0`) or a finite, normal, positive binary64; anything else is `invalid_weight`. Check order:
//! no points (`all_zero`), the per-point length, the per-point weights in index order, then each
//! region in order (its weight, then its members: an index `>= n` or not above the previous one
//! is `invalid_region`). The combined weight starts at the per-point weight (or 1) and is
//! multiplied by each region's weight, regions in order, members in order (one IEEE
//! multiplication each). A product that is infinite, subnormal, or zero while both factors are
//! not, is refused with `invalid_weight` naming that point: a weight is never silently
//! saturated or flushed. If no combined weight is positive, `invalid_weight` (`all_zero`).
//! A combined weight of 0 excludes the point.
//!
//! # Digests (REQ-ALN.BFIT.CONSTR-008)
//!
//! - weights: SHA-256 of the combined weights as little-endian binary64 bytes in index order
//!   (no tag, exactly the leaf's rule);
//! - locks: SHA-256 of `"FIDLCK" | 0x01 | mask u8 | "ZYX" | lock frame ID (16 bytes) | the lock
//!   frame's 9 rotation entries (row-major) and 3 translation components as little-endian
//!   binary64`, `-0.0` written as `+0.0`. The default lock frame records the parent's ID and
//!   its identity transform.
//!
//! Both use `fid-kci`'s portable SHA-256 and the shared `sha256:` [`Digest`] (D0).
//!
//! # Fail closed
//!
//! Nothing here panics; NaN and infinities are errors; buffers are reserved fallibly. A loop
//! that returns an alignment certifies its final pose with [`verify_locks`] before it hands the
//! result out (through `fid_fit::Fit` or its own result type), so a pose that does not honour
//! its locks is refused, never returned.

mod pose;
mod weights;

use fid_geo_frame::{FrameError, FrameId, FrameTree, Rigid, Transform, XformError};
use fid_kci::kci::Sha256;
use fid_types::Digest;
use std::fmt;

pub use pose::{
    linearise, pose_from_params, reduced_jacobian, verify_locks, Jacobian, Linearisation, MAX_JACOBIAN_ROWS,
};
pub use weights::{RegionWeight, WeightInput, WeightSpec, MAX_POINTS, MAX_REGIONS};

/// Largest accepted drift of a locked component, metres or radians (REQ-ALN.BFIT.CONSTR-002).
pub const LOCK_TOL: f64 = 1e-12;

/// Largest accepted `|b|` of a free `b`: 80 degrees, the binary64 nearest `4 pi / 9`
/// (bits `0x3FF6_5718_4AE7_4487`) (REQ-ALN.BFIT.CONSTR-005).
pub const B_MAX: f64 = 1.3962634015954636;

/// The Euler sequence, recorded in the locks digest.
pub const EULER_SEQ: &str = "ZYX";

/// The ASCII tag the locks digest preimage starts with.
pub const LOCKS_TAG: [u8; 6] = *b"FIDLCK";
/// The layout version of the locks digest preimage.
pub const LOCKS_VERSION: u8 = 1;

/// One of the six pose parameters, in their fixed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dof {
    /// Translation along x of the lock frame (metres).
    Tx,
    /// Translation along y (metres).
    Ty,
    /// Translation along z (metres).
    Tz,
    /// Rotation `a` about x (radians).
    Rx,
    /// Rotation `b` about y (radians).
    Ry,
    /// Rotation `c` about z (radians).
    Rz,
}

impl Dof {
    /// The fixed order `tx, ty, tz, a, b, c`.
    pub const ALL: [Dof; 6] = [Dof::Tx, Dof::Ty, Dof::Tz, Dof::Rx, Dof::Ry, Dof::Rz];

    /// The position in [`Dof::ALL`] (and the bit in [`DofLocks`]).
    pub const fn index(self) -> usize {
        match self {
            Dof::Tx => 0,
            Dof::Ty => 1,
            Dof::Tz => 2,
            Dof::Rx => 3,
            Dof::Ry => 4,
            Dof::Rz => 5,
        }
    }

    /// Stable name: `tx`, `ty`, `tz`, `rx`, `ry`, `rz`.
    pub const fn name(self) -> &'static str {
        match self {
            Dof::Tx => "tx",
            Dof::Ty => "ty",
            Dof::Tz => "tz",
            Dof::Rx => "rx",
            Dof::Ry => "ry",
            Dof::Rz => "rz",
        }
    }
}

/// The locked subset of the six parameters: bit `k` locks `Dof::ALL[k]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct DofLocks(u8);

impl DofLocks {
    /// Nothing locked.
    pub const NONE: DofLocks = DofLocks(0);
    /// All six locked (refused by [`validate`]).
    pub const ALL: DofLocks = DofLocks(0x3f);

    /// From a bitmask; `invalid_locks` if bit 6 or 7 is set.
    pub fn from_bits(bits: u8) -> Result<DofLocks, ConstrError> {
        if bits & 0xc0 != 0 {
            return Err(ConstrError::InvalidLocks { bits });
        }
        Ok(DofLocks(bits))
    }

    /// The bitmask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// These locks plus `d`.
    pub const fn with(self, d: Dof) -> DofLocks {
        DofLocks(self.0 | (1 << d.index()))
    }

    /// True when `d` is locked.
    pub const fn is_locked(self, d: Dof) -> bool {
        self.0 & (1 << d.index()) != 0
    }

    /// The number of free parameters.
    pub const fn n_free(self) -> usize {
        6 - self.0.count_ones() as usize
    }

    /// The free parameters in the fixed order.
    pub fn free(self) -> impl Iterator<Item = Dof> {
        Dof::ALL.into_iter().filter(move |d| !self.is_locked(*d))
    }
}

/// Which frame the locks are expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockFrameRef<'a> {
    /// The parent alignment's frame (the default).
    Parent,
    /// A frame of the tree by ID.
    Id(FrameId),
    /// A frame of the tree by name (ASCII case-insensitive, as the tree's names are unique).
    Name(&'a str),
}

/// The frame tree and the parent alignment's frame the pose maps into.
#[derive(Debug, Clone, Copy)]
pub struct FrameContext<'a> {
    /// The document's frame tree.
    pub tree: &'a FrameTree,
    /// The parent alignment's frame: the start pose maps data coordinates into it.
    pub parent: FrameId,
}

/// Every way this module refuses. Codes are stable.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstrError {
    /// A lock bitmask with bit 6 or 7 set.
    InvalidLocks { bits: u8 },
    /// All six parameters locked (REQ-ALN.BFIT.CONSTR-004).
    AllDofLocked,
    /// The lock frame or the parent frame is not in the tree (REQ-ALN.BFIT.CONSTR-004);
    /// `role` is `lock` or `parent`.
    FrameNotFound { role: &'static str },
    /// The lock frame's transform to the parent holds a scale: locks are rigid-only.
    LockFrameNotRigid,
    /// A weight input is refused (REQ-ALN.BFIT.CONSTR-007); names the first offender.
    InvalidWeight(WeightInput),
    /// A region's members are out of range or not strictly increasing.
    InvalidRegion { region: usize },
    /// The free-parameter slice does not have one value per free parameter.
    ParameterCount { expected: usize, got: usize },
    /// A free parameter is NaN or infinite; `index` is its position in the free slice.
    NonFiniteParameter { index: usize },
    /// A free `b` with `|b| > B_MAX` (REQ-ALN.BFIT.CONSTR-005).
    LockParameterisationSingular { b: f64 },
    /// A pose whose locked component, in the lock frame relative to the start pose, differs
    /// from 0 by more than [`LOCK_TOL`] (REQ-ALN.BFIT.CONSTR-002).
    LockDrift { dof: Dof, value: f64 },
    /// A transform, point or derivative overflowed; `op` names the step.
    NonFinite { op: &'static str },
    /// A Jacobian input row holds NaN or an infinity.
    NonFiniteRow { row: usize },
    /// A size budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// A buffer could not be reserved.
    AllocationFailed,
    /// A fault that cannot be the caller's mistake.
    Internal(&'static str),
}

impl ConstrError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ConstrError::InvalidLocks { .. } => "invalid_locks",
            ConstrError::AllDofLocked => "all_dof_locked",
            ConstrError::FrameNotFound { .. } => "frame_not_found",
            ConstrError::LockFrameNotRigid => "lock_frame_not_rigid",
            ConstrError::InvalidWeight(_) => "invalid_weight",
            ConstrError::InvalidRegion { .. } => "invalid_region",
            ConstrError::ParameterCount { .. } => "parameter_count",
            ConstrError::NonFiniteParameter { .. } => "non_finite_parameter",
            ConstrError::LockParameterisationSingular { .. } => "lock_parameterisation_singular",
            ConstrError::LockDrift { .. } => "lock_drift",
            ConstrError::NonFinite { .. } => "non_finite",
            ConstrError::NonFiniteRow { .. } => "non_finite_row",
            ConstrError::InputLimit(_) => "input_limit",
            ConstrError::AllocationFailed => "allocation_failed",
            ConstrError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for ConstrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            ConstrError::InvalidLocks { bits } => write!(f, " (bits 0x{bits:02x})"),
            ConstrError::FrameNotFound { role } => write!(f, " ({role})"),
            ConstrError::InvalidWeight(w) => write!(f, " ({w})"),
            ConstrError::InvalidRegion { region } => write!(f, " (region {region})"),
            ConstrError::ParameterCount { expected, got } => write!(f, " (expected {expected}, got {got})"),
            ConstrError::NonFiniteParameter { index } => write!(f, " (free parameter {index})"),
            ConstrError::LockParameterisationSingular { b } => write!(f, " (b = {b:e} rad)"),
            ConstrError::LockDrift { dof, value } => write!(f, " ({} = {value:e})", dof.name()),
            ConstrError::NonFinite { op } => write!(f, " ({op})"),
            ConstrError::NonFiniteRow { row } => write!(f, " (row {row})"),
            ConstrError::InputLimit(s) | ConstrError::Internal(s) => write!(f, " ({s})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for ConstrError {}

pub(crate) fn xform_err(op: &'static str) -> impl Fn(XformError) -> ConstrError {
    move |_| ConstrError::NonFinite { op }
}

pub(crate) fn rigid_of(t: Transform, op: &'static str) -> Result<Rigid, ConstrError> {
    match t {
        Transform::Rigid(r) => Ok(r),
        Transform::Similarity(_) => Err(ConstrError::Internal(op)),
    }
}

/// The validated constraint set of one best-fit run.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintSet {
    locks: DofLocks,
    lock_frame_id: FrameId,
    lock_frame: Rigid,
    lock_inv: Rigid,
    start: Rigid,
    start_inv: Rigid,
    m: Rigid,
    weights: Vec<f64>,
    n_used: usize,
    locks_digest: Digest,
    weights_digest: Digest,
}

impl ConstraintSet {
    /// The locks.
    pub fn locks(&self) -> DofLocks {
        self.locks
    }

    /// The number of free parameters (1 to 6).
    pub fn n_free(&self) -> usize {
        self.locks.n_free()
    }

    /// The lock frame's ID (the parent's for the default frame).
    pub fn lock_frame_id(&self) -> FrameId {
        self.lock_frame_id
    }

    /// The lock frame: lock-frame coordinates to the parent frame.
    pub fn lock_frame(&self) -> Rigid {
        self.lock_frame
    }

    /// The start pose: data coordinates to the parent frame.
    pub fn start(&self) -> Rigid {
        self.start
    }

    /// The number of points the weights cover.
    pub fn n_points(&self) -> usize {
        self.weights.len()
    }

    /// The number of points with a positive combined weight.
    pub fn n_used(&self) -> usize {
        self.n_used
    }

    /// The combined weights, one per point (REQ-ALN.BFIT.CONSTR-006).
    pub fn combined_weights(&self) -> &[f64] {
        &self.weights
    }

    /// SHA-256 of the canonical locks (REQ-ALN.BFIT.CONSTR-008).
    pub fn locks_digest(&self) -> Digest {
        self.locks_digest
    }

    /// SHA-256 of the combined weights (REQ-ALN.BFIT.CONSTR-008).
    pub fn weights_digest(&self) -> Digest {
        self.weights_digest
    }
}

/// The combined weights of `cs` (REQ-ALN.BFIT.CONSTR-006); `cs.combined_weights()`.
pub fn combined_weights(cs: &ConstraintSet) -> &[f64] {
    cs.combined_weights()
}

fn resolve_frame(frame: LockFrameRef<'_>, ctx: &FrameContext<'_>) -> Result<(FrameId, Rigid), ConstrError> {
    if ctx.tree.get(ctx.parent).is_none() {
        return Err(ConstrError::FrameNotFound { role: "parent" });
    }
    let id = match frame {
        LockFrameRef::Parent => return Ok((ctx.parent, Rigid::IDENTITY)),
        LockFrameRef::Id(id) => id,
        LockFrameRef::Name(name) => ctx.tree.find_by_name(name).ok_or(ConstrError::FrameNotFound { role: "lock" })?,
    };
    if ctx.tree.get(id).is_none() {
        return Err(ConstrError::FrameNotFound { role: "lock" });
    }
    match ctx.tree.resolve(id, ctx.parent) {
        Ok(Transform::Rigid(r)) => Ok((id, r)),
        Ok(Transform::Similarity(_)) => Err(ConstrError::LockFrameNotRigid),
        Err(FrameError::UnknownFrame { .. }) => Err(ConstrError::FrameNotFound { role: "lock" }),
        Err(FrameError::Xform(_)) => Err(ConstrError::NonFinite { op: "lock_frame" }),
        Err(_) => Err(ConstrError::Internal("frame tree")),
    }
}

fn pos0(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

fn locks_digest(locks: DofLocks, id: FrameId, l: &Rigid) -> Digest {
    let mut h = Sha256::new();
    h.update(&LOCKS_TAG);
    h.update(&[LOCKS_VERSION, locks.bits()]);
    h.update(EULER_SEQ.as_bytes());
    h.update(&id.to_bytes());
    let r = l.r();
    for row in &r {
        for x in row {
            h.update(&pos0(*x).to_le_bytes());
        }
    }
    for x in l.t() {
        h.update(&pos0(x).to_le_bytes());
    }
    Digest::from_bytes(h.finish())
}

/// Validates the locks, resolves the lock frame and combines the weights.
///
/// Check order: `all_dof_locked`, then `frame_not_found` (parent, then lock frame),
/// `lock_frame_not_rigid`, then the weights in the order of the module docs.
pub fn validate(
    locks: DofLocks,
    frame: LockFrameRef<'_>,
    ctx: &FrameContext<'_>,
    start: &Rigid,
    weights: &WeightSpec,
    n_points: usize,
) -> Result<ConstraintSet, ConstrError> {
    if locks.n_free() == 0 {
        return Err(ConstrError::AllDofLocked);
    }
    let (lock_frame_id, lock_frame) = resolve_frame(frame, ctx)?;
    let lt = Transform::Rigid(lock_frame);
    let st = Transform::Rigid(*start);
    let lock_inv = rigid_of(lt.inverse().map_err(xform_err("lock_frame"))?, "lock_frame")?;
    let start_inv = rigid_of(st.inverse().map_err(xform_err("start"))?, "start")?;
    let m = rigid_of(Transform::Rigid(lock_inv).compose(&st).map_err(xform_err("start"))?, "start")?;
    let (combined, n_used) = weights::combine(weights, n_points)?;
    let weights_digest = weights::digest(&combined);
    Ok(ConstraintSet {
        locks,
        lock_frame_id,
        lock_frame,
        lock_inv,
        start: *start,
        start_inv,
        m,
        weights: combined,
        n_used,
        locks_digest: locks_digest(locks, lock_frame_id, &lock_frame),
        weights_digest,
    })
}
