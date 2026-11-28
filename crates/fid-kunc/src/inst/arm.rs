//! Articulated-arm point covariance without joint data. Spec node `UNC.INST.PORT.ARM`,
//! algorithm [`ALGORITHM_ID`] = `unc.inst.arm_cov@1.0.0`.
//!
//! A probing point `p` (m, instrument frame, arm base at the origin) gets the lateral standard
//! uncertainty `sigma_lat = sigma_p + sigma_d1 ||p||` and the axial one
//! `sigma_ax = axial_factor sigma_lat` along the unit probe direction `u_hat`. [`arm_cov`]
//! returns `Sigma = sigma_lat² I + (sigma_ax² - sigma_lat²) u_hat u_hat^T` in m², or
//! `sigma_lat² I` without a direction (REQ-UNC.INST.PORT.ARM-001). Its eigenvalues are
//! `sigma_lat²` (twice, across the probe) and `sigma_ax²` (along it), so it is positive
//! semi-definite for every accepted input. Pose-dependent (joint-angle) models are not here.
//!
//! # Rules (pinned; first failure wins)
//!
//! 1. Parameters in the order `sigma_p`, `sigma_d1`, `axial_factor` (when given): NaN or an
//!    infinity is `invalid_parameter{name, not_finite}`, a value below zero
//!    `invalid_parameter{name, negative}` (`-0.0` is zero) (REQ-UNC.INST.PORT.ARM-003). An
//!    absent `axial_factor` is 1.
//! 2. The point in the order `p_x`, `p_y`, `p_z`: `invalid_observation{name, not_finite}`.
//! 3. The direction, when given, in the order `u_x`, `u_y`, `u_z`:
//!    `invalid_direction{name, not_finite}`. Then `invalid_direction{u, below_minimum_norm}`
//!    when `||u|| <` [`MIN_DIRECTION_NORM`] (the double nearest 1e-12; REQ-002). Without a
//!    direction, an `axial_factor` other than 1 cannot be applied and is refused as
//!    `invalid_direction{u, missing}` (a deviation, stated in the NODE.md build notes: the
//!    isotropic result would silently drop the declared axial term).
//!    Both norm tests are decided exactly, with no rounding: `u.u` is compared with
//!    `t²` (and with `(1 - d)²` and `(1 + d)²` below) by splitting every component into its
//!    integer significand and binary exponent and adding the exact integer squares at their
//!    exponents into fixed-width integer accumulators ([`NORM_ACC_BITS`] bits, bit 0 weighing
//!    `2^-2148`), subnormals included.
//! 4. Norms (`||p||`, and `||u||` for the unit direction): with `m` the largest `|x_i|` in
//!    total order and `e = floor(log2 m)` (from the bits, subnormals included),
//!    `a = (-e) / 2` (toward zero) and `b = -e - a`, `v_i = (x_i * 2^a) * 2^b` (exact
//!    except for components below `2^-485 m`, whose rounding is far below `2^-53` of the
//!    norm), `n = sqrt((v_0² + v_1²) + v_2²)` (IEEE-754 correctly rounded, `fid_num::f`),
//!    `||x|| = (n * 2^-a) * 2^-b`, so neither overflows nor underflows on the way; an all-zero
//!    vector has norm 0. A point norm that overflows is `numeric`.
//!    `u_hat_i = v_i / n` (`-0.0` reported as `+0.0`). `u` is always normalised: REQ-002
//!    requires it when `||u||` differs from 1 by more than [`UNIT_TOLERANCE`] (the double
//!    nearest 1e-9, decided exactly as in rule 3: the `direction_off_unit` flag); inside that
//!    window `u` is normalised too rather than used as given, which would put a relative
//!    error of up to `2e-9` into `sigma_ax²` (above the 1e-14 of AC-1).
//! 5. `sigma_lat = sigma_p + sigma_d1 * ||p||`, `sigma_ax = f * sigma_lat`, `s2 = sigma_lat *
//!    sigma_lat` (each rounded once, no FMA; `f` the axial factor).
//! 6. Without a direction, `Sigma = diag(s2, s2, s2)` with `+0.0` off the diagonal. With one,
//!    `d = ((f - 1) * sigma_lat) * ((f + 1) * sigma_lat)` (equal to `sigma_ax² - sigma_lat²`,
//!    but without its cancellation near `f = 1`, and finite whenever `sigma_ax²` is), and for
//!    `i <= l` only `t = d * (u_hat_i * u_hat_l)`, `Sigma_ii = s2 + t`, `Sigma_il = t`,
//!    mirrored to `Sigma_li`, so the result is exactly symmetric.
//! 7. Every entry plus `0.0` (so `-0.0` becomes `+0.0`); a non-finite entry, `sigma_lat` or
//!    `sigma_ax` (an overflow) is `numeric` with the `fid-num` error.
//!
//! # Accuracy and flags
//!
//! To first order in `u = 2^-53`, the relative errors are at most `4.5u` for `sigma_lat`,
//! `10u` for `s2`, `8u` for `u_hat_i u_hat_l`, `14u` for `d` and `23u` for `t`, so the
//! entry-wise error is at most `10u s2 [i = l] + 23u |d| |u_hat_i u_hat_l| + u |Sigma_il|`.
//! With `s2 <= ||Sigma||_F / sqrt(2)` and `|d| = |f² - 1| s2 <= ||Sigma||_F`, the relative
//! Frobenius error is below `(10 sqrt(3/2) + 24) u`, under [`REL_FROBENIUS_BOUND`]
//! `= 37 * 2^-53` (about 4.1e-15), while no intermediate underflows. [`ArmFlags`] says when
//! that may not hold: `scale_below_validated` when `0 < sigma_lat <` [`SIGMA_MIN`]
//! `= 2^-480` (then `s2` and `d` may underflow) or `0 < ||p|| < 2^-1022` (a subnormal norm,
//! rounded to the subnormal grid), and `zero_covariance` when every entry is zero (no
//! relative error is defined; the result is then exactly zero). It also flags
//! `direction_off_unit` (rule 4) and `axial_zero` (`f = 0` with a direction: the declared
//! covariance is singular along the probe, with eigenvalue 0 up to the rounding above). The
//! covariance is still returned, labelled; it is never silently claimed to meet the bound.
//! The final positive semi-definite check of the covariance is `UNC.INST.PORT.EVAL`'s.
//!
//! Determinism: D0. KNP-1: no FMA, no `f64::min`/`max`, the square root only through
//! `fid_num::f`, a NaN is always an error, no hash containers, no panics. The parameters are
//! declared (claimed) model values, so the covariance is derived, never measured (D-045).

