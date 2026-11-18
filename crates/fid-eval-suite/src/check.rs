//! Suite consistency, coverage and oracle isolation check (spec node AGENT.EVAL.CHECK).
//!
//! [`check_suite`] loads a suite through the format parser, the catalog index
//! (AGENT.SKILLS.CATALOG `catalog-index.json`) and a directory of FLOW job packages, then
//! applies every rule and returns all findings, never just the first:
//!
//! - `digest_mismatch`: a start-state archive, a task's golden-journal digest or a
//!   package's journal does not match its recorded digest (REQ-001);
//! - `unknown_job`: a task's source job has no package (REQ-001);
//! - `uncovered_workflow`: a catalog workflow named by no task (REQ-002);
//! - `oracle_leak`: a file under a granted root of the unpacked start state that is, by
//!   digest, an oracle, a golden journal or another task's file (REQ-003);
//! - `oracle_in_prompt`: a prompt naming a characteristic together with its oracle
//!   outcome, or stating an oracle value in the item's unit or a scaled unit (REQ-004);
//! - `parse_error`: anything that could not be read (format errors pass through).
//!
//! The check is static (D0): archives are listed in memory, never extracted.

use crate::json::{parse_bounded, Ctx};
use crate::suite::{is_real_dir, list_dir, load_suite, read_bounded, Suite};
use crate::task::{relpath, wire_enum};
use crate::{file_digest, tar, ErrorCode, FormatError};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

wire_enum! {
    /// Rule code of a check finding.
    Rule {
        DigestMismatch = "digest_mismatch",
        UnknownJob = "unknown_job",
        UncoveredWorkflow = "uncovered_workflow",
        OracleLeak = "oracle_leak",
        OracleInPrompt = "oracle_in_prompt",
        ParseError = "parse_error",
    }
}

/// One finding. `file` is suite-relative for suite files, archive-relative for an entry of
/// a start state (`oracle_leak`), jobs-relative for packages, and as given for the catalog.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckFinding {
    pub task: Option<String>,
    pub rule: Rule,
    pub file: Option<String>,
    pub detail: String,
}

impl CheckFinding {
    fn new(rule: Rule, task: Option<&str>, file: Option<&str>, detail: impl Into<String>) -> CheckFinding {
        CheckFinding { task: task.map(str::to_string), rule, file: file.map(str::to_string), detail: detail.into() }
    }

    /// A format error as a finding: a start-state digest mismatch keeps its own rule, every
    /// other error passes through as `parse_error`. The task is taken from the path.
    pub fn from_format_error(e: &FormatError) -> CheckFinding {
        let mut parts = e.file.split('/');
        let task = match (parts.next(), parts.next()) {
            (Some("tasks" | "oracles"), Some(id)) if !id.is_empty() => Some(id),
            _ => None,
        };
        let file = (!e.file.is_empty()).then_some(e.file.as_str());
        if e.code == ErrorCode::InvalidTask && e.field == "start_state.digest" && e.detail.starts_with("archive ") {
            return CheckFinding::new(Rule::DigestMismatch, task, file, format!("start state: {}", e.detail));
        }
        CheckFinding::new(Rule::ParseError, task, file, e.to_string())
    }
}

impl fmt::Display for CheckFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.rule)?;
        if let Some(t) = &self.task {
            write!(f, ": task {t}")?;
        }
        if let Some(p) = &self.file {
            write!(f, ": {p}")?;
        }
        write!(f, ": {}", self.detail)
    }
}

/// The part of a FLOW job package (`acceptance-job@1`) the check uses. Other fields are
/// validated by FLOW.RUNNER.PKG, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPackage {
    /// The package directory name inside the jobs directory.
    pub dir: String,
    pub job_id: String,
    /// The golden-journal digest the package declares.
    pub journal_digest: Digest,
    /// The digest of the journal file as read.
    pub journal_actual: Digest,
}

/// The result of [`check_suite`]: the loaded suite (if it loaded) and every finding,
/// sorted by task, rule, file and detail.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckReport {
    pub suite: Option<Suite>,
    pub findings: Vec<CheckFinding>,
}

