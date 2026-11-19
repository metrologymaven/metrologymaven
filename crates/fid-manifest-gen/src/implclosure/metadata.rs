//! Bounded, fail-closed reader for the `cargo metadata --format-version 1` file.
//!
//! Only the fields the closure needs are read: `version` (must be `1`), `workspace_root`,
//! each package's `id`, `name`, `version`, `source`, `manifest_path` and `targets` (`kind`, `src_path`),
//! and the resolved graph `resolve.nodes[]` (`id`, `features`, `deps[].pkg`,
//! `deps[].dep_kinds[].kind`). Other fields are ignored, because Cargo adds fields over time.
//! A missing `resolve` (`--no-deps`), a dangling package ID, a duplicate package or node ID,
//! an unknown dependency kind or a non-string where a string belongs is `MetadataInvalid`.
//!
//! Paths are compared after replacing `\` with `/`, so metadata written on Windows and on
//! Unix is read the same way. Package directories are resolved lazily, only for crates that
//! are in some closure, so third-party crates outside the kernel graph never need a path
//! under the workspace root.

use std::collections::{BTreeMap, BTreeSet};

use fid_canon::CValue;

use super::ImplError;

/// Largest accepted metadata file, in bytes (the `fid-canon` reader's own cap).
pub const MAX_METADATA_BYTES: usize = fid_canon::MAX_INPUT_BYTES;

/// Target kinds whose sources must lie under the crate's `src/` directory: they are what
/// the crate builds into, so a source outside `src/` would escape the digest.
const BUILT_KINDS: [&str; 6] = ["lib", "rlib", "dylib", "cdylib", "staticlib", "bin"];
/// Target kinds that are not part of the built crate (REQ-KRN.MAN.MANI.IMPL-002).
const IGNORED_KINDS: [&str; 3] = ["test", "bench", "example"];
/// A build script runs arbitrary code at build time and can read any file or the
/// environment, so no crate in a kernel closure may have one (REQ-KRN.MAN.MANI.IMPL-001).
const BUILD_KIND: &str = "custom-build";
/// A procedural-macro crate also runs code at build time; refused for the same reason.
const PROC_MACRO_KIND: &str = "proc-macro";

fn invalid(reason: &'static str) -> ImplError {
    ImplError::MetadataInvalid { reason }
}

/// One target of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    kinds: Vec<String>,
    src_path: String,
}

/// One package of the metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    id: String,
    name: String,
    version: String,
    /// `source` (`None` for a path crate).
    source: Option<String>,
    /// `manifest_path` with `/` separators.
    manifest_path: String,
    targets: Vec<Target>,
}

impl Package {
    /// The package (crate) name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The resolved version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Cargo's package ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The package source (`None` for a path crate).
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// The parts of a crate's layout the digest needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    /// The crate directory relative to the workspace root, `/`-separated, `""` for the root.
    pub dir: String,
}

/// A node of the resolved graph.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    /// Enabled features, sorted and unique.
    features: Vec<String>,
    /// Package IDs reached over a normal or build edge, for any platform.
    deps: BTreeSet<String>,
}

/// A parsed `cargo metadata` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// `/`-separated, without a trailing `/`.
    workspace_root: String,
    packages: BTreeMap<String, Package>,
    nodes: BTreeMap<String, Node>,
}

/// `\` to `/`.
fn slashes(p: &str) -> String {
    p.replace('\\', "/")
}

/// `path` relative to `base` (both `/`-separated, `base` without a trailing `/`): `""` when
/// equal; `None` when `path` is not strictly inside `base` or has an empty, `.` or `..`
/// component.
fn relative(path: &str, base: &str) -> Option<String> {
    if path == base {
        return Some(String::new());
    }
    let rest = path.strip_prefix(base)?.strip_prefix('/')?;
    if rest.split('/').all(|c| !c.is_empty() && c != "." && c != "..") {
        Some(rest.to_string())
    } else {
        None
    }
}

