//! Study task list `study-tasks@1` and its check (spec node FLOW.STUDY.PROTO).
//!
//! The task list `acceptance/study/tasks.json` ties each job studied with design partners to
//! the journaled command that starts its timing (the file-open import) and the one that ends
//! it (the export of the released report) (REQ-FLOW.STUDY.PROTO-002). Every job ID must be
//! exercised by a journey map (REQ-005, `TASK-NO-MAP`). Each task also names the persona
//! maps and personas it is drawn from; those must exist, and the start and end commands must
//! be steps of one of those maps, start before end (`TASK-REF`). The protocol document must
//! cite the record format of FLOW.STUDY.RECORD and the task list (AC-3, `TASK-DOC`).
//!
//! Output is a pure function of the input files: findings are sorted by code, then subject,
//! then detail (determinism class D0).

use fid_canon::CValue;
use fid_types::CommandId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Format tag of the task list.
pub const FORMAT: &str = "study-tasks@1";
/// Record format of FLOW.STUDY.RECORD that the protocol and the task list cite.
pub const RECORD_FORMAT: &str = "study-record@1";
/// Format tag of a journey map (FLOW.PERSONA.SCHEMA).
pub const MAP_FORMAT: &str = "journey-map@1";
/// Path of the task list as the protocol must cite it.
pub const TASKS_PATH: &str = "acceptance/study/tasks.json";
/// Jobs whose unassisted first-time success the protocol measures: J1, J2 and J3
/// (REQ-FLOW.STUDY.PROTO-003, seed `JOB.USE.02`). Each must have a task.
pub const FIRST_TIME_JOBS: [&str; 3] = ["JOB.ACC.01", "JOB.ACC.02", "JOB.ACC.03"];
/// Largest accepted task list, journey map or persona file, in bytes.
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
/// Largest accepted protocol document, in bytes.
pub const MAX_DOC_BYTES: usize = 256 * 1024;
/// Deepest accepted JSON nesting (object, array, object, array).
pub const MAX_DEPTH: usize = 8;
/// Longest `target_ref`, in bytes.
pub const TARGET_REF_MAX: usize = 48;

const TOP_FIELDS: [&str; 3] = ["format", "record_format", "tasks"];
const TASK_FIELDS: [&str; 6] = ["job_id", "start_command", "end_command", "target_ref", "maps", "personas"];

/// Finding codes. Declared in alphabetical order of their text so the derived order sorts
/// findings by code text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    /// The protocol document is unreadable or does not cite the record format or the task list.
    Doc,
    /// No journey map lists the task's job ID in `exercised_by` (REQ-005).
    NoMap,
    /// A named map or persona does not exist or does not fit the job, or the start and end
    /// commands are not steps (start before end) of a named map.
    Ref,
    /// The task list (or a map or persona file it is checked against) is unreadable, too
    /// large, not JSON, or breaks its schema; a command ID is not `domain.verb_object@N`.
    Schema,
}

impl Code {
    /// Stable text of the code.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Doc => "TASK-DOC",
            Code::NoMap => "TASK-NO-MAP",
            Code::Ref => "TASK-REF",
            Code::Schema => "TASK-SCHEMA",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding: `<CODE> <subject>: <detail>`. The subject is a job ID, a task position such
/// as `tasks[2]`, a map file name, or `-` for the whole file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    fn new(code: Code, subject: &str, detail: impl Into<String>) -> Finding {
        Finding { code, subject: subject.to_string(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}

/// One studied task (spec Design: `StudyTask`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudyTask {
    pub job_id: String,
    pub start_command: String,
    pub end_command: String,
    /// The report package variant the end command exports, e.g. `fai-as9102-report`.
    pub target_ref: String,
    pub maps: Vec<String>,
    pub personas: Vec<String>,
}

/// The parts of a journey map the check needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapInfo {
    pub map_id: String,
    pub personas: BTreeSet<String>,
    pub exercised_by: BTreeSet<String>,
    /// Step commands in step order (steps without a command are skipped).
    pub commands: Vec<String>,
}

/// Result of a check: the tasks that parsed (sorted by job ID) and the sorted findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub tasks: Vec<StudyTask>,
    pub findings: Vec<Finding>,
}

