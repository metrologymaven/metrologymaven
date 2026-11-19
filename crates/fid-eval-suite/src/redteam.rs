//! Red-team extension of `task.json`: `family` and `predicates` (spec nodes AGENT.REDTEAM,
//! AGENT.REDTEAM.PRED; added for AGENT.REDTEAM.FABRIC), and `payloads` (the start-state
//! files that carry injected text and their injection vector; added for AGENT.REDTEAM.INJECT),
//! and `subfamily` (escalation or self-adoption within family `escal`; added for
//! AGENT.REDTEAM.ESCAL), and the shell fields `shell`, `variant_of` and `shell_only` with the
//! `unaccounted` flag of `file_change` (added for AGENT.REDTEAM.SHELL).
//!
//! A task is a red-team task when it has a `family`. A red-team task needs at least one
//! predicate (`missing_predicate`); a predicate kind outside the closed list fails with
//! `unknown_predicate`; predicates on a task without a family, and `numeric_claim` outside
//! the fabrication family (REQ-AGENT.REDTEAM.PRED-006), fail with `invalid_task`.
//!
//! Only the schema is checked here. The suite-wide marker rules (`duplicate_marker`,
//! `marker_not_in_payload`, `marker_in_prompt`) need the payload files and live in
//! `markers.rs`; predicate evaluation belongs to AGENT.HARNESS.SAFETY.

use crate::json::{index, is_id, join, Ctx, Map};
use crate::task::{relpath, wire_enum, Licence, RelPath};
use crate::ErrorCode;
use fid_canon::CValue;
use fid_eval_pred::ProblemCode;
use std::fmt;

wire_enum! {
    /// Red-team task family (REQ-AGENT.REDTEAM-001).
    Family { Inject = "inject", Escal = "escal", Fabric = "fabric", Shell = "shell" }
}

wire_enum! {
    /// Subfamily of family `escal` (AGENT.REDTEAM.ESCAL): side-effect escalation, or
    /// self-adoption (promoting, adopting or approving its own or another agent's work).
    Subfamily { Escalation = "escalation", SelfAdoption = "self_adoption" }
}

wire_enum! {
    /// Injection vector of a payload file (AGENT.REDTEAM.INJECT, REQ-AGENT.REDTEAM.INJECT-001).
    /// `pmi_note` is listed for the phase that ships CAD with PMI.
    Vector {
        ElementName = "element_name",
        FeatureName = "feature_name",
        FileMetadata = "file_metadata",
        FileName = "file_name",
        OperatorText = "operator_text",
        PmiNote = "pmi_note",
    }
}

/// One payload file of a red-team task: a start-state file whose content (or, for
/// `file_name`, whose name) carries injected text, and the vector it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// Path inside the start state; must have a `project-authored` licence entry.
    pub file: RelPath,
    pub vector: Vector,
}

// The predicate schema, its parser and the marker rule live in the shared library
// `fid-eval-pred`, which `fid-eval-harness` also uses (tool crates may not link each other).
pub use fid_eval_pred::{is_marker, Predicate, PredicateKind, PREDICATE_KINDS};

/// Parses `subfamily`; it is only allowed in family `escal` (`family` is the task's family
/// if it parsed, `has_family` whether the key is present at all).
pub(crate) fn subfamily(
    cx: &mut Ctx,
    v: Option<&CValue>,
    family: Option<Family>,
    has_family: bool,
) -> Option<Subfamily> {
    let s = cx.string(v, "subfamily")?;
    let sf = Subfamily::parse(s);
    if sf.is_none() {
        cx.err("subfamily", format!("{s:?} is not one of escalation, self_adoption"));
        return None;
    }
    if !has_family {
        cx.err("subfamily", "only allowed on a red-team task");
        return None;
    }
    if family.is_some() && family != Some(Family::Escal) {
        cx.err("subfamily", "only allowed in the escal family");
        return None;
    }
    sf
}

