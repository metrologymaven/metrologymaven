//! The function library and the display text of values (for `format`).
//!
//! - `abs(x)`: exact for `int` (overflow is `int_overflow`) and decimals, sign cleared for reals.
//! - `min(a, b, ...)`, `max(a, b, ...)`: a left fold that replaces the running value only by a
//!   strictly smaller (larger) argument under the comparison of `<`, reals against reals in
//!   IEEE total order (so `min(0.0, -0.0)` is `-0.0` in both orders, as `fid_num::f::total_min`).
//!   The chosen argument is returned as it is, except that an `int` becomes an exact decimal
//!   when another argument is not an `int` (the result is then typed `decimal`).
//! - `round(x)` (half to even), `floor(x)`, `ceil(x)`: exact, to `int`; `int_overflow` outside `i64`.
//! - `sqrt(x)`: `fid_num::f::sqrt` of the SI value; a negative is `domain`. The kind of a
//!   quantity's root follows the halved dimension.
//! - `sin`, `cos`, `tan`: `fid_num::f` of a plain number (radians) or an angle's SI value.
//! - `atan2(y, x)`: `fid_num::f::atan2` of the SI values, a real of the angle kind.
//! - `len(x)`: items of a list, Unicode scalar values of a string.
//! - `contains(list, v)` (any item `==` v), `contains(s, t)` (substring; `""` is in every string).
//! - `format(template, args...)`: each `{}` takes the [`display`] text of the next argument;
//!   `{{` and `}}` are literal braces. Any other brace, a count of `{}` other than the number
//!   of arguments, or a result above [`MAX_FORMAT_BYTES`] is `domain`.
//! - `to_unit(q, "sym")`: a decimal in unit `u` stays exact, written in `sym`, when the factor
//!   ratio `fu / ft` (both or neither with Π) in lowest terms has a denominator `2^a 5^b`, so
//!   the result is a terminating decimal (`to_unit(1[in], "mm")` is `25.4[mm]`); a result
//!   beyond 38 digits is `decimal_overflow`. Otherwise it is converted once into SI (REQ-003)
//!   and becomes a real. A real is returned unchanged (reals are always SI).

use super::dec::{Big, Dec, IntMode, SBig};
use super::num::{factor_of, finite, reduce, two_five, Fail};
use super::walk::{num, Ctx, Num};
use super::{EvalKind, Value, V};
use crate::expr::types::{Dim, Kind};
use fid_num::f;
use fid_registry::QuantityCatalogue;
use std::cmp::Ordering;

/// Longest `format` result, in bytes.
pub const MAX_FORMAT_BYTES: usize = 64 * 1024;

fn internal<T>() -> Result<T, Fail> {
    Err(Fail::Internal)
}

fn num_err(e: fid_num::NumError) -> Fail {
    match e {
        fid_num::NumError::Domain { .. } => Fail::Kind(EvalKind::Domain),
        _ => Fail::Kind(EvalKind::NonFinite),
    }
}

