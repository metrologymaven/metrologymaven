//! Length and dimensionless formatters `fmt.length@1` and `fmt.scalar@1`, with the
//! side-preserving rule. Spec node `KRN.NUM.FMT.LEN`.
//!
//! Every output is a function of the bits of `x` and the [`FormatSpec`] only. After the
//! `f64` is split into sign, mantissa and exponent, all work is exact integer arithmetic on
//! the fixed-capacity `kdec` big integer: no float operation, no `core` float formatting,
//! no locale. The rules below are pinned in the leaf's Design section; any implementation
//! must reproduce them string for string.
//!
//! # The value
//!
//! `x = (-1)^s * m * 2^e` exactly. The display value is the exact rational
//! `v = x / f(u)` with `f(m) = 1`, `f(mm) = 10^-3`, `f(um) = 10^-6`, `f(in) = 0.0254` and,
//! for `fmt.scalar@1`, `f(1) = 1`. It is held as `N / D` with
//! `N = m * 2^max(e,0) * 10^a(u)` and `D = 2^max(-e,0) * b(u)`, where `a = 0, 3, 6, 4, 0`
//! and `b = 1, 1, 1, 254, 1` for m, mm, um, in, 1.
//!
//! # Rounding (one half-even step)
//!
//! - `Decimals(p)`: `q = round_half_even(|v| * 10^p)`, printed with exactly `p` digits after
//!   the point (no point when `p = 0`).
//! - `SigDigits(n)`: with `k = floor(log10 |v|)` (exact), `p = n - 1 - k` and `q` as above.
//!   If rounding carries into a new decade (`q = 10^n`), the output is `10^(n-1)` at
//!   `p - 1` decimals, so it keeps exactly `n` significant digits (`9.9996` at 3 digits is
//!   `10.0`). A negative `p` prints `q` followed by `-p` zeros, never an exponent. Zero
//!   (either sign) has `k = 0`: `0` at `SigDigits(1)`, `0.00` at `SigDigits(3)`.
//!
//! # Canonical string (REQ-KRN.NUM.FMT.LEN-008)
//!
//! ASCII digits, at most one `.`, at least one digit before the point, and a leading `-`
//! only when `x` is negative and `q != 0`. `-0.0` and negative values that round to zero
//! print without a sign.
//!
//! # Side-preserving rule (REQ-KRN.NUM.FMT.LEN-004 .. -006)
//!
//! The violated limit is the upper one if `kdec::cmp(x, upper)` is `Greater`, else the lower
//! one if `kdec::cmp(x, lower)` is `Less`; if neither, the output is the requested one with
//! `digits_added = 0` even when it equals a limit. Otherwise, starting at the requested
//! precision: if the output, converted back exactly to SI units (`q * 10^-p * f(u)`), is
//! strictly beyond the violated limit, stop. Else, if the output is at the cap, stop with
//! `tie_at_max_precision = true`. Else add one digit (`p + 1` decimals, or `n + 1`
//! significant digits), count it in `digits_added`, and repeat. The cap is `n = 17` for
//! significant digits and `k + 1 + p >= 17` for decimals (`k` as above; `k = 0` for zero),
//! so a request that already shows 17 or more digits is never extended. Decimals added by
//! the rule may exceed 20. While the output is zero it stays zero for every `p < -k - 1`,
//! so those steps are taken at once (same result, same `digits_added`).
//!
//! # Errors (checked in this order)
//!
//! 1. `UnitNotSupported`: an unknown formatter, a length unit other than m, mm, um, in for
//!    `fmt.length@1`, or a unit other than `1` for `fmt.scalar@1`.
//! 2. `PrecisionOutOfRange`: `Decimals(p > 20)`, `SigDigits(0)` or `SigDigits(n > 17)`.
//! 3. `LimitsInverted`: both limits given and `lower > upper` (equal limits are allowed).
//! 4. `NonFinite`: `x` is NaN or infinite.
//!
//! # Capacity
//!
//! The largest intermediate is below 1400 bits (for example `N * 10^p` for the largest
//! finite `x` in um at 20 decimals is about `2^1024 * 10^26`, and for the smallest
//! subnormal at the 17-digit cap about `2^53 * 10^345`), well inside the 2560-bit `kdec`
//! big integer, so [`FmtError::Capacity`] cannot occur.

use core::cmp::Ordering;

