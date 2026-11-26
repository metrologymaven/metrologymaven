//! Static program checker: `program.check@1`. Spec node `AUTO.PROG.CHECK`.
//!
//! [`check_program`] runs every static rule over one program against one manifest and returns
//! one ordered diagnostic list plus a summary of what the program may do ([`CheckResult`]).
//! It parses the program (`AUTO.PROG.FMT`), types it (`AUTO.PROG.EXPR.TYPES`), analyses its
//! control flow (`AUTO.PROG.FLOW`), validates every operator descriptor (`AUTO.PROG.OPER`)
//! and the part-family declarations (`AUTO.PROG.FAMILY`), then resolves every `invoke`
//! against the manifest and checks its arguments (this module). It never evaluates an
//! expression, never touches state and never fails: every problem is a [`CheckDiag`]
//! (REQ-AUTO.PROG.CHECK-001 to -007). The interpreter and the editor call it as a library;
//! the handler of `program.check@1` (reading a program by ID or read-root handle) and its CLI
//! and MCP forms are not built yet (see the leaf's Open questions).
//!
//! Every choice below is pinned here and stated in the PR that built this leaf.
//!
//! **Parse failure.** A program that does not parse gives one `fmt` diagnostic with the
//! format's error code and nothing is typed or classified (`program_class` is `null`, no
//! steps). When the code is `schema_invalid`, the raw document's iteration limits are read
//! with [`iteration_limits`] and reported as `flow` diagnostics, so a loop with
//! `max_iterations: 0` is `bad_iteration_limit` rather than only a schema error. A parsed
//! program never goes through [`iteration_limits`], so each loop's limit is reported once
//! (the obligation `AUTO.PROG.FLOW` records on this node).
//!
//! **Command resolution** (REQ-001, REQ-005). An `invoke` resolves when the manifest holds
//! its exact (command, major) and that major's lifecycle `state` is `active` or
//! `deprecated`. Anything else is `unknown_command` (error) naming `command@major`: an
//! absent command or major, a `removed` one, a `hidden` one (only the replay engine's
//! `system` principal may call a hidden major, and no program runs as it) and one whose
//! lifecycle state cannot be read (fail closed). The message is the same in every case, so
//! the result does not say which. A `deprecated` major is also `deprecated_command`
//! (warning) naming `command@major` and, when the lifecycle has `replaced_by`, the
//! replacement.
//!
//! **Arguments** (REQ-002). For a resolved `invoke`, against its `params_schema`: a name in
//! its `required` list that the step does not pass is `missing`; an argument whose name is not
//! under `properties` is `unknown`; a literal argument is validated against its own property
//! schema by the bus's JSON Schema validator (`fid_bus::validate::Compiled`, with the same
//! quantity-literal normalisation the bus applies) and is `invalid` when it fails; an
//! expression argument is `wrong_type` when the type checker reported a whole-expression
//! `type_mismatch` at `args.<name>` (its inferred type, quantity kind included, does not fit
//! the parameter), the rule `AUTO.PROG.FLOW` uses for call arguments. A property schema is
//! compiled on its own with the root's `$defs` beside it; one that does not compile that way
//! (a `$ref` outside `$defs`, a keyword the bus refuses) is `unchecked`, an error, so a
//! literal is never passed without a check (fail closed). Entity references (`x-ref`,
//! `x-selector`) are removed from the property schema first: whether an entity exists is state,
//! which only the bus sees at run time, so a reference literal is checked for its JSON shape
//! only. Keywords of the root `params_schema` other than `properties` and `required` are not
//! checked statically; the bus checks the whole document at run time. All offending parameters
//! of one step are one `bad_arguments` (error), names sorted by byte order, each named once.
//!
//! **Classes** (REQ-004). Each `invoke` is listed in document order with its command, the
//! side-effect class and `human_only` flag of its definition (`null` for an unresolved one)
//! and whether control flow can reach it (`AUTO.PROG.FLOW`: a step without an
//! `unreachable_step` diagnostic). The program class is the highest class over the reachable
//! invokes in the order R0 < R1 < I1 < W1 < W2 < X1 < D1 < W3 < M1 < S1 < A0 ([`CLASS_ORDER`]),
//! `R0` when none is reachable, and `null` when a reachable invoke did not resolve (its class is
//! unknown, so no class is claimed).
//!
//! **Audience** (D-035). The caller passes the manifest its audience sees:
//! [`check_in_force`] gives an agent the registry's agent view
//! ([`ManifestInForce::agent_view`]), in which a never-projected command does not exist and a
//! `replaced_by` outside the view is removed. A never-projected command is therefore
//! `unknown_command` to an agent, byte-identical to a real miss, and no module sees its
//! schemas.
//!
//! **Diagnostics** (REQ-003). Every module's diagnostics are collected, each with its step (or
//! `null` for a declaration or a whole unit), the field it is in (`slot`, type checker only),
//! its span inside that field's expression source, its module, code, severity, the names its
//! module gives and a message. Severities: type, operator, family, format and check errors
//! are `error` (`deprecated_command` is `warning`); flow diagnostics keep their own
//! (`unreachable_step` warning, `recursion` info). The message is the code, then ` at <slot>`
//! when there is a slot, then `: ` and the names joined by `, ` when there are names;
//! `bad_arguments` lists each parameter with its reason in parentheses. Order: diagnostics
//! without a step first, then by step position in document order (`main`, then subroutines,
//! depth first, as [`Program::step_ids`]); then by module in the order of [`Module`]; then by
//! code (byte order); ties keep the module's own order. `passed` is true only when no
//! diagnostic is an error (REQ-007).
//!
//! **Determinism** (REQ-006). No clock, no environment, no hash map: the same program bytes,
//! manifest and catalogue give the same [`CheckResult`], whose [`CheckResult::to_value`]
//! serialises byte-identically with `fid-canon`. `program_digest` is the SHA-256 of the
//! program bytes as given (for a program that parses, its canonical bytes, so equal to
//! [`Program::digest`]); `manifest_digest` is the digest the caller passes for the manifest.

