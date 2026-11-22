//! The two contributor read commands (`unc.list_contributor@1` and
//! `unc.show_contributor@1`), declared with the registry macros, with their CLI
//! (`fid unc list-contributor`, `fid unc show-contributor`) and MCP
//! (`unc_list_contributor`, `unc_show_contributor`) entry points (REQ-005, AC-3).
//!
//! Both commands are `R0` (read-only, Activity Log only, never modifying the
//! Record Journal).

use super::{ContribError, ContributorLibrary, ContributorModel, MAX_TAG_BYTES};
use fid_bus::principal::Principal;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{fid_command, fid_commands, CommandDef, FidSchema};
use fid_types::Digest;
use std::collections::BTreeMap;

/// `unc.list_contributor@1`.
pub const LIST_CONTRIBUTOR: &str = "unc.list_contributor@1";
/// `unc.show_contributor@1`.
pub const SHOW_CONTRIBUTOR: &str = "unc.show_contributor@1";

/// Longest contributor ID (`uc:<name>@<semver>`), in bytes.
pub const MAX_CONTRIBUTOR_ID_BYTES: usize = 128;
/// Largest MCP parameter payload accepted for contributor read commands.
pub const MAX_PARAMS_BYTES: usize = 4096;
const MAX_PARAMS_DEPTH: usize = 2;
const MAX_CLI_ARGS: usize = 8;

/// Context passed to contributor read commands.
pub struct ContribCtx<'a> {
    /// Bus-derived principal checked against the command's side-effect class (`R0`).
    pub principal: &'a Principal,
    /// Loaded contributor library.
    pub library: &'a ContributorLibrary,
}

/// Parameters of `unc.list_contributor@1`.
#[derive(FidSchema, Debug, Clone, Default, PartialEq, Eq)]
pub struct ListContributorParams {
    #[fid(
        description = "Optional applicability tag filter; when set, only contributors declaring this tag are returned.",
        min_length = 1,
        max_length = MAX_TAG_BYTES
    )]
    pub applicability: Option<String>,
}

/// Summary row for one contributor returned by `unc.list_contributor@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ContributorSummary {
    #[fid(description = "Contributor ID, uc:<name>@<semver>.")]
    pub id: String,
    #[fid(description = "Contributor semantic version MAJOR.MINOR.PATCH.")]
    pub semver: String,
    #[fid(description = "SHA-256 digest of the contributor's RFC 8785 canonical JSON body.", pattern = Digest::PATTERN)]
    pub digest: String,
    #[fid(description = "Declared parameter names, sorted ascending.")]
    pub params: Vec<String>,
    #[fid(description = "Declared applicability tags, sorted ascending.")]
    pub applicability: Vec<String>,
}

impl ContributorSummary {
    /// Builds a summary row from a [`ContributorModel`].
    pub fn from_model(model: &ContributorModel) -> Self {
        Self {
            id: model.id().to_owned(),
            semver: model.semver().to_owned(),
            digest: model.digest().as_str().to_owned(),
            params: model.param_names(),
            applicability: model.applicability().to_vec(),
        }
    }

    /// Converts this summary to a canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("applicability".to_owned(), CValue::Arr(self.applicability.iter().map(|s| text(s)).collect()));
        o.insert("digest".to_owned(), text(&self.digest));
        o.insert("id".to_owned(), text(&self.id));
        o.insert("params".to_owned(), CValue::Arr(self.params.iter().map(|s| text(s)).collect()));
        o.insert("semver".to_owned(), text(&self.semver));
        CValue::Obj(o)
    }
}

/// Result of `unc.list_contributor@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ListContributorResult {
    #[fid(description = "Shipped contributors matching the filter, sorted by ID.")]
    pub contributors: Vec<ContributorSummary>,
}

impl ListContributorResult {
    /// Converts this result to a canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert(
            "contributors".to_owned(),
            CValue::Arr(self.contributors.iter().map(ContributorSummary::to_value).collect()),
        );
        CValue::Obj(o)
    }
}

