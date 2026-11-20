//! `program.adopt_steps@1` and `script.adopt_version@1` (W2, `human_only: true`, D-031),
//! declared with the registry macros, with their CLI and MCP entry points.
//!
//! CLI: `fid program adopt-steps --program <P> (--steps <S1>[,<S2>..] | --all-unadopted)
//! --reviewed-digest <sha256:..> [--json]` and `fid script adopt-version --script <X>
//! --version-digest <sha256:..> [--json]`.
//!
//! MCP tools `program_adopt_steps` and `script_adopt_version` are **request-only**: an agent may
//! ask a person to adopt, and the tool never adopts. [`invoke_mcp`] reads the parameters
//! strictly (`bad_params` on any unknown key, such as a `kind` or `principal` field), then
//! always returns `policy_refused`: `human_only` for a non-human principal, `request_only` for
//! a person, who adopts through the CLI or the GUI.
//!
//! Every handler checks the caller before anything else: the bus class check
//! ([`fid_bus::policy::class::authorise_class`]) against the command's own declared
//! definition, and then, independently, that the bus-derived principal kind is `human`. Any
//! other kind (agent, script, service, device, system) is `policy_refused`. The kind is read
//! from the [`Principal`] in the context; no parameter can name or change it.
//!
//! A refused or failing command changes nothing: all checks run on a read-only view of the
//! store, then the event is committed to the journal, and only after a successful commit are
//! the adoptions appended to the store, with the committed entry's sequence number.

use super::event::AdoptEvent;
use super::{
    is_person, step_contents, AdoptError, ProvenanceStore, ScriptId, Target, ADOPT_STEPS_COMMAND,
    ADOPT_VERSION_COMMAND, UUID7_PATTERN,
};
use fid_bus::principal::Principal;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_program::{Program, Uuid7, MAX_STEPS};
use fid_registry::{fid_command, fid_commands, CommandDef, FidSchema};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// CLI group and subcommand of the step adoption.
pub const CLI_ADOPT_STEPS: (&str, &str) = ("program", "adopt-steps");
/// CLI group and subcommand of the script version adoption.
pub const CLI_ADOPT_VERSION: (&str, &str) = ("script", "adopt-version");
/// MCP tool of the step adoption (request-only).
pub const MCP_ADOPT_STEPS: &str = "program_adopt_steps";
/// MCP tool of the script version adoption (request-only).
pub const MCP_ADOPT_VERSION: &str = "script_adopt_version";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 1024 + 40 * MAX_STEPS;
const MAX_PARAMS_DEPTH: usize = 2;
/// Most CLI words.
const MAX_CLI_ARGS: usize = 16;

// ------------------------------------------------------------------ context

/// The programs the adoption command can see: the current version of each program.
pub trait Programs {
    /// The current program `id`, if it exists.
    fn program(&self, id: &Uuid7) -> Option<&Program>;
}

/// The journal seam the host wires (the Record Journal behind the bus). `Ok` holds the
/// sequence number of the committed, durable entry; `Err` means nothing was committed
/// (display-safe reason).
pub trait AdoptJournal {
    /// Commits one adoption event on the working branch.
    fn commit(&mut self, event: &AdoptEvent) -> Result<u64, String>;
}

/// The context both handlers get.
pub struct AdoptCtx<'a> {
    /// The bus-derived principal of the call.
    pub principal: &'a Principal,
    /// The provenance store.
    pub store: &'a mut ProvenanceStore,
    /// The current programs.
    pub programs: &'a dyn Programs,
    /// The journal seam.
    pub journal: &'a mut dyn AdoptJournal,
}

// ------------------------------------------------------------------ params and results

/// Parameters of `program.adopt_steps@1`. Exactly one of `steps` and `all_unadopted: true`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct AdoptStepsParams {
    #[fid(description = "The program whose steps are adopted.", pattern = UUID7_PATTERN)]
    pub program: String,
    #[fid(
        description = "The steps to adopt; each must be unadopted agent work in the program.",
        pattern = UUID7_PATTERN,
        min_items = 1,
        max_items = MAX_STEPS
    )]
    pub steps: Option<Vec<String>>,
    #[fid(description = "Adopt every unadopted step of the program instead of a list.")]
    pub all_unadopted: Option<bool>,
    #[fid(description = "Digest of the program version the person reviewed.", pattern = Digest::PATTERN)]
    pub reviewed_digest: String,
}

