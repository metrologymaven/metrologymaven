//! `apps-policy`: the app-policy boundary and cross-app command identity check (spec node
//! `ECO.APPS.POLICY`, REQ-ECO.APPS-002 and REQ-ECO.APPS-003).
//!
//! Reads only artifacts: Cargo's resolved graph (`cargo metadata`, shared with `layers`),
//! `layers.toml`, `apps.toml` (when present), Rust source text, one registry manifest JSON per
//! app and `apps-status.json`. Findings:
//!
//! - `POLICY-DEFN`: an `app`-layer crate depends directly on a configured declaration-macro
//!   crate, or its source contains a configured command-declaration marker;
//! - `POLICY-LEAK`: a shared crate depends on an `app`-layer crate, or its non-test source uses
//!   a configured app-identity token;
//! - `POLICY-IDENT`: a command ID effective in two or more apps with differing
//!   `definition_digest`;
//! - `POLICY-SIZE`: an `app`-layer crate has a Rust file other than `src/main.rs`, or a
//!   `src/main.rs` over [`MAIN_RS_MAX_LINES`] lines, without an `[[exception]]`;
//! - `POLICY-INPUT`: an input is missing or malformed.
//!
//! "Shared crate" means a workspace crate in a product layer below `app`; `tool` and
//! `test-support` crates are repository tooling, not shared product code, and are not checked
//! for leaks. Token scans skip comments, string and character literals (a small lexer), and
//! for leaks also `#[cfg(test)]` items and test files. Output is sorted by code, subject
//! (crate or command ID) and location, so two runs give the same bytes.

use super::{parse_layer_map, Graph, LayerMap, TEST_SUPPORT};
use fid_canon::{parse_with_limits, CValue};
use fid_types::{CommandId, Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// The layer that holds per-app binary crates.
pub const APP_LAYER: &str = "app";
/// The default config file name, looked up in the workspace directory.
pub const CONFIG_FILE: &str = "apps-policy.toml";
/// The initial app-identity tokens (REQ-ECO.APPS.POLICY-002), used when the config sets none.
pub const DEFAULT_IDENTITY_TOKENS: [&str; 4] = ["AppId::from", "AppId::new", "current_app", "FID_APP"];
/// Longest allowed `src/main.rs` of an app crate, in lines.
pub const MAIN_RS_MAX_LINES: usize = 60;
/// Largest input file read (config, JSON, Rust source).
pub const MAX_FILE_BYTES: u64 = 16 << 20;
/// Most registry manifests read from one directory.
pub const MAX_MANIFESTS: usize = 1000;
/// Layers not checked for leaks: repository tooling, not shared product code.
const LEAK_EXEMPT: [&str; 2] = ["tool", TEST_SUPPORT];

/// Finding codes, in output order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    Defn,
    Ident,
    Input,
    Leak,
    Size,
}

impl Code {
    /// The printed code.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Defn => "POLICY-DEFN",
            Code::Ident => "POLICY-IDENT",
            Code::Input => "POLICY-INPUT",
            Code::Leak => "POLICY-LEAK",
            Code::Size => "POLICY-SIZE",
        }
    }
}

/// One finding. Field order is the sort order: code, subject, file, line, message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    /// Crate name, command ID or input name.
    pub subject: String,
    /// Path relative to the crate directory (`/`-separated), or empty.
    pub file: String,
    /// 1-based line, or 0 when the finding is about the whole file.
    pub line: usize,
    pub message: String,
}

impl Finding {
    fn new(code: Code, subject: &str, file: &str, line: usize, message: String) -> Finding {
        Finding { code, subject: subject.to_string(), file: file.to_string(), line, message }
    }

    fn input(subject: &str, message: String) -> Finding {
        Finding::new(Code::Input, subject, "", 0, message)
    }
}

impl fmt::Display for Finding {
    /// `<code> <subject>[ <file>[:<line>]]: <message>`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code.as_str(), self.subject)?;
        if !self.file.is_empty() {
            write!(f, " {}", self.file)?;
            if self.line > 0 {
                write!(f, ":{}", self.line)?;
            }
        }
        write!(f, ": {}", self.message)
    }
}

/// A `[[exception]]` entry of `apps-policy.toml`: a maintainer-approved extra file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exception {
    pub krate: String,
    pub file: String,
    pub approver: String,
    pub reason: String,
}

