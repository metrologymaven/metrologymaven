//! Adaptive Monte Carlo stopping rule and trial cap. Spec node `UNC.PROP.MCM.ADAPT`, rule
//! [`RULE`] = `mcm-adapt@1`, algorithm [`ALGORITHM_ID`] = `unc.prop.mcm_adapt@1.0.0`.
//!
//! # Spec card `mcm-adapt@1` (our own words)
//!
//! The rule paraphrases the adaptive procedure of JCGM 101:2008, §7.9 (numerical tolerance
//! §7.9.2, procedure §7.9.4) [unverified: clause numbering to be checked by an uncertainty
//! expert]. Monte Carlo trials run in blocks of `M` trials. After each completed block the
//! caller summarises that block alone (estimate, standard uncertainty and the two endpoints
//! of the coverage interval, with [`super::interval::summarize`]) and the standard
//! uncertainty `u(y)` of all trials so far, and asks [`decide`] whether to run another
//! block. The rule is pure: the same summaries and profile always give the same decision,
//! so a run with a seeded, counter-addressed RNG (trial `t` of the run uses RNG index `t`)
//! stops after the same block on every replay.
//!
//! # Rules (pinned; D0)
//!
//! 1. **Profile** ([`AdaptProfile::new`]): `p` finite with `0 < p < 1`, `1 <= n_dig <= 17`,
//!    `cap >= 2 M`, else `invalid_profile`. `gum_suite_profile` defaults to
//!    `jcgm101-2008`.
//! 2. **Block size** ([`block_size`]): `M = max(10 000, J)` with `J` the smallest integer
//!    `>= 100 / (1 - p)`. `p` is read as the decimal number it was written as: the shortest
//!    decimal string that converts back to the same binary64 (core `Display`), evaluated
//!    in exact integers. So `p = 0.9999` gives `J = 10^6` exactly, although the binary64
//!    nearest to 0.9999 is slightly above it (the binary value would give `10^6 + 1`).
//! 3. **Tolerance** ([`tolerance`]): write `u(y)` rounded to `n_dig` significant decimal
//!    digits as `c * 10^l` with `c` an integer of `n_dig` digits; the tolerance is
//!    `delta = 10^l / 2`. `l` is exact: the decimal exponent `E` (`10^E <= u < 10^(E+1)`)
//!    comes from an integer estimate corrected by exact comparisons, `l = E - n_dig + 1`, and
//!    `l` is raised by one when the rounding carries (`u >= (10^n_dig - 1/2) 10^l`; round
//!    half up and half even agree there). `u(y) = 0` gives `delta = 0` (no exponent).
//!    [`Tolerance::delta`] is the binary64 nearest to the exact `delta`; tests against the
//!    tolerance use the exact value.
//! 4. **Spread of the block values**: for each of the four quantities (estimate, `u`, lower
//!    and upper endpoint) over the `h` block summaries, `s = sqrt(sum (x_r - mean)^2 /
//!    (h (h - 1)))`, the standard deviation associated with the average of the `h` block
//!    values (JCGM 101:2008 §7.9.4). Computed as `red::norm2(x - red::mean(x)) /
//!    f::sqrt(h (h - 1))`, deviations in block order.
//! 5. **Decision** ([`decide`]) after block `h` (`h >= 1`, `h M <= cap`):
//!    - `h = 1`: `Continue` (at least two blocks; `2 M <= cap` by rule 1);
//!    - `h >= 2` and `2 s <= delta` for all four quantities (exact comparison with the
//!      exact `delta`): `Stop`;
//!    - otherwise, if `(h + 1) M > cap`: `CapReached`, flag [`FLAG_NOT_CONVERGED`], with the
//!      four `s` and the tolerance achieved; the rule never reports `Stop` unless the
//!      tolerance test passed;
//!    - otherwise `Continue`.
//!
//! The output records the rule, `gum_suite_profile`, `p`, block size, `n_dig` and cap
//! (REQ-UNC.PROP.MCM.ADAPT-004) through [`AdaptStep::profile`].
//!
//! KNP-1: no FMA, no `f64::min`/`max`, reductions through `fid_num::red`, a NaN is always
//! an error, no panics, no hash containers. The decimal work (rules 2 and 3) is integer
//! arithmetic only. Nothing here produces a measured value: every input is a summary of
//! simulated (model-based) trials.

