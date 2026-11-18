//! `fid-design-pkg`: read-only reader of a design-system package directory (spec node
//! `UX.SYSTEM.PKG`). Tooling layer. It reads nothing outside the given root and writes nothing.
//!
//! Layout: `package.toml` (manifest, [`FORMAT_ID`]), `CHANGELOG.md` ([`changelog`]),
//! `deprecations.json`, `names.json` (generated name index, [`NAMES_FORMAT_ID`]), token files
//! `tokens/**/*.tokens.json` ([`tokens`]), and the sibling UX nodes' folders (`icons/`, `gdt/`,
//! `components/`, `patterns/`, `screens/`, `report/`, `review/`, `research/`, ...). The package
//! holds only UTF-8 text files (`.toml`, `.json`, `.md`, `.svg`) and `.png` reference renderings.
//!
//! [`Package::load`] reads a root and records structural findings (`PKG-SCHEMA`,
//! `PKG-NAME-*`, `PKG-ALIAS-UNRESOLVED`, `PKG-DEPRECATION-*`); [`Package::resolve`] resolves a
//! token through aliases and deprecations. Determinism D0: every map is a `BTreeMap`, the
//! directory walk is sorted, and no timestamp is read.

pub mod changelog;
pub mod tokens;
pub mod version;

use changelog::Changelog;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use tokens::Token;
pub use version::Version;

/// Manifest format ID (`package.toml` key `format`).
pub const FORMAT_ID: &str = "fid-design-package@1";
/// Name-index format ID (`names.json` key `format`).
pub const NAMES_FORMAT_ID: &str = "fid-design-names@1";
/// The one DTCG draft this reader parses (`package.toml` key `token_format`): the Design
/// Tokens Community Group second editors' draft of the Format Module, 2022-06-14
/// [unverified that later drafts differ only as noted in the PR].
pub const TOKEN_FORMAT_ID: &str = "dtcg-second-editors-draft-2022-06-14";
/// Brand-neutral product placeholder (UX.SYSTEM REQ-011).
pub const PRODUCT_PLACEHOLDER: &str = "{{FAMILY_NAME}}";
/// Largest file read, in bytes.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 32;
/// Most alias and deprecation hops followed when resolving one name.
pub const MAX_ALIAS_DEPTH: usize = 16;
/// Most files in one package.
pub const MAX_FILES: usize = 20_000;
/// Deepest directory nesting below the root.
pub const MAX_DIR_DEPTH: usize = 16;

/// Stable finding codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    Schema,
    NameDuplicate,
    NameSyntax,
    AliasUnresolved,
    BreakingMinor,
    ChangelogMissing,
    ChangelogClass,
    DeprecationEarly,
    DeprecationNorepl,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Schema => "PKG-SCHEMA",
            Code::NameDuplicate => "PKG-NAME-DUPLICATE",
            Code::NameSyntax => "PKG-NAME-SYNTAX",
            Code::AliasUnresolved => "PKG-ALIAS-UNRESOLVED",
            Code::BreakingMinor => "PKG-BREAKING-MINOR",
            Code::ChangelogMissing => "PKG-CHANGELOG-MISSING",
            Code::ChangelogClass => "PKG-CHANGELOG-CLASS",
            Code::DeprecationEarly => "PKG-DEPRECATION-EARLY",
            Code::DeprecationNorepl => "PKG-DEPRECATION-NOREPL",
        }
    }
}

/// One finding: a code, the name or file it is about, and a one-line detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    pub fn new(code: Code, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Finding { code, subject: subject.into(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    /// `<CODE> <subject> - <detail>`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} - {}", self.code.as_str(), self.subject, self.detail)
    }
}

