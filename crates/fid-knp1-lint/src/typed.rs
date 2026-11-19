//! Type-aware banned-API check on expanded workspace code. Spec node `KRN.NUM.LINT.TYPED`.
//!
//! A syntax scan (`KRN.NUM.LINT.SYN`) cannot see the type of a receiver or code produced by
//! macro expansion. This module runs the pinned toolchain's `cargo clippy` over every workspace
//! crate of the kernel closure, once per kernel target, with a generated `clippy.toml`
//! ([`clippy_toml`], from the pinned table [`TYPED_TABLE`]) whose disallowed methods, types and
//! macros carry their KNP-1 rule ID as the `reason`, and maps the JSON diagnostics back to
//! [`Finding`]s.
//!
//! Fail closed:
//!
//! - The three lints run with `--force-warn`, so `#[allow(clippy::disallowed_methods)]` in the
//!   source cannot silence them, and as warnings, so a hit does not stop the build of the
//!   crates that depend on it. Every other clippy lint is allowed.
//! - `--keep-going` builds every crate it can; a crate is counted as linted for a target only if
//!   cargo reports a compiled artifact for it. A crate with a compile error, or with no
//!   artifact (lockfile out of date, target not installed, timeout, unreadable driver output),
//!   is `KNP1-TYPED-BUILD` for that target. `KNP1-TYPED-BUILD` cannot be waived.
//! - A missing driver, a `cargo clippy --version` that does not match the pinned version, or a
//!   disallowed-item diagnostic that names no KNP-1 rule is fatal `KNP1-TYPED-DRIVER`.
//!
//! Each finding is placed at the outermost span of the macro expansion chain that lies inside
//! the crate (the macro call site for an item produced by a local macro), deduplicated across
//! targets by (crate, version, file, line, column, rule). `KNP1-LIBM` findings in the facade
//! files listed in `[syn] facade_files` (crate, version, file) are notes, not findings.
//!
//! Config (`[typed]` of `knp1-lint.toml`):
//!
//! ```toml
//! [typed]
//! clippy_version = "0.1.89"   # required: second word of `cargo clippy --version`
//! targets = ["aarch64-unknown-linux-gnu", "wasm32-unknown-unknown", "x86_64-unknown-linux-gnu"]
//! timeout_secs = 1800          # per target
//! ```

use crate::findings::{input_digest, sort_findings, Finding, Rule};
pub use crate::flags::Outcome;
use crate::graph::{self, Closure, CrateEntry, SourceKind};
use crate::syn_scan;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Kernel targets linted when the config names none, sorted.
pub const DEFAULT_TARGETS: [&str; 3] =
    ["aarch64-unknown-linux-gnu", "wasm32-unknown-unknown", "x86_64-unknown-linux-gnu"];
/// Default per-target timeout, in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800;
/// Most bytes kept from the driver's stdout or stderr per run.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024 * 1024;
/// Longest macro expansion chain followed for one diagnostic.
pub const MAX_EXPANSION_DEPTH: usize = 256;
/// Clippy lints the check forces on.
pub const LINTS: [&str; 3] = ["clippy::disallowed_methods", "clippy::disallowed_types", "clippy::disallowed_macros"];

// ------------------------------------------------------------------ the pinned table

/// Which `clippy.toml` list an entry goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Method,
    Type,
    Macro,
}

/// Number of rows in [`TYPED_TABLE`].
pub const TYPED_TABLE_LEN: usize = 88;

