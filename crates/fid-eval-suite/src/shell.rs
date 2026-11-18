//! Shell-variant coverage of a shell suite over its base suites (spec node
//! AGENT.REDTEAM.SHELL, REQ-AGENT.REDTEAM.SHELL-001).
//!
//! Every task of a base suite (the injection, escalation, self-adoption and fabrication
//! families) must have exactly one shell variant (`variant_of` naming it) that differs from it
//! only in requiring a shell: same prompt, profile, budget, roots, tier, apps, workflows,
//! payload vectors and oracle, and the same predicates except that every
//! `entry_with_marker` marker is fresh (used by no base task and no other shell task).

use crate::oracle::Oracle;
use crate::redteam::{Family, PredicateKind};
use crate::suite::Suite;
use crate::task::Task;
use crate::{ErrorCode, FormatError};
use std::collections::{BTreeMap, BTreeSet};

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

/// The fields that must be equal between a variant and its base task: `None` if they are, or
/// the name of the first field that differs.
fn first_difference(v: &Task, b: &Task) -> Option<&'static str> {
    if v.prompt != b.prompt {
        return Some("prompt");
    }
    if v.source_job != b.source_job {
        return Some("source_job");
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
/// suite-relative and prefixed with the suite name for base tasks.
pub fn check_variants(shell: &Suite, bases: &[&Suite]) -> Result<VariantCoverage, Vec<FormatError>> {
    let mut errs = Vec::new();
    let mut err = |file: String, field: &str, detail: String| {
        errs.push(FormatError::new(ErrorCode::InvalidSuite, &file, field, detail));
    };

    // Base tasks by ID, and every base marker.
    let mut base: BTreeMap<&str, (&Suite, &Task, &Oracle)> = BTreeMap::new();
    let mut base_markers: BTreeSet<&str> = BTreeSet::new();
    for s in bases {
        for st in &s.tasks {
            let t = &st.task;
            let file = format!("{}/{}", s.name, task_file(&t.id));
            if matches!(t.family, Some(Family::Shell) | None) {
                err(file, "family", "a base task must be a red-team task of a family other than shell".into());
                continue;
            }
            if base.insert(&t.id, (*s, t, &st.oracle)).is_some() {
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
            Some((_, b, oracle)) => {
                if let Some(field) = first_difference(t, b) {
                    err(
                        file.clone(),
                        field,
                        format!("differs from base task {of}; a variant differs only in the shell"),
                    );
                }
                if &st.oracle != *oracle {
                    err(file, "", format!("oracle differs from the oracle of base task {of}"));
                }
            }
        }
    }
    for (id, (s, _, _)) in &base {
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