use super::interval::McmSummary;
use super::McmError;
use core::cmp::Ordering;
use fid_num::{check_finite, f, red, NumError};

/// Algorithm ID of this rule set.
pub const ALGORITHM_ID: &str = "unc.prop.mcm_adapt@1.0.0";
/// Rule name recorded in every output (REQ-UNC.PROP.MCM.ADAPT-004).
pub const RULE: &str = "mcm-adapt@1";
/// Default number of significant decimal digits of `u(y)` (JCGM 101:2008 §7.9.2).
pub const DEFAULT_N_DIG: u32 = 2;
/// Default trial cap [unverified].
pub const DEFAULT_CAP: u64 = 1_000_000;
/// Smallest block size.
pub const MIN_BLOCK: u64 = 10_000;
/// Largest accepted `n_dig`: a binary64 holds at most 17 significant decimal digits.
pub const MAX_N_DIG: u32 = 17;
/// Flag carried by a [`Decision::CapReached`] result.
pub const FLAG_NOT_CONVERGED: &str = "not_converged";

/// Version of the GUM suite the rule follows, so a revised Supplement 1 can be adopted as
/// a new profile version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GumSuiteProfile {
    /// JCGM 101:2008 (GUM Supplement 1).
    #[default]
    Jcgm101_2008,
}

impl GumSuiteProfile {
    /// The recorded name.
    pub const fn as_str(&self) -> &'static str {
        match self {
            GumSuiteProfile::Jcgm101_2008 => "jcgm101-2008",
        }
    }
}

/// A validated profile of the rule (rule 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptProfile {
    gum_suite_profile: GumSuiteProfile,
    p: f64,
    n_dig: u32,
    cap: u64,
    block_size: u64,
}

impl AdaptProfile {
    /// Validates a profile and derives its block size.
    pub fn new(gum_suite_profile: GumSuiteProfile, p: f64, n_dig: u32, cap: u64) -> Result<Self, McmError> {
        let block_size = block_size(p)?;
        if !(1..=MAX_N_DIG).contains(&n_dig) {
            return Err(McmError::InvalidProfile { detail: "n_dig" });
        }
        match block_size.checked_mul(2) {
            Some(two_m) if cap >= two_m => {}
            _ => return Err(McmError::InvalidProfile { detail: "cap" }),
        }
        Ok(AdaptProfile { gum_suite_profile, p, n_dig, cap, block_size })
    }

    /// The default profile at coverage probability `p`: `jcgm101-2008`, `n_dig` 2, cap
    /// 10^6 trials. Fails with `invalid_profile` (cap) when `2 M > 10^6`, that is for `p`
    /// above 0.99995; such a run needs a larger explicit cap.
    pub fn with_defaults(p: f64) -> Result<Self, McmError> {
        AdaptProfile::new(GumSuiteProfile::default(), p, DEFAULT_N_DIG, DEFAULT_CAP)
    }

    /// The rule name, [`RULE`].
    pub const fn version(&self) -> &'static str {
        RULE
    }
    /// The GUM suite profile.
    pub const fn gum_suite_profile(&self) -> GumSuiteProfile {
        self.gum_suite_profile
    }
    /// The coverage probability.
    pub const fn p(&self) -> f64 {
        self.p
    }
    /// Significant decimal digits of `u(y)`.
    pub const fn n_dig(&self) -> u32 {
        self.n_dig
    }
    /// The trial cap.
    pub const fn cap(&self) -> u64 {
        self.cap
    }
    /// The block size `M` (rule 2).
    pub const fn block_size(&self) -> u64 {
        self.block_size
    }
}

