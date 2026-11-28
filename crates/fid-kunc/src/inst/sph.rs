//! Spherical observation covariance for trackers and total stations. Spec node
//! `UNC.INST.PORT.SPH`, algorithm [`ALGORITHM_ID`] = `unc.inst.sph_cov@1.0.0`.
//!
//! An instrument that measures a range `r` and two angles (azimuth `az`, elevation `el`)
//! has the Cartesian point `x = r cos(el) cos(az)`, `y = r cos(el) sin(az)`,
//! `z = r sin(el)` (radians, metres). [`sph_cov`] returns `J C J^T` in m², where `C` is the
//! declared covariance of `(r, az, el)` and `J` the closed-form Jacobian of that map at the
//! observation (REQ-UNC.INST.PORT.SPH-001). Drivers whose angles use another convention
//! (zenith instead of elevation, for example) convert before calling.
//!
//! # Rules (pinned; first failure wins)
//!
//! 1. Parameters in the order `sigma_r0`, `sigma_r1`, `sigma_az`, `sigma_el`: NaN or an
//!    infinity is `invalid_parameter{name, not_finite}`, a value below zero
//!    `invalid_parameter{name, negative}` (`-0.0` is zero). Then `rho_r_az`, `rho_r_el`,
//!    `rho_az_el`: `not_finite`, or `out_of_range` when `|rho| > 1`
//!    (REQ-UNC.INST.PORT.SPH-003).
//! 2. The three correlations must hold at once. With `a = rho_r_az`, `b = rho_r_el`,
//!    `c = rho_az_el`, the correlation matrix `[[1, a, b], [a, 1, c], [b, c, 1]]` is positive
//!    semi-definite exactly when its determinant `1 - a² - b² - c² + 2abc` is at least zero
//!    (its 1x1 and 2x2 principal minors are `1` and `1 - rho²`, already non-negative). Its
//!    sign is decided exactly, with no rounding: each correlation is split into its integer
//!    significand and binary exponent (`x = ±m 2^e`, `e >= -1074`), the five terms `1`,
//!    `a²`, `b²`, `c²` and `2abc` are formed as exact integer products (at most 159 bits) and
//!    added at their exponents into two fixed-width integer accumulators (positive and
//!    negative terms, [`DET_ACC_BITS`] bits, bit 0 weighing `2^-3222`), which are then
//!    compared. No floating-point operation is involved, so a subnormal or underflowing
//!    product is exact too (a Dekker split product is not error-free there: with `a = 1`,
//!    `b = 1e-300`, `c = 2e-300` the exact determinant is `-(b - c)²`, below zero, while every
//!    rounded or split product of `b` and `c` underflows to zero).
//!    `det < 0` is `invalid_parameter{correlation, not_positive_semidefinite}` (three
//!    pairwise-valid correlations 0.9, 0.9 and -0.9 have `det = -1.888`; `-0.5 - 2^-52` three
//!    times has `det = -(1.5 + 2^-52)² 2^-51`). `det == 0` exactly is accepted as
//!    [`CorrelationCheck::Singular`] (`|rho| = 1`, or the equicorrelation `-0.5`), `det > 0`
//!    is [`CorrelationCheck::PositiveDefinite`].
//!    The final eigenvalue check of the covariance is `UNC.INST.PORT.EVAL`'s.
//! 3. The observation in the order `r`, `az`, `el`: `invalid_observation{name, not_finite}`,
//!    and `invalid_observation{r, negative}` for `r < 0`.
//! 4. `sigma_r = sigma_r0 + sigma_r1 * r` (two roundings, no FMA). `ca = cos(az)`,
//!    `sa = sin(az)`, `ce = cos(el)`, `se = sin(el)` through `fid_num::f`; `rce = r * ce`,
//!    `rse = r * se`.
//! 5. `J = [[ce*ca, -(rce*sa), -(rse*ca)], [ce*sa, rce*ca, -(rse*sa)], [se, 0, rce]]`
//!    (rows x, y, z; columns r, az, el), and the symmetric
//!    `C = [[sr*sr, c01, c02], [c01, saz*saz, c12], [c02, c12, sel*sel]]` with
//!    `c01 = (rho_r_az*sr)*saz`, `c02 = (rho_r_el*sr)*sel`, `c12 = (rho_az_el*saz)*sel`.
//! 6. `B = J C` with `B_ik = (J_i0*C_0k + J_i1*C_1k) + J_i2*C_2k`, then for `i <= l` only
//!    `S_il = (B_i0*J_l0 + B_i1*J_l1) + B_i2*J_l2`, mirrored to `S_li`, so the result is
//!    exactly symmetric.
//! 7. Every entry plus `0.0` (so `-0.0` becomes `+0.0`); a non-finite entry (an overflow) is
//!    `numeric` with the `fid-num` error.
//!
//! # Accuracy and the validated domain (REQ-UNC.INST.PORT.SPH-002)
//!
//! `J = R D` with `R` orthogonal (the unit vectors along `r`, `az` and `el`) and
//! `D = diag(1, r ce, r)`, so `|J| |C| |J|^T = |R| |D C D| |R|^T`, whose Frobenius norm is at
//! most 3 times that of `S`. Entry rounding of `J` and `C` and the two products stay below
//! `16 * 2^-53` relative to that, so the relative Frobenius error is below about `5.4e-15`
//! while no intermediate underflows. The spec states and the fixtures test the bound for
//! `0.1 m <= r <= 100 m` and `|el| <= 1.5 rad`; the fixtures also keep `|az| <= 2 pi`.
//! [`DomainNotes`] says when an observation leaves that domain (`range_outside`,
//! `elevation_outside`, `azimuth_outside`), when the largest entry is below [`SCALE_MIN`]
//! `= 2^-962` (`scale_below_validated`: underflowed intermediates, each off by up to
//! `2^-1075`, may then matter) and when every entry is zero (`zero_covariance`, for example
//! all four sigmas zero). The covariance is still returned, labelled; it is never silently
//! claimed to meet the bound.
//!
//! Determinism: D0. KNP-1: no FMA, no `f64::min`/`max`, transcendentals only through
//! `fid_num::f`, a NaN is always an error, no hash containers, no panics. The parameters are
//! declared (claimed) model values, so the covariance is derived, never measured (D-045).

