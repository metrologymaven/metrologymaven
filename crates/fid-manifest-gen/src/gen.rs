//! Build-time manifest generator and two-stage embedding. Spec node `KRN.MAN.MANI.GEN`.
//!
//! **Stage one** ([`core`]) gathers every [`ManifestCore`] field from its source, validates the
//! core with the kernel's own schema code, and returns the stage-one manifest (the canonical
//! manifest with no artifacts, so it carries the core and its digest) and the generated Rust
//! constant ([`render_constant`]) that the kernel assembly crate compiles in. **Stage two**
//! ([`finish`]) hashes every built artifact, asks each artifact for the core digest it was
//! built with (the `kernel.manifest.core_digest@1.0.0` query of the call interface, sent
//! through the `fid-xart` fail-closed runner by [`XartReporter`]), and writes the full
//! manifest that binds the core digest to the artifact hashes.
//!
//! **No circularity.** The core holds no artifact hash, and the generated constant is the only
//! input that depends on the core digest. It is kept out of the core in two places: the source
//! tree digest excludes exactly the constant's path (and names it in `excluded`), and stage one
//! fails with [`GenError::ConstantInClosure`] if any algorithm's implementation closure reads the
//! constant. So rewriting the constant with the core digest changes nothing in the core, and
//! running stage one again after that gives the same bytes.
//!
//! Field sources, each read from a path in the configuration ([`Config`]), relative to the
//! configuration file:
//!
//! | field | source | digest |
//! |---|---|---|
//! | `source_tree_digest` | every tracked file of `workspace` (from `tracked`, else `git ls-files`) except `constant` | SHA-256 of canonical `{"excluded":[<constant>],"files":[{"path","sha256"},..]}`, files sorted by path bytes |
//! | `lockfile_digest` | `<workspace>/Cargo.lock` | SHA-256 of the file |
//! | `toolchain_digest` | `toolchain_record` (KRN.MAN.REPRO, `fid-manifest-gen toolchain-digest --record`) | SHA-256 of the canonical record; its `lockfile` must equal `lockfile_digest` |
//! | `replay_runtime` | `replay_runtime` (KRN.MAN.WASM `wasm-runtime.toml`) | version = `wasmtime_version`, config digest = SHA-256 of the file |
//! | `algorithms` | `[[algorithm]]` (ID, registering crate, card file), `metadata`, `allowlist` | card digest (KRN.MAN.CARD), implementation digest (KRN.MAN.MANI.IMPL) |
//! | `facade_selftest_digest` | the `fid-num` self-test, run in this tool | SHA-256 of canonical `{"functions":[{"inputs","name","outputs"},..],"inputs_per_fn":"10000"}` |
//! | `solver_limits_digest` | `solver_limits` (KRN.SOLV.CONIC) | SHA-256 of the canonical file |
//! | `reference_suite_digest`, `reference_results_digest` | `rdl_release` (UNC.RDL.FREEZE) | its `suite_digest` and `results_digest` |
//! | `native_gate` | `native_gate_policy` | its `native_gate` value |
//!
//! A source that is not configured or whose file does not exist is
//! [`GenError::MissingSource`] naming the manifest field. JSON sources must be canonical (the
//! kernel's writer reproduces their bytes), so a digest over the file is a digest over its
//! canonical form. Everything is D0: the same inputs give the same bytes.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fid_canon::CValue;
use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::{sha256_digest, Sha256};
use fid_kci::manifest::{
    AlgorithmEntry, Artifact, KernelManifest, ManifestCore, ManifestError, NativeGate, ReplayRuntime,
};
use fid_kci::{
    evaluate, parse_card, AlgorithmId, AlgorithmTable, CardRegistry, Digest, KernelCall, MANIFEST_CORE_DIGEST_QUERY,
};
use fid_num::f::{selftest_inputs, selftest_outputs, FnId, SELFTEST_INPUTS_PER_FN};
use toml::{Table, Value};

use crate::implclosure::{
    git_tracked_files, impl_digests, FsTree, ImplError, Metadata, Registration, SourceTree, TrackedFiles,
    MAX_METADATA_BYTES,
};

/// The only configuration schema this module reads.
pub const CONFIG_SCHEMA: &str = "1";
/// Largest configuration, card, record, limits, release, policy or runtime file read.
pub const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
/// Largest tracked-file list read.
pub const MAX_TRACKED_LIST_BYTES: u64 = 64 * 1024 * 1024;
/// Largest artifact table written by `fid-xart` that is read back.
pub const MAX_TABLE_BYTES: u64 = 1024 * 1024;
/// The schema of the toolchain-and-flags record (KRN.MAN.REPRO).
pub const TOOLCHAIN_RECORD_SCHEMA: &str = "fid-kernel-build/1";
/// The one call of the self-report call set (canonical `fid-kernel-wasm` call encoding).
pub const SELF_REPORT_CALL: &str = r#"{"algorithm":"kernel.manifest.core_digest@1.0.0","inputs":[],"params":""}"#;
/// The call's name in the call set given to `fid-xart`.
pub const SELF_REPORT_NAME: &str = "manifest_core_digest";
/// Host label written into the `fid-xart` table.
pub const XART_HOST: &str = "manifest-gen";

