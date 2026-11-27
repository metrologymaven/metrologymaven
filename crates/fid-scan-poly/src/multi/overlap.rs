//! Per-point overlap detection between registered scans. Spec node `SCAN.POLY.MULTI.OVLP`.
//!
//! [`detect_overlap`] marks, for every point of every scan, which other scans measured the
//! same surface nearby: scan `j` overlaps point `p` of scan `i != j` when `j` has a point
//! within the overlap distance `d` of `p` whose normal faces the same way as `p`'s
//! (REQ-SCAN.POLY.MULTI.OVLP-001), or any point within `d` when `p` has no valid normal
//! (REQ-002). The result is the input of overlap reduction (`SCAN.POLY.MULTI.REDUCE`), which
//! decides which points to keep; nothing here moves, drops or invents a point. Every
//! value-affecting choice is pinned below; the independent reference
//! `fixtures/scan/poly/make_ovlp_fixtures.py` repeats it by brute force over all point pairs.
//!
//! # Scans
//!
//! A scan is a [`ScanView`]: its frame, its point count, random access to a point's position
//! and normal (`None` when the point has no valid normal, SCAN.CLOUD.NORM), and a radius query
//! with the GEO.INDEX.KDTREE contract (every point with pinned `dist2 <= r*r`). [`MemoryScan`]
//! is the in-memory view over a GEO.TYPES [`PointCloud`] and its validity flags. A chunked
//! cloud is viewed through the same trait over a GEO.OOC `ChunkedIndex`, whose queries are
//! identical to the in-memory index's (REQ-GEO.OOC.QUERY-001); that adapter lives above the
//! kernel layer (this crate cannot depend on `fid-geo-ooc`), and the identity of the results
//! is tested in `crates/fid-geo-ooc/tests/scan_overlap.rs` (AC-3).
//!
//! # Input checks (first failure wins)
//!
//! 1. `d` finite with [`MIN_D`] = 2^-100 m `<= d <=` [`MAX_D`] = 2^100 m (`d`);
//! 2. at least two scans, at most `u32::MAX` (`scans`);
//! 3. every scan in the frame of scan 0 (`scans.frame`): overlap between scans in different
//!    frames is meaningless, so it is refused rather than computed;
//! 4. per scan in order, per point in index order: the point count addressable on this target
//!    (`scan.point_count`), the position finite with every coordinate at most `COORD_LIMIT`
//!    = 2^500 in magnitude (`scan.position`), and a present normal a unit normal by GEO.TYPES
//!    `is_unit_normal` (`scan.normal`).
//!
//! Each is [`PolyError::InvalidParameter`] (`scan.poly.invalid_parameter`). Check 4 covers
//! every point before any query runs. Errors of a view (its own storage, its index) are
//! passed through as it reports them.
//!
//! # One point (`p` of scan `i`, position `x`, normal `n` or none)
//!
//! For each other scan `j` in ascending order:
//!
//! 1. Prefilter: a scan without points is skipped. Otherwise, with `lo`, `hi` the per-axis
//!    minimum and maximum of scan `j`'s positions (numeric comparisons) and the gap
//!    `g_k = lo_k - x_k` when `x_k < lo_k`, `x_k - hi_k` when `x_k > hi_k`, else 0, the lower
//!    bound `(g0*g0 + g1*g1) + g2*g2` is compared with `d*d` (rounded once). A bound above
//!    `d*d` skips the scan. This is exact, not a tolerance: for every point `q` of `j`,
//!    `|q_k - x_k| >= g_k` and rounding is monotone, so the pinned `dist2(q, x)` is at least
//!    the bound; no point the query would accept is skipped (the GEO.INDEX pruning argument).
//! 2. Query: scan `j`'s radius query at `x` with radius `d`. For every hit (all of them,
//!    whatever their order) the kernel re-reads the point, checks it again as in input
//!    check 4 and recomputes `dist2 = (dx*dx + dy*dy) + dz*dz` itself; a hit that no longer
//!    passes check 4, lies beyond `d*d` or has an index outside the scan is
//!    [`PolyError::Inconsistent`] (a broken view is refused, never trusted). The query point
//!    itself is re-read and re-checked the same way.
//! 3. Compatibility: with a normal `n`, a hit `q` counts when it has a normal `m` and
//!    `(n0*m0 + n1*m1) + n2*m2 >` [`OVERLAP_NORMAL_COS`] = 0.5, the angle strictly below 60
//!    degrees (a hit without a valid normal does not count: REQ-001 needs its normal); without
//!    a normal every hit counts (REQ-002). Any counting hit marks `j`; the result does not
//!    depend on the hit order.
//!
//! The point's [`PointStatus`] says why its set is what it is, so an empty set always carries
//! a reason: [`PointStatus::Overlapping`] (some scan marked),
//! [`PointStatus::NormalsIncompatible`] (some scan has a point within `d`, but none counted),
//! else [`PointStatus::NothingWithinD`]. Its [`MatchRule`] says which requirement applied.
//!
//! # Output
//!
//! Per scan, per point in index order: the ascending set of overlapping scans (one 64-bit
//! mask per point when there are at most [`MAX_BITSET_SCANS`] = 64 scans, else a sorted list
//! per point, [`OverlapSets`]), the status and the rule; per scan a [`ScanSummary`]; and the
//! per-pair counts ([`PairCount`]): for each ordered pair `(i, j)`, the number of points of
//! scan `i` that scan `j` overlaps. Overlap is not symmetric (point densities and normals
//! differ), so both directions are reported. Pairs with no overlapping point are not listed;
//! [`OverlapMap::pair_count`] returns 0 for them. The map records `d` with its result.
//!
//! # Determinism (D0)
//!
//! Each point's result is a pure function of the scans and `d`; points and scans are visited
//! in index order on the calling thread (no threads, so no thread-count dependence,
//! REQ-SCAN.POLY.MULTI.OVLP-003), there is no fused multiply-add, no hash container and no
//! `f64` min or max, and nothing can be NaN after the input checks.

