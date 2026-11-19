//! Input distributions and correlation. Spec node `UNC.MODEL.DIST`.
//!
//! The probability distributions an input quantity may carry, their validated parameters
//! (SI values with a unit), closed-form expectation and standard uncertainty (used by LPU),
//! deterministic seeded sampling through the `fid-num` counter-based RNG (used by Monte
//! Carlo), the correlation-matrix check and correlated normal sampling. Every
//! value-affecting choice is pinned in the leaf's Design section
//! (`spec/unc/model/dist/NODE.md`); the summary in each submodule is normative for this
//! code.
//!
//! KNP-1: no FMA, transcendentals only through `fid_num::f`, sums through `fid_num::red`,
//! no `f64::min`/`f64::max`, no hash containers, a NaN is always an error, no panics.
//!
//! # Validation (REQ-UNC.MODEL.DIST-002)
//!
//! Constructors check, in this order: every parameter in declaration order for a finite
//! value and its unit (location and scale parameters share the unit of the first
//! parameter; `nu` and `beta` must be [`UnitCode::One`]); then the relations in the order
//! `sigma > 0`, `b > a`, `b - a` finite (named `b`), `a <= c <= b`, `s > 0`, `nu > 2`,
//! `0 <= beta <= 1`. The first failure is [`DistError::InvalidParameter`] naming that
//! parameter.
//!
//! # Moments (REQ-UNC.MODEL.DIST-003)
//!
//! Expectation: `mu` (normal, scaled-t); `0.5 * a + 0.5 * b` (rectangular, arcsine,
//! trapezoidal); `red::sum([a, b, c]) / 3` (triangular). Standard uncertainty: `sigma`;
//! `(b - a) / sqrt(12)`; `red::norm2([b - a, c - a, b - c]) / 6` (triangular: the same
//! quantity as `sqrt((a^2 + b^2 + c^2 - ab - ac - bc) / 18)`, evaluated without
//! cancellation); `(b - a) / sqrt(8)`; `s * sqrt(nu / (nu - 2))`; with `h = (b - a) * 0.5`,
//! `h * sqrt((1 + beta * beta) / 6)`. `sqrt` is `fid_num::f::sqrt`.

mod corr;
mod normal;
mod sample;

pub use corr::{validate_correlation, Correlation, MvNormal, CORR_EPS_FACTOR, MV_REL_TOL_FACTOR};
pub use normal::{ppnd16, AS241_A, AS241_B, AS241_C, AS241_D, AS241_E, AS241_F};
pub use sample::{
    open_unit, GAMMA_ATTEMPTS, GAMMA_STREAM_BIT, MAX_GAMMA_INDEX, MAX_MV_INDEX, MV_STREAM_BIT, STREAM_LIMIT,
};

use core::fmt;
use core::ops::Range;
use fid_num::{f, red, NumError};

/// The unit of a distribution parameter: the coherent SI units of the kernel
/// (CONSTITUTION §4), spelled as in [`crate::statement::UNITS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCode {
    /// `si:m`
    Metre,
    /// `si:rad`
    Radian,
    /// `si:1` (dimensionless)
    One,
}

impl UnitCode {
    /// The unit code string (`si:m`, `si:rad`, `si:1`).
    pub const fn as_str(self) -> &'static str {
        match self {
            UnitCode::Metre => "si:m",
            UnitCode::Radian => "si:rad",
            UnitCode::One => "si:1",
        }
    }

    /// Parses a unit code string; `None` for anything else.
    pub fn parse(s: &str) -> Option<UnitCode> {
        match s {
            "si:m" => Some(UnitCode::Metre),
            "si:rad" => Some(UnitCode::Radian),
            "si:1" => Some(UnitCode::One),
            _ => None,
        }
    }
}

/// A parameter value in SI with its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub unit: UnitCode,
}

impl Quantity {
    /// A value with a unit.
    pub const fn new(value: f64, unit: UnitCode) -> Quantity {
        Quantity { value, unit }
    }
}

/// Errors of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistError {
    /// `invalid_parameter{name}` (REQ-UNC.MODEL.DIST-002); also used for a sampling
    /// `stream` or `index` outside the addressable range.
    InvalidParameter { name: &'static str },
    /// `invalid_correlation{reason}` (REQ-UNC.MODEL.DIST-005).
    InvalidCorrelation { reason: &'static str },
    /// The gamma rejection loop of a scaled-t sample used all [`GAMMA_ATTEMPTS`] attempts
    /// of its sub-stream (probability far below `1e-300` per sample).
    SamplingExhausted { index: u64 },
    /// A result buffer could not be reserved.
    Allocation,
    /// A numeric error from `fid-num` (overflow, domain).
    Num(NumError),
}

impl DistError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            DistError::InvalidParameter { .. } => "invalid_parameter",
            DistError::InvalidCorrelation { .. } => "invalid_correlation",
            DistError::SamplingExhausted { .. } => "sampling_exhausted",
            DistError::Allocation => "allocation_failed",
            DistError::Num(_) => "numeric",
        }
    }
}

