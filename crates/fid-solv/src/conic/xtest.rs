//! Frozen conic test set runner and cross-target bit-identity comparator
//! (spec node `KRN.SOLV.CONIC.XTEST`, REQ-KRN.SOLV.CONIC.XTEST-001 to -004).
//!
//! Pure KNP-1 library logic: no I/O, no `unsafe`, no `HashMap`, no `f64::min`/`max`
//! (`fid_num::f::total_max`), no panics. The dev binary `src/bin/conic-xtest.rs` provides the
//! thin filesystem CLI around [`run_suite`] and [`compare_result_bytes`].

use super::{
    solve_lp, solve_qp, solve_socp, Cone, ConicCause, ConicError, ConicProblem, ConicProblemBuilder, Csc, ProblemClass,
};
use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_num::f::total_max;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Default relative tolerance for reference objective checks (REQ-KRN.SOLV.CONIC.XTEST-003).
pub const DEFAULT_REL_TOL: f64 = 1e-8;
/// Expected manifest format string.
pub const MANIFEST_FORMAT: &str = "fid-conic-xtest-manifest/1";
/// Expected problems file format string.
pub const PROBLEMS_FORMAT: &str = "fid-conic-xtest-problems/1";
/// Minimum problems required per conic class (REQ-KRN.SOLV.CONIC.XTEST-001).
pub const MIN_PER_CLASS: usize = 50;
/// Maximum input JSON document size accepted by the parser (16 MiB).
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_JSON_DEPTH: usize = 32;

/// Expected outcome of a frozen conic test problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedStatus {
    Solved,
    PrimalInfeasible,
    DualInfeasible,
}

impl ExpectedStatus {
    /// Canonical wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            ExpectedStatus::Solved => "solved",
            ExpectedStatus::PrimalInfeasible => "primal_infeasible",
            ExpectedStatus::DualInfeasible => "dual_infeasible",
        }
    }

    fn parse(s: &str) -> Option<ExpectedStatus> {
        match s {
            "solved" => Some(ExpectedStatus::Solved),
            "primal_infeasible" => Some(ExpectedStatus::PrimalInfeasible),
            "dual_infeasible" => Some(ExpectedStatus::DualInfeasible),
            _ => None,
        }
    }

    fn expected_cause(self) -> Option<ConicCause> {
        match self {
            ExpectedStatus::Solved => None,
            ExpectedStatus::PrimalInfeasible => Some(ConicCause::PrimalInfeasible),
            ExpectedStatus::DualInfeasible => Some(ConicCause::DualInfeasible),
        }
    }
}

/// Parsed manifest of a frozen conic test set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtestManifest {
    pub version: String,
    pub problems_file: String,
    pub problems_digest: Digest,
}

/// One parsed problem from `problems.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct XtestProblem {
    pub id: String,
    pub class: ProblemClass,
    pub family: String,
    pub problem: ConicProblem,
    pub expected_status: ExpectedStatus,
    pub reference_objective: Option<f64>,
}

/// Canonical result entry for one solved or infeasible problem (REQ-KRN.SOLV.CONIC.XTEST-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtestResult {
    pub id: String,
    pub status: String,
    pub iterations: u32,
    pub x: Vec<String>,
    pub y: Vec<String>,
    pub s: Vec<String>,
}

/// Complete result of running a frozen conic test set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XtestRun {
    pub version: String,
    pub problem_count: usize,
    pub lp_count: usize,
    pub qp_count: usize,
    pub socp_count: usize,
    pub results: Vec<XtestResult>,
    pub canonical_json: Vec<u8>,
    pub digest: Digest,
}

/// First divergence found between two result lists (REQ-KRN.SOLV.CONIC.XTEST-002, AC-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultDiff {
    pub problem_id: String,
    pub field: String,
    pub expected: String,
    pub actual: String,
}

