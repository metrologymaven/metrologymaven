//! Kernel dependency closure, allowlist and licence check. Spec node `KRN.NUM.LINT.GRAPH`.
//!
//! [`load_config`] reads the `[graph]` table of `knp1-lint.toml`; [`compute_closure`] resolves,
//! offline, the closure of the kernel roots over normal, build and proc-macro edges for every
//! target from `Cargo.lock` and the package manifests (only edges declared solely as
//! dev-dependencies are dropped; an edge the manifest does not explain is kept, fail closed),
//! and lists each crate's `.rs` files; [`check`] tests every third-party crate against
//! `kernel-allowlist.toml` and `spdx-allowlist.toml` and returns sorted [`Finding`]s.
//! Any package without local source fails the whole run with `KNP1-GRAPH-UNRESOLVED`
//! before a report exists. Output is sorted and free of host paths (D0).
//!
//! Config (`[graph]`; every path relative to the config file's directory):
//!
//! ```toml
//! [graph]
//! workspace = "."                       # directory holding Cargo.toml and Cargo.lock
//! roots = ["fid-num"]                   # kernel root crates (workspace members)
//! third_party_dir = "vendor"            # vendored directory or registry cache (optional)
//! third_party_kind = "vendored"         # or "registry"
//! exclude = ["tests/**", "benches/**", "examples/**", "src/bin/**"]
//! allowlist = "kernel-allowlist.toml"   # default
//! spdx_allowlist = "house-rules/spdx-allowlist.toml"  # default
//! ```

pub mod glob;
pub mod semver;
pub mod spdx;

use crate::findings::{input_digest, Finding, Rule};
use fid_canon::{to_canonical_bytes, CValue};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

/// Default allowlist path, relative to the config file.
pub const DEFAULT_ALLOWLIST: &str = "kernel-allowlist.toml";
/// Default SPDX allowlist path, relative to the config file.
pub const DEFAULT_SPDX_ALLOWLIST: &str = "house-rules/spdx-allowlist.toml";
/// Largest config, lockfile, manifest or allowlist read, in bytes.
pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Deepest directory walked below a crate root.
pub const MAX_WALK_DEPTH: usize = 48;
/// Most `.rs` files listed for one crate.
pub const MAX_FILES_PER_CRATE: usize = 100_000;

// ------------------------------------------------------------------ errors

/// Fatal error codes of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphErrorCode {
    /// A package in the closure has no local source, or the lockfile or a manifest cannot be resolved.
    Unresolved,
    /// An exclusion glob reaches outside tests, benches, examples and `src/bin/`.
    ConfigExclude,
    /// The config, allowlist or SPDX allowlist is malformed.
    ConfigInvalid,
}

impl GraphErrorCode {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            GraphErrorCode::Unresolved => "KNP1-GRAPH-UNRESOLVED",
            GraphErrorCode::ConfigExclude => "KNP1-CONFIG-EXCLUDE",
            GraphErrorCode::ConfigInvalid => "KNP1-CONFIG-INVALID",
        }
    }
}

/// A fatal error: the run produces no closure and no findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphError {
    pub code: GraphErrorCode,
    pub message: String,
}

impl GraphError {
    fn new(code: GraphErrorCode, message: impl Into<String>) -> GraphError {
        GraphError { code, message: message.into() }
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for GraphError {}

fn unresolved(m: impl Into<String>) -> GraphError {
    GraphError::new(GraphErrorCode::Unresolved, m)
}

fn invalid(m: impl Into<String>) -> GraphError {
    GraphError::new(GraphErrorCode::ConfigInvalid, m)
}

// ------------------------------------------------------------------ paths and files

/// Lexical normalisation: drops `.`, folds `..` into its parent. No file-system access.
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

/// `p` relative to `base` with `/` separators, if `p` is inside `base`.
fn rel_slash(p: &Path, base: &Path) -> Option<String> {
    let r = p.strip_prefix(base).ok()?;
    let parts: Option<Vec<&str>> = r.components().map(|c| c.as_os_str().to_str()).collect();
    let parts = parts?;
    Some(if parts.is_empty() { ".".to_string() } else { parts.join("/") })
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = Vec::new();
    f.take(MAX_INPUT_BYTES + 1).read_to_end(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    if buf.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!("{}: larger than {MAX_INPUT_BYTES} bytes", path.display()));
    }
    Ok(buf)
}

fn read_toml(path: &Path) -> Result<toml::Table, String> {
    let bytes = read_bounded(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| format!("{}: not UTF-8", path.display()))?;
    text.parse::<toml::Table>().map_err(|e| format!("{}: {}", path.display(), e.to_string().trim_end()))
}

fn str_list(v: &toml::Value, what: &str) -> Result<Vec<String>, String> {
    let toml::Value::Array(a) = v else {
        return Err(format!("`{what}` must be an array of strings"));
    };
    a.iter()
        .map(|x| match x {
            toml::Value::String(s) => Ok(s.clone()),
            _ => Err(format!("`{what}` must be an array of strings")),
        })
        .collect()
}

// ------------------------------------------------------------------ config

/// The `[graph]` table of `knp1-lint.toml`, with paths resolved against the config directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub workspace: PathBuf,
    /// Kernel root crate names, sorted and de-duplicated.
    pub roots: Vec<String>,
    pub third_party_dir: Option<PathBuf>,
    /// [`SourceKind::Vendored`] or [`SourceKind::Registry`].
    pub third_party_kind: SourceKind,
    pub exclude: Vec<String>,
    pub allowlist: PathBuf,
    pub spdx_allowlist: PathBuf,
}