/// Manifest field names used in [`GenError::MissingSource`] and [`GenError::SourceInvalid`].
pub mod field {
    pub const SOURCE_TREE: &str = "source_tree_digest";
    pub const LOCKFILE: &str = "lockfile_digest";
    pub const TOOLCHAIN: &str = "toolchain_digest";
    pub const REPLAY_RUNTIME: &str = "replay_runtime";
    pub const ALGORITHMS: &str = "algorithms";
    pub const SOLVER_LIMITS: &str = "solver_limits_digest";
    pub const REFERENCE_SUITE: &str = "reference_suite_digest";
    pub const REFERENCE_RESULTS: &str = "reference_results_digest";
    pub const NATIVE_GATE: &str = "native_gate";
    /// Not a manifest field: the configuration file itself.
    pub const CONFIG: &str = "config";
    /// Not a manifest field: the stage-one manifest given to `finish`.
    pub const CORE: &str = "core";
    /// Not a manifest field: the artifact list given to `finish`.
    pub const ARTIFACTS: &str = "artifacts";
}

/// Generator errors, each with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// A source of manifest field `field` is not configured or its file does not exist.
    MissingSource { field: &'static str },
    /// A source exists but is malformed.
    SourceInvalid { field: &'static str, reason: String },
    /// The assembled manifest fails the schema (`fid_kci::manifest`).
    SchemaInvalid(ManifestError),
    /// An implementation digest could not be computed (KRN.MAN.MANI.IMPL).
    ImplDigest(Box<ImplError>),
    /// An algorithm's implementation closure reads the generated constant, so the core would
    /// depend on its own digest.
    ConstantInClosure { constant: String },
    /// An artifact reported a core digest other than the stage-one core digest, or reported
    /// none (REQ-KRN.MAN.MANI.GEN-003).
    CoreDigestMismatch { artifact: String },
    /// An artifact could not be asked for its core digest (the runner failed closed).
    SelfReportFailed { artifact: String, reason: String },
    /// A file could not be read or written.
    Io { path: String, reason: String },
}

impl GenError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            GenError::MissingSource { .. } => "missing_source",
            GenError::SourceInvalid { .. } => "source_invalid",
            GenError::SchemaInvalid(_) => "schema_invalid",
            GenError::ImplDigest(_) => "impl_digest",
            GenError::ConstantInClosure { .. } => "constant_in_closure",
            GenError::CoreDigestMismatch { .. } => "core_digest_mismatch",
            GenError::SelfReportFailed { .. } => "self_report_failed",
            GenError::Io { .. } => "io",
        }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            GenError::MissingSource { field } => write!(f, "{code}: {field}"),
            GenError::SourceInvalid { field, reason } => write!(f, "{code}: {field}: {reason}"),
            GenError::SchemaInvalid(e) => write!(f, "{code}: {e}"),
            GenError::ImplDigest(e) => write!(f, "{code}: {e}"),
            GenError::ConstantInClosure { constant } => write!(f, "{code}: {constant}"),
            GenError::CoreDigestMismatch { artifact } => write!(f, "{code}: {artifact}"),
            GenError::SelfReportFailed { artifact, reason } => write!(f, "{code}: {artifact}: {reason}"),
            GenError::Io { path, reason } => write!(f, "{code}: {path}: {reason}"),
        }
    }
}

impl std::error::Error for GenError {}

impl From<ImplError> for GenError {
    fn from(e: ImplError) -> Self {
        GenError::ImplDigest(Box::new(e))
    }
}

fn invalid(field: &'static str, reason: impl Into<String>) -> GenError {
    GenError::SourceInvalid { field, reason: reason.into() }
}

fn io_error(path: &Path, e: &std::io::Error) -> GenError {
    GenError::Io { path: path.display().to_string(), reason: e.kind().to_string() }
}

/// Reads at most `limit` bytes of `path`; `None` if it does not exist.
fn read_optional(path: &Path, field: &'static str, limit: u64) -> Result<Option<Vec<u8>>, GenError> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(path, &e)),
    };
    let mut buf = Vec::new();
    f.take(limit.saturating_add(1)).read_to_end(&mut buf).map_err(|e| io_error(path, &e))?;
    if buf.len() as u64 > limit {
        return Err(invalid(field, format!("{} is larger than {limit} bytes", path.display())));
    }
    Ok(Some(buf))
}

/// Reads the source of `field`; a source that is not configured or absent is `MissingSource`.
fn read_source(field: &'static str, path: Option<&Path>) -> Result<Vec<u8>, GenError> {
    let path = path.ok_or(GenError::MissingSource { field })?;
    read_optional(path, field, MAX_SOURCE_BYTES)?.ok_or(GenError::MissingSource { field })
}

// ------------------------------------------------------------------------------ configuration

/// One algorithm of the kernel's algorithm table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgorithmSource {
    pub id: AlgorithmId,
    /// The registering crate's package name.
    pub crate_name: String,
    /// The algorithm's spec card file.
    pub card: PathBuf,
}