use super::types::{Conversion, FmtError, FormatSpec, FormatterId, Precision, Reported, Unit};
use super::types::{MAX_DECIMALS, MAX_SIG_DIGITS};
use crate::err::NumError;
use crate::kdec::{self, Big, Decimal};

const SIG_CAP: i32 = MAX_SIG_DIGITS as i32;

/// Exact unit factor data for one display unit.
#[derive(Clone, Copy)]
struct Factor {
    /// `N` gets `* 10^num_pow10`.
    num_pow10: u32,
    /// `D` gets `* den`.
    den: u32,
    /// `f(u) = back_coeff * 10^back_exp` (display to SI).
    back_coeff: u32,
    back_exp: i64,
    conv: Conversion,
}

fn factor(formatter: FormatterId, unit: Unit) -> Result<(&'static str, Factor), NumError> {
    let f = |num_pow10, den, back_coeff, back_exp, from, factor| Factor {
        num_pow10,
        den,
        back_coeff,
        back_exp,
        conv: Conversion { from, to: unit, factor },
    };
    if formatter == FormatterId::LENGTH_V1 {
        const OP: &str = "fmt.length@1";
        let fac = match unit {
            Unit::M => f(0, 1, 1, 0, Unit::M, "1"),
            Unit::Mm => f(3, 1, 1, -3, Unit::M, "1000"),
            Unit::Um => f(6, 1, 1, -6, Unit::M, "1000000"),
            Unit::In => f(4, 254, 254, -4, Unit::M, "1/0.0254"),
            _ => return Err(FmtError::UnitNotSupported.at(OP)),
        };
        Ok((OP, fac))
    } else if formatter == FormatterId::SCALAR_V1 {
        const OP: &str = "fmt.scalar@1";
        match unit {
            Unit::One => Ok((OP, f(0, 1, 1, 0, Unit::One, "1"))),
            _ => Err(FmtError::UnitNotSupported.at(OP)),
        }
    } else {
        Err(FmtError::UnitNotSupported.at("fmt"))
    }
}

/// Internal precision; decimals may grow past 20 under the side-preserving rule.
#[derive(Clone, Copy)]
enum Prec {
    Dec(i32),
    Sig(i32),
}

/// The exact display value `(-1)^neg * n / d` and its decimal exponent `k`.
struct Exact {
    neg: bool,
    n: Big,
    d: Big,
    k: i32,
}

/// A rounded output `(-1)^neg * q * 10^-p`.
struct Out {
    q: Big,
    p: i32,
}

/// Formats `x` (metres for `fmt.length@1`, the plain value for `fmt.scalar@1`) as pinned in
/// the module documentation (REQ-KRN.NUM.FMT.LEN-001 .. -010).
pub fn format(x: f64, spec: &FormatSpec) -> Result<Reported, NumError> {
    let (op, fac) = factor(spec.formatter, spec.unit)?;
    let prec = match spec.precision {
        Precision::Decimals(p) if p <= MAX_DECIMALS => Prec::Dec(i32::from(p)),
        Precision::SigDigits(n) if (1..=MAX_SIG_DIGITS).contains(&n) => Prec::Sig(i32::from(n)),
        _ => return Err(FmtError::PrecisionOutOfRange.at(op)),
    };
    let limits = spec.limits.unwrap_or_default();
    if let (Some(lo), Some(hi)) = (&limits.lower, &limits.upper) {
        if kdec::cmp_dec(lo, hi) == Ordering::Greater {
            return Err(FmtError::LimitsInverted.at(op));
        }
    }
    if !x.is_finite() {
        return Err(FmtError::NonFinite.at(op));
    }
    let v = exact(op, x, &fac)?;
    let mut prec = prec;
    let mut out = round_at(op, &v, prec)?;

    let mut side = None;
    if let Some(hi) = &limits.upper {
        if kdec::cmp(x, hi)? == Ordering::Greater {
            side = Some((*hi, Ordering::Greater));
        }
    }
    if side.is_none() {
        if let Some(lo) = &limits.lower {
            if kdec::cmp(x, lo)? == Ordering::Less {
                side = Some((*lo, Ordering::Less));
            }
        }
    }
    let mut digits_added: u16 = 0;
    let mut tie = false;
    if let Some((lim, want)) = side {
        loop {
            if cmp_out(op, v.neg, &out, &fac, &lim)? == want {
                break;
            }
            let next = match prec {
                Prec::Sig(n) if n < SIG_CAP => Prec::Sig(n + 1),
                Prec::Dec(p) if v.k + 1 + p < SIG_CAP => {
                    // A zero output stays zero up to p = -k - 2: skip those steps at once.
                    let skip_to = if out.q.is_zero() { (-v.k - 1).max(p + 1) } else { p + 1 };
                    Prec::Dec(skip_to.min(SIG_CAP - 1 - v.k))
                }
                _ => {
                    tie = true;
                    break;
                }
            };
            let added = match (prec, next) {
                (Prec::Dec(a), Prec::Dec(b)) | (Prec::Sig(a), Prec::Sig(b)) => b - a,
                _ => 1,
            };
            digits_added = digits_added.saturating_add(u16::try_from(added).unwrap_or(u16::MAX));
            prec = next;
            out = round_at(op, &v, prec)?;
        }
    }
    Ok(Reported {
        value: render(v.neg, &out),
        unit: spec.unit,
        exact_conversion: fac.conv,
        formatter: spec.formatter,
        digits_added,
        tie_at_max_precision: tie,
    })
}