impl From<NumError> for DistError {
    fn from(e: NumError) -> Self {
        DistError::Num(e)
    }
}

impl fmt::Display for DistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistError::InvalidParameter { name } => write!(f, "invalid_parameter: {name}"),
            DistError::InvalidCorrelation { reason } => write!(f, "invalid_correlation: {reason}"),
            DistError::SamplingExhausted { index } => write!(f, "sampling_exhausted at index {index}"),
            DistError::Allocation => write!(f, "allocation_failed"),
            DistError::Num(e) => write!(f, "numeric: {e}"),
        }
    }
}

impl core::error::Error for DistError {}

/// The seven distribution kinds (REQ-UNC.MODEL.DIST-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Rect,
    Tri,
    Arcsine,
    ScaledT,
    Trap,
    MvNormal,
}

impl Kind {
    /// All kinds, in declaration order.
    pub const ALL: [Kind; 7] =
        [Kind::Normal, Kind::Rect, Kind::Tri, Kind::Arcsine, Kind::ScaledT, Kind::Trap, Kind::MvNormal];

    /// The algorithm ID `unc.dist.<kind>@1.0.0`.
    pub const fn algorithm_id(self) -> &'static str {
        match self {
            Kind::Normal => "unc.dist.normal@1.0.0",
            Kind::Rect => "unc.dist.rect@1.0.0",
            Kind::Tri => "unc.dist.tri@1.0.0",
            Kind::Arcsine => "unc.dist.arcsine@1.0.0",
            Kind::ScaledT => "unc.dist.scaled_t@1.0.0",
            Kind::Trap => "unc.dist.trap@1.0.0",
            Kind::MvNormal => "unc.dist.mv_normal@1.0.0",
        }
    }
}

/// Validated parameters of a univariate distribution (SI values; the unit is on the
/// [`Distribution`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Params {
    /// Normal with mean `mu` and standard deviation `sigma > 0`.
    Normal { mu: f64, sigma: f64 },
    /// Rectangular on `[a, b]`, `a < b`.
    Rect { a: f64, b: f64 },
    /// Triangular on `[a, b]` with mode `c`, `a <= c <= b`.
    Tri { a: f64, b: f64, c: f64 },
    /// Arcsine (U-shaped) on `[a, b]`.
    Arcsine { a: f64, b: f64 },
    /// Student t with `nu > 2` degrees of freedom, scaled by `s > 0` and shifted by `mu`.
    ScaledT { mu: f64, s: f64, nu: f64 },
    /// Symmetric trapezoid on `[a, b]` whose top has half-width `beta * (b - a) / 2`.
    Trap { a: f64, b: f64, beta: f64 },
}

/// A validated univariate distribution. Fields are private so every value has passed the
/// constructor checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distribution {
    params: Params,
    unit: UnitCode,
}

fn bad(name: &'static str) -> DistError {
    DistError::InvalidParameter { name }
}

/// Per-parameter check: finite value and the required unit.
fn param(name: &'static str, q: Quantity, unit: UnitCode) -> Result<f64, DistError> {
    if q.value.is_finite() && q.unit == unit {
        Ok(q.value)
    } else {
        Err(bad(name))
    }
}

/// `b > a` and a finite width, both named `b`.
fn interval(a: f64, b: f64) -> Result<(), DistError> {
    if b > a && (b - a).is_finite() {
        Ok(())
    } else {
        Err(bad("b"))
    }
}

fn finite(op: &'static str, x: f64) -> Result<f64, DistError> {
    Ok(fid_num::check_finite(op, x)?)
}

impl Distribution {
    /// normal(mu, sigma).
    pub fn normal(mu: Quantity, sigma: Quantity) -> Result<Distribution, DistError> {
        let u = mu.unit;
        let (mu, sigma) = (param("mu", mu, u)?, param("sigma", sigma, u)?);
        if sigma <= 0.0 {
            return Err(bad("sigma"));
        }
        Ok(Distribution { params: Params::Normal { mu, sigma }, unit: u })
    }

    /// rectangular(a, b).
    pub fn rect(a: Quantity, b: Quantity) -> Result<Distribution, DistError> {
        let u = a.unit;
        let (a, b) = (param("a", a, u)?, param("b", b, u)?);
        interval(a, b)?;
        Ok(Distribution { params: Params::Rect { a, b }, unit: u })
    }

    /// triangular(a, b, c) with mode `c`.
    pub fn tri(a: Quantity, b: Quantity, c: Quantity) -> Result<Distribution, DistError> {
        let u = a.unit;
        let (a, b, c) = (param("a", a, u)?, param("b", b, u)?, param("c", c, u)?);
        interval(a, b)?;
        if !(a <= c && c <= b) {
            return Err(bad("c"));
        }
        Ok(Distribution { params: Params::Tri { a, b, c }, unit: u })
    }