const CONFIG_KEYS: [&str; 7] =
    ["workspace", "roots", "third_party_dir", "third_party_kind", "exclude", "allowlist", "spdx_allowlist"];

/// Parses the config text. Other top-level tables belong to other lint modules and are ignored;
/// unknown keys inside `[graph]` are rejected.
pub fn parse_config(text: &str, config_dir: &Path) -> Result<Config, GraphError> {
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| invalid(format!("knp1-lint.toml: {}", e.to_string().trim_end())))?;
    let Some(toml::Value::Table(g)) = table.get("graph") else {
        return Err(invalid("knp1-lint.toml: missing [graph] table"));
    };
    for k in g.keys() {
        if !CONFIG_KEYS.contains(&k.as_str()) {
            return Err(invalid(format!("knp1-lint.toml: unknown key graph.{k}")));
        }
    }
    let path_of = |k: &str, default: Option<&str>| -> Result<Option<PathBuf>, GraphError> {
        match (g.get(k), default) {
            (Some(toml::Value::String(s)), _) if !s.is_empty() => Ok(Some(norm(&config_dir.join(s)))),
            (Some(_), _) => Err(invalid(format!("knp1-lint.toml: graph.{k} must be a non-empty string"))),
            (None, Some(d)) => Ok(Some(norm(&config_dir.join(d)))),
            (None, None) => Ok(None),
        }
    };
    let workspace = path_of("workspace", Some("."))?.unwrap_or_default();
    let roots_v = g.get("roots").ok_or_else(|| invalid("knp1-lint.toml: missing graph.roots"))?;
    let roots: BTreeSet<String> = str_list(roots_v, "graph.roots").map_err(invalid)?.into_iter().collect();
    if roots.is_empty() || roots.iter().any(String::is_empty) {
        return Err(invalid("knp1-lint.toml: graph.roots must list at least one crate name"));
    }
    let third_party_kind = match g.get("third_party_kind") {
        None => SourceKind::Vendored,
        Some(toml::Value::String(s)) if s == "vendored" => SourceKind::Vendored,
        Some(toml::Value::String(s)) if s == "registry" => SourceKind::Registry,
        Some(_) => return Err(invalid("knp1-lint.toml: graph.third_party_kind must be \"vendored\" or \"registry\"")),
    };
    let exclude = match g.get("exclude") {
        None => Vec::new(),
        Some(v) => str_list(v, "graph.exclude").map_err(invalid)?,
    };
    let bad: Vec<&String> = exclude.iter().filter(|e| !glob::exclusion_allowed(e)).collect();
    if !bad.is_empty() {
        let list: Vec<String> = bad.iter().map(|b| format!("{b:?}")).collect();
        return Err(GraphError::new(
            GraphErrorCode::ConfigExclude,
            format!(
                "exclusion globs may only cover tests/, benches/, examples/ and src/bin/ paths; rejected {}",
                list.join(", ")
            ),
        ));
    }
    Ok(Config {
        workspace,
        roots: roots.into_iter().collect(),
        third_party_dir: path_of("third_party_dir", None)?,
        third_party_kind,
        exclude,
        allowlist: path_of("allowlist", Some(DEFAULT_ALLOWLIST))?.unwrap_or_default(),
        spdx_allowlist: path_of("spdx_allowlist", Some(DEFAULT_SPDX_ALLOWLIST))?.unwrap_or_default(),
    })
}

/// Reads and parses `knp1-lint.toml` at `path`.
pub fn load_config(path: &Path) -> Result<Config, GraphError> {
    let bytes = read_bounded(path).map_err(invalid)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("knp1-lint.toml: not UTF-8"))?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    parse_config(text, dir)
}

// ------------------------------------------------------------------ allowlists

/// One `[[allow]]` entry of `kernel-allowlist.toml` (ECO.REPO.ALLOW).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowEntry {
    pub name: String,
    pub version: semver::VersionReq,
    pub reason: String,
    pub reviewer: String,
}

const ALLOW_KEYS: [&str; 4] = ["name", "version", "reason", "reviewer"];