fn capacity(op: &'static str) -> NumError {
    FmtError::Capacity.at(op)
}

fn check(op: &'static str, b: &Big) -> Result<(), NumError> {
    if b.overflowed() {
        Err(capacity(op))
    } else {
        Ok(())
    }
}

/// Decomposes `x` and forms `N / D` (step 1 and 2 of the Design algorithm).
fn exact(op: &'static str, x: f64, fac: &Factor) -> Result<Exact, NumError> {
    let bits = x.to_bits();
    let neg = bits >> 63 == 1;
    let bexp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (m, e) = if bexp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), bexp - 1075) };
    let mut n = Big::from_u128(u128::from(m));
    n.shl(e.max(0).unsigned_abs());
    n.mul_pow10(fac.num_pow10);
    let mut d = Big::from_u128(1);
    d.shl((-e).max(0).unsigned_abs());
    d.mul_small(fac.den);
    check(op, &n)?;
    check(op, &d)?;
    let k = if n.is_zero() { 0 } else { decimal_exponent(op, &n, &d)? };
    Ok(Exact { neg, n, d, k })
}

/// `N >= D * 10^k`, exactly.
fn ge_pow10(op: &'static str, n: &Big, d: &Big, k: i32) -> Result<bool, NumError> {
    let (mut l, mut r) = (*n, *d);
    if k >= 0 {
        r.mul_pow10(k.unsigned_abs());
    } else {
        l.mul_pow10(k.unsigned_abs());
    }
    check(op, &l)?;
    check(op, &r)?;
    Ok(l.cmp_mag(&r) != Ordering::Less)
}

/// The exact `k` with `10^k <= N / D < 10^(k+1)`, for non-zero `N`.
fn decimal_exponent(op: &'static str, n: &Big, d: &Big) -> Result<i32, NumError> {
    // N / D lies in [2^(b-1), 2^(b+1)) with b = bitlen(N) - bitlen(D), so b * log10(2)
    // (1233 / 4096 = 0.301025..) is within 2 of k; the exact tests below settle it.
    let b = n.bit_len() as i32 - d.bit_len() as i32;
    let mut k = (b * 1233).div_euclid(4096);
    for _ in 0..8 {
        if !ge_pow10(op, n, d, k)? {
            k -= 1;
        } else if ge_pow10(op, n, d, k + 1)? {
            k += 1;
        } else {
            return Ok(k);
        }
    }
    Err(capacity(op))
}

/// One half-even rounding of `|v|` at the given precision (step 3).
fn round_at(op: &'static str, v: &Exact, prec: Prec) -> Result<Out, NumError> {
    let mut p = match prec {
        Prec::Dec(p) => p,
        Prec::Sig(n) => n - 1 - v.k,
    };
    let (mut n, mut d) = (v.n, v.d);
    if p >= 0 {
        n.mul_pow10(p.unsigned_abs());
    } else {
        d.mul_pow10(p.unsigned_abs());
    }
    check(op, &n)?;
    check(op, &d)?;
    let (mut q, r) = n.div_rem(&d).ok_or_else(|| capacity(op))?;
    let mut twice = r;
    twice.shl(1);
    match twice.cmp_mag(&d) {
        Ordering::Greater => q.add_small(1),
        Ordering::Equal if q.is_odd() => q.add_small(1),
        _ => {}
    }
    if let Prec::Sig(digits) = prec {
        let mut decade = Big::from_u128(1);
        decade.mul_pow10(digits.unsigned_abs());
        if q.cmp_mag(&decade) == Ordering::Equal {
            q = Big::from_u128(1);
            q.mul_pow10((digits - 1).unsigned_abs());
            p -= 1;
        }
    }
    check(op, &twice)?;
    check(op, &q)?;
    Ok(Out { q, p })
}

