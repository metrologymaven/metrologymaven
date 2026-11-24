//! Read-only package verification runner shared by `fid-verify` CLI and `verify.package@1`
//! (spec nodes `TRUST.VERIFY.CLI`, `TRUST.VERIFY.REPORT`).

use crate::report::{
    ReplayKernelInfo, ReplayResultEntry, ReplaySection, ReplayStatus, SectionInputs, VerificationReport, EXIT_INTERNAL,
    EXIT_MALFORMED, EXIT_USAGE,
};
use fid_pkg::manifest::{hash_bytes, MANIFEST_PATH, PREDICATE_TYPE, STATEMENT_PATH, STATEMENT_TYPE};
use fid_pkg::reader::{
    InventoryTier, MemberReport, MemberState, PackageInventory, PackageOpener, PackageReader, ReadError,
    MAX_MANIFEST_BYTES, MAX_MANIFEST_VALUES,
};
use fid_sign_core::troot::{
    check_envelope, Keyring, PackageStatus, Revocations, SignatureReport, SignatureStatus, TrootError, TrustInputs,
    TrustRoot, UtcTime, MAX_FILE_BYTES, MAX_SIGNATURES_PER_PACKAGE, MAX_SIGNATURE_ENTRIES, MAX_VERIFIED_BYTES,
};
use fid_sign_core::{Envelope, SigError, MAX_ENVELOPE_BYTES};
use fid_types::Digest;
use fid_vreplay::compare::{CompareError, Rep1, MAX_TABLE_BYTES};
use fid_vreplay::kart::{
    default_cache_dir, KernelManifest, LookupConfig, OfflineArtifactSource, Origin, DEFAULT_MAX_ARTIFACT_BYTES,
};
use fid_vreplay::select::{self, ArtifactSource, MemberInfo, PackageError, PackageView, SelectError, Selected};
use std::fmt;
use std::io::{Read, Seek};
use std::path::PathBuf;

const MAX_HANDLE_BYTES: usize = 4096;

/// Verification options matching `fid-verify` CLI flags and `verify.package@1` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyOptions {
    pub package: String,
    pub trust_root: Option<String>,
    pub revoked: Option<String>,
    pub keys: Option<String>,
    pub as_of: Option<String>,
    pub replay: bool,
    pub rep1: Option<String>,
    pub kernel_cache: Option<String>,
    pub fail_fast: bool,
    pub memory_limit: Option<String>,
    pub time_budget: Option<String>,
}

/// Classification of fatal verification errors (exit 64, 65, or 70).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyErrorKind {
    /// Exit 64: malformed, truncated, or limit-exceeded package / trust file / REP-1 table.
    Malformed,
    /// Exit 65: CLI usage or parameter validation error (`params.invalid`).
    Usage,
    /// Exit 70: internal or I/O failure (`pkg.io`, etc.).
    Internal,
}

impl VerifyErrorKind {
    pub fn exit_code(self) -> u8 {
        match self {
            VerifyErrorKind::Malformed => EXIT_MALFORMED,
            VerifyErrorKind::Usage => EXIT_USAGE,
            VerifyErrorKind::Internal => EXIT_INTERNAL,
        }
    }
}

/// Typed error returned when verification cannot produce a section report (exits 64, 65, or 70).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub kind: VerifyErrorKind,
    pub code: String,
    pub reason: Option<String>,
    pub message: String,
}