impl fmt::Display for ResultDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "problem {} field {}: expected {}, got {}", self.problem_id, self.field, self.expected, self.actual)
    }
}

/// Errors from loading, verifying, running or comparing a conic xtest suite.
#[derive(Debug, Clone, PartialEq)]
pub enum XtestError {
    Parse(String),
    Manifest(String),
    DigestMismatch { expected: String, actual: String },
    BuildProblem { id: String, error: ConicError },
    UnexpectedStatus { id: String, expected: ExpectedStatus, actual: String },
    ObjectiveOutOfTolerance { id: String, expected: f64, actual: f64, rel_err: f64, tol: f64 },
    Encode(String),
    Mismatch(Box<ResultDiff>),
}

impl fmt::Display for XtestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XtestError::Parse(m) => write!(f, "xtest parse error: {m}"),
            XtestError::Manifest(m) => write!(f, "xtest manifest error: {m}"),
            XtestError::DigestMismatch { expected, actual } => {
                write!(f, "xtest problems digest mismatch: expected {expected}, got {actual}")
            }
            XtestError::BuildProblem { id, error } => write!(f, "xtest problem {id}: {error}"),
            XtestError::UnexpectedStatus { id, expected, actual } => {
                write!(f, "xtest problem {id}: expected status {}, got {actual}", expected.as_str())
            }
            XtestError::ObjectiveOutOfTolerance { id, expected, actual, rel_err, tol } => {
                write!(
                    f,
                    "xtest problem {id}: objective {actual:e} vs reference {expected:e} (rel {rel_err:e} > {tol:e})"
                )
            }
            XtestError::Encode(m) => write!(f, "xtest encode error: {m}"),
            XtestError::Mismatch(d) => write!(f, "xtest mismatch: {d}"),
        }
    }
}

impl core::error::Error for XtestError {}

/// Formats an `f64` as its exact 16-character lowercase IEEE-754 hex bit pattern.
pub fn f64_to_hex(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

/// Parses a 16-character hex IEEE-754 `f64` string, refusing NaN and infinity.
pub fn hex_to_f64(s: &str) -> Result<f64, XtestError> {
    if s.len() != 16 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(XtestError::Parse(format!("invalid 16-char IEEE-754 hex float: {s:?}")));
    }
    let bits = u64::from_str_radix(s, 16).map_err(|_| XtestError::Parse(format!("invalid hex float: {s:?}")))?;
    let v = f64::from_bits(bits);
    if !v.is_finite() {
        return Err(XtestError::Parse(format!("non-finite float in fixture: {s}")));
    }
    Ok(v)
}

/// Parses `manifest.json` and verifies required fields.
pub fn parse_manifest(bytes: &[u8]) -> Result<XtestManifest, XtestError> {
    let root = parse_json(bytes)?;
    let obj = as_obj(&root, "manifest")?;
    let format = req_str(obj, "format", "manifest")?;
    if format != MANIFEST_FORMAT {
        return Err(XtestError::Manifest(format!("unexpected format {format:?}, expected {MANIFEST_FORMAT:?}")));
    }
    let version = req_str(obj, "version", "manifest")?.to_string();
    let problems_file = req_str(obj, "problems_file", "manifest")?.to_string();
    let digest_str = req_str(obj, "problems_digest", "manifest")?;
    let problems_digest = Digest::parse(digest_str)
        .map_err(|e| XtestError::Manifest(format!("invalid problems_digest {digest_str:?}: {e}")))?;
    Ok(XtestManifest { version, problems_file, problems_digest })
}