/// Result of `program.adopt_steps@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct AdoptStepsResult {
    #[fid(description = "The adopted steps, in document order.", pattern = UUID7_PATTERN, min_items = 1)]
    pub adopted: Vec<String>,
}

/// Parameters of `script.adopt_version@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct AdoptVersionParams {
    #[fid(description = "The script.", pattern = super::SCRIPT_ID_PATTERN)]
    pub script: String,
    #[fid(description = "Digest of the script version adopted as a whole.", pattern = Digest::PATTERN)]
    pub version_digest: String,
}

/// Result of `script.adopt_version@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct AdoptVersionResult {
    #[fid(description = "Always true on success.")]
    pub adopted: bool,
}

/// One parsed call of either command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptCall {
    /// `program.adopt_steps@1`.
    Steps(AdoptStepsParams),
    /// `script.adopt_version@1`.
    Version(AdoptVersionParams),
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

impl AdoptStepsParams {
    /// The JSON value (the parameter object the schema describes).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("program".to_owned(), text(&self.program));
        if let Some(steps) = &self.steps {
            o.insert("steps".to_owned(), CValue::Arr(steps.iter().map(|s| text(s)).collect()));
        }
        if let Some(all) = self.all_unadopted {
            o.insert("all_unadopted".to_owned(), CValue::Bool(all));
        }
        o.insert("reviewed_digest".to_owned(), text(&self.reviewed_digest));
        CValue::Obj(o)
    }
}

impl AdoptVersionParams {
    /// The JSON value (the parameter object the schema describes).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("script".to_owned(), text(&self.script));
        o.insert("version_digest".to_owned(), text(&self.version_digest));
        CValue::Obj(o)
    }
}

impl AdoptCall {
    /// The command ID.
    pub fn command(&self) -> &'static str {
        match self {
            AdoptCall::Steps(_) => ADOPT_STEPS_COMMAND,
            AdoptCall::Version(_) => ADOPT_VERSION_COMMAND,
        }
    }

    /// The parameter object.
    pub fn to_value(&self) -> CValue {
        match self {
            AdoptCall::Steps(p) => p.to_value(),
            AdoptCall::Version(p) => p.to_value(),
        }
    }
}

impl AdoptStepsResult {
    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("adopted".to_owned(), CValue::Arr(self.adopted.iter().map(|s| text(s)).collect()));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }
}

impl AdoptVersionResult {
    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("adopted".to_owned(), CValue::Bool(self.adopted));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ checks shared with replay

/// Which steps a step adoption names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection<'a> {
    Ids(&'a [String]),
    AllUnadopted,
}

/// A checked adoption: the event to journal and the content each target is adopted with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub(crate) event: AdoptEvent,
    contents: Vec<(Target, Digest)>,
    digest: Digest,
}

