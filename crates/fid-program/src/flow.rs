//! Control flow of Command Programs: loops, branches, jumps and subroutines. Spec node
//! `AUTO.PROG.FLOW`.
//!
//! [`analyse_flow`] lowers `main` and every subroutine body into one control-flow graph
//! ([`Cfg`]) each, builds the call graph, checks the static rules of
//! REQ-AUTO.PROG.FLOW-001 to -008 and -010 and returns everything in a [`FlowReport`]. It
//! never evaluates anything and never fails: every problem is a [`FlowDiag`], in a fixed
//! order, so the report is byte-identical across runs and platforms (AC-6). Every walk uses an
//! explicit stack; only `BTreeMap`/`BTreeSet` are used.
//!
//! Every choice below is pinned here and stated in the PR that built this leaf.
//!
//! **Nodes.** Every step is a node (labels and comments too), in document order, depth
//! first. A CFG's `entry` is the first step of its body, or [`Target::Exit`] for an empty
//! body. `main` is a unit like a subroutine ([`SubId::Main`]) and is void.
//!
//! **Successors.** The successor of a step is the next step of its block. After the last
//! step of a block it is: for an `if` branch or `else` body, the successor of the `if`; for
//! a loop body, the loop step itself with kind `loop_back`; for `main` or a subroutine body,
//! `exit` with kind `next` (falling off the end).
//!
//! **Edges** (REQ-009), per step kind, in this order:
//! - `invoke`, `assign`, `operator`, `comment`, `label`: one edge to the successor.
//! - `if`: one `true` edge per branch, in branch order, to the branch's first step (to the
//!   `if`'s successor when the body is empty), then one `false` edge to the `else` body (same
//!   rule). The interpreter tests the conditions in order and takes the `true` edge of the
//!   first that holds, else the `false` edge.
//! - `loop`: `true` to the first body step (`loop_back` to the loop itself when the body is
//!   empty), then `loop_exit` to the loop's successor.
//! - `jump`: `jump` to the label step when the target is legal and after the jump (none
//!   otherwise); a conditional jump then has `false` to its successor.
//! - `call`: `call` to [`Target::Entry`] of the callee when it is known, then the edge to
//!   its successor.
//! - `return`: `return` to `exit`.
//!
//! Decision edges keep their kind (`true`, `false`, `loop_exit`) even when they lead to a
//! loop step; `next` and `loop_back` are the plain fall-through.
//!
//! **Iteration limits** (REQ-001). A loop's `max_iterations` must be in
//! [`MIN_ITERATIONS`]`..=`[`MAX_ITERATIONS`], else `bad_iteration_limit`. `AUTO.PROG.FMT`
//! already refuses, at parse, a loop with no `max_iterations` or with 0, so such a document
//! never becomes a [`Program`]; [`iteration_limits`] reads the raw document and gives those
//! loops `missing_iteration_limit` and `bad_iteration_limit` instead of a schema error.
//!
//! **Conditions and bounds** (REQ-002). Types come from the [`TypeReport`]: an `if` branch
//! condition, a `while` condition and a conditional jump's condition must be `bool`
//! (`bad_condition`), and a counted loop's `from`, `to` and `step` must be `int`
//! (`bad_loop_bounds`). An expression the type checker could not type is already reported
//! there and is not reported again.
//!
//! **Jumps** (REQ-003, -004). Labels are looked up in the jump's own unit. A jump binds to
//! the first label of that name; every later label of the name is `duplicate_label`. A
//! jump is legal when every body that encloses the label also encloses the jump (it may
//! leave bodies, never enter one). Otherwise `illegal_jump` names the label and a reason:
//! `into_body`, `other_subroutine` (the label exists only in another unit) or `undefined`.
//!
//! **Backward jumps** (REQ-010). A jump whose label passes the rule above but is at or before
//! the jump in document order (depth first, as the nodes) is `backward_jump`, at the jump,
//! naming the label; it has no `jump` edge either. A jump is judged by one rule only: an
//! `illegal_jump` is not also a `backward_jump`. So every cycle within a unit passes a loop's
//! `loop_back` edge and is bounded by [`MAX_ITERATIONS`].
//!
//! **Calls** (REQ-005). A call binds to the first subroutine of that name. The format has no
//! parameter defaults, so every parameter is required. `bad_call` names the parameter (or
//! the subroutine) and a reason: `unknown_subroutine`, `missing` (parameters in declared
//! order), then per argument in name order `extra` or `wrong_type`. An expression argument
//! is of the wrong type when the type checker reported a whole-expression `type_mismatch` at
//! `args.<name>` of the call; a literal argument when its JSON form does not fit a `bool`,
//! `int`, `decimal`, `string` or `enum<...>` parameter (literals for other types are not
//! checked here).
//!
//! **Returns** (REQ-006). In a subroutine with `returns`, `missing_return` (at the
//! subroutine) when, with every condition unknown, a path from its entry reaches `exit`
//! other than through a `return`, including an empty body. `bad_return` (at the `return`
//! step) names a reason: `value_in_void` (a value returned from `main` or a void
//! subroutine), `no_value` (no value where a type is declared) or `wrong_type` (the type
//! checker reported a whole-expression `type_mismatch` at the step's `expr`).
//!
//! **Reachability** (REQ-007). From the entry of `main`, over every CFG edge, a `call` edge
//! leading to the callee's entry; a call also falls through to its successor. Every step not
//! reached is `unreachable_step` (warning), so the steps of a subroutine no reachable step
//! calls are all reported.
//!
//! **Recursion** (REQ-008). Each strongly connected component of the call graph with more
//! than one member, or one member that calls itself, is one `recursion` (info) naming the
//! members in document order. Recursion is legal; the interpreter bounds it by
//! [`MAX_CALL_DEPTH`].
//!
//! **Order.** By unit (`main`, then subroutines in document order): the unit's own
//! diagnostics (`missing_return`) first, then by step position; at one step by code in the
//! declaration order of [`FlowCode`], then in check order. `recursion` comes last, ordered
//! by its first member.
//!
//! **Run-time stops.** [`iteration_stop`] and [`call_stop`] state the stops that
//! `AUTO.PROG.INTERP.ENGINE` enforces (REQ-AUTO.PROG-007).

