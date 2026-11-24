//! Release workflow validator and deterministic step-graph simulator
//! (`cargo xtask rel flow-sim`, spec node `ECO.REL.PROC.FLOW`).
//!
//! Models the `release-state.json` state machine (`tagged -> built -> gated ->
//! awaiting_approval -> approved -> signed -> published -> feeds_updated`, plus
//! `blocked` and `failed`), canonical RFC 8785 `presign_digest`, idempotent
//! venue publication with SHA-256 conflict detection, `disabled_venues` filtering,
//! and bounded GitHub Actions workflow secrets-scope validation (`AC-7`).
//!
//! Offline, read-only on `--fixtures` (writes only under `--out` when given),
//! D0 deterministic, zero `unsafe`, zero panics.

use super::runbook::{parse_runbook, parse_workflow_jobs, Performer, Step, SIGN_APPROVAL};
use super::version::{check_tag, parse_tag, Channel};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// CLI usage string for `cargo xtask rel flow-sim`.
pub const USAGE: &str = "usage: cargo xtask rel flow-sim [--tag <tag>] [--channel nightly|beta|stable|lts] \
                         --fixtures <dir> [--approval <json>] [--state <json>] [--out <dir>] [--runbook <dir>] [--json]\n       \
                         cargo xtask rel flow-sim --check-workflow [--workflow <file>] [--runbook <dir>] [--json]";

/// Default release workflow path.
pub const DEFAULT_WORKFLOW: &str = "release/release-workflow.yml";
/// Default runbook directory.
pub const DEFAULT_RUNBOOK_DIR: &str = "release";
/// Maximum workflow YAML size (1 MiB).
pub const MAX_WORKFLOW_BYTES: usize = 1 << 20;
/// Maximum JSON fixture size (16 MiB).
pub const MAX_JSON_BYTES: usize = 16 << 20;
/// Maximum artefact size hashed in simulator (64 MiB).
pub const MAX_ARTEFACT_BYTES: usize = 64 << 20;
/// Maximum JSON nesting depth.
pub const MAX_DEPTH: usize = 32;

/// State machine values for `release-state.json` (spec node `ECO.REL.PROC.FLOW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowState {
    Tagged,
    Built,
    Gated,
    AwaitingApproval,
    Approved,
    Signed,
    Published,
    FeedsUpdated,
    Blocked,
    Failed,
}

impl FlowState {
    pub fn as_str(self) -> &'static str {
        match self {
            FlowState::Tagged => "tagged",
            FlowState::Built => "built",
            FlowState::Gated => "gated",
            FlowState::AwaitingApproval => "awaiting_approval",
            FlowState::Approved => "approved",
            FlowState::Signed => "signed",
            FlowState::Published => "published",
            FlowState::FeedsUpdated => "feeds_updated",
            FlowState::Blocked => "blocked",
            FlowState::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tagged" => Some(FlowState::Tagged),
            "built" => Some(FlowState::Built),
            "gated" => Some(FlowState::Gated),
            "awaiting_approval" => Some(FlowState::AwaitingApproval),
            "approved" => Some(FlowState::Approved),
            "signed" => Some(FlowState::Signed),
            "published" => Some(FlowState::Published),
            "feeds_updated" => Some(FlowState::FeedsUpdated),
            "blocked" => Some(FlowState::Blocked),
            "failed" => Some(FlowState::Failed),
            _ => None,
        }
    }
}

/// One executed or skipped step recorded in `release-state.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub id: String,
    pub status: String,
    pub outputs_sha256: Option<String>,
}

/// Complete `release-state.json` state document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseState {
    pub tag: String,
    pub channel: String,
    pub state: FlowState,
    pub steps: Vec<StepRecord>,
    pub presign_digest: Option<String>,
    pub approval_ref: Option<String>,
    pub reasons: Vec<String>,
    pub published_venues: BTreeMap<String, BTreeMap<String, String>>,
    pub uploaded_count: BTreeMap<String, usize>,
}

impl ReleaseState {
    /// Converts to canonical JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("tag".to_string(), CValue::Str(self.tag.clone()));
        o.insert("channel".to_string(), CValue::Str(self.channel.clone()));
        o.insert("state".to_string(), CValue::Str(self.state.as_str().to_string()));
        let steps_arr = self
            .steps
            .iter()
            .map(|s| {
                let mut st = BTreeMap::new();
                st.insert("id".to_string(), CValue::Str(s.id.clone()));
                st.insert("status".to_string(), CValue::Str(s.status.clone()));
                if let Some(d) = &s.outputs_sha256 {
                    st.insert("outputs_sha256".to_string(), CValue::Str(d.clone()));
                }
                CValue::Obj(st)
            })
            .collect();
        o.insert("steps".to_string(), CValue::Arr(steps_arr));
        o.insert(
            "presign_digest".to_string(),
            self.presign_digest.as_ref().map_or(CValue::Null, |d| CValue::Str(d.clone())),
        );
        o.insert(
            "approval_ref".to_string(),
            self.approval_ref.as_ref().map_or(CValue::Null, |r| CValue::Str(r.clone())),
        );
        o.insert("reasons".to_string(), CValue::Arr(self.reasons.iter().map(|r| CValue::Str(r.clone())).collect()));

