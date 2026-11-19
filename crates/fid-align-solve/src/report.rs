//! The residual report (spec node `ALN.QA.REPORT`): type, builder, statistics and digest.
//!
//! Every alignment solver hands its residuals to a [`ReportBuilder`] one entry at a time, in
//! the order its residual definition fixes, and calls [`ReportBuilder::finish`]. The finished
//! [`ResidualReport`] is immutable: columnar arrays of the entries, the statistics and the
//! digest. Everything below is pinned in the leaf's Design section.
//!
//! # Entries (REQ-ALN.QA.REPORT-001)
//!
//! An entry holds its reference ([`EntryRef`]: the index of the alignment input in command
//! order and the item inside that input, such as a point index or a feature component), its
//! kind, the signed scalar residual in metres, the residual vector in metres when the solver
//! defines one, the weight when the solver gives weights, and whether it was used, with a
//! rejection reason when it was not. `-0.0` in a residual or vector component is stored as
//! `+0.0`; every other value keeps its bits.
//!
//! # Statistics (REQ-ALN.QA.REPORT-002, -003)
//!
//! Over the used entries, in index order, with the scalar residuals `r` and weights `w`:
//!
//! - `rms = sqrt(sum_sq(r) / n_used)`;
//! - `weighted_rms = sqrt(dot(w, q) / sum(w))` with `q[i] = r[i] * r[i]`, only when the
//!   report is weighted (weights are dimensionless, so the unit stays metres);
//! - `max_abs = |r[k]|` and `max_entry = k`, where `k` is the lowest index whose `|r|` is not
//!   less, in IEEE 754 total order, than any other used `|r|`.
//!
//! `sum_sq`, `dot` and `sum` are `fid-num`'s fixed-order reductions (KRN.NUM.RED: blocks of
//! 256 in index order with Neumaier compensation, combined by a fixed binary tree), the
//! division is one correctly rounded IEEE 754 division (the count converts exactly), and
//! `sqrt` is `fid_num::f::sqrt` (correctly rounded). No FMA, no `f64::min`/`f64::max`. The
//! statistics are therefore a function of the entry values and their order only and are
//! bit-identical on every platform, wasm32 included.
//!
//! # Pinned edge cases
//!
//! - A report with no entries is refused with `empty_report`.
//! - A report whose entries are all rejected is valid: counts are set and `rms`,
//!   `weighted_rms`, `max_abs` and `max_entry` are absent.
//! - A square, a weighted square or a sum that exceeds `f64::MAX` (`|r|` from about `1.34e154`
//!   metres) gives `stat_overflow`; an intermediate overflow is never compensated away.
//! - A square below the smallest subnormal (`|r|` below about `1e-162` metres) rounds to zero,
//!   so `rms` can be `0` while `max_abs > 0`; this is the IEEE 754 result, not an error.
//! - A NaN or infinite residual or vector component gives `nan_residual` naming the entry,
//!   when it is pushed (REQ-ALN.QA.REPORT-004). The maximum search refuses a NaN again with
//!   the same error (it cannot occur after the push check).
//! - Weights are all-or-none: the first entry decides; a later entry that differs gives
//!   `mixed_weights`. A weight must be finite and `> 0` (`bad_weight`).
//!
//! # Layout version 1 and digest
//!
//! All integers little-endian, floats as their IEEE 754 bits. The columns layout is
//!
//! ```text
//! n u64 | weighted u8 | n x input u32 | n x item u64 | n x kind u8 | n x residual f64
//! | n x has_vector u8 | (for each entry with a vector) 3 x f64
//! | (weighted reports only) n x weight f64 | n x used u8 | n x (reason_len u8 | reason bytes)
//! ```
//!
//! Kind bytes: `point` 1, `feature` 2, `pair` 3, `locked_direction` 4. A used entry has
//! `reason_len` 0. The stats JSON is `fid-kci`'s kernel canonical JSON (RFC 8785 restricted to
//! strings, arrays and objects) of the object with members `max_abs`, `max_entry`,
//! `n_rejected`, `n_used`, `rms`, `weighted_rms` (absent statistics are left out); a length is
//! `{"unit": "m", "value": "0x" + 16 lowercase hex digits of its bits}`, a count or an index is
//! its decimal string. The digest is
//!
//! ```text
//! SHA-256( "FIDRES" | 0x01 | stats_len u64 | stats JSON | SHA-256(columns layout) )
//! ```
//!
//! using `fid-kci`'s portable SHA-256, so it depends only on the entries in index order.

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::Sha256;
use fid_num::NumError;
use fid_types::Digest;
use std::cmp::Ordering;
use std::fmt;

