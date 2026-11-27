//! Histogram binning kernel (spec node `RPT.STATS.TREND.HIST`, REQ-RPT.STATS.TREND.HIST-001
//! to -008).
//!
//! [`histogram`] turns a list of values and a named [`BinRule`] into bin edges and counts. The
//! edges depend only on the values and the rule, and the result records the rule ID with its
//! version ([`RuleId`]), so the same data set always gives the same histogram and a reader knows
//! how the bins were chosen.
//!
//! # Rules
//!
//! - `sturges@1` (the default): `k = ⌈log2 n⌉ + 1` bins, with `⌈log2 n⌉` computed with integer
//!   operations only as the bit length of `n − 1` (REQ-001). Sturges' rule: H. A. Sturges, 1926,
//!   Journal of the American Statistical Association 21(153) [unverified citation details].
//! - `fixed_count@1 {k}`: `k` bins, `1 ≤ k ≤ 1000`.
//! - `fixed_width@1 {width, anchor}`: bins of width `w` on the grid `a + j·w`.
//!
//! # Order of decisions
//!
//! 1. **Rule parameters** (`stats.bad_binning`): `fixed_count` with `k = 0` (`zero_bins`) or
//!    `k > 1000` (`too_many_bins`); `fixed_width` with a NaN or infinite width
//!    (`width_not_finite`), a width `≤ 0`, `-0.0` included (`width_not_positive`), or a NaN or
//!    infinite anchor (`anchor_not_finite`). A bad rule is refused before the data are looked
//!    at, so it never hides behind `INSUFFICIENT_DATA`.
//! 2. **No values**: `stats.insufficient_data` (REQ-006).
//! 3. **A NaN or infinite value**: `stats.non_finite{index}` naming the first one (NaN is
//!    always an error, KNP-1).
//! 4. **All values equal** (IEEE `==`, so `-0.0` and `+0.0` are equal): one bin `[v, v]` with
//!    count `n` and [`Bins::degenerate`] set, whatever the rule (REQ-005). `v` is the
//!    total-order maximum, so a mix of `-0.0` and `+0.0` gives `+0.0`.
//! 5. **Edges** (REQ-002, REQ-003), see below; a data set the rule cannot bin is refused with
//!    `stats.bad_binning` and no bins are returned.
//! 6. **Counts** (REQ-004): `x` is counted in bin `i` when `e_i ≤ x < e_(i+1)`, the last bin
//!    also holding `x = e_k`. The bin is found by binary search over the edges, never by
//!    dividing by the width. The counts sum to `n` (REQ-007); a value that fell outside every
//!    bin would be refused (`value_outside_edges`), never dropped, though the edge rules below
//!    make that unreachable.
//!
//! # Edges and pinned arithmetic (D0)
//!
//! Minimum and maximum are `fid-num` total-order operations (KRN.NUM.FN). Every edge is plain
//! IEEE 754 multiplication then addition, never fused (KNP-1):
//!
//! - `sturges@1`, `fixed_count@1`: `w = (max − min) / k`, `e_i = min + i·w` for `i < k` and
//!   `e_k = max` exactly. `max − min` overflowing is `range_not_finite`.
//! - `fixed_width@1`: `max − min` overflowing is `range_not_finite`; `(max − min) / w > 1001`
//!   is `too_many_bins` before any edge is made. `t = (min − a) / w`; a non-finite `t` or
//!   `|t| ≥ 2^52` is `anchor_too_far` (the grid index would leave the range where every
//!   integer is a double). `j0 = ⌊t⌋`, computed exactly by integer truncation and a one-step
//!   correction (`fid-num` has no floor function; for `|t| < 2^52` this is `floor` exactly).
//!   If the rounded quotient reached the next integer, so that `a + j0·w > min`, the literal
//!   `⌊t⌋` would leave `min` outside every bin; `j0 − 1` is used instead, and if that edge is
//!   still above `min` the data set is refused (`edges_not_increasing`). Edges are then
//!   `a + j·w` for `j = j0, j0 + 1, …` up to and including the first edge greater than `max`;
//!   an edge overflowing is `edge_not_finite` and more than 1000 bins is `too_many_bins`. When
//!   the rounded quotient fell just short of an integer, the first bin may be empty; that is
//!   REQ-003 as written and loses no value.
//! - Every rule: the edges must be strictly increasing. When `f64` cannot resolve the bin width
//!   at the data's magnitude (for example `min = 1e16`, `max = 1e16 + 2`, `k = 4`: `1e16 + 0.5`
//!   rounds back to `1e16`), two edges coincide and the call fails with
//!   `edges_not_increasing` rather than return an empty-width bin.
//!
//! Bin placement uses the IEEE comparisons `≤` and `<` on finite values, so `-0.0` and `+0.0`
//! fall in the same bin. With only basic operations, integer arithmetic and total-order
//! comparisons the result is bit-identical on every target (REQ-008).