use core::cmp::Ordering;

use fid_num::NumError;

use super::{Cov3, InstError, ValueReason};

/// Algorithm ID of [`sph_cov`].
pub const ALGORITHM_ID: &str = "unc.inst.sph_cov@1.0.0";

/// Width in bits of each exact determinant accumulator (module rule 2): 56 limbs of 64 bits.
/// The largest term, `1`, sits at bit 3222 and the sum of the positive terms is below 3, so
/// 3225 bits would do.
pub const DET_ACC_BITS: usize = DET_LIMBS * 64;

const DET_LIMBS: usize = 56;

/// Binary exponent of bit 0 of the determinant accumulators: `3 * (-1074)`.
const DET_BASE_EXP: i32 = -3222;

/// Smallest largest-entry magnitude inside the validated domain: `2^-962`.
pub const SCALE_MIN: f64 = 2.565_335_500_811_485_2e-290;

/// Lower range bound of the validated domain (m): the double nearest 0.1.
pub const R_MIN: f64 = 0.1;

/// Upper range bound of the validated domain (m).
pub const R_MAX: f64 = 100.0;

/// Largest `|el|` of the validated domain (rad).
pub const EL_MAX: f64 = 1.5;

/// Largest `|az|` of the validated domain (rad): the double nearest `2 pi`.
pub const AZ_MAX: f64 = core::f64::consts::TAU;

const OP: &str = "unc.inst.sph_cov";

/// Declared parameters of a spherical observation model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphParams {
    /// Constant part of the range standard uncertainty (m).
    pub sigma_r0: f64,
    /// Range-proportional part of the range standard uncertainty (m/m).
    pub sigma_r1: f64,
    /// Azimuth standard uncertainty (rad).
    pub sigma_az: f64,
    /// Elevation standard uncertainty (rad).
    pub sigma_el: f64,
    /// Correlation of range and azimuth, in `[-1, 1]`.
    pub rho_r_az: f64,
    /// Correlation of range and elevation, in `[-1, 1]`.
    pub rho_r_el: f64,
    /// Correlation of azimuth and elevation, in `[-1, 1]`.
    pub rho_az_el: f64,
}

