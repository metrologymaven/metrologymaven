//! Session timebase, clock offset and drift estimation, and the fusion rule (spec node
//! `HAL.ACQ.TIME`, REQ-HAL.ACQ-006).
//!
//! - [`SessionTimebase`]: the session's one timebase, host monotonic i64 ns, anchored once to
//!   UTC when the session opens and never re-anchored (REQ-001). The library never reads a
//!   clock itself: callers inject one through [`SessionClock`].
//! - [`estimate`]: ordinary least squares over recorded `(t_dev, t_host)` pairs, giving offset,
//!   drift and the residual standard deviation as sync uncertainty (REQ-002), or
//!   `time_estimate_insufficient` below [`MIN_PAIRS`] pairs (REQ-003).
//! - [`ClockMapping`]: maps a device timestamp to session time, falling back to host-receive
//!   time with source `HOST_RECEIVE` when no clock model exists (REQ-003).
//! - [`fuse`]: the fusion rule and the `time_sync_insufficient` verdict (REQ-004).
//!
//! # Determinism (REQ-005, D0)
//!
//! Every value entering floating point is an integer of magnitude at most 2^53
//! ([`MAX_SPAN_NS`]), so the i64 → f64 conversion is exact. Sums run in index order with one
//! rounding per operation; Rust never contracts `a * b + c` into a fused multiply-add and this
//! module calls no `mul_add`. The square root goes through `fid_num::f::sqrt` (correctly
//! rounded). The outputs are therefore bit-identical on every IEEE 754 platform.
//!
//! # Model
//!
//! With `x_i = t_dev_i − t_dev_0` and `y_i = (t_host_i − t_host_0) − x_i` (pairs centred on
//! their first sample), the fit is `y ≈ intercept + drift · x`. So
//! `t_host ≈ t_dev + offset + drift · (t_dev − t_dev_0)` with
//! `offset = (t_host_0 − t_dev_0) + intercept`: offset is the host-minus-device clock
//! difference at the first device sample, and drift is the dimensionless rate of the host
//! clock relative to the device clock minus one (50 ppm = 5e-5).

use fid_hal_idl::{TimeSource, Timestamp};
use std::fmt;

/// The fewest pairs the estimator accepts (REQ-HAL.ACQ.TIME-002).
pub const MIN_PAIRS: usize = 16;

/// The largest magnitude, in ns, of any centred value that enters floating point (2^53 ns,
/// about 104 days). Every integer up to it is exact in f64; anything larger is
/// `time_span_too_large`.
pub const MAX_SPAN_NS: i64 = 1 << 53;

/// An injected time source: the library never reads a clock itself.
pub trait SessionClock {
    /// Host monotonic time, ns. Never steps backwards.
    fn mono_ns(&self) -> i64;
    /// Host wall-clock (UTC) time, ns since the Unix epoch. May step.
    fn utc_ns(&self) -> i64;
}

/// Why a time computation failed. Every hostile input gives one of these; nothing panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeError {
    /// Fewer than [`MIN_PAIRS`] pairs (`time_estimate_insufficient`); callers fall back to
    /// host-receive time.
    EstimateInsufficient { n: usize },
    /// More pairs than `u32::MAX` (`time_too_many_pairs`).
    TooManyPairs { n: usize },
    /// Pair `index` repeats the previous device timestamp (`time_duplicate`).
    Duplicate { index: usize },
    /// Pair `index` has a device timestamp before the previous one (`time_non_monotonic`).
    NonMonotonic { index: usize },
    /// Pair `index` lies more than [`MAX_SPAN_NS`] from the first pair once centred
    /// (`time_span_too_large`).
    SpanTooLarge { index: usize },
    /// A timestamp passed to [`ClockModel::map_to_host`] lies more than [`MAX_SPAN_NS`] from
    /// the model's reference sample (`time_map_span_too_large`).
    MapSpanTooLarge,
    /// An i64 nanosecond result does not fit (`time_overflow`).
    Overflow { what: &'static str },
    /// A sigma or threshold is NaN, infinite or negative (`time_invalid_input`).
    InvalidInput { what: &'static str },
    /// An intermediate result is not finite (`time_not_finite`); unreachable for inputs that
    /// pass validation, kept so no path can panic.
    NotFinite { what: &'static str },
}

impl TimeError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            TimeError::EstimateInsufficient { .. } => "time_estimate_insufficient",
            TimeError::TooManyPairs { .. } => "time_too_many_pairs",
            TimeError::Duplicate { .. } => "time_duplicate",
            TimeError::NonMonotonic { .. } => "time_non_monotonic",
            TimeError::SpanTooLarge { .. } => "time_span_too_large",
            TimeError::MapSpanTooLarge => "time_map_span_too_large",
            TimeError::Overflow { .. } => "time_overflow",
            TimeError::InvalidInput { .. } => "time_invalid_input",
            TimeError::NotFinite { .. } => "time_not_finite",
        }
    }
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeError::EstimateInsufficient { n } => {
                write!(f, "{}: {n} pairs, at least {MIN_PAIRS} needed", self.code())
            }
            TimeError::TooManyPairs { n } => write!(f, "{}: {n} pairs", self.code()),
            TimeError::Duplicate { index }
            | TimeError::NonMonotonic { index }
            | TimeError::SpanTooLarge { index } => write!(f, "{} at pair {index}", self.code()),
            TimeError::MapSpanTooLarge => write!(f, "{}: more than {MAX_SPAN_NS} ns from reference", self.code()),
            TimeError::Overflow { what } | TimeError::InvalidInput { what } | TimeError::NotFinite { what } => {
                write!(f, "{}: {what}", self.code())
            }
        }
    }
}

