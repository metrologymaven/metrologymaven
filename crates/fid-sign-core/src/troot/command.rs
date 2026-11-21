//! `verify.check_signature@1` (R1, D0): checks every DSSE envelope of a Result Package
//! against its `statement.json` and the offline trust files, declared with the registry
//! macros, with its CLI entry point
//! (`fid verify check-signature <package> --trust-root <file> [--revoked <file>] [--keys <file>] [--as-of <time>] [--json]`)
//! and its MCP tool (`verify_check_signature`).
//!
//! Both surfaces resolve the package and the three files through one
//! [`PackageOpener`](fid_pkg::reader::PackageOpener): on the CLI the handles are paths chosen
//! by the person typing them ([`FileOpener`](fid_pkg::reader::FileOpener)); on MCP they are
//! opaque handles the host registered ([`HandleTable`](fid_pkg::reader::HandleTable)), so an
//! agent never passes a raw path. Nothing is opened for writing and nothing is fetched over
//! a network (REQ-TRUST.SIGN.TROOT-004). The CLI's `--json` output and the MCP result are the
//! same canonical JSON (RFC 8785) of a [`SignatureReport`].
//!
//! The trust files are read and parsed first, so a malformed file fails the command with
//! `troot.malformed` before any signature is looked at (REQ-TRUST.SIGN.TROOT-005). The
//! package is read only through the bounded reader of `fid-pkg` (TRUST.PKG.READER), which
//! pins the SHA-256 of `statement.json` at open; envelopes are `signatures/*` entries in
//! name order, at most [`MAX_SIGNATURE_ENTRIES`] of them, each parsed with the DSSE bounds
//! (`sig.malformed` otherwise). A package with no `signatures/` entry is `UNSIGNED`
//! (REQ-TRUST.SIGN.TROOT-006).

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Seek};

use fid_canon::{to_canonical_bytes, CValue};
use fid_pkg::reader::{PackageOpener, PackageReader, ReadError};
use fid_registry::{fid_command, fid_commands, FidSchema};

use super::{
    check_envelope, Keyring, Revocations, SignatureEntry, SignatureStatus, TrootError, TrustInputs, TrustRoot, UtcTime,
    MAX_FILE_BYTES, MAX_SIGNATURES_PER_PACKAGE,
};
use crate::{Envelope, SigError, MAX_ENVELOPE_BYTES};

/// Command ID.
pub const COMMAND_ID: &str = "verify.check_signature@1";
/// CLI subcommand of `fid verify`.
pub const CLI_CHECK: &str = "check-signature";
/// MCP tool name.
pub const MCP_CHECK: &str = "verify_check_signature";
/// Most `signatures/` entries checked in one package.
pub const MAX_SIGNATURE_ENTRIES: usize = 64;
/// Longest handle, in bytes.
const MAX_HANDLE_BYTES: usize = 4096;
/// Largest MCP parameter payload, in bytes.
const MAX_PARAMS_BYTES: usize = 32 * 1024;
/// Deepest MCP parameter nesting.
const MAX_PARAMS_DEPTH: usize = 4;

/// Why the command failed. [`CheckError::code`] gives the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// `pkg.*`: the package cannot be opened or an entry fails its checks.
    Package(ReadError),
    /// `troot.malformed`: a trust-root, revocation or keyring file is refused.
    Trust(TrootError),
    /// `sig.malformed`: an envelope is not a well-formed DSSE envelope, or there are too many.
    Signature { entry: String, error: SigError },
    /// `params.invalid`: a handle is empty or too long, or `as_of` is not an RFC 3339 UTC time.
    Params(&'static str),
}

impl CheckError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            CheckError::Package(e) => e.code(),
            CheckError::Trust(e) => e.code(),
            CheckError::Signature { error, .. } => error.code(),
            CheckError::Params(_) => "params.invalid",
        }
    }

    /// Stable reason, where the code has one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            CheckError::Package(e) => Some(e.reason()),
            CheckError::Trust(e) => Some(e.reason),
            CheckError::Signature { .. } | CheckError::Params(_) => None,
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Package(e) => write!(f, "{e}"),
            CheckError::Trust(e) => write!(f, "{e}"),
            CheckError::Signature { entry, error } => write!(f, "{entry}: {error}"),
            CheckError::Params(m) => write!(f, "params.invalid: {m}"),
        }
    }
}

impl std::error::Error for CheckError {}

// ------------------------------------------------------------------ params and result