use crate::expr::types::{check_types, DiagCode, Site, TypeReport};
use crate::family::{BindingError, Family};
use crate::flow::{analyse_flow, iteration_limits, FlowCode, FlowDiag, Severity};
use crate::fmt::{Arg, Program, ProgramError, Step, StepKind, Uuid7};
use crate::oper::{FieldError, OperatorDescriptor};
use crate::Span;
use fid_bus::error::BusError;
use fid_bus::unit::{normalise_value, Normalised};
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{digest_bytes, CValue};
use fid_registry::query::Audience;
use fid_registry::{
    CommandDef, CommandRef, DefError, Manifest, ManifestError, ManifestInForce, QuantityCatalogue, State,
};
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Command ID of the checker.
pub const CHECK_COMMAND: &str = "program.check@1";
/// CLI form: `fid program check`.
pub const CLI_CHECK: (&str, &str) = ("program", "check");
/// MCP tool name.
pub const MCP_CHECK: &str = "program_check";
/// Longest name echoed into a diagnostic, in bytes (cut at a char boundary).
const MAX_ECHO: usize = 64;
/// Longest program handle the command accepts, in bytes.
pub const MAX_HANDLE_BYTES: usize = 4096;
const LIFECYCLE_SINCE: &str = "2026-11-25";

/// The side-effect classes in the order of REQ-AUTO.PROG.CHECK-004, lowest first. This is the
/// node's reviewer-facing ranking; it only orders the summary.
pub const CLASS_ORDER: [SideEffect; 11] = [
    SideEffect::R0,
    SideEffect::R1,
    SideEffect::I1,
    SideEffect::W1,
    SideEffect::W2,
    SideEffect::X1,
    SideEffect::D1,
    SideEffect::W3,
    SideEffect::M1,
    SideEffect::S1,
    SideEffect::A0,
];

/// The position of `class` in [`CLASS_ORDER`].
pub fn class_rank(class: SideEffect) -> usize {
    CLASS_ORDER.iter().position(|c| *c == class).unwrap_or(CLASS_ORDER.len())
}

// ------------------------------------------------------------------ model

/// Which check found a diagnostic. Declaration order is report order at one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Module {
    Fmt,
    Expr,
    Flow,
    Oper,
    Family,
    Check,
}

impl Module {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Module::Fmt => "fmt",
            Module::Expr => "expr",
            Module::Flow => "flow",
            Module::Oper => "oper",
            Module::Family => "family",
            Module::Check => "check",
        }
    }
}

