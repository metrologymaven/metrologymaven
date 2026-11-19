//! Kernel build-configuration and emitted-code FMA check. Spec node `KRN.NUM.LINT.FLAGS`.
//!
//! [`run`] reads the `[flags]` table of `knp1-lint.toml` and checks every place the kernel
//! build takes flags from, emitting [`Finding`]s of the `KNP1-FLAG-*` family:
//!
//! | Rule | Fires when |
//! |---|---|
//! | `KNP1-FLAG-NATIVE` | a flag source sets `-C target-cpu=native` |
//! | `KNP1-FLAG-CPU` | a flag source sets any other `-C target-cpu` not in `allowed_target_cpus` (named CPUs such as `haswell` imply FMA) |
//! | `KNP1-FLAG-FMA` | a flag source enables `+fma`, `+relaxed-simd` or a configured `fma_features` entry, or passes `fp-contract` other than `off` to LLVM |
//! | `KNP1-FLAG-CGU` | a kernel profile resolves (Cargo inheritance, built-in defaults) to `codegen-units` other than 1, or a flag source sets it |
//! | `KNP1-FLAG-TOOLCHAIN` | `rust-toolchain.toml` is missing, has no exact `channel = "X.Y.Z"`, or no `components` pin; a legacy `rust-toolchain` file exists; a CI env file sets `RUSTUP_TOOLCHAIN` or runs `cargo +<toolchain>` |
//! | `KNP1-FLAG-LTO` | two kernel profiles resolve to different `opt-level`, `lto`, `debug-assertions`, `overflow-checks` or `panic`, and the key is not in `allowed_profile_differences` |
//! | `KNP1-FLAG-WASMFMA` | the wasm reference module contains a relaxed multiply-add opcode |
//! | `KNP1-FLAG-WASMSIMD` | the wasm reference module contains a SIMD opcode the config does not allow |
//! | `KNP1-FLAG-WASMPARSE` | the wasm reference module is missing or cannot be parsed (fatal finding) |
//!
//! Flag sources: every key named `rustflags` (any case) or ending in `RUSTFLAGS` anywhere in
//! `.cargo/config.toml` (and the legacy `.cargo/config`) and in the workspace `Cargo.toml`
//! (`[profile.*] rustflags`), so `build.rustflags`,
//! `target.<triple>.rustflags`, `target.'cfg(..)'.rustflags`, `host.rustflags` and
//! `env.RUSTFLAGS`/`env.CARGO_ENCODED_RUSTFLAGS` are all covered; and every non-comment line of
//! each `ci_env_files` entry (a YAML block scalar can carry `RUSTFLAGS` over several lines, so
//! the whole file is scanned rather than only `RUSTFLAGS:` lines), including
//! `CARGO_PROFILE_<NAME>_CODEGEN_UNITS` for kernel profiles. Profiles come from `[profile.*]`
//! of the workspace `Cargo.toml`, overridden key by key by `[profile.*]` of `.cargo/config.toml`;
//! package-level `codegen-units` overrides are checked along the whole `inherits` chain.
//!
//! Flags are split into tokens on whitespace, the unit separator `0x1F` (and its escapes
//! `\x1f`, `\u001f`), quotes and brackets; commas around tokens (array syntax) are dropped and
//! an environment-assignment prefix (`RUSTFLAGS=-C…`, `export RUSTFLAGS=…`) is split off.
//! `-C k=v`, `-Ck=v`, `--codegen k=v` and `--codegen=k=v` are recognised, and any token that
//! mentions `fp-contract` is treated as an LLVM argument.
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
pub const BUILTIN_FMA_FEATURES: [&str; 2] = ["fma", "relaxed-simd"];
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
}

