//! Kernel build-configuration and emitted-code FMA check. Spec node `KRN.NUM.LINT.FLAGS`.
//!
//! [`run`] reads the `[flags]` table of `knp1-lint.toml` and checks every place the kernel
//! build takes flags from, emitting [`Finding`]s of the `KNP1-FLAG-*` family:
//!
//! | Rule | Fires when |
//! |---|---|
//! | `KNP1-FLAG-NATIVE` | a flag source sets `-C target-cpu=native` |
//! | `KNP1-FLAG-CPU` | a flag source sets any other `-C target-cpu` not in `allowed_target_cpus`; a CPU on the built-in FMA list ([`BUILTIN_FMA_CPUS`]) is `KNP1-FLAG-FMA` instead |
//! | `KNP1-FLAG-FMA` | a flag source enables `+fma`, `+relaxed-simd` or a configured `fma_features` entry, or passes `fp-contract` other than `off` to LLVM |
//! | `KNP1-FLAG-CGU` | a kernel profile resolves (Cargo inheritance, built-in defaults) to `codegen-units` other than 1, or a flag source sets it |
//! | `KNP1-FLAG-TOOLCHAIN` | `rust-toolchain.toml` is missing, has no exact `channel = "X.Y.Z"`, or no `components` pin; a legacy `rust-toolchain` file exists; a CI env file sets `RUSTUP_TOOLCHAIN` or runs `cargo +<toolchain>` |
//! | `KNP1-FLAG-LTO` | two kernel profiles resolve to different `opt-level`, `lto`, `debug-assertions`, `overflow-checks` or `panic`, and the key is not in `allowed_profile_differences` |
//! | `KNP1-FLAG-KEYWORD` | the fail-closed substring gate ([`keywords`]) hits a CI line or a free-form `.cargo/config` value the structured checks do not explain |
//! | `KNP1-FLAG-AMBIGUOUS` | both `.cargo/config` and `.cargo/config.toml` exist |
//! | `KNP1-FLAG-OPTOUT` | informational note (in [`Outcome::notes`], never a failure): a line opted out with `knp1-flags: allow <RULE> <reason>` |
//! | `KNP1-FLAG-WASMFMA` | the wasm reference module contains a relaxed multiply-add opcode |
//! | `KNP1-FLAG-WASMSIMD` | the wasm reference module contains a SIMD opcode the config does not allow |
//! | `KNP1-FLAG-WASMPARSE` | the wasm reference module is missing or cannot be parsed (fatal finding) |
//!
//! Flag sources: every key named `rustflags`/`rustdocflags` (any case) or ending in
//! `RUSTFLAGS`/`RUSTDOCFLAGS` anywhere in `.cargo/config.toml` (and the legacy `.cargo/config`)
//! and in the workspace `Cargo.toml` (`[profile.*] rustflags`), so `build.rustflags`,
//! `target.<triple>.rustflags`, `target.'cfg(..)'.rustflags`, `host.rustflags` and
//! `env.RUSTFLAGS`/`env.CARGO_ENCODED_RUSTFLAGS` are all covered; and the whole text of each
//! `ci_env_files` entry, not only `RUSTFLAGS:` lines. CI files are first grouped into logical
//! lines ([`ci_spans`]): a line ending in `\` continues on the next, and a line with a non-empty
//! YAML value (a block scalar `|`/`>-` or a plain multi-line scalar) continues over the
//! following more-indented lines, so a `-C` split across lines is still paired with its value.
//! Comment lines are skipped. `CARGO_PROFILE_<NAME>_CODEGEN_UNITS` and
//! `--config profile.<name>.codegen-units=N` are checked for kernel profiles, and
//! `--config build.rustflags=…` goes through the same flag checks.
//! Profiles come from `[profile.*]`
//! of the workspace `Cargo.toml`, overridden key by key by `[profile.*]` of `.cargo/config.toml`;
//! package-level `codegen-units` overrides are checked along the whole `inherits` chain.
//!
//! Every flag source goes through one normaliser, [`tokens`] then [`codegen_options`]:
//! the unit separator `0x1F` and its escapes (`\x1f`, `\u001f`, `\U0000001f`, octal `\037`)
//! become spaces; text is split on whitespace and on the shell/YAML/TOML separators
//! `" ' [ ] { } ( ) ; & | < >` and backtick; commas around tokens (array syntax) are dropped;
//! an assignment prefix (`RUSTFLAGS=-C…`, `export RUSTFLAGS=…`, `build.rustflags=-C…`) is split
//! off when what follows is a flag. `-C k=v`, `-Ck=v`, `--codegen k=v` and `--codegen=k=v`
//! are recognised and option names are normalised with `_` → `-` as rustc does
//! (`-C target_cpu=native` is `target-cpu`). Any other token that mentions `fp-contract` is
//! treated as an LLVM argument.
//!
//! Findings carry crate `(build-config)` version `-`, the file relative to the workspace
//! directory, and the 1-based line and column of the offending text (the section header or
//! line 1 when nothing more precise exists). The excerpt digest is SHA-256 of that line;
//! for a missing file, of the message; for a wasm opcode, of the instruction bytes. Wasm
//! findings are at line 1, column = byte offset + 1. Output is sorted (D0).
//!
//! Waivers: `(build-config)` has no [`crate::findings::SourceTree`] mapping, so a waiver naming
//! it cannot be checked against current source and `apply_waivers` reports it stale
//! (`KNP1-WAIVER-STALE`, fail closed). `KNP1-FLAG-WASM*` findings are unwaivable outright
//! ([`crate::findings::Rule::is_waivable`]): they all sit at line 1 of a binary module, so one
//! waiver would hide every future hit.
//!
//! Config (`[flags]`; every path relative to the config file's directory):
//!
//! ```toml
//! [flags]
//! workspace = "."                         # holds rust-toolchain.toml, .cargo/, Cargo.toml
//! profiles = ["release"]                  # kernel profiles (default ["release"])
//! ci_env_files = [".github/workflows/rust.yml"]
//! fma_features = ["fma4"]                 # extra FMA-enabling target features
//! wasm_module = "target/kernel.wasm"      # optional; when set the file must exist
//! allowed_simd = [12, 240]                # optional; 0xFD sub-opcodes allowed in the module
//! allowed_profile_differences = ["debug-assertions"]
//! allowed_target_cpus = ["x86-64"]        # target-cpu values allowed (default none)
//! ```

pub mod keywords;
pub mod wasm;

use crate::findings::{input_digest, sort_findings, Finding, Rule};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

/// Crate name written into every finding of this module.
pub const BUILD_CONFIG_CRATE: &str = "(build-config)";
/// Crate version written into every finding of this module.
pub const BUILD_CONFIG_VERSION: &str = "-";
/// Largest TOML or CI file read, in bytes.
pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Longest `inherits` chain followed.
pub const MAX_INHERIT_DEPTH: usize = 64;
/// Target features that always count as FMA-enabling.
pub const BUILTIN_FMA_FEATURES: [&str; 3] = ["fma", "fma4", "relaxed-simd"];
/// Target-feature prefixes that always count as FMA-enabling: every AVX-512 and AVX10 feature
/// implies AVX512F, which implies FMA in LLVM's x86 feature graph [unverified per LLVM version].
pub const BUILTIN_FMA_FEATURE_PREFIXES: [&str; 2] = ["avx512", "avx10"];
/// `target-cpu` names that enable FMA (x86) or name a core whose ISA always has fused
/// multiply-add (AArch64). Sources are listed in the leaf's NODE.md [unverified per LLVM version].
pub const BUILTIN_FMA_CPUS: [&str; 42] = [
    "x86-64-v3",
    "x86-64-v4",
    "core-avx2",
    "haswell",
    "broadwell",
    "skylake",
    "skylake-avx512",
    "skx",
    "cascadelake",
    "cooperlake",
    "cannonlake",
    "icelake-client",
    "icelake-server",
    "rocketlake",
    "tigerlake",
    "sapphirerapids",
    "emeraldrapids",
    "graniterapids",
    "graniterapids-d",
    "alderlake",
    "raptorlake",
    "meteorlake",
    "arrowlake",
    "arrowlake-s",
    "lunarlake",
    "pantherlake",
    "gracemont",
    "sierraforest",
    "grandridge",
    "clearwaterforest",
    "knl",
    "knm",
    "bdver1",
    "bdver2",
    "bdver3",
    "bdver4",
    "a64fx",
    "carmel",
    "thunderx2t99",
    "thunderx3t110",
    "ampere1",
    "ampere1a",
];
/// `target-cpu` prefixes that enable FMA: AMD Zen (`znver*`) and AArch64 core families.
pub const BUILTIN_FMA_CPU_PREFIXES: [&str; 6] = ["znver", "apple-", "neoverse-", "cortex-a", "cortex-x", "exynos-"];

