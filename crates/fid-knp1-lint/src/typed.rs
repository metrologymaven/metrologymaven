//! Type-aware banned-API check on expanded workspace code. Spec node `KRN.NUM.LINT.TYPED`.
//!
//! A syntax scan (`KRN.NUM.LINT.SYN`) cannot see the type of a receiver or code produced by
//! macro expansion. This module runs the pinned toolchain's `cargo clippy` over every workspace
//! crate of the kernel closure, once per kernel target, with a generated `clippy.toml`
//! ([`clippy_toml`], from the pinned table [`TYPED_TABLE`]) whose disallowed methods, types and
//! macros carry their KNP-1 rule ID as the `reason`, and maps the JSON diagnostics back to
//! [`Finding`]s.
//!
//! Fail closed (the decisions are pinned in the leaf's NODE.md Design):
//!
//! - The three lints run with `--force-warn`, so `#[allow(clippy::disallowed_methods)]` in the
//!   source cannot silence them, and as warnings, so a hit does not stop the build of the
//!   crates that depend on it. Every other clippy lint is allowed. Each target is linted with
//!   the default features and with `--all-features`. Compiler flags from the environment and
//!   from `.cargo/config.toml` are neutralised (`CARGO_ENCODED_RUSTFLAGS` set empty, wrappers
//!   and `RUSTFLAGS` variants removed).
//! - `--keep-going` builds every crate it can; a crate counts as linted for a run only if cargo
//!   reports a compiled library, binary or proc-macro artifact for it (not a build script). A
//!   crate with a compile error or without such an artifact is `KNP1-TYPED-BUILD`; a finding
//!   with no span inside the crate is `KNP1-TYPED-NOSPAN`. Neither can be waived.
//! - A missing driver, a version that is not the pinned one, an unreadable line of driver
//!   output, a disallowed-item diagnostic that names no KNP-1 rule, or a table path the driver
//!   cannot resolve (outside [`UNRESOLVED_ALLOWED`]) is fatal `KNP1-TYPED-DRIVER`.
//!
//! Each finding is placed at the outermost span of the macro expansion chain that lies inside
//! the crate (the macro call site for an item produced by a local macro), deduplicated across
//! runs by (crate, version, file, line, column, rule). `KNP1-LIBM` findings in the facade
//! files listed in `[syn] facade_files` (crate, version, file) are notes, not findings.
//!
//! Config (`[typed]` of `knp1-lint.toml`):
//!
//! ```toml
//! [typed]
//! clippy_version = "0.1.89"   # required: second word of `cargo clippy --version`
//! targets = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"]   # default: DEFAULT_TARGETS
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
pub const DEFAULT_TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "wasm32-unknown-unknown",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
];
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
pub const TYPED_TABLE_LEN: usize = 132;

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
    (Rule::IoFs, ItemKind::Method, "std::fs::canonicalize"),
    (Rule::IoFs, ItemKind::Method, "std::fs::copy"),
    (Rule::IoFs, ItemKind::Method, "std::fs::create_dir"),
    (Rule::IoFs, ItemKind::Method, "std::fs::create_dir_all"),
    (Rule::IoFs, ItemKind::Method, "std::fs::exists"),
    (Rule::IoFs, ItemKind::Method, "std::fs::hard_link"),
    (Rule::IoFs, ItemKind::Method, "std::fs::metadata"),
    (Rule::IoFs, ItemKind::Method, "std::fs::read"),
    (Rule::IoFs, ItemKind::Method, "std::fs::read_dir"),
    (Rule::IoFs, ItemKind::Method, "std::fs::read_link"),
    (Rule::IoFs, ItemKind::Method, "std::fs::read_to_string"),
    (Rule::IoFs, ItemKind::Method, "std::fs::remove_dir"),
    (Rule::IoFs, ItemKind::Method, "std::fs::remove_dir_all"),
    (Rule::IoFs, ItemKind::Method, "std::fs::remove_file"),
    (Rule::IoFs, ItemKind::Method, "std::fs::rename"),
    (Rule::IoFs, ItemKind::Method, "std::fs::set_permissions"),
    (Rule::IoFs, ItemKind::Method, "std::fs::symlink_metadata"),
    (Rule::IoFs, ItemKind::Method, "std::fs::write"),
    (Rule::Clock, ItemKind::Method, "std::time::Instant::now"),
    (Rule::Clock, ItemKind::Method, "std::time::SystemTime::now"),
    (Rule::Env, ItemKind::Method, "std::env::args"),
    (Rule::Env, ItemKind::Method, "std::env::args_os"),
    (Rule::Env, ItemKind::Method, "std::env::current_dir"),
    (Rule::Env, ItemKind::Method, "std::env::current_exe"),
    (Rule::Env, ItemKind::Method, "std::env::home_dir"),
    (Rule::Env, ItemKind::Method, "std::env::join_paths"),
    (Rule::Env, ItemKind::Method, "std::env::remove_var"),
    (Rule::Env, ItemKind::Method, "std::env::set_current_dir"),
    (Rule::Env, ItemKind::Method, "std::env::set_var"),
    (Rule::Env, ItemKind::Method, "std::env::split_paths"),
    (Rule::Env, ItemKind::Method, "std::env::temp_dir"),
    (Rule::Env, ItemKind::Method, "std::env::var"),
    (Rule::Env, ItemKind::Method, "std::env::var_os"),
    (Rule::Env, ItemKind::Method, "std::env::vars"),
    (Rule::Env, ItemKind::Method, "std::env::vars_os"),
    (Rule::Thread, ItemKind::Method, "std::process::Command::new"),
    (Rule::Thread, ItemKind::Method, "std::process::abort"),
    (Rule::Thread, ItemKind::Method, "std::process::exit"),
    (Rule::Thread, ItemKind::Method, "std::process::id"),
    (Rule::Thread, ItemKind::Method, "std::thread::available_parallelism"),
    (Rule::Thread, ItemKind::Method, "std::thread::current"),
    (Rule::Thread, ItemKind::Method, "std::thread::park"),
    (Rule::Thread, ItemKind::Method, "std::thread::park_timeout"),
    (Rule::Thread, ItemKind::Method, "std::thread::scope"),
    (Rule::Thread, ItemKind::Method, "std::thread::sleep"),
    (Rule::Thread, ItemKind::Method, "std::thread::spawn"),
    (Rule::Thread, ItemKind::Method, "std::thread::yield_now"),
    (Rule::Hash, ItemKind::Type, "std::collections::HashMap"),
    (Rule::Hash, ItemKind::Type, "std::collections::HashSet"),
    (Rule::Hash, ItemKind::Type, "std::collections::hash_map::DefaultHasher"),
    (Rule::Hash, ItemKind::Type, "std::collections::hash_map::RandomState"),
    (Rule::IoFs, ItemKind::Type, "std::fs::DirBuilder"),
    (Rule::IoFs, ItemKind::Type, "std::fs::File"),
    (Rule::IoFs, ItemKind::Type, "std::fs::OpenOptions"),
    (Rule::IoNet, ItemKind::Type, "std::net::TcpListener"),
    (Rule::IoNet, ItemKind::Type, "std::net::TcpStream"),
    (Rule::IoNet, ItemKind::Type, "std::net::UdpSocket"),
    (Rule::Clock, ItemKind::Type, "std::time::Instant"),
    (Rule::Clock, ItemKind::Type, "std::time::SystemTime"),
    (Rule::Thread, ItemKind::Type, "std::process::Child"),
    (Rule::Thread, ItemKind::Type, "std::process::Command"),
    (Rule::Thread, ItemKind::Type, "std::thread::Builder"),
    (Rule::Simd, ItemKind::Macro, "std::arch::is_aarch64_feature_detected"),
    (Rule::Simd, ItemKind::Macro, "std::is_x86_feature_detected"),
    (Rule::Env, ItemKind::Macro, "std::env"),
    (Rule::Env, ItemKind::Macro, "std::option_env"),
    (Rule::IoFs, ItemKind::Macro, "std::include"),
    (Rule::IoFs, ItemKind::Macro, "std::include_bytes"),
    (Rule::IoFs, ItemKind::Macro, "std::include_str"),
    (Rule::Thread, ItemKind::Macro, "std::thread_local"),
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

