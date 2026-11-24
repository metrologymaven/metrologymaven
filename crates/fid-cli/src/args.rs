//! Typed and unit-bearing CLI parameter parsing (spec node `AUTO.CLI.ARGS`).
//!
//! Quantities are kept verbatim as `{"value": <decimal_str>, "unit": <unit_str>}` without
//! parsing to `f64` or reformatting (REQ-AUTO.CLI.ARGS-001..003). `--params` reads bounded JSON
//! (1 MiB, depth 32) and merges explicit flags shallowly on top (REQ-AUTO.CLI.ARGS-004).

use crate::tree::{CliMajor, CliParam};
use fid_canon::{parse_with_limits, CValue};
use fid_surface::roots::is_handle;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read;

/// Maximum JSON size accepted by `--params` (1 MiB, REQ-AUTO.CLI.ARGS-004).
pub const MAX_PARAMS_BYTES: usize = 1024 * 1024;
/// Maximum JSON nesting depth accepted by `--params` (REQ-AUTO.CLI.ARGS-004).
pub const MAX_PARAMS_DEPTH: usize = 32;

/// Typed parameter representation produced by CLI argument parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Bool(bool),
    Int(String),
    Decimal(String),
    Quantity { value: String, unit: String },
    Str(String),
    Enum(String),
    Ref(String),
    Handle(String),
    RawPath(String),
    List(Vec<ParamValue>),
    Null,
    Json(CValue),
}

impl ParamValue {
    /// Converts this typed parameter into canonical bus JSON (`CValue`).
    pub fn to_cvalue(&self) -> CValue {
        match self {
            ParamValue::Bool(b) => CValue::Bool(*b),
            ParamValue::Int(s) => match s.parse::<i64>() {
                Ok(n) if n.unsigned_abs() <= (1u64 << 53) => CValue::Num(n as f64),
                Ok(n) => CValue::I64(n),
                Err(_) => CValue::Str(s.clone()),
            },
            ParamValue::Decimal(s) => match s.parse::<f64>() {
                Ok(f) if f.is_finite() => CValue::Num(f),
                _ => CValue::Str(s.clone()),
            },
            ParamValue::Quantity { value, unit } => {
                let mut m = BTreeMap::new();
                m.insert("unit".to_string(), CValue::Str(unit.clone()));
                m.insert("value".to_string(), CValue::Str(value.clone()));
                CValue::Obj(m)
            }
            ParamValue::Str(s)
            | ParamValue::Enum(s)
            | ParamValue::Ref(s)
            | ParamValue::Handle(s)
            | ParamValue::RawPath(s) => CValue::Str(s.clone()),
            ParamValue::List(items) => CValue::Arr(items.iter().map(ParamValue::to_cvalue).collect()),
            ParamValue::Null => CValue::Null,
            ParamValue::Json(v) => v.clone(),
        }
    }
}

/// Global and dispatch-level flags parsed from `rest`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DispatchFlags {
    pub json: bool,
    pub dry_run: bool,
    pub project: Option<String>,
    pub app: bool,
    pub branch: Option<String>,
    pub wait_timeout_secs: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub reason: Option<String>,
    pub yes: bool,
    pub confirm_human: bool,
    pub params_source: Option<String>,
    pub expected_head: Option<Digest>,
    pub runtime_dir: Option<String>,
    pub token_file: Option<String>,
}

/// Result of parsing `rest` against a [`CliMajor`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedArgs {
    pub flags: DispatchFlags,
    pub params: CValue,
    pub typed: BTreeMap<String, ParamValue>,
    pub raw_paths: Vec<(String, String)>,
}

/// Errors from `AUTO.CLI.ARGS` parsing; all map to `usage_error` (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    Usage { code: &'static str, message: String },
    ParamsTooLarge(String),
    ParamsParse(String),
}

impl ArgError {
    pub fn code(&self) -> &'static str {
        match self {
            ArgError::Usage { code, .. } => code,
            ArgError::ParamsTooLarge(_) => "params_too_large",
            ArgError::ParamsParse(_) => "params_parse",
        }
    }
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgError::Usage { message, .. } => f.write_str(message),
            ArgError::ParamsTooLarge(msg) => write!(f, "params_too_large: {msg}"),
            ArgError::ParamsParse(msg) => write!(f, "params_parse: {msg}"),
        }
    }
}

impl std::error::Error for ArgError {}

/// Source abstraction for reading `--params <handle|->`.
pub trait ParamsReader {
    fn read_params(&self, source: &str, stdin: &mut dyn Read) -> Result<Vec<u8>, ArgError>;
}