use fid_geo_index::{dist2, Neighbour, PointIndex, COORD_LIMIT};
use fid_geo_types::{is_unit_normal, FrameRef, PointCloud};

use crate::prep::PolyError;

/// The smallest accepted overlap distance `d`, 2^-100 m.
pub const MIN_D: f64 = f64::from_bits((1023 - 100) << 52);
/// The largest accepted overlap distance `d`, 2^100 m.
pub const MAX_D: f64 = f64::from_bits((1023 + 100) << 52);
/// The normal-compatibility threshold: a normal dot product strictly above `cos(60 degrees)`
/// = 0.5 (exact) is an angle strictly below 60 degrees.
pub const OVERLAP_NORMAL_COS: f64 = 0.5;
/// The most scans whose per-point sets are 64-bit masks; above it they are sorted lists.
pub const MAX_BITSET_SCANS: usize = 64;

fn invalid(name: &'static str, reason: &'static str) -> PolyError {
    PolyError::InvalidParameter { name, reason }
}

/// One point of a scan as a view returns it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanPoint {
    /// The position in metres, in the scan's frame.
    pub position: [f64; 3],
    /// The unit normal, or `None` when the point has no valid normal.
    pub normal: Option<[f64; 3]>,
}

/// A registered scan as the overlap detection reads it (module documentation, Scans).
pub trait ScanView {
    /// The frame the positions are in.
    fn frame(&self) -> FrameRef;
    /// The number of points.
    fn point_count(&self) -> u64;
    /// Point `i` (`i < point_count()`).
    fn point(&self, i: u64) -> Result<ScanPoint, PolyError>;
    /// Every point `q` with pinned `dist2(q, x) <= r*r` (`r*r` rounded once), as GEO.INDEX
    /// `radius` returns them.
    fn radius(&self, x: [f64; 3], r: f64) -> Result<Vec<Neighbour>, PolyError>;
}

