//! `report.bind_block@1` (W2, D0; REQ-001, REQ-006, REQ-007), declared with the registry
//! macros, with its `--json` entry point [`execute_bind`] and its CLI words
//! [`invoke_cli`] (`fid report bind-block --report <r> --block <b> --results r1,r2 --json`).
//! The MCP tool `report_bind_block` is derived by the registry.
//!
//! The handler checks everything that can fail before it commits, in this order, and a
//! refused call journals nothing:
//!
//! 1. principal and branch (`report.policy`), in this precedence: any MCP call, whatever
//!    its principal, and any `agent` principal write only on an agent branch
//!    `agent/<client>/<n>` (so a human over MCP on an agent branch is allowed); otherwise a
//!    human never writes on an agent branch. Whether the agent branch is the caller's own is
//!    checked by the bus (CORE.BUS.ROUTE), not here. Scripts follow the bus's routing;
//! 2. parameters (`report.bad_params`; a reference carrying anything besides `kind` and `id`
//!    is `report.binding_has_value`);
//! 3. the report and the block (`report.not_found`), then the kind's resolver
//!    (`report.no_resolver`);
//! 4. every reference at the current head, in order: absent, of another kind or a field no
//!    source supplies is `report.not_found`; of another project `report.cross_project_binding`;
//! 5. for each `result` reference, one `value_binding@1` per value field of the stored
//!    record: record ID, SHA-256 of the stored record, the field's pointer and the digest of
//!    the statement the record references for it (a field without a statement is
//!    `report.value_binding_invalid`, `no_statement`; a statement not stored is
//!    `report.not_found`);
//! 6. the kind places the bindings ([`super::BlockResolver::place`]); the command writes the
//!    `binding@1` under `binding`; a value anywhere in the new payload but its
//!    top-level `binding` (a JSON number, number text, a bit pattern: [`super::find_value`]) is
//!    `report.binding_has_value`; then DOC's checks (kind, value rule) run in [`Op::apply`].
//!
//! It then commits the new stored report and one `report.bind_block@1` event through the
//! [`ReportBackend`]. The result echoes the references and the stored report digest.

use super::error::BindError;
use super::journal::{HeadLookup, JournalReads, Lookup, LookupFault};
use super::model::{Binding, Ref, RefKind, ValueBinding, BINDING_KEY, MAX_FIELD_ID_BYTES, MAX_REFS};
use super::resolve::{fault, refuse_value, resolve_ref, FieldSource, ResolverRegistry};
use crate::doc::defs::ID_PATTERN;
use crate::doc::kind::PayloadError;
use crate::doc::{
    BlockId, CanonicalJson, KindRegistry, Op, ReportBackend, ReportError, ReportEvent, ReportId, StoredReport,
    BIND_BLOCK,
};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_registry::{fid_command, fid_commands, FidSchema, Surface};
use fid_types::Digest;
use std::collections::BTreeMap;

/// CLI group (`fid report ...`).
pub const CLI_GROUP: &str = "report";
/// CLI subcommand.
pub const CLI_BIND: &str = "bind-block";
/// MCP tool name.
pub const MCP_BIND: &str = "report_bind_block";
/// Pattern of a reference ID: a canonical UUID, or a field ID.
pub const REF_ID_PATTERN: &str =
    "^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[a-z][a-z0-9_.]{0,63})$";
/// Longest client segment of an agent branch.
const MAX_CLIENT_BYTES: usize = 32;

// ------------------------------------------------------------------ caller and context

/// The kind of principal a call runs as (derived by the bus from the channel, never from
/// parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    Human,
    Agent,
    Script,
}

/// Who calls, and through which surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub principal: Principal,
    pub surface: Surface,
}

impl Caller {
    /// Whether the agent rules apply: an agent principal, or anything arriving over MCP.
    pub fn is_agent(&self) -> bool {
        self.principal == Principal::Agent || self.surface == Surface::Mcp
    }
}

