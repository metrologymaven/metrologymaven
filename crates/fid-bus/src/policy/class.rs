//! Class, principal and human-only matrix. Spec node `CORE.BUS.POL.CLASS`.
//!
//! The first check of the authorise stage: a fixed table from (principal kind, side-effect
//! class, `human_only`) to allow or a `policy` refusal. It encodes
//!
//! - the CONSTITUTION §3 bans: only a `human` principal may run an M1, S1, W3 or A0
//!   command; every other kind gets `policy/class_forbidden` (REQ-001, REQ-003);
//! - the D-031 `human_only` rule: a command whose definition has `human_only: true` is
//!   refused for every non-human principal with `policy/human_only`, whatever its class
//!   (REQ-002; the rule names agent and script, and this table also refuses service,
//!   device and system, failing closed);
//! - the §3.1 job-cancel rule as a policy hook ([`authorise_cancel`]): a principal may
//!   cancel its own job, a human any job; otherwise `policy/not_job_owner` (REQ-004).
//!
//! The class ban is checked before `human_only`: an agent invoking a human-only W3
//! command gets `class_forbidden`.
//!
//! [`class_rule`] is one `match` with an explicit arm for each of the 6 x 11 x 2 = 132
//! combinations and no wildcard (REQ-005). The module denies the wildcard lints, and the
//! two enums are matched by path, so adding a principal kind or a side-effect class fails
//! to build until every new cell is written here. Motion release (CMM), the S1 ceremony
//! (TRUST), MCP projection (`CORE.BUS.POL.PROJ`), file handles (`CORE.BUS.POL.FS`) and X1
//! approval (`CORE.BUS.POL.EGRESS`) are later checks; an `Allow` here only means this
//! check passes. The table is published as `schema/policy-matrix.json`
//! ([`policy_matrix_json`]). Determinism D0; no allocation on the decision path.

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants, clippy::wildcard_in_or_patterns)]

use crate::error::{BusError, Detail, PolicyReason};
use crate::principal::{Principal, PrincipalKind, PrincipalRef};
use fid_canon::{to_canonical_bytes, CValue, CanonError};
use fid_registry::CommandDef;
use fid_types::SideEffect;
use std::collections::BTreeMap;

/// Schema version of `policy-matrix.json`.
pub const MATRIX_SCHEMA_VERSION: u16 = 1;

/// The node the matrix belongs to, as published.
pub const MATRIX_NODE: &str = "CORE.BUS.POL.CLASS";

/// The outcome of the class check for one cell of the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassRule {
    /// This check passes; later checks of the authorise stage still apply.
    Allow,
    /// Refused with `policy` and this reason.
    Refuse(PolicyReason),
}

const ALLOW: ClassRule = ClassRule::Allow;
const CLASS: ClassRule = ClassRule::Refuse(PolicyReason::ClassForbidden);
const HUMAN: ClassRule = ClassRule::Refuse(PolicyReason::HumanOnly);

use crate::principal::PrincipalKind as P;
use fid_types::SideEffect as C;

