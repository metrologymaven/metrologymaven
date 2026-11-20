//! Repository tasks. `layers`: the crate layer map and layering check (spec node
//! `ECO.REPO.LAYERS`, CONSTITUTION §2.1).
//!
//! `layers.toml` lists the layers in order (`order = [...]`) and assigns every
//! first-party crate to one (`[crates]`, `"name" = "layer"`); `test-support` is a
//! special layer usable only as a dev-dependency. The check reads Cargo's resolved
//! graph and fails when a crate reaches a higher layer, directly or through other
//! crates, depends on a `tool` crate, or uses `test-support` outside dev-dependencies.
//!
//! `lint-crate-graph`: the no-side-doors check (spec node `CORE.BUS.LINT`), in
//! [`crate_graph`]; it reuses the resolved graph built here.
//!
//! `plat-matrix lint-tiers`: the tier definitions lint (spec node
//! `PLAT.MATRIX.POLICY.DEFS`), in [`plat_matrix_tiers`].
//!
//! `plat-deploy netlist`: the network-feature list generated from the registry manifest
//! (spec node `PLAT.DEPLOY.OFFLINE.NETLIST`), in [`plat_deploy_netlist`].
//!
//! `rel version-check` and `rel version-compare`: the product version scheme and tag check
//! (spec node `ECO.REL.PROC.VERSION`), in [`rel::version`].
//!
//! `rel sums`: the release checksum file and checksum statement (spec node
//! `ECO.REL.SIGN.SUMS`), in [`rel::sums`].
//!
//! `rel sbom`: one SPDX 2.3 JSON SBOM per planned artefact (spec node `ECO.REL.SIGN.SBOM`), in
//! [`rel::sbom`].
//!
//! `plat-gpu invariance`: the cross-backend value-of-record invariance harness (spec node
//! `PLAT.GPU.FALLBK.INVAR`), in [`plat_gpu_invar`].
//!
//! `apps-policy`: the app-policy boundary and cross-app command identity check (spec node
//! `ECO.APPS.POLICY`), in [`apps_policy`]; it reuses the resolved graph built here.
//!
//! [`bots`]: the bot register check `bots-check` (spec node `ECO.CI.BOTS.REG`), and in
//! [`bots::block`] the maintainer-marked blocking findings check `bot-blocking` (spec node
//! `ECO.CI.BOTS.BLOCK`).
//!
//! `conic-audit`: the vendored conic solver's audit check (spec node `KRN.SOLV.CONIC.VEND`), in
//! [`conic_audit`].
//!
//! `registry-gate`: the build gate against the released baseline manifest (spec node
//! `CORE.REG.VER.GATE`), in [`registry_gate`].
//!
//! `kernel-allow`: the kernel-graph third-party dependency allowlist (spec node
//! `ECO.REPO.ALLOW`), in [`kernel_allow`]; it reuses the layer map and the resolved graph
//! built here.

pub mod apps_policy;
pub mod bots;
pub mod conic_audit;
pub mod crate_graph;
pub mod deny_scope;
pub mod kernel_allow;
pub mod plat_deploy_netlist;
pub mod plat_gpu_invar;
pub mod plat_matrix_tiers;
pub mod registry_gate;
pub mod rel;

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
    /// Package ID of the dependency.
    to: String,
    /// True when the dependency is only a dev-dependency.
    dev_only: bool,
}

