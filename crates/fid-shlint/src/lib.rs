//! `fid-shlint`: shader manifest, readback lint and toolkit-free check for the renderer crate
//! (spec node `VIZ.RENDER.FRAME.SHLINT`, constitution rule C2). A CI tool in the tooling
//! layer: it reads files and `cargo metadata` output only (nearest class R0) and is
//! deterministic (D0: sorted directory walks, `BTreeMap`/`BTreeSet` only, no clock).
//!
//! Command: `fid-shlint --crate-dir <dir> --metadata <cargo-metadata.json> [--package <name>]`.
//!
//! Three checks run on the crate directory (the package defaults to `fid-render`):
//!
//! 1. **Manifest** (REQ-001): every `*.wgsl` file under the crate directory is listed in
//!    `shaders/manifest.toml`, and the resource bindings that the naga WGSL front end finds in
//!    each listed module match the manifest one to one (group, binding and name).
//! 2. **Readback** (REQ-002): every `map_async` call in non-test Rust source (`src/**.rs` and
//!    `build.rs`, minus items under `#[cfg(test)]`) that is not a write mapping must map a
//!    receiver whose name the manifest classifies as a colour, depth or ID target.
//!    Receivers are matched by their last identifier only (`self.frame.id_readback` is
//!    `id_readback`). wgpu readback helpers (`DownloadBuffer`, `read_buffer`) cannot be
//!    attributed and are always findings. Shader text that bypasses the manifest (inline
//!    `ShaderSource::Wgsl(..)` strings, other `ShaderSource` variants, SPIR-V macros, and
//!    `include_str!`/`include_bytes!` of anything but a listed `.wgsl` module) is a finding.
//! 3. **Toolkit** (REQ-003): the package must not reach, directly or transitively, a crate of
//!    the UI-toolkit denylist in the `cargo metadata` resolve graph. A crate named in a
//!    manifest `[[allow]]` entry is reported as an informational finding with its reason,
//!    never silently. The metadata must describe the crate under `--crate-dir`: the package's
//!    `manifest_path` must sit in that folder, or the run fails with exit 2.
//!
//! Every finding names its file and line, file, or crate path and the rule broken (REQ-004).
//! Exit 0: no findings other than informational ones; 1: findings; 2: anything that cannot be
//! read or parsed (fail closed), metadata for another crate, an input over its size limit, or
//! wrong usage.

pub mod deps;
pub mod manifest;
pub mod rust_scan;
pub mod wgsl;

pub use manifest::{BindingClass, ShaderManifest};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::Path;

/// The package checked when `--package` is not given.
pub const DEFAULT_PACKAGE: &str = "fid-render";
/// Location of the shader manifest, relative to the crate directory.
pub const MANIFEST_PATH: &str = "shaders/manifest.toml";
/// Largest accepted source file (WGSL, Rust or the manifest).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Largest accepted `cargo metadata` document.
pub const MAX_METADATA_BYTES: u64 = 64 * 1024 * 1024;
/// Most files (of any kind) one directory walk visits.
pub const MAX_FILES: usize = 20_000;
/// Deepest folder nesting followed below the crate directory.
pub const MAX_DEPTH: usize = 32;

const USAGE: &str = "usage: fid-shlint --crate-dir <dir> --metadata <cargo-metadata.json> [--package <name>]";

/// The rule a finding breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// A WGSL module is not listed in the manifest (REQ-001).
    UnlistedModule,
    /// The manifest lists a module that does not exist (REQ-001).
    MissingModule,
    /// A WGSL binding is not listed in the manifest (REQ-001).
    UnlistedBinding,
    /// The manifest lists a binding the WGSL module does not declare (REQ-001).
    StaleBinding,
    /// A binding has a different name in WGSL and in the manifest (REQ-001).
    BindingName,
    /// Non-test code maps something other than a colour, depth or ID target for reading (REQ-002).
    Readback,
    /// A `map_async` call whose mapped target cannot be determined (REQ-002, fail closed).
    UnresolvedReadback,
    /// Shader text that does not come from a listed `.wgsl` module (REQ-001, fail closed).
    UnmanifestedShader,
    /// The package reaches a UI-toolkit crate (REQ-003).
    Toolkit,
    /// The package reaches a UI-toolkit crate that the manifest allows (informational).
    ToolkitAllowed,
}

