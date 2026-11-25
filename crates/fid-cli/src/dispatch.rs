//! Registry command dispatch onto the bus. Spec node `AUTO.CLI.DISPATCH`.
//!
//! The step between "the tree resolved `fid <domain> <verb-object>` to a command major" and
//! "the envelope is printed":
//!
//! 1. **Visibility** ([`visible`], [`visible_view`], [`resolve_visible`]): lifecycle `hidden`
//!    and `removed` majors, and root-configuration commands for every principal but `human`,
//!    are absent. The argv is resolved again by the tree's own [`resolve_args`] over the
//!    filtered view, so a path without `--major` takes its newest visible major and a miss is
//!    built by the same function as a real miss (D-035, extended to every CLI caller).
//! 2. The [`Gate`] class and dry-run checks (`AUTO.CLI.GATE`).
//! 3. The [`FlagTable`] of the major: one flag per top-level params-schema property, values
//!    parsed by the [`ArgParser`] (`AUTO.CLI.ARGS`), object properties read as one bounded
//!    JSON value, array properties appended per occurrence, `--params` merged under the flags.
//! 4. The [`Gate`] raw-path check, then the required check.
//! 5. One invocation through the `AUTO.SURF.JOBS` [`Tracker`] over the [`TargetTransport`];
//!    the final envelope goes unchanged to `AUTO.CLI.OUT` ([`Output::finish`]), except that a
//!    failure the bus raised as `hidden_command`, or `not_found` with reason `unknown_command`,
//!    `unknown_major` or `removed_major`, becomes the step-1 refusal.
//!
//! **Seams.** `AUTO.CLI.ARGS`, `AUTO.CLI.GATE`, `AUTO.CLI.JOB` and `AUTO.CLI.TARGET` are not
//! built yet, and no node specifies the bus API crate or the composition root that loads every
//! domain's handlers. Each missing piece is a trait here ([`ArgParser`], [`Gate`],
//! [`ProgressSubscriber`], [`TargetTransport`]); the leaf's Open questions list what remains.
//!
//! **Identity.** The principal kind is the one the session holds, as reported by the
//! [`TargetTransport`] (the bus derives it from the channel, CORE.BUS.PRIN). Nothing in the
//! argv or the parameter document can set it, and DISPATCH puts no principal, actor,
//! on-behalf-of or session field anywhere (REQ-009). The visibility filter is a convenience,
//! not a security control: the bus enforces, and DISPATCH never relaxes a bus refusal.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::time::Duration;

use fid_canon::{parse_with_limits, CValue};
use fid_surface::jobs::{
    BusEvent, BusRequest, BusTransport, KeySource, MonotonicClock, ProgressSubscriber, SendError, State, Tracker,
};
use fid_surface::roots::PrincipalKind;
use fid_surface::{Envelope, ErrorCode};
use fid_types::CommandId;

use crate::exit::ExitCode;
use crate::output::Output;
use crate::tree::{nearest_match, resolve_args, CliCommand, CliMajor, CliParam, CliTree, ResolveOutcome};

/// Flags every major shares (REQ-003). A property whose flag equals one is `flag_collision`.
pub const RESERVED_FLAGS: [&str; 9] =
    ["--json", "--dry-run", "--major", "--help", "--params", "--project", "--app", "--confirm-human", "--version"];

/// Reserved flags that take a value.
const VALUE_FLAGS: [&str; 2] = ["--params", "--project"];

/// Size limit of one object-valued flag (REQ-007, the limit of REQ-AUTO.CLI.ARGS-004).
pub const MAX_OBJECT_BYTES: usize = 1024 * 1024;
/// Depth limit of one object-valued flag.
pub const MAX_OBJECT_DEPTH: usize = 32;

/// Field names that carry identity. A `--params` document may not set one that is not a
/// property of the major's schema (REQ-009: the principal comes only from the session).
pub const IDENTITY_FIELDS: [&str; 4] = ["actor", "on_behalf_of", "principal", "session"];