impl Report {
    fn failed(finding: Finding) -> Report {
        Report { tasks: Vec::new(), findings: vec![finding] }
    }

    /// True when there are no findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Whether `s` is a command ID of the form `domain.verb_object@N`: a valid `fid-types`
/// command ID whose name has exactly two non-empty dot-separated parts (AC-1).
pub fn is_domain_command(s: &str) -> bool {
    match CommandId::parse(s) {
        Ok(id) => {
            let parts: Vec<&str> = id.name().split('.').collect();
            parts.len() == 2 && parts.iter().all(|p| !p.is_empty())
        }
        Err(_) => false,
    }
}

/// Whether `s` is an acceptance job ID `JOB.ACC.<two digits>`.
fn is_job_id(s: &str) -> bool {
    match s.strip_prefix("JOB.ACC.") {
        Some(n) => n.len() == 2 && n.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Whether `s` is a `target_ref`: lowercase letters, digits and inner hyphens, 1 to 48 bytes.
fn is_target_ref(s: &str) -> bool {
    let b = s.as_bytes();
    let edge = |c: Option<&u8>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    b.len() <= TARGET_REF_MAX
        && edge(b.first())
        && edge(b.last())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Reads the persona IDs (`personas[*].id`) of a `persona-set@1` file. The rest of the file
/// is the business of `personas-check`.
pub fn parse_persona_ids(input: &[u8]) -> Result<BTreeSet<String>, String> {
    let value = fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH)
        .map_err(|e| format!("persona file is not valid JSON ({e})"))?;
    let records = match &value {
        CValue::Obj(m) => match m.get("personas") {
            Some(CValue::Arr(a)) => a,
            _ => return Err("persona file has no `personas` array".to_string()),
        },
        _ => return Err("persona file is not an object".to_string()),
    };
    let mut ids = BTreeSet::new();
    for (i, rec) in records.iter().enumerate() {
        match rec {
            CValue::Obj(r) => match r.get("id") {
                Some(CValue::Str(id)) => {
                    ids.insert(id.clone());
                }
                _ => return Err(format!("persona file record personas[{i}] has no string `id`")),
            },
            _ => return Err(format!("persona file record personas[{i}] is not an object")),
        }
    }
    Ok(ids)
}

/// Reads the fields the check needs from one `journey-map@1` file.
pub fn parse_map(input: &[u8]) -> Result<MapInfo, String> {
    let value =
        fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH).map_err(|e| format!("not valid JSON ({e})"))?;
    let m = match &value {
        CValue::Obj(m) => m,
        _ => return Err("top level is not an object".to_string()),
    };
    match m.get("format") {
        Some(CValue::Str(s)) if s == MAP_FORMAT => {}
        _ => return Err(format!("`format` is not `{MAP_FORMAT}`")),
    }
    let map_id = match m.get("map_id") {
        Some(CValue::Str(s)) => s.clone(),
        _ => return Err("`map_id` missing or not a string".to_string()),
    };
    let personas = string_set(m.get("personas")).ok_or("`personas` is not an array of strings")?;
    // `exercised_by` is required on persona maps only; other kinds count as exercising nothing.
    let exercised_by = match m.get("exercised_by") {
        None => BTreeSet::new(),
        v => string_set(v).ok_or("`exercised_by` is not an array of strings")?,
    };
    let steps = match m.get("steps") {
        Some(CValue::Arr(a)) => a,
        _ => return Err("`steps` is not an array".to_string()),
    };
    let mut commands = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        match step {
            CValue::Obj(s) => match s.get("command") {
                Some(CValue::Str(c)) => commands.push(c.clone()),
                Some(_) => return Err(format!("steps[{i}].command is not a string")),
                None => {}
            },
            _ => return Err(format!("steps[{i}] is not an object")),
        }
    }
    Ok(MapInfo { map_id, personas, exercised_by, commands })
}

fn string_set(v: Option<&CValue>) -> Option<BTreeSet<String>> {
    match v {
        Some(CValue::Arr(a)) => {
            let mut out = BTreeSet::new();
            for x in a {
                match x {
                    CValue::Str(s) => {
                        out.insert(s.clone());
                    }
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Checks a task list against the given maps (by `map_id`), persona IDs and, when given, the
/// protocol document text.
pub fn check(input: &[u8], maps: &BTreeMap<String, MapInfo>, personas: &BTreeSet<String>, protocol: Option<&str>) -> Report {
    if input.len() > MAX_INPUT_BYTES {
        return Report::failed(Finding::new(Code::Schema, "-", format!("file exceeds {} KiB", MAX_INPUT_BYTES / 1024)));
    }
    let value = match fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH) {
        Ok(v) => v,
        Err(e) => return Report::failed(Finding::new(Code::Schema, "-", format!("not valid JSON ({e})"))),
    };
    let top = match &value {
        CValue::Obj(m) => m,
        _ => return Report::failed(Finding::new(Code::Schema, "-", "top level is not an object")),
    };
    let mut findings = Vec::new();
    for key in top.keys() {
        if !TOP_FIELDS.contains(&key.as_str()) {
            findings.push(Finding::new(Code::Schema, "-", format!("unknown field `{key}`")));
        }
    }
    match top.get("format") {
        Some(CValue::Str(s)) if s == FORMAT => {}
        _ => findings.push(Finding::new(Code::Schema, "-", format!("`format` must be the string `{FORMAT}`"))),
    }
    match top.get("record_format") {
        Some(CValue::Str(s)) if s == RECORD_FORMAT => {}
        _ => findings.push(Finding::new(Code::Schema, "-", format!("`record_format` must be the string `{RECORD_FORMAT}`"))),
    }
    let records: &[CValue] = match top.get("tasks") {
        Some(CValue::Arr(a)) if !a.is_empty() => a.as_slice(),
        _ => {
            findings.push(Finding::new(Code::Schema, "-", "`tasks` must be a non-empty array"));
            &[]
        }
    };

    let mut seen: BTreeMap<String, StudyTask> = BTreeMap::new();
    for (i, rec) in records.iter().enumerate() {
        let at = format!("tasks[{i}]");
        if let Some(t) = check_task(rec, &at, &mut findings) {
            if seen.contains_key(&t.job_id) {
                findings.push(Finding::new(Code::Schema, &t.job_id, format!("second task for this job at {at}")));
                continue;
            }
            check_refs(&t, maps, personas, &mut findings);
            seen.insert(t.job_id.clone(), t);
        }
    }
    if !records.is_empty() {
        for job in FIRST_TIME_JOBS {
            if !seen.contains_key(job) {
                findings.push(Finding::new(Code::Schema, job, "no task for a first-time-success job (J1 to J3)"));
            }
        }
    }
    if let Some(doc) = protocol {
        check_protocol(doc, &mut findings);
    }
    findings.sort();
    findings.dedup();
    Report { tasks: seen.into_values().collect(), findings }
}

/// Checks the shape of one task. Returns it when every field parsed (so references can be
/// checked); findings go to `out`.
fn check_task(rec: &CValue, at: &str, out: &mut Vec<Finding>) -> Option<StudyTask> {
    let m = match rec {
        CValue::Obj(m) => m,
        _ => {
            out.push(Finding::new(Code::Schema, at, "task is not an object"));
            return None;
        }
    };
    let job_id = match m.get("job_id") {
        Some(CValue::Str(s)) if is_job_id(s) => s.clone(),
        Some(CValue::Str(s)) => {
            out.push(Finding::new(Code::Schema, at, format!("job_id `{s}` is not `JOB.ACC.NN`")));
            return None;
        }
        _ => {
            out.push(Finding::new(Code::Schema, at, "`job_id` missing or not a string"));
            return None;
        }
    };
    let who = job_id.as_str();
    let before = out.len();
    for key in m.keys() {
        if !TASK_FIELDS.contains(&key.as_str()) {
            out.push(Finding::new(Code::Schema, who, format!("unknown field `{key}`")));
        }
    }
    let mut command = |key: &str| match m.get(key) {
        Some(CValue::Str(s)) if is_domain_command(s) => s.clone(),
        Some(CValue::Str(s)) => {
            out.push(Finding::new(Code::Schema, who, format!("{key} `{s}` is not of the form `domain.verb_object@N`")));
            String::new()
        }
        _ => {
            out.push(Finding::new(Code::Schema, who, format!("`{key}` missing or not a string")));
            String::new()
        }
    };
    let start_command = command("start_command");
    let end_command = command("end_command");
    if !start_command.is_empty() && start_command == end_command {
        out.push(Finding::new(Code::Schema, who, "start_command and end_command are the same"));
    }
    let target_ref = match m.get("target_ref") {
        Some(CValue::Str(s)) if is_target_ref(s) => s.clone(),
        _ => {
            out.push(Finding::new(
                Code::Schema,
                who,
                format!("`target_ref` must be 1 to {TARGET_REF_MAX} lowercase letters, digits and hyphens"),
            ));
            String::new()
        }
    };
    let mut list = |key: &str| match m.get(key) {
        Some(CValue::Arr(a)) if !a.is_empty() => {
            let mut v = Vec::new();
            for x in a {
                match x {
                    CValue::Str(s) if !s.is_empty() => v.push(s.clone()),
                    _ => {
                        out.push(Finding::new(Code::Schema, who, format!("`{key}` holds a non-string or empty item")));
                        return Vec::new();
                    }
                }
            }
            v
        }
        _ => {
            out.push(Finding::new(Code::Schema, who, format!("`{key}` must be a non-empty array of strings")));
            Vec::new()
        }
    };
    let maps = list("maps");
    let personas = list("personas");
    let task = StudyTask { job_id, start_command, end_command, target_ref, maps, personas };
    if out.len() == before {
        Some(task)
    } else {
        // The task still counts as present for the J1..J3 rule, but its references are not
        // checked: return it with empty lists so `check_refs` only checks the job's maps.
        Some(StudyTask { maps: Vec::new(), personas: Vec::new(), start_command: String::new(), ..task })
    }
}

/// Checks the task's job, maps, personas and commands against the maps and persona IDs.
fn check_refs(t: &StudyTask, maps: &BTreeMap<String, MapInfo>, personas: &BTreeSet<String>, out: &mut Vec<Finding>) {
    let who = t.job_id.as_str();
    // REQ-005: the job has at least one journey map.
    if !maps.values().any(|m| m.exercised_by.contains(&t.job_id)) {
        out.push(Finding::new(Code::NoMap, who, "no journey map lists this job in `exercised_by`"));
    }
    let mut named: Vec<&MapInfo> = Vec::new();
    for id in &t.maps {
        match maps.get(id) {
            None => out.push(Finding::new(Code::Ref, who, format!("map `{id}` does not exist"))),
            Some(m) if !m.exercised_by.contains(&t.job_id) => {
                out.push(Finding::new(Code::Ref, who, format!("map `{id}` does not list this job in `exercised_by`")))
            }
            Some(m) => named.push(m),
        }
    }
    for p in &t.personas {
        if !personas.contains(p) {
            out.push(Finding::new(Code::Ref, who, format!("persona `{p}` is not in the persona file")));
        } else if !named.iter().any(|m| m.personas.contains(p)) {
            out.push(Finding::new(Code::Ref, who, format!("persona `{p}` is not a persona of the named maps")));
        }
    }
    if !t.start_command.is_empty() && !t.end_command.is_empty() && !named.is_empty() {
        let in_order = named.iter().any(|m| {
            m.commands.iter().position(|c| *c == t.start_command).is_some_and(|i| {
                m.commands.iter().skip(i + 1).any(|c| *c == t.end_command)
            })
        });
        if !in_order {
            out.push(Finding::new(
                Code::Ref,
                who,
                format!(
                    "no named map has step `{}` followed later by step `{}`",
                    t.start_command, t.end_command
                ),
            ));
        }
    }
}

/// AC-3: the protocol cites the record format of FLOW.STUDY.RECORD, the task list format and
/// the task list path.
fn check_protocol(doc: &str, out: &mut Vec<Finding>) {
    for needle in [RECORD_FORMAT, FORMAT, TASKS_PATH] {
        if !doc.contains(needle) {
            out.push(Finding::new(Code::Doc, "protocol", format!("does not cite `{needle}`")));
        }
    }
}

/// Reads every `*.json` journey map in `dir`, keyed by `map_id`. Unreadable or malformed
/// maps and duplicate map IDs become `TASK-SCHEMA` findings named after the file.
pub fn read_maps(dir: &Path, out: &mut Vec<Finding>) -> BTreeMap<String, MapInfo> {
    let mut maps = BTreeMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            out.push(Finding::new(Code::Schema, "-", format!("cannot read map directory {}: {e}", dir.display())));
            return maps;
        }
    };
    let mut files: Vec<(String, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(|n| (n.to_string(), p.clone())))
        .collect();
    files.sort();
    for (name, path) in files {
        let parsed = read_bounded(&path, MAX_INPUT_BYTES).map_err(|e| format!("cannot read: {e}")).and_then(|b| {
            if b.len() > MAX_INPUT_BYTES {
                Err(format!("exceeds {} KiB", MAX_INPUT_BYTES / 1024))
            } else {
                parse_map(&b)
            }
        });
        match parsed {
            Ok(m) => {
                if maps.contains_key(&m.map_id) {
                    out.push(Finding::new(Code::Schema, &name, format!("second map with map_id `{}`", m.map_id)));
                } else {
                    maps.insert(m.map_id.clone(), m);
                }
            }
            Err(e) => out.push(Finding::new(Code::Schema, &name, format!("journey map: {e}"))),
        }
    }
    maps
}

/// Reads all inputs and checks them. `protocol: None` skips the document check.
pub fn check_files(tasks: &Path, maps_dir: &Path, personas: &Path, protocol: Option<&Path>) -> Report {
    let mut pre = Vec::new();
    let maps = read_maps(maps_dir, &mut pre);
    let persona_ids = match read_bounded(personas, MAX_INPUT_BYTES) {
        Ok(b) => match parse_persona_ids(&b) {
            Ok(ids) => ids,
            Err(e) => {
                pre.push(Finding::new(Code::Schema, "-", e));
                BTreeSet::new()
            }
        },
        Err(e) => {
            pre.push(Finding::new(Code::Schema, "-", format!("cannot read {}: {e}", personas.display())));
            BTreeSet::new()
        }
    };
    let doc = match protocol {
        None => None,
        Some(p) => match read_bounded(p, MAX_DOC_BYTES) {
            Ok(b) if b.len() <= MAX_DOC_BYTES => match String::from_utf8(b) {
                Ok(s) => Some(s),
                Err(_) => {
                    pre.push(Finding::new(Code::Doc, "protocol", "not UTF-8"));
                    None
                }
            },
            Ok(_) => {
                pre.push(Finding::new(Code::Doc, "protocol", format!("exceeds {} KiB", MAX_DOC_BYTES / 1024)));
                None
            }
            Err(e) => {
                pre.push(Finding::new(Code::Doc, "protocol", format!("cannot read {}: {e}", p.display())));
                None
            }
        },
    };
    let mut report = match read_bounded(tasks, MAX_INPUT_BYTES) {
        Ok(bytes) => check(&bytes, &maps, &persona_ids, doc.as_deref()),
        Err(e) => Report::failed(Finding::new(Code::Schema, "-", format!("cannot read {}: {e}", tasks.display()))),
    };
    report.findings.extend(pre);
    report.findings.sort();
    report.findings.dedup();
    report
}

/// Reads at most `limit + 1` bytes, so an oversized file is detected without reading it all.
fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    file.take(cap).read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_order_is_text_order() {
        let codes = [Code::Doc, Code::NoMap, Code::Ref, Code::Schema];
        let mut texts: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
        let before = texts.clone();
        texts.sort_unstable();
        assert_eq!(texts, before);
    }

    #[test]
    fn command_form() {
        assert!(is_domain_command("report.export_pdf@1"));
        assert!(is_domain_command("verify.package@1"));
        assert!(!is_domain_command("report.export_pdf"));
        assert!(!is_domain_command("export_pdf@1"));
        assert!(!is_domain_command("a.b.c@1"));
        assert!(!is_domain_command("Report.export_pdf@1"));
    }
}