/// Most entries one report holds.
pub const MAX_ENTRIES: usize = 1 << 26;
/// Largest item index; every item converts exactly to a JSON number.
pub const MAX_ITEM: u64 = (1 << 53) - 1;
/// Longest rejection reason, in bytes.
pub const MAX_REASON_BYTES: usize = 64;
/// Most distinct rejection reasons in one report.
pub const MAX_REASONS: usize = 256;
/// The ASCII tag the digest preimage starts with.
pub const LAYOUT_TAG: [u8; 6] = *b"FIDRES";
/// The layout version this module writes.
pub const LAYOUT_VERSION: u8 = 1;

/// Bytes buffered before the columns layout is passed to the hasher.
const SINK_BYTES: usize = 64 * 1024;

// ------------------------------------------------------------------ errors

/// Every way the builder refuses. Nothing is recorded when [`ReportBuilder::push`] fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// A NaN or infinite residual or vector component (REQ-ALN.QA.REPORT-004); `index` is
    /// the entry.
    NanResidual { index: u64 },
    /// `finish` on a builder with no entries.
    EmptyReport,
    /// A square, weighted square or sum of the statistics exceeds `f64::MAX`.
    StatOverflow,
    /// An entry field outside its pinned domain (item index, reason code). Holds a reason.
    BadEntry(&'static str),
    /// A weight that is not finite and `> 0`.
    BadWeight { index: u64 },
    /// An entry with a weight in an unweighted report, or without one in a weighted report.
    MixedWeights { index: u64 },
    /// A size budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// A column could not grow.
    AllocationFailed,
    /// A fault that cannot be the caller's mistake.
    Internal(&'static str),
}

impl ReportError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ReportError::NanResidual { .. } => "nan_residual",
            ReportError::EmptyReport => "empty_report",
            ReportError::StatOverflow => "stat_overflow",
            ReportError::BadEntry(_) => "bad_entry",
            ReportError::BadWeight { .. } => "bad_weight",
            ReportError::MixedWeights { .. } => "mixed_weights",
            ReportError::InputLimit(_) => "input_limit",
            ReportError::AllocationFailed => "allocation_failed",
            ReportError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            ReportError::NanResidual { index }
            | ReportError::BadWeight { index }
            | ReportError::MixedWeights { index } => write!(f, ": entry {index}"),
            ReportError::BadEntry(s) | ReportError::InputLimit(s) | ReportError::Internal(s) => write!(f, ": {s}"),
            ReportError::EmptyReport | ReportError::StatOverflow | ReportError::AllocationFailed => Ok(()),
        }
    }
}

impl std::error::Error for ReportError {}

/// Maps a reduction error: an overflow is `stat_overflow`; anything else cannot happen on
/// values the builder has already checked.
fn stat_err(e: NumError) -> ReportError {
    match e {
        NumError::Infinite { .. } => ReportError::StatOverflow,
        _ => ReportError::Internal("reduction refused checked values"),
    }
}

// ------------------------------------------------------------------ units and values

/// The unit a value is carried with. Residuals are lengths in metres (CONSTITUTION §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unit {
    Metre,
}

impl Unit {
    /// The wire symbol: `m`.
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Metre => "m",
        }
    }
}

/// A length with its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    value: f64,
    unit: Unit,
}

impl Length {
    /// A length in metres.
    pub const fn metres(value: f64) -> Length {
        Length { value, unit: Unit::Metre }
    }

    /// The number, in [`Length::unit`].
    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn unit(&self) -> Unit {
        self.unit
    }
}

/// A 3-vector of lengths with its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LengthVec {
    value: [f64; 3],
    unit: Unit,
}

impl LengthVec {
    /// A vector in metres.
    pub const fn metres(value: [f64; 3]) -> LengthVec {
        LengthVec { value, unit: Unit::Metre }
    }

    /// The components, in [`LengthVec::unit`].
    pub fn value(&self) -> [f64; 3] {
        self.value
    }

