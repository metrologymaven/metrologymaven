//! Gate for the one named `unsafe` exception (decisions/D-048): the Windows token-file ACL
//! shim `fid-ctrl-acl`, module `ffi`.
//!
//! - every workspace member (the `workspace.members` entries of the root `Cargo.toml`, globs
//!   expanded) except `fid-ctrl-acl` has `[lints] workspace = true`;
//! - `fid-ctrl-acl` has no build script, and its `[lints]` are the workspace's with exactly two
//!   changes: `rust.unsafe_code` is `deny` instead of `forbid`, and `clippy` adds
//!   `undocumented_unsafe_blocks = "deny"`. No other key may be added, dropped or changed;
//! - every `.rs` file in the repository is tokenised (with proc-macro2, the lexer under syn),
//!   so comments and string literals are never mistaken for code and an attribute may span any
//!   number of lines. An attribute that names `unsafe_code` must be a plain `deny`/`forbid`.
//!   The only exception is one `#[allow(unsafe_code)]` directly on the top-level
//!   `mod ffi { .. }` of `crates/fid-ctrl-acl/src/lib.rs`. The ident `unsafe_code` may not
//!   appear outside an attribute (macro-built attributes), and an attribute holding `$` may not
//!   name it;
//! - in `fid-ctrl-acl`, no attribute may lift `undocumented_unsafe_blocks` or the `restriction`
//!   group that holds it, and no `#[path = ..]` (also inside `cfg_attr`) or `include!` may pull
//!   other files into the crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const EXCEPTION_CRATE: &str = "fid-ctrl-acl";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn exception_dir(root: &Path) -> PathBuf {
    root.join("crates").join(EXCEPTION_CRATE)
}

fn table(path: &Path) -> toml::Table {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.parse::<toml::Table>().unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn get<'a>(t: &'a toml::Table, keys: &[&str]) -> Option<&'a toml::Value> {
    let (first, rest) = keys.split_first()?;
    let mut v = t.get(*first)?;
    for k in rest {
        v = v.as_table()?.get(*k)?;
    }
    Some(v)
}

/// The member directories named by `workspace.members` of `root/Cargo.toml`. A `dir/*` entry
/// expands to every subdirectory of `dir` holding a `Cargo.toml`; any other glob is refused.
fn members(root: &Path) -> Result<Vec<PathBuf>, String> {
    let ws = table(&root.join("Cargo.toml"));
    let list = get(&ws, &["workspace", "members"]).and_then(toml::Value::as_array).ok_or("no workspace.members")?;
    let glob = |s: &str| s.contains(['*', '?', '[', ']', '{', '}']);
    let mut out = Vec::new();
    for m in list {
        let m = m.as_str().ok_or("a workspace member is not a string")?;
        if let Some(prefix) = m.strip_suffix("/*").filter(|p| !glob(p)) {
            let dir = root.join(prefix);
            let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .map(|e| e.unwrap().path())
                .filter(|p| p.join("Cargo.toml").is_file())
                .collect();
            found.sort();
            out.extend(found);
        } else if glob(m) {
            return Err(format!("workspace member pattern {m:?} is not supported by this gate"));
        } else {
            let dir = root.join(m);
            if !dir.join("Cargo.toml").is_file() {
                return Err(format!("workspace member {m} has no Cargo.toml"));
            }
            out.push(dir);
        }
    }
    Ok(out)
}

/// Members other than the exception crate that lack `[lints] workspace = true`, and members
/// that claim the exception crate's name outside its directory.
fn members_without_workspace_lints(root: &Path) -> Result<Vec<String>, String> {
    let mut bad = Vec::new();
    for d in members(root)? {
        let t = table(&d.join("Cargo.toml"));
        let name = get(&t, &["package", "name"]).and_then(toml::Value::as_str).unwrap_or_default().to_string();
        let workspace = get(&t, &["lints", "workspace"]).and_then(toml::Value::as_bool) == Some(true);
        if d == exception_dir(root) {
            if name != EXCEPTION_CRATE {
                bad.push(format!("{}: the exception directory must hold {EXCEPTION_CRATE}", d.display()));
            }
        } else if name == EXCEPTION_CRATE {
            bad.push(format!("{}: {EXCEPTION_CRATE} outside crates/{EXCEPTION_CRATE}", d.display()));
        } else if !workspace {
            bad.push(format!("{name} ({})", d.display()));
        }
    }
    Ok(bad)
}