pub(crate) fn call(ctx: &Ctx<'_>, name: &str, a: &[Value]) -> Result<Value, Fail> {
    match (name, a) {
        ("abs", [x]) => match num(x).ok_or(Fail::Internal)? {
            Num::Int(i) => i.checked_abs().map(Value::int).ok_or(Fail::Kind(EvalKind::IntOverflow)),
            Num::Dec(d, u) => Ok(Value::of_dec(d.abs(), u.map(str::to_string))),
            Num::Real(r, k) => ctx.real(r.abs(), k.cloned()),
        },
        ("min", [_, ..]) => extreme(ctx, a, Ordering::Less),
        ("max", [_, ..]) => extreme(ctx, a, Ordering::Greater),
        ("round", [x]) => to_int(x, IntMode::HalfEven),
        ("floor", [x]) => to_int(x, IntMode::Floor),
        ("ceil", [x]) => to_int(x, IntMode::Ceil),
        ("sqrt", [x]) => {
            let n = num(x).ok_or(Fail::Internal)?;
            let kind = match ctx.kind_of(n)? {
                None => None,
                Some(k) => {
                    let d = ctx.kinds.dim_of(&k).and_then(Dim::half).ok_or(Fail::Internal)?;
                    ctx.dim_kind(d)?
                }
            };
            let r = f::sqrt(ctx.to_f64(n)?).map_err(num_err)?;
            ctx.real(r, kind)
        }
        ("sin" | "cos" | "tan", [x]) => {
            let v = ctx.to_f64(num(x).ok_or(Fail::Internal)?)?;
            let r = match name {
                "sin" => f::sin(v),
                "cos" => f::cos(v),
                _ => f::tan(v),
            }
            .map_err(num_err)?;
            ctx.real(r, None)
        }
        ("atan2", [y, x]) => {
            let (y, x) = (num(y).ok_or(Fail::Internal)?, num(x).ok_or(Fail::Internal)?);
            let r = f::atan2(ctx.to_f64(y)?, ctx.to_f64(x)?).map_err(num_err)?;
            ctx.real(r, ctx.dim_kind(Dim::ANGLE)?)
        }
        ("len", [x]) => {
            let n = match x.v() {
                V::List(xs) => xs.len(),
                V::Str(s) => s.chars().count(),
                _ => return internal(),
            };
            i64::try_from(n).map(Value::int).map_err(|_| Fail::Kind(EvalKind::IntOverflow))
        }
        ("contains", [hay, needle]) => match (hay.v(), needle.v()) {
            (V::Str(s), V::Str(t)) => Ok(Value::bool(s.contains(t.as_str()))),
            (V::List(xs), _) => {
                for x in xs {
                    if ctx.equal(x, needle)? {
                        return Ok(Value::bool(true));
                    }
                }
                Ok(Value::bool(false))
            }
            _ => internal(),
        },
        ("format", [tpl, args @ ..]) => match tpl.v() {
            V::Str(t) => format(ctx.cat, t, args).map(|s| Value::str(&s)),
            _ => internal(),
        },
        ("to_unit", [q, sym]) => match (q.v(), sym.v()) {
            (V::Dec(d, Some(u)), V::Str(s)) => to_unit(ctx, d, u, s),
            (V::Real(..), V::Str(_)) => Ok(q.clone()),
            _ => internal(),
        },
        _ => internal(),
    }
}

fn extreme(ctx: &Ctx<'_>, a: &[Value], want: Ordering) -> Result<Value, Fail> {
    let mut acc = a.first().ok_or(Fail::Internal)?;
    for x in a.iter().skip(1) {
        let o = match (x.v(), acc.v()) {
            (V::Real(p, _), V::Real(q, _)) => f::total_cmp(*p, *q).map_err(num_err)?,
            _ => ctx.cmp(x, acc)?,
        };
        if o == want {
            acc = x;
        }
    }
    let all_int = a.iter().all(|x| matches!(x.v(), V::Int(_)));
    match acc.v() {
        V::Int(i) if !all_int => Ok(Value::of_dec(Dec::from_i64(*i), None)),
        _ => Ok(acc.clone()),
    }
}

const TWO_63: f64 = 9_223_372_036_854_775_808.0;

fn to_int(x: &Value, mode: IntMode) -> Result<Value, Fail> {
    let ov = Fail::Kind(EvalKind::IntOverflow);
    match num(x).ok_or(Fail::Internal)? {
        Num::Int(i) => Ok(Value::int(i)),
        Num::Dec(d, None) => d.to_int(mode).map(Value::int).ok_or(ov),
        Num::Real(r, None) => {
            if !(-TWO_63..TWO_63).contains(&r) {
                return Err(ov);
            }
            // Truncation is exact here, and so is the fractional part `r - t`.
            let t = r as i64;
            let frac = r - t as f64;
            let step = |up: bool, t: i64| -> Result<i64, Fail> {
                if !up {
                    Ok(t)
                } else if frac > 0.0 {
                    t.checked_add(1).ok_or(ov)
                } else {
                    t.checked_sub(1).ok_or(ov)
                }
            };
            let up = match mode {
                IntMode::Floor => frac < 0.0,
                IntMode::Ceil => frac > 0.0,
                IntMode::HalfEven => {
                    let h = frac.abs();
                    h > 0.5 || (h == 0.5 && t % 2 != 0)
                }
            };
            step(up, t).map(Value::int)
        }
        _ => internal(),
    }
}