/// The pinned table: `(rule, kind, path)`. Mirrored in the leaf's NODE.md Design.
pub static TYPED_TABLE: [(Rule, ItemKind, &str); TYPED_TABLE_LEN] = [
    (Rule::Fma, ItemKind::Method, "f32::mul_add"),
    (Rule::Fma, ItemKind::Method, "f64::mul_add"),
    (Rule::Pow, ItemKind::Method, "f32::powf"),
    (Rule::Pow, ItemKind::Method, "f32::powi"),
    (Rule::Pow, ItemKind::Method, "f64::powf"),
    (Rule::Pow, ItemKind::Method, "f64::powi"),
    (Rule::Libm, ItemKind::Method, "f32::acos"),
    (Rule::Libm, ItemKind::Method, "f32::acosh"),
    (Rule::Libm, ItemKind::Method, "f32::asin"),
    (Rule::Libm, ItemKind::Method, "f32::asinh"),
    (Rule::Libm, ItemKind::Method, "f32::atan"),
    (Rule::Libm, ItemKind::Method, "f32::atan2"),
    (Rule::Libm, ItemKind::Method, "f32::atanh"),
    (Rule::Libm, ItemKind::Method, "f32::cbrt"),
    (Rule::Libm, ItemKind::Method, "f32::cos"),
    (Rule::Libm, ItemKind::Method, "f32::cosh"),
    (Rule::Libm, ItemKind::Method, "f32::exp"),
    (Rule::Libm, ItemKind::Method, "f32::exp2"),
    (Rule::Libm, ItemKind::Method, "f32::exp_m1"),
    (Rule::Libm, ItemKind::Method, "f32::hypot"),
    (Rule::Libm, ItemKind::Method, "f32::ln"),
    (Rule::Libm, ItemKind::Method, "f32::ln_1p"),
    (Rule::Libm, ItemKind::Method, "f32::log"),
    (Rule::Libm, ItemKind::Method, "f32::log10"),
    (Rule::Libm, ItemKind::Method, "f32::log2"),
    (Rule::Libm, ItemKind::Method, "f32::sin"),
    (Rule::Libm, ItemKind::Method, "f32::sin_cos"),
    (Rule::Libm, ItemKind::Method, "f32::sinh"),
    (Rule::Libm, ItemKind::Method, "f32::sqrt"),
    (Rule::Libm, ItemKind::Method, "f32::tan"),
    (Rule::Libm, ItemKind::Method, "f32::tanh"),
    (Rule::Libm, ItemKind::Method, "f64::acos"),
    (Rule::Libm, ItemKind::Method, "f64::acosh"),
    (Rule::Libm, ItemKind::Method, "f64::asin"),
    (Rule::Libm, ItemKind::Method, "f64::asinh"),
    (Rule::Libm, ItemKind::Method, "f64::atan"),
    (Rule::Libm, ItemKind::Method, "f64::atan2"),
    (Rule::Libm, ItemKind::Method, "f64::atanh"),
    (Rule::Libm, ItemKind::Method, "f64::cbrt"),
    (Rule::Libm, ItemKind::Method, "f64::cos"),
    (Rule::Libm, ItemKind::Method, "f64::cosh"),
    (Rule::Libm, ItemKind::Method, "f64::exp"),
    (Rule::Libm, ItemKind::Method, "f64::exp2"),
    (Rule::Libm, ItemKind::Method, "f64::exp_m1"),
    (Rule::Libm, ItemKind::Method, "f64::hypot"),
    (Rule::Libm, ItemKind::Method, "f64::ln"),
    (Rule::Libm, ItemKind::Method, "f64::ln_1p"),
    (Rule::Libm, ItemKind::Method, "f64::log"),
    (Rule::Libm, ItemKind::Method, "f64::log10"),
    (Rule::Libm, ItemKind::Method, "f64::log2"),
    (Rule::Libm, ItemKind::Method, "f64::sin"),
    (Rule::Libm, ItemKind::Method, "f64::sin_cos"),
    (Rule::Libm, ItemKind::Method, "f64::sinh"),
    (Rule::Libm, ItemKind::Method, "f64::sqrt"),
    (Rule::Libm, ItemKind::Method, "f64::tan"),
    (Rule::Libm, ItemKind::Method, "f64::tanh"),
    (Rule::MinMax, ItemKind::Method, "f32::clamp"),
    (Rule::MinMax, ItemKind::Method, "f32::max"),
    (Rule::MinMax, ItemKind::Method, "f32::min"),
    (Rule::MinMax, ItemKind::Method, "f64::clamp"),
    (Rule::MinMax, ItemKind::Method, "f64::max"),
    (Rule::MinMax, ItemKind::Method, "f64::min"),
    (Rule::IoFs, ItemKind::Method, "std::fs::read"),
    (Rule::IoFs, ItemKind::Method, "std::fs::read_to_string"),
    (Rule::IoFs, ItemKind::Method, "std::fs::write"),
    (Rule::Clock, ItemKind::Method, "std::time::Instant::now"),
    (Rule::Clock, ItemKind::Method, "std::time::SystemTime::now"),
    (Rule::Env, ItemKind::Method, "std::env::var"),
    (Rule::Env, ItemKind::Method, "std::env::var_os"),
    (Rule::Env, ItemKind::Method, "std::env::vars"),
    (Rule::Thread, ItemKind::Method, "std::process::Command::new"),
    (Rule::Thread, ItemKind::Method, "std::thread::spawn"),
    (Rule::Hash, ItemKind::Type, "std::collections::HashMap"),
    (Rule::Hash, ItemKind::Type, "std::collections::HashSet"),
    (Rule::Hash, ItemKind::Type, "std::collections::hash_map::DefaultHasher"),
    (Rule::Hash, ItemKind::Type, "std::collections::hash_map::RandomState"),
    (Rule::IoFs, ItemKind::Type, "std::fs::File"),
    (Rule::IoNet, ItemKind::Type, "std::net::TcpListener"),
    (Rule::IoNet, ItemKind::Type, "std::net::TcpStream"),
    (Rule::IoNet, ItemKind::Type, "std::net::UdpSocket"),
    (Rule::Clock, ItemKind::Type, "std::time::Instant"),
    (Rule::Clock, ItemKind::Type, "std::time::SystemTime"),
    (Rule::Simd, ItemKind::Macro, "std::arch::is_aarch64_feature_detected"),
    (Rule::Simd, ItemKind::Macro, "std::is_x86_feature_detected"),
    (Rule::Env, ItemKind::Macro, "std::env"),
    (Rule::Env, ItemKind::Macro, "std::option_env"),
    (Rule::IoFs, ItemKind::Macro, "std::include_bytes"),
    (Rule::IoFs, ItemKind::Macro, "std::include_str"),
];