/// The package cannot be read at all (exit 2 in the commands).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Io { path: String, reason: String },
    TooLarge { path: String, limit: usize },
    Limit(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io { path, reason } => write!(f, "cannot read {path}: {reason}"),
            LoadError::TooLarge { path, limit } => write!(f, "{path} is larger than {limit} bytes"),
            LoadError::Limit(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for LoadError {}

/// Kinds of published names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Component,
    Gdt,
    Icon,
    Palette,
    Pattern,
    Screen,
    Token,
}

impl Kind {
    /// All kinds, in `as_str` order.
    pub const ALL: [Kind; 7] =
        [Kind::Component, Kind::Gdt, Kind::Icon, Kind::Palette, Kind::Pattern, Kind::Screen, Kind::Token];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Component => "component",
            Kind::Gdt => "gdt",
            Kind::Icon => "icon",
            Kind::Palette => "palette",
            Kind::Pattern => "pattern",
            Kind::Screen => "screen",
            Kind::Token => "token",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// `^[a-z][a-z0-9]*(\.[a-z0-9-]+)*$`
pub fn is_valid_name(s: &str) -> bool {
    let mut segs = s.split('.');
    let Some(first) = segs.next() else {
        return false;
    };
    let mut fb = first.bytes();
    let head_ok = fb.next().is_some_and(|b| b.is_ascii_lowercase());
    head_ok
        && fb.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && segs.all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}

/// `package.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub format: String,
    pub version: Version,
    pub token_format: String,
    pub product_name: String,
}

/// One `deprecations.json` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deprecation {
    pub name: String,
    /// `None` when missing, null or empty.
    pub replacement: Option<String>,
    pub since: Version,
    /// The earliest release in which the name may be removed.
    pub remove_after: Version,
}

/// A loaded package. Fields that failed to parse are empty or `None`; the reasons are in
/// [`Package::findings`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Package {
    pub manifest: Option<Manifest>,
    /// Tokens by name (the first definition wins when a name is defined twice).
    pub tokens: BTreeMap<String, Token>,
    /// `names.json` entries in file order.
    pub index: Vec<(String, Kind)>,
    pub deprecations: BTreeMap<String, Deprecation>,
    pub changelog: Changelog,
    /// Structural findings, sorted and de-duplicated.
    pub findings: Vec<Finding>,
}

/// A non-fatal note from resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// `name` is deprecated; its replacement was used.
    DeprecatedName { name: String, replacement: String },
}

impl Warning {
    pub fn code(&self) -> &'static str {
        match self {
            Warning::DeprecatedName { .. } => "deprecated_name",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::DeprecatedName { name, replacement } => {
                write!(f, "deprecated_name: {name} is deprecated; use {replacement}")
            }
        }
    }
}

/// The literal a name resolves to.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The token that holds the literal value.
    pub token: String,
    /// The first `$type` met along the chain.
    pub ty: Option<String>,
    pub value: CValue,
    pub warnings: Vec<Warning>,
}

/// Why a name did not resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// A name on the chain is not a token.
    Unknown(String),
    /// The chain returns to a name already visited.
    Cycle(String),
    /// More than [`MAX_ALIAS_DEPTH`] hops.
    DepthLimit(String),
    /// A deprecated name on the chain has no replacement.
    NoReplacement(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Unknown(n) => write!(f, "{n} is not a token"),
            ResolveError::Cycle(n) => write!(f, "alias cycle through {n}"),
            ResolveError::DepthLimit(n) => write!(f, "more than {MAX_ALIAS_DEPTH} alias hops from {n}"),
            ResolveError::NoReplacement(n) => write!(f, "{n} is deprecated without a replacement"),
        }
    }
}

impl std::error::Error for ResolveError {}

impl Package {
    /// Reads the package at `root`. Returns `Err` only when the root or a file cannot be
    /// read or exceeds a limit; every content problem becomes a finding.
    pub fn load(root: &Path) -> Result<Package, LoadError> {
        let files = walk(root)?;
        let mut pkg = Package::default();
        let mut f = Vec::new();

        match files.text.get("package.toml") {
            Some(t) => pkg.manifest = parse_manifest(t, &mut f),
            None => f.push(Finding::new(Code::Schema, "package.toml", "missing")),
        }
        f.extend(files.findings);

        let mut tokens = Vec::new();
        for (path, text) in files.text.iter().filter(|(p, _)| p.starts_with("tokens/") && p.ends_with(".tokens.json")) {
            if let Some(v) = parse_json(path, text, &mut f) {
                tokens::collect(path, &v, &mut tokens, &mut f);
            }
        }
        for t in tokens {
            if !is_valid_name(&t.name) {
                f.push(Finding::new(Code::NameSyntax, &t.name, format!("{}: token name syntax", t.file)));
            }
            if let Some(first) = pkg.tokens.get(&t.name) {
                f.push(Finding::new(
                    Code::NameDuplicate,
                    &t.name,
                    format!("token defined in {} and {}", first.file, t.file),
                ));
            } else {
                pkg.tokens.insert(t.name.clone(), t);
            }
        }

        match files.text.get("names.json") {
            Some(t) => pkg.index = parse_index(t, &mut f),
            None => f.push(Finding::new(Code::Schema, "names.json", "missing")),
        }
        match files.text.get("deprecations.json") {
            Some(t) => pkg.deprecations = parse_deprecations(t, &mut f),
            None => f.push(Finding::new(Code::Schema, "deprecations.json", "missing")),
        }
        match files.text.get("CHANGELOG.md") {
            Some(t) => {
                let (log, cf) = changelog::parse(t);
                pkg.changelog = log;
                f.extend(cf);
            }
            None => f.push(Finding::new(Code::Schema, "CHANGELOG.md", "missing")),
        }

        pkg.check_index(&mut f);
        pkg.check_aliases(&mut f);
        pkg.check_deprecations(&mut f);
        pkg.check_changelog(&mut f);
        f.sort();
        f.dedup();
        pkg.findings = f;
        Ok(pkg)
    }

