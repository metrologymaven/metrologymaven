//! The command `gdt.export_rule_status@1` (REQ-GDT.RES.STATUS-009): W1, `fs_effect:
//! write_root`, D0. It builds the register, writes it and its digest to an approved export
//! root, and returns the one event the bus journals (register digest, roster digest, conflict
//! inputs digest and the journal head it was built from). Agents may run it: it publishes
//! facts and changes no status. CLI `fid gdt export-rule-status --out <handle> [--at <hash>]
//! [--json]` and MCP `gdt_export_rule_status` build the same parameters and call
//! [`export_rule_status`].

use super::inputs::{CoiInputs, JournalExtract, TrustedRosters};
use super::register::{build_register, digest_of, RuleStatusRegister};
use super::StatusError;
use crate::dmt_flow::roster::Roster;
use fid_canon::{to_canonical_bytes, CValue, CanonError};
use fid_io_reg::{Principal, RootSet};
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::Write;

/// Command ID.
pub const EXPORT_COMMAND: &str = "gdt.export_rule_status@1";
/// CLI subcommand of `fid gdt`.
pub const EXPORT_CLI: &str = "export-rule-status";
/// MCP tool name.
pub const EXPORT_MCP: &str = "gdt_export_rule_status";
/// Suffix of the digest file written next to the register.
pub const DIGEST_SUFFIX: &str = ".sha256";
/// Largest MCP parameter payload, in bytes.
const MAX_PARAMS_BYTES: usize = 4 * 1024;
/// Longest `out` handle, in bytes.
const MAX_OUT_LEN: usize = 1024;
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-20";

fn bad(reason: &'static str) -> StatusError {
    StatusError::BadParams { reason }
}

/// What the export reads. `current` is the roster the register records; `trusted` holds the
/// rosters reviews may name (see [`super::Published::trusted_rosters`]).
#[derive(Debug, Clone, Copy)]
pub struct ExportContext<'a> {
    /// Approved export roots.
    pub roots: &'a RootSet,
    /// The journal extract.
    pub journal: &'a JournalExtract,
    /// The current roster.
    pub current: &'a Roster,
    /// The trusted rosters.
    pub trusted: &'a TrustedRosters,
    /// The conflict-of-interest inputs.
    pub coi: &'a CoiInputs,
}

/// The journal event of one export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEvent {
    /// Hash of the last journal entry the register was built from.
    pub journal_head: Digest,
    /// SHA-256 of the register file.
    pub register_digest: Digest,
    /// Digest of the roster the register records.
    pub roster_digest: Digest,
    /// Digest of the conflict-of-interest inputs.
    pub coi_digest: Digest,
    /// The `out` handle as given.
    pub out: String,
}

impl ExportEvent {
    /// `{"command", "coi_digest", "journal_head", "out", "register_digest", "roster_digest"}`.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("coi_digest", s(self.coi_digest.as_str())),
            ("command", s(EXPORT_COMMAND)),
            ("journal_head", s(self.journal_head.as_str())),
            ("out", s(&self.out)),
            ("register_digest", s(self.register_digest.as_str())),
            ("roster_digest", s(self.roster_digest.as_str())),
        ])
    }

    /// RFC 8785 canonical JSON of the event.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StatusError> {
        to_canonical_bytes(&self.to_cvalue()).map_err(|e| StatusError::RegisterUnreadable { reason: e.code() })
    }

    /// Reads an event back strictly (every key present, no other).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Option<ExportEvent> {
        let v = fid_canon::parse_with_limits(bytes, 8 * 1024, 1).ok()?;
        let CValue::Obj(m) = &v else { return None };
        let keys = ["coi_digest", "command", "journal_head", "out", "register_digest", "roster_digest"];
        if m.len() != keys.len() || !keys.iter().all(|k| m.contains_key(*k)) {
            return None;
        }
        let t = |k: &str| match m.get(k) {
            Some(CValue::Str(x)) => Some(x.as_str()),
            _ => None,
        };
        let d = |k: &str| t(k).and_then(|x| Digest::parse(x).ok());
        if t("command")? != EXPORT_COMMAND {
            return None;
        }
        Some(ExportEvent {
            journal_head: d("journal_head")?,
            register_digest: d("register_digest")?,
            roster_digest: d("roster_digest")?,
            coi_digest: d("coi_digest")?,
            out: t("out")?.to_owned(),
        })
    }
}

