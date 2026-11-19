//! Governing specs per characteristic, plan and tenant defaults, the W2 command
//! `gdt.set_governing_spec@1`, creation-time inheritance and the evaluation gate.
//!
//! **Branches (CONSTITUTION §3 W2, C5; the rule of `GDT.CHAR.DPROF`).** A human principal
//! writes the working branch; agents and scripts write an agent overlay that sees the working
//! branch beneath it. Nothing an agent does changes or blocks the working branch: an
//! agent-created characteristic or an agent-set spec lives only in the overlay, and the
//! working branch can only name profiles held on the working branch, so an agent-published
//! profile is `defaults_profile_unknown` there until a human adopts it.
//!
//! **Bindings.** The profile store of `GDT.CHAR.DPROF` keeps a per-characteristic profile
//! binding, which its re-point command reads and moves. Here the governing spec is the
//! authority and the binding its mirror: each successful create or set writes the binding of
//! the characteristic on the principal's branch, [`SpecState::repoint_defaults_profile`]
//! moves both together, and the gate refuses a characteristic whose binding and spec differ.
//!
//! **Case.** Characteristic and plan IDs may hold upper case. Two IDs that differ only in
//! ASCII case are refused (`char_case_conflict`, `bad_request{plan_case_conflict}`); the
//! working branch is checked on its own, so an agent-branch ID never blocks a human.

use super::event::SpecEvent;
use super::types::{parse_dialect_wire, GoverningSpec, InheritedFrom, PlanId, Standards, StdRef, MAX_STANDARDS};
use super::{echo, SpecError};
use crate::chars::dprof::{Branch, CharId, DprofState, ProfileRef, Repointed};
use fid_canon::CValue;
use fid_io_reg::Principal;
use std::collections::BTreeMap;

/// Most characteristics per branch (the binding cap of `GDT.CHAR.DPROF`).
pub const MAX_CHARS: usize = crate::chars::dprof::store::MAX_BINDINGS;
/// Most plans with a default governing spec.
pub const MAX_PLANS: usize = 10_000;

/// The parameters of `gdt.set_governing_spec@1`, as given on the command line or over MCP.
#[derive(Debug, Clone, Copy)]
pub struct SetRequest<'a> {
    /// `char`: the characteristic ID.
    pub char: &'a str,
    /// `dialect`: `ASME` or `ISO`.
    pub dialect: &'a str,
    /// `standards`: one or more `std:<body>:<designator>:<edition>`.
    pub standards: &'a [&'a str],
    /// `defaults_profile`: `dp:<dialect>-<edition>@<x.y.z>#sha256:<hex>`.
    pub defaults_profile: &'a str,
}

/// The result of `gdt.set_governing_spec@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSpec {
    char: CharId,
    old: Option<GoverningSpec>,
    new: GoverningSpec,
    branch: Branch,
    dry_run: bool,
    event: Option<SpecEvent>,
}

impl SetSpec {
    /// The characteristic.
    pub fn char(&self) -> &CharId {
        &self.char
    }
    /// The spec before, as seen from the principal's branch.
    pub fn old(&self) -> Option<&GoverningSpec> {
        self.old.as_ref()
    }
    /// The spec after.
    pub fn new_spec(&self) -> &GoverningSpec {
        &self.new
    }
    /// The branch written.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    /// The journal event; `None` for a dry run.
    pub fn event(&self) -> Option<&SpecEvent> {
        self.event.as_ref()
    }
    /// The `--json` / MCP result: `{branch, char, dry_run, governing_spec}`.
    pub fn to_cvalue(&self) -> CValue {
        result(self.branch, &self.char, self.dry_run, Some(&self.new))
    }
}

/// The result of creation-time inheritance ([`SpecState::create_characteristic`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    char: CharId,
    spec: Option<GoverningSpec>,
    branch: Branch,
    dry_run: bool,
    event: Option<SpecEvent>,
}

impl Created {
    /// The characteristic.
    pub fn char(&self) -> &CharId {
        &self.char
    }
    /// The inherited spec; `None` when neither a plan nor a tenant default exists.
    pub fn spec(&self) -> Option<&GoverningSpec> {
        self.spec.as_ref()
    }
    /// The branch written.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    /// The journal event; `None` when nothing was inherited, or for a dry run.
    pub fn event(&self) -> Option<&SpecEvent> {
        self.event.as_ref()
    }
    /// The `--json` / MCP result: `{branch, char, dry_run, governing_spec}` (`null` spec when
    /// nothing was inherited).
    pub fn to_cvalue(&self) -> CValue {
        result(self.branch, &self.char, self.dry_run, self.spec.as_ref())
    }
}