/// The generator configuration (`manifest-gen.toml`), with paths resolved against the
/// configuration file's directory.
///
/// ```toml
/// schema = "1"
/// name = "fid-kernel"
/// semver = "0.1.0"
/// numeric_profile = "knp-1"
/// workspace = "."                  # checkout whose tracked files are hashed
/// tracked = "tracked-files.txt"    # optional: tracked paths, one per line (default: git ls-files)
/// metadata = "metadata.json"       # cargo metadata --format-version 1 (needed with algorithms)
/// constant = "crates/fid-kernel-wasm/src/manifest_core_digest.rs"   # relative to workspace
/// toolchain_record = "toolchain-record.json"
/// replay_runtime = "crates/fid-kernel-wasm/wasm-runtime.toml"
/// solver_limits = "<limits file>"
/// rdl_release = "<release file>"
/// native_gate_policy = "native-gate.toml"
/// allowlist = ["fid-kci", "fid-num"]
///
/// [[algorithm]]
/// id = "fit.plane.ls@1.0.0"
/// crate = "fid-fit"
/// card = "<card file>"
/// ```
///
/// Unknown keys, a wrong `schema` and duplicate keys are refused. Source paths may be absent
/// here; the source is then missing when stage one needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub name: String,
    pub semver: String,
    pub numeric_profile: String,
    pub workspace: Option<PathBuf>,
    pub tracked: Option<PathBuf>,
    pub metadata: Option<PathBuf>,
    /// `/`-separated path of the generated constant, relative to the workspace.
    pub constant: String,
    pub toolchain_record: Option<PathBuf>,
    pub replay_runtime: Option<PathBuf>,
    pub solver_limits: Option<PathBuf>,
    pub rdl_release: Option<PathBuf>,
    pub native_gate_policy: Option<PathBuf>,
    pub allowlist: Vec<String>,
    pub algorithms: Vec<AlgorithmSource>,
}

const CONFIG_KEYS: [&str; 15] = [
    "algorithm",
    "allowlist",
    "constant",
    "metadata",
    "name",
    "native_gate_policy",
    "numeric_profile",
    "rdl_release",
    "replay_runtime",
    "schema",
    "semver",
    "solver_limits",
    "toolchain_record",
    "tracked",
    "workspace",
];

fn table_str<'t>(t: &'t Table, key: &str) -> Result<Option<&'t str>, GenError> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s)),
        Some(_) => Err(invalid(field::CONFIG, format!("{key} must be a non-empty string"))),
    }
}

fn required_str(t: &Table, key: &str) -> Result<String, GenError> {
    table_str(t, key)?.map(str::to_string).ok_or_else(|| invalid(field::CONFIG, format!("{key} is required")))
}

/// A `/`-separated relative path with no empty, `.` or `..` component and no `\` or `:`.
fn is_relative_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

impl Config {
    /// Parses `bytes`; relative paths are resolved against `base` (the file's directory).
    pub fn parse(bytes: &[u8], base: &Path) -> Result<Config, GenError> {
        let text = std::str::from_utf8(bytes).map_err(|_| invalid(field::CONFIG, "not UTF-8"))?;
        let t: Table = text.parse().map_err(|e: toml::de::Error| invalid(field::CONFIG, e.message().to_string()))?;
        if let Some(k) = t.keys().find(|k| !CONFIG_KEYS.contains(&k.as_str())) {
            return Err(invalid(field::CONFIG, format!("unknown key {k}")));
        }
        if table_str(&t, "schema")? != Some(CONFIG_SCHEMA) {
            return Err(invalid(field::CONFIG, format!("schema must be \"{CONFIG_SCHEMA}\"")));
        }
        let path = |key: &str| -> Result<Option<PathBuf>, GenError> { Ok(table_str(&t, key)?.map(|p| base.join(p))) };
        let constant = required_str(&t, "constant")?;
        if !is_relative_path(&constant) {
            return Err(invalid(field::CONFIG, "constant must be a relative /-separated path"));
        }
        let allowlist = match t.get("allowlist") {
            None => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| match v {
                    Value::String(s) => Ok(s.clone()),
                    _ => Err(invalid(field::CONFIG, "allowlist entries must be strings")),
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(invalid(field::CONFIG, "allowlist must be an array")),
        };
        let mut algorithms = Vec::new();
        match t.get("algorithm") {
            None => {}
            Some(Value::Array(a)) => {
                for v in a {
                    let Value::Table(e) = v else {
                        return Err(invalid(field::CONFIG, "algorithm entries must be tables"));
                    };
                    if let Some(k) = e.keys().find(|k| !["card", "crate", "id"].contains(&k.as_str())) {
                        return Err(invalid(field::CONFIG, format!("unknown key algorithm.{k}")));
                    }
                    let id_text = required_str(e, "id")?;
                    let id = AlgorithmId::parse(&id_text)
                        .map_err(|_| invalid(field::CONFIG, format!("bad algorithm id {id_text:?}")))?;
                    algorithms.push(AlgorithmSource {
                        id,
                        crate_name: required_str(e, "crate")?,
                        card: base.join(required_str(e, "card")?),
                    });
                }
            }
            Some(_) => return Err(invalid(field::CONFIG, "algorithm must be an array of tables")),
        }
        let mut ids: Vec<&AlgorithmId> = algorithms.iter().map(|a| &a.id).collect();
        ids.sort();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid(field::CONFIG, "an algorithm is listed twice"));
        }
        Ok(Config {
            name: required_str(&t, "name")?,
            semver: required_str(&t, "semver")?,
            numeric_profile: required_str(&t, "numeric_profile")?,
            workspace: path("workspace")?,
            tracked: path("tracked")?,
            metadata: path("metadata")?,
            constant,
            toolchain_record: path("toolchain_record")?,
            replay_runtime: path("replay_runtime")?,
            solver_limits: path("solver_limits")?,
            rdl_release: path("rdl_release")?,
            native_gate_policy: path("native_gate_policy")?,
            allowlist,
            algorithms,
        })
    }

    /// Reads and parses the configuration file at `path`.
    pub fn load(path: &Path) -> Result<Config, GenError> {
        let bytes = read_optional(path, field::CONFIG, MAX_SOURCE_BYTES)?
            .ok_or(GenError::MissingSource { field: field::CONFIG })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Config::parse(&bytes, base)
    }
}

