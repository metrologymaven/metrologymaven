//! The `sign.list_meanings@1` R0 read command, declared with the registry macros,
//! with CLI (`fid sign list-meanings --json`) and MCP (`sign_list_meanings`)
//! entry points (REQ-TRUST.ESIG.MEANING-004, AC-1..AC-4).

use super::{Meaning, MeaningCatalogue, MeaningError};
use fid_bus::principal::Principal;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{fid_command, fid_commands, CommandDef, FidSchema};
use std::collections::BTreeMap;

/// `sign.list_meanings@1`.
pub const LIST_MEANINGS: &str = "sign.list_meanings@1";

const MAX_PARAMS_BYTES: usize = 1024;
const MAX_PARAMS_DEPTH: usize = 2;
const MAX_CLI_ARGS: usize = 6;

/// Context passed to `sign.list_meanings@1`.
pub struct EsigCtx<'a> {
    /// Bus-derived principal checked against class `R0`.
    pub principal: &'a Principal,
    /// Active signature meanings catalogue.
    pub catalogue: &'a MeaningCatalogue,
}

/// Parameters of `sign.list_meanings@1` (none).
#[derive(FidSchema, Debug, Clone, Default, PartialEq, Eq)]
pub struct ListMeaningsParams {}

/// Wire representation of one signature meaning returned by `sign.list_meanings@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct MeaningView {
    #[fid(description = "Machine-readable meaning code (^[A-Z][A-Z0-9_]{1,31}$).")]
    pub code: String,
    #[fid(description = "Default English display label.")]
    pub label: String,
    #[fid(description = "One-sentence description of what signing with this meaning asserts.")]
    pub description: String,
    #[fid(description = "Origin of this meaning: builtin or profile.")]
    pub origin: String,
}

impl MeaningView {
    /// Builds a wire view from a [`Meaning`].
    pub fn from_meaning(m: &Meaning) -> Self {
        Self {
            code: m.code.as_str().to_owned(),
            label: m.label.clone(),
            description: m.description.clone(),
            origin: m.origin.as_str().to_owned(),
        }
    }

    /// Converts this view to a canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_owned(), CValue::Str(self.code.clone()));
        o.insert("description".to_owned(), CValue::Str(self.description.clone()));
        o.insert("label".to_owned(), CValue::Str(self.label.clone()));
        o.insert("origin".to_owned(), CValue::Str(self.origin.clone()));
        CValue::Obj(o)
    }
}

/// Result of `sign.list_meanings@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ListMeaningsResult {
    #[fid(
        description = "Signature meanings: built-in codes first in fixed order, then profile tenant codes sorted by code."
    )]
    pub meanings: Vec<MeaningView>,
}

impl ListMeaningsResult {
    /// Converts this result to a canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("meanings".to_owned(), CValue::Arr(self.meanings.iter().map(MeaningView::to_value).collect()));
        CValue::Obj(o)
    }
}

fn authorise_class(principal: &Principal, def: &CommandDef) -> Result<(), MeaningError> {
    fid_bus::policy::class::authorise_class(principal, def)
        .map_err(|e| MeaningError::PolicyRefused { reason: e.reason() })
}

#[fid_command(
    id = "sign.list_meanings",
    major = 1,
    title = "List signature meanings",
    description = "Returns every signature meaning in the active catalogue (code, label, description, origin), ordered with built-in codes first followed by tenant profile codes sorted by code.",
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
pub fn list_meanings(ctx: &EsigCtx<'_>, _params: ListMeaningsParams) -> Result<ListMeaningsResult, MeaningError> {
    authorise_class(ctx.principal, &__fid_command_list_meanings().def)?;
    let meanings = ctx.catalogue.list().into_iter().map(MeaningView::from_meaning).collect();
    Ok(ListMeaningsResult { meanings })
}

fid_commands![list_meanings];

// ------------------------------------------------------------------ surfaces

/// One parsed call of a signature-meaning command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EsigCall {
    /// `sign.list_meanings@1`.
    ListMeanings(ListMeaningsParams),
}

impl EsigCall {
    /// Command ID with major version.
    pub const fn command(&self) -> &'static str {
        match self {
            EsigCall::ListMeanings(_) => LIST_MEANINGS,
        }
    }
}

/// Parses MCP parameters for `sign_list_meanings` (strict empty object `{}`).
pub fn mcp_params(tool: &str, raw: &[u8]) -> Result<EsigCall, MeaningError> {
    if tool != "sign_list_meanings" {
        return Err(MeaningError::bad("unknown tool"));
    }
    let val = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| MeaningError::bad("params are not JSON within limits"))?;
    let CValue::Obj(o) = val else {
        return Err(MeaningError::bad("params must be a JSON object"));
    };
    if !o.is_empty() {
        return Err(MeaningError::bad("unknown parameter"));
    }
    Ok(EsigCall::ListMeanings(ListMeaningsParams {}))
}

/// Parses CLI arguments: `fid sign list-meanings [--json]`.
pub fn cli_params(args: &[&str]) -> Result<EsigCall, MeaningError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(MeaningError::bad("too many arguments"));
    }
    let [group, sub, rest @ ..] = args else {
        return Err(MeaningError::bad("missing subcommand"));
    };
    if *group != "sign" {
        return Err(MeaningError::bad("unknown command group"));
    }
    if *sub != "list-meanings" {
        return Err(MeaningError::bad("unknown subcommand"));
    }
    let mut json = false;
    for arg in rest {
        if *arg == "--json" && !json {
            json = true;
        } else {
            return Err(MeaningError::bad("unknown or repeated option"));
        }
    }
    Ok(EsigCall::ListMeanings(ListMeaningsParams {}))
}

fn dispatch(ctx: &EsigCtx<'_>, call: EsigCall) -> Result<Vec<u8>, MeaningError> {
    match call {
        EsigCall::ListMeanings(p) => {
            let res = list_meanings(ctx, p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| MeaningError::Internal { reason: "result not canonical" })
        }
    }
}

/// Invokes `sign_list_meanings` via MCP, returning canonical JSON bytes.
pub fn invoke_mcp(ctx: &EsigCtx<'_>, tool: &str, raw: &[u8]) -> Result<Vec<u8>, MeaningError> {
    let call = mcp_params(tool, raw)?;
    dispatch(ctx, call)
}

/// Invokes `fid sign list-meanings [--json]` via CLI, returning canonical JSON bytes.
pub fn invoke_cli(ctx: &EsigCtx<'_>, args: &[&str]) -> Result<Vec<u8>, MeaningError> {
    let call = cli_params(args)?;
    dispatch(ctx, call)
}