fn result(branch: Branch, c: &CharId, dry_run: bool, spec: Option<&GoverningSpec>) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("branch".to_owned(), CValue::Str(branch.as_str().to_owned()));
    m.insert("char".to_owned(), CValue::Str(c.as_str().to_owned()));
    m.insert("dry_run".to_owned(), CValue::Bool(dry_run));
    m.insert("governing_spec".to_owned(), super::types::opt_cvalue(spec));
    CValue::Obj(m)
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Checks that `r` names a profile selectable on `branch` with that exact digest:
/// `defaults_profile_digest_mismatch` when a profile of the same dialect, edition and version
/// is selectable there with another digest, `defaults_profile_unknown` when none is.
pub fn check_profile(store: &DprofState, branch: Branch, r: &ProfileRef) -> Result<(), SpecError> {
    if store.resolve(branch, r).is_ok() {
        return Ok(());
    }
    let same_version = store.get(Branch::Working, r.id()).is_some()
        || (branch == Branch::Agent && store.get(Branch::Agent, r.id()).is_some());
    let reference = echo(&r.to_string());
    if same_version {
        Err(SpecError::DefaultsProfileDigestMismatch { reference })
    } else {
        Err(SpecError::DefaultsProfileUnknown { reference })
    }
}

/// Governing specs of every characteristic on the working branch and the agent overlay, and
/// the plan and tenant defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpecState {
    pub(super) working: BTreeMap<CharId, Option<GoverningSpec>>,
    pub(super) agent: BTreeMap<CharId, Option<GoverningSpec>>,
    working_ci: BTreeMap<String, CharId>,
    agent_ci: BTreeMap<String, CharId>,
    pub(super) plans: BTreeMap<PlanId, GoverningSpec>,
    plans_ci: BTreeMap<String, PlanId>,
    pub(super) tenant: Option<GoverningSpec>,
}

impl SpecState {
    /// No characteristics and no defaults.
    pub fn new() -> SpecState {
        SpecState::default()
    }

    /// The characteristics held on `branch` itself, by ID, with their specs.
    pub fn characteristics(&self, branch: Branch) -> impl Iterator<Item = (&CharId, Option<&GoverningSpec>)> {
        let map = match branch {
            Branch::Working => &self.working,
            Branch::Agent => &self.agent,
        };
        map.iter().map(|(c, s)| (c, s.as_ref()))
    }

    /// A characteristic's governing spec as seen from `branch` (the agent overlay first, then
    /// the working branch): `None` when the characteristic is unknown there, `Some(None)` when
    /// it has no governing spec.
    pub fn governing_spec(&self, branch: Branch, c: &CharId) -> Option<Option<&GoverningSpec>> {
        let rec = match branch {
            Branch::Working => self.working.get(c),
            Branch::Agent => self.agent.get(c).or_else(|| self.working.get(c)),
        };
        rec.map(Option::as_ref)
    }

    /// The default of `plan`, if any.
    pub fn plan_default(&self, plan: &PlanId) -> Option<&GoverningSpec> {
        self.plans.get(plan)
    }

    /// The tenant default, if any.
    pub fn tenant_default(&self) -> Option<&GoverningSpec> {
        self.tenant.as_ref()
    }

    /// What a characteristic created in `plan` inherits (REQ-004): a copy of the plan
    /// default marked `plan`, else of the tenant default marked `tenant`, else nothing. Pure;
    /// this is the body of the `SpecInheritor` that `GDT.CHAR.MODEL.CMDS` injects.
    pub fn inherit(&self, plan: &PlanId) -> Option<GoverningSpec> {
        match (self.plans.get(plan), &self.tenant) {
            (Some(p), _) => Some(p.with_inherited_from(InheritedFrom::Plan)),
            (None, Some(t)) => Some(t.with_inherited_from(InheritedFrom::Tenant)),
            (None, None) => None,
        }
    }

    pub(super) fn put(&mut self, branch: Branch, id: CharId, spec: Option<GoverningSpec>) {
        let (map, ci) = match branch {
            Branch::Working => (&mut self.working, &mut self.working_ci),
            Branch::Agent => (&mut self.agent, &mut self.agent_ci),
        };
        ci.insert(lower(id.as_str()), id.clone());
        map.insert(id, spec);
    }

    /// Whether `branch`'s own map already holds an ID equal to `id` ignoring ASCII case.
    pub(super) fn case_taken(&self, branch: Branch, id: &CharId) -> bool {
        let ci = match branch {
            Branch::Working => &self.working_ci,
            Branch::Agent => &self.agent_ci,
        };
        ci.contains_key(&lower(id.as_str()))
    }