/// Codes this module adds (REQ-001, -002, -005).
pub const UNKNOWN_COMMAND: &str = "unknown_command";
/// See [`UNKNOWN_COMMAND`].
pub const BAD_ARGUMENTS: &str = "bad_arguments";
/// See [`UNKNOWN_COMMAND`].
pub const DEPRECATED_COMMAND: &str = "deprecated_command";

/// Why an argument of an `invoke` is offending (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgProblem {
    /// A required parameter the step does not pass.
    Missing,
    /// A name the parameter schema does not declare.
    Unknown,
    /// A literal that fails its JSON Schema.
    Invalid,
    /// An expression whose inferred type does not fit the parameter.
    WrongType,
    /// A literal whose property schema the checker cannot compile on its own.
    Unchecked,
}

impl ArgProblem {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            ArgProblem::Missing => "missing",
            ArgProblem::Unknown => "unknown",
            ArgProblem::Invalid => "invalid",
            ArgProblem::WrongType => "wrong_type",
            ArgProblem::Unchecked => "unchecked",
        }
    }
}

/// One diagnostic of the whole check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiag {
    /// The step, or `None` for a declaration, a whole unit or the document.
    pub step_id: Option<Uuid7>,
    /// The step field (or declaration path) holding the problem; type checker only.
    pub slot: Option<String>,
    /// Byte span inside that field's expression source.
    pub span: Option<Span>,
    pub module: Module,
    pub code: &'static str,
    pub severity: Severity,
    /// What the code names, as its module gives it (parameters for `bad_arguments`).
    pub names: Vec<String>,
    pub message: String,
}

impl CheckDiag {
    /// `{"code", "message", "module", "names", "severity", "slot", "span": {"end", "start"} |
    /// null, "step_id"}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".into(), CValue::Str(self.code.into()));
        o.insert("message".into(), CValue::Str(self.message.clone()));
        o.insert("module".into(), CValue::Str(self.module.as_str().into()));
        o.insert("names".into(), CValue::Arr(self.names.iter().map(|n| CValue::Str(n.clone())).collect()));
        o.insert("severity".into(), CValue::Str(self.severity.as_str().into()));
        o.insert("slot".into(), self.slot.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone())));
        o.insert("span".into(), self.span.map_or(CValue::Null, span_value));
        o.insert("step_id".into(), self.step_id.map_or(CValue::Null, |s| CValue::Str(s.to_string())));
        CValue::Obj(o)
    }
}

fn span_value(s: Span) -> CValue {
    let n = |x: usize| CValue::Num(x as f64);
    CValue::Obj(BTreeMap::from([("end".to_string(), n(s.end)), ("start".to_string(), n(s.start))]))
}

/// The class of one `invoke` (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepClass {
    pub step_id: Uuid7,
    /// `command@major` as the step names it.
    pub command: String,
    /// The definition's class; `None` when the command did not resolve.
    pub class: Option<SideEffect>,
    /// The definition's `human_only`; `None` when the command did not resolve.
    pub human_only: Option<bool>,
    /// Whether control flow can reach the step.
    pub reachable: bool,
}

impl StepClass {
    /// `{"class", "command", "human_only", "reachable", "step_id"}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("class".into(), self.class.map_or(CValue::Null, |c| CValue::Str(c.as_str().into())));
        o.insert("command".into(), CValue::Str(self.command.clone()));
        o.insert("human_only".into(), self.human_only.map_or(CValue::Null, CValue::Bool));
        o.insert("reachable".into(), CValue::Bool(self.reachable));
        o.insert("step_id".into(), CValue::Str(self.step_id.to_string()));
        CValue::Obj(o)
    }
}

/// What [`check_program`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub program_digest: Digest,
    pub manifest_digest: Digest,
    /// No diagnostic is an error (REQ-007).
    pub passed: bool,
    /// The highest class over the reachable invokes; `None` when the program did not parse or
    /// a reachable invoke did not resolve.
    pub program_class: Option<SideEffect>,
    /// Every `invoke`, in document order.
    pub steps: Vec<StepClass>,
    pub diagnostics: Vec<CheckDiag>,
}