/// The in-memory [`ScanView`] over a point cloud and its validity flags, with its GEO.INDEX
/// k-d tree.
#[derive(Debug, Clone)]
pub struct MemoryScan<'a> {
    cloud: &'a PointCloud,
    valid: &'a [bool],
    index: PointIndex<'a>,
}

impl<'a> MemoryScan<'a> {
    /// Views `cloud`; `valid[i]` says whether point `i` has a valid normal (SCAN.CLOUD.NORM).
    /// Refuses a flag count other than the point count (`valid`) and a valid flag on a cloud
    /// without normals (`cloud.normal`); the k-d tree's refusals are [`PolyError::Index`].
    pub fn new(cloud: &'a PointCloud, valid: &'a [bool]) -> Result<MemoryScan<'a>, PolyError> {
        if u64::try_from(valid.len()).ok() != Some(cloud.len()) {
            return Err(invalid("valid", "not one flag per cloud point"));
        }
        if cloud.attributes().normal.is_none() && valid.iter().any(|v| *v) {
            return Err(invalid("cloud.normal", "a point is flagged valid but the cloud has no normals"));
        }
        let index = PointIndex::build(cloud.buffer(), 1)?;
        Ok(MemoryScan { cloud, valid, index })
    }
}

impl ScanView for MemoryScan<'_> {
    fn frame(&self) -> FrameRef {
        self.cloud.frame()
    }

    fn point_count(&self) -> u64 {
        self.cloud.len()
    }

    fn point(&self, i: u64) -> Result<ScanPoint, PolyError> {
        let position = self.cloud.point(i).ok_or(PolyError::Inconsistent)?;
        let k = usize::try_from(i).map_err(|_| PolyError::Inconsistent)?;
        let normal = if *self.valid.get(k).ok_or(PolyError::Inconsistent)? {
            Some(self.cloud.attributes().normal_at(i).ok_or(PolyError::Inconsistent)?)
        } else {
            None
        };
        Ok(ScanPoint { position, normal })
    }

    fn radius(&self, x: [f64; 3], r: f64) -> Result<Vec<Neighbour>, PolyError> {
        Ok(self.index.radius(x, r)?)
    }
}

/// Which requirement decided a point's set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchRule {
    /// The point has a normal: a neighbour counts when its normal is within 60 degrees
    /// (REQ-SCAN.POLY.MULTI.OVLP-001).
    Normal,
    /// The point has no valid normal: any neighbour within `d` counts (REQ-002).
    AnyPoint,
}

impl MatchRule {
    /// A stable machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            MatchRule::Normal => "normal",
            MatchRule::AnyPoint => "any_point",
        }
    }
}

/// Why a point's set is what it is (module documentation, One point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointStatus {
    /// At least one other scan overlaps the point.
    Overlapping,
    /// No other scan has a point within `d`.
    NothingWithinD,
    /// Some other scan has a point within `d`, but none with a compatible normal.
    NormalsIncompatible,
}

impl PointStatus {
    /// A stable machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            PointStatus::Overlapping => "overlapping",
            PointStatus::NothingWithinD => "nothing_within_d",
            PointStatus::NormalsIncompatible => "normals_incompatible",
        }
    }
}

/// The per-point sets of overlapping scans of one scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlapSets {
    /// At most [`MAX_BITSET_SCANS`] scans: bit `j` of entry `k` is set when scan `j` overlaps
    /// point `k`.
    Bits(Vec<u64>),
    /// More scans: point `k`'s scans are `scans[offsets[k] .. offsets[k + 1]]`, ascending;
    /// `offsets` has one entry more than there are points and starts at 0.
    Lists {
        /// Start of each point's run, plus the total at the end.
        offsets: Vec<u64>,
        /// The concatenated runs.
        scans: Vec<u32>,
    },
}

