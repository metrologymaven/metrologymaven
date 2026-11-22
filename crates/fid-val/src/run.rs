//! Case-to-call mapping, versioned pass criteria, artifact execution, representation
//! normalisation and canonical `RunResults` aggregation (spec node `KRN.MAN.VAL.RUN`).
//!
//! - REQ-001: Maps every case in a frozen RDL release to a registered algorithm and runs it
//!   through the artifact runner.
//! - REQ-002: Reads pass criteria exclusively from versioned data files
//!   `criteria/<set>-<version>.toml`; fails with [`ValError::MissingCriteria`] if any set in
//!   the release lacks a criteria file. No criterion threshold appears in Rust source.
//! - REQ-003: Records per `(algorithm, primitive)` passed, failed, not-run counts (with typed
//!   reasons `no_mapping` or `kernel_error{code}`) and worst absolute deviation per parameter.
//! - REQ-004: Normalises representations (direction sign, axis point along axis, plane point
//!   along plane) before measuring deviations, driven by rules in the mapping data.
//! - REQ-005: Deterministic D0 canonical JSON encoding (`val-run-results/v1`), byte-identical
//!   across hosts via `fid-num::f` transcendentals and `fid-canon`.

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_kci::kci::Sha256;
use fid_rdl::RdlDataset;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Schema identifier of canonical [`RunResults`] output.
pub const RUN_RESULTS_SCHEMA: &str = "val-run-results/v1";
/// Maximum TOML mapping or criteria file size (1 MiB).
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
/// Maximum release manifest or record file size (16 MiB).
pub const MAX_RELEASE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum JSON nesting depth for release/output parsing.
pub const MAX_JSON_DEPTH: usize = 32;
/// Maximum stdout bytes accepted from one artifact call (16 MiB).
pub const MAX_ANSWER_BYTES: usize = 16 * 1024 * 1024;

// ------------------------------------------------------------------ errors

/// Typed errors of the validation harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValError {
    /// A reference set present in the release has no pass-criteria file (REQ-002, AC-3).
    MissingCriteria { set: String },
    /// Mapping file or criteria file is missing, unreadable or malformed.
    MappingInvalid(String),
    /// Release manifest or RDL record is unreadable or malformed.
    ReleaseInvalid(String),
    /// Artifact runner failed to spawn, timed out, wrote stderr, or returned non-protocol output.
    RunnerError(String),
    /// Internal canonical encoding error.
    Internal(&'static str),
}

impl ValError {
    pub const fn code(&self) -> &'static str {
        match self {
            ValError::MissingCriteria { .. } => "missing_criteria",
            ValError::MappingInvalid(_) => "mapping_invalid",
            ValError::ReleaseInvalid(_) => "release_invalid",
            ValError::RunnerError(_) => "runner_error",
            ValError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for ValError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValError::MissingCriteria { set } => write!(f, "MissingCriteria{{{set}}}"),
            ValError::MappingInvalid(s) => write!(f, "mapping_invalid: {s}"),
            ValError::ReleaseInvalid(s) => write!(f, "release_invalid: {s}"),
            ValError::RunnerError(s) => write!(f, "runner_error: {s}"),
            ValError::Internal(s) => write!(f, "internal: {s}"),
        }
    }
}

impl std::error::Error for ValError {}

fn read_bounded(path: &Path, limit: usize, err_fn: impl Fn(String) -> ValError) -> Result<Vec<u8>, ValError> {
    let f = File::open(path).map_err(|e| err_fn(format!("cannot open {}: {e}", path.display())))?;
    let mut buf = Vec::new();
    let mut limited = f.take((limit as u64).saturating_add(1));
    limited.read_to_end(&mut buf).map_err(|e| err_fn(format!("cannot read {}: {e}", path.display())))?;
    if buf.len() > limit {
        return Err(err_fn(format!("{} exceeds byte limit {limit}", path.display())));
    }
    Ok(buf)
}

fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

fn abs_f64(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}

// ------------------------------------------------------------------ mapping data

/// Normalisation & comparison rule declared per parameter in the versioned mapping data
/// (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareRule {
    /// Absolute scalar difference `|actual - reference|` (radius, form, etc.).
    Scalar { param: String },
    /// Direction vector comparison: angle in radians via KNP-1 `acos(clamp(dot(u, v)))`.
    /// When `sign_invariant` is true, uses `|dot(u, v)|` so `n` and `-n` have 0 deviation.
    Direction { param: String, sign_invariant: bool },
    /// 3D point Euclidean distance `||p - p_ref||`.
    Point3D { param: String },
    /// Perpendicular distance from actual point to reference line `(point_param, dir_param)`:
    /// invariant to sliding the point along the axis and to flipping `dir`.
    AxisPoint { param: String, dir_param: String },
    /// Normal distance from actual plane point to reference plane `(point_param, normal_param)`:
    /// `|(p - p_ref) · n_ref|`, invariant to sliding in the plane and to flipping `n`.
    PlaneOffset { param: String, normal_param: String },
}

impl CompareRule {
    pub fn param_name(&self) -> &str {
        match self {
            CompareRule::Scalar { param }
            | CompareRule::Direction { param, .. }
            | CompareRule::Point3D { param }
            | CompareRule::AxisPoint { param, .. }
            | CompareRule::PlaneOffset { param, .. } => param,
        }
    }
}

/// One row of the versioned case-to-call mapping table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub set: String,
    pub primitive: String,
    pub case_type: String,
    pub algorithm: String,
    pub params_template: BTreeMap<String, String>,
    pub compare_rules: Vec<CompareRule>,
}

