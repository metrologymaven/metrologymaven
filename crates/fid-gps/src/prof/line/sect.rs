//! Section bands and line-element deviations (spec node `GDT.PROF.LINE.SECT`, REQ-001..008,
//! AC-1..AC-6).
//!
//! [`build_sections`] turns the measured points and their signed deviations
//! ([`DeviationSet`], `GDT.PROF.DEV`) into line elements: one [`Section`] per plane
//! `{x : n . x = s_k}` the chain names, holding the points of its band
//! `|n . q - s_k| <= w` and one line-element deviation per member. It is the extraction step
//! of profile of a line; zone intervals, pose and value are `GDT.PROF.LINE.EVAL`'s.
//!
//! # Frame (REQ-001)
//!
//! The section normal `n` and the positions `s_k` are in the **nominal frame of the
//! deviations**: the frame the caller's pose maps the measured points into, where
//! `GDT.PROF.DEV` found the feet and the surface normals. [`build_sections`] takes that pose
//! and maps each measured point `p` to `q = R p + t` with GEO.FRAME.XFORM
//! `Transform::apply_point`, the very operation `GDT.PROF.DEV` projected, so the point that is
//! grouped is bit for bit the point whose deviation was measured. The pose must be the one
//! the deviations were taken under: its `pose_digest` must equal
//! [`DeviationSet::pose_digest`], else `pose_mismatch`. A chain whose sections are stated in
//! another frame (a datum reference frame) is mapped into the nominal frame by the caller.
//!
//! Positions are explicit (in chain order) or `start + k * step` for `k = 0..count` (one
//! product and one sum, each rounded once). A section's `index` is its chain order.
//!
//! # Exact band membership (REQ-002, AC-4)
//!
//! "Compared without tolerance" is taken literally: a point is a member of band `k` exactly
//! when the exact real number `n . q - s_k` (on the binary64 values of `n`, `q`, `s_k`)
//! lies in `[-w, w]`, never on a rounded dot product. Each comparison first runs a float
//! filter (the dot product with a forward error bound four times larger than the classical
//! one) and, only when the bound cannot decide, the exact sign of
//! `n0 q0 + n1 q1 + n2 q2 - s_k -+ w` from Dekker products and a Shewchuk grow-expansion
//! (no FMA, no division). The verdict is therefore never wrong against the exact band. The
//! exact path needs every non-zero `n_i`, `q_i` within `[2^-480, 2^480]` (the products are
//! then exact); outside that range it refuses with `out_of_range` rather than guess, and
//! every `|q_i|`, `|s_k|` and `w` above `2^480` is refused up front.
//!
//! Bands are searched by bisection over the positions in ascending order (the exact
//! predicate is monotone in `s_k`), so a point is tested against `O(log K)` bands.
//!
//! # Overlap (REQ-003, AC-3)
//!
//! Adjacent bands overlap when their positions are closer than `2w`. The gap is taken
//! exactly (Knuth two-sum, so a gap that rounds to `2w` but is smaller still overlaps), and
//! two identical positions always overlap (their bands coincide, also for `w = 0`). The
//! error names both sections by chain index, lower index first, for the first overlapping
//! pair in ascending position order. Two bands exactly `2w` apart do not overlap; a point
//! exactly on their shared bound is a member of both, as REQ-002 says of each band.
//!
//! # Line-element deviation (REQ-004, REQ-005)
//!
//! - `Normal3d`: the point's signed deviation, unchanged.
//! - `InPlane { min_inplane }`: `d / m` with `m = |n_s - (n_s . n) n|`, `n_s` the surface
//!   normal at the foot (as `GDT.PROF.DEV` gives it): `c = (n_s0 n0 + n_s1 n1) + n_s2 n2`,
//!   `v_i = n_s_i - c n_i`, `m = sqrt((v0 v0 + v1 v1) + v2 v2)`, each step rounded once. A point
//!   with `m < min_inplane` is flagged `oblique_section` and excluded from that section.
//!   `min_inplane` has no default: the row must state it (`0 < min_inplane <= 1`).
//!
//! # Honesty (D-045)
//!
//! A line-element deviation is never made up:
//! - A point with `no_projection` is excluded from every section and counted in each
//!   section whose band holds it (REQ-008).
//! - A projected point whose `signed_dev` is `None` (no sign of record, including a foot on
//!   the edge of the toleranced set, where `GDT.PROF.DEV` gives no sign) is not a member: it
//!   is listed in [`Section::withheld`] with [`Withheld::NoSign`]. Under `InPlane`, a point
//!   without a surface normal is withheld with [`Withheld::NoNormal`]. Neither sign nor
//!   normal is guessed.
//! - The faces' approximation errors stay in the [`DeviationSet`] (declared by the caller to
//!   `GDT.PROF.DEV`, not verified); members index into it. How the in-plane correction scales
//!   them is `GDT.PROF.LINE.EVAL`'s and UNC's call.
//! - Nothing here decides conformance (UNC.DEC does) or evaluates a zone. Torus faces never
//!   reach this module: `GDT.PROF.DEV` refuses them.
//!
//! # Determinism (D0, KNP-1)
//!
//! Points are visited in input order, so members are ascending. The arithmetic is a fixed
//! sequence of binary64 operations (no FMA, no `f64::min`/`max`, square root through
//! `fid_num::f`), the band verdicts are exact, and no hash containers are used: the output
//! is bit-identical on every platform (AC-6). NaN and infinite inputs are errors; nothing
//! panics. Validation order: the section spec, then the counts and the pose, then each point
//! in input order (its coordinates, its deviation, its image).

