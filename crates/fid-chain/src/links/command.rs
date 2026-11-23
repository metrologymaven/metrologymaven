//! `verify.check_chain@1` (R1, D0; REQ-TRUST.CHAIN.LINKS-007): checks a Result Package's
//! journal hash chain, checkpoints, and ResultRecord linkage, declared with the registry
//! macros, with its CLI entry point
//! (`fid verify check-chain <package> [--trust-root <file>] [--revoked <file>] [--keys <file>] [--as-of <time>] [--json]`)
//! and its MCP tool (`verify_check_chain`).
//!
//! Both surfaces resolve handles through one [`PackageOpener`](fid_pkg::reader::PackageOpener)
//! and call the same handler, so the CLI's `--json` output and the MCP result are byte-identical
//! RFC 8785 canonical JSON encoded strictly via `fid-trust-canon` (sharing no canonicalisation
//! code with `fid-canon`).

use super::{check_package, ChainReport, ChainVerdict, CheckChainError, Finding, FindingKind};
use crate::ckpt::{CheckpointResult, CheckpointSigResult, CheckpointStatus};
use crate::reslink::{LinkResult, LinkStatus};
use fid_pkg::reader::PackageOpener;
use fid_registry::{fid_command, fid_commands, FidSchema};
use fid_sign_core::troot::{
    Keyring, Revocations, SignatureStatus, TrootError, TrustInputs, TrustRoot, UtcTime, MAX_FILE_BYTES,
};
use fid_trust_canon::{parse_with_limits, serialize, Limits, Value};
use std::io::Read;

/// Command ID.
pub const COMMAND_ID: &str = "verify.check_chain@1";
/// CLI group (`fid verify ...`).
pub const CLI_GROUP: &str = "verify";
/// CLI subcommand of `fid verify`.
pub const CLI_CHECK_CHAIN: &str = "check-chain";
/// MCP tool name.
pub const MCP_CHECK_CHAIN: &str = "verify_check_chain";
/// Longest handle, in bytes.
const MAX_HANDLE_BYTES: usize = 4096;
/// Largest MCP parameter payload, in bytes.
const MAX_PARAMS_BYTES: usize = 32 * 1024;
/// Deepest MCP parameter nesting.
const MAX_PARAMS_DEPTH: usize = 4;

// ------------------------------------------------------------------ params and wire views

/// Parameters of `verify.check_chain@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct CheckChainParams {
    #[fid(
        description = "Handle of the Result Package to check (a file path on the CLI, a host-issued handle on MCP).",
        min_length = 1,
        max_length = 4096
    )]
    pub package: String,
    #[fid(
        description = "Optional handle of the offline trust-root file for verifying signed checkpoints.",
        min_length = 1,
        max_length = 4096
    )]
    pub trust_root: Option<String>,
    #[fid(
        description = "Optional handle of the revocation file for verifying signed checkpoints.",
        min_length = 1,
        max_length = 4096
    )]
    pub revoked: Option<String>,
    #[fid(
        description = "Optional handle of a keyring of known untrusted keys for verifying signed checkpoints.",
        min_length = 1,
        max_length = 4096
    )]
    pub keys: Option<String>,
    #[fid(
        description = "Optional reference time supplied by the caller (RFC 3339 UTC; the command never reads a clock).",
        min_length = 20,
        max_length = 30
    )]
    pub as_of: Option<String>,
}

/// Wire enum for checkpoint head-match verdict.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointStatusView {
    #[fid(rename = "MATCH")]
    Match,
    #[fid(rename = "MISMATCH")]
    Mismatch,
    #[fid(rename = "OUT_OF_RANGE")]
    OutOfRange,
    #[fid(rename = "MALFORMED")]
    Malformed,
}

impl CheckpointStatusView {
    fn from_status(s: CheckpointStatus) -> Self {
        match s {
            CheckpointStatus::Match => CheckpointStatusView::Match,
            CheckpointStatus::Mismatch => CheckpointStatusView::Mismatch,
            CheckpointStatus::OutOfRange => CheckpointStatusView::OutOfRange,
            CheckpointStatus::Malformed => CheckpointStatusView::Malformed,
        }
    }

    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            CheckpointStatusView::Match => "MATCH",
            CheckpointStatusView::Mismatch => "MISMATCH",
            CheckpointStatusView::OutOfRange => "OUT_OF_RANGE",
            CheckpointStatusView::Malformed => "MALFORMED",
        }
    }
}

/// Wire enum for ResultRecord linkage verdict.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatusView {
    #[fid(rename = "LINKED")]
    Linked,
    #[fid(rename = "UNLINKED")]
    Unlinked,
    #[fid(rename = "MISLINKED")]
    Mislinked,
}