/// Parses `problems.json`, verifying format, ID ordering and every problem via
/// [`ConicProblemBuilder`].
pub fn parse_problems(bytes: &[u8]) -> Result<Vec<XtestProblem>, XtestError> {
    let root = parse_json(bytes)?;
    let obj = as_obj(&root, "problems document")?;
    let format = req_str(obj, "format", "problems document")?;
    if format != PROBLEMS_FORMAT {
        return Err(XtestError::Manifest(format!("unexpected problems format {format:?}")));
    }
    let items = req_arr(obj, "problems", "problems document")?;
    let mut out = Vec::with_capacity(items.len());
    let mut prev_id: Option<String> = None;

    for (idx, item) in items.iter().enumerate() {
        let p = parse_one_problem(item, idx)?;
        if let Some(prev) = &prev_id {
            if p.id <= *prev {
                return Err(XtestError::Manifest(format!(
                    "problems must be strictly sorted by id: {:?} after {:?}",
                    p.id, prev
                )));
            }
        }
        prev_id = Some(p.id.clone());
        out.push(p);
    }
    Ok(out)
}

fn parse_one_problem(val: &JVal, idx: usize) -> Result<XtestProblem, XtestError> {
    let ctx = format!("problem[{idx}]");
    let obj = as_obj(val, &ctx)?;
    let id = req_str(obj, "id", &ctx)?.to_string();
    let class = match req_str(obj, "class", &id)? {
        "lp" => ProblemClass::Lp,
        "qp" => ProblemClass::Qp,
        "socp" => ProblemClass::Socp,
        other => return Err(XtestError::Parse(format!("{id}: unknown class {other:?}"))),
    };
    let family = req_str(obj, "family", &id)?.to_string();
    let n = req_usize(obj, "n", &id)?;
    let m = req_usize(obj, "m", &id)?;
    let q = req_hex_vec(obj, "q", &id)?;
    let b = req_hex_vec(obj, "b", &id)?;
    let a = parse_csc(obj.get("a").ok_or_else(|| XtestError::Parse(format!("{id}: missing a")))?, &id, "a")?;
    let p_opt = match obj.get("p") {
        None | Some(JVal::Null) => None,
        Some(pv) => Some(parse_csc(pv, &id, "p")?),
    };
    let cones_arr = req_arr(obj, "cones", &id)?;
    let mut cones = Vec::with_capacity(cones_arr.len());
    for (ci, cv) in cones_arr.iter().enumerate() {
        let co = as_obj(cv, &format!("{id}.cones[{ci}]"))?;
        let kind = req_str(co, "kind", &id)?;
        let dim = req_usize(co, "dim", &id)?;
        let cone = match kind {
            "zero" => Cone::Zero(dim),
            "nonneg" => Cone::Nonneg(dim),
            "soc" => Cone::Soc(dim),
            other => return Err(XtestError::Parse(format!("{id}: unknown cone kind {other:?}"))),
        };
        cones.push(cone);
    }
    let expected_status_str = req_str(obj, "expected_status", &id)?;
    let expected_status = ExpectedStatus::parse(expected_status_str)
        .ok_or_else(|| XtestError::Parse(format!("{id}: unknown expected_status {expected_status_str:?}")))?;
    let reference_objective = match obj.get("reference_objective") {
        None | Some(JVal::Null) => None,
        Some(JVal::Str(s)) => Some(hex_to_f64(s)?),
        Some(_) => return Err(XtestError::Parse(format!("{id}: reference_objective must be hex string or null"))),
    };
    if expected_status == ExpectedStatus::Solved && reference_objective.is_none() {
        return Err(XtestError::Manifest(format!("{id}: solved problem requires reference_objective")));
    }
    if q.len() != n || b.len() != m {
        return Err(XtestError::Manifest(format!("{id}: declared dimensions ({n}, {m}) mismatch q/b")));
    }

    let mut builder = ConicProblemBuilder::new(q, a, b, cones);
    if let Some(p) = p_opt {
        builder = builder.p(p);
    }
    let problem = builder.build().map_err(|error| XtestError::BuildProblem { id: id.clone(), error })?;

    Ok(XtestProblem { id, class, family, problem, expected_status, reference_objective })
}

