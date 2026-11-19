//! `kernel-allow`: the kernel-graph dependency allowlist (spec node `ECO.REPO.ALLOW`,
//! REQ-ECO.REPO-003).
//!
//! `kernel-allowlist.toml` lists every third-party crate allowed anywhere in the dependency
//! closure of the workspace crates in the `num` and `kernel` layers:
//!
//! ```toml
//! [[allow]]
//! name = "ext-a"
//! version = "^1.2"        # a Cargo version requirement
//! reason = "why the kernel needs it"
//! reviewer = "who approved it"
//! ```
//!
//! The check reuses the layer map ([`crate::parse_layer_map`]) and the resolved graph
//! ([`crate::Graph`], loaded offline with `--all-features`, as `lint-crate-graph` does). From
//! each kernel-layer root it walks normal and build edges (dev-only edges are ignored) and
//! classifies every package that is not a workspace member as third-party. It fails closed:
//!
//! - `ALLOW-UNLISTED <name> via <root -> ... -> name>`: a third-party crate with no entry;
//! - `ALLOW-VERSION <name> <version> <requirement>`: the resolved version does not satisfy
//!   the entry;
//! - `ALLOW-SCHEMA ...`: the allowlist is malformed (missing or empty `name`, `version`,
//!   `reason` or `reviewer`, an unknown key, a duplicate name, or a bad requirement);
//!
//! and warns (without failing) `warn ALLOW-UNUSED <name>` for an entry no kernel crate reaches.
//!
//! Paths are the shortest ones found by a breadth-first search from the roots in name order,
//! visiting dependencies in name order, so the output is deterministic. Like the other graph
//! checks, the graph is resolved for the host platform (so it works offline); CI runs it on
//! Linux, Windows and macOS, which together cover the target-gated dependencies.

use crate::crate_graph::{pkg_name, product_edges};
use crate::{parse_layer_map, Graph, LayerMap};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use toml::{Table, Value};

/// The allowlist file name, at the workspace root.
pub const ALLOWLIST_FILE: &str = "kernel-allowlist.toml";

/// The layers whose dependency closure is checked.
pub const KERNEL_LAYERS: [&str; 2] = ["num", "kernel"];

/// One `[[allow]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The requirement as written.
    pub version: String,
    pub req: VersionReq,
    pub reason: String,
    pub reviewer: String,
}

/// The outcome of a check: failing findings and non-failing warnings, each sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<String>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------
// Versions and Cargo version requirements (the subset of SemVer that Cargo accepts).

/// A pre-release identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ident {
    Num(u64),
    Alpha(String),
}

impl Ident {
    /// SemVer 2.0.0 §11: numeric identifiers sort numerically and below alphanumeric ones.
    fn cmp_ident(&self, other: &Ident) -> Ordering {
        match (self, other) {
            (Ident::Num(a), Ident::Num(b)) => a.cmp(b),
            (Ident::Num(_), Ident::Alpha(_)) => Ordering::Less,
            (Ident::Alpha(_), Ident::Num(_)) => Ordering::Greater,
            (Ident::Alpha(a), Ident::Alpha(b)) => a.as_bytes().cmp(b.as_bytes()),
        }
    }
}

/// Pre-release precedence: no pre-release is greater than any pre-release; otherwise
/// identifiers compare left to right and a shorter prefix is smaller.
fn cmp_pre(a: &[Ident], b: &[Ident]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            for (x, y) in a.iter().zip(b) {
                let o = x.cmp_ident(y);
                if o != Ordering::Equal {
                    return o;
                }
            }
            a.len().cmp(&b.len())
        }
    }
}

/// A resolved package version (`major.minor.patch[-pre][+build]`; build metadata ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<Ident>,
}

/// A numeric identifier: ASCII digits, no leading zero.
fn number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

fn parse_pre(s: &str) -> Option<Vec<Ident>> {
    s.split('.')
        .map(|id| {
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                None
            } else if id.bytes().all(|b| b.is_ascii_digit()) {
                number(id).map(Ident::Num)
            } else {
                Some(Ident::Alpha(id.to_string()))
            }
        })
        .collect()
}

/// Splits off build metadata and the pre-release: `(core, pre)`.
fn split_version(s: &str) -> Option<(&str, Vec<Ident>)> {
    let s = match s.split_once('+') {
        Some((v, build)) => {
            if build.is_empty() || !build.split('.').all(|x| !x.is_empty()) {
                return None;
            }
            v
        }
        None => s,
    };
    match s.split_once('-') {
        Some((core, pre)) => Some((core, parse_pre(pre)?)),
        None => Some((s, Vec::new())),
    }
}