use crate::expr::types::{DiagCode, Site, Type, TypeReport};
use crate::fmt::{
    Arg, Bound, LoopMode, Program, ProgramError, Step, StepKind, Subroutine, Uuid7, MAX_BYTES, MAX_DEPTH,
};
use crate::Span;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use std::collections::{BTreeMap, BTreeSet};

/// Smallest accepted `max_iterations`.
pub const MIN_ITERATIONS: u32 = 1;
/// Largest accepted `max_iterations` (REQ-AUTO.PROG.FLOW-001) [unverified].
pub const MAX_ITERATIONS: u32 = 1_000_000;
/// Deepest call nesting a run may reach (REQ-AUTO.PROG-007) [unverified].
pub const MAX_CALL_DEPTH: u32 = 64;

// ------------------------------------------------------------------ model

/// A body unit: `main`, or a subroutine by its index in [`Program::subroutines`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubId {
    Main,
    Sub(usize),
}

impl SubId {
    fn rank(self) -> usize {
        match self {
            SubId::Main => 0,
            SubId::Sub(i) => i.saturating_add(1),
        }
    }
}

/// Why control moves along an edge (REQ-AUTO.PROG.FLOW-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    Next,
    True,
    False,
    LoopBack,
    LoopExit,
    Jump,
    Call,
    Return,
}

impl EdgeKind {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Next => "next",
            EdgeKind::True => "true",
            EdgeKind::False => "false",
            EdgeKind::LoopBack => "loop_back",
            EdgeKind::LoopExit => "loop_exit",
            EdgeKind::Jump => "jump",
            EdgeKind::Call => "call",
            EdgeKind::Return => "return",
        }
    }
}

/// Where an edge leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// A step of the same unit.
    Step(Uuid7),
    /// The end of the unit (back to the caller, or the end of the run for `main`).
    Exit,
    /// The entry of a subroutine (the target of a `call` edge).
    Entry(SubId),
}

/// One CFG edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: Uuid7,
    pub to: Target,
    pub kind: EdgeKind,
}

/// The control-flow graph of one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cfg {
    pub subroutine: SubId,
    /// The subroutine's name; `None` for `main`.
    pub name: Option<String>,
    pub entry: Target,
    /// Every step of the unit, in document order.
    pub nodes: Vec<Uuid7>,
    /// Grouped by `from` in node order; per step in the order of the module docs.
    pub edges: Vec<Edge>,
}

impl Cfg {
    /// The successors of `step`, in edge order.
    pub fn successors(&self, step: Uuid7) -> Vec<(Target, EdgeKind)> {
        self.edges.iter().filter(|e| e.from == step).map(|e| (e.to, e.kind)).collect()
    }
}

/// Which unit calls which (caller, callee); calls to unknown subroutines are not edges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallGraph {
    pub edges: BTreeSet<(SubId, SubId)>,
}

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// Diagnostic codes (REQ-AUTO.PROG.FLOW-001 to -008, -010). Declaration order is report
/// order at one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlowCode {
    MissingIterationLimit,
    BadIterationLimit,
    BadCondition,
    BadLoopBounds,
    IllegalJump,
    BackwardJump,
    DuplicateLabel,
    BadCall,
    MissingReturn,
    BadReturn,
    UnreachableStep,
    Recursion,
}