/// Parses `kernel-allowlist.toml`: `[[allow]] name, version, reason, reviewer`, all non-empty.
/// An empty file allows nothing.
pub fn parse_allowlist(text: &str) -> Result<Vec<AllowEntry>, GraphError> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| invalid(format!("kernel-allowlist.toml: {}", e.to_string().trim_end())))?;
    let mut out = Vec::new();
    for (k, v) in &table {
        if k != "allow" {
            return Err(invalid(format!("kernel-allowlist.toml: unknown top-level key {k:?}")));
        }
        let toml::Value::Array(items) = v else {
            return Err(invalid("kernel-allowlist.toml: `allow` must be [[allow]] tables"));
        };
        for (i, item) in items.iter().enumerate() {
            let toml::Value::Table(t) = item else {
                return Err(invalid(format!("kernel-allowlist.toml: entry #{} is not a table", i + 1)));
            };
            let field = |f: &str| match t.get(f) {
                Some(toml::Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
                _ => Err(invalid(format!("kernel-allowlist.toml: entry #{} needs a non-empty `{f}`", i + 1))),
            };
            if let Some(k) = t.keys().find(|k| !ALLOW_KEYS.contains(&k.as_str())) {
                return Err(invalid(format!("kernel-allowlist.toml: entry #{}: unknown field {k:?}", i + 1)));
            }
            let req = field("version")?;
            let version = semver::VersionReq::parse(&req).ok_or_else(|| {
                invalid(format!("kernel-allowlist.toml: entry #{}: bad version requirement {req:?}", i + 1))
            })?;
            let (name, reason, reviewer) = (field("name")?, field("reason")?, field("reviewer")?);
            out.push(AllowEntry { name, version, reason, reviewer });
        }
    }
    Ok(out)
}

/// Allowed licence and exception IDs from `spdx-allowlist.toml` (ECO.RULES.SPDX).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpdxAllowlist {
    pub licences: BTreeSet<String>,
    pub exceptions: BTreeSet<String>,
}

/// Parses `spdx-allowlist.toml`; `licences` and `exceptions` are required, the asset keys and
/// `[basis]` belong to other checks and are not read.
pub fn parse_spdx_allowlist(text: &str) -> Result<SpdxAllowlist, GraphError> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| invalid(format!("spdx-allowlist.toml: {}", e.to_string().trim_end())))?;
    let list = |k: &str| -> Result<BTreeSet<String>, GraphError> {
        let v = table.get(k).ok_or_else(|| invalid(format!("spdx-allowlist.toml: missing `{k}`")))?;
        Ok(str_list(v, k).map_err(|e| invalid(format!("spdx-allowlist.toml: {e}")))?.into_iter().collect())
    };
    Ok(SpdxAllowlist { licences: list("licences")?, exceptions: list("exceptions")? })
}

// ------------------------------------------------------------------ lockfile and manifests

#[derive(Debug, Clone)]
struct LockPkg {
    name: String,
    version: String,
    source: Option<String>,
    deps: Vec<String>,
}

fn parse_lock(text: &str) -> Result<Vec<LockPkg>, GraphError> {
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| unresolved(format!("Cargo.lock: {}", e.to_string().trim_end())))?;
    let items = match table.get("package") {
        Some(toml::Value::Array(a)) => a.as_slice(),
        None => &[],
        Some(_) => return Err(unresolved("Cargo.lock: `package` must be [[package]] tables")),
    };
    let mut out: Vec<LockPkg> = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let bad = || unresolved(format!("Cargo.lock: package #{} is malformed", i + 1));
        let toml::Value::Table(t) = item else { return Err(bad()) };
        let s = |k: &str| match t.get(k) {
            Some(toml::Value::String(s)) => Ok(Some(s.clone())),
            None => Ok(None),
            Some(_) => Err(bad()),
        };
        let name = s("name")?.ok_or_else(bad)?;
        let version = s("version")?.ok_or_else(bad)?;
        let source = s("source")?;
        let deps = match t.get("dependencies") {
            Some(v) => str_list(v, "dependencies").map_err(|_| bad())?,
            None => Vec::new(),
        };
        if !seen.insert((name.clone(), version.clone(), source.clone())) {
            return Err(unresolved(format!("Cargo.lock: duplicate package {name} {version}")));
        }
        out.push(LockPkg { name, version, source, deps });
    }
    Ok(out)
}