/// Parameters of `unc.show_contributor@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ShowContributorParams {
    #[fid(
        description = "Exact contributor ID, uc:<name>@<semver>.",
        min_length = 1,
        max_length = MAX_CONTRIBUTOR_ID_BYTES
    )]
    pub id: String,
}

/// Result of `unc.show_contributor@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ShowContributorResult {
    #[fid(description = "Contributor ID, uc:<name>@<semver>.")]
    pub id: String,
    #[fid(description = "Contributor semantic version MAJOR.MINOR.PATCH.")]
    pub semver: String,
    #[fid(description = "SHA-256 digest of the contributor's RFC 8785 canonical JSON body.", pattern = Digest::PATTERN)]
    pub digest: String,
    #[fid(description = "Full canonical contributor/v1 document (RFC 8785 JSON text).")]
    pub canonical: String,
}

impl ShowContributorResult {
    /// Converts this result to a canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("canonical".to_owned(), text(&self.canonical));
        o.insert("digest".to_owned(), text(&self.digest));
        o.insert("id".to_owned(), text(&self.id));
        o.insert("semver".to_owned(), text(&self.semver));
        CValue::Obj(o)
    }
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn canonical(v: &CValue) -> Result<Vec<u8>, ContribError> {
    to_canonical_bytes(v).map_err(|_| ContribError::Internal { reason: "result has no canonical form" })
}

fn authorise_class(principal: &Principal, def: &CommandDef) -> Result<(), ContribError> {
    fid_bus::policy::class::authorise_class(principal, def)
        .map_err(|e| ContribError::PolicyRefused { reason: e.reason() })
}

#[fid_command(
    id = "unc.list_contributor",
    major = 1,
    title = "List contributors",
    description = "Returns ID, semver, digest, parameter names and applicability of every shipped uncertainty contributor sorted by ID, optionally filtered by applicability.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn list_contributor(
    ctx: &ContribCtx<'_>,
    params: ListContributorParams,
) -> Result<ListContributorResult, ContribError> {
    authorise_class(ctx.principal, &__fid_command_list_contributor().def)?;
    if let Some(tag) = &params.applicability {
        if tag.is_empty() || tag.len() > MAX_TAG_BYTES {
            return Err(ContribError::bad("invalid applicability filter"));
        }
    }
    let contributors =
        ctx.library.list(params.applicability.as_deref()).into_iter().map(ContributorSummary::from_model).collect();
    Ok(ListContributorResult { contributors })
}

#[fid_command(
    id = "unc.show_contributor",
    major = 1,
    title = "Show contributor",
    description = "Returns one shipped uncertainty contributor in full by its exact ID (uc:<name>@<semver>).",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn show_contributor(
    ctx: &ContribCtx<'_>,
    params: ShowContributorParams,
) -> Result<ShowContributorResult, ContribError> {
    authorise_class(ctx.principal, &__fid_command_show_contributor().def)?;
    if params.id.is_empty() || params.id.len() > MAX_CONTRIBUTOR_ID_BYTES {
        return Err(ContribError::bad("invalid contributor id"));
    }
    let Some(model) = ctx.library.get(&params.id) else {
        return Err(ContribError::ContributorNotFound { id: params.id });
    };
    let canonical_str = String::from_utf8(model.canonical_json().to_vec())
        .map_err(|_| ContribError::Internal { reason: "canonical JSON is not UTF-8" })?;
    Ok(ShowContributorResult {
        id: model.id().to_owned(),
        semver: model.semver().to_owned(),
        digest: model.digest().as_str().to_owned(),
        canonical: canonical_str,
    })
}

fid_commands![list_contributor, show_contributor];

// ------------------------------------------------------------------ parameter readers

/// One parsed call of a contributor read command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContribCall {
    /// `unc.list_contributor@1`.
    List(ListContributorParams),
    /// `unc.show_contributor@1`.
    Show(ShowContributorParams),
}