use core::cmp::Ordering;
use core::fmt;

use fid_geo_frame::{Rigid, Transform};
use fid_num::f as numf;

use crate::prof::dev::{pose_digest, DevOutcome, Deviation, DeviationSet};

type V3 = [f64; 3];

/// REQ-006: `|(|n|) - 1|` above this is `non_unit_direction`.
pub const UNIT_TOL: f64 = 1e-12;

/// Most sections one call accepts.
pub const MAX_SECTIONS: usize = 1_000_000;

/// Most points one call accepts: indexes are `u32`.
pub const MAX_POINTS: usize = u32::MAX as usize;

/// `2^480`: the largest magnitude of a mapped coordinate, a position or the half-width.
pub const MAX_MAGNITUDE: f64 = f64::from_bits((1023 + 480) << 52);

/// `2^-480`: the smallest non-zero magnitude of `n_i` and `q_i` the exact path takes.
pub const MIN_EXACT_MAGNITUDE: f64 = f64::from_bits((1023 - 480) << 52);

/// `2^-49`: relative width of the float filter.
const FILTER_REL: f64 = f64::from_bits((1023 - 49) << 52);

/// `2^-1070`: absolute width of the float filter (covers underflow of the products).
const FILTER_ABS: f64 = f64::from_bits(16);

/// Dekker's splitter `2^27 + 1`.
const SPLITTER: f64 = 134_217_729.0;

// ---------------------------------------------------------------- errors

/// Every error of this module. [`SectError::code`] is stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectError {
    /// `section_positions_missing`: no positions are stated (none, an empty list or a
    /// count of zero).
    SectionPositionsMissing,
    /// `too_many_sections`: more than [`MAX_SECTIONS`] positions.
    TooManySections,
    /// `non_unit_direction`: `|(|n|) - 1| > 1e-12`.
    NonUnitDirection,
    /// `bad_half_width`: the band half-width is negative.
    BadHalfWidth,
    /// `bad_min_inplane`: `min_inplane` is not in `(0, 1]`.
    BadMinInplane,
    /// `overlapping_bands`: sections `a` and `b` (chain indices, `a < b`) are adjacent and
    /// closer than `2w`, or at the same position.
    OverlappingBands {
        /// The lower chain index.
        a: u32,
        /// The higher chain index.
        b: u32,
    },
    /// `nan_input`: a NaN or infinite value in the spec (`index: None`) or in point `index`
    /// (its coordinates, signed deviation or surface normal).
    NanInput {
        /// The point, or `None` for the section spec.
        index: Option<u32>,
    },
    /// `out_of_range`: a magnitude outside the exact domain (module docs), or an in-plane
    /// deviation that overflows; in the spec (`index: None`) or at point `index`.
    OutOfRange {
        /// The point, or `None` for the section spec.
        index: Option<u32>,
    },
    /// `too_many_points`: more than [`MAX_POINTS`] points.
    TooManyPoints,
    /// `deviation_count_mismatch`: the deviations are not one per point in input order.
    DeviationCountMismatch,
    /// `pose_mismatch`: the pose is not the one the deviations were taken under.
    PoseMismatch,
    /// `pose_digest`: the pose could not be encoded (never expected for a validated pose).
    PoseDigest,
    /// `inconsistent`: an internal invariant failed (never expected).
    Inconsistent,
}