impl Version {
    /// Parses a full SemVer version as `cargo metadata` prints it.
    pub fn parse(s: &str) -> Option<Version> {
        let (core, pre) = split_version(s.trim())?;
        let mut parts = core.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version { major, minor, patch, pre })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
}

/// One comparator; a missing minor or patch is a partial version (`1`, `1.2`, `1.2.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<Ident>,
}

/// A Cargo version requirement: comma-separated comparators, all of which must match.
/// `*` alone is an empty list and matches every release version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    comparators: Vec<Comparator>,
}

fn is_wild(s: &str) -> bool {
    matches!(s, "*" | "x" | "X")
}

fn parse_comparator(s: &str) -> Option<Comparator> {
    let s = s.trim();
    let (op, rest) = [
        (">=", Op::GreaterEq),
        ("<=", Op::LessEq),
        (">", Op::Greater),
        ("<", Op::Less),
        ("=", Op::Exact),
        ("~", Op::Tilde),
        ("^", Op::Caret),
    ]
    .iter()
    .find_map(|&(p, op)| s.strip_prefix(p).map(|r| (Some(op), r.trim_start())))
    .unwrap_or((None, s));
    let (core, pre) = split_version(rest)?;
    let mut parts = core.split('.');
    let major = number(parts.next()?)?;
    let (mut minor, mut patch, mut wild) = (None, None, false);
    if let Some(m) = parts.next() {
        if is_wild(m) {
            wild = true;
        } else {
            minor = Some(number(m)?);
        }
    }
    if let Some(p) = parts.next() {
        if is_wild(p) {
            wild = true;
        } else if wild {
            return None; // `1.*.3`
        } else {
            patch = Some(number(p)?);
        }
    }
    if parts.next().is_some() || (!pre.is_empty() && patch.is_none()) {
        return None;
    }
    let op = match op {
        Some(op) => op,
        None if wild => Op::Exact,
        None => Op::Caret,
    };
    Some(Comparator { op, major, minor, patch, pre })
}

impl VersionReq {
    /// Parses a requirement; `None` when it is not one Cargo would accept.
    pub fn parse(s: &str) -> Option<VersionReq> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if is_wild(s) {
            return Some(VersionReq { comparators: Vec::new() });
        }
        let comparators: Option<Vec<Comparator>> = s.split(',').map(parse_comparator).collect();
        Some(VersionReq { comparators: comparators? })
    }

    /// Cargo's matching rule: every comparator matches, and a pre-release version matches
    /// only when some comparator names the same `major.minor.patch` with a pre-release.
    pub fn matches(&self, v: &Version) -> bool {
        self.comparators.iter().all(|c| c.matches(v))
            && (v.pre.is_empty()
                || self.comparators.iter().any(|c| {
                    c.major == v.major && c.minor == Some(v.minor) && c.patch == Some(v.patch) && !c.pre.is_empty()
                }))
    }
}

impl Comparator {
    fn matches(&self, v: &Version) -> bool {
        match self.op {
            Op::Exact => self.exact(v),
            Op::Greater => self.greater(v),
            Op::GreaterEq => self.exact(v) || self.greater(v),
            Op::Less => self.less(v),
            Op::LessEq => self.exact(v) || self.less(v),
            Op::Tilde => self.tilde(v),
            Op::Caret => self.caret(v),
        }
    }

    fn exact(&self, v: &Version) -> bool {
        v.major == self.major
            && self.minor.is_none_or_eq(v.minor)
            && self.patch.is_none_or_eq(v.patch)
            && cmp_pre(&v.pre, &self.pre) == Ordering::Equal
    }

    /// Compares `v` against the comparator's (partial) version, field by field; a missing
    /// field ends the comparison as equal.
    fn cmp_partial(&self, v: &Version) -> Ordering {
        if v.major != self.major {
            return v.major.cmp(&self.major);
        }
        let Some(minor) = self.minor else { return Ordering::Equal };
        if v.minor != minor {
            return v.minor.cmp(&minor);
        }
        let Some(patch) = self.patch else { return Ordering::Equal };
        if v.patch != patch {
            return v.patch.cmp(&patch);
        }
        cmp_pre(&v.pre, &self.pre)
    }