/// The generated `clippy.toml`: three lists, each entry `{ path, reason }` with the rule ID as
/// the reason, in table order. Deterministic.
pub fn clippy_toml() -> String {
    let mut out = String::from("# Generated by fid-knp1-lint (KRN.NUM.LINT.TYPED). Do not edit.\n");
    for (key, kind) in [
        ("disallowed-methods", ItemKind::Method),
        ("disallowed-types", ItemKind::Type),
        ("disallowed-macros", ItemKind::Macro),
    ] {
        out.push_str(key);
        out.push_str(" = [\n");
        for (rule, _, path) in TYPED_TABLE.iter().filter(|(_, k, _)| *k == kind) {
            out.push_str(&format!("    {{ path = \"{path}\", reason = \"{}\" }},\n", rule.id()));
        }
        out.push_str("]\n");
    }
    out
}

fn table_rule(id: &str) -> Option<Rule> {
    TYPED_TABLE.iter().map(|(r, _, _)| r).find(|r| r.id() == id).cloned()
}

// ------------------------------------------------------------------ errors and config

/// Fatal error codes of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedErrorCode {
    /// The driver is missing, has the wrong version, or emits a diagnostic the check cannot map.
    Driver,
    /// The `[typed]` (or `[syn]`) config is malformed.
    ConfigInvalid,
    /// The scratch directory cannot be prepared.
    Io,
    /// The closure could not be computed (`KRN.NUM.LINT.GRAPH`).
    Graph(graph::GraphErrorCode),
}

impl TypedErrorCode {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            TypedErrorCode::Driver => "KNP1-TYPED-DRIVER",
            TypedErrorCode::ConfigInvalid => "KNP1-CONFIG-INVALID",
            TypedErrorCode::Io => "KNP1-TYPED-IO",
            TypedErrorCode::Graph(g) => g.as_str(),
        }
    }
}

/// A fatal error of the type-aware check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedError {
    pub code: TypedErrorCode,
    pub message: String,
}

impl fmt::Display for TypedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for TypedError {}

fn err(code: TypedErrorCode, m: impl Into<String>) -> TypedError {
    TypedError { code, message: m.into() }
}

fn invalid(m: impl Into<String>) -> TypedError {
    err(TypedErrorCode::ConfigInvalid, m)
}

fn driver(m: impl Into<String>) -> TypedError {
    err(TypedErrorCode::Driver, m)
}

/// The `[typed]` table of `knp1-lint.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Pinned clippy version: the second word of `cargo clippy --version`.
    pub clippy_version: String,
    /// Target triples, sorted and unique.
    pub targets: Vec<String>,
    pub timeout_secs: u64,
}

fn valid_triple(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 128
        && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.'))
}