const CONFIG_KEYS: [&str; 8] = [
    "workspace",
    "profiles",
    "ci_env_files",
    "fma_features",
    "wasm_module",
    "allowed_simd",
    "allowed_profile_differences",
    "allowed_target_cpus",
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
    Ok(Config {
        workspace,
        profiles: profiles.into_iter().collect(),
        ci_env_files,
        fma_features: fma_features.into_iter().collect(),
        wasm_module,
        simd_policy,
        allowed_profile_differences: allowed.into_iter().collect(),
        allowed_target_cpus: allowed_target_cpus.into_iter().collect(),
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
        BUILTIN_FMA_FEATURES.contains(&f) || self.cfg.fma_features.iter().any(|x| x == f)
    }
}

// ------------------------------------------------------------------ flag tokens

/// One `-C key=value` codegen option found in a flag source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Codegen {
    pub key: String,
    pub value: String,
    /// The text to look for when placing the finding (`key=value`).
    pub needle: String,
}

/// `NAME=rest` with `NAME` a shell/env identifier: returns `rest`.
fn env_assignment(t: &str) -> Option<&str> {
    let (name, rest) = t.split_once('=')?;
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    (first_ok && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(rest)
}

/// Splits raw flag text into tokens (whitespace, `0x1F` and its escapes, quotes, brackets).
/// Commas around a token (array syntax) are dropped, comma-only tokens vanish, and an
/// environment assignment prefix is split off when what follows is empty or a flag
/// (`RUSTFLAGS=-Ctarget-cpu=native` gives `-Ctarget-cpu=native`; `-C lto=fat` is untouched).
pub fn tokens(text: &str) -> Vec<String> {
    let t = text.replace("\\x1f", " ").replace("\\x1F", " ").replace("\\u001f", " ").replace("\\u001F", " ");
    t.split(|c: char| c.is_whitespace() || matches!(c, '\u{1f}' | '"' | '\'' | '[' | ']'))
        .map(|s| s.trim_matches(','))
        .map(|s| match env_assignment(s) {
            Some(rest) if rest.is_empty() || rest.starts_with('-') => rest,
            _ => s,
        })
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Extracts the `-C key=value` options from a token list. Handles `-C k=v`, `-Ck=v`,
/// `--codegen k=v` and `--codegen=k=v`. Any other token that mentions `fp-contract` (an LLVM
/// argument split away from its `-C llvm-args=`) is returned as key `llvm-args`.
pub fn codegen_options(tokens: &[String]) -> Vec<Codegen> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens.get(i).map(String::as_str).unwrap_or("");
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
                out.push(Codegen { key: k.to_string(), value: v.to_string(), needle: format!("{k}={v}") })
            }
            None if opt.is_none() && t.contains("fp-contract") => {
                out.push(Codegen { key: "llvm-args".to_string(), value: t.to_string(), needle: t.to_string() })
            }
            None => {}
        }
        i += 1;
    }
    out
}