/// Standard `--params` reader: reads `"-"` from `stdin` or a local file path, bounded to
/// [`MAX_PARAMS_BYTES`].
#[derive(Debug, Default, Clone, Copy)]
pub struct StdParamsReader;

impl ParamsReader for StdParamsReader {
    fn read_params(&self, source: &str, stdin: &mut dyn Read) -> Result<Vec<u8>, ArgError> {
        let limit = (MAX_PARAMS_BYTES as u64).saturating_add(1);
        let mut buf = Vec::new();
        if source == "-" {
            stdin
                .take(limit)
                .read_to_end(&mut buf)
                .map_err(|e| ArgError::ParamsParse(format!("reading stdin `--params`: {e}")))?;
        } else {
            let file =
                File::open(source).map_err(|e| ArgError::ParamsParse(format!("opening `--params {source}`: {e}")))?;
            file.take(limit)
                .read_to_end(&mut buf)
                .map_err(|e| ArgError::ParamsParse(format!("reading `--params {source}`: {e}")))?;
        }
        if buf.len() > MAX_PARAMS_BYTES {
            return Err(ArgError::ParamsTooLarge(format!("`--params` input exceeds {MAX_PARAMS_BYTES} bytes")));
        }
        Ok(buf)
    }
}

fn usage(code: &'static str, msg: impl Into<String>) -> ArgError {
    ArgError::Usage { code, message: msg.into() }
}

/// Parses a decimal prefix `[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?` and returns its byte length.
fn scan_decimal_prefix(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0usize;
    if matches!(b.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let int_start = i;
    while matches!(b.get(i), Some(b'0'..=b'9')) {
        i += 1;
    }
    let has_int = i > int_start;
    let mut has_frac = false;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let frac_start = i;
        while matches!(b.get(i), Some(b'0'..=b'9')) {
            i += 1;
        }
        has_frac = i > frac_start;
    }
    if !has_int && !has_frac {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let save = i;
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        while matches!(b.get(i), Some(b'0'..=b'9')) {
            i += 1;
        }
        if i == exp_start {
            i = save;
        }
    }
    Some(i)
}

/// Parses a quantity literal (`<decimal>[ ]<unit>`) keeping numeric digits and unit symbol
/// verbatim (REQ-AUTO.CLI.ARGS-001..003).
pub fn parse_quantity(raw: &str, param_name: &str, accepted_unit: Option<&str>) -> Result<(String, String), ArgError> {
    let trimmed = raw.trim();
    let Some(num_end) = scan_decimal_prefix(trimmed) else {
        return Err(usage(
            "usage_error",
            format!("parameter `{param_name}` expects a numeric quantity literal with unit, got `{raw}`"),
        ));
    };
    let value_str = &trimmed[..num_end];
    let rest = trimmed[num_end..].trim_start();
    if rest.is_empty() {
        let units_hint = accepted_unit.unwrap_or("declared unit");
        return Err(usage(
            "usage_error",
            format!("quantity parameter `{param_name}` requires a unit symbol (accepted: {units_hint})"),
        ));
    }
    if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Err(usage("usage_error", format!("quantity parameter `{param_name}` has malformed unit `{rest}`")));
    }
    Ok((value_str.to_string(), rest.to_string()))
}

fn type_allows(ty: &str, candidate: &str) -> bool {
    ty == candidate || ty.split('|').any(|part| part == candidate)
}