/// Versioned table mapping `(set, primitive, case_type)` to `(algorithm, params_template, compare_rules)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingTable {
    pub version: String,
    entries: BTreeMap<(String, String, String), Mapping>,
}

impl MappingTable {
    pub fn lookup(&self, set: &str, primitive: &str, case_type: &str) -> Option<&Mapping> {
        self.entries.get(&(set.to_string(), primitive.to_string(), case_type.to_string()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn toml_str<'a>(t: &'a toml::value::Table, key: &str, ctx: &str) -> Result<&'a str, ValError> {
    t.get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| ValError::MappingInvalid(format!("{ctx}: missing or non-string '{key}'")))
}

/// Loads a versioned mapping table TOML file (`mappings.toml`).
pub fn load_mapping_table(path: &Path) -> Result<MappingTable, ValError> {
    let raw = read_bounded(path, MAX_CONFIG_BYTES, ValError::MappingInvalid)?;
    let text = core::str::from_utf8(&raw).map_err(|_| ValError::MappingInvalid("mapping file is not UTF-8".into()))?;
    parse_mapping_table(text)
}

/// Parses a versioned mapping table from TOML text.
pub fn parse_mapping_table(toml_text: &str) -> Result<MappingTable, ValError> {
    let root: toml::Value =
        toml::from_str(toml_text).map_err(|e| ValError::MappingInvalid(format!("invalid TOML: {e}")))?;
    let table = root.as_table().ok_or_else(|| ValError::MappingInvalid("root must be a TOML table".into()))?;
    let version = toml_str(table, "version", "mappings")?.to_string();
    let arr = table
        .get("mapping")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| ValError::MappingInvalid("missing [[mapping]] array".into()))?;
    let mut entries = BTreeMap::new();
    for (idx, item) in arr.iter().enumerate() {
        let ctx = format!("mapping[{idx}]");
        let mt = item.as_table().ok_or_else(|| ValError::MappingInvalid(format!("{ctx}: expected table")))?;
        let set = toml_str(mt, "set", &ctx)?.to_string();
        let primitive = toml_str(mt, "primitive", &ctx)?.to_string();
        let case_type = toml_str(mt, "case_type", &ctx)?.to_string();
        let algorithm = toml_str(mt, "algorithm", &ctx)?.to_string();

        let mut params_template = BTreeMap::new();
        if let Some(pt) = mt.get("params_template") {
            let pt_tbl = pt
                .as_table()
                .ok_or_else(|| ValError::MappingInvalid(format!("{ctx}.params_template: expected table")))?;
            for (k, v) in pt_tbl {
                let s = v
                    .as_str()
                    .ok_or_else(|| ValError::MappingInvalid(format!("{ctx}.params_template.{k}: expected string")))?;
                params_template.insert(k.clone(), s.to_string());
            }
        }

        let rules_arr = mt
            .get("compare_rules")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| ValError::MappingInvalid(format!("{ctx}: missing compare_rules array")))?;
        let mut compare_rules = Vec::with_capacity(rules_arr.len());
        let mut seen_params = BTreeSet::new();
        for (ri, ritem) in rules_arr.iter().enumerate() {
            let rctx = format!("{ctx}.compare_rules[{ri}]");
            let rt = ritem.as_table().ok_or_else(|| ValError::MappingInvalid(format!("{rctx}: expected table")))?;
            let param = toml_str(rt, "param", &rctx)?.to_string();
            if !seen_params.insert(param.clone()) {
                return Err(ValError::MappingInvalid(format!("{rctx}: duplicate compare rule for '{param}'")));
            }
            let kind = toml_str(rt, "kind", &rctx)?;
            let rule = match kind {
                "scalar" => CompareRule::Scalar { param },
                "direction" => {
                    let sign_invariant = rt.get("sign_invariant").and_then(toml::Value::as_bool).unwrap_or(true);
                    CompareRule::Direction { param, sign_invariant }
                }
                "point3d" => CompareRule::Point3D { param },
                "axis_point" => {
                    let dir_param = toml_str(rt, "dir_param", &rctx)?.to_string();
                    CompareRule::AxisPoint { param, dir_param }
                }
                "plane_offset" => {
                    let normal_param = toml_str(rt, "normal_param", &rctx)?.to_string();
                    CompareRule::PlaneOffset { param, normal_param }
                }
                other => return Err(ValError::MappingInvalid(format!("{rctx}: unknown rule kind '{other}'"))),
            };
            compare_rules.push(rule);
        }

        let key = (set.clone(), primitive.clone(), case_type.clone());
        if entries.contains_key(&key) {
            return Err(ValError::MappingInvalid(format!("{ctx}: duplicate mapping for {key:?}")));
        }
        entries.insert(key, Mapping { set, primitive, case_type, algorithm, params_template, compare_rules });
    }
    Ok(MappingTable { version, entries })
}

// ------------------------------------------------------------------ criteria data (REQ-002)

/// Pass criteria for one `(set, version)` loaded from `criteria/<set>-<version>.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Criteria {
    pub set: String,
    pub version: String,
    pub per_parameter_limits: BTreeMap<String, f64>,
}

/// Collection of loaded criteria files keyed by `(set, version)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CriteriaSet {
    by_set_version: BTreeMap<(String, String), Criteria>,
}

impl CriteriaSet {
    pub fn get(&self, set: &str, version: &str) -> Option<&Criteria> {
        self.by_set_version.get(&(set.to_string(), version.to_string()))
    }

    pub fn insert(&mut self, c: Criteria) -> Result<(), ValError> {
        let key = (c.set.clone(), c.version.clone());
        if self.by_set_version.contains_key(&key) {
            return Err(ValError::MappingInvalid(format!("duplicate criteria for {key:?}")));
        }
        self.by_set_version.insert(key, c);
        Ok(())
    }
}