impl OverlapSets {
    /// The number of points.
    pub fn len(&self) -> usize {
        match self {
            OverlapSets::Bits(b) => b.len(),
            OverlapSets::Lists { offsets, .. } => offsets.len().saturating_sub(1),
        }
    }

    /// Whether the scan has no points.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The ascending scans overlapping point `k`, or `None` when there is no point `k`.
    pub fn scans_of(&self, k: usize) -> Option<Vec<u32>> {
        match self {
            OverlapSets::Bits(b) => {
                let mask = *b.get(k)?;
                Some((0u32..64).filter(|&j| (mask >> j) & 1 == 1).collect())
            }
            OverlapSets::Lists { offsets, scans } => {
                let lo = usize::try_from(*offsets.get(k)?).ok()?;
                let hi = usize::try_from(*offsets.get(k.checked_add(1)?)?).ok()?;
                scans.get(lo..hi).map(<[u32]>::to_vec)
            }
        }
    }

    /// Whether scan `j` overlaps point `k`, or `None` when there is no point `k`.
    pub fn contains(&self, k: usize, j: u32) -> Option<bool> {
        match self {
            OverlapSets::Bits(b) => {
                let mask = *b.get(k)?;
                Some(j < 64 && (mask >> j) & 1 == 1)
            }
            OverlapSets::Lists { .. } => Some(self.scans_of(k)?.binary_search(&j).is_ok()),
        }
    }
}

/// Point counts of one scan by outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanSummary {
    /// Points of the scan.
    pub points: u64,
    /// Points with status [`PointStatus::Overlapping`].
    pub overlapping: u64,
    /// Points with status [`PointStatus::NothingWithinD`].
    pub nothing_within_d: u64,
    /// Points with status [`PointStatus::NormalsIncompatible`].
    pub normals_incompatible: u64,
    /// Points without a valid normal (rule [`MatchRule::AnyPoint`]).
    pub without_normal: u64,
}

/// The result for one scan.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanOverlap {
    /// The per-point sets of overlapping scans, in point index order.
    pub sets: OverlapSets,
    /// Each point's status, in point index order.
    pub status: Vec<PointStatus>,
    /// Each point's rule, in point index order.
    pub rule: Vec<MatchRule>,
    /// The counts by outcome.
    pub summary: ScanSummary,
}

/// The number of points of `scan` that scan `other` overlaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PairCount {
    /// The scan whose points are counted.
    pub scan: u32,
    /// The overlapping scan.
    pub other: u32,
    /// The count (never 0 in [`OverlapMap::pairs`]).
    pub points: u64,
}

/// The result of [`detect_overlap`].
#[derive(Debug, Clone, PartialEq)]
pub struct OverlapMap {
    /// The overlap distance in metres the map was computed with.
    pub d: f64,
    /// One entry per input scan, in input order.
    pub scans: Vec<ScanOverlap>,
    /// The non-zero ordered pair counts, ascending by `(scan, other)`.
    pub pairs: Vec<PairCount>,
}

impl OverlapMap {
    /// The number of points of `scan` that `other` overlaps (0 for an unlisted pair), or
    /// `None` when either is not a scan of the map or they are the same scan.
    pub fn pair_count(&self, scan: u32, other: u32) -> Option<u64> {
        let s = self.scans.len();
        if scan == other || usize::try_from(scan).ok()? >= s || usize::try_from(other).ok()? >= s {
            return None;
        }
        Some(match self.pairs.binary_search_by(|p| (p.scan, p.other).cmp(&(scan, other))) {
            Ok(k) => self.pairs.get(k)?.points,
            Err(_) => 0,
        })
    }
}

/// The per-axis bounds of a scan's positions (`None` for a scan without points).
type Bounds = Option<([f64; 3], [f64; 3])>;

/// The position check of input check 4.
fn position_ok(p: &ScanPoint) -> bool {
    p.position.iter().all(|c| c.is_finite() && c.abs() <= COORD_LIMIT)
}