// ------------------------------------------------------------------ JSON

/// Deepest JSON nesting read from the driver: a diagnostic's expansion chain of
/// [`MAX_EXPANSION_DEPTH`] spans nests two levels per span.
pub const MAX_JSON_DEPTH: usize = MAX_EXPANSION_DEPTH * 2 + 16;

enum Frame {
    Arr(Vec<CValue>),
    Obj(BTreeMap<String, CValue>, Option<String>),
}

fn skip_ws(s: &[u8], i: &mut usize) {
    while s.get(*i).is_some_and(u8::is_ascii_whitespace) {
        *i += 1;
    }
}

fn parse_json_str(s: &[u8], i: &mut usize) -> Option<String> {
    if s.get(*i) != Some(&b'"') {
        return None;
    }
    *i += 1;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let b = *s.get(*i)?;
        *i += 1;
        match b {
            b'"' => return String::from_utf8(out).ok(),
            b'\\' => {
                let e = *s.get(*i)?;
                *i += 1;
                let c = match e {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'u' => {
                        let hex = |i: &mut usize| -> Option<u32> {
                            let h = std::str::from_utf8(s.get(*i..*i + 4)?).ok()?;
                            *i += 4;
                            u32::from_str_radix(h, 16).ok()
                        };
                        let hi = hex(i)?;
                        let code = if (0xD800..0xDC00).contains(&hi) {
                            if s.get(*i..*i + 2) != Some(b"\\u") {
                                return None;
                            }
                            *i += 2;
                            let lo = hex(i)?;
                            if !(0xDC00..0xE000).contains(&lo) {
                                return None;
                            }
                            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                        } else {
                            hi
                        };
                        char::from_u32(code)?
                    }
                    _ => return None,
                };
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            b if b < 0x20 => return None,
            b => out.push(b),
        }
    }
}

