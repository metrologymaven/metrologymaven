//! Export envelope: canonical metadata record and atomic local export-root writes
//! (spec node `RPT.OUTPUT.META`).
//!
//! - [`ExportMeta`] holds the resolved-report digest, optional `(template_id, version)`,
//!   `kernel_version`, sorted `(formatter_id, version)` list, and `(renderer_id, version)`.
//!   It holds no wall-clock time, host name, or user name (`REQ-001`, `REQ-002`).
//! - [`ExportName`] validates a file name of 1..=128 ASCII bytes matching `[A-Za-z0-9._-]`
//!   and not starting with `.` (`REQ-005`).
//! - [`ExportSink`] resolves the root handle via [`ExportRootResolver`], refuses raw paths
//!   from agents/scripts (`raw_path_forbidden`, `REQ-003`), refuses non-local roots
//!   (`report.export_root_not_local`, `REQ-004`), refuses existing files unless `overwrite`
//!   (`report.export_exists`, `REQ-006`), sweeps stale `.*.tmp-*` files at the start of every
//!   call, writes atomically via `cap-std` directory capability (temp file, flush, `sync_all`,
//!   rename; never follows symlinks out of the root, `REQ-007`), and returns an
//!   [`ExportReceipt`] (`REQ-008`).

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_io_reg::roots::{Direction, RootAccess, RootHandle, RootId};
use fid_io_reg::Principal;
use fid_project::Uuid;
use fid_report::bind::{FormatterVersion, ResolvedReport};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Schema tag inside every canonical [`ExportMeta`] JSON record.
pub const EXPORT_META_SCHEMA: &str = "export_meta@1";
/// Maximum UTF-8/ASCII byte length of an [`ExportName`] (`REQ-005`).
pub const MAX_EXPORT_NAME_BYTES: usize = 128;
/// Maximum canonical byte length when parsing an [`ExportMeta`] record.
pub const MAX_META_BYTES: usize = 1024 * 1024;
/// Maximum directory entries scanned when sweeping stale `.*.tmp-*` files.
pub const MAX_STALE_SCAN: usize = 16_384;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(1);

// ------------------------------------------------------------------ errors

/// Typed errors of the report export envelope (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// `raw_path_forbidden`: an agent or script supplied a raw path instead of an export-root handle (`REQ-003`).
    RawPathForbidden,
    /// `report.export_root_not_local`: the resolved export root is not on a local filesystem (`REQ-004`).
    ExportRootNotLocal(String),
    /// `report.export_name_invalid`: file name breaks `1..=128` `[A-Za-z0-9._-]`, non-dot-prefix rule (`REQ-005`).
    ExportNameInvalid,
    /// `report.export_exists`: target file already exists and `overwrite` was false (`REQ-006`).
    ExportExists(String),
    /// `root_not_found`: named export root handle is not registered or not approved for export.
    RootNotFound(String),
    /// `report.export_io`: filesystem or canonical serialization failure.
    Io { kind: String },
    /// `report.meta_invalid`: invalid metadata field or canonical JSON.
    MetaInvalid(&'static str),
}