/// Lifecycle states whose majors are absent (REQ-010).
const ABSENT_STATES: [&str; 2] = ["hidden", "removed"];

/// Bus failures that DISPATCH replaces by the absent-major refusal (REQ-012), as
/// (`bus_code`, `bus_reason`) in the envelope's error details; `None` matches any reason.
const ABSENT_BUS_ERRORS: [(&str, Option<&str>); 4] = [
    ("hidden_command", None),
    ("not_found", Some("unknown_command")),
    ("not_found", Some("unknown_major")),
    ("not_found", Some("removed_major")),
];

// ------------------------------------------------------------------ visibility

/// REQ-010: whether `major` is visible to a session whose principal kind is `kind`.
pub fn visible(major: &CliMajor, kind: PrincipalKind) -> bool {
    let live = !major.lifecycle.as_deref().is_some_and(|s| ABSENT_STATES.contains(&s));
    live && (!major.root_config || kind == PrincipalKind::Human)
}

/// The tree without the majors absent for `kind`; commands and domains left with nothing are
/// dropped, so the view is exactly the tree a manifest without those majors would give.
pub fn visible_view(tree: &CliTree, kind: PrincipalKind) -> CliTree {
    filter_tree(tree, |_, m| visible(m, kind))
}

fn filter_tree(tree: &CliTree, keep: impl Fn(&CliCommand, &CliMajor) -> bool) -> CliTree {
    let mut domains = BTreeMap::new();
    for (domain, cmds) in &tree.domains {
        let mut kept = Vec::new();
        for cmd in cmds {
            let majors: Vec<CliMajor> = cmd.majors.iter().filter(|m| keep(cmd, m)).cloned().collect();
            if !majors.is_empty() {
                kept.push(CliCommand { path: cmd.path.clone(), command_id: cmd.command_id.clone(), majors });
            }
        }
        if !kept.is_empty() {
            domains.insert(domain.clone(), kept);
        }
    }
    CliTree { schema: tree.schema.clone(), manifest_digest: tree.manifest_digest, domains }
}

/// REQ-011: resolves `args` as against a tree without the majors absent for `kind`.
///
/// The root listing, domain listings, `--version` and host subcommands are resolved over the
/// full tree (their filtering is `AUTO.CLI.TREE`'s, see the leaf's Open questions); every
/// command path, `--major` and command help is resolved over [`visible_view`]. Returns the
/// outcome and the tree to render it with.
pub fn resolve_visible(tree: &CliTree, kind: PrincipalKind, args: &[String]) -> (ResolveOutcome, CliTree) {
    let full = resolve_args(tree, args);
    match full {
        ResolveOutcome::Version
        | ResolveOutcome::RootHelp { .. }
        | ResolveOutcome::DomainHelp { .. }
        | ResolveOutcome::HostSubcommand { .. } => (full, tree.clone()),
        ResolveOutcome::CommandHelp { .. } | ResolveOutcome::Invoke { .. } | ResolveOutcome::UsageError { .. } => {
            let view = visible_view(tree, kind);
            (resolve_args(&view, args), view)
        }
    }
}

/// The refusal REQ-012 puts in place of a bus answer that `id` is hidden or unknown: `args`
/// resolved over `view` without `id`. If that still resolves to a command (another visible
/// major of the same path), the whole command is left out, so the answer is a refusal and
/// nothing is sent a second time.
fn absent_refusal(view: &CliTree, id: &CommandId, args: &[String]) -> (ResolveOutcome, CliTree) {
    let without = filter_tree(view, |_, m| m.command_id != *id);
    let outcome = resolve_args(&without, args);
    if matches!(outcome, ResolveOutcome::UsageError { .. }) {
        return (outcome, without);
    }
    let name = id.name();
    let without = filter_tree(view, |c, _| c.command_id != name);
    (resolve_args(&without, args), without)
}

