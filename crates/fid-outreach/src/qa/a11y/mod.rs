//! `fid-outreach qa a11y`: static accessibility gate for built pages (spec node
//! `OUTR.SITE.QA.A11Y`).
//!
//! Every `.html`/`.htm` page and `.css` stylesheet under the site root is read and checked
//! against rules A01 to A13, each mapped to a WCAG 2.2 success criterion. Contrast (A12) is
//! computed per theme of the token input; colours are resolved only through token custom
//! properties, and any literal colour is rule A13. The rule set is the automatable subset of
//! WCAG 2.2 AA for static pages, not a conformance claim.
//!
//! Supported `--tokens <path>` inputs:
//! 1. Design-system package / UX token directory (`<dir>/tokens/**/*.tokens.json` in DTCG
//!    format via `fid-design-pkg`, `<dir>/tokens/themes/*.json` in `fid-design-theme@1` or
//!    default `"light"` from DTCG `$value` when `tokens/themes/` is absent, and optional
//!    `deprecations.json`). Exposes `sem.*` and `comp.*` colour tokens (`--sem-*`, `--comp-*`)
//!    while skipping `prim.*` and non-colour tokens (`dimension`, `duration`, `typography`, ...).
//! 2. Single DTCG `.tokens.json` file (no `"themes"` wrapper, loaded as `"light"`).
//! 3. Multi-theme JSON file (`{"themes": {"<theme>": ...}}`), supporting both nested DTCG
//!    groups per theme and legacy flat `"name": "#rrggbb"` / `{"$value": "#rrggbb"}` entries.
//!
//! Configuration: `{"suppressions": [{"page": "a.html", "rule": "A04", "reason": "..."}]}`.

pub mod css;
pub mod html;
pub mod rules;
pub mod vocab;

use css::{Rgba, Scope, StyleRule, WHITE};
use fid_canon::{parse as parse_json, parse_with_limits, to_canonical_bytes, CValue};
use fid_design_pkg::tokens::{collect as collect_dtcg, parse_alias, Token};
use fid_design_pkg::{
    is_valid_name, MAX_ALIAS_DEPTH, MAX_DIR_DEPTH as PKG_MAX_DIR_DEPTH, MAX_FILES as PKG_MAX_FILES,
    MAX_FILE_BYTES as PKG_MAX_FILE_BYTES, MAX_JSON_DEPTH, MAX_RETAINED_BYTES,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Largest page, stylesheet, token or configuration file accepted, in bytes.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Most pages and stylesheets accepted under one root.
pub const MAX_FILES: usize = 100_000;
const MAX_DIR_DEPTH: usize = 64;

/// Report schema identifier.
pub const SCHEMA: &str = "a11y-report@1";

/// Rule IDs, WCAG 2.2 success criteria and short titles.
pub const RULES: [(&str, &str, &str); 13] = [
    ("A01", "1.1.1", "image without text alternative"),
    ("A02", "1.3.1", "heading level skipped or not exactly one h1"),
    ("A03", "1.3.1, 4.1.2", "form control without a programmatic label"),
    ("A04", "1.3.1", "data table without header cells"),
    ("A05", "2.4.2", "missing, empty or shared page title"),
    ("A06", "2.4.4, 4.1.2", "link or button without an accessible name"),
    ("A07", "3.1.1", "missing or invalid lang on html"),
    ("A08", "4.1.2", "duplicate id or ARIA attribute or role not in WAI-ARIA 1.2"),
    ("A09", "2.4.1", "no main landmark or no skip link to it"),
    ("A10", "1.4.4", "viewport setting that disables zoom"),
    ("A11", "1.4.2", "autoplay media not muted or without controls"),
    ("A12", "1.4.3", "text contrast below the minimum"),
    ("A13", "1.4.3", "literal colour not resolved through a token custom property"),
];

/// WCAG 2.2 success criterion (or criteria) of a rule ID; empty for unknown IDs.
pub fn success_criterion(rule: &str) -> &'static str {
    RULES.iter().find(|(id, _, _)| *id == rule).map_or("", |(_, sc, _)| *sc)
}

/// One violation. The derived order is the report order: page, line, rule, element, detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// Page or stylesheet path relative to the root, with `/` separators.
    pub page: String,
    pub line: u32,
    pub rule: String,
    /// Offending element (HTML) or selector (CSS).
    pub element: String,
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} {} (WCAG {}) {}: {}",
            self.page,
            self.line,
            self.rule,
            success_criterion(&self.rule),
            self.element,
            self.detail
        )
    }
}

/// A configured suppression: page, rule and a non-empty reason.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Suppression {
    pub page: String,
    pub rule: String,
    pub reason: String,
}

/// Unreadable or unparsable input (exit code 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A11yError {
    /// `A11Y-IO`: a file or directory could not be read.
    Io { file: String, message: String },
    /// `A11Y-PARSE(file)`: a page, stylesheet, token or configuration file could not be parsed.
    Parse { file: String, message: String },
}