fn parse_csc(val: &JVal, id: &str, name: &str) -> Result<Csc, XtestError> {
    let ctx = format!("{id}.{name}");
    let obj = as_obj(val, &ctx)?;
    let nrows = req_usize(obj, "nrows", &ctx)?;
    let ncols = req_usize(obj, "ncols", &ctx)?;
    let colptr = req_usize_vec(obj, "colptr", &ctx)?;
    let rowval = req_usize_vec(obj, "rowval", &ctx)?;
    let nzval = req_hex_vec(obj, "nzval", &ctx)?;
    Ok(Csc::new(nrows, ncols, colptr, rowval, nzval))
}

/// Runs the full frozen conic test set, verifying the manifest digest, class counts, every
/// problem's expected status and reference objective, and returning the canonical JSON result
/// list plus its SHA-256 digest (REQ-KRN.SOLV.CONIC.XTEST-001 to -004).
pub fn run_suite(manifest_bytes: &[u8], problems_bytes: &[u8], rel_tol: f64) -> Result<XtestRun, XtestError> {
    if !rel_tol.is_finite() || rel_tol < 0.0 {
        return Err(XtestError::Manifest("relative tolerance must be finite and non-negative".into()));
    }
    let manifest = parse_manifest(manifest_bytes)?;
    let actual_digest = sha256_digest(problems_bytes);
    if actual_digest != manifest.problems_digest {
        return Err(XtestError::DigestMismatch {
            expected: manifest.problems_digest.to_string(),
            actual: actual_digest.to_string(),
        });
    }

    let problems = parse_problems(problems_bytes)?;
    let mut lp_count = 0usize;
    let mut qp_count = 0usize;
    let mut socp_count = 0usize;
    for p in &problems {
        match p.class {
            ProblemClass::Lp => lp_count += 1,
            ProblemClass::Qp => qp_count += 1,
            ProblemClass::Socp => socp_count += 1,
        }
    }
    if lp_count < MIN_PER_CLASS || qp_count < MIN_PER_CLASS || socp_count < MIN_PER_CLASS {
        return Err(XtestError::Manifest(format!(
            "test set requires at least {MIN_PER_CLASS} per class; got lp={lp_count}, qp={qp_count}, socp={socp_count}"
        )));
    }

    let mut results = Vec::with_capacity(problems.len());
    for p in &problems {
        results.push(solve_and_check(p, rel_tol)?);
    }

    let (canonical_json, digest) = encode_result_list(&results)?;
    Ok(XtestRun {
        version: manifest.version,
        problem_count: problems.len(),
        lp_count,
        qp_count,
        socp_count,
        results,
        canonical_json,
        digest,
    })
}