fn object<'a>(v: &'a CValue, reason: &'static str) -> Result<&'a BTreeMap<String, CValue>, ImplError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(invalid(reason)),
    }
}

fn array<'a>(v: Option<&'a CValue>, reason: &'static str) -> Result<&'a [CValue], ImplError> {
    match v {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(invalid(reason)),
    }
}

fn string(v: Option<&CValue>, reason: &'static str) -> Result<String, ImplError> {
    match v {
        Some(CValue::Str(s)) => Ok(s.clone()),
        _ => Err(invalid(reason)),
    }
}

fn strings(v: Option<&CValue>, reason: &'static str) -> Result<Vec<String>, ImplError> {
    array(v, reason)?.iter().map(|x| string(Some(x), reason)).collect()
}

fn parse_package(v: &CValue) -> Result<Package, ImplError> {
    let m = object(v, "package")?;
    let mut targets = Vec::new();
    for t in array(m.get("targets"), "package_targets")? {
        let t = object(t, "target")?;
        targets.push(Target {
            kinds: strings(t.get("kind"), "target_kind")?,
            src_path: slashes(&string(t.get("src_path"), "target_src_path")?),
        });
    }
    let source = match m.get("source") {
        Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => return Err(invalid("package_source")),
    };
    Ok(Package {
        id: string(m.get("id"), "package_id")?,
        name: string(m.get("name"), "package_name")?,
        version: string(m.get("version"), "package_version")?,
        source,
        manifest_path: slashes(&string(m.get("manifest_path"), "package_manifest_path")?),
        targets,
    })
}

fn parse_node(v: &CValue) -> Result<(String, Node), ImplError> {
    let m = object(v, "node")?;
    let id = string(m.get("id"), "node_id")?;
    let mut features = strings(m.get("features"), "node_features")?;
    features.sort();
    features.dedup();
    let mut deps = BTreeSet::new();
    for d in array(m.get("deps"), "node_deps")? {
        let d = object(d, "node_dep")?;
        let pkg = string(d.get("pkg"), "node_dep_pkg")?;
        let kinds = array(d.get("dep_kinds"), "dep_kinds")?;
        if kinds.is_empty() {
            return Err(invalid("dep_kinds"));
        }
        let mut in_graph = false;
        for k in kinds {
            // `target` (a platform gate) is deliberately not read: the kernel graph spans
            // every platform, so the digest does not depend on the host.
            match object(k, "dep_kind")?.get("kind") {
                Some(CValue::Null) => in_graph = true,
                Some(CValue::Str(s)) if s == "build" => in_graph = true,
                Some(CValue::Str(s)) if s == "dev" => {}
                _ => return Err(invalid("dep_kind")),
            }
        }
        if in_graph {
            deps.insert(pkg);
        }
    }
    Ok((id, Node { features, deps }))
}

impl Metadata {
    /// Parses `cargo metadata --format-version 1` output (bounded by [`MAX_METADATA_BYTES`]).
    pub fn parse(bytes: &[u8]) -> Result<Metadata, ImplError> {
        if bytes.len() > MAX_METADATA_BYTES {
            return Err(invalid("too_large"));
        }
        let v = fid_canon::parse(bytes).map_err(|_| invalid("json"))?;
        let top = object(&v, "top_level")?;
        match top.get("version") {
            Some(CValue::Num(n)) if n.to_bits() == 1.0f64.to_bits() => {}
            _ => return Err(invalid("format_version")),
        }
        let root = slashes(&string(top.get("workspace_root"), "workspace_root")?);
        let workspace_root = root.trim_end_matches('/').to_string();
        let mut packages = BTreeMap::new();
        for p in array(top.get("packages"), "packages")? {
            let p = parse_package(p)?;
            if packages.insert(p.id.clone(), p).is_some() {
                return Err(invalid("duplicate_package_id"));
            }
        }
        let resolve = match top.get("resolve") {
            Some(r @ CValue::Obj(_)) => object(r, "resolve")?,
            _ => return Err(invalid("no_resolve")),
        };
        let mut nodes = BTreeMap::new();
        for n in array(resolve.get("nodes"), "resolve_nodes")? {
            let (id, node) = parse_node(n)?;
            if !packages.contains_key(&id) || !node.deps.iter().all(|d| packages.contains_key(d)) {
                return Err(invalid("dangling_package_id"));
            }
            if nodes.insert(id, node).is_some() {
                return Err(invalid("duplicate_node_id"));
            }
        }
        Ok(Metadata { workspace_root, packages, nodes })
    }