/// Parses one criteria TOML file (`criteria/<set>-<version>.toml`).
pub fn parse_criteria_toml(toml_text: &str) -> Result<Criteria, ValError> {
    let root: toml::Value =
        toml::from_str(toml_text).map_err(|e| ValError::MappingInvalid(format!("invalid criteria TOML: {e}")))?;
    let tbl = root.as_table().ok_or_else(|| ValError::MappingInvalid("criteria root must be a table".into()))?;
    let set = toml_str(tbl, "set", "criteria")?.to_string();
    let version = toml_str(tbl, "version", "criteria")?.to_string();
    let limits_tbl = tbl
        .get("limits")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| ValError::MappingInvalid("criteria missing [limits] table".into()))?;
    if limits_tbl.is_empty() {
        return Err(ValError::MappingInvalid("criteria [limits] table must not be empty".into()));
    }
    let mut per_parameter_limits = BTreeMap::new();
    for (param, val) in limits_tbl {
        let limit = match val {
            toml::Value::Float(f) => *f,
            toml::Value::Integer(i) => *i as f64,
            _ => return Err(ValError::MappingInvalid(format!("criteria limit '{param}' must be numeric"))),
        };
        if !limit.is_finite() || limit < 0.0 {
            return Err(ValError::MappingInvalid(format!("criteria limit '{param}' must be finite and >= 0")));
        }
        per_parameter_limits.insert(param.clone(), plus_zero(limit));
    }
    Ok(Criteria { set, version, per_parameter_limits })
}

/// Loads all `*.toml` criteria files from `criteria_dir`, verifying each filename matches
/// `<set>-<version>.toml`.
pub fn load_criteria_dir(criteria_dir: &Path) -> Result<CriteriaSet, ValError> {
    let rd = std::fs::read_dir(criteria_dir)
        .map_err(|e| ValError::MappingInvalid(format!("cannot read criteria dir {}: {e}", criteria_dir.display())))?;
    let mut paths = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| ValError::MappingInvalid(format!("dir entry error: {e}")))?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("toml") {
            paths.push(p);
        }
    }
    paths.sort();
    let mut set = CriteriaSet::default();
    for p in paths {
        let raw = read_bounded(&p, MAX_CONFIG_BYTES, ValError::MappingInvalid)?;
        let txt = core::str::from_utf8(&raw)
            .map_err(|_| ValError::MappingInvalid(format!("{} is not UTF-8", p.display())))?;
        let c = parse_criteria_toml(txt)?;
        let expected_name = format!("{}-{}.toml", c.set, c.version);
        let actual_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if actual_name != expected_name {
            return Err(ValError::MappingInvalid(format!(
                "criteria file {actual_name:?} does not match expected {expected_name:?}"
            )));
        }
        set.insert(c)?;
    }
    Ok(set)
}

// ------------------------------------------------------------------ release & reference values

/// A numeric parameter value (scalar or 3-vector) in reference or artifact output.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Scalar(f64),
    Vec3([f64; 3]),
}

impl ParamValue {
    fn from_cvalue(v: &CValue, ctx: &str) -> Result<ParamValue, ValError> {
        match v {
            CValue::Num(x) if x.is_finite() => Ok(ParamValue::Scalar(plus_zero(*x))),
            CValue::Str(s) => Ok(ParamValue::Scalar(parse_hex_or_dec_f64(s, ctx)?)),
            CValue::Arr(arr) if arr.len() == 3 => {
                let mut out = [0.0; 3];
                for (i, item) in arr.iter().enumerate() {
                    let x = match item {
                        CValue::Num(n) if n.is_finite() => plus_zero(*n),
                        CValue::Str(s) => parse_hex_or_dec_f64(s, ctx)?,
                        _ => return Err(ValError::ReleaseInvalid(format!("{ctx}[{i}]: expected finite number"))),
                    };
                    out[i] = x;
                }
                Ok(ParamValue::Vec3(out))
            }
            _ => Err(ValError::ReleaseInvalid(format!("{ctx}: expected scalar or 3-vector"))),
        }
    }
}

fn parse_hex_or_dec_f64(s: &str, ctx: &str) -> Result<f64, ValError> {
    if let Some(hex) = s.strip_prefix("0x") {
        if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(ValError::ReleaseInvalid(format!("{ctx}: malformed hex f64 {s:?}")));
        }
        let bits =
            u64::from_str_radix(hex, 16).map_err(|_| ValError::ReleaseInvalid(format!("{ctx}: invalid hex f64")))?;
        let f = f64::from_bits(bits);
        if !f.is_finite() {
            return Err(ValError::ReleaseInvalid(format!("{ctx}: non-finite f64")));
        }
        return Ok(plus_zero(f));
    }
    let f: f64 = s.parse().map_err(|_| ValError::ReleaseInvalid(format!("{ctx}: invalid float {s:?}")))?;
    if !f.is_finite() {
        return Err(ValError::ReleaseInvalid(format!("{ctx}: non-finite float")));
    }
    Ok(plus_zero(f))
}

/// One reference parameter with optional expanded uncertainty `U_d` (defaults to `0.0`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceParam {
    pub value: ParamValue,
    pub uncertainty: f64,
}

/// One reference test case in a frozen RDL release.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseCase {
    pub case_id: String,
    pub set: String,
    pub set_version: String,
    pub primitive: String,
    pub case_type: String,
    /// Input points (`[[x, y, z], ...]`) passed to the kernel artifact.
    pub points: Vec<[f64; 3]>,
    /// Reference parameter map (name -> value + uncertainty).
    pub reference: BTreeMap<String, ReferenceParam>,
}