/// The outcome of an export: the register, its file bytes and the event to journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The register.
    pub register: RuleStatusRegister,
    /// The file written (canonical JSON).
    pub bytes: Vec<u8>,
    /// The event to journal.
    pub event: ExportEvent,
}

impl Exported {
    /// The command result `{register_digest, roster_digest, entries, path}`.
    pub fn result(&self) -> CValue {
        let n = u32::try_from(self.register.entries().len()).map_or(f64::from(u32::MAX), f64::from);
        obj([
            ("entries", CValue::Num(n)),
            ("path", s(&self.event.out)),
            ("register_digest", s(self.event.register_digest.as_str())),
            ("roster_digest", s(self.event.roster_digest.as_str())),
        ])
    }

    /// RFC 8785 canonical JSON of [`Exported::result`] (the `--json` output).
    pub fn result_bytes(&self) -> Result<Vec<u8>, StatusError> {
        to_canonical_bytes(&self.result()).map_err(|e| StatusError::RegisterUnreadable { reason: e.code() })
    }
}

/// Runs `gdt.export_rule_status@1`: builds the register from the journal up to `at` (default:
/// head), writes it to `out` (a `.json` file) and `sha256:<hex>` plus a newline to `out` +
/// `.sha256`, both new
/// files under an approved export root (`export_root_denied{<io code>}` otherwise; an agent or
/// script must name a `root://` handle), and returns the event to journal. Same inputs, same
/// bytes.
pub fn export_rule_status(
    ctx: &ExportContext<'_>,
    principal: Principal,
    out: &str,
    at: Option<Digest>,
) -> Result<Exported, StatusError> {
    if out.len() > MAX_OUT_LEN || !is_json_name(out) {
        return Err(bad("bad_out"));
    }
    let register = build_register(ctx.journal, at, ctx.current, ctx.trusted, ctx.coi)?;
    let bytes = register.canonical_bytes()?;
    let digest = digest_of(&bytes);
    let denied = |e: fid_io_reg::IoError| StatusError::ExportRootDenied { reason: e.code() };
    let target = ctx.roots.resolve_export(principal, out).map_err(denied)?;
    let digest_target = ctx.roots.resolve_export(principal, &format!("{out}{DIGEST_SUFFIX}")).map_err(denied)?;
    let write = |t: &fid_io_reg::ExportTarget, data: &[u8]| -> Result<(), StatusError> {
        let mut f = t.create_new().map_err(denied)?;
        f.write_all(data).and_then(|()| f.sync_all()).map_err(|_| StatusError::ExportRootDenied { reason: "io" })
    };
    write(&target, &bytes)?;
    write(&digest_target, format!("{}\n", digest.as_str()).as_bytes())?;
    let built = register.built_from();
    let event = ExportEvent {
        journal_head: built.journal_head,
        register_digest: digest,
        roster_digest: built.roster_digest,
        coi_digest: built.coi_digest,
        out: out.to_owned(),
    };
    Ok(Exported { register, bytes, event })
}

