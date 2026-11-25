//! Typed access to the parameters of parsed instances. Every accessor returns a [`Fail`]
//! naming the entity whose parameter was wrong; nothing here panics.

use super::Fail;
use crate::{Instance, P21File, Param};

/// Real numbers within this magnitude convert from an integer token exactly.
const EXACT_INT: i64 = 1 << 53;

/// The parameters of instance `n` when it is a simple instance named `name`.
pub(crate) fn simple<'a>(file: &'a P21File, n: u64, name: &str) -> Result<&'a [Param], Fail> {
    let inst = get(file, n)?;
    match inst.parts.as_slice() {
        [(id, params)] if id.as_str() == name => Ok(params),
        _ => Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
    }
}

/// Instance `n`, or `dangling_reference`.
pub(crate) fn get(file: &P21File, n: u64) -> Result<&Instance, Fail> {
    file.get(n).ok_or(Fail::Dangling { to: n })
}

/// The parameters of part `name` of a complex instance, if present. A repeated part is
/// `malformed`: there is no first-wins.
pub(crate) fn part<'a>(inst: &'a Instance, name: &str) -> Result<Option<&'a [Param]>, Fail> {
    let mut found: Option<&'a [Param]> = None;
    for (id, params) in &inst.parts {
        if id.as_str() == name {
            if found.is_some() {
                return Err(Fail::Malformed { at: inst.number, what: "duplicate_part" });
            }
            found = Some(params);
        }
    }
    Ok(found)
}

/// Parameter `i` of `params`, or `malformed` (`arity`).
pub(crate) fn arg(params: &[Param], i: usize, at: u64) -> Result<&Param, Fail> {
    params.get(i).ok_or(Fail::Malformed { at, what: "arity" })
}

/// Requires exactly `count` parameters.
pub(crate) fn arity(params: &[Param], count: usize, at: u64) -> Result<(), Fail> {
    if params.len() == count {
        Ok(())
    } else {
        Err(Fail::Malformed { at, what: "arity" })
    }
}

/// A real: a real token, an integer token of magnitude at most 2^53 (converted exactly), or
/// either wrapped once in a typed parameter whose type name ends in `_MEASURE` or is
/// `PARAMETER_VALUE`. Non-finite values are refused.
pub(crate) fn real(p: &Param, at: u64) -> Result<f64, Fail> {
    let inner = match p {
        Param::Typed(name, inner) if name.as_str().ends_with("_MEASURE") || name.as_str() == "PARAMETER_VALUE" => {
            &**inner
        }
        other => other,
    };
    let v = match inner {
        Param::Real(v, _) => *v,
        Param::Int(i) if (-EXACT_INT..=EXACT_INT).contains(i) => *i as f64,
        _ => return Err(Fail::Malformed { at, what: "expected_real" }),
    };
    if v.is_finite() {
        Ok(v)
    } else {
        Err(Fail::Malformed { at, what: "non_finite_real" })
    }
}

/// A non-negative integer that fits `usize`.
pub(crate) fn count(p: &Param, at: u64) -> Result<usize, Fail> {
    match p {
        Param::Int(i) if *i >= 0 => usize::try_from(*i).map_err(|_| Fail::Malformed { at, what: "integer_range" }),
        _ => Err(Fail::Malformed { at, what: "expected_count" }),
    }
}

/// An entity reference.
pub(crate) fn reference(p: &Param, at: u64) -> Result<u64, Fail> {
    match p {
        Param::Ref(r) => Ok(*r),
        _ => Err(Fail::Malformed { at, what: "expected_reference" }),
    }
}

/// An optional entity reference (`$` is `None`).
pub(crate) fn opt_reference(p: &Param, at: u64) -> Result<Option<u64>, Fail> {
    match p {
        Param::Unset => Ok(None),
        other => reference(other, at).map(Some),
    }
}

/// A list.
pub(crate) fn list(p: &Param, at: u64) -> Result<&[Param], Fail> {
    match p {
        Param::List(items) => Ok(items),
        _ => Err(Fail::Malformed { at, what: "expected_list" }),
    }
}

/// A boolean enumeration `.T.` or `.F.`.
pub(crate) fn boolean(p: &Param, at: u64) -> Result<bool, Fail> {
    match p {
        Param::Enum(e) if e.as_str() == "T" => Ok(true),
        Param::Enum(e) if e.as_str() == "F" => Ok(false),
        _ => Err(Fail::Malformed { at, what: "expected_boolean" }),
    }
}

/// An enumeration value.
pub(crate) fn enumeration(p: &Param, at: u64) -> Result<&str, Fail> {
    match p {
        Param::Enum(e) => Ok(e.as_str()),
        _ => Err(Fail::Malformed { at, what: "expected_enumeration" }),
    }
}

/// A list of reals.
pub(crate) fn reals(p: &Param, at: u64) -> Result<Vec<f64>, Fail> {
    list(p, at)?.iter().map(|x| real(x, at)).collect()
}

/// A list of non-negative integers.
pub(crate) fn counts(p: &Param, at: u64) -> Result<Vec<usize>, Fail> {
    list(p, at)?.iter().map(|x| count(x, at)).collect()
}

/// A list of references.
pub(crate) fn references(p: &Param, at: u64) -> Result<Vec<u64>, Fail> {
    list(p, at)?.iter().map(|x| reference(x, at)).collect()
}
