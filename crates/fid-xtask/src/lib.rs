//! Repository tasks. `layers`: the crate layer map and layering check (spec node
//! `ECO.REPO.LAYERS`, CONSTITUTION §2.1).
//!
//! `layers.toml` lists the layers in order (`order = [...]`) and assigns every
//! first-party crate to one (`[crates]`, `"name" = "layer"`); `test-support` is a
//! special layer usable only as a dev-dependency. The check reads Cargo's resolved
//! graph and fails when a crate reaches a higher layer, directly or through other
//! crates, depends on a `tool` crate, or uses `test-support` outside dev-dependencies.

use fid_canon::{parse, CValue};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::process::Command;

/// The special layer for test helpers.
pub const TEST_SUPPORT: &str = "test-support";

/// A parsed `layers.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerMap {
    pub order: Vec<String>,
    pub crates: BTreeMap<String, String>,
}

fn unquote(s: &str) -> Option<&str> {
    let s = s.trim();
    s.strip_prefix('"')?.strip_suffix('"').filter(|x| !x.contains('"'))
}

/// Parses `layers.toml` (a fixed, small TOML shape). Errors are `LAYER-MAP` findings.
pub fn parse_layer_map(text: &str) -> Result<LayerMap, String> {
    let mut order: Option<Vec<String>> = None;
    let mut crates = BTreeMap::new();
    let mut in_crates = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_crates = line == "[crates]";
            if !in_crates {
                return Err(format!("LAYER-MAP line {}: unknown section {line}", i + 1));
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("LAYER-MAP line {}: expected key = value", i + 1));
        };
        if in_crates {
            let (Some(name), Some(layer)) = (unquote(k), unquote(v)) else {
                return Err(format!("LAYER-MAP line {}: expected \"crate\" = \"layer\"", i + 1));
            };
            if crates.insert(name.to_string(), layer.to_string()).is_some() {
                return Err(format!("LAYER-MAP line {}: {name} assigned twice", i + 1));
            }
        } else if k.trim() == "order" {
            let v = v.trim();
            let inner = v
                .strip_prefix('[')
                .and_then(|x| x.strip_suffix(']'))
                .ok_or_else(|| format!("LAYER-MAP line {}: order must be a one-line array", i + 1))?;
            let items: Option<Vec<String>> = inner
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(|x| unquote(x).map(String::from))
                .collect();
            order = Some(items.ok_or_else(|| format!("LAYER-MAP line {}: order items must be strings", i + 1))?);
        } else {
            return Err(format!("LAYER-MAP line {}: unknown key {}", i + 1, k.trim()));
        }
    }
    let order = order.ok_or_else(|| "LAYER-MAP: missing order".to_string())?;
    for (name, layer) in &crates {
        if layer != TEST_SUPPORT && !order.contains(layer) {
            return Err(format!("LAYER-MAP: {name} is in unknown layer {layer}"));
        }
    }
    Ok(LayerMap { order, crates })
}

/// One resolved dependency edge.
#[derive(Debug, Clone)]
struct Edge {
    to: String,
    /// True when the dependency is only a dev-dependency.
    dev_only: bool,
}

/// The resolved crate graph, by package name.
#[derive(Debug, Default)]
pub struct Graph {
    members: BTreeSet<String>,
    edges: BTreeMap<String, Vec<Edge>>,
}

fn field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a CValue, String> {
    o.get(k).ok_or_else(|| format!("LAYER-METADATA: missing {k}"))
}

fn as_arr(v: &CValue) -> Result<&Vec<CValue>, String> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err("LAYER-METADATA: expected array".into()),
    }
}

fn as_obj(v: &CValue) -> Result<&BTreeMap<String, CValue>, String> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err("LAYER-METADATA: expected object".into()),
    }
}

fn as_str(v: &CValue) -> Result<&str, String> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err("LAYER-METADATA: expected string".into()),
    }
}

