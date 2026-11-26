//! Bounded, fail-closed parsing of a doctor report (REQ-PLAT.DOCTOR.REPORT-005).
//!
//! Order: size cap ([`MAX_REPORT_BYTES`]), JSON syntax (duplicate keys refused), root object,
//! `schema_version` (an integer other than 1 is [`ReportError::UnsupportedSchema`], so a
//! report of a later schema is told apart from a broken one), then every key of the schema.
//! Beyond what the JSON Schema can say, the parser also refuses an assessment whose verdicts,
//! reasons, outcome and exit code do not fit together, profiles out of name order, and
//! shortfalls or unknowns out of field order.

use super::{valid_tool_version, DoctorReport, ReportError, MAX_PROFILES, MAX_PROFILE_NAME_CHARS, MAX_REPORT_BYTES};
use crate::lookup::{self, Assessment, Field, Outcome, ProfileVerdict, Shortfall, Value, Verdict, VerdictReason};
use crate::probe::{EnvSnapshot, Probed, UnknownReason, MAX_DETAIL_CHARS};
use fid_canon::CValue;
use fid_platform_core::hw_profile::{GpuClass, MINIMUM, RECOMMENDED};
use fid_platform_core::matrix::{EffectiveTier, Phase, Tier};
use std::collections::BTreeMap;

/// Deepest JSON nesting accepted in a report.
const MAX_JSON_DEPTH: usize = 16;

/// CPU instruction-set levels a shortfall may name (the levels `lookup` ranks).
const CPU_LEVELS: [&str; 6] = ["x86-64-v1", "x86-64-v2", "x86-64-v3", "x86-64-v4", "armv8.0", "armv8.2"];

type Obj = BTreeMap<String, CValue>;

fn invalid(pointer: &str, detail: impl Into<String>) -> ReportError {
    ReportError::SchemaInvalid { pointer: pointer.to_string(), detail: detail.into() }
}

fn as_obj<'a>(v: &'a CValue, ptr: &str) -> Result<&'a Obj, ReportError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(invalid(ptr, "must be an object")),
    }
}

fn exact_keys(m: &Obj, expected: &[&str], ptr: &str) -> Result<(), ReportError> {
    for k in expected {
        if !m.contains_key(*k) {
            return Err(invalid(&format!("{ptr}/{k}"), "missing"));
        }
    }
    if let Some(extra) = m.keys().find(|k| !expected.contains(&k.as_str())) {
        return Err(invalid(&format!("{ptr}/{}", escape_pointer(extra)), "unknown key"));
    }
    Ok(())
}

/// RFC 6901 escaping of one reference token.
fn escape_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn field<'a>(m: &'a Obj, key: &str, ptr: &str) -> Result<&'a CValue, ReportError> {
    m.get(key).ok_or_else(|| invalid(&format!("{ptr}/{key}"), "missing"))
}

fn string<'a>(v: &'a CValue, ptr: &str) -> Result<&'a str, ReportError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(invalid(ptr, "must be a string")),
    }
}

fn u32_of(v: &CValue, ptr: &str) -> Result<u32, ReportError> {
    match v {
        CValue::Num(n) if n.is_finite() && *n >= 0.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => Ok(*n as u32),
        _ => Err(invalid(ptr, "must be a non-negative integer")),
    }
}

