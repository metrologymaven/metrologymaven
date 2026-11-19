//! DRF construction, precedence and DoF bookkeeping. Spec node `GDT.DRF.BUILD` (P1a: planar
//! and cylindrical datum features at RMB).
//!
//! [`build_drf`] takes one to three [`SimulatedDatum`] values in the precedence order of the
//! feature control frame (primary, secondary, tertiary). Each one is a simulated plane (a point
//! and a unit normal) or a simulated axis (a point and a unit direction), together with the
//! simulator algorithm ID and the DMT row that named it; producing them is `GDT.DRF.SIM`'s job,
//! so this module is pure geometry and bookkeeping. It returns a [`Drf`]: the origin, the three
//! orthonormal axes, the DoFs each datum constrains (a [`DofSet`] over `{tx, ty, tz, rx, ry, rz}`
//! in the DRF's own axes, rotations about axes through the origin), the DoFs left free, and the
//! simulator and DMT row of every datum (REQ-001, REQ-002, REQ-005).
//!
//! # The rule
//!
//! A datum constrains exactly those still-free DoFs that would move it. A plane is unchanged by
//! translations within it and by rotation about its normal; an axis is unchanged by translation
//! along it and rotation about it (the invariance classes of ISO 17450-1, planar and
//! cylindrical/straight-line [unverified clause numbers]). Every datum must constrain at least
//! one DoF its predecessors left free, otherwise the DRF is refused with
//! [`DrfError::DegenerateDatumPair`] (REQ-003, REQ-004). The resulting table, with `z` the
//! primary direction:
//!
//! | precedence | datum | constrains |
//! |---|---|---|
//! | primary | plane | `{tz, rx, ry}` |
//! | primary | axis | `{tx, ty, rx, ry}` |
//! | after a plane | plane not parallel to it | `{ty, rz}` |
//! | after a plane | axis parallel to `z` (perpendicular to the plane) | `{tx, ty}` |
//! | after a plane | axis perpendicular to `z` (lying along the plane) | `{ty, rz}` |
//! | after a plane | axis at any other angle | `{tx, ty, rz}` |
//! | after an axis | plane perpendicular to it | `{tz}` |
//! | after an axis | plane parallel to it | `{rz}` |
//! | after an axis | plane at any other angle | `{tz, rz}` |
//! | after an axis | parallel axis offset from it | `{rz}` |
//! | after an axis | non-parallel axis | `{tz, rz}` |
//! | tertiary | any | whichever of the one or two remaining free DoFs moves it |
//!
//! Concretely, for a datum with unit direction `u` (normal or axis direction):
//!
//! - **`rz`** (only ever free before the datum is applied when the primary leaves it): the datum
//!   constrains it when `|u - (u.z) z| >= ANGLE_TOL` (its direction is not parallel to `z`), or,
//!   for an axis parallel to `z` while `tx` and `ty` are already constrained, when that axis is
//!   at least [`COAXIAL_TOL`] from the line through the origin along `z`. An axis parallel to `z`
//!   while `tx`, `ty` are still free constrains those translations instead, and a plane whose
//!   normal is parallel to `z` never constrains `rz`.
//! - **`tx`, `ty`, `tz`** (each translation `e` still free): a plane constrains it when
//!   `|u.e| >= ANGLE_TOL`; an axis when `|e - (e.u) u| >= ANGLE_TOL` (it does not run along `e`).
//! - `rx`, `ry` are always constrained by the primary.
//!
//! "Parallel" and "perpendicular" are decided only by these pinned tolerances: [`ANGLE_TOL`]
//! (1e-9, the sine or cosine of the angle, so radians to first order) and [`COAXIAL_TOL`]
//! (1e-9 m). A direction within the tolerance counts as exactly parallel or perpendicular; any
//! other angle is oblique and handled by the same rule.
//!
//! # Axes (pinned order)
//!
//! `z` is the primary direction normalised. The first datum that constrains `rz` sets the
//! clocking, orthonormalised once in this fixed order: for a plane, `y = w / |w|` with
//! `w = n - (n.z) z`, then `x = y x z`; for an axis not parallel to `z`, `x = w / |w|` with
//! `w = d - (d.z) z`, then `y = z x x`; for an axis parallel to `z`, `x` is the unit vector from
//! the origin to that axis, perpendicular to `z`, then `y = z x x`. When no datum constrains
//! `rz`, the clocking is free and pinned: `x` is the world axis with the smallest `|z_k|`
//! (lowest `k` on a tie) projected perpendicular to `z` and normalised, `y = z x x`. Every norm
//! is `fid_num::f::sqrt` of a dot product summed in index order; nothing uses FMA.
//!
//! # Origin
//!
//! The primary's point. Each later datum moves the origin along only the translations it
//! constrains (so the move has minimum length): onto a plane, or to the point of that line or
//! plane of translations closest to an axis (a least-squares solve of at most two unknowns,
//! Cramer's rule). The origin therefore lies on every plane and, where they meet, on every axis:
//! for three mutually perpendicular planes it is their intersection point.
//!
//! # Degenerate pairs
//!
//! [`DrfError::DegenerateDatumPair`] names the datum that constrains nothing and, as its partner,
//! the highest-precedence earlier datum of the same type (plane or axis) whose direction is
//! parallel to it within [`ANGLE_TOL`]; when there is none, the datum immediately before it.
//!
//! # Determinism
//!
//! D0: closed form, a fixed operation order, `fid-num` for the square root, no `f64::min` or
//! `f64::max`, and any NaN or infinite input refused. (`fid-gps` is a model-layer crate, so
//! KNP-1 is followed here voluntarily rather than enforced by the kernel lint.)

