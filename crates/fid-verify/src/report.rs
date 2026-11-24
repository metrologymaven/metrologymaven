//! Verification report schema, verdict, exit codes, and human summary
//! (spec node `TRUST.VERIFY.REPORT`).
//!
//! Pure composition over section outcomes (`package`, `signatures`, `chain`, `replay`).
//! Deterministic (D0): no wall-clock timestamps inside the report, ordered `BTreeMap` maps,
//! canonical RFC 8785 JSON via `fid-canon`, and stable plain-text summary with zero ANSI codes.

use fid_canon::{to_canonical_bytes, CValue};
use fid_chain::links::{
    ChainReport, ChainReportView, ChainVerdict, CheckpointReportView, CheckpointSigView, FindingView, LinkResultView,
};
use fid_pkg::reader::PackageInventory;
use fid_registry::FidSchema;
use fid_sign_core::troot::{PackageStatus, SignatureEntry, SignatureReport, SignatureStatus};
use std::collections::BTreeMap;

/// Semantic version of the report schema (`schema/verify-report-1.json`).
pub const REPORT_VERSION: &str = "1.0.0";
/// Tool identifier emitted in every report and `--version --json`.
pub const TOOL_NAME: &str = "fid-verify";
/// Tool semantic version from Cargo package metadata.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Supported Result Package format major versions.
pub const SUPPORTED_PACKAGE_FORMATS: &[u32] = &[1];

/// Exit code when every checked section passes.
pub const EXIT_PASS: u8 = 0;
/// Exit code bitmask flag: signature verification failed or package unsigned.
pub const FAIL_BIT_SIG: u8 = 1;
/// Exit code bitmask flag: chain walk, checkpoint, or ResultRecord linkage failed.
pub const FAIL_BIT_CHAIN: u8 = 2;
/// Exit code bitmask flag: replay classified one or more results `DIVERGENT`.
pub const FAIL_BIT_REPLAY_DIVERGENT: u8 = 4;
/// Exit code bitmask flag: `--replay` requested and replay could not run (`NOT_REPLAYABLE`).
pub const FAIL_BIT_REPLAY_NOT_REPLAYABLE: u8 = 8;
/// Exit code for malformed, truncated, or limit-exceeded inputs (`pkg.*`, `troot.malformed`, `sig.malformed`, `canon.*`, `rep1.malformed`).
pub const EXIT_MALFORMED: u8 = 64;
/// Exit code for CLI usage or invalid parameter errors (`params.invalid`).
pub const EXIT_USAGE: u8 = 65;
/// Exit code for internal or I/O errors (`pkg.io`, etc.).
pub const EXIT_INTERNAL: u8 = 70;

/// Known limitation message when any signature is present (signer-asserted timestamp).
pub const LIMITATION_SIGNER_TIME: &str = "signing time is signer-asserted";
/// Known limitation message when the package carries no signatures.
pub const LIMITATION_UNSIGNED: &str = "unsigned package";

/// Tool identification block.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

impl ToolInfo {
    pub fn current() -> Self {
        Self { name: TOOL_NAME.to_string(), version: TOOL_VERSION.to_string() }
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("name".to_string(), text(&self.name));
        o.insert("version".to_string(), text(&self.version));
        CValue::Obj(o)
    }
}

/// Overall verification verdict.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    #[fid(rename = "PASS")]
    Pass,
    #[fid(rename = "FAIL")]
    Fail,
    #[fid(rename = "ERROR")]
    Error,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::Error => "ERROR",
        }
    }
}

/// Execution status of a verification section (`signatures` or `chain`).
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStatus {
    #[fid(rename = "checked")]
    Checked,
    #[fid(rename = "skipped")]
    Skipped,
    #[fid(rename = "not_available")]
    NotAvailable,
}

impl SectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SectionStatus::Checked => "checked",
            SectionStatus::Skipped => "skipped",
            SectionStatus::NotAvailable => "not_available",
        }
    }
}