impl std::error::Error for TimeError {}

/// The session's timebase: host monotonic ns, anchored once to UTC (REQ-HAL.ACQ.TIME-001).
/// The fields are private and there is no setter, so a timebase can never be re-anchored;
/// a host wall-clock step after opening does not change session time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionTimebase {
    anchor_utc_ns: i64,
    anchor_mono_ns: i64,
}

impl SessionTimebase {
    /// Records the one anchor: host monotonic `mono_ns` corresponds to UTC `utc_ns`.
    pub fn anchor(mono_ns: i64, utc_ns: i64) -> SessionTimebase {
        SessionTimebase { anchor_utc_ns: utc_ns, anchor_mono_ns: mono_ns }
    }

    /// Opens a session on an injected clock: reads monotonic, then UTC, once each.
    pub fn open<C: SessionClock + ?Sized>(clock: &C) -> SessionTimebase {
        let mono = clock.mono_ns();
        let utc = clock.utc_ns();
        SessionTimebase::anchor(mono, utc)
    }

    /// The UTC anchor, ns since the Unix epoch.
    pub fn anchor_utc_ns(&self) -> i64 {
        self.anchor_utc_ns
    }

    /// The host monotonic reading at the anchor, ns.
    pub fn anchor_mono_ns(&self) -> i64 {
        self.anchor_mono_ns
    }

    /// Session time elapsed since the anchor for a host monotonic reading (negative before it).
    pub fn elapsed_ns(&self, mono_ns: i64) -> Result<i64, TimeError> {
        mono_ns.checked_sub(self.anchor_mono_ns).ok_or(TimeError::Overflow { what: "session elapsed time" })
    }

    /// UTC for a host monotonic reading: the anchor plus elapsed monotonic time. Never reads
    /// the wall clock.
    pub fn to_utc_ns(&self, mono_ns: i64) -> Result<i64, TimeError> {
        self.anchor_utc_ns.checked_add(self.elapsed_ns(mono_ns)?).ok_or(TimeError::Overflow { what: "session UTC" })
    }

    /// UTC now, from the clock's monotonic reading only.
    pub fn now_utc_ns<C: SessionClock + ?Sized>(&self, clock: &C) -> Result<i64, TimeError> {
        self.to_utc_ns(clock.mono_ns())
    }
}

/// A device clock's mapping onto the host clock (REQ-HAL.ACQ.TIME-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockModel {
    /// Host minus device clock at the reference sample, ns: `(ref_host − ref_dev) + intercept`
    /// rounded once to f64.
    pub offset_ns: f64,
    /// Dimensionless rate of host time per device time, minus one.
    pub drift: f64,
    /// Residual standard deviation (n − 2 degrees of freedom): the sync uncertainty, ns.
    pub sigma_ns: f64,
    /// The time source the pairs came from.
    pub source: TimeSource,
    /// Number of pairs fitted.
    pub n: u32,
    /// Device timestamp of the first pair (the centring reference), ns.
    pub ref_dev_ns: i64,
    /// Host timestamp of the first pair, ns.
    pub ref_host_ns: i64,
    /// Fitted intercept of the centred model, ns: the offset relative to the first pair's
    /// raw offset. Kept separately so mapping stays exact when the raw offset is large.
    pub intercept_ns: f64,
}

