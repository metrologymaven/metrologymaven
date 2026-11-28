//! Instrument observation covariances. Spec node `UNC.INST.PORT` (kernel children).
//!
//! Each portable or optical instrument kind has a pure kernel function that turns one
//! observation and the model's typed parameters into a 3x3 covariance in m² in the
//! instrument frame. Implemented: `UNC.INST.PORT.SPH` (spherical observations of trackers
//! and total stations, [`sph::sph_cov`]). Not built here: `UNC.INST.PORT.ARM`,
//! `UNC.INST.PORT.OPT`, and the entry point with its parameter extraction, range check and
//! positive semi-definite check (`UNC.INST.PORT.EVAL`, crate `fid-unc-inst`).
//!
//! The parameters are what an instrument model declares (claimed data). A covariance from
//! these functions is derived from that declaration; it is never a measured uncertainty, and
//! the caller records which model it came from (D-045).

use core::fmt;

use fid_num::NumError;

pub mod sph;

/// A 3x3 covariance in m², exactly symmetric, every entry finite, `-0.0` written as `+0.0`.
///
/// Only the kernel functions of this module build one, so the invariants hold for every
/// value of the type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cov3 {
    m: [[f64; 3]; 3],
}

impl Cov3 {
    /// Builds from the upper triangle (row-major `00, 01, 02, 11, 12, 22`), mirrors it, maps
    /// `-0.0` to `+0.0` and refuses a non-finite entry with the `fid-num` error.
    pub(crate) fn from_upper(op: &'static str, u: [f64; 6]) -> Result<Cov3, NumError> {
        let [a00, a01, a02, a11, a12, a22] = u;
        let mut out = [0.0_f64; 6];
        for (o, v) in out.iter_mut().zip([a00, a01, a02, a11, a12, a22]) {
            *o = fid_num::check_finite(op, v)? + 0.0;
        }
        let [s00, s01, s02, s11, s12, s22] = out;
        Ok(Cov3 { m: [[s00, s01, s02], [s01, s11, s12], [s02, s12, s22]] })
    }

    /// Entry `(i, j)`, or `None` when `i` or `j` is not 0, 1 or 2.
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        self.m.get(i).and_then(|row| row.get(j)).copied()
    }

    /// The three rows.
    pub fn rows(&self) -> [[f64; 3]; 3] {
        self.m
    }

    /// The nine entries' IEEE-754 bit patterns, row-major.
    pub fn to_bits(&self) -> [u64; 9] {
        let [[a, b, c], [d, e, f], [g, h, i]] = self.m;
        [a, b, c, d, e, f, g, h, i].map(f64::to_bits)
    }
}

/// Why a parameter or an observation value is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueReason {
    /// `not_finite`: NaN or an infinity.
    NotFinite,
    /// `negative`: below zero (`-0.0` is accepted as zero).
    Negative,
    /// `out_of_range`: a correlation with `|rho| > 1`.
    OutOfRange,
    /// `not_positive_semidefinite`: the declared correlations cannot all hold at once.
    NotPositiveSemidefinite,
}

impl ValueReason {
    /// Stable code.
    pub const fn code(self) -> &'static str {
        match self {
            ValueReason::NotFinite => "not_finite",
            ValueReason::Negative => "negative",
            ValueReason::OutOfRange => "out_of_range",
            ValueReason::NotPositiveSemidefinite => "not_positive_semidefinite",
        }
    }
}

/// Errors of the instrument kernel functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstError {
    /// `invalid_parameter`: the model parameter `name` is refused for `reason`
    /// (REQ-UNC.INST.PORT.SPH-003). `name` is `correlation` for a joint refusal of the three
    /// correlations.
    InvalidParameter { name: &'static str, reason: ValueReason },
    /// `invalid_observation`: the observation value `name` is refused for `reason`.
    InvalidObservation { name: &'static str, reason: ValueReason },
    /// `numeric`: a `fid-num` error, for example an overflow to infinity.
    Numeric(NumError),
}

impl InstError {
    /// Stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            InstError::InvalidParameter { .. } => "invalid_parameter",
            InstError::InvalidObservation { .. } => "invalid_observation",
            InstError::Numeric(_) => "numeric",
        }
    }
}

impl From<NumError> for InstError {
    fn from(e: NumError) -> Self {
        InstError::Numeric(e)
    }
}

impl fmt::Display for InstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstError::InvalidParameter { name, reason } => {
                write!(f, "invalid_parameter{{{name}: {}}}", reason.code())
            }
            InstError::InvalidObservation { name, reason } => {
                write!(f, "invalid_observation{{{name}: {}}}", reason.code())
            }
            InstError::Numeric(e) => write!(f, "numeric{{{e:?}}}"),
        }
    }
}

impl std::error::Error for InstError {}
