//! Suite layout, loading and the suite version (REQ-AGENT.EVAL.FORMAT-005, -006).

use crate::json::{is_id, parse_bounded, Ctx};
use crate::oracle::{parse_oracle, Oracle};
use crate::task::{parse_task, Task};
use crate::{file_digest, ErrorCode, FormatError, MAX_FILE_BYTES};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The `format` value of `suite.json`.
pub const SUITE_FORMAT: &str = "fid-eval-suite@1";

/// One task with its oracle.
#[derive(Debug, Clone, PartialEq)]
pub struct SuiteTask {
    pub task: Task,
    pub oracle: Oracle,
}

/// A loaded suite. `files` are the hashed files, sorted by suite-relative path.
#[derive(Debug, Clone, PartialEq)]
pub struct Suite {
    pub name: String,
    pub tasks: Vec<SuiteTask>,
    pub files: Vec<(String, Digest)>,
    pub version: Digest,
}

/// Reads a file of at most [`MAX_FILE_BYTES`]; `name` is used in errors.
fn read_bounded(path: &Path, name: &str) -> Result<Vec<u8>, FormatError> {
    let missing = |d: String| FormatError::new(ErrorCode::MissingFile, name, "", d);
    let meta = std::fs::symlink_metadata(path).map_err(|e| missing(format!("cannot read: {e}")))?;
    if !meta.is_file() {
        return Err(missing("not a regular file".to_string()));
    }
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(FormatError::new(ErrorCode::InputLimit, name, "", "file larger than 64 MiB"));
    }
    let f = std::fs::File::open(path).map_err(|e| missing(format!("cannot read: {e}")))?;
    let mut buf = Vec::new();
    f.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut buf).map_err(|e| missing(format!("cannot read: {e}")))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(FormatError::new(ErrorCode::InputLimit, name, "", "file larger than 64 MiB"));
    }
    Ok(buf)
}

/// Reads and parses one task file.
pub fn read_task(path: &Path) -> Result<Task, Vec<FormatError>> {
    let name = path.to_string_lossy();
    parse_task(&read_bounded(path, &name).map_err(|e| vec![e])?, &name)
}

/// Reads and parses one oracle file.
pub fn read_oracle(path: &Path) -> Result<Oracle, Vec<FormatError>> {
    let name = path.to_string_lossy();
    parse_oracle(&read_bounded(path, &name).map_err(|e| vec![e])?, &name)
}

/// The suite version: `sha256:` over the RFC 8785 canonical JSON of the
/// `[relative path, file digest]` pairs sorted by path.
pub fn suite_version(files: &[(String, Digest)]) -> Result<Digest, FormatError> {
    let mut sorted: Vec<&(String, Digest)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let list = CValue::Arr(
        sorted
            .into_iter()
            .map(|(p, d)| CValue::Arr(vec![CValue::Str(p.clone()), CValue::Str(d.as_str().to_string())]))
            .collect(),
    );
    let d = fid_canon::digest(&list)
        .map_err(|e| FormatError::new(ErrorCode::InvalidSuite, "", "", format!("cannot canonicalise: {e}")))?;
    Ok(Digest::from_bytes(d.0))
}

/// Sorted entry names of a directory; non-UTF-8 names are layout errors.
fn list_dir(path: &Path, name: &str, errs: &mut Vec<FormatError>) -> Option<Vec<String>> {
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => {
            errs.push(FormatError::new(ErrorCode::MissingFile, name, "", format!("cannot list directory: {e}")));
            return None;
        }
    };
    let mut out = Vec::new();
    for entry in rd {
        match entry {
            Ok(e) => match e.file_name().into_string() {
                Ok(s) => out.push(s),
                Err(raw) => errs.push(FormatError::new(
                    ErrorCode::InvalidSuite,
                    name,
                    "",
                    format!("entry name is not UTF-8: {}", raw.to_string_lossy()),
                )),
            },
            Err(e) => errs.push(FormatError::new(ErrorCode::MissingFile, name, "", format!("cannot list: {e}"))),
        }
    }
    out.sort();
    Some(out)
}