impl ClockModel {
    /// Maps a device timestamp to host monotonic (session) time:
    /// `ref_host + dx + round(intercept + drift · dx)` with `dx = t_dev − ref_dev`, rounding
    /// half away from zero. Fails with `time_map_span_too_large` when `|dx| > 2^53` and
    /// `time_overflow` when the result leaves i64.
    pub fn map_to_host(&self, t_dev: i64) -> Result<i64, TimeError> {
        let dx = i128::from(t_dev) - i128::from(self.ref_dev_ns);
        if dx.unsigned_abs() > u128::from(MAX_SPAN_NS.unsigned_abs()) {
            return Err(TimeError::MapSpanTooLarge);
        }
        let corr = self.intercept_ns + self.drift * (dx as f64);
        if !corr.is_finite() {
            return Err(TimeError::NotFinite { what: "clock correction" });
        }
        let corr = corr.round();
        // |corr| < 2^63 is checked in f64 so the cast below is exact.
        if corr.abs() >= 9_223_372_036_854_775_808.0 {
            return Err(TimeError::Overflow { what: "clock correction" });
        }
        let t = i128::from(self.ref_host_ns) + dx + i128::from(corr as i64);
        i64::try_from(t).map_err(|_| TimeError::Overflow { what: "mapped host time" })
    }

    /// The stream's sync description for [`fuse`].
    pub fn sync(&self) -> StreamSync {
        StreamSync { source: self.source, sigma_ns: self.sigma_ns }
    }
}

/// Estimates offset, drift and sync uncertainty from `(t_dev, t_host)` pairs by ordinary
/// least squares on pairs centred at the first sample, sums in index order, no FMA.
///
/// Inputs must have strictly increasing device timestamps and stay within [`MAX_SPAN_NS`]
/// of the first pair once centred. Errors are checked in this
/// order: count (`time_estimate_insufficient`, `time_too_many_pairs`), then each pair in index
/// order. Host timestamps are not required to be monotonic: host-receive jitter and buffered
/// delivery can invert neighbouring host times, and OLS absorbs that as residual (maintainer
/// decision in review of PR #164).
pub fn estimate(pairs: &[(i64, i64)], source: TimeSource) -> Result<ClockModel, TimeError> {
    let n = pairs.len();
    if n < MIN_PAIRS {
        return Err(TimeError::EstimateInsufficient { n });
    }
    let n32 = u32::try_from(n).map_err(|_| TimeError::TooManyPairs { n })?;
    let (d0, h0) = match pairs.first() {
        Some(&p) => p,
        None => return Err(TimeError::EstimateInsufficient { n }),
    };
    let span = i128::from(MAX_SPAN_NS);
    let mut xs = Vec::with_capacity(n);
    let mut ys = Vec::with_capacity(n);
    let mut prev: Option<i64> = None;
    for (index, &(d, h)) in pairs.iter().enumerate() {
        if let Some(pd) = prev {
            if d == pd {
                return Err(TimeError::Duplicate { index });
            }
            if d < pd {
                return Err(TimeError::NonMonotonic { index });
            }
        }
        prev = Some(d);
        let x = i128::from(d) - i128::from(d0);
        let y = (i128::from(h) - i128::from(h0)) - x;
        if x > span || y > span || y < -span {
            return Err(TimeError::SpanTooLarge { index });
        }
        // Exact: |x|, |y| <= 2^53.
        xs.push(x as f64);
        ys.push(y as f64);
    }

    let nf = f64::from(n32);
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    for (x, y) in xs.iter().zip(&ys) {
        sx += *x;
        sy += *y;
    }
    let mx = sx / nf;
    let my = sy / nf;
    let mut sxx = 0.0_f64;
    let mut sxy = 0.0_f64;
    for (x, y) in xs.iter().zip(&ys) {
        let dx = *x - mx;
        let dy = *y - my;
        let pxx = dx * dx;
        let pxy = dx * dy;
        sxx += pxx;
        sxy += pxy;
    }
    if !sxx.is_finite() || sxx <= 0.0 {
        // Unreachable with >= 16 strictly increasing device times; no spread in device time
        // means the pairs cannot determine a drift.
        return Err(TimeError::EstimateInsufficient { n });
    }
    let drift = sxy / sxx;
    let p = drift * mx;
    let intercept = my - p;
    let mut ssr = 0.0_f64;
    for (x, y) in xs.iter().zip(&ys) {
        let q = drift * *x;
        let fit = intercept + q;
        let r = *y - fit;
        let rr = r * r;
        ssr += rr;
    }
    let var = ssr / (nf - 2.0);
    let sigma = fid_num::f::sqrt(var).map_err(|_| TimeError::NotFinite { what: "residual variance" })?;
    let base = i128::from(h0) - i128::from(d0);
    // i128 → f64 rounds to nearest, ties to even.
    let offset = (base as f64) + intercept;
    for (v, what) in [(drift, "drift"), (intercept, "intercept"), (offset, "offset")] {
        if !v.is_finite() {
            return Err(TimeError::NotFinite { what });
        }
    }
    Ok(ClockModel {
        offset_ns: offset,
        drift,
        sigma_ns: sigma,
        source,
        n: n32,
        ref_dev_ns: d0,
        ref_host_ns: h0,
        intercept_ns: intercept,
    })
}

