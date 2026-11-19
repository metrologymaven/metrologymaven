//! The `gdt.define_datum@1` journal event.
//!
//! ```json
//! { "command": "gdt.define_datum@1", "principal": "human", "branch": "working",
//!   "plan": "block-1", "label_pattern": "[A-Z]{1,3}", "replace": false,
//!   "old": { ..definition.. } | null, "new": { ..definition.. } }
//! ```
//!
//! The event holds the full new definition (REQ-GDT.DRF.DATUM-001), and on `replace` of an
//! existing datum the full previous one (REQ-002). It also records the label pattern the
//! label was checked against, so exact replay applies the same rule. Serialised as RFC 8785
//! canonical JSON.

use super::types::{DatumDefinition, LabelPattern};
use super::{DatumError, DEFINE_COMMAND};
use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::Branch;
use crate::chars::spec::PlanId;
use fid_canon::CValue;
use fid_io_reg::Principal;
use std::collections::BTreeMap;

/// Largest event accepted by [`DatumEvent::from_canonical_bytes`], in bytes.
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
/// Deepest event nesting: the event, a definition, its targets, one target.
const MAX_EVENT_DEPTH: usize = 4;
const FIELDS: [&str; 8] = ["branch", "command", "label_pattern", "new", "old", "plan", "principal", "replace"];

/// One `gdt.define_datum@1` event. Fields are private; events are made by
/// [`super::DatumModel::define_datum`] or read back by [`DatumEvent::from_canonical_bytes`],
/// which applies the same rules. The branch is always derived from the principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumEvent {
    principal: Principal,
    branch: Branch,
    plan: PlanId,
    pattern: LabelPattern,
    replace: bool,
    old: Option<DatumDefinition>,
    new: DatumDefinition,
}

fn bad(reason: &'static str) -> DatumError {
    DatumError::Event { reason }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, DatumError> {
    match o.get(k) {
        Some(CValue::Str(v)) => Ok(v),
        _ => Err(bad("wrong_type")),
    }
}

fn definition(v: &CValue) -> Result<DatumDefinition, DatumError> {
    DatumDefinition::from_cvalue(v, bad).map_err(|e| match e {
        DatumError::Event { .. } => e,
        _ => bad("bad_definition"),
    })
}

impl DatumEvent {
    /// Callers keep the shape rules: `old` is `Some` only when `replace`, and `old` and `new`
    /// share the label.
    pub(crate) fn new(
        principal: Principal,
        plan: PlanId,
        pattern: LabelPattern,
        replace: bool,
        old: Option<DatumDefinition>,
        new: DatumDefinition,
    ) -> DatumEvent {
        DatumEvent { principal, branch: Branch::for_principal(principal), plan, pattern, replace, old, new }
    }

    /// Who acted.
    pub fn principal(&self) -> Principal {
        self.principal
    }
    /// The branch written (derived from the principal).
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// The plan.
    pub fn plan(&self) -> &PlanId {
        &self.plan
    }
    /// The label pattern the label was checked against.
    pub fn label_pattern(&self) -> &LabelPattern {
        &self.pattern
    }
    /// Whether `replace` was given.
    pub fn replace(&self) -> bool {
        self.replace
    }
    /// The definition replaced.
    pub fn old(&self) -> Option<&DatumDefinition> {
        self.old.as_ref()
    }
    /// The new definition.
    pub fn new_definition(&self) -> &DatumDefinition {
        &self.new
    }

    /// The event as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("command".to_owned(), s(DEFINE_COMMAND));
        m.insert("principal".to_owned(), s(self.principal.as_str()));
        m.insert("branch".to_owned(), s(self.branch.as_str()));
        m.insert("plan".to_owned(), s(self.plan.as_str()));
        m.insert("label_pattern".to_owned(), s(self.pattern.as_str()));
        m.insert("replace".to_owned(), CValue::Bool(self.replace));
        m.insert("old".to_owned(), self.old.as_ref().map_or(CValue::Null, DatumDefinition::to_cvalue));
        m.insert("new".to_owned(), self.new.to_cvalue());
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DatumError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| bad("encode"))
    }

    /// Reads an event back strictly: at most [`MAX_EVENT_BYTES`], nesting at most four, no
    /// duplicate key, the eight fields and no other, the known command, the branch of the
    /// principal (`branch_principal_mismatch`), a valid plan ID, a valid label pattern
    /// (`bad_label_pattern`), valid definitions (`bad_definition`), the new label matching the
    /// pattern (`label_not_in_pattern`), and the shape rule: `old` only with `replace`, and
    /// with the same label (`bad_shape`). Fails `event_invalid{reason}`.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<DatumEvent, DatumError> {
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(bad("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json"))?;
        let o = match &v {
            CValue::Obj(o) if o.len() == FIELDS.len() && FIELDS.iter().all(|k| o.contains_key(*k)) => o,
            CValue::Obj(_) => return Err(bad("bad_keys")),
            _ => return Err(bad("wrong_type")),
        };
        if text(o, "command")? != DEFINE_COMMAND {
            return Err(bad("unknown_command"));
        }
        let principal = parse_principal(text(o, "principal")?).ok_or_else(|| bad("bad_principal"))?;
        let branch = Branch::parse(text(o, "branch")?).ok_or_else(|| bad("bad_branch"))?;
        if Branch::for_principal(principal) != branch {
            return Err(bad("branch_principal_mismatch"));
        }
        let plan = PlanId::parse(text(o, "plan")?).ok_or_else(|| bad("bad_plan_id"))?;
        let pattern = LabelPattern::parse(text(o, "label_pattern")?).map_err(|_| bad("bad_label_pattern"))?;
        let replace = match o.get("replace") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(bad("wrong_type")),
        };
        let old = match o.get("old") {
            Some(CValue::Null) => None,
            Some(v) => Some(definition(v)?),
            None => return Err(bad("bad_keys")),
        };
        let new = definition(o.get("new").ok_or_else(|| bad("bad_keys"))?)?;
        if !pattern.matches(new.label()) {
            return Err(bad("label_not_in_pattern"));
        }
        if let Some(old) = &old {
            if !replace || old.label() != new.label() {
                return Err(bad("bad_shape"));
            }
        }
        Ok(DatumEvent { principal, branch, plan, pattern, replace, old, new })
    }
}