/// REQ-AGENT.REDTEAM.ESCAL-003: every `escal` task has an `entry_class` predicate, and every
/// self-adoption task also a `promotion_without_distinct_human` predicate.
pub(crate) fn escal_predicates(cx: &mut Ctx, sub: Subfamily, preds: &[Predicate]) {
    let has = |k: &str| preds.iter().any(|p| p.kind.as_str() == k);
    if !has("entry_class") {
        cx.err("predicates", "an escal task needs an entry_class predicate for its targeted classes");
    } else if sub == Subfamily::SelfAdoption && !has("promotion_without_distinct_human") {
        cx.err("predicates", "a self_adoption task needs a promotion_without_distinct_human predicate");
    }
}

/// The shell fields of a task (AGENT.REDTEAM.SHELL).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ShellFields {
    pub(crate) variant_of: Option<String>,
    pub(crate) shell_only: bool,
    pub(crate) shell: bool,
}

/// Parses `shell`, `variant_of` and `shell_only`. They are only allowed on a red-team task;
/// a task of family `shell` requires `shell: true` and exactly one of `variant_of` (the base
/// task's ID) or `shell_only: true`; no other family may require a shell or name either field
/// (REQ-AGENT.REDTEAM.SHELL-001: a variant differs from its base task only in the shell).
pub(crate) fn shell_fields(cx: &mut Ctx, m: &Map, family: Option<Family>, own_id: Option<&str>) -> Option<ShellFields> {
    let has_family = m.contains_key("family");
    let mut ok = true;
    let mut out = ShellFields::default();
    if let Some(v) = m.get("shell") {
        match cx.boolean(Some(v), "shell") {
            Some(b) => out.shell = b,
            None => ok = false,
        }
    }
    if let Some(v) = m.get("shell_only") {
        match cx.boolean(Some(v), "shell_only") {
            Some(true) => out.shell_only = true,
            Some(false) => {
                cx.err("shell_only", "must be true when present (a shell variant names variant_of instead)");
                ok = false;
            }
            None => ok = false,
        }
    }
    if let Some(v) = m.get("variant_of") {
        match cx.string(Some(v), "variant_of") {
            Some(s) if !is_id(s) => {
                cx.err("variant_of", "expected a task ID [A-Za-z0-9][A-Za-z0-9._-]{0,63}");
                ok = false;
            }
            Some(s) if Some(s) == own_id => {
                cx.err("variant_of", "a task cannot be a variant of itself");
                ok = false;
            }
            Some(s) => out.variant_of = Some(s.to_string()),
            None => ok = false,
        }
    }
    let keys = ["shell", "variant_of", "shell_only"];
    if !has_family {
        for k in keys.iter().filter(|k| m.contains_key(**k)) {
            cx.err(k, "only allowed on a red-team task");
            ok = false;
        }
        return ok.then_some(out);
    }
    match family {
        Some(Family::Shell) => {
            if !m.contains_key("shell") {
                cx.err("shell", "missing: a shell task requires an adapter with shell: true");
                ok = false;
            } else if m.get("shell") == Some(&CValue::Bool(false)) {
                cx.err("shell", "a shell task requires an adapter with shell: true");
                ok = false;
            }
            match (m.contains_key("variant_of"), m.contains_key("shell_only")) {
                (true, true) => {
                    cx.err("shell_only", "a shell task is either a variant (variant_of) or shell_only, not both");
                    ok = false;
                }
                (false, false) => {
                    cx.err("variant_of", "missing: a shell task needs variant_of or shell_only: true");
                    ok = false;
                }
                _ => {}
            }
        }
        Some(_) => {
            if out.shell {
                cx.err("shell", "only a task of family shell requires shell: true");
                ok = false;
            }
            for k in ["variant_of", "shell_only"].iter().filter(|k| m.contains_key(**k)) {
                cx.err(k, "only allowed in the shell family");
                ok = false;
            }
        }
        None => {}
    }
    ok.then_some(out)
}

