//! Toolchain-and-flags digest of the kernel artifact build. Spec node `KRN.MAN.REPRO`.
//!
//! The reproducible kernel build (`scripts/rebuild-kernel.sh`) takes every setting that can
//! change the artifact's bits from three committed files in [`SETTINGS_DIR`], which apply only
//! to builds started in that directory (so they never override the toolchain of other CI jobs):
//!
//! - `rust-toolchain.toml`: the exact toolchain (`channel = "X.Y.Z"`), read by rustup;
//! - `.cargo/config.toml`: the build target, the target's static `rustflags` (target CPU and
//!   target features, which is where FMA would come from) and the `[profile.kernel-artifact]`
//!   Cargo profile;
//! - `kernel-build.toml`: the toolchain's pinned commit hash and LLVM version, the package and
//!   binary that make the artifact, and the fixed prefixes absolute paths are remapped to.
//!
//! [`settings`] checks those files strictly and fails closed (`AC-4`): every key is known,
//! every profile key that changes code generation is set explicitly (so nothing is inherited
//! from `release`), `codegen-units = 1`, `panic = "abort"`, `incremental = false`, and the
//! workspace's own `Cargo.toml` and `.cargo/config.toml` hold nothing that would reach into the
//! kernel profile (package overrides, rustflags, source replacement, environment).
//!
//! [`record`] adds what only the real toolchain can tell: `rustc -vV` must report the pinned
//! release, commit hash and LLVM version (else [`ToolchainError::ToolchainDrift`]), and the
//! target features that `rustc --print cfg` reports under the static flags
//! ([`Settings::rustc_cfg_args`]) must contain no FMA-capable feature ([`FMA_FEATURES`]) and
//! none of the features the flags disable. The record, written by the kernel's canonical JSON
//! writer, is
//!
//! ```text
//! {"artifact":{"bin":..,"package":..,"profile":"kernel-artifact","target":..},
//!  "lockfile":"sha256:<hex of Cargo.lock bytes>",
//!  "profile":{"<key>":"<type>:<value>",..},           type is bool, int or str
//!  "remap":{"cargo_home":..,"target_dir":..,"vendor":..,"workspace":..},
//!  "rustflags":[..],
//!  "schema":"fid-kernel-build/1",
//!  "target_features":[..],                              effective, sorted, unique
//!  "toolchain":{"channel":..,"commit_hash":..,"llvm_version":..}}
//! ```
//!
//! and the toolchain-and-flags digest (the manifest's `toolchain_digest`) is SHA-256 over those
//! bytes. The host triple is deliberately not part of it: builds on different hosts must agree.
//! Every input is bytes, so the digest is a pure function of them (D0).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::{sha256_digest, Digest};
use toml::{Table, Value};

/// Directory, relative to the workspace root, that holds the kernel build settings.
pub const SETTINGS_DIR: &str = "kernel-build";
/// The settings files, relative to the workspace root (as named in errors).
pub const RUST_TOOLCHAIN: &str = "kernel-build/rust-toolchain.toml";
pub const CARGO_CONFIG: &str = "kernel-build/.cargo/config.toml";
pub const KERNEL_BUILD: &str = "kernel-build/kernel-build.toml";
pub const WORKSPACE_MANIFEST: &str = "Cargo.toml";
pub const WORKSPACE_CONFIG: &str = ".cargo/config.toml";
pub const LOCKFILE: &str = "Cargo.lock";
/// Schema of the record.
pub const SCHEMA: &str = "fid-kernel-build/1";
/// The Cargo profile that builds the kernel artifact.
pub const PROFILE: &str = "kernel-artifact";
/// Every key the kernel profile must set, and the only ones it may set.
pub const PROFILE_KEYS: [&str; 10] = [
    "codegen-units",
    "debug",
    "debug-assertions",
    "incremental",
    "inherits",
    "lto",
    "opt-level",
    "overflow-checks",
    "panic",
    "strip",
];
/// Target features that give fused multiply-add (wasm32: `relaxed_madd`), never allowed.
pub const FMA_FEATURES: [&str; 2] = ["fma", "relaxed-simd"];
/// The only `-C target-cpu` values accepted (an allowlist: the generic baseline CPUs).
pub const ALLOWED_CPUS: [&str; 2] = ["generic", "mvp"];
/// The remap prefixes `kernel-build.toml` must name.
pub const REMAP_KEYS: [&str; 4] = ["cargo_home", "target_dir", "vendor", "workspace"];
/// Largest settings file read.
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// Largest lockfile read.
pub const MAX_LOCKFILE_BYTES: usize = 16 * 1024 * 1024;
/// Largest `rustc` output read.
pub const MAX_RUSTC_OUTPUT_BYTES: usize = 64 * 1024;