#[test]
fn every_member_but_the_exception_uses_the_workspace_lints() {
    let root = root();
    let dirs = members(&root).unwrap();
    assert!(dirs.len() > 50, "member scan found too few crates");
    assert!(dirs.contains(&root.join("vendor/fid-clarabel/crate")), "the vendored solver is a member");
    assert!(dirs.contains(&exception_dir(&root)));
    let bad = members_without_workspace_lints(&root).unwrap();
    assert!(
        bad.is_empty(),
        "members without `[lints] workspace = true` (D-048 allows only {EXCEPTION_CRATE}): {bad:?}"
    );
}

/// Differences between the exception crate's manifest `ex` and what D-048 allows, given the
/// workspace manifest `ws`.
fn lint_violations(ws: &toml::Table, ex: &toml::Table, has_build_rs: bool) -> Vec<String> {
    let mut bad = Vec::new();
    let empty = toml::Table::new();
    let ws_lints = get(ws, &["workspace", "lints"]).and_then(toml::Value::as_table).unwrap_or(&empty);
    let ex_lints = get(ex, &["lints"]).and_then(toml::Value::as_table).unwrap_or(&empty);
    if get(ws, &["workspace", "lints", "rust", "unsafe_code"]).and_then(toml::Value::as_str) != Some("forbid") {
        bad.push("the workspace must forbid unsafe_code".to_string());
    }
    if has_build_rs || get(ex, &["package", "build"]).is_some() {
        bad.push("the exception crate may not have a build script".to_string());
    }
    let ws_keys: BTreeSet<&String> = ws_lints.keys().collect();
    let ex_keys: BTreeSet<&String> = ex_lints.keys().collect();
    if ws_keys != ex_keys {
        bad.push(format!("[lints] tables {ex_keys:?} must be exactly the workspace's {ws_keys:?}"));
    }
    for (group, ws_group) in ws_lints {
        let ws_group = ws_group.as_table().unwrap_or(&empty);
        let ex_group = ex_lints.get(group).and_then(toml::Value::as_table).unwrap_or(&empty);
        let mut want = ws_group.clone();
        match group.as_str() {
            "rust" => {
                want.insert("unsafe_code".into(), toml::Value::from("deny"));
            }
            "clippy" => {
                want.insert("undocumented_unsafe_blocks".into(), toml::Value::from("deny"));
            }
            _ => {}
        }
        let want_keys: BTreeSet<&String> = want.keys().collect();
        let have_keys: BTreeSet<&String> = ex_group.keys().collect();
        for k in want_keys.symmetric_difference(&have_keys) {
            bad.push(format!("lints.{group}.{k}: added or missing (D-048 allows no other change)"));
        }
        for (k, v) in &want {
            if let Some(have) = ex_group.get(k) {
                if have != v {
                    bad.push(format!("lints.{group}.{k} = {have:?}, must be {v:?}"));
                }
            }
        }
    }
    bad
}

#[test]
fn the_exception_crate_lints_are_the_workspace_lints_plus_two_changes() {
    let root = root();
    let ws = table(&root.join("Cargo.toml"));
    let ex = table(&exception_dir(&root).join("Cargo.toml"));
    assert!(get(&ws, &["workspace", "lints", "clippy"]).and_then(toml::Value::as_table).is_some_and(|t| !t.is_empty()));
    let bad = lint_violations(&ws, &ex, exception_dir(&root).join("build.rs").exists());
    assert!(bad.is_empty(), "{EXCEPTION_CRATE}: {bad:#?}");
}

fn set(t: &mut toml::Table, keys: &[&str], v: &str) {
    let (last, path) = keys.split_last().unwrap();
    let mut cur = t;
    for k in path {
        cur =
            cur.entry(k.to_string()).or_insert_with(|| toml::Value::Table(toml::Table::new())).as_table_mut().unwrap();
    }
    cur.insert(last.to_string(), toml::Value::from(v));
}

fn unset(t: &mut toml::Table, keys: &[&str]) {
    let (last, path) = keys.split_last().unwrap();
    let mut cur = t;
    for k in path {
        cur = cur.get_mut(*k).unwrap().as_table_mut().unwrap();
    }
    cur.remove(*last);
}