/// Runs every check of a step adoption against a read-only store and returns what to commit.
/// Order: parameters, `program_not_found`, `stale_review`, `unknown_step`, `nothing_to_adopt`.
pub(crate) fn prepare_steps(
    store: &ProvenanceStore,
    programs: &dyn Programs,
    human: &str,
    program: &str,
    selection: Selection<'_>,
    reviewed_digest: &str,
) -> Result<Prepared, AdoptError> {
    let id = Uuid7::parse(program).ok_or(AdoptError::bad("program is not a step-format UUIDv7"))?;
    let reviewed =
        Digest::parse(reviewed_digest).map_err(|_| AdoptError::bad("reviewed_digest is not sha256:<hex>"))?;
    let doc = programs.program(&id).ok_or_else(|| AdoptError::ProgramNotFound { program: super::echo(program) })?;
    let current = doc.digest().map_err(|_| AdoptError::Internal { reason: "program cannot be serialised" })?;
    if current != reviewed {
        return Err(AdoptError::StaleReview { current: current.as_str().to_owned() });
    }
    let contents = step_contents(doc).ok_or(AdoptError::Internal { reason: "program cannot be serialised" })?;
    let order: BTreeMap<Uuid7, usize> = doc.step_ids().into_iter().enumerate().map(|(i, s)| (s, i)).collect();
    let target = |step: Uuid7| Target::Step { program: id, step };
    let steps = match selection {
        Selection::AllUnadopted => {
            let all = store.unadopted_steps(doc);
            if all.is_empty() {
                return Err(AdoptError::NothingToAdopt { target: id.to_string() });
            }
            all
        }
        Selection::Ids(named) => {
            if named.is_empty() || named.len() > MAX_STEPS {
                return Err(AdoptError::bad("steps must name 1 to 100000 steps"));
            }
            let mut parsed = Vec::with_capacity(named.len());
            let mut seen = BTreeSet::new();
            for s in named {
                let step = Uuid7::parse(s).ok_or(AdoptError::bad("a step is not a step-format UUIDv7"))?;
                if !seen.insert(step) {
                    return Err(AdoptError::bad("a step is named twice"));
                }
                parsed.push(step);
            }
            if let Some(s) = parsed.iter().find(|s| !order.contains_key(s) || !contents.contains_key(s)) {
                return Err(AdoptError::UnknownStep { step: s.to_string() });
            }
            let adoptable = |s: &Uuid7| contents.get(s).is_some_and(|c| store.is_unadopted(&target(*s), c));
            if let Some(s) = parsed.iter().find(|s| !adoptable(s)) {
                return Err(AdoptError::NothingToAdopt { target: s.to_string() });
            }
            parsed.sort_by_key(|s| order.get(s).copied().unwrap_or(usize::MAX));
            parsed
        }
    };
    let contents = steps
        .iter()
        .map(|s| contents.get(s).map(|c| (target(*s), *c)))
        .collect::<Option<Vec<_>>>()
        .ok_or(AdoptError::Internal { reason: "step without content" })?;
    let event = AdoptEvent::Steps { human: human.to_owned(), program: id, steps, program_digest: current };
    Ok(Prepared { event, contents, digest: current })
}

/// Runs every check of a script version adoption against a read-only store.
pub(crate) fn prepare_version(
    store: &ProvenanceStore,
    human: &str,
    script: &str,
    version_digest: &str,
) -> Result<Prepared, AdoptError> {
    let script = ScriptId::new(script)?;
    let version = Digest::parse(version_digest).map_err(|_| AdoptError::bad("version_digest is not sha256:<hex>"))?;
    let target = Target::ScriptVersion { script: script.clone(), version };
    if !store.knows(&target) {
        return Err(AdoptError::UnknownVersion { target: target.to_string() });
    }
    if !store.is_unadopted(&target, &version) {
        return Err(AdoptError::NothingToAdopt { target: target.to_string() });
    }
    let event = AdoptEvent::Version { human: human.to_owned(), script, version_digest: version };
    Ok(Prepared { event, contents: vec![(target, version)], digest: version })
}

/// Appends the adoptions of a prepared event, in journal entry `entry`. Only after a
/// successful journal commit, or inside replay's comparison-only copy.
pub(crate) fn apply(store: &mut ProvenanceStore, prepared: &Prepared, entry: u64) {
    let human = prepared.event.human();
    for (target, content) in &prepared.contents {
        store.record_adoption(target.clone(), *content, human, prepared.digest, entry);
    }
}

/// The caller check: the bus class rule for `def` (D-031 `human_only`), then the principal
/// kind itself. Both must allow; anything that is not a `human` principal is refused.
fn require_person(principal: &Principal, def: &CommandDef) -> Result<(), AdoptError> {
    if !def.human_only {
        // The declarations below set it; a definition without it is a bug, fail closed.
        return Err(AdoptError::PolicyRefused { reason: "human_only" });
    }
    fid_bus::policy::class::authorise_class(principal, def)
        .map_err(|e| AdoptError::PolicyRefused { reason: e.reason() })?;
    if is_person(principal.kind()) {
        Ok(())
    } else {
        Err(AdoptError::PolicyRefused { reason: "human_only" })
    }
}

fn commit(ctx: &mut AdoptCtx<'_>, prepared: &Prepared) -> Result<(), AdoptError> {
    let entry =
        ctx.journal.commit(&prepared.event).map_err(|reason| AdoptError::Journal { reason: super::echo(&reason) })?;
    apply(ctx.store, prepared, entry);
    Ok(())
}