impl VerifyError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self { kind: VerifyErrorKind::Usage, code: "params.invalid".to_string(), reason: None, message: message.into() }
    }

    pub fn malformed(code: impl Into<String>, reason: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            kind: VerifyErrorKind::Malformed,
            code: code.into(),
            reason: reason.map(str::to_string),
            message: message.into(),
        }
    }

    pub fn internal(code: impl Into<String>, reason: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            kind: VerifyErrorKind::Internal,
            code: code.into(),
            reason: reason.map(str::to_string),
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.kind.exit_code()
    }

    /// Canonical JSON `{"error":{...}}` representation.
    pub fn to_canonical_json(&self) -> Vec<u8> {
        let mut e = std::collections::BTreeMap::new();
        e.insert("code".to_string(), fid_canon::CValue::Str(self.code.clone()));
        if let Some(r) = &self.reason {
            e.insert("reason".to_string(), fid_canon::CValue::Str(r.clone()));
        }
        e.insert("message".to_string(), fid_canon::CValue::Str(self.message.clone()));
        let mut root = std::collections::BTreeMap::new();
        root.insert("error".to_string(), fid_canon::CValue::Obj(e));
        fid_canon::to_canonical_bytes(&fid_canon::CValue::Obj(root)).unwrap_or_default()
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            Some(r) => write!(f, "{} ({r}): {}", self.code, self.message),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

impl std::error::Error for VerifyError {}

fn check_handle(label: &str, h: &str) -> Result<(), VerifyError> {
    if h.is_empty() || h.len() > MAX_HANDLE_BYTES {
        return Err(VerifyError::usage(format!("{label} handle must be 1 to {MAX_HANDLE_BYTES} bytes")));
    }
    Ok(())
}

/// Parses `--memory-limit` (`N`, `NK`, `NKB`, `NKiB`, `NM`, `NMB`, `NMiB`, `NG`, `NGB`, `NGiB`).
pub fn parse_memory_limit(raw: &str) -> Result<u64, VerifyError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(VerifyError::usage("memory limit cannot be empty"));
    }
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (digits, suffix) = s.split_at(split);
    if digits.is_empty() {
        return Err(VerifyError::usage(format!("invalid --memory-limit {raw:?}")));
    }
    let base: u64 = digits.parse().map_err(|_| VerifyError::usage(format!("invalid --memory-limit {raw:?}")))?;
    let mult: u64 = match suffix.to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1_000,
        "kib" => 1024,
        "m" | "mb" => 1_000_000,
        "mib" => 1024 * 1024,
        "g" | "gb" => 1_000_000_000,
        "gib" => 1024 * 1024 * 1024,
        _ => return Err(VerifyError::usage(format!("unknown unit in --memory-limit {raw:?}"))),
    };
    let bytes =
        base.checked_mul(mult).ok_or_else(|| VerifyError::usage(format!("--memory-limit overflow: {raw:?}")))?;
    if bytes == 0 {
        return Err(VerifyError::usage("--memory-limit must be greater than zero"));
    }
    Ok(bytes)
}

/// Parses `--time-budget` (`0s`, `0ms`, `500ms`, `10s`, `2m`, or plain integer seconds).
pub fn parse_time_budget_ms(raw: &str) -> Result<u64, VerifyError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(VerifyError::usage("time budget cannot be empty"));
    }
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (digits, suffix) = s.split_at(split);
    if digits.is_empty() {
        return Err(VerifyError::usage(format!("invalid --time-budget {raw:?}")));
    }
    let base: u64 = digits.parse().map_err(|_| VerifyError::usage(format!("invalid --time-budget {raw:?}")))?;
    let mult: u64 = match suffix.to_ascii_lowercase().as_str() {
        "ms" => 1,
        "" | "s" => 1000,
        "m" => 60_000,
        _ => return Err(VerifyError::usage(format!("unknown unit in --time-budget {raw:?}"))),
    };
    base.checked_mul(mult).ok_or_else(|| VerifyError::usage(format!("--time-budget overflow: {raw:?}")))
}

fn map_read_error(e: ReadError) -> VerifyError {
    match e {
        ReadError::Io(detail) => VerifyError::internal("pkg.io", Some("io"), detail),
        other => VerifyError::malformed(other.code(), Some(other.reason()), other.to_string()),
    }
}

fn map_troot_error(e: TrootError) -> VerifyError {
    if e.reason == "io" {
        VerifyError::internal(e.code(), Some(e.reason), e.detail)
    } else {
        VerifyError::malformed(e.code(), Some(e.reason), e.detail)
    }
}

fn read_rep1_bytes(opener: &dyn PackageOpener, handle: &str) -> Result<Vec<u8>, VerifyError> {
    let src =
        opener.open_package(handle).map_err(|e| VerifyError::internal("pkg.io", Some("io"), format!("rep1: {e}")))?;
    let limit = u64::try_from(MAX_TABLE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    let mut buf = Vec::new();
    src.take(limit)
        .read_to_end(&mut buf)
        .map_err(|e| VerifyError::internal("pkg.io", Some("io"), format!("rep1: {e}")))?;
    if buf.len() > MAX_TABLE_BYTES {
        return Err(VerifyError::malformed(
            "rep1.malformed",
            Some("too_large"),
            format!("rep1 exceeds maximum size of {MAX_TABLE_BYTES} bytes"),
        ));
    }
    Ok(buf)
}

fn read_trust_bytes(opener: &dyn PackageOpener, handle: &str, what: &str) -> Result<Vec<u8>, VerifyError> {
    let src = opener
        .open_package(handle)
        .map_err(|e| map_troot_error(TrootError { reason: "io", detail: format!("{what}: {e}") }))?;
    let limit = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    let mut buf = Vec::new();
    src.take(limit)
        .read_to_end(&mut buf)
        .map_err(|e| map_troot_error(TrootError { reason: "io", detail: format!("{what}: {e}") }))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(map_troot_error(TrootError { reason: "too_large", detail: format!("{what}: larger than 16 MiB") }));
    }
    Ok(buf)
}