// ------------------------------------------------------------------ flag table

/// One flag of a major (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagSpec {
    /// The params-schema property name.
    pub property: String,
    /// The property's schema type as the tree records it (`string`, `null|object`, ...).
    pub schema_type: String,
    /// Listed in the schema's `required`.
    pub required: bool,
    /// Declares `x-quantity`.
    pub quantity: bool,
}

impl FlagSpec {
    fn types(&self) -> impl Iterator<Item = &str> {
        self.schema_type.split('|').filter(|t| *t != "null")
    }

    /// Repeats append one element each (REQ-004).
    pub fn is_array(&self) -> bool {
        self.types().any(|t| t == "array")
    }

    /// The value is one bounded JSON object (REQ-007).
    pub fn is_object(&self) -> bool {
        !self.quantity && self.types().eq(["object"])
    }
}

/// The flags of one major, keyed by flag (`--` plus the property, `_` replaced by `-`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagTable {
    pub major: CommandId,
    pub flags: BTreeMap<String, FlagSpec>,
}

/// REQ-003: a derived flag equals a reserved flag or the flag of another property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagCollision {
    pub major: CommandId,
    pub property: String,
    pub flag: String,
    /// The reserved flag, or the other property, it collides with.
    pub with: String,
}

impl FlagCollision {
    pub fn code(&self) -> &'static str {
        "flag_collision"
    }
}

impl fmt::Display for FlagCollision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "flag_collision: {} property {:?} derives flag `{}`, which collides with {}",
            self.major.as_str(),
            self.property,
            self.flag,
            self.with
        )
    }
}

impl std::error::Error for FlagCollision {}

/// The flag of a property (REQ-002).
pub fn flag_of(property: &str) -> String {
    format!("--{}", property.replace('_', "-"))
}

impl FlagTable {
    /// Builds the table of `major`, failing on the first collision in property order.
    pub fn build(major: &CliMajor) -> Result<FlagTable, FlagCollision> {
        let mut flags: BTreeMap<String, FlagSpec> = BTreeMap::new();
        for p in &major.params {
            let flag = flag_of(&p.name);
            let clash = |with: String| FlagCollision {
                major: major.command_id.clone(),
                property: p.name.clone(),
                flag: flag.clone(),
                with,
            };
            if RESERVED_FLAGS.contains(&flag.as_str()) {
                return Err(clash(format!("reserved flag `{flag}`")));
            }
            if let Some(other) = flags.get(&flag) {
                return Err(clash(format!("property {:?}", other.property)));
            }
            let spec = FlagSpec {
                property: p.name.clone(),
                schema_type: p.ty.clone(),
                required: p.required,
                quantity: p.quantity_kind.is_some(),
            };
            flags.insert(flag, spec);
        }
        Ok(FlagTable { major: major.command_id.clone(), flags })
    }
}