impl A11yError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            A11yError::Io { .. } => "A11Y-IO",
            A11yError::Parse { .. } => "A11Y-PARSE",
        }
    }

    /// The file the error is about.
    pub fn file(&self) -> &str {
        match self {
            A11yError::Io { file, .. } | A11yError::Parse { file, .. } => file,
        }
    }

    fn message(&self) -> &str {
        match self {
            A11yError::Io { message, .. } | A11yError::Parse { message, .. } => message,
        }
    }
}

impl fmt::Display for A11yError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({}): {}", self.code(), self.file(), self.message())
    }
}

impl std::error::Error for A11yError {}

/// Inputs of one gate run.
#[derive(Debug, Clone)]
pub struct Options {
    pub root: PathBuf,
    pub tokens: PathBuf,
    pub config: Option<PathBuf>,
}

/// Result of a gate run.
#[derive(Debug, Clone)]
pub struct Report {
    pub pages: usize,
    pub stylesheets: usize,
    pub themes: Vec<String>,
    /// Unsuppressed violations, sorted.
    pub violations: Vec<Violation>,
    /// Violations matched by a suppression, sorted.
    pub suppressed: Vec<Violation>,
    /// Suppressions that matched at least one violation, with the match count.
    pub used: Vec<(Suppression, usize)>,
    /// Suppressions that matched nothing.
    pub unmatched: Vec<Suppression>,
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(v: usize) -> CValue {
    CValue::Num(v as f64)
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn violation_json(v: &Violation) -> CValue {
    obj(vec![
        ("page", s(&v.page)),
        ("line", CValue::Num(f64::from(v.line))),
        ("rule", s(&v.rule)),
        ("sc", s(success_criterion(&v.rule))),
        ("element", s(&v.element)),
        ("detail", s(&v.detail)),
    ])
}

fn suppression_json(x: &Suppression, matched: Option<usize>) -> CValue {
    let mut f = vec![("page", s(&x.page)), ("rule", s(&x.rule)), ("reason", s(&x.reason))];
    if let Some(m) = matched {
        f.push(("matched", num(m)));
    }
    obj(f)
}

impl Report {
    /// 0 when there is no unsuppressed violation, else 1.
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.violations.is_empty())
    }

    /// The report as canonical JSON (RFC 8785), byte-identical for identical inputs.
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        let v = obj(vec![
            ("schema", s(SCHEMA)),
            ("result", s(if self.violations.is_empty() { "pass" } else { "fail" })),
            ("pages", num(self.pages)),
            ("stylesheets", num(self.stylesheets)),
            ("themes", CValue::Arr(self.themes.iter().cloned().map(CValue::Str).collect())),
            ("violations", CValue::Arr(self.violations.iter().map(violation_json).collect())),
            ("suppressed", CValue::Arr(self.suppressed.iter().map(violation_json).collect())),
            (
                "suppressions",
                obj(vec![
                    ("used", CValue::Arr(self.used.iter().map(|(x, m)| suppression_json(x, Some(*m))).collect())),
                    ("unmatched", CValue::Arr(self.unmatched.iter().map(|x| suppression_json(x, None)).collect())),
                ]),
            ),
        ]);
        to_canonical_bytes(&v).map_err(|e| e.to_string())
    }
}

/// Report written when the run fails with exit code 2.
pub fn error_json(e: &A11yError) -> Vec<u8> {
    let v = obj(vec![
        ("schema", s(SCHEMA)),
        ("result", s("error")),
        ("error", obj(vec![("code", s(e.code())), ("file", s(e.file())), ("message", s(e.message()))])),
    ]);
    to_canonical_bytes(&v).unwrap_or_default()
}