/// Resolves a lockfile dependency reference (`name`, `name version` or `name version (source)`).
fn resolve_ref(pkgs: &[LockPkg], r: &str, from: &LockPkg) -> Result<usize, GraphError> {
    let (name, rest) = r.split_once(' ').map_or((r, None), |(n, rest)| (n, Some(rest)));
    let (version, source) = match rest {
        None => (None, None),
        Some(rest) => match rest.split_once(' ') {
            None => (Some(rest), None),
            Some((v, s)) => {
                let s = s.strip_prefix('(').and_then(|s| s.strip_suffix(')'));
                (Some(v), Some(s.ok_or_else(|| unresolved(format!("Cargo.lock: bad dependency reference {r:?}")))?))
            }
        },
    };
    let hits: Vec<usize> = pkgs
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.name == name
                && version.is_none_or(|v| p.version == v)
                && source.is_none_or(|s| p.source.as_deref() == Some(s))
        })
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(unresolved(format!(
            "Cargo.lock: {} {} depends on {r:?}, which the lockfile does not contain",
            from.name, from.version
        ))),
        _ => Err(unresolved(format!("Cargo.lock: dependency {r:?} of {} is ambiguous", from.name))),
    }
}

/// How a manifest declares one dependency (keyed by package name, after renames).
#[derive(Debug, Clone, Default)]
struct DepDecl {
    /// Declared as a normal or build dependency in some section.
    normal: bool,
    /// Declared as a dev-dependency in some section.
    dev: bool,
    /// `path =` locations, resolved.
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct Manifest {
    name: String,
    version: String,
    licence: Option<String>,
    deps: BTreeMap<String, DepDecl>,
}

/// The workspace root manifest: members and inheritable values.
#[derive(Debug, Clone, Default)]
struct Workspace {
    dir: PathBuf,
    version: Option<String>,
    licence: Option<String>,
    deps: toml::Table,
    /// Member directories (normalised), each with its package (name, version), read once.
    members: BTreeMap<PathBuf, (String, String)>,
}

fn expand_member(dir: &Path, pattern: &str, out: &mut BTreeSet<PathBuf>) -> Result<(), GraphError> {
    let mut cur = vec![dir.to_path_buf()];
    for seg in pattern.split('/') {
        let mut next = Vec::new();
        for base in &cur {
            if seg.contains('*') || seg.contains('?') {
                let rd = std::fs::read_dir(base).map_err(|e| unresolved(format!("{}: {e}", base.display())))?;
                let mut names: Vec<String> = Vec::new();
                for entry in rd {
                    let entry = entry.map_err(|e| unresolved(format!("{}: {e}", base.display())))?;
                    if let Some(n) = entry.file_name().to_str() {
                        names.push(n.to_string());
                    }
                }
                names.sort();
                next.extend(names.iter().filter(|n| glob::matches(seg, n)).map(|n| base.join(n)));
            } else {
                next.push(base.join(seg));
            }
        }
        cur = next;
    }
    out.extend(cur.into_iter().filter(|p| p.join("Cargo.toml").is_file()).map(|p| norm(&p)));
    Ok(())
}

fn load_workspace(dir: &Path) -> Result<Workspace, GraphError> {
    let t = read_toml(&dir.join("Cargo.toml")).map_err(unresolved)?;
    let mut ws = Workspace { dir: dir.to_path_buf(), ..Workspace::default() };
    let mut member_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    if t.contains_key("package") {
        member_dirs.insert(norm(dir));
    }
    if let Some(toml::Value::Table(w)) = t.get("workspace") {
        let list = |k: &str| match w.get(k) {
            Some(v) => str_list(v, k).map_err(|e| unresolved(format!("workspace Cargo.toml: {e}"))),
            None => Ok(Vec::new()),
        };
        let mut members = BTreeSet::new();
        for m in list("members")? {
            expand_member(dir, &m, &mut members)?;
        }
        let excluded: Vec<PathBuf> = list("exclude")?.iter().map(|e| norm(&dir.join(e))).collect();
        member_dirs.extend(members.into_iter().filter(|m| !excluded.iter().any(|e| m.starts_with(e))));
        if let Some(toml::Value::Table(p)) = w.get("package") {
            ws.version = p.get("version").and_then(toml::Value::as_str).map(str::to_string);
            ws.licence = p.get("license").and_then(toml::Value::as_str).map(str::to_string);
        }
        if let Some(toml::Value::Table(d)) = w.get("dependencies") {
            ws.deps = d.clone();
        }
    }
    for m in member_dirs {
        let path = m.join("Cargo.toml");
        let mt = read_toml(&path).map_err(unresolved)?;
        let id = package_id(&mt, &path, &ws, true)?;
        ws.members.insert(m, id);
    }
    Ok(ws)
}

fn is_workspace_true(v: &toml::Value) -> bool {
    matches!(v, toml::Value::Table(t) if t.get("workspace").and_then(toml::Value::as_bool) == Some(true))
}

/// A string field, or `{ workspace = true }` inherited from the workspace only by a member.
fn inherit(v: Option<&toml::Value>, from_ws: Option<&String>, member: bool) -> Result<Option<String>, &'static str> {
    match v {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(w) if is_workspace_true(w) => match (member, from_ws) {
            (true, Some(s)) => Ok(Some(s.clone())),
            (true, None) => Err("is inherited but the workspace does not set it"),
            (false, _) => Err("is inherited from a workspace the crate is not a member of"),
        },
        Some(_) => Err("is neither a string nor { workspace = true }"),
    }
}