    fn greater(&self, v: &Version) -> bool {
        self.cmp_partial(v) == Ordering::Greater
    }

    fn less(&self, v: &Version) -> bool {
        self.cmp_partial(v) == Ordering::Less
    }

    fn tilde(&self, v: &Version) -> bool {
        if v.major != self.major || !self.minor.is_none_or_eq(v.minor) {
            return false;
        }
        match self.patch {
            Some(p) if v.patch != p => v.patch > p,
            _ => cmp_pre(&v.pre, &self.pre) != Ordering::Less,
        }
    }

    fn caret(&self, v: &Version) -> bool {
        if v.major != self.major {
            return false;
        }
        let Some(minor) = self.minor else { return true };
        let Some(patch) = self.patch else {
            return if self.major > 0 { v.minor >= minor } else { v.minor == minor };
        };
        if self.major > 0 {
            if v.minor != minor {
                return v.minor > minor;
            }
            if v.patch != patch {
                return v.patch > patch;
            }
        } else if minor > 0 {
            if v.minor != minor {
                return false;
            }
            if v.patch != patch {
                return v.patch > patch;
            }
        } else if v.minor != minor || v.patch != patch {
            return false;
        }
        cmp_pre(&v.pre, &self.pre) != Ordering::Less
    }
}

/// `Option::is_none_or` without needing a newer std.
trait NoneOrEq {
    fn is_none_or_eq(&self, x: u64) -> bool;
}

impl NoneOrEq for Option<u64> {
    fn is_none_or_eq(&self, x: u64) -> bool {
        match self {
            None => true,
            Some(y) => *y == x,
        }
    }
}

// ---------------------------------------------------------------------------------------
// The allowlist file.

const KEYS: [&str; 4] = ["name", "version", "reason", "reviewer"];

/// Parses `kernel-allowlist.toml`. Every problem is an `ALLOW-SCHEMA` finding; all of them
/// are reported, sorted.
pub fn parse_allowlist(text: &str) -> Result<Vec<Entry>, Vec<String>> {
    let table: Table = text.parse().map_err(|e: toml::de::Error| {
        vec![format!("ALLOW-SCHEMA {ALLOWLIST_FILE}: {}", e.to_string().trim_end().replace('\n', " "))]
    })?;
    let mut errors = BTreeSet::new();
    for key in table.keys() {
        if key != "allow" {
            errors.insert(format!("ALLOW-SCHEMA unknown top-level key {key}"));
        }
    }
    let items: &[Value] = match table.get("allow") {
        None => &[],
        Some(Value::Array(items)) => items,
        Some(_) => {
            errors.insert("ALLOW-SCHEMA allow must be an array of tables ([[allow]])".to_string());
            &[]
        }
    };
    let mut entries: Vec<Entry> = Vec::new();
    let mut names = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let Value::Table(t) = item else {
            errors.insert(format!("ALLOW-SCHEMA entry {n}: not a table"));
            continue;
        };
        for k in t.keys() {
            if !KEYS.contains(&k.as_str()) {
                errors.insert(format!("ALLOW-SCHEMA entry {n}: unknown key {k}"));
            }
        }
        let mut field = |k: &str| match t.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(Value::String(_)) => {
                errors.insert(format!("ALLOW-SCHEMA entry {n}: empty {k}"));
                None
            }
            Some(_) => {
                errors.insert(format!("ALLOW-SCHEMA entry {n}: {k} must be a string"));
                None
            }
            None => {
                errors.insert(format!("ALLOW-SCHEMA entry {n}: missing {k}"));
                None
            }
        };
        let (name, version, reason, reviewer) = (field("name"), field("version"), field("reason"), field("reviewer"));
        let req = version.as_deref().and_then(|v| {
            let r = VersionReq::parse(v);
            if r.is_none() {
                errors.insert(format!("ALLOW-SCHEMA entry {n}: invalid version requirement {v:?}"));
            }
            r
        });
        if let Some(name) = &name {
            if !names.insert(name.clone()) {
                errors.insert(format!("ALLOW-SCHEMA entry {n}: {name} is listed twice"));
            }
        }
        if let (Some(name), Some(version), Some(req), Some(reason), Some(reviewer)) =
            (name, version, req, reason, reviewer)
        {
            entries.push(Entry { name, version, req, reason, reviewer });
        }
    }
    if errors.is_empty() {
        Ok(entries)
    } else {
        Err(errors.into_iter().collect())
    }
}