/// AC-6: builds the flag table of every major in `tree`; returns every collision.
pub fn check_tree_flags(tree: &CliTree) -> Result<(), Vec<FlagCollision>> {
    let errors: Vec<FlagCollision> = tree
        .domains
        .values()
        .flatten()
        .flat_map(|c| c.majors.iter())
        .filter_map(|m| FlagTable::build(m).err())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ------------------------------------------------------------------ seams

/// `AUTO.CLI.ARGS`: parsing one value token and reading the `--params` document. A refusal
/// is a `failed` envelope (for example `usage_error` naming the parameter).
pub trait ArgParser {
    /// Parses one value token of `param` into its typed JSON value. For an array property it
    /// is called once per occurrence and returns one element.
    fn parse_value(&self, param: &CliParam, raw: &str) -> Result<CValue, Box<Envelope>>;
    /// Reads the `--params` document named by `source` (`-` is stdin) as an object keyed by
    /// property name, with the bounded reader of REQ-AUTO.CLI.ARGS-004.
    fn read_params(&mut self, source: &str) -> Result<BTreeMap<String, CValue>, Box<Envelope>>;
}

/// `AUTO.CLI.GATE`: the class, dry-run and raw-path checks. A refusal is a `failed` envelope.
pub trait Gate {
    /// Class (M1, S1, A0) and dry-run checks, before any flag is mapped.
    fn class_and_dry_run(&self, major: &CliMajor, dry_run: bool) -> Result<(), Box<Envelope>>;
    /// The raw-path check (`AUTO.SURF.ROOTS`) on the assembled parameter document.
    fn raw_paths(&self, kind: PrincipalKind, major: &CliMajor, params: &CValue) -> Result<(), Box<Envelope>>;
}

/// Which target the invocation goes to (`AUTO.CLI.TARGET.SELECT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// In-process session over a project (`AUTO.CLI.TARGET.HEADLESS`).
    Headless,
    /// The running app, over the control channel (`AUTO.CLI.TARGET.APP`).
    App,
    /// A project-free command with no target.
    None,
}

/// The kind of channel a transport sends on (CORE.BUS.PRIN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Cli,
    Control,
}

/// The channel a transport sends on: what the bus authenticates and derives the principal
/// from. DISPATCH reads it; it never writes any of it into a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub kind: ChannelKind,
    /// The authenticated identity subject (the operating-system user on a `cli` channel).
    pub identity: String,
    /// The session key, alive for the process.
    pub session: String,
}

/// A transport chosen by `AUTO.CLI.TARGET.SELECT`, as DISPATCH needs it.
pub trait TargetTransport: BusTransport {
    fn kind(&self) -> TargetKind;
    /// The channel the transport sends on.
    fn channel(&self) -> Channel;
    /// The principal kind the session holds, as the bus derived it from the channel (`script`
    /// unless a `--confirm-human` upgrade succeeded). Never taken from argv or parameters.
    fn principal_kind(&self) -> PrincipalKind;
    /// Called while the tracker waits and no event is pending.
    fn idle(&mut self) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Lends a `&mut dyn TargetTransport` to the tracker, which owns its transport.
struct Lend<'t>(&'t mut dyn TargetTransport);

impl BusTransport for Lend<'_> {
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError> {
        self.0.send(request)
    }

    fn recv(&mut self) -> Option<BusEvent> {
        self.0.recv()
    }
}

// ------------------------------------------------------------------ pre-send

/// The reserved flags found after the command path. `--major` and `--help` are consumed by
/// the tree; `--project`, `--app` and `--confirm-human` are for `AUTO.CLI.TARGET.SELECT`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReservedFlags {
    pub json: bool,
    pub dry_run: bool,
    pub params: Option<String>,
    pub project: Option<String>,
    pub app: bool,
    pub confirm_human: bool,
    pub version: bool,
}

/// What the pre-send steps hand to the tracker (Design: `Invocation`).
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub command_id: CommandId,
    /// One JSON object keyed by property name: only the values given, no defaults (REQ-006).
    pub params: CValue,
    pub dry_run: bool,
}

/// The result of the pre-send steps.
#[derive(Debug)]
pub enum Prepared {
    /// Every step passed.
    Send { invocation: Invocation, reserved: ReservedFlags, json: bool, view: CliTree },
    /// Not a command invocation (help, version, ...) or the absent-major refusal: render it.
    Render { outcome: ResolveOutcome, tree: CliTree },
    /// A pre-send step refused; nothing may be sent.
    Refused { envelope: Envelope, json: bool },
}

fn usage(id: &CommandId, msg: String) -> Envelope {
    Envelope::failed(id.as_str(), ErrorCode::UsageError, &msg, None)
}

fn usage_box(id: &CommandId, msg: String) -> Box<Envelope> {
    Box::new(usage(id, msg))
}