impl CheckResult {
    /// `{"diagnostics", "manifest_digest", "passed", "program_class", "program_digest",
    /// "steps"}`; serialise with `fid-canon` for byte-identical output.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("diagnostics".into(), CValue::Arr(self.diagnostics.iter().map(CheckDiag::to_value).collect()));
        o.insert("manifest_digest".into(), CValue::Str(self.manifest_digest.to_string()));
        o.insert("passed".into(), CValue::Bool(self.passed));
        o.insert("program_class".into(), self.program_class.map_or(CValue::Null, |c| CValue::Str(c.as_str().into())));
        o.insert("program_digest".into(), CValue::Str(self.program_digest.to_string()));
        o.insert("steps".into(), CValue::Arr(self.steps.iter().map(StepClass::to_value).collect()));
        CValue::Obj(o)
    }
}

/// Why [`check_in_force`] could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// The manifest view in force does not read back as a manifest (`manifest_invalid`).
    Manifest(ManifestError),
}

impl CheckError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            CheckError::Manifest(e) => e.code(),
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Manifest(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CheckError {}

// ------------------------------------------------------------------ entry points

/// Checks `program` (the stored bytes) against `manifest`, whose digest the caller gives, with
/// the kinds and units of `catalogue`. See the module docs. Never fails.
pub fn check_program(
    program: &[u8],
    manifest: &Manifest,
    manifest_digest: Digest,
    catalogue: &dyn QuantityCatalogue,
) -> CheckResult {
    let program_digest = Digest::from_bytes(digest_bytes(program).0);
    let mut c = Collector::default();
    let parsed = match Program::parse(program) {
        Ok(p) => p,
        Err(e) => {
            c.push(0, diag(None, None, None, Module::Fmt, e.code(), Severity::Error, Vec::new()));
            if matches!(e, ProgramError::SchemaInvalid { .. }) {
                if let Ok(found) = iteration_limits(program) {
                    for (i, d) in found.into_iter().enumerate() {
                        c.push(i.saturating_add(1), flow_diag(d));
                    }
                }
            }
            return c.finish(program_digest, manifest_digest, None, Vec::new());
        }
    };
    let mut pos = BTreeMap::new();
    for (i, id) in parsed.step_ids().into_iter().enumerate() {
        pos.entry(id).or_insert(i.saturating_add(1));
    }
    let at = |step: Option<Uuid7>| step.and_then(|s| pos.get(&s).copied()).unwrap_or(0);

    let types = check_types(&parsed, manifest, catalogue);
    let flow = analyse_flow(&parsed, &types);
    for d in &types.diagnostics {
        let names = d.names.clone();
        c.push(
            at(d.step),
            diag(d.step, Some(d.slot.clone()), d.span, Module::Expr, d.code.as_str(), Severity::Error, names),
        );
    }
    let mut unreachable = BTreeSet::new();
    for d in &flow.diagnostics {
        if d.code == FlowCode::UnreachableStep {
            if let Some(s) = d.step {
                unreachable.insert(s);
            }
        }
        c.push(at(d.step), flow_diag(d.clone()));
    }
    if let Err(errors) = Family::from_program(&parsed, catalogue) {
        for e in errors {
            c.push(0, family_diag(&e));
        }
    }

    let mut ck = Checker { manifest, catalogue, types: &types, compiled: BTreeMap::new() };
    let mut steps = Vec::new();
    let mut found: Vec<(Uuid7, CheckDiag)> = Vec::new();
    each_step(&parsed, &mut |s: &Step| match &s.kind {
        StepKind::Operator { descriptor } => {
            if let Err(errors) = OperatorDescriptor::from_object(descriptor, catalogue) {
                for e in errors {
                    found.push((s.step_id, oper_diag(s.step_id, &e)));
                }
            }
        }
        StepKind::Invoke { command, major, args, .. } => {
            let (class, diags) = ck.invoke(s.step_id, command, *major, args);
            steps.push(StepClass {
                step_id: s.step_id,
                command: format!("{}@{major}", echo(command)),
                class: class.map(|(c, _)| c),
                human_only: class.map(|(_, h)| h),
                reachable: !unreachable.contains(&s.step_id),
            });
            found.extend(diags.into_iter().map(|d| (s.step_id, d)));
        }
        _ => {}
    });
    for (step, d) in found {
        c.push(at(Some(step)), d);
    }
    let reachable: Vec<&StepClass> = steps.iter().filter(|s| s.reachable).collect();
    let program_class = if reachable.iter().any(|s| s.class.is_none()) {
        None
    } else {
        Some(reachable.iter().filter_map(|s| s.class).fold(SideEffect::R0, |a, b| {
            if class_rank(b) > class_rank(a) {
                b
            } else {
                a
            }
        }))
    };
    c.finish(program_digest, manifest_digest, program_class, steps)
}

/// [`check_program`] against the manifest in force, as `audience` sees it (D-035): a person gets
/// the full manifest and its digest, an agent the agent view and its digest.
pub fn check_in_force(
    program: &[u8],
    manifest: &ManifestInForce,
    audience: Audience,
) -> Result<CheckResult, CheckError> {
    let view = match audience {
        Audience::Person => manifest.export(),
        Audience::Agent => manifest.agent_view(),
    };
    let m = Manifest::from_json(&view.bytes).map_err(CheckError::Manifest)?;
    Ok(check_program(program, &m, view.digest, manifest.catalogue()))
}

// ------------------------------------------------------------------ collection

#[derive(Default)]
struct Collector {
    items: Vec<(usize, CheckDiag)>,
}

impl Collector {
    fn push(&mut self, pos: usize, d: CheckDiag) {
        self.items.push((pos, d));
    }

    fn finish(
        mut self,
        program_digest: Digest,
        manifest_digest: Digest,
        program_class: Option<SideEffect>,
        steps: Vec<StepClass>,
    ) -> CheckResult {
        // Stable: ties keep each module's own order.
        self.items.sort_by(|(pa, a), (pb, b)| (pa, a.module, a.code).cmp(&(pb, b.module, b.code)));
        let diagnostics: Vec<CheckDiag> = self.items.into_iter().map(|(_, d)| d).collect();
        let passed = !diagnostics.iter().any(|d| d.severity == Severity::Error);
        CheckResult { program_digest, manifest_digest, passed, program_class, steps, diagnostics }
    }
}

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or_default().to_string()
}

