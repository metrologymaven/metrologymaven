//! Syntax-level banned-API scan over the kernel closure. Spec node `KRN.NUM.LINT.SYN`.
//!
//! [`scan_closure`] reads every file that `KRN.NUM.LINT.GRAPH` lists for every crate of the
//! kernel closure (third-party and proc-macro crates included), lexes it with the bounded lexer
//! in [`lex`], and matches the token stream against the pinned rule table [`RULE_TABLE`]. The
//! scan is deliberately token-level, not a grammar-level parse: every token of the file is
//! looked at, including `macro_rules!` bodies, macro invocation arguments and attributes, so
//! there is no grammar position in which a banned name can hide. Comments (doc comments
//! included) and the contents of string literals are not code and are not matched.
//!
//! Fail closed:
//!
//! - A file the lexer cannot classify (bad UTF-8, an unterminated literal or comment, an
//!   unbalanced delimiter, a non-ASCII identifier, a stray character, too many tokens, larger
//!   than [`MAX_FILE_BYTES`]) yields one `KNP1-PARSE` finding. `KNP1-PARSE` cannot be waived.
//! - A `use` declaration the scan cannot take apart, an alias chain that is cyclic, deeper
//!   than [`MAX_ALIAS_DEPTH`] or has more than [`MAX_RESOLUTIONS`] targets, and macro code whose
//!   meaning the caller picks (`$x.$f()`, `f64::$f`, `$($p)::*::read`) are `KNP1-PARSE-RESOLVE`
//!   at that token: waivable only through a reviewed waiver, whose justification is reported.
//! - A macro metavariable (`$x`) in a path is a wildcard that matches any segment, and as a
//!   receiver or operand of `.min`/`.max`/`.clamp` it counts as a float.
//! - An I/O error reading a listed file is fatal (`KNP1-READ`): the run produces no findings.
//!
//! `use` renames are resolved per file (REQ-004): every `use` leaf and `extern crate … as …`
//! adds its binding to one alias map for the whole file (module scopes are merged, so a name
//! bound to several targets is checked against all of them), and every path whose first
//! segment is a binding is checked as the full path it stands for.
//!
//! The only opt-outs are reported, never silent ([`Outcome::notes`], informational):
//!
//! - `[syn] facade_files`: in each listed file `KNP1-LIBM` does not fire (REQ-003); every other
//!   rule applies. Each entry carries a reason, and each suppressed hit is a note naming it.
//! - `[syn] facade_paths`: a `KNP1-LIBM` name reached through a listed path prefix (for
//!   example `fid_num::f::sin`, `crate::f::sin` inside `fid-num`, or a `use` of one) is the
//!   sanctioned facade call, not a finding. Each entry carries a reason, and each such use is a
//!   note naming it. `std`, `core`, `alloc` and other built-in roots cannot be facades.
//! - Reviewed waivers of `KRN.NUM.LINT.FIND` (`knp1-waivers.toml`) apply to the findings.
//!
//! Findings carry the crate name and version from the closure, the file relative to the crate
//! root, the 1-based line and column (in characters) of the offending token, and the SHA-256 of
//! that line's text (without its line ending). Output is sorted (D0).
//!
//! Config (`[syn]` of `knp1-lint.toml`; optional, absent means no opt-outs):
//!
//! ```toml
//! [syn.facade_paths]
//! "fid_num::f" = { crate = "fid-num", version = "0.1.0", reason = "KNP-1 facade (KRN.NUM.FN)" }
//!
//! [[syn.facade_files]]
//! crate = "fid-num"
//! version = "0.1.0"
//! file = "src/f/mod.rs"
//! reason = "implements the facade on top of IEEE sqrt and the vendored routines"
//! ```

pub mod lex;

use crate::findings::{input_digest, sort_findings, Finding, Rule};
pub use crate::flags::Outcome;
use crate::graph::{self, Closure, GraphErrorCode};
use lex::{Delim, Kind, LitKind, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::Path;

/// Largest source file scanned, in bytes; a larger file is a `KNP1-PARSE` finding.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Longest alias chain followed (`use a as b; use b::c as d; …`).
pub const MAX_ALIAS_DEPTH: usize = 8;
/// Most full paths one path may resolve to through aliases.
pub const MAX_RESOLUTIONS: usize = 64;
/// Most segments read in one path.
pub const MAX_PATH_SEGMENTS: usize = 64;
/// Deepest `{…}` nesting in one `use` tree.
pub const MAX_USE_DEPTH: usize = 32;
/// Most tokens looked at when checking the receiver or arguments of `.min`/`.max`/`.clamp`
/// or the `<…>` before `::min`; past this the operand counts as a float (fail closed).
pub const MAX_OPERAND_TOKENS: usize = 512;
/// Largest config file read, in bytes.
pub const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

// ------------------------------------------------------------------ rule table

/// How a rule-table pattern matches the token stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// An identifier equal to the pattern, anywhere (code, attribute or macro body).
    Ident,
    /// An identifier equal to the pattern, or to the pattern plus a float suffix
    /// (`f`, `f16`, `f32`, `f64`, `f128`), anywhere.
    IdentFloat,
    /// An identifier equal to the pattern plus a float suffix only (`sinf64`, `powf`), anywhere.
    IdentSuffixed,
    /// An identifier containing the pattern (case-sensitive), anywhere.
    IdentContains,
    /// A call-like use of a name that is too common to ban as a bare identifier: the last
    /// segment of a path of two or more segments, of a path whose first segment is a `use`
    /// alias, of a `use` leaf, a name after `>::` (`<f64>::sin`, `Libm::<f64>::exp`), or a
    /// single identifier directly followed by `(`.
    Call,
    /// A method call `.name(` or `.name::`.
    Method,
    /// `.name(…)` with a float receiver or operand (float literal, `f32`/`f64`, a path through
    /// `f32`/`f64`, or a metavariable), a path ending in `f32::name`/`f64::name` (aliases
    /// resolved), or `<… f32/f64 …>::name`.
    FloatMethod,
    /// A path (aliases resolved; metavariables match any segment) starting with the
    /// `::`-separated pattern, or a glob import that can reach it.
    Module,
    /// A macro invocation `name!(…)`, `name![…]` or `name!{…}`.
    Macro,
    /// A macro invocation unless its only argument is a plain string literal naming a relative
    /// `.rs` file (no `..`, no backslash, no drive) that the scan itself lists.
    IncludeMacro,
    /// `name = …` inside an attribute unless the value is a plain string literal naming a
    /// relative `.rs` file that the scan lists (the `#[path]` attribute).
    Attr,
    /// An identifier equal to the pattern inside an attribute (`#[no_mangle]`).
    AttrName,
    /// The keyword (not a raw identifier). For `extern` only when not followed by `crate`.
    Keyword,
}

use PatternKind as P;

/// Number of rows in [`RULE_TABLE`].
pub const RULE_TABLE_LEN: usize = 144;