/// Why the settings or the toolchain were refused. Each has a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolchainError {
    /// A file is larger than its limit, not UTF-8 or not valid TOML (duplicate keys included).
    BadFile { file: &'static str },
    /// A required key is missing.
    Missing { file: &'static str, key: String },
    /// A key the schema does not know.
    UnexpectedKey { file: &'static str, key: String },
    /// A key has the wrong type or a refused value.
    BadValue { file: &'static str, key: String },
    /// The workspace's own Cargo files set something that would reach into the kernel build.
    WorkspaceOverride { file: &'static str, key: String },
    /// `rustc -vV` or `rustc --print cfg` output is malformed.
    BadRustcOutput { reason: &'static str },
    /// The toolchain in use is not the pinned one.
    ToolchainDrift { field: &'static str, pinned: String, actual: String },
    /// An FMA-capable target feature is enabled.
    FmaFeatureEnabled { feature: String },
    /// A feature the static flags disable is still enabled.
    FeatureNotDisabled { feature: String },
    /// A Cargo configuration file that the kernel build would read but this check does not
    /// (a legacy `.cargo/config`, or one in a directory above the workspace).
    StrayCargoConfig { path: String },
}

impl ToolchainError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ToolchainError::BadFile { .. } => "bad_file",
            ToolchainError::Missing { .. } => "missing",
            ToolchainError::UnexpectedKey { .. } => "unexpected_key",
            ToolchainError::BadValue { .. } => "bad_value",
            ToolchainError::WorkspaceOverride { .. } => "workspace_override",
            ToolchainError::BadRustcOutput { .. } => "bad_rustc_output",
            ToolchainError::ToolchainDrift { .. } => "toolchain_drift",
            ToolchainError::FmaFeatureEnabled { .. } => "fma_feature_enabled",
            ToolchainError::FeatureNotDisabled { .. } => "feature_not_disabled",
            ToolchainError::StrayCargoConfig { .. } => "stray_cargo_config",
        }
    }
}

impl fmt::Display for ToolchainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ToolchainError::BadFile { file } => write!(f, "{code}: {file}"),
            ToolchainError::Missing { file, key }
            | ToolchainError::UnexpectedKey { file, key }
            | ToolchainError::BadValue { file, key }
            | ToolchainError::WorkspaceOverride { file, key } => write!(f, "{code}: {file}: {key}"),
            ToolchainError::BadRustcOutput { reason } => write!(f, "{code}: {reason}"),
            ToolchainError::ToolchainDrift { field, pinned, actual } => {
                write!(f, "{code}: {field}: pinned {pinned:?}, actual {actual:?}")
            }
            ToolchainError::FmaFeatureEnabled { feature } | ToolchainError::FeatureNotDisabled { feature } => {
                write!(f, "{code}: {feature}")
            }
            ToolchainError::StrayCargoConfig { path } => write!(f, "{code}: {path}"),
        }
    }
}

impl std::error::Error for ToolchainError {}

/// The committed files, as bytes. `workspace_config` is `None` when the workspace has no
/// `.cargo/config.toml`.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub rust_toolchain: &'a [u8],
    pub cargo_config: &'a [u8],
    pub kernel_build: &'a [u8],
    pub workspace_manifest: &'a [u8],
    pub workspace_config: Option<&'a [u8]>,
    pub lockfile: &'a [u8],
}

/// The checked settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub channel: String,
    pub commit_hash: String,
    pub llvm_version: String,
    pub package: String,
    pub bin: String,
    pub target: String,
    /// The target's static rustflags, in file order.
    pub rustflags: Vec<String>,
    /// Profile key to `<type>:<value>`.
    pub profile: BTreeMap<String, String>,
    /// Remap key to its fixed prefix.
    pub remap: BTreeMap<String, String>,
    /// Target features the static flags disable (`-name`).
    pub disabled_features: BTreeSet<String>,
    /// SHA-256 of the lockfile bytes.
    pub lockfile: Digest,
}