impl Graph {
    /// Builds the graph from `cargo metadata --format-version 1` output.
    pub fn from_metadata(json: &[u8]) -> Result<Graph, String> {
        let root = parse(json).map_err(|e| format!("LAYER-METADATA: {e}"))?;
        let root = as_obj(&root)?;
        let mut name_of = BTreeMap::new();
        for p in as_arr(field(root, "packages")?)? {
            let p = as_obj(p)?;
            name_of.insert(as_str(field(p, "id")?)?.to_string(), as_str(field(p, "name")?)?.to_string());
        }
        let name = |id: &str| name_of.get(id).cloned().ok_or_else(|| format!("LAYER-METADATA: unknown package {id}"));
        let mut g = Graph::default();
        for id in as_arr(field(root, "workspace_members")?)? {
            g.members.insert(name(as_str(id)?)?);
        }
        let resolve = as_obj(field(root, "resolve")?)?;
        for n in as_arr(field(resolve, "nodes")?)? {
            let n = as_obj(n)?;
            let from = name(as_str(field(n, "id")?)?)?;
            let mut edges = Vec::new();
            for d in as_arr(field(n, "deps")?)? {
                let d = as_obj(d)?;
                let to = name(as_str(field(d, "pkg")?)?)?;
                let kinds = as_arr(field(d, "dep_kinds")?)?;
                let dev_only = !kinds.is_empty()
                    && kinds.iter().all(|k| {
                        as_obj(k)
                            .ok()
                            .and_then(|k| k.get("kind"))
                            .is_some_and(|k| matches!(k, CValue::Str(s) if s == "dev"))
                    });
                edges.push(Edge { to, dev_only });
            }
            edges.sort_by(|a, b| a.to.cmp(&b.to));
            g.edges.insert(from, edges);
        }
        Ok(g)
    }

    /// Runs `cargo metadata` for the workspace at `dir`.
    pub fn load(dir: &Path) -> Result<Graph, String> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
            .arg(dir.join("Cargo.toml"))
            .output()
            .map_err(|e| format!("LAYER-METADATA: cannot run cargo: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "LAYER-METADATA: cargo metadata failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Graph::from_metadata(&out.stdout)
    }
}

/// Checks the graph against the map. Returns sorted, de-duplicated findings.
pub fn check(map: &LayerMap, g: &Graph) -> Vec<String> {
    let mut findings = BTreeSet::new();
    for m in &g.members {
        if !map.crates.contains_key(m) {
            findings.insert(format!("LAYER-UNASSIGNED {m}"));
        }
    }
    for c in map.crates.keys() {
        if !g.members.contains(c) {
            findings.insert(format!("LAYER-UNKNOWN {c}"));
        }
    }
    let rank = |layer: &str| map.order.iter().position(|l| l == layer);
    for root in &g.members {
        let Some(root_layer) = map.crates.get(root) else { continue };
        // Breadth-first search with parent pointers, so each finding shows the shortest path.
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        let mut seen: BTreeSet<String> = BTreeSet::from([root.clone()]);
        let mut queue: VecDeque<(String, bool)> = VecDeque::new();
        for e in g.edges.get(root).into_iter().flatten() {
            if seen.insert(e.to.clone()) {
                parent.insert(e.to.clone(), root.clone());
                queue.push_back((e.to.clone(), e.dev_only));
            }
        }
        while let Some((node, via_dev)) = queue.pop_front() {
            let path = || {
                let mut p = vec![node.clone()];
                while let Some(prev) = parent.get(p.last().map(String::as_str).unwrap_or_default()) {
                    p.push(prev.clone());
                }
                p.reverse();
                p.join(" -> ")
            };
            if g.members.contains(&node) {
                let layer = map.crates.get(&node).map(String::as_str);
                let direct = parent.get(&node) == Some(root);
                let root_may_reach_up = root_layer == TEST_SUPPORT || root_layer == "tool";
                match layer {
                    Some("tool") => {
                        findings.insert(format!("LAYER-TOOL {}", path()));
                    }
                    Some(TEST_SUPPORT) if !(direct && via_dev) => {
                        findings.insert(format!("LAYER-TEST-SUPPORT {}", path()));
                    }
                    Some(TEST_SUPPORT) => {}
                    Some(l) if !root_may_reach_up && rank(l) > rank(root_layer) => {
                        findings.insert(format!("LAYER-UPWARD {}", path()));
                    }
                    _ => {}
                }
                // A first-party crate is checked from its own root; stop here.
                continue;
            }
            for e in g.edges.get(&node).into_iter().flatten() {
                if e.dev_only {
                    continue; // dev edges count only from the root crate itself
                }
                if seen.insert(e.to.clone()) {
                    parent.insert(e.to.clone(), node.clone());
                    queue.push_back((e.to.clone(), false));
                }
            }
        }
    }
    findings.into_iter().collect()
}

/// Loads the map and the workspace graph, then checks. `Err` is a map or metadata failure.
pub fn run(workspace: &Path, map_file: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(map_file).map_err(|e| format!("LAYER-MAP: {}: {e}", map_file.display()))?;
    let map = parse_layer_map(&text)?;
    let g = Graph::load(workspace)?;
    Ok(check(&map, &g))
}