/// Every regular file under `dir`, as `/`-joined paths relative to it. Symlinks and
/// trees deeper than 64 levels are layout errors (fail closed).
fn walk(dir: &Path, name: &str, errs: &mut Vec<FormatError>) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(dir.to_path_buf(), String::new(), 0)];
    while let Some((path, rel, depth)) = stack.pop() {
        let shown = if rel.is_empty() { name.to_string() } else { format!("{name}/{rel}") };
        if depth > 64 {
            errs.push(FormatError::new(ErrorCode::InvalidSuite, &shown, "", "directory tree deeper than 64"));
            continue;
        }
        let Some(entries) = list_dir(&path, &shown, errs) else { continue };
        for e in entries {
            let child = path.join(&e);
            let child_rel = if rel.is_empty() { e.clone() } else { format!("{rel}/{e}") };
            match std::fs::symlink_metadata(&child) {
                Ok(m) if m.is_dir() => stack.push((child, child_rel, depth + 1)),
                Ok(m) if m.is_file() => files.push(child_rel),
                Ok(_) => errs.push(FormatError::new(
                    ErrorCode::InvalidSuite,
                    &format!("{name}/{child_rel}"),
                    "",
                    "symbolic links and special files are not allowed",
                )),
                Err(err) => errs.push(FormatError::new(
                    ErrorCode::MissingFile,
                    &format!("{name}/{child_rel}"),
                    "",
                    format!("cannot read: {err}"),
                )),
            }
        }
    }
    files.sort();
    files
}

fn read_header(dir: &Path, errs: &mut Vec<FormatError>) -> Option<String> {
    let file = "suite.json";
    let bytes = match read_bounded(&dir.join(file), file) {
        Ok(b) => b,
        Err(e) => {
            errs.push(e);
            return None;
        }
    };
    let root = match parse_bounded(&bytes, file, ErrorCode::InvalidSuite) {
        Ok(v) => v,
        Err(e) => {
            errs.push(e);
            return None;
        }
    };
    let mut cx = Ctx::new(ErrorCode::InvalidSuite, file);
    let keys = &["format", "name"];
    let Some(m) = cx.obj(Some(&root), "", keys, keys) else {
        errs.extend(cx.finish());
        return None;
    };
    if let Some(f) = cx.string(m.get("format"), "format") {
        if f != SUITE_FORMAT {
            cx.err("format", format!("expected {SUITE_FORMAT}"));
        }
    }
    let name = cx.string(m.get("name"), "name").map(str::to_string);
    if let Some(n) = &name {
        if !is_id(n) {
            cx.err("name", "expected [A-Za-z0-9][A-Za-z0-9._-]{0,63}");
        }
    }
    if cx.has_errors() {
        errs.extend(cx.finish());
        return None;
    }
    name
}