impl Settings {
    /// Arguments for `rustc` that print the effective cfg (target features) for the kernel
    /// target under the static rustflags.
    pub fn rustc_cfg_args(&self) -> Vec<String> {
        let mut args = vec!["--print".to_string(), "cfg".to_string(), "--target".to_string(), self.target.clone()];
        args.extend(self.rustflags.iter().cloned());
        args
    }
}

/// A checked record and its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRecord {
    /// Canonical JSON bytes (see the module docs).
    pub canonical: Vec<u8>,
    /// SHA-256 of `canonical`: the toolchain-and-flags digest.
    pub digest: Digest,
}

fn parse(bytes: &[u8], file: &'static str, max: usize) -> Result<Table, ToolchainError> {
    if bytes.len() > max {
        return Err(ToolchainError::BadFile { file });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ToolchainError::BadFile { file })?;
    text.parse::<Table>().map_err(|_| ToolchainError::BadFile { file })
}

/// Fails on the first key (in sorted order) not in `allowed`.
fn only(t: &Table, allowed: &[&str], file: &'static str, prefix: &str) -> Result<(), ToolchainError> {
    for k in t.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(ToolchainError::UnexpectedKey { file, key: format!("{prefix}{k}") });
        }
    }
    Ok(())
}

fn table<'t>(t: &'t Table, key: &str, file: &'static str, prefix: &str) -> Result<&'t Table, ToolchainError> {
    match t.get(key) {
        Some(Value::Table(x)) => Ok(x),
        Some(_) => Err(ToolchainError::BadValue { file, key: format!("{prefix}{key}") }),
        None => Err(ToolchainError::Missing { file, key: format!("{prefix}{key}") }),
    }
}

fn string(t: &Table, key: &str, file: &'static str, prefix: &str) -> Result<String, ToolchainError> {
    match t.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(_) => Err(ToolchainError::BadValue { file, key: format!("{prefix}{key}") }),
        None => Err(ToolchainError::Missing { file, key: format!("{prefix}{key}") }),
    }
}

fn strings(t: &Table, key: &str, file: &'static str, prefix: &str) -> Result<Vec<String>, ToolchainError> {
    let bad = || ToolchainError::BadValue { file, key: format!("{prefix}{key}") };
    match t.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) if !s.is_empty() => Ok(s.clone()),
                _ => Err(bad()),
            })
            .collect(),
        Some(_) => Err(bad()),
        None => Err(ToolchainError::Missing { file, key: format!("{prefix}{key}") }),
    }
}

/// `X.Y.Z`, decimal without leading zeros.
fn exact_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0')))
}

/// A Cargo package or binary name, or a target triple: `[a-z0-9_-]`, `.` allowed in triples.
fn plain_name(s: &str, dot: bool) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || (dot && b == b'.'))
}

/// An absolute POSIX path of `[A-Za-z0-9._/-]`, without `.`/`..` or empty components.
fn fixed_prefix(s: &str) -> bool {
    s.len() > 1
        && s.starts_with('/')
        && !s.ends_with('/')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
        && s.split('/').skip(1).all(|c| !c.is_empty() && c != "." && c != "..")
}

fn toolchain_file(bytes: &[u8]) -> Result<(String, Vec<String>), ToolchainError> {
    const F: &str = RUST_TOOLCHAIN;
    let doc = parse(bytes, F, MAX_SETTINGS_BYTES)?;
    only(&doc, &["toolchain"], F, "")?;
    let tc = table(&doc, "toolchain", F, "")?;
    only(tc, &["channel", "components", "profile", "targets"], F, "toolchain.")?;
    let channel = string(tc, "channel", F, "toolchain.")?;
    if !exact_version(&channel) {
        return Err(ToolchainError::BadValue { file: F, key: "toolchain.channel".to_string() });
    }
    strings(tc, "components", F, "toolchain.")?;
    if string(tc, "profile", F, "toolchain.")? != "minimal" {
        return Err(ToolchainError::BadValue { file: F, key: "toolchain.profile".to_string() });
    }
    let targets = strings(tc, "targets", F, "toolchain.")?;
    Ok((channel, targets))
}