/// The normal check of input check 4.
fn normal_ok(p: &ScanPoint) -> bool {
    p.normal.is_none_or(is_unit_normal)
}

/// Re-reads point `i` of a view after the input checks: a point that no longer passes them
/// is a broken view, refused as [`PolyError::Inconsistent`].
fn reread(view: &dyn ScanView, i: u64) -> Result<ScanPoint, PolyError> {
    let p = view.point(i)?;
    if position_ok(&p) && normal_ok(&p) {
        Ok(p)
    } else {
        Err(PolyError::Inconsistent)
    }
}

/// Checks 4 of the module documentation for one scan; returns its point count and bounds.
fn check_scan(view: &dyn ScanView) -> Result<(usize, Bounds), PolyError> {
    let count = view.point_count();
    let n =
        usize::try_from(count).map_err(|_| invalid("scan.point_count", "more points than this target addresses"))?;
    let mut bounds: Bounds = None;
    for i in 0..count {
        let p = view.point(i)?;
        if !position_ok(&p) {
            return Err(invalid("scan.position", "a coordinate is not finite or exceeds 2^500 m"));
        }
        if !normal_ok(&p) {
            return Err(invalid("scan.normal", "a present normal is not a unit normal"));
        }
        bounds = Some(match bounds {
            None => (p.position, p.position),
            Some((mut lo, mut hi)) => {
                for ((l, h), &c) in lo.iter_mut().zip(hi.iter_mut()).zip(p.position.iter()) {
                    if c < *l {
                        *l = c;
                    }
                    if c > *h {
                        *h = c;
                    }
                }
                (lo, hi)
            }
        });
    }
    Ok((n, bounds))
}

/// The exact lower bound of `dist2` from `x` to any point inside `lo ..= hi` (step 1).
fn lower_bound(x: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> f64 {
    let mut g = [0.0f64; 3];
    for (((gk, &xk), &lk), &hk) in g.iter_mut().zip(x.iter()).zip(lo.iter()).zip(hi.iter()) {
        if xk < lk {
            *gk = lk - xk;
        } else if xk > hk {
            *gk = xk - hk;
        }
    }
    (g[0] * g[0] + g[1] * g[1]) + g[2] * g[2]
}

/// Whether a hit with normal `m` counts for a point with normal `n` (step 3).
fn compatible(n: Option<[f64; 3]>, m: Option<[f64; 3]>) -> bool {
    match (n, m) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(n), Some(m)) => (n[0] * m[0] + n[1] * m[1]) + n[2] * m[2] > OVERLAP_NORMAL_COS,
    }
}

/// What scan `j` contributes to one point: nothing within `d`, only incompatible points, or
/// overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contribution {
    Nothing,
    Incompatible,
    Overlap,
}

/// Steps 2 and 3 for one point against one scan.
fn against(view: &dyn ScanView, n_points: usize, p: &ScanPoint, d: f64, d2: f64) -> Result<Contribution, PolyError> {
    let hits = view.radius(p.position, d)?;
    let mut seen = Contribution::Nothing;
    // Every hit is checked before the result is returned, so a broken view is refused
    // whatever order it returns its hits in.
    for hit in &hits {
        let k = usize::try_from(hit.index).map_err(|_| PolyError::Inconsistent)?;
        if k >= n_points {
            return Err(PolyError::Inconsistent);
        }
        // Re-validated, so dist2 is finite and the comparison below cannot miss a NaN.
        let q = reread(view, hit.index)?;
        if dist2(p.position, q.position) > d2 {
            return Err(PolyError::Inconsistent);
        }
        if compatible(p.normal, q.normal) {
            seen = Contribution::Overlap;
        } else if seen == Contribution::Nothing {
            seen = Contribution::Incompatible;
        }
    }
    Ok(seen)
}

/// Collects one scan's per-point sets in the representation chosen by the scan count.
enum SetsBuilder {
    Bits(Vec<u64>),
    Lists { offsets: Vec<u64>, scans: Vec<u32> },
}

