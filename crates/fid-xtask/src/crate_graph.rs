//! `lint-crate-graph`: the no-side-doors crate-graph check (spec node `CORE.BUS.LINT`,
//! CONSTITUTION C3, REQ-CORE.BUS-016).
//!
//! Reads Cargo's resolved graph (`cargo metadata --format-version 1`, shared with the
//! `layers` check) and an allowlist, then reports:
//!
//! - `mutable_state`: a workspace crate outside `[mutable_state] allowed` that reaches the
//!   mutable-state crate, directly or transitively;
//! - `sqlite_driver`: a workspace crate outside the storage writers that reaches a SQLite
//!   driver crate, directly or transitively;
//! - `surface_dep`: a surface crate that depends directly on a CORE crate other than the
//!   bus API crate.
//!
//! The storage writers are the union of `[storage_writers] allowed` in the allowlist and
//! in `storage-writers.toml` (published by `CORE.STO.LAYOUT`), when that file exists.
//!
//! Only shipped edges count: dev-only dependencies are ignored. Allowed crates are the
//! doors: a path is not followed through them, because reaching state through the bus or
//! the store is the intended route. Each crate reports at most one path per rule: the first
//! one found by a breadth-first search that visits dependencies in name order. Findings are
//! printed one per line, `<crate>: <rule> <a -> b -> c>`, sorted by crate name.

use super::{unquote, Graph};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// Rule names, as printed.
pub const RULE_STATE: &str = "mutable_state";
pub const RULE_SQLITE: &str = "sqlite_driver";
pub const RULE_SURFACE: &str = "surface_dep";

/// The parsed allowlist (`lint-allowlist.toml` merged with `storage-writers.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    /// Name of the crate that holds mutable project state.
    pub mutable_state: String,
    /// Crates that may reach it.
    pub mutable_state_allowed: BTreeSet<String>,
    /// Crates that may reach a SQLite driver.
    pub storage_writers: BTreeSet<String>,
    /// SQLite driver crate names.
    pub sqlite_drivers: BTreeSet<String>,
    /// Surface crates (CLI, MCP, Python, GUI, control channel).
    pub surfaces: BTreeSet<String>,
    /// The bus API crate, the only CORE crate a surface may depend on.
    pub bus_api: String,
    /// The CORE crates.
    pub core: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Val {
    Str(String),
    List(Vec<String>),
}

/// Section name to key to (line number, value).
type Tables = BTreeMap<String, BTreeMap<String, (usize, Val)>>;

fn invalid(file: &str, msg: &str) -> String {
    format!("allowlist_invalid: {file}: {msg}")
}

fn invalid_at(file: &str, line: usize, msg: &str) -> String {
    format!("allowlist_invalid: {file} line {line}: {msg}")
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn name_value(s: &str) -> Option<String> {
    unquote(s).filter(|x| !x.is_empty()).map(String::from)
}

/// Parses the small TOML subset the allowlist files use: `[section]` headers and
/// `key = "string"` or `key = ["a", "b"]` (one-line arrays). Anything else is refused.
fn parse_tables(text: &str, file: &str) -> Result<Tables, String> {
    let mut tables: Tables = BTreeMap::new();
    let mut current: Option<String> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let name = rest
                .strip_suffix(']')
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .ok_or_else(|| invalid_at(file, n, "malformed section header"))?;
            if tables.insert(name.to_string(), BTreeMap::new()).is_some() {
                return Err(invalid_at(file, n, &format!("section [{name}] appears twice")));
            }
            current = Some(name.to_string());
            continue;
        }
        let Some(section) = current.as_ref() else {
            return Err(invalid_at(file, n, "key outside a section"));
        };
        let Some((k, v)) = line.split_once('=') else {
            return Err(invalid_at(file, n, "expected key = value"));
        };
        let (key, v) = (k.trim(), v.trim());
        let val = if let Some(inner) = v.strip_prefix('[') {
            let inner = inner.strip_suffix(']').ok_or_else(|| invalid_at(file, n, "arrays must be on one line"))?;
            let items: Option<Vec<String>> =
                inner.split(',').map(str::trim).filter(|x| !x.is_empty()).map(name_value).collect();
            Val::List(items.ok_or_else(|| invalid_at(file, n, "array items must be non-empty strings"))?)
        } else {
            Val::Str(name_value(v).ok_or_else(|| invalid_at(file, n, "value must be a non-empty string or an array"))?)
        };
        let table = tables.entry(section.clone()).or_default();
        if table.insert(key.to_string(), (n, val)).is_some() {
            return Err(invalid_at(file, n, &format!("key {key} appears twice in [{section}]")));
        }
    }
    Ok(tables)
}