/// The matrix: one explicit arm per (principal kind, side-effect class, `human_only`).
/// No wildcard arm; see the module documentation.
#[rustfmt::skip]
pub const fn class_rule(kind: PrincipalKind, class: SideEffect, human_only: bool) -> ClassRule {
    match (kind, class, human_only) {
        // human: allowed everywhere by this check.
        (P::Human, C::R0, false) => ALLOW, (P::Human, C::R0, true) => ALLOW,
        (P::Human, C::R1, false) => ALLOW, (P::Human, C::R1, true) => ALLOW,
        (P::Human, C::W1, false) => ALLOW, (P::Human, C::W1, true) => ALLOW,
        (P::Human, C::W2, false) => ALLOW, (P::Human, C::W2, true) => ALLOW,
        (P::Human, C::W3, false) => ALLOW, (P::Human, C::W3, true) => ALLOW,
        (P::Human, C::X1, false) => ALLOW, (P::Human, C::X1, true) => ALLOW,
        (P::Human, C::I1, false) => ALLOW, (P::Human, C::I1, true) => ALLOW,
        (P::Human, C::D1, false) => ALLOW, (P::Human, C::D1, true) => ALLOW,
        (P::Human, C::M1, false) => ALLOW, (P::Human, C::M1, true) => ALLOW,
        (P::Human, C::S1, false) => ALLOW, (P::Human, C::S1, true) => ALLOW,
        (P::Human, C::A0, false) => ALLOW, (P::Human, C::A0, true) => ALLOW,

        // agent: CONSTITUTION §3 ban (REQ-001), then D-031 human_only (REQ-002).
        (P::Agent, C::R0, false) => ALLOW, (P::Agent, C::R0, true) => HUMAN,
        (P::Agent, C::R1, false) => ALLOW, (P::Agent, C::R1, true) => HUMAN,
        (P::Agent, C::W1, false) => ALLOW, (P::Agent, C::W1, true) => HUMAN,
        (P::Agent, C::W2, false) => ALLOW, (P::Agent, C::W2, true) => HUMAN,
        (P::Agent, C::W3, false) => CLASS, (P::Agent, C::W3, true) => CLASS,
        (P::Agent, C::X1, false) => ALLOW, (P::Agent, C::X1, true) => HUMAN,
        (P::Agent, C::I1, false) => ALLOW, (P::Agent, C::I1, true) => HUMAN,
        (P::Agent, C::D1, false) => ALLOW, (P::Agent, C::D1, true) => HUMAN,
        (P::Agent, C::M1, false) => CLASS, (P::Agent, C::M1, true) => CLASS,
        (P::Agent, C::S1, false) => CLASS, (P::Agent, C::S1, true) => CLASS,
        (P::Agent, C::A0, false) => CLASS, (P::Agent, C::A0, true) => CLASS,

        // script: REQ-003 ban, then D-031 human_only (REQ-002).
        (P::Script, C::R0, false) => ALLOW, (P::Script, C::R0, true) => HUMAN,
        (P::Script, C::R1, false) => ALLOW, (P::Script, C::R1, true) => HUMAN,
        (P::Script, C::W1, false) => ALLOW, (P::Script, C::W1, true) => HUMAN,
        (P::Script, C::W2, false) => ALLOW, (P::Script, C::W2, true) => HUMAN,
        (P::Script, C::W3, false) => CLASS, (P::Script, C::W3, true) => CLASS,
        (P::Script, C::X1, false) => ALLOW, (P::Script, C::X1, true) => HUMAN,
        (P::Script, C::I1, false) => ALLOW, (P::Script, C::I1, true) => HUMAN,
        (P::Script, C::D1, false) => ALLOW, (P::Script, C::D1, true) => HUMAN,
        (P::Script, C::M1, false) => CLASS, (P::Script, C::M1, true) => CLASS,
        (P::Script, C::S1, false) => CLASS, (P::Script, C::S1, true) => CLASS,
        (P::Script, C::A0, false) => CLASS, (P::Script, C::A0, true) => CLASS,

        // service: REQ-003 ban, then human_only (fail closed).
        (P::Service, C::R0, false) => ALLOW, (P::Service, C::R0, true) => HUMAN,
        (P::Service, C::R1, false) => ALLOW, (P::Service, C::R1, true) => HUMAN,
        (P::Service, C::W1, false) => ALLOW, (P::Service, C::W1, true) => HUMAN,
        (P::Service, C::W2, false) => ALLOW, (P::Service, C::W2, true) => HUMAN,
        (P::Service, C::W3, false) => CLASS, (P::Service, C::W3, true) => CLASS,
        (P::Service, C::X1, false) => ALLOW, (P::Service, C::X1, true) => HUMAN,
        (P::Service, C::I1, false) => ALLOW, (P::Service, C::I1, true) => HUMAN,
        (P::Service, C::D1, false) => ALLOW, (P::Service, C::D1, true) => HUMAN,
        (P::Service, C::M1, false) => CLASS, (P::Service, C::M1, true) => CLASS,
        (P::Service, C::S1, false) => CLASS, (P::Service, C::S1, true) => CLASS,
        (P::Service, C::A0, false) => CLASS, (P::Service, C::A0, true) => CLASS,

        // device: REQ-003 ban, then human_only (fail closed).
        (P::Device, C::R0, false) => ALLOW, (P::Device, C::R0, true) => HUMAN,
        (P::Device, C::R1, false) => ALLOW, (P::Device, C::R1, true) => HUMAN,
        (P::Device, C::W1, false) => ALLOW, (P::Device, C::W1, true) => HUMAN,
        (P::Device, C::W2, false) => ALLOW, (P::Device, C::W2, true) => HUMAN,
        (P::Device, C::W3, false) => CLASS, (P::Device, C::W3, true) => CLASS,
        (P::Device, C::X1, false) => ALLOW, (P::Device, C::X1, true) => HUMAN,
        (P::Device, C::I1, false) => ALLOW, (P::Device, C::I1, true) => HUMAN,
        (P::Device, C::D1, false) => ALLOW, (P::Device, C::D1, true) => HUMAN,
        (P::Device, C::M1, false) => CLASS, (P::Device, C::M1, true) => CLASS,
        (P::Device, C::S1, false) => CLASS, (P::Device, C::S1, true) => CLASS,
        (P::Device, C::A0, false) => CLASS, (P::Device, C::A0, true) => CLASS,

        // system: REQ-003 ban, then human_only (fail closed). Replay re-executes W3 entries
        // through CORE.RPL, not through this check.
        (P::System, C::R0, false) => ALLOW, (P::System, C::R0, true) => HUMAN,
        (P::System, C::R1, false) => ALLOW, (P::System, C::R1, true) => HUMAN,
        (P::System, C::W1, false) => ALLOW, (P::System, C::W1, true) => HUMAN,
        (P::System, C::W2, false) => ALLOW, (P::System, C::W2, true) => HUMAN,
        (P::System, C::W3, false) => CLASS, (P::System, C::W3, true) => CLASS,
        (P::System, C::X1, false) => ALLOW, (P::System, C::X1, true) => HUMAN,
        (P::System, C::I1, false) => ALLOW, (P::System, C::I1, true) => HUMAN,
        (P::System, C::D1, false) => ALLOW, (P::System, C::D1, true) => HUMAN,
        (P::System, C::M1, false) => CLASS, (P::System, C::M1, true) => CLASS,
        (P::System, C::S1, false) => CLASS, (P::System, C::S1, true) => CLASS,
        (P::System, C::A0, false) => CLASS, (P::System, C::A0, true) => CLASS,
    }
}