/// The parsed `apps-policy.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Registry declaration-macro crates an app crate may not depend on.
    pub macro_crates: BTreeSet<String>,
    /// Command-declaration attributes or macros an app crate may not contain.
    pub declaration_markers: BTreeSet<String>,
    /// App-identity tokens shared crates may not use.
    pub identity_tokens: BTreeSet<String>,
    pub exceptions: Vec<Exception>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            macro_crates: BTreeSet::new(),
            declaration_markers: BTreeSet::new(),
            identity_tokens: DEFAULT_IDENTITY_TOKENS.iter().map(|t| t.to_string()).collect(),
            exceptions: Vec::new(),
        }
    }
}

fn string_list(v: &Value, key: &str) -> Result<BTreeSet<String>, String> {
    let Value::Array(items) = v else {
        return Err(format!("{key} must be an array of strings"));
    };
    items
        .iter()
        .map(|i| match i {
            Value::String(s) if !s.trim().is_empty() => Ok(s.clone()),
            _ => Err(format!("{key} items must be non-empty strings")),
        })
        .collect()
}

impl Config {
    /// Parses `apps-policy.toml`. Keys not set keep their defaults; unknown keys are refused.
    pub fn parse(text: &str) -> Result<Config, String> {
        let table: Table = text.parse().map_err(|e: toml::de::Error| e.to_string().trim_end().replace('\n', " "))?;
        let mut c = Config::default();
        for (key, v) in &table {
            match key.as_str() {
                "macro_crates" => c.macro_crates = string_list(v, key)?,
                "declaration_markers" => c.declaration_markers = string_list(v, key)?,
                "identity_tokens" => c.identity_tokens = string_list(v, key)?,
                "exception" => {
                    let Value::Array(items) = v else {
                        return Err("exception must be an array of tables ([[exception]])".into());
                    };
                    for item in items {
                        c.exceptions.push(parse_exception(item)?);
                    }
                }
                other => return Err(format!("unknown key {other}")),
            }
        }
        Ok(c)
    }

    fn excepted(&self, krate: &str, file: &str) -> bool {
        self.exceptions.iter().any(|e| e.krate == krate && e.file == file)
    }
}

fn parse_exception(v: &Value) -> Result<Exception, String> {
    let Value::Table(t) = v else {
        return Err("exception must be an array of tables ([[exception]])".into());
    };
    for k in t.keys() {
        if !["crate", "file", "approver", "reason"].contains(&k.as_str()) {
            return Err(format!("unknown key {k} in [[exception]]"));
        }
    }
    let get = |k: &str| match t.get(k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(format!("[[exception]] needs a non-empty string {k}")),
    };
    Ok(Exception { krate: get("crate")?, file: get("file")?, approver: get("approver")?, reason: get("reason")? })
}

// ---------------------------------------------------------------------------------------
// Source scanning

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn blank(out: &mut [char], from: usize, to: usize) {
    for c in out.iter_mut().take(to).skip(from) {
        if *c != '\n' {
            *c = ' ';
        }
    }
}