// ------------------------------------------------------------------------------ helpers

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn canonical(v: &Json, field: &'static str) -> Result<Vec<u8>, GenError> {
    to_canonical(v).map_err(|e| invalid(field, e.to_string()))
}

/// Strings, arrays and objects only (the kernel convention), converted for the kernel writer.
fn to_json(v: &CValue) -> Option<Json> {
    match v {
        CValue::Str(x) => Some(Json::Str(x.clone())),
        CValue::Arr(a) => a.iter().map(to_json).collect::<Option<Vec<_>>>().map(Json::Arr),
        CValue::Obj(m) => {
            m.iter().map(|(k, x)| to_json(x).map(|j| (k.clone(), j))).collect::<Option<Vec<_>>>().map(Json::Obj)
        }
        _ => None,
    }
}

/// Parses a JSON source that must be canonical: strict parse (duplicate keys refused), string,
/// array and object values only, and the kernel writer must reproduce the bytes exactly.
fn canonical_source(field: &'static str, bytes: &[u8]) -> Result<CValue, GenError> {
    let v = fid_canon::parse(bytes).map_err(|e| invalid(field, format!("not JSON: {}", e.code())))?;
    let j = to_json(&v).ok_or_else(|| invalid(field, "only strings, arrays and objects are allowed"))?;
    if canonical(&j, field)? != bytes {
        return Err(invalid(field, "not in canonical form (RFC 8785, no trailing newline)"));
    }
    Ok(v)
}

fn member<'v>(v: &'v CValue, key: &str) -> Option<&'v CValue> {
    match v {
        CValue::Obj(m) => m.get(key),
        _ => None,
    }
}

fn digest_member(v: &CValue, key: &str, field: &'static str) -> Result<Digest, GenError> {
    match member(v, key) {
        Some(CValue::Str(x)) => Digest::parse(x).map_err(|_| invalid(field, format!("{key} is not a sha256: digest"))),
        _ => Err(invalid(field, format!("{key} is missing or not a string"))),
    }
}

/// A source tree that remembers every path read, so stage one can prove the generated constant
/// is in no implementation closure.
struct Recording<'a> {
    inner: &'a dyn SourceTree,
    read: RefCell<BTreeSet<String>>,
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

impl SourceTree for Recording<'_> {
    fn files_under(&self, dir: &str) -> Result<Vec<(String, Vec<u8>)>, ImplError> {
        let files = self.inner.files_under(dir)?;
        self.read.borrow_mut().extend(files.iter().map(|(rel, _)| join(dir, rel)));
        Ok(files)
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, ImplError> {
        self.read.borrow_mut().insert(path.to_string());
        self.inner.read_file(path)
    }

    fn read_optional(&self, path: &str) -> Result<Option<Vec<u8>>, ImplError> {
        self.read.borrow_mut().insert(path.to_string());
        self.inner.read_optional(path)
    }
}

// ------------------------------------------------------------------------------ sources

/// The tracked files of the workspace: the configured list (one `/`-separated path per line,
/// LF), else `git ls-files` in the workspace.
fn tracked_files(config: &Config, ws: &Path) -> Result<TrackedFiles, GenError> {
    match &config.tracked {
        None => Ok(git_tracked_files(ws)?),
        Some(p) => {
            let bytes = read_optional(p, field::SOURCE_TREE, MAX_TRACKED_LIST_BYTES)?
                .ok_or(GenError::MissingSource { field: field::SOURCE_TREE })?;
            let text =
                std::str::from_utf8(&bytes).map_err(|_| invalid(field::SOURCE_TREE, "tracked list is not UTF-8"))?;
            if text.contains('\r') {
                return Err(invalid(field::SOURCE_TREE, "tracked list has a CR"));
            }
            Ok(TrackedFiles::new(text.lines().filter(|l| !l.is_empty()).map(str::to_string))?)
        }
    }
}