fn usage_detail(id: &CommandId, cli_error: &str, msg: String) -> Box<Envelope> {
    let details = CValue::Obj(BTreeMap::from([("cli_error".to_string(), CValue::Str(cli_error.to_string()))]));
    Box::new(Envelope::failed(id.as_str(), ErrorCode::UsageError, &format!("{cli_error}: {msg}"), Some(details)))
}

/// REQ-007: one object value with the bounded reader.
fn read_object(id: &CommandId, flag: &str, raw: &str) -> Result<CValue, Box<Envelope>> {
    if raw.len() > MAX_OBJECT_BYTES {
        return Err(usage_detail(
            id,
            "params_too_large",
            format!("the value of `{flag}` is larger than {MAX_OBJECT_BYTES} bytes"),
        ));
    }
    match parse_with_limits(raw.as_bytes(), MAX_OBJECT_BYTES, MAX_OBJECT_DEPTH) {
        Ok(v @ CValue::Obj(_)) => Ok(v),
        Ok(_) => Err(usage_detail(id, "params_parse", format!("the value of `{flag}` must be a JSON object"))),
        Err(e) => Err(usage_detail(id, "params_parse", format!("the value of `{flag}` is not valid JSON ({e})"))),
    }
}

/// Splits `rest` (the argv after the command path, without `--major` and `--help`) into
/// reserved flags and parameter values (REQ-002, -004, -007).
fn map_flags(
    major: &CliMajor,
    table: &FlagTable,
    rest: &[String],
    args: &dyn ArgParser,
    command_path: &str,
) -> Result<(ReservedFlags, BTreeMap<String, CValue>), Box<Envelope>> {
    let id = &major.command_id;
    let mut reserved = ReservedFlags::default();
    let mut values: BTreeMap<String, CValue> = BTreeMap::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = &rest[i];
        i += 1;
        if !arg.starts_with('-') || arg == "-" {
            return Err(usage_box(id, format!("unexpected argument `{arg}` for `{command_path}`")));
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (arg.as_str(), None),
        };
        let take_value = |i: &mut usize| -> Result<String, Box<Envelope>> {
            if let Some(v) = inline {
                return Ok(v.to_string());
            }
            match rest.get(*i) {
                Some(v) => {
                    *i += 1;
                    Ok(v.clone())
                }
                None => Err(usage_box(id, format!("flag `{flag}` requires a value"))),
            }
        };
        if RESERVED_FLAGS.contains(&flag) {
            if VALUE_FLAGS.contains(&flag) {
                let v = take_value(&mut i)?;
                let slot = if flag == "--params" { &mut reserved.params } else { &mut reserved.project };
                if slot.is_some() {
                    return Err(usage_box(id, format!("flag `{flag}` given more than once")));
                }
                *slot = Some(v);
                continue;
            }
            if inline.is_some() {
                return Err(usage_box(id, format!("flag `{flag}` takes no value")));
            }
            match flag {
                "--json" => reserved.json = true,
                "--dry-run" => reserved.dry_run = true,
                "--app" => reserved.app = true,
                "--confirm-human" => reserved.confirm_human = true,
                "--version" => reserved.version = true,
                // `--major` and `--help` were consumed by the tree's resolution.
                _ => {}
            }
            continue;
        }
        let Some(spec) = table.flags.get(flag) else {
            let candidates: Vec<&str> = table.flags.keys().map(String::as_str).collect();
            let msg = match nearest_match(flag, &candidates) {
                Some(s) => format!("unknown flag `{flag}` for `{command_path}`; did you mean `{s}`?"),
                None => format!("unknown flag `{flag}` for `{command_path}`"),
            };
            return Err(usage_box(id, msg));
        };
        let array = spec.is_array();
        if !array && values.contains_key(&spec.property) {
            return Err(usage_box(id, format!("flag `{flag}` given more than once")));
        }
        let raw = take_value(&mut i)?;
        let value = if spec.is_object() {
            read_object(id, flag, &raw)?
        } else {
            let Some(param) = major.params.iter().find(|p| p.name == spec.property) else {
                return Err(Box::new(Envelope::failed(
                    id.as_str(),
                    ErrorCode::InternalError,
                    "flag table and parameters disagree",
                    None,
                )));
            };
            args.parse_value(param, &raw)?
        };
        if array {
            let slot = values.entry(spec.property.clone()).or_insert_with(|| CValue::Arr(Vec::new()));
            if let CValue::Arr(items) = slot {
                items.push(value);
            }
        } else {
            values.insert(spec.property.clone(), value);
        }
    }
    Ok((reserved, values))
}