fn message(code: &str, slot: Option<&str>, names: &[String]) -> String {
    let mut m = code.to_string();
    if let Some(s) = slot {
        m.push_str(" at ");
        m.push_str(s);
    }
    if !names.is_empty() {
        m.push_str(": ");
        m.push_str(&names.join(", "));
    }
    m
}

fn diag(
    step_id: Option<Uuid7>,
    slot: Option<String>,
    span: Option<Span>,
    module: Module,
    code: &'static str,
    severity: Severity,
    names: Vec<String>,
) -> CheckDiag {
    let message = message(code, slot.as_deref(), &names);
    CheckDiag { step_id, slot, span, module, code, severity, names, message }
}

fn flow_diag(d: FlowDiag) -> CheckDiag {
    let severity = d.severity();
    diag(d.step, None, None, Module::Flow, d.code.as_str(), severity, d.names)
}

fn family_diag(e: &BindingError) -> CheckDiag {
    let names: Vec<String> = [e.variant(), e.param()].into_iter().flatten().map(echo).collect();
    diag(None, None, None, Module::Family, e.code(), Severity::Error, names)
}

fn oper_diag(step: Uuid7, e: &FieldError) -> CheckDiag {
    let names: Vec<String> = e.field.iter().map(|f| echo(f)).collect();
    diag(Some(step), None, None, Module::Oper, e.code(), Severity::Error, names)
}

/// Every step in document order (`main`, then subroutines; depth first).
fn each_step(program: &Program, f: &mut dyn FnMut(&Step)) {
    fn walk(steps: &[Step], f: &mut dyn FnMut(&Step)) {
        for s in steps {
            f(s);
            match &s.kind {
                StepKind::If { branches, else_body } => {
                    for b in branches {
                        walk(&b.body, f);
                    }
                    walk(else_body, f);
                }
                StepKind::Loop { body, .. } => walk(body, f),
                _ => {}
            }
        }
    }
    walk(&program.main, f);
    for sub in &program.subroutines {
        walk(&sub.body, f);
    }
}

// ------------------------------------------------------------------ invokes

/// The project keywords' outside access for a static check. Entity references are removed from
/// every schema before it is compiled ([`strip_refs`]), so `entity` is never asked.
struct StaticHooks<'a> {
    catalogue: &'a dyn QuantityCatalogue,
}