/// The source with comments, string literals (plain, byte and raw) and character literals
/// replaced by spaces. Line breaks are kept, so line numbers stay valid.
pub fn code_only(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = c.clone();
    let n = c.len();
    let at = |k: usize| c.get(k).copied();
    let mut i = 0;
    while i < n {
        let ch = c[i];
        let start = i;
        if ch == '/' && at(i + 1) == Some('/') {
            while i < n && c[i] != '\n' {
                i += 1;
            }
            blank(&mut out, start, i);
            continue;
        }
        if ch == '/' && at(i + 1) == Some('*') {
            let mut depth = 1usize;
            i += 2;
            while i < n && depth > 0 {
                if c[i] == '/' && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            blank(&mut out, start, i);
            continue;
        }
        let prev_ident = i > 0 && is_ident(c[i - 1]);
        if !prev_ident && (ch == 'r' || (ch == 'b' && at(i + 1) == Some('r'))) {
            // Raw string: r"..", r#".."#, br"..". Anything else (r#ident, a name) is code.
            let mut j = if ch == 'r' { i + 1 } else { i + 2 };
            let mut hashes = 0usize;
            while at(j) == Some('#') {
                hashes += 1;
                j += 1;
            }
            if at(j) == Some('"') {
                j += 1;
                loop {
                    match at(j) {
                        None => break,
                        Some('"') if (1..=hashes).all(|h| at(j + h) == Some('#')) => {
                            j += 1 + hashes;
                            break;
                        }
                        Some(_) => j += 1,
                    }
                }
                blank(&mut out, start, j);
                i = j;
                continue;
            }
        }
        if ch == '"' {
            i += 1;
            while i < n {
                if c[i] == '\\' {
                    i += 2;
                    continue;
                }
                i += 1;
                if c[i - 1] == '"' {
                    break;
                }
            }
            blank(&mut out, start, i);
            continue;
        }
        if ch == '\'' {
            if at(i + 1) == Some('\\') {
                // Escaped character literal: '\n', '\'', '\u{..}'.
                i += 3;
                while i < n && c[i] != '\'' && c[i] != '\n' {
                    i += 1;
                }
                if i < n && c[i] == '\'' {
                    i += 1;
                }
                blank(&mut out, start, i);
                continue;
            }
            if at(i + 2) == Some('\'') {
                blank(&mut out, start, i + 3);
                i += 3;
                continue;
            }
            // A lifetime or a loop label.
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// Blanks every item marked `#[cfg(test)]` (up to its `;` or matching `}`) in code that has
/// already been through [`code_only`].
pub fn without_cfg_test(code: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let attr: Vec<char> = ATTR.chars().collect();
    let mut out: Vec<char> = code.chars().collect();
    let n = out.len();
    let mut i = 0;
    while i + attr.len() <= n {
        if out[i..i + attr.len()] != attr[..] {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i + attr.len();
        while j < n && out[j] != '{' && out[j] != ';' {
            j += 1;
        }
        if j < n && out[j] == '{' {
            let mut depth = 0usize;
            while j < n {
                match out[j] {
                    '{' => depth += 1,
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        let end = (j + 1).min(n);
        blank(&mut out, start, end);
        i = end;
    }
    out.into_iter().collect()
}

/// Occurrences of `token` in `code` as (1-based line). A token that starts or ends with an
/// identifier character must not touch another identifier character there.
pub fn token_lines(code: &str, token: &str) -> Vec<usize> {
    let tok: Vec<char> = token.chars().collect();
    let (Some(&first), Some(&last)) = (tok.first(), tok.last()) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for (k, line) in code.lines().enumerate() {
        let l: Vec<char> = line.chars().collect();
        let hit = (0..l.len().saturating_sub(tok.len() - 1)).any(|i| {
            i + tok.len() <= l.len()
                && l[i..i + tok.len()] == tok[..]
                && !(is_ident(first) && i > 0 && is_ident(l[i - 1]))
                && !(is_ident(last) && l.get(i + tok.len()).is_some_and(|c| is_ident(*c)))
        });
        if hit {
            lines.push(k + 1);
        }
    }
    lines
}

// ---------------------------------------------------------------------------------------
// File access

fn read_text(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("{} is larger than {MAX_FILE_BYTES} bytes", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))
}

/// Every `.rs` file under `dir`, as sorted `/`-separated paths relative to it. Skips `target`
/// and hidden directories.
fn rust_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, String)> = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, rel)) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("cannot list {}: {e}", d.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot list {}: {e}", d.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    stack.push((path, child));
                }
            } else if name.ends_with(".rs") {
                out.push(child);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A file that counts as non-test source for the leak scan: under `src/` (or `build.rs`),
/// outside any `tests` directory and not named `tests.rs`.
fn is_non_test_source(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    (rel == "build.rs" || parts.first() == Some(&"src")) && !parts.iter().any(|p| *p == "tests" || *p == "tests.rs")
}

// ---------------------------------------------------------------------------------------
// Crate checks: DEFN, LEAK, SIZE

/// A workspace crate as the checks see it.
#[derive(Debug, Clone)]
struct Krate {
    name: String,
    layer: Option<String>,
    dir: Option<PathBuf>,
    /// Names of direct dependencies, (name, dev-only).
    deps: BTreeSet<(String, bool)>,
}

fn crates_of(map: &LayerMap, g: &Graph) -> Vec<Krate> {
    let name = |id: &str| g.names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut out: Vec<Krate> = g
        .members
        .iter()
        .map(|id| {
            let n = name(id);
            Krate {
                layer: map.crates.get(&n).cloned(),
                dir: g.manifest_paths.get(id).and_then(|m| Path::new(m).parent().map(Path::to_path_buf)),
                deps: g.edges.get(id).into_iter().flatten().map(|e| (name(&e.to), e.dev_only)).collect(),
                name: n,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn scan(k: &Krate, out: &mut BTreeSet<Finding>) -> Vec<(String, String)> {
    let Some(dir) = &k.dir else {
        out.insert(Finding::input(&k.name, "cargo metadata gives no manifest_path".into()));
        return Vec::new();
    };
    let files = match rust_files(dir) {
        Ok(f) => f,
        Err(e) => {
            out.insert(Finding::input(&k.name, e));
            return Vec::new();
        }
    };
    let mut texts = Vec::new();
    for rel in files {
        match read_text(&dir.join(&rel)) {
            Ok(t) => texts.push((rel, t)),
            Err(e) => {
                out.insert(Finding::input(&k.name, e));
            }
        }
    }
    texts
}

/// DEFN, LEAK and SIZE over the workspace crates.
pub fn check_crates(map: &LayerMap, g: &Graph, cfg: &Config) -> Vec<Finding> {
    let mut out = BTreeSet::new();
    let crates = crates_of(map, g);
    let app_crates: BTreeSet<&str> =
        crates.iter().filter(|k| k.layer.as_deref() == Some(APP_LAYER)).map(|k| k.name.as_str()).collect();
    for k in &crates {
        let is_app = app_crates.contains(k.name.as_str());
        if is_app {
            for (dep, _) in &k.deps {
                if cfg.macro_crates.contains(dep) {
                    let msg = format!("depends on declaration-macro crate {dep}");
                    out.insert(Finding::new(Code::Defn, &k.name, "Cargo.toml", 0, msg));
                }
            }
            for (rel, text) in scan(k, &mut out) {
                let code = code_only(&text);
                for m in &cfg.declaration_markers {
                    for line in token_lines(&code, m) {
                        out.insert(Finding::new(Code::Defn, &k.name, &rel, line, format!("declaration marker {m}")));
                    }
                }
                if rel != "src/main.rs" {
                    if !cfg.excepted(&k.name, &rel) {
                        let msg = "Rust source file other than src/main.rs".to_string();
                        out.insert(Finding::new(Code::Size, &k.name, &rel, 0, msg));
                    }
                } else {
                    let lines = text.lines().count();
                    if lines > MAIN_RS_MAX_LINES && !cfg.excepted(&k.name, &rel) {
                        let msg = format!("{lines} lines, limit {MAIN_RS_MAX_LINES}");
                        out.insert(Finding::new(Code::Size, &k.name, &rel, 0, msg));
                    }
                }
            }
            continue;
        }
        if k.layer.as_deref().is_some_and(|l| LEAK_EXEMPT.contains(&l)) {
            continue;
        }
        for (dep, dev_only) in &k.deps {
            if !dev_only && app_crates.contains(dep.as_str()) {
                let msg = format!("depends on app crate {dep}");
                out.insert(Finding::new(Code::Leak, &k.name, "Cargo.toml", 0, msg));
            }
        }
        if cfg.identity_tokens.is_empty() {
            continue;
        }
        for (rel, text) in scan(k, &mut out) {
            if !is_non_test_source(&rel) {
                continue;
            }
            let code = without_cfg_test(&code_only(&text));
            for t in &cfg.identity_tokens {
                for line in token_lines(&code, t) {
                    out.insert(Finding::new(Code::Leak, &k.name, &rel, line, format!("uses app-identity token {t}")));
                }
            }
        }
    }
    out.into_iter().collect()
}

// ---------------------------------------------------------------------------------------
// IDENT

fn obj<'a>(v: &'a CValue, what: &str) -> Result<&'a BTreeMap<String, CValue>, String> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(format!("{what} must be an object")),
    }
}

fn arr<'a>(o: &'a BTreeMap<String, CValue>, key: &str, what: &str) -> Result<&'a Vec<CValue>, String> {
    match o.get(key) {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(format!("{what}: {key} must be an array")),
    }
}

fn string<'a>(o: &'a BTreeMap<String, CValue>, key: &str, what: &str) -> Result<&'a str, String> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(format!("{what}: {key} must be a string")),
    }
}