impl SphParams {
    /// Parameters with all three correlations zero.
    pub const fn uncorrelated(sigma_r0: f64, sigma_r1: f64, sigma_az: f64, sigma_el: f64) -> SphParams {
        SphParams { sigma_r0, sigma_r1, sigma_az, sigma_el, rho_r_az: 0.0, rho_r_el: 0.0, rho_az_el: 0.0 }
    }
}

/// Outcome of the joint correlation test (module rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationCheck {
    /// `positive_definite`: the exact determinant is above zero.
    PositiveDefinite,
    /// `singular`: the exact determinant is zero; accepted (positive semi-definite).
    Singular,
}

impl CorrelationCheck {
    /// Stable code.
    pub const fn code(self) -> &'static str {
        match self {
            CorrelationCheck::PositiveDefinite => "positive_definite",
            CorrelationCheck::Singular => "singular",
        }
    }
}

/// Where an observation lies relative to the validated domain (module section on accuracy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainNotes {
    range_outside: bool,
    elevation_outside: bool,
    azimuth_outside: bool,
    zero_covariance: bool,
    scale_below_validated: bool,
}

impl DomainNotes {
    /// `true` when no note applies: the stated accuracy bound holds.
    pub fn is_validated(&self) -> bool {
        !(self.range_outside
            || self.elevation_outside
            || self.azimuth_outside
            || self.zero_covariance
            || self.scale_below_validated)
    }

    /// `r` is outside `[R_MIN, R_MAX]`.
    pub fn range_outside(&self) -> bool {
        self.range_outside
    }

    /// `|el| > EL_MAX`.
    pub fn elevation_outside(&self) -> bool {
        self.elevation_outside
    }

    /// `|az| > AZ_MAX`.
    pub fn azimuth_outside(&self) -> bool {
        self.azimuth_outside
    }

    /// Every entry of the covariance is zero.
    pub fn zero_covariance(&self) -> bool {
        self.zero_covariance
    }

    /// The largest entry is non-zero but below `SCALE_MIN`.
    pub fn scale_below_validated(&self) -> bool {
        self.scale_below_validated
    }

    /// The codes of the notes that apply, in the order `range_outside`,
    /// `elevation_outside`, `azimuth_outside`, `zero_covariance`, `scale_below_validated`.
    pub fn codes(&self) -> Vec<&'static str> {
        [
            (self.range_outside, "range_outside"),
            (self.elevation_outside, "elevation_outside"),
            (self.azimuth_outside, "azimuth_outside"),
            (self.zero_covariance, "zero_covariance"),
            (self.scale_below_validated, "scale_below_validated"),
        ]
        .into_iter()
        .filter_map(|(on, code)| on.then_some(code))
        .collect()
    }
}

/// Result of [`sph_cov`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphCov {
    cov: Cov3,
    sigma_r: f64,
    correlation: CorrelationCheck,
    notes: DomainNotes,
}

impl SphCov {
    /// The covariance `J C J^T` in m².
    pub fn cov(&self) -> &Cov3 {
        &self.cov
    }

    /// The range standard uncertainty `sigma_r0 + sigma_r1 * r` used (m; `-0.0` as `+0.0`).
    pub fn sigma_r(&self) -> f64 {
        self.sigma_r
    }

    /// The outcome of the joint correlation test.
    pub fn correlation(&self) -> CorrelationCheck {
        self.correlation
    }

    /// The validated-domain notes.
    pub fn notes(&self) -> &DomainNotes {
        &self.notes
    }
}

fn param(name: &'static str, v: f64) -> Result<(), InstError> {
    if !v.is_finite() {
        return Err(InstError::InvalidParameter { name, reason: ValueReason::NotFinite });
    }
    if v < 0.0 {
        return Err(InstError::InvalidParameter { name, reason: ValueReason::Negative });
    }
    Ok(())
}

fn correlation(name: &'static str, v: f64) -> Result<(), InstError> {
    if !v.is_finite() {
        return Err(InstError::InvalidParameter { name, reason: ValueReason::NotFinite });
    }
    if v.abs() > 1.0 {
        return Err(InstError::InvalidParameter { name, reason: ValueReason::OutOfRange });
    }
    Ok(())
}