fn array<'a>(v: &'a CValue, ptr: &str) -> Result<&'a [CValue], ReportError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(invalid(ptr, "must be an array")),
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<DoctorReport, ReportError> {
    if bytes.len() > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge { len: bytes.len(), max: MAX_REPORT_BYTES });
    }
    let v = fid_canon::parse_with_limits(bytes, MAX_REPORT_BYTES, MAX_JSON_DEPTH)
        .map_err(|e| ReportError::Parse { code: e.code() })?;
    let root = as_obj(&v, "")?;
    match root.get("schema_version") {
        None => return Err(invalid("/schema_version", "missing")),
        Some(CValue::Num(n)) if n.is_finite() && n.fract() == 0.0 => {
            if *n != f64::from(super::SCHEMA_VERSION) {
                let found = fid_canon::format_number(*n).unwrap_or_else(|_| "?".to_string());
                return Err(ReportError::UnsupportedSchema { found });
            }
        }
        Some(_) => return Err(invalid("/schema_version", "must be an integer")),
    }
    exact_keys(root, &super::REPORT_KEYS, "")?;

    let tool_version = string(field(root, "tool_version", "")?, "/tool_version")?;
    if !valid_tool_version(tool_version) {
        return Err(invalid("/tool_version", "must be 1 to 64 characters with no control or format character"));
    }
    let phase_s = string(field(root, "phase", "")?, "/phase")?;
    let phase = Phase::from_name(phase_s).ok_or_else(|| invalid("/phase", "unknown phase"))?;
    let redacted = match field(root, "redacted", "")? {
        CValue::Bool(b) => *b,
        _ => return Err(invalid("/redacted", "must be a boolean")),
    };
    let snapshot =
        EnvSnapshot::from_cvalue(field(root, "snapshot", "")?).map_err(|e| invalid("/snapshot", e.message))?;
    let assessment = parse_assessment(field(root, "assessment", "")?, "/assessment")?;

    Ok(DoctorReport {
        schema_version: super::SCHEMA_VERSION,
        tool_version: tool_version.to_string(),
        phase,
        redacted,
        snapshot,
        assessment,
    })
}

fn parse_assessment(v: &CValue, ptr: &str) -> Result<Assessment, ReportError> {
    let m = as_obj(v, ptr)?;
    exact_keys(m, &["exit_code", "outcome", "profiles", "schema_version", "tier"], ptr)?;
    let sv_ptr = format!("{ptr}/schema_version");
    if u32_of(field(m, "schema_version", ptr)?, &sv_ptr)? != lookup::SCHEMA_VERSION {
        return Err(invalid(&sv_ptr, "unsupported assessment schema_version"));
    }
    let tier = parse_tier(field(m, "tier", ptr)?, &format!("{ptr}/tier"))?;

    let profiles_ptr = format!("{ptr}/profiles");
    let items = array(field(m, "profiles", ptr)?, &profiles_ptr)?;
    if items.len() > MAX_PROFILES {
        return Err(invalid(&profiles_ptr, format!("more than {MAX_PROFILES} profiles")));
    }
    let mut profiles: Vec<ProfileVerdict> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let p = parse_profile(item, &format!("{profiles_ptr}/{i}"))?;
        if let Some(prev) = profiles.last() {
            if prev.name.as_str() >= p.name.as_str() {
                return Err(invalid(
                    &format!("{profiles_ptr}/{i}/name"),
                    "profiles must be in strictly increasing name order",
                ));
            }
        }
        profiles.push(p);
    }

    let outcome_ptr = format!("{ptr}/outcome");
    let outcome_s = string(field(m, "outcome", ptr)?, &outcome_ptr)?;
    let outcome = Outcome::from_name(outcome_s).ok_or_else(|| invalid(&outcome_ptr, "unknown outcome"))?;
    let exit_ptr = format!("{ptr}/exit_code");
    let exit_code = u32_of(field(m, "exit_code", ptr)?, &exit_ptr)?;
    if i64::from(exit_code) != i64::from(outcome.exit_code()) {
        return Err(invalid(&exit_ptr, "does not match the outcome"));
    }
    let verdict_of =
        |name: &str| profiles.iter().find(|p| p.name == name).map_or(Verdict::Indeterminate, |p| p.verdict);
    if lookup::outcome(&tier, verdict_of(MINIMUM), verdict_of(RECOMMENDED)) != outcome {
        return Err(invalid(&outcome_ptr, "does not follow from the tier and the profile verdicts"));
    }
    Ok(Assessment { tier, profiles, outcome })
}