/// The workflow IDs of a catalog index `{manifest_digest, rows: [{catalog_version,
/// workflow_id}]}`, in file order. Read fail-closed like every suite file.
pub fn load_catalog(path: &Path) -> Result<Vec<String>, Vec<FormatError>> {
    let name = path.to_string_lossy().to_string();
    let bytes = read_bounded(path, &name).map_err(|e| vec![e])?;
    let root = parse_bounded(&bytes, &name, ErrorCode::InvalidSuite).map_err(|e| vec![e])?;
    let mut cx = Ctx::new(ErrorCode::InvalidSuite, &name);
    let keys = &["manifest_digest", "rows"];
    let Some(m) = cx.obj(Some(&root), "", keys, keys) else {
        return Err(cx.finish());
    };
    cx.digest(m.get("manifest_digest"), "manifest_digest");
    let mut out: Vec<String> = Vec::new();
    if let Some(rows) = cx.arr(m.get("rows"), "rows") {
        for (i, row) in rows.iter().enumerate() {
            let f = format!("rows[{i}]");
            let keys = &["catalog_version", "workflow_id"];
            let Some(r) = cx.obj(Some(row), &f, keys, keys) else { continue };
            cx.digest(r.get("catalog_version"), &format!("{f}.catalog_version"));
            match cx.text(r.get("workflow_id"), &format!("{f}.workflow_id")) {
                Some(w) if out.contains(&w) => cx.err(&format!("{f}.workflow_id"), "repeated workflow"),
                Some(w) => out.push(w),
                None => {}
            }
        }
    }
    if cx.has_errors() {
        return Err(cx.finish());
    }
    Ok(out)
}

/// Reads one package directory; problems become findings.
fn load_package(dir: &Path, name: &str, out: &mut Vec<CheckFinding>) -> Option<JobPackage> {
    let file = format!("{name}/package.json");
    let parse_err = |e: &FormatError| CheckFinding::new(Rule::ParseError, None, Some(&e.file), e.to_string());
    let bytes = match read_bounded(&dir.join("package.json"), &file) {
        Ok(b) => b,
        Err(e) => {
            out.push(parse_err(&e));
            return None;
        }
    };
    let root = match parse_bounded(&bytes, &file, ErrorCode::InvalidSuite) {
        Ok(v) => v,
        Err(e) => {
            out.push(parse_err(&e));
            return None;
        }
    };
    let CValue::Obj(m) = &root else {
        out.push(CheckFinding::new(Rule::ParseError, None, Some(&file), "expected a JSON object"));
        return None;
    };
    let mut cx = Ctx::new(ErrorCode::InvalidSuite, &file);
    for k in ["format", "job_id", "journal"] {
        if !m.contains_key(k) {
            cx.err(k, "missing");
        }
    }
    if let Some(f) = cx.string(m.get("format"), "format") {
        if f != "acceptance-job@1" {
            cx.err("format", "expected acceptance-job@1");
        }
    }
    let job_id = cx.text(m.get("job_id"), "job_id");
    let journal = cx.obj(m.get("journal"), "journal", &["file", "digest"], &["file", "digest"]).and_then(|j| {
        let path = relpath(&mut cx, j.get("file"), "journal.file");
        let digest = cx.digest(j.get("digest"), "journal.digest");
        Some((path?, digest?))
    });
    if cx.has_errors() {
        out.extend(cx.finish().iter().map(parse_err));
        return None;
    }
    let (Some(job_id), Some((jpath, journal_digest))) = (job_id, journal) else {
        return None;
    };
    let jname = format!("{name}/{jpath}");
    let jbytes = match read_bounded(&dir.join(jpath.as_str()), &jname) {
        Ok(b) => b,
        Err(e) => {
            out.push(parse_err(&e));
            return None;
        }
    };
    let journal_actual = file_digest(&jbytes);
    if journal_actual != journal_digest {
        out.push(CheckFinding::new(
            Rule::DigestMismatch,
            None,
            Some(&file),
            format!("journal {jname} has digest {journal_actual}, the package declares {journal_digest}"),
        ));
    }
    Some(JobPackage { dir: name.to_string(), job_id, journal_digest, journal_actual })
}

/// Reads every package directory `<jobs>/<name>/package.json` with its golden journal.
/// Problems are returned as findings next to the packages that did load.
pub fn load_jobs(jobs: &Path) -> (Vec<JobPackage>, Vec<CheckFinding>) {
    let mut findings = Vec::new();
    let mut errs = Vec::new();
    let mut packages = Vec::new();
    let jname = jobs.to_string_lossy().to_string();
    let names = list_dir(jobs, &jname, &mut errs).unwrap_or_default();
    for name in names {
        let dir = jobs.join(&name);
        if !is_real_dir(&dir) {
            findings.push(CheckFinding::new(
                Rule::ParseError,
                None,
                Some(&name),
                "expected a job package directory (symbolic links and junctions are not allowed)",
            ));
            continue;
        }
        if let Some(p) = load_package(&dir, &name, &mut findings) {
            packages.push(p);
        }
    }
    if packages.is_empty() && findings.is_empty() && errs.is_empty() {
        findings.push(CheckFinding::new(Rule::ParseError, None, Some(&jname), "no job packages"));
    }
    findings.extend(errs.iter().map(|e| CheckFinding::new(Rule::ParseError, None, Some(&e.file), e.to_string())));
    (packages, findings)
}