/// The pre-send steps in the order of REQ-013, for a session whose principal kind is `kind`.
/// Pure: reads nothing but the tree, the argv, the principal kind and the seams.
pub fn prepare(
    tree: &CliTree,
    kind: PrincipalKind,
    args: &[String],
    parser: &mut dyn ArgParser,
    gate: &dyn Gate,
) -> Prepared {
    // 1. Visibility, and the tree's own resolution over the filtered view.
    let (outcome, view) = resolve_visible(tree, kind, args);
    let ResolveOutcome::Invoke { command_id, json, rest, .. } = outcome else {
        return Prepared::Render { outcome, tree: view };
    };
    let found =
        view.domains.values().flatten().find_map(|c| {
            c.majors.iter().find(|m| m.command_id == command_id).map(|m| (c.path[1..].join(" "), m.clone()))
        });
    let Some((path, major)) = found else {
        let msg = "resolved command major is not in the tree";
        return Prepared::Refused {
            envelope: Envelope::failed(command_id.as_str(), ErrorCode::InternalError, msg, None),
            json,
        };
    };
    let refused = |envelope: Envelope| Prepared::Refused { envelope, json };
    // 2. Class and dry-run checks; `--dry-run` is the exact token, like `--json`.
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    if let Err(e) = gate.class_and_dry_run(&major, dry_run) {
        return refused(*e);
    }
    // 3. Flag mapping with ARGS parsing, then the `--params` document under the flags.
    let table = match FlagTable::build(&major) {
        Ok(t) => t,
        Err(c) => {
            return refused(Envelope::failed(command_id.as_str(), ErrorCode::InternalError, &c.to_string(), None))
        }
    };
    let (reserved, mut values) = match map_flags(&major, &table, &rest, parser, &path) {
        Ok(x) => x,
        Err(e) => return refused(*e),
    };
    if let Some(source) = &reserved.params {
        let doc = match parser.read_params(source) {
            Ok(d) => d,
            Err(e) => return refused(*e),
        };
        for (k, v) in doc {
            if IDENTITY_FIELDS.contains(&k.as_str()) && !major.params.iter().any(|p| p.name == k) {
                let msg = format!("`--params` may not set `{k}`: the principal comes only from the session");
                return refused(usage(&command_id, msg));
            }
            values.entry(k).or_insert(v);
        }
    }
    let params = CValue::Obj(values);
    // 4. Raw-path check, then the required check.
    if let Err(e) = gate.raw_paths(kind, &major, &params) {
        return refused(*e);
    }
    if let CValue::Obj(given) = &params {
        if let Some((flag, _)) = table.flags.iter().find(|(_, s)| s.required && !given.contains_key(&s.property)) {
            return refused(usage(&command_id, format!("missing required flag `{flag}` for `{path}`")));
        }
    }
    Prepared::Send { invocation: Invocation { command_id, params, dry_run }, reserved, json, view }
}

// ------------------------------------------------------------------ send and finish

