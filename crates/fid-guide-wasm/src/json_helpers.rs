//! Zero-panic extraction and construction helpers over `fid_canon::CValue`.

use std::collections::BTreeMap;

use fid_canon::CValue;

use crate::LabError;

/// Canonical f64 (normalizes `-0.0` to `+0.0` and rejects non-finite values).
pub fn c_f64(v: f64) -> Result<CValue, LabError> {
    if !v.is_finite() {
        return Err(LabError::new("non_finite_output", "computed a non-finite floating-point value"));
    }
    let clean = if v == 0.0 { 0.0 } else { v };
    Ok(CValue::Num(clean))
}

pub fn c_u64(v: u64) -> Result<CValue, LabError> {
    let f = v as f64;
    if !f.is_finite() {
        return Err(LabError::new("non_finite_output", "integer out of f64 range"));
    }
    Ok(CValue::Num(f))
}

pub fn c_usize(v: usize) -> Result<CValue, LabError> {
    c_u64(u64::try_from(v).map_err(|_| LabError::new("overflow", "size exceeds u64"))?)
}

pub fn c_str(s: impl Into<String>) -> CValue {
    CValue::Str(s.into())
}

pub fn c_bool(b: bool) -> CValue {
    CValue::Bool(b)
}

pub fn c_vec_f64(vals: &[f64]) -> Result<CValue, LabError> {
    let mut out = Vec::with_capacity(vals.len());
    for &v in vals {
        out.push(c_f64(v)?);
    }
    Ok(CValue::Arr(out))
}

pub fn c_vec_u32(vals: &[u32]) -> Result<CValue, LabError> {
    let mut out = Vec::with_capacity(vals.len());
    for &v in vals {
        out.push(c_u64(u64::from(v))?);
    }
    Ok(CValue::Arr(out))
}

pub fn req_f64(map: &BTreeMap<String, CValue>, key: &str) -> Result<f64, LabError> {
    match map.get(key) {
        Some(CValue::Num(v)) if v.is_finite() => Ok(*v),
        _ => Err(LabError::new("invalid_param", format!("expected finite number for `{key}`"))),
    }
}

pub fn opt_f64(map: &BTreeMap<String, CValue>, key: &str) -> Result<Option<f64>, LabError> {
    match map.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Num(v)) if v.is_finite() => Ok(Some(*v)),
        _ => Err(LabError::new("invalid_param", format!("expected finite number for `{key}`"))),
    }
}

pub fn opt_u64(map: &BTreeMap<String, CValue>, key: &str) -> Result<Option<u64>, LabError> {
    match map.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Num(v)) if v.is_finite() && *v >= 0.0 && v.fract() == 0.0 && *v <= (u64::MAX as f64) => {
            Ok(Some(*v as u64))
        }
        _ => Err(LabError::new("invalid_param", format!("expected non-negative integer for `{key}`"))),
    }
}

pub fn req_str<'a>(map: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, LabError> {
    match map.get(key) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(LabError::new("invalid_param", format!("expected string for `{key}`"))),
    }
}

pub fn opt_str<'a>(map: &'a BTreeMap<String, CValue>, key: &str) -> Result<Option<&'a str>, LabError> {
    match map.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        _ => Err(LabError::new("invalid_param", format!("expected string for `{key}`"))),
    }
}

pub fn opt_bool(map: &BTreeMap<String, CValue>, key: &str, default: bool) -> Result<bool, LabError> {
    match map.get(key) {
        None | Some(CValue::Null) => Ok(default),
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(LabError::new("invalid_param", format!("expected boolean for `{key}`"))),
    }
}

pub fn req_arr<'a>(map: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a [CValue], LabError> {
    match map.get(key) {
        Some(CValue::Arr(a)) => Ok(a.as_slice()),
        _ => Err(LabError::new("invalid_param", format!("expected array for `{key}`"))),
    }
}

pub fn parse_vec3(v: &CValue, label: &str) -> Result<[f64; 3], LabError> {
    let CValue::Arr(a) = v else {
        return Err(LabError::new("invalid_param", format!("expected 3-element array for `{label}`")));
    };
    if a.len() != 3 {
        return Err(LabError::new("invalid_param", format!("expected 3 numbers for `{label}`")));
    }
    let mut out = [0.0; 3];
    for (dst, item) in out.iter_mut().zip(a.iter()) {
        let CValue::Num(x) = item else {
            return Err(LabError::new("invalid_param", format!("non-numeric entry in `{label}`")));
        };
        if !x.is_finite() {
            return Err(LabError::new("invalid_param", format!("non-finite entry in `{label}`")));
        }
        *dst = *x;
    }
    Ok(out)
}

pub fn parse_points2d(map: &BTreeMap<String, CValue>, key: &str, max_pts: usize) -> Result<Vec<[f64; 2]>, LabError> {
    let arr = req_arr(map, key)?;
    if arr.len() > max_pts {
        return Err(LabError::new("too_many_points", format!("`{key}` exceeds {max_pts} points")));
    }
    let mut pts = Vec::with_capacity(arr.len());
    for item in arr {
        let CValue::Arr(pair) = item else {
            return Err(LabError::new("invalid_param", format!("each entry of `{key}` must be [x, y]")));
        };
        if pair.len() != 2 {
            return Err(LabError::new("invalid_param", format!("each entry of `{key}` must have 2 numbers")));
        }
        let (CValue::Num(x), CValue::Num(y)) = (&pair[0], &pair[1]) else {
            return Err(LabError::new("invalid_param", format!("non-numeric coordinate in `{key}`")));
        };
        if !x.is_finite() || !y.is_finite() {
            return Err(LabError::new("invalid_param", format!("non-finite coordinate in `{key}`")));
        }
        pts.push([*x, *y]);
    }
    Ok(pts)
}

pub fn parse_points3d(map: &BTreeMap<String, CValue>, key: &str, max_pts: usize) -> Result<Vec<[f64; 3]>, LabError> {
    let arr = req_arr(map, key)?;
    if arr.len() > max_pts {
        return Err(LabError::new("too_many_points", format!("`{key}` exceeds {max_pts} points")));
    }
    let mut pts = Vec::with_capacity(arr.len());
    for item in arr {
        pts.push(parse_vec3(item, key)?);
    }
    Ok(pts)
}