/// Whether `b` is an agent branch `agent/<client>/<n>`: `<client>` 1 to 32 of `[a-z0-9-]`,
/// `<n>` a positive integer without leading zero (CORE.BUS.ROUTE).
pub fn is_agent_branch(b: &str) -> bool {
    let mut parts = b.split('/');
    let (Some("agent"), Some(client), Some(n), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let client_ok = !client.is_empty()
        && client.len() <= MAX_CLIENT_BYTES
        && client.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    let n_ok = !n.is_empty() && n.len() <= 20 && !n.starts_with('0') && n.chars().all(|c| c.is_ascii_digit());
    client_ok && n_ok
}

/// What `report.bind_block@1` runs against: the report store and journal seam, the kind
/// and resolver registries, the project's journal reads, the form data sources, the caller
/// and the branch the backend commits to.
pub struct BindCtx<'a> {
    pub backend: &'a mut dyn ReportBackend,
    pub kinds: &'a KindRegistry,
    pub resolvers: &'a ResolverRegistry,
    pub journal: &'a dyn JournalReads,
    pub sources: &'a [&'a dyn FieldSource],
    pub caller: Caller,
    pub branch: &'a str,
}

// ------------------------------------------------------------------ params and result

/// One reference: its kind and its ID. There is no other field: a reference cannot carry a
/// value.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct RefParam {
    #[fid(description = "What the ID names.")]
    pub kind: RefKind,
    #[fid(
        description = "A canonical lowercase UUID, or a field ID for kind field.",
        pattern = REF_ID_PATTERN,
        max_length = MAX_FIELD_ID_BYTES
    )]
    pub id: String,
}

impl RefParam {
    fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), CValue::Str(self.id.clone()));
        o.insert("kind".to_string(), CValue::Str(self.kind.as_str().to_string()));
        CValue::Obj(o)
    }
}

/// Parameters of `report.bind_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct BindBlockParams {
    #[fid(description = "The report.", pattern = ID_PATTERN)]
    pub report: String,
    #[fid(description = "The block to bind.", pattern = ID_PATTERN)]
    pub block: String,
    #[fid(description = "The stable IDs the block shows, in order; IDs only, never values.", max_items = MAX_REFS)]
    pub refs: Vec<RefParam>,
}

impl BindBlockParams {
    /// Reads `--json` / MCP parameters. Unknown keys and wrong types are
    /// `report.bad_params`; a reference with any key besides `kind` and `id` (a value, a
    /// unit, formatted text, a label) is `report.binding_has_value` naming that key.
    pub fn from_value(v: &CValue) -> Result<BindBlockParams, BindError> {
        let CValue::Obj(o) = v else { return Err(BindError::BadParams("params is not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "report" | "block" | "refs")) {
            return Err(BindError::BadParams("unknown parameter"));
        }
        let text = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.clone()),
            _ => Err(BindError::BadParams("report and block are id strings")),
        };
        let Some(CValue::Arr(items)) = o.get("refs") else { return Err(BindError::BadParams("refs is not an array")) };
        if items.len() > MAX_REFS {
            return Err(BindError::BadParams("too many refs"));
        }
        let mut refs = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let CValue::Obj(r) = item else { return Err(BindError::BadParams("ref is not an object")) };
            if let Some(k) = r.keys().find(|k| !matches!(k.as_str(), "kind" | "id")) {
                return Err(BindError::BindingHasValue(PayloadError::new(
                    &format!("/refs/{i}/{k}"),
                    "ref_carries_value",
                )));
            }
            let kind = match r.get("kind") {
                Some(CValue::Str(s)) => RefKind::parse(s).ok_or(BindError::BadParams("unknown ref kind"))?,
                _ => return Err(BindError::BadParams("ref kind is not a string")),
            };
            let id = match r.get("id") {
                Some(CValue::Str(s)) => s.clone(),
                _ => return Err(BindError::BadParams("ref id is not a string")),
            };
            refs.push(RefParam { kind, id });
        }
        Ok(BindBlockParams { report: text("report")?, block: text("block")?, refs })
    }
}