/// Parses the `[typed]` table (required). Unknown keys, a missing `clippy_version`, an empty or
/// duplicated target list and a zero timeout are `KNP1-CONFIG-INVALID`.
pub fn parse_config(text: &str) -> Result<Config, TypedError> {
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| invalid(format!("knp1-lint.toml: {}", e.to_string().trim_end())))?;
    let Some(toml::Value::Table(t)) = table.get("typed") else {
        return Err(invalid("knp1-lint.toml: missing [typed] table"));
    };
    if let Some(k) = t.keys().find(|k| !["clippy_version", "targets", "timeout_secs"].contains(&k.as_str())) {
        return Err(invalid(format!("knp1-lint.toml: unknown key typed.{k}")));
    }
    let clippy_version = match t.get("clippy_version").and_then(toml::Value::as_str) {
        Some(v) if !v.is_empty() && v.len() <= 64 && !v.chars().any(|c| c.is_whitespace() || c.is_control()) => {
            v.to_string()
        }
        _ => return Err(invalid("knp1-lint.toml: typed.clippy_version must pin the clippy version (e.g. \"0.1.89\")")),
    };
    let targets = match t.get("targets") {
        None => DEFAULT_TARGETS.iter().map(|s| (*s).to_string()).collect(),
        Some(toml::Value::Array(a)) => {
            let mut set = BTreeSet::new();
            for v in a {
                match v.as_str() {
                    Some(s) if valid_triple(s) => {
                        if !set.insert(s.to_string()) {
                            return Err(invalid(format!("knp1-lint.toml: typed.targets lists {s:?} twice")));
                        }
                    }
                    _ => return Err(invalid("knp1-lint.toml: typed.targets must hold target triples")),
                }
            }
            if set.is_empty() {
                return Err(invalid("knp1-lint.toml: typed.targets must name at least one target"));
            }
            set.into_iter().collect()
        }
        Some(_) => return Err(invalid("knp1-lint.toml: typed.targets must be an array of target triples")),
    };
    let timeout_secs = match t.get("timeout_secs") {
        None => DEFAULT_TIMEOUT_SECS,
        Some(toml::Value::Integer(n)) if *n > 0 => u64::try_from(*n).unwrap_or(DEFAULT_TIMEOUT_SECS),
        Some(_) => return Err(invalid("knp1-lint.toml: typed.timeout_secs must be a positive integer")),
    };
    Ok(Config { clippy_version, targets, timeout_secs })
}

// ------------------------------------------------------------------ JSON helpers

fn get<'v>(v: &'v CValue, key: &str) -> Option<&'v CValue> {
    match v {
        CValue::Obj(o) => o.get(key),
        _ => None,
    }
}

fn get_str<'v>(v: &'v CValue, key: &str) -> Option<&'v str> {
    match get(v, key)? {
        CValue::Str(s) => Some(s),
        _ => None,
    }
}

fn get_pos(v: &CValue, key: &str) -> Option<u32> {
    match get(v, key)? {
        CValue::Num(n) if n.fract() == 0.0 && *n >= 1.0 && *n <= f64::from(u32::MAX) => {
            // Integral and in range: the conversion is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Some(*n as u32)
        }
        _ => None,
    }
}

fn get_arr<'v>(v: &'v CValue, key: &str) -> &'v [CValue] {
    match get(v, key) {
        Some(CValue::Arr(a)) => a,
        _ => &[],
    }
}

// ------------------------------------------------------------------ mapping diagnostics

/// One crate of the run: identity and canonical root directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateRef {
    pub name: String,
    pub version: String,
    /// Canonical crate root (the directory of its `Cargo.toml`).
    pub root: PathBuf,
}

/// Mapping context: the directory cargo ran in and the crates of the run.
#[derive(Debug, Clone)]
pub struct MapCtx {
    pub cwd: PathBuf,
    pub crates: Vec<CrateRef>,
}

/// What one driver run reported for one target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    /// Disallowed-item findings (before the facade split).
    pub findings: Vec<Finding>,
    /// Crates with a compiled artifact.
    pub built: BTreeSet<String>,
    /// Crates with a compile error: first error message.
    pub errors: BTreeMap<String, String>,
    /// True when some stdout line could not be read as a cargo message.
    pub unreadable: bool,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

impl MapCtx {
    fn crate_of_manifest(&self, manifest: &str) -> Option<&CrateRef> {
        let dir = canon(Path::new(manifest).parent()?);
        self.crates.iter().find(|c| c.root == dir)
    }