/// Signed exact comparison of the output, converted back to SI units, with a limit
/// (step 5). Equivalent to `kdec::cmp_dec` of the output as a `Decimal` against the limit;
/// done on the big integer because the output coefficient may not fit a `Decimal`.
fn cmp_out(op: &'static str, neg: bool, out: &Out, fac: &Factor, lim: &Decimal) -> Result<Ordering, NumError> {
    let sign = |zero: bool, neg: bool| -> i8 {
        match (zero, neg) {
            (true, _) => 0,
            (false, true) => -1,
            (false, false) => 1,
        }
    };
    let os = sign(out.q.is_zero(), neg);
    let ls = sign(lim.is_zero(), lim.is_negative());
    if os != ls || os == 0 {
        return Ok(os.cmp(&ls));
    }
    // |out| = A * 10^a_exp, |lim| = B * 10^b_exp, A and B non-zero integers.
    let mut a = out.q;
    a.mul_small(fac.back_coeff);
    let a_exp = fac.back_exp - i64::from(out.p);
    let mut b = Big::from_u128(lim.coeff());
    let b_exp = i64::from(lim.exp10());
    let mag = if a_exp >= b_exp {
        let diff = a_exp - b_exp;
        if diff >= 39 {
            // A >= 1 and B <= u128::MAX < 10^39.
            Ordering::Greater
        } else {
            a.mul_pow10(diff as u32);
            check(op, &a)?;
            a.cmp_mag(&b)
        }
    } else {
        let diff = b_exp - a_exp;
        if diff * 3 >= i64::from(a.bit_len()) {
            // B * 10^diff >= 8^diff >= 2^bitlen(A) > A.
            Ordering::Less
        } else {
            b.mul_pow10(diff as u32);
            check(op, &b)?;
            a.cmp_mag(&b)
        }
    };
    check(op, &a)?;
    Ok(if os < 0 { mag.reverse() } else { mag })
}

/// Decimal digits of `q` (most significant first), `"0"` for zero.
fn digits(q: &Big) -> Vec<u8> {
    let mut t = *q;
    let mut chunks: Vec<u32> = Vec::new();
    while !t.is_zero() && !t.overflowed() {
        chunks.push(t.div_small(1_000_000_000));
    }
    let mut out = Vec::with_capacity(chunks.len() * 9 + 1);
    for c in chunks.iter().rev() {
        let mut c = *c;
        let mut nine = [b'0'; 9];
        for slot in nine.iter_mut().rev() {
            *slot = b'0' + (c % 10) as u8;
            c /= 10;
        }
        out.extend_from_slice(&nine);
    }
    let first = out.iter().position(|&c| c != b'0').unwrap_or(out.len());
    out.drain(..first);
    if out.is_empty() {
        out.push(b'0');
    }
    out
}

/// The canonical string (step 4).
fn render(neg: bool, out: &Out) -> String {
    let zero = out.q.is_zero();
    let mut ds = digits(&out.q);
    let mut s = String::with_capacity(ds.len() + out.p.unsigned_abs() as usize + 3);
    if neg && !zero {
        s.push('-');
    }
    if out.p <= 0 {
        if !zero {
            ds.resize(ds.len() + out.p.unsigned_abs() as usize, b'0');
        }
        s.extend(ds.iter().map(|&c| char::from(c)));
    } else {
        let p = out.p.unsigned_abs() as usize;
        if ds.len() < p + 1 {
            let mut padded = vec![b'0'; p + 1 - ds.len()];
            padded.extend_from_slice(&ds);
            ds = padded;
        }
        let (int, frac) = ds.split_at(ds.len() - p);
        s.extend(int.iter().map(|&c| char::from(c)));
        s.push('.');
        s.extend(frac.iter().map(|&c| char::from(c)));
    }
    s
}