/// A bounded, iterative JSON parser for one line of the driver's message stream. Numbers become
/// [`CValue::Num`]; duplicate keys, nesting deeper than `max_depth`, trailing text and malformed
/// input give `None`.
pub fn parse_json(s: &[u8], max_depth: usize) -> Option<CValue> {
    let mut i = 0usize;
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        skip_ws(s, &mut i);
        let mut v = match *s.get(i)? {
            b'{' => {
                i += 1;
                if stack.len() >= max_depth {
                    return None;
                }
                skip_ws(s, &mut i);
                if s.get(i) == Some(&b'}') {
                    i += 1;
                    CValue::Obj(BTreeMap::new())
                } else {
                    let k = parse_json_str(s, &mut i)?;
                    skip_ws(s, &mut i);
                    if s.get(i) != Some(&b':') {
                        return None;
                    }
                    i += 1;
                    stack.push(Frame::Obj(BTreeMap::new(), Some(k)));
                    continue;
                }
            }
            b'[' => {
                i += 1;
                if stack.len() >= max_depth {
                    return None;
                }
                skip_ws(s, &mut i);
                if s.get(i) == Some(&b']') {
                    i += 1;
                    CValue::Arr(Vec::new())
                } else {
                    stack.push(Frame::Arr(Vec::new()));
                    continue;
                }
            }
            b'"' => CValue::Str(parse_json_str(s, &mut i)?),
            b't' if s.get(i..i + 4) == Some(b"true") => {
                i += 4;
                CValue::Bool(true)
            }
            b'f' if s.get(i..i + 5) == Some(b"false") => {
                i += 5;
                CValue::Bool(false)
            }
            b'n' if s.get(i..i + 4) == Some(b"null") => {
                i += 4;
                CValue::Null
            }
            _ => {
                let start = i;
                while s.get(i).is_some_and(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')) {
                    i += 1;
                }
                let n: f64 = std::str::from_utf8(s.get(start..i)?).ok()?.parse().ok()?;
                if start == i || !n.is_finite() {
                    return None;
                }
                CValue::Num(n)
            }
        };
        // Attach the finished value to its parents, closing every frame that ends here.
        loop {
            match stack.last_mut() {
                None => {
                    skip_ws(s, &mut i);
                    return (i == s.len()).then_some(v);
                }
                Some(Frame::Arr(a)) => {
                    a.push(v);
                    skip_ws(s, &mut i);
                    match *s.get(i)? {
                        b',' => {
                            i += 1;
                            break;
                        }
                        b']' => {
                            i += 1;
                            let Some(Frame::Arr(a)) = stack.pop() else { return None };
                            v = CValue::Arr(a);
                        }
                        _ => return None,
                    }
                }
                Some(Frame::Obj(o, key)) => {
                    let k = key.take()?;
                    if o.insert(k, v).is_some() {
                        return None;
                    }
                    skip_ws(s, &mut i);
                    match *s.get(i)? {
                        b',' => {
                            i += 1;
                            skip_ws(s, &mut i);
                            let k = parse_json_str(s, &mut i)?;
                            skip_ws(s, &mut i);
                            if s.get(i) != Some(&b':') {
                                return None;
                            }
                            i += 1;
                            *key = Some(k);
                            break;
                        }
                        b'}' => {
                            i += 1;
                            let Some(Frame::Obj(o, _)) = stack.pop() else { return None };
                            v = CValue::Obj(o);
                        }
                        _ => return None,
                    }
                }
            }
        }
    }
}

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
    /// Crates with a compiled library, binary or proc-macro artifact (not a build script).
    pub built: BTreeSet<String>,
    /// Crates with a compile error: first error message.
    pub errors: BTreeMap<String, String>,
}