    /// The package version, when the manifest parsed.
    pub fn version(&self) -> Option<Version> {
        self.manifest.as_ref().map(|m| m.version)
    }

    /// Every published name with its first-listed kind.
    pub fn names(&self) -> BTreeMap<&str, Kind> {
        let mut m = BTreeMap::new();
        for (n, k) in &self.index {
            m.entry(n.as_str()).or_insert(*k);
        }
        m
    }

    /// Number of index entries per kind (every kind present, zero included).
    pub fn counts(&self) -> BTreeMap<Kind, usize> {
        let mut m: BTreeMap<Kind, usize> = Kind::ALL.into_iter().map(|k| (k, 0)).collect();
        for (_, k) in &self.index {
            *m.entry(*k).or_insert(0) += 1;
        }
        m
    }

    /// Resolves `name` to a literal value: a deprecated name redirects to its replacement
    /// (with a [`Warning::DeprecatedName`]), and aliases are followed, cycle-safe and at most
    /// [`MAX_ALIAS_DEPTH`] hops (REQ-UX.SYSTEM.PKG-007).
    pub fn resolve(&self, name: &str) -> Result<Resolved, ResolveError> {
        self.resolve_chain(name, true)
    }

    fn resolve_chain(&self, name: &str, follow_deprecations: bool) -> Result<Resolved, ResolveError> {
        let mut cur = name.to_string();
        let mut seen = BTreeSet::new();
        let mut warnings = Vec::new();
        let mut ty: Option<String> = None;
        for _ in 0..=MAX_ALIAS_DEPTH {
            if !seen.insert(cur.clone()) {
                return Err(ResolveError::Cycle(cur));
            }
            if follow_deprecations {
                if let Some(d) = self.deprecations.get(&cur) {
                    let Some(r) = &d.replacement else {
                        return Err(ResolveError::NoReplacement(cur));
                    };
                    warnings.push(Warning::DeprecatedName { name: cur.clone(), replacement: r.clone() });
                    cur = r.clone();
                    continue;
                }
            }
            let Some(t) = self.tokens.get(&cur) else {
                return Err(ResolveError::Unknown(cur));
            };
            if ty.is_none() {
                ty = t.ty.clone();
            }
            match t.alias() {
                Some(target) => cur = target.to_string(),
                None => return Ok(Resolved { token: cur, ty, value: t.value.clone(), warnings }),
            }
        }
        Err(ResolveError::DepthLimit(name.to_string()))
    }

    fn check_index(&self, f: &mut Vec<Finding>) {
        let mut kinds: BTreeMap<&str, Vec<Kind>> = BTreeMap::new();
        for (n, k) in &self.index {
            kinds.entry(n.as_str()).or_default().push(*k);
        }
        for (n, ks) in &kinds {
            if !is_valid_name(n) {
                f.push(Finding::new(Code::NameSyntax, *n, "names.json: name does not match ^[a-z][a-z0-9]*(\\.[a-z0-9-]+)*$"));
            }
            if ks.len() > 1 {
                let list: Vec<&str> = ks.iter().map(|k| k.as_str()).collect();
                f.push(Finding::new(Code::NameDuplicate, *n, format!("listed {} times: {}", ks.len(), list.join(", "))));
            }
            if ks.contains(&Kind::Token) && !self.tokens.contains_key(*n) {
                f.push(Finding::new(Code::Schema, *n, "names.json lists a token that no token file defines"));
            }
        }
        for n in self.tokens.keys() {
            if !kinds.get(n.as_str()).is_some_and(|ks| ks.contains(&Kind::Token)) {
                f.push(Finding::new(Code::Schema, n, "token is missing from names.json (regenerate the index)"));
            }
        }
    }