impl FlowCode {
    /// Stable machine-readable code.
    pub fn as_str(self) -> &'static str {
        match self {
            FlowCode::MissingIterationLimit => "missing_iteration_limit",
            FlowCode::BadIterationLimit => "bad_iteration_limit",
            FlowCode::BadCondition => "bad_condition",
            FlowCode::BadLoopBounds => "bad_loop_bounds",
            FlowCode::IllegalJump => "illegal_jump",
            FlowCode::BackwardJump => "backward_jump",
            FlowCode::DuplicateLabel => "duplicate_label",
            FlowCode::BadCall => "bad_call",
            FlowCode::MissingReturn => "missing_return",
            FlowCode::BadReturn => "bad_return",
            FlowCode::UnreachableStep => "unreachable_step",
            FlowCode::Recursion => "recursion",
        }
    }

    /// `unreachable_step` is a warning, `recursion` information, the rest errors.
    pub fn severity(self) -> Severity {
        match self {
            FlowCode::UnreachableStep => Severity::Warning,
            FlowCode::Recursion => Severity::Info,
            _ => Severity::Error,
        }
    }
}

/// One control-flow diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDiag {
    pub code: FlowCode,
    /// The unit the problem is in (for `recursion`, its first member).
    pub subroutine: SubId,
    /// The step, or `None` for a unit-level diagnostic (`missing_return`, `recursion`).
    pub step: Option<Uuid7>,
    /// What the code names (see the module docs per code).
    pub names: Vec<String>,
}

impl FlowDiag {
    /// [`FlowCode::severity`] of the code.
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }
}

/// What [`analyse_flow`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowReport {
    pub cfgs: BTreeMap<SubId, Cfg>,
    pub call_graph: CallGraph,
    pub diagnostics: Vec<FlowDiag>,
}

impl FlowReport {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity() == Severity::Error)
    }

    fn sub_value(&self, s: SubId) -> CValue {
        match self.cfgs.get(&s).and_then(|c| c.name.as_ref()) {
            Some(n) => CValue::Str(n.clone()),
            None => CValue::Null,
        }
    }

    fn target_value(&self, t: Target) -> CValue {
        match t {
            Target::Step(s) => CValue::Str(s.to_string()),
            Target::Exit => CValue::Str("exit".into()),
            Target::Entry(s) => match self.sub_value(s) {
                CValue::Str(n) => CValue::Str(format!("sub:{n}")),
                _ => CValue::Str("sub:".into()),
            },
        }
    }

    /// `{"call_graph": [{"callee", "caller"}], "cfgs": [{"edges": [{"from", "kind", "to"}],
    /// "entry", "nodes", "subroutine"}]}`. A unit is `null` (`main`) or its name; a target is
    /// a step ID, `"exit"` or `"sub:<name>"`. Serialise with `fid-canon`.
    pub fn cfg_value(&self) -> CValue {
        let calls = self
            .call_graph
            .edges
            .iter()
            .map(|(a, b)| obj([("callee", self.sub_value(*b)), ("caller", self.sub_value(*a))]))
            .collect();
        let cfgs = self
            .cfgs
            .values()
            .map(|c| {
                let edges = c
                    .edges
                    .iter()
                    .map(|e| {
                        obj([
                            ("from", CValue::Str(e.from.to_string())),
                            ("kind", CValue::Str(e.kind.as_str().into())),
                            ("to", self.target_value(e.to)),
                        ])
                    })
                    .collect();
                obj([
                    ("edges", CValue::Arr(edges)),
                    ("entry", self.target_value(c.entry)),
                    ("nodes", CValue::Arr(c.nodes.iter().map(|n| CValue::Str(n.to_string())).collect())),
                    ("subroutine", self.sub_value(c.subroutine)),
                ])
            })
            .collect();
        obj([("call_graph", CValue::Arr(calls)), ("cfgs", CValue::Arr(cfgs))])
    }

    /// `[{"code", "names", "severity", "step", "subroutine"}]`, in report order.
    pub fn diagnostics_value(&self) -> CValue {
        CValue::Arr(self.diagnostics.iter().map(|d| diag_value(d, self.sub_value(d.subroutine))).collect())
    }

    /// [`FlowReport::cfg_value`] plus `"diagnostics"`.
    pub fn to_value(&self) -> CValue {
        let mut v = match self.cfg_value() {
            CValue::Obj(m) => m,
            _ => BTreeMap::new(),
        };
        v.insert("diagnostics".into(), self.diagnostics_value());
        CValue::Obj(v)
    }
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn diag_value(d: &FlowDiag, sub: CValue) -> CValue {
    obj([
        ("code", CValue::Str(d.code.as_str().into())),
        ("names", CValue::Arr(d.names.iter().map(|n| CValue::Str(n.clone())).collect())),
        ("severity", CValue::Str(d.severity().as_str().into())),
        ("step", d.step.map_or(CValue::Null, |s| CValue::Str(s.to_string()))),
        ("subroutine", sub),
    ])
}

/// Renders diagnostics of [`iteration_limits`] (whose units have no CFG): a unit is `null`
/// for `main`, else the subroutine's index as a number.
pub fn document_diagnostics_value(diags: &[FlowDiag]) -> CValue {
    CValue::Arr(
        diags
            .iter()
            .map(|d| {
                let sub = match d.subroutine {
                    SubId::Main => CValue::Null,
                    SubId::Sub(i) => CValue::Num(i as f64),
                };
                diag_value(d, sub)
            })
            .collect(),
    )
}

// ------------------------------------------------------------------ run-time stops