impl SetsBuilder {
    fn new(n_scans: usize, n_points: usize) -> Result<SetsBuilder, PolyError> {
        if n_scans <= MAX_BITSET_SCANS {
            let mut b = Vec::new();
            b.try_reserve_exact(n_points).map_err(|_| PolyError::Allocation)?;
            Ok(SetsBuilder::Bits(b))
        } else {
            let mut offsets = Vec::new();
            offsets
                .try_reserve_exact(n_points.checked_add(1).ok_or(PolyError::Allocation)?)
                .map_err(|_| PolyError::Allocation)?;
            offsets.push(0);
            Ok(SetsBuilder::Lists { offsets, scans: Vec::new() })
        }
    }

    /// Appends the next point's ascending set.
    fn push(&mut self, set: &[u32]) -> Result<(), PolyError> {
        match self {
            SetsBuilder::Bits(b) => {
                let mut mask = 0u64;
                for &j in set {
                    if j >= 64 {
                        return Err(PolyError::Inconsistent);
                    }
                    mask |= 1u64 << j;
                }
                b.push(mask);
            }
            SetsBuilder::Lists { offsets, scans } => {
                scans.try_reserve(set.len()).map_err(|_| PolyError::Allocation)?;
                scans.extend_from_slice(set);
                offsets.push(u64::try_from(scans.len()).map_err(|_| PolyError::Inconsistent)?);
            }
        }
        Ok(())
    }

    fn finish(self) -> OverlapSets {
        match self {
            SetsBuilder::Bits(b) => OverlapSets::Bits(b),
            SetsBuilder::Lists { offsets, scans } => OverlapSets::Lists { offsets, scans },
        }
    }
}