/// Result of `report.bind_block@1`: the block, the references now stored, and the digest of
/// the stored report after the command.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct BindBlockResult {
    #[fid(description = "The bound block (UUIDv7).", pattern = ID_PATTERN)]
    pub block_id: String,
    #[fid(description = "The stored references, in order.", max_items = MAX_REFS)]
    pub refs: Vec<RefParam>,
    #[fid(description = "SHA-256 of the stored report after the command.", pattern = Digest::PATTERN)]
    pub report_digest: String,
}

impl BindBlockResult {
    /// The `--json` / MCP result value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("block_id".to_string(), CValue::Str(self.block_id.clone()));
        o.insert("refs".to_string(), CValue::Arr(self.refs.iter().map(RefParam::to_value).collect()));
        o.insert("report_digest".to_string(), CValue::Str(self.report_digest.clone()));
        CValue::Obj(o)
    }
}

// ------------------------------------------------------------------ handler

fn canonical_uuid(s: &str) -> Option<Uuid> {
    Uuid::parse(s).ok().filter(|u| u.to_string() == s)
}

fn check_branch(ctx: &BindCtx<'_>) -> Result<(), BindError> {
    let on_agent_branch = is_agent_branch(ctx.branch);
    if ctx.caller.is_agent() && !on_agent_branch {
        return Err(BindError::Policy("agents bind blocks only on their agent branch"));
    }
    if ctx.caller.principal == Principal::Human && ctx.caller.surface != Surface::Mcp && on_agent_branch {
        return Err(BindError::Policy("a person binds blocks on the working branch, not on an agent branch"));
    }
    Ok(())
}

/// The value bindings of one `result` reference at the lookup's head (step 5).
fn record_bindings(block: BlockId, id: &Uuid, lookup: &dyn Lookup) -> Result<Vec<ValueBinding>, BindError> {
    let record = lookup.result(id).map_err(|e| fault(block, &id.to_string(), e))?;
    let mut out = Vec::with_capacity(record.values().len());
    for f in record.values() {
        let statement = f.statement().ok_or_else(|| {
            BindError::from(ReportError::ValueBindingInvalid(PayloadError::new(f.field(), "no_statement")))
        })?;
        lookup.statement(&statement).map_err(|e| match e {
            LookupFault::Invalid(r) => BindError::StoreInvalid(r),
            other => fault(block, &id.to_string(), other),
        })?;
        out.push(ValueBinding {
            record: *id,
            record_digest: record.digest(),
            field: f.field().to_string(),
            statement_digest: statement,
        });
    }
    Ok(out)
}