/// True when `cpu` is a built-in FMA-implying CPU.
pub fn is_fma_cpu(cpu: &str) -> bool {
    BUILTIN_FMA_CPUS.contains(&cpu) || BUILTIN_FMA_CPU_PREFIXES.iter().any(|p| cpu.starts_with(p))
}

/// True when `feature` (without `+`) is a built-in FMA-enabling target feature.
pub fn is_builtin_fma_feature(feature: &str) -> bool {
    BUILTIN_FMA_FEATURES.contains(&feature) || BUILTIN_FMA_FEATURE_PREFIXES.iter().any(|p| feature.starts_with(p))
}
/// Profile keys compared between kernel profiles by `KNP1-FLAG-LTO`.
pub const COMPARED_PROFILE_KEYS: [&str; 5] = ["debug-assertions", "lto", "opt-level", "overflow-checks", "panic"];

// ------------------------------------------------------------------ errors

/// Fatal error codes of this module (the run produces no findings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagsErrorCode {
    /// The config, a TOML input or a CI env file is missing, malformed or too large.
    ConfigInvalid,
}

impl FlagsErrorCode {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            FlagsErrorCode::ConfigInvalid => "KNP1-CONFIG-INVALID",
        }
    }
}

/// A fatal error of the flags check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagsError {
    pub code: FlagsErrorCode,
    pub message: String,
}

impl fmt::Display for FlagsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for FlagsError {}

fn invalid(m: impl Into<String>) -> FlagsError {
    FlagsError { code: FlagsErrorCode::ConfigInvalid, message: m.into() }
}

// ------------------------------------------------------------------ paths and files

fn norm(p: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(c),
            },
            other => out.push(other),
        }
    }
    out.iter().collect()
}

fn rel_slash(p: &Path, base: &Path) -> Option<String> {
    let r = p.strip_prefix(base).ok()?;
    let parts: Option<Vec<&str>> = r.components().map(|c| c.as_os_str().to_str()).collect();
    let parts = parts?;
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, FlagsError> {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(invalid(format!("{}: {e}", path.display()))),
    };
    let mut buf = Vec::new();
    f.take(limit + 1).read_to_end(&mut buf).map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    if buf.len() as u64 > limit {
        return Err(invalid(format!("{}: larger than {limit} bytes", path.display())));
    }
    Ok(Some(buf))
}

fn read_text(path: &Path, shown: &str) -> Result<Option<String>, FlagsError> {
    match read_bounded(path, MAX_INPUT_BYTES)? {
        None => Ok(None),
        Some(b) => String::from_utf8(b).map(Some).map_err(|_| invalid(format!("{shown}: not UTF-8"))),
    }
}

fn parse_toml(text: &str, shown: &str) -> Result<toml::Table, FlagsError> {
    text.parse().map_err(|e: toml::de::Error| invalid(format!("{shown}: {}", e.to_string().trim_end())))
}

// ------------------------------------------------------------------ config

/// A file inside the workspace: absolute-ish path and its `/`-separated workspace-relative name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsFile {
    pub path: PathBuf,
    pub rel: String,
}

/// The `[flags]` table of `knp1-lint.toml`, with paths resolved against the config directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub workspace: PathBuf,
    /// Kernel profile names, sorted and de-duplicated.
    pub profiles: Vec<String>,
    pub ci_env_files: Vec<WsFile>,
    /// Extra FMA-enabling target features, without a leading `+`, sorted.
    pub fma_features: Vec<String>,
    pub wasm_module: Option<WsFile>,
    pub simd_policy: wasm::SimdPolicy,
    pub allowed_profile_differences: Vec<String>,
    /// `-C target-cpu` values that are allowed (never `native`), sorted.
    pub allowed_target_cpus: Vec<String>,
    /// FMA-implying CPUs explicitly allowed anyway: CPU name -> reason.
    pub fma_cpu_overrides: BTreeMap<String, String>,
}

const CONFIG_KEYS: [&str; 9] = [
    "workspace",
    "profiles",
    "ci_env_files",
    "fma_features",
    "wasm_module",
    "allowed_simd",
    "allowed_profile_differences",
    "allowed_target_cpus",
    "fma_cpu_overrides",
];

fn str_list(v: &toml::Value, what: &str) -> Result<Vec<String>, FlagsError> {
    let arr = v.as_array().ok_or_else(|| invalid(format!("knp1-lint.toml: {what} must be an array of strings")))?;
    arr.iter()
        .map(|x| match x.as_str() {
            Some(s) if !s.is_empty() => Ok(s.to_string()),
            _ => Err(invalid(format!("knp1-lint.toml: {what} must hold non-empty strings"))),
        })
        .collect()
}

fn ws_file(config_dir: &Path, workspace: &Path, s: &str, what: &str) -> Result<WsFile, FlagsError> {
    let path = norm(&config_dir.join(s));
    let rel = rel_slash(&path, workspace)
        .ok_or_else(|| invalid(format!("knp1-lint.toml: {what} {s:?} is not inside the workspace directory")))?;
    Ok(WsFile { path, rel })
}