/// Composed signature verification section.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SignaturesSection {
    pub status: SectionStatus,
    pub package_status: Option<PackageStatus>,
    #[fid(pattern = "^[0-9a-f]{64}$")]
    pub statement_sha256: Option<String>,
    pub signatures: Option<Vec<SignatureEntry>>,
}

impl SignaturesSection {
    pub fn from_report(report: &SignatureReport) -> Self {
        Self {
            status: SectionStatus::Checked,
            package_status: Some(report.package_status),
            statement_sha256: report.statement_sha256.clone(),
            signatures: Some(report.signatures.clone()),
        }
    }

    pub fn skipped() -> Self {
        Self { status: SectionStatus::Skipped, package_status: None, statement_sha256: None, signatures: None }
    }

    pub fn all_trusted(&self) -> bool {
        match (&self.status, &self.signatures) {
            (SectionStatus::Checked, Some(sigs)) => {
                !sigs.is_empty() && sigs.iter().all(|s| s.status == SignatureStatus::ValidTrusted)
            }
            _ => false,
        }
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("status".to_string(), text(self.status.as_str()));
        if let Some(ps) = self.package_status {
            o.insert("package_status".to_string(), text(ps.as_str()));
        }
        if let Some(sha) = &self.statement_sha256 {
            o.insert("statement_sha256".to_string(), text(sha));
        }
        if let Some(sigs) = &self.signatures {
            let arr = sigs
                .iter()
                .map(|s| {
                    let mut entry = BTreeMap::new();
                    entry.insert("index".to_string(), CValue::Num(f64::from(s.index)));
                    entry.insert("envelope".to_string(), text(&s.envelope));
                    entry.insert("claimed_keyid".to_string(), text(&s.claimed_keyid));
                    entry.insert("status".to_string(), text(s.status.as_str()));
                    let optional = [
                        ("fingerprint", &s.fingerprint),
                        ("key_label", &s.key_label),
                        ("detail", &s.detail),
                        ("claimed_signed_at", &s.claimed_signed_at),
                        ("claimed_meaning_code", &s.claimed_meaning_code),
                    ];
                    for (k, v) in optional {
                        if let Some(val) = v {
                            entry.insert(k.to_string(), text(val));
                        }
                    }
                    entry.insert("notes".to_string(), CValue::Arr(s.notes.iter().map(|n| text(n)).collect()));
                    CValue::Obj(entry)
                })
                .collect();
            o.insert("signatures".to_string(), CValue::Arr(arr));
        }
        CValue::Obj(o)
    }
}

/// Composed journal chain, checkpoint, and ResultRecord linkage section.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ChainSection {
    pub status: SectionStatus,
    pub verdict: Option<ChainVerdict>,
    pub entries_verified: Option<u32>,
    pub findings: Option<Vec<FindingView>>,
    pub checkpoints: Option<Vec<CheckpointReportView>>,
    pub links: Option<Vec<LinkResultView>>,
    pub unlinked: Option<Vec<String>>,
    pub mislinked: Option<Vec<String>>,
}

impl ChainSection {
    pub fn from_report(report: &ChainReport) -> Self {
        let view = ChainReportView::from_report(report);
        Self {
            status: SectionStatus::Checked,
            verdict: Some(view.verdict),
            entries_verified: Some(view.entries_verified),
            findings: Some(view.findings),
            checkpoints: Some(view.checkpoints),
            links: Some(view.links),
            unlinked: Some(view.unlinked),
            mislinked: Some(view.mislinked),
        }
    }