impl LinkStatusView {
    fn from_status(s: LinkStatus) -> Self {
        match s {
            LinkStatus::Linked => LinkStatusView::Linked,
            LinkStatus::Unlinked => LinkStatusView::Unlinked,
            LinkStatus::Mislinked => LinkStatusView::Mislinked,
        }
    }

    /// Wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkStatusView::Linked => "LINKED",
            LinkStatusView::Unlinked => "UNLINKED",
            LinkStatusView::Mislinked => "MISLINKED",
        }
    }
}

/// Wire representation of one chain-walk finding.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct FindingView {
    #[fid(description = "Zero-based index of the failing entry in journal/segment.jsonl.")]
    pub index: u32,
    pub kind: FindingKind,
    pub detail: String,
}

impl FindingView {
    fn from_finding(f: &Finding) -> Self {
        FindingView { index: u32::try_from(f.index).unwrap_or(u32::MAX), kind: f.kind, detail: f.detail.clone() }
    }

    fn to_value(&self) -> Value {
        Value::Object(vec![
            ("index".to_owned(), Value::Number(f64::from(self.index))),
            ("kind".to_owned(), Value::String(self.kind.as_str().to_owned())),
            ("detail".to_owned(), Value::String(self.detail.clone())),
        ])
    }
}

/// Wire representation of one signature on a checkpoint.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSigView {
    pub claimed_keyid: String,
    pub fingerprint: Option<String>,
    pub key_label: Option<String>,
    pub status: SignatureStatus,
    pub detail: Option<String>,
    pub notes: Vec<String>,
    pub claimed_signed_at: String,
}

impl CheckpointSigView {
    fn from_sig(s: &CheckpointSigResult) -> Self {
        CheckpointSigView {
            claimed_keyid: s.claimed_keyid.clone(),
            fingerprint: s.fingerprint.clone(),
            key_label: s.key_label.clone(),
            status: s.status,
            detail: s.detail.clone(),
            notes: s.notes.clone(),
            claimed_signed_at: s.claimed_signed_at.clone(),
        }
    }

    fn to_value(&self) -> Value {
        let mut fields = vec![
            ("claimed_keyid".to_owned(), Value::String(self.claimed_keyid.clone())),
            ("status".to_owned(), Value::String(self.status.as_str().to_owned())),
            ("notes".to_owned(), Value::Array(self.notes.iter().map(|n| Value::String(n.clone())).collect())),
            ("claimed_signed_at".to_owned(), Value::String(self.claimed_signed_at.clone())),
        ];
        if let Some(fp) = &self.fingerprint {
            fields.push(("fingerprint".to_owned(), Value::String(fp.clone())));
        }
        if let Some(label) = &self.key_label {
            fields.push(("key_label".to_owned(), Value::String(label.clone())));
        }
        if let Some(detail) = &self.detail {
            fields.push(("detail".to_owned(), Value::String(detail.clone())));
        }
        Value::Object(fields)
    }
}

/// Wire representation of one checkpoint verification outcome.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct CheckpointReportView {
    pub path: String,
    #[fid(
        description = "1-based journal sequence index of the checkpoint as a decimal string, if parsed.",
        pattern = "^(0|[1-9][0-9]*)$"
    )]
    pub index: Option<String>,
    pub stream: Option<String>,
    #[fid(pattern = "^sha256:[0-9a-f]{64}$")]
    pub head_sha256: Option<String>,
    pub status: CheckpointStatusView,
    pub error_code: Option<String>,
    pub detail: Option<String>,
    pub signature: Option<CheckpointSigView>,
    pub signatures: Vec<CheckpointSigView>,
}

impl CheckpointReportView {
    fn from_ckpt(c: &CheckpointResult) -> Self {
        CheckpointReportView {
            path: c.path.clone(),
            index: c.index.map(|n| n.to_string()),
            stream: c.stream.clone(),
            head_sha256: c.head_sha256.map(|d| d.to_string()),
            status: CheckpointStatusView::from_status(c.status),
            error_code: c.error_code.map(str::to_owned),
            detail: c.detail.clone(),
            signature: c.signature.as_ref().map(CheckpointSigView::from_sig),
            signatures: c.signatures.iter().map(CheckpointSigView::from_sig).collect(),
        }
    }