        let mut venues_obj = BTreeMap::new();
        for (vname, files) in &self.published_venues {
            let mut fobj = BTreeMap::new();
            for (fname, sha) in files {
                fobj.insert(fname.clone(), CValue::Str(sha.clone()));
            }
            venues_obj.insert(vname.clone(), CValue::Obj(fobj));
        }
        o.insert("published_venues".to_string(), CValue::Obj(venues_obj));

        let mut uploads_obj = BTreeMap::new();
        for (vname, cnt) in &self.uploaded_count {
            uploads_obj.insert(vname.clone(), CValue::Num(*cnt as f64));
        }
        o.insert("uploaded_count".to_string(), CValue::Obj(uploads_obj));

        CValue::Obj(o)
    }

    /// Canonical RFC 8785 JSON string (no trailing newline).
    pub fn to_canonical_json(&self) -> String {
        let bytes = to_canonical_bytes(&self.to_value()).unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_default()
    }
}

/// Workflow check finding (`AC-7` and structural checks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowFinding {
    pub code: &'static str,
    pub subject: String,
    pub message: String,
}

impl fmt::Display for WorkflowFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.message)
    }
}

/// Result of [`check_workflow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCheckReport {
    pub jobs: Vec<String>,
    pub pre_approval_jobs: Vec<String>,
    pub post_approval_jobs: Vec<String>,
    pub findings: Vec<WorkflowFinding>,
}

impl WorkflowCheckReport {
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn to_canonical_json(&self) -> String {
        let mut o = BTreeMap::new();
        o.insert("ok".to_string(), CValue::Bool(self.is_ok()));
        o.insert("jobs".to_string(), CValue::Arr(self.jobs.iter().map(|s| CValue::Str(s.clone())).collect()));
        o.insert(
            "pre_approval_jobs".to_string(),
            CValue::Arr(self.pre_approval_jobs.iter().map(|s| CValue::Str(s.clone())).collect()),
        );
        o.insert(
            "post_approval_jobs".to_string(),
            CValue::Arr(self.post_approval_jobs.iter().map(|s| CValue::Str(s.clone())).collect()),
        );
        o.insert(
            "findings".to_string(),
            CValue::Arr(
                self.findings
                    .iter()
                    .map(|f| {
                        let mut m = BTreeMap::new();
                        m.insert("code".to_string(), CValue::Str(f.code.to_string()));
                        m.insert("subject".to_string(), CValue::Str(f.subject.clone()));
                        m.insert("message".to_string(), CValue::Str(f.message.clone()));
                        CValue::Obj(m)
                    })
                    .collect(),
            ),
        );
        let bytes = to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_default()
    }
}

/// Fatal input/usage/I/O error (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowError {
    pub code: &'static str,
    pub message: String,
}

impl FlowError {
    fn parse(msg: impl Into<String>) -> Self {
        Self { code: "FLOW-INPUT", message: msg.into() }
    }

    fn io(msg: impl Into<String>) -> Self {
        Self { code: "FLOW-IO", message: msg.into() }
    }
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for FlowError {}

fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, FlowError> {
    let f = std::fs::File::open(path).map_err(|e| FlowError::parse(format!("{}: {e}", path.display())))?;
    let cap = u64::try_from(max_bytes).unwrap_or(u64::MAX).saturating_add(1);
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf).map_err(|e| FlowError::parse(format!("{}: {e}", path.display())))?;
    if buf.len() > max_bytes {
        return Err(FlowError::parse(format!("{}: exceeds {max_bytes} bytes", path.display())));
    }
    Ok(buf)
}

fn read_text_bounded(path: &Path, max_bytes: usize) -> Result<String, FlowError> {
    let bytes = read_bounded(path, max_bytes)?;
    String::from_utf8(bytes).map_err(|_| FlowError::parse(format!("{}: not valid UTF-8", path.display())))
}

fn strip_yaml_comment(line: &str) -> &str {
    let mut prev_space = true;
    for (i, c) in line.char_indices() {
        if c == '#' && prev_space {
            return line.get(..i).unwrap_or("").trim_end();
        }
        prev_space = c == ' ' || c == '\t';
    }
    line.trim_end()
}

fn extract_top_level_block<'a>(lines: &[&'a str], key: &str) -> Option<Vec<(usize, &'a str)>> {
    let target = format!("{key}:");
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with([' ', '\t', '#']) {
            let trimmed = strip_yaml_comment(line).trim_start_matches('\u{feff}');
            if trimmed == target || trimmed.starts_with(&format!("{target} ")) {
                start = Some(i + 1);
                break;
            }
        }
    }
    let start = start?;
    let mut out = Vec::new();
    for (offset, line) in lines.get(start..)?.iter().enumerate() {
        let lineno = start + offset + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with([' ', '\t']) {
            break;
        }
        out.push((lineno, *line));
    }
    Some(out)
}