/// Marks, for every point of every scan, the other scans that overlap it (module
/// documentation). `d` is the overlap distance in metres.
pub fn detect_overlap(scans: &[&dyn ScanView], d: f64) -> Result<OverlapMap, PolyError> {
    if !(d.is_finite() && (MIN_D..=MAX_D).contains(&d)) {
        return Err(invalid("d", "not a finite distance in [2^-100, 2^100] m"));
    }
    if scans.len() < 2 {
        return Err(invalid("scans", "fewer than two scans"));
    }
    if u32::try_from(scans.len()).is_err() {
        return Err(invalid("scans", "more than 2^32 - 1 scans"));
    }
    let first = scans.first().ok_or(PolyError::Inconsistent)?;
    let frame = first.frame();
    if scans.iter().any(|s| s.frame() != frame) {
        return Err(invalid("scans.frame", "the scans are not all in the same frame"));
    }
    let mut info: Vec<(usize, Bounds)> = Vec::new();
    info.try_reserve_exact(scans.len()).map_err(|_| PolyError::Allocation)?;
    for s in scans {
        info.push(check_scan(*s)?);
    }

    let d2 = d * d;
    let mut out = Vec::new();
    out.try_reserve_exact(scans.len()).map_err(|_| PolyError::Allocation)?;
    let mut pairs: Vec<PairCount> = Vec::new();
    let mut per_pair: Vec<u64> = Vec::new();
    per_pair.try_reserve_exact(scans.len()).map_err(|_| PolyError::Allocation)?;
    per_pair.resize(scans.len(), 0);
    let mut set: Vec<u32> = Vec::new();
    set.try_reserve_exact(scans.len()).map_err(|_| PolyError::Allocation)?;

    for (i, (view, &(n_i, _))) in scans.iter().zip(info.iter()).enumerate() {
        let scan_id = u32::try_from(i).map_err(|_| PolyError::Inconsistent)?;
        per_pair.iter_mut().for_each(|c| *c = 0);
        let mut sets = SetsBuilder::new(scans.len(), n_i)?;
        let mut status = Vec::new();
        status.try_reserve_exact(n_i).map_err(|_| PolyError::Allocation)?;
        let mut rule = Vec::new();
        rule.try_reserve_exact(n_i).map_err(|_| PolyError::Allocation)?;
        let mut summary =
            ScanSummary { points: u64::try_from(n_i).map_err(|_| PolyError::Inconsistent)?, ..ScanSummary::default() };

        for k in 0..summary.points {
            let p = reread(*view, k)?;
            set.clear();
            let mut any_within = false;
            for (j, (other, &(n_j, bounds))) in scans.iter().zip(info.iter()).enumerate() {
                if j == i {
                    continue;
                }
                let Some((lo, hi)) = bounds else { continue };
                if lower_bound(p.position, lo, hi) > d2 {
                    continue;
                }
                match against(*other, n_j, &p, d, d2)? {
                    Contribution::Nothing => {}
                    Contribution::Incompatible => any_within = true,
                    Contribution::Overlap => {
                        any_within = true;
                        set.push(u32::try_from(j).map_err(|_| PolyError::Inconsistent)?);
                        let c = per_pair.get_mut(j).ok_or(PolyError::Inconsistent)?;
                        *c = c.checked_add(1).ok_or(PolyError::Inconsistent)?;
                    }
                }
            }
            let st = if !set.is_empty() {
                summary.overlapping += 1;
                PointStatus::Overlapping
            } else if any_within {
                summary.normals_incompatible += 1;
                PointStatus::NormalsIncompatible
            } else {
                summary.nothing_within_d += 1;
                PointStatus::NothingWithinD
            };
            let r = if p.normal.is_some() {
                MatchRule::Normal
            } else {
                summary.without_normal += 1;
                MatchRule::AnyPoint
            };
            sets.push(&set)?;
            status.push(st);
            rule.push(r);
        }

        for (j, &c) in per_pair.iter().enumerate() {
            if c > 0 {
                pairs.try_reserve(1).map_err(|_| PolyError::Allocation)?;
                pairs.push(PairCount {
                    scan: scan_id,
                    other: u32::try_from(j).map_err(|_| PolyError::Inconsistent)?,
                    points: c,
                });
            }
        }
        out.push(ScanOverlap { sets: sets.finish(), status, rule, summary });
    }
    Ok(OverlapMap { d, scans: out, pairs })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn constants() {
        assert_eq!(MIN_D, 2f64.powi(-100));
        assert_eq!(MAX_D, 2f64.powi(100));
    }

    #[test]
    fn lower_bound_is_zero_inside_and_exact_outside() {
        assert_eq!(lower_bound([0.5, 0.5, 0.5], [0.0; 3], [1.0; 3]), 0.0);
        assert_eq!(lower_bound([-1.0, 2.0, 0.5], [0.0; 3], [1.0; 3]), 2.0);
    }

    #[test]
    fn compatibility_rules() {
        let z = Some([0.0, 0.0, 1.0]);
        assert!(compatible(None, None));
        assert!(compatible(None, Some([0.0, 0.0, -1.0])));
        assert!(!compatible(z, None));
        assert!(!compatible(z, Some([0.75f64.sqrt(), 0.0, 0.5])));
        assert!(compatible(z, z));
    }

    #[test]
    fn sets_round_trip() {
        let mut b = SetsBuilder::new(65, 2).unwrap();
        b.push(&[1, 64]).unwrap();
        b.push(&[]).unwrap();
        let s = b.finish();
        assert_eq!(s.len(), 2);
        assert_eq!(s.scans_of(0).unwrap(), vec![1, 64]);
        assert_eq!(s.scans_of(1).unwrap(), Vec::<u32>::new());
        assert_eq!(s.contains(0, 64), Some(true));
        assert_eq!(s.contains(2, 0), None);
        let mut b = SetsBuilder::new(64, 1).unwrap();
        b.push(&[0, 63]).unwrap();
        let s = b.finish();
        assert_eq!(s.scans_of(0).unwrap(), vec![0, 63]);
        assert_eq!(s.contains(0, 64), Some(false));
    }
}