/// The pinned rule table of this leaf: `(rule, pattern kind, pattern)`. Mirrored in the
/// leaf's NODE.md Design; changing it is a spec change.
pub static RULE_TABLE: [(Rule, PatternKind, &str); RULE_TABLE_LEN] = [
    // KNP1-FMA: fused multiply-add and contraction-licensing intrinsics.
    (Rule::Fma, P::Ident, "mul_add"),
    (Rule::Fma, P::IdentFloat, "fma"),
    (Rule::Fma, P::IdentContains, "fmadd"),
    (Rule::Fma, P::IdentContains, "fmsub"),
    (Rule::Fma, P::IdentContains, "fnmadd"),
    (Rule::Fma, P::IdentContains, "fnmsub"),
    (Rule::Fma, P::IdentContains, "fmuladd"),
    (Rule::Fma, P::IdentContains, "relaxed_madd"),
    (Rule::Fma, P::IdentContains, "relaxed_nmadd"),
    (Rule::Fma, P::IdentContains, "fmla"),
    (Rule::Fma, P::IdentContains, "fmls"),
    (Rule::Fma, P::IdentContains, "vfma"),
    (Rule::Fma, P::IdentContains, "vfms"),
    (Rule::Fma, P::Ident, "fadd_fast"),
    (Rule::Fma, P::Ident, "fsub_fast"),
    (Rule::Fma, P::Ident, "fmul_fast"),
    (Rule::Fma, P::Ident, "fdiv_fast"),
    (Rule::Fma, P::Ident, "frem_fast"),
    (Rule::Fma, P::Ident, "fadd_algebraic"),
    (Rule::Fma, P::Ident, "fsub_algebraic"),
    (Rule::Fma, P::Ident, "fmul_algebraic"),
    (Rule::Fma, P::Ident, "fdiv_algebraic"),
    (Rule::Fma, P::Ident, "frem_algebraic"),
    (Rule::Fma, P::Ident, "algebraic_add"),
    (Rule::Fma, P::Ident, "algebraic_sub"),
    (Rule::Fma, P::Ident, "algebraic_mul"),
    (Rule::Fma, P::Ident, "algebraic_div"),
    (Rule::Fma, P::Ident, "algebraic_rem"),
    (Rule::Fma, P::Ident, "simd_fma"),
    (Rule::Fma, P::Ident, "simd_relaxed_fma"),
    // KNP1-POW
    (Rule::Pow, P::IdentFloat, "powi"),
    (Rule::Pow, P::IdentSuffixed, "pow"),
    (Rule::Pow, P::Call, "pow"),
    // KNP1-LIBM: names that are banned as any identifier.
    (Rule::Libm, P::IdentFloat, "sqrt"),
    (Rule::Libm, P::IdentFloat, "cbrt"),
    (Rule::Libm, P::IdentFloat, "hypot"),
    (Rule::Libm, P::IdentFloat, "exp_m1"),
    (Rule::Libm, P::IdentFloat, "expm1"),
    (Rule::Libm, P::IdentFloat, "exp2"),
    (Rule::Libm, P::IdentFloat, "ln_1p"),
    (Rule::Libm, P::IdentFloat, "log1p"),
    (Rule::Libm, P::IdentFloat, "log2"),
    (Rule::Libm, P::IdentFloat, "log10"),
    (Rule::Libm, P::IdentFloat, "asin"),
    (Rule::Libm, P::IdentFloat, "acos"),
    (Rule::Libm, P::IdentFloat, "atan"),
    (Rule::Libm, P::IdentFloat, "atan2"),
    (Rule::Libm, P::IdentFloat, "sinh"),
    (Rule::Libm, P::IdentFloat, "cosh"),
    (Rule::Libm, P::IdentFloat, "tanh"),
    (Rule::Libm, P::IdentFloat, "asinh"),
    (Rule::Libm, P::IdentFloat, "acosh"),
    (Rule::Libm, P::IdentFloat, "atanh"),
    (Rule::Libm, P::IdentFloat, "sin_cos"),
    (Rule::Libm, P::IdentFloat, "ln_gamma"),
    (Rule::Libm, P::IdentFloat, "lgamma"),
    (Rule::Libm, P::IdentFloat, "tgamma"),
    (Rule::Libm, P::IdentFloat, "erf"),
    (Rule::Libm, P::IdentFloat, "erfc"),
    (Rule::Libm, P::IdentFloat, "sincos"),
    (Rule::Libm, P::IdentFloat, "lgamma_r"),
    (Rule::Libm, P::Ident, "lgammaf_r"),
    // KNP1-LIBM: short names, banned in call position, as methods and with a float suffix.
    (Rule::Libm, P::Call, "sin"),
    (Rule::Libm, P::Call, "cos"),
    (Rule::Libm, P::Call, "tan"),
    (Rule::Libm, P::Call, "exp"),
    (Rule::Libm, P::Call, "ln"),
    (Rule::Libm, P::Call, "log"),
    (Rule::Libm, P::Call, "gamma"),
    (Rule::Libm, P::Call, "exp10"),
    (Rule::Libm, P::Call, "j0"),
    (Rule::Libm, P::Call, "j1"),
    (Rule::Libm, P::Call, "jn"),
    (Rule::Libm, P::Call, "y0"),
    (Rule::Libm, P::Call, "y1"),
    (Rule::Libm, P::Call, "yn"),
    (Rule::Libm, P::Method, "sin"),
    (Rule::Libm, P::Method, "cos"),
    (Rule::Libm, P::Method, "tan"),
    (Rule::Libm, P::Method, "exp"),
    (Rule::Libm, P::Method, "ln"),
    (Rule::Libm, P::Method, "log"),
    (Rule::Libm, P::Method, "gamma"),
    (Rule::Libm, P::Method, "exp10"),
    (Rule::Libm, P::Method, "j0"),
    (Rule::Libm, P::Method, "j1"),
    (Rule::Libm, P::Method, "jn"),
    (Rule::Libm, P::Method, "y0"),
    (Rule::Libm, P::Method, "y1"),
    (Rule::Libm, P::Method, "yn"),
    (Rule::Libm, P::IdentSuffixed, "sin"),
    (Rule::Libm, P::IdentSuffixed, "cos"),
    (Rule::Libm, P::IdentSuffixed, "tan"),
    (Rule::Libm, P::IdentSuffixed, "exp"),
    (Rule::Libm, P::IdentSuffixed, "ln"),
    (Rule::Libm, P::IdentSuffixed, "log"),
    (Rule::Libm, P::IdentSuffixed, "gamma"),
    (Rule::Libm, P::IdentSuffixed, "exp10"),
    (Rule::Libm, P::IdentSuffixed, "j0"),
    (Rule::Libm, P::IdentSuffixed, "j1"),
    (Rule::Libm, P::IdentSuffixed, "jn"),
    (Rule::Libm, P::IdentSuffixed, "y0"),
    (Rule::Libm, P::IdentSuffixed, "y1"),
    (Rule::Libm, P::IdentSuffixed, "yn"),
    // KNP1-MINMAX
    (Rule::MinMax, P::FloatMethod, "min"),
    (Rule::MinMax, P::FloatMethod, "max"),
    (Rule::MinMax, P::FloatMethod, "clamp"),
    (Rule::MinMax, P::IdentFloat, "fmin"),
    (Rule::MinMax, P::IdentFloat, "fmax"),
    (Rule::MinMax, P::IdentFloat, "minnum"),
    (Rule::MinMax, P::IdentFloat, "maxnum"),
    // KNP1-HASH
    (Rule::Hash, P::IdentContains, "HashMap"),
    (Rule::Hash, P::IdentContains, "HashSet"),
    (Rule::Hash, P::Ident, "RandomState"),
    (Rule::Hash, P::Ident, "DefaultHasher"),
    // KNP1-SIMD
    (Rule::Simd, P::IdentContains, "_feature_detected"),
    (Rule::Simd, P::Ident, "target_feature"),
    (Rule::Simd, P::Ident, "target_clones"),
    (Rule::Simd, P::Ident, "multiversion"),
    (Rule::Simd, P::Module, "std::arch"),
    (Rule::Simd, P::Module, "core::arch"),
    // KNP1-UNSAFE
    (Rule::Unsafe, P::Keyword, "unsafe"),
    (Rule::Unsafe, P::Keyword, "extern"),
    (Rule::Unsafe, P::Macro, "asm"),
    (Rule::Unsafe, P::Macro, "global_asm"),
    (Rule::Unsafe, P::Macro, "naked_asm"),
    (Rule::Unsafe, P::Macro, "llvm_asm"),
    (Rule::Unsafe, P::AttrName, "no_mangle"),
    (Rule::Unsafe, P::AttrName, "export_name"),
    (Rule::Unsafe, P::AttrName, "link_section"),
    // KNP1-IO-FS (and source the scan would not see)
    (Rule::IoFs, P::Module, "std::fs"),
    (Rule::IoFs, P::IncludeMacro, "include"),
    (Rule::IoFs, P::IncludeMacro, "include_bytes"),
    (Rule::IoFs, P::IncludeMacro, "include_str"),
    (Rule::IoFs, P::Attr, "path"),
    // KNP1-IO-NET, KNP1-CLOCK, KNP1-ENV, KNP1-THREAD
    (Rule::IoNet, P::Module, "std::net"),
    (Rule::Clock, P::Module, "std::time"),
    (Rule::Env, P::Module, "std::env"),
    (Rule::Env, P::Module, "core::env"),
    (Rule::Env, P::Macro, "env"),
    (Rule::Env, P::Macro, "option_env"),
    (Rule::Thread, P::Module, "std::thread"),
    (Rule::Thread, P::Module, "std::process"),
    (Rule::Thread, P::Macro, "thread_local"),
];

/// Path roots that can never be a facade.
const RESERVED_ROOTS: [&str; 10] =
    ["std", "core", "alloc", "proc_macro", "test", "crate", "self", "super", "Self", "$crate"];

fn float_suffix(rest: &str) -> bool {
    matches!(rest, "f" | "f16" | "f32" | "f64" | "f128")
}

/// True when identifier `name` matches a name-like pattern.
fn name_matches(kind: PatternKind, pat: &str, name: &str) -> bool {
    match kind {
        P::Ident => name == pat,
        P::IdentFloat => name == pat || name.strip_prefix(pat).is_some_and(float_suffix),
        P::IdentSuffixed => name.strip_prefix(pat).is_some_and(float_suffix),
        P::IdentContains => name.contains(pat),
        _ => false,
    }
}

/// Rules whose name-like patterns match `name`, with the pattern, in table order.
fn name_rules(name: &str) -> Vec<(&'static Rule, &'static str)> {
    let mut out: Vec<(&'static Rule, &'static str)> = Vec::new();
    for (rule, kind, pat) in &RULE_TABLE {
        if name_matches(*kind, pat, name) && !out.iter().any(|(r, _)| *r == rule) {
            out.push((rule, *pat));
        }
    }
    out
}

/// True when `name` is a `KNP1-LIBM` name in any form (call, method or identifier pattern).
fn is_libm_name(name: &str) -> bool {
    RULE_TABLE.iter().any(|(rule, kind, pat)| {
        *rule == Rule::Libm && (name_matches(*kind, pat, name) || (matches!(kind, P::Call | P::Method) && name == *pat))
    })
}

/// The rule of a non-`KNP1-LIBM` `Call` pattern equal to `name`.
fn other_call_rule(name: &str) -> Option<&'static Rule> {
    RULE_TABLE.iter().find(|(r, k, p)| *k == P::Call && *r != Rule::Libm && *p == name).map(|(r, _, _)| r)
}

fn patterns(kind: PatternKind) -> impl Iterator<Item = (&'static Rule, &'static str)> {
    RULE_TABLE.iter().filter(move |(_, k, _)| *k == kind).map(|(r, _, p)| (r, *p))
}

fn is_metavar(seg: &str) -> bool {
    seg.starts_with('$') && seg != "$crate"
}

fn seg_eq(path_seg: &str, pat_seg: &str) -> bool {
    path_seg == pat_seg || is_metavar(path_seg)
}

/// True when `path` starts with `pat` (metavariables in `path` match any segment).
fn has_prefix(path: &[String], pat: &[&str]) -> bool {
    path.len() >= pat.len() && path.iter().zip(pat).all(|(a, b)| seg_eq(a, b))
}

/// True when `path` is a proper prefix of `pat` (a glob over `path` can reach `pat`).
fn reaches(path: &[String], pat: &[&str]) -> bool {
    path.len() < pat.len() && path.iter().zip(pat).all(|(a, b)| seg_eq(a, b))
}