fn solve_and_check(p: &XtestProblem, rel_tol: f64) -> Result<XtestResult, XtestError> {
    let outcome = match p.class {
        ProblemClass::Lp => solve_lp(&p.problem),
        ProblemClass::Qp => solve_qp(&p.problem),
        ProblemClass::Socp => solve_socp(&p.problem),
    };

    match p.expected_status {
        ExpectedStatus::Solved => match outcome {
            Ok(sol) => {
                let ref_obj = p
                    .reference_objective
                    .ok_or_else(|| XtestError::Manifest(format!("{}: missing reference_objective", p.id)))?;
                let denom = total_max(ref_obj.abs(), 1e-12)
                    .map_err(|_| XtestError::Manifest(format!("{}: non-finite reference_objective", p.id)))?;
                let rel_err = (sol.objective - ref_obj).abs() / denom;
                if !rel_err.is_finite() || rel_err > rel_tol {
                    return Err(XtestError::ObjectiveOutOfTolerance {
                        id: p.id.clone(),
                        expected: ref_obj,
                        actual: sol.objective,
                        rel_err,
                        tol: rel_tol,
                    });
                }
                Ok(XtestResult {
                    id: p.id.clone(),
                    status: ExpectedStatus::Solved.as_str().to_string(),
                    iterations: sol.iterations,
                    x: sol.x.iter().map(|&v| f64_to_hex(v)).collect(),
                    y: sol.y.iter().map(|&v| f64_to_hex(v)).collect(),
                    s: sol.s.iter().map(|&v| f64_to_hex(v)).collect(),
                })
            }
            Err(err) => Err(XtestError::UnexpectedStatus {
                id: p.id.clone(),
                expected: p.expected_status,
                actual: format!("{err}"),
            }),
        },
        ExpectedStatus::PrimalInfeasible | ExpectedStatus::DualInfeasible => match outcome {
            Err(ConicError::Solver(se)) if Some(se.cause) == p.expected_status.expected_cause() => Ok(XtestResult {
                id: p.id.clone(),
                status: p.expected_status.as_str().to_string(),
                iterations: se.iterations,
                x: Vec::new(),
                y: Vec::new(),
                s: Vec::new(),
            }),
            Ok(_) => Err(XtestError::UnexpectedStatus {
                id: p.id.clone(),
                expected: p.expected_status,
                actual: "solved".to_string(),
            }),
            Err(err) => Err(XtestError::UnexpectedStatus {
                id: p.id.clone(),
                expected: p.expected_status,
                actual: format!("{err}"),
            }),
        },
    }
}