use crate::dmt::{AlgId, DmtKey};
use std::fmt;

/// Angular tolerance (sine or cosine of the angle) for parallel and perpendicular decisions.
pub const ANGLE_TOL: f64 = 1e-9;
/// Distance (m) below which two parallel axes count as coaxial.
pub const COAXIAL_TOL: f64 = 1e-9;
/// A datum direction must have norm `1 +- UNIT_TOL`; it is then normalised.
pub const UNIT_TOL: f64 = 1e-9;
/// At most a primary, a secondary and a tertiary datum.
pub const MAX_DATUMS: usize = 3;

/// One rigid-body degree of freedom in the DRF's own axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dof {
    /// Translation along `x`.
    Tx,
    /// Translation along `y`.
    Ty,
    /// Translation along `z`.
    Tz,
    /// Rotation about `x`.
    Rx,
    /// Rotation about `y`.
    Ry,
    /// Rotation about `z`.
    Rz,
}

impl Dof {
    /// All six, in canonical order.
    pub const ALL: [Dof; 6] = [Dof::Tx, Dof::Ty, Dof::Tz, Dof::Rx, Dof::Ry, Dof::Rz];

    /// The wire name (`tx` .. `rz`).
    pub fn name(self) -> &'static str {
        match self {
            Dof::Tx => "tx",
            Dof::Ty => "ty",
            Dof::Tz => "tz",
            Dof::Rx => "rx",
            Dof::Ry => "ry",
            Dof::Rz => "rz",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<Dof> {
        Dof::ALL.into_iter().find(|d| d.name() == s)
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// A set of DoFs as six bits (bit `i` is `Dof::ALL[i]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DofSet(u8);

impl DofSet {
    /// No DoF.
    pub const EMPTY: DofSet = DofSet(0);
    /// All six DoFs.
    pub const ALL: DofSet = DofSet(0b11_1111);

    /// The set holding exactly `dofs`.
    pub fn of(dofs: &[Dof]) -> DofSet {
        DofSet(dofs.iter().fold(0, |b, d| b | d.bit()))
    }

    /// The set from its six bits; `None` if a higher bit is set.
    pub fn from_bits(bits: u8) -> Option<DofSet> {
        ((bits & !DofSet::ALL.0) == 0).then_some(DofSet(bits))
    }

    /// The six bits.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// Whether `d` is in the set.
    pub fn contains(self, d: Dof) -> bool {
        self.0 & d.bit() != 0
    }

    /// The set with `d` added.
    pub fn with(self, d: Dof) -> DofSet {
        DofSet(self.0 | d.bit())
    }

    /// The members of `self` not in `other`.
    pub fn minus(self, other: DofSet) -> DofSet {
        DofSet(self.0 & !other.0)
    }

    /// The members of either set.
    pub fn union(self, other: DofSet) -> DofSet {
        DofSet(self.0 | other.0)
    }

    /// The members of both sets.
    pub fn intersection(self, other: DofSet) -> DofSet {
        DofSet(self.0 & other.0)
    }

    /// Whether the set is empty.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The number of members.
    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// The members in canonical order.
    pub fn dofs(self) -> Vec<Dof> {
        Dof::ALL.into_iter().filter(|d| self.contains(*d)).collect()
    }
}

impl fmt::Display for DofSet {
    /// `{tz, rx, ry}`; `{}` for the empty set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.dofs().into_iter().map(Dof::name).collect();
        write!(f, "{{{}}}", names.join(", "))
    }
}

/// The geometry of a simulated datum (from `GDT.DRF.SIM`), in metres in the part frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimGeometry {
    /// A simulated datum plane: a point on it and its unit normal.
    Plane {
        /// A point on the plane.
        point: [f64; 3],
        /// Unit normal (norm `1 +- UNIT_TOL`).
        normal: [f64; 3],
    },
    /// A simulated datum axis: a point on it and its unit direction.
    Axis {
        /// A point on the axis.
        point: [f64; 3],
        /// Unit direction (norm `1 +- UNIT_TOL`).
        direction: [f64; 3],
    },
}

/// Plane or axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DatumKind {
    /// A simulated plane.
    Plane,
    /// A simulated axis.
    Axis,
}