/// Refuses sections and keys outside `schema` (section, allowed keys).
fn check_keys(tables: &Tables, file: &str, schema: &[(&str, &[&str])]) -> Result<(), String> {
    for (sec, keys) in tables {
        let Some((_, allowed)) = schema.iter().find(|(s, _)| *s == sec.as_str()) else {
            return Err(invalid(file, &format!("unknown section [{sec}]")));
        };
        for (k, (n, _)) in keys {
            if !allowed.contains(&k.as_str()) {
                return Err(invalid_at(file, *n, &format!("unknown key {k} in [{sec}]")));
            }
        }
    }
    Ok(())
}

fn get<'t>(tables: &'t Tables, sec: &str, key: &str) -> Option<&'t Val> {
    tables.get(sec)?.get(key).map(|(_, v)| v)
}

fn req_str(tables: &Tables, file: &str, sec: &str, key: &str) -> Result<String, String> {
    match get(tables, sec, key) {
        Some(Val::Str(s)) => Ok(s.clone()),
        Some(Val::List(_)) => Err(invalid(file, &format!("[{sec}] {key} must be a string"))),
        None => Err(invalid(file, &format!("missing [{sec}] {key}"))),
    }
}

fn list(tables: &Tables, file: &str, sec: &str, key: &str, required: bool) -> Result<BTreeSet<String>, String> {
    match get(tables, sec, key) {
        Some(Val::List(v)) => Ok(v.iter().cloned().collect()),
        Some(Val::Str(_)) => Err(invalid(file, &format!("[{sec}] {key} must be an array"))),
        None if required => Err(invalid(file, &format!("missing [{sec}] {key}"))),
        None => Ok(BTreeSet::new()),
    }
}

/// The allowlist file's name, used in messages.
pub const ALLOWLIST_FILE: &str = "lint-allowlist.toml";
/// The storage-writer list published by `CORE.STO.LAYOUT`.
pub const WRITERS_FILE: &str = "storage-writers.toml";

impl Allowlist {
    /// Parses the allowlist text and, if given, the `storage-writers.toml` text.
    pub fn parse(allowlist: &str, writers: Option<&str>) -> Result<Allowlist, String> {
        let f = ALLOWLIST_FILE;
        let t = parse_tables(allowlist, f)?;
        check_keys(
            &t,
            f,
            &[
                ("mutable_state", &["crate", "allowed"]),
                ("storage_writers", &["allowed"]),
                ("sqlite", &["drivers"]),
                ("surfaces", &["crates", "bus_api"]),
                ("core", &["crates"]),
            ],
        )?;
        let mut storage_writers = list(&t, f, "storage_writers", "allowed", false)?;
        if let Some(w) = writers {
            let wt = parse_tables(w, WRITERS_FILE)?;
            check_keys(&wt, WRITERS_FILE, &[("storage_writers", &["allowed"])])?;
            storage_writers.extend(list(&wt, WRITERS_FILE, "storage_writers", "allowed", true)?);
        }
        Ok(Allowlist {
            mutable_state: req_str(&t, f, "mutable_state", "crate")?,
            mutable_state_allowed: list(&t, f, "mutable_state", "allowed", true)?,
            storage_writers,
            sqlite_drivers: list(&t, f, "sqlite", "drivers", true)?,
            surfaces: list(&t, f, "surfaces", "crates", true)?,
            bus_api: req_str(&t, f, "surfaces", "bus_api")?,
            core: list(&t, f, "core", "crates", true)?,
        })
    }