    pub fn skipped() -> Self {
        Self {
            status: SectionStatus::Skipped,
            verdict: None,
            entries_verified: None,
            findings: None,
            checkpoints: None,
            links: None,
            unlinked: None,
            mislinked: None,
        }
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("status".to_string(), text(self.status.as_str()));
        if let Some(v) = self.verdict {
            o.insert("verdict".to_string(), text(v.as_str()));
        }
        if let Some(n) = self.entries_verified {
            o.insert("entries_verified".to_string(), CValue::Num(f64::from(n)));
        }
        if let Some(findings) = &self.findings {
            o.insert("findings".to_string(), CValue::Arr(findings.iter().map(finding_to_value).collect()));
        }
        if let Some(ckpts) = &self.checkpoints {
            o.insert("checkpoints".to_string(), CValue::Arr(ckpts.iter().map(checkpoint_to_value).collect()));
        }
        if let Some(links) = &self.links {
            o.insert("links".to_string(), CValue::Arr(links.iter().map(link_to_value).collect()));
        }
        if let Some(unlinked) = &self.unlinked {
            o.insert("unlinked".to_string(), CValue::Arr(unlinked.iter().map(|s| text(s)).collect()));
        }
        if let Some(mislinked) = &self.mislinked {
            o.insert("mislinked".to_string(), CValue::Arr(mislinked.iter().map(|s| text(s)).collect()));
        }
        CValue::Obj(o)
    }
}

/// Replay outcome/status wire enum.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStatus {
    #[fid(rename = "not_requested")]
    NotRequested,
    #[fid(rename = "not_applicable")]
    NotApplicable,
    #[fid(rename = "skipped")]
    Skipped,
    #[fid(rename = "not_available")]
    NotAvailable,
    #[fid(rename = "IDENTICAL")]
    Identical,
    #[fid(rename = "EQUIVALENT")]
    Equivalent,
    #[fid(rename = "DIVERGENT")]
    Divergent,
    #[fid(rename = "NOT_REPLAYABLE")]
    NotReplayable,
}

impl ReplayStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ReplayStatus::NotRequested => "not_requested",
            ReplayStatus::NotApplicable => "not_applicable",
            ReplayStatus::Skipped => "skipped",
            ReplayStatus::NotAvailable => "not_available",
            ReplayStatus::Identical => "IDENTICAL",
            ReplayStatus::Equivalent => "EQUIVALENT",
            ReplayStatus::Divergent => "DIVERGENT",
            ReplayStatus::NotReplayable => "NOT_REPLAYABLE",
        }
    }
}

/// Kernel identification recorded during replay selection/lookup.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ReplayKernelInfo {
    pub manifest_digest: String,
    pub artifact_digest: Option<String>,
    pub origin: Option<String>,
}

impl ReplayKernelInfo {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("manifest_digest".to_string(), text(&self.manifest_digest));
        if let Some(d) = &self.artifact_digest {
            o.insert("artifact_digest".to_string(), text(d));
        }
        if let Some(origin) = &self.origin {
            o.insert("origin".to_string(), text(origin));
        }
        CValue::Obj(o)
    }
}

/// Per-result replay entry.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ReplayResultEntry {
    pub record_id: String,
    pub outcome: String,
    pub first_diff: Option<String>,
    pub reason: Option<String>,
    pub detail: Option<String>,
}

impl ReplayResultEntry {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("record_id".to_string(), text(&self.record_id));
        o.insert("outcome".to_string(), text(&self.outcome));
        if let Some(fd) = &self.first_diff {
            o.insert("first_diff".to_string(), text(fd));
        }
        if let Some(r) = &self.reason {
            o.insert("reason".to_string(), text(r));
        }
        if let Some(d) = &self.detail {
            o.insert("detail".to_string(), text(d));
        }
        CValue::Obj(o)
    }
}

/// Replay verification section.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ReplaySection {
    pub status: ReplayStatus,
    pub reason: Option<String>,
    pub detail: Option<String>,
    pub kernel: Option<ReplayKernelInfo>,
    pub rep1_version: Option<String>,
    pub rep1_digest: Option<String>,
    pub rep1_production: Option<bool>,
    pub results: Option<Vec<ReplayResultEntry>>,
}

impl ReplaySection {
    pub fn not_requested() -> Self {
        Self {
            status: ReplayStatus::NotRequested,
            reason: None,
            detail: None,
            kernel: None,
            rep1_version: None,
            rep1_digest: None,
            rep1_production: None,
            results: None,
        }
    }