fn parse_param_value(param: &CliParam, raw: &str, is_path_like: bool) -> Result<ParamValue, ArgError> {
    if type_allows(&param.ty, "null") && raw == "null" {
        return Ok(ParamValue::Null);
    }
    if param.quantity_kind.is_some() {
        let (value, unit) = parse_quantity(raw, &param.name, param.unit.as_deref())?;
        return Ok(ParamValue::Quantity { value, unit });
    }
    if type_allows(&param.ty, "boolean") && (param.ty == "boolean" || param.ty == "null|boolean") {
        return match raw {
            "true" => Ok(ParamValue::Bool(true)),
            "false" => Ok(ParamValue::Bool(false)),
            _ => Err(usage(
                "usage_error",
                format!("parameter `{}` expects boolean (`true` or `false`), got `{raw}`", param.name),
            )),
        };
    }
    if type_allows(&param.ty, "integer") && (param.ty == "integer" || param.ty == "null|integer") {
        if raw.parse::<i64>().is_ok() {
            return Ok(ParamValue::Int(raw.to_string()));
        }
        return Err(usage("usage_error", format!("parameter `{}` expects an integer, got `{raw}`", param.name)));
    }
    if type_allows(&param.ty, "number") && (param.ty == "number" || param.ty == "null|number") {
        if scan_decimal_prefix(raw.trim()) == Some(raw.trim().len())
            && raw.trim().parse::<f64>().is_ok_and(f64::is_finite)
        {
            return Ok(ParamValue::Decimal(raw.trim().to_string()));
        }
        return Err(usage("usage_error", format!("parameter `{}` expects a finite number, got `{raw}`", param.name)));
    }
    if type_allows(&param.ty, "array") && (param.ty == "array" || param.ty == "null|array") {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            let parsed = parse_with_limits(trimmed.as_bytes(), MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
                .map_err(|e| usage("usage_error", format!("parameter `{}` invalid JSON array: {e}", param.name)))?;
            if let CValue::Arr(items) = parsed {
                return Ok(ParamValue::List(items.into_iter().map(ParamValue::Json).collect()));
            }
        }
        let parts: Vec<ParamValue> =
            if raw.is_empty() { Vec::new() } else { raw.split(',').map(|s| ParamValue::Str(s.to_string())).collect() };
        return Ok(ParamValue::List(parts));
    }
    if (param.ty == "object" || param.ty == "any" || param.ty.starts_with('#'))
        && (raw.trim_start().starts_with('{') || raw.trim_start().starts_with('['))
    {
        let parsed = parse_with_limits(raw.as_bytes(), MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
            .map_err(|e| usage("usage_error", format!("parameter `{}` invalid JSON: {e}", param.name)))?;
        return Ok(ParamValue::Json(parsed));
    }
    if is_path_like {
        if is_handle(raw) {
            return Ok(ParamValue::Handle(raw.to_string()));
        }
        return Ok(ParamValue::RawPath(raw.to_string()));
    }
    Ok(ParamValue::Str(raw.to_string()))
}

fn flag_value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, ArgError> {
    if let Some(val) = args.get(*i + 1) {
        *i += 2;
        Ok(val.as_str())
    } else {
        Err(usage("usage_error", format!("`{flag}` requires a value")))
    }
}

fn parse_u64_flag(val: &str, flag: &str) -> Result<u64, ArgError> {
    val.parse::<u64>().map_err(|_| usage("usage_error", format!("`{flag}` requires an unsigned integer, got `{val}`")))
}

/// Parses `rest` against `major` metadata and optional `--params` JSON document.
pub fn parse_command_args(
    major: &CliMajor,
    rest: &[String],
    json_flag: bool,
    stdin: &mut dyn Read,
    path_like_params: Option<&BTreeSet<String>>,
    params_reader: &dyn ParamsReader,
) -> Result<ParsedArgs, ArgError> {
    let mut flags = DispatchFlags { json: json_flag, ..DispatchFlags::default() };
    let mut explicit: BTreeMap<String, ParamValue> = BTreeMap::new();

    let by_flag: BTreeMap<String, &CliParam> =
        major.params.iter().map(|p| (format!("--{}", p.name.replace('_', "-")), p)).collect();

    let mut i = 0usize;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--json" => {
                flags.json = true;
                i += 1;
            }
            "--dry-run" => {
                flags.dry_run = true;
                i += 1;
            }
            "--app" => {
                flags.app = true;
                i += 1;
            }
            "--yes" => {
                flags.yes = true;
                i += 1;
            }
            "--confirm-human" => {
                flags.confirm_human = true;
                i += 1;
            }
            "--project" => {
                flags.project = Some(flag_value(rest, &mut i, "--project")?.to_string());
            }
            s if s.starts_with("--project=") => {
                flags.project = Some(s.trim_start_matches("--project=").to_string());
                i += 1;
            }
            "--branch" => {
                flags.branch = Some(flag_value(rest, &mut i, "--branch")?.to_string());
            }
            s if s.starts_with("--branch=") => {
                flags.branch = Some(s.trim_start_matches("--branch=").to_string());
                i += 1;
            }
            "--wait-timeout" => {
                let v = flag_value(rest, &mut i, "--wait-timeout")?;
                flags.wait_timeout_secs = Some(parse_u64_flag(v, "--wait-timeout")?);
            }
            s if s.starts_with("--wait-timeout=") => {
                let v = s.trim_start_matches("--wait-timeout=");
                flags.wait_timeout_secs = Some(parse_u64_flag(v, "--wait-timeout")?);
                i += 1;
            }
            "--timeout" => {
                let v = flag_value(rest, &mut i, "--timeout")?;
                flags.timeout_ms = Some(parse_u64_flag(v, "--timeout")?);
            }
            s if s.starts_with("--timeout=") => {
                let v = s.trim_start_matches("--timeout=");
                flags.timeout_ms = Some(parse_u64_flag(v, "--timeout")?);
                i += 1;
            }
            "--reason" => {
                flags.reason = Some(flag_value(rest, &mut i, "--reason")?.to_string());
            }
            s if s.starts_with("--reason=") => {
                flags.reason = Some(s.trim_start_matches("--reason=").to_string());
                i += 1;
            }
            "--params" => {
                flags.params_source = Some(flag_value(rest, &mut i, "--params")?.to_string());
            }
            s if s.starts_with("--params=") => {
                flags.params_source = Some(s.trim_start_matches("--params=").to_string());
                i += 1;
            }
            "--expected-head" => {
                let v = flag_value(rest, &mut i, "--expected-head")?;
                let d = Digest::parse(v)
                    .map_err(|_| usage("usage_error", format!("invalid `--expected-head` digest `{v}`")))?;
                flags.expected_head = Some(d);
            }
            s if s.starts_with("--expected-head=") => {
                let v = s.trim_start_matches("--expected-head=");
                let d = Digest::parse(v)
                    .map_err(|_| usage("usage_error", format!("invalid `--expected-head` digest `{v}`")))?;
                flags.expected_head = Some(d);
                i += 1;
            }
            "--runtime-dir" => {
                flags.runtime_dir = Some(flag_value(rest, &mut i, "--runtime-dir")?.to_string());
            }
            s if s.starts_with("--runtime-dir=") => {
                flags.runtime_dir = Some(s.trim_start_matches("--runtime-dir=").to_string());
                i += 1;
            }
            "--token-file" => {
                flags.token_file = Some(flag_value(rest, &mut i, "--token-file")?.to_string());
            }
            s if s.starts_with("--token-file=") => {
                flags.token_file = Some(s.trim_start_matches("--token-file=").to_string());
                i += 1;
            }
            s if s.starts_with("--") => {
                let (flag_name, inline_val) = match s.split_once('=') {
                    Some((k, v)) => (k, Some(v)),
                    None => (s, None),
                };
                let Some(param) = by_flag.get(flag_name).copied() else {
                    return Err(usage("usage_error", format!("unknown option `{flag_name}`")));
                };
                let raw_val = match inline_val {
                    Some(v) => {
                        i += 1;
                        v.to_string()
                    }
                    None => {
                        if param.ty == "boolean" && rest.get(i + 1).is_none_or(|nxt| nxt.starts_with('-')) {
                            i += 1;
                            "true".to_string()
                        } else {
                            flag_value(rest, &mut i, flag_name)?.to_string()
                        }
                    }
                };
                let is_pl = path_like_params.is_some_and(|set| set.contains(&param.name));
                let parsed = parse_param_value(param, &raw_val, is_pl)?;
                explicit.insert(param.name.clone(), parsed);
            }
            other => {
                return Err(usage("usage_error", format!("unexpected argument `{other}`")));
            }
        }
    }

    let mut merged = BTreeMap::new();
    if let Some(src) = &flags.params_source {
        let bytes = params_reader.read_params(src, stdin)?;
        let val = parse_with_limits(&bytes, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
            .map_err(|e| ArgError::ParamsParse(format!("invalid JSON in `--params`: {e}")))?;
        let CValue::Obj(obj) = val else {
            return Err(ArgError::ParamsParse("`--params` JSON root must be an object".to_string()));
        };
        for (k, v) in obj {
            merged.insert(k, v);
        }
    }

    let mut typed = BTreeMap::new();
    for (k, v) in explicit {
        merged.insert(k.clone(), v.to_cvalue());
        typed.insert(k, v);
    }

    // Apply defaults / verify required params.
    for p in &major.params {
        if !merged.contains_key(&p.name) {
            if let Some(def) = &p.default {
                if p.quantity_kind.is_none() {
                    merged.insert(p.name.clone(), def.clone());
                } else if p.required {
                    return Err(usage(
                        "usage_error",
                        format!("missing required parameter `--{}`", p.name.replace('_', "-")),
                    ));
                }
            } else if p.required {
                return Err(usage(
                    "usage_error",
                    format!("missing required parameter `--{}`", p.name.replace('_', "-")),
                ));
            }
        }
    }

    let mut raw_paths = Vec::new();
    if let Some(pl_set) = path_like_params {
        for pname in pl_set {
            if let Some(CValue::Str(s)) = merged.get(pname) {
                if !is_handle(s) {
                    raw_paths.push((pname.clone(), s.clone()));
                }
            }
        }
    }

    Ok(ParsedArgs { flags, params: CValue::Obj(merged), typed, raw_paths })
}