/// A frozen RDL release loaded from `release.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub release_id: String,
    pub suite_digest: Digest,
    pub cases: Vec<ReleaseCase>,
}

/// Loads a frozen release manifest from `path` (bounded `<= 16 MiB`).
/// Also validates any referenced `rdl_record` file via `fid_rdl::RdlDataset::from_json` when present.
pub fn load_release(path: &Path) -> Result<Release, ValError> {
    let raw = read_bounded(path, MAX_RELEASE_BYTES, ValError::ReleaseInvalid)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    parse_release_json(&raw, Some(base_dir))
}

/// Parses a release manifest from canonical JSON bytes.
pub fn parse_release_json(bytes: &[u8], base_dir: Option<&Path>) -> Result<Release, ValError> {
    let val = parse_with_limits(bytes, MAX_RELEASE_BYTES, MAX_JSON_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => ValError::ReleaseInvalid("release exceeds size/depth limit".into()),
        other => ValError::ReleaseInvalid(format!("invalid release JSON: {other}")),
    })?;
    let CValue::Obj(o) = val else {
        return Err(ValError::ReleaseInvalid("release root must be a JSON object".into()));
    };
    let release_id = match o.get("release_id") {
        Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
        _ => return Err(ValError::ReleaseInvalid("missing or empty release_id".into())),
    };
    let suite_digest = match o.get("suite_digest") {
        Some(CValue::Str(s)) => {
            Digest::parse(s).map_err(|_| ValError::ReleaseInvalid("invalid suite_digest".into()))?
        }
        _ => {
            let mut h = Sha256::new();
            h.update(bytes);
            Digest::from_bytes(h.finish())
        }
    };
    let Some(CValue::Arr(case_vals)) = o.get("cases") else {
        return Err(ValError::ReleaseInvalid("missing cases array".into()));
    };

    let mut cases = Vec::with_capacity(case_vals.len());
    for (idx, cv) in case_vals.iter().enumerate() {
        let ctx = format!("cases[{idx}]");
        let CValue::Obj(co) = cv else {
            return Err(ValError::ReleaseInvalid(format!("{ctx}: expected object")));
        };
        let get_s = |k: &str| -> Result<String, ValError> {
            match co.get(k) {
                Some(CValue::Str(s)) if !s.is_empty() => Ok(s.clone()),
                _ => Err(ValError::ReleaseInvalid(format!("{ctx}: missing or empty '{k}'"))),
            }
        };
        let case_id = get_s("case_id")?;
        let set = get_s("set")?;
        let set_version = get_s("set_version")?;
        let primitive = get_s("primitive")?;
        let case_type = get_s("case_type")?;

        // If the case points to an `rdl-dataset/v1` record file, validate it through fid-rdl.
        if let (Some(CValue::Str(rel_rec)), Some(dir)) = (co.get("rdl_record"), base_dir) {
            let rec_bytes = read_bounded(&dir.join(rel_rec), fid_rdl::MAX_RECORD_BYTES, ValError::ReleaseInvalid)?;
            let _ds = RdlDataset::from_json(&rec_bytes)
                .map_err(|e| ValError::ReleaseInvalid(format!("{ctx}.rdl_record: {e}")))?;
        }

        let Some(CValue::Arr(pts_arr)) = co.get("points") else {
            return Err(ValError::ReleaseInvalid(format!("{ctx}: missing points array")));
        };
        let mut points = Vec::with_capacity(pts_arr.len());
        for (pi, pv) in pts_arr.iter().enumerate() {
            let ParamValue::Vec3(p) = ParamValue::from_cvalue(pv, &format!("{ctx}.points[{pi}]"))? else {
                return Err(ValError::ReleaseInvalid(format!("{ctx}.points[{pi}]: expected [x, y, z]")));
            };
            points.push(p);
        }

        let Some(CValue::Obj(ref_obj)) = co.get("reference") else {
            return Err(ValError::ReleaseInvalid(format!("{ctx}: missing reference object")));
        };
        let mut reference = BTreeMap::new();
        for (pname, rval) in ref_obj {
            let pctx = format!("{ctx}.reference.{pname}");
            let rp = match rval {
                CValue::Obj(ro) => {
                    let v =
                        ro.get("value").ok_or_else(|| ValError::ReleaseInvalid(format!("{pctx}: missing 'value'")))?;
                    let value = ParamValue::from_cvalue(v, &pctx)?;
                    let uncertainty = match ro.get("uncertainty") {
                        None => 0.0,
                        Some(CValue::Num(u)) if u.is_finite() && *u >= 0.0 => plus_zero(*u),
                        Some(CValue::Str(s)) => {
                            let u = parse_hex_or_dec_f64(s, &pctx)?;
                            if u < 0.0 {
                                return Err(ValError::ReleaseInvalid(format!("{pctx}.uncertainty: negative")));
                            }
                            u
                        }
                        _ => return Err(ValError::ReleaseInvalid(format!("{pctx}.uncertainty: invalid"))),
                    };
                    ReferenceParam { value, uncertainty }
                }
                other => ReferenceParam { value: ParamValue::from_cvalue(other, &pctx)?, uncertainty: 0.0 },
            };
            reference.insert(pname.clone(), rp);
        }

        cases.push(ReleaseCase { case_id, set, set_version, primitive, case_type, points, reference });
    }
    Ok(Release { release_id, suite_digest, cases })
}

// ------------------------------------------------------------------ artifact runner (KRN.MAN.XART protocol)