use core::cmp::Ordering;

use fid_num::NumError;

use super::{Cov3, InstError, ValueReason};

/// Algorithm ID of [`arm_cov`].
pub const ALGORITHM_ID: &str = "unc.inst.arm_cov@1.0.0";

/// Smallest accepted `||u||` (REQ-UNC.INST.PORT.ARM-002): the double nearest 1e-12.
pub const MIN_DIRECTION_NORM: f64 = 1e-12;

/// `||u||` farther than this from 1 sets `direction_off_unit` (REQ-UNC.INST.PORT.ARM-002):
/// the double nearest 1e-9.
pub const UNIT_TOLERANCE: f64 = 1e-9;

/// Smallest non-zero `sigma_lat` inside the validated domain: `2^-480`.
pub const SIGMA_MIN: f64 = 3.203_332_952_292_961_5e-145;

/// First-order bound on the relative Frobenius error while no flag limits accuracy:
/// `37 * 2^-53`.
pub const REL_FROBENIUS_BOUND: f64 = 37.0 / 9_007_199_254_740_992.0;

/// Width in bits of each exact norm accumulator (module rule 3): 68 limbs of 64 bits. A
/// squared component `m² 2^(2e)` has `m² < 2^106` and `-2148 <= 2e <= 1942`, so every term
/// ends below bit 4196 and a sum of four terms below bit 4198.
pub const NORM_ACC_BITS: usize = ACC_LIMBS * 64;

const ACC_LIMBS: usize = 68;

/// Binary exponent of bit 0 of the norm accumulators: `2 * (-1074)`.
const ACC_BASE_EXP: i32 = -2148;

const OP: &str = "unc.inst.arm_cov";

const ACC_RANGE: NumError = NumError::Domain { op: OP, detail: "norm term outside the exact accumulator" };

/// Declared parameters of an articulated-arm point model without joint data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmParams {
    /// Probing standard uncertainty (m).
    pub sigma_p: f64,
    /// Standard uncertainty per metre of distance from the arm base (m/m).
    pub sigma_d1: f64,
    /// Ratio of the axial to the lateral standard uncertainty (dimensionless); `None` is 1.
    pub axial_factor: Option<f64>,
}