impl Hooks for StaticHooks<'_> {
    fn literal(&self, kind: &str, value: &CValue) -> Result<Normalised, BusError> {
        normalise_value(value, kind, self.catalogue)
    }

    fn entity(&self, _id: &str) -> RefLookup {
        RefLookup::BadId
    }
}

struct Checker<'a> {
    manifest: &'a Manifest,
    catalogue: &'a dyn QuantityCatalogue,
    types: &'a TypeReport,
    /// Compiled property schemas by (command, major, parameter); `None` when one does not compile.
    compiled: BTreeMap<(CommandRef, String), Option<Compiled>>,
}

fn state_of(def: &CommandDef) -> Option<State> {
    match def.lifecycle.get("state") {
        Some(CValue::Str(s)) => State::parse(s).ok(),
        _ => None,
    }
}

fn obj(v: &CValue) -> Option<&BTreeMap<String, CValue>> {
    match v {
        CValue::Obj(m) => Some(m),
        _ => None,
    }
}

impl Checker<'_> {
    /// Resolves one `invoke` and checks its arguments: its class and `human_only` when it
    /// resolves, and its check diagnostics.
    fn invoke(
        &mut self,
        step: Uuid7,
        command: &str,
        major: u32,
        args: &BTreeMap<String, Arg>,
    ) -> (Option<(SideEffect, bool)>, Vec<CheckDiag>) {
        let key = CommandRef { id: command.to_string(), major };
        let label = format!("{}@{major}", echo(command));
        let manifest = self.manifest;
        let def = manifest.commands.get(&key);
        let state = def.and_then(state_of);
        let def = match (def, state) {
            (Some(d), Some(State::Active | State::Deprecated)) => d,
            _ => {
                let d = diag(Some(step), None, None, Module::Check, UNKNOWN_COMMAND, Severity::Error, vec![label]);
                return (None, vec![d]);
            }
        };
        let mut out = Vec::new();
        if state == Some(State::Deprecated) {
            let mut names = vec![label];
            if let Some(CValue::Str(r)) = def.lifecycle.get("replaced_by") {
                names.push(echo(r));
            }
            out.push(diag(Some(step), None, None, Module::Check, DEPRECATED_COMMAND, Severity::Warning, names));
        }
        let problems = self.arguments(step, &key, def, args);
        if !problems.is_empty() {
            let names: Vec<String> = problems.keys().cloned().collect();
            let listed: Vec<String> = problems.iter().map(|(n, p)| format!("{n} ({})", p.as_str())).collect();
            let message = format!("{BAD_ARGUMENTS}: {}", listed.join(", "));
            out.push(CheckDiag {
                step_id: Some(step),
                slot: None,
                span: None,
                module: Module::Check,
                code: BAD_ARGUMENTS,
                severity: Severity::Error,
                names,
                message,
            });
        }
        (Some((def.side_effect, def.human_only)), out)
    }

    /// The offending parameters of one resolved `invoke`, by name (REQ-002).
    fn arguments(
        &mut self,
        step: Uuid7,
        key: &CommandRef,
        def: &CommandDef,
        args: &BTreeMap<String, Arg>,
    ) -> BTreeMap<String, ArgProblem> {
        let mut out = BTreeMap::new();
        let schema = &def.params_schema;
        let root = obj(schema);
        let empty = BTreeMap::new();
        let props = root.and_then(|m| m.get("properties")).and_then(obj).unwrap_or(&empty);
        if let Some(CValue::Arr(required)) = root.and_then(|m| m.get("required")) {
            for r in required {
                if let CValue::Str(name) = r {
                    if !args.contains_key(name) {
                        out.insert(echo(name), ArgProblem::Missing);
                    }
                }
            }
        }
        for (name, arg) in args {
            let Some(prop) = props.get(name) else {
                out.insert(echo(name), ArgProblem::Unknown);
                continue;
            };
            let problem = match arg {
                Arg::Literal(v) => self.literal(key, name, prop, schema, v),
                Arg::Expr(_) => self.expression(step, name).then_some(ArgProblem::WrongType),
            };
            if let Some(p) = problem {
                out.insert(echo(name), p);
            }
        }
        out
    }

    /// A literal against its own property schema, compiled once per (command, major, name).
    fn literal(
        &mut self,
        key: &CommandRef,
        name: &str,
        prop: &CValue,
        root: &CValue,
        value: &CValue,
    ) -> Option<ArgProblem> {
        let compiled = self
            .compiled
            .entry((key.clone(), name.to_string()))
            .or_insert_with(|| Compiled::compile(&property_schema(prop, root)).ok());
        let Some(c) = compiled else { return Some(ArgProblem::Unchecked) };
        let hooks = StaticHooks { catalogue: self.catalogue };
        c.evaluate(value, &hooks).is_err().then_some(ArgProblem::Invalid)
    }

    /// Whether the type checker reported a whole-expression `type_mismatch` at `args.<name>`.
    fn expression(&self, step: Uuid7, name: &str) -> bool {
        let slot = format!("args.{name}");
        let whole = self.types.types.get(&Site { step, slot: slot.clone() }).map(|t| t.span);
        self.types.diagnostics.iter().any(|d| {
            d.code == DiagCode::TypeMismatch
                && d.step == Some(step)
                && d.slot == slot
                && d.span.is_some()
                && d.span == whole
        })
    }
}