/// Parses the config text. Other top-level tables belong to other lint modules and are ignored;
/// unknown keys inside `[flags]` are rejected.
pub fn parse_config(text: &str, config_dir: &Path) -> Result<Config, FlagsError> {
    let table = parse_toml(text, "knp1-lint.toml")?;
    let Some(toml::Value::Table(t)) = table.get("flags") else {
        return Err(invalid("knp1-lint.toml: missing [flags] table"));
    };
    for k in t.keys() {
        if !CONFIG_KEYS.contains(&k.as_str()) {
            return Err(invalid(format!("knp1-lint.toml: unknown key flags.{k}")));
        }
    }
    let workspace = match t.get("workspace") {
        None => norm(config_dir),
        Some(toml::Value::String(s)) if !s.is_empty() => norm(&config_dir.join(s)),
        Some(_) => return Err(invalid("knp1-lint.toml: flags.workspace must be a non-empty string")),
    };
    let profiles: BTreeSet<String> = match t.get("profiles") {
        None => std::iter::once("release".to_string()).collect(),
        Some(v) => str_list(v, "flags.profiles")?.into_iter().collect(),
    };
    if profiles.is_empty() {
        return Err(invalid("knp1-lint.toml: flags.profiles must name at least one profile"));
    }
    let ci_env_files = match t.get("ci_env_files") {
        None => Vec::new(),
        Some(v) => str_list(v, "flags.ci_env_files")?
            .iter()
            .map(|s| ws_file(config_dir, &workspace, s, "flags.ci_env_files entry"))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let fma_features: BTreeSet<String> = match t.get("fma_features") {
        None => BTreeSet::new(),
        Some(v) => str_list(v, "flags.fma_features")?.iter().map(|s| s.trim_start_matches('+').to_string()).collect(),
    };
    let wasm_module = match t.get("wasm_module") {
        None => None,
        Some(toml::Value::String(s)) if !s.is_empty() => Some(ws_file(config_dir, &workspace, s, "flags.wasm_module")?),
        Some(_) => return Err(invalid("knp1-lint.toml: flags.wasm_module must be a non-empty string")),
    };
    let simd_policy = match t.get("allowed_simd") {
        None => wasm::SimdPolicy::Default,
        Some(toml::Value::Array(a)) => {
            let mut list = Vec::new();
            for x in a {
                match x.as_integer().and_then(|i| u32::try_from(i).ok()) {
                    Some(i) => list.push(i),
                    None => {
                        return Err(invalid("knp1-lint.toml: flags.allowed_simd must hold 0xFD sub-opcodes (integers)"))
                    }
                }
            }
            list.sort_unstable();
            list.dedup();
            wasm::SimdPolicy::Only(list)
        }
        Some(_) => return Err(invalid("knp1-lint.toml: flags.allowed_simd must be an array of integers")),
    };
    let allowed: BTreeSet<String> = match t.get("allowed_profile_differences") {
        None => BTreeSet::new(),
        Some(v) => str_list(v, "flags.allowed_profile_differences")?.into_iter().collect(),
    };
    if let Some(bad) = allowed.iter().find(|k| !COMPARED_PROFILE_KEYS.contains(&k.as_str())) {
        return Err(invalid(format!(
            "knp1-lint.toml: flags.allowed_profile_differences entry {bad:?} is not one of {}",
            COMPARED_PROFILE_KEYS.join(", ")
        )));
    }
    let allowed_target_cpus: BTreeSet<String> = match t.get("allowed_target_cpus") {
        None => BTreeSet::new(),
        Some(v) => str_list(v, "flags.allowed_target_cpus")?.into_iter().collect(),
    };
    if allowed_target_cpus.contains("native") {
        return Err(invalid("knp1-lint.toml: flags.allowed_target_cpus may not contain \"native\""));
    }
    let mut fma_cpu_overrides = BTreeMap::new();
    match t.get("fma_cpu_overrides") {
        None => {}
        Some(toml::Value::Table(o)) => {
            for (cpu, why) in o {
                match why.as_str().map(str::trim) {
                    Some(r) if !r.is_empty() => {
                        fma_cpu_overrides.insert(cpu.clone(), r.to_string());
                    }
                    _ => {
                        return Err(invalid(format!(
                            "knp1-lint.toml: flags.fma_cpu_overrides.{cpu} must be a non-empty reason"
                        )))
                    }
                }
            }
        }
        Some(_) => return Err(invalid("knp1-lint.toml: flags.fma_cpu_overrides must be a table cpu = \"reason\"")),
    }
    for cpu in &allowed_target_cpus {
        if is_fma_cpu(cpu) && !fma_cpu_overrides.contains_key(cpu) {
            return Err(invalid(format!(
                "knp1-lint.toml: flags.allowed_target_cpus lists {cpu:?}, which implies FMA; \
                 add flags.fma_cpu_overrides.{cpu} = \"<reason>\" to allow it anyway"
            )));
        }
    }
    if let Some(cpu) = fma_cpu_overrides.keys().find(|c| !allowed_target_cpus.contains(*c)) {
        return Err(invalid(format!(
            "knp1-lint.toml: flags.fma_cpu_overrides.{cpu} has no matching flags.allowed_target_cpus entry"
        )));
    }
    Ok(Config {
        workspace,
        profiles: profiles.into_iter().collect(),
        ci_env_files,
        fma_features: fma_features.into_iter().collect(),
        wasm_module,
        simd_policy,
        allowed_profile_differences: allowed.into_iter().collect(),
        allowed_target_cpus: allowed_target_cpus.into_iter().collect(),
        fma_cpu_overrides,
    })
}

/// Reads and parses `knp1-lint.toml` at `path`.
pub fn load_config(path: &Path) -> Result<Config, FlagsError> {
    let text = read_text(path, "knp1-lint.toml")?.ok_or_else(|| invalid(format!("{}: not found", path.display())))?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    parse_config(&text, dir)
}

// ------------------------------------------------------------------ line index

/// Splits a dotted TOML key (`a."b.c".d`) into its segments; whitespace around segments is dropped.
fn key_segments(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None => match c {
                '"' | '\'' => quote = Some(c),
                '.' => out.push(std::mem::take(&mut cur).trim().to_string()),
                _ => cur.push(c),
            },
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Line positions of TOML table headers and keys, found by a light line scan of the source.
/// Used only to place findings; the semantics come from the real parser.
struct LineIndex<'a> {
    lines: Vec<&'a str>,
    /// Full key path -> (1-based line, 1-based column) of its first occurrence.
    keys: BTreeMap<Vec<String>, (u32, u32)>,
}

fn line_no(i: usize) -> u32 {
    u32::try_from(i + 1).unwrap_or(u32::MAX)
}

fn col_no(line: &str, byte: usize) -> u32 {
    let chars = line.get(..byte).map_or(0, |p| p.chars().count());
    u32::try_from(chars + 1).unwrap_or(u32::MAX)
}

/// The key part of `key = value` (outside quotes), if the line is a key/value line.
fn key_part(line: &str) -> Option<&str> {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '=' => return line.get(..i),
                '#' | '[' | '{' | ',' => return None,
                _ => {}
            },
        }
    }
    None
}

impl<'a> LineIndex<'a> {
    fn new(text: &'a str) -> LineIndex<'a> {
        let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
        let mut keys = BTreeMap::new();
        let mut header: Vec<String> = Vec::new();
        for (i, raw) in lines.iter().enumerate() {
            let indent = raw.len() - raw.trim_start().len();
            let line = raw.trim();
            if line.starts_with('[') {
                let inner = line.trim_start_matches('[');
                let close = inner.find(']').unwrap_or(inner.len());
                header = key_segments(inner.get(..close).unwrap_or(""));
                keys.entry(header.clone()).or_insert((line_no(i), col_no(raw, indent)));
            } else if let Some(k) = key_part(line) {
                let mut full = header.clone();
                full.extend(key_segments(k));
                keys.entry(full).or_insert((line_no(i), col_no(raw, indent)));
            }
        }
        LineIndex { lines, keys }
    }

    /// Position of `path`, or of its longest known prefix, or line 1.
    fn locate(&self, path: &[String]) -> (u32, u32) {
        for n in (1..=path.len()).rev() {
            if let Some(p) = path.get(..n).and_then(|p| self.keys.get(p)) {
                return *p;
            }
        }
        (1, 1)
    }

    /// First occurrence of `needle` at or after `from` (1-based line), else `from` itself.
    fn find_from(&self, from: (u32, u32), needle: &str) -> (u32, u32) {
        let start = (from.0 as usize).saturating_sub(1);
        for (i, l) in self.lines.iter().enumerate().skip(start) {
            if let Some(b) = l.find(needle) {
                return (line_no(i), col_no(l, b));
            }
        }
        from
    }

    fn line_text(&self, line: u32) -> &str {
        self.lines.get((line as usize).saturating_sub(1)).copied().unwrap_or("")
    }
}

// ------------------------------------------------------------------ findings helpers

struct Sink<'c> {
    cfg: &'c Config,
    out: Vec<Finding>,
}

