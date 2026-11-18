//! Shell-variant coverage of a shell suite over its base suites (spec node
//! AGENT.REDTEAM.SHELL, REQ-AGENT.REDTEAM.SHELL-001).
//!
//! Every task of a base suite (the injection, escalation, self-adoption and fabrication
//! families) must have exactly one shell variant (`variant_of` naming it) that differs from it
//! only in requiring a shell: same prompt, source job, golden digest, profile, budget, roots,
//! tier, apps, workflows, payload vectors and oracle, and the same predicates except that
//! every `entry_with_marker` marker is fresh (used by no base task and no other shell task).
//! Licence entries, payload files and the start-state archive's files (names and contents)
//! must equal the base task's once each base marker is replaced by the variant's marker at the
//! same predicate position.

use crate::oracle::Oracle;
use crate::redteam::{Family, PredicateKind};
use crate::suite::Suite;
use crate::task::Task;
use crate::{ErrorCode, FormatError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Counts of a checked shell suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantCoverage {
    /// Tasks of the base suites (each has exactly one variant).
    pub base_tasks: usize,
    /// Shell variants (tasks with `variant_of`).
    pub variants: usize,
    /// Shell-only tasks.
    pub shell_only: usize,
}

fn task_file(id: &str) -> String {
    format!("tasks/{id}/task.json")
}

fn markers(t: &Task) -> Vec<&str> {
    t.predicates
        .iter()
        .filter_map(|p| match &p.kind {
            PredicateKind::EntryWithMarker { marker } => Some(marker.as_str()),
            _ => None,
        })
        .collect()
}

/// Base marker to variant marker, pairing the `entry_with_marker` predicates by position.
type Mapping = Vec<(String, String)>;

fn mapping(v: &Task, b: &Task) -> Mapping {
    markers(b).into_iter().zip(markers(v)).map(|(o, n)| (o.to_string(), n.to_string())).collect()
}

fn map_str(s: &str, m: &Mapping) -> String {
    m.iter().fold(s.to_string(), |acc, (o, n)| acc.replace(o.as_str(), n))
}

fn replace_bytes(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return hay.to_vec();
    }
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay.get(i..i + from.len()) == Some(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.extend(hay.get(i).copied());
            i += 1;
        }
    }
    out
}

fn map_bytes(b: &[u8], m: &Mapping) -> Vec<u8> {
    m.iter().fold(b.to_vec(), |acc, (o, n)| replace_bytes(&acc, o.as_bytes(), n.as_bytes()))
}

/// A NUL-terminated header field as text.
fn field(h: &[u8], from: usize, to: usize) -> Result<String, String> {
    let f = h.get(from..to).ok_or("truncated header")?;
    let end = f.iter().position(|c| *c == 0).unwrap_or(f.len());
    String::from_utf8(f.get(..end).unwrap_or_default().to_vec()).map_err(|_| "header field is not UTF-8".to_string())
}

/// The regular files of a ustar archive, by path. Anything else (links, directories, devices,
/// truncation) is an error (fail closed).
fn untar(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut out = BTreeMap::new();
    let mut pos = 0usize;
    loop {
        let h = bytes.get(pos..pos.saturating_add(512)).ok_or("truncated archive")?;
        if h.iter().all(|b| *b == 0) {
            return Ok(out);
        }
        if h.get(257..262) != Some(b"ustar".as_slice()) {
            return Err(format!("no ustar header at byte {pos}"));
        }
        let (name, prefix) = (field(h, 0, 100)?, field(h, 345, 500)?);
        let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let size = usize::from_str_radix(field(h, 124, 136)?.trim(), 8).map_err(|_| "bad size field".to_string())?;
        if !matches!(h.get(156), Some(b'0') | Some(0)) {
            return Err(format!("{path}: not a regular file"));
        }
        let start = pos.saturating_add(512);
        let data = bytes.get(start..start.saturating_add(size)).ok_or("truncated archive")?;
        if out.insert(path.clone(), data.to_vec()).is_some() {
            return Err(format!("{path}: repeated in the archive"));
        }
        pos = start.saturating_add(size.div_ceil(512).saturating_mul(512));
    }
}

fn start_files(dir: &Path, t: &Task) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let p = dir.join("tasks").join(&t.id).join(t.start_state.path.as_str());
    let bytes = std::fs::read(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    untar(&bytes)
}

