//! Rocking candidate sets, the tie-break and the tangential variant table. Spec node
//! `KRN.ENV.TAN.ROCK`.
//!
//! A datum feature that rocks (a slightly convex face on a surface plate) can touch its
//! simulator in more than one stable way. The tangential fits (`KRN.ENV.TAN.PLANE`, `.LINE`,
//! `.CYL`) pass every candidate contact set their multi-start runs (and any hull-neighbour
//! search) find to [`rock_report`], which deduplicates them, keeps those within `rock_tol` of
//! the best, orders them by one pinned rule and marks the first one selected. Nothing is
//! picked silently: the report says how the selection was decided ([`TieBreak`]), how many
//! duplicates were merged and how many distinct sets fell outside `rock_tol`, and a duplicate
//! that cannot be merged by rule (an objective tie between different features) is refused.
//!
//! This module also owns the shared [`TanVariant`] and [`MaterialSide`] types and the table of
//! defined (primitive, variant) pairs ([`DEFINED_PAIRS`], [`check_defined`]).
//!
//! # Pinned rules (REQ-KRN.ENV.TAN.ROCK-001 .. -005)
//!
//! 1. **Validation**, candidate by candidate in input order: the contact list is sorted
//!    ascending; an empty list or a repeated index is `InvalidParams{field:
//!    "candidates.contacts"}`. The objective must be finite (NaN is `Num(NotANumber)`, an
//!    infinity `Num(Infinite)`, both with `op = "tan_rock.objective"` and the input index) and
//!    not negative (`InvalidParams{field: "candidates.objective"}`); `-0.0` becomes `+0.0`.
//!    The feature must pass [`FittedFeature::canonical`] (finite, in range) but is stored
//!    unchanged, because a tangent feature's normal sign carries the material side.
//!    `rock_tol` must be finite and in `[0, 1]` (`InvalidParams{field: "rock_tol"}`); `-0.0`
//!    becomes `+0.0`. No candidates, or more than [`MAX_CANDIDATES`], is
//!    `InvalidParams{field: "candidates"}`.
//! 2. **Deduplication** (REQ-001): candidates are grouped by their sorted contact indices.
//!    Within a group the kept one has the lowest objective (IEEE-754 total order). Members
//!    that repeat it exactly (bit-equal objective and identical canonical feature bytes,
//!    [`encode_feature`]) merge. A member with the bit-equal lowest objective but a different
//!    feature (for example the opposite-sign normal, which is the other material side) has no
//!    rule that may choose between them, so the report is refused with
//!    `FitError::AmbiguousCandidate` naming the set (its lowest index and size; the first such
//!    set in contact order, so the error does not depend on input order either).
//! 3. **Filter** (REQ-002): with `best` the lowest objective over the distinct sets,
//!    `bound = best + rock_tol * best`, evaluated as one IEEE-754 binary64 product and one
//!    sum, each rounded to nearest-even (no FMA). A distinct set is a candidate iff its
//!    objective is `<= bound`. A non-finite `bound` (overflow) is `Num(Infinite)` with
//!    `op = "tan_rock.bound"`.
//! 4. **Order** (REQ-002, the tie-break): objective ascending in total order, then the sorted
//!    contact indices in lexicographic order (element by element as unsigned integers; a
//!    proper prefix sorts first). The order depends only on the candidate values, never on
//!    their input order (REQ-005).
//! 5. **Selection** (REQ-003): the first candidate is selected (`selected = 0`);
//!    `rocking = candidates.len() > 1`. [`TieBreak`] states whether the selection was decided
//!    by a strictly lower objective or, on bit-equal objectives, by the contact-index order.
//!
//! All arithmetic is the single product and sum of rule 3; every comparison is a total-order
//! comparison on validated (non-NaN) values. No hashing, no FMA, no `f64::min`/`f64::max`,
//! nothing that depends on the platform, so the report and its canonical bytes
//! ([`encode_report`]) are bit-identical on x86-64, ARM64 and wasm32 (D0).

use core::cmp::Ordering;
use core::fmt;

use fid_num::NumError;