    pub(super) fn put_plan(&mut self, plan: PlanId, spec: GoverningSpec) {
        self.plans_ci.insert(lower(plan.as_str()), plan.clone());
        self.plans.insert(plan, spec);
    }

    pub(super) fn plan_case_taken(&self, plan: &PlanId) -> bool {
        self.plans_ci.contains_key(&lower(plan.as_str()))
    }

    /// Finds a characteristic as seen from `branch`: `bad_request{bad_char_id}`,
    /// `char_case_conflict` (another ID on a visible branch equals it ignoring case), or
    /// `char_unknown`.
    fn lookup(&self, branch: Branch, text: &str) -> Result<(CharId, Option<&GoverningSpec>), SpecError> {
        let id = CharId::parse(text).ok_or_else(|| SpecError::bad_request("bad_char_id"))?;
        let l = lower(id.as_str());
        let conflict = |ci: &BTreeMap<String, CharId>| ci.get(&l).is_some_and(|k| *k != id);
        if conflict(&self.working_ci) || (branch == Branch::Agent && conflict(&self.agent_ci)) {
            return Err(SpecError::CharCaseConflict { char: id.as_str().to_owned() });
        }
        match self.governing_spec(branch, &id) {
            Some(s) => Ok((id, s)),
            None => Err(SpecError::CharUnknown { char: id.as_str().to_owned() }),
        }
    }

    /// Creation-time inheritance (REQ-004, REQ-005): registers characteristic `char` in
    /// `plan` on the principal's branch with a copy of what [`Self::inherit`] gives, binds it
    /// in `dprof` when the copied profile resolves there, and returns a
    /// `gdt.set_governing_spec@1` event with `old: null`, the plan, and
    /// `inherited_from: plan|tenant`. Nothing is journaled when nothing is inherited (the
    /// characteristic then has no governing spec). Checks, in order: `bad_request{bad_plan_id}`,
    /// `bad_request{plan_case_conflict}`, `bad_request{bad_char_id}`, `char_exists` /
    /// `char_case_conflict` (the working branch; an agent also its overlay),
    /// `bad_request{too_many_chars}`. A dry run changes nothing and returns no event.
    pub fn create_characteristic(
        &mut self,
        dprof: &mut DprofState,
        principal: Principal,
        plan: &str,
        char: &str,
        dry_run: bool,
    ) -> Result<Created, SpecError> {
        let plan = PlanId::parse(plan).ok_or_else(|| SpecError::bad_request("bad_plan_id"))?;
        if self.plans_ci.get(&lower(plan.as_str())).is_some_and(|k| *k != plan) {
            return Err(SpecError::bad_request("plan_case_conflict"));
        }
        let id = CharId::parse(char).ok_or_else(|| SpecError::bad_request("bad_char_id"))?;
        let branch = Branch::for_principal(principal);
        let l = lower(id.as_str());
        let agent_ci = (branch == Branch::Agent).then_some(&self.agent_ci);
        for ci in std::iter::once(&self.working_ci).chain(agent_ci) {
            if let Some(k) = ci.get(&l) {
                if *k == id {
                    return Err(SpecError::CharExists { char: id.as_str().to_owned() });
                }
                return Err(SpecError::CharCaseConflict { char: id.as_str().to_owned() });
            }
        }
        let own = match branch {
            Branch::Working => &self.working,
            Branch::Agent => &self.agent,
        };
        if own.len() >= MAX_CHARS {
            return Err(SpecError::bad_request("too_many_chars"));
        }
        let spec = self.inherit(&plan);
        let event = spec.as_ref().map(|s| SpecEvent::new(principal, id.clone(), Some(plan.clone()), None, s.clone()));
        if dry_run {
            return Ok(Created { char: id, spec, branch, dry_run, event: None });
        }
        if let Some(s) = &spec {
            if check_profile(dprof, branch, s.defaults_profile()).is_ok() {
                dprof.set_binding(branch, &id, s.defaults_profile())?;
            }
        }
        self.put(branch, id.clone(), spec.clone());
        Ok(Created { char: id, spec, branch, dry_run, event })
    }