/// One command entry: its ID and, when present, its digest. The side-effect class, when
/// present, must be one of CONSTITUTION §3.
fn command(v: &CValue, what: &str, digest_required: bool) -> Result<(CommandId, Option<Digest>), String> {
    let o = obj(v, what)?;
    let id_text = string(o, "id", what)?;
    let id = CommandId::parse(id_text).map_err(|e| format!("{what}: bad command ID {id_text:?}: {e}"))?;
    let digest = match o.get("definition_digest") {
        Some(CValue::Str(s)) => {
            Some(Digest::parse(s).map_err(|e| format!("{what}: {id}: bad definition_digest {s:?}: {e}"))?)
        }
        None if !digest_required => None,
        _ => return Err(format!("{what}: {id}: definition_digest must be a sha256: string")),
    };
    match o.get("side_effect") {
        None => {}
        Some(CValue::Str(s)) => {
            SideEffect::parse(s).map_err(|e| format!("{what}: {id}: bad side_effect {s:?}: {e}"))?;
        }
        Some(_) => return Err(format!("{what}: {id}: side_effect must be a string")),
    }
    Ok((id, digest))
}

fn parse_json(text: &str) -> Result<CValue, String> {
    parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, 64).map_err(|e| format!("not valid JSON: {e}"))
}