    fn to_value(&self) -> Value {
        let mut fields = vec![
            ("path".to_owned(), Value::String(self.path.clone())),
            ("status".to_owned(), Value::String(self.status.as_str().to_owned())),
            ("signatures".to_owned(), Value::Array(self.signatures.iter().map(CheckpointSigView::to_value).collect())),
        ];
        if let Some(idx) = &self.index {
            fields.push(("index".to_owned(), Value::String(idx.clone())));
        }
        if let Some(stream) = &self.stream {
            fields.push(("stream".to_owned(), Value::String(stream.clone())));
        }
        if let Some(head) = &self.head_sha256 {
            fields.push(("head_sha256".to_owned(), Value::String(head.clone())));
        }
        if let Some(code) = &self.error_code {
            fields.push(("error_code".to_owned(), Value::String(code.clone())));
        }
        if let Some(detail) = &self.detail {
            fields.push(("detail".to_owned(), Value::String(detail.clone())));
        }
        if let Some(sig) = &self.signature {
            fields.push(("signature".to_owned(), sig.to_value()));
        }
        Value::Object(fields)
    }
}

/// Wire representation of one ResultRecord linkage outcome.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct LinkResultView {
    pub record_id: String,
    pub status: LinkStatusView,
    pub entry_index: Option<u32>,
    pub reason: Option<String>,
}

impl LinkResultView {
    fn from_link(l: &LinkResult) -> Self {
        LinkResultView {
            record_id: l.record_id.clone(),
            status: LinkStatusView::from_status(l.status),
            entry_index: l.entry_index.map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
            reason: l.reason.map(|r| r.as_str().to_owned()),
        }
    }

    fn to_value(&self) -> Value {
        let mut fields = vec![
            ("record_id".to_owned(), Value::String(self.record_id.clone())),
            ("status".to_owned(), Value::String(self.status.as_str().to_owned())),
        ];
        if let Some(idx) = self.entry_index {
            fields.push(("entry_index".to_owned(), Value::Number(f64::from(idx))));
        }
        if let Some(reason) = &self.reason {
            fields.push(("reason".to_owned(), Value::String(reason.clone())));
        }
        Value::Object(fields)
    }
}

/// Result of `verify.check_chain@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
#[fid(
    description = "Journal chain walk verdict, first-failure findings, checkpoint verification results, and ResultRecord linkage."
)]
pub struct ChainReportView {
    pub verdict: ChainVerdict,
    #[fid(description = "Number of verified journal entries strictly before any failure.")]
    pub entries_verified: u32,
    pub findings: Vec<FindingView>,
    pub checkpoints: Vec<CheckpointReportView>,
    pub links: Vec<LinkResultView>,
    #[fid(description = "IDs of ResultRecords whose journal_entry hash was not found in the verified segment.")]
    pub unlinked: Vec<String>,
    #[fid(description = "IDs of ResultRecords whose linked journal entry did not validly produce the record.")]
    pub mislinked: Vec<String>,
}

impl ChainReportView {
    /// Converts a library [`ChainReport`] into its schema-checked wire view.
    pub fn from_report(r: &ChainReport) -> Self {
        ChainReportView {
            verdict: r.verdict,
            entries_verified: u32::try_from(r.entries_verified).unwrap_or(u32::MAX),
            findings: r.findings.iter().map(FindingView::from_finding).collect(),
            checkpoints: r.checkpoints.iter().map(CheckpointReportView::from_ckpt).collect(),
            links: r.links.iter().map(LinkResultView::from_link).collect(),
            unlinked: r.unlinked.clone(),
            mislinked: r.mislinked.clone(),
        }
    }

    /// Converts to a `fid-trust-canon` [`Value`] omitting absent optional fields.
    pub fn to_value(&self) -> Value {
        Value::Object(vec![
            ("verdict".to_owned(), Value::String(self.verdict.as_str().to_owned())),
            ("entries_verified".to_owned(), Value::Number(f64::from(self.entries_verified))),
            ("findings".to_owned(), Value::Array(self.findings.iter().map(FindingView::to_value).collect())),
            (
                "checkpoints".to_owned(),
                Value::Array(self.checkpoints.iter().map(CheckpointReportView::to_value).collect()),
            ),
            ("links".to_owned(), Value::Array(self.links.iter().map(LinkResultView::to_value).collect())),
            ("unlinked".to_owned(), Value::Array(self.unlinked.iter().map(|s| Value::String(s.clone())).collect())),
            ("mislinked".to_owned(), Value::Array(self.mislinked.iter().map(|s| Value::String(s.clone())).collect())),
        ])
    }