fn observation(name: &'static str, v: f64) -> Result<(), InstError> {
    if !v.is_finite() {
        return Err(InstError::InvalidObservation { name, reason: ValueReason::NotFinite });
    }
    Ok(())
}

/// `x = (-1)^neg * m * 2^e` exactly, for a finite double (`e >= -1074`).
fn split(x: f64) -> (bool, u64, i32) {
    let bits = x.to_bits();
    let neg = bits >> 63 == 1;
    let field = (bits >> 52) & 0x7ff;
    let frac = bits & ((1_u64 << 52) - 1);
    // `field` is at most 0x7ff, so the cast is exact.
    let biased = field as i32;
    if field == 0 {
        (neg, frac, -1074)
    } else {
        (neg, frac | (1_u64 << 52), biased - 1075)
    }
}

/// The exact product `a * b * c` of three 53-bit integers as three little-endian limbs.
fn mul3(a: u64, b: u64, c: u64) -> [u64; 3] {
    let ab = u128::from(a) * u128::from(b);
    let ab_lo = u128::from(ab as u64);
    let ab_hi = ab >> 64;
    let c = u128::from(c);
    // `ab_hi < 2^42` and `c < 2^53`, so neither product overflows.
    let lo = ab_lo * c;
    let hi = ab_hi * c;
    let mid = (lo >> 64) + u128::from(hi as u64);
    [lo as u64, mid as u64, ((hi >> 64) + (mid >> 64)) as u64]
}

const DET_RANGE: NumError = NumError::Domain { op: OP, detail: "determinant term outside the exact accumulator" };

/// Adds the integer `mag * 2^exp` to the accumulator `acc` (bit 0 weighing `2^DET_BASE_EXP`).
/// A term the accumulator cannot hold, or a carry out of it, is an error, never dropped
/// (neither can happen for correlations in `[-1, 1]`).
fn add_term(acc: &mut [u64; DET_LIMBS], mag: [u64; 3], exp: i32) -> Result<(), InstError> {
    let shift = usize::try_from(exp - DET_BASE_EXP).map_err(|_| InstError::Numeric(DET_RANGE))?;
    let (word, bit) = (shift / 64, shift % 64);
    let [m0, m1, m2] = mag;
    let words = if bit == 0 {
        [m0, m1, m2, 0]
    } else {
        let back = 64 - bit;
        [m0 << bit, (m1 << bit) | (m0 >> back), (m2 << bit) | (m1 >> back), m2 >> back]
    };
    if word + words.len() > DET_LIMBS {
        return Err(InstError::Numeric(DET_RANGE));
    }
    let mut carry = 0_u64;
    for (k, slot) in acc.iter_mut().skip(word).enumerate() {
        let add = words.get(k).copied().unwrap_or(0);
        if k >= words.len() && carry == 0 {
            return Ok(());
        }
        let (s1, c1) = slot.overflowing_add(add);
        let (s2, c2) = s1.overflowing_add(carry);
        *slot = s2;
        carry = u64::from(c1) + u64::from(c2);
    }
    if carry == 0 {
        Ok(())
    } else {
        Err(InstError::Numeric(DET_RANGE))
    }
}

/// The exact sign of `1 - a² - b² - c² + 2abc` for finite `a`, `b`, `c` (module rule 2).
fn det_sign(a: f64, b: f64, c: f64) -> Result<Ordering, InstError> {
    let (sa, ma, ea) = split(a);
    let (sb, mb, eb) = split(b);
    let (sc, mc, ec) = split(c);
    let mut pos = [0_u64; DET_LIMBS];
    let mut neg = [0_u64; DET_LIMBS];
    add_term(&mut pos, [1, 0, 0], 0)?;
    add_term(&mut neg, mul3(ma, ma, 1), 2 * ea)?;
    add_term(&mut neg, mul3(mb, mb, 1), 2 * eb)?;
    add_term(&mut neg, mul3(mc, mc, 1), 2 * ec)?;
    let abc = mul3(ma, mb, mc);
    let abc_exp = ea + eb + ec + 1;
    if sa ^ sb ^ sc {
        add_term(&mut neg, abc, abc_exp)?;
    } else {
        add_term(&mut pos, abc, abc_exp)?;
    }
    // Most significant limb first: lexicographic order is numeric order.
    Ok(pos.iter().rev().cmp(neg.iter().rev()))
}