    fn check_aliases(&self, f: &mut Vec<Finding>) {
        for t in self.tokens.values() {
            if t.alias().is_some() {
                if let Err(e) = self.resolve_chain(&t.name, false) {
                    f.push(Finding::new(Code::AliasUnresolved, &t.name, e.to_string()));
                }
            }
            for r in &t.nested_refs {
                if !self.tokens.contains_key(r) {
                    f.push(Finding::new(Code::AliasUnresolved, &t.name, format!("{r} is not a token")));
                }
            }
        }
    }

    fn check_deprecations(&self, f: &mut Vec<Finding>) {
        let names = self.names();
        for d in self.deprecations.values() {
            match &d.replacement {
                None => f.push(Finding::new(Code::DeprecationNorepl, &d.name, "deprecation entry has no replacement")),
                Some(r) if r == &d.name => {
                    f.push(Finding::new(Code::DeprecationNorepl, &d.name, "replacement is the deprecated name itself"))
                }
                Some(r) if !names.contains_key(r.as_str()) => f.push(Finding::new(
                    Code::DeprecationNorepl,
                    &d.name,
                    format!("replacement {r} is not a published name"),
                )),
                Some(_) => {}
            }
            if d.remove_after.line() <= d.since.line() {
                f.push(Finding::new(
                    Code::DeprecationEarly,
                    &d.name,
                    format!("remove_after {} is not at least one minor version after since {}", d.remove_after, d.since),
                ));
            }
            if let Some(v) = self.version() {
                if d.since > v {
                    f.push(Finding::new(Code::Schema, &d.name, format!("since {} is later than the package {v}", d.since)));
                }
            }
        }
    }

    fn check_changelog(&self, f: &mut Vec<Finding>) {
        let Some(v) = self.version() else {
            return;
        };
        if !self.changelog.sections.contains_key(&v) {
            f.push(Finding::new(Code::Schema, "CHANGELOG.md", format!("no `## {v}` section for the package version")));
        }
        for s in self.changelog.sections.keys().filter(|s| **s > v) {
            f.push(Finding::new(Code::Schema, "CHANGELOG.md", format!("section {s} is later than the package {v}")));
        }
    }
}

// ------------------------------------------------------------------ parsing helpers

fn parse_json(path: &str, text: &str, f: &mut Vec<Finding>) -> Option<CValue> {
    match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES, MAX_JSON_DEPTH) {
        Ok(v) => Some(v),
        Err(e) => {
            f.push(Finding::new(Code::Schema, path, format!("invalid JSON: {e}")));
            None
        }
    }
}

fn parse_manifest(text: &str, f: &mut Vec<Finding>) -> Option<Manifest> {
    let file = "package.toml";
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            let e: toml::de::Error = e;
            let msg = e.to_string().split_whitespace().collect::<Vec<_>>().join(" ");
            f.push(Finding::new(Code::Schema, file, format!("invalid TOML: {msg}")));
            return None;
        }
    };
    for k in table.keys() {
        if !["format", "version", "token_format", "product_name"].contains(&k.as_str()) {
            f.push(Finding::new(Code::Schema, file, format!("unexpected key {k:?}")));
        }
    }
    let get = |k: &str, f: &mut Vec<Finding>| match table.get(k) {
        Some(toml::Value::String(s)) => Some(s.clone()),
        _ => {
            f.push(Finding::new(Code::Schema, file, format!("{k} must be a string")));
            None
        }
    };
    let format = get("format", f);
    let version = get("version", f);
    let token_format = get("token_format", f);
    let product_name = get("product_name", f);
    let (format, version, token_format, product_name) = (format?, version?, token_format?, product_name?);
    let mut ok = true;
    if format != FORMAT_ID {
        f.push(Finding::new(Code::Schema, file, format!("format must be {FORMAT_ID:?}")));
        ok = false;
    }
    if token_format != TOKEN_FORMAT_ID {
        f.push(Finding::new(Code::Schema, file, format!("unsupported token_format; this reader parses {TOKEN_FORMAT_ID:?}")));
        ok = false;
    }
    if product_name != PRODUCT_PLACEHOLDER {
        f.push(Finding::new(Code::Schema, file, format!("product_name must be the placeholder {PRODUCT_PLACEHOLDER:?}")));
    }
    let Some(version) = Version::parse(&version) else {
        f.push(Finding::new(Code::Schema, file, "version must be MAJOR.MINOR.PATCH"));
        return None;
    };
    ok.then_some(Manifest { format, version, token_format, product_name })
}