use crate::contract::{encode_feature, FitError, FittedFeature, Primitive};

/// The most candidates one report accepts (64 Ki). Multi-start is capped at
/// `fid_solv::mstart::MAX_STARTS` starts; the rest of the room is for hull-neighbour sets.
pub const MAX_CANDIDATES: usize = 1 << 16;

const OP_OBJECTIVE: &str = "tan_rock.objective";
const OP_BOUND: &str = "tan_rock.bound";
const OP_ALLOC: &str = "tan_rock";

/// The tangential objective. Required wherever a tangential fit is requested; it has no
/// `Default`, because the planar datum default differs by standard and edition (`GDT`
/// decides it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TanVariant {
    /// The tangent feature with the smallest maximum distance to the points.
    ExternalMinimax,
    /// The smallest sum of squared distances with every point on the material side.
    ConstrainedL2,
    /// The smallest sum of distances with every point on the material side.
    ConstrainedL1,
}

impl TanVariant {
    /// Every variant, in declaration order.
    pub const ALL: &'static [TanVariant] =
        &[TanVariant::ExternalMinimax, TanVariant::ConstrainedL2, TanVariant::ConstrainedL1];

    /// The wire name: `external_minimax`, `constrained_l2`, `constrained_l1`.
    pub const fn slug(self) -> &'static str {
        match self {
            TanVariant::ExternalMinimax => "external_minimax",
            TanVariant::ConstrainedL2 => "constrained_l2",
            TanVariant::ConstrainedL1 => "constrained_l1",
        }
    }

    /// The method segment of the algorithm IDs the tangential leaves declare
    /// (`fit.<feature>.<segment>@<version>`): `tan_minimax`, `tan_l2`, `tan_l1`.
    pub const fn method_segment(self) -> &'static str {
        match self {
            TanVariant::ExternalMinimax => "tan_minimax",
            TanVariant::ConstrainedL2 => "tan_l2",
            TanVariant::ConstrainedL1 => "tan_l1",
        }
    }

    /// Parses a wire name exactly.
    pub fn parse(s: &str) -> Option<TanVariant> {
        TanVariant::ALL.iter().copied().find(|v| v.slug() == s)
    }

    /// The variant of a request, which must be present: `None` is
    /// `InvalidParams{field: "variant"}` (there is no default).
    pub fn require(v: Option<TanVariant>) -> Result<TanVariant, FitError> {
        v.ok_or(FitError::InvalidParams { field: "variant" })
    }
}

impl fmt::Display for TanVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// Which side of the surface is material. The tangent feature lies on the other
/// (material-free) side and touches the points from there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaterialSide {
    /// One unit direction pointing out of the material, into the material-free side, for the
    /// whole point set (a plate face: the outward face normal). Build it with
    /// [`MaterialSide::outward`].
    Outward([f64; 3]),
    /// Each point's own normal (from the `PointsView`) points out of the material.
    PointNormals,
}

impl MaterialSide {
    /// An explicit outward direction, scaled to unit length with its sign kept (the sign is
    /// the side). A NaN or infinite component is `Num`; a zero or too small vector is
    /// `InvalidDirection`.
    pub fn outward(d: [f64; 3]) -> Result<MaterialSide, FitError> {
        Ok(MaterialSide::Outward(crate::contract::unit_direction(d)?))
    }

    /// Checks the side against the data: [`MaterialSide::PointNormals`] needs normals
    /// (`InvalidParams{field: "material_side"}` otherwise); an `Outward` direction must be
    /// finite and unit length within `contract::UNIT_NORMAL_TOL`.
    pub fn validate(&self, pts: &crate::contract::PointsView<'_>) -> Result<(), FitError> {
        match self {
            MaterialSide::PointNormals => {
                if pts.normals().is_none() {
                    return Err(FitError::InvalidParams { field: "material_side" });
                }
                Ok(())
            }
            MaterialSide::Outward(d) => {
                fid_num::check_finite_slice("tan_rock.material_side", d)?;
                let n = fid_num::red::norm2(d)?;
                let dev = n - 1.0;
                if !(dev <= crate::contract::UNIT_NORMAL_TOL && -dev <= crate::contract::UNIT_NORMAL_TOL) {
                    return Err(FitError::InvalidParams { field: "material_side" });
                }
                Ok(())
            }
        }
    }
}