/// `(a0*b0 + a1*b1) + a2*b2`, each product and sum rounded once (no FMA).
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    (a0 * b0 + a1 * b1) + a2 * b2
}

/// Row `row` times the matrix whose columns are read from rows `c`: `row · C`.
fn row_times(row: [f64; 3], c: [[f64; 3]; 3]) -> [f64; 3] {
    let [c0, c1, c2] = c;
    let [c00, c01, c02] = c0;
    let [c10, c11, c12] = c1;
    let [c20, c21, c22] = c2;
    [dot3(row, [c00, c10, c20]), dot3(row, [c01, c11, c21]), dot3(row, [c02, c12, c22])]
}

/// The Cartesian covariance (m²) of the spherical observation `(r, az, el)` under the
/// declared `params`, algorithm `unc.inst.sph_cov@1.0.0` (module rules 1 to 7).
pub fn sph_cov(params: &SphParams, r: f64, az: f64, el: f64) -> Result<SphCov, InstError> {
    let p = params;
    param("sigma_r0", p.sigma_r0)?;
    param("sigma_r1", p.sigma_r1)?;
    param("sigma_az", p.sigma_az)?;
    param("sigma_el", p.sigma_el)?;
    correlation("rho_r_az", p.rho_r_az)?;
    correlation("rho_r_el", p.rho_r_el)?;
    correlation("rho_az_el", p.rho_az_el)?;
    let correlation_check = match det_sign(p.rho_r_az, p.rho_r_el, p.rho_az_el)? {
        Ordering::Less => {
            return Err(InstError::InvalidParameter {
                name: "correlation",
                reason: ValueReason::NotPositiveSemidefinite,
            })
        }
        Ordering::Equal => CorrelationCheck::Singular,
        Ordering::Greater => CorrelationCheck::PositiveDefinite,
    };
    observation("r", r)?;
    observation("az", az)?;
    observation("el", el)?;
    if r < 0.0 {
        return Err(InstError::InvalidObservation { name: "r", reason: ValueReason::Negative });
    }

    let sr = p.sigma_r0 + p.sigma_r1 * r;
    let ca = fid_num::f::cos(az)?;
    let sa = fid_num::f::sin(az)?;
    let ce = fid_num::f::cos(el)?;
    let se = fid_num::f::sin(el)?;
    let rce = r * ce;
    let rse = r * se;
    let j0 = [ce * ca, -(rce * sa), -(rse * ca)];
    let j1 = [ce * sa, rce * ca, -(rse * sa)];
    let j2 = [se, 0.0, rce];

    let (saz, sel) = (p.sigma_az, p.sigma_el);
    let c01 = (p.rho_r_az * sr) * saz;
    let c02 = (p.rho_r_el * sr) * sel;
    let c12 = (p.rho_az_el * saz) * sel;
    let cm = [[sr * sr, c01, c02], [c01, saz * saz, c12], [c02, c12, sel * sel]];

    let b0 = row_times(j0, cm);
    let b1 = row_times(j1, cm);
    let b2 = row_times(j2, cm);
    let cov =
        Cov3::from_upper(OP, [dot3(b0, j0), dot3(b0, j1), dot3(b0, j2), dot3(b1, j1), dot3(b1, j2), dot3(b2, j2)])?;
    let sigma_r = fid_num::check_finite(OP, sr)? + 0.0;

    let mut largest = 0.0_f64;
    for row in cov.rows() {
        for v in row {
            largest = fid_num::f::total_max(largest, v.abs())?;
        }
    }
    let notes = DomainNotes {
        range_outside: !(R_MIN..=R_MAX).contains(&r),
        elevation_outside: el.abs() > EL_MAX,
        azimuth_outside: az.abs() > AZ_MAX,
        zero_covariance: largest == 0.0,
        scale_below_validated: largest != 0.0 && largest < SCALE_MIN,
    };
    Ok(SphCov { cov, sigma_r, correlation: correlation_check, notes })
}
