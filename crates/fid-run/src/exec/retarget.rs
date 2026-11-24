//! The step-gate hook and the not-re-targetable step guard (spec node `AUTO.RUN.EXEC.RETGT`).
//!
//! **The hook.** [`StepGate`] is the pre-step hook `AUTO.RUN.EXEC.CMD` calls before every step
//! of a run, in chain order, with the step's dynamic key ([`StepKey`]: step ID and iteration
//! path) and what the step will do ([`StepInfo`]); [`StepGate::after_step`] after each
//! committed step and [`StepGate::before_end`] before the run is finalised `succeeded`. A gate
//! answers [`GateDecision::Allow`], [`GateDecision::Stop`] (code and detail: the run ends
//! `failed` with that code) or [`GateDecision::Hold`] (the run waits at the step boundary).
//! `AUTO.RUN.EXEC.CMD` re-exports these types and adapts the chain to the interpreter's own
//! `StepHook` (`AUTO.PROG.INTERP.ENGINE`), which cannot depend on this crate.
//!
//! **The guard** ([`RetargetGuard`], the first gate of every run):
//!
//! - **Re-targeted run** (REQ-001). Built once from the [`RunPlan`]: the run is re-targeted
//!   when, for any bound input parameter (every entry of the record's `inputs`), the
//!   parameter has no recorded default digest ([`RunPlan::default_digests`]) or the bound
//!   input's SHA-256 differs from it. The differing parameters are kept in name order.
//! - **Blocked steps.** The step IDs listed `false` in [`RunPlan::steps_retargetable`].
//! - **Decision** (REQ-002, REQ-003). [`StepGate::before_step`] returns
//!   `Stop(not_retargetable, detail)` for a re-targeted run at a blocked step, before the step
//!   is invoked, and `Allow` otherwise. The detail is
//!   `{"iteration_path": [<n>, ..], "params": ["<param>", ..], "step_id": "<uuidv7>"}`
//!   ([`NotRetargetable`]). The guard never holds, and it allows the run's end.
//! - **Purity** (REQ-004). The decision is a function of the plan and the step key only: no
//!   I/O, no clock, no state changed by `after_step`. O(log n) per step (a `BTreeSet` lookup).
//!
//! The guard compares digests only; it never reads an input. The digests come from
//! `prepare_run` (`AUTO.RUN.BIND`), which hashes the bound inputs and the default of every
//! `data_handle` parameter through the host's input source.
//!
//! **Not settled by this leaf** (NODE.md, Open questions): no stored program can mark a step
//! `retargetable: false` yet (`AUTO.PROG.FMT` has no per-step field; `AUTO.REC.LIFT` sets it),
//! so `prepare_run` lists every step re-targetable and the guard allows every step of a real
//! plan. A program stores a default's handle, not its digest, so the recorded default is the
//! dataset that handle names when the run is prepared.

use crate::prepare::RunPlan;
use fid_canon::CValue;
use fid_program::Uuid7;
use fid_types::{CommandId, SideEffect};
use std::collections::{BTreeMap, BTreeSet};

/// The stop code of the guard.
pub const NOT_RETARGETABLE: &str = "not_retargetable";

/// One dynamic execution of a step: its ID and the iteration counts of the loops around it,
/// outermost first (`AUTO.PROG.INTERP.ENGINE`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StepKey {
    /// The step's ID in the program.
    pub step_id: Uuid7,
    /// Iteration counts of the enclosing loops, outermost first; empty outside any loop.
    pub iteration_path: Vec<u32>,
}

impl StepKey {
    /// A step outside any loop.
    pub fn top(step_id: Uuid7) -> StepKey {
        StepKey { step_id, iteration_path: Vec::new() }
    }
}

/// What a step about to run will do, as the interpreter knows it before invoking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInfo {
    /// The command an `invoke` step issues (`None` for an operator step).
    pub command: Option<CommandId>,
    /// The command's side-effect class, from the registry (`None` for an operator step).
    pub class: Option<SideEffect>,
}

/// What a committed step returned, handed to [`StepGate::after_step`].
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    /// The command the step issued (`None` for an operator step).
    pub command: Option<CommandId>,
    /// The command's result document (an operator step's recorded response).
    pub output: CValue,
}

/// A gate's answer at a step boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// Run the step (or end the run).
    Allow,
    /// Stop the run before the step: the stable code and a detail document. The run ends
    /// `failed` with this code (`AUTO.RUN.EXEC.CMD`).
    Stop(&'static str, CValue),
    /// Wait at the boundary, for the given display-safe reason, until the gate allows or stops.
    Hold(String),
}