fn extract_job_blocks<'a>(lines: &[&'a str]) -> Result<BTreeMap<String, Vec<(usize, &'a str)>>, WorkflowFinding> {
    let mut jobs_start = None;
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with([' ', '\t', '#']) && strip_yaml_comment(line).trim_start_matches('\u{feff}') == "jobs:" {
            jobs_start = Some(i + 1);
            break;
        }
    }
    let Some(start) = jobs_start else {
        return Err(WorkflowFinding {
            code: "FLOW-PARSE",
            subject: "jobs".to_string(),
            message: "missing top-level `jobs:` block".to_string(),
        });
    };

    let mut blocks: BTreeMap<String, Vec<(usize, &'a str)>> = BTreeMap::new();
    let mut indent: Option<usize> = None;
    let mut current_job: Option<String> = None;

    for (offset, line) in lines.get(start..).unwrap_or_default().iter().enumerate() {
        let lineno = start + offset + 1;
        let body = line.trim_start_matches(' ');
        if body.is_empty() || body.starts_with('#') || body.trim().is_empty() {
            continue;
        }
        if body.starts_with('\t') {
            return Err(WorkflowFinding {
                code: "FLOW-PARSE",
                subject: format!("line {lineno}"),
                message: "tab in indentation".to_string(),
            });
        }
        let depth = line.len() - body.len();
        if depth == 0 {
            break;
        }
        let level = *indent.get_or_insert(depth);
        if depth == level {
            let content = strip_yaml_comment(body);
            let Some((raw_key, rest)) = content.split_once(':') else {
                return Err(WorkflowFinding {
                    code: "FLOW-PARSE",
                    subject: format!("line {lineno}"),
                    message: "expected `<job-id>:`".to_string(),
                });
            };
            if !(rest.is_empty() || rest.starts_with(' ')) {
                return Err(WorkflowFinding {
                    code: "FLOW-PARSE",
                    subject: format!("line {lineno}"),
                    message: "expected `<job-id>:`".to_string(),
                });
            }
            let key = raw_key
                .strip_prefix('"')
                .and_then(|k| k.strip_suffix('"'))
                .or_else(|| raw_key.strip_prefix('\'').and_then(|k| k.strip_suffix('\'')))
                .unwrap_or(raw_key)
                .to_string();
            blocks.entry(key.clone()).or_default();
            current_job = Some(key);
        } else if depth > level {
            if let Some(job) = &current_job {
                if let Some(vec) = blocks.get_mut(job) {
                    vec.push((lineno, *line));
                }
            }
        } else {
            return Err(WorkflowFinding {
                code: "FLOW-PARSE",
                subject: format!("line {lineno}"),
                message: "unexpected indentation under `jobs:`".to_string(),
            });
        }
    }

    Ok(blocks)
}