fn parse_index(text: &str, f: &mut Vec<Finding>) -> Vec<(String, Kind)> {
    let file = "names.json";
    let mut out = Vec::new();
    let Some(v) = parse_json(file, text, f) else {
        return out;
    };
    let shape = "names.json must be {\"format\": \"fid-design-names@1\", \"names\": [{\"name\", \"kind\"}, ...]}";
    let CValue::Obj(top) = v else {
        f.push(Finding::new(Code::Schema, file, shape));
        return out;
    };
    if top.get("format") != Some(&CValue::Str(NAMES_FORMAT_ID.to_string())) || top.len() != 2 {
        f.push(Finding::new(Code::Schema, file, shape));
    }
    let Some(CValue::Arr(items)) = top.get("names") else {
        f.push(Finding::new(Code::Schema, file, shape));
        return out;
    };
    for (i, item) in items.iter().enumerate() {
        let entry = match item {
            CValue::Obj(m) if m.len() == 2 => match (m.get("name"), m.get("kind")) {
                (Some(CValue::Str(n)), Some(CValue::Str(k))) => Kind::parse(k).map(|k| (n.clone(), k)),
                _ => None,
            },
            _ => None,
        };
        match entry {
            Some(e) => out.push(e),
            None => f.push(Finding::new(
                Code::Schema,
                file,
                format!("entry {i}: needs exactly a string name and a kind in component|gdt|icon|palette|pattern|screen|token"),
            )),
        }
    }
    out
}

fn parse_deprecations(text: &str, f: &mut Vec<Finding>) -> BTreeMap<String, Deprecation> {
    let file = "deprecations.json";
    let mut out = BTreeMap::new();
    let Some(v) = parse_json(file, text, f) else {
        return out;
    };
    let CValue::Arr(items) = v else {
        f.push(Finding::new(Code::Schema, file, "must be an array of {name, replacement, since, remove_after}"));
        return out;
    };
    for (i, item) in items.iter().enumerate() {
        let CValue::Obj(m) = item else {
            f.push(Finding::new(Code::Schema, file, format!("entry {i} is not an object")));
            continue;
        };
        for k in m.keys() {
            if !["name", "replacement", "since", "remove_after"].contains(&k.as_str()) {
                f.push(Finding::new(Code::Schema, file, format!("entry {i}: unexpected key {k:?}")));
            }
        }
        let s = |k: &str| match m.get(k) {
            Some(CValue::Str(x)) => Some(x.clone()),
            _ => None,
        };
        let Some(name) = s("name") else {
            f.push(Finding::new(Code::Schema, file, format!("entry {i}: name must be a string")));
            continue;
        };
        let ver = |k: &str, f: &mut Vec<Finding>| {
            let v = s(k).as_deref().and_then(Version::parse);
            if v.is_none() {
                f.push(Finding::new(Code::Schema, &name, format!("{file}: {k} must be \"MAJOR.MINOR.PATCH\"")));
            }
            v
        };
        let since = ver("since", f);
        let remove_after = ver("remove_after", f);
        let replacement = match m.get("replacement") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(r)) if r.is_empty() => None,
            Some(CValue::Str(r)) => Some(r.clone()),
            Some(_) => {
                f.push(Finding::new(Code::Schema, &name, format!("{file}: replacement must be a string")));
                None
            }
        };
        if !is_valid_name(&name) {
            f.push(Finding::new(Code::NameSyntax, &name, format!("{file}: name syntax")));
        }
        let (Some(since), Some(remove_after)) = (since, remove_after) else {
            continue;
        };
        if out.contains_key(&name) {
            f.push(Finding::new(Code::Schema, &name, format!("{file}: listed twice")));
            continue;
        }
        out.insert(name.clone(), Deprecation { name, replacement, since, remove_after });
    }
    out
}

// ------------------------------------------------------------------ directory walk

struct Files {
    /// Text files the reader parses, by package-relative path with `/` separators.
    text: BTreeMap<String, String>,
    findings: Vec<Finding>,
}

fn io_err(path: &Path, e: std::io::Error) -> LoadError {
    LoadError::Io { path: path.display().to_string(), reason: e.to_string() }
}