/// Loads the suite, catalog index and job packages and applies every rule.
pub fn check_suite(suite_dir: &Path, catalog: &Path, jobs: &Path) -> CheckReport {
    let mut findings = Vec::new();
    let suite = match load_suite(suite_dir) {
        Ok(s) => Some(s),
        Err(errs) => {
            findings.extend(errs.iter().map(CheckFinding::from_format_error));
            None
        }
    };
    let workflows = match load_catalog(catalog) {
        Ok(w) => Some(w),
        Err(errs) => {
            findings
                .extend(errs.iter().map(|e| CheckFinding::new(Rule::ParseError, None, Some(&e.file), e.to_string())));
            None
        }
    };
    let (packages, mut job_findings) = load_jobs(jobs);
    findings.append(&mut job_findings);
    if let Some(st) = &suite {
        findings.extend(check_loaded(suite_dir, st, workflows.as_deref(), &packages));
    }
    findings.sort();
    findings.dedup();
    CheckReport { suite, findings }
}

/// The rules over a loaded suite. `workflows` is `None` if the catalog did not load.
pub fn check_loaded(
    suite_dir: &Path,
    st: &Suite,
    workflows: Option<&[String]>,
    packages: &[JobPackage],
) -> Vec<CheckFinding> {
    let mut out = Vec::new();

    // REQ-001: source job and golden-journal digest against the job packages.
    let mut by_job: BTreeMap<&str, BTreeSet<&Digest>> = BTreeMap::new();
    for p in packages {
        by_job.entry(p.job_id.as_str()).or_default().insert(&p.journal_digest);
    }
    for t in &st.tasks {
        let t = &t.task;
        let tfile = format!("tasks/{}/task.json", t.id);
        match by_job.get(t.source_job.as_str()) {
            None => out.push(CheckFinding::new(
                Rule::UnknownJob,
                Some(&t.id),
                Some(&tfile),
                format!("source job {} has no package in the jobs directory", t.source_job),
            )),
            Some(ds) if !ds.contains(&&t.golden_digest) => out.push(CheckFinding::new(
                Rule::DigestMismatch,
                Some(&t.id),
                Some(&tfile),
                format!("golden_digest {} matches no golden journal of job {}", t.golden_digest, t.source_job),
            )),
            Some(_) => {}
        }
    }

    // REQ-002: every catalog workflow is named by some task.
    if let Some(ws) = workflows {
        let covered: BTreeSet<&str> =
            st.tasks.iter().flat_map(|t| t.task.workflows.iter().map(String::as_str)).collect();
        for w in ws.iter().filter(|w| !covered.contains(w.as_str())) {
            out.push(CheckFinding::new(
                Rule::UncoveredWorkflow,
                None,
                None,
                format!("workflow {w} is named by no task"),
            ));
        }
    }

    // REQ-003: nothing under a granted root has the digest of protected material.
    let mut protected: BTreeMap<&Digest, Vec<(Option<&str>, String)>> = BTreeMap::new();
    for (path, d) in &st.files {
        let mut parts = path.split('/');
        match (parts.next(), parts.next()) {
            (Some("oracles"), Some(_)) => protected.entry(d).or_default().push((None, format!("oracle {path}"))),
            (Some("tasks"), Some(id)) => {
                protected.entry(d).or_default().push((Some(id), format!("file {path} of task {id}")))
            }
            _ => {}
        }
    }
    for p in packages {
        for d in [&p.journal_digest, &p.journal_actual] {
            let label = format!("golden journal of {} ({})", p.job_id, p.dir);
            let v = protected.entry(d).or_default();
            if !v.iter().any(|(_, l)| *l == label) {
                v.push((None, label));
            }
        }
    }
    for t in &st.tasks {
        let t = &t.task;
        let aname = format!("tasks/{}/{}", t.id, t.start_state.path);
        let bytes = match read_bounded(&suite_dir.join("tasks").join(&t.id).join(t.start_state.path.as_str()), &aname) {
            Ok(b) => b,
            Err(e) => {
                out.push(CheckFinding::new(Rule::ParseError, Some(&t.id), Some(&aname), e.to_string()));
                continue;
            }
        };
        let entries = match tar::list(&bytes) {
            Ok(es) => es,
            Err(reason) => {
                out.push(CheckFinding::new(
                    Rule::ParseError,
                    Some(&t.id),
                    Some(&aname),
                    format!("start state cannot be unpacked safely: {reason}"),
                ));
                continue;
            }
        };
        let roots: Vec<&str> = t.roots.read.iter().chain(&t.roots.write).map(|r| r.as_str()).collect();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for e in &entries {
            let granted = roots
                .iter()
                .any(|r| e.path == *r || (e.path.starts_with(r) && e.path.as_bytes().get(r.len()) == Some(&b'/')));
            if !granted || !seen.insert(e.path.as_str()) {
                continue;
            }
            let Some(hits) = protected.get(&e.digest) else { continue };
            if let Some((_, label)) = hits.iter().find(|(owner, _)| *owner != Some(t.id.as_str())) {
                out.push(CheckFinding::new(
                    Rule::OracleLeak,
                    Some(&t.id),
                    Some(&e.path),
                    format!("granted file has the same content as the {label}"),
                ));
            }
        }
    }

    // REQ-004: the prompt does not give away the oracle.
    for t in &st.tasks {
        let prompt = &t.task.prompt;
        let tfile = format!("tasks/{}/task.json", t.task.id);
        for item in &t.oracle.items {
            let c = &item.characteristic;
            if has_token(prompt, c) && has_token(prompt, item.outcome.as_str()) {
                out.push(CheckFinding::new(
                    Rule::OracleInPrompt,
                    Some(&t.task.id),
                    Some(&tfile),
                    format!("prompt names characteristic {c} together with its oracle outcome {}", item.outcome),
                ));
            }
            if let (Some(v), Some(tol)) = (&item.value, &item.tol) {
                if value_in_prompt(prompt, v.amount, &v.unit, tol.amount) {
                    out.push(CheckFinding::new(
                        Rule::OracleInPrompt,
                        Some(&t.task.id),
                        Some(&tfile),
                        format!("prompt states the oracle value of characteristic {c}"),
                    ));
                }
            }
        }
    }
    out
}