/// The defined (primitive, variant) pairs, as data (REQ-KRN.ENV.TAN.ROCK-004): plane, 2D line
/// and cylinder, each in all three variants. Every other pair (3D line, sphere, cone, circle,
/// ...) is undefined: a 3D line has no material side, and the others have no tangential
/// leaf. Ordered by primitive, then variant, in declaration order.
pub const DEFINED_PAIRS: [(Primitive, TanVariant); 9] = [
    (Primitive::Line2, TanVariant::ExternalMinimax),
    (Primitive::Line2, TanVariant::ConstrainedL2),
    (Primitive::Line2, TanVariant::ConstrainedL1),
    (Primitive::Plane, TanVariant::ExternalMinimax),
    (Primitive::Plane, TanVariant::ConstrainedL2),
    (Primitive::Plane, TanVariant::ConstrainedL1),
    (Primitive::Cylinder, TanVariant::ExternalMinimax),
    (Primitive::Cylinder, TanVariant::ConstrainedL2),
    (Primitive::Cylinder, TanVariant::ConstrainedL1),
];

/// Whether the pair is in [`DEFINED_PAIRS`].
pub fn is_defined(primitive: Primitive, variant: TanVariant) -> bool {
    DEFINED_PAIRS.iter().any(|&(p, v)| p == primitive && v == variant)
}

/// `Ok` for a pair in [`DEFINED_PAIRS`]; otherwise
/// `FitError::TanMethodUndefined{primitive, variant}` (code `method_undefined`), naming both
/// (REQ-KRN.ENV.TAN.ROCK-004).
pub fn check_defined(primitive: Primitive, variant: TanVariant) -> Result<(), FitError> {
    if is_defined(primitive, variant) {
        Ok(())
    } else {
        Err(FitError::TanMethodUndefined { primitive, variant })
    }
}

/// One stable contact set found by a tangential fit.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateSet {
    /// Zero-based point indices in contact, sorted ascending without repeats (the report
    /// sorts them; repeats are refused).
    pub contacts: Vec<u32>,
    /// The variant's objective value for this set (not negative).
    pub objective: f64,
    /// The tangent feature for this set, as the fit produced it.
    pub feature: FittedFeature,
}

/// How the selected candidate was decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieBreak {
    /// Only one candidate: nothing to break.
    Single,
    /// The selected candidate's objective is strictly the lowest.
    Objective,
    /// The two lowest objectives are bit-equal; the lexicographic contact order decided.
    ContactOrder,
}

impl TieBreak {
    /// The wire name: `single`, `objective`, `contact_order`.
    pub const fn as_str(self) -> &'static str {
        match self {
            TieBreak::Single => "single",
            TieBreak::Objective => "objective",
            TieBreak::ContactOrder => "contact_order",
        }
    }
}

/// The rocking report (REQ-KRN.ENV.TAN.ROCK-002, -003).
#[derive(Debug, Clone, PartialEq)]
pub struct RockReport {
    /// The distinct sets within `rock_tol` of the best, in the pinned order.
    pub candidates: Vec<CandidateSet>,
    /// Index of the selected candidate in `candidates`: always `0`.
    pub selected: u32,
    /// `candidates.len() > 1`.
    pub rocking: bool,
    /// The relative tolerance used (`-0.0` stored as `+0.0`).
    pub rock_tol: f64,
    /// How the selection was decided.
    pub tie_break: TieBreak,
    /// Input candidates folded into another one with the same contact set.
    pub merged: u32,
    /// Distinct contact sets left out because their objective exceeded the bound.
    pub excluded: u32,
}

impl RockReport {
    /// The selected candidate.
    pub fn selected(&self) -> Option<&CandidateSet> {
        self.candidates.get(self.selected as usize)
    }
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| FitError::Num(NumError::Domain { op: OP_ALLOC, detail: "allocation_failed" }))?;
    Ok(v)
}

