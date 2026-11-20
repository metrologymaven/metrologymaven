//! `verify.inspect_package@1` (R0, D0; REQ-TRUST.PKG.READER-006): the read-only command that
//! lists a package's contents, declared with the registry macros, with its CLI entry point
//! (`fid verify inspect-package <package> [--json]`) and its MCP tool
//! (`verify_inspect_package`).
//!
//! Both surfaces resolve the package through a [`PackageOpener`] and call one handler, so the
//! CLI's `--json` output and the MCP result are the same canonical JSON (RFC 8785) of a
//! [`PackageInventory`]. On the CLI the handle is a path chosen by the person typing it
//! ([`FileOpener`]); on MCP it is an opaque handle the host registered ([`HandleTable`]), so an
//! agent never passes a raw path. Neither opens anything for writing.
//!
//! The command opens the package with the bounded reader, then verifies each manifest member
//! (length, SHA-256, CRC-32) and reports a status per member; it never checks signatures and
//! only counts the `signatures/` entries. A package that fails to open (container, limits,
//! manifest, membership) is an error with the reader's code.

use super::error::ReadError;
use super::PackageReader;
use crate::manifest::EvidenceTier;
use fid_canon::{to_canonical_bytes, CValue};
use fid_registry::{fid_command, fid_commands, FidSchema};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::PathBuf;

/// Command ID.
pub const COMMAND_ID: &str = "verify.inspect_package@1";
/// CLI group (`fid verify ...`).
pub const CLI_GROUP: &str = "verify";
/// CLI subcommand of `fid verify`.
pub const CLI_INSPECT: &str = "inspect-package";
/// MCP tool name.
pub const MCP_INSPECT: &str = "verify_inspect_package";
/// Longest package handle, in bytes.
const MAX_HANDLE_BYTES: usize = 4096;
/// Largest MCP parameter payload, in bytes.
const MAX_PARAMS_BYTES: usize = 8192;
/// Deepest MCP parameter nesting.
const MAX_PARAMS_DEPTH: usize = 4;

/// A seekable byte source a package is read from.
pub trait PackageSource: Read + Seek {}
impl<T: Read + Seek + ?Sized> PackageSource for T {}

/// Resolves a package handle to a read-only source.
pub trait PackageOpener {
    /// Opens the package named by `handle`; failures are `pkg.io`.
    fn open_package(&self, handle: &str) -> Result<Box<dyn PackageSource>, ReadError>;
}

/// The CLI's opener: the handle is a file path, opened read-only.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileOpener;

impl PackageOpener for FileOpener {
    fn open_package(&self, handle: &str) -> Result<Box<dyn PackageSource>, ReadError> {
        let f = File::open(handle).map_err(|e| ReadError::Io(format!("cannot open the package: {e}")))?;
        Ok(Box::new(f))
    }
}

/// The MCP host's opener: only handles the host registered resolve, each to a file it chose.
#[derive(Debug, Clone, Default)]
pub struct HandleTable {
    map: BTreeMap<String, PathBuf>,
}

impl HandleTable {
    /// An empty table.
    pub fn new() -> HandleTable {
        HandleTable::default()
    }

    /// Registers `handle` for the file at `path`.
    pub fn insert(&mut self, handle: &str, path: impl Into<PathBuf>) {
        self.map.insert(handle.to_string(), path.into());
    }
}

impl PackageOpener for HandleTable {
    fn open_package(&self, handle: &str) -> Result<Box<dyn PackageSource>, ReadError> {
        let path = self.map.get(handle).ok_or_else(|| ReadError::Io("unknown package handle".into()))?;
        let f = File::open(path).map_err(|e| ReadError::Io(format!("cannot open the package: {e}")))?;
        Ok(Box::new(f))
    }
}

// ------------------------------------------------------------------ params and result

/// Parameters of `verify.inspect_package@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct InspectParams {
    #[fid(
        description = "Handle of the Result Package to inspect (a file path on the CLI, a host-issued handle on MCP).",
        min_length = 1,
        max_length = 4096
    )]
    pub package: String,
}

/// Evidence tier of the package.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryTier {
    Results,
    Features,
    Points,
}

impl InventoryTier {
    fn of(t: EvidenceTier) -> InventoryTier {
        match t {
            EvidenceTier::Results => InventoryTier::Results,
            EvidenceTier::Features => InventoryTier::Features,
            EvidenceTier::Points => InventoryTier::Points,
        }
    }

    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            InventoryTier::Results => "results",
            InventoryTier::Features => "features",
            InventoryTier::Points => "points",
        }
    }
}