impl ExportError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ExportError::RawPathForbidden => "raw_path_forbidden",
            ExportError::ExportRootNotLocal(_) => "report.export_root_not_local",
            ExportError::ExportNameInvalid => "report.export_name_invalid",
            ExportError::ExportExists(_) => "report.export_exists",
            ExportError::RootNotFound(_) => "root_not_found",
            ExportError::Io { .. } => "report.export_io",
            ExportError::MetaInvalid(_) => "report.meta_invalid",
        }
    }

    /// Canonical `--json` / MCP error object.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        match self {
            ExportError::RawPathForbidden | ExportError::ExportNameInvalid => {}
            ExportError::ExportRootNotLocal(h) | ExportError::RootNotFound(h) => {
                o.insert("root".to_string(), CValue::Str(h.clone()));
            }
            ExportError::ExportExists(name) => {
                o.insert("name".to_string(), CValue::Str(name.clone()));
            }
            ExportError::Io { kind } => {
                o.insert("kind".to_string(), CValue::Str(kind.clone()));
            }
            ExportError::MetaInvalid(reason) => {
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
            }
        }
        CValue::Obj(o)
    }

    fn from_io(err: &io::Error) -> ExportError {
        ExportError::Io { kind: format!("{:?}", err.kind()) }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::RawPathForbidden => f.write_str("raw_path_forbidden"),
            ExportError::ExportRootNotLocal(h) => write!(f, "report.export_root_not_local: {h}"),
            ExportError::ExportNameInvalid => f.write_str("report.export_name_invalid"),
            ExportError::ExportExists(n) => write!(f, "report.export_exists: {n}"),
            ExportError::RootNotFound(h) => write!(f, "root_not_found: {h}"),
            ExportError::Io { kind } => write!(f, "report.export_io: {kind}"),
            ExportError::MetaInvalid(r) => write!(f, "report.meta_invalid: {r}"),
        }
    }
}

impl std::error::Error for ExportError {}

// ------------------------------------------------------------------ validated newtypes

/// Validated export file name: 1..=128 ASCII bytes from `[A-Za-z0-9._-]`, not starting with `.`
/// (`REQ-RPT.OUTPUT.META-005`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExportName(String);

impl ExportName {
    /// Parses and validates an export file name.
    pub fn parse(raw: &str) -> Result<ExportName, ExportError> {
        let bytes = raw.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_EXPORT_NAME_BYTES {
            return Err(ExportError::ExportNameInvalid);
        }
        if bytes[0] == b'.' {
            return Err(ExportError::ExportNameInvalid);
        }
        let valid = bytes.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-');
        if !valid {
            return Err(ExportError::ExportNameInvalid);
        }
        // Reject reserved Windows device names (CON, PRN, AUX, NUL, COM1..9, LPT1..9) for cross-OS safety.
        let stem = raw.split('.').next().unwrap_or(raw);
        let upper = stem.to_ascii_uppercase();
        let reserved = matches!(
            upper.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        );
        if reserved {
            return Err(ExportError::ExportNameInvalid);
        }
        Ok(ExportName(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExportName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Validated export root handle (`root://<root-id>` or plain `<root-id>`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExportRootHandle {
    root_id: RootId,
}

impl ExportRootHandle {
    /// Parses an approved export root handle (`root://<id>` or `<id>`).
    ///
    /// Agents and scripts passing a raw filesystem path (e.g. `/tmp/out`, `C:\out`, `../out`)
    /// fail immediately with [`ExportError::RawPathForbidden`] (`REQ-003`).
    pub fn parse(raw: &str) -> Result<ExportRootHandle, ExportError> {
        if let Some(rest) = raw.strip_prefix("root://") {
            let trimmed = rest.trim_end_matches('/');
            if trimmed.contains('/') || trimmed.contains('\\') {
                // Verify through fid-io-reg's RootHandle parser for consistent error behavior.
                let _ = RootHandle::parse(raw).map_err(|_| ExportError::RawPathForbidden)?;
                return Err(ExportError::RawPathForbidden);
            }
            let root_id = RootId::new(trimmed).ok_or(ExportError::RawPathForbidden)?;
            Ok(ExportRootHandle { root_id })
        } else {
            // Bare root ID must have no path separators, drive letters, or dots.
            if raw.contains('/') || raw.contains('\\') || raw.contains(':') || raw.contains('.') {
                return Err(ExportError::RawPathForbidden);
            }
            let root_id = RootId::new(raw).ok_or(ExportError::RawPathForbidden)?;
            Ok(ExportRootHandle { root_id })
        }
    }

    pub fn root_id(&self) -> &RootId {
        &self.root_id
    }

    /// Canonical `root://<id>` wire string.
    pub fn wire(&self) -> String {
        format!("root://{}", self.root_id.as_str())
    }
}

impl fmt::Display for ExportRootHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "root://{}", self.root_id.as_str())
    }
}

// ------------------------------------------------------------------ ExportMeta

/// Optional template reference carried inside [`ExportMeta`] (`id` + `version`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateVersionRef {
    pub id: Uuid,
    pub version: u32,
}

impl TemplateVersionRef {
    pub fn to_value(&self) -> CValue {
        obj([("id", CValue::Str(self.id.to_string())), ("version", CValue::Str(self.version.to_string()))])
    }