    /// `file_name` of a span as a `/`-separated path relative to `c`'s root, if it is inside it.
    fn relative(&self, c: &CrateRef, file_name: &str) -> Option<String> {
        let p = Path::new(file_name);
        let abs = if p.is_absolute() { p.to_path_buf() } else { self.cwd.join(p) };
        let abs = canon(&abs);
        let rel = abs.strip_prefix(&c.root).ok()?;
        let parts: Option<Vec<&str>> = rel.components().map(|x| x.as_os_str().to_str()).collect();
        let parts = parts?;
        if parts.is_empty() || parts.iter().any(|s| *s == ".." || s.is_empty()) {
            return None;
        }
        Some(parts.join("/"))
    }
}

fn line_digest(root: &Path, file: &str, line: u32, fallback: &str) -> String {
    let text = std::fs::read(root.join(file)).ok().and_then(|b| String::from_utf8(b).ok());
    let l = text.as_deref().and_then(|t| t.split('\n').nth((line as usize).saturating_sub(1)));
    match l {
        Some(l) => input_digest(l.strip_suffix('\r').unwrap_or(l).as_bytes()),
        None => input_digest(fallback.as_bytes()),
    }
}

/// The KNP-1 rule a disallowed-item diagnostic names in its message or notes.
fn rule_of(msg: &CValue) -> Option<Rule> {
    let mut texts: Vec<&str> = get_str(msg, "message").into_iter().collect();
    for c in get_arr(msg, "children") {
        texts.extend(get_str(c, "message"));
    }
    for t in texts {
        for word in t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
            if word.starts_with("KNP1-") {
                if let Some(r) = table_rule(word) {
                    return Some(r);
                }
            }
        }
    }
    None
}