    /// `gdt.set_governing_spec@1` (W2; CLI `fid gdt set-governing-spec`, MCP
    /// `gdt_set_governing_spec`): replaces the characteristic's governing spec on the
    /// principal's branch with an explicit one, binds the profile in `dprof`, and returns one
    /// event with the old and new spec and `inherited_from: explicit` (REQ-006). All checks
    /// run before any change, in this order: `bad_request{bad_char_id}`; the dialect
    /// (`governing_spec_missing{dialect}` when empty, `bad_request{bad_dialect}`); the
    /// standards (`governing_spec_missing{standards}` when none,
    /// `bad_request{too_many_standards}`, `bad_std_ref` each in the order given,
    /// `bad_request{duplicate_standard}`, `bad_request{conflicting_editions}`); the profile
    /// (`governing_spec_missing{defaults_profile}` when empty, `bad_profile_ref`); consistency
    /// (`dialect_mismatch`, `defaults_profile_edition_mismatch`); the characteristic
    /// (`char_case_conflict`, `char_unknown`); the profile store on the principal's branch
    /// (`defaults_profile_digest_mismatch`, `defaults_profile_unknown`). A refusal and a dry
    /// run change nothing and journal nothing.
    pub fn set_governing_spec(
        &mut self,
        dprof: &mut DprofState,
        principal: Principal,
        req: &SetRequest<'_>,
        dry_run: bool,
    ) -> Result<SetSpec, SpecError> {
        CharId::parse(req.char).ok_or_else(|| SpecError::bad_request("bad_char_id"))?;
        if req.dialect.is_empty() {
            return Err(SpecError::missing("dialect"));
        }
        let dialect = parse_dialect_wire(req.dialect).ok_or_else(|| SpecError::bad_request("bad_dialect"))?;
        if req.standards.is_empty() {
            return Err(SpecError::missing("standards"));
        }
        if req.standards.len() > MAX_STANDARDS {
            return Err(SpecError::bad_request("too_many_standards"));
        }
        let refs = req.standards.iter().map(|s| StdRef::parse(s)).collect::<Result<Vec<_>, _>>()?;
        let standards = Standards::new(refs)?;
        if req.defaults_profile.is_empty() {
            return Err(SpecError::missing("defaults_profile"));
        }
        let profile = ProfileRef::parse(req.defaults_profile).ok_or(SpecError::BadProfileRef)?;
        let new = GoverningSpec::new(dialect, standards, profile, InheritedFrom::Explicit)?;
        let branch = Branch::for_principal(principal);
        let (id, old) = self.lookup(branch, req.char)?;
        let old = old.cloned();
        check_profile(dprof, branch, new.defaults_profile())?;
        if dry_run {
            return Ok(SetSpec { char: id, old, new, branch, dry_run, event: None });
        }
        let own_len = match branch {
            Branch::Working => self.working.len(),
            Branch::Agent => self.agent.len(),
        };
        let adds = match branch {
            Branch::Working => !self.working.contains_key(&id),
            Branch::Agent => !self.agent.contains_key(&id),
        };
        if adds && own_len >= MAX_CHARS {
            return Err(SpecError::bad_request("too_many_chars"));
        }
        dprof.set_binding(branch, &id, new.defaults_profile())?;
        self.put(branch, id.clone(), Some(new.clone()));
        let event = SpecEvent::new(principal, id.clone(), None, old.clone(), new.clone());
        Ok(SetSpec { char: id, old, new, branch, dry_run, event: Some(event) })
    }

    /// Sets (`Some`) or clears (`None`) the default governing spec of `plan`. Human only
    /// (`human_only`); then `bad_request{bad_plan_id}`, `bad_request{plan_case_conflict}`,
    /// `bad_request{default_not_explicit}` (a default's origin is `explicit`), the profile on
    /// the working branch (`defaults_profile_digest_mismatch`, `defaults_profile_unknown`),
    /// `bad_request{too_many_plans}`. Never changes an existing characteristic (REQ-005).
    pub fn set_plan_default(
        &mut self,
        store: &DprofState,
        principal: Principal,
        plan: &str,
        spec: Option<GoverningSpec>,
    ) -> Result<(), SpecError> {
        if principal != Principal::Human {
            return Err(SpecError::HumanOnly);
        }
        let plan = PlanId::parse(plan).ok_or_else(|| SpecError::bad_request("bad_plan_id"))?;
        if self.plans_ci.get(&lower(plan.as_str())).is_some_and(|k| *k != plan) {
            return Err(SpecError::bad_request("plan_case_conflict"));
        }
        let spec = match spec {
            None => {
                self.plans.remove(&plan);
                self.plans_ci.remove(&lower(plan.as_str()));
                return Ok(());
            }
            Some(s) => s,
        };
        check_default(store, &spec)?;
        if !self.plans.contains_key(&plan) && self.plans.len() >= MAX_PLANS {
            return Err(SpecError::bad_request("too_many_plans"));
        }
        self.put_plan(plan, spec);
        Ok(())
    }