/// A named change to a manifest.
type Edit = (&'static str, fn(&mut toml::Table));

#[test]
fn the_lint_table_check_catches_every_change() {
    let ws = table(&root().join("Cargo.toml"));
    let good = table(&exception_dir(&root()).join("Cargo.toml"));
    assert_eq!(lint_violations(&ws, &good, false), Vec::<String>::new());
    assert!(!lint_violations(&ws, &good, true).is_empty(), "not caught: build.rs");
    let edits: [Edit; 9] = [
        ("extra clippy allow", |t| set(t, &["lints", "clippy", "missing_safety_doc"], "allow")),
        ("undocumented_unsafe_blocks warn", |t| set(t, &["lints", "clippy", "undocumented_unsafe_blocks"], "warn")),
        ("undocumented_unsafe_blocks missing", |t| unset(t, &["lints", "clippy", "undocumented_unsafe_blocks"])),
        ("workspace lint relaxed", |t| set(t, &["lints", "clippy", "unwrap_used"], "allow")),
        ("workspace lint dropped", |t| unset(t, &["lints", "clippy", "panic"])),
        ("unsafe_code allowed", |t| set(t, &["lints", "rust", "unsafe_code"], "allow")),
        ("extra rust lint", |t| set(t, &["lints", "rust", "unused"], "allow")),
        ("extra lint table", |t| set(t, &["lints", "rustdoc", "all"], "allow")),
        ("build script", |t| set(t, &["package", "build"], "build.rs")),
    ];
    for (what, edit) in edits {
        let mut ex = good.clone();
        edit(&mut ex);
        assert!(!lint_violations(&ws, &ex, false).is_empty(), "not caught: {what}");
    }
    let mut ex = good.clone();
    ex.get_mut("lints").unwrap().as_table_mut().unwrap().insert("workspace".into(), toml::Value::from(true));
    assert!(!lint_violations(&ws, &ex, false).is_empty(), "not caught: workspace = true");
    let mut relaxed_ws = ws.clone();
    set(&mut relaxed_ws, &["workspace", "lints", "rust", "unsafe_code"], "deny");
    assert!(!lint_violations(&relaxed_ws, &good, false).is_empty(), "not caught: workspace no longer forbids");
}

/// Where a file sits, which decides the rules applied to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    /// Anywhere outside the exception crate.
    Other,
    /// A file of the exception crate other than `src/lib.rs`.
    Exception,
    /// `crates/fid-ctrl-acl/src/lib.rs`: the only file that may hold the `mod ffi` allow.
    Shim,
}

/// Result of scanning one file.
#[derive(Debug, Default)]
struct Scan {
    /// Violations as "line: what".
    bad: Vec<String>,
    /// Allowed `#[allow(unsafe_code)] mod ffi { .. }` sites found.
    ffi_sites: usize,
}

fn ident(t: &TokenTree) -> Option<String> {
    match t {
        TokenTree::Ident(i) => {
            let s = i.to_string();
            Some(s.strip_prefix("r#").map(str::to_string).unwrap_or(s))
        }
        _ => None,
    }
}

fn is_punct(t: Option<&TokenTree>, c: char) -> bool {
    matches!(t, Some(TokenTree::Punct(p)) if p.as_char() == c)
}

fn line_of(t: &TokenTree) -> usize {
    t.span().start().line
}

/// Every token of `ts`, depth first, groups opened.
fn flatten(ts: TokenStream, out: &mut Vec<TokenTree>) {
    for t in ts {
        if let TokenTree::Group(g) = &t {
            flatten(g.stream(), out);
        } else {
            out.push(t);
        }
    }
}

/// Tokenises `text` and applies the rules for `place`. Text that is not Rust tokens fails.
fn scan(text: &str, place: Place) -> Scan {
    let mut s = Scan::default();
    match text.parse::<TokenStream>() {
        Ok(ts) => walk(ts, place, true, &mut s),
        Err(e) => s.bad.push(format!("cannot be tokenised: {e}")),
    }
    s
}

const LIFTS: [&str; 3] = ["allow", "expect", "warn"];

