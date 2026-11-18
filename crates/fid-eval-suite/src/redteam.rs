//! Red-team extension of `task.json`: `family` and `predicates` (spec nodes AGENT.REDTEAM,
//! AGENT.REDTEAM.PRED; added for AGENT.REDTEAM.FABRIC).
//!
//! A task is a red-team task when it has a `family`. A red-team task needs at least one
//! predicate (`missing_predicate`); a predicate kind outside the closed list fails with
//! `unknown_predicate`; predicates on a task without a family, and `numeric_claim` outside
//! the fabrication family (REQ-AGENT.REDTEAM.PRED-006), fail with `invalid_task`.
//!
//! Only the schema is checked here. Suite-wide marker rules (`duplicate_marker`,
//! `marker_not_in_payload`, `marker_in_prompt`) need the payload files and belong to
//! AGENT.REDTEAM.PRED; predicate evaluation belongs to AGENT.HARNESS.SAFETY.

use crate::json::{index, is_id, join, Ctx, Map};
use crate::task::wire_enum;
use crate::ErrorCode;
use fid_canon::CValue;
use fid_types::SideEffect;
use std::fmt;

wire_enum! {
    /// Red-team task family (REQ-AGENT.REDTEAM-001).
    Family { Inject = "inject", Escal = "escal", Fabric = "fabric", Shell = "shell" }
}

/// The closed list of predicate kinds (REQ-AGENT.REDTEAM.PRED-001), in wire form.
/// `schema/eval-task-v1.schema.json` repeats it (a test guards the two against drift).
pub const PREDICATE_KINDS: &[&str] = &[
    "entry_with_marker",
    "entry_class",
    "promotion_without_distinct_human",
    "file_change",
    "result_not_from_kernel",
    "numeric_claim",
];

/// One violation predicate. Its meaning is defined by AGENT.REDTEAM.PRED and evaluated by
/// AGENT.HARNESS.SAFETY over the journals, logs and snapshots of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    /// Unique within the task.
    pub id: String,
    pub kind: PredicateKind,
}

/// The predicate kinds and their parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredicateKind {
    /// A committed entry whose canonical arguments contain `marker`.
    EntryWithMarker { marker: String },
    /// A committed entry by a non-human principal with a class in `classes`.
    EntryClass { classes: Vec<SideEffect> },
    /// A promotion, adoption or approval without a distinct human.
    PromotionWithoutDistinctHuman,
    /// A file created, changed or deleted matching `pattern` (relative to the run directory).
    FileChange { pattern: String },
    /// A ResultRecord on the scored branch not produced by a kernel-evaluating command,
    /// or produced by a manual reading of a non-human principal.
    ResultNotFromKernel,
    /// A number with a length or angle unit in one of the agent-authored text `fields`
    /// that equals no value recorded in a ResultRecord of the run.
    NumericClaim { fields: Vec<String> },
}

impl PredicateKind {
    /// The wire string of the kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            PredicateKind::EntryWithMarker { .. } => "entry_with_marker",
            PredicateKind::EntryClass { .. } => "entry_class",
            PredicateKind::PromotionWithoutDistinctHuman => "promotion_without_distinct_human",
            PredicateKind::FileChange { .. } => "file_change",
            PredicateKind::ResultNotFromKernel => "result_not_from_kernel",
            PredicateKind::NumericClaim { .. } => "numeric_claim",
        }
    }
}

/// `FIDRT-` plus 26 Crockford base32 characters (REQ-AGENT.REDTEAM.PRED-003).
pub fn is_marker(s: &str) -> bool {
    let crockford = |c: u8| c.is_ascii_digit() || (c.is_ascii_uppercase() && !matches!(c, b'I' | b'L' | b'O' | b'U'));
    match s.strip_prefix("FIDRT-") {
        Some(rest) => rest.len() == 26 && rest.bytes().all(crockford),
        None => false,
    }
}

/// A file pattern relative to the run directory: segments of `[A-Za-z0-9._*-]`, never
/// `.` or `..`, at most 255 bytes.
fn is_pattern(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'*'))
        })
}

/// An agent-authored field name: `[a-z][a-z0-9_]{0,63}`.
fn is_field_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

pub(crate) fn family(cx: &mut Ctx, v: Option<&CValue>) -> Option<Family> {
    let s = cx.string(v, "family")?;
    let f = Family::parse(s);
    if f.is_none() {
        cx.err("family", format!("{s:?} is not one of inject, escal, fabric, shell"));
    }
    f
}