    pub fn from_value(v: &CValue) -> Result<TemplateVersionRef, ExportError> {
        let o = exact_obj(v, &["id", "version"])?;
        let id = Uuid::parse(req_str(o, "id")?).map_err(|_| ExportError::MetaInvalid("template id"))?;
        let version =
            req_str(o, "version")?.parse::<u32>().map_err(|_| ExportError::MetaInvalid("template version"))?;
        if version == 0 {
            return Err(ExportError::MetaInvalid("template version zero"));
        }
        Ok(TemplateVersionRef { id, version })
    }
}

/// Renderer identity and version (e.g. `id = "pdf"`, `version = "1.0.0"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererVersion {
    pub id: String,
    pub version: String,
}

impl RendererVersion {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Result<RendererVersion, ExportError> {
        let id = id.into();
        let version = version.into();
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(ExportError::MetaInvalid("renderer id"));
        }
        if version.is_empty() || version.len() > 64 || !version.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(ExportError::MetaInvalid("renderer version"));
        }
        Ok(RendererVersion { id, version })
    }

    pub fn to_value(&self) -> CValue {
        obj([("id", CValue::Str(self.id.clone())), ("version", CValue::Str(self.version.clone()))])
    }

    pub fn from_value(v: &CValue) -> Result<RendererVersion, ExportError> {
        let o = exact_obj(v, &["id", "version"])?;
        RendererVersion::new(req_str(o, "id")?, req_str(o, "version")?)
    }
}

/// Deterministic export metadata record shared by all RPT exporters (`REQ-001`, `REQ-002`).
///
/// Contains NO wall-clock time, host name, or user name so that the same resolved report and
/// renderer always produce byte-identical canonical JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportMeta {
    pub report_digest: Digest,
    pub template: Option<TemplateVersionRef>,
    pub kernel_version: String,
    pub formatters: Vec<FormatterVersion>,
    pub renderer: RendererVersion,
}

impl ExportMeta {
    /// Constructs a canonical [`ExportMeta`] from a [`ResolvedReport`] and [`RendererVersion`].
    pub fn from_resolved(resolved: &ResolvedReport, renderer: RendererVersion) -> ExportMeta {
        let mut formatters = resolved.provenance.formatters.clone();
        formatters.sort();
        formatters.dedup();
        let template = resolved.provenance.template.map(|t| TemplateVersionRef { id: t.id, version: t.version });
        ExportMeta {
            report_digest: resolved.digest,
            template,
            kernel_version: resolved.provenance.kernel_version.clone(),
            formatters,
            renderer,
        }
    }

    /// Builds canonical [`ExportMeta`] directly from its parts (sorting/deduplicating formatters).
    pub fn new(
        report_digest: Digest,
        template: Option<TemplateVersionRef>,
        kernel_version: impl Into<String>,
        mut formatters: Vec<FormatterVersion>,
        renderer: RendererVersion,
    ) -> Result<ExportMeta, ExportError> {
        let kernel_version = kernel_version.into();
        if kernel_version.is_empty() || kernel_version.len() > 64 {
            return Err(ExportError::MetaInvalid("kernel_version"));
        }
        formatters.sort();
        formatters.dedup();
        Ok(ExportMeta { report_digest, template, kernel_version, formatters, renderer })
    }

    /// Canonical JSON value (keys sorted per RFC 8785).
    pub fn to_value(&self) -> CValue {
        let mut sorted_fmts = self.formatters.clone();
        sorted_fmts.sort();
        sorted_fmts.dedup();
        obj([
            ("formatters", CValue::Arr(sorted_fmts.iter().map(FormatterVersion::to_value).collect())),
            ("kernel_version", CValue::Str(self.kernel_version.clone())),
            ("renderer", self.renderer.to_value()),
            ("report_digest", CValue::Str(self.report_digest.as_str().to_string())),
            ("schema", CValue::Str(EXPORT_META_SCHEMA.to_string())),
            ("template", self.template.as_ref().map_or(CValue::Null, TemplateVersionRef::to_value)),
        ])
    }