/// Block size `M = max(10 000, ceil(100 / (1 - p)))` (rule 2). `invalid_profile` unless
/// `0 < p < 1`.
pub fn block_size(p: f64) -> Result<u64, McmError> {
    if !p.is_finite() || p <= 0.0 || p >= 1.0 {
        return Err(McmError::InvalidProfile { detail: "p" });
    }
    // For 0 < p < 1 the shortest round-trip `Display` form is "0." followed by digits.
    let text = format!("{p}");
    let digits = text.strip_prefix("0.").ok_or(McmError::InvalidProfile { detail: "p" })?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(McmError::InvalidProfile { detail: "p" });
    }
    // At most 17 significant digits, so more than 30 fraction digits means p < 10^-13,
    // hence 100 / (1 - p) < 101 and M = 10 000.
    if digits.len() > 30 {
        return Ok(MIN_BLOCK);
    }
    let mut num: u128 = 0;
    let mut den: u128 = 1;
    for b in digits.bytes() {
        num = num * 10 + u128::from(b - b'0');
        den *= 10;
    }
    // p = num / den with num < den; J = ceil(100 den / (den - num)), den <= 10^30.
    let gap = den - num;
    let j = (100 * den).div_ceil(gap);
    let j = u64::try_from(j).map_err(|_| McmError::InvalidProfile { detail: "p" })?;
    Ok(if j > MIN_BLOCK { j } else { MIN_BLOCK })
}

// ------------------------------------------------------------------ exact decimal compare

/// Minimal unsigned big integer, little-endian 32-bit limbs.
#[derive(Clone)]
struct Big(Vec<u32>);

impl Big {
    fn from_u64(x: u64) -> Big {
        Big(vec![x as u32, (x >> 32) as u32])
    }

    fn mul_small(&mut self, m: u32) {
        let mut carry: u64 = 0;
        for limb in self.0.iter_mut() {
            let t = u64::from(*limb) * u64::from(m) + carry;
            *limb = t as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
    }

    fn mul_pow5(&mut self, mut k: u32) {
        // 5^13 = 1 220 703 125 < 2^32.
        while k >= 13 {
            self.mul_small(1_220_703_125);
            k -= 13;
        }
        let mut m: u32 = 1;
        for _ in 0..k {
            m *= 5;
        }
        self.mul_small(m);
    }

    fn shl(&mut self, bits: u32) {
        let words = (bits / 32) as usize;
        let rem = bits % 32;
        if rem != 0 {
            let mut carry: u32 = 0;
            for limb in self.0.iter_mut() {
                let next = *limb >> (32 - rem);
                *limb = (*limb << rem) | carry;
                carry = next;
            }
            if carry != 0 {
                self.0.push(carry);
            }
        }
        if words != 0 {
            let mut v = vec![0u32; words];
            v.extend_from_slice(&self.0);
            self.0 = v;
        }
    }

    fn trimmed_len(&self) -> usize {
        let mut n = self.0.len();
        while n > 0 && self.0[n - 1] == 0 {
            n -= 1;
        }
        n
    }

    fn cmp(&self, other: &Big) -> Ordering {
        let (a, b) = (self.trimmed_len(), other.trimmed_len());
        if a != b {
            return a.cmp(&b);
        }
        for i in (0..a).rev() {
            match self.0[i].cmp(&other.0[i]) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        Ordering::Equal
    }
}

/// `(mant, e)` with `x = mant * 2^e` for finite `x >= 0`.
fn decompose(x: f64) -> (u64, i32) {
    let bits = x.to_bits() & !(1u64 << 63);
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    if exp == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp - 1075)
    }
}

/// Largest `|k|` for which [`cmp_dec`] builds `10^k`. Every exponent the rule uses is
/// within `[-341, 309]` (decimal exponents of finite binary64 values, shifted by at most
/// `n_dig <= 17`), so this bound only keeps the big-integer work finite by construction.
const MAX_ABS_EXP10: u32 = 400;