fn walk(ts: TokenStream, place: Place, top: bool, s: &mut Scan) {
    let toks: Vec<TokenTree> = ts.into_iter().collect();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        // An attribute: `#` [`!`] `[ .. ]`.
        if is_punct(Some(t), '#') {
            let inner = is_punct(toks.get(i + 1), '!');
            let at = if inner { i + 2 } else { i + 1 };
            if let Some(TokenTree::Group(g)) = toks.get(at) {
                if g.delimiter() == Delimiter::Bracket {
                    let site = Site { line: line_of(t), inner, top, place };
                    attribute(g.stream(), &toks[at + 1..], site, s);
                    i = at + 1;
                    continue;
                }
            }
        }
        match t {
            TokenTree::Group(g) => walk(g.stream(), place, false, s),
            TokenTree::Ident(_) => {
                let name = ident(t).unwrap_or_default();
                if name == "unsafe_code" {
                    s.bad.push(format!("{}: names the unsafe_code lint outside an attribute", line_of(t)));
                }
                if place != Place::Other && name == "include" && is_punct(toks.get(i + 1), '!') {
                    s.bad.push(format!("{}: include! in {EXCEPTION_CRATE}", line_of(t)));
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Where an attribute was found.
#[derive(Clone, Copy)]
struct Site {
    line: usize,
    /// `#![..]`.
    inner: bool,
    /// At the top level of the file.
    top: bool,
    place: Place,
}

/// Checks one attribute with content `content`, followed by the tokens `rest`.
fn attribute(content: TokenStream, rest: &[TokenTree], at: Site, s: &mut Scan) {
    let direct: Vec<TokenTree> = content.clone().into_iter().collect();
    let mut all = Vec::new();
    flatten(content, &mut all);
    let names: Vec<String> = all.iter().filter_map(ident).collect();
    let has = |n: &str| names.iter().any(|x| x == n);
    let dollar = all.iter().any(|t| is_punct(Some(t), '$'));
    let lifted = LIFTS.iter().any(|l| has(l));
    let first = direct.first().and_then(ident).unwrap_or_default();
    let plain = (first == "deny" || first == "forbid") && !lifted && !dollar;
    let line = at.line;
    if has("unsafe_code") {
        let ffi_site =
            at.place == Place::Shim && at.top && !at.inner && is_allow_unsafe_code(&direct) && is_mod_ffi(rest);
        if ffi_site {
            s.ffi_sites += 1;
        } else if !plain {
            s.bad.push(format!("{line}: lifts or rebuilds the unsafe_code lint"));
        }
    }
    if at.place != Place::Other {
        for n in ["undocumented_unsafe_blocks", "restriction"] {
            if has(n) && !plain {
                s.bad.push(format!("{line}: lifts clippy::{n} in {EXCEPTION_CRATE}"));
            }
        }
        for (k, t) in all.iter().enumerate() {
            if ident(t).as_deref() == Some("path") && is_punct(all.get(k + 1), '=') {
                s.bad.push(format!("{line}: #[path] in {EXCEPTION_CRATE}"));
            }
        }
    }
}

/// `allow(unsafe_code)`, exactly.
fn is_allow_unsafe_code(direct: &[TokenTree]) -> bool {
    let [a, TokenTree::Group(g)] = direct else { return false };
    let inside: Vec<TokenTree> = g.stream().into_iter().collect();
    ident(a).as_deref() == Some("allow")
        && g.delimiter() == Delimiter::Parenthesis
        && inside.len() == 1
        && ident(&inside[0]).as_deref() == Some("unsafe_code")
}

/// `mod ffi { .. }`, directly.
fn is_mod_ffi(rest: &[TokenTree]) -> bool {
    match rest {
        [m, f, TokenTree::Group(g), ..] => {
            ident(m).as_deref() == Some("mod")
                && ident(f).as_deref() == Some("ffi")
                && g.delimiter() == Delimiter::Brace
        }
        _ => false,
    }
}

/// Fixtures that are deliberately not Rust tokens (lexer and parser error cases of other
/// tools). They are no workspace member and are never compiled; for them the gate falls back to
/// a plain text check.
const UNLEXABLE_FIXTURES: [&str; 3] = [
    "fixtures/knp1/data/forms/src/lib.rs",
    "fixtures/knp1/syn/parse-error/src/lib.rs",
    "fixtures/rpt/model/fmt/lint/unlexable/src/lib.rs",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn unsafe_code_is_allowed_only_on_mod_ffi_of_the_exception_crate() {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(files.len() > 1000, "file scan found too few Rust files: {}", files.len());
    let ex_dir = exception_dir(&root);
    let shim = ex_dir.join("src").join("lib.rs");
    assert!(files.contains(&shim));
    let mut found_ffi = 0;
    let mut bad = Vec::new();
    for p in &files {
        let place = if *p == shim {
            Place::Shim
        } else if p.starts_with(&ex_dir) {
            Place::Exception
        } else {
            Place::Other
        };
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if UNLEXABLE_FIXTURES.iter().any(|f| *p == root.join(f)) {
            // Must stay unlexable (or leave the list), and must not even mention the lint.
            assert!(text.parse::<TokenStream>().is_err(), "{} now lexes: drop it from the list", p.display());
            if text.contains("unsafe_code") || text.contains(EXCEPTION_CRATE) {
                bad.push(format!("{}: an unlexable fixture names the lint or the crate", p.display()));
            }
            continue;
        }
        let s = scan(&text, place);
        found_ffi += s.ffi_sites;
        bad.extend(s.bad.into_iter().map(|b| format!("{}:{b}", p.display())));
    }
    assert!(bad.is_empty(), "D-048 gate:\n{}", bad.join("\n"));
    assert_eq!(found_ffi, 1, "expected exactly one allow on `mod ffi` in {}", shim.display());
}

/// Each bypass found in review, and the forms that must stay clean. The lint names are built
/// from pieces so this file holds no such attribute itself.
#[test]
fn the_scan_catches_every_bypass() {
    let l = concat!("unsafe", "_code");
    let u = concat!("undocumented", "_unsafe_blocks");
    let caught = |text: &str, place: Place| {
        let s = scan(text, place);
        assert!(!s.bad.is_empty(), "not caught in {place:?}:\n{text}");
        assert_eq!(s.ffi_sites, 0, "{text}");
    };
    for place in [Place::Other, Place::Exception, Place::Shim] {
        caught(&format!("#![allow({l})]\nfn f() {{}}\n"), place);
        caught(&format!("#[allow(dead_code, {l})]\nfn f() {{}}\n"), place);
        caught(&format!("#[ expect( {l} ) ]\nfn f() {{}}\n"), place);
        caught(&format!("#![warn({l})]\n"), place);
        // An attribute spanning several lines.
        caught(&format!("#[allow(\n    dead_code,\n    {l}\n)]\nfn f() {{}}\n"), place);
        caught(&format!("#\n[\nallow\n(\n{l}\n)\n]\nfn f() {{}}\n"), place);
        // Behind cfg_attr.
        caught(&format!("#[cfg_attr(all(), allow({l}))]\nfn f() {{}}\n"), place);
        caught(&format!("#![cfg_attr(\n    windows,\n    allow({l})\n)]\n"), place);
        // Inside a nested item, and as a raw identifier.
        caught(&format!("mod a {{ fn f() {{ #[allow({l})] {{}} }} }}\n"), place);
        caught(&format!("#[allow(r#{l})]\nfn f() {{}}\n"), place);
        // Built by a macro.
        caught(&format!("macro_rules! m {{ ($a:ident) => {{ #[$a({l})] fn f() {{}} }} }}\nm!(allow);\n"), place);
        caught(
            &format!("macro_rules! m {{ ($a:ident, $b:ident) => {{ #[$a($b)] fn f() {{}} }} }}\nm!(allow, {l});\n"),
            place,
        );
        // Not Rust tokens at all: fail closed.
        caught("fn f() { \"unterminated }\n", place);
    }
    // The `mod ffi` site: exactly `#[allow(..)]`, directly on the top-level `mod ffi { }` of the shim.
    let site = format!("#[allow({l})]\nmod ffi {{\n    fn f() {{}}\n}}\n");
    assert_eq!(scan(&site, Place::Shim).ffi_sites, 1);
    assert!(scan(&site, Place::Shim).bad.is_empty());
    caught(&site, Place::Exception);
    caught(&site, Place::Other);
    caught(&format!("mod outer {{\n    #[allow({l})]\n    mod ffi {{}}\n}}\n"), Place::Shim);
    caught(&format!("#[allow({l})]\nmod ffi2 {{}}\n"), Place::Shim);
    caught(&format!("#[allow({l})]\nmod ffi;\n"), Place::Shim);
    caught(&format!("#[allow({l})]\n#[cfg(windows)]\nmod ffi {{}}\n"), Place::Shim);
    caught(&format!("#![allow({l})]\nmod ffi {{}}\n"), Place::Shim);
    caught(&format!("#[allow({l}, dead_code)]\nmod ffi {{}}\n"), Place::Shim);
    caught(&format!("#[allow({l})]\nfn ffi() {{}}\n"), Place::Shim);
    let twice = format!("{site}{}", site.replace("fn f", "fn g"));
    assert_eq!(scan(&twice, Place::Shim).ffi_sites, 2, "both sites are counted; the gate wants one");
    // fid-ctrl-acl only: the clippy lint, its group, #[path] and include!.
    for place in [Place::Exception, Place::Shim] {
        caught(&format!("#[allow(clippy::{u})]\nfn f() {{}}\n"), place);
        caught(&format!("#![allow(\n    clippy::{u}\n)]\n"), place);
        caught(&format!("#[expect(clippy::{u})]\nfn f() {{}}\n"), place);
        caught(&format!("#[cfg_attr(windows, allow(clippy::{u}))]\nfn f() {{}}\n"), place);
        caught("#![allow(clippy::restriction)]\n", place);
        caught("#[path = \"elsewhere.rs\"]\nmod m;\n", place);
        caught("#[cfg_attr(windows, path = \"elsewhere.rs\")]\nmod m;\n", place);
        caught("mod m { include!(\"elsewhere.rs\"); }\n", place);
        caught("include ! (\"elsewhere.rs\");\n", place);
    }
    // Forms that must stay clean.
    let clean = [
        format!("#![forbid({l})]\n"),
        format!("#![deny({l})]\n"),
        format!("// #[allow({l})]\n/* #![allow({l})] */\nfn f() {{}}\n"),
        format!("/// #[allow({l})]\nfn f() {{}}\n"),
        format!("const S: &str = \"#[allow({l})]\";\nconst R: &str = r#\"#![allow({l})]\"#;\n"),
        format!("#[deny(clippy::{u})]\nfn f() {{}}\n"),
        "#[doc = \"path = x\"]\nfn f() {}\n".to_string(),
    ];
    for place in [Place::Other, Place::Exception, Place::Shim] {
        for c in &clean {
            let s = scan(c, place);
            assert!(s.bad.is_empty(), "false positive in {place:?}: {:?}\n{c}", s.bad);
        }
    }
    // The clippy lint is the exception crate's business only.
    assert!(scan(&format!("#[allow(clippy::{u})]\nfn f() {{}}\n"), Place::Other).bad.is_empty());
}

/// The member scan follows `workspace.members`, so a member outside `crates/` (like
/// vendor/fid-clarabel/crate) without the workspace lints is caught.
#[test]
fn the_member_scan_covers_every_workspace_member() {
    let dir = std::env::temp_dir().join(format!("fid-xtask-unsafe-members-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let write = |rel: &str, text: &str| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    let with_lints = |name: &str| format!("[package]\nname = \"{name}\"\n\n[lints]\nworkspace = true\n");
    write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"vendor/x/crate\"]\n");
    write("crates/a/Cargo.toml", &with_lints("a"));
    write(&format!("crates/{EXCEPTION_CRATE}/Cargo.toml"), &format!("[package]\nname = \"{EXCEPTION_CRATE}\"\n"));
    write("crates/notes/README.md", "not a crate\n");
    write("vendor/x/crate/Cargo.toml", &with_lints("x"));
    let found = members(&dir).unwrap();
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(members_without_workspace_lints(&dir).unwrap().is_empty());
    write("vendor/x/crate/Cargo.toml", "[package]\nname = \"x\"\n");
    assert_eq!(
        members_without_workspace_lints(&dir).unwrap(),
        vec![format!("x ({})", dir.join("vendor/x/crate").display())]
    );
    // The exception's name in another directory is not the exception.
    write("vendor/x/crate/Cargo.toml", &format!("[package]\nname = \"{EXCEPTION_CRATE}\"\n"));
    assert_eq!(members_without_workspace_lints(&dir).unwrap().len(), 1);
    // Globs this gate cannot expand are refused, not skipped.
    write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"vendor/*/crate\"]\n");
    assert!(members(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// span-locations must be on, or every violation would point at line 0.
#[test]
fn violations_carry_line_numbers() {
    let s = scan(&format!("fn f() {{}}\n\n#[allow({})]\nfn g() {{}}\n", concat!("unsafe", "_code")), Place::Other);
    assert_eq!(s.bad.len(), 1, "{:?}", s.bad);
    assert!(s.bad[0].starts_with("3:"), "{:?}", s.bad);
}