/// Package name and version. A non-member inheriting its version is unresolved (fail closed).
fn package_id(t: &toml::Table, path: &Path, ws: &Workspace, member: bool) -> Result<(String, String), GraphError> {
    let bad = |what: &str| unresolved(format!("{}: {what}", path.display()));
    let Some(toml::Value::Table(p)) = t.get("package") else { return Err(bad("no [package] table")) };
    let name = p.get("name").and_then(toml::Value::as_str).ok_or_else(|| bad("no package name"))?.to_string();
    let version = inherit(p.get("version"), ws.version.as_ref(), member)
        .map_err(|e| bad(&format!("package version {e}")))?
        .unwrap_or_else(|| "0.0.0".to_string());
    Ok((name, version))
}

fn read_manifest(dir: &Path, ws: &Workspace) -> Result<Manifest, GraphError> {
    let path = dir.join("Cargo.toml");
    let t = read_toml(&path).map_err(unresolved)?;
    let member = ws.members.contains_key(&norm(dir));
    let bad = |what: &str| unresolved(format!("{}: {what}", path.display()));
    let (name, version) = package_id(&t, &path, ws, member)?;
    // A non-member's inherited (or malformed) licence counts as missing: fail closed.
    let licence = match t.get("package") {
        Some(toml::Value::Table(p)) => inherit(p.get("license"), ws.licence.as_ref(), member).unwrap_or(None),
        _ => None,
    };
    let mut deps: BTreeMap<String, DepDecl> = BTreeMap::new();
    let mut sections: Vec<&toml::Table> = vec![&t];
    if let Some(toml::Value::Table(targets)) = t.get("target") {
        for v in targets.values() {
            if let toml::Value::Table(tt) = v {
                sections.push(tt);
            }
        }
    }
    for sec in sections {
        for (key, dev) in [
            ("dependencies", false),
            ("build-dependencies", false),
            ("build_dependencies", false),
            ("dev-dependencies", true),
            ("dev_dependencies", true),
        ] {
            let Some(v) = sec.get(key) else { continue };
            let toml::Value::Table(list) = v else { return Err(bad(&format!("`{key}` is not a table"))) };
            for (dep_key, spec) in list {
                let target = dep_target(dep_key, spec, dir, ws, member);
                let (pkg, path) = target.ok_or_else(|| bad(&format!("bad dependency {dep_key:?}")))?;
                let d = deps.entry(pkg).or_default();
                if dev {
                    d.dev = true;
                } else {
                    d.normal = true;
                }
                if let Some(p) = path {
                    d.paths.push(p);
                }
            }
        }
    }
    Ok(Manifest { name, version, licence, deps })
}

/// The package name and optional resolved `path` of one dependency declaration.
/// `{ workspace = true }` is honoured only for workspace members (`None` otherwise: fail closed).
fn dep_target(
    key: &str,
    spec: &toml::Value,
    dir: &Path,
    ws: &Workspace,
    member: bool,
) -> Option<(String, Option<PathBuf>)> {
    match spec {
        toml::Value::String(_) => Some((key.to_string(), None)),
        w if is_workspace_true(w) => {
            if !member {
                return None;
            }
            match ws.deps.get(key) {
                Some(toml::Value::String(_)) => Some((key.to_string(), None)),
                Some(toml::Value::Table(w)) => {
                    let pkg = w.get("package").and_then(toml::Value::as_str).unwrap_or(key);
                    let path = w.get("path").and_then(toml::Value::as_str).map(|p| norm(&ws.dir.join(p)));
                    Some((pkg.to_string(), path))
                }
                _ => None,
            }
        }
        toml::Value::Table(t) => {
            let pkg = t.get("package").and_then(toml::Value::as_str).unwrap_or(key);
            let path = t.get("path").and_then(toml::Value::as_str).map(|p| norm(&dir.join(p)));
            Some((pkg.to_string(), path))
        }
        _ => None,
    }
}

// ------------------------------------------------------------------ closure

/// Where a crate's source comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceKind {
    /// A member of the configured workspace (first-party).
    Workspace,
    /// A vendored or path crate outside the workspace members (third-party).
    Vendored,
    /// A crate found in a registry source cache (third-party).
    Registry,
}

impl SourceKind {
    /// `workspace`, `vendored` or `registry`.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Workspace => "workspace",
            SourceKind::Vendored => "vendored",
            SourceKind::Registry => "registry",
        }
    }

    /// True for crates checked against the allowlists.
    pub fn is_third_party(self) -> bool {
        self != SourceKind::Workspace
    }
}