/// Verification status of one member.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberState {
    /// Length, SHA-256 and CRC-32 match.
    Ok,
    /// Length or SHA-256 differs from the manifest (`pkg.digest_mismatch`).
    DigestMismatch,
    /// Decoding stopped at a limit (`pkg.limit`).
    Limit,
    /// The member's data is broken (`pkg.malformed`).
    Malformed,
    /// Reading failed (`pkg.io`).
    Io,
}

impl MemberState {
    fn of(r: &Result<(), ReadError>) -> MemberState {
        match r {
            Ok(()) => MemberState::Ok,
            Err(ReadError::DigestMismatch { .. }) => MemberState::DigestMismatch,
            Err(ReadError::Limit { .. }) => MemberState::Limit,
            Err(ReadError::Io(_)) => MemberState::Io,
            Err(_) => MemberState::Malformed,
        }
    }

    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            MemberState::Ok => "ok",
            MemberState::DigestMismatch => "digest_mismatch",
            MemberState::Limit => "limit",
            MemberState::Malformed => "malformed",
            MemberState::Io => "io",
        }
    }
}

/// One manifest member with its status.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct MemberReport {
    pub path: String,
    pub media_type: String,
    #[fid(description = "Byte length from the manifest, as a decimal string.", pattern = "^(0|[1-9][0-9]*)$")]
    pub length: String,
    #[fid(description = "SHA-256 from the manifest, lowercase hex.", pattern = "^[0-9a-f]{64}$")]
    pub sha256: String,
    pub status: MemberState,
}