/// A timestamp mapped to session time, with the source actually used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MappedTime {
    /// Session time (host monotonic), ns.
    pub t_session_ns: i64,
    /// The source recorded with the mapped time.
    pub source: TimeSource,
}

/// How a device's timestamps reach session time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockMapping {
    /// Through an estimated clock model.
    Model(ClockModel),
    /// Host-receive time; the recorded source is `HOST_RECEIVE` (REQ-HAL.ACQ.TIME-003).
    HostReceive,
}

impl ClockMapping {
    /// Turns an estimate into a mapping: `time_estimate_insufficient` falls back to
    /// [`ClockMapping::HostReceive`]; every other error is returned unchanged.
    pub fn from_estimate(estimate: Result<ClockModel, TimeError>) -> Result<ClockMapping, TimeError> {
        match estimate {
            Ok(m) => Ok(ClockMapping::Model(m)),
            Err(TimeError::EstimateInsufficient { .. }) => Ok(ClockMapping::HostReceive),
            Err(e) => Err(e),
        }
    }

    /// The source recorded for timestamps mapped this way.
    pub fn source(&self) -> TimeSource {
        match self {
            ClockMapping::Model(m) => m.source,
            ClockMapping::HostReceive => TimeSource::HostReceive,
        }
    }

    /// Maps one observation timestamp. A timestamp without `t_dev` falls back to host-receive
    /// time with source `HOST_RECEIVE`, as does the [`ClockMapping::HostReceive`] mapping.
    pub fn map(&self, ts: Timestamp) -> Result<MappedTime, TimeError> {
        match (self, ts.t_dev) {
            (ClockMapping::Model(m), Some(t_dev)) => {
                Ok(MappedTime { t_session_ns: m.map_to_host(t_dev)?, source: m.source })
            }
            _ => Ok(MappedTime { t_session_ns: ts.t_host, source: TimeSource::HostReceive }),
        }
    }

    /// The stream's sync description for [`fuse`]; a host-receive stream carries the
    /// caller-declared host-receive uncertainty `host_receive_sigma_ns`.
    pub fn sync(&self, host_receive_sigma_ns: f64) -> StreamSync {
        match self {
            ClockMapping::Model(m) => m.sync(),
            ClockMapping::HostReceive => {
                StreamSync { source: TimeSource::HostReceive, sigma_ns: host_receive_sigma_ns }
            }
        }
    }
}

/// What the fusion rule needs from one stream: its time source and sync uncertainty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamSync {
    pub source: TimeSource,
    /// Sync uncertainty (one standard deviation), ns.
    pub sigma_ns: f64,
}

/// The verdict of the fusion rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FusionVerdict {
    /// The streams may be fused.
    Ok,
    /// `time_sync_insufficient`: the fused batch must be flagged. `rss_ns` is the
    /// root-sum-square of the two sync uncertainties (infinite if it overflows).
    TimeSyncInsufficient { rss_ns: f64 },
}

impl FusionVerdict {
    /// The flag code, if any.
    pub fn flag(&self) -> Option<&'static str> {
        match self {
            FusionVerdict::Ok => None,
            FusionVerdict::TimeSyncInsufficient { .. } => Some("time_sync_insufficient"),
        }
    }
}

fn check_nonneg(v: f64, what: &'static str) -> Result<f64, TimeError> {
    if v.is_finite() && v >= 0.0 {
        Ok(v)
    } else {
        Err(TimeError::InvalidInput { what })
    }
}

/// The fusion rule (REQ-HAL.ACQ.TIME-004): passes when both streams share a `HW_TRIGGER`
/// source or both share a `PTP` source, or when `sqrt(σa² + σb²) <= threshold_ns`; otherwise
/// returns `time_sync_insufficient`. Sigmas and the threshold must be finite and non-negative
/// (`time_invalid_input`), checked before the source rule.
pub fn fuse(a: StreamSync, b: StreamSync, threshold_ns: f64) -> Result<FusionVerdict, TimeError> {
    let sa = check_nonneg(a.sigma_ns, "sigma of stream a")?;
    let sb = check_nonneg(b.sigma_ns, "sigma of stream b")?;
    let threshold = check_nonneg(threshold_ns, "threshold")?;
    if a.source == b.source && matches!(a.source, TimeSource::HwTrigger | TimeSource::Ptp) {
        return Ok(FusionVerdict::Ok);
    }
    let qa = sa * sa;
    let qb = sb * sb;
    let ss = qa + qb;
    let rss = if ss.is_finite() {
        fid_num::f::sqrt(ss).map_err(|_| TimeError::NotFinite { what: "root-sum-square" })?
    } else {
        f64::INFINITY
    };
    if rss <= threshold {
        Ok(FusionVerdict::Ok)
    } else {
        Ok(FusionVerdict::TimeSyncInsufficient { rss_ns: rss })
    }
}