/// Parameters of `verify.check_signature@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct CheckParams {
    #[fid(
        description = "Handle of the Result Package (a file path on the CLI, a host-issued handle on MCP).",
        min_length = 1,
        max_length = 4096
    )]
    pub package: String,
    #[fid(description = "Handle of the trust-root file (trusted keys).", min_length = 1, max_length = 4096)]
    pub trust_root: String,
    #[fid(description = "Handle of the revocation file, if any.", min_length = 1, max_length = 4096)]
    pub revoked: Option<String>,
    #[fid(
        description = "Handle of a keyring of known but untrusted keys, so their valid signatures report VALID_UNTRUSTED.",
        min_length = 1,
        max_length = 4096
    )]
    pub keys: Option<String>,
    #[fid(
        description = "Reference time supplied by the caller (RFC 3339 UTC; the command never reads a clock). A key whose not_after is before it gets the note key_validity_ended_as_of.",
        min_length = 20,
        max_length = 30
    )]
    pub as_of: Option<String>,
}

/// Whether the package is signed at all.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageStatus {
    #[fid(rename = "SIGNED")]
    Signed,
    #[fid(rename = "UNSIGNED")]
    Unsigned,
}

impl PackageStatus {
    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            PackageStatus::Signed => "SIGNED",
            PackageStatus::Unsigned => "UNSIGNED",
        }
    }
}