/// Result of `verify.inspect_package@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
#[fid(description = "The contents of a Result Package; signatures are counted, not verified.")]
pub struct PackageInventory {
    #[fid(pattern = r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")]
    pub format_version: String,
    pub evidence_tier: InventoryTier,
    #[fid(pattern = "^sha256:[0-9a-f]{64}$")]
    pub profile_digest: String,
    pub members: Vec<MemberReport>,
    #[fid(description = "Number of entries under signatures/ (not verified).")]
    pub signature_count: u32,
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

impl PackageInventory {
    /// The JSON value (RFC 8785 order when written).
    pub fn to_value(&self) -> CValue {
        let members = self
            .members
            .iter()
            .map(|m| {
                let mut o = BTreeMap::new();
                o.insert("path".to_string(), text(&m.path));
                o.insert("media_type".to_string(), text(&m.media_type));
                o.insert("length".to_string(), text(&m.length));
                o.insert("sha256".to_string(), text(&m.sha256));
                o.insert("status".to_string(), text(m.status.as_str()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("format_version".to_string(), text(&self.format_version));
        o.insert("evidence_tier".to_string(), text(self.evidence_tier.as_str()));
        o.insert("profile_digest".to_string(), text(&self.profile_digest));
        o.insert("members".to_string(), CValue::Arr(members));
        o.insert("signature_count".to_string(), CValue::Num(f64::from(self.signature_count)));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        // Only strings and a u32 count: canonicalisation cannot fail.
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Whether every member is `ok`.
    pub fn all_ok(&self) -> bool {
        self.members.iter().all(|m| m.status == MemberState::Ok)
    }
}

// ------------------------------------------------------------------ the command

/// Opens `src` with the bounded reader and builds the inventory.
pub fn inspect<R: Read + Seek>(src: R) -> Result<PackageInventory, ReadError> {
    let mut reader = PackageReader::open(src)?;
    let results = reader.member_results();
    let signature_count = u32::try_from(reader.signature_count()).unwrap_or(u32::MAX);
    let m = reader.manifest();
    let members = m
        .members
        .iter()
        .zip(&results)
        .map(|(member, r)| MemberReport {
            path: member.path.as_str().to_string(),
            media_type: member.media_type.clone(),
            length: member.length.to_string(),
            sha256: member.sha256.hex().to_string(),
            status: MemberState::of(r),
        })
        .collect();
    Ok(PackageInventory {
        format_version: m.format_version.to_string(),
        evidence_tier: InventoryTier::of(m.evidence_tier),
        profile_digest: m.profile_digest.as_str().to_string(),
        members,
        signature_count,
    })
}

#[fid_command(
    id = "verify.inspect_package",
    major = 1,
    title = "Inspect package",
    description = "Lists a Result Package's format version, evidence tier, profile digest, members with their digests and verification status, and the number of signatures, without verifying signatures.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = ReadRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Unavailable,
    web_note = "Packages are opened from the local file system, which the web build cannot reach.",
)]
pub fn inspect_package(ctx: &dyn PackageOpener, params: InspectParams) -> Result<PackageInventory, ReadError> {
    if params.package.is_empty() || params.package.len() > MAX_HANDLE_BYTES {
        return Err(ReadError::Io("the package handle must be 1 to 4096 bytes".into()));
    }
    inspect(ctx.open_package(&params.package)?)
}

fid_commands![inspect_package];

// ------------------------------------------------------------------ surfaces

fn error_value(code: &str, reason: Option<&str>, message: &str) -> Vec<u8> {
    let mut e = BTreeMap::new();
    e.insert("code".to_string(), text(code));
    if let Some(r) = reason {
        e.insert("reason".to_string(), text(r));
    }
    e.insert("message".to_string(), text(message));
    let mut o = BTreeMap::new();
    o.insert("error".to_string(), CValue::Obj(e));
    to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
}

fn read_error_value(e: &ReadError) -> Vec<u8> {
    error_value(e.code(), Some(e.reason()), &e.to_string())
}

/// What the CLI prints and its exit code: 0 when every member is `ok`, 1 when the package
/// cannot be opened or a member fails, 2 for a usage error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn usage(msg: &str) -> CliOutput {
    let text = format!("error: {msg}\nusage: fid {CLI_GROUP} {CLI_INSPECT} <package> [--json]\n");
    CliOutput { code: 2, stdout: Vec::new(), stderr: text.into_bytes() }
}

/// `fid verify <args>`: `args` are the words after `verify`.
pub fn invoke_cli(opener: &dyn PackageOpener, args: &[&str]) -> CliOutput {
    let Some((&sub, rest)) = args.split_first() else { return usage("missing subcommand") };
    if sub != CLI_INSPECT {
        return usage("unknown subcommand");
    }
    let mut json = false;
    let mut package: Option<&str> = None;
    for &a in rest {
        match a {
            "--json" if !json => json = true,
            _ if a.starts_with("--") => return usage("unknown option"),
            _ if package.is_none() => package = Some(a),
            _ => return usage("more than one package"),
        }
    }
    let Some(package) = package else { return usage("missing package") };
    match inspect_package(opener, InspectParams { package: package.to_string() }) {
        Ok(inv) => {
            let code = if inv.all_ok() { 0 } else { 1 };
            let stdout = if json {
                let mut b = inv.to_canonical_bytes();
                b.push(b'\n');
                b
            } else {
                let mut s = format!(
                    "format_version {}\nevidence_tier {}\nprofile_digest {}\nsignatures {} (not verified)\n",
                    inv.format_version,
                    inv.evidence_tier.as_str(),
                    inv.profile_digest,
                    inv.signature_count
                );
                for m in &inv.members {
                    s.push_str(&format!("{} {} {} {:?}\n", m.status.as_str(), m.sha256, m.length, m.path));
                }
                s.into_bytes()
            };
            CliOutput { code, stdout, stderr: Vec::new() }
        }
        Err(e) if json => {
            let mut b = read_error_value(&e);
            b.push(b'\n');
            CliOutput { code: 1, stdout: b, stderr: Vec::new() }
        }
        Err(e) => CliOutput { code: 1, stdout: Vec::new(), stderr: format!("error: {e}\n").into_bytes() },
    }
}

/// The MCP tool: `Ok` holds the canonical result bytes (equal to the CLI's `--json` output
/// without its newline), `Err` a canonical `{"error": {...}}` object.
pub fn invoke_mcp(opener: &dyn PackageOpener, tool: &str, params: &[u8]) -> Result<Vec<u8>, Vec<u8>> {
    if tool != MCP_INSPECT {
        return Err(error_value("mcp.unknown_tool", None, "unknown tool"));
    }
    let bad = |m: &str| error_value("params.invalid", None, m);
    if params.len() > MAX_PARAMS_BYTES {
        return Err(bad("parameters too large"));
    }
    let v = fid_canon::parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|e| bad(&format!("parameters are not JSON: {}", e.code())))?;
    let CValue::Obj(o) = v else { return Err(bad("parameters must be an object")) };
    if o.keys().any(|k| k != "package") {
        return Err(bad("unknown parameter"));
    }
    let package = match o.get("package") {
        Some(CValue::Str(s)) if !s.is_empty() && s.len() <= MAX_HANDLE_BYTES => s.clone(),
        _ => return Err(bad("package must be a string of 1 to 4096 bytes")),
    };
    inspect_package(opener, InspectParams { package })
        .map(|inv| inv.to_canonical_bytes())
        .map_err(|e| read_error_value(&e))
}