/// One crate of the kernel closure (REQ-KRN.NUM.LINT.GRAPH-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateEntry {
    pub name: String,
    pub version: String,
    pub source_kind: SourceKind,
    /// Crate root directory on this host (not written to reports).
    pub root: PathBuf,
    /// Host-independent root: relative to the workspace, else `$third_party/<dir>`, else `$external/<name>-<version>`.
    pub root_label: String,
    /// `.rs` files relative to `root`, `/`-separated, sorted by bytes, exclusions removed.
    pub files: Vec<String>,
    /// The manifest's `license` expression, if any.
    pub licence: Option<String>,
    /// Crate names from a kernel root to this crate (first path found by sorted BFS).
    pub path_from_root: Vec<String>,
}

/// The kernel closure, sorted by (name, version).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Closure {
    pub crates: Vec<CrateEntry>,
}

impl Closure {
    /// The closure as a canonical-JSON value (no host paths).
    pub fn to_cvalue(&self) -> CValue {
        let strs = |v: &[String]| CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect());
        let crates = self
            .crates
            .iter()
            .map(|c| {
                let mut o = BTreeMap::new();
                o.insert("name".to_string(), CValue::Str(c.name.clone()));
                o.insert("version".to_string(), CValue::Str(c.version.clone()));
                o.insert("source_kind".to_string(), CValue::Str(c.source_kind.as_str().to_string()));
                o.insert("root".to_string(), CValue::Str(c.root_label.clone()));
                o.insert("files".to_string(), strs(&c.files));
                o.insert("licence".to_string(), c.licence.clone().map_or(CValue::Null, CValue::Str));
                o.insert("path_from_root".to_string(), strs(&c.path_from_root));
                CValue::Obj(o)
            })
            .collect();
        let mut r = BTreeMap::new();
        r.insert("crates".to_string(), CValue::Arr(crates));
        CValue::Obj(r)
    }

    /// RFC 8785 canonical bytes of [`Closure::to_cvalue`].
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, GraphError> {
        to_canonical_bytes(&self.to_cvalue()).map_err(|e| invalid(format!("closure serialisation: {e}")))
    }
}

/// Checks that `dir` holds the manifest of `pkg` (same name and version). Members come from the
/// cache; any other manifest that exists but cannot be read fails the run.
fn manifest_matches(dir: &Path, pkg: &LockPkg, ws: &Workspace) -> Result<bool, GraphError> {
    if let Some((n, v)) = ws.members.get(dir) {
        return Ok(*n == pkg.name && *v == pkg.version);
    }
    if !dir.join("Cargo.toml").is_file() {
        return Ok(false);
    }
    let m = read_manifest(dir, ws)?;
    Ok(m.name == pkg.name && m.version == pkg.version)
}

/// Finds the local source directory of `pkg`; `hints` are `path =` locations from the dependent.
fn locate(pkg: &LockPkg, hints: &[PathBuf], cfg: &Config, ws: &Workspace) -> Result<(PathBuf, SourceKind), GraphError> {
    let kind_of = |dir: &Path| {
        if ws.members.contains_key(dir) {
            SourceKind::Workspace
        } else if cfg.third_party_dir.as_ref().is_some_and(|tp| dir.starts_with(tp)) {
            cfg.third_party_kind
        } else {
            SourceKind::Vendored
        }
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if pkg.source.is_none() {
        candidates.extend(hints.iter().cloned());
        candidates.extend(ws.members.keys().cloned());
    } else if let Some(tp) = &cfg.third_party_dir {
        candidates.push(tp.join(format!("{}-{}", pkg.name, pkg.version)));
        candidates.push(tp.join(&pkg.name));
    }
    for c in candidates {
        let c = norm(&c);
        if manifest_matches(&c, pkg, ws)? {
            let k = kind_of(&c);
            return Ok((c, k));
        }
    }
    let src = pkg.source.as_deref().unwrap_or("path");
    Err(unresolved(format!("{} {} ({src}) has no local source", pkg.name, pkg.version)))
}

fn walk(
    dir: &Path,
    rel: &str,
    depth: usize,
    exclude: &[String],
    skip: &BTreeSet<PathBuf>,
    out: &mut Vec<String>,
) -> Result<(), GraphError> {
    if depth > MAX_WALK_DEPTH {
        return Err(unresolved(format!("{}: deeper than {MAX_WALK_DEPTH} directories", dir.display())));
    }
    let rd = std::fs::read_dir(dir).map_err(|e| unresolved(format!("{}: {e}", dir.display())))?;
    let mut names = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| unresolved(format!("{}: {e}", dir.display())))?;
        let name = entry.file_name().to_str().map(str::to_string);
        let name = name.ok_or_else(|| unresolved(format!("{}: non-UTF-8 file name", dir.display())))?;
        names.push(name);
    }
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if path.is_dir() {
            // Only known package roots (workspace members, closure crates) are someone else's
            // files; every other directory, nested `Cargo.toml` or `target/` included, is scanned.
            if skip.contains(&norm(&path)) {
                continue;
            }
            walk(&path, &r, depth + 1, exclude, skip, out)?;
        } else if name.ends_with(".rs") && path.is_file() && !exclude.iter().any(|g| glob::matches(g, &r)) {
            out.push(r);
            if out.len() > MAX_FILES_PER_CRATE {
                return Err(unresolved(format!("{}: more than {MAX_FILES_PER_CRATE} files", dir.display())));
            }
        }
    }
    Ok(())
}

