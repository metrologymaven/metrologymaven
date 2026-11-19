//! Toolkit-free check on `cargo metadata --format-version 1` output
//! (REQ-VIZ.RENDER.FRAME.SHLINT-003).
//!
//! The package is looked up among the workspace members by name, and the resolve graph is
//! walked breadth first from it with neighbours in (name, id) order, so every denied crate is
//! reported once with its shortest, deterministic crate path. Every dependency kind (normal,
//! build and dev) and every target platform counts: the check fails closed.

use crate::{Finding, Rule, ShlintError};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The UI-toolkit denylist named by REQ-003.
pub const TOOLKITS: [&str; 5] = ["egui", "eframe", "winit", "iced", "slint"];
/// Sub-crates of the denylist whose names do not start with a toolkit name: egui's
/// `epaint`, `emath` and `ecolor`; slint's internal crates start with `i-slint-`.
pub const TOOLKIT_SUBCRATES: [&str; 3] = ["epaint", "emath", "ecolor"];
/// Prefix of slint's internal crates.
pub const SLINT_INTERNAL_PREFIX: &str = "i-slint-";
/// Most packages accepted in one metadata document.
pub const MAX_PACKAGES: usize = 100_000;

/// True when `name` is a denylisted toolkit crate or one of its sub-crates: the toolkit name
/// itself, the toolkit name followed by `-` or `_`, or a listed sub-crate. Names compare
/// ASCII-case-insensitively with `_` and `-` treated alike, as crates.io does.
pub fn is_toolkit(name: &str) -> bool {
    let n = name.to_ascii_lowercase().replace('_', "-");
    TOOLKITS.iter().any(|t| n == *t || n.strip_prefix(*t).is_some_and(|rest| rest.starts_with('-')))
        || TOOLKIT_SUBCRATES.contains(&n.as_str())
        || n.starts_with(SLINT_INTERNAL_PREFIX)
}

fn bad(reason: impl Into<String>) -> ShlintError {
    ShlintError::MetadataMalformed { reason: reason.into() }
}

fn obj<'a>(v: &'a CValue, what: &str) -> Result<&'a BTreeMap<String, CValue>, ShlintError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(bad(format!("{what} must be an object"))),
    }
}

fn arr<'a>(v: Option<&'a CValue>, what: &str) -> Result<&'a [CValue], ShlintError> {
    match v {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(bad(format!("{what} must be an array"))),
    }
}

fn string<'a>(v: Option<&'a CValue>, what: &str) -> Result<&'a str, ShlintError> {
    match v {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(bad(format!("{what} must be a string"))),
    }
}

/// Runs REQ-003 on a metadata document for `package`.
pub fn check(metadata: &[u8], package: &str) -> Result<Vec<Finding>, ShlintError> {
    let doc = fid_canon::parse(metadata).map_err(|e| bad(format!("not usable JSON ({})", e.code())))?;
    let top = obj(&doc, "the document")?;
    match top.get("version") {
        Some(CValue::Num(n)) if n.to_bits() == 1.0f64.to_bits() => {}
        _ => return Err(bad("`version` must be 1 (cargo metadata --format-version 1)")),
    }

    let packages = arr(top.get("packages"), "`packages`")?;
    if packages.len() > MAX_PACKAGES {
        return Err(bad(format!("more than {MAX_PACKAGES} packages")));
    }
    let mut names: BTreeMap<&str, &str> = BTreeMap::new();
    for p in packages {
        let p = obj(p, "a package")?;
        let id = string(p.get("id"), "package `id`")?;
        let name = string(p.get("name"), "package `name`")?;
        if names.insert(id, name).is_some() {
            return Err(bad(format!("package id {id} is listed twice")));
        }
    }

    let members = arr(top.get("workspace_members"), "`workspace_members`")?;
    let mut roots = Vec::new();
    for m in members {
        let id = string(Some(m), "a workspace member")?;
        let name = names.get(id).ok_or_else(|| bad(format!("workspace member {id} is not in `packages`")))?;
        if *name == package {
            roots.push(id);
        }
    }
    let root = match roots.as_slice() {
        [one] => *one,
        [] => return Err(bad(format!("package {package} is not a workspace member"))),
        _ => return Err(bad(format!("package {package} names more than one workspace member"))),
    };

    let resolve = match top.get("resolve") {
        Some(v @ CValue::Obj(_)) => obj(v, "`resolve`")?,
        _ => return Err(bad("`resolve` is missing or null (was --no-deps used?)")),
    };
    let mut graph: BTreeMap<&str, BTreeSet<(&str, &str)>> = BTreeMap::new();
    for node in arr(resolve.get("nodes"), "`resolve.nodes`")? {
        let node = obj(node, "a resolve node")?;
        let id = string(node.get("id"), "resolve node `id`")?;
        if !names.contains_key(id) {
            return Err(bad(format!("resolve node {id} is not in `packages`")));
        }
        let mut edges = BTreeSet::new();
        let deps = node.get("deps");
        let plain = node.get("dependencies");
        if deps.is_none() && plain.is_none() {
            return Err(bad(format!("resolve node {id} has neither `deps` nor `dependencies`")));
        }
        if deps.is_some() {
            for d in arr(deps, "`deps`")? {
                let d = obj(d, "a dependency")?;
                edges.insert(string(d.get("pkg"), "dependency `pkg`")?);
            }
        }
        if plain.is_some() {
            for d in arr(plain, "`dependencies`")? {
                edges.insert(string(Some(d), "a dependency id")?);
            }
        }
        let mut named = BTreeSet::new();
        for dep in edges {
            let name = names.get(dep).ok_or_else(|| bad(format!("dependency {dep} is not in `packages`")))?;
            named.insert((*name, dep));
        }
        if graph.insert(id, named).is_some() {
            return Err(bad(format!("resolve node {id} is listed twice")));
        }
    }
    if !graph.contains_key(root) {
        return Err(bad(format!("package {package} is not in `resolve.nodes`")));
    }

    // Breadth-first walk from the root; `parent` doubles as the visited set.
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([root]);
    let mut findings = Vec::new();
    while let Some(id) = queue.pop_front() {
        let Some(edges) = graph.get(id) else {
            return Err(bad(format!("dependency {id} has no resolve node")));
        };
        for &(name, dep) in edges {
            if dep == root || parent.contains_key(dep) {
                continue;
            }
            parent.insert(dep, id);
            if is_toolkit(name) {
                findings.push(Finding {
                    location: crate_path(&names, &parent, root, dep),
                    rule: Rule::Toolkit,
                    message: format!("{package} depends on UI-toolkit crate {name}"),
                });
            }
            queue.push_back(dep);
        }
    }
    findings.sort();
    Ok(findings)
}

/// `root -> .. -> id` by package names, following `parent` links.
fn crate_path(names: &BTreeMap<&str, &str>, parent: &BTreeMap<&str, &str>, root: &str, id: &str) -> String {
    let mut chain = vec![names.get(id).copied().unwrap_or(id)];
    let mut cur = id;
    while cur != root {
        let Some(&p) = parent.get(cur) else {
            break;
        };
        chain.push(names.get(p).copied().unwrap_or(p));
        cur = p;
        if chain.len() > parent.len() + 1 {
            break;
        }
    }
    chain.reverse();
    chain.join(" -> ")
}