/// The first position at which `pat` occurs in `path` (metavariables match any segment).
fn find_at(path: &[String], pat: &[&str]) -> Option<usize> {
    (0..path.len()).find(|&k| path.get(k..).is_some_and(|rest| has_prefix(rest, pat)))
}

/// True when some non-empty suffix of `path` is a proper prefix of `pat`: a glob over `path`,
/// or a binding of it, can reach `pat`.
fn reaches_any(path: &[String], pat: &[&str]) -> bool {
    (0..path.len()).any(|k| path.get(k..).is_some_and(|rest| reaches(rest, pat)))
}

/// Built-in attributes. Any other attribute is an attribute macro (proc-macro item input,
/// `KNP1-PARSE`), and its arguments are checked for banned names passed bare.
pub const BUILTIN_ATTRIBUTES: &[&str] = &[
    "allow",
    "automatically_derived",
    "cfg",
    "cfg_attr",
    "clippy",
    "cold",
    "collapse_debuginfo",
    "coverage",
    "crate_name",
    "crate_type",
    "debugger_visualizer",
    "deny",
    "deprecated",
    "derive",
    "diagnostic",
    "doc",
    "expect",
    "export_name",
    "feature",
    "forbid",
    "global_allocator",
    "ignore",
    "inline",
    "instruction_set",
    "link",
    "link_name",
    "link_ordinal",
    "link_section",
    "macro_export",
    "macro_use",
    "must_use",
    "naked",
    "no_builtins",
    "no_implicit_prelude",
    "no_link",
    "no_main",
    "no_mangle",
    "no_std",
    "non_exhaustive",
    "optimize",
    "panic_handler",
    "path",
    "proc_macro",
    "proc_macro_attribute",
    "proc_macro_derive",
    "recursion_limit",
    "repr",
    "rustfmt",
    "should_panic",
    "target_feature",
    "test",
    "track_caller",
    "type_length_limit",
    "unsafe",
    "used",
    "warn",
    "windows_subsystem",
];

/// Built-in derives. Any other derive is a derive macro (proc-macro item input, `KNP1-PARSE`).
pub const BUILTIN_DERIVES: &[&str] =
    &["Clone", "Copy", "Debug", "Default", "Eq", "Hash", "Ord", "PartialEq", "PartialOrd"];

/// The built-in roots a re-export can expose (`pub use std;`).
const BUILTIN_ROOTS: [&str; 3] = ["std", "core", "alloc"];

fn is_int_type(seg: &str) -> bool {
    matches!(seg, "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize")
}

fn is_float_type(seg: &str) -> bool {
    seg == "f32" || seg == "f64" || is_metavar(seg)
}

// ------------------------------------------------------------------ errors and config

/// Fatal error codes of this module (the run produces no findings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynErrorCode {
    /// A file listed by the closure cannot be read.
    Read,
    /// The `[syn]` config is malformed or names a facade file outside the closure.
    ConfigInvalid,
    /// The closure could not be computed (`KRN.NUM.LINT.GRAPH`).
    Graph(GraphErrorCode),
}

impl SynErrorCode {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            SynErrorCode::Read => "KNP1-READ",
            SynErrorCode::ConfigInvalid => "KNP1-CONFIG-INVALID",
            SynErrorCode::Graph(g) => g.as_str(),
        }
    }
}

/// A fatal error of the syntax scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynError {
    pub code: SynErrorCode,
    pub message: String,
}

impl fmt::Display for SynError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for SynError {}

fn invalid(m: impl Into<String>) -> SynError {
    SynError { code: SynErrorCode::ConfigInvalid, message: m.into() }
}

/// One `[[syn.facade_files]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadeFile {
    pub crate_name: String,
    pub version: String,
    /// Path relative to the crate root, `/` separated.
    pub file: String,
    pub reason: String,
}

/// One `[syn.facade_paths]` entry: the crate (name and exact version) whose library the path
/// prefix names, and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadePath {
    pub crate_name: String,
    pub version: String,
    pub reason: String,
}

/// The `[syn]` table of `knp1-lint.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    /// Facade path prefix (segments) -> crate, version and reason.
    pub facade_paths: BTreeMap<Vec<String>, FacadePath>,
    /// Facade implementation files, sorted by (crate, version, file).
    pub facade_files: Vec<FacadeFile>,
}

impl Config {
    fn facade_reason(&self, path: &[String]) -> Option<(String, &str)> {
        self.facade_paths
            .iter()
            .find(|(p, _)| path.len() >= p.len() && path.iter().zip(p.iter()).all(|(a, b)| a == b))
            .map(|(p, f)| (p.join("::"), f.reason.as_str()))
    }

    pub fn facade_file(&self, crate_name: &str, version: &str, file: &str) -> Option<&str> {
        self.facade_files
            .iter()
            .find(|f| f.crate_name == crate_name && f.version == version && f.file == file)
            .map(|f| f.reason.as_str())
    }
}

fn plain_ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn reason(v: Option<&toml::Value>, what: &str) -> Result<String, SynError> {
    match v.and_then(toml::Value::as_str).map(str::trim) {
        Some(r) if !r.is_empty() && r.len() <= 4096 => Ok(r.to_string()),
        _ => Err(invalid(format!("knp1-lint.toml: {what} must be a non-empty reason (at most 4096 bytes)"))),
    }
}

fn valid_relpath(p: &str) -> bool {
    let b = p.as_bytes();
    !p.is_empty()
        && p.len() <= 4096
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.chars().any(char::is_control)
        && !(b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        && p.split('/').all(|seg| !seg.is_empty() && seg != ".." && seg != ".")
}

/// A table with exactly `keys`, each a non-empty string; `what` names it in errors.
fn string_table(v: &toml::Value, keys: &[&str], what: &str) -> Result<BTreeMap<String, String>, SynError> {
    let toml::Value::Table(t) = v else {
        return Err(invalid(format!("knp1-lint.toml: {what} must be a table with {}", keys.join(", "))));
    };
    if let Some(k) = t.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(invalid(format!("knp1-lint.toml: {what}: unknown key {k:?}")));
    }
    let mut out = BTreeMap::new();
    for k in keys {
        let val = if *k == "reason" {
            reason(t.get(*k), &format!("{what} reason"))?
        } else {
            match t.get(*k).and_then(toml::Value::as_str) {
                Some(x) if !x.is_empty() && x.len() <= 128 && !x.chars().any(char::is_control) => x.to_string(),
                _ => return Err(invalid(format!("knp1-lint.toml: {what}: missing or bad `{k}`"))),
            }
        };
        out.insert((*k).to_string(), val);
    }
    Ok(out)
}

/// Parses the `[syn]` table of the config text; other tables are ignored. No `[syn]` table
/// means no opt-outs. Unknown keys, empty reasons, reserved facade roots, a facade path whose
/// root is not its crate, and duplicate facade files are rejected with `KNP1-CONFIG-INVALID`.
pub fn parse_config(text: &str) -> Result<Config, SynError> {
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| invalid(format!("knp1-lint.toml: {}", e.to_string().trim_end())))?;
    let Some(syn) = table.get("syn") else {
        return Ok(Config::default());
    };
    let toml::Value::Table(t) = syn else {
        return Err(invalid("knp1-lint.toml: `syn` must be a table"));
    };
    let mut cfg = Config::default();
    for (k, v) in t {
        match k.as_str() {
            "facade_paths" => {
                let toml::Value::Table(paths) = v else {
                    return Err(invalid("knp1-lint.toml: syn.facade_paths must be a table of \"a::b\" = { … }"));
                };
                for (p, entry) in paths {
                    let segs: Vec<String> = p.split("::").map(str::to_string).collect();
                    if !segs.iter().all(|s| plain_ident(s)) {
                        return Err(invalid(format!("knp1-lint.toml: syn.facade_paths key {p:?} is not a Rust path")));
                    }
                    if segs.first().is_some_and(|r| RESERVED_ROOTS.contains(&r.as_str())) {
                        return Err(invalid(format!(
                            "knp1-lint.toml: syn.facade_paths key {p:?} starts at a built-in root and cannot be a facade"
                        )));
                    }
                    let mut e =
                        string_table(entry, &["crate", "version", "reason"], &format!("syn.facade_paths.{p:?}"))?;
                    let crate_name = e.remove("crate").unwrap_or_default();
                    if segs.first().map(String::as_str) != Some(crate_name.replace('-', "_").as_str()) {
                        return Err(invalid(format!(
                            "knp1-lint.toml: syn.facade_paths key {p:?} does not start at crate {crate_name:?}"
                        )));
                    }
                    let version = e.remove("version").unwrap_or_default();
                    let reason = e.remove("reason").unwrap_or_default();
                    cfg.facade_paths.insert(segs, FacadePath { crate_name, version, reason });
                }
            }
            "facade_files" => {
                let toml::Value::Array(items) = v else {
                    return Err(invalid("knp1-lint.toml: syn.facade_files must be an array of tables"));
                };
                let mut seen = BTreeSet::new();
                for (i, item) in items.iter().enumerate() {
                    let what = format!("syn.facade_files #{}", i + 1);
                    let mut e = string_table(item, &["crate", "version", "file", "reason"], &what)?;
                    let crate_name = e.remove("crate").unwrap_or_default();
                    let version = e.remove("version").unwrap_or_default();
                    let file = e.remove("file").unwrap_or_default();
                    if !valid_relpath(&file) {
                        return Err(invalid(format!(
                            "knp1-lint.toml: {what}: `file` must be a '/'-separated path relative to the crate root"
                        )));
                    }
                    if !seen.insert((crate_name.clone(), version.clone(), file.clone())) {
                        return Err(invalid(format!(
                            "knp1-lint.toml: syn.facade_files lists {crate_name} {version} {file} twice"
                        )));
                    }
                    let reason = e.remove("reason").unwrap_or_default();
                    cfg.facade_files.push(FacadeFile { crate_name, version, file, reason });
                }
                cfg.facade_files
                    .sort_by(|a, b| (&a.crate_name, &a.version, &a.file).cmp(&(&b.crate_name, &b.version, &b.file)));
            }
            other => return Err(invalid(format!("knp1-lint.toml: unknown key syn.{other}"))),
        }
    }
    Ok(cfg)
}