// ---------------------------------------------------------------------------------------
// The check.

/// Workspace members in the `num` and `kernel` layers, in (name, ID) order.
fn kernel_roots<'g>(map: &LayerMap, g: &'g Graph) -> Vec<&'g str> {
    let mut roots: Vec<&str> = g
        .members
        .iter()
        .map(String::as_str)
        .filter(|id| map.crates.get(pkg_name(g, id)).is_some_and(|l| KERNEL_LAYERS.contains(&l.as_str())))
        .collect();
    roots.sort_by(|x, y| (pkg_name(g, x), *x).cmp(&(pkg_name(g, y), *y)));
    roots
}

/// Every third-party package ID reached from the kernel roots over normal and build edges,
/// with the first shortest path to it (crate names).
fn third_party_closure<'g>(map: &LayerMap, g: &'g Graph) -> BTreeMap<&'g str, Vec<&'g str>> {
    let adj = product_edges(g);
    let mut reached: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for root in kernel_roots(map, g) {
        let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
        let mut seen: BTreeSet<&str> = BTreeSet::from([root]);
        let mut queue: VecDeque<&str> = VecDeque::from([root]);
        while let Some(node) = queue.pop_front() {
            if !g.members.contains(node) && !reached.contains_key(node) {
                let mut path = vec![pkg_name(g, node)];
                let mut cur = node;
                while let Some(&p) = parent.get(cur) {
                    path.push(pkg_name(g, p));
                    cur = p;
                }
                path.reverse();
                reached.insert(node, path);
            }
            for &next in adj.get(node).into_iter().flatten() {
                if seen.insert(next) {
                    parent.insert(next, node);
                    queue.push_back(next);
                }
            }
        }
    }
    reached
}

/// Checks the kernel graph against the allowlist entries.
pub fn check(entries: &[Entry], map: &LayerMap, g: &Graph) -> Report {
    let by_name: BTreeMap<&str, &Entry> = entries.iter().map(|e| (e.name.as_str(), e)).collect();
    let mut findings = BTreeSet::new();
    let mut used = BTreeSet::new();
    let mut unlisted: BTreeMap<&str, String> = BTreeMap::new();
    // Order by (name, ID) so that, for a crate reached in two versions, the reported path
    // is the same on every run.
    let mut reached: Vec<(&str, Vec<&str>)> = third_party_closure(map, g).into_iter().collect();
    reached.sort_by(|(x, _), (y, _)| (pkg_name(g, x), *x).cmp(&(pkg_name(g, y), *y)));
    for (id, path) in reached {
        let name = pkg_name(g, id);
        let version = g.versions.get(id).map(String::as_str).unwrap_or("?");
        match by_name.get(name) {
            None => {
                unlisted.entry(name).or_insert_with(|| path.join(" -> "));
            }
            Some(e) => {
                used.insert(name);
                if !Version::parse(version).is_some_and(|v| e.req.matches(&v)) {
                    findings.insert(format!("ALLOW-VERSION {name} {version} {}", e.version));
                }
            }
        }
    }
    for (name, path) in unlisted {
        findings.insert(format!("ALLOW-UNLISTED {name} via {path}"));
    }
    let warnings =
        by_name.keys().filter(|n| !used.contains(*n)).map(|n| format!("warn ALLOW-UNUSED {n}")).collect::<Vec<_>>();
    Report { findings: findings.into_iter().collect(), warnings }
}