impl DatumKind {
    /// `plane` or `axis`.
    pub fn name(self) -> &'static str {
        match self {
            DatumKind::Plane => "plane",
            DatumKind::Axis => "axis",
        }
    }
}

impl SimGeometry {
    /// Plane or axis.
    pub fn kind(&self) -> DatumKind {
        match self {
            SimGeometry::Plane { .. } => DatumKind::Plane,
            SimGeometry::Axis { .. } => DatumKind::Axis,
        }
    }

    fn parts(&self) -> ([f64; 3], [f64; 3]) {
        match *self {
            SimGeometry::Plane { point, normal } => (point, normal),
            SimGeometry::Axis { point, direction } => (point, direction),
        }
    }
}

/// The narrow interface from `GDT.DRF.SIM`: one simulated datum with the simulator algorithm ID
/// and the DMT row that named the simulator.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedDatum {
    /// The datum label from the feature control frame (`A`, `B`, ...); non-empty.
    pub label: String,
    /// The simulated plane or axis.
    pub geometry: SimGeometry,
    /// The simulator algorithm ID.
    pub alg: AlgId,
    /// The DMT row that named the simulator.
    pub dmt_row: DmtKey,
}

/// Precedence in the feature control frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Precedence {
    /// First datum.
    Primary,
    /// Second datum.
    Secondary,
    /// Third datum.
    Tertiary,
}

/// One datum's row of the DRF's DoF table.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumDof {
    /// The datum label.
    pub label: String,
    /// Its precedence.
    pub precedence: Precedence,
    /// Plane or axis.
    pub kind: DatumKind,
    /// The DoFs this datum constrains (none of them constrained by an earlier datum).
    pub constrained: DofSet,
    /// The simulator algorithm ID.
    pub alg: AlgId,
    /// The DMT row that named the simulator.
    pub dmt_row: DmtKey,
}