/// Encodes a result list into canonical RFC 8785 JSON (`[{id,iterations,s,status,x,y}, ...]`)
/// and returns `(bytes, sha256_digest)` (REQ-KRN.SOLV.CONIC.XTEST-004).
pub fn encode_result_list(results: &[XtestResult]) -> Result<(Vec<u8>, Digest), XtestError> {
    let mut bytes = Vec::new();
    bytes.push(b'[');
    for (idx, r) in results.iter().enumerate() {
        if idx > 0 {
            bytes.push(b',');
        }
        bytes.extend_from_slice(br#"{"id":"#);
        push_json_str(&mut bytes, &r.id)?;
        bytes.extend_from_slice(br#","iterations":"#);
        bytes.extend_from_slice(r.iterations.to_string().as_bytes());
        bytes.extend_from_slice(br#","s":"#);
        push_json_str_arr(&mut bytes, &r.s)?;
        bytes.extend_from_slice(br#","status":"#);
        push_json_str(&mut bytes, &r.status)?;
        bytes.extend_from_slice(br#","x":"#);
        push_json_str_arr(&mut bytes, &r.x)?;
        bytes.extend_from_slice(br#","y":"#);
        push_json_str_arr(&mut bytes, &r.y)?;
        bytes.push(b'}');
    }
    bytes.push(b']');
    let digest = sha256_digest(&bytes);
    Ok((bytes, digest))
}

fn push_json_str(out: &mut Vec<u8>, s: &str) -> Result<(), XtestError> {
    let enc = to_canonical(&Json::Str(s.to_string())).map_err(|e| XtestError::Encode(e.code().to_string()))?;
    out.extend_from_slice(&enc);
    Ok(())
}

fn push_json_str_arr(out: &mut Vec<u8>, items: &[String]) -> Result<(), XtestError> {
    out.push(b'[');
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        push_json_str(out, s)?;
    }
    out.push(b']');
    Ok(())
}

/// Parses a canonical JSON result list produced by [`encode_result_list`].
pub fn parse_result_list(bytes: &[u8]) -> Result<Vec<XtestResult>, XtestError> {
    let root = parse_json(bytes)?;
    let JVal::Arr(items) = root else {
        return Err(XtestError::Parse("result list must be a JSON array".into()));
    };
    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let ctx = format!("result[{idx}]");
        let obj = as_obj(item, &ctx)?;
        let id = req_str(obj, "id", &ctx)?.to_string();
        let status = req_str(obj, "status", &id)?.to_string();
        let iter_u64 = req_u64(obj, "iterations", &id)?;
        let iterations =
            u32::try_from(iter_u64).map_err(|_| XtestError::Parse(format!("{id}: iterations out of u32 range")))?;
        let x = req_str_vec(obj, "x", &id)?;
        let y = req_str_vec(obj, "y", &id)?;
        let s = req_str_vec(obj, "s", &id)?;
        out.push(XtestResult { id, status, iterations, x, y, s });
    }
    Ok(out)
}

/// Compares two canonical result lists byte-for-byte, returning the first differing problem ID
/// and field on any divergence (REQ-KRN.SOLV.CONIC.XTEST-002, AC-2).
pub fn compare_result_bytes(expected_bytes: &[u8], actual_bytes: &[u8]) -> Result<Digest, XtestError> {
    let expected_digest = sha256_digest(expected_bytes);
    let actual_digest = sha256_digest(actual_bytes);
    if expected_bytes == actual_bytes && expected_digest == actual_digest {
        return Ok(expected_digest);
    }
    let expected = parse_result_list(expected_bytes)?;
    let actual = parse_result_list(actual_bytes)?;
    if let Some(diff) = first_diff(&expected, &actual) {
        return Err(XtestError::Mismatch(Box::new(diff)));
    }
    // Same parsed fields but non-canonical byte representation differed.
    Err(XtestError::Mismatch(Box::new(ResultDiff {
        problem_id: "<canonical>".to_string(),
        field: "digest".to_string(),
        expected: expected_digest.to_string(),
        actual: actual_digest.to_string(),
    })))
}

/// Returns the first differing problem ID and field between two parsed result lists.
pub fn first_diff(expected: &[XtestResult], actual: &[XtestResult]) -> Option<ResultDiff> {
    let common = expected.len().min(actual.len());
    for i in 0..common {
        let e = &expected[i];
        let a = &actual[i];
        if e.id != a.id {
            return Some(ResultDiff {
                problem_id: e.id.clone(),
                field: "id".to_string(),
                expected: e.id.clone(),
                actual: a.id.clone(),
            });
        }
        if e.status != a.status {
            return Some(ResultDiff {
                problem_id: e.id.clone(),
                field: "status".to_string(),
                expected: e.status.clone(),
                actual: a.status.clone(),
            });
        }
        if e.iterations != a.iterations {
            return Some(ResultDiff {
                problem_id: e.id.clone(),
                field: "iterations".to_string(),
                expected: e.iterations.to_string(),
                actual: a.iterations.to_string(),
            });
        }
        if let Some(d) = diff_vec(&e.id, "x", &e.x, &a.x) {
            return Some(d);
        }
        if let Some(d) = diff_vec(&e.id, "y", &e.y, &a.y) {
            return Some(d);
        }
        if let Some(d) = diff_vec(&e.id, "s", &e.s, &a.s) {
            return Some(d);
        }
    }
    if expected.len() != actual.len() {
        let pid =
            expected.get(common).or_else(|| actual.get(common)).map_or_else(|| "<list>".to_string(), |r| r.id.clone());
        return Some(ResultDiff {
            problem_id: pid,
            field: "len".to_string(),
            expected: expected.len().to_string(),
            actual: actual.len().to_string(),
        });
    }
    None
}

fn diff_vec(problem_id: &str, name: &str, exp: &[String], act: &[String]) -> Option<ResultDiff> {
    let common = exp.len().min(act.len());
    for i in 0..common {
        if exp[i] != act[i] {
            return Some(ResultDiff {
                problem_id: problem_id.to_string(),
                field: format!("{name}[{i}]"),
                expected: exp[i].clone(),
                actual: act[i].clone(),
            });
        }
    }
    if exp.len() != act.len() {
        return Some(ResultDiff {
            problem_id: problem_id.to_string(),
            field: format!("{name}.len"),
            expected: exp.len().to_string(),
            actual: act.len().to_string(),
        });
    }
    None
}

// ------------------------------------------------------------------ bounded pure-Rust JSON parser

#[derive(Debug, Clone)]
enum JVal {
    Null,
    Bool,
    Int(i64),
    Str(String),
    Arr(Vec<JVal>),
    Obj(BTreeMap<String, JVal>),
}

fn parse_json(bytes: &[u8]) -> Result<JVal, XtestError> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(XtestError::Parse(format!("JSON input exceeds {MAX_JSON_BYTES} bytes")));
    }
    let text = core::str::from_utf8(bytes).map_err(|_| XtestError::Parse("input is not valid UTF-8".into()))?;
    let mut p = Parser { src: text.as_bytes(), pos: 0 };
    p.skip_ws();
    let v = p.parse_val(0)?;
    p.skip_ws();
    if p.pos != p.src.len() {
        return Err(XtestError::Parse(format!("trailing bytes at offset {}", p.pos)));
    }
    Ok(v)
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\n' | b'\r' | b'\t') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), XtestError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(XtestError::Parse(format!("expected '{}' at offset {}", char::from(byte), self.pos)))
        }
    }

    fn parse_val(&mut self, depth: usize) -> Result<JVal, XtestError> {
        if depth > MAX_JSON_DEPTH {
            return Err(XtestError::Parse("JSON nesting limit exceeded".into()));
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_obj(depth + 1),
            Some(b'[') => self.parse_arr(depth + 1),
            Some(b'"') => self.parse_str().map(JVal::Str),
            Some(b't') => self.parse_lit(b"true", JVal::Bool),
            Some(b'f') => self.parse_lit(b"false", JVal::Bool),
            Some(b'n') => self.parse_lit(b"null", JVal::Null),
            Some(b'-' | b'0'..=b'9') => self.parse_int().map(JVal::Int),
            _ => Err(XtestError::Parse(format!("unexpected token at offset {}", self.pos))),
        }
    }

    fn parse_lit(&mut self, lit: &[u8], out: JVal) -> Result<JVal, XtestError> {
        if self.src.get(self.pos..self.pos + lit.len()) == Some(lit) {
            self.pos += lit.len();
            Ok(out)
        } else {
            Err(XtestError::Parse(format!("invalid literal at offset {}", self.pos)))
        }
    }

    fn parse_int(&mut self) -> Result<i64, XtestError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == digits_start {
            return Err(XtestError::Parse(format!("expected integer at offset {start}")));
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(XtestError::Parse(format!(
                "floating-point JSON numbers are not used in xtest format (use 16-char IEEE-754 hex strings) at offset {}",
                self.pos
            )));
        }
        let slice = self.src.get(start..self.pos).ok_or_else(|| XtestError::Parse("integer slice error".into()))?;
        let text = core::str::from_utf8(slice).map_err(|_| XtestError::Parse("invalid utf8 in integer".into()))?;
        text.parse::<i64>().map_err(|_| XtestError::Parse(format!("integer out of range at offset {start}")))
    }

    fn parse_str(&mut self) -> Result<String, XtestError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(XtestError::Parse("unterminated string".into()));
            };
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(esc) = self.peek() else {
                        return Err(XtestError::Parse("unterminated escape".into()));
                    };
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        _ => return Err(XtestError::Parse(format!("unsupported escape at offset {}", self.pos))),
                    }
                }
                c if c < 0x20 => return Err(XtestError::Parse("control character in JSON string".into())),
                c if c < 0x80 => out.push(char::from(c)),
                _ => {
                    // Multi-byte UTF-8 sequence starting at self.pos - 1
                    let start = self.pos - 1;
                    let rest = self
                        .src
                        .get(start..)
                        .and_then(|s| core::str::from_utf8(s).ok())
                        .ok_or_else(|| XtestError::Parse("invalid utf8 in string".into()))?;
                    let Some(ch) = rest.chars().next() else {
                        return Err(XtestError::Parse("invalid utf8 char".into()));
                    };
                    out.push(ch);
                    self.pos = start + ch.len_utf8();
                }
            }
        }
    }

    fn parse_arr(&mut self, depth: usize) -> Result<JVal, XtestError> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JVal::Arr(out));
        }
        loop {
            out.push(self.parse_val(depth)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JVal::Arr(out));
                }
                _ => return Err(XtestError::Parse(format!("expected ',' or ']' at offset {}", self.pos))),
            }
        }
    }

    fn parse_obj(&mut self, depth: usize) -> Result<JVal, XtestError> {
        self.expect(b'{')?;
        let mut out = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JVal::Obj(out));
        }
        loop {
            self.skip_ws();
            let key = self.parse_str()?;
            self.skip_ws();
            self.expect(b':')?;
            let val = self.parse_val(depth)?;
            if out.insert(key.clone(), val).is_some() {
                return Err(XtestError::Parse(format!("duplicate JSON key {key:?}")));
            }
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JVal::Obj(out));
                }
                _ => return Err(XtestError::Parse(format!("expected ',' or '}}' at offset {}", self.pos))),
            }
        }
    }
}