/// The schema a literal of one parameter is validated against: its property schema with entity
/// references removed and, when the root has `$defs` and the property schema has none, the
/// root's `$defs` beside it (so a local `$ref` still lands).
fn property_schema(prop: &CValue, root: &CValue) -> CValue {
    let mut s = strip_refs(prop);
    if let (CValue::Obj(m), Some(defs)) = (&mut s, obj(root).and_then(|r| r.get("$defs"))) {
        if !m.contains_key("$defs") {
            m.insert("$defs".into(), strip_refs(defs));
        }
    }
    s
}

/// Keywords whose value is one subschema.
const ONE_SCHEMA: [&str; 11] = [
    "additionalProperties",
    "contains",
    "else",
    "if",
    "items",
    "not",
    "propertyNames",
    "then",
    "unevaluatedItems",
    "unevaluatedProperties",
    "contentSchema",
];
/// Keywords whose value is an array of subschemas.
const SCHEMA_LIST: [&str; 4] = ["allOf", "anyOf", "oneOf", "prefixItems"];
/// Keywords whose value maps names to subschemas (`$defs` included when a `$defs` value is
/// passed whole).
const SCHEMA_MAP: [&str; 4] = ["$defs", "dependentSchemas", "patternProperties", "properties"];

/// A copy of `schema` without the `x-ref` and `x-selector` keywords, at every schema position.
/// Values of other keywords (`enum`, `const`, `default`, ...) are copied as they are.
fn strip_refs(schema: &CValue) -> CValue {
    let CValue::Obj(m) = schema else { return schema.clone() };
    let mut out = BTreeMap::new();
    for (k, v) in m {
        let k = k.as_str();
        let nv = match (k, v) {
            ("x-ref" | "x-selector", _) => continue,
            (_, CValue::Obj(_)) if ONE_SCHEMA.contains(&k) => strip_refs(v),
            (_, CValue::Arr(items)) if SCHEMA_LIST.contains(&k) => CValue::Arr(items.iter().map(strip_refs).collect()),
            (_, CValue::Obj(named)) if SCHEMA_MAP.contains(&k) => {
                CValue::Obj(named.iter().map(|(n, s)| (n.clone(), strip_refs(s))).collect())
            }
            _ => v.clone(),
        };
        out.insert(k.to_string(), nv);
    }
    CValue::Obj(out)
}

// ------------------------------------------------------------------ registry definition

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| s(x)).collect())
}

fn ob<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// The class names in [`CLASS_ORDER`], then `null`.
fn class_names() -> CValue {
    CValue::Arr(CLASS_ORDER.iter().map(|c| s(c.as_str())).chain(std::iter::once(CValue::Null)).collect())
}