/// Raw outcome of running one canonical kernel call through an artifact process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactAnswer {
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Executes one canonical kernel call against the WASM reference artifact (or native/toy build).
pub trait ArtifactRunner {
    fn invoke(&mut self, call_json: &[u8]) -> Result<ArtifactAnswer, ValError>;
}

/// Subprocess artifact runner speaking the `fid-kernel-wasm` / `fid-xart` stdin/stdout protocol
/// with bounded stdout and a per-call wall-clock deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArtifactRunner {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
}

fn spawn_pipe_reader<R: Read + Send + 'static>(pipe: R) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        pipe.take((MAX_ANSWER_BYTES as u64).saturating_add(1)).read_to_end(&mut buf).map(|_| buf)
    })
}

impl ArtifactRunner for CommandArtifactRunner {
    fn invoke(&mut self, call_json: &[u8]) -> Result<ArtifactAnswer, ValError> {
        let deadline = Instant::now() + self.timeout;
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ValError::RunnerError(format!("cannot start {}: {e}", self.program)))?;
        let mut stdin = child.stdin.take().ok_or_else(|| ValError::RunnerError("no stdin".into()))?;
        let payload = call_json.to_vec();
        let feeder = std::thread::spawn(move || {
            let _ = stdin.write_all(&payload);
        });
        let out_r = spawn_pipe_reader(child.stdout.take().ok_or_else(|| ValError::RunnerError("no stdout".into()))?);
        let err_r = spawn_pipe_reader(child.stderr.take().ok_or_else(|| ValError::RunnerError("no stderr".into()))?);

        let mut pause = Duration::from_millis(1);
        let status = loop {
            if let Some(st) = child.try_wait().map_err(|e| ValError::RunnerError(format!("wait: {e}")))? {
                break st;
            }
            let now = Instant::now();
            if now >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ValError::RunnerError(format!("artifact timed out after {} ms", self.timeout.as_millis())));
            }
            std::thread::sleep(pause.min(deadline - now));
            pause = (pause * 2).min(Duration::from_millis(20));
        };
        if feeder.join().is_err() {
            return Err(ValError::RunnerError("stdin feeder thread failed".into()));
        }
        let stdout = match out_r.join() {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => return Err(ValError::RunnerError(format!("stdout read: {e}"))),
            Err(_) => return Err(ValError::RunnerError("stdout thread failed".into())),
        };
        let stderr = match err_r.join() {
            Ok(Ok(b)) => b,
            _ => b"<stderr unreadable>".to_vec(),
        };
        if stdout.len() > MAX_ANSWER_BYTES {
            return Err(ValError::RunnerError("artifact stdout exceeded limit".into()));
        }
        Ok(ArtifactAnswer { exit: status.code(), stdout, stderr })
    }
}

fn build_kernel_call(mapping: &Mapping, case: &ReleaseCase) -> Result<Vec<u8>, ValError> {
    let pts = CValue::Arr(
        case.points
            .iter()
            .map(|p| CValue::Arr(vec![CValue::F64Bits(p[0]), CValue::F64Bits(p[1]), CValue::F64Bits(p[2])]))
            .collect(),
    );
    let mut params = BTreeMap::new();
    for (k, v) in &mapping.params_template {
        params.insert(k.clone(), CValue::Str(v.clone()));
    }
    let mut call = BTreeMap::new();
    call.insert("algorithm".into(), CValue::Str(mapping.algorithm.clone()));
    call.insert("case_id".into(), CValue::Str(case.case_id.clone()));
    call.insert("params".into(), CValue::Obj(params));
    call.insert("points".into(), pts);
    to_canonical_bytes(&CValue::Obj(call)).map_err(|_| ValError::Internal("canonical call json"))
}

enum DecodedArtifact {
    Output(BTreeMap<String, ParamValue>),
    KernelError(String),
}

fn decode_artifact_answer(ans: &ArtifactAnswer, expected_algo: &str) -> Result<DecodedArtifact, ValError> {
    if !ans.stderr.is_empty() {
        let msg = String::from_utf8_lossy(&ans.stderr);
        return Err(ValError::RunnerError(format!("artifact wrote to stderr: {msg}")));
    }
    match ans.exit {
        Some(1) => {
            let v = parse_with_limits(&ans.stdout, MAX_ANSWER_BYTES, MAX_JSON_DEPTH)
                .map_err(|e| ValError::RunnerError(format!("malformed error envelope: {e}")))?;
            let CValue::Obj(o) = v else {
                return Err(ValError::RunnerError("error envelope is not an object".into()));
            };
            if o.len() != 1 {
                return Err(ValError::RunnerError("error envelope must have only 'error' key".into()));
            }
            match o.get("error") {
                Some(CValue::Str(code)) if !code.is_empty() => Ok(DecodedArtifact::KernelError(code.clone())),
                _ => Err(ValError::RunnerError("error envelope missing string 'error'".into())),
            }
        }
        Some(0) => {
            let v = parse_with_limits(&ans.stdout, MAX_ANSWER_BYTES, MAX_JSON_DEPTH)
                .map_err(|e| ValError::RunnerError(format!("malformed result envelope: {e}")))?;
            let CValue::Obj(o) = v else {
                return Err(ValError::RunnerError("result envelope is not an object".into()));
            };
            match o.get("algorithm") {
                Some(CValue::Str(a)) if a == expected_algo => {}
                other => {
                    return Err(ValError::RunnerError(format!(
                        "result algorithm mismatch: expected {expected_algo:?}, got {other:?}"
                    )))
                }
            }
            let output_val = o.get("output").ok_or_else(|| ValError::RunnerError("missing output".into()))?;
            let output_bytes = to_canonical_bytes(output_val).map_err(|_| ValError::Internal("canonical output"))?;
            let mut h = Sha256::new();
            h.update(&output_bytes);
            let expected_digest = Digest::from_bytes(h.finish());
            match o.get("output_digest") {
                Some(CValue::Str(d)) if d == expected_digest.as_str() => {}
                other => {
                    return Err(ValError::RunnerError(format!(
                        "output_digest mismatch: expected {expected_digest}, got {other:?}"
                    )))
                }
            }
            let CValue::Obj(out_map) = output_val else {
                return Err(ValError::RunnerError("output must be a JSON object".into()));
            };
            let mut params = BTreeMap::new();
            for (k, pv) in out_map {
                let parsed = ParamValue::from_cvalue(pv, &format!("output.{k}"))
                    .map_err(|e| ValError::RunnerError(e.to_string()))?;
                params.insert(k.clone(), parsed);
            }
            Ok(DecodedArtifact::Output(params))
        }
        other => Err(ValError::RunnerError(format!("unexpected artifact exit status: {other:?}"))),
    }
}