// ------------------------------------------------------------------ the scan of one file

/// One file to scan.
#[derive(Debug, Clone, Copy)]
pub struct FileRef<'a> {
    pub crate_name: &'a str,
    pub crate_version: &'a str,
    /// Path relative to the crate root, `/` separated.
    pub file: &'a str,
    /// Every file the scan lists for this crate (for `include!` and `#[path]` targets).
    pub crate_files: &'a [String],
}

/// Scans one source file. Never fails: anything it cannot classify is a `KNP1-PARSE` finding.
pub fn scan_file(f: FileRef<'_>, bytes: &[u8], cfg: &Config) -> Outcome {
    let mut out = Outcome::default();
    let parse = |line: u32, col: u32, excerpt: &[u8], msg: String| Finding {
        rule: Rule::Parse,
        crate_name: f.crate_name.to_string(),
        crate_version: f.crate_version.to_string(),
        file: f.file.to_string(),
        line: line.max(1),
        column: col.max(1),
        excerpt_digest: input_digest(excerpt),
        message: msg,
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        let m = format!("file larger than {MAX_FILE_BYTES} bytes: not scanned");
        out.findings.push(parse(1, 1, m.as_bytes(), m.clone()));
        return out;
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            let good = bytes.get(..e.valid_up_to()).unwrap_or_default();
            let good = std::str::from_utf8(good).unwrap_or_default();
            let line = good.matches('\n').count() + 1;
            let col = good.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            let m = "file is not valid UTF-8".to_string();
            let line = u32::try_from(line).unwrap_or(u32::MAX);
            let col = u32::try_from(col).unwrap_or(u32::MAX);
            out.findings.push(parse(line, col, m.as_bytes(), m.clone()));
            return out;
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let toks = match lex::lex(text) {
        Ok(t) => t,
        Err(e) => {
            let excerpt = line_text(&lines, e.line).unwrap_or(&e.message).as_bytes().to_vec();
            out.findings.push(parse(e.line, e.col, &excerpt, format!("cannot lex file: {}", e.message)));
            return out;
        }
    };
    let mut s = Scan::new(f, cfg, &toks, &lines);
    s.run();
    out.findings = s.hits.into_values().collect();
    out.notes = s.notes.into_values().collect();
    sort_findings(&mut out.findings);
    sort_findings(&mut out.notes);
    out
}

fn line_text<'l>(lines: &[&'l str], line: u32) -> Option<&'l str> {
    lines.get((line as usize).checked_sub(1)?).copied()
}

/// One leaf of a `use` tree.
#[derive(Debug, Clone)]
struct UseLeaf {
    segs: Vec<String>,
    toks: Vec<usize>,
    absolute: bool,
    binding: Option<String>,
    /// Token index of the binding (the name after `as`, else the last segment).
    bind_tok: Option<usize>,
    /// Token index of the `*` of a glob import.
    glob: Option<usize>,
}

struct Scan<'a> {
    f: FileRef<'a>,
    cfg: &'a Config,
    toks: &'a [Token],
    lines: &'a [&'a str],
    crate_ident: String,
    facade_file: Option<&'a str>,
    /// For an open delimiter the index of its close, for a close the index of its open.
    partner: Vec<usize>,
    in_use: Vec<bool>,
    in_attr: Vec<bool>,
    aliases: BTreeMap<String, Vec<Vec<String>>>,
    /// Token indices of facade-path `KNP1-LIBM` names (never findings there).
    exempt: BTreeSet<usize>,
    hits: BTreeMap<(u32, u32, String), Finding>,
    notes: BTreeMap<(u32, u32, String), Finding>,
}

type Resolved = (Vec<Vec<String>>, bool);

impl<'a> Scan<'a> {
    fn new(f: FileRef<'a>, cfg: &'a Config, toks: &'a [Token], lines: &'a [&'a str]) -> Scan<'a> {
        let n = toks.len();
        Scan {
            f,
            cfg,
            toks,
            lines,
            crate_ident: f.crate_name.replace('-', "_"),
            facade_file: cfg.facade_file(f.crate_name, f.crate_version, f.file),
            partner: vec![usize::MAX; n],
            in_use: vec![false; n],
            in_attr: vec![false; n],
            aliases: BTreeMap::new(),
            exempt: BTreeSet::new(),
            hits: BTreeMap::new(),
            notes: BTreeMap::new(),
        }
    }

    fn t(&self, i: usize) -> Option<&'a Token> {
        self.toks.get(i)
    }

    fn kind(&self, i: usize) -> Option<&'a Kind> {
        self.t(i).map(|t| &t.kind)
    }