    pub fn not_applicable(detail: impl Into<String>) -> Self {
        Self {
            status: ReplayStatus::NotApplicable,
            reason: Some("RESULTS_TIER".to_string()),
            detail: Some(detail.into()),
            kernel: None,
            rep1_version: None,
            rep1_digest: None,
            rep1_production: None,
            results: None,
        }
    }

    pub fn skipped() -> Self {
        Self {
            status: ReplayStatus::Skipped,
            reason: None,
            detail: None,
            kernel: None,
            rep1_version: None,
            rep1_digest: None,
            rep1_production: None,
            results: None,
        }
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("status".to_string(), text(self.status.as_str()));
        if let Some(r) = &self.reason {
            o.insert("reason".to_string(), text(r));
        }
        if let Some(d) = &self.detail {
            o.insert("detail".to_string(), text(d));
        }
        if let Some(k) = &self.kernel {
            o.insert("kernel".to_string(), k.to_value());
        }
        if let Some(v) = &self.rep1_version {
            o.insert("rep1_version".to_string(), text(v));
        }
        if let Some(d) = &self.rep1_digest {
            o.insert("rep1_digest".to_string(), text(d));
        }
        if let Some(p) = self.rep1_production {
            o.insert("rep1_production".to_string(), CValue::Bool(p));
        }
        if let Some(results) = &self.results {
            o.insert("results".to_string(), CValue::Arr(results.iter().map(ReplayResultEntry::to_value).collect()));
        }
        CValue::Obj(o)
    }
}

/// Inputs passed to [`VerificationReport::compose`].
#[derive(Debug, Clone)]
pub struct SectionInputs<'a> {
    pub package: PackageInventory,
    pub sig_report: Option<&'a SignatureReport>,
    pub signatures_skipped: bool,
    pub chain_report: Option<&'a ChainReport>,
    pub chain_skipped: bool,
    pub replay: ReplaySection,
}

/// Complete verification report (`schema/verify-report-1.json`, `report_version: "1.0.0"`).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
#[fid(description = "Composed offline verification report for a Metrology Maven Result Package.")]
pub struct VerificationReport {
    #[fid(pattern = r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")]
    pub report_version: String,
    pub tool: ToolInfo,
    pub package: PackageInventory,
    pub signatures: SignaturesSection,
    pub chain: ChainSection,
    pub replay: ReplaySection,
    pub verdict: Verdict,
    pub exit_code: u8,
    pub limitations: Vec<String>,
}