fn contains_token_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// Validates a GitHub Actions workflow against `release/runbook.toml` and `AC-7`
/// security invariants (no fork-PR triggers, read-only permissions, zero secrets
/// or `release-signing` environment in pre-approval jobs).
pub fn check_workflow(workflow_path: &Path, runbook_dir: &Path) -> Result<WorkflowCheckReport, FlowError> {
    let wf_text = read_text_bounded(workflow_path, MAX_WORKFLOW_BYTES)?;
    let rb_text = read_text_bounded(&runbook_dir.join("runbook.toml"), MAX_WORKFLOW_BYTES)?;
    let steps = parse_runbook(&rb_text).map_err(|f| FlowError::parse(f.to_string()))?;

    let approval_pos = steps
        .iter()
        .position(|s| s.id == SIGN_APPROVAL && matches!(s.performer, Performer::Role(_)))
        .ok_or_else(|| FlowError::parse("runbook.toml lacks `sign-approval` human step"))?;

    let mut pre_approval_jobs = Vec::new();
    let mut post_approval_jobs = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        if step.performer == Performer::Ci {
            if let Some(job) = &step.job {
                if idx < approval_pos {
                    pre_approval_jobs.push(job.clone());
                } else {
                    post_approval_jobs.push(job.clone());
                }
            }
        }
    }

    let mut findings = Vec::new();
    let wf_label = workflow_path.file_name().and_then(|s| s.to_str()).unwrap_or("release-workflow.yml");
    let jobs = match parse_workflow_jobs(&wf_text, wf_label) {
        Ok(j) => j,
        Err(f) => {
            findings.push(WorkflowFinding { code: "FLOW-PARSE", subject: f.subject, message: f.message });
            return Ok(WorkflowCheckReport { jobs: Vec::new(), pre_approval_jobs, post_approval_jobs, findings });
        }
    };

    let expected_jobs: Vec<String> = pre_approval_jobs.iter().chain(post_approval_jobs.iter()).cloned().collect();
    if jobs != expected_jobs {
        findings.push(WorkflowFinding {
            code: "FLOW-RUNBOOK-MISMATCH",
            subject: wf_label.to_string(),
            message: format!("workflow jobs {:?} do not match runbook CI job order {:?}", jobs, expected_jobs),
        });
    }

    let lines: Vec<&str> = wf_text.lines().collect();

    // Trigger check: fork PR triggers (`pull_request`, `pull_request_target`, `workflow_run`) forbidden.
    match extract_top_level_block(&lines, "on") {
        None => findings.push(WorkflowFinding {
            code: "FLOW-PARSE",
            subject: "on".to_string(),
            message: "missing top-level `on:` trigger block".to_string(),
        }),
        Some(on_lines) => {
            for (lineno, line) in on_lines {
                let stripped = strip_yaml_comment(line).trim();
                for bad in ["pull_request", "pull_request_target", "workflow_run"] {
                    if stripped == format!("{bad}:")
                        || stripped.starts_with(&format!("{bad}:"))
                        || stripped == format!("- {bad}")
                    {
                        findings.push(WorkflowFinding {
                            code: "FLOW-FORK-TRIGGER",
                            subject: format!("line {lineno}"),
                            message: format!("forbidden trigger `{bad}` on release workflow"),
                        });
                    }
                }
            }
        }
    }

    // Permissions check: must include `contents: read` and never `write-all` / `contents: write`.
    match extract_top_level_block(&lines, "permissions") {
        None => findings.push(WorkflowFinding {
            code: "FLOW-PERMISSIONS",
            subject: "permissions".to_string(),
            message: "missing top-level `permissions: contents: read`".to_string(),
        }),
        Some(perm_lines) => {
            let mut has_contents_read = false;
            for (lineno, line) in perm_lines {
                let stripped = strip_yaml_comment(line).trim();
                if stripped == "contents: read" {
                    has_contents_read = true;
                }
                if stripped.contains(": write") || stripped == "write-all" {
                    findings.push(WorkflowFinding {
                        code: "FLOW-PERMISSIONS",
                        subject: format!("line {lineno}"),
                        message: format!("excessive top-level permission `{stripped}`"),
                    });
                }
            }
            if !has_contents_read {
                findings.push(WorkflowFinding {
                    code: "FLOW-PERMISSIONS",
                    subject: "permissions".to_string(),
                    message: "top-level `permissions` must include `contents: read`".to_string(),
                });
            }
        }
    }

    match extract_job_blocks(&lines) {
        Err(f) => findings.push(f),
        Ok(job_blocks) => {
            let pre_set: BTreeSet<&str> = pre_approval_jobs.iter().map(String::as_str).collect();
            for job_id in &jobs {
                let Some(body) = job_blocks.get(job_id) else { continue };
                if pre_set.contains(job_id.as_str()) {
                    for (lineno, line) in body {
                        let trimmed = line.trim();
                        if trimmed.starts_with('#') {
                            continue;
                        }
                        if contains_token_ci(trimmed, "secrets.") {
                            findings.push(WorkflowFinding {
                                code: "FLOW-PREAPPROVAL-SECRET",
                                subject: job_id.clone(),
                                message: format!("line {lineno}: pre-approval job references `secrets.`"),
                            });
                        }
                        if trimmed.starts_with("environment:") && contains_token_ci(trimmed, "release-signing") {
                            findings.push(WorkflowFinding {
                                code: "FLOW-PREAPPROVAL-ENV",
                                subject: job_id.clone(),
                                message: format!("line {lineno}: pre-approval job uses `release-signing` environment"),
                            });
                        }
                        if contains_token_ci(trimmed, "release-signing") && !trimmed.starts_with('#') {
                            findings.push(WorkflowFinding {
                                code: "FLOW-PREAPPROVAL-ENV",
                                subject: job_id.clone(),
                                message: format!("line {lineno}: pre-approval job references `release-signing`"),
                            });
                        }
                    }
                } else if job_id == "os-sign" || job_id == "sign-statements" {
                    let has_signing_env = body.iter().any(|(_, line)| {
                        let t = strip_yaml_comment(line).trim();
                        t.starts_with("environment:") && contains_token_ci(t, "release-signing")
                    });
                    if !has_signing_env {
                        findings.push(WorkflowFinding {
                            code: "FLOW-SIGNING-ENV",
                            subject: job_id.clone(),
                            message: "signing job must specify `environment: release-signing`".to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(WorkflowCheckReport { jobs, pre_approval_jobs, post_approval_jobs, findings })
}

// ------------------------------------------------------------------ simulator

/// Options for [`simulate`].
#[derive(Debug, Clone)]
pub struct SimOptions<'a> {
    pub tag: Option<&'a str>,
    pub channel_override: Option<Channel>,
    pub fixtures: &'a Path,
    pub approval: Option<&'a Path>,
    pub prior_state: Option<&'a Path>,
    pub out: Option<&'a Path>,
    pub runbook_dir: &'a Path,
}

struct GatesDoc {
    release: Option<String>,
    channel: Option<Channel>,
    verdict: String,
    disabled_venues: BTreeSet<String>,
    venues: Option<Vec<String>>,
    reasons: Vec<String>,
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn sha256_file(path: &Path) -> Result<String, FlowError> {
    let bytes = read_bounded(path, MAX_ARTEFACT_BYTES)?;
    Ok(sha256_bytes(&bytes))
}

fn parse_gates_doc(path: &Path) -> Result<(GatesDoc, String), FlowError> {
    let bytes = read_bounded(path, MAX_JSON_BYTES)?;
    let gates_sha = sha256_bytes(&bytes);
    let v = parse_with_limits(&bytes, MAX_JSON_BYTES, MAX_DEPTH)
        .map_err(|e| FlowError::parse(format!("{}: invalid JSON: {e}", path.display())))?;
    let CValue::Obj(o) = v else {
        return Err(FlowError::parse(format!("{}: must be a JSON object", path.display())));
    };
    let release = match o.get("release") {
        Some(CValue::Str(s)) if !s.is_empty() => Some(s.clone()),
        Some(_) => return Err(FlowError::parse("release-gates.json: `release` must be a non-empty string")),
        None => None,
    };
    let channel = match o.get("channel") {
        Some(CValue::Str(s)) => Some(
            Channel::parse(s).ok_or_else(|| FlowError::parse(format!("release-gates.json: unknown channel {s:?}")))?,
        ),
        Some(_) => return Err(FlowError::parse("release-gates.json: `channel` must be a string")),
        None => None,
    };
    let verdict = match o.get("verdict") {
        Some(CValue::Str(s)) if s == "pass" || s == "block" => s.clone(),
        _ => return Err(FlowError::parse("release-gates.json: `verdict` must be \"pass\" or \"block\"")),
    };
    let mut disabled_venues = BTreeSet::new();
    if let Some(dv) = o.get("disabled_venues") {
        let CValue::Arr(items) = dv else {
            return Err(FlowError::parse("release-gates.json: `disabled_venues` must be an array"));
        };
        for item in items {
            let CValue::Str(s) = item else {
                return Err(FlowError::parse("release-gates.json: `disabled_venues` entries must be strings"));
            };
            disabled_venues.insert(s.clone());
        }
    }
    let venues = if let Some(vs) = o.get("venues") {
        let CValue::Arr(items) = vs else {
            return Err(FlowError::parse("release-gates.json: `venues` must be an array"));
        };
        let mut out = Vec::new();
        for item in items {
            let CValue::Str(s) = item else {
                return Err(FlowError::parse("release-gates.json: `venues` entries must be strings"));
            };
            out.push(s.clone());
        }
        out.sort();
        out.dedup();
        Some(out)
    } else {
        None
    };
    let mut reasons = Vec::new();
    if let Some(CValue::Arr(gates)) = o.get("gates") {
        for g in gates {
            if let CValue::Obj(go) = g {
                if go.get("status") == Some(&CValue::Str("block".to_string())) {
                    if let Some(CValue::Str(id)) = go.get("id") {
                        reasons.push(format!("gate_blocked:{id}"));
                    }
                }
            }
        }
    }
    if verdict == "block" && reasons.is_empty() {
        reasons.push("gate_blocked".to_string());
    }
    Ok((GatesDoc { release, channel, verdict, disabled_venues, venues, reasons }, gates_sha))
}

struct ApprovalDoc {
    presign_digest: String,
    approval_ref: String,
}

fn parse_approval(path: &Path) -> Result<ApprovalDoc, FlowError> {
    let bytes = read_bounded(path, MAX_JSON_BYTES)?;
    let v = parse_with_limits(&bytes, MAX_JSON_BYTES, MAX_DEPTH)
        .map_err(|e| FlowError::parse(format!("{}: invalid JSON: {e}", path.display())))?;
    let CValue::Obj(o) = v else {
        return Err(FlowError::parse(format!("{}: must be a JSON object", path.display())));
    };
    if let Some(CValue::Str(kind)) = o.get("approver_kind") {
        if kind != "human" {
            return Err(FlowError::parse("approval: only human approver credentials are accepted"));
        }
    }
    if let Some(CValue::Str(role)) = o.get("role") {
        if role != "release-approver" {
            return Err(FlowError::parse(format!("approval: role {role:?} is not `release-approver`")));
        }
    }
    let presign_digest = match o.get("presign_digest") {
        Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
        _ => return Err(FlowError::parse("approval: missing `presign_digest` string")),
    };
    let approval_ref = match o.get("approval_ref") {
        Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
        _ => "approval-recorded".to_string(),
    };
    Ok(ApprovalDoc { presign_digest, approval_ref })
}

struct PriorState {
    state: FlowState,
    completed_steps: BTreeMap<String, String>,
    presign_digest: Option<String>,
    approval_ref: Option<String>,
    published_venues: BTreeMap<String, BTreeMap<String, String>>,
}

fn parse_prior_state(path: &Path) -> Result<PriorState, FlowError> {
    let bytes = read_bounded(path, MAX_JSON_BYTES)?;
    let v = parse_with_limits(&bytes, MAX_JSON_BYTES, MAX_DEPTH)
        .map_err(|e| FlowError::parse(format!("{}: invalid JSON: {e}", path.display())))?;
    let CValue::Obj(o) = v else {
        return Err(FlowError::parse(format!("{}: must be a JSON object", path.display())));
    };
    let state_str = match o.get("state") {
        Some(CValue::Str(s)) => s.as_str(),
        _ => return Err(FlowError::parse("prior release-state.json: missing `state`")),
    };
    let state = FlowState::parse(state_str)
        .ok_or_else(|| FlowError::parse(format!("prior release-state.json: unknown state {state_str:?}")))?;
    let mut completed_steps = BTreeMap::new();
    if let Some(CValue::Arr(steps)) = o.get("steps") {
        for s in steps {
            if let CValue::Obj(so) = s {
                if so.get("status") == Some(&CValue::Str("completed".to_string())) {
                    if let (Some(CValue::Str(id)), Some(CValue::Str(sha))) = (so.get("id"), so.get("outputs_sha256")) {
                        completed_steps.insert(id.clone(), sha.clone());
                    }
                }
            }
        }
    }
    let presign_digest = match o.get("presign_digest") {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let approval_ref = match o.get("approval_ref") {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let mut published_venues = BTreeMap::new();
    if let Some(CValue::Obj(vs)) = o.get("published_venues") {
        for (vname, files_v) in vs {
            if let CValue::Obj(fmap) = files_v {
                let mut inner = BTreeMap::new();
                for (fname, sha_v) in fmap {
                    if let CValue::Str(sha) = sha_v {
                        inner.insert(fname.clone(), sha.clone());
                    }
                }
                published_venues.insert(vname.clone(), inner);
            }
        }
    }
    Ok(PriorState { state, completed_steps, presign_digest, approval_ref, published_venues })
}

fn collect_dir_files(dir: &Path) -> Result<BTreeMap<String, (PathBuf, String)>, FlowError> {
    let mut out = BTreeMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let entries = std::fs::read_dir(dir).map_err(|e| FlowError::io(format!("{}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| FlowError::io(format!("{}: {e}", dir.display())))?;
        let p = entry.path();
        if p.is_file() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                let sha = sha256_file(&p)?;
                out.insert(name.to_string(), (p, sha));
            }
        }
    }
    Ok(out)
}

fn compute_presign_digest(items: &BTreeMap<String, String>) -> String {
    let arr: Vec<CValue> = items
        .iter()
        .map(|(name, sha)| {
            let mut m = BTreeMap::new();
            m.insert("name".to_string(), CValue::Str(name.clone()));
            m.insert("sha256".to_string(), CValue::Str(sha.clone()));
            CValue::Obj(m)
        })
        .collect();
    let bytes = to_canonical_bytes(&CValue::Arr(arr)).unwrap_or_default();
    sha256_bytes(&bytes)
}

fn step_digest(step_id: &str, tag: &str, channel: &str, presign: Option<&str>) -> String {
    let mut m = BTreeMap::new();
    m.insert("channel".to_string(), CValue::Str(channel.to_string()));
    m.insert("step".to_string(), CValue::Str(step_id.to_string()));
    m.insert("tag".to_string(), CValue::Str(tag.to_string()));
    if let Some(p) = presign {
        m.insert("presign_digest".to_string(), CValue::Str(p.to_string()));
    }
    let bytes = to_canonical_bytes(&CValue::Obj(m)).unwrap_or_default();
    sha256_bytes(&bytes)
}

fn write_state_atomic(out_dir: &Path, state: &ReleaseState) -> Result<(), FlowError> {
    std::fs::create_dir_all(out_dir).map_err(|e| FlowError::io(format!("{}: {e}", out_dir.display())))?;
    let tmp = out_dir.join(".release-state.json.tmp");
    let dest = out_dir.join("release-state.json");
    let mut text = state.to_canonical_json();
    text.push('\n');
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| FlowError::io(format!("{}: {e}", tmp.display())))?;
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(FlowError::io(format!("{}: {e}", dest.display())));
    }
    Ok(())
}

/// Runs the deterministic release step-graph simulator (`AC-1`..`AC-6`).
pub fn simulate(opts: &SimOptions<'_>) -> Result<ReleaseState, FlowError> {
    let gates_path = opts.fixtures.join("release-gates.json");
    let (gates, gates_sha) = parse_gates_doc(&gates_path)?;

    let tag = opts
        .tag
        .map(str::to_string)
        .or_else(|| gates.release.clone())
        .ok_or_else(|| FlowError::parse("--tag is required when release-gates.json omits `release`"))?;
    let parsed_tag = parse_tag(&tag).map_err(|e| FlowError::parse(e.to_string()))?;
    let channel = opts.channel_override.or(gates.channel).unwrap_or_else(|| match parsed_tag.pre {
        Some(super::version::Pre::Nightly(_)) => Channel::Nightly,
        Some(super::version::Pre::Beta(_)) => Channel::Beta,
        None => Channel::Stable,
    });
    check_tag(&tag, channel).map_err(|e| FlowError::parse(e.to_string()))?;

    let rb_text = read_text_bounded(&opts.runbook_dir.join("runbook.toml"), MAX_WORKFLOW_BYTES)?;
    let steps = parse_runbook(&rb_text).map_err(|f| FlowError::parse(f.to_string()))?;

    let prior_path = opts.prior_state.map(Path::to_path_buf).or_else(|| {
        let p = opts.fixtures.join("release-state.json");
        p.is_file().then_some(p)
    });
    let prior = match &prior_path {
        Some(p) => Some(parse_prior_state(p)?),
        None => None,
    };

    let artefacts = collect_dir_files(&opts.fixtures.join("artefacts"))?;
    let sboms = collect_dir_files(&opts.fixtures.join("sboms"))?;

    // Pre-sign manifest items: unsigned artefacts + SBOMs + release-gates.json.
    let mut presign_items: BTreeMap<String, String> = BTreeMap::new();
    for (name, (_, sha)) in &artefacts {
        presign_items.insert(name.clone(), sha.clone());
    }
    for (name, (_, sha)) in &sboms {
        presign_items.insert(name.clone(), sha.clone());
    }
    presign_items.insert("release-gates.json".to_string(), gates_sha);
    let computed_presign = compute_presign_digest(&presign_items);

    // Discover venues and any existing venue files (to detect idempotency vs SHA-256 conflicts).
    let mut venues_state: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut venue_names: BTreeSet<String> = BTreeSet::new();
    if let Some(vs) = &gates.venues {
        for v in vs {
            venue_names.insert(v.clone());
        }
    }
    let venues_dir = opts.fixtures.join("venues");
    if venues_dir.is_dir() {
        let entries =
            std::fs::read_dir(&venues_dir).map_err(|e| FlowError::io(format!("{}: {e}", venues_dir.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| FlowError::io(format!("{}: {e}", venues_dir.display())))?;
            let p = entry.path();
            if p.is_dir() {
                if let Some(vname) = p.file_name().and_then(|n| n.to_str()) {
                    venue_names.insert(vname.to_string());
                    let existing = collect_dir_files(&p)?;
                    let map = existing.into_iter().map(|(k, (_, sha))| (k, sha)).collect();
                    venues_state.insert(vname.to_string(), map);
                }
            }
        }
    }
    for vname in &venue_names {
        venues_state.entry(vname.clone()).or_default();
    }
    if let Some(pr) = &prior {
        for (vname, fmap) in &pr.published_venues {
            let entry = venues_state.entry(vname.clone()).or_default();
            for (fname, sha) in fmap {
                entry.entry(fname.clone()).or_insert_with(|| sha.clone());
            }
        }
    }
    let mut uploaded_count: BTreeMap<String, usize> = venues_state.keys().map(|k| (k.clone(), 0usize)).collect();

    let applicable: Vec<&Step> = steps.iter().filter(|s| s.channels.iter().any(|c| c == channel.name())).collect();
    let approval_idx = applicable.iter().position(|s| s.id == SIGN_APPROVAL).unwrap_or(applicable.len());

    let mut state = FlowState::Tagged;
    let mut step_records: Vec<StepRecord> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    let mut presign_digest: Option<String> = prior.as_ref().and_then(|p| p.presign_digest.clone());
    let mut approval_ref: Option<String> = prior.as_ref().and_then(|p| p.approval_ref.clone());

    // Phase 1: pre-approval CI steps.
    for step in applicable.iter().take(approval_idx) {
        if step.performer != Performer::Ci {
            continue;
        }
        if let Some(prev_sha) = prior.as_ref().and_then(|p| p.completed_steps.get(&step.id)) {
            step_records.push(StepRecord {
                id: step.id.clone(),
                status: "completed".to_string(),
                outputs_sha256: Some(prev_sha.clone()),
            });
            continue;
        }

        if step.id == "build-packages" || step.id == "build-web" || step.id == "sbom" {
            state = FlowState::Built;
        }
        if step.id == "gates-summary" {
            if gates.verdict == "block" {
                step_records.push(StepRecord {
                    id: step.id.clone(),
                    status: "blocked".to_string(),
                    outputs_sha256: None,
                });
                let result = ReleaseState {
                    tag,
                    channel: channel.name().to_string(),
                    state: FlowState::Blocked,
                    steps: step_records,
                    presign_digest: None,
                    approval_ref: None,
                    reasons: gates.reasons,
                    published_venues: venues_state,
                    uploaded_count,
                };
                if let Some(out) = opts.out {
                    write_state_atomic(out, &result)?;
                }
                return Ok(result);
            }
            state = FlowState::Gated;
        }

        let d = step_digest(&step.id, &tag, channel.name(), None);
        step_records.push(StepRecord { id: step.id.clone(), status: "completed".to_string(), outputs_sha256: Some(d) });
    }

    presign_digest = Some(computed_presign.clone());

    // Phase 2: human approval gate (skipped on nightly).
    if channel != Channel::Nightly {
        let already_approved = prior.as_ref().is_some_and(|p| {
            matches!(p.state, FlowState::Approved | FlowState::Signed | FlowState::Published | FlowState::FeedsUpdated)
        });
        if let Some(approval_path) = opts.approval {
            let approval = parse_approval(approval_path)?;
            if approval.presign_digest != computed_presign {
                reasons.push("approval_digest_mismatch".to_string());
                let result = ReleaseState {
                    tag,
                    channel: channel.name().to_string(),
                    state: FlowState::AwaitingApproval,
                    steps: step_records,
                    presign_digest,
                    approval_ref: None,
                    reasons,
                    published_venues: venues_state,
                    uploaded_count,
                };
                if let Some(out) = opts.out {
                    write_state_atomic(out, &result)?;
                }
                return Ok(result);
            }
            approval_ref = Some(approval.approval_ref);
            state = FlowState::Approved;
        } else if already_approved {
            state = FlowState::Approved;
        } else {
            let result = ReleaseState {
                tag,
                channel: channel.name().to_string(),
                state: FlowState::AwaitingApproval,
                steps: step_records,
                presign_digest,
                approval_ref: None,
                reasons,
                published_venues: venues_state,
                uploaded_count,
            };
            if let Some(out) = opts.out {
                write_state_atomic(out, &result)?;
            }
            return Ok(result);
        }
    }

    // Publishable files = artefacts + sboms.
    let mut publish_files: BTreeMap<String, (Option<PathBuf>, String)> = BTreeMap::new();
    for (name, (path, sha)) in &artefacts {
        publish_files.insert(name.clone(), (Some(path.clone()), sha.clone()));
    }
    for (name, (path, sha)) in &sboms {
        publish_files.insert(name.clone(), (Some(path.clone()), sha.clone()));
    }

    // Phase 3: post-approval CI steps.
    for step in applicable.iter().skip(approval_idx) {
        if step.performer != Performer::Ci {
            continue;
        }
        if let Some(prev_sha) = prior.as_ref().and_then(|p| p.completed_steps.get(&step.id)) {
            step_records.push(StepRecord {
                id: step.id.clone(),
                status: "completed".to_string(),
                outputs_sha256: Some(prev_sha.clone()),
            });
            continue;
        }

        if step.id == "sign-statements" || step.id == "os-sign" {
            state = FlowState::Signed;
        }

        if step.id == "publish" {
            let enabled_venues: Vec<String> =
                venues_state.keys().filter(|v| !gates.disabled_venues.contains(*v)).cloned().collect();
            for venue in enabled_venues {
                for (fname, (maybe_src, sha)) in &publish_files {
                    let existing_sha = venues_state.get(&venue).and_then(|m| m.get(fname)).cloned();
                    match existing_sha {
                        Some(prev) if prev == *sha => {
                            // Idempotent: already holds exact file name and SHA-256 -> skip upload.
                        }
                        Some(other) => {
                            step_records.push(StepRecord {
                                id: step.id.clone(),
                                status: "failed".to_string(),
                                outputs_sha256: None,
                            });
                            reasons.push(format!("venue_conflict({fname}, {venue}): remote {other} != {sha}"));
                            let result = ReleaseState {
                                tag,
                                channel: channel.name().to_string(),
                                state: FlowState::Failed,
                                steps: step_records,
                                presign_digest,
                                approval_ref,
                                reasons,
                                published_venues: venues_state,
                                uploaded_count,
                            };
                            if let Some(out) = opts.out {
                                write_state_atomic(out, &result)?;
                            }
                            return Ok(result);
                        }
                        None => {
                            if let Some(m) = venues_state.get_mut(&venue) {
                                m.insert(fname.clone(), sha.clone());
                            }
                            if let Some(cnt) = uploaded_count.get_mut(&venue) {
                                *cnt += 1;
                            }
                            if let (Some(out_dir), Some(src_path)) = (opts.out, maybe_src) {
                                let vdir = out_dir.join("venues").join(&venue);
                                std::fs::create_dir_all(&vdir)
                                    .map_err(|e| FlowError::io(format!("{}: {e}", vdir.display())))?;
                                std::fs::copy(src_path, vdir.join(fname))
                                    .map_err(|e| FlowError::io(format!("{}: {e}", vdir.display())))?;
                            }
                        }
                    }
                }
            }
            state = FlowState::Published;
        }

        if step.id == "feeds" {
            state = FlowState::FeedsUpdated;
        }

        let d = step_digest(&step.id, &tag, channel.name(), presign_digest.as_deref());
        step_records.push(StepRecord { id: step.id.clone(), status: "completed".to_string(), outputs_sha256: Some(d) });
    }

    if state != FlowState::FeedsUpdated {
        state = FlowState::FeedsUpdated;
    }

    let result = ReleaseState {
        tag,
        channel: channel.name().to_string(),
        state,
        steps: step_records,
        presign_digest,
        approval_ref,
        reasons,
        published_venues: venues_state,
        uploaded_count,
    };
    if let Some(out) = opts.out {
        write_state_atomic(out, &result)?;
    }
    Ok(result)
}

/// CLI entrypoint for `cargo xtask rel flow-sim`.
pub fn cli(args: &[String]) -> ExitCode {
    let mut tag: Option<String> = None;
    let mut channel: Option<Channel> = None;
    let mut fixtures: Option<PathBuf> = None;
    let mut approval: Option<PathBuf> = None;
    let mut prior_state: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut workflow = PathBuf::from(DEFAULT_WORKFLOW);
    let mut runbook = PathBuf::from(DEFAULT_RUNBOOK_DIR);
    let mut check_wf = false;
    let mut json = false;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" if !json => json = true,
            "--check-workflow" if !check_wf => check_wf = true,
            "--tag" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                tag = Some(v.clone());
            }
            "--channel" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                match Channel::parse(v) {
                    Some(c) => channel = Some(c),
                    None => {
                        eprintln!("rel flow-sim: unknown channel {v:?}\n{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            "--fixtures" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                fixtures = Some(PathBuf::from(v));
            }
            "--approval" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                approval = Some(PathBuf::from(v));
            }
            "--state" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                prior_state = Some(PathBuf::from(v));
            }
            "--out" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                out = Some(PathBuf::from(v));
            }
            "--workflow" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                workflow = PathBuf::from(v);
                check_wf = true;
            }
            "--runbook" => {
                let Some(v) = it.next() else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                runbook = PathBuf::from(v);
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    if check_wf && fixtures.is_none() {
        return match check_workflow(&workflow, &runbook) {
            Ok(report) => {
                if json {
                    println!("{}", report.to_canonical_json());
                } else if report.is_ok() {
                    println!(
                        "flow-sim workflow: ok ({} jobs, {} pre-approval)",
                        report.jobs.len(),
                        report.pre_approval_jobs.len()
                    );
                } else {
                    for f in &report.findings {
                        println!("{f}");
                    }
                }
                if report.is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(2)
            }
        };
    }

    let Some(fixtures_dir) = fixtures else {
        eprintln!("rel flow-sim: --fixtures or --check-workflow is required\n{USAGE}");
        return ExitCode::from(2);
    };

    let opts = SimOptions {
        tag: tag.as_deref(),
        channel_override: channel,
        fixtures: &fixtures_dir,
        approval: approval.as_deref(),
        prior_state: prior_state.as_deref(),
        out: out.as_deref(),
        runbook_dir: &runbook,
    };

    match simulate(&opts) {
        Ok(res) => {
            if json {
                println!("{}", res.to_canonical_json());
            } else {
                println!(
                    "flow-sim: {} ({}) state={}{}",
                    res.tag,
                    res.channel,
                    res.state.as_str(),
                    res.presign_digest.as_deref().map_or_else(String::new, |d| format!(" presign={d}"))
                );
                for r in &res.reasons {
                    println!("  reason: {r}");
                }
            }
            let ok = match res.state {
                FlowState::FeedsUpdated => true,
                FlowState::AwaitingApproval => res.reasons.is_empty(),
                _ => false,
            };
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