    /// Reads the allowlist file and `writers` (skipped when it does not exist).
    pub fn load(allowlist: &Path, writers: &Path) -> Result<Allowlist, String> {
        let text = std::fs::read_to_string(allowlist)
            .map_err(|e| format!("allowlist_invalid: {}: {e}", allowlist.display()))?;
        let writers_text = match std::fs::read_to_string(writers) {
            Ok(w) => Some(w),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("allowlist_invalid: {}: {e}", writers.display())),
        };
        Allowlist::parse(&text, writers_text.as_deref())
    }
}

fn pkg_name<'a>(g: &'a Graph, id: &'a str) -> &'a str {
    g.names.get(id).map(String::as_str).unwrap_or(id)
}

/// Shipped dependency edges (dev-only edges dropped), each list in (name, ID) order.
fn product_edges(g: &Graph) -> BTreeMap<&str, Vec<&str>> {
    let mut adj = BTreeMap::new();
    for (from, edges) in &g.edges {
        let mut to: Vec<&str> = edges.iter().filter(|e| !e.dev_only).map(|e| e.to.as_str()).collect();
        to.sort_by(|x, y| (pkg_name(g, x), *x).cmp(&(pkg_name(g, y), *y)));
        to.dedup();
        adj.insert(from.as_str(), to);
    }
    adj
}

/// Breadth-first search from `root` to the first crate named in `targets`, not walking
/// through crates named in `doors`. Returns the path as crate names.
fn first_path<'a>(
    g: &'a Graph,
    adj: &BTreeMap<&'a str, Vec<&'a str>>,
    root: &'a str,
    targets: &BTreeSet<String>,
    doors: &BTreeSet<String>,
) -> Option<Vec<&'a str>> {
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([root]);
    let mut queue: VecDeque<&str> = VecDeque::from([root]);
    while let Some(node) = queue.pop_front() {
        for &next in adj.get(node).into_iter().flatten() {
            if !seen.insert(next) {
                continue;
            }
            parent.insert(next, node);
            let next_name = pkg_name(g, next);
            if targets.contains(next_name) {
                let mut path = vec![next_name];
                let mut cur = next;
                while let Some(&p) = parent.get(cur) {
                    path.push(pkg_name(g, p));
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            if !doors.contains(next_name) {
                queue.push_back(next);
            }
        }
    }
    None
}

/// Checks the graph against the allowlist. Returns the findings, sorted by crate name.
pub fn check(a: &Allowlist, g: &Graph) -> Vec<String> {
    let adj = product_edges(g);
    let state = BTreeSet::from([a.mutable_state.clone()]);
    let rules: [(&str, &BTreeSet<String>, &BTreeSet<String>); 2] =
        [(RULE_STATE, &state, &a.mutable_state_allowed), (RULE_SQLITE, &a.sqlite_drivers, &a.storage_writers)];
    let mut out: BTreeSet<(String, &str, String)> = BTreeSet::new();
    for root in &g.members {
        let root = root.as_str();
        let rn = pkg_name(g, root);
        for (rule, targets, doors) in rules {
            if doors.contains(rn) || targets.contains(rn) {
                continue;
            }
            if let Some(path) = first_path(g, &adj, root, targets, doors) {
                out.insert((rn.to_string(), rule, path.join(" -> ")));
            }
        }
        if a.surfaces.contains(rn) {
            for &to in adj.get(root).into_iter().flatten() {
                let tn = pkg_name(g, to);
                if a.core.contains(tn) && tn != a.bus_api {
                    out.insert((rn.to_string(), RULE_SURFACE, format!("{rn} -> {tn}")));
                }
            }
        }
    }
    out.into_iter().map(|(c, r, p)| format!("{c}: {r} {p}")).collect()
}

/// Loads the allowlist (and `writers`, if present) and the workspace graph, then checks.
/// `Err` is an `allowlist_invalid` or `metadata_unavailable` failure.
pub fn run(workspace: &Path, allowlist: &Path, writers: &Path) -> Result<Vec<String>, String> {
    let a = Allowlist::load(allowlist, writers)?;
    let g = Graph::load(workspace).map_err(|e| format!("metadata_unavailable: {e}"))?;
    Ok(check(&a, &g))
}