impl Rule {
    /// Stable rule identifier printed with every finding.
    pub fn id(self) -> &'static str {
        match self {
            Rule::UnlistedModule => "SHLINT-001/unlisted-module",
            Rule::MissingModule => "SHLINT-001/missing-module",
            Rule::UnlistedBinding => "SHLINT-001/unlisted-binding",
            Rule::StaleBinding => "SHLINT-001/stale-binding",
            Rule::BindingName => "SHLINT-001/binding-name",
            Rule::Readback => "SHLINT-002/readback",
            Rule::UnresolvedReadback => "SHLINT-002/unresolved-readback",
            Rule::UnmanifestedShader => "SHLINT-001/unmanifested-shader",
            Rule::Toolkit => "SHLINT-003/toolkit",
            Rule::ToolkitAllowed => "SHLINT-003/toolkit-allowed",
        }
    }

    /// False only for informational rules, which never make the run fail.
    pub fn is_error(self) -> bool {
        !matches!(self, Rule::ToolkitAllowed)
    }
}

/// One lint finding: where, which rule, and what.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// `path`, `path:line` (paths relative to the crate directory, `/` separators) or a crate
    /// path such as `fid-render -> a -> winit`.
    pub location: String,
    pub rule: Rule,
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: [{}] {}", self.location, self.rule.id(), self.message)
    }
}

/// Errors; each makes the tool exit 2 (fail closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShlintError {
    /// Wrong command-line usage.
    Usage(String),
    /// `shaders/manifest.toml` is missing, not TOML, or breaks the manifest schema.
    ManifestMalformed { reason: String },
    /// The `cargo metadata` document cannot be used.
    MetadataMalformed { reason: String },
    /// The metadata describes a package whose `manifest_path` is not in `--crate-dir`.
    MetadataMismatch { reason: String },
    /// A file or folder cannot be read, is not UTF-8, or is a symbolic link.
    Unreadable { path: String, reason: String },
    /// A file is larger than its limit, or a walk exceeds [`MAX_FILES`] or [`MAX_DEPTH`].
    Limit { path: String, reason: String },
    /// A listed WGSL module cannot be parsed.
    WgslParse { path: String, line: Option<u32>, reason: String },
    /// A Rust source file cannot be tokenised or its brackets do not balance.
    RustParse { path: String, line: u32, reason: String },
}

impl ShlintError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ShlintError::Usage(_) => "Usage",
            ShlintError::ManifestMalformed { .. } => "ManifestMalformed",
            ShlintError::MetadataMalformed { .. } => "MetadataMalformed",
            ShlintError::MetadataMismatch { .. } => "MetadataMismatch",
            ShlintError::Unreadable { .. } => "Unreadable",
            ShlintError::Limit { .. } => "Limit",
            ShlintError::WgslParse { .. } => "WgslParse",
            ShlintError::RustParse { .. } => "RustParse",
        }
    }
}

impl fmt::Display for ShlintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShlintError::Usage(m) => write!(f, "Usage: {m}\n{USAGE}"),
            ShlintError::ManifestMalformed { reason } => write!(f, "ManifestMalformed: {MANIFEST_PATH}: {reason}"),
            ShlintError::MetadataMalformed { reason } => write!(f, "MetadataMalformed: {reason}"),
            ShlintError::MetadataMismatch { reason } => write!(f, "MetadataMismatch: {reason}"),
            ShlintError::Unreadable { path, reason } => write!(f, "Unreadable: {path}: {reason}"),
            ShlintError::Limit { path, reason } => write!(f, "Limit: {path}: {reason}"),
            ShlintError::WgslParse { path, line: Some(l), reason } => write!(f, "WgslParse: {path}:{l}: {reason}"),
            ShlintError::WgslParse { path, line: None, reason } => write!(f, "WgslParse: {path}: {reason}"),
            ShlintError::RustParse { path, line, reason } => write!(f, "RustParse: {path}:{line}: {reason}"),
        }
    }
}