#[fid_command(
    id = "report.bind_block",
    major = 1,
    title = "Bind block",
    description = "Stores in a block the stable IDs it shows (ResultRecords, characteristics, saved views, \
                   statistics series, form fields) and one value binding per value field of each bound record; \
                   never a copied value.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [BIND_BLOCK],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn bind_block(ctx: &mut BindCtx<'_>, params: BindBlockParams) -> Result<BindBlockResult, BindError> {
    // 1. Principal and branch.
    check_branch(ctx)?;
    // 2. Parameters.
    let report = canonical_uuid(&params.report).ok_or(BindError::BadParams("report is not a canonical uuid"))?;
    let block = canonical_uuid(&params.block).ok_or(BindError::BadParams("block is not a canonical uuid"))?;
    let refs = params
        .refs
        .iter()
        .map(|r| Ref::new(r.kind, &r.id).ok_or(BindError::BadParams("ref id does not fit its kind")))
        .collect::<Result<Vec<_>, _>>()?;
    let binding = Binding::new(refs)?;
    // 3. Report, block, resolver.
    let rid = ReportId::new(report).ok_or(ReportError::NotFound(report))?;
    let stored = ctx.backend.load(&rid)?.ok_or(ReportError::NotFound(report))?;
    let current = stored.open(&rid, ctx.kinds)?;
    let b = current.block(&block).ok_or(ReportError::NotFound(block))?;
    let bid = b.id();
    let resolver = ctx.resolvers.get(b.kind())?;
    // 4. and 5. References and value bindings at the current head.
    let head = ctx.journal.current_head();
    let lookup = match &head {
        Some(h) => Some(HeadLookup::new(ctx.journal, ctx.backend.project(), h)?),
        None => None,
    };
    let mut values = Vec::new();
    for r in binding.refs() {
        let Some(lookup) = &lookup else { return Err(BindError::NotFound(r.id_text())) };
        resolve_ref(bid, r, lookup, ctx.sources).map_err(|e| match e {
            BindError::UnjournaledRecord { record, .. } => BindError::NotFound(record),
            other => other,
        })?;
        if let Ref::Result(u) = r {
            values.extend(record_bindings(bid, u, lookup)?);
        }
    }
    // 6. Placement, the binding, the no-number rule.
    let placed = resolver.place(b.payload(), &binding, &values).map_err(ReportError::InvalidBlockPayload)?;
    let CValue::Obj(mut payload) = placed else {
        return Err(ReportError::InvalidBlockPayload(PayloadError::new("", "not_object")).into());
    };
    payload.insert(BINDING_KEY.to_string(), binding.to_value());
    let payload = CValue::Obj(payload);
    refuse_value(&payload, true)?;
    let payload = CanonicalJson::new(&payload)
        .ok_or_else(|| ReportError::InvalidBlockPayload(PayloadError::new("", "payload_bounds")))?;
    let op = Op::BindBlock { report: rid, block: bid, payload };
    let next = op.apply(Some(current), ctx.kinds)?;
    let stored = StoredReport::of(&next)?;
    let digest = stored.digest;
    ctx.backend.commit(&rid, stored, ReportEvent::new(op, digest))?;
    Ok(BindBlockResult {
        block_id: bid.uuid().to_string(),
        refs: params.refs,
        report_digest: digest.as_str().to_string(),
    })
}

fid_commands![bind_block];

// ------------------------------------------------------------------ surfaces

/// `--json` / MCP: reads the parameters and returns the result value.
pub fn execute_bind(ctx: &mut BindCtx<'_>, params: &CValue) -> Result<CValue, BindError> {
    bind_block(ctx, BindBlockParams::from_value(params)?).map(|r| r.to_value())
}

/// `fid report <args>`: `args` are the words after `report`, e.g.
/// `bind-block --report <r> --block <b> --results r1,r2 --characteristics c1 --views v1
/// --series s1 --fields f1 --json`. References are taken in the order the options are
/// written, each list left to right. The result is the `--json` value.
pub fn invoke_cli(ctx: &mut BindCtx<'_>, args: &[&str]) -> Result<CValue, BindError> {
    let Some((&sub, mut rest)) = args.split_first() else { return Err(BindError::BadParams("missing subcommand")) };
    if sub != CLI_BIND {
        return Err(BindError::BadParams("unknown subcommand"));
    }
    let (mut report, mut block, mut refs) = (None, None, Vec::new());
    while let Some((&opt, tail)) = rest.split_first() {
        rest = tail;
        if opt == "--json" {
            continue;
        }
        let Some((&value, tail)) = rest.split_first() else { return Err(BindError::BadParams("option without value")) };
        rest = tail;
        let kind = match opt {
            "--report" if report.is_none() => {
                report = Some(value.to_string());
                continue;
            }
            "--block" if block.is_none() => {
                block = Some(value.to_string());
                continue;
            }
            "--results" => RefKind::Result,
            "--characteristics" => RefKind::Characteristic,
            "--views" => RefKind::NamedView,
            "--series" => RefKind::Series,
            "--fields" => RefKind::Field,
            _ => return Err(BindError::BadParams("unknown or repeated option")),
        };
        for id in value.split(',') {
            if refs.len() >= MAX_REFS {
                return Err(BindError::BadParams("too many refs"));
            }
            refs.push(RefParam { kind, id: id.to_string() });
        }
    }
    let (Some(report), Some(block)) = (report, block) else {
        return Err(BindError::BadParams("--report and --block are required"));
    };
    bind_block(ctx, BindBlockParams { report, block, refs }).map(|r| r.to_value())
}