impl ContribCall {
    /// The command ID with major version.
    pub fn command(&self) -> &'static str {
        match self {
            ContribCall::List(_) => LIST_CONTRIBUTOR,
            ContribCall::Show(_) => SHOW_CONTRIBUTOR,
        }
    }

    /// Parameter object matching the declared command schema.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        match self {
            ContribCall::List(p) => {
                if let Some(a) = &p.applicability {
                    o.insert("applicability".to_owned(), text(a));
                }
            }
            ContribCall::Show(p) => {
                o.insert("id".to_owned(), text(&p.id));
            }
        }
        CValue::Obj(o)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    List,
    Show,
}

fn call_from(which: Which, o: &BTreeMap<String, CValue>) -> Result<ContribCall, ContribError> {
    let allowed: &[&str] = match which {
        Which::List => &["applicability"],
        Which::Show => &["id"],
    };
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(ContribError::bad("unknown parameter"));
    }
    let opt_str = |k: &str| match o.get(k) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ContribError::bad("parameter is not a string")),
    };
    match which {
        Which::List => Ok(ContribCall::List(ListContributorParams { applicability: opt_str("applicability")? })),
        Which::Show => {
            let id = opt_str("id")?.ok_or(ContribError::bad("missing id"))?;
            Ok(ContribCall::Show(ShowContributorParams { id }))
        }
    }
}

/// Parses MCP parameters for `unc_list_contributor` or `unc_show_contributor`.
pub fn mcp_params(tool: &str, raw: &[u8]) -> Result<ContribCall, ContribError> {
    let which = match tool {
        "unc_list_contributor" => Which::List,
        "unc_show_contributor" => Which::Show,
        _ => return Err(ContribError::bad("unknown tool")),
    };
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| ContribError::bad("params are not JSON within limits"))?;
    let CValue::Obj(o) = v else {
        return Err(ContribError::bad("params is not an object"));
    };
    call_from(which, &o)
}

/// Parses `fid <args>`:
/// - `unc list-contributor [--applicability <tag>] [--json]`
/// - `unc show-contributor --id <id> [--json]`
pub fn cli_params(args: &[&str]) -> Result<ContribCall, ContribError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(ContribError::bad("too many arguments"));
    }
    let [group, sub, rest @ ..] = args else {
        return Err(ContribError::bad("missing subcommand"));
    };
    if *group != "unc" {
        return Err(ContribError::bad("unknown command group"));
    }
    let which = match *sub {
        "list-contributor" => Which::List,
        "show-contributor" => Which::Show,
        _ => return Err(ContribError::bad("unknown subcommand")),
    };
    let mut o = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match (*arg, which) {
            ("--json", _) if !json => {
                json = true;
                continue;
            }
            ("--applicability", Which::List) => "applicability",
            ("--id", Which::Show) => "id",
            _ => return Err(ContribError::bad("unknown or repeated option")),
        };
        let val = it.next().ok_or(ContribError::bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*val).to_owned())).is_some() {
            return Err(ContribError::bad("repeated option"));
        }
    }
    call_from(which, &o)
}

// ------------------------------------------------------------------ surfaces

fn dispatch(ctx: &ContribCtx<'_>, call: ContribCall) -> Result<Vec<u8>, ContribError> {
    match call {
        ContribCall::List(p) => canonical(&list_contributor(ctx, p)?.to_value()),
        ContribCall::Show(p) => canonical(&show_contributor(ctx, p)?.to_value()),
    }
}

/// Invokes `unc_list_contributor` or `unc_show_contributor` via MCP, returning canonical JSON.
pub fn invoke_mcp(ctx: &ContribCtx<'_>, tool: &str, raw: &[u8]) -> Result<Vec<u8>, ContribError> {
    let call = mcp_params(tool, raw)?;
    dispatch(ctx, call)
}

/// Invokes `fid unc list-contributor` or `fid unc show-contributor` via CLI, returning canonical JSON.
pub fn invoke_cli(ctx: &ContribCtx<'_>, args: &[&str]) -> Result<Vec<u8>, ContribError> {
    let call = cli_params(args)?;
    dispatch(ctx, call)
}