impl VerificationReport {
    /// Composes section results into a [`VerificationReport`] with deterministic verdict,
    /// bitmask exit code (`0` or `1..=15`), and known limitations.
    pub fn compose(inputs: SectionInputs<'_>) -> Self {
        let mut exit_code: u8 = EXIT_PASS;
        let mut limitations = Vec::new();

        let signatures = match inputs.sig_report {
            Some(sr) => {
                if !sr.all_trusted() {
                    exit_code |= FAIL_BIT_SIG;
                }
                if !sr.signatures.is_empty() {
                    push_unique(&mut limitations, LIMITATION_SIGNER_TIME);
                }
                if sr.package_status == PackageStatus::Unsigned {
                    push_unique(&mut limitations, LIMITATION_UNSIGNED);
                }
                SignaturesSection::from_report(sr)
            }
            None if inputs.signatures_skipped => SignaturesSection::skipped(),
            None => {
                exit_code |= FAIL_BIT_SIG;
                SignaturesSection {
                    status: SectionStatus::NotAvailable,
                    package_status: None,
                    statement_sha256: None,
                    signatures: None,
                }
            }
        };

        let chain = match inputs.chain_report {
            Some(cr) => {
                if !cr.chain_and_links_ok() {
                    exit_code |= FAIL_BIT_CHAIN;
                }
                // Checkpoint signature trust failures are also blocking (if a checkpoint carries signatures).
                let ckpt_sig_bad = cr.checkpoints.iter().any(|c| {
                    !c.signatures.is_empty() && c.signatures.iter().any(|s| s.status != SignatureStatus::ValidTrusted)
                });
                if ckpt_sig_bad {
                    exit_code |= FAIL_BIT_SIG;
                }
                ChainSection::from_report(cr)
            }
            None if inputs.chain_skipped => ChainSection::skipped(),
            None => {
                exit_code |= FAIL_BIT_CHAIN;
                ChainSection {
                    status: SectionStatus::NotAvailable,
                    verdict: None,
                    entries_verified: None,
                    findings: None,
                    checkpoints: None,
                    links: None,
                    unlinked: None,
                    mislinked: None,
                }
            }
        };

        let replay = inputs.replay;
        match replay.status {
            ReplayStatus::Identical
            | ReplayStatus::Equivalent
            | ReplayStatus::NotRequested
            | ReplayStatus::NotApplicable
            | ReplayStatus::Skipped => {}
            ReplayStatus::Divergent => {
                exit_code |= FAIL_BIT_REPLAY_DIVERGENT;
            }
            ReplayStatus::NotReplayable | ReplayStatus::NotAvailable => {
                exit_code |= FAIL_BIT_REPLAY_NOT_REPLAYABLE;
                if let Some(detail) = &replay.detail {
                    push_unique(&mut limitations, detail);
                } else {
                    push_unique(&mut limitations, "replay not replayable");
                }
            }
        }

        let verdict = if exit_code == EXIT_PASS { Verdict::Pass } else { Verdict::Fail };

        Self {
            report_version: REPORT_VERSION.to_string(),
            tool: ToolInfo::current(),
            package: inputs.package,
            signatures,
            chain,
            replay,
            verdict,
            exit_code,
            limitations,
        }
    }

    /// Converts the report into a canonical JSON [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("report_version".to_string(), text(&self.report_version));
        o.insert("tool".to_string(), self.tool.to_value());
        o.insert("package".to_string(), self.package.to_value());
        o.insert("signatures".to_string(), self.signatures.to_value());
        o.insert("chain".to_string(), self.chain.to_value());
        o.insert("replay".to_string(), self.replay.to_value());
        o.insert("verdict".to_string(), text(self.verdict.as_str()));
        o.insert("exit_code".to_string(), CValue::Num(f64::from(self.exit_code)));
        o.insert("limitations".to_string(), CValue::Arr(self.limitations.iter().map(|s| text(s)).collect()));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Deterministic plain-text human summary with zero ANSI escape sequences.
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{} {} (report_version {})\n", self.tool.name, self.tool.version, self.report_version));
        out.push_str(&format!("verdict: {} (exit_code {})\n", self.verdict.as_str(), self.exit_code));
        out.push_str(&format!(
            "package: format_version={} evidence_tier={} members={} signature_entries={}\n",
            self.package.format_version,
            self.package.evidence_tier.as_str(),
            self.package.members.len(),
            self.package.signature_count
        ));

        match self.signatures.status {
            SectionStatus::Checked => {
                let ps = self.signatures.package_status.map_or("UNKNOWN", PackageStatus::as_str);
                let count = self.signatures.signatures.as_ref().map_or(0, Vec::len);
                let trusted = self
                    .signatures
                    .signatures
                    .as_ref()
                    .map_or(0, |s| s.iter().filter(|e| e.status == SignatureStatus::ValidTrusted).count());
                out.push_str(&format!("signatures: {ps} ({trusted}/{count} VALID_TRUSTED)\n"));
                if let Some(sigs) = &self.signatures.signatures {
                    for s in sigs {
                        out.push_str(&format!(
                            "  [{}] {} {} {}\n",
                            s.index,
                            s.status.as_str(),
                            s.fingerprint.as_deref().unwrap_or("-"),
                            s.envelope
                        ));
                    }
                }
            }
            other => {
                out.push_str(&format!("signatures: {}\n", other.as_str()));
            }
        }