impl SectError {
    /// A stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            SectError::SectionPositionsMissing => "section_positions_missing",
            SectError::TooManySections => "too_many_sections",
            SectError::NonUnitDirection => "non_unit_direction",
            SectError::BadHalfWidth => "bad_half_width",
            SectError::BadMinInplane => "bad_min_inplane",
            SectError::OverlappingBands { .. } => "overlapping_bands",
            SectError::NanInput { .. } => "nan_input",
            SectError::OutOfRange { .. } => "out_of_range",
            SectError::TooManyPoints => "too_many_points",
            SectError::DeviationCountMismatch => "deviation_count_mismatch",
            SectError::PoseMismatch => "pose_mismatch",
            SectError::PoseDigest => "pose_digest",
            SectError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for SectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SectError::OverlappingBands { a, b } => write!(f, "overlapping_bands{{{a},{b}}}"),
            SectError::NanInput { index: Some(i) } => write!(f, "nan_input{{point {i}}}"),
            SectError::OutOfRange { index: Some(i) } => write!(f, "out_of_range{{point {i}}}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for SectError {}

// ---------------------------------------------------------------- the spec

/// Where the section planes are (Design: `Positions`).
#[derive(Debug, Clone, PartialEq)]
pub enum Positions {
    /// Explicit positions `s_k` along `n`, metres, in chain order.
    Explicit(Vec<f64>),
    /// `s_k = start + k * step` for `k = 0..count`, metres.
    Stepped {
        /// The first position.
        start: f64,
        /// The spacing (any sign).
        step: f64,
        /// How many sections.
        count: u32,
    },
}

/// How a line-element deviation is taken (REQ-004).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    /// The 3D signed deviation as is.
    Normal3d,
    /// The 3D signed deviation divided by the in-plane length `m` of the surface normal;
    /// points with `m < min_inplane` are `oblique_section` and excluded.
    InPlane {
        /// The smallest accepted `m`, `0 < min_inplane <= 1`; stated by the row.
        min_inplane: f64,
    },
}

/// The chain's section parameters (Design: `SectionSpec`), nominal frame, metres.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionSpec {
    /// The unit normal `n` of every section plane.
    pub normal: V3,
    /// The positions; `None` when neither the chain nor the characteristic states them.
    pub positions: Option<Positions>,
    /// The band half-width `w >= 0`.
    pub half_width_m: f64,
    /// The line-element deviation method.
    pub method: Method,
}

// ---------------------------------------------------------------- results

/// A per-section flag (REQ-005, REQ-007, REQ-008), listed in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectFlag {
    /// `empty_section`: the section has no member.
    EmptySection,
    /// `oblique_section`: at least one point was excluded as oblique.
    ObliqueSection,
    /// `no_projection`: at least one point in the band had no projection.
    NoProjection,
    /// `withheld`: at least one point in the band has no line-element deviation of record.
    Withheld,
}

impl SectFlag {
    /// The stable code.
    pub const fn code(self) -> &'static str {
        match self {
            SectFlag::EmptySection => "empty_section",
            SectFlag::ObliqueSection => "oblique_section",
            SectFlag::NoProjection => "no_projection",
            SectFlag::Withheld => "withheld",
        }
    }
}

/// Why a projected point in a band has no line-element deviation (module docs, D-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Withheld {
    /// `no_sign`: `GDT.PROF.DEV` gives no signed deviation of record.
    NoSign,
    /// `no_normal`: under `InPlane`, no surface normal is defined at the foot.
    NoNormal,
}

impl Withheld {
    /// The stable code.
    pub const fn code(self) -> &'static str {
        match self {
            Withheld::NoSign => "no_sign",
            Withheld::NoNormal => "no_normal",
        }
    }
}

/// One line element (Design: `Section`, REQ-007).
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// The chain order of the position.
    pub index: u32,
    /// The section normal `n`, nominal frame.
    pub normal: V3,
    /// The position `s_k`, metres.
    pub position_m: f64,
    /// The member point indices, ascending.
    pub members: Vec<u32>,
    /// The line-element deviation of each member, metres, parallel to `members`.
    pub dev_m: Vec<f64>,
    /// Projected points in the band without a line-element deviation, ascending.
    pub withheld: Vec<(u32, Withheld)>,
    /// Points in the band flagged `oblique_section` and excluded, ascending.
    pub oblique: Vec<u32>,
    /// How many points in the band had no projection (REQ-008).
    pub excluded_no_projection: u32,
    /// How many points in the band were excluded as oblique (`oblique.len()`).
    pub excluded_oblique: u32,
    /// The section's flags, in [`SectFlag`] order.
    pub flags: Vec<SectFlag>,
}