impl std::error::Error for ShlintError {}

/// What a run prints and returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the command line (arguments without the program name).
pub fn run(args: &[String]) -> Output {
    match run_inner(args) {
        Ok(findings) => {
            let errors = findings.iter().filter(|f| f.rule.is_error()).count();
            let info = findings.len() - errors;
            let mut stdout = String::new();
            for f in &findings {
                stdout.push_str(&f.to_string());
                stdout.push('\n');
            }
            let summary = match (errors, info) {
                (0, 0) => "fid-shlint: ok".to_string(),
                (0, i) => format!("fid-shlint: ok ({i} informational)"),
                (e, 0) => format!("fid-shlint: {e} finding(s)"),
                (e, i) => format!("fid-shlint: {e} finding(s), {i} informational"),
            };
            stdout.push_str(&summary);
            stdout.push('\n');
            Output { code: u8::from(errors > 0), stdout, stderr: String::new() }
        }
        Err(e) => Output { code: 2, stdout: String::new(), stderr: format!("fid-shlint: error: {e}\n") },
    }
}

fn run_inner(args: &[String]) -> Result<Vec<Finding>, ShlintError> {
    let mut crate_dir: Option<&str> = None;
    let mut metadata: Option<&str> = None;
    let mut package: Option<&str> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let slot = match a.as_str() {
            "--crate-dir" => &mut crate_dir,
            "--metadata" => &mut metadata,
            "--package" => &mut package,
            other => return Err(ShlintError::Usage(format!("unknown argument `{other}`"))),
        };
        if slot.is_some() {
            return Err(ShlintError::Usage(format!("{a} given twice")));
        }
        let Some(v) = it.next() else {
            return Err(ShlintError::Usage(format!("{a} needs a value")));
        };
        *slot = Some(v.as_str());
    }
    let crate_dir = crate_dir.ok_or_else(|| ShlintError::Usage("--crate-dir is required".to_string()))?;
    let metadata = metadata.ok_or_else(|| ShlintError::Usage("--metadata is required".to_string()))?;
    let package = package.unwrap_or(DEFAULT_PACKAGE);
    let meta_path = Path::new(metadata);
    let meta_bytes = read_bounded(meta_path, metadata, MAX_METADATA_BYTES)?;
    let meta_dir = match meta_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    check(Path::new(crate_dir), &meta_bytes, meta_dir, package)
}

/// Runs all three checks on a crate directory and a `cargo metadata` document. `metadata_dir`
/// is the folder of the metadata file: a relative `manifest_path` (only in hand-written
/// fixtures; cargo always writes absolute paths) is resolved against it. Findings are sorted
/// and free of duplicates.
pub fn check(
    crate_dir: &Path,
    metadata: &[u8],
    metadata_dir: &Path,
    package: &str,
) -> Result<Vec<Finding>, ShlintError> {
    let manifest_bytes = read_bounded(&crate_dir.join(MANIFEST_PATH), MANIFEST_PATH, MAX_FILE_BYTES)
        .map_err(|e| ShlintError::ManifestMalformed { reason: e.to_string() })?;
    let manifest_text =
        utf8(MANIFEST_PATH, manifest_bytes).map_err(|e| ShlintError::ManifestMalformed { reason: e.to_string() })?;
    let manifest = ShaderManifest::parse(&manifest_text).map_err(|reason| ShlintError::ManifestMalformed { reason })?;

    // REQ-003 first: metadata for another crate stops the run before anything is reported.
    let report = deps::check(metadata, package, &manifest.allows)?;
    same_crate(crate_dir, metadata_dir, &report.manifest_path)?;

    let files = walk(crate_dir)?;
    let mut wgsl_files = Vec::new();
    let mut rust_files = Vec::new();
    for rel in files {
        if rel.ends_with(".wgsl") {
            wgsl_files.push(rel);
        } else if rel.ends_with(".rs") && (rel.starts_with("src/") || rel == "build.rs") {
            rust_files.push(rel);
        }
    }

    let mut findings = BTreeSet::new();

    // REQ-001: only listed modules are parsed; an unlisted module is a finding on its own.
    let mut sources = Vec::new();
    for rel in &wgsl_files {
        if manifest.module(rel).is_some() {
            let bytes = read_bounded(&crate_dir.join(rel), rel, MAX_FILE_BYTES)?;
            sources.push((rel.clone(), utf8(rel, bytes)?));
        }
    }
    findings.extend(check_shaders(&manifest, &wgsl_files, &sources)?);

    // REQ-002.
    for rel in &rust_files {
        let bytes = read_bounded(&crate_dir.join(rel), rel, MAX_FILE_BYTES)?;
        let text = utf8(rel, bytes)?;
        findings.extend(rust_scan::scan(rel, &text, &manifest)?);
        findings.extend(rust_scan::scan_shader_sources(rel, &text, &manifest)?);
    }

    findings.extend(report.findings);
    Ok(findings.into_iter().collect())
}

