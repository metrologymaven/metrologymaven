//! Wire parsing of `capability-declaration/v1` and `runtime-support/v1` documents. Every
//! object has a closed key set; duplicate keys, oversize, too-deep or too-large inputs are
//! refused before any value is built. Errors never echo input text: they name a checked
//! capability name or a JSON pointer built from fixed keys and indices.

use super::{
    check_offer, is_capability_name, CapabilityDeclaration, CapabilityOffer, FirmwareRange, MajorSupport, NegError,
    VersionRange, DECLARATION_SCHEMA, MAX_CAPABILITIES, MAX_DEPTH, MAX_ELEMENTS, MAX_FEATURES, MAX_INPUT_BYTES,
    MAX_LIMITS, MAX_MAJORS, SUPPORT_SCHEMA,
};
use crate::base::device::DeviceInstance;
use crate::base::error::DeviceText;
use crate::package::is_semver;
use crate::units::{Quantity, TimeSource};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

type Obj = BTreeMap<String, CValue>;
type Fail = fn(String, &'static str) -> NegError;

const DECL_FIELDS: [&str; 7] =
    ["capabilities", "conformance_report", "device", "driver_version", "schema", "tested_firmware", "time_sources"];
const OFFER_FIELDS: [&str; 4] = ["features", "limits", "name", "versions"];
const FIRMWARE_FIELDS: [&str; 2] = ["highest", "lowest"];
const SUPPORT_FIELDS: [&str; 2] = ["capabilities", "schema"];
const MAJOR_FIELDS: [&str; 4] = ["deprecated", "features", "highest_minor", "major"];

fn parse(bytes: &[u8], fail: Fail) -> Result<CValue, NegError> {
    fid_canon::parse_with_budget(bytes, MAX_INPUT_BYTES, MAX_DEPTH, MAX_ELEMENTS)
        .map_err(|_| fail(String::new(), "json"))
}

/// An object whose keys are all in `allowed`; an unknown key is reported at `ptr`, without
/// echoing the key.
fn object(v: CValue, ptr: &str, allowed: &[&str], fail: Fail) -> Result<Obj, NegError> {
    let CValue::Obj(o) = v else {
        return Err(fail(ptr.to_string(), "not_object"));
    };
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(fail(ptr.to_string(), "field_unknown"));
    }
    Ok(o)
}

fn take(o: &mut Obj, ptr: &str, key: &str, fail: Fail) -> Result<CValue, NegError> {
    o.remove(key).ok_or_else(|| fail(format!("{ptr}/{key}"), "field_missing"))
}

fn string(v: CValue, ptr: String, fail: Fail) -> Result<String, NegError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(fail(ptr, "wrong_type")),
    }
}

fn array(v: CValue, ptr: String, max: usize, fail: Fail) -> Result<Vec<CValue>, NegError> {
    match v {
        CValue::Arr(a) if a.len() <= max => Ok(a),
        CValue::Arr(_) => Err(fail(ptr, "too_many_entries")),
        _ => Err(fail(ptr, "wrong_type")),
    }
}

fn strings(v: CValue, ptr: &str, max: usize, fail: Fail) -> Result<Vec<String>, NegError> {
    array(v, ptr.to_string(), max, fail)?
        .into_iter()
        .enumerate()
        .map(|(i, item)| string(item, format!("{ptr}/{i}"), fail))
        .collect()
}

/// A non-negative integer that fits `u32`, written as a plain JSON number.
fn integer(v: CValue, ptr: String, fail: Fail) -> Result<u32, NegError> {
    let ok = match v {
        CValue::Num(f) if (0.0..=f64::from(u32::MAX)).contains(&f) && f.fract() == 0.0 => u32::try_from(f as u64).ok(),
        _ => None,
    };
    ok.ok_or_else(|| fail(ptr, "integer_invalid"))
}

fn text(v: CValue, ptr: String) -> Result<DeviceText, NegError> {
    DeviceText::from_value(v).map_err(|_| NegError::decl(ptr, "device_text_invalid"))
}

// ------------------------------------------------------------------ declaration

pub(super) fn declaration(bytes: &[u8]) -> Result<CapabilityDeclaration, NegError> {
    let fail: Fail = NegError::decl;
    let mut o = object(parse(bytes, fail)?, "", &DECL_FIELDS, fail)?;
    if string(take(&mut o, "", "schema", fail)?, "/schema".into(), fail)? != DECLARATION_SCHEMA {
        return Err(NegError::decl("/schema", "schema_unknown"));
    }
    let driver_version = string(take(&mut o, "", "driver_version", fail)?, "/driver_version".into(), fail)?;
    if !is_semver(&driver_version) {
        return Err(NegError::decl("/driver_version", "driver_version_invalid"));
    }
    let device = DeviceInstance::from_value(take(&mut o, "", "device", fail)?)
        .map_err(|e| NegError::decl(format!("/device{}", e.pointer), e.reason))?;

    let mut capabilities = Vec::new();
    let items = array(take(&mut o, "", "capabilities", fail)?, "/capabilities".into(), MAX_CAPABILITIES, fail)?;
    for (i, item) in items.into_iter().enumerate() {
        capabilities.push(offer(item, &format!("/capabilities/{i}"))?);
    }

    let mut time_sources = Vec::new();
    let names = strings(take(&mut o, "", "time_sources", fail)?, "/time_sources", TimeSource::ALL.len(), fail)?;
    for (i, s) in names.iter().enumerate() {
        let t =
            TimeSource::parse(s).map_err(|_| NegError::decl(format!("/time_sources/{i}"), "time_source_invalid"))?;
        if time_sources.contains(&t) {
            return Err(NegError::decl(format!("/time_sources/{i}"), "duplicate_time_source"));
        }
        time_sources.push(t);
    }
    if time_sources.is_empty() {
        return Err(NegError::decl("/time_sources", "time_sources_empty"));
    }

    let mut fw = object(take(&mut o, "", "tested_firmware", fail)?, "/tested_firmware", &FIRMWARE_FIELDS, fail)?;
    let tested_firmware = FirmwareRange {
        lowest: text(take(&mut fw, "/tested_firmware", "lowest", fail)?, "/tested_firmware/lowest".into())?,
        highest: text(take(&mut fw, "/tested_firmware", "highest", fail)?, "/tested_firmware/highest".into())?,
    };
    let report = string(take(&mut o, "", "conformance_report", fail)?, "/conformance_report".into(), fail)?;
    let conformance_report =
        Digest::parse(&report).map_err(|_| NegError::decl("/conformance_report", "digest_invalid"))?;

    Ok(CapabilityDeclaration {
        driver_version,
        device,
        capabilities,
        time_sources,
        tested_firmware,
        conformance_report,
    })
}