    /// RFC 8785 canonical JSON bytes (`REQ-001`, `REQ-002`).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ExportError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| ExportError::MetaInvalid("canonical_encode"))
    }

    /// Strict parser for RFC 8785 canonical [`ExportMeta`] bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<ExportMeta, ExportError> {
        let v = parse_with_limits(bytes, MAX_META_BYTES, 16).map_err(|_| ExportError::MetaInvalid("invalid_json"))?;
        if to_canonical_bytes(&v).map_err(|_| ExportError::MetaInvalid("canonical_check"))? != bytes {
            return Err(ExportError::MetaInvalid("not_canonical"));
        }
        let o = exact_obj(&v, &["formatters", "kernel_version", "renderer", "report_digest", "schema", "template"])?;
        if req_str(o, "schema")? != EXPORT_META_SCHEMA {
            return Err(ExportError::MetaInvalid("bad_schema"));
        }
        let report_digest =
            Digest::parse(req_str(o, "report_digest")?).map_err(|_| ExportError::MetaInvalid("report_digest"))?;
        let kernel_version = req_str(o, "kernel_version")?.to_string();
        let renderer =
            RendererVersion::from_value(o.get("renderer").ok_or(ExportError::MetaInvalid("renderer_missing"))?)?;
        let template = match o.get("template") {
            Some(CValue::Null) => None,
            Some(tv) => Some(TemplateVersionRef::from_value(tv)?),
            None => return Err(ExportError::MetaInvalid("template_missing")),
        };
        let Some(CValue::Arr(fmts)) = o.get("formatters") else {
            return Err(ExportError::MetaInvalid("formatters_not_array"));
        };
        let mut formatters = Vec::with_capacity(fmts.len());
        for f in fmts {
            formatters
                .push(FormatterVersion::from_value(f).map_err(|_| ExportError::MetaInvalid("formatter_invalid"))?);
        }
        ExportMeta::new(report_digest, template, kernel_version, formatters, renderer)
    }
}

// ------------------------------------------------------------------ ExportReceipt

/// Receipt returned by [`ExportSink::write`] for the calling command's W1 journal event (`REQ-008`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReceipt {
    pub root: ExportRootHandle,
    pub name: ExportName,
    pub bytes: u64,
    pub sha256: Digest,
}

impl ExportReceipt {
    pub fn to_value(&self) -> CValue {
        obj([
            ("bytes", CValue::Str(self.bytes.to_string())),
            ("name", CValue::Str(self.name.as_str().to_string())),
            ("root", CValue::Str(self.root.wire())),
            ("sha256", CValue::Str(self.sha256.as_str().to_string())),
        ])
    }
}

// ------------------------------------------------------------------ ExportRootResolver

/// Resolved directory backing an approved export root handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub handle: ExportRootHandle,
    pub dir: PathBuf,
    pub is_local: bool,
}

/// Resolution seam consumed from `IO.REG` (`Design: Interfaces consumed`).
pub trait ExportRootResolver {
    /// Resolves `input` (an export-root handle, or raw path if permitted) for `principal`.
    ///
    /// Agents and scripts passing a raw path MUST fail with [`ExportError::RawPathForbidden`].
    fn resolve(&self, input: &str, principal: Principal) -> Result<ResolvedRoot, ExportError>;

    /// Reports whether `root` resides on a local filesystem (`REQ-004`).
    fn is_local(&self, root: &ResolvedRoot) -> bool {
        root.is_local
    }
}

/// In-memory configured table of approved export roots (local or non-local) with principal-aware
/// handle enforcement.
#[derive(Debug, Clone, Default)]
pub struct LocalRootResolver {
    roots: BTreeMap<RootId, (RootAccess, PathBuf, bool)>,
}