fn show(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Token colours per theme, keyed by custom property name.
#[derive(Debug, Clone, Default)]
pub struct Tokens {
    pub themes: BTreeMap<String, BTreeMap<String, Rgba>>,
    /// Every custom property name that carries a token colour in some theme.
    pub props: BTreeSet<String>,
}

fn token_tier(name: &str) -> &str {
    name.split('.').next().unwrap_or("")
}

fn effective_type<'a>(tokens: &'a BTreeMap<String, Token>, tok: &'a Token) -> Option<&'a str> {
    let mut cur = tok;
    for _ in 0..=MAX_ALIAS_DEPTH {
        if let Some(ty) = cur.ty.as_deref() {
            return Some(ty);
        }
        cur = tokens.get(cur.alias()?)?;
    }
    None
}

fn is_colour_token(tokens: &BTreeMap<String, Token>, tok: &Token) -> bool {
    match effective_type(tokens, tok) {
        Some(ty) => ty == "color",
        None => matches!(token_tier(&tok.name), "prim" | "sem" | "comp"),
    }
}

fn resolve_dtcg_colour(
    start: &str,
    tokens: &BTreeMap<String, Token>,
    colour_names: &BTreeSet<String>,
    sem_names: &BTreeSet<String>,
    bindings: Option<&BTreeMap<String, String>>,
    deprecations: &BTreeMap<String, String>,
    require_prim_literal: bool,
) -> Result<Rgba, String> {
    let mut visited: Vec<String> = Vec::new();
    let mut cur = start.to_string();
    loop {
        if let Some(pos) = visited.iter().position(|n| n == &cur) {
            let cycle = visited[pos..].join(" -> ");
            return Err(format!("alias cycle: {cycle} -> {cur}"));
        }
        if visited.len() > MAX_ALIAS_DEPTH {
            return Err(format!("{start}: more than {MAX_ALIAS_DEPTH} alias hops"));
        }
        visited.push(cur.clone());

        if let Some(binds) = bindings {
            if sem_names.contains(&cur) {
                let target = binds.get(&cur).ok_or_else(|| format!("no theme binding for {cur}"))?;
                cur = target.clone();
                continue;
            }
        }

        if let Some(tok) = tokens.get(&cur) {
            if visited.len() >= 2 && !colour_names.contains(&cur) {
                return Err(format!("{cur} is not a colour token"));
            }
            if let Some(alias) = tok.alias() {
                cur = alias.to_string();
                continue;
            }
            let CValue::Str(s) = &tok.value else {
                return Err(format!("{cur}: colour value must be a string"));
            };
            if require_prim_literal && token_tier(&cur) != "prim" {
                return Err(format!("{cur}: literal colour outside prim tier (colour tokens must resolve to prim.*)"));
            }
            return css::parse_colour(s).ok_or_else(|| format!("{cur}: invalid colour value {s:?}"));
        }

        if let Some(repl) = deprecations.get(&cur) {
            cur = repl.clone();
            continue;
        }

        return Err(format!("{cur} is not a token"));
    }
}

fn populate_dtcg_theme(
    theme: &str,
    tokens: &BTreeMap<String, Token>,
    bindings: Option<&BTreeMap<String, String>>,
    deprecations: &BTreeMap<String, String>,
    require_prim_literal: bool,
    out: &mut Tokens,
) -> Result<(), String> {
    let mut colour_names = BTreeSet::new();
    let mut sem_names = BTreeSet::new();
    let mut has_tiers = false;
    for (name, tok) in tokens {
        if is_colour_token(tokens, tok) {
            let tier = token_tier(name);
            if matches!(tier, "prim" | "sem" | "comp") {
                has_tiers = true;
            }
            if tier == "sem" {
                sem_names.insert(name.clone());
            }
            colour_names.insert(name.clone());
        }
    }
    if colour_names.is_empty() {
        return Err(format!("theme \"{theme}\" has no colour tokens"));
    }

    let mut m = BTreeMap::new();
    for name in &colour_names {
        let c = resolve_dtcg_colour(
            name,
            tokens,
            &colour_names,
            &sem_names,
            bindings,
            deprecations,
            require_prim_literal && has_tiers,
        )?;
        let expose = if has_tiers { matches!(token_tier(name), "sem" | "comp") } else { true };
        if expose {
            let prop = format!("--{}", name.replace('.', "-"));
            out.props.insert(prop.clone());
            m.insert(prop, c);
        }
    }
    if m.is_empty() {
        return Err(format!("theme \"{theme}\" has no exposed colour tokens"));
    }
    out.themes.insert(theme.to_string(), m);
    Ok(())
}

fn collect_dtcg_map(file: &str, root: &CValue) -> Result<BTreeMap<String, Token>, String> {
    let mut raw = Vec::new();
    let mut findings = Vec::new();
    collect_dtcg(file, root, &mut raw, &mut findings);
    if let Some(first) = findings.first() {
        return Err(first.to_string());
    }
    let mut map = BTreeMap::new();
    for tok in raw {
        if map.contains_key(&tok.name) {
            return Err(format!("{file}: duplicate token name {:?}", tok.name));
        }
        map.insert(tok.name.clone(), tok);
    }
    Ok(map)
}

fn is_legacy_flat_theme(values: &BTreeMap<String, CValue>) -> bool {
    !values.is_empty()
        && values.iter().all(|(k, v)| {
            !k.starts_with('$')
                && match v {
                    CValue::Str(_) => true,
                    CValue::Obj(o) => o.len() == 1 && matches!(o.get("$value"), Some(CValue::Str(_))),
                    _ => false,
                }
        })
}

/// Reads the theme names and colour values of a token JSON file (single DTCG token file,
/// multi-theme DTCG JSON, or legacy flat `{"themes": ...}`).
pub fn load_tokens(bytes: &[u8]) -> Result<Tokens, String> {
    let v =
        parse_with_limits(bytes, MAX_FILE_BYTES as usize, MAX_JSON_DEPTH).map_err(|e| format!("invalid JSON: {e}"))?;
    let CValue::Obj(top) = &v else { return Err("token file is not a JSON object".to_string()) };
    let mut out = Tokens::default();
    let empty_deps = BTreeMap::new();

    let Some(themes_val) = top.get("themes") else {
        let map = collect_dtcg_map("tokens", &v)?;
        populate_dtcg_theme("light", &map, None, &empty_deps, false, &mut out)?;
        return Ok(out);
    };

    let CValue::Obj(themes) = themes_val else {
        return Err("token file \"themes\" field is not an object".to_string());
    };
    if themes.is_empty() {
        return Err("token file defines no theme".to_string());
    }

    for (theme, values) in themes {
        let CValue::Obj(values_obj) = values else { return Err(format!("theme \"{theme}\" is not an object")) };
        if is_legacy_flat_theme(values_obj) {
            let mut m = BTreeMap::new();
            for (name, val) in values_obj {
                let text = match val {
                    CValue::Str(t) => Some(t.as_str()),
                    CValue::Obj(o) => match o.get("$value") {
                        Some(CValue::Str(t)) => Some(t.as_str()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(c) = text.and_then(css::parse_colour) {
                    let prop = format!("--{}", name.replace('.', "-"));
                    out.props.insert(prop.clone());
                    m.insert(prop, c);
                }
            }
            if m.is_empty() {
                return Err(format!(
                    "theme \"{theme}\" has no colour values (only flat \"name\": \"#rrggbb\" or {{\"$value\": ...}} entries are read)"
                ));
            }
            out.themes.insert(theme.clone(), m);
        } else {
            let map = collect_dtcg_map(theme, values)?;
            populate_dtcg_theme(theme, &map, None, &empty_deps, false, &mut out)?;
        }
    }
    Ok(out)
}

fn read_pkg_json(path: &Path, label: &str, retained: &mut usize) -> Result<CValue, A11yError> {
    let io = |e: std::io::Error| A11yError::Io { file: label.to_string(), message: e.to_string() };
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if meta.file_type().is_symlink() {
        return Err(parse_err(label, "symbolic links are not allowed in token packages".to_string()));
    }
    if !meta.is_file() {
        return Err(parse_err(label, "expected a regular file".to_string()));
    }
    if meta.len() > PKG_MAX_FILE_BYTES as u64 {
        return Err(parse_err(label, format!("larger than {PKG_MAX_FILE_BYTES} bytes")));
    }
    let bytes = fs::read(path).map_err(io)?;
    if bytes.len() > PKG_MAX_FILE_BYTES {
        return Err(parse_err(label, format!("larger than {PKG_MAX_FILE_BYTES} bytes")));
    }
    *retained = retained.saturating_add(bytes.len());
    if *retained > MAX_RETAINED_BYTES {
        return Err(parse_err(label, format!("token package exceeds {MAX_RETAINED_BYTES} retained bytes")));
    }
    parse_with_limits(&bytes, PKG_MAX_FILE_BYTES, MAX_JSON_DEPTH).map_err(|e| parse_err(label, e.to_string()))
}

fn collect_token_files(
    dir: &Path,
    rel: &str,
    depth: usize,
    seen_files: &mut usize,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), A11yError> {
    if depth > PKG_MAX_DIR_DEPTH {
        return Err(parse_err(&show(dir), format!("deeper than {PKG_MAX_DIR_DEPTH} directories")));
    }
    let io = |e: std::io::Error| A11yError::Io { file: show(dir), message: e.to_string() };
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        entries.push((entry.file_name(), entry.path()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (os_name, child_path) in entries {
        let name = os_name
            .to_str()
            .ok_or_else(|| parse_err(&show(&child_path), "file name is not valid UTF-8".to_string()))?;
        let child_rel = if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
        *seen_files += 1;
        if *seen_files > PKG_MAX_FILES {
            return Err(parse_err(&show(dir), format!("more than {PKG_MAX_FILES} files in token package")));
        }
        let meta = fs::symlink_metadata(&child_path)
            .map_err(|e| A11yError::Io { file: show(&child_path), message: e.to_string() })?;
        if meta.file_type().is_symlink() {
            return Err(parse_err(&child_rel, "symbolic links are not allowed in token packages".to_string()));
        }
        if meta.is_dir() {
            collect_token_files(&child_path, &child_rel, depth + 1, seen_files, out)?;
        } else if meta.is_file() && name.ends_with(".tokens.json") {
            out.push((child_rel, child_path));
        }
    }
    Ok(())
}

fn parse_theme_file(v: &CValue, rel: &str, expected_theme: &str) -> Result<BTreeMap<String, String>, A11yError> {
    let CValue::Obj(obj) = v else {
        return Err(parse_err(rel, "theme file must be a JSON object".to_string()));
    };
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    if keys != ["bindings", "format", "theme"] {
        return Err(parse_err(rel, "theme file must have exact keys bindings, format, theme".to_string()));
    }
    if obj.get("format") != Some(&CValue::Str("fid-design-theme@1".to_string())) {
        return Err(parse_err(rel, "theme format must be \"fid-design-theme@1\"".to_string()));
    }
    if obj.get("theme") != Some(&CValue::Str(expected_theme.to_string())) {
        return Err(parse_err(rel, format!("theme field must match filename {expected_theme:?}")));
    }
    let Some(CValue::Obj(bindings_obj)) = obj.get("bindings") else {
        return Err(parse_err(rel, "bindings must be a JSON object".to_string()));
    };
    let mut out = BTreeMap::new();
    for (k, val) in bindings_obj {
        let CValue::Str(raw) = val else {
            return Err(parse_err(rel, format!("binding {k:?} must be a \"{{target}}\" alias string")));
        };
        let Some(target) = parse_alias(raw).filter(|t| is_valid_name(t)) else {
            return Err(parse_err(rel, format!("binding {k:?} value {raw:?} is not a valid \"{{name}}\" alias")));
        };
        out.insert(k.clone(), target.to_string());
    }
    Ok(out)
}

fn read_deprecations(root: &Path, retained: &mut usize) -> Result<BTreeMap<String, String>, A11yError> {
    let dep_path = root.join("deprecations.json");
    let dep_label = show(&dep_path);
    match fs::symlink_metadata(&dep_path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(A11yError::Io { file: dep_label, message: e.to_string() }),
        Ok(m) => {
            if m.file_type().is_symlink() {
                return Err(parse_err(&dep_label, "symbolic links are not allowed in token packages".to_string()));
            }
            if !m.is_file() {
                return Err(parse_err(&dep_label, "deprecations.json must be a regular file".to_string()));
            }
        }
    }
    let val = read_pkg_json(&dep_path, &dep_label, retained)?;
    let CValue::Arr(items) = val else {
        return Err(parse_err(&dep_label, "deprecations.json must be a JSON array".to_string()));
    };
    let mut out = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let CValue::Obj(o) = item else {
            return Err(parse_err(&dep_label, format!("deprecation entry {i} is not an object")));
        };
        let Some(CValue::Str(name)) = o.get("name") else {
            return Err(parse_err(&dep_label, format!("deprecation entry {i}: missing string \"name\"")));
        };
        if let Some(CValue::Str(repl)) = o.get("replacement") {
            if !repl.is_empty() {
                out.insert(name.clone(), repl.clone());
            }
        }
    }
    Ok(out)
}

/// Loads tokens from either a JSON file or a design-system UX package directory (`<dir>/tokens/`).
pub fn load_tokens_path(path: &Path) -> Result<Tokens, A11yError> {
    let label = show(path);
    let meta = fs::symlink_metadata(path).map_err(|e| A11yError::Io { file: label.clone(), message: e.to_string() })?;
    if meta.file_type().is_symlink() {
        return Err(parse_err(&label, "symbolic links are not allowed as token input".to_string()));
    }
    if meta.is_file() {
        let bytes = read_bytes(path)?;
        return load_tokens(&bytes).map_err(|message| A11yError::Parse { file: label, message });
    }
    if !meta.is_dir() {
        return Err(parse_err(&label, "token input must be a file or directory".to_string()));
    }

    let tokens_dir = path.join("tokens");
    let tokens_label = show(&tokens_dir);
    let tokens_meta = fs::symlink_metadata(&tokens_dir)
        .map_err(|e| parse_err(&tokens_label, format!("missing tokens directory: {e}")))?;
    if tokens_meta.file_type().is_symlink() || !tokens_meta.is_dir() {
        return Err(parse_err(&tokens_label, "tokens entry must be a real directory".to_string()));
    }

    let mut seen_files = 0usize;
    let mut token_files = Vec::new();
    collect_token_files(&tokens_dir, "tokens", 0, &mut seen_files, &mut token_files)?;
    if token_files.is_empty() {
        return Err(parse_err(&label, "no *.tokens.json files found under tokens/".to_string()));
    }

    let mut retained = 0usize;
    let deprecations = read_deprecations(path, &mut retained)?;

    let mut token_map: BTreeMap<String, Token> = BTreeMap::new();
    for (rel, file_path) in &token_files {
        let val = read_pkg_json(file_path, rel, &mut retained)?;
        let mut collected = Vec::new();
        let mut findings = Vec::new();
        collect_dtcg(rel, &val, &mut collected, &mut findings);
        if let Some(first) = findings.first() {
            return Err(parse_err(rel, first.to_string()));
        }
        for tok in collected {
            if let Some(prev) = token_map.get(&tok.name) {
                return Err(parse_err(
                    rel,
                    format!("duplicate token {:?} (already defined in {})", tok.name, prev.file),
                ));
            }
            token_map.insert(tok.name.clone(), tok);
        }
    }

    let mut out = Tokens::default();
    let themes_dir = tokens_dir.join("themes");
    match fs::symlink_metadata(&themes_dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => {
            populate_dtcg_theme("light", &token_map, None, &deprecations, true, &mut out)
                .map_err(|m| parse_err(&label, m))?;
        }
        Err(e) => return Err(A11yError::Io { file: show(&themes_dir), message: e.to_string() }),
        Ok(tm) => {
            if tm.file_type().is_symlink() || !tm.is_dir() {
                return Err(parse_err(&show(&themes_dir), "tokens/themes must be a real directory".to_string()));
            }
            let io = |e: std::io::Error| A11yError::Io { file: show(&themes_dir), message: e.to_string() };
            let mut theme_entries = Vec::new();
            for entry in fs::read_dir(&themes_dir).map_err(io)? {
                let entry = entry.map_err(io)?;
                theme_entries.push((entry.file_name(), entry.path()));
            }
            theme_entries.sort_by(|a, b| a.0.cmp(&b.0));

            let mut found_themes = 0usize;
            for (os_name, theme_path) in theme_entries {
                let Some(name) = os_name.to_str() else {
                    return Err(parse_err(&show(&theme_path), "theme file name is not valid UTF-8".to_string()));
                };
                let Some(theme_name) = name.strip_suffix(".json") else {
                    continue;
                };
                let rel = format!("tokens/themes/{name}");
                let val = read_pkg_json(&theme_path, &rel, &mut retained)?;
                let bindings = parse_theme_file(&val, &rel, theme_name)?;
                populate_dtcg_theme(theme_name, &token_map, Some(&bindings), &deprecations, true, &mut out)
                    .map_err(|m| parse_err(&rel, m))?;
                found_themes += 1;
            }
            if found_themes == 0 {
                return Err(parse_err(&show(&themes_dir), "tokens/themes contains no *.json theme files".to_string()));
            }
        }
    }

    if out.themes.is_empty() {
        return Err(parse_err(&label, "token package defines no theme".to_string()));
    }
    Ok(out)
}

/// Reads the suppression list of the gate configuration.
pub fn load_config(bytes: &[u8]) -> Result<Vec<Suppression>, String> {
    let v = parse_json(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    let CValue::Obj(top) = v else { return Err("configuration is not a JSON object".to_string()) };
    let list = match top.get("suppressions") {
        None => return Ok(Vec::new()),
        Some(CValue::Arr(a)) => a,
        Some(_) => return Err("\"suppressions\" is not an array".to_string()),
    };
    let mut out = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let CValue::Obj(o) = item else { return Err(format!("suppression {i} is not an object")) };
        let field = |k: &str| match o.get(k) {
            Some(CValue::Str(t)) if !t.trim().is_empty() => Ok(t.trim().to_string()),
            _ => Err(format!("suppression {i}: \"{k}\" must be a non-empty string")),
        };
        let page = field("page")?;
        let rule = field("rule")?;
        let reason = field("reason")?;
        if success_criterion(&rule).is_empty() {
            return Err(format!("suppression {i}: unknown rule \"{rule}\""));
        }
        out.push(Suppression { page, rule, reason });
    }
    Ok(out)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, A11yError> {
    let file = show(path);
    let meta = fs::metadata(path).map_err(|e| A11yError::Io { file: file.clone(), message: e.to_string() })?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(A11yError::Parse { file, message: format!("larger than {MAX_FILE_BYTES} bytes") });
    }
    fs::read(path).map_err(|e| A11yError::Io { file, message: e.to_string() })
}

fn read_text(path: &Path, label: &str) -> Result<String, A11yError> {
    let bytes = read_bytes(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| A11yError::Parse { file: label.to_string(), message: "not valid UTF-8".to_string() })?;
    Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
}

#[derive(Default)]
struct Files {
    html: Vec<String>,
    css: Vec<String>,
}

fn walk(dir: &Path, rel: &str, depth: usize, files: &mut Files) -> Result<(), A11yError> {
    if depth > MAX_DIR_DEPTH {
        return Err(A11yError::Parse { file: show(dir), message: "directory nesting too deep".to_string() });
    }
    let io = |e: std::io::Error| A11yError::Io { file: show(dir), message: e.to_string() };
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let ft = entry.file_type().map_err(io)?;
        entries.push((entry.file_name().to_string_lossy().into_owned(), ft));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, ft) in entries {
        let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            walk(&dir.join(&name), &child, depth + 1, files)?;
        } else if ft.is_file() {
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            match ext.as_str() {
                "html" | "htm" => files.html.push(child),
                "css" => files.css.push(child),
                _ => continue,
            }
            if files.html.len() + files.css.len() > MAX_FILES {
                return Err(A11yError::Parse { file: show(dir), message: format!("more than {MAX_FILES} files") });
            }
        }
    }
    Ok(())
}

fn parse_err(file: &str, message: String) -> A11yError {
    A11yError::Parse { file: file.to_string(), message }
}

fn applies(rule: &StyleRule, theme: &str) -> bool {
    match &rule.scheme {
        None => true,
        Some(s) => s == theme,
    }
}

/// 3 for a `body` selector, 2 for `html`, 1 for `:root`, else 0.
fn root_rank(selector: &str) -> u8 {
    selector
        .split(',')
        .map(|x| match x.trim() {
            "body" => 3,
            "html" => 2,
            ":root" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

type Sheets = Vec<(String, Vec<StyleRule>)>;
type Aliases = BTreeMap<String, BTreeMap<String, String>>;

const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

/// Custom properties declared on `:root`, `html` or `body` per theme (`--a: var(--b)` aliases).
fn aliases_by_theme(tokens: &Tokens, sheets: &Sheets) -> Aliases {
    let mut out = Aliases::new();
    for theme in tokens.themes.keys() {
        let mut aliases: BTreeMap<String, String> = BTreeMap::new();
        for (_, rules) in sheets {
            for r in rules.iter().filter(|r| applies(r, theme) && root_rank(&r.selector) > 0) {
                for d in r.decls.iter().filter(|d| d.name.starts_with("--")) {
                    aliases.insert(d.name.clone(), d.value.clone());
                }
            }
        }
        out.insert(theme.clone(), aliases);
    }
    out
}

/// Last root-level colour by selector rank (`body` over `html` over `:root`).
fn root_colour(sheets: &Sheets, theme: &str, pick: impl Fn(&[css::Decl]) -> Option<Rgba>) -> Option<Rgba> {
    let mut best: Option<(u8, Rgba)> = None;
    for (_, rules) in sheets {
        for r in rules.iter().filter(|r| r.contrast && applies(r, theme)) {
            let rank = root_rank(&r.selector);
            if rank == 0 {
                continue;
            }
            if let Some(c) = pick(&r.decls) {
                let better = match best {
                    None => true,
                    Some((k, _)) => rank >= k,
                };
                if better {
                    best = Some((rank, c));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Rule A12 for one theme. Rules that set only a background are checked against the
/// inherited page text colour (black when no root rule sets one).
fn contrast_theme(
    theme: &str,
    tokens: &BTreeMap<String, Rgba>,
    aliases: &BTreeMap<String, String>,
    sheets: &Sheets,
    out: &mut Vec<Violation>,
) {
    let scope = Scope { tokens, aliases };
    let page = css::over(root_colour(sheets, theme, |d| scope.background(d)).unwrap_or(WHITE), WHITE);
    let root_sets_colour = sheets.iter().any(|(_, rules)| {
        rules
            .iter()
            .any(|r| applies(r, theme) && root_rank(&r.selector) > 0 && r.decls.iter().any(|d| d.name == "color"))
    });
    let page_fg = match root_colour(sheets, theme, |d| scope.foreground(d)) {
        Some(c) => Some(c),
        None if root_sets_colour => None,
        None => Some(BLACK),
    };
    for (path, rules) in sheets {
        for r in rules.iter().filter(|r| r.contrast && applies(r, theme)) {
            let own_bg = scope.background(&r.decls);
            let (fg, inherited) = match scope.foreground(&r.decls) {
                Some(c) => (c, false),
                None => {
                    // An own `color` that does not resolve is reported by A13.
                    if r.decls.iter().any(|d| d.name == "color") || own_bg.is_none() {
                        continue;
                    }
                    match page_fg {
                        Some(c) => (c, true),
                        None => continue,
                    }
                }
            };
            let bg = css::over(own_bg.unwrap_or(page), page);
            let ratio = css::contrast(css::over(fg, bg), bg);
            let large = css::large_text(&r.decls);
            let need = if large { 3.0 } else { 4.5 };
            if ratio < need {
                out.push(Violation {
                    page: path.clone(),
                    line: r.line,
                    rule: "A12".to_string(),
                    element: r.selector.clone(),
                    detail: format!(
                        "theme {theme}: contrast {}:1 below {}:1{}{}",
                        css::round2(ratio),
                        if large { "3" } else { "4.5" },
                        if large { " (large text)" } else { "" },
                        if inherited { " (background with inherited text colour)" } else { "" }
                    ),
                });
            }
        }
    }
}

fn same_colour(a: Rgba, b: Rgba) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() < 1e-9;
    close(a.r, b.r) && close(a.g, b.g) && close(a.b, b.b) && close(a.a, b.a)
}

/// Themes a declaration outside any colour-scheme block must agree with: `light` when the
/// token file has it, else every theme.
fn base_themes(tokens: &Tokens) -> Vec<String> {
    if tokens.themes.contains_key("light") {
        vec!["light".to_string()]
    } else {
        tokens.themes.keys().cloned().collect()
    }
}

struct ColourCtx<'a> {
    tokens: &'a Tokens,
    aliases: &'a Aliases,
}

impl ColourCtx<'_> {
    /// Rule A13 for one declaration: literal colours, token custom properties whose value
    /// differs from the token file, and `var()` references in colour properties that do not
    /// resolve to a token colour.
    fn check(&self, page: &str, element: &str, scheme: Option<&str>, decl: &css::Decl, out: &mut Vec<Violation>) {
        let mut push = |detail: String| {
            out.push(Violation {
                page: page.to_string(),
                line: decl.line,
                rule: "A13".to_string(),
                element: element.to_string(),
                detail,
            });
        };
        if decl.name.starts_with("--") && self.tokens.props.contains(&decl.name) {
            let themes = match scheme {
                Some(t) => vec![t.to_string()],
                None => base_themes(self.tokens),
            };
            let value = css::parse_colour(&decl.value);
            for t in themes {
                let want = self.tokens.themes.get(&t).and_then(|m| m.get(&decl.name)).copied();
                let ok = matches!((value, want), (Some(a), Some(b)) if same_colour(a, b));
                if !ok {
                    push(format!(
                        "token custom property {} = {} does not equal the token file value in theme {t}",
                        decl.name, decl.value
                    ));
                }
            }
            return;
        }
        for lit in css::literal_colours(&decl.name, &decl.value) {
            push(format!("literal colour {lit} in {}: not resolved through a token custom property", decl.name));
        }
        if css::is_colour_property(&decl.name) {
            for v in css::var_refs(&decl.value) {
                for (t, map) in &self.tokens.themes {
                    if scheme.is_some_and(|s| s != t.as_str()) {
                        continue;
                    }
                    let empty = BTreeMap::new();
                    let aliases = self.aliases.get(t).unwrap_or(&empty);
                    if (Scope { tokens: map, aliases }).resolve(&v).is_none() {
                        push(format!("{v} in {} does not resolve to a token colour in theme {t}", decl.name));
                    }
                }
            }
        }
    }
}

/// Runs the gate. Returns the report, or the error that makes the run exit with code 2.
pub fn run(opts: &Options) -> Result<Report, A11yError> {
    let tokens_label = show(&opts.tokens);
    let tokens = load_tokens_path(&opts.tokens)?;
    let suppressions = match &opts.config {
        Some(p) => load_config(&read_bytes(p)?).map_err(|message| A11yError::Parse { file: show(p), message })?,
        None => Vec::new(),
    };
    let mut files = Files::default();
    walk(&opts.root, "", 0, &mut files)?;

    let mut found: Vec<Violation> = Vec::new();
    let mut sheets: Sheets = Vec::new();
    for rel in &files.css {
        let text = read_text(&opts.root.join(rel), rel)?;
        let rules = css::parse_sheet(&text, 1).map_err(|m| parse_err(rel, m))?;
        sheets.push((rel.clone(), rules));
    }
    let mut titles: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    let mut inline: Vec<(String, String, Vec<css::Decl>)> = Vec::new();
    for rel in &files.html {
        let text = read_text(&opts.root.join(rel), rel)?;
        let doc = html::parse(&text).map_err(|m| parse_err(rel, m))?;
        let (findings, facts) = rules::check_page(&doc);
        for f in findings {
            found.push(Violation {
                page: rel.clone(),
                line: f.line,
                rule: f.rule.to_string(),
                element: f.element,
                detail: f.detail,
            });
        }
        if let Some((t, line)) = facts.title {
            titles.entry(t).or_default().push((rel.clone(), line));
        }
        for (block, line) in facts.style_blocks {
            let rules = css::parse_sheet(&block, line).map_err(|m| parse_err(rel, m))?;
            sheets.push((rel.clone(), rules));
        }
        for (style, line, element) in facts.style_attrs {
            inline.push((rel.clone(), element, css::parse_inline(&style, line)));
        }
    }

    // A05: a title shared by two or more pages.
    for (title, pages) in &titles {
        if pages.len() < 2 {
            continue;
        }
        for (page, line) in pages {
            let others: Vec<&str> = pages.iter().filter(|(q, _)| q != page).map(|(q, _)| q.as_str()).collect();
            found.push(Violation {
                page: page.clone(),
                line: *line,
                rule: "A05".to_string(),
                element: "<title>".to_string(),
                detail: format!("title \"{title}\" shared with {}", others.join(", ")),
            });
        }
    }

    // A colour-scheme block needs a theme of that name in the token file (fail closed).
    for (page, rules) in &sheets {
        for r in rules {
            if let Some(t) = r.scheme.as_deref() {
                if !tokens.themes.contains_key(t) {
                    return Err(parse_err(
                        page,
                        format!(
                            "prefers-color-scheme: {t} rule at line {} but token file {tokens_label} has no \"{t}\" theme",
                            r.line
                        ),
                    ));
                }
            }
        }
    }
    let aliases = aliases_by_theme(&tokens, &sheets);
    let cc = ColourCtx { tokens: &tokens, aliases: &aliases };

    // A13: literal or unresolved colours in stylesheets and style attributes.
    for (page, rules) in &sheets {
        for r in rules {
            for d in &r.decls {
                cc.check(page, &r.selector, r.scheme.as_deref(), d, &mut found);
            }
        }
    }
    for (page, element, decls) in &inline {
        for d in decls {
            cc.check(page, element, None, d, &mut found);
        }
    }

    // A12: contrast per theme.
    for (theme, values) in &tokens.themes {
        let empty = BTreeMap::new();
        contrast_theme(theme, values, aliases.get(theme).unwrap_or(&empty), &sheets, &mut found);
    }

    found.sort();
    let mut counts = vec![0usize; suppressions.len()];
    let mut violations = Vec::new();
    let mut suppressed = Vec::new();
    for v in found {
        match suppressions.iter().position(|x| x.page == v.page && x.rule == v.rule) {
            Some(k) => {
                if let Some(c) = counts.get_mut(k) {
                    *c += 1;
                }
                suppressed.push(v);
            }
            None => violations.push(v),
        }
    }
    let mut used = Vec::new();
    let mut unmatched = Vec::new();
    for (x, c) in suppressions.into_iter().zip(counts) {
        if c > 0 {
            used.push((x, c));
        } else {
            unmatched.push(x);
        }
    }
    used.sort();
    unmatched.sort();
    Ok(Report {
        pages: files.html.len(),
        stylesheets: files.css.len(),
        themes: tokens.themes.keys().cloned().collect(),
        violations,
        suppressed,
        used,
        unmatched,
    })
}