/// Reads the layer map and the allowlist, loads the workspace graph offline (all features)
/// and checks it. A malformed allowlist gives `ALLOW-SCHEMA` findings without loading the
/// graph. `Err` is an unreadable file, a bad layer map or a metadata failure.
pub fn run(workspace: &Path, map_file: &Path, allowlist: &Path) -> Result<Report, String> {
    let text = std::fs::read_to_string(allowlist).map_err(|e| format!("ALLOW-FILE {}: {e}", allowlist.display()))?;
    let entries = match parse_allowlist(&text) {
        Ok(e) => e,
        Err(findings) => return Ok(Report { findings, warnings: Vec::new() }),
    };
    let map_text =
        std::fs::read_to_string(map_file).map_err(|e| format!("LAYER-MAP: {}: {e}", map_file.display()))?;
    let map = parse_layer_map(&map_text)?;
    let g = Graph::load_with(workspace, true)?;
    Ok(check(&entries, &map, &g))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn m(req: &str, v: &str) -> bool {
        VersionReq::parse(req).unwrap().matches(&Version::parse(v).unwrap())
    }

    // Expected results hand-labelled from the Cargo reference ("Specifying dependencies",
    // version requirement syntax) and SemVer 2.0.0 §11.
    #[test]
    fn caret() {
        assert!(m("^1.2", "1.2.0") && m("^1.2", "1.9.9") && !m("^1.2", "2.0.0") && !m("^1.2", "1.1.9"));
        assert!(m("1.2.3", "1.2.3") && m("1.2.3", "1.3.0") && !m("1.2.3", "1.2.2"));
        assert!(m("^0.2.3", "0.2.9") && !m("^0.2.3", "0.3.0") && !m("^0.2.3", "0.2.2"));
        assert!(m("^0.0.3", "0.0.3") && !m("^0.0.3", "0.0.4"));
        assert!(m("^0.0", "0.0.7") && !m("^0.0", "0.1.0") && m("^0", "0.9.0") && !m("^0", "1.0.0"));
        assert!(m("^1", "1.99.0") && !m("^1", "2.0.0"));
    }

    #[test]
    fn tilde_wildcard_and_ranges() {
        assert!(m("~1.2.3", "1.2.9") && !m("~1.2.3", "1.3.0") && !m("~1.2.3", "1.2.2"));
        assert!(m("~1.2", "1.2.0") && !m("~1.2", "1.3.0") && m("~1", "1.9.0") && !m("~1", "2.0.0"));
        assert!(m("*", "7.0.0") && m("1.*", "1.4.0") && !m("1.*", "2.0.0") && m("1.2.x", "1.2.5"));
        assert!(m(">= 1.2, < 1.5", "1.4.9") && !m(">= 1.2, < 1.5", "1.5.0") && !m(">=1.2,<1.5", "1.1.0"));
        assert!(m("=1.2.3", "1.2.3") && !m("=1.2.3", "1.2.4") && m("=1.2", "1.2.8"));
        assert!(m(">1", "2.0.0") && !m(">1", "1.9.0") && m("<=1.2", "1.2.9") && !m("<=1.2", "1.3.0"));
    }

    #[test]
    fn pre_releases() {
        assert!(!m("^1.2", "1.3.0-beta.1"));
        assert!(m("^1.2.3-beta.2", "1.2.3-beta.10") && !m("^1.2.3-beta.2", "1.2.3-beta.1"));
        assert!(m("^1.2.3-beta.2", "1.2.3") && m("^1.2.3-alpha", "1.2.3-beta"));
        assert!(!m("^1.2.3-beta.2", "1.2.4-beta.3"));
        assert!(m("^1.0.0", "1.0.0+build.5"));
    }

    #[test]
    fn bad_requirements_are_refused() {
        for r in ["", "1.2.3.4", "01.2", "^", "1.*.3", ">=1.2-beta", "abc", "1.2,"] {
            assert!(VersionReq::parse(r).is_none(), "{r}");
        }
    }

    #[test]
    fn schema() {
        assert_eq!(parse_allowlist("").unwrap(), vec![]);
        let ok = "[[allow]]\nname = \"a\"\nversion = \"^1\"\nreason = \"r\"\nreviewer = \"me\"\n";
        assert_eq!(parse_allowlist(ok).unwrap().len(), 1);
        let errs = parse_allowlist(&format!("{ok}{ok}")).unwrap_err();
        assert_eq!(errs, vec!["ALLOW-SCHEMA entry 2: a is listed twice"]);
        let errs = parse_allowlist("[[allow]]\nname = \"a\"\nversion = \"nope\"\nreason = \" \"\nextra = 1\n")
            .unwrap_err();
        assert_eq!(
            errs,
            vec![
                "ALLOW-SCHEMA entry 1: empty reason",
                "ALLOW-SCHEMA entry 1: invalid version requirement \"nope\"",
                "ALLOW-SCHEMA entry 1: missing reviewer",
                "ALLOW-SCHEMA entry 1: unknown key extra",
            ]
        );
        assert!(parse_allowlist("allow = 3\n").unwrap_err()[0].starts_with("ALLOW-SCHEMA"));
        assert!(parse_allowlist("other = 3\n").unwrap_err()[0].starts_with("ALLOW-SCHEMA"));
    }
}