    /// RFC 8785 canonical JSON bytes (no trailing newline), encoded strictly via `fid-trust-canon`.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        // Contains only finite u32 integers and UTF-8 strings with unique keys: cannot fail.
        serialize(&self.to_value()).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ the command

fn check_handle(h: &str) -> Result<(), CheckChainError> {
    if h.is_empty() || h.len() > MAX_HANDLE_BYTES {
        return Err(CheckChainError::Params("every handle must be 1 to 4096 bytes"));
    }
    Ok(())
}

/// Reads at most [`MAX_FILE_BYTES`] + 1 bytes of an optional trust file through the opener.
fn read_trust_file(ctx: &dyn PackageOpener, h: &str, what: &str) -> Result<Vec<u8>, CheckChainError> {
    let io = |m: String| CheckChainError::Trust(TrootError { reason: "io", detail: format!("{what}: {m}") });
    let src = ctx.open_package(h).map_err(|e| io(e.to_string()))?;
    let limit = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    let mut buf = Vec::new();
    src.take(limit).read_to_end(&mut buf).map_err(|e| io(e.to_string()))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(CheckChainError::Trust(TrootError {
            reason: "too_large",
            detail: format!("{what}: larger than 16 MiB"),
        }));
    }
    Ok(buf)
}

/// Runs `verify.check_chain@1` and returns the rich library [`ChainReport`].
pub fn check_chain_report(ctx: &dyn PackageOpener, params: &CheckChainParams) -> Result<ChainReport, CheckChainError> {
    check_handle(&params.package)?;
    for h in [&params.trust_root, &params.revoked, &params.keys].into_iter().flatten() {
        check_handle(h)?;
    }

    let as_of = match &params.as_of {
        Some(t) => Some(UtcTime::parse(t).ok_or(CheckChainError::Params("as_of must be an RFC 3339 UTC time"))?),
        None => None,
    };

    let has_trust = params.trust_root.is_some() || params.revoked.is_some() || params.keys.is_some() || as_of.is_some();

    let trust_root = match &params.trust_root {
        Some(h) => TrustRoot::parse(&read_trust_file(ctx, h, "trust root")?)?,
        None => TrustRoot::default(),
    };
    let revocations = match &params.revoked {
        Some(h) => Revocations::parse(&read_trust_file(ctx, h, "revocations")?)?,
        None => Revocations::default(),
    };
    let keyring = match &params.keys {
        Some(h) => Keyring::parse(&read_trust_file(ctx, h, "keyring")?)?,
        None => Keyring::default(),
    };

    let src = ctx.open_package(&params.package)?;
    if has_trust {
        let inputs = TrustInputs {
            trust_root: &trust_root,
            revocations: &revocations,
            keyring: &keyring,
            as_of: as_of.as_ref(),
        };
        check_package(src, Some(&inputs))
    } else {
        check_package(src, None)
    }
}

#[fid_command(
    id = "verify.check_chain",
    major = 1,
    title = "Check journal chain",
    description = "Walks a Result Package's journal segment entry by entry, recomputes each entry hash independently, checks anchor, predecessor-hash and sequence continuity, verifies packaged checkpoints (and optional checkpoint signatures), and checks ResultRecord-to-journal linkage.",
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
    lifecycle(state = "active", since = "2026-11-22"),
    web = Unavailable,
    web_note = "Packages and trust files are opened from the local file system, which the web build cannot reach.",
)]
pub fn check_chain(ctx: &dyn PackageOpener, params: CheckChainParams) -> Result<ChainReportView, CheckChainError> {
    let report = check_chain_report(ctx, &params)?;
    Ok(report.to_view())
}

fid_commands![check_chain];

// ------------------------------------------------------------------ surfaces

fn error_value(code: &str, reason: Option<&str>, message: &str) -> Vec<u8> {
    let mut e = vec![
        ("code".to_owned(), Value::String(code.to_owned())),
        ("message".to_owned(), Value::String(message.to_owned())),
    ];
    if let Some(r) = reason {
        e.push(("reason".to_owned(), Value::String(r.to_owned())));
    }
    let root = Value::Object(vec![("error".to_owned(), Value::Object(e))]);
    serialize(&root).unwrap_or_default()
}

fn check_error_bytes(e: &CheckChainError) -> Vec<u8> {
    error_value(e.code(), e.reason(), &e.to_string())
}

/// CLI execution output: exit code 0 when the chain is `VALID`, checkpoints match, and all
/// records are `LINKED`; 1 when the chain is `BROKEN`, any record is `UNLINKED`/`MISLINKED`,
/// any checkpoint failed, or an error occurred; 2 for a usage error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn usage(msg: &str) -> CliOutput {
    let text = format!(
        "error: {msg}\nusage: fid {CLI_GROUP} {CLI_CHECK_CHAIN} <package> [--trust-root <file>] [--revoked <file>] [--keys <file>] [--as-of <time>] [--json]\n"
    );
    CliOutput { code: 2, stdout: Vec::new(), stderr: text.into_bytes() }
}