fn to_unit(ctx: &Ctx<'_>, d: &Dec, u: &str, sym: &str) -> Result<Value, Fail> {
    let kind = ctx.kind_of(Num::Dec(d, Some(u)))?;
    if u == sym {
        return Ok(Value::of_dec(*d, Some(sym.to_string())));
    }
    let ((nu, du, pu), (nt, dt, pt)) = (factor_of(Some(u), ctx.cat)?, factor_of(Some(sym), ctx.cat)?);
    if pu == pt {
        // value in sym = value in u * p / q, with p / q = fu / ft in lowest terms. It is a
        // terminating decimal exactly when q = 2^a 5^b: then p / q = p 2^(m-a) 5^(m-b) / 10^m
        // with m = max(a, b).
        let (p, q) = reduce(u128::from(nu) * u128::from(dt), u128::from(du) * u128::from(nt));
        if let Some((a, b)) = two_five(q) {
            let m = a.max(b);
            let mut mag = Big::from_u128(d.coef().unsigned_abs()).mul(&Big::from_u128(p));
            mag.shl(m - a);
            for _ in 0..(m - b) {
                mag.mul_u32(5);
            }
            let exact = SBig { neg: d.is_negative(), mag };
            let dd = Dec::fit(exact, i64::from(d.scale()) + i64::from(m)).map_err(|_| Fail::DecOverflow)?;
            return Ok(Value::of_dec(dd, Some(sym.to_string())));
        }
    }
    let si = ctx.to_f64(Num::Dec(d, Some(u)))?;
    Value::real(finite(si)?, kind).map_err(|_| Fail::Kind(EvalKind::NonFinite))
}

fn format(cat: &dyn QuantityCatalogue, t: &str, args: &[Value]) -> Result<String, Fail> {
    let dom = Fail::Kind(EvalKind::Domain);
    let mut out = String::new();
    let mut it = t.chars().peekable();
    let mut next = args.iter();
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '{' if it.peek() == Some(&'}') => {
                it.next();
                let v = next.next().ok_or(dom)?;
                out.push_str(&display(cat, v).ok_or(Fail::Internal)?);
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '{' | '}' => return Err(dom),
            c => out.push(c),
        }
        if out.len() > MAX_FORMAT_BYTES {
            return Err(dom);
        }
    }
    if next.next().is_some() {
        return Err(dom);
    }
    Ok(out)
}

/// The display text of a value: `true`/`false`; an `int` in decimal; a decimal as written,
/// then ` <unit>` if it has one; a real as Rust's shortest round-trip `f64` text (`{}`, no
/// exponent), then ` <canonical unit>` of its kind (or ` <dimension>` for a derived kind);
/// a string, enum member or element ID as itself; a list as `[a, b]`; a record as
/// `{k: v, ...}` in key order. `None` for a kind the catalogue lacks.
pub fn display(cat: &dyn QuantityCatalogue, v: &Value) -> Option<String> {
    Some(match v.v() {
        V::Bool(b) => b.to_string(),
        V::Int(i) => i.to_string(),
        V::Dec(d, None) => d.text(),
        V::Dec(d, Some(u)) => format!("{} {u}", d.text()),
        V::Real(x, None) => format!("{x}"),
        V::Real(x, Some(Kind::Named(k))) => format!("{x} {}", cat.kind(k)?.canonical),
        V::Real(x, Some(Kind::Derived(dim))) => format!("{x} {dim}"),
        V::Str(s) | V::Enum(s) | V::Element(s) => s.clone(),
        V::List(xs) => {
            let parts = xs.iter().map(|x| display(cat, x)).collect::<Option<Vec<_>>>()?;
            format!("[{}]", parts.join(", "))
        }
        V::Record(m) => {
            let parts =
                m.iter().map(|(k, x)| display(cat, x).map(|s| format!("{k}: {s}"))).collect::<Option<Vec<_>>>()?;
            format!("{{{}}}", parts.join(", "))
        }
    })
}
