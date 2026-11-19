//! The `gdt.set_governing_spec@1` journal event.
//!
//! ```json
//! { "command": "gdt.set_governing_spec@1", "principal": "human", "branch": "working",
//!   "char": "flat-1", "plan": null, "inherited_from": "explicit",
//!   "old": { ..spec.. } | null, "new": { ..spec.. } }
//! ```
//!
//! One shape serves both the explicit command (`inherited_from: explicit`, `plan: null`) and
//! creation-time inheritance (`inherited_from: plan|tenant`, the plan the characteristic was
//! created in, `old: null`). `inherited_from` always equals `new.inherited_from`. Serialised
//! as RFC 8785 canonical JSON.

use super::types::{opt_cvalue, GoverningSpec, InheritedFrom, PlanId};
use super::{SpecError, SET_COMMAND};
use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::{Branch, CharId};
use fid_canon::CValue;
use fid_io_reg::Principal;
use std::collections::BTreeMap;

/// Largest event accepted by [`SpecEvent::from_canonical_bytes`], in bytes.
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
/// Deepest event nesting: the event, a spec, its standards.
const MAX_EVENT_DEPTH: usize = 3;
const FIELDS: [&str; 8] = ["branch", "char", "command", "inherited_from", "new", "old", "plan", "principal"];

/// One `gdt.set_governing_spec@1` event. Fields are private; events are made by the commands
/// of [`super::SpecState`] or read back by [`SpecEvent::from_canonical_bytes`], which apply
/// the same rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecEvent {
    principal: Principal,
    branch: Branch,
    char: CharId,
    plan: Option<PlanId>,
    old: Option<GoverningSpec>,
    new: GoverningSpec,
}

fn bad(reason: &'static str) -> SpecError {
    SpecError::Event { reason }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, SpecError> {
    match o.get(k) {
        Some(CValue::Str(v)) => Ok(v),
        _ => Err(bad("wrong_type")),
    }
}

fn spec(v: &CValue) -> Result<GoverningSpec, SpecError> {
    GoverningSpec::from_cvalue(v, bad).map_err(|e| match e {
        SpecError::Event { .. } => e,
        _ => bad("bad_spec"),
    })
}

impl SpecEvent {
    /// Callers keep the shape rules: `plan` is `Some` and `old` is `None` exactly when `new`
    /// was inherited.
    pub(crate) fn new(
        principal: Principal,
        char: CharId,
        plan: Option<PlanId>,
        old: Option<GoverningSpec>,
        new: GoverningSpec,
    ) -> SpecEvent {
        SpecEvent { principal, branch: Branch::for_principal(principal), char, plan, old, new }
    }

    /// Who acted.
    pub fn principal(&self) -> Principal {
        self.principal
    }
    /// The branch written.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// The characteristic.
    pub fn char(&self) -> &CharId {
        &self.char
    }
    /// The plan the characteristic was created in (inheritance events only).
    pub fn plan(&self) -> Option<&PlanId> {
        self.plan.as_ref()
    }
    /// The spec before.
    pub fn old(&self) -> Option<&GoverningSpec> {
        self.old.as_ref()
    }
    /// The spec after.
    pub fn new_spec(&self) -> &GoverningSpec {
        &self.new
    }
    /// Where the new spec came from.
    pub fn inherited_from(&self) -> InheritedFrom {
        self.new.inherited_from()
    }

    /// The event as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("command".to_owned(), s(SET_COMMAND));
        m.insert("principal".to_owned(), s(self.principal.as_str()));
        m.insert("branch".to_owned(), s(self.branch.as_str()));
        m.insert("char".to_owned(), s(self.char.as_str()));
        m.insert("plan".to_owned(), self.plan.as_ref().map_or(CValue::Null, |p| s(p.as_str())));
        m.insert("inherited_from".to_owned(), s(self.inherited_from().as_str()));
        m.insert("old".to_owned(), opt_cvalue(self.old.as_ref()));
        m.insert("new".to_owned(), self.new.to_cvalue());
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SpecError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| bad("encode"))
    }

    /// Reads an event back strictly: at most [`MAX_EVENT_BYTES`], no duplicate key, the
    /// eight fields and no other, the known command, the branch of the principal, valid IDs
    /// and specs (`bad_spec`), `inherited_from` equal to `new.inherited_from`, and the shape
    /// rule (`plan` set and `old` null exactly for `plan`/`tenant`). Fails
    /// `event_invalid{reason}`.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<SpecEvent, SpecError> {
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(bad("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json"))?;
        let o = match &v {
            CValue::Obj(o) if o.len() == FIELDS.len() && FIELDS.iter().all(|k| o.contains_key(*k)) => o,
            CValue::Obj(_) => return Err(bad("bad_keys")),
            _ => return Err(bad("wrong_type")),
        };
        if text(o, "command")? != SET_COMMAND {
            return Err(bad("unknown_command"));
        }
        let principal = parse_principal(text(o, "principal")?).ok_or_else(|| bad("bad_principal"))?;
        let branch = Branch::parse(text(o, "branch")?).ok_or_else(|| bad("bad_branch"))?;
        if Branch::for_principal(principal) != branch {
            return Err(bad("branch_principal_mismatch"));
        }
        let char = CharId::parse(text(o, "char")?).ok_or_else(|| bad("bad_char_id"))?;
        let plan = match o.get("plan") {
            Some(CValue::Null) => None,
            Some(CValue::Str(p)) => Some(PlanId::parse(p).ok_or_else(|| bad("bad_plan_id"))?),
            _ => return Err(bad("wrong_type")),
        };
        let from = InheritedFrom::parse(text(o, "inherited_from")?).ok_or_else(|| bad("bad_inherited_from"))?;
        let old = match o.get("old") {
            Some(CValue::Null) => None,
            Some(v) => Some(spec(v)?),
            None => return Err(bad("bad_keys")),
        };
        let new = spec(o.get("new").ok_or_else(|| bad("bad_keys"))?)?;
        if new.inherited_from() != from {
            return Err(bad("inherited_from_mismatch"));
        }
        let inherited = from != InheritedFrom::Explicit;
        if inherited != plan.is_some() || (inherited && old.is_some()) {
            return Err(bad("bad_shape"));
        }
        Ok(SpecEvent { principal, branch, char, plan, old, new })
    }
}