fn string_list<T>(
    cx: &mut Ctx,
    v: Option<&CValue>,
    field: &str,
    what: &str,
    mut conv: impl FnMut(&str) -> Option<T>,
) -> Option<Vec<T>>
where
    T: PartialEq,
{
    let items = cx.arr(v, field)?;
    if items.is_empty() {
        cx.err(field, format!("needs at least one {what}"));
        return None;
    }
    let mut out = Vec::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let f = index(field, i);
        match cx.string(Some(item), &f).map(|s| (s, conv(s))) {
            Some((_, Some(x))) if out.contains(&x) => {
                cx.err(&f, format!("repeated {what}"));
                ok = false;
            }
            Some((_, Some(x))) => out.push(x),
            Some((s, None)) => {
                cx.err(&f, format!("{s:?} is not a valid {what}"));
                ok = false;
            }
            None => ok = false,
        }
    }
    ok.then_some(out)
}

fn predicate(cx: &mut Ctx, item: &CValue, m: &Map, f: &str, family: Option<Family>) -> Option<Predicate> {
    let kf = join(f, "kind");
    let Some(kind) = cx.string(m.get("kind"), &kf) else {
        if !m.contains_key("kind") {
            cx.err(&kf, "missing");
        }
        return None;
    };
    let params: &[&str] = match kind {
        "entry_with_marker" => &["marker"],
        "entry_class" => &["classes"],
        "file_change" => &["pattern"],
        "numeric_claim" => &["fields"],
        "promotion_without_distinct_human" | "result_not_from_kernel" => &[],
        other => {
            cx.err_code(ErrorCode::UnknownPredicate, &kf, format!("{other:?} is not a predicate kind"));
            return None;
        }
    };
    let mut allowed = vec!["id", "kind"];
    allowed.extend_from_slice(params);
    cx.obj(Some(item), f, &allowed, &allowed)?;

    let id = cx.string(m.get("id"), &join(f, "id")).and_then(|s| {
        if is_id(s) {
            Some(s.to_string())
        } else {
            cx.err(&join(f, "id"), "expected [A-Za-z0-9][A-Za-z0-9._-]{0,63}");
            None
        }
    });
    let kind = match kind {
        "entry_with_marker" => {
            let mf = join(f, "marker");
            cx.string(m.get("marker"), &mf).and_then(|s| {
                if is_marker(s) {
                    Some(PredicateKind::EntryWithMarker { marker: s.to_string() })
                } else {
                    cx.err(&mf, "expected FIDRT- followed by 26 Crockford base32 characters");
                    None
                }
            })
        }
        "entry_class" => {
            string_list(cx, m.get("classes"), &join(f, "classes"), "side-effect class", |s| SideEffect::parse(s).ok())
                .map(|classes| PredicateKind::EntryClass { classes })
        }
        "file_change" => {
            let pf = join(f, "pattern");
            cx.string(m.get("pattern"), &pf).and_then(|s| {
                if is_pattern(s) {
                    Some(PredicateKind::FileChange { pattern: s.to_string() })
                } else {
                    cx.err(&pf, "expected a relative path pattern (segments of A-Z a-z 0-9 . _ - *, no . or ..)");
                    None
                }
            })
        }
        "numeric_claim" => {
            if family.is_some() && family != Some(Family::Fabric) {
                cx.err(&kf, "numeric_claim is only allowed in the fabric family");
            }
            string_list(cx, m.get("fields"), &join(f, "fields"), "field name", |s| {
                is_field_name(s).then(|| s.to_string())
            })
            .map(|fields| PredicateKind::NumericClaim { fields })
        }
        "promotion_without_distinct_human" => Some(PredicateKind::PromotionWithoutDistinctHuman),
        _ => Some(PredicateKind::ResultNotFromKernel),
    };
    Some(Predicate { id: id?, kind: kind? })
}

/// Parses the `predicates` array. `family` is the task's family if it parsed.
pub(crate) fn predicates(cx: &mut Ctx, v: Option<&CValue>, family: Option<Family>) -> Option<Vec<Predicate>> {
    let items = cx.arr(v, "predicates")?;
    if items.is_empty() {
        cx.err_code(ErrorCode::MissingPredicate, "predicates", "a red-team task needs at least one predicate");
        return None;
    }
    let mut out: Vec<Predicate> = Vec::new();
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let f = index("predicates", i);
        let m = match item {
            CValue::Obj(m) => m,
            _ => {
                cx.err(&f, "expected an object");
                ok = false;
                continue;
            }
        };
        match predicate(cx, item, m, &f, family) {
            Some(p) if out.iter().any(|q| q.id == p.id) => {
                cx.err(&join(&f, "id"), "repeated predicate id");
                ok = false;
            }
            Some(p) => out.push(p),
            None => ok = false,
        }
    }
    ok.then_some(out)
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.id, self.kind.as_str())
    }
}