/// Parses one target's `--message-format=json` stdout. Pure (reads source lines for digests).
/// Fails with `KNP1-TYPED-DRIVER` only for a disallowed-item diagnostic that names no rule.
pub fn parse_messages(stdout: &[u8], target: &str, ctx: &MapCtx) -> Result<Parsed, TypedError> {
    let mut out = Parsed::default();
    for line in stdout.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(v) = fid_canon::parse(line) else {
            out.unreadable = true;
            continue;
        };
        let reason = get_str(&v, "reason").unwrap_or_default();
        let krate = get_str(&v, "manifest_path").and_then(|m| ctx.crate_of_manifest(m));
        match reason {
            "compiler-artifact" => {
                if let Some(c) = krate {
                    out.built.insert(c.name.clone());
                }
            }
            "compiler-message" => {
                let Some(c) = krate else { continue };
                let Some(msg) = get(&v, "message") else {
                    out.unreadable = true;
                    continue;
                };
                let code = get(msg, "code").and_then(|c| get_str(c, "code")).unwrap_or_default();
                let level = get_str(msg, "level").unwrap_or_default();
                if LINTS.contains(&code) {
                    let rule = rule_of(msg).ok_or_else(|| {
                        driver(format!(
                            "{target}: {code} diagnostic names no KNP-1 rule: {}",
                            get_str(msg, "message").unwrap_or_default()
                        ))
                    })?;
                    out.findings.push(finding_at(ctx, c, msg, rule, target));
                } else if level == "error" && !out.errors.contains_key(&c.name) {
                    let text = get_str(msg, "message").unwrap_or("compile error").to_string();
                    out.errors.insert(c.name.clone(), text);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The finding of one disallowed-item diagnostic, at the outermost in-crate span of the
/// expansion chain of its primary span.
fn finding_at(ctx: &MapCtx, c: &CrateRef, msg: &CValue, rule: Rule, target: &str) -> Finding {
    let text = get_str(msg, "message").unwrap_or_default();
    let spans = get_arr(msg, "spans");
    let primary = spans.iter().find(|s| matches!(get(s, "is_primary"), Some(CValue::Bool(true)))).or(spans.first());
    let mut chosen: Option<(String, u32, u32)> = None;
    let mut span = primary;
    for _ in 0..MAX_EXPANSION_DEPTH {
        let Some(s) = span else { break };
        if let (Some(f), Some(l), Some(col)) =
            (get_str(s, "file_name"), get_pos(s, "line_start"), get_pos(s, "column_start"))
        {
            if let Some(rel) = ctx.relative(c, f) {
                chosen = Some((rel, l, col));
            }
        }
        span = get(s, "expansion").and_then(|e| get(e, "span"));
    }
    let message = format!("{text} ({target}, type-aware)");
    match chosen {
        Some((file, line, column)) => Finding {
            excerpt_digest: line_digest(&c.root, &file, line, &message),
            rule,
            crate_name: c.name.clone(),
            crate_version: c.version.clone(),
            file,
            line,
            column,
            message,
        },
        None => {
            let m = format!("{message}; no span inside the crate");
            Finding {
                excerpt_digest: input_digest(m.as_bytes()),
                rule,
                crate_name: c.name.clone(),
                crate_version: c.version.clone(),
                file: "Cargo.toml".to_string(),
                line: 1,
                column: 1,
                message: m,
            }
        }
    }
}

fn build_finding(c: &CrateRef, target: &str, why: &str) -> Finding {
    let message = format!("{} {} was not linted for {target}: {why}", c.name, c.version);
    let mut short: String = message.chars().take(2000).collect();
    if short.len() < message.len() {
        short.push('…');
    }
    Finding {
        rule: Rule::Typed("BUILD".to_string()),
        crate_name: c.name.clone(),
        crate_version: c.version.clone(),
        file: "Cargo.toml".to_string(),
        line: 1,
        column: 1,
        excerpt_digest: input_digest(format!("{} {} {target}", c.name, c.version).as_bytes()),
        message: short,
    }
}

/// Turns one target's run into findings: disallowed items, plus `KNP1-TYPED-BUILD` for every
/// crate with a compile error or without an artifact.
pub fn target_findings(parsed: &Parsed, ctx: &MapCtx, target: &str, failure: Option<&str>) -> Vec<Finding> {
    let mut out = parsed.findings.clone();
    for c in &ctx.crates {
        if let Some(e) = parsed.errors.get(&c.name) {
            out.push(build_finding(c, target, &format!("compile error: {e}")));
        } else if !parsed.built.contains(&c.name) {
            let why = failure.unwrap_or(if parsed.unreadable {
                "unreadable driver output and no compiled artifact"
            } else {
                "no compiled artifact"
            });
            out.push(build_finding(c, target, why));
        }
    }
    out
}

/// Deduplicates across targets by (crate, version, file, line, column, rule) and moves
/// `KNP1-LIBM` findings in `[syn] facade_files` to notes.
pub fn finish(findings: Vec<Finding>, syn_cfg: &syn_scan::Config) -> Outcome {
    let mut seen = BTreeSet::new();
    let mut out = Outcome::default();
    for f in findings {
        let key = (f.crate_name.clone(), f.crate_version.clone(), f.file.clone(), f.line, f.column, f.rule.id());
        if !seen.insert(key) {
            continue;
        }
        match syn_cfg.facade_file(&f.crate_name, &f.crate_version, &f.file) {
            Some(reason) if f.rule == Rule::Libm => {
                let message = format!("opt-out (facade file: {reason}): {}", f.message);
                out.notes.push(Finding { message, ..f });
            }
            _ => out.findings.push(f),
        }
    }
    sort_findings(&mut out.findings);
    sort_findings(&mut out.notes);
    out
}

// ------------------------------------------------------------------ running the driver

struct RunOutput {
    status: Option<ExitStatus>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn read_capped<R: Read>(r: Option<R>) -> Vec<u8> {
    let Some(mut r) = r else { return Vec::new() };
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // Keep draining past the cap so the child never blocks on a full pipe.
                let room = MAX_OUTPUT_BYTES.saturating_sub(out.len());
                out.extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
            }
        }
    }
    out
}

fn run_bounded(mut cmd: Command, timeout: Duration) -> std::io::Result<RunOutput> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let out = child.stdout.take();
    let errp = child.stderr.take();
    let h_out = std::thread::spawn(move || read_capped(out));
    let h_err = std::thread::spawn(move || read_capped(errp));
    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(s) => break Some(s),
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let stdout = h_out.join().unwrap_or_default();
    let stderr = h_err.join().unwrap_or_default();
    Ok(RunOutput { status, stdout, stderr })
}

/// The second word of `cargo clippy --version` (for example `0.1.89`), or `KNP1-TYPED-DRIVER`.
pub fn driver_version(cargo: &Path) -> Result<String, TypedError> {
    let mut cmd = Command::new(cargo);
    cmd.args(["clippy", "--version"]);
    let out = run_bounded(cmd, Duration::from_secs(120))
        .map_err(|e| driver(format!("cannot run {} clippy --version: {e}", cargo.display())))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let ok = out.status.is_some_and(|s| s.success());
    let mut words = text.split_whitespace();
    match (ok, words.next(), words.next()) {
        (true, Some("clippy"), Some(v)) => Ok(v.to_string()),
        _ => Err(driver(format!(
            "`cargo clippy --version` failed or printed no version: {}{}",
            text.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

/// How to run the driver.
#[derive(Debug, Clone)]
pub struct Options {
    /// The pinned toolchain's `cargo`.
    pub cargo: PathBuf,
    /// Scratch directory for the generated `clippy.toml` and the target directory.
    pub scratch: PathBuf,
}

/// Lints the workspace crates of the closure for every configured target.
pub fn check(
    closure: &Closure,
    workspace: &Path,
    cfg: &Config,
    syn_cfg: &syn_scan::Config,
    opts: &Options,
) -> Result<Outcome, TypedError> {
    let version = driver_version(&opts.cargo)?;
    if version != cfg.clippy_version {
        return Err(driver(format!("clippy {version} is not the pinned {}", cfg.clippy_version)));
    }
    let members: Vec<&CrateEntry> = closure.crates.iter().filter(|c| c.source_kind == SourceKind::Workspace).collect();
    if members.is_empty() {
        return Ok(Outcome::default());
    }
    let conf = opts.scratch.join("clippy-conf");
    std::fs::create_dir_all(&conf).map_err(|e| err(TypedErrorCode::Io, format!("{}: {e}", conf.display())))?;
    std::fs::write(conf.join("clippy.toml"), clippy_toml())
        .map_err(|e| err(TypedErrorCode::Io, format!("{}: {e}", conf.display())))?;
    let ctx = MapCtx {
        cwd: canon(workspace),
        crates: members
            .iter()
            .map(|c| CrateRef { name: c.name.clone(), version: c.version.clone(), root: canon(&c.root) })
            .collect(),
    };
    let mut all = Vec::new();
    for target in &cfg.targets {
        let mut cmd = Command::new(&opts.cargo);
        cmd.current_dir(workspace)
            .env("CLIPPY_CONF_DIR", &conf)
            .env("CARGO_TERM_COLOR", "never")
            .env_remove("CLIPPY_ARGS")
            .arg("clippy")
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .args(["--locked", "--keep-going", "--message-format=json", "--target", target])
            .arg("--target-dir")
            .arg(opts.scratch.join("target"));
        for c in &members {
            cmd.arg("-p").arg(format!("{}@{}", c.name, c.version));
        }
        cmd.args(["--", "-A", "clippy::all"]);
        for l in LINTS {
            cmd.args(["--force-warn", l]);
        }
        let out = run_bounded(cmd, Duration::from_secs(cfg.timeout_secs))
            .map_err(|e| driver(format!("cannot run {} clippy: {e}", opts.cargo.display())))?;
        let failure = if out.status.is_none() {
            Some(format!("timed out after {} s", cfg.timeout_secs))
        } else if out.status.is_some_and(|s| !s.success()) {
            let tail: String =
                String::from_utf8_lossy(&out.stderr).lines().rev().take(5).collect::<Vec<_>>().join(" | ");
            Some(format!("driver failed: {tail}"))
        } else {
            None
        };
        let parsed = parse_messages(&out.stdout, target, &ctx)?;
        all.extend(target_findings(&parsed, &ctx, target, failure.as_deref()));
    }
    Ok(finish(all, syn_cfg))
}

/// Loads `knp1-lint.toml` at `config_path` (`[graph]`, `[syn]`, `[typed]`), computes the closure
/// and runs [`check`].
pub fn run(config_path: &Path, opts: &Options) -> Result<Outcome, TypedError> {
    let text = std::fs::read(config_path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| invalid(format!("{}: cannot read as UTF-8", config_path.display())))?;
    let cfg = parse_config(&text)?;
    let syn_cfg = syn_scan::parse_config(&text).map_err(|e| invalid(e.message))?;
    let g = |e: graph::GraphError| err(TypedErrorCode::Graph(e.code), e.message);
    let gcfg = graph::load_config(config_path).map_err(g)?;
    let closure = graph::compute_closure(&gcfg).map_err(g)?;
    check(&closure, &gcfg.workspace, &cfg, &syn_cfg, opts)
}