fn as_obj<'a>(v: &'a JVal, ctx: &str) -> Result<&'a BTreeMap<String, JVal>, XtestError> {
    match v {
        JVal::Obj(m) => Ok(m),
        _ => Err(XtestError::Parse(format!("{ctx}: expected JSON object"))),
    }
}

fn req_str<'a>(obj: &'a BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<&'a str, XtestError> {
    match obj.get(key) {
        Some(JVal::Str(s)) => Ok(s.as_str()),
        _ => Err(XtestError::Parse(format!("{ctx}: expected string field {key:?}"))),
    }
}

fn req_u64(obj: &BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<u64, XtestError> {
    match obj.get(key) {
        Some(JVal::Int(n)) => u64::try_from(*n).map_err(|_| XtestError::Parse(format!("{ctx}: negative {key:?}"))),
        _ => Err(XtestError::Parse(format!("{ctx}: expected integer field {key:?}"))),
    }
}

fn req_usize(obj: &BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<usize, XtestError> {
    let u = req_u64(obj, key, ctx)?;
    usize::try_from(u).map_err(|_| XtestError::Parse(format!("{ctx}: {key:?} overflows usize")))
}

fn req_arr<'a>(obj: &'a BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<&'a [JVal], XtestError> {
    match obj.get(key) {
        Some(JVal::Arr(a)) => Ok(a.as_slice()),
        _ => Err(XtestError::Parse(format!("{ctx}: expected array field {key:?}"))),
    }
}

fn req_usize_vec(obj: &BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<Vec<usize>, XtestError> {
    let arr = req_arr(obj, key, ctx)?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        match v {
            JVal::Int(n) => {
                let u = usize::try_from(*n).map_err(|_| XtestError::Parse(format!("{ctx}.{key}: invalid index")))?;
                out.push(u);
            }
            _ => return Err(XtestError::Parse(format!("{ctx}.{key}: expected integer"))),
        }
    }
    Ok(out)
}

fn req_str_vec(obj: &BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<Vec<String>, XtestError> {
    let arr = req_arr(obj, key, ctx)?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        match v {
            JVal::Str(s) => out.push(s.clone()),
            _ => return Err(XtestError::Parse(format!("{ctx}.{key}: expected string"))),
        }
    }
    Ok(out)
}

fn req_hex_vec(obj: &BTreeMap<String, JVal>, key: &str, ctx: &str) -> Result<Vec<f64>, XtestError> {
    let arr = req_arr(obj, key, ctx)?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        match v {
            JVal::Str(s) => out.push(hex_to_f64(s)?),
            _ => return Err(XtestError::Parse(format!("{ctx}.{key}: expected hex float string"))),
        }
    }
    Ok(out)
}
