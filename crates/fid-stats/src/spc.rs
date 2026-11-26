//! Shewhart control-chart constants table `spc.constants.normal@1.0.0` (spec node
//! `RPT.STATS.SPC.CONST`, REQ-RPT.STATS.SPC.CONST-001 to -007).
//!
//! For each subgroup size n = 2..25 the table holds the base constants of the normal
//! distribution d2 (mean range), d3 (standard deviation of the range) and c4 (bias of the sample
//! standard deviation), the chart factors A2, A3, B3, B4, D3, D4 that follow from them, and, in
//! the n = 2 row only, E2 for individuals charts (REQ-001). The released table is the canonical
//! JSON file [`SPC_TABLE_JSON`], embedded at build time; its SHA-256 is pinned in
//! `fixtures/rpt/stats/spc_constants_v1.sha256`, so a released version's values cannot change
//! without a new version (REQ-007).
//!
//! # Where the numbers come from
//!
//! No published table was copied. The independent reference
//! `fixtures/rpt/stats/make_spc_constants.py` integrates the range distribution of n standard
//! normals for d2 and d3, evaluates the gamma-function ratio for c4, and checks its rounded
//! results against the seed values in the leaf. The authoritative source and edition are an
//! expert choice that has not been made, so **every row is `UNVERIFIED`** and its citation says
//! so (REQ-002). Every lookup returns that status with the row, and every limit computed from it
//! must carry it on: a value from an unreviewed row is never presented as settled.
//!
//! # Rounding
//!
//! d2 and d3 are stored to 3 decimals, c4 to 4 decimals, the chart factors to 3 decimals. The
//! factors are computed from the *stored* base constants with the formulas of REQ-005 and then
//! rounded, so every stored factor agrees with its formula within half a unit in its last digit
//! (AC-1). A factor rounded from unrounded base constants can differ from it in the last digit
//! (for example A2 at n = 2 is 1.881 here, from 3 / (1.128 √2)).
//!
//! # Determinism (D0)
//!
//! [`constants`] is a lookup: no arithmetic, no I/O, no allocation.

use core::fmt;

use crate::cap::StatsError;
use crate::spc_table::ROWS;

/// The table ID (REQ-003).
pub const SPC_TABLE_ID: &str = "spc.constants.normal";

/// The table version (REQ-003, REQ-007). A change to any stored value needs a new version.
pub const SPC_TABLE_VERSION: &str = "1.0.0";

/// The smallest subgroup size the table covers.
pub const SPC_MIN_N: u8 = 2;

/// The largest subgroup size the table covers.
pub const SPC_MAX_N: u8 = 25;

/// The released table, canonical JSON (sorted keys, no whitespace, every stored value a decimal
/// string), as embedded at build time (AC-4).
pub const SPC_TABLE_JSON: &[u8] = include_bytes!("../data/spc_constants_normal_v1.json");

/// One stored constant: the decimal text the table holds and the nearest `f64` to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoredValue {
    text: &'static str,
    value: f64,
}

impl StoredValue {
    /// The stored decimal, exactly as released (for example `"0.9400"`).
    pub fn text(&self) -> &'static str {
        self.text
    }

    /// The nearest double to [`StoredValue::text`].
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The number of decimals stored (the rounding of this value).
    pub fn decimals(&self) -> usize {
        self.text.split_once('.').map_or(0, |(_, frac)| frac.len())
    }
}

/// A stored constant (used by the generated table).
pub(crate) const fn sv(text: &'static str, value: f64) -> StoredValue {
    StoredValue { text, value }
}

/// Where a row's values are cited from (REQ-002). Designators only, never copied text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Citation {
    /// Source designator or published reference; in version 1.0.0 it states that no source has
    /// been chosen and names the computation the values come from.
    pub source: &'static str,
    /// Edition of the source.
    pub edition: &'static str,
    /// Table number in the source.
    pub table: &'static str,
}

/// The review status of a row (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    /// No expert has checked the row against a chosen source. Results computed from it carry
    /// this status.
    Unverified,
    /// An expert has reviewed the row.
    ExpertReviewed,
}

impl ReviewStatus {
    /// `"UNVERIFIED"` or `"EXPERT_REVIEWED"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewStatus::Unverified => "UNVERIFIED",
            ReviewStatus::ExpertReviewed => "EXPERT_REVIEWED",
        }
    }
}

impl fmt::Display for ReviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One row of the table (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpcRow {
    /// Subgroup size.
    pub n: u8,
    /// Mean of the range of n standard normals.
    pub d2: StoredValue,
    /// Standard deviation of that range.
    pub d3: StoredValue,
    /// Bias factor of the sample standard deviation.
    pub c4: StoredValue,
    /// X-bar chart factor from the mean range, 3 / (d2 √n).
    pub a2: StoredValue,
    /// X-bar chart factor from the mean standard deviation, 3 / (c4 √n).
    pub a3: StoredValue,
    /// Lower s-chart factor, max(0, 1 − 3 √(1 − c4²) / c4).
    pub b3: StoredValue,
    /// Upper s-chart factor, 1 + 3 √(1 − c4²) / c4.
    pub b4: StoredValue,
    /// Lower range-chart factor D3, max(0, 1 − 3 d3 / d2) (named apart from the base d3).
    pub d3_factor: StoredValue,
    /// Upper range-chart factor, 1 + 3 d3 / d2.
    pub d4: StoredValue,
    /// Individuals-chart factor E2 = 3 / d2(2); present in the n = 2 row only.
    pub e2: Option<StoredValue>,
    /// The row's citation.
    pub citation: Citation,
    /// The row's review status.
    pub review: ReviewStatus,
}

/// The result of [`constants`]: a row with the table it came from (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpcConstants {
    /// [`SPC_TABLE_ID`].
    pub table_id: &'static str,
    /// [`SPC_TABLE_VERSION`].
    pub table_version: &'static str,
    /// The row's review status, repeated here so no caller can miss it.
    pub review: ReviewStatus,
    /// The row.
    pub row: &'static SpcRow,
}

/// Every row of the table, n = 2..25 in order.
pub fn spc_table() -> &'static [SpcRow] {
    &ROWS
}

/// The constants for subgroup size `n` (REQ-003), or `stats.subgroup_size_unsupported` when `n`
/// is outside 2..=25 (REQ-004).
pub fn constants(n: u8) -> Result<SpcConstants, StatsError> {
    if !(SPC_MIN_N..=SPC_MAX_N).contains(&n) {
        return Err(StatsError::SubgroupSizeUnsupported { n });
    }
    match ROWS.iter().find(|r| r.n == n) {
        Some(row) => {
            Ok(SpcConstants { table_id: SPC_TABLE_ID, table_version: SPC_TABLE_VERSION, review: row.review, row })
        }
        None => Err(StatsError::SubgroupSizeUnsupported { n }),
    }
}