/// Artifact kinds that count as the crate being linted (a `custom-build` script does not).
pub const BUILT_KINDS: [&str; 7] = ["lib", "rlib", "dylib", "cdylib", "staticlib", "bin", "proc-macro"];

/// Table paths that may legitimately not resolve on some targets: `(path, arch prefixes on
/// which it must resolve)`. Anywhere else the "does not refer to" warning is expected.
pub const UNRESOLVED_ALLOWED: [(&str, &[&str]); 2] = [
    ("std::arch::is_aarch64_feature_detected", &["aarch64"]),
    ("std::is_x86_feature_detected", &["x86_64", "i686", "i586"]),
];

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

/// True when the driver may report `path` as unresolved on `target` (see [`UNRESOLVED_ALLOWED`]).
pub fn unresolved_allowed(path: &str, target: &str) -> bool {
    UNRESOLVED_ALLOWED.iter().any(|(p, archs)| *p == path && !archs.iter().any(|a| target.starts_with(a)))
}

/// The table path a "does not refer to …" configuration warning names, if the message is one.
fn unresolved_path(text: &str) -> Option<&str> {
    if !text.contains("does not refer to") {
        return None;
    }
    let start = text.find('`')? + 1;
    let len = text.get(start..)?.find('`')?;
    text.get(start..start + len)
}