impl Section {
    /// Whether the section carries `flag`.
    pub fn has(&self, flag: SectFlag) -> bool {
        self.flags.contains(&flag)
    }
}

// ---------------------------------------------------------------- exact arithmetic

/// Knuth's two-sum: `a + b = s + e` exactly (no overflow in this module's domain).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    (s, e)
}

/// Dekker's split of `a` into two 26-bit halves.
fn split(a: f64) -> (f64, f64) {
    let c = SPLITTER * a;
    let hi = c - (c - a);
    (hi, a - hi)
}

/// Dekker's two-product: `a b = p + e` exactly for operands in the exact domain.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    let e = al * bl - (((p - ah * bh) - al * bh) - ah * bl);
    (p, e)
}

/// The exact sign of the sum of `terms` (Shewchuk grow-expansion; the sign of a
/// non-overlapping expansion is the sign of its largest non-zero component).
fn exact_sign(terms: &[f64; 8]) -> Ordering {
    let mut e = [0.0f64; 8];
    let mut len = 0usize;
    for &b in terms {
        let mut q = b;
        for slot in e.iter_mut().take(len) {
            let (s, h) = two_sum(q, *slot);
            *slot = h;
            q = s;
        }
        if let Some(slot) = e.get_mut(len) {
            *slot = q;
        }
        len += 1;
    }
    for x in e.iter().rev() {
        if *x > 0.0 {
            return Ordering::Greater;
        }
        if *x < 0.0 {
            return Ordering::Less;
        }
    }
    Ordering::Equal
}

fn in_exact_domain(x: f64) -> bool {
    x == 0.0 || (x.abs() >= MIN_EXACT_MAGNITUDE && x.abs() <= MAX_MAGNITUDE)
}

/// `n . q` of one point: the float value, its absolute-term sum, and the exact products
/// once they are needed.
struct Dot {
    n: V3,
    q: V3,
    d: f64,
    abs_sum: f64,
    exact: Option<[f64; 6]>,
    index: u32,
}

impl Dot {
    fn new(n: V3, q: V3, index: u32) -> Dot {
        let (a0, a1, a2) = (n[0] * q[0], n[1] * q[1], n[2] * q[2]);
        let d = (a0 + a1) + a2;
        let abs_sum = (a0.abs() + a1.abs()) + a2.abs();
        Dot { n, q, d, abs_sum, exact: None, index }
    }

    /// The exact ordering of `n . q` against `s + c`.
    fn cmp(&mut self, s: f64, c: f64) -> Result<Ordering, SectError> {
        let bf = s + c;
        let diff = self.d - bf;
        let margin = (self.abs_sum + bf.abs()) * FILTER_REL + FILTER_ABS;
        if diff > margin {
            return Ok(Ordering::Greater);
        }
        if diff < -margin {
            return Ok(Ordering::Less);
        }
        let p = match self.exact {
            Some(p) => p,
            None => {
                if !self.n.iter().chain(self.q.iter()).all(|x| in_exact_domain(*x)) {
                    return Err(SectError::OutOfRange { index: Some(self.index) });
                }
                let (p0, e0) = two_prod(self.n[0], self.q[0]);
                let (p1, e1) = two_prod(self.n[1], self.q[1]);
                let (p2, e2) = two_prod(self.n[2], self.q[2]);
                let p = [e0, e1, e2, p0, p1, p2];
                self.exact = Some(p);
                p
            }
        };
        Ok(exact_sign(&[p[0], p[1], p[2], p[3], p[4], p[5], -s, -c]))
    }
}

// ---------------------------------------------------------------- validation

fn finite3(a: &V3) -> bool {
    a.iter().all(|x| x.is_finite())
}

/// The validated spec: positions sorted ascending with their chain indices.
struct Plan {
    n: V3,
    w: f64,
    method: Method,
    chain: Vec<f64>,
    sorted: Vec<(f64, u32)>,
}