/// The class check for one call: `principal` is the bus-derived principal
/// (`SessionTable::derive_principal`), `def` the registry definition of the invoked command.
/// Pure; commits nothing. Errors: `policy/class_forbidden`, `policy/human_only`.
pub fn authorise_class(principal: &Principal, def: &CommandDef) -> Result<(), BusError> {
    let kind = principal.kind();
    match class_rule(kind, def.side_effect, def.human_only) {
        ClassRule::Allow => Ok(()),
        ClassRule::Refuse(reason) => {
            let what = if reason == PolicyReason::HumanOnly { "the human-only command" } else { "the command" };
            Err(BusError::Policy(
                reason,
                Detail::new(&format!(
                    "a {} principal may not invoke {what} {}@{} (class {})",
                    kind.as_str(),
                    def.id,
                    def.major,
                    def.side_effect.as_str()
                )),
            ))
        }
    }
}

/// The job-cancel rule of CONSTITUTION §3.1 (REQ-004), a policy hook for
/// `job.cancel_job@1`: a `human` may cancel any job; any other principal only a job whose
/// recorded owner is itself, compared by kind and identity ([`PrincipalRef`]), so `alice`
/// as `agent` does not own a job of `alice` as `script`.
///
/// `recorded_owner` must be the principal the job service recorded when the job was
/// submitted, never a value taken from the request. `None` (no recorded owner) is open to a
/// human only. Error: `policy/not_job_owner`.
pub fn authorise_cancel(caller: &Principal, recorded_owner: Option<&PrincipalRef>) -> Result<(), BusError> {
    let may_cancel_others = match caller.kind() {
        PrincipalKind::Human => true,
        PrincipalKind::Agent
        | PrincipalKind::Script
        | PrincipalKind::Service
        | PrincipalKind::Device
        | PrincipalKind::System => false,
    };
    if may_cancel_others {
        return Ok(());
    }
    let own = recorded_owner.is_some_and(|owner| owner.kind == caller.kind() && owner.id == *caller.id());
    if own {
        Ok(())
    } else {
        Err(BusError::Policy(
            PolicyReason::NotJobOwner,
            Detail::new(&format!(
                "a {} principal may cancel only its own jobs; a human may cancel any job",
                caller.kind().as_str()
            )),
        ))
    }
}

/// The matrix as published in `schema/policy-matrix.json` (REQ-005): the principal kinds
/// and classes in declaration order, then one row per cell in that order, `human_only`
/// false before true. A row is `{allow, class, human_only, principal}` plus `code` and
/// `reason` when refused.
pub fn policy_matrix_value() -> CValue {
    let s = |v: &str| CValue::Str(v.to_string());
    let mut rows = Vec::with_capacity(PrincipalKind::ALL.len() * SideEffect::ALL.len() * 2);
    for kind in PrincipalKind::ALL {
        for class in SideEffect::ALL {
            for human_only in [false, true] {
                let mut row = BTreeMap::new();
                row.insert("principal".to_string(), s(kind.as_str()));
                row.insert("class".to_string(), s(class.as_str()));
                row.insert("human_only".to_string(), CValue::Bool(human_only));
                match class_rule(*kind, *class, human_only) {
                    ClassRule::Allow => {
                        row.insert("allow".to_string(), CValue::Bool(true));
                    }
                    ClassRule::Refuse(reason) => {
                        row.insert("allow".to_string(), CValue::Bool(false));
                        row.insert("code".to_string(), s("policy"));
                        row.insert("reason".to_string(), s(reason.as_str()));
                    }
                }
                rows.push(CValue::Obj(row));
            }
        }
    }
    let mut o = BTreeMap::new();
    o.insert("node".to_string(), s(MATRIX_NODE));
    o.insert("principal_kinds".to_string(), CValue::Arr(PrincipalKind::ALL.iter().map(|k| s(k.as_str())).collect()));
    o.insert("classes".to_string(), CValue::Arr(SideEffect::ALL.iter().map(|c| s(c.as_str())).collect()));
    o.insert("rows".to_string(), CValue::Arr(rows));
    o.insert("schema_version".to_string(), CValue::Num(f64::from(MATRIX_SCHEMA_VERSION)));
    CValue::Obj(o)
}

/// The canonical bytes of `policy-matrix.json`, without a trailing newline.
pub fn policy_matrix_json() -> Result<Vec<u8>, CanonError> {
    to_canonical_bytes(&policy_matrix_value())
}