/// Parses one target's `--message-format=json` stdout. Pure (reads source lines for digests).
///
/// Fails with `KNP1-TYPED-DRIVER` for a line that is not a cargo message, a disallowed-item
/// diagnostic that names no rule, and a table path the driver cannot resolve on this target
/// (unless [`unresolved_allowed`]).
pub fn parse_messages(stdout: &[u8], target: &str, ctx: &MapCtx) -> Result<Parsed, TypedError> {
    let mut out = Parsed::default();
    for (n, line) in stdout.split(|b| *b == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let v = parse_json(line, MAX_JSON_DEPTH)
            .ok_or_else(|| driver(format!("{target}: driver output line {} is not a readable cargo message", n + 1)))?;
        let reason = get_str(&v, "reason").unwrap_or_default();
        let krate = get_str(&v, "manifest_path").and_then(|m| ctx.crate_of_manifest(m));
        match reason {
            "compiler-artifact" => {
                let kinds = get(&v, "target").map(|t| get_arr(t, "kind")).unwrap_or_default();
                let counts = kinds.iter().any(|k| matches!(k, CValue::Str(s) if BUILT_KINDS.contains(&s.as_str())));
                if let (Some(c), true) = (krate, counts) {
                    out.built.insert(c.name.clone());
                }
            }
            "compiler-message" => {
                let Some(msg) = get(&v, "message") else {
                    return Err(driver(format!("{target}: compiler message without `message`")));
                };
                let text = get_str(msg, "message").unwrap_or_default();
                if let Some(path) = unresolved_path(text) {
                    if !unresolved_allowed(path, target) {
                        return Err(driver(format!("{target}: the driver cannot resolve a table entry: {text}")));
                    }
                    continue;
                }
                let Some(c) = krate else { continue };
                let code = get(msg, "code").and_then(|c| get_str(c, "code")).unwrap_or_default();
                let level = get_str(msg, "level").unwrap_or_default();
                if LINTS.contains(&code) {
                    let rule = rule_of(msg)
                        .ok_or_else(|| driver(format!("{target}: {code} diagnostic names no KNP-1 rule: {text}")))?;
                    out.findings.push(finding_at(ctx, c, msg, rule, target));
                } else if level == "error" && !out.errors.contains_key(&c.name) {
                    out.errors
                        .insert(c.name.clone(), if text.is_empty() { "compile error".into() } else { text.into() });
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The finding of one disallowed-item diagnostic, at the outermost in-crate span of the
/// expansion chain of its primary span; with no span inside the crate it is
/// `KNP1-TYPED-NOSPAN` (never waivable) at the crate's `Cargo.toml` 1:1.
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
            let m = format!("{} {message}; no span inside the crate, so it cannot be waived", rule.id());
            Finding {
                excerpt_digest: input_digest(m.as_bytes()),
                rule: Rule::Typed("NOSPAN".to_string()),
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

/// Turns one run into findings: disallowed items, plus `KNP1-TYPED-BUILD` for every crate with
/// a compile error or without a compiled library, binary or proc-macro artifact. `target`
/// names the run (target triple and feature set) in messages.
pub fn target_findings(parsed: &Parsed, ctx: &MapCtx, target: &str, failure: Option<&str>) -> Vec<Finding> {
    let mut out = parsed.findings.clone();
    for c in &ctx.crates {
        if let Some(e) = parsed.errors.get(&c.name) {
            out.push(build_finding(c, target, &format!("compile error: {e}")));
        } else if !parsed.built.contains(&c.name) {
            out.push(build_finding(c, target, failure.unwrap_or("no compiled library, binary or proc-macro artifact")));
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
    let spawn = |name: &str, r: Option<std::process::ChildStdout>| {
        std::thread::Builder::new().name(name.to_string()).spawn(move || read_capped(r))
    };
    let h_out = match spawn("knp1-typed-stdout", out) {
        Ok(h) => h,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let h_err = match std::thread::Builder::new().name("knp1-typed-stderr".to_string()).spawn(move || read_capped(errp))
    {
        Ok(h) => h,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
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

/// Environment variables removed from the driver's environment: every way to inject compiler
/// flags or wrap the compiler besides the check's own arguments.
pub const REMOVED_ENV: [&str; 8] = [
    "RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CLIPPY_ARGS",
    "CLIPPY_DISABLE_DOCS_LINKS",
];

/// Feature sets each target is linted with: the default features, and all features.
pub const FEATURE_SETS: [(&str, bool); 2] = [("default features", false), ("all features", true)];

fn driver_command(
    opts: &Options,
    workspace: &Path,
    conf: &Path,
    target: &str,
    all_features: bool,
    members: &[&CrateEntry],
) -> Command {
    let mut cmd = Command::new(&opts.cargo);
    cmd.current_dir(workspace);
    for k in REMOVED_ENV {
        cmd.env_remove(k);
    }
    for (k, _) in std::env::vars_os() {
        if let Some(k) = k.to_str() {
            if k.starts_with("CARGO_TARGET_") && (k.ends_with("_RUSTFLAGS") || k.ends_with("_RUNNER")) {
                cmd.env_remove(k);
            }
        }
    }
    // An explicitly empty CARGO_ENCODED_RUSTFLAGS takes precedence over RUSTFLAGS and every
    // `build.rustflags` / `target.<…>.rustflags` in `.cargo/config.toml`.
    cmd.env("CARGO_ENCODED_RUSTFLAGS", "")
        .env("CLIPPY_CONF_DIR", conf)
        .env("CARGO_TERM_COLOR", "never")
        .arg("clippy")
        .arg("--manifest-path")
        .arg(workspace.join("Cargo.toml"))
        .args(["--locked", "--keep-going", "--message-format=json", "--target", target])
        .arg("--target-dir")
        .arg(opts.scratch.join("target"));
    if all_features {
        cmd.arg("--all-features");
    }
    for c in members {
        cmd.arg("-p").arg(format!("{}@{}", c.name, c.version));
    }
    cmd.args(["--", "-A", "clippy::all"]);
    for l in LINTS {
        cmd.args(["--force-warn", l]);
    }
    cmd
}

/// Lints the workspace crates of the closure for every configured target, with the default
/// features and with all features.
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
        for (label, all_features) in FEATURE_SETS {
            let cmd = driver_command(opts, workspace, &conf, target, all_features, &members);
            let run_name = format!("{target}, {label}");
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
            all.extend(target_findings(&parsed, &ctx, &run_name, failure.as_deref()));
        }
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