/// Loads a suite directory fail-closed and computes its version. All errors found in
/// the layout and in every file are returned together.
pub fn load_suite(dir: &Path) -> Result<Suite, Vec<FormatError>> {
    let mut errs = Vec::new();
    let name = read_header(dir, &mut errs);
    let mut files: Vec<(String, Digest)> = Vec::new();
    let mut tasks: BTreeMap<String, Task> = BTreeMap::new();

    let task_ids = list_dir(&dir.join("tasks"), "tasks", &mut errs).unwrap_or_default();
    for id in &task_ids {
        let tdir = dir.join("tasks").join(id);
        let tname = format!("tasks/{id}");
        if !is_id(id) || !std::fs::symlink_metadata(&tdir).map(|m| m.is_dir()).unwrap_or(false) {
            errs.push(FormatError::new(ErrorCode::InvalidSuite, &tname, "", "expected a task directory named by ID"));
            continue;
        }
        let present = walk(&tdir, &tname, &mut errs);
        let tfile = format!("{tname}/task.json");
        let task = match read_bounded(&tdir.join("task.json"), &tfile) {
            Ok(b) => {
                files.push((tfile.clone(), file_digest(&b)));
                match parse_task(&b, &tfile) {
                    Ok(t) => t,
                    Err(e) => {
                        errs.extend(e);
                        continue;
                    }
                }
            }
            Err(e) => {
                errs.push(e);
                continue;
            }
        };
        if &task.id != id {
            errs.push(FormatError::new(ErrorCode::InvalidTask, &tfile, "id", "must equal the task directory name"));
        }
        let spath = task.start_state.path.as_str();
        let sfile = format!("{tname}/{spath}");
        match read_bounded(&tdir.join(spath), &sfile) {
            Ok(b) => {
                let d = file_digest(&b);
                if d != task.start_state.digest {
                    errs.push(FormatError::new(
                        ErrorCode::InvalidTask,
                        &tfile,
                        "start_state.digest",
                        format!("archive {sfile} has digest {d}"),
                    ));
                }
                files.push((sfile, d));
            }
            Err(e) => errs.push(e),
        }
        for extra in present.iter().filter(|p| p.as_str() != "task.json" && p.as_str() != spath) {
            errs.push(FormatError::new(
                ErrorCode::InvalidSuite,
                &format!("{tname}/{extra}"),
                "",
                "file is neither task.json nor the start-state archive, so the suite version would miss it",
            ));
        }
        tasks.insert(id.clone(), task);
    }

    let mut oracles: BTreeMap<String, Oracle> = BTreeMap::new();
    let oracle_ids = list_dir(&dir.join("oracles"), "oracles", &mut errs).unwrap_or_default();
    for id in oracle_ids.iter().cloned() {
        let odir = dir.join("oracles").join(&id);
        let oname = format!("oracles/{id}");
        if !task_ids.contains(&id) {
            errs.push(FormatError::new(ErrorCode::InvalidSuite, &oname, "", "no task directory with this ID"));
            continue;
        }
        let present = walk(&odir, &oname, &mut errs);
        for extra in present.iter().filter(|p| p.as_str() != "oracle.json") {
            errs.push(FormatError::new(
                ErrorCode::InvalidSuite,
                &format!("{oname}/{extra}"),
                "",
                "file is not oracle.json, so the suite version would miss it",
            ));
        }
        let ofile = format!("{oname}/oracle.json");
        match read_bounded(&odir.join("oracle.json"), &ofile) {
            Ok(b) => {
                files.push((ofile.clone(), file_digest(&b)));
                match parse_oracle(&b, &ofile) {
                    Ok(o) => {
                        oracles.insert(id, o);
                    }
                    Err(e) => errs.extend(e),
                }
            }
            Err(e) => errs.push(e),
        }
    }
    for id in &task_ids {
        if is_id(id) && !oracle_ids.contains(id) {
            errs.push(FormatError::new(ErrorCode::MissingFile, &format!("oracles/{id}/oracle.json"), "", "no oracle"));
        }
    }
    if task_ids.is_empty() && errs.is_empty() {
        errs.push(FormatError::new(ErrorCode::InvalidSuite, "tasks", "", "a suite needs at least one task"));
    }

    let version = match suite_version(&files) {
        Ok(v) => Some(v),
        Err(e) => {
            errs.push(e);
            None
        }
    };
    let (Some(name), Some(version), true) = (name, version, errs.is_empty()) else {
        if errs.is_empty() {
            errs.push(FormatError::new(ErrorCode::InvalidSuite, "", "", "incomplete suite"));
        }
        return Err(errs);
    };
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (id, task) in tasks {
        match oracles.remove(&id) {
            Some(oracle) => out.push(SuiteTask { task, oracle }),
            None => return Err(vec![FormatError::new(ErrorCode::MissingFile, &format!("oracles/{id}"), "", "no oracle")]),
        }
    }
    Ok(Suite { name, tasks: out, files, version })
}