/// A byte that continues an identifier: ASCII letter, digit, `_` or `-`.
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// Whether `tok` occurs in `text` with no identifier byte directly before or after it.
pub fn has_token(text: &str, tok: &str) -> bool {
    if tok.is_empty() {
        return false;
    }
    let b = text.as_bytes();
    (0..b.len()).any(|i| {
        b[i..].starts_with(tok.as_bytes())
            && !(i > 0 && is_word(b[i - 1]))
            && !b.get(i + tok.len()).is_some_and(|c| is_word(*c))
    })
}

/// Unit symbols a value may be written in, with their factor to the SI unit.
fn unit_scales(unit: &str) -> Vec<(&str, f64)> {
    let deg = std::f64::consts::PI / 180.0;
    match unit {
        "m" => vec![
            ("m", 1.0),
            ("mm", 1e-3),
            ("cm", 1e-2),
            ("um", 1e-6),
            ("\u{b5}m", 1e-6),
            ("\u{3bc}m", 1e-6),
            ("nm", 1e-9),
        ],
        "m2" => vec![("m2", 1.0), ("mm2", 1e-6)],
        "m3" => vec![("m3", 1.0), ("mm3", 1e-9)],
        "rad" => vec![("rad", 1.0), ("mrad", 1e-3), ("deg", deg), ("\u{b0}", deg)],
        // A bare number cannot be told from any other number in a prompt.
        "1" => vec![],
        other => vec![(other, 1.0)],
    }
}

/// End of a decimal number starting at `i` (`[+-]?(\d+(\.\d+)?|\.\d+)([eE][+-]?\d+)?`).
fn number_end(b: &[u8], i: usize) -> Option<usize> {
    let digit = |k: usize| b.get(k).is_some_and(u8::is_ascii_digit);
    let mut j = i;
    if matches!(b.get(j), Some(b'+' | b'-')) {
        j += 1;
    }
    let start = j;
    while digit(j) {
        j += 1;
    }
    if b.get(j) == Some(&b'.') && digit(j + 1) {
        j += 1;
        while digit(j) {
            j += 1;
        }
    } else if j == start {
        return None;
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(b.get(k), Some(b'+' | b'-')) {
            k += 1;
        }
        let exp = k;
        while digit(k) {
            k += 1;
        }
        if k > exp {
            j = k;
        }
    }
    Some(j)
}

/// Whether the prompt states `amount` (within `tol`) as a number followed by a symbol
/// of `unit` or a scaled unit (e.g. `12.5 um` for 1.25e-5 m).
pub fn value_in_prompt(prompt: &str, amount: f64, unit: &str, tol: f64) -> bool {
    let scales = unit_scales(unit);
    if scales.is_empty() {
        return false;
    }
    let b = prompt.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let free = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b'_' | b'.' | b'+' | b'-'));
        if let Some(end) = number_end(b, i).filter(|_| free) {
            if let Ok(n) = prompt[i..end].parse::<f64>() {
                let rest = prompt[end..].trim_start_matches([' ', '\t']);
                for &(sym, scale) in &scales {
                    let bounded = rest.starts_with(sym)
                        && !rest.as_bytes().get(sym.len()).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
                    if bounded && (n * scale - amount).abs() <= tol {
                        return true;
                    }
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    false
}