fn build_inventory<R: Read + Seek>(reader: &mut PackageReader<R>) -> PackageInventory {
    let results = reader.member_results();
    let signature_count = u32::try_from(reader.signature_count()).unwrap_or(u32::MAX);
    let m = reader.manifest();
    let evidence_tier = match m.evidence_tier {
        fid_pkg::manifest::EvidenceTier::Results => InventoryTier::Results,
        fid_pkg::manifest::EvidenceTier::Features => InventoryTier::Features,
        fid_pkg::manifest::EvidenceTier::Points => InventoryTier::Points,
    };
    let members = m
        .members
        .iter()
        .zip(&results)
        .map(|(member, r)| {
            let status = match r {
                Ok(()) => MemberState::Ok,
                Err(ReadError::DigestMismatch { .. }) => MemberState::DigestMismatch,
                Err(ReadError::Limit { .. }) => MemberState::Limit,
                Err(ReadError::Io(_)) => MemberState::Io,
                Err(_) => MemberState::Malformed,
            };
            MemberReport {
                path: member.path.as_str().to_string(),
                media_type: member.media_type.clone(),
                length: member.length.to_string(),
                sha256: member.sha256.hex().to_string(),
                status,
            }
        })
        .collect();
    PackageInventory {
        format_version: m.format_version.to_string(),
        evidence_tier,
        profile_digest: m.profile_digest.as_str().to_string(),
        members,
        signature_count,
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn stmt_malformed(detail: impl Into<String>) -> VerifyError {
    VerifyError::malformed("sig.malformed", Some("statement_malformed"), detail)
}

/// Parses `statement.json` with bounded limits and verifies strict in-toto Statement v1 binding
/// to `manifest.json` (`predicate.manifest_sha256` and every `subject` member digest).
///
/// Returns `Ok(None)` when `statement.json` is absent or binds cleanly, `Ok(Some(reason))` when
/// `statement.json` is syntactically valid but mismatches `manifest.json` / members, and
/// `Err(VerifyError::malformed)` (exit 64) when `statement.json` itself is malformed.
fn check_statement_binding<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<Option<String>, VerifyError> {
    let Some(stmt_bytes) = reader.read_member(STATEMENT_PATH, MAX_MANIFEST_BYTES).map_err(map_read_error)? else {
        return Ok(None);
    };
    let max_bytes = usize::try_from(MAX_MANIFEST_BYTES).unwrap_or(usize::MAX);
    let max_elements = usize::try_from(MAX_MANIFEST_VALUES).unwrap_or(usize::MAX);
    let root = fid_canon::parse_with_budget(&stmt_bytes, max_bytes, 16, max_elements)
        .map_err(|e| stmt_malformed(format!("statement.json: {e}")))?;
    let fid_canon::CValue::Obj(obj) = root else {
        return Err(stmt_malformed("statement.json: root must be a JSON object"));
    };

    match obj.get("_type") {
        Some(fid_canon::CValue::Str(s)) if s == STATEMENT_TYPE => {}
        Some(fid_canon::CValue::Str(s)) => {
            return Err(stmt_malformed(format!("statement.json: unexpected _type {s:?}")));
        }
        _ => return Err(stmt_malformed("statement.json: missing or invalid _type")),
    }
    match obj.get("predicateType") {
        Some(fid_canon::CValue::Str(s)) if s == PREDICATE_TYPE => {}
        Some(fid_canon::CValue::Str(s)) => {
            return Err(stmt_malformed(format!("statement.json: unexpected predicateType {s:?}")));
        }
        _ => return Err(stmt_malformed("statement.json: missing or invalid predicateType")),
    }

    let Some(fid_canon::CValue::Obj(pred)) = obj.get("predicate") else {
        return Err(stmt_malformed("statement.json: missing or invalid predicate object"));
    };
    let pred_manifest_sha = match pred.get("manifest_sha256") {
        Some(fid_canon::CValue::Str(s)) if is_hex64(s) => s.clone(),
        _ => return Err(stmt_malformed("statement.json: predicate.manifest_sha256 must be 64 lowercase hex digits")),
    };

    let Some(fid_canon::CValue::Arr(subjects)) = obj.get("subject") else {
        return Err(stmt_malformed("statement.json: missing or invalid subject array"));
    };
    let mut subject_digests = std::collections::BTreeMap::new();
    for (idx, item) in subjects.iter().enumerate() {
        let fid_canon::CValue::Obj(sub) = item else {
            return Err(stmt_malformed(format!("statement.json: subject[{idx}] must be an object")));
        };
        let Some(fid_canon::CValue::Str(name)) = sub.get("name") else {
            return Err(stmt_malformed(format!("statement.json: subject[{idx}].name must be a string")));
        };
        if name.is_empty() {
            return Err(stmt_malformed(format!("statement.json: subject[{idx}].name cannot be empty")));
        }
        let Some(fid_canon::CValue::Obj(dig)) = sub.get("digest") else {
            return Err(stmt_malformed(format!("statement.json: subject[{idx}].digest must be an object")));
        };
        let Some(fid_canon::CValue::Str(sha)) = dig.get("sha256") else {
            return Err(stmt_malformed(format!("statement.json: subject[{idx}].digest.sha256 must be a string")));
        };
        if !is_hex64(sha) {
            return Err(stmt_malformed(format!(
                "statement.json: subject[{idx}].digest.sha256 must be 64 lowercase hex digits"
            )));
        }
        if let Some(prev) = subject_digests.insert(name.clone(), sha.clone()) {
            if prev != *sha {
                return Err(stmt_malformed(format!("statement.json: conflicting subject digest for {name:?}")));
            }
            return Err(stmt_malformed(format!("statement.json: duplicate subject {name:?}")));
        }
    }

    let manifest_hex = hash_bytes(reader.manifest_bytes()).hex().to_string();
    if pred_manifest_sha != manifest_hex {
        return Ok(Some("statement_manifest_mismatch".to_string()));
    }
    if subject_digests.get(MANIFEST_PATH).map(String::as_str) != Some(manifest_hex.as_str()) {
        return Ok(Some("statement_subject_mismatch".to_string()));
    }
    let manifest_members = &reader.manifest().members;
    if subject_digests.len() != manifest_members.len() + 1 {
        return Ok(Some("statement_subject_mismatch".to_string()));
    }
    for m in manifest_members {
        if subject_digests.get(m.path.as_str()).map(String::as_str) != Some(m.sha256.hex()) {
            return Ok(Some("statement_subject_mismatch".to_string()));
        }
    }

    Ok(None)
}

fn check_signatures_on_reader<R: Read + Seek>(
    reader: &mut PackageReader<R>,
    inputs: &TrustInputs<'_>,
) -> Result<SignatureReport, VerifyError> {
    let statement = reader.statement_sha256().map(|d| d.hex().to_owned());
    let binding_mismatch = if reader.has_statement() { check_statement_binding(reader)? } else { None };
    let names: Vec<String> =
        reader.entry_names().into_iter().filter(|n| n.starts_with("signatures/")).map(str::to_owned).collect();
    if names.len() > MAX_SIGNATURE_ENTRIES {
        return Err(VerifyError::malformed("sig.malformed", None, "signatures/: more than 64 signature entries"));
    }
    let package_status = if names.is_empty() { PackageStatus::Unsigned } else { PackageStatus::Signed };
    let cap = u64::try_from(MAX_ENVELOPE_BYTES).unwrap_or(u64::MAX);
    let mut signatures = Vec::new();
    let mut budget = MAX_VERIFIED_BYTES;
    for name in names {
        let bytes = reader.read_member(&name, cap).map_err(map_read_error)?.unwrap_or_default();
        let envelope = Envelope::from_json(&bytes)
            .map_err(|e: SigError| VerifyError::malformed(e.code(), None, format!("{name}: {e}")))?;
        if signatures.len().saturating_add(envelope.signatures.len()) > MAX_SIGNATURES_PER_PACKAGE {
            return Err(VerifyError::malformed(
                "sig.malformed",
                None,
                format!("{name}: more than 64 signatures in the package"),
            ));
        }
        let first = u32::try_from(signatures.len()).unwrap_or(u32::MAX);
        let mut checked =
            check_envelope(&envelope, &name, statement.as_deref(), inputs, first, &mut budget).map_err(|b| {
                VerifyError::malformed(
                    "sig.malformed",
                    Some("verify_budget"),
                    format!("{name}: needs up to {} PAE bytes hashed, {} left", b.needed, b.left),
                )
            })?;
        if let Some(reason) = &binding_mismatch {
            for sig in &mut checked {
                sig.status = SignatureStatus::Invalid;
                sig.detail = Some(reason.clone());
            }
        }
        signatures.extend(checked);
    }
    Ok(SignatureReport { package_status, statement_sha256: statement, signatures })
}

struct ReaderPackageView<'a, R>(&'a mut PackageReader<R>);

impl<R: Read + Seek> PackageView for ReaderPackageView<'_, R> {
    fn evidence_tier(&self) -> String {
        self.0.manifest().evidence_tier.as_str().to_string()
    }

    fn kernel_manifest_digest(&self) -> Digest {
        self.0.manifest().kernel_manifest_digest
    }

    fn members(&self) -> Vec<MemberInfo> {
        self.0
            .manifest()
            .members
            .iter()
            .map(|m| MemberInfo { path: m.path.as_str().to_string(), sha256: m.sha256 })
            .collect()
    }

    fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError> {
        self.0
            .read_member(path, max_bytes)
            .map_err(|e| PackageError { code: e.code().to_string(), detail: e.to_string() })
    }
}