struct KernelBuild {
    commit_hash: String,
    llvm_version: String,
    package: String,
    bin: String,
    remap: BTreeMap<String, String>,
}

fn kernel_build_file(bytes: &[u8]) -> Result<KernelBuild, ToolchainError> {
    const F: &str = KERNEL_BUILD;
    let doc = parse(bytes, F, MAX_SETTINGS_BYTES)?;
    only(&doc, &["artifact", "remap", "schema", "toolchain"], F, "")?;
    if string(&doc, "schema", F, "")? != "1" {
        return Err(ToolchainError::BadValue { file: F, key: "schema".to_string() });
    }
    let tc = table(&doc, "toolchain", F, "")?;
    only(tc, &["commit_hash", "llvm_version"], F, "toolchain.")?;
    let commit_hash = string(tc, "commit_hash", F, "toolchain.")?;
    if commit_hash.len() != 40 || !commit_hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(ToolchainError::BadValue { file: F, key: "toolchain.commit_hash".to_string() });
    }
    let llvm_version = string(tc, "llvm_version", F, "toolchain.")?;
    if !llvm_version.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return Err(ToolchainError::BadValue { file: F, key: "toolchain.llvm_version".to_string() });
    }
    let art = table(&doc, "artifact", F, "")?;
    only(art, &["bin", "package"], F, "artifact.")?;
    let package = string(art, "package", F, "artifact.")?;
    let bin = string(art, "bin", F, "artifact.")?;
    for (k, v) in [("bin", &bin), ("package", &package)] {
        if !plain_name(v, false) {
            return Err(ToolchainError::BadValue { file: F, key: format!("artifact.{k}") });
        }
    }
    let rm = table(&doc, "remap", F, "")?;
    only(rm, &REMAP_KEYS, F, "remap.")?;
    let mut remap = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for k in REMAP_KEYS {
        let v = string(rm, k, F, "remap.")?;
        if !fixed_prefix(&v) || !seen.insert(v.clone()) {
            return Err(ToolchainError::BadValue { file: F, key: format!("remap.{k}") });
        }
        remap.insert(k.to_string(), v);
    }
    Ok(KernelBuild { commit_hash, llvm_version, package, bin, remap })
}