/// A datum reference frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Drf {
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
    datums: Vec<DatumDof>,
    free: DofSet,
}

impl Drf {
    /// The origin in the part frame (m).
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// The axes `[x, y, z]`: orthonormal and right-handed, in the part frame.
    pub fn axes(&self) -> [[f64; 3]; 3] {
        self.axes
    }

    /// The DoF table, in precedence order.
    pub fn datums(&self) -> &[DatumDof] {
        &self.datums
    }

    /// The DoFs no datum constrains.
    pub fn free(&self) -> DofSet {
        self.free
    }
}

/// Why a DRF was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrfError {
    /// No datum given.
    NoDatums,
    /// More than [`MAX_DATUMS`] datums given.
    TooManyDatums {
        /// How many were given.
        count: usize,
    },
    /// A datum label is empty.
    EmptyLabel,
    /// The same label appears twice.
    DuplicateLabel {
        /// The repeated label.
        label: String,
    },
    /// A point or direction holds a NaN or an infinity, or a computed value overflowed.
    NotFinite {
        /// The datum.
        label: String,
    },
    /// A direction is not unit within [`UNIT_TOL`].
    NotUnit {
        /// The datum.
        label: String,
    },
    /// `second` constrains no DoF left free by the datums before it; `first` is its partner
    /// (see the module documentation).
    DegenerateDatumPair {
        /// The earlier datum named in the pair.
        first: String,
        /// The datum that constrains nothing new.
        second: String,
    },
}

impl DrfError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            DrfError::NoDatums => "no_datums",
            DrfError::TooManyDatums { .. } => "too_many_datums",
            DrfError::EmptyLabel => "empty_datum_label",
            DrfError::DuplicateLabel { .. } => "duplicate_datum_label",
            DrfError::NotFinite { .. } => "not_finite",
            DrfError::NotUnit { .. } => "direction_not_unit",
            DrfError::DegenerateDatumPair { .. } => "degenerate_datum_pair",
        }
    }
}

impl fmt::Display for DrfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrfError::NoDatums | DrfError::EmptyLabel => f.write_str(self.code()),
            DrfError::TooManyDatums { count } => write!(f, "too_many_datums ({count} > {MAX_DATUMS})"),
            DrfError::DuplicateLabel { label } | DrfError::NotFinite { label } | DrfError::NotUnit { label } => {
                write!(f, "{}{{{label}}}", self.code())
            }
            DrfError::DegenerateDatumPair { first, second } => write!(f, "degenerate_datum_pair{{{first},{second}}}"),
        }
    }
}

impl std::error::Error for DrfError {}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add_scaled(a: V3, b: V3, s: f64) -> V3 {
    [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]]
}

fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `a - (a.u) u`.
fn reject(a: V3, u: V3) -> V3 {
    add_scaled(a, u, -dot(a, u))
}

fn finite3(a: V3) -> bool {
    a.iter().all(|c| c.is_finite())
}

/// A datum after input checks: kind, point, normalised direction, label for errors.
struct Feat<'a> {
    label: &'a str,
    kind: DatumKind,
    p: V3,
    u: V3,
}

struct Ctx<'a> {
    label: &'a str,
}

impl Ctx<'_> {
    fn not_finite(&self) -> DrfError {
        DrfError::NotFinite { label: self.label.to_owned() }
    }

    fn norm(&self, a: V3) -> Result<f64, DrfError> {
        fid_num::f::sqrt(dot(a, a)).map_err(|_| self.not_finite())
    }

    fn unit(&self, a: V3) -> Result<V3, DrfError> {
        let n = self.norm(a)?;
        let v = scale(a, 1.0 / n);
        if finite3(v) {
            Ok(v)
        } else {
            Err(self.not_finite())
        }
    }

    fn finite(&self, a: V3) -> Result<V3, DrfError> {
        if finite3(a) {
            Ok(a)
        } else {
            Err(self.not_finite())
        }
    }
}