// ------------------------------------------------------------------ the commands

#[fid_command(
    id = "program.adopt_steps",
    major = 1,
    title = "Adopt program steps",
    description = "A person adopts agent-authored steps of a draft program after reviewing it, so the program can be released. Human principals only; agents may request it.",
    side_effect = W2,
    mcp_projection = RequestOnly,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [ADOPT_STEPS_COMMAND],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Available,
)]
pub fn adopt_steps(ctx: &mut AdoptCtx<'_>, params: AdoptStepsParams) -> Result<AdoptStepsResult, AdoptError> {
    require_person(ctx.principal, &__fid_command_adopt_steps().def)?;
    let selection = match (&params.steps, params.all_unadopted) {
        (Some(steps), None | Some(false)) => Selection::Ids(steps),
        (None, Some(true)) => Selection::AllUnadopted,
        (Some(_), Some(true)) => return Err(AdoptError::bad("give steps or all_unadopted, not both")),
        (None, None | Some(false)) => return Err(AdoptError::bad("give steps or all_unadopted")),
    };
    let human = ctx.principal.id().as_str().to_owned();
    let prepared = prepare_steps(ctx.store, ctx.programs, &human, &params.program, selection, &params.reviewed_digest)?;
    commit(ctx, &prepared)?;
    let adopted = match &prepared.event {
        AdoptEvent::Steps { steps, .. } => steps.iter().map(Uuid7::to_string).collect(),
        AdoptEvent::Version { .. } => Vec::new(),
    };
    Ok(AdoptStepsResult { adopted })
}

#[fid_command(
    id = "script.adopt_version",
    major = 1,
    title = "Adopt script version",
    description = "A person adopts an agent-authored script version as a whole, so it can be released. Human principals only; agents may request it.",
    side_effect = W2,
    mcp_projection = RequestOnly,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [ADOPT_VERSION_COMMAND],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Available,
)]
pub fn adopt_version(ctx: &mut AdoptCtx<'_>, params: AdoptVersionParams) -> Result<AdoptVersionResult, AdoptError> {
    require_person(ctx.principal, &__fid_command_adopt_version().def)?;
    let human = ctx.principal.id().as_str().to_owned();
    let prepared = prepare_version(ctx.store, &human, &params.script, &params.version_digest)?;
    commit(ctx, &prepared)?;
    Ok(AdoptVersionResult { adopted: true })
}

fid_commands![adopt_steps, adopt_version];

// ------------------------------------------------------------------ parameter readers

fn string(o: &BTreeMap<String, CValue>, k: &str) -> Result<Option<String>, AdoptError> {
    match o.get(k) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(AdoptError::bad("parameter is not a string")),
    }
}

fn required(o: &BTreeMap<String, CValue>, k: &str) -> Result<String, AdoptError> {
    string(o, k)?.ok_or(AdoptError::bad("missing parameter"))
}