/// Result of `verify.check_signature@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
#[fid(description = "One status per signature of a Result Package, checked against offline trust files.")]
pub struct SignatureReport {
    pub package_status: PackageStatus,
    #[fid(description = "SHA-256 of the package's statement.json, lowercase hex.", pattern = "^[0-9a-f]{64}$")]
    pub statement_sha256: Option<String>,
    pub signatures: Vec<SignatureEntry>,
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

impl SignatureReport {
    /// The JSON value (RFC 8785 order when written); absent optional fields are omitted.
    pub fn to_value(&self) -> CValue {
        let sigs = self
            .signatures
            .iter()
            .map(|s| {
                let mut o = BTreeMap::new();
                o.insert("index".to_owned(), CValue::Num(f64::from(s.index)));
                o.insert("envelope".to_owned(), text(&s.envelope));
                o.insert("claimed_keyid".to_owned(), text(&s.claimed_keyid));
                o.insert("status".to_owned(), text(s.status.as_str()));
                let optional = [
                    ("fingerprint", &s.fingerprint),
                    ("key_label", &s.key_label),
                    ("detail", &s.detail),
                    ("claimed_signed_at", &s.claimed_signed_at),
                    ("claimed_meaning_code", &s.claimed_meaning_code),
                ];
                for (k, v) in optional {
                    if let Some(v) = v {
                        o.insert(k.to_owned(), text(v));
                    }
                }
                o.insert("notes".to_owned(), CValue::Arr(s.notes.iter().map(|n| text(n)).collect()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("package_status".to_owned(), text(self.package_status.as_str()));
        if let Some(d) = &self.statement_sha256 {
            o.insert("statement_sha256".to_owned(), text(d));
        }
        o.insert("signatures".to_owned(), CValue::Arr(sigs));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        // Only strings and u32 indexes: canonicalisation cannot fail.
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Whether every signature is `VALID_TRUSTED` (false for an unsigned package).
    pub fn all_trusted(&self) -> bool {
        !self.signatures.is_empty() && self.signatures.iter().all(|s| s.status == SignatureStatus::ValidTrusted)
    }
}

// ------------------------------------------------------------------ the command

/// Checks the package in `src` against already parsed trust inputs.
pub fn check_package<R: Read + Seek>(src: R, inputs: &TrustInputs<'_>) -> Result<SignatureReport, CheckError> {
    let mut reader = PackageReader::open(src).map_err(CheckError::Package)?;
    let statement = reader.statement_sha256().map(|d| d.hex().to_owned());
    let names: Vec<String> =
        reader.entry_names().into_iter().filter(|n| n.starts_with("signatures/")).map(str::to_owned).collect();
    if names.len() > MAX_SIGNATURE_ENTRIES {
        return Err(CheckError::Signature {
            entry: "signatures/".to_owned(),
            error: SigError::Malformed("more than 64 signature entries"),
        });
    }
    let package_status = if names.is_empty() { PackageStatus::Unsigned } else { PackageStatus::Signed };
    let cap = u64::try_from(MAX_ENVELOPE_BYTES).unwrap_or(u64::MAX);
    let mut signatures = Vec::new();
    for name in names {
        let bytes = reader.read_member(&name, cap).map_err(CheckError::Package)?.unwrap_or_default();
        let envelope =
            Envelope::from_json(&bytes).map_err(|error| CheckError::Signature { entry: name.clone(), error })?;
        // The cap is on signatures over all envelopes, checked before this envelope's are.
        if signatures.len().saturating_add(envelope.signatures.len()) > MAX_SIGNATURES_PER_PACKAGE {
            return Err(CheckError::Signature {
                entry: name,
                error: SigError::Malformed("more than 64 signatures in the package"),
            });
        }
        let first = u32::try_from(signatures.len()).unwrap_or(u32::MAX);
        signatures.extend(check_envelope(&envelope, &name, statement.as_deref(), inputs, first));
    }
    Ok(SignatureReport { package_status, statement_sha256: statement, signatures })
}

fn handle(h: &str) -> Result<(), CheckError> {
    if h.is_empty() || h.len() > MAX_HANDLE_BYTES {
        return Err(CheckError::Params("every handle must be 1 to 4096 bytes"));
    }
    Ok(())
}

/// Reads at most [`MAX_FILE_BYTES`] + 1 bytes of a trust file through the opener.
fn read_file(ctx: &dyn PackageOpener, h: &str, what: &str) -> Result<Vec<u8>, CheckError> {
    let io = |m: String| CheckError::Trust(TrootError { reason: "io", detail: format!("{what}: {m}") });
    let src = ctx.open_package(h).map_err(|e| io(e.to_string()))?;
    let limit = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    let mut buf = Vec::new();
    src.take(limit).read_to_end(&mut buf).map_err(|e| io(e.to_string()))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(CheckError::Trust(TrootError {
            reason: "too_large",
            detail: format!("{what}: larger than 16 MiB"),
        }));
    }
    Ok(buf)
}

#[fid_command(
    id = "verify.check_signature",
    major = 1,
    title = "Check signatures",
    description = "Checks every DSSE signature of a Result Package against its statement and offline trust-root, revocation and keyring files, and reports one status per signature (VALID_TRUSTED, VALID_UNTRUSTED, REVOKED, EXPIRED, INVALID or UNKNOWN_KEY) with the key fingerprint; time rules use the signer-asserted signing time, never the local clock.",
    side_effect = R1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = ReadRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-21"),
    web = Unavailable,
    web_note = "Packages and trust files are opened from the local file system, which the web build cannot reach.",
)]
pub fn check_signature(ctx: &dyn PackageOpener, params: CheckParams) -> Result<SignatureReport, CheckError> {
    handle(&params.package)?;
    handle(&params.trust_root)?;
    for h in [&params.revoked, &params.keys].into_iter().flatten() {
        handle(h)?;
    }
    let as_of = match &params.as_of {
        Some(t) => Some(UtcTime::parse(t).ok_or(CheckError::Params("as_of must be an RFC 3339 UTC time"))?),
        None => None,
    };
    let trust_root = TrustRoot::parse(&read_file(ctx, &params.trust_root, "trust root")?).map_err(CheckError::Trust)?;
    let revocations = match &params.revoked {
        Some(h) => Revocations::parse(&read_file(ctx, h, "revocations")?).map_err(CheckError::Trust)?,
        None => Revocations::default(),
    };
    let keyring = match &params.keys {
        Some(h) => Keyring::parse(&read_file(ctx, h, "keyring")?).map_err(CheckError::Trust)?,
        None => Keyring::default(),
    };
    let inputs =
        TrustInputs { trust_root: &trust_root, revocations: &revocations, keyring: &keyring, as_of: as_of.as_ref() };
    let src = ctx.open_package(&params.package).map_err(CheckError::Package)?;
    check_package(src, &inputs)
}

fid_commands![check_signature];

// ------------------------------------------------------------------ surfaces

fn error_bytes(e: &CheckError) -> Vec<u8> {
    error_value(e.code(), e.reason(), &e.to_string())
}

fn error_value(code: &str, reason: Option<&str>, message: &str) -> Vec<u8> {
    let mut e = BTreeMap::new();
    e.insert("code".to_owned(), text(code));
    if let Some(r) = reason {
        e.insert("reason".to_owned(), text(r));
    }
    e.insert("message".to_owned(), text(message));
    let mut o = BTreeMap::new();
    o.insert("error".to_owned(), CValue::Obj(e));
    to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
}

/// What the CLI prints and its exit code: 0 when the report was produced (whatever the
/// statuses; composing a verdict is TRUST.VERIFY.REPORT's job), 1 when the command fails,
/// 2 for a usage error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn usage(msg: &str) -> CliOutput {
    let text = format!(
        "error: {msg}\nusage: fid verify {CLI_CHECK} <package> --trust-root <file> [--revoked <file>] [--keys <file>] [--as-of <time>] [--json]\n"
    );
    CliOutput { code: 2, stdout: Vec::new(), stderr: text.into_bytes() }
}

/// `fid verify <args>`: `args` are the words after `verify`.
pub fn invoke_cli(opener: &dyn PackageOpener, args: &[&str]) -> CliOutput {
    let Some((&sub, rest)) = args.split_first() else { return usage("missing subcommand") };
    if sub != CLI_CHECK {
        return usage("unknown subcommand");
    }
    let mut json = false;
    let mut package: Option<&str> = None;
    let (mut trust_root, mut revoked, mut keys, mut as_of): (Option<&str>, Option<&str>, Option<&str>, Option<&str>) =
        (None, None, None, None);
    let mut it = rest.iter();
    while let Some(&a) = it.next() {
        let slot = match a {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--trust-root" => &mut trust_root,
            "--revoked" => &mut revoked,
            "--keys" => &mut keys,
            "--as-of" => &mut as_of,
            _ if a.starts_with("--") => return usage("unknown or repeated option"),
            _ if package.is_none() => {
                package = Some(a);
                continue;
            }
            _ => return usage("more than one package"),
        };
        if slot.is_some() {
            return usage("repeated option");
        }
        match it.next() {
            Some(&v) if !v.starts_with("--") => *slot = Some(v),
            _ => return usage("an option is missing its file"),
        }
    }
    let Some(package) = package else { return usage("missing package") };
    let Some(trust_root) = trust_root else { return usage("missing --trust-root") };
    let params = CheckParams {
        package: package.to_owned(),
        trust_root: trust_root.to_owned(),
        revoked: revoked.map(str::to_owned),
        keys: keys.map(str::to_owned),
        as_of: as_of.map(str::to_owned),
    };
    match check_signature(opener, params) {
        Ok(report) => {
            let stdout = if json {
                let mut b = report.to_canonical_bytes();
                b.push(b'\n');
                b
            } else {
                let mut s = format!("package {}\n", report.package_status.as_str());
                for e in &report.signatures {
                    s.push_str(&format!(
                        "{} {} {} {}{}\n",
                        e.index,
                        e.status.as_str(),
                        e.fingerprint.as_deref().unwrap_or("-"),
                        e.envelope,
                        e.detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default(),
                    ));
                    for n in &e.notes {
                        s.push_str(&format!(
                            "  note: {n}
"
                        ));
                    }
                    if let Some(t) = &e.claimed_signed_at {
                        s.push_str(&format!("  signing time claimed by the signer: {t}\n"));
                    }
                }
                s.into_bytes()
            };
            CliOutput { code: 0, stdout, stderr: Vec::new() }
        }
        Err(e) if json => {
            let mut b = error_bytes(&e);
            b.push(b'\n');
            CliOutput { code: 1, stdout: b, stderr: Vec::new() }
        }
        Err(e) => CliOutput { code: 1, stdout: Vec::new(), stderr: format!("error: {e}\n").into_bytes() },
    }
}

/// The MCP tool: `Ok` holds the canonical result bytes (equal to the CLI's `--json` output
/// without its newline), `Err` a canonical `{"error": {...}}` object.
pub fn invoke_mcp(opener: &dyn PackageOpener, tool: &str, params: &[u8]) -> Result<Vec<u8>, Vec<u8>> {
    if tool != MCP_CHECK {
        return Err(error_value("mcp.unknown_tool", None, "unknown tool"));
    }
    let bad = |m: &str| error_value("params.invalid", None, m);
    if params.len() > MAX_PARAMS_BYTES {
        return Err(bad("parameters too large"));
    }
    let v = fid_canon::parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|e| bad(&format!("parameters are not JSON: {}", e.code())))?;
    let CValue::Obj(o) = v else { return Err(bad("parameters must be an object")) };
    if o.keys().any(|k| !matches!(k.as_str(), "package" | "trust_root" | "revoked" | "keys" | "as_of")) {
        return Err(bad("unknown parameter"));
    }
    let get = |k: &str| -> Result<Option<String>, Vec<u8>> {
        match o.get(k) {
            None => Ok(None),
            Some(CValue::Str(s)) if !s.is_empty() && s.len() <= MAX_HANDLE_BYTES => Ok(Some(s.clone())),
            Some(_) => Err(bad(&format!("{k} must be a string of 1 to 4096 bytes"))),
        }
    };
    let package = get("package")?.ok_or_else(|| bad("package is required"))?;
    let trust_root = get("trust_root")?.ok_or_else(|| bad("trust_root is required"))?;
    let params =
        CheckParams { package, trust_root, revoked: get("revoked")?, keys: get("keys")?, as_of: get("as_of")? };
    check_signature(opener, params).map(|r| r.to_canonical_bytes()).map_err(|e| error_bytes(&e))
}
