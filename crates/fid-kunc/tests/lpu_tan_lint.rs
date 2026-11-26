//! REQ-UNC.PROP.LPU.TAN-003: the engine-facing LPU modules contain no finite-difference code
//! path. Plus the guard that only dev-dependencies enable the `tangent-harness` feature.
//!
//! A word deny-list is easy to evade (review of #634), so the lint is an allowlist. Every
//! file under `src/prop/lpu/` is lexed (string and character literals blanked first, then
//! comments removed, so a `//` inside a string cannot hide code), and then:
//!
//! - every `use` path (braces expanded, `as` aliases followed to their target) must be on
//!   [`USE_ALLOW`]; globs and `super` are refused;
//! - every other path rooted at `std`, `core`, `alloc`, `crate`, `self`, `super` or a `fid_*`
//!   crate must start with an allowed path, so `crate::prop::mcm::block::run_block(..)` or a
//!   fully qualified `fid_kci::kci::evaluate` is refused even without a `use`;
//! - the Monte Carlo block and kernel call interface names (`run_block`, `BlockModel`,
//!   `SamplingPlan`, `evaluate`, `KernelCall`, `KernelAlgorithm`, `AlgorithmTable`, `.run(`),
//!   evaluation callbacks (`Fn(`, `FnMut(`, `FnOnce(`, `fn(` pointer types), macros, `extern`,
//!   `include*` and perturbation identifiers are refused;
//! - `impl Trait` in argument or return position, `dyn` other than `dyn OperatorTangent`, a
//!   `trait` other than `OperatorTangent`, `where` clauses and generic type parameters are
//!   refused: an operator cannot be smuggled in through a local trait and called at
//!   `x + h`.
//!
//! Adding an entry to [`USE_ALLOW`] is a reviewed change.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// Paths LPU modules may name. A path is allowed when it equals an entry or extends it with
/// `::`.
const USE_ALLOW: &[&str] = &[
    "std::collections::BTreeMap",
    "std::fmt",
    "std::cmp::Ordering",
    "std::error::Error",
    "fid_kci::kci::json",
    "fid_kci::AlgorithmId",
    "fid_kci::KernelManifest",
    "fid_num::la::Mat",
    "fid_num::la::MAX_DIM",
    "fid_types::Digest",
    "crate::dist::UnitCode",
    "crate::model::Endpoint",
    "crate::model::OperatorNode",
    "crate::model::UncertaintyModel",
    "self",
    "tan",
];

/// Names refused anywhere in code.
const FORBIDDEN_IDENTS: &[&str] = &[
    "run_block",
    "BlockModel",
    "SamplingPlan",
    "BlockSpec",
    "mcm",
    "evaluate",
    "eval",
    "Eval",
    "KernelCall",
    "KernelAlgorithm",
    "AlgorithmTable",
    "tangent_harness",
    "STEP",
    "macro_rules",
    "include",
    "include_str",
    "include_bytes",
    "extern",
    "super",
    "where",
    "unsafe",
    "asm",
];
/// Identifier parts refused anywhere in code (case-insensitive).
const FORBIDDEN_IDENT_PARTS: &[&str] = &["perturb", "finite_diff", "central_diff", "forward_diff", "backward_diff"];
/// The only trait an LPU module may define or name behind `dyn`.
const TRAIT_ALLOW: &[&str] = &["OperatorTangent"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Lifetime,
    Lit,
    Punct(String),
}

impl Tok {
    fn is(&self, s: &str) -> bool {
        match self {
            Tok::Ident(x) | Tok::Punct(x) => x == s,
            _ => false,
        }
    }
    fn ident(&self) -> Option<&str> {
        match self {
            Tok::Ident(x) => Some(x),
            _ => None,
        }
    }
}