// ------------------------------------------------------------------ deterministic normalisation & comparison (REQ-004)

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm3(v: [f64; 3]) -> Result<f64, ValError> {
    fid_num::f::sqrt(dot3(v, v)).map_err(|_| ValError::RunnerError("non-finite vector norm".into()))
}

fn unit3(v: [f64; 3], ctx: &str) -> Result<[f64; 3], ValError> {
    let n = norm3(v)?;
    if !n.is_finite() || n == 0.0 {
        return Err(ValError::RunnerError(format!("{ctx}: zero-length direction vector")));
    }
    Ok([v[0] / n, v[1] / n, v[2] / n])
}

fn compute_deviation(
    rule: &CompareRule,
    actual: &BTreeMap<String, ParamValue>,
    reference: &BTreeMap<String, ReferenceParam>,
) -> Result<(f64, f64), ValError> {
    let pname = rule.param_name();
    let ref_p = reference
        .get(pname)
        .ok_or_else(|| ValError::ReleaseInvalid(format!("missing reference parameter '{pname}'")))?;
    let act_p = actual
        .get(pname)
        .ok_or_else(|| ValError::RunnerError(format!("artifact output missing parameter '{pname}'")))?;

    let dev = match rule {
        CompareRule::Scalar { .. } => match (act_p, &ref_p.value) {
            (ParamValue::Scalar(a), ParamValue::Scalar(r)) => abs_f64(a - r),
            _ => return Err(ValError::RunnerError(format!("parameter '{pname}' type mismatch (expected scalar)"))),
        },
        CompareRule::Direction { sign_invariant, .. } => match (act_p, &ref_p.value) {
            (ParamValue::Vec3(a), ParamValue::Vec3(r)) => {
                let ua = unit3(*a, pname)?;
                let ur = unit3(*r, pname)?;
                let mut d = dot3(ua, ur);
                if *sign_invariant {
                    d = abs_f64(d);
                }
                d = d.clamp(-1.0, 1.0);
                fid_num::f::acos(d).map_err(|_| ValError::RunnerError(format!("acos failed on '{pname}'")))?
            }
            _ => return Err(ValError::RunnerError(format!("parameter '{pname}' type mismatch (expected vec3)"))),
        },
        CompareRule::Point3D { .. } => match (act_p, &ref_p.value) {
            (ParamValue::Vec3(a), ParamValue::Vec3(r)) => norm3(sub3(*a, *r))?,
            _ => return Err(ValError::RunnerError(format!("parameter '{pname}' type mismatch (expected vec3)"))),
        },
        CompareRule::AxisPoint { dir_param, .. } => {
            let ref_dir_p = reference
                .get(dir_param)
                .ok_or_else(|| ValError::ReleaseInvalid(format!("missing reference dir_param '{dir_param}'")))?;
            match (act_p, &ref_p.value, &ref_dir_p.value) {
                (ParamValue::Vec3(a), ParamValue::Vec3(r), ParamValue::Vec3(d)) => {
                    let ud = unit3(*d, dir_param)?;
                    let delta = sub3(*a, *r);
                    let along = dot3(delta, ud);
                    let perp = [delta[0] - along * ud[0], delta[1] - along * ud[1], delta[2] - along * ud[2]];
                    norm3(perp)?
                }
                _ => {
                    return Err(ValError::RunnerError(format!(
                        "parameter '{pname}' / '{dir_param}' type mismatch (expected vec3)"
                    )))
                }
            }
        }
        CompareRule::PlaneOffset { normal_param, .. } => {
            let ref_n_p = reference
                .get(normal_param)
                .ok_or_else(|| ValError::ReleaseInvalid(format!("missing reference normal_param '{normal_param}'")))?;
            match (act_p, &ref_p.value, &ref_n_p.value) {
                (ParamValue::Vec3(a), ParamValue::Vec3(r), ParamValue::Vec3(n)) => {
                    let un = unit3(*n, normal_param)?;
                    let delta = sub3(*a, *r);
                    abs_f64(dot3(delta, un))
                }
                _ => {
                    return Err(ValError::RunnerError(format!(
                        "parameter '{pname}' / '{normal_param}' type mismatch (expected vec3)"
                    )))
                }
            }
        }
    };
    Ok((plus_zero(dev), plus_zero(ref_p.uncertainty)))
}

// ------------------------------------------------------------------ RunResults

/// Why a case was not run (REQ-003).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotRunReason {
    NoMapping,
    KernelError { code: String },
}