impl LocalRootResolver {
    pub fn new() -> LocalRootResolver {
        LocalRootResolver::default()
    }

    /// Registers a local approved root directory (`is_local = true`).
    pub fn insert_local(&mut self, id: RootId, access: RootAccess, dir: impl Into<PathBuf>) {
        self.roots.insert(id, (access, dir.into(), true));
    }

    /// Registers a root directory with explicit locality (`is_local = false` triggers
    /// `report.export_root_not_local` on W1 export, `AC-6`).
    pub fn insert_with_locality(&mut self, id: RootId, access: RootAccess, dir: impl Into<PathBuf>, is_local: bool) {
        self.roots.insert(id, (access, dir.into(), is_local));
    }
}

impl ExportRootResolver for LocalRootResolver {
    fn resolve(&self, input: &str, principal: Principal) -> Result<ResolvedRoot, ExportError> {
        let handle = match ExportRootHandle::parse(input) {
            Ok(h) => h,
            Err(ExportError::RawPathForbidden) => {
                // REQ-003: agents and scripts can never pass raw paths; even for W1 export
                // roots, this sink requires an approved handle.
                if matches!(principal, Principal::Agent | Principal::Script) {
                    return Err(ExportError::RawPathForbidden);
                }
                return Err(ExportError::RawPathForbidden);
            }
            Err(e) => return Err(e),
        };
        let Some((access, dir, is_local)) = self.roots.get(handle.root_id()) else {
            return Err(ExportError::RootNotFound(handle.wire()));
        };
        if !access.allows(Direction::Export) {
            return Err(ExportError::RootNotFound(handle.wire()));
        }
        Ok(ResolvedRoot { handle, dir: dir.clone(), is_local: *is_local })
    }
}

// ------------------------------------------------------------------ ExportSink

/// Fault-injection hook called after writing and syncing the temporary file and BEFORE renaming
/// it into place (`AC-7` crash safety verification).
pub type BeforeRenameHook = Box<dyn Fn(&str) -> Result<(), ExportError> + Send + Sync>;

/// Atomic local export sink shared by all W1 (`fs_effect: write_root`) RPT exporters.
pub struct ExportSink<'a> {
    resolver: &'a dyn ExportRootResolver,
    before_rename: Option<BeforeRenameHook>,
}

impl<'a> ExportSink<'a> {
    /// Creates an [`ExportSink`] backed by `resolver`.
    pub fn new(resolver: &'a dyn ExportRootResolver) -> ExportSink<'a> {
        ExportSink { resolver, before_rename: None }
    }

    /// Installs a crash/fault hook run after flushing and syncing the temp file and before
    /// renaming it onto the target (`AC-7`).
    pub fn with_before_rename_hook(mut self, hook: BeforeRenameHook) -> ExportSink<'a> {
        self.before_rename = Some(hook);
        self
    }