/// A typed run-time stop the interpreter journals (REQ-AUTO.PROG-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunStop {
    IterationLimit,
    DepthLimit,
}

impl RunStop {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            RunStop::IterationLimit => "iteration_limit",
            RunStop::DepthLimit => "depth_limit",
        }
    }
}

/// Before the body of a loop runs for the `iteration`-th time (1-based), the run stops with
/// `iteration_limit` if that exceeds the loop's `max_iterations`.
pub fn iteration_stop(iteration: u64, max_iterations: u32) -> Option<RunStop> {
    (iteration > u64::from(max_iterations)).then_some(RunStop::IterationLimit)
}

/// Before a call enters a frame at `depth` (`main` is depth 0, its callees depth 1), the run
/// stops with `depth_limit` if that exceeds [`MAX_CALL_DEPTH`].
pub fn call_stop(depth: u32) -> Option<RunStop> {
    (depth > MAX_CALL_DEPTH).then_some(RunStop::DepthLimit)
}

// ------------------------------------------------------------------ walk

type Path = Vec<(Uuid7, usize)>;

/// One step as the walk meets it.
struct Visit<'a> {
    step: &'a Step,
    /// Enclosing bodies, outermost first: (container step, body index), where a loop body is
    /// 0, `if` branch `i` is `i` and `else` is the number of branches.
    path: Path,
    succ: Target,
    succ_kind: EdgeKind,
}

struct Frame<'a> {
    steps: &'a [Step],
    i: usize,
    follow: Target,
    end_kind: EdgeKind,
    path: Path,
}

/// A frame for a nested body: where control goes after it, and its place in `path`.
fn child<'a>(steps: &'a [Step], after: (Target, EdgeKind), path: &Path, at: (Uuid7, usize)) -> Frame<'a> {
    let mut p = path.clone();
    p.push(at);
    Frame { steps, i: 0, follow: after.0, end_kind: after.1, path: p }
}

fn first(steps: &[Step], or: Target) -> Target {
    steps.first().map_or(or, |s| Target::Step(s.step_id))
}

/// Every step of `body` in document order (depth first), with an explicit stack.
fn visits(body: &[Step]) -> Vec<Visit<'_>> {
    let mut out = Vec::new();
    let mut stack = vec![Frame { steps: body, i: 0, follow: Target::Exit, end_kind: EdgeKind::Next, path: Vec::new() }];
    while let Some(mut f) = stack.pop() {
        let Some(step) = f.steps.get(f.i) else { continue };
        let (succ, succ_kind) = match f.steps.get(f.i.saturating_add(1)) {
            Some(n) => (Target::Step(n.step_id), EdgeKind::Next),
            None => (f.follow, f.end_kind),
        };
        let id = step.step_id;
        let mut kids = Vec::new();
        match &step.kind {
            StepKind::If { branches, else_body } => {
                for (k, b) in branches.iter().enumerate() {
                    kids.push(child(&b.body, (succ, succ_kind), &f.path, (id, k)));
                }
                kids.push(child(else_body, (succ, succ_kind), &f.path, (id, branches.len())));
            }
            StepKind::Loop { body, .. } => {
                kids.push(child(body, (Target::Step(id), EdgeKind::LoopBack), &f.path, (id, 0)));
            }
            _ => {}
        }
        out.push(Visit { step, path: f.path.clone(), succ, succ_kind });
        f.i = f.i.saturating_add(1);
        stack.push(f);
        while let Some(k) = kids.pop() {
            stack.push(k);
        }
    }
    out
}

struct Unit<'a> {
    id: SubId,
    name: Option<&'a str>,
    returns: bool,
    visits: Vec<Visit<'a>>,
    label_names: BTreeSet<&'a str>,
}