/// Lexes Rust source: literals first (strings, raw strings, byte strings, characters), then
/// comments, so neither can hide the other.
fn lex(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let n = c.len();
    while i < n {
        let ch = c[i];
        // Raw strings r"..", r#".."#, br#".."#.
        let raw_start = if ch == 'r' && i + 1 < n && (c[i + 1] == '"' || c[i + 1] == '#') {
            Some(i + 1)
        } else if ch == 'b' && i + 2 < n && c[i + 1] == 'r' && (c[i + 2] == '"' || c[i + 2] == '#') {
            Some(i + 2)
        } else {
            None
        };
        if let Some(mut j) = raw_start {
            let mut hashes = 0;
            while j < n && c[j] == '#' {
                hashes += 1;
                j += 1;
            }
            if j < n && c[j] == '"' {
                j += 1;
                loop {
                    if j >= n {
                        break;
                    }
                    if c[j] == '"' && (0..hashes).all(|k| j + 1 + k < n && c[j + 1 + k] == '#') {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                out.push(Tok::Lit);
                i = j;
                continue;
            }
        }
        if ch == '"' || (ch == 'b' && i + 1 < n && c[i + 1] == '"') {
            let mut j = if ch == 'b' { i + 2 } else { i + 1 };
            while j < n && c[j] != '"' {
                if c[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            out.push(Tok::Lit);
            i = j + 1;
            continue;
        }
        if ch == '\'' || (ch == 'b' && i + 1 < n && c[i + 1] == '\'') {
            let s = if ch == 'b' { i + 1 } else { i };
            if s + 1 < n && c[s + 1] == '\\' {
                let mut j = s + 2;
                while j < n && c[j] != '\'' {
                    j += 1;
                }
                out.push(Tok::Lit);
                i = j + 1;
                continue;
            }
            if s + 2 < n && c[s + 2] == '\'' {
                out.push(Tok::Lit);
                i = s + 3;
                continue;
            }
            // A lifetime or label.
            let mut j = s + 1;
            while j < n && (c[j].is_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            out.push(Tok::Lifetime);
            i = j;
            continue;
        }
        if ch == '/' && i + 1 < n && c[i + 1] == '/' {
            while i < n && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && i + 1 < n && c[i + 1] == '*' {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if c[i] == '/' && i + 1 < n && c[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && i + 1 < n && c[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let mut j = i;
            while j < n && (c[j].is_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            let mut word: String = c[i..j].iter().collect();
            // Raw identifiers r#name.
            if word == "r" && j + 1 < n && c[j] == '#' && (c[j + 1].is_alphabetic() || c[j + 1] == '_') {
                let mut k = j + 1;
                while k < n && (c[k].is_alphanumeric() || c[k] == '_') {
                    k += 1;
                }
                word = c[j + 1..k].iter().collect();
                j = k;
            }
            out.push(Tok::Ident(word));
            i = j;
            continue;
        }
        if ch.is_ascii_digit() {
            let mut j = i;
            while j < n
                && (c[j].is_alphanumeric() || c[j] == '_' || (c[j] == '.' && j + 1 < n && c[j + 1].is_ascii_digit()))
            {
                j += 1;
            }
            out.push(Tok::Lit);
            i = j;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let two: String = c[i..(i + 2).min(n)].iter().collect();
        if two == "::" || two == "->" {
            out.push(Tok::Punct(two));
            i += 2;
        } else {
            out.push(Tok::Punct(ch.to_string()));
            i += 1;
        }
    }
    out
}

fn allowed(path: &str) -> bool {
    USE_ALLOW.iter().any(|a| path == *a || path.starts_with(&format!("{a}::")))
}

fn crate_root(s: &str) -> bool {
    matches!(s, "std" | "core" | "alloc" | "crate" | "self" | "super") || s.starts_with("fid_")
}

/// Expands one `use` tree starting at `toks[*i]` into full paths.
fn use_tree(toks: &[Tok], i: &mut usize, prefix: &str, out: &mut Vec<String>) {
    let join = |p: &str, s: &str| if p.is_empty() { s.to_string() } else { format!("{p}::{s}") };
    let mut path = prefix.to_string();
    if toks.get(*i).is_some_and(|t| t.is("::")) {
        *i += 1;
    }
    loop {
        match toks.get(*i) {
            Some(Tok::Ident(s)) => {
                path = join(&path, s);
                *i += 1;
            }
            Some(t) if t.is("*") => {
                out.push(join(&path, "*"));
                *i += 1;
                return;
            }
            Some(t) if t.is("{") => {
                *i += 1;
                loop {
                    match toks.get(*i) {
                        Some(t) if t.is("}") => {
                            *i += 1;
                            break;
                        }
                        Some(t) if t.is(",") => *i += 1,
                        None => break,
                        _ => use_tree(toks, i, &path, out),
                    }
                }
                return;
            }
            _ => {
                out.push(path);
                return;
            }
        }
        match toks.get(*i) {
            Some(t) if t.is("::") => *i += 1,
            Some(Tok::Ident(a)) if a == "as" => {
                *i += 2;
                out.push(path);
                return;
            }
            _ => {
                out.push(path);
                return;
            }
        }
    }
}

/// Every finding in `src` (`name` for messages).
fn lint(name: &str, src: &str) -> Vec<String> {
    let toks = lex(src);
    let mut found = Vec::new();
    let mut bad = |what: String| found.push(format!("{name}: {what}"));
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let prev = if i > 0 { Some(&toks[i - 1]) } else { None };
        let next = toks.get(i + 1);
        if let Some(w) = t.ident() {
            let lower = w.to_ascii_lowercase();
            if FORBIDDEN_IDENTS.contains(&w) || FORBIDDEN_IDENT_PARTS.iter().any(|p| lower.contains(p)) {
                bad(format!("identifier {w}"));
            }
            if matches!(w, "Fn" | "FnMut" | "FnOnce" | "fn") && next.is_some_and(|x| x.is("(")) {
                bad(format!("callback type {w}("));
            }
            if w == "run" && prev.is_some_and(|p| p.is(".") || p.is("::")) {
                bad("call of run".to_string());
            }
            if w == "impl" {
                let item_level = prev.is_none_or(|p| p.is("}") || p.is(";") || p.is("]") || p.is("{"));
                if !item_level {
                    bad("impl Trait in argument or return position".to_string());
                }
                if next.is_some_and(|x| x.is("<")) {
                    generic_params(&toks, i + 1, &mut bad);
                }
            }
            if w == "dyn" && !next.and_then(Tok::ident).is_some_and(|x| TRAIT_ALLOW.contains(&x)) {
                bad("dyn other than dyn OperatorTangent".to_string());
            }
            if w == "trait" && !next.and_then(Tok::ident).is_some_and(|x| TRAIT_ALLOW.contains(&x)) {
                bad("trait other than OperatorTangent".to_string());
            }
            if matches!(w, "fn" | "struct" | "enum" | "type" | "union")
                && next.is_some_and(|x| x.ident().is_some())
                && toks.get(i + 2).is_some_and(|x| x.is("<"))
            {
                generic_params(&toks, i + 2, &mut bad);
            }
            if w == "use" {
                let mut j = i + 1;
                let mut paths = Vec::new();
                use_tree(&toks, &mut j, "", &mut paths);
                for p in paths {
                    if !allowed(&p) {
                        bad(format!("use path {p}"));
                    }
                }
                i = j;
                continue;
            }
            // A path expression `root::a::b`.
            let after_sep = prev.is_some_and(|p| p.is("::"));
            if !after_sep && next.is_some_and(|x| x.is("::")) && crate_root(w) {
                let mut path = w.to_string();
                let mut j = i + 1;
                while toks.get(j).is_some_and(|x| x.is("::")) {
                    match toks.get(j + 1).and_then(Tok::ident) {
                        Some(s) => {
                            path = format!("{path}::{s}");
                            j += 2;
                        }
                        None => break,
                    }
                }
                if !allowed(&path) {
                    bad(format!("path {path}"));
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// Generic parameters starting at the `<` at `toks[at]`: only lifetimes are allowed.
fn generic_params(toks: &[Tok], at: usize, bad: &mut impl FnMut(String)) {
    let mut depth = 0;
    for t in &toks[at..] {
        if t.is("<") {
            depth += 1;
        } else if t.is(">") {
            depth -= 1;
            if depth == 0 {
                return;
            }
        } else if !(matches!(t, Tok::Lifetime) || t.is(",")) {
            bad("generic type parameter".to_string());
            return;
        }
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let p = e.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn req003_lpu_modules_pass_the_allowlist_lint() {
    let mut files = Vec::new();
    rust_files(&crate_dir().join("src/prop/lpu"), &mut files);
    files.sort();
    assert!(files.iter().any(|f| f.ends_with("tan.rs")) && files.iter().any(|f| f.ends_with("mod.rs")), "{files:?}");
    let mut found = Vec::new();
    for f in &files {
        found.extend(lint(&f.display().to_string(), &std::fs::read_to_string(f).unwrap()));
    }
    assert!(found.is_empty(), "REQ-003 lint findings in LPU modules: {found:#?}");
}

#[test]
fn req003_lint_flags_the_harness_and_known_evasions() {
    // The harness itself differentiates numerically.
    let harness = std::fs::read_to_string(crate_dir().join("src/tangent_harness.rs")).unwrap();
    assert!(!lint("tangent_harness.rs", &harness).is_empty());
    let cases = [
        // Review of #634, evasion 1: the Monte Carlo block, fully qualified, no `use`.
        "fn s(m: &M, p: &P, k: &K) -> f64 { let out = crate::prop::mcm::block::run_block(m, p, k); 0.0 }",
        "use crate::prop::mcm::block::run_block;",
        "use crate::prop::{mcm::block::{run_block as go}};",
        // Evasion 2: the kernel algorithm trait under an alias, called as A::run.
        "use fid_kci::KernelAlgorithm as A;\nfn s(k: &Kx, p: &[u8], x: &[&[u8]]) { let _ = A::run(k, p, None, x); }",
        // Evasion 3: a `//` inside a string literal hiding code after it.
        "fn s(t: &T, c: &C) { let sep = \"//\"; let r = fid_kci::kci::evaluate(t, c); }",
        "fn s(t: &T, c: &C) { let sep = r#\"//\"#; let r = fid_kci::kci::sha256_digest(t); }",
        // Evasion 4: an operator behind `impl Trait`, applied at x + h.
        "fn s(op: impl Apply, x0: f64, h: f64) -> f64 { (op.apply(x0 + h) - op.apply(x0)) / h }",
        "trait Apply { fn apply(&self, x: f64) -> f64; }",
        "fn s<T: Apply>(op: &T, x0: f64, h: f64) -> f64 { op.apply(x0 + h) }",
        "fn s(op: &dyn Apply, x0: f64) -> f64 { op.apply(x0) }",
        "fn s() -> impl Sized { 0 }",
        // Plain finite differences and callbacks.
        "fn d(f: &dyn Fn(&[f64]) -> f64, x: &mut [f64]) { x[0] += 1e-6; }",
        "fn d(f: fn(f64) -> f64) {}",
        "let perturbed = x + h;",
        "fn central_difference() {}",
        "use super::super::mcm::block::run_block;",
        "use std::process::Command;",
        "use fid_kci::*;",
        "macro_rules! m { () => {} }",
    ];
    for s in cases {
        assert!(!lint("sample", s).is_empty(), "not flagged: {s}");
    }
    // Allowed shapes stay clean: comments, strings, lifetimes, allowed paths, item impls.
    let clean = [
        "// never evaluate an operator at a perturbed input\nlet a = 1;",
        "let s = \"run_block evaluate perturb\"; let c = 'x'; let e = '\\n';",
        "use std::collections::BTreeMap;\nuse crate::model::{Endpoint, OperatorNode};",
        "pub fn f<'m>(m: &'m M) -> Vec<&'m E> { let o = std::cmp::Ordering::Less; Vec::new() }",
        "impl fmt::Display for E { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { Ok(()) } }",
        "fn g(p: &dyn OperatorTangent) {}",
        "pub use tan::{coverage_report, TanError};",
    ];
    for s in clean {
        assert!(lint("sample", s).is_empty(), "false finding for {s}: {:?}", lint("sample", s));
    }
}

// ------------------------------------------------------------------ feature guard

/// Findings for every Cargo.toml that enables `fid-kunc/tangent-harness` outside a
/// dev-dependency, or makes it a default feature of fid-kunc.
fn feature_findings(name: &str, toml: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut section = String::new();
    for raw in toml.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            continue;
        }
        if !line.contains("tangent-harness") {
            continue;
        }
        let dev = section.ends_with("dev-dependencies") || section.contains("dev-dependencies.");
        let dep_section = section.ends_with("dependencies") || section.contains("dependencies.");
        let kunc_table = section.ends_with("dependencies.fid-kunc");
        if dep_section && !dev && (line.contains("fid-kunc") || kunc_table) {
            found.push(format!("{name}: [{section}] {line}"));
        }
        if section == "features"
            && (line.contains("fid-kunc/tangent-harness") || line.contains("fid-kunc?/tangent-harness"))
        {
            found.push(format!("{name}: [features] {line}"));
        }
        if section == "features" && line.starts_with("default") {
            found.push(format!("{name}: default feature {line}"));
        }
    }
    found
}

#[test]
fn tangent_harness_is_enabled_by_dev_dependencies_only() {
    let crates = crate_dir().join("..");
    let mut found = Vec::new();
    let mut seen = 0;
    for e in std::fs::read_dir(&crates).unwrap() {
        let toml = e.unwrap().path().join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&toml) {
            seen += 1;
            found.extend(feature_findings(&toml.display().to_string(), &text));
        }
    }
    assert!(seen > 10, "only {seen} crates found under {}", crates.display());
    assert!(found.is_empty(), "tangent-harness enabled outside dev-dependencies: {found:#?}");
    // fid-kunc's own dev-dependency is the one allowed use.
    let own = std::fs::read_to_string(crate_dir().join("Cargo.toml")).unwrap();
    assert!(own.contains("fid-kunc = { path = \".\", features = [\"tangent-harness\"] }"));
    // The guard itself catches the shapes it exists for.
    for bad in [
        "[dependencies]\nfid-kunc = { path = \"../fid-kunc\", features = [\"tangent-harness\"] }",
        "[dependencies.fid-kunc]\npath = \"../fid-kunc\"\nfeatures = [\"tangent-harness\"]",
        "[target.'cfg(unix)'.dependencies]\nfid-kunc = { path = \"../fid-kunc\", features = [\"tangent-harness\"] }",
        "[features]\nx = [\"fid-kunc/tangent-harness\"]",
        "[features]\ndefault = [\"tangent-harness\"]",
    ] {
        assert!(!feature_findings("sample", bad).is_empty(), "not flagged: {bad}");
    }
    assert!(feature_findings(
        "sample",
        "[dev-dependencies]\nfid-kunc = { path = \"../fid-kunc\", features = [\"tangent-harness\"] }"
    )
    .is_empty());
}