impl NotRunReason {
    pub fn as_tag(&self) -> String {
        match self {
            NotRunReason::NoMapping => "no_mapping".into(),
            NotRunReason::KernelError { code } => format!("kernel_error{{{code}}}"),
        }
    }
}

/// Outcome of one case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CaseOutcome {
    Passed,
    Failed,
    NotRun,
}

impl CaseOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            CaseOutcome::Passed => "passed",
            CaseOutcome::Failed => "failed",
            CaseOutcome::NotRun => "not_run",
        }
    }
}

/// Per-case result row, sorted by `(algorithm, set, case_id)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    pub case_id: String,
    pub set: String,
    pub set_version: String,
    pub primitive: String,
    pub case_type: String,
    pub algorithm: String,
    pub outcome: CaseOutcome,
    pub reason: Option<NotRunReason>,
    pub deviations: BTreeMap<String, f64>,
    pub uncertainties: BTreeMap<String, f64>,
}

/// Aggregated counts and worst absolute deviation per `(algorithm, primitive)` (REQ-003).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSummary {
    pub algorithm: String,
    pub primitive: String,
    pub passed: u64,
    pub failed: u64,
    pub not_run: u64,
    pub not_run_reasons: BTreeMap<String, u64>,
    pub worst_deviation: BTreeMap<String, f64>,
}

/// Full deterministic validation run results (`val-run-results/v1`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunResults {
    pub release_id: String,
    pub suite_digest: Digest,
    pub mapping_version: String,
    pub cases: Vec<CaseResult>,
    pub summaries: Vec<GroupSummary>,
}

impl RunResults {
    pub fn to_value(&self) -> CValue {
        let mut case_arr = Vec::with_capacity(self.cases.len());
        for c in &self.cases {
            let mut devs = BTreeMap::new();
            for (k, v) in &c.deviations {
                devs.insert(k.clone(), CValue::F64Bits(plus_zero(*v)));
            }
            let mut uncs = BTreeMap::new();
            for (k, v) in &c.uncertainties {
                uncs.insert(k.clone(), CValue::F64Bits(plus_zero(*v)));
            }
            let mut co = BTreeMap::new();
            co.insert("algorithm".into(), CValue::Str(c.algorithm.clone()));
            co.insert("case_id".into(), CValue::Str(c.case_id.clone()));
            co.insert("case_type".into(), CValue::Str(c.case_type.clone()));
            co.insert("deviations".into(), CValue::Obj(devs));
            co.insert("outcome".into(), CValue::Str(c.outcome.as_str().into()));
            co.insert("primitive".into(), CValue::Str(c.primitive.clone()));
            co.insert("reason".into(), c.reason.as_ref().map_or(CValue::Null, |r| CValue::Str(r.as_tag())));
            co.insert("set".into(), CValue::Str(c.set.clone()));
            co.insert("set_version".into(), CValue::Str(c.set_version.clone()));
            co.insert("uncertainties".into(), CValue::Obj(uncs));
            case_arr.push(CValue::Obj(co));
        }

        let mut sum_arr = Vec::with_capacity(self.summaries.len());
        for s in &self.summaries {
            let mut worst = BTreeMap::new();
            for (k, v) in &s.worst_deviation {
                worst.insert(k.clone(), CValue::F64Bits(plus_zero(*v)));
            }
            let mut reasons = BTreeMap::new();
            for (k, n) in &s.not_run_reasons {
                reasons.insert(k.clone(), CValue::U64(*n));
            }
            let mut so = BTreeMap::new();
            so.insert("algorithm".into(), CValue::Str(s.algorithm.clone()));
            so.insert("failed".into(), CValue::U64(s.failed));
            so.insert("not_run".into(), CValue::U64(s.not_run));
            so.insert("not_run_reasons".into(), CValue::Obj(reasons));
            so.insert("passed".into(), CValue::U64(s.passed));
            so.insert("primitive".into(), CValue::Str(s.primitive.clone()));
            so.insert("worst_deviation".into(), CValue::Obj(worst));
            sum_arr.push(CValue::Obj(so));
        }

        let mut root = BTreeMap::new();
        root.insert("cases".into(), CValue::Arr(case_arr));
        root.insert("mapping_version".into(), CValue::Str(self.mapping_version.clone()));
        root.insert("release_id".into(), CValue::Str(self.release_id.clone()));
        root.insert("schema".into(), CValue::Str(RUN_RESULTS_SCHEMA.into()));
        root.insert("suite_digest".into(), CValue::Str(self.suite_digest.as_str().into()));
        root.insert("summaries".into(), CValue::Arr(sum_arr));
        CValue::Obj(root)
    }

    /// Canonical RFC 8785 JSON bytes (D0, byte-identical across hosts, REQ-005).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, ValError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| ValError::Internal("canonical json"))
    }

    /// SHA-256 digest of [`Self::to_canonical_json`].
    pub fn digest(&self) -> Result<Digest, ValError> {
        let bytes = self.to_canonical_json()?;
        let mut h = Sha256::new();
        h.update(&bytes);
        Ok(Digest::from_bytes(h.finish()))
    }
}