/// Reads at most [`MAX_FILE_BYTES`]; a larger file is a [`LoadError::TooLarge`].
fn read_limited(path: &Path) -> Result<Vec<u8>, LoadError> {
    let file = std::fs::File::open(path).map_err(|e| io_err(path, e))?;
    let mut buf = Vec::new();
    let cap = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    file.take(cap).read_to_end(&mut buf).map_err(|e| io_err(path, e))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(LoadError::TooLarge { path: path.display().to_string(), limit: MAX_FILE_BYTES });
    }
    Ok(buf)
}

fn is_parsed(rel: &str) -> bool {
    matches!(rel, "package.toml" | "names.json" | "deprecations.json" | "CHANGELOG.md")
        || (rel.starts_with("tokens/") && rel.ends_with(".tokens.json"))
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Sorted walk of `root`; checks REQ-UX.SYSTEM.PKG-001 (only UTF-8 text files and PNG) and
/// keeps the text of the files the reader parses. Symbolic links are refused, not followed.
fn walk(root: &Path) -> Result<Files, LoadError> {
    let meta = std::fs::metadata(root).map_err(|e| io_err(root, e))?;
    if !meta.is_dir() {
        return Err(LoadError::Io { path: root.display().to_string(), reason: "not a directory".to_string() });
    }
    let mut out = Files { text: BTreeMap::new(), findings: Vec::new() };
    let mut count = 0usize;
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let mut entries = Vec::new();
        for e in std::fs::read_dir(&dir).map_err(|e| io_err(&dir, e))? {
            let e = e.map_err(|e| io_err(&dir, e))?;
            entries.push((e.file_name(), e.path()));
        }
        entries.sort();
        // Reverse so that the stack visits subdirectories in name order.
        for (name, path) in entries.into_iter().rev() {
            let Some(name) = name.to_str().map(str::to_string) else {
                out.findings.push(Finding::new(Code::Schema, &rel, "a file name is not valid UTF-8"));
                continue;
            };
            let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            count += 1;
            if count > MAX_FILES {
                return Err(LoadError::Limit(format!("more than {MAX_FILES} files under {}", root.display())));
            }
            let m = std::fs::symlink_metadata(&path).map_err(|e| io_err(&path, e))?;
            if m.file_type().is_symlink() {
                out.findings.push(Finding::new(Code::Schema, &child, "symbolic links are not allowed in a package"));
            } else if m.is_dir() {
                if depth + 1 > MAX_DIR_DEPTH {
                    return Err(LoadError::Limit(format!("{child}: deeper than {MAX_DIR_DEPTH} directories")));
                }
                stack.push((path, child, depth + 1));
            } else {
                check_file(&path, &child, &mut out)?;
            }
        }
    }
    Ok(out)
}

fn check_file(path: &Path, rel: &str, out: &mut Files) -> Result<(), LoadError> {
    let ext = rel.rsplit_once('.').map(|(_, e)| e).filter(|_| !rel.rsplit('/').next().unwrap_or(rel).starts_with('.'));
    match ext {
        Some("toml" | "json" | "md" | "svg") => {
            let bytes = read_limited(path)?;
            match String::from_utf8(bytes) {
                Ok(text) if is_parsed(rel) => {
                    out.text.insert(rel.to_string(), text);
                }
                Ok(_) => {}
                Err(_) => out.findings.push(Finding::new(Code::Schema, rel, "not valid UTF-8")),
            }
        }
        Some("png") => {
            let bytes = read_limited(path)?;
            if !bytes.starts_with(&PNG_SIGNATURE) {
                out.findings.push(Finding::new(Code::Schema, rel, "not a PNG file"));
            }
        }
        _ => out.findings.push(Finding::new(
            Code::Schema,
            rel,
            "only .toml, .json, .md, .svg and .png files are allowed in a package",
        )),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_syntax() {
        for ok in ["a", "status.ok", "prim.color.blue.500", "a1.b-c.0", "sem.text.muted"] {
            assert!(is_valid_name(ok), "{ok}");
        }
        for bad in ["", "A", "1a", "a-b", "a..b", "a.", ".a", "a.B", "a_b", "a.b c", "é"] {
            assert!(!is_valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(Code::DeprecationNorepl.as_str(), "PKG-DEPRECATION-NOREPL");
        assert_eq!(Finding::new(Code::NameDuplicate, "status.ok", "x").to_string(), "PKG-NAME-DUPLICATE status.ok - x");
    }
}