impl Sink<'_> {
    fn push(&mut self, suffix: &str, file: &str, pos: (u32, u32), excerpt: &[u8], message: String) {
        self.out.push(Finding {
            rule: Rule::Flag(suffix.to_string()),
            crate_name: BUILD_CONFIG_CRATE.to_string(),
            crate_version: BUILD_CONFIG_VERSION.to_string(),
            file: file.to_string(),
            line: pos.0,
            column: pos.1,
            excerpt_digest: input_digest(excerpt),
            message,
        });
    }

    fn at_line(&mut self, suffix: &str, file: &str, idx: &LineIndex<'_>, pos: (u32, u32), message: String) {
        let text = idx.line_text(pos.0).to_string();
        self.push(suffix, file, pos, text.as_bytes(), message);
    }

    fn whole_file(&mut self, suffix: &str, file: &str, message: String) {
        let m = message.clone();
        self.push(suffix, file, (1, 1), m.as_bytes(), message);
    }

    fn is_fma_feature(&self, f: &str) -> bool {
        is_builtin_fma_feature(f) || self.cfg.fma_features.iter().any(|x| x == f)
    }
}

// ------------------------------------------------------------------ flag tokens

/// One codegen option found in a flag source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Codegen {
    /// Option name with `_` normalised to `-` (`target_cpu` becomes `target-cpu`).
    pub key: String,
    pub value: String,
    /// The text to look for when placing the finding (as written in the source).
    pub needle: String,
    /// For `--config profile.<name>.codegen-units=N`: the profile it applies to.
    pub profile: Option<String>,
}

/// Escapes of the unit separator `0x1F` that YAML, shells and TOML sources may carry.
const US_ESCAPES: [&str; 7] = ["\\x1f", "\\x1F", "\\u001f", "\\u001F", "\\U0000001f", "\\U0000001F", "\\037"];

/// True for characters that separate tokens in flag text: whitespace, `0x1F`, and the
/// shell/YAML/TOML punctuation that can never be part of a codegen option.
fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || matches!(c, '\u{1f}' | '"' | '\'' | '[' | ']' | '{' | '}' | '(' | ')' | ';' | '&' | '|' | '<' | '>' | '`')
}

/// Normalises raw flag text and splits it into tokens: `0x1F` and its escapes become spaces,
/// text is split on [`is_separator`] characters, commas around a token (array and flow syntax)
/// are dropped, and empty tokens vanish. This is the single tokeniser used for every source.
pub fn tokens(text: &str) -> Vec<String> {
    let mut t = text.to_string();
    for e in US_ESCAPES {
        t = t.replace(e, " ");
    }
    t.split(is_separator).map(|s| s.trim_matches(',')).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// `NAME=rest` where `NAME` looks like an environment variable or a Cargo config key
/// (`RUSTFLAGS`, `build.rustflags`): returns `rest`.
fn assignment_rest(t: &str) -> Option<&str> {
    let (name, rest) = t.split_once('=')?;
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    (first_ok && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))).then_some(rest)
}

/// rustc treats `_` and `-` alike in `-C` option names.
fn norm_key(k: &str) -> String {
    k.trim().replace('_', "-")
}

/// One `--config` value: `profile.<name>.codegen-units=N` is returned as a profile-scoped
/// `codegen-units` option; any other value whose right-hand side is a flag
/// (`build.rustflags=-Ctarget-cpu=native`) is parsed as flags.
fn cargo_config_override(v: &str, out: &mut Vec<Codegen>) {
    let Some((k, val)) = v.split_once('=') else { return };
    let segs: Vec<String> = k.split('.').map(norm_key).collect();
    if let [p, name, key] = segs.as_slice() {
        if p == "profile" && key == "codegen-units" {
            out.push(Codegen {
                key: "codegen-units".to_string(),
                value: val.to_string(),
                needle: v.to_string(),
                profile: Some(name.clone()),
            });
            return;
        }
    }
    if val.starts_with('-') {
        out.extend(codegen_options(&[val.to_string()]));
    }
}

/// Extracts the codegen options from a token list (output of [`tokens`]). Handles `-C k=v`,
/// `-Ck=v`, `--codegen k=v`, `--codegen=k=v`, Cargo `--config X` / `--config=X`, and an
/// assignment prefix in front of a flag (`RUSTFLAGS=-C`, `export RUSTFLAGS=-Ctarget-cpu=native`).
/// Option names are normalised with `_` → `-`. Any other token that mentions `fp-contract`
/// (an LLVM argument split away from its `-C llvm-args=`) is returned as key `llvm-args`.
pub fn codegen_options(tokens: &[String]) -> Vec<Codegen> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let mut t = tokens.get(i).map(String::as_str).unwrap_or("");
        if let Some(rest) = assignment_rest(t).filter(|r| r.is_empty() || r.starts_with('-')) {
            t = rest;
        }
        if t == "--config" {
            i += 1;
            if let Some(v) = tokens.get(i) {
                cargo_config_override(v, &mut out);
            }
            i += 1;
            continue;
        }
        if let Some(v) = t.strip_prefix("--config=") {
            cargo_config_override(v, &mut out);
            i += 1;
            continue;
        }
        let opt: Option<&str> = if t == "-C" || t == "--codegen" {
            i += 1;
            tokens.get(i).map(String::as_str)
        } else if let Some(rest) = t.strip_prefix("--codegen=") {
            Some(rest)
        } else {
            t.strip_prefix("-C").filter(|r| !r.is_empty())
        };
        match opt.and_then(|o| o.split_once('=')) {
            Some((k, v)) => {
                out.push(Codegen { key: norm_key(k), value: v.to_string(), needle: format!("{k}={v}"), profile: None })
            }
            None if opt.is_none() && t.contains("fp-contract") => out.push(Codegen {
                key: "llvm-args".to_string(),
                value: t.to_string(),
                needle: t.to_string(),
                profile: None,
            }),
            None => {}
        }
        i += 1;
    }
    out
}

/// True when an LLVM argument string permits floating-point contraction: it mentions
/// `fp-contract` (or `fp_contract`) with any value other than `off`, or with no value.
fn llvm_args_contract(v: &str) -> bool {
    let v = v.replace('_', "-");
    let mut rest = v.as_str();
    while let Some(i) = rest.find("fp-contract") {
        let after = rest.get(i + "fp-contract".len()..).unwrap_or("");
        let value: String =
            after.strip_prefix('=').unwrap_or("").chars().take_while(|c| !c.is_whitespace() && *c != ',').collect();
        if value != "off" {
            return true;
        }
        rest = after;
    }
    false
}

/// Maps a needle to its (line, column) and the text of that line.
type Place<'p> = &'p mut dyn FnMut(&str) -> ((u32, u32), String);