/// Lists every `.rs` file under `root` (including `build.rs`) not matched by `exclude`, sorted.
/// Subdirectories in `skip` (other known package roots, normalised) are not entered.
pub fn enumerate_files(root: &Path, exclude: &[String], skip: &BTreeSet<PathBuf>) -> Result<Vec<String>, GraphError> {
    let mut out = Vec::new();
    walk(&norm(root), "", 0, exclude, skip, &mut out)?;
    out.sort();
    Ok(out)
}

struct Visit {
    dir: PathBuf,
    kind: SourceKind,
    path: Vec<String>,
}

/// Computes the kernel closure offline from `Cargo.lock` and the manifests (REQ-001, REQ-002).
/// Fails with `KNP1-GRAPH-UNRESOLVED` and no partial result if any package lacks local source.
pub fn compute_closure(cfg: &Config) -> Result<Closure, GraphError> {
    closure_and_lock(cfg).map(|(c, _)| c)
}

/// [`compute_closure`] plus the exact `Cargo.lock` bytes it parsed.
fn closure_and_lock(cfg: &Config) -> Result<(Closure, Vec<u8>), GraphError> {
    let lock_bytes = read_bounded(&cfg.workspace.join("Cargo.lock")).map_err(unresolved)?;
    let lock_text = std::str::from_utf8(&lock_bytes).map_err(|_| unresolved("Cargo.lock: not UTF-8"))?;
    let pkgs = parse_lock(lock_text)?;
    let ws = load_workspace(&cfg.workspace)?;

    let mut visits: BTreeMap<usize, Visit> = BTreeMap::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    for root in &cfg.roots {
        let hits: Vec<usize> =
            pkgs.iter().enumerate().filter(|(_, p)| &p.name == root && p.source.is_none()).map(|(i, _)| i).collect();
        let [i] = hits.as_slice() else {
            return Err(unresolved(format!("kernel root {root} is not exactly one workspace package in Cargo.lock")));
        };
        let pkg = pkgs.get(*i).ok_or_else(|| unresolved("lockfile index"))?;
        let (dir, kind) = locate(pkg, &[], cfg, &ws)?;
        if kind != SourceKind::Workspace {
            return Err(unresolved(format!("kernel root {root} is not a workspace member")));
        }
        visits.insert(*i, Visit { dir, kind, path: vec![root.clone()] });
        queue.push_back(*i);
    }

    let mut manifests: BTreeMap<usize, Manifest> = BTreeMap::new();
    while let Some(i) = queue.pop_front() {
        let pkg = pkgs.get(i).ok_or_else(|| unresolved("lockfile index"))?;
        let (dir, path) = match visits.get(&i) {
            Some(v) => (v.dir.clone(), v.path.clone()),
            None => return Err(unresolved("closure bookkeeping")),
        };
        let m = read_manifest(&dir, &ws)?;
        let mut children: Vec<usize> = Vec::new();
        for r in &pkg.deps {
            let j = resolve_ref(&pkgs, r, pkg)?;
            let child = pkgs.get(j).ok_or_else(|| unresolved("lockfile index"))?;
            // Drop only edges the manifest declares purely as dev-dependencies.
            let keep = m.deps.get(&child.name).is_none_or(|d| d.normal || !d.dev);
            if keep {
                children.push(j);
            }
        }
        children.sort_by_cached_key(|k| pkgs.get(*k).map(|p| (p.name.clone(), p.version.clone(), p.source.clone())));
        children.dedup();
        for j in children {
            if visits.contains_key(&j) {
                continue;
            }
            let child = pkgs.get(j).ok_or_else(|| unresolved("lockfile index"))?;
            let hints = m.deps.get(&child.name).map(|d| d.paths.clone()).unwrap_or_default();
            let (cdir, kind) = locate(child, &hints, cfg, &ws)?;
            let mut cpath = path.clone();
            cpath.push(child.name.clone());
            visits.insert(j, Visit { dir: cdir, kind, path: cpath });
            queue.push_back(j);
        }
        manifests.insert(i, m);
    }

    let mut skip: BTreeSet<PathBuf> = ws.members.keys().cloned().collect();
    skip.extend(visits.values().map(|v| v.dir.clone()));
    let mut crates = Vec::new();
    for (i, v) in &visits {
        let pkg = pkgs.get(*i).ok_or_else(|| unresolved("lockfile index"))?;
        let m = manifests.get(i).ok_or_else(|| unresolved("closure bookkeeping"))?;
        let root_label = rel_slash(&v.dir, &norm(&cfg.workspace))
            .or_else(|| {
                let tp = cfg.third_party_dir.as_ref()?;
                rel_slash(&v.dir, tp).map(|r| format!("$third_party/{r}"))
            })
            .unwrap_or_else(|| format!("$external/{}-{}", pkg.name, pkg.version));
        crates.push(CrateEntry {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            source_kind: v.kind,
            root: v.dir.clone(),
            root_label,
            files: enumerate_files(&v.dir, &cfg.exclude, &skip)?,
            licence: m.licence.clone(),
            path_from_root: v.path.clone(),
        });
    }
    crates.sort_by_cached_key(|c| (c.name.clone(), c.version.clone(), c.root_label.clone()));
    Ok((Closure { crates }, lock_bytes))
}