/// `source_tree_digest`: every tracked file except the generated constant.
fn source_tree_digest(tracked: &TrackedFiles, tree: &dyn SourceTree, constant: &str) -> Result<Digest, GenError> {
    let mut files = Vec::new();
    for p in tracked.paths().filter(|p| *p != constant) {
        let bytes = tree.read_file(p)?;
        files.push(obj(vec![("path", s(p)), ("sha256", s(sha256_digest(&bytes).as_str()))]));
    }
    let doc = obj(vec![("excluded", Json::Arr(vec![s(constant)])), ("files", Json::Arr(files))]);
    Ok(sha256_digest(&canonical(&doc, field::SOURCE_TREE)?))
}

fn toolchain_digest(config: &Config, lockfile: &Digest) -> Result<Digest, GenError> {
    let f = field::TOOLCHAIN;
    let bytes = read_source(f, config.toolchain_record.as_deref())?;
    let v = canonical_source(f, &bytes)?;
    if member(&v, "schema") != Some(&CValue::Str(TOOLCHAIN_RECORD_SCHEMA.to_string())) {
        return Err(invalid(f, format!("schema is not {TOOLCHAIN_RECORD_SCHEMA}")));
    }
    if &digest_member(&v, "lockfile", f)? != lockfile {
        return Err(invalid(f, "the record's lockfile digest is not this workspace's Cargo.lock"));
    }
    Ok(sha256_digest(&bytes))
}

fn replay_runtime(config: &Config) -> Result<ReplayRuntime, GenError> {
    let f = field::REPLAY_RUNTIME;
    let bytes = read_source(f, config.replay_runtime.as_deref())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid(f, "not UTF-8"))?;
    if text.contains('\r') {
        return Err(invalid(f, "has a CR"));
    }
    let versions: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("wasmtime_version = \"").and_then(|r| r.strip_suffix('"')))
        .collect();
    let [version] = versions.as_slice() else {
        return Err(invalid(f, "needs exactly one wasmtime_version line"));
    };
    Ok(ReplayRuntime { version: version.to_string(), config_digest: sha256_digest(&bytes) })
}

fn algorithms(config: &Config, tree: &dyn SourceTree) -> Result<Vec<AlgorithmEntry>, GenError> {
    let f = field::ALGORITHMS;
    if config.algorithms.is_empty() {
        return Ok(Vec::new());
    }
    let mut cards = Vec::with_capacity(config.algorithms.len());
    for a in &config.algorithms {
        let bytes = read_source(f, Some(&a.card))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid(f, format!("{}: not UTF-8", a.card.display())))?;
        let card = parse_card(text).map_err(|e| invalid(f, format!("{}: {}", a.card.display(), e.code())))?;
        if card.id != a.id {
            return Err(invalid(f, format!("{}: card is for {}, not {}", a.card.display(), card.id, a.id)));
        }
        cards.push(card);
    }
    let registry = CardRegistry::new(cards).map_err(|e| invalid(f, e.code()))?;
    let meta_path = config.metadata.as_deref().ok_or(GenError::MissingSource { field: f })?;
    let meta_bytes =
        read_optional(meta_path, f, MAX_METADATA_BYTES as u64)?.ok_or(GenError::MissingSource { field: f })?;
    let metadata = Metadata::parse(&meta_bytes)?;
    let ids: Vec<AlgorithmId> = config.algorithms.iter().map(|a| a.id.clone()).collect();
    let regs: Vec<Registration> = config
        .algorithms
        .iter()
        .map(|a| Registration { algorithm: a.id.clone(), crate_name: a.crate_name.clone() })
        .collect();
    let recording = Recording { inner: tree, read: RefCell::new(BTreeSet::new()) };
    let table = impl_digests(&metadata, &ids, &regs, &config.allowlist, &recording)?;
    if recording.read.borrow().contains(&config.constant) {
        return Err(GenError::ConstantInClosure { constant: config.constant.clone() });
    }
    let mut out = Vec::with_capacity(ids.len());
    for e in table.entries() {
        let card_digest = registry.digest(&e.algorithm).cloned().ok_or_else(|| invalid(f, "card digest missing"))?;
        out.push(AlgorithmEntry { id: e.algorithm.clone(), card_digest, impl_digest: e.impl_digest });
    }
    Ok(out)
}

/// `facade_selftest_digest`: the `fid-num` self-test run in this tool. Per function, in
/// self-test order, the SHA-256 of its input bit patterns (u64 little-endian, `a` then `b`) and
/// of its output bit patterns, as `fixtures/num/fn-selftest-v1.txt` defines them.
pub fn facade_selftest_digest() -> Result<Digest, GenError> {
    let mut functions = Vec::with_capacity(FnId::ALL.len());
    for f in FnId::ALL {
        let mut hin = Sha256::new();
        for (a, b) in selftest_inputs(f) {
            hin.update(&a.to_bits().to_le_bytes());
            hin.update(&b.to_bits().to_le_bytes());
        }
        let mut hout = Sha256::new();
        for y in selftest_outputs(f) {
            hout.update(&y.to_le_bytes());
        }
        functions.push(obj(vec![
            ("inputs", s(Digest::from_bytes(hin.finish()).as_str())),
            ("name", s(f.name())),
            ("outputs", s(Digest::from_bytes(hout.finish()).as_str())),
        ]));
    }
    let doc = obj(vec![("functions", Json::Arr(functions)), ("inputs_per_fn", s(&SELFTEST_INPUTS_PER_FN.to_string()))]);
    Ok(sha256_digest(&canonical(&doc, "facade_selftest_digest")?))
}