impl ArmParams {
    /// Parameters with no declared axial factor (so 1: isotropic).
    pub const fn isotropic(sigma_p: f64, sigma_d1: f64) -> ArmParams {
        ArmParams { sigma_p, sigma_d1, axial_factor: None }
    }
}

/// Flags on an [`arm_cov`] result (module section on accuracy and flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmFlags {
    direction_off_unit: bool,
    axial_zero: bool,
    zero_covariance: bool,
    scale_below_validated: bool,
}

impl ArmFlags {
    /// `true` when no flag limits accuracy (neither `zero_covariance` nor
    /// `scale_below_validated`): the stated bound holds.
    pub fn is_validated(&self) -> bool {
        !(self.zero_covariance || self.scale_below_validated)
    }

    /// The given direction's norm differs from 1 by more than [`UNIT_TOLERANCE`] (decided
    /// exactly); it was normalised.
    pub fn direction_off_unit(&self) -> bool {
        self.direction_off_unit
    }

    /// A direction was given and the axial factor is zero: singular along the probe.
    pub fn axial_zero(&self) -> bool {
        self.axial_zero
    }

    /// Every entry of the covariance is zero.
    pub fn zero_covariance(&self) -> bool {
        self.zero_covariance
    }

    /// `0 < sigma_lat < SIGMA_MIN`, or the point norm is non-zero and subnormal.
    pub fn scale_below_validated(&self) -> bool {
        self.scale_below_validated
    }

    /// The codes of the flags that apply, in the order `direction_off_unit`, `axial_zero`,
    /// `zero_covariance`, `scale_below_validated`.
    pub fn codes(&self) -> Vec<&'static str> {
        [
            (self.direction_off_unit, "direction_off_unit"),
            (self.axial_zero, "axial_zero"),
            (self.zero_covariance, "zero_covariance"),
            (self.scale_below_validated, "scale_below_validated"),
        ]
        .into_iter()
        .filter_map(|(on, code)| on.then_some(code))
        .collect()
    }
}

/// Result of [`arm_cov`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmCov {
    cov: Cov3,
    sigma_lat: f64,
    sigma_ax: f64,
    axial_factor: f64,
    direction: Option<[f64; 3]>,
    flags: ArmFlags,
}

impl ArmCov {
    /// The covariance in m².
    pub fn cov(&self) -> &Cov3 {
        &self.cov
    }

    /// The lateral standard uncertainty `sigma_p + sigma_d1 ||p||` used (m; `-0.0` as `+0.0`).
    pub fn sigma_lat(&self) -> f64 {
        self.sigma_lat
    }

    /// The axial standard uncertainty `axial_factor * sigma_lat` (m; `-0.0` as `+0.0`). With
    /// no direction it equals `sigma_lat` (the factor is then 1).
    pub fn sigma_ax(&self) -> f64 {
        self.sigma_ax
    }

    /// The axial factor applied: the declared one, or 1 when absent (`-0.0` as `+0.0`).
    pub fn axial_factor(&self) -> f64 {
        self.axial_factor
    }

    /// The unit probe direction used, or `None` (isotropic).
    pub fn direction(&self) -> Option<[f64; 3]> {
        self.direction
    }