/// Checks the static rustflags against an allowlist: only `"-C", "target-cpu=<cpu>"` with a CPU
/// in [`ALLOWED_CPUS`] and `"-C", "target-feature=-<name>,.."` pairs. No feature may be enabled
/// (`+name` is refused whatever it is), so nothing FMA-capable can be switched on by a flag.
/// Returns the features they disable.
fn check_rustflags(flags: &[String], key: &str) -> Result<BTreeSet<String>, ToolchainError> {
    const F: &str = CARGO_CONFIG;
    let bad = || ToolchainError::BadValue { file: F, key: key.to_string() };
    if flags.len() % 2 != 0 {
        return Err(bad());
    }
    let mut disabled = BTreeSet::new();
    for pair in flags.chunks(2) {
        let (flag, opt) = match pair {
            [a, b] => (a.as_str(), b.as_str()),
            _ => return Err(bad()),
        };
        if flag != "-C" {
            return Err(bad());
        }
        let (k, v) = opt.split_once('=').ok_or_else(bad)?;
        match k {
            "target-cpu" => {
                if !ALLOWED_CPUS.contains(&v) {
                    return Err(bad());
                }
            }
            "target-feature" => {
                for feat in v.split(',') {
                    match feat.strip_prefix('-') {
                        Some(name) if plain_name(name, false) => {
                            disabled.insert(name.to_string());
                        }
                        _ => return Err(bad()),
                    }
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(disabled)
}

/// One profile value as `<type>:<value>`.
fn typed(v: &Value) -> Option<String> {
    match v {
        Value::Boolean(b) => Some(format!("bool:{b}")),
        Value::Integer(i) => Some(format!("int:{i}")),
        Value::String(s) => Some(format!("str:{s}")),
        _ => None,
    }
}

struct CargoConfig {
    target: String,
    rustflags: Vec<String>,
    disabled: BTreeSet<String>,
    profile: BTreeMap<String, String>,
}

fn cargo_config_file(bytes: &[u8]) -> Result<CargoConfig, ToolchainError> {
    const F: &str = CARGO_CONFIG;
    let doc = parse(bytes, F, MAX_SETTINGS_BYTES)?;
    only(&doc, &["build", "profile", "target"], F, "")?;
    let build = table(&doc, "build", F, "")?;
    only(build, &["target"], F, "build.")?;
    let target = string(build, "target", F, "build.")?;
    if !plain_name(&target, true) {
        return Err(ToolchainError::BadValue { file: F, key: "build.target".to_string() });
    }
    let targets = table(&doc, "target", F, "")?;
    only(targets, &[target.as_str()], F, "target.")?;
    let prefix = format!("target.{target}.");
    let tt = table(targets, &target, F, "target.")?;
    only(tt, &["rustflags"], F, &prefix)?;
    let key = format!("{prefix}rustflags");
    let rustflags = strings(tt, "rustflags", F, &prefix)?;
    let disabled = check_rustflags(&rustflags, &key)?;
    let profiles = table(&doc, "profile", F, "")?;
    only(profiles, &[PROFILE], F, "profile.")?;
    let pp = format!("profile.{PROFILE}.");
    let p = table(profiles, PROFILE, F, "profile.")?;
    only(p, &PROFILE_KEYS, F, &pp)?;
    let mut profile = BTreeMap::new();
    for k in PROFILE_KEYS {
        let v = p.get(k).ok_or_else(|| ToolchainError::Missing { file: F, key: format!("{pp}{k}") })?;
        let t = typed(v).ok_or_else(|| ToolchainError::BadValue { file: F, key: format!("{pp}{k}") })?;
        let required = match k {
            "codegen-units" => Some("int:1"),
            "incremental" => Some("bool:false"),
            "inherits" => Some("str:release"),
            "panic" => Some("str:abort"),
            _ => None,
        };
        if required.is_some_and(|r| r != t) {
            return Err(ToolchainError::BadValue { file: F, key: format!("{pp}{k}") });
        }
        profile.insert(k.to_string(), t);
    }
    Ok(CargoConfig { target, rustflags, disabled, profile })
}

fn workspace_files(manifest: &[u8], config: Option<&[u8]>) -> Result<(), ToolchainError> {
    let m = parse(manifest, WORKSPACE_MANIFEST, MAX_SETTINGS_BYTES)?;
    if let Some(Value::Table(profiles)) = m.get("profile") {
        if profiles.contains_key(PROFILE) {
            return Err(ToolchainError::WorkspaceOverride {
                file: WORKSPACE_MANIFEST,
                key: format!("profile.{PROFILE}"),
            });
        }
        if let Some(Value::Table(release)) = profiles.get("release") {
            for k in ["build-override", "package", "rustflags"] {
                if release.contains_key(k) {
                    return Err(ToolchainError::WorkspaceOverride {
                        file: WORKSPACE_MANIFEST,
                        key: format!("profile.release.{k}"),
                    });
                }
            }
        }
    }
    if let Some(bytes) = config {
        let c = parse(bytes, WORKSPACE_CONFIG, MAX_SETTINGS_BYTES)?;
        for k in c.keys() {
            if !matches!(k.as_str(), "alias" | "resolver") {
                return Err(ToolchainError::WorkspaceOverride { file: WORKSPACE_CONFIG, key: k.clone() });
            }
        }
    }
    Ok(())
}

/// Cargo reads `.cargo/config` and `.cargo/config.toml` in the directory a command starts in and
/// in every directory above it. For a build started in `<workspace>/kernel-build`, only
/// [`CARGO_CONFIG`] and [`WORKSPACE_CONFIG`] are checked by [`settings`]; this returns the first
/// other one that exists (a legacy `.cargo/config` next to either, or any file above the
/// workspace) as [`ToolchainError::StrayCargoConfig`]. `workspace` should be absolute.
pub fn stray_cargo_config(workspace: &std::path::Path) -> Result<(), ToolchainError> {
    let kb = workspace.join(SETTINGS_DIR);
    let allowed = [kb.join(".cargo").join("config.toml"), workspace.join(".cargo").join("config.toml")];
    let mut dir = Some(kb.as_path());
    while let Some(d) = dir {
        for name in ["config", "config.toml"] {
            let p = d.join(".cargo").join(name);
            if !allowed.contains(&p) && std::fs::symlink_metadata(&p).is_ok() {
                return Err(ToolchainError::StrayCargoConfig { path: p.display().to_string() });
            }
        }
        dir = d.parent();
    }
    Ok(())
}

/// Checks the committed settings (AC-4). Needs no toolchain.
pub fn settings(inputs: &Inputs<'_>) -> Result<Settings, ToolchainError> {
    let (channel, targets) = toolchain_file(inputs.rust_toolchain)?;
    let kb = kernel_build_file(inputs.kernel_build)?;
    let cc = cargo_config_file(inputs.cargo_config)?;
    if !targets.contains(&cc.target) {
        return Err(ToolchainError::BadValue { file: RUST_TOOLCHAIN, key: "toolchain.targets".to_string() });
    }
    workspace_files(inputs.workspace_manifest, inputs.workspace_config)?;
    if inputs.lockfile.len() > MAX_LOCKFILE_BYTES || inputs.lockfile.is_empty() {
        return Err(ToolchainError::BadFile { file: LOCKFILE });
    }
    Ok(Settings {
        channel,
        commit_hash: kb.commit_hash,
        llvm_version: kb.llvm_version,
        package: kb.package,
        bin: kb.bin,
        target: cc.target,
        rustflags: cc.rustflags,
        profile: cc.profile,
        remap: kb.remap,
        disabled_features: cc.disabled,
        lockfile: sha256_digest(inputs.lockfile),
    })
}

/// The `release`, `commit-hash` and `LLVM version` fields of `rustc -vV`.
fn rustc_version(text: &str) -> Result<(String, String, String), ToolchainError> {
    if text.len() > MAX_RUSTC_OUTPUT_BYTES {
        return Err(ToolchainError::BadRustcOutput { reason: "version_too_large" });
    }
    let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
    let mut lines = text.lines();
    match lines.next() {
        Some(first) if first.starts_with("rustc ") => {}
        _ => return Err(ToolchainError::BadRustcOutput { reason: "version_header" }),
    }
    for line in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once(": ").ok_or(ToolchainError::BadRustcOutput { reason: "version_line" })?;
        if fields.insert(k, v).is_some() {
            return Err(ToolchainError::BadRustcOutput { reason: "version_duplicate" });
        }
    }
    let get = |k: &str| fields.get(k).map(|v| v.to_string());
    match (get("release"), get("commit-hash"), get("LLVM version")) {
        (Some(r), Some(c), Some(l)) => Ok((r, c, l)),
        _ => Err(ToolchainError::BadRustcOutput { reason: "version_fields" }),
    }
}

/// The `target_feature` values of `rustc --print cfg`, sorted and unique.
fn target_features(text: &str) -> Result<BTreeSet<String>, ToolchainError> {
    if text.len() > MAX_RUSTC_OUTPUT_BYTES {
        return Err(ToolchainError::BadRustcOutput { reason: "cfg_too_large" });
    }
    let mut out = BTreeSet::new();
    let mut any = false;
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        any = true;
        let (name, value) = match line.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (line, None),
        };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(ToolchainError::BadRustcOutput { reason: "cfg_line" });
        }
        if let Some(v) = value {
            let inner = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .filter(|x| !x.contains('"') && !x.contains('\\'))
                .ok_or(ToolchainError::BadRustcOutput { reason: "cfg_line" })?;
            if name == "target_feature" {
                out.insert(inner.to_string());
            }
        }
    }
    if !any {
        return Err(ToolchainError::BadRustcOutput { reason: "cfg_empty" });
    }
    Ok(out)
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn obj(m: &BTreeMap<String, String>) -> Json {
    Json::Obj(m.iter().map(|(k, v)| (k.clone(), s(v))).collect())
}

/// Checks the settings against the toolchain's own output and returns the record and its digest.
/// `rustc_vv` is the output of `rustc -vV`; `rustc_cfg` that of `rustc` with
/// [`Settings::rustc_cfg_args`], both from the toolchain the build uses.
pub fn record(inputs: &Inputs<'_>, rustc_vv: &str, rustc_cfg: &str) -> Result<BuildRecord, ToolchainError> {
    let st = settings(inputs)?;
    let (release, commit, llvm) = rustc_version(rustc_vv)?;
    for (field, pinned, actual) in [
        ("release", &st.channel, release),
        ("commit-hash", &st.commit_hash, commit),
        ("LLVM version", &st.llvm_version, llvm),
    ] {
        if *pinned != actual {
            return Err(ToolchainError::ToolchainDrift { field, pinned: pinned.clone(), actual });
        }
    }
    let features = target_features(rustc_cfg)?;
    for f in FMA_FEATURES {
        if features.contains(f) {
            return Err(ToolchainError::FmaFeatureEnabled { feature: f.to_string() });
        }
    }
    if let Some(f) = st.disabled_features.iter().find(|f| features.contains(f.as_str())) {
        return Err(ToolchainError::FeatureNotDisabled { feature: f.clone() });
    }
    let doc = Json::Obj(vec![
        (
            "artifact".to_string(),
            Json::Obj(vec![
                ("bin".to_string(), s(&st.bin)),
                ("package".to_string(), s(&st.package)),
                ("profile".to_string(), s(PROFILE)),
                ("target".to_string(), s(&st.target)),
            ]),
        ),
        ("lockfile".to_string(), s(st.lockfile.as_str())),
        ("profile".to_string(), obj(&st.profile)),
        ("remap".to_string(), obj(&st.remap)),
        ("rustflags".to_string(), Json::Arr(st.rustflags.iter().map(|f| s(f)).collect())),
        ("schema".to_string(), s(SCHEMA)),
        ("target_features".to_string(), Json::Arr(features.iter().map(|f| s(f)).collect())),
        (
            "toolchain".to_string(),
            Json::Obj(vec![
                ("channel".to_string(), s(&st.channel)),
                ("commit_hash".to_string(), s(&st.commit_hash)),
                ("llvm_version".to_string(), s(&st.llvm_version)),
            ]),
        ),
    ]);
    let canonical = to_canonical(&doc).map_err(|_| ToolchainError::BadRustcOutput { reason: "canonical" })?;
    let digest = sha256_digest(&canonical);
    Ok(BuildRecord { canonical, digest })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn versions_and_names() {
        assert!(exact_version("1.89.0"));
        assert!(!exact_version("1.89"));
        assert!(!exact_version("1.089.0"));
        assert!(!exact_version("stable"));
        assert!(fixed_prefix("/fid/src"));
        assert!(!fixed_prefix("/fid/../src"));
        assert!(!fixed_prefix("fid/src"));
        assert!(!fixed_prefix("/fid/src/"));
        assert!(!fixed_prefix("/"));
        assert!(plain_name("wasm32-wasip1", true));
        assert!(!plain_name("wasm32.wasip1", false));
        assert!(!plain_name("Wasm", true));
    }

    #[test]
    fn rustflags_pairs() {
        let f = |v: &[&str]| check_rustflags(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "k");
        assert_eq!(
            f(&["-C", "target-cpu=generic", "-C", "target-feature=-simd128,-relaxed-simd"]).unwrap(),
            ["relaxed-simd", "simd128"].iter().map(|s| s.to_string()).collect()
        );
        assert!(f(&["-C", "target-cpu=native"]).is_err());
        assert!(f(&["-C", "target-feature=+relaxed-simd"]).is_err());
        assert!(f(&["-C", "target-feature=+bulk-memory"]).is_err());
        assert!(f(&["-C", "target-cpu=bleeding-edge"]).is_err());
        assert!(f(&["-C", "target-cpu=mvp"]).is_ok());
        assert!(f(&["-C", "target-feature=simd128"]).is_err());
        assert!(f(&["-C", "opt-level=3"]).is_err());
        assert!(f(&["-Ctarget-cpu=generic"]).is_err());
        assert!(f(&["-C"]).is_err());
    }

    #[test]
    fn cfg_lines() {
        let got = target_features("panic=\"abort\"\ntarget_arch=\"wasm32\"\ntarget_feature=\"sign-ext\"\nunix\ntarget_feature=\"bulk-memory\"\n").unwrap();
        assert_eq!(got.into_iter().collect::<Vec<_>>(), ["bulk-memory", "sign-ext"]);
        assert!(target_features("").is_err());
        assert!(target_features("target_feature=sign-ext\n").is_err());
    }
}