/// Exact comparison of `mant * 2^e2` with `a * 10^k`, for `|e2| <= 1100` (binary64 range
/// plus small shifts). For `|k| > MAX_ABS_EXP10` the result is decided without building
/// `10^k`: `mant * 2^e2` lies strictly between `10^-400` and `10^400` unless it is zero.
fn cmp_dec(mant: u64, e2: i32, a: u64, k: i32) -> Ordering {
    if k.unsigned_abs() > MAX_ABS_EXP10 {
        return match (mant == 0, a == 0, k > 0) {
            (true, true, _) => Ordering::Equal,
            (true, false, _) => Ordering::Less,
            (false, true, _) => Ordering::Greater,
            (false, false, true) => Ordering::Less,
            (false, false, false) => Ordering::Greater,
        };
    }
    let mut lhs = Big::from_u64(mant);
    let mut rhs = Big::from_u64(a);
    if e2 >= 0 {
        lhs.shl(e2.unsigned_abs());
    } else {
        rhs.shl(e2.unsigned_abs());
    }
    // 10^k = 5^k 2^k.
    if k >= 0 {
        rhs.mul_pow5(k.unsigned_abs());
        rhs.shl(k.unsigned_abs());
    } else {
        lhs.mul_pow5(k.unsigned_abs());
        lhs.shl(k.unsigned_abs());
    }
    lhs.cmp(&rhs)
}

/// Exact decimal exponent `E` with `10^E <= x < 10^(E+1)`, for finite `x > 0`.
fn decimal_exponent(x: f64) -> i32 {
    let (mant, e2) = decompose(x);
    // Binary exponent b with 2^b <= x < 2^(b+1); then E is floor(b log10 2) or one more.
    let b = e2 + 63 - mant.leading_zeros() as i32;
    // floor(b * log10(2)) by the integer approximation 1233 / 4096 (off by at most one
    // over the binary64 range); the corrections below make the result exact.
    let mut e = (b * 1233).div_euclid(4096);
    for _ in 0..4 {
        if cmp_dec(mant, e2, 1, e) == Ordering::Less {
            e -= 1;
        } else if cmp_dec(mant, e2, 1, e + 1) != Ordering::Less {
            e += 1;
        } else {
            break;
        }
    }
    e
}

/// The numerical tolerance of rule 3: `delta = 10^exp10 / 2`. Built only by [`tolerance`],
/// so `exp10` is always within `[-341, 309]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    exp10: Option<i32>,
    delta: f64,
}

impl Tolerance {
    /// `l`, the decimal exponent of the last retained digit of `u(y)`; `None` when
    /// `u(y) = 0` (then `delta = 0`).
    pub const fn exp10(&self) -> Option<i32> {
        self.exp10
    }

    /// The binary64 nearest to the exact `delta` (0 when `u(y) = 0`; may underflow to 0
    /// for a subnormal `u(y)`, the comparison in [`Tolerance::admits`] stays exact).
    pub const fn delta(&self) -> f64 {
        self.delta
    }

    /// Whether `2 s <= delta`, compared exactly with the exact decimal `delta`. A NaN,
    /// infinite or negative `s` is never admitted (`-0.0` counts as zero).
    pub fn admits(&self, s: f64) -> bool {
        if !s.is_finite() || s < 0.0 {
            return false;
        }
        match self.exp10 {
            None => s == 0.0,
            Some(l) => {
                // 2 s <= 10^l / 2  <=>  4 s <= 10^l.
                let (mant, e2) = decompose(s);
                cmp_dec(mant, e2 + 2, 1, l) != Ordering::Greater
            }
        }
    }

    /// Whether `d <= delta`, compared exactly with the exact decimal `delta = 10^l / 2`
    /// (JCGM 101:2008 validation check, `UNC.PROP.VALID`). A NaN, infinite or negative `d`
    /// is never within `delta` (`-0.0` counts as zero).
    pub fn within_delta(&self, d: f64) -> bool {
        if !d.is_finite() || d < 0.0 {
            return false;
        }
        match self.exp10 {
            None => d == 0.0,
            Some(l) => {
                // d <= 10^l / 2  <=>  2 d <= 10^l.
                let (mant, e2) = decompose(d);
                cmp_dec(mant, e2 + 1, 1, l) != Ordering::Greater
            }
        }
    }
}