fn count(n: usize) -> u32 {
    // n <= MAX_CANDIDATES < 2^32.
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A validated candidate with its feature's canonical bytes (the duplicate key).
struct Norm {
    set: CandidateSet,
    key: Vec<u8>,
}

fn normalise(i: usize, c: &CandidateSet) -> Result<Norm, FitError> {
    let mut contacts = alloc::<u32>(c.contacts.len())?;
    contacts.extend_from_slice(&c.contacts);
    contacts.sort_unstable();
    if contacts.is_empty() || contacts.windows(2).any(|w| w[0] == w[1]) {
        return Err(FitError::InvalidParams { field: "candidates.contacts" });
    }
    let o = c.objective;
    if o.is_nan() {
        return Err(FitError::Num(NumError::NotANumber { op: OP_OBJECTIVE, index: Some(i) }));
    }
    if o.is_infinite() {
        return Err(FitError::Num(NumError::Infinite { op: OP_OBJECTIVE, index: Some(i) }));
    }
    if o < 0.0 {
        return Err(FitError::InvalidParams { field: "candidates.objective" });
    }
    let canonical = c.feature.canonical()?;
    let feature = match c.feature {
        // `canonical` copies these unchanged (no direction to normalise), with fallible
        // allocation; the other variants own no heap memory, so `clone` cannot allocate.
        FittedFeature::Polyline { .. } | FittedFeature::Spline { .. } => canonical,
        _ => c.feature.clone(),
    };
    let key = encode_feature(&feature)?;
    Ok(Norm { set: CandidateSet { contacts, objective: o + 0.0, feature }, key })
}

/// Builds the rocking report from every candidate a tangential fit found (module
/// documentation, rules 1 to 5). The result depends only on the multiset of candidate
/// values, not on their order.
pub fn rock_report(cands: &[CandidateSet], rock_tol: f64) -> Result<RockReport, FitError> {
    if !(rock_tol.is_finite() && (0.0..=1.0).contains(&rock_tol)) {
        return Err(FitError::InvalidParams { field: "rock_tol" });
    }
    let rock_tol = rock_tol + 0.0;
    if cands.is_empty() || cands.len() > MAX_CANDIDATES {
        return Err(FitError::InvalidParams { field: "candidates" });
    }

    let mut all = alloc::<Norm>(cands.len())?;
    for (i, c) in cands.iter().enumerate() {
        all.push(normalise(i, c)?);
    }

    // Rule 2: group by contacts; best objective, then smallest feature bytes, first.
    all.sort_unstable_by(|a, b| {
        a.set
            .contacts
            .cmp(&b.set.contacts)
            .then_with(|| a.set.objective.total_cmp(&b.set.objective))
            .then_with(|| a.key.cmp(&b.key))
    });
    // `heads[k]`: element k is the first (kept) one of its contact group. Within a group the
    // members are sorted by (objective, feature bytes), so a member with the head's objective
    // bits and other feature bytes is an unresolvable tie (rule 2). Groups are visited in
    // contact order, so the reported set is the first such set in that order.
    let mut heads = alloc::<bool>(all.len())?;
    let mut h = 0usize;
    for k in 0..all.len() {
        let head = k == 0 || all[k - 1].set.contacts != all[k].set.contacts;
        heads.push(head);
        if head {
            h = k;
            continue;
        }
        if all[k].set.objective.to_bits() == all[h].set.objective.to_bits() && all[k].key != all[h].key {
            let c = &all[h].set.contacts;
            return Err(FitError::AmbiguousCandidate {
                first_contact: c.first().copied().unwrap_or(0),
                contacts: c.len(),
            });
        }
    }
    let total_in = all.len();
    let mut distinct = alloc::<CandidateSet>(total_in)?;
    for (n, head) in all.into_iter().zip(heads) {
        if head {
            distinct.push(n.set);
        }
    }
    let merged = total_in - distinct.len();

    // Rule 4 order (also gives `best` as the first objective).
    distinct.sort_unstable_by(|a, b| a.objective.total_cmp(&b.objective).then_with(|| a.contacts.cmp(&b.contacts)));
    let best = match distinct.first() {
        Some(c) => c.objective,
        None => return Err(FitError::InvalidParams { field: "candidates" }),
    };

    // Rule 3.
    let bound = best + rock_tol * best;
    if !bound.is_finite() {
        return Err(FitError::Num(NumError::Infinite { op: OP_BOUND, index: None }));
    }
    let total = distinct.len();
    distinct.retain(|c| c.objective.total_cmp(&bound) != Ordering::Greater);
    let excluded = total - distinct.len();

    // Rule 5.
    let tie_break = match distinct.get(1) {
        None => TieBreak::Single,
        Some(second) if second.objective.to_bits() == best.to_bits() => TieBreak::ContactOrder,
        Some(_) => TieBreak::Objective,
    };
    let rocking = distinct.len() > 1;
    Ok(RockReport {
        candidates: distinct,
        selected: 0,
        rocking,
        rock_tol,
        tie_break,
        merged: count(merged),
        excluded: count(excluded),
    })
}

struct Out(Vec<u8>);

impl Out {
    fn s(&mut self, s: &str) -> Result<(), FitError> {
        self.b(s.as_bytes())
    }

    fn b(&mut self, b: &[u8]) -> Result<(), FitError> {
        self.0
            .try_reserve(b.len())
            .map_err(|_| FitError::Num(NumError::Domain { op: OP_ALLOC, detail: "allocation_failed" }))?;
        self.0.extend_from_slice(b);
        Ok(())
    }

    fn f(&mut self, x: f64) -> Result<(), FitError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bits = x.to_bits();
        let mut b = [b'"'; 18];
        for (k, slot) in b[1..17].iter_mut().enumerate() {
            *slot = HEX[((bits >> (60 - 4 * k)) & 0xF) as usize];
        }
        self.b(&b)
    }

    fn i(&mut self, v: u32) -> Result<(), FitError> {
        let mut b = [b'"'; 12];
        let mut pos = b.len() - 1;
        let mut x = v;
        loop {
            pos -= 1;
            b[pos] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        self.b(&b[pos - 1..])
    }
}

/// The canonical bytes of a report, for digests and the cross-target check
/// (REQ-KRN.ENV.TAN.ROCK-005). Compact JSON, ASCII, keys in this fixed order (shown wrapped):
///
/// ```text
/// {"rock_tol":<f>,"selected":<i>,"rocking":true|false,"tie_break":"<single|objective|contact_order>",
///  "merged":<i>,"excluded":<i>,
///  "candidates":[{"contacts":[<i>...],"objective":<f>,"feature":<feature>}...]}
/// ```
///
/// `<f>` and `<i>` and `<feature>` as in `contract::encode` (16 lowercase hex digits of the
/// binary64 bits; decimal digits in a JSON string; the canonical feature object).
pub fn encode_report(r: &RockReport) -> Result<Vec<u8>, FitError> {
    let mut o = Out(Vec::new());
    o.s("{\"rock_tol\":")?;
    o.f(r.rock_tol)?;
    o.s(",\"selected\":")?;
    o.i(r.selected)?;
    o.s(if r.rocking { ",\"rocking\":true" } else { ",\"rocking\":false" })?;
    o.s(",\"tie_break\":\"")?;
    o.s(r.tie_break.as_str())?;
    o.s("\",\"merged\":")?;
    o.i(r.merged)?;
    o.s(",\"excluded\":")?;
    o.i(r.excluded)?;
    o.s(",\"candidates\":[")?;
    for (k, c) in r.candidates.iter().enumerate() {
        o.s(if k == 0 { "{\"contacts\":[" } else { ",{\"contacts\":[" })?;
        for (j, &p) in c.contacts.iter().enumerate() {
            if j > 0 {
                o.s(",")?;
            }
            o.i(p)?;
        }
        o.s("],\"objective\":")?;
        o.f(c.objective)?;
        o.s(",\"feature\":")?;
        o.b(&encode_feature(&c.feature)?)?;
        o.s("}")?;
    }
    o.s("]}")?;
    Ok(o.0)
}