/// Evaluates codegen options of one source. `place` maps a needle to a position and excerpt line.
fn eval_codegen(sink: &mut Sink<'_>, file: &str, opts: &[Codegen], place: Place<'_>) {
    for o in opts {
        match o.key.as_str() {
            "target-cpu" if o.value == "native" => {
                let (pos, text) = place(&o.needle);
                sink.push(
                    "NATIVE",
                    file,
                    pos,
                    text.as_bytes(),
                    format!("`-C {}` lets the compiler use the host's FMA", o.needle),
                );
            }
            "target-cpu" if !sink.cfg.allowed_target_cpus.contains(&o.value) && is_fma_cpu(&o.value) => {
                let (pos, text) = place(&o.needle);
                sink.push(
                    "FMA",
                    file,
                    pos,
                    text.as_bytes(),
                    format!("`-C {}` names a CPU whose target features include FMA", o.needle),
                );
            }
            "target-cpu" if !sink.cfg.allowed_target_cpus.contains(&o.value) => {
                let (pos, text) = place(&o.needle);
                sink.push(
                    "CPU",
                    file,
                    pos,
                    text.as_bytes(),
                    format!("`-C {}` is not in allowed_target_cpus; a named CPU can enable FMA", o.needle),
                );
            }
            "llvm-args" if llvm_args_contract(&o.value) => {
                let (pos, text) = place(&o.needle);
                sink.push(
                    "FMA",
                    file,
                    pos,
                    text.as_bytes(),
                    format!("`{}` lets LLVM contract floating-point operations into FMA", o.needle),
                );
            }
            "target-feature" => {
                let bad: Vec<&str> = o
                    .value
                    .split(',')
                    .filter_map(|f| f.trim().strip_prefix('+'))
                    .filter(|f| sink.is_fma_feature(f))
                    .collect();
                if !bad.is_empty() {
                    let (pos, text) = place(&o.needle);
                    let list: Vec<String> = bad.iter().map(|f| format!("+{f}")).collect();
                    sink.push(
                        "FMA",
                        file,
                        pos,
                        text.as_bytes(),
                        format!("`-C {}` enables fused multiply-add ({})", o.needle, list.join(", ")),
                    );
                }
            }
            "codegen-units" if o.value != "1" => {
                let msg = match &o.profile {
                    None => format!("`-C {}`: kernel builds need one codegen unit", o.needle),
                    Some(p) if sink.cfg.profiles.contains(p) => {
                        format!("`--config {}` sets codegen-units of kernel profile `{p}`", o.needle)
                    }
                    Some(_) => continue,
                };
                let (pos, text) = place(&o.needle);
                sink.push("CGU", file, pos, text.as_bytes(), msg);
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ .cargo/config.toml rustflags

fn flag_strings(v: &toml::Value) -> Vec<String> {
    match v {
        toml::Value::String(s) => vec![s.clone()],
        toml::Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        // `[env] RUSTFLAGS = { value = "..." }`
        toml::Value::Table(t) => t.get("value").map(flag_strings).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn is_flags_key(k: &str) -> bool {
    k.eq_ignore_ascii_case("rustflags")
        || k.eq_ignore_ascii_case("rustdocflags")
        || k.ends_with("RUSTFLAGS")
        || k.ends_with("RUSTDOCFLAGS")
}

fn walk_rustflags(t: &toml::Table, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, Vec<String>)>) {
    for (k, v) in t {
        path.push(k.clone());
        if is_flags_key(k) {
            let toks: Vec<String> = flag_strings(v).iter().flat_map(|s| tokens(s)).collect();
            out.push((path.clone(), toks));
        } else if let toml::Value::Table(sub) = v {
            walk_rustflags(sub, path, out);
        }
        path.pop();
    }
}

fn check_cargo_config(sink: &mut Sink<'_>, file: &str, text: &str, table: &toml::Table) {
    let idx = LineIndex::new(text);
    let mut sources = Vec::new();
    walk_rustflags(table, &mut Vec::new(), &mut sources);
    for (path, toks) in sources {
        let key_pos = idx.locate(&path);
        let opts = codegen_options(&toks);
        let mut seen: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        let mut place = |needle: &str| {
            // Repeated needles in one source move on to the next occurrence.
            let from = seen.get(needle).map_or(key_pos, |p| (p.0 + 1, 1));
            let pos = idx.find_from(from, needle);
            seen.insert(needle.to_string(), pos);
            (pos, idx.line_text(pos.0).to_string())
        };
        eval_codegen(sink, file, &opts, &mut place);
    }
}

// ------------------------------------------------------------------ CI env files

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// True when a YAML line has a non-empty value after its key (`key: value`, `- run: |`), or
/// is not a `key:` line at all; such a line continues over the following more-indented lines.
fn has_inline_value(line: &str) -> bool {
    let t = line.trim();
    let t = t.strip_prefix("- ").unwrap_or(t);
    if let Some(i) = t.find(": ") {
        return !t.get(i + 2..).unwrap_or("").trim().is_empty();
    }
    !t.ends_with(':')
}

/// Groups the lines of a CI file into logical lines, as half-open index ranges. A line ending
/// in `\` continues on the next line; a line with a non-empty value ([`has_inline_value`])
/// continues over the following lines that are blank or indented more deeply (YAML block
/// scalars `|`, `>-` and plain multi-line scalars). Comment lines never start a span.
pub fn ci_spans(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        let first = lines.get(i).copied().unwrap_or("");
        let base = indent_of(first);
        let folds = !is_comment(first) && !first.trim().is_empty() && has_inline_value(first);
        i += 1;
        while let Some(l) = lines.get(i) {
            let prev = lines.get(i - 1).copied().unwrap_or("");
            let continued = prev.trim_end().ends_with('\\') && !is_comment(prev);
            let deeper = folds && (l.trim().is_empty() || indent_of(l) > base);
            if !(continued || deeper) {
                break;
            }
            i += 1;
        }
        out.push((start, i));
    }
    out
}

fn check_ci_file(sink: &mut Sink<'_>, file: &str, text: &str) {
    let idx = LineIndex::new(text);
    let prof_vars: Vec<(String, String)> = sink
        .cfg
        .profiles
        .iter()
        .map(|p| (p.clone(), format!("CARGO_PROFILE_{}_CODEGEN_UNITS", p.to_ascii_uppercase().replace('-', "_"))))
        .collect();
    for (start, end) in ci_spans(&idx.lines) {
        let span: Vec<(usize, &str)> =
            (start..end).filter_map(|i| idx.lines.get(i).map(|l| (i, *l))).filter(|(_, l)| !is_comment(l)).collect();
        let logical: Vec<&str> = span.iter().map(|(_, l)| l.trim_end().trim_end_matches('\\')).collect();
        let toks = tokens(&logical.join(" "));
        let opts = codegen_options(&toks);
        // Needles are placed on the physical line that holds them; repeats move forward.
        let mut seen: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut find = |needle: &str| -> Option<(usize, usize)> {
            let (from_line, from_byte) = seen.get(needle).copied().unwrap_or((start, 0));
            for (i, l) in &span {
                if *i < from_line {
                    continue;
                }
                let skip = if *i == from_line { from_byte } else { 0 };
                if let Some(b) = l.get(skip..).and_then(|s| s.find(needle)) {
                    seen.insert(needle.to_string(), (*i, skip + b + 1));
                    return Some((*i, skip + b));
                }
            }
            None
        };
        let mut place = |needle: &str| {
            let (i, b) = find(needle).unwrap_or((start, 0));
            let raw = idx.lines.get(i).copied().unwrap_or("");
            ((line_no(i), col_no(raw, b)), raw.to_string())
        };
        eval_codegen(sink, file, &opts, &mut place);
        for w in toks.windows(2) {
            if let [cmd, tc] = w {
                if cmd == "cargo" && tc.starts_with('+') {
                    let (pos, _) = place(tc.as_str());
                    sink.at_line(
                        "TOOLCHAIN",
                        file,
                        &idx,
                        pos,
                        format!("`cargo {tc}` overrides the pinned rust-toolchain.toml"),
                    );
                }
            }
        }
        for (i, raw) in &span {
            let line = line_no(*i);
            if let Some(b) = raw.find("RUSTUP_TOOLCHAIN") {
                sink.at_line(
                    "TOOLCHAIN",
                    file,
                    &idx,
                    (line, col_no(raw, b)),
                    "`RUSTUP_TOOLCHAIN` overrides the pinned rust-toolchain.toml".to_string(),
                );
            }
            for (prof, var) in &prof_vars {
                if let Some(b) = raw.find(var.as_str()) {
                    let rest = raw.get(b + var.len()..).unwrap_or("");
                    let val = rest.trim_start_matches([':', '=', ' ', '\t', '"', '\'']);
                    let val: String = val.chars().take_while(|c| !is_separator(*c) && *c != ',').collect();
                    if val != "1" {
                        sink.at_line(
                            "CGU",
                            file,
                            &idx,
                            (line, col_no(raw, b)),
                            format!("`{var}` sets codegen-units of kernel profile `{prof}` to {val:?}"),
                        );
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ toolchain

fn exact_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn check_toolchain(sink: &mut Sink<'_>) -> Result<(), FlagsError> {
    const FILE: &str = "rust-toolchain.toml";
    const LEGACY: &str = "rust-toolchain";
    if sink.cfg.workspace.join(LEGACY).exists() {
        sink.whole_file(
            "TOOLCHAIN",
            LEGACY,
            "legacy rust-toolchain file present: rustup prefers it over rust-toolchain.toml".to_string(),
        );
    }
    let Some(text) = read_text(&sink.cfg.workspace.join(FILE), FILE)? else {
        sink.whole_file("TOOLCHAIN", FILE, "rust-toolchain.toml is missing: the toolchain is not pinned".to_string());
        return Ok(());
    };
    let table = parse_toml(&text, FILE)?;
    let idx = LineIndex::new(&text);
    let tc = match table.get("toolchain") {
        Some(toml::Value::Table(t)) => t,
        _ => {
            sink.at_line("TOOLCHAIN", FILE, &idx, (1, 1), "rust-toolchain.toml has no [toolchain] table".to_string());
            return Ok(());
        }
    };
    let key = |k: &str| vec!["toolchain".to_string(), k.to_string()];
    match tc.get("channel") {
        Some(toml::Value::String(c)) if exact_version(c) => {}
        Some(toml::Value::String(c)) => {
            let pos = idx.locate(&key("channel"));
            sink.at_line("TOOLCHAIN", FILE, &idx, pos, format!("channel {c:?} is not an exact version X.Y.Z"));
        }
        _ => {
            let pos = idx.locate(&key("channel"));
            sink.at_line("TOOLCHAIN", FILE, &idx, pos, "toolchain.channel is missing or not a string".to_string());
        }
    }
    match tc.get("components") {
        Some(toml::Value::Array(_)) => {}
        _ => {
            let pos = idx.locate(&key("components"));
            sink.at_line("TOOLCHAIN", FILE, &idx, pos, "toolchain.components is not pinned".to_string());
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ profiles

/// Built-in Cargo profiles: (name, inherits, default codegen-units and compared keys).
fn builtin(name: &str) -> Option<(Option<&'static str>, BTreeMap<&'static str, String>)> {
    let dev = || {
        BTreeMap::from([
            ("codegen-units", "256".to_string()),
            ("debug-assertions", "true".to_string()),
            ("lto", "false".to_string()),
            ("opt-level", "0".to_string()),
            ("overflow-checks", "true".to_string()),
            ("panic", "unwind".to_string()),
        ])
    };
    let release = || {
        BTreeMap::from([
            ("codegen-units", "16".to_string()),
            ("debug-assertions", "false".to_string()),
            ("lto", "false".to_string()),
            ("opt-level", "3".to_string()),
            ("overflow-checks", "false".to_string()),
            ("panic", "unwind".to_string()),
        ])
    };
    match name {
        "dev" => Some((None, dev())),
        "release" => Some((None, release())),
        "test" => Some((Some("dev"), dev())),
        "bench" => Some((Some("release"), release())),
        _ => None,
    }
}

fn value_text(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

/// Normalises an `lto` value so that equivalent spellings compare equal.
fn norm_lto(v: &str) -> String {
    match v {
        "true" | "fat" => "fat".to_string(),
        other => other.to_string(),
    }
}

/// One profile source file: its workspace-relative name, line index and `[profile]` table.
struct ProfileSource<'t> {
    file: &'static str,
    idx: LineIndex<'t>,
    profiles: Option<&'t toml::Table>,
}

impl ProfileSource<'_> {
    fn get(&self, prof: &str) -> Option<&toml::Table> {
        self.profiles.and_then(|p| p.get(prof)).and_then(toml::Value::as_table)
    }
}

/// Where a resolved key value came from.
struct Resolved {
    value: String,
    /// (file, line, column) of the setting, or of the profile header when it is a default.
    at: (&'static str, (u32, u32)),
    explicit: bool,
}

/// Resolves `key` of `prof` with Cargo's rules: `.cargo/config.toml` over `Cargo.toml`, then
/// `inherits`, then the built-in default. `Err` names why the profile cannot be resolved.
fn resolve(sources: &[ProfileSource<'_>], prof: &str, key: &str) -> Result<Resolved, String> {
    let header = |p: &str| -> (&'static str, (u32, u32)) {
        for s in sources.iter().rev() {
            if s.get(p).is_some() {
                return (s.file, s.idx.locate(&["profile".to_string(), p.to_string()]));
            }
        }
        ("Cargo.toml", (1, 1))
    };
    let mut cur = prof.to_string();
    let mut chain = vec![cur.clone()];
    loop {
        // Later sources (`.cargo/config.toml`) override earlier ones (`Cargo.toml`).
        for s in sources.iter().rev() {
            if let Some(v) = s.get(&cur).and_then(|t| t.get(key)) {
                let path = ["profile".to_string(), cur.clone(), key.to_string()];
                return Ok(Resolved { value: value_text(v), at: (s.file, s.idx.locate(&path)), explicit: true });
            }
        }
        let inherits = sources
            .iter()
            .rev()
            .find_map(|s| s.get(&cur).and_then(|t| t.get("inherits")).and_then(toml::Value::as_str))
            .map(str::to_string);
        let next = match (inherits, builtin(&cur)) {
            (_, Some((None, defaults))) => {
                return match defaults.get(key) {
                    Some(d) => Ok(Resolved { value: d.clone(), at: header(prof), explicit: false }),
                    None => Err(format!("no default for {key}")),
                };
            }
            (_, Some((Some(parent), _))) => parent.to_string(),
            (Some(i), None) => i,
            (None, None) => {
                return Err(if chain.len() == 1 && sources.iter().all(|s| s.get(&cur).is_none()) {
                    format!("profile `{cur}` is not defined")
                } else {
                    format!("custom profile `{cur}` has no `inherits`")
                });
            }
        };
        if chain.contains(&next) || chain.len() > MAX_INHERIT_DEPTH {
            return Err(format!("profile inheritance cycle: {} -> {next}", chain.join(" -> ")));
        }
        chain.push(next.clone());
        cur = next;
    }
}

/// The `inherits` chain of `prof`, starting with `prof` itself (best effort: stops at an
/// undefined parent or a cycle; those are reported by [`resolve`]).
fn profile_chain(sources: &[ProfileSource<'_>], prof: &str) -> Vec<String> {
    let mut chain = vec![prof.to_string()];
    let mut cur = prof.to_string();
    while chain.len() <= MAX_INHERIT_DEPTH {
        let explicit = sources
            .iter()
            .rev()
            .find_map(|s| s.get(&cur).and_then(|t| t.get("inherits")).and_then(toml::Value::as_str))
            .map(str::to_string);
        let next = match builtin(&cur) {
            Some((None, _)) => None,
            Some((Some(parent), _)) => Some(parent.to_string()),
            None => explicit,
        };
        match next {
            Some(n) if !chain.contains(&n) => {
                chain.push(n.clone());
                cur = n;
            }
            _ => break,
        }
    }
    chain
}

fn check_profiles(sink: &mut Sink<'_>, sources: &[ProfileSource<'_>]) {
    let cfg = sink.cfg;
    let profiles = &cfg.profiles;
    // KNP1-FLAG-CGU per kernel profile, including package overrides.
    for p in profiles {
        match resolve(sources, p, "codegen-units") {
            Ok(r) if r.value == "1" => {}
            Ok(r) => {
                let how = if r.explicit { "sets" } else { "defaults to" };
                let idx = sources.iter().find(|s| s.file == r.at.0).map(|s| &s.idx);
                let msg = format!("kernel profile `{p}` {how} codegen-units = {}", r.value);
                match idx {
                    Some(idx) => sink.at_line("CGU", r.at.0, idx, r.at.1, msg),
                    None => sink.whole_file("CGU", r.at.0, msg),
                }
            }
            Err(e) => {
                let file = sources.first().map_or("Cargo.toml", |s| s.file);
                sink.whole_file(
                    "CGU",
                    file,
                    format!("kernel profile `{p}`: {e}; codegen-units cannot be shown to be 1"),
                );
            }
        }
        for q in profile_chain(sources, p) {
            for s in sources {
                let Some(pkgs) = s.get(&q).and_then(|t| t.get("package")).and_then(toml::Value::as_table) else {
                    continue;
                };
                for (name, v) in pkgs {
                    if let Some(cgu) = v.as_table().and_then(|t| t.get("codegen-units")) {
                        let val = value_text(cgu);
                        if val != "1" {
                            let path: Vec<String> = ["profile", q.as_str(), "package", name.as_str(), "codegen-units"]
                                .map(str::to_string)
                                .to_vec();
                            let pos = s.idx.locate(&path);
                            let via = if q == *p { String::new() } else { format!(" (inherited from `{q}`)") };
                            sink.at_line(
                                "CGU",
                                s.file,
                                &s.idx,
                                pos,
                                format!("kernel profile `{p}` sets codegen-units = {val} for package `{name}`{via}"),
                            );
                        }
                    }
                }
            }
        }
    }
    // KNP1-FLAG-LTO: compared keys must agree across kernel profiles unless allowed.
    let Some((first, rest)) = profiles.split_first() else { return };
    for key in COMPARED_PROFILE_KEYS {
        if cfg.allowed_profile_differences.iter().any(|a| a == key) {
            continue;
        }
        let canon = |v: String| if key == "lto" { norm_lto(&v) } else { v };
        let Ok(base) = resolve(sources, first, key) else { continue };
        let base_v = canon(base.value);
        for p in rest {
            let Ok(r) = resolve(sources, p, key) else { continue };
            let v = canon(r.value);
            if v != base_v {
                let msg = format!("kernel profiles `{first}` and `{p}` differ in {key}: {base_v} vs {v}");
                match sources.iter().find(|s| s.file == r.at.0) {
                    Some(s) => sink.at_line("LTO", r.at.0, &s.idx, r.at.1, msg),
                    None => sink.whole_file("LTO", r.at.0, msg),
                }
            }
        }
    }
}

// ------------------------------------------------------------------ wasm

/// Scans wasm module bytes and returns its findings against `file` (REQ-005). A module that
/// cannot be parsed yields exactly one `KNP1-FLAG-WASMPARSE` finding.
pub fn check_wasm(bytes: &[u8], file: &str, policy: &wasm::SimdPolicy) -> Vec<Finding> {
    let mk = |suffix: &str, offset: usize, excerpt: &[u8], message: String| Finding {
        rule: Rule::Flag(suffix.to_string()),
        crate_name: BUILD_CONFIG_CRATE.to_string(),
        crate_version: BUILD_CONFIG_VERSION.to_string(),
        file: file.to_string(),
        line: 1,
        column: u32::try_from(offset + 1).unwrap_or(u32::MAX),
        excerpt_digest: input_digest(excerpt),
        message,
    };
    match wasm::scan(bytes, policy) {
        Ok(hits) => hits
            .into_iter()
            .map(|h| match h {
                wasm::WasmHit::Fma { offset, func, sub, name, bytes } => mk(
                    "WASMFMA",
                    offset,
                    &bytes,
                    format!("function body {func} uses {name} (0xfd {sub:#x}) at byte {offset}"),
                ),
                wasm::WasmHit::Simd { offset, func, sub, bytes } => mk(
                    "WASMSIMD",
                    offset,
                    &bytes,
                    format!("function body {func} uses SIMD opcode 0xfd {sub:#x} at byte {offset}, not allowed by the config"),
                ),
            })
            .collect(),
        Err(e) => {
            let m = format!("cannot parse the wasm module at byte {}: {}", e.offset, e.message);
            let ex = m.clone();
            vec![mk("WASMPARSE", e.offset, ex.as_bytes(), m)]
        }
    }
}

// ------------------------------------------------------------------ keyword gate

/// The fail-closed substring gate ([`keywords`]) over a CI file, in three passes. A hit is
/// reported as `KNP1-FLAG-KEYWORD` only where no earlier finding covers it.
///
/// 1. Each raw line (text before an opt-out marker): finding at the first hit's column.
/// 2. Each logical line from [`ci_spans`], joined with a space, or with nothing after a
///    trailing `\` (shell continuation): a hit in a span none of whose lines is covered is
///    reported at column 1 of the span's first line.
/// 3. The whole file with every whitespace character removed ([`keywords::compact`]): a hit
///    none of whose lines is covered is reported at its first character, so spacing tricks
///    such as `target - cpu` or a keyword folded over two YAML lines are caught.
fn keyword_gate(sink: &mut Sink<'_>, file: &str, text: &str) {
    let idx = LineIndex::new(text);
    let mut flagged: BTreeSet<u32> = sink.out.iter().filter(|f| f.file == file).map(|f| f.line).collect();
    keyword_lines(sink, file, &idx, &mut flagged);
    keyword_spans(sink, file, &idx, &mut flagged);
    keyword_compact(sink, file, &idx, &flagged);
}

fn keyword_message(what: &BTreeSet<String>, how: &str) -> String {
    let list: Vec<String> = what.iter().map(|k| format!("`{k}`")).collect();
    format!("mentions {}{how}: build flags and toolchain selection must not be set here", list.join(", "))
}

fn keyword_spans(sink: &mut Sink<'_>, file: &str, idx: &LineIndex<'_>, flagged: &mut BTreeSet<u32>) {
    for (start, end) in ci_spans(&idx.lines) {
        if (start..end).any(|i| flagged.contains(&line_no(i))) {
            continue;
        }
        let mut joined = String::new();
        for raw in idx.lines.get(start..end).unwrap_or(&[]) {
            let (piece, _) = keywords::split_marker(raw);
            if joined.ends_with('\\') {
                joined.pop();
            } else if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(piece);
        }
        let hits = keywords::scan(&joined);
        if !hits.is_empty() {
            let what: BTreeSet<String> = hits.into_iter().map(|(_, k)| k).collect();
            let line = line_no(start);
            sink.at_line("KEYWORD", file, idx, (line, 1), keyword_message(&what, " across a logical line"));
            for i in start..end {
                flagged.insert(line_no(i));
            }
        }
    }
}

fn keyword_compact(sink: &mut Sink<'_>, file: &str, idx: &LineIndex<'_>, flagged: &BTreeSet<u32>) {
    let chars = keywords::compact(&idx.lines);
    let mut reported: BTreeSet<u32> = BTreeSet::new();
    for (a, z) in keywords::scan_compact(&chars) {
        let (Some(first), Some(last)) = (chars.get(a), chars.get(z)) else { continue };
        let lines = line_no(first.1)..=line_no(last.1);
        if lines.clone().any(|l| flagged.contains(&l) || reported.contains(&l)) {
            continue;
        }
        let what: String = chars.get(a..=z).unwrap_or(&[]).iter().map(|c| c.0).collect();
        let col = u32::try_from(first.2 + 1).unwrap_or(u32::MAX);
        let line = line_no(first.1);
        sink.at_line(
            "KEYWORD",
            file,
            idx,
            (line, col),
            keyword_message(&BTreeSet::from([what]), " once whitespace is removed"),
        );
        reported.extend(lines);
    }
}

fn keyword_lines(sink: &mut Sink<'_>, file: &str, idx: &LineIndex<'_>, flagged: &mut BTreeSet<u32>) {
    for (i, raw) in idx.lines.iter().enumerate() {
        let line = line_no(i);
        if flagged.contains(&line) {
            continue;
        }
        let (scanned, _) = keywords::split_marker(raw);
        let hits = keywords::scan(scanned);
        if let Some((c, _)) = hits.first() {
            let col = u32::try_from(c + 1).unwrap_or(u32::MAX);
            let what: BTreeSet<String> = hits.iter().map(|(_, k)| k.clone()).collect();
            sink.at_line("KEYWORD", file, idx, (line, col), keyword_message(&what, ""));
            flagged.insert(line);
        }
    }
}

fn walk_free_values(t: &toml::Table, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, String)>) {
    for (k, v) in t {
        if is_flags_key(k) {
            continue; // parsed by the structured flag checks
        }
        path.push(k.clone());
        match v {
            toml::Value::Table(sub) => walk_free_values(sub, path, out),
            other => {
                let mut text = path.join(" ");
                for s in flag_strings_deep(other) {
                    text.push(' ');
                    text.push_str(&s);
                }
                out.push((path.clone(), text));
            }
        }
        path.pop();
    }
}

fn flag_strings_deep(v: &toml::Value) -> Vec<String> {
    match v {
        toml::Value::String(s) => vec![s.clone()],
        toml::Value::Array(a) => a.iter().flat_map(flag_strings_deep).collect(),
        toml::Value::Table(t) => t.values().flat_map(flag_strings_deep).collect(),
        _ => Vec::new(),
    }
}

/// The substring gate over the free-form tables of a Cargo config (`[alias]`, `[env]`, every
/// `[target.*]`): any key (other than a rustflags key) whose name or string values hit a keyword.
fn cargo_table_gate(sink: &mut Sink<'_>, file: &str, text: &str, table: &toml::Table) {
    let idx = LineIndex::new(text);
    let mut items = Vec::new();
    for top in ["alias", "env", "target"] {
        if let Some(toml::Value::Table(t)) = table.get(top) {
            walk_free_values(t, &mut vec![top.to_string()], &mut items);
        }
    }
    let mut done: BTreeSet<(u32, u32)> = BTreeSet::new();
    for (path, text) in items {
        let hits = keywords::scan(&text);
        if hits.is_empty() {
            continue;
        }
        let pos = idx.locate(&path);
        if !done.insert(pos) {
            continue; // one finding per key line (inline tables yield several values)
        }
        let what: BTreeSet<&str> = hits.iter().map(|(_, k)| k.as_str()).collect();
        let list: Vec<String> = what.iter().map(|k| format!("`{k}`")).collect();
        sink.at_line(
            "KEYWORD",
            file,
            &idx,
            pos,
            format!(
                "`{}` mentions {}: build flags and toolchain selection must not be set here",
                path.join("."),
                list.join(", ")
            ),
        );
    }
}

// ------------------------------------------------------------------ run

/// Findings of a run plus informational notes (`KNP1-FLAG-OPTOUT`, one per honoured
/// `knp1-flags: allow` marker). Notes never fail the lint; the CLI shows them to reviewers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    pub findings: Vec<Finding>,
    pub notes: Vec<Finding>,
}

/// Moves findings whose line carries a matching `knp1-flags: allow <RULE> <reason>` marker into
/// notes. `texts` maps each text file to its content; wasm findings are never opted out.
fn apply_markers(findings: Vec<Finding>, texts: &BTreeMap<String, String>) -> Outcome {
    let mut out = Outcome::default();
    for f in findings {
        let marker = texts
            .get(&f.file)
            .filter(|_| f.rule.is_waivable())
            .and_then(|t| t.split('\n').nth((f.line as usize).saturating_sub(1)))
            .and_then(|l| keywords::split_marker(l.strip_suffix('\r').unwrap_or(l)).1);
        match marker {
            Some((rule, reason)) if rule == f.rule.id() => {
                let note = Finding {
                    rule: Rule::Flag("OPTOUT".to_string()),
                    message: format!("line opts out of {rule}: {reason} (was: {})", f.message),
                    ..f
                };
                out.notes.push(note);
            }
            _ => out.findings.push(f),
        }
    }
    sort_findings(&mut out.findings);
    sort_findings(&mut out.notes);
    out
}

/// Runs every check of the module against a parsed config. Returns sorted findings.
pub fn check(cfg: &Config) -> Result<Vec<Finding>, FlagsError> {
    Ok(check_all(cfg)?.findings)
}

/// Runs every check and returns findings and opt-out notes.
pub fn check_all(cfg: &Config) -> Result<Outcome, FlagsError> {
    let mut sink = Sink { cfg, out: Vec::new() };
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    check_toolchain(&mut sink)?;

    // .cargo/config.toml (and legacy .cargo/config); both at once is ambiguous.
    let mut cargo_cfg: Option<(&'static str, String, toml::Table)> = None;
    let mut present = Vec::new();
    for name in [".cargo/config.toml", ".cargo/config"] {
        if let Some(text) = read_text(&cfg.workspace.join(name), name)? {
            let table = parse_toml(&text, name)?;
            check_cargo_config(&mut sink, name, &text, &table);
            cargo_table_gate(&mut sink, name, &text, &table);
            present.push(name);
            texts.insert(name.to_string(), text.clone());
            if cargo_cfg.is_none() {
                cargo_cfg = Some((name, text, table));
            }
        }
    }
    if present.len() > 1 {
        sink.whole_file(
            "AMBIGUOUS",
            ".cargo/config",
            "both .cargo/config and .cargo/config.toml exist: which one Cargo reads is ambiguous".to_string(),
        );
    }

    // Profiles: Cargo.toml, overridden by .cargo/config.toml.
    let manifest = read_text(&cfg.workspace.join("Cargo.toml"), "Cargo.toml")?
        .ok_or_else(|| invalid(format!("{}: Cargo.toml not found", cfg.workspace.display())))?;
    let manifest_t = parse_toml(&manifest, "Cargo.toml")?;
    texts.insert("Cargo.toml".to_string(), manifest.clone());
    // `[profile.*] rustflags` (Cargo's profile-rustflags) and any other rustflags key.
    check_cargo_config(&mut sink, "Cargo.toml", &manifest, &manifest_t);
    let mut sources = vec![ProfileSource {
        file: "Cargo.toml",
        idx: LineIndex::new(&manifest),
        profiles: manifest_t.get("profile").and_then(toml::Value::as_table),
    }];
    if let Some((name, text, table)) = &cargo_cfg {
        sources.push(ProfileSource {
            file: name,
            idx: LineIndex::new(text),
            profiles: table.get("profile").and_then(toml::Value::as_table),
        });
    }
    check_profiles(&mut sink, &sources);

    for f in &cfg.ci_env_files {
        let text = read_text(&f.path, &f.rel)?.ok_or_else(|| invalid(format!("{}: CI env file not found", f.rel)))?;
        check_ci_file(&mut sink, &f.rel, &text);
        keyword_gate(&mut sink, &f.rel, &text);
        texts.insert(f.rel.clone(), text);
    }

    if let Some(w) = &cfg.wasm_module {
        match read_bounded(&w.path, wasm::MAX_WASM_BYTES as u64) {
            Ok(Some(bytes)) => sink.out.extend(check_wasm(&bytes, &w.rel, &cfg.simd_policy)),
            Ok(None) => sink.whole_file("WASMPARSE", &w.rel, format!("{}: wasm reference module not found", w.rel)),
            Err(e) => sink.whole_file("WASMPARSE", &w.rel, e.message),
        }
    }

    Ok(apply_markers(sink.out, &texts))
}

/// Loads `knp1-lint.toml` at `config_path` and runs [`check`].
pub fn run(config_path: &Path) -> Result<Vec<Finding>, FlagsError> {
    check(&load_config(config_path)?)
}

/// Loads `knp1-lint.toml` at `config_path` and runs [`check_all`].
pub fn run_all(config_path: &Path) -> Result<Outcome, FlagsError> {
    check_all(&load_config(config_path)?)
}