fn unit<'a>(id: SubId, name: Option<&'a str>, returns: bool, body: &'a [Step]) -> Unit<'a> {
    let visits = visits(body);
    let label_names = visits
        .iter()
        .filter_map(|v| match &v.step.kind {
            StepKind::Label { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    Unit { id, name, returns, visits, label_names }
}

// ------------------------------------------------------------------ analysis

/// Sort key: (section, unit rank, 0 for the unit itself or 1 + step position, code).
type Key = (u8, usize, usize, FlowCode);

struct Analysis<'a> {
    types: &'a TypeReport,
    /// Whole-expression `type_mismatch` sites of the type report.
    mismatches: BTreeSet<(Uuid7, &'a str, Span)>,
    pos: BTreeMap<Uuid7, usize>,
    diags: Vec<(Key, FlowDiag)>,
}

impl<'a> Analysis<'a> {
    fn step_diag(&mut self, code: FlowCode, unit: SubId, step: Uuid7, names: Vec<String>) {
        let at = self.pos.get(&step).map_or(0, |p| p.saturating_add(1));
        self.diags.push(((0, unit.rank(), at, code), FlowDiag { code, subroutine: unit, step: Some(step), names }));
    }

    fn type_at(&self, step: Uuid7, slot: &str) -> Option<&'a Type> {
        self.types.types.get(&Site { step, slot: slot.to_string() }).map(|t| &t.ty)
    }

    /// Whether the type checker reported the whole expression at `slot` as mismatched.
    fn whole_mismatch(&self, step: Uuid7, slot: &str) -> bool {
        match self.types.types.get(&Site { step, slot: slot.to_string() }) {
            Some(t) => self.mismatches.contains(&(step, slot, t.span)),
            None => false,
        }
    }

    fn want(&mut self, code: FlowCode, unit: SubId, step: Uuid7, slot: &str, want: &Type) {
        if let Some(t) = self.type_at(step, slot) {
            if t != want {
                let found = t.to_string();
                self.step_diag(code, unit, step, vec![slot.to_string(), found]);
            }
        }
    }

    fn unit_cfg(
        &mut self,
        u: &Unit<'_>,
        units: &[Unit<'_>],
        program: &Program,
        subs: &BTreeMap<&str, usize>,
        calls: &mut BTreeSet<(SubId, SubId)>,
    ) -> Cfg {
        let mut labels: BTreeMap<&str, &Visit<'_>> = BTreeMap::new();
        for v in &u.visits {
            if let StepKind::Label { name } = &v.step.kind {
                if labels.contains_key(name.as_str()) {
                    self.step_diag(FlowCode::DuplicateLabel, u.id, v.step.step_id, vec![name.clone()]);
                } else {
                    labels.insert(name.as_str(), v);
                }
            }
        }
        let mut edges = Vec::new();
        for v in &u.visits {
            let id = v.step.step_id;
            let next = (v.succ, v.succ_kind);
            let out: Vec<(Target, EdgeKind)> = match &v.step.kind {
                StepKind::Invoke { .. }
                | StepKind::Assign { .. }
                | StepKind::Operator { .. }
                | StepKind::Comment { .. }
                | StepKind::Label { .. } => vec![next],
                StepKind::If { branches, else_body } => {
                    for k in 0..branches.len() {
                        self.want(FlowCode::BadCondition, u.id, id, &format!("branches.{k}.cond"), &Type::Bool);
                    }
                    let mut o: Vec<_> = branches.iter().map(|b| (first(&b.body, v.succ), EdgeKind::True)).collect();
                    o.push((first(else_body, v.succ), EdgeKind::False));
                    o
                }
                StepKind::Loop { mode, max_iterations, body } => {
                    if !(MIN_ITERATIONS..=MAX_ITERATIONS).contains(max_iterations) {
                        self.step_diag(FlowCode::BadIterationLimit, u.id, id, vec![max_iterations.to_string()]);
                    }
                    match mode {
                        LoopMode::Counted { .. } => {
                            for slot in ["from", "to", "step"] {
                                self.want(FlowCode::BadLoopBounds, u.id, id, slot, &Type::Int);
                            }
                        }
                        LoopMode::While { .. } => self.want(FlowCode::BadCondition, u.id, id, "cond", &Type::Bool),
                    }
                    let enter = match body.first() {
                        Some(s) => (Target::Step(s.step_id), EdgeKind::True),
                        None => (Target::Step(id), EdgeKind::LoopBack),
                    };
                    vec![enter, (v.succ, EdgeKind::LoopExit)]
                }
                StepKind::Jump { label, cond } => {
                    if cond.is_some() {
                        self.want(FlowCode::BadCondition, u.id, id, "cond", &Type::Bool);
                    }
                    let mut o = Vec::new();
                    if let Some(t) = self.resolve(u, &labels, units, v, label) {
                        o.push((Target::Step(t), EdgeKind::Jump));
                    }
                    if cond.is_some() {
                        o.push((v.succ, EdgeKind::False));
                    }
                    o
                }
                StepKind::Call { subroutine, args, .. } => {
                    let mut o = Vec::new();
                    match subs.get(subroutine.as_str()).and_then(|&i| program.subroutines.get(i).map(|s| (i, s))) {
                        Some((i, sub)) => {
                            calls.insert((u.id, SubId::Sub(i)));
                            self.call_args(u.id, id, sub, args);
                            o.push((Target::Entry(SubId::Sub(i)), EdgeKind::Call));
                        }
                        None => self.step_diag(
                            FlowCode::BadCall,
                            u.id,
                            id,
                            vec![subroutine.clone(), "unknown_subroutine".into()],
                        ),
                    }
                    o.push(next);
                    o
                }
                StepKind::Return { expr } => {
                    let reason = match (u.returns, expr.is_some()) {
                        (false, true) => Some("value_in_void"),
                        (true, false) => Some("no_value"),
                        (true, true) if self.whole_mismatch(id, "expr") => Some("wrong_type"),
                        _ => None,
                    };
                    if let Some(r) = reason {
                        self.step_diag(FlowCode::BadReturn, u.id, id, vec![r.into()]);
                    }
                    vec![(Target::Exit, EdgeKind::Return)]
                }
            };
            edges.extend(out.into_iter().map(|(to, kind)| Edge { from: id, to, kind }));
        }
        let cfg = Cfg {
            subroutine: u.id,
            name: u.name.map(str::to_string),
            entry: u.visits.first().map_or(Target::Exit, |v| Target::Step(v.step.step_id)),
            nodes: u.visits.iter().map(|v| v.step.step_id).collect(),
            edges,
        };
        if u.returns && falls_off(&cfg) {
            let d = FlowDiag { code: FlowCode::MissingReturn, subroutine: u.id, step: None, names: Vec::new() };
            self.diags.push(((0, u.id.rank(), 0, FlowCode::MissingReturn), d));
        }
        cfg
    }

    /// The label step a jump binds to, or `illegal_jump` / `backward_jump` (REQ-003, -010).
    fn resolve(
        &mut self,
        u: &Unit<'_>,
        labels: &BTreeMap<&str, &Visit<'_>>,
        units: &[Unit<'_>],
        jump: &Visit<'_>,
        label: &str,
    ) -> Option<Uuid7> {
        let reason = match labels.get(label) {
            Some(l) if jump.path.starts_with(&l.path) => {
                let (to, from) = (l.step.step_id, jump.step.step_id);
                match (self.pos.get(&to), self.pos.get(&from)) {
                    (Some(t), Some(f)) if t > f => return Some(to),
                    _ => {
                        self.step_diag(FlowCode::BackwardJump, u.id, from, vec![label.to_string()]);
                        return None;
                    }
                }
            }
            Some(_) => "into_body",
            None if units.iter().any(|o| o.id != u.id && o.label_names.contains(label)) => "other_subroutine",
            None => "undefined",
        };
        self.step_diag(FlowCode::IllegalJump, u.id, jump.step.step_id, vec![label.to_string(), reason.into()]);
        None
    }

    fn call_args(&mut self, unit: SubId, id: Uuid7, sub: &Subroutine, args: &BTreeMap<String, Arg>) {
        let mut seen = BTreeSet::new();
        for p in &sub.params {
            if seen.insert(p.name.as_str()) && !args.contains_key(&p.name) {
                self.step_diag(FlowCode::BadCall, unit, id, vec![p.name.clone(), "missing".into()]);
            }
        }
        for (name, arg) in args {
            let reason = match sub.params.iter().find(|p| p.name == *name) {
                None => Some("extra"),
                Some(p) => {
                    let wrong = match arg {
                        Arg::Expr(_) => self.whole_mismatch(id, &format!("args.{name}")),
                        Arg::Literal(v) => literal_fits(&p.ty, v) == Some(false),
                    };
                    wrong.then_some("wrong_type")
                }
            };
            if let Some(r) = reason {
                self.step_diag(FlowCode::BadCall, unit, id, vec![name.clone(), r.into()]);
            }
        }
    }
}

/// Whether a literal argument fits a declared type source; `None` when not checked here.
fn literal_fits(ty: &str, v: &CValue) -> Option<bool> {
    let int = |v: &CValue| match v {
        CValue::Num(n) => n.is_finite() && n.fract() == 0.0,
        CValue::I64(_) | CValue::U64(_) => true,
        _ => false,
    };
    match ty {
        "bool" => Some(matches!(v, CValue::Bool(_))),
        "int" => Some(int(v)),
        "decimal" => Some(int(v) || matches!(v, CValue::Num(_) | CValue::F64Bits(_))),
        "string" => Some(matches!(v, CValue::Str(_))),
        _ => {
            let members = ty.strip_prefix("enum<")?.strip_suffix('>')?;
            Some(matches!(v, CValue::Str(s) if members.split('|').any(|m| m == s)))
        }
    }
}

/// Whether, with every condition unknown, a path from the entry reaches `exit` other than
/// through a `return` edge. `call` edges are not followed (the call falls through).
fn falls_off(cfg: &Cfg) -> bool {
    let mut by_from: BTreeMap<Uuid7, Vec<&Edge>> = BTreeMap::new();
    for e in &cfg.edges {
        by_from.entry(e.from).or_default().push(e);
    }
    let mut seen = BTreeSet::new();
    let mut work = vec![cfg.entry];
    while let Some(t) = work.pop() {
        match t {
            Target::Exit => return true,
            Target::Entry(_) => {}
            Target::Step(s) => {
                if !seen.insert(s) {
                    continue;
                }
                for e in by_from.get(&s).map(Vec::as_slice).unwrap_or_default() {
                    match (e.to, e.kind) {
                        (Target::Exit, EdgeKind::Return) | (Target::Entry(_), _) => {}
                        (to, _) => work.push(to),
                    }
                }
            }
        }
    }
    false
}

/// Every step reachable from the entry of `main` (REQ-007).
fn reachable(cfgs: &BTreeMap<SubId, Cfg>) -> BTreeSet<Uuid7> {
    let mut succ: BTreeMap<Uuid7, Vec<Target>> = BTreeMap::new();
    for e in cfgs.values().flat_map(|c| c.edges.iter()) {
        succ.entry(e.from).or_default().push(e.to);
    }
    let mut seen = BTreeSet::new();
    let mut entered = BTreeSet::from([SubId::Main]);
    let mut work: Vec<Target> = cfgs.get(&SubId::Main).map(|c| c.entry).into_iter().collect();
    while let Some(t) = work.pop() {
        match t {
            Target::Exit => {}
            Target::Entry(s) => {
                if entered.insert(s) {
                    work.extend(cfgs.get(&s).map(|c| c.entry));
                }
            }
            Target::Step(s) => {
                if seen.insert(s) {
                    work.extend(succ.get(&s).map(Vec::as_slice).unwrap_or_default().iter().copied());
                }
            }
        }
    }
    seen
}

/// Strongly connected components of the call graph that are cycles, each sorted, in order of
/// their first member (Tarjan, with an explicit stack).
fn cycles(units: &[SubId], edges: &BTreeSet<(SubId, SubId)>) -> Vec<Vec<SubId>> {
    let n = units.len();
    let at: BTreeMap<SubId, usize> = units.iter().enumerate().map(|(i, s)| (*s, i)).collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, b) in edges {
        if let (Some(&i), Some(&j)) = (at.get(a), at.get(b)) {
            if let Some(v) = adj.get_mut(i) {
                v.push(j);
            }
        }
    }
    let mut index: Vec<Option<usize>> = vec![None; n];
    let mut low = vec![0usize; n];
    let mut on = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut counter = 0usize;
    let mut out: Vec<Vec<SubId>> = Vec::new();
    for root in 0..n {
        if index.get(root).copied().flatten().is_some() {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        enter(root, &mut index, &mut low, &mut on, &mut stack, &mut counter);
        while let Some(&(u, ni)) = call.last() {
            let neighbours = adj.get(u).map(Vec::as_slice).unwrap_or_default();
            if let Some(&w) = neighbours.get(ni) {
                if let Some(top) = call.last_mut() {
                    top.1 = ni.saturating_add(1);
                }
                match index.get(w).copied().flatten() {
                    None => {
                        enter(w, &mut index, &mut low, &mut on, &mut stack, &mut counter);
                        call.push((w, 0));
                    }
                    Some(iw) if on.get(w).copied().unwrap_or(false) => {
                        if let Some(lu) = low.get_mut(u) {
                            *lu = (*lu).min(iw);
                        }
                    }
                    Some(_) => {}
                }
                continue;
            }
            call.pop();
            let lu = low.get(u).copied().unwrap_or(0);
            if let Some(&(p, _)) = call.last() {
                if let Some(lp) = low.get_mut(p) {
                    *lp = (*lp).min(lu);
                }
            }
            if Some(lu) == index.get(u).copied().flatten() {
                let mut comp = Vec::new();
                while let Some(w) = stack.pop() {
                    if let Some(o) = on.get_mut(w) {
                        *o = false;
                    }
                    comp.push(w);
                    if w == u {
                        break;
                    }
                }
                comp.sort_unstable();
                let self_loop = comp.len() == 1 && adj.get(u).is_some_and(|v| v.contains(&u));
                if comp.len() > 1 || self_loop {
                    out.push(comp.into_iter().filter_map(|i| units.get(i).copied()).collect());
                }
            }
        }
    }
    out.sort();
    out
}

fn enter(
    v: usize,
    index: &mut [Option<usize>],
    low: &mut [usize],
    on: &mut [bool],
    stack: &mut Vec<usize>,
    counter: &mut usize,
) {
    if let Some(i) = index.get_mut(v) {
        *i = Some(*counter);
    }
    if let Some(l) = low.get_mut(v) {
        *l = *counter;
    }
    if let Some(o) = on.get_mut(v) {
        *o = true;
    }
    stack.push(v);
    *counter = counter.saturating_add(1);
}

/// Lowers every unit of `program` into a CFG, builds the call graph and checks the static
/// control-flow rules (REQ-AUTO.PROG.FLOW-001 to -010). `types` is the report of
/// [`check_types`](crate::check_types) for the same program; conditions, loop bounds, call
/// arguments and return values are judged by the types it gives. See the module docs for
/// the rules and the diagnostic order. Never fails.
pub fn analyse_flow(program: &Program, types: &TypeReport) -> FlowReport {
    let mut units = vec![unit(SubId::Main, None, false, &program.main)];
    for (i, s) in program.subroutines.iter().enumerate() {
        units.push(unit(SubId::Sub(i), Some(&s.name), s.returns.is_some(), &s.body));
    }
    let mut pos = BTreeMap::new();
    for (n, v) in units.iter().flat_map(|u| u.visits.iter()).enumerate() {
        pos.entry(v.step.step_id).or_insert(n);
    }
    let mut subs: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, s) in program.subroutines.iter().enumerate() {
        subs.entry(s.name.as_str()).or_insert(i);
    }
    let mismatches = types
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagCode::TypeMismatch)
        .filter_map(|d| Some((d.step?, d.slot.as_str(), d.span?)))
        .collect();
    let mut a = Analysis { types, mismatches, pos, diags: Vec::new() };
    let mut cfgs = BTreeMap::new();
    let mut calls = BTreeSet::new();
    for u in &units {
        let cfg = a.unit_cfg(u, &units, program, &subs, &mut calls);
        cfgs.insert(u.id, cfg);
    }
    let seen = reachable(&cfgs);
    for c in cfgs.values() {
        for n in c.nodes.iter().filter(|n| !seen.contains(*n)) {
            a.step_diag(FlowCode::UnreachableStep, c.subroutine, *n, Vec::new());
        }
    }
    let ids: Vec<SubId> = units.iter().map(|u| u.id).collect();
    for comp in cycles(&ids, &calls) {
        let Some(&head) = comp.first() else { continue };
        let names = comp.iter().filter_map(|s| cfgs.get(s).and_then(|c| c.name.clone())).collect();
        let d = FlowDiag { code: FlowCode::Recursion, subroutine: head, step: None, names };
        a.diags.push(((1, head.rank(), 0, FlowCode::Recursion), d));
    }
    a.diags.sort_by_key(|(k, _)| *k);
    FlowReport {
        cfgs,
        call_graph: CallGraph { edges: calls },
        diagnostics: a.diags.into_iter().map(|(_, d)| d).collect(),
    }
}

// ------------------------------------------------------------------ raw documents

/// Checks the `max_iterations` of every loop in a raw program document, including the
/// documents [`Program::parse`] refuses for it: a loop without the field is
/// `missing_iteration_limit`; one whose value is not an integer in
/// [`MIN_ITERATIONS`]`..=`[`MAX_ITERATIONS`] is `bad_iteration_limit` naming the value's
/// canonical JSON (REQ-AUTO.PROG.FLOW-001, AC-3). Reads only `main`, `subroutines[].body`
/// and the step bodies (`body`, `branches[].body`, `else_body`) and skips anything else that
/// is malformed; [`Program::parse`] reports that. Diagnostics are in document order (`main`,
/// then subroutines; depth first); a step whose `step_id` is not a UUIDv7 has `step: None`.
/// Errors only when the bytes are not JSON within the format's size and depth bounds.
pub fn iteration_limits(bytes: &[u8]) -> Result<Vec<FlowDiag>, ProgramError> {
    if bytes.len() > MAX_BYTES {
        return Err(ProgramError::ProgramTooLarge(Bound::Bytes));
    }
    let doc = parse_with_limits(bytes, MAX_BYTES, MAX_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => ProgramError::ProgramTooLarge(Bound::Depth),
        other => ProgramError::InvalidJson(other.code()),
    })?;
    let mut out = Vec::new();
    let CValue::Obj(root) = &doc else { return Ok(out) };
    let mut bodies: Vec<(SubId, &CValue)> = Vec::new();
    if let Some(m) = root.get("main") {
        bodies.push((SubId::Main, m));
    }
    if let Some(CValue::Arr(subs)) = root.get("subroutines") {
        for (i, s) in subs.iter().enumerate() {
            if let Some(b) = field(s, "body") {
                bodies.push((SubId::Sub(i), b));
            }
        }
    }
    for (sub, body) in bodies {
        let CValue::Arr(steps) = body else { continue };
        let mut stack = vec![steps.iter()];
        while let Some(it) = stack.last_mut() {
            let Some(step) = it.next() else {
                stack.pop();
                continue;
            };
            let CValue::Obj(m) = step else { continue };
            if matches!(m.get("kind"), Some(CValue::Str(k)) if k == "loop") {
                let id = match m.get("step_id") {
                    Some(CValue::Str(s)) => Uuid7::parse(s),
                    _ => None,
                };
                let found = match m.get("max_iterations") {
                    None => Some((FlowCode::MissingIterationLimit, Vec::new())),
                    Some(v) if limit_ok(v) => None,
                    Some(v) => Some((FlowCode::BadIterationLimit, vec![canonical_text(v)])),
                };
                if let Some((code, names)) = found {
                    out.push(FlowDiag { code, subroutine: sub, step: id, names });
                }
            }
            let mut kids: Vec<&Vec<CValue>> = Vec::new();
            if let Some(CValue::Arr(branches)) = m.get("branches") {
                kids.extend(branches.iter().filter_map(|b| match field(b, "body") {
                    Some(CValue::Arr(a)) => Some(a),
                    _ => None,
                }));
            }
            for k in ["else_body", "body"] {
                if let Some(CValue::Arr(a)) = m.get(k) {
                    kids.push(a);
                }
            }
            while let Some(k) = kids.pop() {
                stack.push(k.iter());
            }
        }
    }
    Ok(out)
}

fn field<'a>(v: &'a CValue, k: &str) -> Option<&'a CValue> {
    match v {
        CValue::Obj(m) => m.get(k),
        _ => None,
    }
}

fn limit_ok(v: &CValue) -> bool {
    match v {
        CValue::Num(n) => n.fract() == 0.0 && *n >= f64::from(MIN_ITERATIONS) && *n <= f64::from(MAX_ITERATIONS),
        _ => false,
    }
}

fn canonical_text(v: &CValue) -> String {
    to_canonical_bytes(v).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| String::from("?"))
}