/// Per-app effective command IDs from `apps-status.json`; apps with `manifest: false` are
/// left out.
pub fn parse_apps_status(text: &str) -> Result<BTreeMap<String, BTreeSet<CommandId>>, String> {
    let root = parse_json(text)?;
    let root = obj(&root, "apps-status.json")?;
    let mut out = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for app in arr(root, "apps", "apps-status.json")? {
        let o = obj(app, "apps-status.json app")?;
        let id = string(o, "id", "apps-status.json app")?;
        let what = format!("apps-status.json app {id}");
        let has_manifest = match o.get("manifest") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(format!("{what}: manifest must be a boolean")),
        };
        let mut cmds = BTreeSet::new();
        if has_manifest {
            for c in arr(o, "commands", &what)? {
                cmds.insert(command(c, &what, false)?.0);
            }
        }
        if !seen.insert(id.to_string()) {
            return Err(format!("{what}: listed twice"));
        }
        if has_manifest {
            out.insert(id.to_string(), cmds);
        }
    }
    Ok(out)
}

/// Command ID to definition digest from one app's registry manifest.
pub fn parse_registry_manifest(text: &str, what: &str) -> Result<BTreeMap<CommandId, Digest>, String> {
    let root = parse_json(text)?;
    let root = obj(&root, what)?;
    let mut out: BTreeMap<CommandId, Digest> = BTreeMap::new();
    for c in arr(root, "commands", what)? {
        let (id, digest) = command(c, what, true)?;
        let Some(digest) = digest else {
            return Err(format!("{what}: {id}: missing definition_digest"));
        };
        if let Some(prev) = out.insert(id.clone(), digest) {
            if prev != digest {
                return Err(format!("{what}: {id} listed twice with different digests"));
            }
        }
    }
    Ok(out)
}

/// IDENT over effective commands. `manifests` maps app ID to its registry manifest.
pub fn check_ident(
    effective: &BTreeMap<String, BTreeSet<CommandId>>,
    manifests: &BTreeMap<String, BTreeMap<CommandId, Digest>>,
) -> Vec<Finding> {
    let mut out = BTreeSet::new();
    let mut by_cmd: BTreeMap<&CommandId, BTreeMap<&str, Digest>> = BTreeMap::new();
    for (app, cmds) in effective {
        let Some(manifest) = manifests.get(app) else {
            out.insert(Finding::input(app, format!("no registry manifest {app}.json for an app with a manifest")));
            continue;
        };
        for c in cmds {
            match manifest.get(c) {
                Some(d) => {
                    by_cmd.entry(c).or_default().insert(app.as_str(), *d);
                }
                None => {
                    let msg = format!("effective command {c} is not in registry manifest {app}.json");
                    out.insert(Finding::input(app, msg));
                }
            }
        }
    }
    for (cmd, apps) in by_cmd {
        let distinct: BTreeSet<&Digest> = apps.values().collect();
        if apps.len() >= 2 && distinct.len() > 1 {
            let list: Vec<String> = apps.iter().map(|(a, d)| format!("{a}={d}")).collect();
            let msg = format!("definition_digest differs between apps: {}", list.join(" "));
            out.insert(Finding::new(Code::Ident, cmd.as_str(), "", 0, msg));
        }
    }
    out.into_iter().collect()
}