// ------------------------------------------------------------------ checks

fn finding(c: &CrateEntry, suffix: &str, message: String) -> Finding {
    Finding {
        rule: Rule::Allow(suffix.to_string()),
        crate_name: c.name.clone(),
        crate_version: c.version.clone(),
        file: "Cargo.toml".to_string(),
        line: 1,
        column: 1,
        excerpt_digest: input_digest(message.as_bytes()),
        message,
    }
}

/// Checks every third-party crate of the closure against the kernel allowlist (REQ-003) and its
/// licence against the SPDX allowlist (REQ-004). Workspace crates are first-party and skipped.
/// Findings are `KNP1-ALLOW-UNLISTED`, `KNP1-ALLOW-VERSION` and `KNP1-ALLOW-LICENCE` against the
/// crate's `Cargo.toml` (line 1, column 1; excerpt digest = SHA-256 of the message), sorted.
pub fn check(closure: &Closure, allow: &[AllowEntry], spdx: &SpdxAllowlist) -> Vec<Finding> {
    let mut out = Vec::new();
    for c in closure.crates.iter().filter(|c| c.source_kind.is_third_party()) {
        let via = c.path_from_root.join(" -> ");
        let entries: Vec<&AllowEntry> = allow.iter().filter(|e| e.name == c.name).collect();
        if entries.is_empty() {
            out.push(finding(c, "UNLISTED", format!("{} via {via}", c.name)));
        } else {
            let ver = semver::Version::parse(&c.version);
            if !ver.as_ref().is_some_and(|v| entries.iter().any(|e| e.version.matches(v))) {
                let reqs: Vec<&str> = entries.iter().map(|e| e.version.as_str()).collect();
                out.push(finding(
                    c,
                    "VERSION",
                    format!("{} {} does not satisfy {} via {via}", c.name, c.version, reqs.join(" or ")),
                ));
            }
        }
        let licence_msg = match &c.licence {
            None => Some(format!("{} {} has no licence expression", c.name, c.version)),
            Some(expr) => match spdx::evaluate(expr, &spdx.licences, &spdx.exceptions) {
                Ok(true) => None,
                Ok(false) => Some(format!("{} {} licence `{expr}` is not allowed", c.name, c.version)),
                Err(e) => Some(format!("{} {} licence `{expr}` does not parse: {e}", c.name, c.version)),
            },
        };
        if let Some(m) = licence_msg {
            out.push(finding(c, "LICENCE", m));
        }
    }
    crate::findings::sort_findings(&mut out);
    out
}

/// The outcome of a successful graph run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRun {
    pub config: Config,
    pub closure: Closure,
    /// Sorted allowlist and licence findings.
    pub findings: Vec<Finding>,
    /// `sha256:` digest of `Cargo.lock`, for the report's input digests.
    pub lockfile_digest: String,
}

/// Runs the whole module: config, allowlists, closure, checks. Any fatal error returns no findings.
pub fn run(config_path: &Path) -> Result<GraphRun, GraphError> {
    let config = load_config(config_path)?;
    let text = |p: &Path| -> Result<String, GraphError> {
        let b = read_bounded(p).map_err(invalid)?;
        String::from_utf8(b).map_err(|_| invalid(format!("{}: not UTF-8", p.display())))
    };
    let allow = parse_allowlist(&text(&config.allowlist)?)?;
    let spdx = parse_spdx_allowlist(&text(&config.spdx_allowlist)?)?;
    let (closure, lock) = closure_and_lock(&config)?;
    let findings = check(&closure, &allow, &spdx);
    Ok(GraphRun { config, closure, findings, lockfile_digest: input_digest(&lock) })
}