/// The fields that must be equal between a variant and its base task: `None` if they are, or
/// the name of the first field that differs.
fn first_difference(v: &Task, b: &Task) -> Option<&'static str> {
    if v.prompt != b.prompt {
        return Some("prompt");
    }
    if v.source_job != b.source_job {
        return Some("source_job");
    }
    if v.golden_digest != b.golden_digest {
        return Some("golden_digest");
    }
    let m = mapping(v, b);
    let licences_equal = v.licences.len() == b.licences.len()
        && v.licences.iter().zip(&b.licences).all(|(lv, lb)| {
            lv.spdx == lb.spdx && lv.file.as_str() == map_str(lb.file.as_str(), &m)
        });
    if !licences_equal {
        return Some("licences");
    }
    let payloads_equal = v.payloads.len() == b.payloads.len()
        && v.payloads.iter().zip(&b.payloads).all(|(pv, pb)| pv.file.as_str() == map_str(pb.file.as_str(), &m));
    if !payloads_equal {
        return Some("payloads");
    }
    if v.profile != b.profile {
        return Some("profile");
    }
    if v.budget != b.budget {
        return Some("budget");
    }
    if v.roots != b.roots {
        return Some("roots");
    }
    if v.tier != b.tier {
        return Some("tier");
    }
    if v.apps != b.apps {
        return Some("apps");
    }
    if v.workflows != b.workflows {
        return Some("workflows");
    }
    let vectors = |t: &Task| t.payloads.iter().map(|p| p.vector).collect::<Vec<_>>();
    if vectors(v) != vectors(b) {
        return Some("payloads");
    }
    if v.predicates.len() != b.predicates.len() {
        return Some("predicates");
    }
    for (pv, pb) in v.predicates.iter().zip(&b.predicates) {
        let same = pv.id == pb.id
            && match (&pv.kind, &pb.kind) {
                // Same kind; the marker itself must be fresh (checked separately).
                (PredicateKind::EntryWithMarker { .. }, PredicateKind::EntryWithMarker { .. }) => true,
                (kv, kb) => kv == kb,
            };
        if !same {
            return Some("predicates");
        }
    }
    None
}

/// Checks a shell suite against its base suites. Every task of `shell` must be of family
/// `shell`; every base task must be a red-team task of another family with a unique ID across
/// the bases. All problems are returned together as `invalid_suite` errors; `file` is
/// suite-relative and prefixed with the suite name for base tasks. The directories are the
/// suites' folders, read for the start-state archives.
pub fn check_variants(
    shell_dir: &Path,
    shell: &Suite,
    bases: &[(&Path, &Suite)],
) -> Result<VariantCoverage, Vec<FormatError>> {
    let mut errs = Vec::new();
    let mut err = |file: String, field: &str, detail: String| {
        errs.push(FormatError::new(ErrorCode::InvalidSuite, &file, field, detail));
    };

    // Base tasks by ID, and every base marker.
    let mut base: BTreeMap<&str, (&Suite, &Task, &Oracle, &Path)> = BTreeMap::new();
    let mut base_markers: BTreeSet<&str> = BTreeSet::new();
    for (dir, s) in bases {
        for st in &s.tasks {
            let t = &st.task;
            let file = format!("{}/{}", s.name, task_file(&t.id));
            if matches!(t.family, Some(Family::Shell) | None) {
                err(file, "family", "a base task must be a red-team task of a family other than shell".into());
                continue;
            }
            if base.insert(&t.id, (*s, t, &st.oracle, *dir)).is_some() {
                err(file, "id", "task ID repeated across the base suites".into());
            }
            base_markers.extend(markers(t));
        }
    }

    let mut variants_of: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut shell_markers: BTreeMap<&str, &str> = BTreeMap::new();
    let mut cov = VariantCoverage { base_tasks: base.len(), variants: 0, shell_only: 0 };
    for st in &shell.tasks {
        let t = &st.task;
        let file = task_file(&t.id);
        if t.family != Some(Family::Shell) {
            err(file, "family", "a shell suite holds only tasks of family shell".into());
            continue;
        }
        for (i, m) in markers(t).into_iter().enumerate() {
            if base_markers.contains(m) {
                err(
                    file.clone(),
                    "predicates",
                    format!("marker {m} is used by a base task; a shell task needs a fresh marker"),
                );
            }
            match shell_markers.insert(m, &t.id) {
                Some(other) if other != t.id => {
                    err(file.clone(), "predicates", format!("marker {m} is also used by shell task {other}"))
                }
                Some(_) => err(file.clone(), "predicates", format!("marker {m} repeated (entry_with_marker #{i})")),
                None => {}
            }
        }
        if t.shell_only {
            cov.shell_only += 1;
            continue;
        }
        let Some(of) = t.variant_of.as_deref() else {
            err(file, "variant_of", "a shell task needs variant_of or shell_only".into());
            continue;
        };
        cov.variants += 1;
        variants_of.entry(of).or_default().push(&t.id);
        match base.get(of) {
            None => err(file, "variant_of", format!("{of} is not a task of the base suites")),
            Some((_, b, oracle, bdir)) => {
                if let Some(field) = first_difference(t, b) {
                    err(
                        file.clone(),
                        field,
                        format!("differs from base task {of}; a variant differs only in the shell"),
                    );
                }
                let m = mapping(t, b);
                match (start_files(shell_dir, t), start_files(bdir, b)) {
                    (Ok(vf), Ok(bf)) => {
                        let mapped: BTreeMap<String, Vec<u8>> =
                            bf.iter().map(|(n, d)| (map_str(n, &m), map_bytes(d, &m))).collect();
                        if vf != mapped {
                            err(
                                file.clone(),
                                "start_state",
                                format!("start-state files differ from base task {of} beyond the marker change"),
                            );
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => err(file.clone(), "start_state", e),
                }
                if &st.oracle != *oracle {
                    err(file, "", format!("oracle differs from the oracle of base task {of}"));
                }
            }
        }
    }
    for (id, (s, _, _, _)) in &base {
        let file = format!("{}/{}", s.name, task_file(id));
        match variants_of.get(id).map(Vec::as_slice) {
            None | Some([]) => err(file, "", "no shell variant".into()),
            Some([_]) => {}
            Some(many) => err(file, "", format!("{} shell variants: {}", many.len(), many.join(", "))),
        }
    }
    if errs.is_empty() {
        Ok(cov)
    } else {
        Err(errs)
    }
}