fn chain_positions(positions: &Option<Positions>) -> Result<Vec<f64>, SectError> {
    let list = match positions {
        None => return Err(SectError::SectionPositionsMissing),
        Some(Positions::Explicit(v)) => {
            if v.is_empty() {
                return Err(SectError::SectionPositionsMissing);
            }
            if v.len() > MAX_SECTIONS {
                return Err(SectError::TooManySections);
            }
            v.clone()
        }
        Some(Positions::Stepped { start, step, count }) => {
            if *count == 0 {
                return Err(SectError::SectionPositionsMissing);
            }
            if *count as usize > MAX_SECTIONS {
                return Err(SectError::TooManySections);
            }
            if !(start.is_finite() && step.is_finite()) {
                return Err(SectError::NanInput { index: None });
            }
            (0..*count).map(|k| *start + f64::from(k) * *step).collect()
        }
    };
    let explicit = matches!(positions, Some(Positions::Explicit(_)));
    for s in &list {
        if explicit && !s.is_finite() {
            return Err(SectError::NanInput { index: None });
        }
        // A stepped position that overflows (or any finite one above 2^480) is out of range.
        if !s.is_finite() || s.abs() > MAX_MAGNITUDE {
            return Err(SectError::OutOfRange { index: None });
        }
    }
    Ok(list)
}

fn validate(spec: &SectionSpec) -> Result<Plan, SectError> {
    let chain = chain_positions(&spec.positions)?;
    let n = spec.normal;
    if !finite3(&n) {
        return Err(SectError::NanInput { index: None });
    }
    let len = numf::sqrt((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]).map_err(|_| SectError::NanInput { index: None })?;
    if !len.is_finite() || (len - 1.0).abs() > UNIT_TOL {
        return Err(SectError::NonUnitDirection);
    }
    let w = spec.half_width_m;
    if !w.is_finite() {
        return Err(SectError::NanInput { index: None });
    }
    if w < 0.0 {
        return Err(SectError::BadHalfWidth);
    }
    if w > MAX_MAGNITUDE {
        return Err(SectError::OutOfRange { index: None });
    }
    // A zero half-width is +0.0 whatever its sign bit.
    let w = if w == 0.0 { 0.0 } else { w };
    if let Method::InPlane { min_inplane } = spec.method {
        if !min_inplane.is_finite() {
            return Err(SectError::NanInput { index: None });
        }
        if min_inplane <= 0.0 || min_inplane > 1.0 {
            return Err(SectError::BadMinInplane);
        }
    }
    let mut sorted = Vec::with_capacity(chain.len());
    for (k, s) in chain.iter().enumerate() {
        sorted.push((*s, u32::try_from(k).map_err(|_| SectError::TooManySections)?));
    }
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let two_w = w * 2.0;
    for pair in sorted.windows(2) {
        if let [(lo, ia), (hi, ib)] = pair {
            let (g, e) = two_sum(*hi, -*lo);
            let identical = g == 0.0 && e == 0.0;
            if identical || g < two_w || (g == two_w && e < 0.0) {
                let (a, b) = if ia < ib { (*ia, *ib) } else { (*ib, *ia) };
                return Err(SectError::OverlappingBands { a, b });
            }
        }
    }
    Ok(Plan { n, w, method: spec.method, chain, sorted })
}

// ---------------------------------------------------------------- per point

/// What one point contributes to a section it lies in.
enum Item {
    NoProjection,
    NoSign,
    Dev { d: f64, normal: Option<V3> },
}

fn item_of(dv: &Deviation, index: u32) -> Result<Item, SectError> {
    let pr = match &dv.outcome {
        DevOutcome::NoProjection => return Ok(Item::NoProjection),
        DevOutcome::Projected(pr) => pr,
    };
    if let Some(ns) = &pr.normal {
        if !finite3(ns) {
            return Err(SectError::NanInput { index: Some(index) });
        }
    }
    match pr.signed_dev {
        None => Ok(Item::NoSign),
        Some(d) if !d.is_finite() => Err(SectError::NanInput { index: Some(index) }),
        Some(d) => Ok(Item::Dev { d, normal: pr.normal }),
    }
}

/// `m = |n_s - (n_s . n) n|` (module docs).
fn inplane_len(ns: &V3, n: &V3) -> Result<f64, SectError> {
    let c = (ns[0] * n[0] + ns[1] * n[1]) + ns[2] * n[2];
    let v = [ns[0] - c * n[0], ns[1] - c * n[1], ns[2] - c * n[2]];
    numf::sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).map_err(|_| SectError::Inconsistent)
}