fn check(d: &SimulatedDatum) -> Result<Feat<'_>, DrfError> {
    let ctx = Ctx { label: &d.label };
    let (p, u) = d.geometry.parts();
    let p = ctx.finite(p)?;
    let u = ctx.finite(u)?;
    let n = ctx.norm(u)?;
    if (n - 1.0).abs() > UNIT_TOL {
        return Err(DrfError::NotUnit { label: d.label.clone() });
    }
    Ok(Feat { label: &d.label, kind: d.geometry.kind(), p, u: ctx.unit(u)? })
}

/// The pinned clocking when no datum fixes `rz` (module documentation).
fn default_clock(z: V3, ctx: &Ctx<'_>) -> Result<(V3, V3), DrfError> {
    let k = (1..3).fold(0, |k, i| if z[i].abs() < z[k].abs() { i } else { k });
    let mut e = [0.0; 3];
    e[k] = 1.0;
    let x = ctx.unit(reject(e, z))?;
    Ok((x, cross(z, x)))
}

/// Whether a translation along unit `e` moves the datum.
fn moved_by_translation(f: &Feat<'_>, e: V3, ctx: &Ctx<'_>) -> Result<bool, DrfError> {
    Ok(match f.kind {
        DatumKind::Plane => dot(f.u, e).abs() >= ANGLE_TOL,
        DatumKind::Axis => ctx.norm(reject(e, f.u))? >= ANGLE_TOL,
    })
}

/// Moves `o` along the translations `dirs` (unit, mutually orthogonal) onto the plane, or to
/// the closest point to the axis (module documentation, Origin).
fn relocate(o: V3, dirs: &[V3], f: &Feat<'_>, ctx: &Ctx<'_>) -> Result<V3, DrfError> {
    let moved = match (f.kind, dirs) {
        (_, []) => o,
        (DatumKind::Plane, _) => {
            // Projection of the normal onto the span of dirs; the move is along it.
            let mut m = [0.0; 3];
            for e in dirs {
                m = add_scaled(m, *e, dot(f.u, *e));
            }
            let s = dot(f.u, sub(f.p, o)) / dot(f.u, m);
            add_scaled(o, m, s)
        }
        (DatumKind::Axis, [e]) => {
            let a = reject(*e, f.u);
            let s = dot(a, sub(f.p, o)) / dot(a, a);
            add_scaled(o, *e, s)
        }
        (DatumKind::Axis, [e1, e2, ..]) => {
            // Normal equations of min |P(o + c1 e1 + c2 e2 - p)|, P the projector across the
            // axis; dirs never holds three translations for an axis (it is free along itself).
            let (a1, a2) = (reject(*e1, f.u), reject(*e2, f.u));
            let r = sub(f.p, o);
            let (g11, g12, g22) = (dot(a1, a1), dot(a1, a2), dot(a2, a2));
            let (h1, h2) = (dot(a1, r), dot(a2, r));
            let det = g11 * g22 - g12 * g12;
            let c1 = (h1 * g22 - g12 * h2) / det;
            let c2 = (g11 * h2 - g12 * h1) / det;
            add_scaled(add_scaled(o, *e1, c1), *e2, c2)
        }
    };
    ctx.finite(moved)
}

/// The partner named with a degenerate datum (module documentation).
fn partner<'a>(feats: &[Feat<'a>], i: usize) -> Result<&'a str, DrfError> {
    let f = &feats[i];
    let ctx = Ctx { label: f.label };
    for g in &feats[..i] {
        if g.kind == f.kind && ctx.norm(cross(g.u, f.u))? < ANGLE_TOL {
            return Ok(g.label);
        }
    }
    Ok(feats[i - 1].label)
}