fn run_replay_section<R: Read + Seek>(
    reader: &mut PackageReader<R>,
    inventory: &PackageInventory,
    rep1: &Rep1,
    kernel_cache: Option<&str>,
    memory_limit_bytes: Option<u64>,
    time_budget_ms: Option<u64>,
    chain_broken: bool,
) -> Result<ReplaySection, VerifyError> {
    let _ = memory_limit_bytes;
    if inventory.evidence_tier == InventoryTier::Results {
        return Ok(ReplaySection::not_applicable("results-tier package carries no point sets to replay"));
    }

    let km_bytes =
        reader.read_member(select::KERNEL_MANIFEST_PATH, select::MAX_KERNEL_MANIFEST_BYTES).map_err(map_read_error)?;
    let (parsed_km, km_parse_err) = match km_bytes.as_deref() {
        Some(bytes) => match KernelManifest::parse(bytes) {
            Ok(km) => (Some(km), None),
            Err(e) => (None, Some(e.to_string())),
        },
        None => (None, None),
    };

    let cache_dir = kernel_cache.map(PathBuf::from).or_else(default_cache_dir);
    let offline = OfflineArtifactSource::new(LookupConfig {
        bundled_dir: None,
        cache_dir,
        max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
    });

    let selection = {
        let probe_km = parsed_km.as_ref();
        // Run SELECT with offline+package artifact probe first, without consuming package reader inside closure.
        // We check package member availability via manifest digest & verify bytes via OfflineArtifactSource below.
        let available_in_pkg_or_cache = if let Some(km) = probe_km {
            if let Some(expected) = km.wasm32_wasip1_digest {
                let member = fid_vreplay::kart::package_member_path(&expected);
                reader.manifest().members.iter().any(|m| m.path.as_str() == member && m.sha256 == expected)
                    || offline.get(km, None).is_ok()
            } else {
                false
            }
        } else {
            false
        };
        struct BoolSource {
            digest: Option<Digest>,
            ok: bool,
        }
        impl ArtifactSource for BoolSource {
            fn is_available(&self, d: &Digest) -> bool {
                self.ok && self.digest.as_ref() == Some(d)
            }
        }
        let source = BoolSource { digest: probe_km.map(|k| k.manifest_digest), ok: available_in_pkg_or_cache };
        let kernel_manifest_digest = reader.manifest().kernel_manifest_digest;
        let mut view = ReaderPackageView(reader);
        match select::plan(&mut view, &source) {
            Ok(sel) => sel,
            Err(SelectError::Malformed { member, reason, detail }) if chain_broken => {
                return Ok(ReplaySection {
                    status: ReplayStatus::NotReplayable,
                    reason: Some("JOURNAL_INVALID".to_string()),
                    detail: Some(format!("{member}: {reason}: {detail}")),
                    kernel: Some(ReplayKernelInfo {
                        manifest_digest: kernel_manifest_digest.as_str().to_string(),
                        artifact_digest: parsed_km
                            .as_ref()
                            .and_then(|k| k.wasm32_wasip1_digest.map(|d| d.as_str().to_string())),
                        origin: None,
                    }),
                    rep1_version: Some(rep1.version().to_string()),
                    rep1_digest: Some(rep1.digest().as_str().to_string()),
                    rep1_production: Some(rep1.is_production()),
                    results: None,
                });
            }
            Err(e) => {
                return Err(match e {
                    SelectError::Package(pe) => {
                        if pe.code == "pkg.io" {
                            VerifyError::internal(pe.code, Some("io"), pe.detail)
                        } else {
                            VerifyError::malformed(pe.code, None, pe.detail)
                        }
                    }
                    SelectError::Malformed { reason, .. } => {
                        VerifyError::malformed("pkg.malformed", Some(reason), e.to_string())
                    }
                    SelectError::Limit { what, .. } => VerifyError::malformed("pkg.limit", Some(what), e.to_string()),
                });
            }
        }
    };

    let kernel_info_base = ReplayKernelInfo {
        manifest_digest: selection.kernel_manifest_digest.as_str().to_string(),
        artifact_digest: parsed_km.as_ref().and_then(|k| k.wasm32_wasip1_digest.map(|d| d.as_str().to_string())),
        origin: None,
    };

    if time_budget_ms == Some(0) {
        let results: Vec<ReplayResultEntry> = selection
            .results
            .iter()
            .map(|r| match r {
                Selected::Replayable { record_id, .. } => ReplayResultEntry {
                    record_id: record_id.clone(),
                    outcome: "NOT_REPLAYABLE".to_string(),
                    first_diff: None,
                    reason: Some("BUDGET".to_string()),
                    detail: Some("time budget 0ms exhausted before execution".to_string()),
                },
                Selected::NotReplayable(n) => ReplayResultEntry {
                    record_id: n.record_id.clone(),
                    outcome: "NOT_REPLAYABLE".to_string(),
                    first_diff: None,
                    reason: Some(n.reason.as_str().to_string()),
                    detail: Some(n.detail.clone()),
                },
            })
            .collect();
        return Ok(ReplaySection {
            status: ReplayStatus::NotReplayable,
            reason: Some("BUDGET".to_string()),
            detail: Some("replay stopped: time budget 0ms exhausted".to_string()),
            kernel: Some(kernel_info_base),
            rep1_version: Some(rep1.version().to_string()),
            rep1_digest: Some(rep1.digest().as_str().to_string()),
            rep1_production: Some(rep1.is_production()),
            results: Some(results),
        });
    }

    if selection.plan.calls.is_empty() || selection.results.iter().all(|r| matches!(r, Selected::NotReplayable(_))) {
        let first_nr = selection.not_replayable().next();
        let reason = first_nr.map_or("NO_RESULTS", |n| n.reason.as_str()).to_string();
        let detail =
            first_nr.map_or_else(|| "package contains no replayable results".to_string(), |n| n.detail.clone());
        let results: Vec<ReplayResultEntry> = selection
            .results
            .iter()
            .map(|r| match r {
                Selected::Replayable { record_id, .. } => ReplayResultEntry {
                    record_id: record_id.clone(),
                    outcome: "NOT_REPLAYABLE".to_string(),
                    first_diff: None,
                    reason: Some("NO_CALLS".to_string()),
                    detail: Some("no kernel calls in plan".to_string()),
                },
                Selected::NotReplayable(n) => ReplayResultEntry {
                    record_id: n.record_id.clone(),
                    outcome: "NOT_REPLAYABLE".to_string(),
                    first_diff: None,
                    reason: Some(n.reason.as_str().to_string()),
                    detail: Some(n.detail.clone()),
                },
            })
            .collect();
        return Ok(ReplaySection {
            status: ReplayStatus::NotReplayable,
            reason: Some(reason),
            detail: Some(detail),
            kernel: Some(kernel_info_base),
            rep1_version: Some(rep1.version().to_string()),
            rep1_digest: Some(rep1.digest().as_str().to_string()),
            rep1_production: Some(rep1.is_production()),
            results: Some(results),
        });
    }

    // Check offline kernel artifact lookup chain (`package -> bundled -> cache`).
    let mut view = ReaderPackageView(reader);
    let lookup_res = match &parsed_km {
        Some(km) => offline.get_with_notes(km, Some(&mut view)),
        None => {
            let detail = match &km_parse_err {
                Some(err) => format!("kernel/manifest.json unparseable or unsupported: {err}"),
                None => "kernel/manifest.json missing".to_string(),
            };
            return Ok(ReplaySection {
                status: ReplayStatus::NotReplayable,
                reason: Some("KERNEL_UNAVAILABLE".to_string()),
                detail: Some(detail),
                kernel: Some(kernel_info_base),
                rep1_version: Some(rep1.version().to_string()),
                rep1_digest: Some(rep1.digest().as_str().to_string()),
                rep1_production: Some(rep1.is_production()),
                results: None,
            });
        }
    };

    match lookup_res {
        Err(unavail) => {
            let results: Vec<ReplayResultEntry> = selection
                .results
                .iter()
                .map(|r| match r {
                    Selected::Replayable { record_id, .. } => ReplayResultEntry {
                        record_id: record_id.clone(),
                        outcome: "NOT_REPLAYABLE".to_string(),
                        first_diff: None,
                        reason: Some("KERNEL_UNAVAILABLE".to_string()),
                        detail: Some(unavail.to_string()),
                    },
                    Selected::NotReplayable(n) => ReplayResultEntry {
                        record_id: n.record_id.clone(),
                        outcome: "NOT_REPLAYABLE".to_string(),
                        first_diff: None,
                        reason: Some(n.reason.as_str().to_string()),
                        detail: Some(n.detail.clone()),
                    },
                })
                .collect();
            Ok(ReplaySection {
                status: ReplayStatus::NotReplayable,
                reason: Some(unavail.code.to_string()),
                detail: Some(unavail.to_string()),
                kernel: Some(ReplayKernelInfo {
                    manifest_digest: selection.kernel_manifest_digest.as_str().to_string(),
                    artifact_digest: unavail.digest().map(|d| d.as_str().to_string()),
                    origin: None,
                }),
                rep1_version: Some(rep1.version().to_string()),
                rep1_digest: Some(rep1.digest().as_str().to_string()),
                rep1_production: Some(rep1.is_production()),
                results: Some(results),
            })
        }
        Ok(found) => {
            // Artifact resolved and verified offline, but WASM sandbox execution (`TRUST.REPLAY.SANDBOX` /
            // `TRUST.REPLAY.RUN`) is not yet implemented in `fid-vreplay`. Fail closed honestly.
            let origin_str = match found.origin {
                Origin::Package => "package",
                Origin::Bundled => "bundled",
                Origin::Cache => "cache",
            };
            let detail =
                "WASM replay sandbox (TRUST.REPLAY.SANDBOX / TRUST.REPLAY.RUN) is not yet implemented; failing closed"
                    .to_string();
            let results: Vec<ReplayResultEntry> = selection
                .results
                .iter()
                .map(|r| match r {
                    Selected::Replayable { record_id, .. } => ReplayResultEntry {
                        record_id: record_id.clone(),
                        outcome: "NOT_REPLAYABLE".to_string(),
                        first_diff: None,
                        reason: Some("SANDBOX_UNAVAILABLE".to_string()),
                        detail: Some(detail.clone()),
                    },
                    Selected::NotReplayable(n) => ReplayResultEntry {
                        record_id: n.record_id.clone(),
                        outcome: "NOT_REPLAYABLE".to_string(),
                        first_diff: None,
                        reason: Some(n.reason.as_str().to_string()),
                        detail: Some(n.detail.clone()),
                    },
                })
                .collect();
            Ok(ReplaySection {
                status: ReplayStatus::NotReplayable,
                reason: Some("SANDBOX_UNAVAILABLE".to_string()),
                detail: Some(detail),
                kernel: Some(ReplayKernelInfo {
                    manifest_digest: selection.kernel_manifest_digest.as_str().to_string(),
                    artifact_digest: parsed_km
                        .as_ref()
                        .and_then(|k| k.wasm32_wasip1_digest.map(|d| d.as_str().to_string())),
                    origin: Some(origin_str.to_string()),
                }),
                rep1_version: Some(rep1.version().to_string()),
                rep1_digest: Some(rep1.digest().as_str().to_string()),
                rep1_production: Some(rep1.is_production()),
                results: Some(results),
            })
        }
    }
}