/// Exact replay of a recorded export event (D0): rebuilds the register at the event's journal
/// head and compares the re-derived event with the recorded bytes. `IDENTICAL` when equal,
/// `DIVERGENT` when they differ or the rebuild fails, `NOT_REPLAYABLE` when the event cannot be
/// read or its head is not in the journal. Writes nothing.
pub fn replay_export(
    recorded: &[u8],
    journal: &JournalExtract,
    current: &Roster,
    trusted: &TrustedRosters,
    coi: &CoiInputs,
) -> ReplayOutcome {
    let Some(event) = ExportEvent::from_canonical_bytes(recorded) else { return ReplayOutcome::NotReplayable };
    if journal.up_to(event.journal_head).is_err() {
        return ReplayOutcome::NotReplayable;
    }
    let Ok(register) = build_register(journal, Some(event.journal_head), current, trusted, coi) else {
        return ReplayOutcome::Divergent;
    };
    let Ok(bytes) = register.canonical_bytes() else { return ReplayOutcome::Divergent };
    let built = register.built_from();
    let derived = ExportEvent {
        journal_head: built.journal_head,
        register_digest: digest_of(&bytes),
        roster_digest: built.roster_digest,
        coi_digest: built.coi_digest,
        out: event.out,
    };
    match derived.canonical_bytes() {
        Ok(b) if b == recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

/// Reads `{"out": .., "at"?: ..}`: `out` a string, `at` a `sha256:` journal hash.
fn params(o: &BTreeMap<String, CValue>) -> Result<(&str, Option<Digest>), StatusError> {
    if o.keys().any(|k| k != "out" && k != "at") {
        return Err(bad("unknown parameter"));
    }
    let out = match o.get("out") {
        Some(CValue::Str(v)) => v.as_str(),
        Some(_) => return Err(bad("parameter is not a string")),
        None => return Err(bad("missing parameter")),
    };
    let at = match o.get("at") {
        None => None,
        Some(CValue::Str(v)) => Some(Digest::parse(v).map_err(|_| bad("bad journal hash"))?),
        Some(_) => return Err(bad("parameter is not a string")),
    };
    Ok((out, at))
}

/// Runs the MCP tool `gdt_export_rule_status` on raw JSON parameters (at most 4 KiB, one
/// level, duplicate keys refused).
pub fn invoke_mcp(
    ctx: &ExportContext<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<Exported, StatusError> {
    if tool != EXPORT_MCP {
        return Err(bad("unknown tool"));
    }
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, 1).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    let (out, at) = params(&o)?;
    export_rule_status(ctx, principal, out, at)
}

/// Runs `fid gdt export-rule-status --out <handle> [--at <hash>] [--json]`. Unknown
/// subcommands, unknown, repeated or value-less options and stray arguments are `bad_params`.
/// The output is [`Exported::result_bytes`] either way.
pub fn invoke_cli(ctx: &ExportContext<'_>, principal: Principal, args: &[&str]) -> Result<Exported, StatusError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(bad("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(bad("missing subcommand"))?;
    if *sub != EXPORT_CLI {
        return Err(bad("unknown subcommand"));
    }
    let mut o = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--out" => "out",
            "--at" => "at",
            "--json" => return Err(bad("repeated option")),
            a if a.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(bad("repeated option"));
        }
    }
    let (out, at) = params(&o)?;
    export_rule_status(ctx, principal, out, at)
}

/// Whether the last path component of `out` is `<stem>.json` with a non-empty stem (ASCII
/// case-insensitive).
fn is_json_name(out: &str) -> bool {
    let name = out.rsplit(['/', '\\']).next().unwrap_or("");
    name.len() > 5 && name.get(name.len() - 5..).is_some_and(|e| e.eq_ignore_ascii_case(".json"))
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| s(x)).collect())
}

fn digest_schema(description: &str) -> CValue {
    obj([("type", s("string")), ("pattern", s(Digest::PATTERN)), ("description", s(description))])
}

/// The registry definition of `gdt.export_rule_status@1` (W1, D0, `write_root`, CLI and MCP),
/// built through [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn command_def() -> Result<CommandDef, DefError> {
    let name = EXPORT_COMMAND.split('@').next().unwrap_or(EXPORT_COMMAND);
    CommandDef::from_value(obj([
        ("id", s(name)),
        ("major", CValue::Num(1.0)),
        ("title", s("Export rule status")),
        (
            "description",
            s("Builds the interpretation status register from the DMT workflow journal and writes it, with its \
               digest, to an approved export root. Publishes facts; changes no status."),
        ),
        (
            "params_schema",
            obj([
                ("type", s("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("required", strs(&["out"])),
                (
                    "properties",
                    obj([
                        ("at", digest_schema("Journal entry to build the register at; default the journal head.")),
                        (
                            "out",
                            obj([
                                ("type", s("string")),
                                ("path_like", CValue::Bool(true)),
                                ("x-extensions", strs(&["json"])),
                                ("description", s("Export handle of the register file (a .json file).")),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
        (
            "result_schema",
            obj([
                ("type", s("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("required", strs(&["entries", "path", "register_digest", "roster_digest"])),
                (
                    "properties",
                    obj([
                        ("entries", obj([("type", s("integer")), ("minimum", CValue::Num(0.0))])),
                        ("path", obj([("type", s("string"))])),
                        ("register_digest", digest_schema("SHA-256 of the register file.")),
                        ("roster_digest", digest_schema("Digest of the roster the register records.")),
                    ]),
                ),
            ]),
        ),
        ("side_effect", s("W1")),
        ("mcp_projection", s("exposed")),
        ("determinism", s("D0")),
        ("idempotency", s("none")),
        ("fs_effect", s("write_root")),
        ("undo", s("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[EXPORT_COMMAND])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", s("active")), ("since", s(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", s("none")),
        ("web", s("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