/// Fails unless the folder holding `manifest_path` is `crate_dir` (both canonicalised).
fn same_crate(crate_dir: &Path, metadata_dir: &Path, manifest_path: &str) -> Result<(), ShlintError> {
    let mismatch = |reason: String| ShlintError::MetadataMismatch { reason };
    let mp = Path::new(manifest_path);
    let mp = if mp.is_absolute() { mp.to_path_buf() } else { metadata_dir.join(mp) };
    let Some(pkg_dir) = mp.parent() else {
        return Err(mismatch(format!("manifest_path {manifest_path} has no parent folder")));
    };
    let pkg_dir = std::fs::canonicalize(pkg_dir)
        .map_err(|e| mismatch(format!("cannot resolve the folder of manifest_path {manifest_path}: {e}")))?;
    let dir = std::fs::canonicalize(crate_dir)
        .map_err(|e| ShlintError::Unreadable { path: crate_dir.display().to_string(), reason: e.to_string() })?;
    if pkg_dir != dir {
        return Err(mismatch(format!("manifest_path {manifest_path} is not in --crate-dir {}", crate_dir.display())));
    }
    Ok(())
}

/// REQ-001 on in-memory data: `wgsl_files` are all WGSL paths found in the crate, `sources`
/// the text of the listed ones.
pub fn check_shaders(
    manifest: &ShaderManifest,
    wgsl_files: &[String],
    sources: &[(String, String)],
) -> Result<Vec<Finding>, ShlintError> {
    let mut findings = Vec::new();
    let present: BTreeSet<&str> = wgsl_files.iter().map(String::as_str).collect();
    for rel in wgsl_files {
        if manifest.module(rel).is_none() {
            findings.push(Finding {
                location: rel.clone(),
                rule: Rule::UnlistedModule,
                message: format!("WGSL module is not listed in {MANIFEST_PATH}"),
            });
        }
    }
    for m in &manifest.modules {
        if !present.contains(m.path.as_str()) {
            findings.push(Finding {
                location: MANIFEST_PATH.to_string(),
                rule: Rule::MissingModule,
                message: format!("lists module {}, which does not exist", m.path),
            });
        }
    }
    for (rel, text) in sources {
        let Some(module) = manifest.module(rel) else {
            continue;
        };
        let found = wgsl::bindings(text).map_err(|(line, reason)| ShlintError::WgslParse {
            path: rel.clone(),
            line,
            reason,
        })?;
        let listed: BTreeMap<(u32, u32), &str> =
            module.bindings.iter().map(|b| ((b.group, b.binding), b.name.as_str())).collect();
        let mut seen = BTreeSet::new();
        for b in &found {
            let name = b.name.as_deref().unwrap_or("<unnamed>");
            seen.insert((b.group, b.binding));
            match listed.get(&(b.group, b.binding)) {
                None => findings.push(Finding {
                    location: rel.clone(),
                    rule: Rule::UnlistedBinding,
                    message: format!(
                        "binding @group({}) @binding({}) `{name}` is not listed in {MANIFEST_PATH}",
                        b.group, b.binding
                    ),
                }),
                Some(listed_name) if *listed_name != name => findings.push(Finding {
                    location: rel.clone(),
                    rule: Rule::BindingName,
                    message: format!(
                        "binding @group({}) @binding({}) is `{name}` in WGSL but `{listed_name}` in {MANIFEST_PATH}",
                        b.group, b.binding
                    ),
                }),
                Some(_) => {}
            }
        }
        for b in &module.bindings {
            if !seen.contains(&(b.group, b.binding)) {
                findings.push(Finding {
                    location: MANIFEST_PATH.to_string(),
                    rule: Rule::StaleBinding,
                    message: format!(
                        "module {rel} lists @group({}) @binding({}) `{}`, which the WGSL does not declare",
                        b.group, b.binding, b.name
                    ),
                });
            }
        }
    }
    Ok(findings)
}