fn params_schema() -> CValue {
    ob([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("oneOf", CValue::Arr(vec![ob([("required", strs(&["program"]))]), ob([("required", strs(&["program_id"]))])])),
        (
            "properties",
            ob([
                (
                    "program",
                    ob([
                        ("type", s("string")),
                        ("path_like", CValue::Bool(true)),
                        ("x-extensions", strs(&["json"])),
                        ("minLength", CValue::Num(1.0)),
                        ("maxLength", CValue::Num(MAX_HANDLE_BYTES as f64)),
                        ("description", s("The program file, as a read-root handle. Give this or program_id.")),
                    ]),
                ),
                (
                    "program_id",
                    ob([
                        ("type", s("string")),
                        ("pattern", s(crate::fmt::UUID7_PATTERN)),
                        ("description", s("A draft or registered program, by its program ID. Give this or program.")),
                    ]),
                ),
                (
                    "manifest",
                    ob([
                        ("type", s("string")),
                        ("pattern", s(Digest::PATTERN)),
                        (
                            "description",
                            s("The digest of the manifest to check against; default: the running manifest."),
                        ),
                    ]),
                ),
            ]),
        ),
    ])
}

fn result_schema() -> CValue {
    let digest = || ob([("type", s("string")), ("pattern", s(Digest::PATTERN))]);
    let nullable = |t: &str| strs(&[t, "null"]);
    let span = ob([
        ("type", strs(&["object", "null"])),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["end", "start"])),
        (
            "properties",
            ob([
                ("end", ob([("type", s("integer")), ("minimum", CValue::Num(0.0))])),
                ("start", ob([("type", s("integer")), ("minimum", CValue::Num(0.0))])),
            ]),
        ),
    ]);
    let diag = ob([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["code", "message", "module", "names", "severity", "slot", "span", "step_id"])),
        (
            "properties",
            ob([
                ("code", ob([("type", s("string"))])),
                ("message", ob([("type", s("string")), ("x-untrusted", CValue::Bool(true))])),
                ("module", ob([("enum", strs(&["fmt", "expr", "flow", "oper", "family", "check"]))])),
                (
                    "names",
                    ob([
                        ("type", s("array")),
                        ("items", ob([("type", s("string")), ("x-untrusted", CValue::Bool(true))])),
                    ]),
                ),
                ("severity", ob([("enum", strs(&["error", "warning", "info"]))])),
                ("slot", ob([("type", nullable("string"))])),
                ("span", span),
                ("step_id", ob([("type", nullable("string"))])),
            ]),
        ),
    ]);
    let step = ob([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["class", "command", "human_only", "reachable", "step_id"])),
        (
            "properties",
            ob([
                ("class", ob([("enum", class_names())])),
                ("command", ob([("type", s("string")), ("x-untrusted", CValue::Bool(true))])),
                ("human_only", ob([("type", nullable("boolean"))])),
                ("reachable", ob([("type", s("boolean"))])),
                ("step_id", ob([("type", s("string"))])),
            ]),
        ),
    ]);
    ob([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["diagnostics", "manifest_digest", "passed", "program_class", "program_digest", "steps"])),
        (
            "properties",
            ob([
                ("diagnostics", ob([("type", s("array")), ("items", diag)])),
                ("manifest_digest", digest()),
                ("passed", ob([("type", s("boolean"))])),
                (
                    "program_class",
                    ob([
                        ("enum", class_names()),
                        ("description", s("The highest class over the reachable invokes; null when unknown.")),
                    ]),
                ),
                ("program_digest", digest()),
                ("steps", ob([("type", s("array")), ("items", step)])),
            ]),
        ),
    ])
}

/// The registry definition of `program.check@1` (R1, MCP exposed, D0). Its handler is not
/// built yet.
pub fn command_def() -> Result<CommandDef, DefError> {
    CommandDef::from_value(ob([
        ("id", s("program.check")),
        ("major", CValue::Num(1.0)),
        ("title", s("Check program")),
        (
            "description",
            s("Runs every static rule over a Command Program against a manifest: command resolution, argument \
               schemas, expression types, control flow, operator descriptors and part-family declarations. \
               Returns the ordered diagnostics and the side-effect class of each step and of the whole program. \
               Changes nothing."),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
        ("side_effect", s("R1")),
        ("mcp_projection", s("exposed")),
        ("determinism", s("D0")),
        ("idempotency", s("natural")),
        ("fs_effect", s("read_root")),
        ("undo", s("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", ob([("state", s("active")), ("since", s(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", s("none")),
        ("web", s("partial")),
        ("web_note", s("Programs by ID only; the web build has no file-system roots for handles.")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// Every command this crate declares (for `fid-registry-build`): `program.check@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![command_def()?])
}