/// True when an LLVM argument string permits floating-point contraction: it mentions
/// `fp-contract` with any value other than `off` (or with no value).
fn llvm_args_contract(v: &str) -> bool {
    let mut rest = v;
    while let Some(i) = rest.find("fp-contract") {
        let after = rest.get(i + "fp-contract".len()..).unwrap_or("");
        let value: String = after
            .strip_prefix('=')
            .unwrap_or("")
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ',')
            .collect();
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
            "target-cpu" if !sink.cfg.allowed_target_cpus.iter().any(|c| *c == o.value) => {
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
                let (pos, text) = place(&o.needle);
                sink.push(
                    "CGU",
                    file,
                    pos,
                    text.as_bytes(),
                    format!("`-C {}`: kernel builds need one codegen unit", o.needle),
                );
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
    k.eq_ignore_ascii_case("rustflags") || k.ends_with("RUSTFLAGS")
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

fn check_ci_file(sink: &mut Sink<'_>, file: &str, text: &str) {
    let idx = LineIndex::new(text);
    let prof_vars: Vec<(String, String)> = sink
        .cfg
        .profiles
        .iter()
        .map(|p| (p.clone(), format!("CARGO_PROFILE_{}_CODEGEN_UNITS", p.to_ascii_uppercase().replace('-', "_"))))
        .collect();
    for (i, raw) in idx.lines.iter().enumerate() {
        if raw.trim_start().starts_with('#') {
            continue;
        }
        let line = line_no(i);
        let toks = tokens(raw);
        let opts = codegen_options(&toks);
        let mut place = |needle: &str| {
            let b = raw.find(needle).unwrap_or(0);
            ((line, col_no(raw, b)), (*raw).to_string())
        };
        eval_codegen(sink, file, &opts, &mut place);
        if let Some(b) = raw.find("RUSTUP_TOOLCHAIN") {
            sink.at_line(
                "TOOLCHAIN",
                file,
                &idx,
                (line, col_no(raw, b)),
                "`RUSTUP_TOOLCHAIN` overrides the pinned rust-toolchain.toml".to_string(),
            );
        }
        for w in toks.windows(2) {
            if let [cmd, tc] = w {
                if cmd == "cargo" && tc.starts_with('+') {
                    let b = raw.find(&format!("cargo {tc}")).or_else(|| raw.find(tc.as_str())).unwrap_or(0);
                    sink.at_line(
                        "TOOLCHAIN",
                        file,
                        &idx,
                        (line, col_no(raw, b)),
                        format!("`cargo {tc}` overrides the pinned rust-toolchain.toml"),
                    );
                }
            }
        }
        for (prof, var) in &prof_vars {
            if let Some(b) = raw.find(var.as_str()) {
                let rest = raw.get(b + var.len()..).unwrap_or("");
                let val = rest.trim_start_matches([':', '=', ' ', '\t']);
                let val: String = val.chars().filter(|c| !matches!(c, '"' | '\'')).collect();
                let val = val.split_whitespace().next().unwrap_or("");
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

// ------------------------------------------------------------------ run

/// Runs every check of the module against a parsed config. Returns sorted findings.
pub fn check(cfg: &Config) -> Result<Vec<Finding>, FlagsError> {
    let mut sink = Sink { cfg, out: Vec::new() };
    check_toolchain(&mut sink)?;

    // .cargo/config.toml (and legacy .cargo/config).
    let mut cargo_cfg: Option<(&'static str, String, toml::Table)> = None;
    for name in [".cargo/config.toml", ".cargo/config"] {
        if let Some(text) = read_text(&cfg.workspace.join(name), name)? {
            let table = parse_toml(&text, name)?;
            check_cargo_config(&mut sink, name, &text, &table);
            if cargo_cfg.is_none() {
                cargo_cfg = Some((name, text, table));
            }
        }
    }

    // Profiles: Cargo.toml, overridden by .cargo/config.toml.
    let manifest = read_text(&cfg.workspace.join("Cargo.toml"), "Cargo.toml")?
        .ok_or_else(|| invalid(format!("{}: Cargo.toml not found", cfg.workspace.display())))?;
    let manifest_t = parse_toml(&manifest, "Cargo.toml")?;
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
    }

    if let Some(w) = &cfg.wasm_module {
        match read_bounded(&w.path, wasm::MAX_WASM_BYTES as u64) {
            Ok(Some(bytes)) => sink.out.extend(check_wasm(&bytes, &w.rel, &cfg.simd_policy)),
            Ok(None) => sink.whole_file("WASMPARSE", &w.rel, format!("{}: wasm reference module not found", w.rel)),
            Err(e) => sink.whole_file("WASMPARSE", &w.rel, e.message),
        }
    }

    let mut out = sink.out;
    sort_findings(&mut out);
    Ok(out)
}

/// Loads `knp1-lint.toml` at `config_path` and runs [`check`].
pub fn run(config_path: &Path) -> Result<Vec<Finding>, FlagsError> {
    check(&load_config(config_path)?)
}