fn place(sec: &mut Section, index: u32, item: &Item, plan: &Plan) -> Result<(), SectError> {
    match item {
        Item::NoProjection => sec.excluded_no_projection = sec.excluded_no_projection.saturating_add(1),
        Item::NoSign => sec.withheld.push((index, Withheld::NoSign)),
        Item::Dev { d, normal } => match plan.method {
            Method::Normal3d => {
                sec.members.push(index);
                sec.dev_m.push(*d);
            }
            Method::InPlane { min_inplane } => match normal {
                None => sec.withheld.push((index, Withheld::NoNormal)),
                Some(ns) => {
                    let m = inplane_len(ns, &plan.n)?;
                    if m < min_inplane {
                        sec.oblique.push(index);
                        sec.excluded_oblique = sec.excluded_oblique.saturating_add(1);
                    } else {
                        let dev = *d / m;
                        if !dev.is_finite() {
                            return Err(SectError::OutOfRange { index: Some(index) });
                        }
                        sec.members.push(index);
                        sec.dev_m.push(dev);
                    }
                }
            },
        },
    }
    Ok(())
}

/// The line elements of `points` (measured frame) with their deviations `devs`, taken under
/// `pose`, in the section planes of `spec` (module docs). One [`Section`] per position, in
/// chain order.
pub fn build_sections(
    points: &[V3],
    pose: &Rigid,
    devs: &DeviationSet,
    spec: &SectionSpec,
) -> Result<Vec<Section>, SectError> {
    let plan = validate(spec)?;
    if points.len() > MAX_POINTS {
        return Err(SectError::TooManyPoints);
    }
    if devs.items.len() != points.len() {
        return Err(SectError::DeviationCountMismatch);
    }
    if pose_digest(pose).map_err(|_| SectError::PoseDigest)? != devs.pose_digest {
        return Err(SectError::PoseMismatch);
    }
    let mut sections = Vec::with_capacity(plan.chain.len());
    for (k, s) in plan.chain.iter().enumerate() {
        sections.push(Section {
            index: u32::try_from(k).map_err(|_| SectError::TooManySections)?,
            normal: plan.n,
            position_m: *s,
            members: Vec::new(),
            dev_m: Vec::new(),
            withheld: Vec::new(),
            oblique: Vec::new(),
            excluded_no_projection: 0,
            excluded_oblique: 0,
            flags: Vec::new(),
        });
    }
    let t = Transform::Rigid(*pose);
    let w = plan.w;
    for (k, (p, dv)) in points.iter().zip(&devs.items).enumerate() {
        let index = u32::try_from(k).map_err(|_| SectError::TooManyPoints)?;
        if dv.index != index {
            return Err(SectError::DeviationCountMismatch);
        }
        if !finite3(p) {
            return Err(SectError::NanInput { index: Some(index) });
        }
        let item = item_of(dv, index)?;
        let q = t.apply_point(p).map_err(|_| SectError::OutOfRange { index: Some(index) })?;
        if !q.iter().all(|x| x.abs() <= MAX_MAGNITUDE) {
            return Err(SectError::OutOfRange { index: Some(index) });
        }
        let mut dot = Dot::new(plan.n, q, index);
        // The first band (ascending position) whose upper bound s + w is not below n . q.
        let (mut lo, mut hi) = (0usize, plan.sorted.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let (s, _) = *plan.sorted.get(mid).ok_or(SectError::Inconsistent)?;
            if dot.cmp(s, w)? == Ordering::Greater {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // Every band from there whose lower bound s - w is not above n . q.
        let mut j = lo;
        while let Some(&(s, ci)) = plan.sorted.get(j) {
            if dot.cmp(s, -w)? == Ordering::Less {
                break;
            }
            let sec = sections.get_mut(ci as usize).ok_or(SectError::Inconsistent)?;
            place(sec, index, &item, &plan)?;
            j += 1;
        }
    }
    for sec in &mut sections {
        if sec.members.is_empty() {
            sec.flags.push(SectFlag::EmptySection);
        }
        if sec.excluded_oblique > 0 {
            sec.flags.push(SectFlag::ObliqueSection);
        }
        if sec.excluded_no_projection > 0 {
            sec.flags.push(SectFlag::NoProjection);
        }
        if !sec.withheld.is_empty() {
            sec.flags.push(SectFlag::Withheld);
        }
    }
    Ok(sections)
}