    /// Sets or clears the tenant default. Human only; the same spec checks as
    /// [`Self::set_plan_default`]. Never changes an existing characteristic (REQ-005).
    pub fn set_tenant_default(
        &mut self,
        store: &DprofState,
        principal: Principal,
        spec: Option<GoverningSpec>,
    ) -> Result<(), SpecError> {
        if principal != Principal::Human {
            return Err(SpecError::HumanOnly);
        }
        if let Some(s) = &spec {
            check_default(store, s)?;
        }
        self.tenant = spec;
        Ok(())
    }

    /// The gate every `check.eval_*` command calls before computing (REQ-003). The branch is
    /// the principal's ([`Branch::for_principal`]): a human sees the working branch only, an
    /// agent or script its overlay over it. Checks, in
    /// order: `bad_request{bad_char_id}`, `char_case_conflict`, `char_unknown`, no spec
    /// (`governing_spec_missing{dialect}`), the profile in `store` on `branch`
    /// (`defaults_profile_digest_mismatch`, `defaults_profile_unknown`), the store's binding
    /// (`defaults_profile_binding_mismatch`). Returns the spec to evaluate under.
    pub fn require_governing_spec(
        &self,
        principal: Principal,
        char: &str,
        store: &DprofState,
    ) -> Result<&GoverningSpec, SpecError> {
        let branch = Branch::for_principal(principal);
        let (id, spec) = self.lookup(branch, char)?;
        let spec = spec.ok_or_else(|| SpecError::missing("dialect"))?;
        check_profile(store, branch, spec.defaults_profile())?;
        if store.binding(branch, &id) != Some(spec.defaults_profile()) {
            return Err(SpecError::DefaultsProfileBindingMismatch { char: id.as_str().to_owned() });
        }
        Ok(spec)
    }

    /// Runs `eval` (which builds the ResultRecord) only when [`Self::require_governing_spec`]
    /// passes; on refusal `eval` never runs, so no ResultRecord exists.
    pub fn evaluate<R>(
        &self,
        principal: Principal,
        char: &str,
        store: &DprofState,
        eval: impl FnOnce(&GoverningSpec) -> R,
    ) -> Result<R, SpecError> {
        self.require_governing_spec(principal, char, store).map(eval)
    }

    /// `gdt.repoint_defaults_profile@1` of `GDT.CHAR.DPROF`, keeping governing specs in step:
    /// runs the re-point's own checks as a dry run, then checks that each named
    /// characteristic has a governing spec visible on the principal's branch
    /// (`char_case_conflict`, `char_unknown`, `governing_spec_missing{dialect}`) whose
    /// standards fit the target's edition (`defaults_profile_edition_mismatch`), and only then
    /// re-points and writes each spec on the principal's branch with origin `explicit` (the
    /// characteristic no longer follows the plan or tenant default).
    /// Returns the re-point result and its `gdt.repoint_defaults_profile@1` event.
    pub fn repoint_defaults_profile(
        &mut self,
        dprof: &mut DprofState,
        principal: Principal,
        chars: &[&str],
        to: &str,
        dry_run: bool,
    ) -> Result<Repointed, SpecError> {
        let planned = dprof.repoint(principal, chars, to, true)?;
        let branch = planned.branch();
        let mut updates = Vec::with_capacity(planned.changes().len());
        for ch in planned.changes() {
            let (id, spec) = self.lookup(branch, ch.char().as_str())?;
            let spec = spec.ok_or_else(|| SpecError::missing("dialect"))?;
            updates.push((id, spec.with_profile(ch.new_ref())?));
        }
        if dry_run {
            return Ok(planned);
        }
        let own = match branch {
            Branch::Working => &self.working,
            Branch::Agent => &self.agent,
        };
        let adds = updates.iter().filter(|(id, _)| !own.contains_key(id)).count();
        if own.len().saturating_add(adds) > MAX_CHARS {
            return Err(SpecError::bad_request("too_many_chars"));
        }
        let done = dprof.repoint(principal, chars, to, false)?;
        for (id, spec) in updates {
            self.put(branch, id, Some(spec));
        }
        Ok(done)
    }
}

fn check_default(store: &DprofState, spec: &GoverningSpec) -> Result<(), SpecError> {
    if spec.inherited_from() != InheritedFrom::Explicit {
        return Err(SpecError::bad_request("default_not_explicit"));
    }
    check_profile(store, Branch::Working, spec.defaults_profile())
}

pub(super) fn check_loaded_default(store: &DprofState, spec: &GoverningSpec) -> Result<(), SpecError> {
    if spec.inherited_from() != InheritedFrom::Explicit {
        return Err(SpecError::project("default_not_explicit"));
    }
    check_profile(store, Branch::Working, spec.defaults_profile())
}