use core::fmt;

use fid_num::f::{total_max, total_min};

use crate::StatsError;

/// The most bins a histogram may have (REQ-RPT.STATS.TREND.HIST-006).
pub const HISTOGRAM_MAX_BINS: usize = 1000;

/// `2^52`: `fixed_width@1` refuses a grid index `|t|` at or above it (`anchor_too_far`).
const ANCHOR_LIMIT: f64 = 4_503_599_627_370_496.0;

/// A binning rule. [`BinRule::default`] is `sturges@1`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BinRule {
    /// `sturges@1`: `⌈log2 n⌉ + 1` bins.
    #[default]
    Sturges,
    /// `fixed_count@1`: exactly `k` bins, `1 ≤ k ≤ 1000`.
    FixedCount {
        /// The number of bins.
        k: u32,
    },
    /// `fixed_width@1`: bins of width `width` on the grid `anchor + j·width`.
    FixedWidth {
        /// The bin width, positive and finite.
        width: f64,
        /// A point on the grid of edges, finite.
        anchor: f64,
    },
}

impl BinRule {
    /// The rule's ID.
    pub fn id(&self) -> RuleId {
        match self {
            BinRule::Sturges => RuleId::Sturges,
            BinRule::FixedCount { .. } => RuleId::FixedCount,
            BinRule::FixedWidth { .. } => RuleId::FixedWidth,
        }
    }
}

/// The ID and version of a binning rule, recorded on every [`Bins`] (REQ-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleId {
    /// `sturges@1`.
    Sturges,
    /// `fixed_count@1`.
    FixedCount,
    /// `fixed_width@1`.
    FixedWidth,
}

impl RuleId {
    /// `"sturges@1"`, `"fixed_count@1"` or `"fixed_width@1"`.
    pub fn as_str(self) -> &'static str {
        match self {
            RuleId::Sturges => "sturges@1",
            RuleId::FixedCount => "fixed_count@1",
            RuleId::FixedWidth => "fixed_width@1",
        }
    }

    /// The name without the version: `"sturges"`, `"fixed_count"` or `"fixed_width"`.
    pub fn name(self) -> &'static str {
        match self {
            RuleId::Sturges => "sturges",
            RuleId::FixedCount => "fixed_count",
            RuleId::FixedWidth => "fixed_width",
        }
    }

    /// The rule's major version; every rule is at version 1.
    pub fn version(self) -> u32 {
        1
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The result of [`histogram`]: `edges.len() == counts.len() + 1`, the edges strictly
/// increasing (or, for a degenerate histogram, the two equal edges `[v, v]`), and the counts
/// summing to the number of values.
#[derive(Debug, Clone, PartialEq)]
pub struct Bins {
    /// The rule that chose the bins.
    pub rule: RuleId,
    /// The bin edges `e_0 … e_k`.
    pub edges: Vec<f64>,
    /// The number of values in each bin.
    pub counts: Vec<u64>,
    /// `true` when every value was equal and the single bin is `[v, v]` (`DEGENERATE`).
    pub degenerate: bool,
}

impl Bins {
    /// The number of bins.
    pub fn bin_count(&self) -> usize {
        self.counts.len()
    }

    /// The sum of the counts: the number of values binned.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }
}

/// Why [`histogram`] refused to bin (`stats.bad_binning{reason}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinningReason {
    /// `fixed_count@1` with `k = 0`.
    ZeroBins,
    /// More than [`HISTOGRAM_MAX_BINS`] bins.
    TooManyBins,
    /// `fixed_width@1` with a NaN or infinite width.
    WidthNotFinite,
    /// `fixed_width@1` with a width that is not positive (`-0.0` included).
    WidthNotPositive,
    /// `fixed_width@1` with a NaN or infinite anchor.
    AnchorNotFinite,
    /// `max − min` overflows the `f64` range.
    RangeNotFinite,
    /// `fixed_width@1`: the data lie too far from the anchor, in widths, for an exact grid
    /// index.
    AnchorTooFar,
    /// An edge overflows the `f64` range.
    EdgeNotFinite,
    /// Two edges coincide: `f64` cannot resolve the bin width at this magnitude.
    EdgesNotIncreasing,
    /// A value fell outside every bin (unreachable with the edge rules; refused, never
    /// dropped).
    ValueOutsideEdges,
}