/// Whether `env` is a failure the bus raised as a hidden or unknown command (REQ-012). Keys
/// on `bus_code` and `bus_reason` in the error details, as the transport carries them.
pub fn is_absent_answer(env: &Envelope) -> bool {
    let Some(CValue::Obj(d)) = env.error.as_ref().and_then(|e| e.details.as_ref()) else {
        return false;
    };
    let text = |k: &str| match d.get(k) {
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    let (Some(code), reason) = (text("bus_code"), text("bus_reason")) else {
        return false;
    };
    ABSENT_BUS_ERRORS.iter().any(|(c, r)| {
        *c == code
            && match r {
                None => true,
                Some(r) => reason == Some(*r),
            }
    })
}

/// Everything DISPATCH needs besides the argv and the target.
pub struct Dispatch<'a> {
    pub tree: &'a CliTree,
    pub version: &'a str,
    pub args: &'a mut dyn ArgParser,
    pub gate: &'a dyn Gate,
    /// The tracker deadline of the invocation.
    pub timeout: Duration,
}

/// Runs one `fid` argv: the pre-send steps, one invocation through the tracker over `target`,
/// and the final envelope through `AUTO.CLI.OUT`. Returns the exit code (REQ-014).
#[allow(clippy::too_many_arguments)]
pub fn run<C: MonotonicClock, K: KeySource>(
    d: Dispatch<'_>,
    argv: &[String],
    target: &mut dyn TargetTransport,
    clock: C,
    keys: K,
    progress: &mut dyn ProgressSubscriber,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let kind = target.principal_kind();
    let (invocation, json, view) = match prepare(d.tree, kind, argv, d.args, d.gate) {
        Prepared::Render { outcome, tree } => return crate::render_outcome(&tree, d.version, outcome, stdout, stderr),
        Prepared::Refused { envelope, json } => return Output::new(json, stdout, stderr).finish(&envelope),
        Prepared::Send { invocation, json, view, .. } => (invocation, json, view),
    };
    let id = invocation.command_id.clone();
    // REQ-008: headless and target-free invocations go on the process's `cli` channel.
    if matches!(target.kind(), TargetKind::Headless | TargetKind::None) {
        let ch = target.channel();
        if ch.kind != ChannelKind::Cli || ch.identity.is_empty() || ch.session.is_empty() {
            let msg = "the target's channel is not an authenticated `cli` channel; nothing was sent";
            return Output::new(json, stdout, stderr).finish(&Envelope::failed(
                id.as_str(),
                ErrorCode::InternalError,
                msg,
                None,
            ));
        }
    }
    let envelope = send(target, clock, keys, progress, invocation, d.timeout);
    if is_absent_answer(&envelope) {
        let (outcome, tree) = absent_refusal(&view, &id, argv);
        return crate::render_outcome(&tree, d.version, outcome, stdout, stderr);
    }
    Output::new(json, stdout, stderr).finish(&envelope)
}

/// REQ-001: exactly one invocation through the tracker; waits for its final envelope.
fn send<C: MonotonicClock, K: KeySource>(
    target: &mut dyn TargetTransport,
    clock: C,
    keys: K,
    progress: &mut dyn ProgressSubscriber,
    inv: Invocation,
    timeout: Duration,
) -> Envelope {
    let id = inv.command_id.clone();
    let internal = |msg: &str| Envelope::failed(id.as_str(), ErrorCode::InternalError, msg, None);
    let mut tracker = Tracker::new(Lend(target), clock, keys);
    let handle = match tracker.submit(inv.command_id, inv.params, timeout) {
        Ok(h) => h,
        Err(e) => return internal(&format!("the invocation could not be tracked: {e}")),
    };
    loop {
        if matches!(tracker.state(handle), Some(State::Finished(_)) | None) {
            break;
        }
        if tracker.pump(progress) == 0 && !matches!(tracker.state(handle), Some(State::Finished(_))) {
            tracker.transport_mut().0.idle();
        }
    }
    match tracker.take(handle) {
        Some(report) => report.envelope,
        None => internal("the tracker lost the invocation"),
    }
}