/// A pre-step hook of a run (module docs). Gates are called in chain order; the first `Stop`
/// ends the run.
pub trait StepGate {
    /// Called before `key` is invoked.
    fn before_step(&self, key: &StepKey, info: &StepInfo) -> GateDecision;

    /// Called after `key` committed. The default does nothing.
    fn after_step(&mut self, _key: &StepKey, _result: &StepResult) {}

    /// Called before the run is finalised `succeeded`. The default allows it.
    fn before_end(&self) -> GateDecision {
        GateDecision::Allow
    }
}

/// The typed detail of a `not_retargetable` stop: the step, its iteration path and the
/// differing input parameters (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRetargetable {
    /// The blocked step and its iteration path.
    pub step: StepKey,
    /// The bound input parameters whose digest differs from the recorded default (or that
    /// have no recorded default), in name order.
    pub params: Vec<String>,
}

impl NotRetargetable {
    /// The detail document of the stop (module docs).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert(
            "iteration_path".to_owned(),
            CValue::Arr(self.step.iteration_path.iter().map(|n| CValue::Num(f64::from(*n))).collect()),
        );
        m.insert("params".to_owned(), CValue::Arr(self.params.iter().map(|p| CValue::Str(p.clone())).collect()));
        m.insert("step_id".to_owned(), CValue::Str(self.step.step_id.to_string()));
        CValue::Obj(m)
    }

    /// Reads a detail document back; `None` if it is not exactly that shape.
    pub fn from_value(v: &CValue) -> Option<NotRetargetable> {
        let CValue::Obj(m) = v else { return None };
        if m.len() != 3 {
            return None;
        }
        let step_id = match m.get("step_id")? {
            CValue::Str(s) => Uuid7::parse(s)?,
            _ => return None,
        };
        let CValue::Arr(path) = m.get("iteration_path")? else { return None };
        let mut iteration_path = Vec::with_capacity(path.len());
        for n in path {
            match n {
                CValue::Num(x) if *x >= 0.0 && *x <= f64::from(u32::MAX) && x.fract() == 0.0 => {
                    iteration_path.push(*x as u32)
                }
                _ => return None,
            }
        }
        let CValue::Arr(ps) = m.get("params")? else { return None };
        let mut params = Vec::with_capacity(ps.len());
        for p in ps {
            match p {
                CValue::Str(s) => params.push(s.clone()),
                _ => return None,
            }
        }
        Some(NotRetargetable { step: StepKey { step_id, iteration_path }, params })
    }
}

/// The not-re-targetable step guard (module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetargetGuard {
    retargeted: bool,
    differing: Vec<String>,
    blocked: BTreeSet<Uuid7>,
}

impl RetargetGuard {
    /// Builds the guard of a run from its plan (REQ-001).
    pub fn from_plan(plan: &RunPlan) -> RetargetGuard {
        let differing: Vec<String> = plan
            .record
            .inputs()
            .iter()
            .filter(|(param, input)| plan.default_digests.get(*param) != Some(&input.sha256))
            .map(|(param, _)| param.clone())
            .collect();
        let blocked = plan.steps_retargetable.iter().filter(|(_, ok)| !*ok).map(|(id, _)| *id).collect();
        RetargetGuard { retargeted: !differing.is_empty(), differing, blocked }
    }

    /// Whether the run is re-targeted.
    pub fn retargeted(&self) -> bool {
        self.retargeted
    }

    /// The differing input parameters, in name order (empty when not re-targeted).
    pub fn differing(&self) -> &[String] {
        &self.differing
    }

    /// The step IDs marked `retargetable: false`.
    pub fn blocked(&self) -> &BTreeSet<Uuid7> {
        &self.blocked
    }

    /// The guard's decision for `key` (REQ-002, REQ-003): a pure function of the plan the guard
    /// was built from and the key.
    pub fn decide(&self, key: &StepKey) -> GateDecision {
        if self.retargeted && self.blocked.contains(&key.step_id) {
            let detail = NotRetargetable { step: key.clone(), params: self.differing.clone() };
            GateDecision::Stop(NOT_RETARGETABLE, detail.to_value())
        } else {
            GateDecision::Allow
        }
    }
}

impl StepGate for RetargetGuard {
    fn before_step(&self, key: &StepKey, _info: &StepInfo) -> GateDecision {
        self.decide(key)
    }
}