fn solver_limits_digest(config: &Config) -> Result<Digest, GenError> {
    let bytes = read_source(field::SOLVER_LIMITS, config.solver_limits.as_deref())?;
    canonical_source(field::SOLVER_LIMITS, &bytes)?;
    Ok(sha256_digest(&bytes))
}

fn reference_digests(config: &Config) -> Result<(Digest, Digest), GenError> {
    let bytes = read_source(field::REFERENCE_SUITE, config.rdl_release.as_deref())?;
    let v = canonical_source(field::REFERENCE_SUITE, &bytes)?;
    Ok((
        digest_member(&v, "suite_digest", field::REFERENCE_SUITE)?,
        digest_member(&v, "results_digest", field::REFERENCE_RESULTS)?,
    ))
}

fn native_gate(config: &Config) -> Result<NativeGate, GenError> {
    let f = field::NATIVE_GATE;
    let bytes = read_source(f, config.native_gate_policy.as_deref())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid(f, "not UTF-8"))?;
    let t: Table = text.parse().map_err(|e: toml::de::Error| invalid(f, e.message().to_string()))?;
    if t.len() != 1 {
        return Err(invalid(f, "the policy file holds exactly one key, native_gate"));
    }
    match t.get("native_gate") {
        Some(Value::String(v)) => NativeGate::parse(v).ok_or_else(|| invalid(f, format!("unknown value {v:?}"))),
        _ => Err(invalid(f, "native_gate must be a string")),
    }
}

// ------------------------------------------------------------------------------ stage one

/// The generated constant for `digest` (`None`: the committed form). The kernel assembly crate
/// includes it as a module; nothing else may write it.
pub fn render_constant(digest: Option<&Digest>) -> Vec<u8> {
    let value = match digest {
        None => "None".to_string(),
        Some(d) => format!("Some(\"{d}\")"),
    };
    format!(
        "// @generated by `fid-manifest-gen core` (spec node KRN.MAN.MANI.GEN). Do not edit.\n\
         // The committed file holds `None`; a release build overwrites it with the stage-one\n\
         // manifest core digest before the kernel artifacts are built.\n\
         \n\
         /// Manifest core digest this artifact was built from (`None` outside a release build).\n\
         pub const MANIFEST_CORE_DIGEST: Option<&str> = {value};\n"
    )
    .into_bytes()
}

/// The result of stage one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageOne {
    /// The manifest with no artifacts: the core and its digest, canonical.
    pub manifest: KernelManifest,
    /// The generated constant ([`render_constant`] of the core digest).
    pub constant: Vec<u8>,
    /// Where the constant goes, relative to the workspace.
    pub constant_path: String,
}

/// Stage one: gathers every core field from its source (see the module docs), validates the
/// core and digests it (REQ-KRN.MAN.MANI.GEN-001, -002).
pub fn core(config: &Config) -> Result<StageOne, GenError> {
    let ws = config.workspace.as_deref().ok_or(GenError::MissingSource { field: field::SOURCE_TREE })?;
    if !ws.is_dir() {
        return Err(GenError::MissingSource { field: field::SOURCE_TREE });
    }
    let tracked = tracked_files(config, ws)?;
    let tree = FsTree::new(ws, tracked.clone());
    let source_tree_digest = source_tree_digest(&tracked, &tree, &config.constant)?;
    if !tracked.contains("Cargo.lock") {
        return Err(GenError::MissingSource { field: field::LOCKFILE });
    }
    let lockfile_digest = sha256_digest(&tree.read_file("Cargo.lock")?);
    let toolchain_digest = toolchain_digest(config, &lockfile_digest)?;
    let replay_runtime = replay_runtime(config)?;
    let algorithms = algorithms(config, &tree)?;
    let facade_selftest_digest = facade_selftest_digest()?;
    let solver_limits_digest = solver_limits_digest(config)?;
    let (reference_suite_digest, reference_results_digest) = reference_digests(config)?;
    let native_gate = native_gate(config)?;
    let core = ManifestCore {
        name: config.name.clone(),
        semver: config.semver.clone(),
        numeric_profile: config.numeric_profile.clone(),
        source_tree_digest,
        lockfile_digest,
        toolchain_digest,
        replay_runtime,
        algorithms,
        facade_selftest_digest,
        solver_limits_digest,
        reference_suite_digest,
        reference_results_digest,
        native_gate,
    };
    let manifest = KernelManifest::new(core, Vec::new()).map_err(GenError::SchemaInvalid)?;
    let constant = render_constant(Some(manifest.core_digest()));
    Ok(StageOne { manifest, constant, constant_path: config.constant.clone() })
}

// ------------------------------------------------------------------------------ stage two

/// One built artifact and the command that answers one call on stdin (the
/// `fid-kernel-wasm` protocol): the pinned runner for the WASM module, the binary itself for a
/// native build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSpec {
    pub role: String,
    pub target: String,
    pub file: PathBuf,
    pub command: Vec<String>,
}