fn parse_tier(v: &CValue, ptr: &str) -> Result<Probed<EffectiveTier>, ReportError> {
    let m = as_obj(v, ptr)?;
    let status_ptr = format!("{ptr}/status");
    match string(field(m, "status", ptr)?, &status_ptr)? {
        "known" => {
            exact_keys(m, &["status", "value"], ptr)?;
            let vptr = format!("{ptr}/value");
            let t = as_obj(field(m, "value", ptr)?, &vptr)?;
            let kind_ptr = format!("{vptr}/kind");
            let tier_of = |t: &Obj| -> Result<Tier, ReportError> {
                let tp = format!("{vptr}/tier");
                Tier::from_name(string(field(t, "tier", &vptr)?, &tp)?).ok_or_else(|| invalid(&tp, "unknown tier"))
            };
            let et = match string(field(t, "kind", &vptr)?, &kind_ptr)? {
                "tier" => {
                    exact_keys(t, &["kind", "tier"], &vptr)?;
                    EffectiveTier::Tier(tier_of(t)?)
                }
                "planned" => {
                    exact_keys(t, &["kind", "since_phase", "tier"], &vptr)?;
                    let sp = format!("{vptr}/since_phase");
                    let since = Phase::from_name(string(field(t, "since_phase", &vptr)?, &sp)?)
                        .ok_or_else(|| invalid(&sp, "unknown phase"))?;
                    EffectiveTier::Planned(tier_of(t)?, since)
                }
                "not_listed" => {
                    exact_keys(t, &["kind"], &vptr)?;
                    EffectiveTier::NotListed
                }
                _ => return Err(invalid(&kind_ptr, "must be tier, planned or not_listed")),
            };
            Ok(Probed::Known(et))
        }
        "unknown" => {
            exact_keys(m, &["detail", "reason", "status"], ptr)?;
            let rp = format!("{ptr}/reason");
            let reason = UnknownReason::parse(string(field(m, "reason", ptr)?, &rp)?)
                .ok_or_else(|| invalid(&rp, "unknown reason"))?;
            let dp = format!("{ptr}/detail");
            let detail = string(field(m, "detail", ptr)?, &dp)?;
            if detail.chars().count() > MAX_DETAIL_CHARS {
                return Err(invalid(&dp, format!("exceeds {MAX_DETAIL_CHARS} characters")));
            }
            Ok(Probed::Unknown { reason, detail: detail.to_string() })
        }
        _ => Err(invalid(&status_ptr, "must be known or unknown")),
    }
}

fn parse_profile(v: &CValue, ptr: &str) -> Result<ProfileVerdict, ReportError> {
    let m = as_obj(v, ptr)?;
    exact_keys(m, &["name", "reason", "shortfalls", "unknowns", "verdict"], ptr)?;
    let np = format!("{ptr}/name");
    let name = string(field(m, "name", ptr)?, &np)?;
    let n = name.chars().count();
    if n == 0 || n > MAX_PROFILE_NAME_CHARS || name.chars().any(super::is_escaped_char) {
        return Err(invalid(&np, "must be 1 to 64 characters with no control or format character"));
    }
    let vp = format!("{ptr}/verdict");
    let verdict =
        Verdict::from_name(string(field(m, "verdict", ptr)?, &vp)?).ok_or_else(|| invalid(&vp, "unknown verdict"))?;
    let rp = format!("{ptr}/reason");
    let reason = match field(m, "reason", ptr)? {
        CValue::Null => None,
        CValue::Str(s) => Some(VerdictReason::from_name(s).ok_or_else(|| invalid(&rp, "unknown reason"))?),
        _ => return Err(invalid(&rp, "must be a string or null")),
    };

    let sp = format!("{ptr}/shortfalls");
    let mut shortfalls: Vec<Shortfall> = Vec::new();
    for (i, s) in array(field(m, "shortfalls", ptr)?, &sp)?.iter().enumerate() {
        let s = parse_shortfall(s, &format!("{sp}/{i}"))?;
        if shortfalls.last().is_some_and(|prev| prev.field >= s.field) {
            return Err(invalid(&format!("{sp}/{i}/field"), "shortfalls must be in strictly increasing field order"));
        }
        shortfalls.push(s);
    }
    let up = format!("{ptr}/unknowns");
    let mut unknowns: Vec<Field> = Vec::new();
    for (i, f) in array(field(m, "unknowns", ptr)?, &up)?.iter().enumerate() {
        let fp = format!("{up}/{i}");
        let f = Field::from_name(string(f, &fp)?).ok_or_else(|| invalid(&fp, "unknown field"))?;
        if unknowns.last().is_some_and(|prev| *prev >= f) {
            return Err(invalid(&fp, "unknowns must be in strictly increasing field order"));
        }
        if shortfalls.iter().any(|s| s.field == f) {
            return Err(invalid(&fp, "a field cannot be both a shortfall and unknown"));
        }
        unknowns.push(f);
    }

    // The combinations `lookup::assess` produces.
    let consistent = match (verdict, reason) {
        (Verdict::Indeterminate, Some(_)) => shortfalls.is_empty() && unknowns.is_empty(),
        (_, Some(_)) => false,
        (Verdict::Meets, None) => shortfalls.is_empty() && unknowns.is_empty(),
        (Verdict::Below, None) => !shortfalls.is_empty(),
        (Verdict::Indeterminate, None) => shortfalls.is_empty() && !unknowns.is_empty(),
    };
    if !consistent {
        return Err(invalid(&vp, "verdict does not fit its reason, shortfalls and unknowns"));
    }
    Ok(ProfileVerdict { name: name.to_string(), verdict, reason, shortfalls, unknowns })
}

