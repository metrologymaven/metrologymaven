//! Suite-wide marker rules of spec node AGENT.REDTEAM.PRED (REQ-AGENT.REDTEAM.PRED-003 to
//! -005), applied by [`crate::load_suite`] and so by `fid-eval suite check`.
//!
//! The predicate schema itself (the six kinds, the marker format, `missing_predicate` and
//! `unknown_predicate`) is parsed per task in `redteam.rs`. The rules here need the whole
//! suite and the payload files inside the start-state archives:
//!
//! - `marker_not_in_payload`: an `entry_with_marker` marker that appears in none of the
//!   task's payload files. A payload file is searched in its archive path (vector
//!   `file_name` carries the marker in the name) and in its bytes. A task without payloads,
//!   a payload file missing from the archive, or an archive that cannot be listed
//!   fail-closed all count as "appears in none".
//! - `marker_in_prompt`: the marker appears in the task prompt, compared ASCII
//!   case-insensitively (Crockford base32 decodes lower case to the same value).
//! - `duplicate_marker`: a marker declared by two `entry_with_marker` predicates of the
//!   suite, or a declared marker that appears in a payload file of another task (two
//!   payloads sharing a marker would make a hit untraceable to its source).
//!
//! Every error is reported on the declaring task's `task.json`, so it names the task.

use crate::redteam::{is_marker, PredicateKind};
use crate::task::Task;
use crate::{tar, ErrorCode, FormatError};
use std::collections::{BTreeMap, BTreeSet};

const PREFIX: &[u8] = b"FIDRT-";
/// `FIDRT-` plus 26 characters.
const MARKER_LEN: usize = 32;

/// Markers found in a task's payload files, or why the archive could not be listed.
pub(crate) type Found = Result<BTreeSet<String>, String>;

/// Every marker-shaped string (`FIDRT-` plus 26 Crockford base32 characters) in `bytes`.
/// A longer run of Crockford characters still contains the marker of its first 26.
fn markers_in(bytes: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i + MARKER_LEN <= bytes.len() {
        if bytes[i..].starts_with(PREFIX) {
            if let Ok(s) = std::str::from_utf8(&bytes[i..i + MARKER_LEN]) {
                if is_marker(s) {
                    out.insert(s.to_string());
                    i += MARKER_LEN;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// The markers in the payload files of `task`, read from its start-state archive bytes.
/// A task without payloads has none, and its archive is not listed.
pub(crate) fn payload_markers(task: &Task, archive: &[u8]) -> Found {
    let mut out = BTreeSet::new();
    if task.payloads.is_empty() {
        return Ok(out);
    }
    let files = tar::contents(archive)?;
    for p in &task.payloads {
        for (path, data) in files.iter().filter(|(path, _)| path == p.file.as_str()) {
            out.extend(markers_in(path.as_bytes()));
            out.extend(markers_in(data));
        }
    }
    Ok(out)
}

/// The `entry_with_marker` markers of a task with their predicate index.
fn declared(task: &Task) -> impl Iterator<Item = (usize, &str)> {
    task.predicates.iter().enumerate().filter_map(|(i, p)| match &p.kind {
        PredicateKind::EntryWithMarker { marker } => Some((i, marker.as_str())),
        _ => None,
    })
}

/// Applies the marker rules to the parsed tasks of a suite (`tasks` by ID) and the
/// markers found in their payload files (`found` by task ID; a task missing from it has
/// no readable archive). Errors are in task order, then predicate order.
pub(crate) fn check(tasks: &BTreeMap<String, Task>, found: &BTreeMap<String, Found>) -> Vec<FormatError> {
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, t) in tasks {
        for (_, m) in declared(t) {
            owners.entry(m).or_default().push(id.as_str());
        }
    }
    let mut errs = Vec::new();
    for (id, t) in tasks {
        let file = format!("tasks/{id}/task.json");
        let prompt = t.prompt.to_ascii_uppercase();
        for (i, m) in declared(t) {
            let field = format!("predicates[{i}].marker");
            if prompt.contains(m) {
                errs.push(FormatError::new(
                    ErrorCode::MarkerInPrompt,
                    &file,
                    "prompt",
                    format!("task {id}: marker {m} of predicate {} appears in the prompt", t.predicates[i].id),
                ));
            }
            let not_in = |why: String| FormatError::new(ErrorCode::MarkerNotInPayload, &file, &field, why);
            match found.get(id) {
                Some(Ok(set)) if set.contains(m) => {}
                Some(Err(e)) => errs.push(not_in(format!("task {id}: start-state archive cannot be listed: {e}"))),
                _ if t.payloads.is_empty() => {
                    errs.push(not_in(format!("task {id}: marker {m} appears in no payload file (the task has none)")))
                }
                _ => errs.push(not_in(format!("task {id}: marker {m} appears in none of its payload files"))),
            }
            let owner_list = owners.get(m).map(Vec::as_slice).unwrap_or(&[]);
            let mut others: Vec<&str> = owner_list.iter().copied().filter(|o| *o != id.as_str()).collect();
            let repeats_here = owner_list.iter().filter(|o| **o == id.as_str()).count() > 1;
            if repeats_here || !others.is_empty() {
                if repeats_here {
                    others.insert(0, id.as_str());
                }
                others.dedup();
                errs.push(FormatError::new(
                    ErrorCode::DuplicateMarker,
                    &file,
                    &field,
                    format!("task {id}: marker {m} is also declared by task(s) {}", others.join(", ")),
                ));
            }
            let leaked: Vec<&str> = found
                .iter()
                .filter(|(o, _)| o.as_str() != id.as_str() && !owner_list.contains(&o.as_str()))
                .filter(|(_, f)| matches!(f, Ok(set) if set.contains(m)))
                .map(|(o, _)| o.as_str())
                .collect();
            if !leaked.is_empty() {
                errs.push(FormatError::new(
                    ErrorCode::DuplicateMarker,
                    &file,
                    &field,
                    format!("task {id}: marker {m} also appears in a payload file of task(s) {}", leaked.join(", ")),
                ));
            }
        }
    }
    errs
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const M: &str = "FIDRT-0123456789ABCDEFGHJKMNPQRS";

    #[test]
    fn scan_finds_markers_by_the_spec_shape() {
        let set = |b: &[u8]| markers_in(b).into_iter().collect::<Vec<_>>();
        assert_eq!(set(format!("x {M} y").as_bytes()), vec![M.to_string()]);
        assert_eq!(set(M.as_bytes()), vec![M.to_string()]);
        // Longer Crockford runs contain the marker of their first 26 characters.
        assert_eq!(set(format!("{M}TV").as_bytes()), vec![M.to_string()]);
        // Too short, a non-Crockford letter, lower case or the wrong prefix: no marker.
        assert!(set(&M.as_bytes()[..31]).is_empty());
        assert!(set(M.replace('S', "U").as_bytes()).is_empty());
        assert!(set(M.to_ascii_lowercase().as_bytes()).is_empty());
        assert!(set(M.replace("FIDRT", "FIDRX").as_bytes()).is_empty());
        // Two markers, one after the other.
        let two = format!("{M}FIDRT-00000000000000000000000000");
        assert_eq!(set(two.as_bytes()).len(), 2);
        // Bytes that are not UTF-8 around a marker do not hide it.
        let mut b = vec![0xff, 0xfe];
        b.extend_from_slice(M.as_bytes());
        b.push(0x80);
        assert_eq!(set(&b), vec![M.to_string()]);
    }
}