impl ArtifactSpec {
    /// The artifact's label in errors and in the `fid-xart` table: `<role>-<target>`.
    pub fn label(&self) -> String {
        format!("{}-{}", self.role, self.target)
    }

    /// The `fid-xart` artifact kind: `wasm` for role `wasm`, else `native`.
    pub fn kind(&self) -> &'static str {
        if self.role == "wasm" {
            "wasm"
        } else {
            "native"
        }
    }
}

/// Parses the artifact list (`[[artifact]]` tables with `role`, `target`, `file` and
/// `command`, a non-empty array of strings). Paths are used as written (relative to the
/// directory `finish` runs in).
pub fn parse_artifacts(bytes: &[u8]) -> Result<Vec<ArtifactSpec>, GenError> {
    let f = field::ARTIFACTS;
    let text = std::str::from_utf8(bytes).map_err(|_| invalid(f, "not UTF-8"))?;
    let t: Table = text.parse().map_err(|e: toml::de::Error| invalid(f, e.message().to_string()))?;
    if let Some(k) = t.keys().find(|k| k.as_str() != "artifact") {
        return Err(invalid(f, format!("unknown key {k}")));
    }
    let Some(Value::Array(list)) = t.get("artifact") else {
        return Err(invalid(f, "needs at least one [[artifact]]"));
    };
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        let Value::Table(e) = v else {
            return Err(invalid(f, "artifact entries must be tables"));
        };
        if let Some(k) = e.keys().find(|k| !["command", "file", "role", "target"].contains(&k.as_str())) {
            return Err(invalid(f, format!("unknown key artifact.{k}")));
        }
        let get = |key: &str| match e.get(key) {
            Some(Value::String(x)) if !x.is_empty() => Ok(x.clone()),
            _ => Err(invalid(f, format!("artifact.{key} must be a non-empty string"))),
        };
        let command = match e.get("command") {
            Some(Value::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|x| match x {
                    Value::String(x) => Ok(x.clone()),
                    _ => Err(invalid(f, "artifact.command entries must be strings")),
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(invalid(f, "artifact.command must be a non-empty array")),
        };
        out.push(ArtifactSpec {
            role: get("role")?,
            target: get("target")?,
            file: PathBuf::from(get("file")?),
            command,
        });
    }
    Ok(out)
}

/// What an artifact answered to the self-report call, as the runner tabulated it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfReport {
    /// SHA-256 of the artifact file the runner ran.
    pub artifact_sha256: Digest,
    /// 0 for a result, 1 for a kernel error.
    pub exit: u8,
    /// SHA-256 of the artifact's whole stdout.
    pub result_digest: Digest,
}

/// Asks one artifact the self-report call set (one call, [`SELF_REPORT_CALL`]).
pub trait SelfReporter {
    /// `Err` when the artifact could not answer (the run failed closed).
    fn report(&mut self, artifact: &ArtifactSpec, call_set: &[u8]) -> Result<SelfReport, String>;
}

/// The call-set file sent to `fid-xart` (one line, `<name><TAB><call>`).
pub fn self_report_call_set() -> Vec<u8> {
    format!("# KRN.MAN.MANI.GEN self-report\n{SELF_REPORT_NAME}\t{SELF_REPORT_CALL}\n").into_bytes()
}

/// The exact stdout of an artifact built with `core_digest`: the kernel's own answer to the
/// self-report call, computed by `fid_kci::evaluate` on a table carrying that digest.
pub fn expected_answer(core_digest: &Digest) -> Result<Vec<u8>, GenError> {
    let algorithm = AlgorithmId::parse(MANIFEST_CORE_DIGEST_QUERY).map_err(|e| invalid(field::CORE, e.code()))?;
    let table = AlgorithmTable::new(Vec::new(), CardRegistry::default())
        .map_err(|e| invalid(field::CORE, e.code()))?
        .with_manifest_core_digest(*core_digest);
    let call = KernelCall { algorithm, params: Vec::new(), seed: None, inputs: Vec::new() };
    let result = evaluate(&table, &call).map_err(|e| invalid(field::CORE, e.code()))?;
    Ok(result.canonical_json())
}