/// App IDs listed in `apps.toml` (`[[app]] id = ...`).
pub fn parse_apps_toml(text: &str) -> Result<BTreeSet<String>, String> {
    let table: Table = text.parse().map_err(|e: toml::de::Error| e.to_string().trim_end().replace('\n', " "))?;
    let mut ids = BTreeSet::new();
    match table.get("app") {
        None => {}
        Some(Value::Array(apps)) => {
            for a in apps {
                match a.get("id") {
                    Some(Value::String(id)) => {
                        ids.insert(id.clone());
                    }
                    _ => return Err("every [[app]] needs a string id".into()),
                }
            }
        }
        Some(_) => return Err("app must be an array of tables ([[app]])".into()),
    }
    Ok(ids)
}

fn load_manifests(dir: &Path, out: &mut BTreeSet<Finding>) -> BTreeMap<String, BTreeMap<CommandId, Digest>> {
    let mut manifests = BTreeMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            out.insert(Finding::input("registry-manifests", format!("cannot list {}: {e}", dir.display())));
            return manifests;
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => names.push(e.file_name().to_string_lossy().into_owned()),
            Err(e) => {
                out.insert(Finding::input("registry-manifests", format!("cannot list {}: {e}", dir.display())));
            }
        }
    }
    names.sort();
    if names.len() > MAX_MANIFESTS {
        out.insert(Finding::input("registry-manifests", format!("more than {MAX_MANIFESTS} files")));
        return manifests;
    }
    for name in names {
        let Some(app) = name.strip_suffix(".json") else { continue };
        let parsed = read_text(&dir.join(&name)).and_then(|t| parse_registry_manifest(&t, &name));
        match parsed {
            Ok(m) => {
                manifests.insert(app.to_string(), m);
            }
            Err(e) => {
                out.insert(Finding::input(app, e));
            }
        }
    }
    manifests
}

// ---------------------------------------------------------------------------------------
// Entry point

/// The command-line inputs.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// Workspace directory: `Cargo.toml`, `layers.toml` and optionally `apps.toml` and
    /// `apps-policy.toml`.
    pub workspace: PathBuf,
    /// Directory with one `<app>.json` registry manifest per app.
    pub registry_manifests: PathBuf,
    /// The `apps-status.json` file.
    pub apps_status: PathBuf,
    /// Config file; default `<workspace>/apps-policy.toml`, or built-in defaults when absent.
    pub config: Option<PathBuf>,
}

fn load_config(inp: &Inputs) -> Result<Config, String> {
    let path = match &inp.config {
        Some(p) => p.clone(),
        None => {
            let p = inp.workspace.join(CONFIG_FILE);
            if !p.exists() {
                return Ok(Config::default());
            }
            p
        }
    };
    let text = read_text(&path)?;
    Config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs every check. Returns the findings, sorted by code, subject and location.
pub fn run(inp: &Inputs) -> Vec<Finding> {
    let mut out: BTreeSet<Finding> = BTreeSet::new();
    let cfg = match load_config(inp) {
        Ok(c) => c,
        Err(e) => return vec![Finding::input(CONFIG_FILE, e)],
    };
    let layers = read_text(&inp.workspace.join("layers.toml")).and_then(|t| parse_layer_map(&t));
    match layers {
        Ok(map) => match Graph::load_with(&inp.workspace, true) {
            Ok(g) => out.extend(check_crates(&map, &g, &cfg)),
            Err(e) => {
                out.insert(Finding::input("workspace", e));
            }
        },
        Err(e) => {
            out.insert(Finding::input("layers.toml", e));
        }
    }
    let apps_toml = inp.workspace.join("apps.toml");
    let known_apps = if apps_toml.exists() {
        match read_text(&apps_toml).and_then(|t| parse_apps_toml(&t)) {
            Ok(ids) => Some(ids),
            Err(e) => {
                out.insert(Finding::input("apps.toml", e));
                None
            }
        }
    } else {
        None
    };
    let effective = match read_text(&inp.apps_status).and_then(|t| parse_apps_status(&t)) {
        Ok(e) => e,
        Err(e) => {
            out.insert(Finding::input("apps-status.json", e));
            return out.into_iter().collect();
        }
    };
    let manifests = load_manifests(&inp.registry_manifests, &mut out);
    if let Some(known) = &known_apps {
        let named: BTreeSet<&String> = effective.keys().chain(manifests.keys()).collect();
        for app in named {
            if !known.contains(app) {
                out.insert(Finding::input(app, "app is not listed in apps.toml".into()));
            }
        }
    }
    out.extend(check_ident(&effective, &manifests));
    out.into_iter().collect()
}