    /// arcsine(a, b).
    pub fn arcsine(a: Quantity, b: Quantity) -> Result<Distribution, DistError> {
        let u = a.unit;
        let (a, b) = (param("a", a, u)?, param("b", b, u)?);
        interval(a, b)?;
        Ok(Distribution { params: Params::Arcsine { a, b }, unit: u })
    }

    /// scaled-t(mu, s, nu); `nu` is dimensionless.
    pub fn scaled_t(mu: Quantity, s: Quantity, nu: Quantity) -> Result<Distribution, DistError> {
        let u = mu.unit;
        let (mu, s, nu) = (param("mu", mu, u)?, param("s", s, u)?, param("nu", nu, UnitCode::One)?);
        if s <= 0.0 {
            return Err(bad("s"));
        }
        if nu <= 2.0 {
            return Err(bad("nu"));
        }
        Ok(Distribution { params: Params::ScaledT { mu, s, nu }, unit: u })
    }

    /// trapezoidal(a, b, beta); `beta` is dimensionless.
    pub fn trap(a: Quantity, b: Quantity, beta: Quantity) -> Result<Distribution, DistError> {
        let u = a.unit;
        let (a, b, beta) = (param("a", a, u)?, param("b", b, u)?, param("beta", beta, UnitCode::One)?);
        interval(a, b)?;
        if !(0.0..=1.0).contains(&beta) {
            return Err(bad("beta"));
        }
        Ok(Distribution { params: Params::Trap { a, b, beta }, unit: u })
    }

    /// The validated parameters.
    pub fn params(&self) -> Params {
        self.params
    }

    /// The unit of the quantity (and of every location and scale parameter).
    pub fn unit(&self) -> UnitCode {
        self.unit
    }

    /// The distribution kind.
    pub fn kind(&self) -> Kind {
        match self.params {
            Params::Normal { .. } => Kind::Normal,
            Params::Rect { .. } => Kind::Rect,
            Params::Tri { .. } => Kind::Tri,
            Params::Arcsine { .. } => Kind::Arcsine,
            Params::ScaledT { .. } => Kind::ScaledT,
            Params::Trap { .. } => Kind::Trap,
        }
    }

    /// The expectation from the closed form (REQ-UNC.MODEL.DIST-003).
    pub fn expectation(&self) -> Result<f64, DistError> {
        const OP: &str = "unc.dist.expectation";
        let e = match self.params {
            Params::Normal { mu, .. } | Params::ScaledT { mu, .. } => mu,
            Params::Rect { a, b } | Params::Arcsine { a, b } | Params::Trap { a, b, .. } => 0.5 * a + 0.5 * b,
            Params::Tri { a, b, c } => red::sum(&[a, b, c])? / 3.0,
        };
        finite(OP, e)
    }

    /// The standard uncertainty (standard deviation) from the closed form
    /// (REQ-UNC.MODEL.DIST-003).
    pub fn std_uncertainty(&self) -> Result<f64, DistError> {
        const OP: &str = "unc.dist.std_uncertainty";
        let u = match self.params {
            Params::Normal { sigma, .. } => sigma,
            Params::Rect { a, b } => (b - a) / f::sqrt(12.0)?,
            Params::Tri { a, b, c } => red::norm2(&[b - a, c - a, b - c])? / 6.0,
            Params::Arcsine { a, b } => (b - a) / f::sqrt(8.0)?,
            Params::ScaledT { s, nu, .. } => s * f::sqrt(nu / (nu - 2.0))?,
            Params::Trap { a, b, beta } => (b - a) * 0.5 * f::sqrt((1.0 + beta * beta) / 6.0)?,
        };
        finite(OP, u)
    }

    /// Sample `index` of `stream` under `seed`: a pure function of the RNG address
    /// `(seed, stream, index)` (REQ-UNC.MODEL.DIST-004; rules in the `sample` module).
    pub fn sample_at(&self, seed: u64, stream: u64, index: u64) -> Result<f64, DistError> {
        sample::check_address(stream, index, self.kind())?;
        sample::draw(&self.params, seed, stream, index)
    }

    /// Samples `range.start .. range.end` of `stream` under `seed`, in index order. Each
    /// element equals [`Distribution::sample_at`] of its index, so ranges drawn in any order
    /// or on any thread concatenate to the same bytes (REQ-UNC.MODEL.DIST-004, -006).
    pub fn sample(&self, seed: u64, stream: u64, range: Range<u64>) -> Result<Vec<f64>, DistError> {
        if range.end < range.start {
            return Err(bad("index"));
        }
        if range.end > range.start {
            sample::check_address(stream, range.end - 1, self.kind())?;
        } else {
            sample::check_address(stream, range.start, self.kind())?;
        }
        let len = usize::try_from(range.end - range.start).map_err(|_| DistError::Allocation)?;
        let mut out = Vec::new();
        out.try_reserve_exact(len).map_err(|_| DistError::Allocation)?;
        for i in range {
            out.push(sample::draw(&self.params, seed, stream, i)?);
        }
        Ok(out)
    }
}