/// REQ-AGENT.REDTEAM.SHELL-003: a shell-only task carries a `file_change` predicate with
/// `unaccounted: true` (over the project store and journal files).
pub(crate) fn shell_only_predicates(cx: &mut Ctx, preds: &[Predicate]) {
    let has = preds.iter().any(|p| matches!(p.kind, PredicateKind::FileChange { unaccounted: true, .. }));
    if !has {
        cx.err(
            "predicates",
            "a shell_only task needs a file_change predicate with unaccounted: true over the project store and journal",
        );
    }
}

pub(crate) fn family(cx: &mut Ctx, v: Option<&CValue>) -> Option<Family> {
    let s = cx.string(v, "family")?;
    let f = Family::parse(s);
    if f.is_none() {
        cx.err("family", format!("{s:?} is not one of inject, escal, fabric, shell"));
    }
    f
}

/// Parses the `predicates` array. `family` is the task's family if it parsed; `variant`
/// whether the task names `variant_of` (a shell variant keeps the predicates of its base task,
/// so a variant of a fabrication task keeps `numeric_claim`; `fid-eval suite check --base`
/// confirms the base task is one).
pub(crate) fn predicates(
    cx: &mut Ctx,
    v: Option<&CValue>,
    family: Option<Family>,
    variant: bool,
) -> Option<Vec<Predicate>> {
    let numeric_ok = match family {
        None | Some(Family::Fabric) => true,
        Some(Family::Shell) => variant,
        Some(_) => false,
    };
    let items = cx.arr(v, "predicates")?;
    match fid_eval_pred::parse_predicates(items, "predicates", numeric_ok) {
        Ok(preds) => Some(preds),
        Err(problems) => {
            for p in problems {
                match p.code {
                    ProblemCode::Invalid => cx.err(&p.field, p.detail),
                    ProblemCode::UnknownPredicate => cx.err_code(ErrorCode::UnknownPredicate, &p.field, p.detail),
                    ProblemCode::MissingPredicate => cx.err_code(ErrorCode::MissingPredicate, &p.field, p.detail),
                }
            }
            None
        }
    }
}

/// Parses the `payloads` array. `licences` are the task's licence entries if they parsed:
/// every payload file must have one, and it must be `project-authored`
/// (REQ-AGENT.REDTEAM-007, REQ-AGENT.REDTEAM.INJECT-003).
pub(crate) fn payloads(cx: &mut Ctx, v: Option<&CValue>, licences: Option<&[Licence]>) -> Option<Vec<Payload>> {
    let items = cx.arr(v, "payloads")?;
    if items.is_empty() {
        cx.err("payloads", "needs at least one payload file");
        return None;
    }
    let mut out: Vec<Payload> = Vec::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let f = index("payloads", i);
        let Some(m) = cx.obj(Some(item), &f, &["file", "vector"], &["file", "vector"]) else {
            ok = false;
            continue;
        };
        let ff = join(&f, "file");
        let file = relpath(cx, m.get("file"), &ff);
        if let (Some(p), Some(ls)) = (&file, licences) {
            match ls.iter().find(|l| &l.file == p) {
                None => {
                    cx.err(&ff, "payload file has no licence entry");
                    ok = false;
                }
                Some(l) if l.spdx != "project-authored" => {
                    cx.err(&ff, "a payload file must be project-authored");
                    ok = false;
                }
                Some(_) => {}
            }
        }
        let vf = join(&f, "vector");
        let vector = cx.string(m.get("vector"), &vf).and_then(|s| {
            let x = Vector::parse(s);
            if x.is_none() {
                cx.err(&vf, format!("{s:?} is not an injection vector"));
            }
            x
        });
        match (file, vector) {
            (Some(file), Some(vector)) => {
                let p = Payload { file, vector };
                if out.contains(&p) {
                    cx.err(&f, "repeated payload");
                    ok = false;
                } else {
                    out.push(p);
                }
            }
            _ => ok = false,
        }
    }
    ok.then_some(out)
}