/// Runs the complete read-only package verification pipeline.
pub fn verify_package(opener: &dyn PackageOpener, opts: &VerifyOptions) -> Result<VerificationReport, VerifyError> {
    check_handle("package", &opts.package)?;
    if let Some(h) = &opts.trust_root {
        check_handle("trust_root", h)?;
    }
    if let Some(h) = &opts.revoked {
        check_handle("revoked", h)?;
    }
    if let Some(h) = &opts.keys {
        check_handle("keys", h)?;
    }
    if let Some(h) = &opts.rep1 {
        check_handle("rep1", h)?;
    }
    if let Some(h) = &opts.kernel_cache {
        check_handle("kernel_cache", h)?;
    }

    let as_of = match &opts.as_of {
        Some(t) => Some(UtcTime::parse(t).ok_or_else(|| VerifyError::usage("as_of must be an RFC 3339 UTC time"))?),
        None => None,
    };
    let memory_limit_bytes = opts.memory_limit.as_deref().map(parse_memory_limit).transpose()?;
    let time_budget_ms = opts.time_budget.as_deref().map(parse_time_budget_ms).transpose()?;

    let trust_root = match &opts.trust_root {
        Some(h) => TrustRoot::parse(&read_trust_bytes(opener, h, "trust root")?).map_err(map_troot_error)?,
        None => TrustRoot::default(),
    };
    let revocations = match &opts.revoked {
        Some(h) => Revocations::parse(&read_trust_bytes(opener, h, "revocations")?).map_err(map_troot_error)?,
        None => Revocations::default(),
    };
    let keyring = match &opts.keys {
        Some(h) => Keyring::parse(&read_trust_bytes(opener, h, "keyring")?).map_err(map_troot_error)?,
        None => Keyring::default(),
    };

    let rep1 = if opts.replay {
        match &opts.rep1 {
            Some(h) => {
                let bytes = read_rep1_bytes(opener, h)?;
                Some(
                    Rep1::load(&bytes)
                        .map_err(|e: CompareError| VerifyError::malformed(e.code(), None, e.to_string()))?,
                )
            }
            None => Some(Rep1::embedded().map_err(|e| VerifyError::internal(e.code(), None, e.to_string()))?),
        }
    } else if let Some(h) = &opts.rep1 {
        // Still validate an explicitly provided --rep1 file so malformed inputs fail closed (exit 64).
        let bytes = read_rep1_bytes(opener, h)?;
        let _ = Rep1::load(&bytes).map_err(|e: CompareError| VerifyError::malformed(e.code(), None, e.to_string()))?;
        None
    } else {
        None
    };

    let trust_inputs =
        TrustInputs { trust_root: &trust_root, revocations: &revocations, keyring: &keyring, as_of: as_of.as_ref() };

    let src = opener.open_package(&opts.package).map_err(map_read_error)?;
    let mut reader = PackageReader::open(src).map_err(map_read_error)?;
    reader.verify_all().map_err(map_read_error)?;
    let inventory = build_inventory(&mut reader);

    // 1. Signatures section.
    let sig_report = check_signatures_on_reader(&mut reader, &trust_inputs)?;
    if opts.fail_fast && !sig_report.all_trusted() {
        return Ok(VerificationReport::compose(SectionInputs {
            package: inventory,
            sig_report: Some(&sig_report),
            signatures_skipped: false,
            chain_report: None,
            chain_skipped: true,
            replay: ReplaySection::skipped(),
        }));
    }

    // 2. Chain, checkpoint, and ResultRecord linkage section.
    let chain_report = fid_chain::links::check_reader(&mut reader, Some(&trust_inputs)).map_err(|e| match e {
        fid_chain::links::CheckChainError::Package(re) => map_read_error(re),
        fid_chain::links::CheckChainError::Trust(te) => map_troot_error(te),
        fid_chain::links::CheckChainError::Params(msg) => VerifyError::usage(msg),
        other => VerifyError::malformed(other.code(), other.reason(), other.to_string()),
    })?;

    let ckpt_sig_bad = chain_report.checkpoints.iter().any(|c| {
        !c.signatures.is_empty()
            && c.signatures.iter().any(|s| s.status != fid_sign_core::troot::SignatureStatus::ValidTrusted)
    });
    if opts.fail_fast && (!chain_report.chain_and_links_ok() || ckpt_sig_bad) {
        return Ok(VerificationReport::compose(SectionInputs {
            package: inventory,
            sig_report: Some(&sig_report),
            signatures_skipped: false,
            chain_report: Some(&chain_report),
            chain_skipped: false,
            replay: ReplaySection::skipped(),
        }));
    }

    // 3. Replay section (only when --replay requested).
    let chain_broken = !chain_report.chain_and_links_ok();
    let replay = match (&opts.replay, &rep1) {
        (true, Some(table)) => run_replay_section(
            &mut reader,
            &inventory,
            table,
            opts.kernel_cache.as_deref(),
            memory_limit_bytes,
            time_budget_ms,
            chain_broken,
        )?,
        _ => ReplaySection::not_requested(),
    };

    Ok(VerificationReport::compose(SectionInputs {
        package: inventory,
        sig_report: Some(&sig_report),
        signatures_skipped: false,
        chain_report: Some(&chain_report),
        chain_skipped: false,
        replay,
    }))
}