/// `fid verify <args>`: `args` are the words after `verify`.
pub fn invoke_cli(opener: &dyn PackageOpener, args: &[&str]) -> CliOutput {
    let Some((&sub, rest)) = args.split_first() else { return usage("missing subcommand") };
    if sub != CLI_CHECK_CHAIN {
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
            _ => return usage("an option is missing its value"),
        }
    }
    let Some(package) = package else { return usage("missing package") };
    let params = CheckChainParams {
        package: package.to_owned(),
        trust_root: trust_root.map(str::to_owned),
        revoked: revoked.map(str::to_owned),
        keys: keys.map(str::to_owned),
        as_of: as_of.map(str::to_owned),
    };
    match check_chain_report(opener, &params) {
        Ok(report) => {
            let code = if report.chain_and_links_ok() { 0 } else { 1 };
            let stdout = if json {
                let mut b = report.to_canonical_bytes();
                b.push(b'\n');
                b
            } else {
                let mut s = format!(
                    "verdict {}\nentries_verified {}\nfindings {}\ncheckpoints {}\nunlinked {}\nmislinked {}\n",
                    report.verdict.as_str(),
                    report.entries_verified,
                    report.findings.len(),
                    report.checkpoints.len(),
                    report.unlinked.len(),
                    report.mislinked.len(),
                );
                for f in &report.findings {
                    s.push_str(&format!("finding {} {}: {}\n", f.index, f.kind.as_str(), f.detail));
                }
                for c in &report.checkpoints {
                    s.push_str(&format!(
                        "checkpoint {} {}{}\n",
                        c.path,
                        c.status.as_str(),
                        c.signature.as_ref().map(|sig| format!(" ({})", sig.status.as_str())).unwrap_or_default(),
                    ));
                }
                for id in &report.unlinked {
                    s.push_str(&format!("unlinked {id}\n"));
                }
                for id in &report.mislinked {
                    s.push_str(&format!("mislinked {id}\n"));
                }
                s.into_bytes()
            };
            CliOutput { code, stdout, stderr: Vec::new() }
        }
        Err(e) if json => {
            let mut b = check_error_bytes(&e);
            b.push(b'\n');
            CliOutput { code: 1, stdout: b, stderr: Vec::new() }
        }
        Err(e) => CliOutput { code: 1, stdout: Vec::new(), stderr: format!("error: {e}\n").into_bytes() },
    }
}

/// The MCP tool `verify_check_chain`: `Ok` holds the canonical JSON result bytes (equal to the
/// CLI's `--json` stdout without its trailing newline), `Err` holds a canonical
/// `{"error": {...}}` JSON object.
pub fn invoke_mcp(opener: &dyn PackageOpener, tool: &str, params: &[u8]) -> Result<Vec<u8>, Vec<u8>> {
    if tool != MCP_CHECK_CHAIN {
        return Err(error_value("mcp.unknown_tool", None, "unknown tool"));
    }
    let bad = |m: &str| error_value("params.invalid", None, m);
    if params.len() > MAX_PARAMS_BYTES {
        return Err(bad("parameters too large"));
    }
    let v = parse_with_limits(params, Limits { max_bytes: MAX_PARAMS_BYTES, max_depth: MAX_PARAMS_DEPTH })
        .map_err(|e| bad(&format!("parameters are not JSON: {}", e.code())))?;
    let Value::Object(fields) = v else {
        return Err(bad("parameters must be an object"));
    };
    for (k, _) in &fields {
        if !matches!(k.as_str(), "package" | "trust_root" | "revoked" | "keys" | "as_of") {
            return Err(bad("unknown parameter"));
        }
    }
    let get_str = |name: &str| -> Result<Option<String>, Vec<u8>> {
        match fields.iter().find(|(k, _)| k == name).map(|(_, val)| val) {
            None => Ok(None),
            Some(Value::String(s)) if !s.is_empty() && s.len() <= MAX_HANDLE_BYTES => Ok(Some(s.clone())),
            Some(_) => Err(bad(&format!("{name} must be a string of 1 to 4096 bytes"))),
        }
    };
    let package = get_str("package")?.ok_or_else(|| bad("package is required"))?;
    let params = CheckChainParams {
        package,
        trust_root: get_str("trust_root")?,
        revoked: get_str("revoked")?,
        keys: get_str("keys")?,
        as_of: get_str("as_of")?,
    };
    check_chain(opener, params).map(|view| view.to_canonical_bytes()).map_err(|e| check_error_bytes(&e))
}