/// Runs the validation harness over `release` using `mappings`, `criteria`, and `runner`.
///
/// Fails closed BEFORE running any case if any `(set, set_version)` referenced in `release`
/// lacks a criteria file (`ValError::MissingCriteria`, REQ-002, AC-3).
pub fn run_validation(
    release: &Release,
    mappings: &MappingTable,
    criteria: &CriteriaSet,
    runner: &mut dyn ArtifactRunner,
) -> Result<RunResults, ValError> {
    // 1. REQ-002: verify every set present in the release has a loaded criteria file first.
    let mut required_sets: BTreeSet<(String, String)> = BTreeSet::new();
    for case in &release.cases {
        required_sets.insert((case.set.clone(), case.set_version.clone()));
    }
    for (set, ver) in &required_sets {
        if criteria.get(set, ver).is_none() {
            return Err(ValError::MissingCriteria { set: format!("{set}-{ver}") });
        }
    }

    // 2. Sort input cases deterministically by (mapped_algorithm_or_empty, set, case_id).
    let mut ordered: Vec<(&ReleaseCase, Option<&Mapping>)> =
        release.cases.iter().map(|c| (c, mappings.lookup(&c.set, &c.primitive, &c.case_type))).collect();
    ordered.sort_by(|(ca, ma), (cb, mb)| {
        let aa = ma.map_or("", |m| m.algorithm.as_str());
        let ab = mb.map_or("", |m| m.algorithm.as_str());
        (aa, ca.set.as_str(), ca.case_id.as_str()).cmp(&(ab, cb.set.as_str(), cb.case_id.as_str()))
    });

    // 3. Execute mapped cases through the artifact runner and evaluate pass criteria.
    let mut results = Vec::with_capacity(ordered.len());
    let mut groups: BTreeMap<(String, String), GroupSummary> = BTreeMap::new();

    for (case, mapping_opt) in ordered {
        let Some(mapping) = mapping_opt else {
            let reason = NotRunReason::NoMapping;
            let key = (String::new(), case.primitive.clone());
            let g = groups.entry(key).or_insert_with(|| GroupSummary {
                algorithm: String::new(),
                primitive: case.primitive.clone(),
                passed: 0,
                failed: 0,
                not_run: 0,
                not_run_reasons: BTreeMap::new(),
                worst_deviation: BTreeMap::new(),
            });
            g.not_run += 1;
            *g.not_run_reasons.entry(reason.as_tag()).or_insert(0) += 1;
            results.push(CaseResult {
                case_id: case.case_id.clone(),
                set: case.set.clone(),
                set_version: case.set_version.clone(),
                primitive: case.primitive.clone(),
                case_type: case.case_type.clone(),
                algorithm: String::new(),
                outcome: CaseOutcome::NotRun,
                reason: Some(reason),
                deviations: BTreeMap::new(),
                uncertainties: BTreeMap::new(),
            });
            continue;
        };

        let crit = criteria
            .get(&case.set, &case.set_version)
            .ok_or_else(|| ValError::MissingCriteria { set: format!("{}-{}", case.set, case.set_version) })?;

        let call_bytes = build_kernel_call(mapping, case)?;
        let ans = runner.invoke(&call_bytes)?;
        let decoded = decode_artifact_answer(&ans, &mapping.algorithm)?;

        let g = groups.entry((mapping.algorithm.clone(), case.primitive.clone())).or_insert_with(|| GroupSummary {
            algorithm: mapping.algorithm.clone(),
            primitive: case.primitive.clone(),
            passed: 0,
            failed: 0,
            not_run: 0,
            not_run_reasons: BTreeMap::new(),
            worst_deviation: BTreeMap::new(),
        });

        match decoded {
            DecodedArtifact::KernelError(code) => {
                let reason = NotRunReason::KernelError { code };
                g.not_run += 1;
                *g.not_run_reasons.entry(reason.as_tag()).or_insert(0) += 1;
                results.push(CaseResult {
                    case_id: case.case_id.clone(),
                    set: case.set.clone(),
                    set_version: case.set_version.clone(),
                    primitive: case.primitive.clone(),
                    case_type: case.case_type.clone(),
                    algorithm: mapping.algorithm.clone(),
                    outcome: CaseOutcome::NotRun,
                    reason: Some(reason),
                    deviations: BTreeMap::new(),
                    uncertainties: BTreeMap::new(),
                });
            }
            DecodedArtifact::Output(actual) => {
                let mut deviations = BTreeMap::new();
                let mut uncertainties = BTreeMap::new();
                let mut all_pass = true;
                for rule in &mapping.compare_rules {
                    let pname = rule.param_name();
                    let limit = *crit.per_parameter_limits.get(pname).ok_or_else(|| {
                        ValError::MappingInvalid(format!(
                            "criteria {}-{} missing limit for parameter '{pname}'",
                            crit.set, crit.version
                        ))
                    })?;
                    let (dev, unc) = compute_deviation(rule, &actual, &case.reference)?;
                    if !dev.is_finite() || (dev + unc) > limit {
                        all_pass = false;
                    }
                    deviations.insert(pname.to_string(), dev);
                    uncertainties.insert(pname.to_string(), unc);
                    let entry = g.worst_deviation.entry(pname.to_string()).or_insert(0.0);
                    if dev > *entry {
                        *entry = dev;
                    }
                }
                let outcome = if all_pass {
                    g.passed += 1;
                    CaseOutcome::Passed
                } else {
                    g.failed += 1;
                    CaseOutcome::Failed
                };
                results.push(CaseResult {
                    case_id: case.case_id.clone(),
                    set: case.set.clone(),
                    set_version: case.set_version.clone(),
                    primitive: case.primitive.clone(),
                    case_type: case.case_type.clone(),
                    algorithm: mapping.algorithm.clone(),
                    outcome,
                    reason: None,
                    deviations,
                    uncertainties,
                });
            }
        }
    }

    let summaries: Vec<GroupSummary> = groups.into_values().collect();
    Ok(RunResults {
        release_id: release.release_id.clone(),
        suite_digest: release.suite_digest,
        mapping_version: mappings.version.clone(),
        cases: results,
        summaries,
    })
}