/// The resolved crate graph. Keyed by Cargo package ID, so two versions of one
/// crate stay distinct; names are used only to look up layers and to print paths.
#[derive(Debug, Default)]
pub struct Graph {
    /// Workspace member package IDs.
    members: BTreeSet<String>,
    /// Package ID to package name.
    names: BTreeMap<String, String>,
    /// Package ID to its resolved version, as `cargo metadata` prints it.
    versions: BTreeMap<String, String>,
    /// Package ID to its dependency edges.
    edges: BTreeMap<String, Vec<Edge>>,
    /// Package ID to its `Cargo.toml` path, when `cargo metadata` gives one.
    manifest_paths: BTreeMap<String, String>,
    /// Package ID to its declared dependencies as (package name, dev-only), from
    /// `packages[].dependencies`. Unlike the resolved edges, these include dependencies on
    /// binary-only packages, which Cargo leaves out of `resolve`.
    declared: BTreeMap<String, BTreeSet<(String, bool)>>,
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
        let mut g = Graph::default();
        for p in as_arr(field(root, "packages")?)? {
            let p = as_obj(p)?;
            let id = as_str(field(p, "id")?)?.to_string();
            if let Some(CValue::Str(m)) = p.get("manifest_path") {
                g.manifest_paths.insert(id.clone(), m.clone());
            }
            if let Some(CValue::Arr(deps)) = p.get("dependencies") {
                let mut set = BTreeSet::new();
                for d in deps {
                    let CValue::Obj(d) = d else { continue };
                    let Some(CValue::Str(n)) = d.get("name") else { continue };
                    let dev = matches!(d.get("kind"), Some(CValue::Str(k)) if k == "dev");
                    set.insert((n.clone(), dev));
                }
                g.declared.insert(id.clone(), set);
            }
            if let Some(CValue::Str(v)) = p.get("version") {
                g.versions.insert(id.clone(), v.clone());
            }
            g.names.insert(id, as_str(field(p, "name")?)?.to_string());
        }
        let known = |g: &Graph, id: &str| {
            if g.names.contains_key(id) {
                Ok(id.to_string())
            } else {
                Err(format!("LAYER-METADATA: unknown package {id}"))
            }
        };
        for id in as_arr(field(root, "workspace_members")?)? {
            let id = known(&g, as_str(id)?)?;
            g.members.insert(id);
        }
        let resolve = as_obj(field(root, "resolve")?)?;
        for n in as_arr(field(resolve, "nodes")?)? {
            let n = as_obj(n)?;
            let from = known(&g, as_str(field(n, "id")?)?)?;
            let mut edges = Vec::new();
            for d in as_arr(field(n, "deps")?)? {
                let d = as_obj(d)?;
                let to = known(&g, as_str(field(d, "pkg")?)?)?;
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

    /// Runs `cargo metadata` for the workspace at `dir`, resolved for the host platform so
    /// that it works offline: dependencies of other platforms (e.g. `libc` for macOS-only
    /// code paths) need not be downloaded. CI runs the check on Linux, Windows and macOS.
    pub fn load(dir: &Path) -> Result<Graph, String> {
        Graph::load_with(dir, false)
    }

    /// Like [`Graph::load`]; with `all_features`, passes `--all-features` so optional
    /// (feature-gated) dependencies are part of the resolved graph.
    pub fn load_with(dir: &Path, all_features: bool) -> Result<Graph, String> {
        Graph::load_metadata(dir, all_features, Some(host_triple()?))
    }

    /// Like [`Graph::load_with`], but without `--filter-platform`: dependencies of every
    /// target (including `[target.'cfg(...)'.dependencies]` for other platforms) are part of
    /// the graph. Offline, this needs them in the local registry cache (`cargo fetch` with no
    /// `--target` downloads them all).
    pub fn load_all_targets(dir: &Path, all_features: bool) -> Result<Graph, String> {
        Graph::load_metadata(dir, all_features, None)
    }

    fn load_metadata(dir: &Path, all_features: bool, platform: Option<String>) -> Result<Graph, String> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.args(["metadata", "--format-version", "1", "--offline"]);
        if let Some(p) = platform {
            cmd.args(["--filter-platform", &p]);
        }
        if all_features {
            cmd.arg("--all-features");
        }
        let out = cmd
            .arg("--manifest-path")
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

/// The host target triple, from `rustc -vV`.
fn host_triple() -> Result<String, String> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let out = Command::new(rustc).arg("-vV").output().map_err(|e| format!("LAYER-METADATA: cannot run rustc: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(|h| h.trim().to_string())
        .ok_or_else(|| "LAYER-METADATA: rustc -vV printed no host line".to_string())
}

/// Checks the graph against the map. Returns sorted, de-duplicated findings.
pub fn check(map: &LayerMap, g: &Graph) -> Vec<String> {
    let mut findings = BTreeSet::new();
    let name = |id: &str| g.names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let member_names: BTreeSet<String> = g.members.iter().map(|id| name(id)).collect();
    for m in &member_names {
        if !map.crates.contains_key(m) {
            findings.insert(format!("LAYER-UNASSIGNED {m}"));
        }
    }
    for c in map.crates.keys() {
        if !member_names.contains(c) {
            findings.insert(format!("LAYER-UNKNOWN {c}"));
        }
    }
    let rank = |layer: &str| map.order.iter().position(|l| l == layer);
    let layer_of = |id: &str| -> Option<String> {
        if g.members.contains(id) {
            map.crates.get(&name(id)).cloned()
        } else {
            None
        }
    };
    // A search state is (package ID, reached only through the root's dev-dependency edge).
    type State = (String, bool);
    for root in &g.members {
        let Some(root_layer) = layer_of(root) else { continue };
        let root_may_reach_up = root_layer == TEST_SUPPORT || root_layer == "tool";
        // Breadth-first search with parent pointers, so each finding shows a shortest path.
        let mut parent: BTreeMap<State, State> = BTreeMap::new();
        let start: State = (root.clone(), false);
        let mut seen: BTreeSet<State> = BTreeSet::from([start.clone()]);
        let mut queue: VecDeque<State> = VecDeque::new();
        for e in g.edges.get(root).into_iter().flatten() {
            let s: State = (e.to.clone(), e.dev_only);
            if seen.insert(s.clone()) {
                parent.insert(s.clone(), start.clone());
                queue.push_back(s);
            }
        }
        while let Some(state) = queue.pop_front() {
            let (node, via_dev) = (&state.0, state.1);
            let path = || {
                let mut p = vec![name(node)];
                let mut cur = &state;
                while let Some(prev) = parent.get(cur) {
                    p.push(name(&prev.0));
                    cur = prev;
                }
                p.reverse();
                p.join(" -> ")
            };
            let direct = parent.get(&state) == Some(&start);
            let mut keep_walking = !g.members.contains(node);
            match layer_of(node).as_deref() {
                Some("tool") => {
                    findings.insert(format!("LAYER-TOOL {}", path()));
                }
                Some(TEST_SUPPORT) => {
                    if !(direct && via_dev) {
                        findings.insert(format!("LAYER-TEST-SUPPORT {}", path()));
                    }
                    // Test-support crates are exempt from their own upward check, so
                    // anything they reach counts against the crate that uses them.
                    keep_walking = true;
                }
                Some(l) if !root_may_reach_up && rank(l) > rank(&root_layer) => {
                    findings.insert(format!("LAYER-UPWARD {}", path()));
                }
                // Any other first-party crate is checked from its own root.
                _ => {}
            }
            if !keep_walking {
                continue;
            }
            for e in g.edges.get(node).into_iter().flatten() {
                if e.dev_only {
                    continue; // dev edges count only from the root crate itself
                }
                let s: State = (e.to.clone(), via_dev);
                if seen.insert(s.clone()) {
                    parent.insert(s.clone(), state.clone());
                    queue.push_back(s);
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