impl BinningReason {
    /// The reason code, for example `width_not_positive`.
    pub fn as_str(self) -> &'static str {
        match self {
            BinningReason::ZeroBins => "zero_bins",
            BinningReason::TooManyBins => "too_many_bins",
            BinningReason::WidthNotFinite => "width_not_finite",
            BinningReason::WidthNotPositive => "width_not_positive",
            BinningReason::AnchorNotFinite => "anchor_not_finite",
            BinningReason::RangeNotFinite => "range_not_finite",
            BinningReason::AnchorTooFar => "anchor_too_far",
            BinningReason::EdgeNotFinite => "edge_not_finite",
            BinningReason::EdgesNotIncreasing => "edges_not_increasing",
            BinningReason::ValueOutsideEdges => "value_outside_edges",
        }
    }
}

fn bad(reason: BinningReason) -> StatsError {
    StatsError::BadBinning { reason }
}

/// Bins `values` with `rule` (see the module documentation for the rules, the order of
/// decisions and the pinned arithmetic).
pub fn histogram(values: &[f64], rule: &BinRule) -> Result<Bins, StatsError> {
    check_rule(rule)?;
    if values.is_empty() {
        return Err(StatsError::InsufficientData);
    }
    if let Some(index) = values.iter().position(|x| !x.is_finite()) {
        return Err(StatsError::NonFinite { index });
    }
    let (lo, hi) = min_max(values)?;
    if lo == hi {
        return Ok(Bins { rule: rule.id(), edges: vec![hi, hi], counts: vec![values.len() as u64], degenerate: true });
    }
    let edges = match *rule {
        BinRule::Sturges => count_edges(lo, hi, sturges_k(values.len())),
        BinRule::FixedCount { k } => count_edges(lo, hi, k),
        BinRule::FixedWidth { width, anchor } => width_edges(lo, hi, width, anchor),
    }?;
    if edges.windows(2).any(|p| p[0] >= p[1]) {
        return Err(bad(BinningReason::EdgesNotIncreasing));
    }
    let counts = count(values, &edges)?;
    Ok(Bins { rule: rule.id(), edges, counts, degenerate: false })
}

fn check_rule(rule: &BinRule) -> Result<(), StatsError> {
    match *rule {
        BinRule::Sturges => Ok(()),
        BinRule::FixedCount { k } => {
            if k == 0 {
                Err(bad(BinningReason::ZeroBins))
            } else if k as usize > HISTOGRAM_MAX_BINS {
                Err(bad(BinningReason::TooManyBins))
            } else {
                Ok(())
            }
        }
        BinRule::FixedWidth { width, anchor } => {
            if !width.is_finite() {
                Err(bad(BinningReason::WidthNotFinite))
            } else if width <= 0.0 {
                Err(bad(BinningReason::WidthNotPositive))
            } else if !anchor.is_finite() {
                Err(bad(BinningReason::AnchorNotFinite))
            } else {
                Ok(())
            }
        }
    }
}