/// The tolerance from `u(y)` at `n_dig` significant digits (rule 3). `u` must be finite
/// and non-negative (`-0.0` counts as zero) and `1 <= n_dig <= 17`.
pub fn tolerance(u: f64, n_dig: u32) -> Result<Tolerance, McmError> {
    let u = check_finite("mcm.adapt.u", u)?;
    if u < 0.0 {
        return Err(McmError::InvalidSummary { block: None });
    }
    if !(1..=MAX_N_DIG).contains(&n_dig) {
        return Err(McmError::InvalidProfile { detail: "n_dig" });
    }
    if u == 0.0 {
        return Ok(Tolerance { exp10: None, delta: 0.0 });
    }
    let e = decimal_exponent(u);
    let n = n_dig as i32;
    let mut l = e - n + 1;
    // Carry: u >= (10^n - 1/2) 10^l  <=>  2 u >= (2 10^n - 1) 10^l.
    let mut ten_n: u64 = 1;
    for _ in 0..n_dig {
        ten_n *= 10;
    }
    let (mant, e2) = decompose(u);
    if cmp_dec(mant, e2 + 1, 2 * ten_n - 1, l) != Ordering::Less {
        l += 1;
    }
    if l.unsigned_abs() > MAX_ABS_EXP10 {
        // Unreachable for finite u and n_dig <= 17 (l is within [-341, 309]).
        return Err(McmError::InvalidProfile { detail: "n_dig" });
    }
    // delta = 5 * 10^(l - 1); the core parser rounds correctly to nearest.
    let delta: f64 = format!("5e{}", l - 1).parse().map_err(|_| McmError::InvalidProfile { detail: "n_dig" })?;
    Ok(Tolerance { exp10: Some(l), delta })
}

// ------------------------------------------------------------------ rule

/// Summary of one block alone: estimate, standard uncertainty and the coverage interval
/// endpoints (all from simulated trials).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockSummary {
    /// Estimate (mean of the block's values).
    pub y_mean: f64,
    /// Standard uncertainty of the block's values.
    pub u: f64,
    /// Lower coverage interval endpoint.
    pub low: f64,
    /// Upper coverage interval endpoint.
    pub high: f64,
}

impl BlockSummary {
    /// From an interval summary, using its probabilistically symmetric interval.
    pub fn symmetric(s: &McmSummary) -> BlockSummary {
        BlockSummary { y_mean: s.mean, u: s.u, low: s.sym.low, high: s.sym.high }
    }

    /// From an interval summary, using its shortest interval.
    pub fn shortest(s: &McmSummary) -> BlockSummary {
        BlockSummary { y_mean: s.mean, u: s.u, low: s.shortest.low, high: s.shortest.high }
    }
}

/// The rule's decision after a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Run another block.
    Continue,
    /// The tolerance test passed: the computation has stabilised.
    Stop,
    /// The next block would exceed the cap and the tolerance test failed: `not_converged`.
    CapReached,
}

impl Decision {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            Decision::Continue => "continue",
            Decision::Stop => "stop",
            Decision::CapReached => "cap_reached",
        }
    }
}

/// Output of [`decide`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptStep {
    /// The decision.
    pub decision: Decision,
    /// Number of completed blocks `h`.
    pub blocks: u64,
    /// Number of completed trials `h M`.
    pub trials: u64,
    /// The four `s` values (estimate, `u`, lower, upper; rule 4); `None` after block 1.
    pub sd: Option<[f64; 4]>,
    /// The tolerance from the overall `u(y)`; `None` after block 1.
    pub tolerance: Option<Tolerance>,
    /// The profile, recording rule, `gum_suite_profile`, `p`, block size, `n_dig`, cap.
    pub profile: AdaptProfile,
}

impl AdaptStep {
    /// True exactly for [`Decision::CapReached`].
    pub fn not_converged(&self) -> bool {
        self.decision == Decision::CapReached
    }