/// One offer. The name is checked first, so later errors can name the capability.
fn offer(v: CValue, ptr: &str) -> Result<CapabilityOffer, NegError> {
    let fail: Fail = NegError::decl;
    let mut o = object(v, ptr, &OFFER_FIELDS, fail)?;
    let name = string(take(&mut o, ptr, "name", fail)?, format!("{ptr}/name"), fail)?;
    if !is_capability_name(&name) {
        return Err(NegError::decl(format!("{ptr}/name"), "capability_name_invalid"));
    }
    let versions = match take(&mut o, ptr, "versions", fail)? {
        CValue::Str(s) => VersionRange::parse(&s),
        _ => None,
    }
    .ok_or_else(|| NegError::decl(name.clone(), "version_range_invalid"))?;
    let features = match take(&mut o, ptr, "features", fail)? {
        CValue::Arr(a) if a.len() <= MAX_FEATURES => a
            .into_iter()
            .map(|f| match f {
                CValue::Str(s) => Ok(s),
                _ => Err(NegError::decl(name.clone(), "feature_invalid")),
            })
            .collect::<Result<Vec<String>, NegError>>()?,
        CValue::Arr(_) => return Err(NegError::decl(name, "too_many_features")),
        _ => return Err(NegError::decl(name, "feature_invalid")),
    };
    let CValue::Obj(raw_limits) = take(&mut o, ptr, "limits", fail)? else {
        return Err(NegError::decl(name, "limit_invalid"));
    };
    if raw_limits.len() > MAX_LIMITS {
        return Err(NegError::decl(name, "too_many_limits"));
    }
    let mut limits = BTreeMap::new();
    for (k, q) in raw_limits {
        let q = Quantity::from_value(q).map_err(|_| NegError::decl(name.clone(), "limit_invalid"))?;
        limits.insert(k, q);
    }
    let offer = CapabilityOffer { name, versions, features, limits };
    check_offer(&offer)?;
    Ok(offer)
}

// ------------------------------------------------------------------ runtime support

pub(super) fn support(bytes: &[u8]) -> Result<BTreeMap<String, BTreeMap<u32, MajorSupport>>, NegError> {
    let fail: Fail = NegError::support;
    let mut o = object(parse(bytes, fail)?, "", &SUPPORT_FIELDS, fail)?;
    if string(take(&mut o, "", "schema", fail)?, "/schema".into(), fail)? != SUPPORT_SCHEMA {
        return Err(NegError::support("/schema", "schema_unknown"));
    }
    let CValue::Obj(caps) = take(&mut o, "", "capabilities", fail)? else {
        return Err(NegError::support("/capabilities", "not_object"));
    };
    if caps.len() > MAX_CAPABILITIES {
        return Err(NegError::support("/capabilities", "too_many_capabilities"));
    }
    let mut out = BTreeMap::new();
    for (name, rows) in caps {
        if !is_capability_name(&name) {
            return Err(NegError::support("/capabilities", "capability_name_invalid"));
        }
        let ptr = format!("/capabilities/{name}");
        let mut majors = BTreeMap::new();
        for (i, row) in array(rows, ptr.clone(), MAX_MAJORS, fail)?.into_iter().enumerate() {
            let rp = format!("{ptr}/{i}");
            let mut r = object(row, &rp, &MAJOR_FIELDS, fail)?;
            let major = integer(take(&mut r, &rp, "major", fail)?, format!("{rp}/major"), fail)?;
            let highest_minor =
                integer(take(&mut r, &rp, "highest_minor", fail)?, format!("{rp}/highest_minor"), fail)?;
            let deprecated = match take(&mut r, &rp, "deprecated", fail)? {
                CValue::Bool(b) => b,
                _ => return Err(NegError::support(format!("{rp}/deprecated"), "wrong_type")),
            };
            let mut features = BTreeSet::new();
            for f in strings(take(&mut r, &rp, "features", fail)?, &format!("{rp}/features"), MAX_FEATURES, fail)? {
                if !features.insert(f) {
                    return Err(NegError::support(format!("{rp}/features"), "duplicate_feature"));
                }
            }
            if majors.insert(major, MajorSupport { highest_minor, features, deprecated }).is_some() {
                return Err(NegError::support(name.clone(), "duplicate_major"));
            }
        }
        out.insert(name, majors);
    }
    Ok(out)
}