    /// The package named `name`: `Ok(None)` when there is none, `MetadataInvalid` when
    /// several packages share the name (two versions of one crate).
    pub(crate) fn package_named(&self, name: &str) -> Result<Option<&Package>, ImplError> {
        let mut found = self.packages.values().filter(|p| p.name == name);
        let first = found.next();
        if found.next().is_some() {
            return Err(invalid("ambiguous_crate_name"));
        }
        Ok(first)
    }

    /// The package with ID `id` (every ID in the graph was checked at parse time).
    pub(crate) fn package(&self, id: &str) -> Result<&Package, ImplError> {
        self.packages.get(id).ok_or_else(|| invalid("dangling_package_id"))
    }

    /// The enabled features of package `id`, sorted.
    pub(crate) fn features(&self, id: &str) -> Result<&[String], ImplError> {
        Ok(self.nodes.get(id).ok_or_else(|| invalid("package_without_node"))?.features.as_slice())
    }

    /// The package IDs reachable from `start` over normal and build edges, `start` included.
    /// Depth-first with sorted adjacency; the result is a set, so the order does not matter.
    pub(crate) fn closure(&self, start: &str) -> Result<BTreeSet<&str>, ImplError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let first = self.package(start)?;
        let mut stack: Vec<&str> = vec![first.id.as_str()];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let node = self.nodes.get(id).ok_or_else(|| invalid("package_without_node"))?;
            for d in node.deps.iter().rev() {
                if !seen.contains(d.as_str()) {
                    stack.push(d);
                }
            }
        }
        Ok(seen)
    }

    /// Where package `p` lives. Fails closed: the crate must lie inside the workspace root,
    /// every built target (lib, bin, ...) must have its source under `src/`, a build script or
    /// a proc-macro target is refused, and an unknown target kind is an error.
    pub(crate) fn layout(&self, p: &Package) -> Result<Layout, ImplError> {
        let crate_abs = p.manifest_path.strip_suffix("/Cargo.toml").ok_or_else(|| invalid("manifest_path"))?;
        let dir = relative(crate_abs, &self.workspace_root).ok_or_else(|| invalid("package_outside_workspace"))?;
        for t in &p.targets {
            if t.kinds.is_empty() {
                return Err(invalid("target_kind"));
            }
            for k in &t.kinds {
                let k = k.as_str();
                if BUILT_KINDS.contains(&k) {
                    match relative(&t.src_path, crate_abs) {
                        Some(r) if r.starts_with("src/") => {}
                        _ => return Err(invalid("target_outside_src")),
                    }
                } else if k == BUILD_KIND {
                    return Err(invalid("build_script_in_closure"));
                } else if k == PROC_MACRO_KIND {
                    return Err(invalid("proc_macro_in_closure"));
                } else if !IGNORED_KINDS.contains(&k) {
                    return Err(invalid("target_kind"));
                }
            }
        }
        Ok(Layout { dir })
    }
}

#[cfg(test)]
mod tests {
    use super::relative;

    #[test]
    fn relative_paths() {
        assert_eq!(relative("/w", "/w").as_deref(), Some(""));
        assert_eq!(relative("/w/a/b", "/w").as_deref(), Some("a/b"));
        assert_eq!(relative("/wx/a", "/w"), None);
        assert_eq!(relative("/w/a/../b", "/w"), None);
        assert_eq!(relative("/w//a", "/w"), None);
        assert_eq!(relative("/w/./a", "/w"), None);
        assert_eq!(relative("C:/u/w/a", "C:/u/w").as_deref(), Some("a"));
    }
}