    pub fn unit(&self) -> Unit {
        self.unit
    }
}

/// `-0.0` becomes `+0.0`; every other value keeps its bits.
fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

// ------------------------------------------------------------------ entries

/// What a residual entry measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EntryKind {
    Point = 1,
    Feature = 2,
    Pair = 3,
    LockedDirection = 4,
}

impl EntryKind {
    /// Every kind, in byte order.
    pub const ALL: [EntryKind; 4] = [EntryKind::Point, EntryKind::Feature, EntryKind::Pair, EntryKind::LockedDirection];

    /// The layout byte.
    pub const fn byte(self) -> u8 {
        self as u8
    }

    /// The wire name: `point`, `feature`, `pair` or `locked_direction`.
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Point => "point",
            EntryKind::Feature => "feature",
            EntryKind::Pair => "pair",
            EntryKind::LockedDirection => "locked_direction",
        }
    }

    /// The kind with this wire name.
    pub fn parse(s: &str) -> Option<EntryKind> {
        EntryKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Where an entry comes from: the alignment input (its index in the alignment's `inputs`,
/// command order) and the item inside it (a point index, a feature component, a pair index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryRef {
    pub input: u32,
    /// At most [`MAX_ITEM`].
    pub item: u64,
}

/// A rejection reason code: `^[a-z][a-z0-9_]*$`, at most [`MAX_REASON_BYTES`] bytes, for
/// example `outlier`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RejectReason(String);

impl RejectReason {
    /// Builds a reason; a code outside the grammar gives `bad_entry`.
    pub fn new(code: &str) -> Result<RejectReason, ReportError> {
        let mut b = code.bytes();
        let ok = code.len() <= MAX_REASON_BYTES
            && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
            && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
        if ok {
            Ok(RejectReason(code.to_string()))
        } else {
            Err(ReportError::BadEntry("reason must match ^[a-z][a-z0-9_]*$, at most 64 bytes"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether the solver used an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    Used,
    Rejected(RejectReason),
}

/// One residual as a solver pushes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub reference: EntryRef,
    pub kind: EntryKind,
    /// The signed scalar residual, in metres.
    pub residual_m: f64,
    /// The residual vector, in metres, when the solver defines one.
    pub vector_m: Option<[f64; 3]>,
    /// The weight (dimensionless), when the solver gives weights.
    pub weight: Option<f64>,
}

/// One stored entry, read back with its units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryView<'a> {
    pub index: u64,
    pub reference: EntryRef,
    pub kind: EntryKind,
    pub residual: Length,
    pub vector: Option<LengthVec>,
    pub weight: Option<f64>,
    pub used: bool,
    /// The rejection reason; `None` exactly when `used`.
    pub reason: Option<&'a str>,
}

// ------------------------------------------------------------------ columns

/// The columnar arrays of the entries.
#[derive(Debug, Clone, PartialEq, Default)]
struct Columns {
    inputs: Vec<u32>,
    items: Vec<u64>,
    kinds: Vec<EntryKind>,
    scalars: Vec<f64>,
    vectors: Vec<Option<[f64; 3]>>,
    /// Empty unless the report is weighted.
    weights: Vec<f64>,
    weighted: bool,
    used: Vec<bool>,
    /// 0 for a used entry, else `k` for `reason_names[k - 1]`.
    reasons: Vec<u16>,
    reason_names: Vec<RejectReason>,
}

fn grow<T>(v: &mut Vec<T>) -> Result<(), ReportError> {
    v.try_reserve(1).map_err(|_| ReportError::AllocationFailed)
}

impl Columns {
    fn len(&self) -> usize {
        self.scalars.len()
    }

    fn view(&self, i: usize) -> Option<EntryView<'_>> {
        let reason = match *self.reasons.get(i)? {
            0 => None,
            k => Some(self.reason_names.get(usize::from(k) - 1)?.as_str()),
        };
        Some(EntryView {
            index: i as u64,
            reference: EntryRef { input: *self.inputs.get(i)?, item: *self.items.get(i)? },
            kind: *self.kinds.get(i)?,
            residual: Length::metres(*self.scalars.get(i)?),
            vector: self.vectors.get(i)?.map(LengthVec::metres),
            weight: if self.weighted { Some(*self.weights.get(i)?) } else { None },
            used: *self.used.get(i)?,
            reason,
        })
    }
}

// ------------------------------------------------------------------ builder

/// Collects a solver's residuals in index order; [`ReportBuilder::finish`] computes the
/// statistics and the digest.
#[derive(Debug, Clone, Default)]
pub struct ReportBuilder {
    cols: Columns,
    /// Set by the first entry: whether the report carries weights.
    weighted: Option<bool>,
}

impl ReportBuilder {
    pub fn new() -> ReportBuilder {
        ReportBuilder::default()
    }

    /// Entries pushed so far.
    pub fn len(&self) -> usize {
        self.cols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cols.len() == 0
    }

    /// Appends one entry and returns its index. Checks, in this order: the entry budget
    /// (`input_limit`), the item index (`bad_entry`), the residual and then the vector
    /// components (`nan_residual` for NaN or an infinity), weight presence against the first
    /// entry (`mixed_weights`), the weight's value (`bad_weight`), the number of distinct
    /// reasons (`input_limit`), then column growth (`allocation_failed`). On any error
    /// nothing is recorded.
    pub fn push(&mut self, entry: Entry, disposition: Disposition) -> Result<u64, ReportError> {
        let n = self.cols.len();
        if n >= MAX_ENTRIES {
            return Err(ReportError::InputLimit("entries"));
        }
        let index = n as u64;
        if entry.reference.item > MAX_ITEM {
            return Err(ReportError::BadEntry("item exceeds 2^53 - 1"));
        }
        if !entry.residual_m.is_finite() {
            return Err(ReportError::NanResidual { index });
        }
        if let Some(v) = entry.vector_m {
            if v.iter().any(|c| !c.is_finite()) {
                return Err(ReportError::NanResidual { index });
            }
        }
        let has_weight = entry.weight.is_some();
        if self.weighted.is_some_and(|w| w != has_weight) {
            return Err(ReportError::MixedWeights { index });
        }
        if let Some(w) = entry.weight {
            if !(w.is_finite() && w > 0.0) {
                return Err(ReportError::BadWeight { index });
            }
        }
        // The reason's number, and the name to add when it is new.
        let (reason, new_name) = match disposition {
            Disposition::Used => (0u16, None),
            Disposition::Rejected(r) => match self.cols.reason_names.iter().position(|x| *x == r) {
                Some(k) => ((k + 1) as u16, None),
                None if self.cols.reason_names.len() >= MAX_REASONS => {
                    return Err(ReportError::InputLimit("reasons"));
                }
                None => ((self.cols.reason_names.len() + 1) as u16, Some(r)),
            },
        };
        let c = &mut self.cols;
        if new_name.is_some() {
            grow(&mut c.reason_names)?;
        }
        grow(&mut c.inputs)?;
        grow(&mut c.items)?;
        grow(&mut c.kinds)?;
        grow(&mut c.scalars)?;
        grow(&mut c.vectors)?;
        grow(&mut c.used)?;
        grow(&mut c.reasons)?;
        if has_weight {
            grow(&mut c.weights)?;
        }
        c.inputs.push(entry.reference.input);
        c.items.push(entry.reference.item);
        c.kinds.push(entry.kind);
        c.scalars.push(plus_zero(entry.residual_m));
        c.vectors.push(entry.vector_m.map(|v| v.map(plus_zero)));
        if let Some(w) = entry.weight {
            c.weights.push(w);
        }
        c.used.push(reason == 0);
        c.reasons.push(reason);
        if let Some(r) = new_name {
            c.reason_names.push(r);
        }
        self.weighted = Some(has_weight);
        c.weighted = has_weight;
        Ok(index)
    }

    /// The finished report. `empty_report` without entries; `stat_overflow` if a statistic
    /// overflows (see the module documentation); `allocation_failed` if the working copies of
    /// the used values cannot be allocated.
    pub fn finish(self) -> Result<ResidualReport, ReportError> {
        if self.cols.len() == 0 {
            return Err(ReportError::EmptyReport);
        }
        let stats = statistics(&self.cols)?;
        let stats_json = stats_json(&stats)?;
        let columns_digest = columns_digest(&self.cols);
        let mut h = Sha256::new();
        h.update(&LAYOUT_TAG);
        h.update(&[LAYOUT_VERSION]);
        h.update(&(stats_json.len() as u64).to_le_bytes());
        h.update(&stats_json);
        h.update(&columns_digest);
        let digest = Digest::from_bytes(h.finish());
        Ok(ResidualReport { cols: self.cols, stats, stats_json, digest })
    }
}

// ------------------------------------------------------------------ statistics

/// The statistics of a report (REQ-ALN.QA.REPORT-002). The four optional values are absent
/// exactly when no entry is used; `weighted_rms` is also absent in an unweighted report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResidualStats {
    rms: Option<Length>,
    weighted_rms: Option<Length>,
    max_abs: Option<Length>,
    max_entry: Option<u64>,
    n_used: u64,
    n_rejected: u64,
}

impl ResidualStats {
    pub fn rms(&self) -> Option<Length> {
        self.rms
    }

    pub fn weighted_rms(&self) -> Option<Length> {
        self.weighted_rms
    }

    /// The maximum absolute scalar residual over the used entries.
    pub fn max_abs(&self) -> Option<Length> {
        self.max_abs
    }

    /// The index of the entry [`ResidualStats::max_abs`] comes from (lowest on ties).
    pub fn max_entry(&self) -> Option<u64> {
        self.max_entry
    }

    pub fn n_used(&self) -> u64 {
        self.n_used
    }

    pub fn n_rejected(&self) -> u64 {
        self.n_rejected
    }
}

fn collect(n: usize, values: impl Iterator<Item = f64>) -> Result<Vec<f64>, ReportError> {
    let mut v: Vec<f64> = Vec::new();
    v.try_reserve_exact(n).map_err(|_| ReportError::AllocationFailed)?;
    v.extend(values);
    Ok(v)
}

fn statistics(c: &Columns) -> Result<ResidualStats, ReportError> {
    let n = c.len();
    let n_used = c.used.iter().filter(|u| **u).count();
    let mut st = ResidualStats {
        rms: None,
        weighted_rms: None,
        max_abs: None,
        max_entry: None,
        n_used: n_used as u64,
        n_rejected: (n - n_used) as u64,
    };
    if n_used == 0 {
        return Ok(st);
    }
    let used = |i: &usize| c.used.get(*i).copied().unwrap_or(false);
    let r = collect(n_used, (0..n).filter(used).filter_map(|i| c.scalars.get(i).copied()))?;
    // Slice lengths never exceed 2^53, so the count converts exactly.
    let mean_sq = fid_num::red::sum_sq(&r).map_err(stat_err)? / n_used as f64;
    st.rms = Some(Length::metres(fid_num::f::sqrt(mean_sq).map_err(stat_err)?));
    if c.weighted {
        let w = collect(n_used, (0..n).filter(used).filter_map(|i| c.weights.get(i).copied()))?;
        // Every square is finite: `sum_sq` above refused an overflowing one.
        let q = collect(n_used, r.iter().map(|x| x * x))?;
        let num = fid_num::red::dot(&w, &q).map_err(stat_err)?;
        let den = fid_num::red::sum(&w).map_err(stat_err)?;
        let ratio = num / den;
        if !ratio.is_finite() {
            return Err(ReportError::StatOverflow);
        }
        st.weighted_rms = Some(Length::metres(fid_num::f::sqrt(ratio).map_err(stat_err)?));
    }
    // Maximum by explicit total-order comparison; the strict `Greater` keeps the lowest
    // index on ties. `total_cmp` refuses NaN, which the push check already excluded.
    let mut best: Option<(f64, usize)> = None;
    for i in (0..n).filter(used) {
        let a = c.scalars.get(i).copied().unwrap_or(0.0).abs();
        best = match best {
            None => Some((a, i)),
            Some((b, k)) => match fid_num::f::total_cmp(a, b) {
                Ok(Ordering::Greater) => Some((a, i)),
                Ok(_) => Some((b, k)),
                Err(_) => return Err(ReportError::NanResidual { index: i as u64 }),
            },
        };
    }
    if let Some((a, k)) = best {
        st.max_abs = Some(Length::metres(a));
        st.max_entry = Some(k as u64);
    }
    Ok(st)
}

// ------------------------------------------------------------------ digest

fn hex_bits(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

fn length_json(l: &Length) -> Json {
    Json::Obj(vec![
        ("unit".to_string(), Json::Str(l.unit.as_str().to_string())),
        ("value".to_string(), Json::Str(hex_bits(l.value))),
    ])
}

/// The kernel canonical JSON of the statistics (layout version 1).
fn stats_json(s: &ResidualStats) -> Result<Vec<u8>, ReportError> {
    let mut m = vec![
        ("n_rejected".to_string(), Json::Str(s.n_rejected.to_string())),
        ("n_used".to_string(), Json::Str(s.n_used.to_string())),
    ];
    for (k, v) in [("max_abs", &s.max_abs), ("rms", &s.rms), ("weighted_rms", &s.weighted_rms)] {
        if let Some(l) = v {
            m.push((k.to_string(), length_json(l)));
        }
    }
    if let Some(k) = s.max_entry {
        m.push(("max_entry".to_string(), Json::Str(k.to_string())));
    }
    to_canonical(&Json::Obj(m)).map_err(|_| ReportError::Internal("stats json"))
}

/// Buffers the layout and passes it to the hasher in large blocks.
struct Sink {
    h: Sha256,
    buf: Vec<u8>,
}

impl Sink {
    fn put(&mut self, b: &[u8]) {
        if self.buf.len() + b.len() > SINK_BYTES {
            self.h.update(&self.buf);
            self.buf.clear();
        }
        self.buf.extend_from_slice(b);
    }

    fn finish(mut self) -> [u8; 32] {
        self.h.update(&self.buf);
        self.h.finish()
    }
}

/// SHA-256 of the columns layout (layout version 1).
fn columns_digest(c: &Columns) -> [u8; 32] {
    let mut s = Sink { h: Sha256::new(), buf: Vec::with_capacity(SINK_BYTES) };
    s.put(&(c.len() as u64).to_le_bytes());
    s.put(&[u8::from(c.weighted)]);
    for x in &c.inputs {
        s.put(&x.to_le_bytes());
    }
    for x in &c.items {
        s.put(&x.to_le_bytes());
    }
    for k in &c.kinds {
        s.put(&[k.byte()]);
    }
    for x in &c.scalars {
        s.put(&x.to_bits().to_le_bytes());
    }
    for v in &c.vectors {
        s.put(&[u8::from(v.is_some())]);
    }
    for v in c.vectors.iter().flatten() {
        for x in v {
            s.put(&x.to_bits().to_le_bytes());
        }
    }
    if c.weighted {
        for x in &c.weights {
            s.put(&x.to_bits().to_le_bytes());
        }
    }
    for u in &c.used {
        s.put(&[u8::from(*u)]);
    }
    for k in &c.reasons {
        let name = match *k {
            0 => "",
            k => c.reason_names.get(usize::from(k) - 1).map_or("", |r| r.as_str()),
        };
        s.put(&[name.len() as u8]);
        s.put(name.as_bytes());
    }
    s.finish()
}

// ------------------------------------------------------------------ report

/// A finished residual report: the entries as columnar arrays, the statistics and the
/// digest. Immutable; only [`ReportBuilder::finish`] creates one.
#[derive(Debug, Clone, PartialEq)]
pub struct ResidualReport {
    cols: Columns,
    stats: ResidualStats,
    stats_json: Vec<u8>,
    digest: Digest,
}

impl ResidualReport {
    /// Number of entries (used and rejected); never zero.
    pub fn len(&self) -> usize {
        self.cols.len()
    }

    /// Always false: an empty report is refused.
    pub fn is_empty(&self) -> bool {
        self.cols.len() == 0
    }

    pub fn stats(&self) -> &ResidualStats {
        &self.stats
    }

    /// Whether the entries carry weights.
    pub fn weighted(&self) -> bool {
        self.cols.weighted
    }

    /// The report digest (layout version 1).
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The kernel canonical JSON of the statistics that the digest covers.
    pub fn stats_json(&self) -> &[u8] {
        &self.stats_json
    }

    /// Entry `index`, if there is one.
    pub fn entry(&self, index: usize) -> Option<EntryView<'_>> {
        self.cols.view(index)
    }

    /// Entries `[offset, offset + limit)` cut at the end, in index order.
    pub fn entries(&self, offset: usize, limit: usize) -> impl Iterator<Item = EntryView<'_>> + '_ {
        let end = offset.saturating_add(limit);
        (offset..end).map_while(move |i| self.cols.view(i))
    }
}