        match self.chain.status {
            SectionStatus::Checked => {
                let cv = self.chain.verdict.map_or("UNKNOWN", ChainVerdict::as_str);
                let entries = self.chain.entries_verified.unwrap_or(0);
                let findings = self.chain.findings.as_ref().map_or(0, Vec::len);
                let ckpts = self.chain.checkpoints.as_ref().map_or(0, Vec::len);
                let unlinked = self.chain.unlinked.as_ref().map_or(0, Vec::len);
                let mislinked = self.chain.mislinked.as_ref().map_or(0, Vec::len);
                out.push_str(&format!(
                    "chain: {cv} (entries_verified={entries} findings={findings} checkpoints={ckpts} unlinked={unlinked} mislinked={mislinked})\n"
                ));
            }
            other => {
                out.push_str(&format!("chain: {}\n", other.as_str()));
            }
        }

        match (&self.replay.reason, &self.replay.detail) {
            (Some(r), Some(d)) => {
                out.push_str(&format!("replay: {} ({r}: {d})\n", self.replay.status.as_str()));
            }
            (Some(r), None) => {
                out.push_str(&format!("replay: {} ({r})\n", self.replay.status.as_str()));
            }
            _ => {
                out.push_str(&format!("replay: {}\n", self.replay.status.as_str()));
            }
        }

        if self.limitations.is_empty() {
            out.push_str("limitations: none\n");
        } else {
            out.push_str("limitations:\n");
            for lim in &self.limitations {
                out.push_str(&format!("  - {lim}\n"));
            }
        }
        out
    }
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn push_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|existing| existing == item) {
        list.push(item.to_string());
    }
}

fn finding_to_value(f: &FindingView) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("index".to_string(), CValue::Num(f64::from(f.index)));
    o.insert("kind".to_string(), text(f.kind.as_str()));
    o.insert("detail".to_string(), text(&f.detail));
    CValue::Obj(o)
}

fn ckpt_sig_to_value(s: &CheckpointSigView) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("claimed_keyid".to_string(), text(&s.claimed_keyid));
    o.insert("status".to_string(), text(s.status.as_str()));
    o.insert("notes".to_string(), CValue::Arr(s.notes.iter().map(|n| text(n)).collect()));
    o.insert("claimed_signed_at".to_string(), text(&s.claimed_signed_at));
    if let Some(fp) = &s.fingerprint {
        o.insert("fingerprint".to_string(), text(fp));
    }
    if let Some(label) = &s.key_label {
        o.insert("key_label".to_string(), text(label));
    }
    if let Some(detail) = &s.detail {
        o.insert("detail".to_string(), text(detail));
    }
    CValue::Obj(o)
}

fn checkpoint_to_value(c: &CheckpointReportView) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("path".to_string(), text(&c.path));
    o.insert("status".to_string(), text(c.status.as_str()));
    o.insert("signatures".to_string(), CValue::Arr(c.signatures.iter().map(ckpt_sig_to_value).collect()));
    if let Some(idx) = &c.index {
        o.insert("index".to_string(), text(idx));
    }
    if let Some(stream) = &c.stream {
        o.insert("stream".to_string(), text(stream));
    }
    if let Some(head) = &c.head_sha256 {
        o.insert("head_sha256".to_string(), text(head));
    }
    if let Some(code) = &c.error_code {
        o.insert("error_code".to_string(), text(code));
    }
    if let Some(detail) = &c.detail {
        o.insert("detail".to_string(), text(detail));
    }
    if let Some(sig) = &c.signature {
        o.insert("signature".to_string(), ckpt_sig_to_value(sig));
    }
    CValue::Obj(o)
}

fn link_to_value(l: &LinkResultView) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("record_id".to_string(), text(&l.record_id));
    o.insert("status".to_string(), text(l.status.as_str()));
    if let Some(idx) = l.entry_index {
        o.insert("entry_index".to_string(), CValue::Num(f64::from(idx)));
    }
    if let Some(reason) = &l.reason {
        o.insert("reason".to_string(), text(reason));
    }
    CValue::Obj(o)
}