/// Builds a DRF from one to three simulated datums in precedence order (module
/// documentation). Pure and deterministic; every outcome is a returned `Result`.
pub fn build_drf(datums: &[SimulatedDatum]) -> Result<Drf, DrfError> {
    if datums.is_empty() {
        return Err(DrfError::NoDatums);
    }
    if datums.len() > MAX_DATUMS {
        return Err(DrfError::TooManyDatums { count: datums.len() });
    }
    for (i, d) in datums.iter().enumerate() {
        if d.label.is_empty() {
            return Err(DrfError::EmptyLabel);
        }
        if datums[..i].iter().any(|e| e.label == d.label) {
            return Err(DrfError::DuplicateLabel { label: d.label.clone() });
        }
    }
    let feats = datums.iter().map(check).collect::<Result<Vec<_>, _>>()?;

    let primary = &feats[0];
    let z = primary.u;
    let mut origin = primary.p;
    let first = match primary.kind {
        DatumKind::Plane => DofSet::of(&[Dof::Tz, Dof::Rx, Dof::Ry]),
        DatumKind::Axis => DofSet::of(&[Dof::Tx, Dof::Ty, Dof::Rx, Dof::Ry]),
    };
    let mut free = DofSet::ALL.minus(first);
    let mut constrained = vec![first];
    // (x, y) once a datum has fixed rz.
    let mut clock: Option<(V3, V3)> = None;

    for (i, f) in feats.iter().enumerate().skip(1) {
        let ctx = Ctx { label: f.label };
        let mut c = DofSet::EMPTY;
        if free.contains(Dof::Rz) {
            let w = reject(f.u, z);
            let nw = ctx.norm(w)?;
            if nw >= ANGLE_TOL {
                c = c.with(Dof::Rz);
                let w = ctx.unit(w)?;
                clock = Some(match f.kind {
                    DatumKind::Plane => (cross(w, z), w),
                    DatumKind::Axis => (w, cross(z, w)),
                });
            } else if f.kind == DatumKind::Axis && !free.contains(Dof::Tx) && !free.contains(Dof::Ty) {
                // Axis parallel to z: its offset from the z line through the origin, measured
                // in the plane through the origin perpendicular to z.
                let s = dot(z, sub(origin, f.p)) / dot(z, f.u);
                let foot = add_scaled(f.p, f.u, s);
                let off = reject(sub(foot, origin), z);
                let r = ctx.norm(off)?;
                if r >= COAXIAL_TOL {
                    c = c.with(Dof::Rz);
                    let x = ctx.unit(off)?;
                    clock = Some((x, cross(z, x)));
                }
            }
        }
        let (x, y) = match clock {
            Some(xy) => xy,
            None => default_clock(z, &ctx)?,
        };
        let mut dirs: Vec<V3> = Vec::new();
        for (dof, e) in [(Dof::Tx, x), (Dof::Ty, y), (Dof::Tz, z)] {
            if free.contains(dof) && moved_by_translation(f, e, &ctx)? {
                c = c.with(dof);
                dirs.push(e);
            }
        }
        if c.is_empty() {
            return Err(DrfError::DegenerateDatumPair {
                first: partner(&feats, i)?.to_owned(),
                second: f.label.to_owned(),
            });
        }
        origin = relocate(origin, &dirs, f, &ctx)?;
        free = free.minus(c);
        constrained.push(c);
    }

    let (x, y) = match clock {
        Some(xy) => xy,
        None => default_clock(z, &Ctx { label: primary.label })?,
    };
    let precedence = [Precedence::Primary, Precedence::Secondary, Precedence::Tertiary];
    let datums = datums
        .iter()
        .zip(constrained)
        .zip(precedence)
        .map(|((d, c), p)| DatumDof {
            label: d.label.clone(),
            precedence: p,
            kind: d.geometry.kind(),
            constrained: c,
            alg: d.alg.clone(),
            dmt_row: d.dmt_row.clone(),
        })
        .collect();
    Ok(Drf { origin, axes: [x, y, z], datums, free })
}