    /// The flags.
    pub fn flags(&self) -> &ArmFlags {
        &self.flags
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

// ------------------------------------------------------------------ exact norm tests

type Acc = [u64; ACC_LIMBS];

/// `|x| = m * 2^e` exactly, for a finite double (`e >= -1074`).
fn split(x: f64) -> (u64, i32) {
    let bits = x.to_bits();
    let field = (bits >> 52) & 0x7ff;
    let frac = bits & ((1_u64 << 52) - 1);
    // `field` is at most 0x7ff, so the cast is exact.
    let biased = field as i32;
    if field == 0 {
        (frac, -1074)
    } else {
        (frac | (1_u64 << 52), biased - 1075)
    }
}

/// Adds the integer `mag * 2^exp` to `acc` (bit 0 weighing `2^ACC_BASE_EXP`). A term the
/// accumulator cannot hold, or a carry out of it, is an error, never dropped (neither can
/// happen for finite inputs, module rule 3).
fn add_term(acc: &mut Acc, mag: u128, exp: i32) -> Result<(), InstError> {
    let shift = usize::try_from(exp - ACC_BASE_EXP).map_err(|_| InstError::Numeric(ACC_RANGE))?;
    let (word, bit) = (shift / 64, shift % 64);
    let lo = mag as u64;
    let hi = (mag >> 64) as u64;
    let words = if bit == 0 { [lo, hi, 0] } else { [lo << bit, (hi << bit) | (lo >> (64 - bit)), hi >> (64 - bit)] };
    if word + words.len() > ACC_LIMBS {
        return Err(InstError::Numeric(ACC_RANGE));
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
        Err(InstError::Numeric(ACC_RANGE))
    }
}

/// Adds the exact square of the finite double `x`.
fn add_square(acc: &mut Acc, x: f64) -> Result<(), InstError> {
    let (m, e) = split(x);
    add_term(acc, u128::from(m) * u128::from(m), 2 * e)
}

/// Most significant limb first: lexicographic order is numeric order.
fn acc_cmp(a: &Acc, b: &Acc) -> Ordering {
    a.iter().rev().cmp(b.iter().rev())
}

/// The exact side of `u.u` against `t²` and the unit window `[(1 - d)², (1 + d)²]`.
struct NormTests {
    below_min: bool,
    off_unit: bool,
}

fn norm_tests(u: [f64; 3]) -> Result<NormTests, InstError> {
    let mut s: Acc = [0; ACC_LIMBS];
    for c in u {
        add_square(&mut s, c)?;
    }
    let mut t2: Acc = [0; ACC_LIMBS];
    add_square(&mut t2, MIN_DIRECTION_NORM)?;
    let below_min = acc_cmp(&s, &t2) == Ordering::Less;

    // (1 - d)² = 1 - 2d + d², (1 + d)² = 1 + 2d + d²; 2d = m 2^(e + 1).
    let (md, ed) = split(UNIT_TOLERANCE);
    let mut one_d2: Acc = [0; ACC_LIMBS];
    add_term(&mut one_d2, 1, 0)?;
    add_square(&mut one_d2, UNIT_TOLERANCE)?;
    // ||u|| < 1 - d  <=>  u.u + 2d < 1 + d².
    let mut s_2d = s;
    add_term(&mut s_2d, u128::from(md), ed + 1)?;
    let below_window = acc_cmp(&s_2d, &one_d2) == Ordering::Less;
    // ||u|| > 1 + d  <=>  u.u > 1 + 2d + d².
    let mut hi = one_d2;
    add_term(&mut hi, u128::from(md), ed + 1)?;
    let above_window = acc_cmp(&s, &hi) == Ordering::Greater;
    Ok(NormTests { below_min, off_unit: below_window || above_window })
}

// ------------------------------------------------------------------ scaled norms

/// `2^k` for `-1022 <= k <= 1023` (a normal power of two, exact).
fn pow2(k: i32) -> Result<f64, InstError> {
    let biased = u64::try_from(k + 1023).map_err(|_| InstError::Numeric(SCALE_RANGE))?;
    if !(1..=2046).contains(&biased) {
        return Err(InstError::Numeric(SCALE_RANGE));
    }
    Ok(f64::from_bits(biased << 52))
}

const SCALE_RANGE: NumError = NumError::Domain { op: OP, detail: "scale exponent outside the normal range" };

/// `floor(log2 m)` for a finite `m > 0`, subnormals included.
fn floor_log2(m: f64) -> i32 {
    let bits = m.to_bits();
    let field = (bits >> 52) & 0x7ff;
    if field != 0 {
        // `field` is at most 0x7ff, so the cast is exact.
        field as i32 - 1023
    } else {
        let frac = bits & ((1_u64 << 52) - 1);
        // `frac` is non-zero for a subnormal `m > 0`; its top bit index is below 52.
        let top = 63 - frac.leading_zeros() as i32;
        top - 1074
    }
}

/// Module rule 4: the scaled vector `v`, `n = ||v||` and the exponents `a`, `b`.
struct Scaled {
    v: [f64; 3],
    n: f64,
    a: i32,
    b: i32,
}

/// [`Scaled`] for `x`, or `None` for an all-zero vector.
fn scaled(x: [f64; 3]) -> Result<Option<Scaled>, InstError> {
    let mut m = 0.0_f64;
    for c in x {
        m = fid_num::f::total_max(m, c.abs())?;
    }
    if m == 0.0 {
        return Ok(None);
    }
    let e = floor_log2(m);
    let a = (-e) / 2;
    let b = -e - a;
    let (sa, sb) = (pow2(a)?, pow2(b)?);
    let [x0, x1, x2] = x;
    let v = [(x0 * sa) * sb, (x1 * sa) * sb, (x2 * sa) * sb];
    let [v0, v1, v2] = v;
    let n = fid_num::f::sqrt((v0 * v0 + v1 * v1) + v2 * v2)?;
    Ok(Some(Scaled { v, n, a, b }))
}

fn norm(x: [f64; 3]) -> Result<f64, InstError> {
    match scaled(x)? {
        None => Ok(0.0),
        Some(s) => Ok((s.n * pow2(-s.a)?) * pow2(-s.b)?),
    }
}

/// The covariance (m²) of an articulated-arm probing point `p` with optional probe direction
/// `u` under the declared `params`, algorithm `unc.inst.arm_cov@1.0.0` (module rules 1 to 7).
pub fn arm_cov(params: &ArmParams, p: [f64; 3], u: Option<[f64; 3]>) -> Result<ArmCov, InstError> {
    param("sigma_p", params.sigma_p)?;
    param("sigma_d1", params.sigma_d1)?;
    if let Some(f) = params.axial_factor {
        param("axial_factor", f)?;
    }
    for (name, c) in ["p_x", "p_y", "p_z"].into_iter().zip(p) {
        if !c.is_finite() {
            return Err(InstError::InvalidObservation { name, reason: ValueReason::NotFinite });
        }
    }
    let f = params.axial_factor.unwrap_or(1.0) + 0.0;
    let mut off_unit = false;
    if let Some(u) = u {
        for (name, c) in ["u_x", "u_y", "u_z"].into_iter().zip(u) {
            if !c.is_finite() {
                return Err(InstError::InvalidDirection { name, reason: ValueReason::NotFinite });
            }
        }
        let tests = norm_tests(u)?;
        if tests.below_min {
            return Err(InstError::InvalidDirection { name: "u", reason: ValueReason::BelowMinimumNorm });
        }
        off_unit = tests.off_unit;
    } else if f != 1.0 {
        return Err(InstError::InvalidDirection { name: "u", reason: ValueReason::Missing });
    }

    let np = norm(p)?;
    if !np.is_finite() {
        return Err(InstError::Numeric(NumError::Infinite { op: OP, index: None }));
    }
    let sl = fid_num::check_finite(OP, params.sigma_p + params.sigma_d1 * np)? + 0.0;
    let sax = fid_num::check_finite(OP, f * sl)? + 0.0;
    let s2 = sl * sl;

    let scaled_u = match u {
        None => None,
        // Unreachable after rule 3 (an all-zero `u` is below the minimum norm), refused
        // rather than evaluated as isotropic.
        Some(u) => {
            Some(scaled(u)?.ok_or(InstError::InvalidDirection { name: "u", reason: ValueReason::BelowMinimumNorm })?)
        }
    };
    let (upper, direction) = match scaled_u {
        None => ([s2, 0.0, 0.0, s2, 0.0, s2], None),
        Some(s) => {
            let [v0, v1, v2] = s.v;
            let uh = [v0 / s.n, v1 / s.n, v2 / s.n];
            let d = ((f - 1.0) * sl) * ((f + 1.0) * sl);
            let [u0, u1, u2] = uh;
            let t = |a: f64, b: f64| d * (a * b);
            let upper = [s2 + t(u0, u0), t(u0, u1), t(u0, u2), s2 + t(u1, u1), t(u1, u2), s2 + t(u2, u2)];
            (upper, Some(uh.map(|c| c + 0.0)))
        }
    };
    let cov = Cov3::from_upper(OP, upper)?;

    let mut largest = 0.0_f64;
    for row in cov.rows() {
        for v in row {
            largest = fid_num::f::total_max(largest, v.abs())?;
        }
    }
    let flags = ArmFlags {
        direction_off_unit: off_unit,
        axial_zero: direction.is_some() && f == 0.0,
        zero_covariance: largest == 0.0,
        scale_below_validated: (sl != 0.0 && sl < SIGMA_MIN) || (np != 0.0 && np < f64::MIN_POSITIVE),
    };
    Ok(ArmCov { cov, sigma_lat: sl, sigma_ax: sax, axial_factor: f, direction, flags })
}