/// SHA-256 of a file, read in chunks.
pub fn hash_file(path: &Path) -> Result<Digest, GenError> {
    let mut f = File::open(path).map_err(|e| io_error(path, &e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| io_error(path, &e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(Digest::from_bytes(h.finish()))
}

/// Stage two (REQ-KRN.MAN.MANI.GEN-003): reads the stage-one manifest strictly, hashes every
/// artifact, checks that each one reports the stage-one core digest and assembles the full
/// manifest. Any artifact that answers anything but exactly the expected bytes with exit 0
/// (another digest, no digest, a kernel error) is `CoreDigestMismatch`; one that cannot
/// answer is `SelfReportFailed`; either stops the run and no manifest is built.
pub fn finish(
    stage_one: &[u8],
    artifacts: &[ArtifactSpec],
    reporter: &mut dyn SelfReporter,
) -> Result<KernelManifest, GenError> {
    let first = KernelManifest::parse_canonical(stage_one).map_err(GenError::SchemaInvalid)?;
    if !first.artifacts().is_empty() {
        return Err(invalid(field::CORE, "the stage-one manifest already lists artifacts"));
    }
    if artifacts.is_empty() {
        return Err(invalid(field::ARTIFACTS, "no artifacts"));
    }
    let expected = sha256_digest(&expected_answer(first.core_digest())?);
    let call_set = self_report_call_set();
    let mut out = Vec::with_capacity(artifacts.len());
    for a in artifacts {
        let label = a.label();
        let sha256 = hash_file(&a.file)?;
        let report = reporter
            .report(a, &call_set)
            .map_err(|reason| GenError::SelfReportFailed { artifact: label.clone(), reason })?;
        if report.artifact_sha256 != sha256 {
            return Err(GenError::SelfReportFailed {
                artifact: label,
                reason: "the runner hashed a different artifact file".to_string(),
            });
        }
        if report.exit != 0 || report.result_digest != expected {
            return Err(GenError::CoreDigestMismatch { artifact: label });
        }
        out.push(Artifact { role: a.role.clone(), target: a.target.clone(), sha256 });
    }
    out.sort_by(|x, y| (&x.role, &x.target).cmp(&(&y.role, &y.target)));
    KernelManifest::new(first.core().clone(), out).map_err(GenError::SchemaInvalid)
}

/// Sends the self-report call through the `fid-xart` fail-closed runner (KRN.MAN.XART):
/// `fid-xart run --calls <set> --kind <wasm|native> --host manifest-gen --artifact <label>
/// --artifact-file <file> --out <table> --timeout-ms <ms> -- <command>`, then reads the one
/// row of the table it writes. `fid-xart` is run as a program because a tool crate may not
/// depend on another (ECO.REPO.LAYERS); every check of the answer (canonical envelope, digests,
/// no stderr, exit code, deadline) is the runner's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XartReporter {
    /// The `fid-xart` program.
    pub program: String,
    /// Directory for the call-set and table files (removed after each artifact).
    pub work_dir: PathBuf,
    /// Per-call deadline passed to `fid-xart`.
    pub timeout_ms: u64,
}

fn row_str<'v>(v: &'v CValue, key: &str) -> Result<&'v str, String> {
    match member(v, key) {
        Some(CValue::Str(x)) => Ok(x),
        _ => Err(format!("table: {key} missing")),
    }
}

/// Reads the one row of a `fid-xart` table.
pub fn parse_xart_table(bytes: &[u8]) -> Result<SelfReport, String> {
    let v = fid_canon::parse(bytes).map_err(|e| format!("table: {}", e.code()))?;
    if row_str(&v, "schema")? != "fid-xart-table@1" {
        return Err("table: unknown schema".to_string());
    }
    let artifact_sha256 = Digest::parse(row_str(&v, "artifact_sha256")?).map_err(|_| "table: artifact_sha256")?;
    let rows = match member(&v, "rows") {
        Some(CValue::Arr(r)) => r,
        _ => return Err("table: rows missing".to_string()),
    };
    let [row] = rows.as_slice() else {
        return Err(format!("table: {} rows, expected 1", rows.len()));
    };
    if row_str(row, "name")? != SELF_REPORT_NAME {
        return Err("table: row is not the self-report call".to_string());
    }
    let exit = match row_str(row, "exit")? {
        "0" => 0,
        "1" => 1,
        _ => return Err("table: bad exit".to_string()),
    };
    let result_digest = Digest::parse(row_str(row, "result_digest")?).map_err(|_| "table: result_digest")?;
    Ok(SelfReport { artifact_sha256, exit, result_digest })
}

impl SelfReporter for XartReporter {
    fn report(&mut self, artifact: &ArtifactSpec, call_set: &[u8]) -> Result<SelfReport, String> {
        let label = artifact.label();
        let calls = self.work_dir.join(format!("{label}.self-report-calls.txt"));
        let table = self.work_dir.join(format!("{label}.self-report-table.json"));
        std::fs::write(&calls, call_set).map_err(|e| format!("{}: {e}", calls.display()))?;
        let out = Command::new(&self.program)
            .arg("run")
            .arg("--calls")
            .arg(&calls)
            .args(["--kind", artifact.kind(), "--host", XART_HOST, "--artifact", &label, "--artifact-file"])
            .arg(&artifact.file)
            .arg("--out")
            .arg(&table)
            .arg("--timeout-ms")
            .arg(self.timeout_ms.to_string())
            .arg("--")
            .args(&artifact.command)
            .stdin(Stdio::null())
            .output();
        let _ = std::fs::remove_file(&calls);
        let out = out.map_err(|e| format!("cannot start {}: {e}", self.program))?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&table);
            let text = String::from_utf8_lossy(&out.stderr);
            let last = text.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
            return Err(format!("{} exited with {:?}: {last}", self.program, out.status.code()));
        }
        let bytes = read_optional(&table, field::ARTIFACTS, MAX_TABLE_BYTES).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&table);
        parse_xart_table(&bytes.ok_or("fid-xart wrote no table")?)
    }
}