fn steps_params(o: &BTreeMap<String, CValue>) -> Result<AdoptStepsParams, AdoptError> {
    const KEYS: [&str; 4] = ["all_unadopted", "program", "reviewed_digest", "steps"];
    if o.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return Err(AdoptError::bad("unknown parameter"));
    }
    let steps = match o.get("steps") {
        None => None,
        Some(CValue::Arr(a)) => Some(
            a.iter()
                .map(|x| match x {
                    CValue::Str(s) => Ok(s.clone()),
                    _ => Err(AdoptError::bad("steps is not a string array")),
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => return Err(AdoptError::bad("steps is not a string array")),
    };
    let all_unadopted = match o.get("all_unadopted") {
        None => None,
        Some(CValue::Bool(b)) => Some(*b),
        Some(_) => return Err(AdoptError::bad("all_unadopted is not a boolean")),
    };
    Ok(AdoptStepsParams {
        program: required(o, "program")?,
        steps,
        all_unadopted,
        reviewed_digest: required(o, "reviewed_digest")?,
    })
}

fn version_params(o: &BTreeMap<String, CValue>) -> Result<AdoptVersionParams, AdoptError> {
    if o.keys().any(|k| k != "script" && k != "version_digest") {
        return Err(AdoptError::bad("unknown parameter"));
    }
    Ok(AdoptVersionParams { script: required(o, "script")?, version_digest: required(o, "version_digest")? })
}

/// Reads MCP parameters for `tool` strictly: at most [`MAX_PARAMS_BYTES`] bytes, duplicate
/// keys refused, and any key the schema does not declare (a `kind`, a `principal`, anything)
/// is `bad_params`.
pub fn mcp_params(tool: &str, raw: &[u8]) -> Result<AdoptCall, AdoptError> {
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| AdoptError::bad("params are not JSON within the limits"))?;
    let CValue::Obj(o) = v else { return Err(AdoptError::bad("params is not an object")) };
    match tool {
        MCP_ADOPT_STEPS => steps_params(&o).map(AdoptCall::Steps),
        MCP_ADOPT_VERSION => version_params(&o).map(AdoptCall::Version),
        _ => Err(AdoptError::bad("unknown tool")),
    }
}

/// Reads `fid <args>`: `program adopt-steps --program <P> (--steps <S1>[,<S2>..] |
/// --all-unadopted) --reviewed-digest <D> [--json]` or `script adopt-version --script <X>
/// --version-digest <V> [--json]`. Unknown, repeated or value-less options and stray words
/// are `bad_params`.
pub fn cli_params(args: &[&str]) -> Result<AdoptCall, AdoptError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(AdoptError::bad("too many arguments"));
    }
    let [group, sub, rest @ ..] = args else { return Err(AdoptError::bad("missing subcommand")) };
    let steps_cmd = (*group, *sub) == CLI_ADOPT_STEPS;
    if !steps_cmd && (*group, *sub) != CLI_ADOPT_VERSION {
        return Err(AdoptError::bad("unknown subcommand"));
    }
    let mut o = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match (*arg, steps_cmd) {
            ("--json", _) if !json => {
                json = true;
                continue;
            }
            ("--all-unadopted", true) => {
                if o.insert("all_unadopted".to_owned(), CValue::Bool(true)).is_some() {
                    return Err(AdoptError::bad("repeated option"));
                }
                continue;
            }
            ("--program", true) => "program",
            ("--steps", true) => "steps",
            ("--reviewed-digest", true) => "reviewed_digest",
            ("--script", false) => "script",
            ("--version-digest", false) => "version_digest",
            _ => return Err(AdoptError::bad("unknown or repeated option")),
        };
        let value = it.next().ok_or(AdoptError::bad("option needs a value"))?;
        let v = if key == "steps" { CValue::Arr(value.split(',').map(text).collect()) } else { text(value) };
        if o.insert(key.to_owned(), v).is_some() {
            return Err(AdoptError::bad("repeated option"));
        }
    }
    if steps_cmd {
        steps_params(&o).map(AdoptCall::Steps)
    } else {
        version_params(&o).map(AdoptCall::Version)
    }
}

// ------------------------------------------------------------------ surfaces

/// The MCP tools. Request-only: the parameters are read strictly (`bad_params` first, so a
/// spoofed `kind` or `principal` field is refused as such), and then the call is always
/// `policy_refused`. A non-human principal gets `human_only` (the class check); a person gets
/// `request_only` and adopts through the CLI or the GUI. Nothing is journaled or adopted.
pub fn invoke_mcp(ctx: &mut AdoptCtx<'_>, tool: &str, raw: &[u8]) -> Result<Vec<u8>, AdoptError> {
    let call = mcp_params(tool, raw)?;
    let def = match call {
        AdoptCall::Steps(_) => __fid_command_adopt_steps().def,
        AdoptCall::Version(_) => __fid_command_adopt_version().def,
    };
    require_person(ctx.principal, &def)?;
    Err(AdoptError::PolicyRefused { reason: "request_only" })
}

/// Runs `fid <args>` (see [`cli_params`]). Output is the canonical result JSON either way.
pub fn invoke_cli(ctx: &mut AdoptCtx<'_>, args: &[&str]) -> Result<Vec<u8>, AdoptError> {
    match cli_params(args)? {
        AdoptCall::Steps(p) => adopt_steps(ctx, p).map(|r| r.to_canonical_bytes()),
        AdoptCall::Version(p) => adopt_version(ctx, p).map(|r| r.to_canonical_bytes()),
    }
}