    fn ident(&self, i: usize) -> Option<&'a str> {
        match self.t(i) {
            Some(Token { kind: Kind::Ident { .. }, text, .. }) => Some(text.as_str()),
            _ => None,
        }
    }

    fn keyword(&self, i: usize, kw: &str) -> bool {
        matches!(self.t(i), Some(Token { kind: Kind::Ident { raw: false }, text, .. }) if text == kw)
    }

    fn is(&self, i: usize, k: &Kind) -> bool {
        self.kind(i) == Some(k)
    }

    fn finding(&self, i: usize, rule: Rule, message: String) -> Option<Finding> {
        let t = self.t(i)?;
        let excerpt = line_text(self.lines, t.line).unwrap_or_default();
        Some(Finding {
            rule,
            crate_name: self.f.crate_name.to_string(),
            crate_version: self.f.crate_version.to_string(),
            file: self.f.file.to_string(),
            line: t.line,
            column: t.col,
            excerpt_digest: input_digest(excerpt.as_bytes()),
            message,
        })
    }

    fn hit(&mut self, i: usize, rule: &Rule, message: String) {
        if let Some(f) = self.finding(i, rule.clone(), message) {
            self.hits.entry((f.line, f.column, f.rule.id())).or_insert(f);
        }
    }

    fn note(&mut self, i: usize, rule: &Rule, message: String) {
        if let Some(f) = self.finding(i, rule.clone(), message) {
            self.notes.entry((f.line, f.column, f.rule.id())).or_insert(f);
        }
    }

    /// A `KNP1-LIBM` hit: a note in a facade file or on a facade-path token, else a finding.
    fn libm(&mut self, i: usize, message: String) {
        if let Some(r) = self.facade_file {
            let m = format!("opt-out (facade file: {r}): {message}");
            self.note(i, &Rule::Libm, m);
        } else if !self.exempt.contains(&i) {
            self.hit(i, &Rule::Libm, message);
        }
    }

    fn run(&mut self) {
        self.pair_delimiters();
        self.mark_attributes();
        let leaves = self.collect_uses();
        for l in &leaves {
            if let Some(b) = &l.binding {
                if !l.segs.is_empty() {
                    self.aliases.entry(b.clone()).or_default().push(l.segs.clone());
                }
            }
        }
        self.collect_type_aliases();
        for l in &leaves {
            match l.glob {
                Some(star) => self.check_glob(l, star),
                None => {
                    self.check_path(&l.segs, &l.toks, l.absolute, false, true);
                    self.check_binding(l);
                }
            }
        }
        self.check_metavariables();
        self.check_macro_args_and_mods();
        for i in 0..self.toks.len() {
            if let Some((segs, toks, absolute, end)) = self.path_at(i) {
                let call = self.is(end, &Kind::Open(Delim::Paren));
                self.check_path(&segs, &toks, absolute, call, false);
            }
        }
        for i in 0..self.toks.len() {
            self.check_token(i);
        }
    }

    fn pair_delimiters(&mut self) {
        let toks = self.toks;
        let mut stack: Vec<usize> = Vec::new();
        for (i, t) in toks.iter().enumerate() {
            match t.kind {
                Kind::Open(_) => stack.push(i),
                Kind::Close(_) => {
                    if let Some(o) = stack.pop() {
                        if let Some(p) = self.partner.get_mut(o) {
                            *p = i;
                        }
                        if let Some(p) = self.partner.get_mut(i) {
                            *p = o;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn mark_attributes(&mut self) {
        for i in 0..self.toks.len() {
            if !self.is(i, &Kind::Pound) {
                continue;
            }
            let open = if self.is(i + 1, &Kind::Bang) { i + 2 } else { i + 1 };
            if !self.is(open, &Kind::Open(Delim::Bracket)) {
                continue;
            }
            let close = self.partner.get(open).copied().unwrap_or(usize::MAX);
            if close == usize::MAX {
                continue;
            }
            for k in i..=close {
                if let Some(b) = self.in_attr.get_mut(k) {
                    *b = true;
                }
            }
        }
    }

    // -------------------------------------------------------------- use declarations

    fn collect_uses(&mut self) -> Vec<UseLeaf> {
        let mut leaves = Vec::new();
        let mut i = 0;
        while i < self.toks.len() {
            if self.keyword(i, "extern") && self.keyword(i + 1, "crate") {
                // `extern crate X;` and `extern crate X as Y;`
                if let Some(x) = self.ident(i + 2) {
                    let (binding, bind_tok) = if self.keyword(i + 3, "as") {
                        match self.ident(i + 4) {
                            Some("_") | None => (None, None),
                            Some(y) => (Some(y.to_string()), Some(i + 4)),
                        }
                    } else {
                        (Some(x.to_string()), Some(i + 2))
                    };
                    leaves.push(UseLeaf {
                        segs: vec![x.to_string()],
                        toks: vec![i + 2],
                        absolute: true,
                        binding,
                        bind_tok,
                        glob: None,
                    });
                }
                i += 1;
                continue;
            }
            if !self.keyword(i, "use") || self.is(i + 1, &Kind::Lt) {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            let mut found = Vec::new();
            let r = self.use_tree(&mut j, &mut Vec::new(), &mut Vec::new(), false, 0, &mut found);
            match r {
                Ok(()) if self.is(j, &Kind::Semi) => {
                    for k in i..=j {
                        if let Some(b) = self.in_use.get_mut(k) {
                            *b = true;
                        }
                    }
                    leaves.extend(found);
                    i = j + 1;
                }
                Ok(()) => {
                    self.parse_hit(j.min(self.toks.len().saturating_sub(1)), "`use` declaration not ended by `;`");
                    i += 1;
                }
                Err(k) => {
                    self.parse_hit(
                        k.min(self.toks.len().saturating_sub(1)),
                        "`use` declaration the scan cannot take apart",
                    );
                    i += 1;
                }
            }
        }
        leaves
    }

    fn parse_hit(&mut self, i: usize, what: &str) {
        self.hit(i, &Rule::ParseResolve, what.to_string());
    }

    /// One path segment at `i` (an identifier, or a metavariable `$name`) and the next index.
    fn seg_at(&self, i: usize) -> Option<(String, usize)> {
        match self.kind(i)? {
            Kind::Ident { .. } => Some((self.ident(i)?.to_string(), i + 1)),
            Kind::Dollar => self.ident(i + 1).map(|n| (format!("${n}"), i + 2)),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn use_tree(
        &self,
        i: &mut usize,
        prefix: &mut Vec<String>,
        ptoks: &mut Vec<usize>,
        absolute: bool,
        depth: usize,
        out: &mut Vec<UseLeaf>,
    ) -> Result<(), usize> {
        if depth > MAX_USE_DEPTH {
            return Err(*i);
        }
        let mut absolute = absolute;
        if prefix.is_empty() && self.is(*i, &Kind::PathSep) {
            absolute = true;
            *i += 1;
        }
        loop {
            if prefix.len() > MAX_PATH_SEGMENTS {
                return Err(*i);
            }
            if matches!(self.t(*i), Some(Token { kind: Kind::Other, text, .. }) if text == "*") {
                out.push(UseLeaf {
                    segs: prefix.clone(),
                    toks: ptoks.clone(),
                    absolute,
                    binding: None,
                    bind_tok: None,
                    glob: Some(*i),
                });
                *i += 1;
                return Ok(());
            }
            if self.is(*i, &Kind::Open(Delim::Brace)) {
                *i += 1;
                loop {
                    if self.is(*i, &Kind::Close(Delim::Brace)) {
                        *i += 1;
                        return Ok(());
                    }
                    let mut p = prefix.clone();
                    let mut pt = ptoks.clone();
                    self.use_tree(i, &mut p, &mut pt, absolute, depth + 1, out)?;
                    if self.is(*i, &Kind::Comma) {
                        *i += 1;
                    } else if self.is(*i, &Kind::Close(Delim::Brace)) {
                        *i += 1;
                        return Ok(());
                    } else {
                        return Err(*i);
                    }
                }
            }
            let Some((seg, next)) = self.seg_at(*i) else {
                return Err(*i);
            };
            prefix.push(seg);
            ptoks.push(*i);
            *i = next;
            if self.is(*i, &Kind::PathSep) {
                *i += 1;
                continue;
            }
            // End of a simple path: `self` refers to the prefix.
            if prefix.last().is_some_and(|s| s == "self") && prefix.len() > 1 {
                prefix.pop();
                ptoks.pop();
            }
            let mut binding = prefix.last().cloned();
            let mut bind_tok = ptoks.last().copied();
            if self.keyword(*i, "as") {
                match self.ident(*i + 1) {
                    Some(b) => {
                        binding = if b == "_" { None } else { Some(b.to_string()) };
                        bind_tok = Some(*i + 1);
                        *i += 2;
                    }
                    None => return Err(*i + 1),
                }
            }
            out.push(UseLeaf { segs: prefix.clone(), toks: ptoks.clone(), absolute, binding, bind_tok, glob: None });
            return Ok(());
        }
    }

    // -------------------------------------------------------------- paths

    /// The path starting at `i`, if a path starts there: segments, their token indices,
    /// whether it is absolute (`::a::b`), and the index after it. Tokens inside `use`
    /// declarations are handled by [`Scan::collect_uses`].
    #[allow(clippy::type_complexity)]
    fn path_at(&self, i: usize) -> Option<(Vec<String>, Vec<usize>, bool, usize)> {
        if self.in_use.get(i).copied().unwrap_or(true) {
            return None;
        }
        let prev = i.checked_sub(1).and_then(|p| self.kind(p));
        let mut j = i;
        let absolute = match self.kind(i)? {
            Kind::PathSep => {
                if matches!(prev, Some(Kind::Ident { .. } | Kind::Gt)) {
                    return None;
                }
                j += 1;
                true
            }
            Kind::Ident { .. } | Kind::Dollar => {
                if matches!(prev, Some(Kind::PathSep | Kind::Dot | Kind::Dollar)) {
                    return None;
                }
                false
            }
            _ => return None,
        };
        let (s, n) = self.seg_at(j)?;
        let mut segs = vec![s];
        let mut toks = vec![j];
        j = n;
        while segs.len() < MAX_PATH_SEGMENTS && self.is(j, &Kind::PathSep) {
            let Some((s, n)) = self.seg_at(j + 1) else { break };
            segs.push(s);
            toks.push(j + 1);
            j = n;
        }
        Some((segs, toks, absolute, j))
    }

    /// Every full path `segs` stands for: `crate`/`$crate` become this crate, an alias root
    /// is replaced by each of its targets (recursively). The flag is set when the chain is
    /// cyclic, too deep or too wide.
    fn resolve(&self, segs: &[String], absolute: bool) -> Resolved {
        let mut out: Vec<Vec<String>> = Vec::new();
        let mut overflow = false;
        let mut work: Vec<(Vec<String>, usize)> = vec![(segs.to_vec(), 0)];
        while let Some((p, d)) = work.pop() {
            if out.len() + work.len() >= MAX_RESOLUTIONS {
                overflow = true;
                break;
            }
            let Some(root) = p.first() else {
                out.push(p);
                continue;
            };
            if root == "crate" || root == "$crate" || root == "self" || root == "super" {
                // The segment after `crate`/`self`/`super` may be a binding of this file (or of
                // the module it names): resolve it through the alias map too (fail closed).
                let rest: Vec<String> = p
                    .iter()
                    .skip_while(|x| *x == "self" || *x == "super")
                    .skip(usize::from(root == "crate" || root == "$crate"))
                    .cloned()
                    .collect();
                if root == "crate" || root == "$crate" {
                    let mut n = vec![self.crate_ident.clone()];
                    n.extend(rest.iter().cloned());
                    out.push(n);
                } else {
                    out.push(p.clone());
                }
                if rest.first().is_some_and(|r| self.aliases.contains_key(r)) {
                    if d < MAX_ALIAS_DEPTH {
                        work.push((rest, d + 1));
                    } else {
                        overflow = true;
                    }
                }
                continue;
            }
            let targets = if absolute && d == 0 { None } else { self.aliases.get(root) };
            match targets {
                Some(ts) if d < MAX_ALIAS_DEPTH => {
                    for t in ts {
                        let mut n = t.clone();
                        n.extend(p.iter().skip(1).cloned());
                        // `use a::a;`: the root of the target is the module, not the binding.
                        if n == p || t.first() == Some(root) {
                            out.push(n);
                        } else {
                            work.push((n, d + 1));
                        }
                    }
                }
                Some(_) => {
                    overflow = true;
                    out.push(p);
                }
                None => out.push(p),
            }
        }
        out.sort();
        out.dedup();
        (out, overflow)
    }

    fn check_path(&mut self, segs: &[String], toks: &[usize], absolute: bool, call: bool, is_use: bool) {
        let (Some(&first), Some(&last)) = (toks.first(), toks.last()) else {
            return;
        };
        let (rs, overflow) = self.resolve(segs, absolute);
        if overflow {
            self.parse_hit(first, "alias chain is cyclic, too deep or too wide to resolve");
        }
        let aliased = rs.len() != 1 || rs.first().map(Vec::as_slice) != Some(segs);
        let shown = segs.join("::");
        // Module rules.
        for (rule, pat) in patterns(P::Module) {
            let p: Vec<&str> = pat.split("::").collect();
            if let Some(k) = find_at(segs, &p) {
                let at = toks.get(k + p.len() - 1).copied().unwrap_or(first);
                self.hit(at, rule, format!("path `{shown}` uses `{pat}`"));
            } else if let Some(r) = rs.iter().find(|r| find_at(r, &p).is_some()) {
                self.hit(first, rule, format!("path `{shown}` resolves to `{}`, which uses `{pat}`", r.join("::")));
            }
        }
        // f32::min / f64::max / f64::clamp, aliases resolved.
        if segs.len() >= 2 || aliased {
            for (rule, pat) in patterns(P::FloatMethod) {
                let found = rs.iter().find(|r| {
                    r.len() >= 2
                        && r.last().is_some_and(|l| l == pat)
                        && r.get(r.len() - 2).is_some_and(|t| is_float_type(t))
                });
                if let Some(r) = found {
                    let at = if segs.last().is_some_and(|l| l == pat) { last } else { first };
                    self.hit(at, rule, format!("`{}`: float min/max/clamp is not total-order", r.join("::")));
                }
            }
        }
        // KNP1-LIBM in call position, with the facade exemption.
        if segs.len() >= 2 || aliased || is_use || call {
            let libm_hit = rs.iter().find(|r| r.last().is_some_and(|l| is_libm_name(l)));
            if let Some(r) = libm_hit {
                let at = if segs.last().is_some_and(|l| is_libm_name(l)) { last } else { first };
                let cfg = self.cfg;
                let facades: Vec<Option<(String, &str)>> = rs.iter().map(|r| cfg.facade_reason(r)).collect();
                if !rs.is_empty() && facades.iter().all(Option::is_some) {
                    let (p, why) = facades.first().cloned().flatten().unwrap_or_default();
                    let why = why.to_string();
                    self.exempt.insert(at);
                    self.note(at, &Rule::Libm, format!("opt-out (facade path `{p}`: {why}): `{shown}`"));
                } else {
                    self.libm(at, format!("`{shown}` calls `{}` (float transcendental)", r.join("::")));
                }
            }
        }
        // Other call-position rules (`pow`).
        if segs.len() >= 2 || aliased || is_use || call {
            for r in &rs {
                // `i32::pow` and the like are exact integer powers.
                let int_owner = r.len() >= 2 && r.get(r.len() - 2).is_some_and(|t| is_int_type(t));
                if let Some(rule) = r.last().and_then(|l| other_call_rule(l)).filter(|_| !int_owner) {
                    let at = if segs.last() == r.last() { last } else { first };
                    self.hit(at, rule, format!("`{shown}` calls `{}`", r.join("::")));
                }
            }
        }
        // Name rules reached only through an alias.
        if aliased {
            for r in &rs {
                for seg in r {
                    if segs.contains(seg) {
                        continue;
                    }
                    for (rule, pat) in name_rules(seg) {
                        if *rule != Rule::Libm {
                            self.hit(first, rule, format!("`{shown}` resolves to `{}` (`{pat}`)", r.join("::")));
                        }
                    }
                }
            }
        }
    }

    /// A `use` leaf or `extern crate … as …`: a renamed binding whose target can reach a
    /// `Module` pattern is reported for that rule, because a later `b::fs` in another file is
    /// not seen; a leaf naming a `Macro` or `IncludeMacro` pattern is reported like a call of it.
    fn check_binding(&mut self, l: &UseLeaf) {
        let (Some(&last), Some(lastseg)) = (l.toks.last(), l.segs.last()) else {
            return;
        };
        let (rs, _) = self.resolve(&l.segs, l.absolute);
        let shown = l.segs.join("::");
        let renamed = l.binding.as_ref().is_some_and(|b| b != lastseg);
        let bare_root = l.segs.len() == 1 && BUILTIN_ROOTS.contains(&lastseg.as_str());
        if renamed || bare_root {
            for (rule, pat) in patterns(P::Module) {
                let p: Vec<&str> = pat.split("::").collect();
                if rs.iter().any(|r| reaches_any(r, &p)) {
                    let why = if bare_root { "exposes a built-in root" } else { "is renamed" };
                    self.hit(last, rule, format!("`{shown}` {why}; the binding can reach `{pat}`"));
                }
            }
        }
        if let (Some(b), Some(bt)) = (&l.binding, l.bind_tok) {
            let is_itself = l.segs.len() == 1 && l.segs.first() == Some(b);
            if !is_itself {
                self.check_facade_shadow(bt, b);
            }
        }
        for kind in [P::Macro, P::IncludeMacro] {
            for (rule, pat) in patterns(kind) {
                if rs.iter().any(|r| r.last().is_some_and(|x| x == pat)) {
                    self.hit(last, rule, format!("`{shown}` imports the built-in macro `{pat}!`"));
                }
            }
        }
    }

    /// `type F = a::b;` adds `F` to the alias map (so `type F = f64; F::max` is seen).
    fn collect_type_aliases(&mut self) {
        for i in 0..self.toks.len() {
            if !self.keyword(i, "type") || !self.is(i + 2, &Kind::Eq) {
                continue;
            }
            let Some(name) = self.ident(i + 1) else { continue };
            self.check_facade_shadow(i + 1, name);
            if let Some((segs, _, _, _)) = self.path_at(i + 3) {
                self.aliases.entry(name.to_string()).or_default().push(segs);
            }
        }
    }

    /// Macro code the scan cannot classify is `KNP1-PARSE` (never waivable), reported in the
    /// macro definition: a metavariable in callee position (`$m!`, `$f(`, `.$f(`, `::$f`,
    /// `>::$f`; not `$crate`, not `fn $f(`), and a `$(…)` repetition that contains `::` or is
    /// followed by `::`.
    fn check_metavariables(&mut self) {
        for i in 0..self.toks.len() {
            if !self.is(i, &Kind::Dollar) {
                continue;
            }
            if let Some(name) = self.ident(i + 1) {
                if name == "crate" {
                    continue;
                }
                let prev = i.checked_sub(1).and_then(|p| self.kind(p));
                let after_fn = i.checked_sub(1).is_some_and(|p| self.keyword(p, "fn"));
                // `$d m`, where `$d` expands to `$`, is the escaped metavariable `$m`: test `m`.
                let at = if self.ident(i + 2).is_some() { i + 3 } else { i + 2 };
                let callee = self.is(at, &Kind::Bang)
                    || (self.is(at, &Kind::Open(Delim::Paren)) && !after_fn)
                    || matches!(prev, Some(Kind::Dot | Kind::PathSep));
                if callee {
                    self.hit(
                        i,
                        &Rule::Parse,
                        format!("metavariable `${name}` in callee position: the scan cannot classify the call"),
                    );
                }
            } else if self.is(i + 1, &Kind::Open(Delim::Paren)) {
                let close = self.partner.get(i + 1).copied().unwrap_or(usize::MAX);
                let unclassifiable = close == usize::MAX
                    || (i + 2..close).any(|k| self.is(k, &Kind::PathSep))
                    || (close + 1..close + 4).any(|k| self.is(k, &Kind::PathSep));
                if unclassifiable {
                    self.hit(
                        i,
                        &Rule::Parse,
                        "macro repetition builds a path: the scan cannot classify it".to_string(),
                    );
                }
            }
        }
    }

    /// One attribute item `name …` in `[from, to)`: a non-built-in attribute is an attribute
    /// macro (`KNP1-PARSE`) whose arguments are checked for banned names; `derive` lists are
    /// checked for derive macros; each attribute after the predicate of `cfg_attr` is checked
    /// the same way.
    fn attr_item(&mut self, from: usize, to: usize, in_args: &mut [bool], depth: usize) {
        let Some(name) = self.ident(from) else { return };
        let group = if self.is(from + 1, &Kind::Open(Delim::Paren)) {
            let close = self.partner.get(from + 1).copied().unwrap_or(usize::MAX);
            (close != usize::MAX && close < to).then_some((from + 1, close))
        } else {
            None
        };
        if name == "cfg_attr" && depth < 8 {
            let Some((open, close)) = group else { return };
            let mut starts = Vec::new();
            let mut k = open + 1;
            while k < close {
                if matches!(self.kind(k), Some(Kind::Open(_))) {
                    k = self.partner.get(k).copied().unwrap_or(close).max(k) + 1;
                    continue;
                }
                if self.is(k, &Kind::Comma) {
                    starts.push(k + 1);
                }
                k += 1;
            }
            starts.push(close + 1);
            for w in starts.windows(2) {
                if let (Some(&a), Some(&b)) = (w.first(), w.get(1)) {
                    self.attr_item(a, b - 1, in_args, depth + 1);
                }
            }
            return;
        }
        if name == "derive" {
            if let Some((open, close)) = group {
                for k in open + 1..close {
                    if let Some(d) = self.ident(k) {
                        if !self.is(k + 1, &Kind::PathSep) && !BUILTIN_DERIVES.contains(&d) {
                            self.hit(k, &Rule::Parse, format!("derive macro `{d}`: proc-macro item input"));
                        }
                    }
                }
            }
            return;
        }
        if !BUILTIN_ATTRIBUTES.contains(&name) {
            self.hit(from, &Rule::Parse, format!("attribute macro `{name}`: proc-macro item input"));
            for b in in_args.iter_mut().take(to).skip(from + 1) {
                *b = true;
            }
        }
    }

    /// A module, binding or type alias named like a facade root (`mod fid_num`,
    /// `use std::primitive::f64 as fid_num`) could stand in for the facade: `KNP1-LIBM`.
    fn check_facade_shadow(&mut self, at: usize, name: &str) {
        if self.cfg.facade_paths.keys().any(|p| p.first().is_some_and(|r| r == name)) {
            self.hit(at, &Rule::Libm, format!("`{name}` is named like a facade root but is not the facade crate"));
        }
    }

    /// `mod <facade root>` and banned names passed bare to a macro (`call!(env, …)`).
    fn check_macro_args_and_mods(&mut self) {
        let n = self.toks.len();
        let mut in_args = vec![false; n];
        let mut in_template = vec![false; n];
        for i in 0..n {
            if self.keyword(i, "mod") {
                if let Some(name) = self.ident(i + 1) {
                    self.check_facade_shadow(i + 1, name);
                }
            }
            if self.ident(i).is_some() && self.is(i + 1, &Kind::Bang) && matches!(self.kind(i + 2), Some(Kind::Open(_)))
            {
                let close = self.partner.get(i + 2).copied().unwrap_or(usize::MAX).min(n);
                for b in in_args.iter_mut().take(close).skip(i + 3) {
                    *b = true;
                }
            }
            // A `macro_rules!` inside a `macro_rules! name { … }` body defines macros from macro
            // input (`$d m! $d a`): never waivable.
            if self.keyword(i, "macro_rules")
                && self.is(i + 1, &Kind::Bang)
                && self.ident(i + 2).is_some()
                && matches!(self.kind(i + 3), Some(Kind::Open(_)))
            {
                let close = self.partner.get(i + 3).copied().unwrap_or(usize::MAX).min(n);
                for k in i + 4..close {
                    if self.keyword(k, "macro_rules") {
                        self.hit(
                            k,
                            &Rule::Parse,
                            "macro-defining macro: a `macro_rules!` inside a macro body".to_string(),
                        );
                    }
                }
            }
            // Attributes: attribute and derive macros, and the arguments of non-built-ins.
            if self.is(i, &Kind::Pound) {
                let open = if self.is(i + 1, &Kind::Bang) { i + 2 } else { i + 1 };
                if self.is(open, &Kind::Open(Delim::Bracket)) {
                    let close = self.partner.get(open).copied().unwrap_or(usize::MAX).min(n);
                    self.attr_item(open + 1, close, &mut in_args, 0);
                }
            }
            // Macro transcribers: `macro_rules! name { … }` and `macro name … { … }`.
            let body = if self.keyword(i, "macro_rules") && self.is(i + 1, &Kind::Bang) && self.ident(i + 2).is_some() {
                Some((i + 3, self.partner.get(i + 3).copied().unwrap_or(usize::MAX)))
            } else if self.keyword(i, "macro") && self.ident(i + 1).is_some() {
                let first = self.partner.get(i + 2).copied().unwrap_or(usize::MAX);
                let last = if first != usize::MAX && self.is(first + 1, &Kind::Open(Delim::Brace)) {
                    self.partner.get(first + 1).copied().unwrap_or(usize::MAX)
                } else {
                    first
                };
                Some((i + 2, last))
            } else {
                None
            };
            if let Some((from, to)) = body {
                if matches!(self.kind(from), Some(Kind::Open(_))) {
                    for b in in_template.iter_mut().take(to.min(n)).skip(from + 1) {
                        *b = true;
                    }
                }
            }
        }
        for i in 0..n {
            let Some(name) = self.ident(i) else { continue };
            let prev = i.checked_sub(1).and_then(|p| self.kind(p));
            let next = self.kind(i + 1);
            let template = in_template.get(i).copied().unwrap_or(false) && !matches!(prev, Some(Kind::Dollar));
            let inside = in_args.get(i).copied().unwrap_or(false);
            let after_ok = !matches!(prev, Some(Kind::Dot | Kind::PathSep | Kind::Dollar));
            let bare =
                inside && after_ok && !matches!(next, Some(Kind::PathSep | Kind::Bang | Kind::Open(Delim::Paren)));
            // Macro names count whatever follows them (`inner!(env ("X"))`), except `!` (a call,
            // checked as a macro already).
            let macro_name = inside && after_ok && !matches!(next, Some(Kind::Bang));
            // A banned name followed by a metavariable or repetition is called by it.
            let meta_called = matches!(next, Some(Kind::Dollar)) && !matches!(prev, Some(Kind::Dollar));
            for kind in [P::Call, P::Method, P::FloatMethod, P::Macro, P::IncludeMacro] {
                let applies = template
                    || meta_called
                    || if matches!(kind, P::Macro | P::IncludeMacro) { macro_name } else { bare };
                if !applies {
                    continue;
                }
                for (rule, pat) in patterns(kind) {
                    if pat == name {
                        let m = if template {
                            format!("`{name}` in a macro transcriber: a code template naming a banned item")
                        } else {
                            format!("`{name}` passed to a macro or a metavariable: it may be called")
                        };
                        if *rule == Rule::Libm {
                            self.libm(i, m);
                        } else {
                            self.hit(i, rule, m);
                        }
                    }
                }
            }
        }
    }

    fn check_glob(&mut self, l: &UseLeaf, star: usize) {
        let (rs, overflow) = self.resolve(&l.segs, l.absolute);
        let first = l.toks.first().copied().unwrap_or(star);
        if overflow {
            self.parse_hit(first, "alias chain is cyclic, too deep or too wide to resolve");
        }
        let shown = if l.segs.is_empty() { "::".to_string() } else { l.segs.join("::") };
        for (rule, pat) in patterns(P::Module) {
            let p: Vec<&str> = pat.split("::").collect();
            if let Some(k) = find_at(&l.segs, &p) {
                let at = l.toks.get(k + p.len() - 1).copied().unwrap_or(star);
                self.hit(at, rule, format!("glob import `{shown}::*` uses `{pat}`"));
            } else if rs.iter().any(|r| find_at(r, &p).is_some() || reaches_any(r, &p)) {
                self.hit(star, rule, format!("glob import `{shown}::*` can reach `{pat}`"));
            }
        }
    }

    // -------------------------------------------------------------- single tokens

    fn check_token(&mut self, i: usize) {
        let Some(tok) = self.t(i) else { return };
        let Kind::Ident { raw } = tok.kind else { return };
        let name = tok.text.as_str();
        if !raw && name == "unsafe" {
            self.hit(i, &Rule::Unsafe, "`unsafe` code".to_string());
        }
        if !raw && name == "extern" && !self.keyword(i + 1, "crate") {
            self.hit(i, &Rule::Unsafe, "`extern`: foreign ABI or FFI".to_string());
        }
        for (rule, pat) in name_rules(name) {
            let m = format!("identifier `{name}` (`{pat}`)");
            if *rule == Rule::Libm {
                self.libm(i, m);
            } else {
                self.hit(i, rule, m);
            }
        }
        let prev_dot = i.checked_sub(1).is_some_and(|p| self.is(p, &Kind::Dot));
        let call_next = self.is(i + 1, &Kind::Open(Delim::Paren)) || self.is(i + 1, &Kind::PathSep);
        if prev_dot && call_next {
            if patterns(P::Method).any(|(_, p)| p == name) || is_libm_name(name) {
                self.libm(i, format!("method call `.{name}(` (float transcendental)"));
            }
            if patterns(P::FloatMethod).any(|(_, p)| p == name) && self.float_method_operands(i) {
                self.hit(i, &Rule::MinMax, format!("`.{name}(` with a float operand is not total-order"));
            }
        }
        let prev_sep = i.checked_sub(1).is_some_and(|p| self.is(p, &Kind::PathSep));
        let prev_gt = i.checked_sub(2).is_some_and(|p| self.is(p, &Kind::Gt));
        if prev_sep && prev_gt && patterns(P::FloatMethod).any(|(_, p)| p == name) && self.qualified_float(i - 2) {
            self.hit(i, &Rule::MinMax, format!("`<…>::{name}` on a float type is not total-order"));
        }
        // `<f64>::sin`, `<f64 as Float>::cos`, `Libm::<f64>::exp`: like a path of two segments.
        if prev_sep && prev_gt {
            if is_libm_name(name) {
                self.libm(i, format!("`<…>::{name}` (float transcendental)"));
            }
            if let Some(rule) = other_call_rule(name) {
                if !self.qualified_int(i - 2) {
                    self.hit(i, rule, format!("`<…>::{name}`"));
                }
            }
        }
        if self.is(i + 1, &Kind::Bang) && matches!(self.kind(i + 2), Some(Kind::Open(_))) {
            // A renamed built-in macro (`use core::include_str as t; t!(…)`) counts as the macro.
            let mut names = vec![name.to_string()];
            if self.aliases.contains_key(name) {
                let (rs, _) = self.resolve(&[name.to_string()], false);
                names.extend(rs.into_iter().filter_map(|r| r.last().cloned()));
            }
            names.sort();
            names.dedup();
            for n in &names {
                self.check_macro(i, n);
            }
        }
        if self.in_attr.get(i).copied().unwrap_or(false) {
            for (rule, pat) in patterns(P::AttrName) {
                if pat == name {
                    self.hit(i, rule, format!("attribute `{name}`: exported or placed symbol"));
                }
            }
        }
        if name == "path" && self.in_attr.get(i).copied().unwrap_or(false) && self.is(i + 1, &Kind::Eq) {
            for (rule, pat) in patterns(P::Attr) {
                if pat == name && !self.safe_include(i + 2, None) {
                    self.hit(i, rule, "`#[path]` names a file the scan does not list".to_string());
                }
            }
        }
    }

    fn check_macro(&mut self, i: usize, name: &str) {
        for (rule, pat) in patterns(P::Macro) {
            if pat == name {
                self.hit(i, rule, format!("macro `{name}!`"));
            }
        }
        for (rule, pat) in patterns(P::IncludeMacro) {
            if pat == name {
                let close = self.partner.get(i + 2).copied().unwrap_or(usize::MAX);
                if !self.safe_include(i + 3, Some(close)) {
                    self.hit(i, rule, format!("`{name}!` of a file the scan does not list"));
                }
            }
        }
    }

    /// True when token `i` is a plain string literal naming a `.rs` file that this scan lists
    /// (relative to the current file's directory), and (for a macro) it is the only argument.
    fn safe_include(&self, i: usize, close: Option<usize>) -> bool {
        let Some(Token { kind: Kind::Lit { kind: LitKind::Str | LitKind::RawStr, suffix: false }, text, .. }) =
            self.t(i)
        else {
            return false;
        };
        if let Some(c) = close {
            if c != i + 1 {
                return false;
            }
        }
        if !text.ends_with(".rs") || !valid_relpath(text) {
            return false;
        }
        let dir = self.f.file.rsplit_once('/').map_or("", |(d, _)| d);
        let target = if dir.is_empty() { text.clone() } else { format!("{dir}/{text}") };
        self.f.crate_files.contains(&target)
    }

    /// True when a token range holds a float marker: a float literal, `f32`/`f64`, a
    /// metavariable, or an alias whose target goes through `f32`/`f64`.
    fn floaty(&self, from: usize, to: usize) -> bool {
        if to.saturating_sub(from) > MAX_OPERAND_TOKENS {
            return true;
        }
        (from..to).any(|k| match self.t(k) {
            Some(Token { kind: Kind::Lit { kind: LitKind::Float, .. }, .. }) => true,
            Some(Token { kind: Kind::Dollar, .. }) => true,
            Some(Token { kind: Kind::Ident { .. }, text, .. }) => {
                is_float_type(text)
                    || self.aliases.get(text).is_some_and(|ts| ts.iter().any(|t| t.iter().any(|s| is_float_type(s))))
            }
            _ => false,
        })
    }

    /// `.min(`/`.max(`/`.clamp(` at identifier `i`: true when the receiver or an argument is a float.
    fn float_method_operands(&self, i: usize) -> bool {
        // Arguments. `f64::min`/`max` take one and `clamp` two: an empty list is another method
        // (`Iterator::max()`).
        if self.is(i + 1, &Kind::Open(Delim::Paren)) {
            let close = self.partner.get(i + 1).copied().unwrap_or(usize::MAX);
            if close == i + 2 {
                return false;
            }
            if close == usize::MAX || self.floaty(i + 2, close) {
                return true;
            }
        }
        // Receiver: the token before the `.`, walking back over `(…)`, `[…]` and `{…}` groups
        // to the callee or indexed expression before them (`f64::from(a)`, `[1.0][0]`).
        let Some(mut r) = i.checked_sub(2) else { return false };
        for _ in 0..MAX_PATH_SEGMENTS {
            match self.kind(r) {
                Some(Kind::Close(_)) => {
                    let open = self.partner.get(r).copied().unwrap_or(usize::MAX);
                    if open == usize::MAX || self.floaty(open + 1, r) {
                        return true;
                    }
                    match open.checked_sub(1) {
                        Some(p) if matches!(self.kind(p), Some(Kind::Ident { .. } | Kind::Close(_) | Kind::Gt)) => {
                            r = p
                        }
                        _ => return false,
                    }
                }
                Some(Kind::Gt) => return self.qualified_float(r),
                _ => return self.receiver_float(r),
            }
        }
        true
    }

    /// The receiver token `r` (not a group): a float literal, or a path through `f32`/`f64`.
    fn receiver_float(&self, r: usize) -> bool {
        match self.kind(r) {
            Some(Kind::Lit { kind: LitKind::Float, .. }) => true,
            Some(Kind::Ident { .. }) => {
                // Walk back over `a::b::c` and look for a float type or a metavariable.
                let mut k = r;
                let mut steps = 0;
                loop {
                    if self.floaty(k, k + 1) || (k > 0 && self.is(k - 1, &Kind::Dollar)) {
                        return true;
                    }
                    if steps > MAX_PATH_SEGMENTS || k < 2 || !self.is(k - 1, &Kind::PathSep) {
                        return false;
                    }
                    if self.ident(k - 2).is_none() {
                        return false;
                    }
                    k -= 2;
                    steps += 1;
                }
            }
            _ => false,
        }
    }

    /// `<…>` ending at the `>` at `gt`: true when it mentions `f32`/`f64` or a metavariable,
    /// or is too long to check.
    /// `<…>` ending at `gt` names an integer type: an integer type name that is not a type or
    /// `use` alias, or an alias whose every target ends in an integer type.
    fn qualified_int(&self, gt: usize) -> bool {
        let mut depth = 0usize;
        let mut k = gt;
        for _ in 0..MAX_OPERAND_TOKENS {
            match self.kind(k) {
                Some(Kind::Gt) => depth += 1,
                Some(Kind::Lt) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return (k..gt).any(|j| {
                            self.ident(j).is_some_and(|name| {
                                let (rs, _) = self.resolve(&[name.to_string()], false);
                                !rs.is_empty() && rs.iter().all(|r| r.last().is_some_and(|l| is_int_type(l)))
                            })
                        });
                    }
                }
                None => return false,
                _ => {}
            }
            match k.checked_sub(1) {
                Some(p) => k = p,
                None => return false,
            }
        }
        false
    }

    fn qualified_float(&self, gt: usize) -> bool {
        let mut depth = 0usize;
        let mut k = gt;
        for _ in 0..MAX_OPERAND_TOKENS {
            match self.kind(k) {
                Some(Kind::Gt) => depth += 1,
                Some(Kind::Lt) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return self.floaty(k, gt);
                    }
                }
                None => return true,
                _ => {}
            }
            match k.checked_sub(1) {
                Some(p) => k = p,
                None => return true,
            }
        }
        true
    }
}

// ------------------------------------------------------------------ the closure

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(limit + 1).read_to_end(&mut buf)?;
    Ok(buf)
}

/// A proc-macro crate in the kernel closure is `KNP1-PARSE` at its `Cargo.toml` 1:1: its item
/// input (derive and attribute macros, and function-like macros from it) cannot be scanned, so
/// kernel crates may not depend on any proc-macro crate (maintainer decision, review 4). A
/// manifest that is not valid TOML is `KNP1-PARSE` too.
fn proc_macro_finding(c: &graph::CrateEntry) -> Result<Option<Finding>, SynError> {
    let bytes = read_bounded(&c.root.join("Cargo.toml"), MAX_CONFIG_BYTES).map_err(|e| SynError {
        code: SynErrorCode::Read,
        message: format!("{} {} Cargo.toml: {e}", c.name, c.version),
    })?;
    let parsed = std::str::from_utf8(&bytes).ok().and_then(|t| t.parse::<toml::Table>().ok());
    let message = match parsed {
        None => format!("{} {}: Cargo.toml is not valid UTF-8 TOML", c.name, c.version),
        Some(t) => {
            let lib = t.get("lib").and_then(toml::Value::as_table);
            let pm = lib.is_some_and(|l| {
                ["proc-macro", "proc_macro"].iter().any(|k| l.get(*k).and_then(toml::Value::as_bool) == Some(true))
            });
            if !pm {
                return Ok(None);
            }
            format!(
                "{} {} is a proc-macro crate in the kernel closure: its item input cannot be scanned",
                c.name, c.version
            )
        }
    };
    Ok(Some(Finding {
        rule: Rule::Parse,
        crate_name: c.name.clone(),
        crate_version: c.version.clone(),
        file: "Cargo.toml".to_string(),
        line: 1,
        column: 1,
        excerpt_digest: input_digest(message.as_bytes()),
        message,
    }))
}

/// Scans every listed file of every crate of the closure, in the closure's sorted order.
/// Fails with `KNP1-READ` if a listed file cannot be read and with `KNP1-CONFIG-INVALID` if a
/// facade file is not part of the closure.
pub fn scan_closure(closure: &Closure, cfg: &Config) -> Result<Outcome, SynError> {
    for ff in &cfg.facade_files {
        let known = closure
            .crates
            .iter()
            .any(|c| c.name == ff.crate_name && c.version == ff.version && c.files.contains(&ff.file));
        if !known {
            return Err(invalid(format!(
                "knp1-lint.toml: syn.facade_files entry {} {} {} is not a scanned file of the kernel closure",
                ff.crate_name, ff.version, ff.file
            )));
        }
    }
    for (p, fp) in &cfg.facade_paths {
        // The path names the crate by its library name, so it must be that one crate at that
        // exact version, and no other crate of the closure may share the library name.
        let ident = fp.crate_name.replace('-', "_");
        let same: Vec<&graph::CrateEntry> =
            closure.crates.iter().filter(|c| c.name.replace('-', "_") == ident).collect();
        let exact = same.len() == 1 && same.iter().all(|c| c.name == fp.crate_name && c.version == fp.version);
        if !exact {
            return Err(invalid(format!(
                "knp1-lint.toml: syn.facade_paths {:?} names {} {}, which is not exactly one crate of the kernel closure",
                p.join("::"),
                fp.crate_name,
                fp.version
            )));
        }
    }
    let mut out = Outcome::default();
    for c in &closure.crates {
        if let Some(f) = proc_macro_finding(c)? {
            out.findings.push(f);
        }
        for file in &c.files {
            let path = c.root.join(file);
            let bytes = read_bounded(&path, MAX_FILE_BYTES).map_err(|e| SynError {
                code: SynErrorCode::Read,
                message: format!("{} {} {file}: {e}", c.name, c.version),
            })?;
            let fr = FileRef { crate_name: &c.name, crate_version: &c.version, file, crate_files: &c.files };
            let o = scan_file(fr, &bytes, cfg);
            out.findings.extend(o.findings);
            out.notes.extend(o.notes);
        }
    }
    sort_findings(&mut out.findings);
    sort_findings(&mut out.notes);
    Ok(out)
}

/// Loads `knp1-lint.toml` at `config_path` (its `[graph]` and `[syn]` tables), computes the
/// kernel closure and scans it.
pub fn run(config_path: &Path) -> Result<Outcome, SynError> {
    let bytes =
        read_bounded(config_path, MAX_CONFIG_BYTES).map_err(|e| invalid(format!("{}: {e}", config_path.display())))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(invalid(format!("{}: larger than {MAX_CONFIG_BYTES} bytes", config_path.display())));
    }
    let text = String::from_utf8(bytes).map_err(|_| invalid("knp1-lint.toml: not UTF-8"))?;
    let cfg = parse_config(&text)?;
    let g = |e: graph::GraphError| SynError { code: SynErrorCode::Graph(e.code), message: e.message };
    let gcfg = graph::load_config(config_path).map_err(g)?;
    let closure = graph::compute_closure(&gcfg).map_err(g)?;
    scan_closure(&closure, &cfg)
}