/// Total-order minimum and maximum of finite, non-empty `values`.
fn min_max(values: &[f64]) -> Result<(f64, f64), StatsError> {
    let mut lo = values[0];
    let mut hi = values[0];
    for (index, &x) in values.iter().enumerate().skip(1) {
        lo = total_min(lo, x).map_err(|_| StatsError::NonFinite { index })?;
        hi = total_max(hi, x).map_err(|_| StatsError::NonFinite { index })?;
    }
    Ok((lo, hi))
}

/// `⌈log2 n⌉ + 1` for `n ≥ 1`: the bit length of `n − 1`, plus one (REQ-001).
fn sturges_k(n: usize) -> u32 {
    let m = (n as u64).saturating_sub(1);
    (u64::BITS - m.leading_zeros()) + 1
}

/// `sturges@1` and `fixed_count@1` edges (REQ-002).
fn count_edges(lo: f64, hi: f64, k: u32) -> Result<Vec<f64>, StatsError> {
    let span = hi - lo;
    if !span.is_finite() {
        return Err(bad(BinningReason::RangeNotFinite));
    }
    let w = span / f64::from(k);
    let mut edges = Vec::with_capacity(k as usize + 1);
    for i in 0..k {
        edges.push(lo + f64::from(i) * w);
    }
    edges.push(hi);
    Ok(edges)
}

/// `⌊t⌋` for finite `|t| < 2^52`, exactly: truncation toward zero, then one step down for a
/// negative non-integer.
fn floor_index(t: f64) -> i64 {
    let j = t as i64;
    if (j as f64) > t {
        j - 1
    } else {
        j
    }
}

/// `fixed_width@1` edges (REQ-003).
fn width_edges(lo: f64, hi: f64, w: f64, a: f64) -> Result<Vec<f64>, StatsError> {
    let span = hi - lo;
    if !span.is_finite() {
        return Err(bad(BinningReason::RangeNotFinite));
    }
    if span / w > (HISTOGRAM_MAX_BINS + 1) as f64 {
        return Err(bad(BinningReason::TooManyBins));
    }
    let t = (lo - a) / w;
    if !t.is_finite() || t.abs() >= ANCHOR_LIMIT {
        return Err(bad(BinningReason::AnchorTooFar));
    }
    let edge = |j: i64| a + (j as f64) * w;
    let mut j = floor_index(t);
    if edge(j) > lo {
        j -= 1;
        if edge(j) > lo {
            return Err(bad(BinningReason::EdgesNotIncreasing));
        }
    }
    let first = edge(j);
    if !first.is_finite() {
        return Err(bad(BinningReason::EdgeNotFinite));
    }
    let mut edges = Vec::with_capacity(16);
    edges.push(first);
    while edges.last().is_some_and(|&e| e <= hi) {
        if edges.len() == HISTOGRAM_MAX_BINS + 1 {
            return Err(bad(BinningReason::TooManyBins));
        }
        j += 1;
        let e = edge(j);
        if !e.is_finite() {
            return Err(bad(BinningReason::EdgeNotFinite));
        }
        edges.push(e);
    }
    Ok(edges)
}

/// Counts per bin by binary search over strictly increasing `edges` (REQ-004).
fn count(values: &[f64], edges: &[f64]) -> Result<Vec<u64>, StatsError> {
    let bins = edges.len() - 1;
    let last = edges[bins];
    let mut counts = vec![0u64; bins];
    for &x in values {
        // The number of edges `≤ x`: bin `p − 1` holds `x`, except that `x = e_k` closes the
        // last bin.
        let p = edges.partition_point(|&e| e <= x);
        let i = if p == edges.len() && x == last { bins } else { p };
        if i == 0 || i > bins {
            return Err(bad(BinningReason::ValueOutsideEdges));
        }
        counts[i - 1] += 1;
    }
    Ok(counts)
}