fn parse_shortfall(v: &CValue, ptr: &str) -> Result<Shortfall, ReportError> {
    let m = as_obj(v, ptr)?;
    exact_keys(m, &["field", "found", "required"], ptr)?;
    let fp = format!("{ptr}/field");
    let field_v =
        Field::from_name(string(field(m, "field", ptr)?, &fp)?).ok_or_else(|| invalid(&fp, "unknown field"))?;
    let rq = format!("{ptr}/required");
    let fd = format!("{ptr}/found");
    let required_v = field(m, "required", ptr)?;
    let found_v = field(m, "found", ptr)?;
    let (required, found) = match field_v {
        Field::CpuLevel => (cpu_level(required_v, &rq)?, cpu_level(found_v, &fd)?),
        Field::Cores => (Value::Count(u32_of(required_v, &rq)?), Value::Count(u32_of(found_v, &fd)?)),
        Field::RamGib | Field::VramGib | Field::StorageFreeGib => {
            let g = match found_v {
                CValue::Num(n) if n.is_finite() && *n >= 0.0 => *n,
                _ => return Err(invalid(&fd, "must be a finite non-negative number")),
            };
            (Value::Count(u32_of(required_v, &rq)?), Value::Gib(g))
        }
        Field::GpuClass => (gpu_class(required_v, &rq)?, gpu_class(found_v, &fd)?),
        Field::Display => (resolution(required_v, &rq)?, resolution(found_v, &fd)?),
    };
    Ok(Shortfall { field: field_v, required, found })
}

fn cpu_level(v: &CValue, ptr: &str) -> Result<Value, ReportError> {
    let s = string(v, ptr)?;
    if CPU_LEVELS.contains(&s) {
        Ok(Value::Name(s.to_string()))
    } else {
        Err(invalid(ptr, "unknown instruction-set level"))
    }
}

fn gpu_class(v: &CValue, ptr: &str) -> Result<Value, ReportError> {
    let s = string(v, ptr)?;
    match GpuClass::from_name(s) {
        Some(c) => Ok(Value::Name(c.as_str().to_string())),
        None => Err(invalid(ptr, "unknown GPU class")),
    }
}

fn resolution(v: &CValue, ptr: &str) -> Result<Value, ReportError> {
    let a = array(v, ptr)?;
    if a.len() != 2 {
        return Err(invalid(ptr, "must be [width, height]"));
    }
    Ok(Value::Resolution(u32_of(&a[0], &format!("{ptr}/0"))?, u32_of(&a[1], &format!("{ptr}/1"))?))
}