/// Reads at most `max` bytes; a longer file is a [`ShlintError::Limit`].
fn read_bounded(path: &Path, shown: &str, max: u64) -> Result<Vec<u8>, ShlintError> {
    let unreadable = |e: std::io::Error| ShlintError::Unreadable { path: shown.to_string(), reason: e.to_string() };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut buf = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut buf).map_err(unreadable)?;
    if buf.len() as u64 > max {
        return Err(ShlintError::Limit { path: shown.to_string(), reason: format!("larger than {max} bytes") });
    }
    Ok(buf)
}

fn utf8(shown: &str, bytes: Vec<u8>) -> Result<String, ShlintError> {
    String::from_utf8(bytes)
        .map_err(|_| ShlintError::Unreadable { path: shown.to_string(), reason: "not valid UTF-8".to_string() })
}

/// All regular files below `root` as sorted relative paths with `/` separators. The top-level
/// `target` and `.git` folders are skipped; a symbolic link or a non-UTF-8 name fails closed.
pub fn walk(root: &Path) -> Result<Vec<String>, ShlintError> {
    let mut out = Vec::new();
    let mut visited = 0usize;
    walk_dir(root, "", 0, &mut visited, &mut out)?;
    Ok(out)
}

fn walk_dir(
    dir: &Path,
    prefix: &str,
    depth: usize,
    visited: &mut usize,
    out: &mut Vec<String>,
) -> Result<(), ShlintError> {
    let shown = if prefix.is_empty() { ".".to_string() } else { prefix.to_string() };
    if depth > MAX_DEPTH {
        return Err(ShlintError::Limit { path: shown, reason: format!("nested deeper than {MAX_DEPTH} folders") });
    }
    let unreadable = |e: std::io::Error| ShlintError::Unreadable { path: shown.clone(), reason: e.to_string() };
    let mut entries = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        *visited += 1;
        if *visited > MAX_FILES {
            return Err(ShlintError::Limit { path: shown.clone(), reason: format!("more than {MAX_FILES} entries") });
        }
        let name = entry.file_name().into_string().map_err(|_| ShlintError::Unreadable {
            path: shown.clone(),
            reason: "an entry name is not valid UTF-8".to_string(),
        })?;
        let kind = entry.file_type().map_err(unreadable)?;
        entries.insert(name, kind);
    }
    for (name, kind) in entries {
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if kind.is_symlink() {
            return Err(ShlintError::Unreadable { path: rel, reason: "symbolic links are not followed".to_string() });
        }
        if kind.is_dir() {
            if depth == 0 && (name == "target" || name == ".git") {
                continue;
            }
            walk_dir(&dir.join(&name), &rel, depth + 1, visited, out)?;
        } else if kind.is_file() {
            out.push(rel);
        } else {
            return Err(ShlintError::Unreadable { path: rel, reason: "not a regular file or folder".to_string() });
        }
    }
    Ok(())
}