    /// Validates `name`, resolves `root_handle` for `principal`, verifies locality, sweeps
    /// stale `.*.tmp-*` files, writes `bytes` atomically inside the root capability directory,
    /// and returns an [`ExportReceipt`] (`REQ-003`..`REQ-008`).
    pub fn write(
        &self,
        principal: Principal,
        root_handle: &str,
        name: &str,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<ExportReceipt, ExportError> {
        // 1. Validate export file name before touching the filesystem (REQ-005, AC-4).
        let valid_name = ExportName::parse(name)?;

        // 2. Resolve handle through IO.REG seam (REQ-003, AC-3).
        let resolved = self.resolver.resolve(root_handle, principal)?;

        // 3. Enforce local filesystem requirement for W1 exports (REQ-004, AC-6).
        if !self.resolver.is_local(&resolved) {
            return Err(ExportError::ExportRootNotLocal(resolved.handle.wire()));
        }

        // 4. Open root directory as a capability (prevents symlink escape out of root).
        let dir = Dir::open_ambient_dir(&resolved.dir, ambient_authority()).map_err(|e| ExportError::from_io(&e))?;

        // 5. Sweep stale `.*.tmp-*` regular files at the start of every call (AC-7).
        sweep_stale_temps(&dir)?;

        // 6. Check existing target (REQ-006, AC-5; refuse symlinks or directories).
        let target = valid_name.as_str();
        match dir.symlink_metadata(target) {
            Ok(meta) => {
                if !overwrite {
                    return Err(ExportError::ExportExists(target.to_string()));
                }
                if meta.file_type().is_symlink() || meta.is_dir() {
                    return Err(ExportError::Io { kind: "InvalidInput".to_string() });
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(ExportError::from_io(&e)),
        }

        // 7. Create unique temporary file `.<name>.tmp-<seq>` inside the same capability dir.
        let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let tmp_name = format!(".{target}.tmp-{pid}-{seq:016x}");
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        let mut file = dir.open_with(&tmp_name, &opts).map_err(|e| ExportError::from_io(&e))?;

        let mut hasher = Sha256::new();
        let write_res: Result<(), ExportError> = (|| {
            for chunk in bytes.chunks(64 * 1024) {
                file.write_all(chunk).map_err(|e| ExportError::from_io(&e))?;
                hasher.update(chunk);
            }
            file.flush().map_err(|e| ExportError::from_io(&e))?;
            file.sync_all().map_err(|e| ExportError::from_io(&e))?;
            Ok(())
        })();
        drop(file);

        if let Err(err) = write_res {
            let _ = dir.remove_file(&tmp_name);
            return Err(err);
        }

        // Optional crash-simulation hook AFTER temp write/sync and BEFORE rename (AC-7).
        // Intentionally does NOT delete tmp_name so the test proves target is unchanged AND
        // the next ExportSink::write call cleans up the stale temp file!
        if let Some(hook) = &self.before_rename {
            hook(&tmp_name)?;
        }

        // 8. Atomic commit into place inside the same capability directory (REQ-007).
        if overwrite {
            if let Err(e) = dir.rename(&tmp_name, &dir, target) {
                let _ = dir.remove_file(&tmp_name);
                return Err(ExportError::from_io(&e));
            }
        } else {
            // Hard link fails with AlreadyExists if a concurrent writer created target; never clobbers.
            match dir.hard_link(&tmp_name, &dir, target) {
                Ok(()) => {
                    let _ = dir.remove_file(&tmp_name);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = dir.remove_file(&tmp_name);
                    return Err(ExportError::ExportExists(target.to_string()));
                }
                Err(e) => {
                    let _ = dir.remove_file(&tmp_name);
                    return Err(ExportError::from_io(&e));
                }
            }
        }

        let digest = Digest::from_bytes(hasher.finalize().into());
        let byte_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        Ok(ExportReceipt { root: resolved.handle, name: valid_name, bytes: byte_len, sha256: digest })
    }
}

fn is_stale_tmp_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    let Some((prefix, suffix)) = rest.rsplit_once(".tmp-") else {
        return false;
    };
    !prefix.is_empty() && !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn sweep_stale_temps(dir: &Dir) -> Result<(), ExportError> {
    let entries = dir.entries().map_err(|e| ExportError::from_io(&e))?;
    for entry_res in entries.take(MAX_STALE_SCAN) {
        let Ok(entry) = entry_res else {
            continue;
        };
        let fname = entry.file_name();
        let Some(name_str) = fname.to_str() else {
            continue;
        };
        if !is_stale_tmp_name(name_str) {
            continue;
        }
        if let Ok(meta) = dir.symlink_metadata(name_str) {
            if meta.is_file() && !meta.file_type().is_symlink() {
                let _ = dir.remove_file(name_str);
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ private JSON helpers

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    CValue::Obj(m)
}

fn exact_obj<'a>(v: &'a CValue, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, ExportError> {
    let CValue::Obj(o) = v else {
        return Err(ExportError::MetaInvalid("not_object"));
    };
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err(ExportError::MetaInvalid("unexpected_keys"));
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, ExportError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(ExportError::MetaInvalid("expected_string")),
    }
}