    /// Result flags: [`FLAG_NOT_CONVERGED`] on a cap, else none.
    pub fn flags(&self) -> &'static [&'static str] {
        if self.not_converged() {
            &[FLAG_NOT_CONVERGED]
        } else {
            &[]
        }
    }
}

/// `s` of rule 4 for one quantity over the blocks.
fn spread(xs: &[f64], h: u64) -> Result<f64, McmError> {
    let mean = red::mean(xs)?;
    let mut dev: Vec<f64> = Vec::new();
    dev.try_reserve_exact(xs.len()).map_err(|_| McmError::Allocation)?;
    dev.extend(xs.iter().map(|&x| x - mean));
    // h <= cap / M <= 2^64 / 10^4, so h (h - 1) < 2^53 needs h < 2^26.5: guarded by the
    // caller (h <= 2^26).
    let hh = (h as f64) * ((h - 1) as f64);
    let s = red::norm2(&dev)? / f::sqrt(hh)?;
    check_finite("mcm.adapt.sd", s).map_err(McmError::from)
}

/// Decides after `blocks.len()` completed blocks, given the overall `u(y)` of all
/// `blocks.len() * M` trials (rule 5). Errors: no blocks (`numeric`, empty input); more
/// blocks than the cap allows (`cap_exceeded`); a non-finite value (`numeric`, op
/// `mcm.adapt.blocks` with the block index, or `mcm.adapt.u`); a negative `u`, or a lower
/// endpoint above the upper one (`invalid_summary`).
pub fn decide(profile: &AdaptProfile, blocks: &[BlockSummary], u_all: f64) -> Result<AdaptStep, McmError> {
    let h = blocks.len() as u64;
    if h == 0 {
        return Err(McmError::Num(NumError::EmptyInput { op: "mcm.adapt.blocks" }));
    }
    let m = profile.block_size;
    let max_blocks = profile.cap / m;
    if h > max_blocks {
        return Err(McmError::CapExceeded { blocks: h, max: max_blocks });
    }
    for (i, b) in blocks.iter().enumerate() {
        for x in [b.y_mean, b.u, b.low, b.high] {
            if x.is_nan() {
                return Err(McmError::Num(NumError::NotANumber { op: "mcm.adapt.blocks", index: Some(i) }));
            }
            if !x.is_finite() {
                return Err(McmError::Num(NumError::Infinite { op: "mcm.adapt.blocks", index: Some(i) }));
            }
        }
        if b.u < 0.0 || b.low > b.high {
            return Err(McmError::InvalidSummary { block: Some(i as u64) });
        }
    }
    let u_all = check_finite("mcm.adapt.u", u_all)?;
    if u_all < 0.0 {
        return Err(McmError::InvalidSummary { block: None });
    }
    let trials = h * m;
    let mut step =
        AdaptStep { decision: Decision::Continue, blocks: h, trials, sd: None, tolerance: None, profile: *profile };
    if h == 1 {
        return Ok(step);
    }
    if h > (1u64 << 26) {
        return Err(McmError::CapExceeded { blocks: h, max: 1u64 << 26 });
    }
    let mut col: Vec<f64> = Vec::new();
    col.try_reserve_exact(blocks.len()).map_err(|_| McmError::Allocation)?;
    let mut sd = [0.0f64; 4];
    for (k, slot) in sd.iter_mut().enumerate() {
        col.clear();
        col.extend(blocks.iter().map(|b| match k {
            0 => b.y_mean,
            1 => b.u,
            2 => b.low,
            _ => b.high,
        }));
        *slot = spread(&col, h)?;
    }
    let tol = tolerance(u_all, profile.n_dig)?;
    step.sd = Some(sd);
    step.tolerance = Some(tol);
    step.decision = if sd.iter().all(|&s| tol.admits(s)) {
        Decision::Stop
    } else if trials.checked_add(m).is_none_or(|next| next > profile.cap) {
        Decision::CapReached
    } else {
        Decision::Continue
    };
    Ok(step)
}
