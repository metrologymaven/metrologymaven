//! `cargo xtask pins`: the repository map and cross-repository pin check (spec node
//! `ECO.REPO.TOPO`, REQ-ECO.REPO-005).
//!
//! `repos.toml` lists the first-party repositories (`[[repo]] id, url, purpose,
//! protected_tags`, optional `status`). The check reads the workspace root `Cargo.toml`,
//! every workspace member, and every path dependency inside the workspace root (Cargo's
//! automatic members), without network access or `cargo metadata`. For each `git`
//! dependency whose normalised URL is a first-party repository it requires an immutable
//! selector: a full 40-hex `rev`, or a `tag` on a repository whose tags are protected.
//! A manifest or member pattern that cannot be checked is a `PIN-MANIFEST` finding,
//! never a silent skip.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use toml::{Table, Value};

const USAGE: &str = "usage: cargo xtask pins [--workspace <dir>] [--repos <repos.toml>]";

/// Largest file the check reads.
const MAX_FILE_BYTES: u64 = 1 << 20;
/// Most manifests one run reads.
const MAX_MANIFESTS: usize = 10_000;

/// Dependency tables of a manifest (and of `target.<cfg>`), including Cargo's legacy spellings.
const DEP_TABLES: [&str; 5] =
    ["dependencies", "dev-dependencies", "build-dependencies", "dev_dependencies", "build_dependencies"];

/// Keys a `[[repo]]` entry may have.
const REPO_KEYS: [&str; 5] = ["id", "url", "purpose", "protected_tags", "status"];

/// Hosts whose owner/repository paths are case-insensitive. On every other host the
/// path is compared case-sensitively.
const CASE_INSENSITIVE_HOSTS: [&str; 2] = ["github.com", "www.github.com"];

/// Entry point for `cargo xtask pins ...` (arguments after `pins`).
pub fn cli(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut repos: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--repos", Some(v)) => repos = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let repos = repos.unwrap_or_else(|| workspace.join("repos.toml"));
    match run(&workspace, &repos) {
        Ok(findings) if findings.is_empty() => {
            println!("pins: ok");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Loads the repository map, then checks every workspace manifest. Returns sorted
/// findings; `Err` is a bad repository map (`PIN-REPOS`) or an unreadable root manifest.
fn run(ws: &Path, repos_file: &Path) -> Result<Vec<String>, String> {
    let text = read_bounded(repos_file).map_err(|e| format!("PIN-REPOS {}: {e}", repos_file.display()))?;
    let repos = parse_repos(&text).map_err(|e| format!("PIN-REPOS {}: {e}", repos_file.display()))?;
    let root_file = ws.join("Cargo.toml");
    let root = read_bounded(&root_file)
        .map_err(|e| format!("PIN-IO {}: {e}", root_file.display()))
        .and_then(|t| parse_toml(&t).map_err(|e| format!("PIN-MANIFEST Cargo.toml {e}")))?;
    let mut findings = BTreeSet::new();
    let (member_dirs, exclude) = members(ws, &root, &mut findings);
    let excluded = |rel: &str| exclude.iter().any(|x| rel == x || rel.starts_with(&format!("{x}/")));
    let mut queue: Vec<String> = member_dirs.into_iter().filter(|m| !excluded(m)).collect();
    queue.push(String::new());
    let mut seen = BTreeSet::new();
    while let Some(rel) = queue.pop() {
        if !seen.insert(rel.clone()) {
            continue;
        }
        if seen.len() > MAX_MANIFESTS {
            return Err(format!("PIN-IO more than {MAX_MANIFESTS} manifests"));
        }
        let shown = if rel.is_empty() { "Cargo.toml".to_string() } else { format!("{rel}/Cargo.toml") };
        let doc = if rel.is_empty() {
            root.clone()
        } else {
            let file = ws.join(&rel).join("Cargo.toml");
            if !file.is_file() {
                findings.insert(format!("PIN-MANIFEST {shown} missing"));
                continue;
            }
            match read_bounded(&file).and_then(|t| parse_toml(&t)) {
                Ok(doc) => doc,
                Err(e) => {
                    findings.insert(format!("PIN-MANIFEST {shown} {e}"));
                    continue;
                }
            }
        };
        check_manifest(&doc, &shown, &repos, &mut findings);
        // Path dependencies inside the workspace root are workspace members too.
        for (_, spec) in dependency_specs(&doc, false) {
            if let Some(Value::String(p)) = spec.get("path") {
                if let Some(dep) = resolve(&rel, p) {
                    if !dep.is_empty() && !excluded(&dep) && !seen.contains(&dep) {
                        queue.push(dep);
                    }
                }
            }
        }
    }
    Ok(findings.into_iter().collect())
}

fn read_bounded(path: &Path) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE_BYTES {
        return Err(format!("larger than {MAX_FILE_BYTES} bytes"));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

fn parse_toml(text: &str) -> Result<Table, String> {
    text.parse::<Table>().map_err(|e: toml::de::Error| e.to_string().trim_end().replace('\n', " "))
}

// ---------------------------------------------------------------------------------------
// Repository map
// ---------------------------------------------------------------------------------------

/// The repository map, keyed by normalised URL; the value is `protected_tags`.
type Repos = BTreeMap<String, bool>;

fn parse_repos(text: &str) -> Result<Repos, String> {
    let doc = parse_toml(text)?;
    if let Some(k) = doc.keys().find(|k| k.as_str() != "repo") {
        return Err(format!("unknown top-level key {k}"));
    }
    let Some(Value::Array(list)) = doc.get("repo") else {
        return Err("expected [[repo]] entries".into());
    };
    let mut ids = BTreeSet::new();
    let mut repos = Repos::new();
    for (i, entry) in list.iter().enumerate() {
        let n = i + 1;
        let Value::Table(t) = entry else {
            return Err(format!("repo {n}: expected a table"));
        };
        if let Some(k) = t.keys().find(|k| !REPO_KEYS.contains(&k.as_str())) {
            return Err(format!("repo {n}: unknown key {k}"));
        }
        let text_field = |k: &str| match t.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
            _ => Err(format!("repo {n}: {k} must be a non-empty string")),
        };
        let id = text_field("id")?;
        let url = text_field("url")?;
        text_field("purpose")?;
        let Some(Value::Boolean(protected)) = t.get("protected_tags") else {
            return Err(format!("repo {n}: protected_tags must be true or false"));
        };
        match t.get("status") {
            None => {}
            Some(Value::String(s)) if ["active", "archived", "planned"].contains(&s.as_str()) => {}
            _ => return Err(format!("repo {n}: status must be \"active\", \"archived\" or \"planned\"")),
        }
        if !ids.insert(id.clone()) {
            return Err(format!("repo {n}: duplicate id {id}"));
        }
        if repos.insert(normalise_url(&url), *protected).is_some() {
            return Err(format!("repo {n}: duplicate url {url}"));
        }
    }
    if repos.is_empty() {
        return Err("expected [[repo]] entries".into());
    }
    Ok(repos)
}

/// Normalises a git URL for comparison:
/// - drops the scheme and any user name, and reads `user@host:path` as `host/path`;
/// - lower-cases the host and drops the scheme's default port (443 https, 80 http,
///   22 ssh, 9418 git);
/// - drops a query or fragment, and strips trailing `/` and `.git`;
/// - lower-cases the owner/repository path on case-insensitive hosts (github.com);
///   elsewhere the path keeps its case.
fn normalise_url(url: &str) -> String {
    let u = url.trim();
    let (scheme, rest) = match u.find("://") {
        Some(i) if i > 0 && u[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)) => {
            (u[..i].to_ascii_lowercase(), &u[i + 3..])
        }
        _ => (String::new(), u),
    };
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (mut authority, mut path) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), String::new()),
    };
    if scheme.is_empty() {
        if let Some(i) = authority.find(':') {
            path = format!("/{}{path}", &authority[i + 1..]);
            authority.truncate(i);
        }
    }
    let mut host = authority.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "https" | "git+https" => ":443",
        "http" | "git+http" => ":80",
        "ssh" | "git+ssh" => ":22",
        "git" => ":9418",
        _ => "",
    };
    if !default_port.is_empty() && host.ends_with(default_port) {
        host.truncate(host.len() - default_port.len());
    }
    loop {
        let before = path.len();
        while path.ends_with('/') {
            path.pop();
        }
        if let Some(p) = path.strip_suffix(".git") {
            path = p.to_string();
        }
        if path.len() == before {
            break;
        }
    }
    if CASE_INSENSITIVE_HOSTS.contains(&host.as_str()) {
        path = path.to_ascii_lowercase();
    }
    format!("{host}{path}")
}

// ---------------------------------------------------------------------------------------
// Workspace manifests
// ---------------------------------------------------------------------------------------

/// The member folders listed by `workspace.members` (patterns expanded) and the
/// `workspace.exclude` folders, relative to the root. Problems become findings.
fn members(ws: &Path, root: &Table, findings: &mut BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    let Some(Value::Table(w)) = root.get("workspace") else {
        return (Vec::new(), Vec::new());
    };
    let mut strings = |k: &str| -> Vec<String> {
        match w.get(k) {
            None => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(clean_rel(s)),
                    _ => {
                        findings.insert(format!("PIN-MANIFEST Cargo.toml workspace.{k} must list strings"));
                        None
                    }
                })
                .collect(),
            Some(_) => {
                findings.insert(format!("PIN-MANIFEST Cargo.toml workspace.{k} must be an array"));
                Vec::new()
            }
        }
    };
    let exclude = strings("exclude");
    let patterns = strings("members");
    let mut out = BTreeSet::new();
    for pattern in patterns {
        out.extend(expand(ws, &pattern, findings));
    }
    (out.into_iter().collect(), exclude)
}

/// `./a//b/` -> `a/b`, with `\` read as `/`.
fn clean_rel(s: &str) -> String {
    s.replace('\\', "/").split('/').filter(|x| !x.is_empty() && *x != ".").collect::<Vec<_>>().join("/")
}

/// Joins a relative `path` dependency onto the folder `base` of its manifest, resolving
/// `.` and `..`. `None` when it is absolute or leaves the workspace root.
fn resolve(base: &str, path: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    if p.starts_with('/') || Path::new(path).is_absolute() || p.get(1..2) == Some(":") {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|x| !x.is_empty()).collect();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Expands a member pattern; `*` and `?` match within one path segment (dot folders
/// included, as Cargo's glob does). Other glob syntax (`**`, `[...]`, `{...}`) is
/// reported, not guessed. Non-folder matches are not crates and are left out.
fn expand(ws: &Path, pattern: &str, findings: &mut BTreeSet<String>) -> Vec<String> {
    if pattern.contains("**") || pattern.contains(['[', ']', '{', '}', '!']) {
        findings.insert(format!("PIN-MANIFEST Cargo.toml unsupported member pattern {pattern}"));
        return Vec::new();
    }
    let mut cur = vec![String::new()];
    for seg in pattern.split('/').filter(|s| !s.is_empty()) {
        let mut next = BTreeSet::new();
        for base in &cur {
            if !seg.contains(['*', '?']) {
                next.insert(join_rel(base, seg));
                continue;
            }
            let Ok(rd) = std::fs::read_dir(ws.join(base)) else {
                let shown = if base.is_empty() { "." } else { base.as_str() };
                findings.insert(format!("PIN-MANIFEST {shown} cannot read folder for member pattern {pattern}"));
                continue;
            };
            for e in rd {
                let Ok(e) = e else {
                    findings.insert(format!("PIN-MANIFEST {base} cannot read folder entry"));
                    continue;
                };
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_dir() && wildcard(seg.as_bytes(), name.as_bytes()) {
                    next.insert(join_rel(base, &name));
                }
            }
        }
        cur = next.into_iter().collect();
    }
    cur
}

fn join_rel(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

fn wildcard(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

fn sub<'t>(t: &'t Table, k: &str) -> Option<&'t Table> {
    match t.get(k) {
        Some(Value::Table(m)) => Some(m),
        _ => None,
    }
}

fn push_dep_tables<'t>(t: &'t Table, out: &mut Vec<&'t Table>) {
    for k in DEP_TABLES {
        if let Some(m) = sub(t, k) {
            out.push(m);
        }
    }
}

/// Every `(name, spec table)` in the dependency tables of one manifest: the plain and
/// target-specific tables and `workspace.dependencies`; with `overrides`, also
/// `patch.<source>` and `replace`.
fn dependency_specs(doc: &Table, overrides: bool) -> Vec<(&str, &Table)> {
    let mut tables: Vec<&Table> = Vec::new();
    push_dep_tables(doc, &mut tables);
    if let Some(w) = sub(doc, "workspace") {
        tables.extend(sub(w, "dependencies"));
    }
    if let Some(targets) = sub(doc, "target") {
        for v in targets.values() {
            if let Value::Table(t) = v {
                push_dep_tables(t, &mut tables);
            }
        }
    }
    if overrides {
        if let Some(patches) = sub(doc, "patch") {
            for v in patches.values() {
                if let Value::Table(t) = v {
                    tables.push(t);
                }
            }
        }
        tables.extend(sub(doc, "replace"));
    }
    let mut specs = Vec::new();
    for t in tables {
        for (name, v) in t {
            if let Value::Table(spec) = v {
                specs.push((name.as_str(), spec));
            }
        }
    }
    specs
}

fn check_manifest(doc: &Table, file: &str, repos: &Repos, findings: &mut BTreeSet<String>) {
    for (name, spec) in dependency_specs(doc, true) {
        let Some(Value::String(git)) = spec.get("git") else { continue };
        let Some(&protected_tags) = repos.get(&normalise_url(git)) else { continue };
        let code = if spec.contains_key("branch") {
            Some("PIN-MOVING")
        } else if let Some(rev) = spec.get("rev") {
            let full = matches!(rev, Value::String(r) if r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()));
            (!full).then_some("PIN-REV-SHORT")
        } else if spec.contains_key("tag") {
            (!protected_tags).then_some("PIN-TAG-UNPROTECTED")
        } else {
            Some("PIN-MOVING")
        };
        if let Some(code) = code {
            findings.insert(format!("{code} {file} {name}"));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn dependency_tables_are_found() {
        let doc = parse_toml(concat!(
            "[dependencies]\n",
            "x = { git = 'https://h/x', rev = \"0123\" }\n",
            "[target.'cfg(windows)'.dependencies.y]\n",
            "git = \"https://h/y\"\n",
            "[workspace.dependencies]\n",
            "w = { path = \"w\" }\n",
            "[patch.crates-io]\n",
            "p = { git = \"https://h/p\" }\n",
        ))
        .unwrap();
        let names = |o| dependency_specs(&doc, o).iter().map(|(n, _)| n.to_string()).collect::<Vec<_>>();
        assert_eq!(names(false), ["x", "w", "y"]);
        assert_eq!(names(true), ["x", "w", "y", "p"]);
    }

    /// REQ-ECO.REPO.TOPO-004, hand-labelled.
    #[test]
    fn urls_normalise() {
        let base = "example.org/fid/libs";
        for u in [
            "https://example.org/fid/libs.git",
            "HTTPS://Example.org/fid/libs",
            "https://example.org/fid/libs/",
            "https://example.org:443/fid/libs",
            "http://EXAMPLE.ORG:80/fid/libs.git/",
            "ssh://git@example.org/fid/libs.git",
            "ssh://git@example.org:22/fid/libs.git",
            "git@example.org:fid/libs.git",
            "git+https://example.org/fid/libs?x=1",
        ] {
            assert_eq!(normalise_url(u), base, "{u}");
        }
        for u in ["https://example.org/fid/Libs", "https://example.org/fid/libs2", "https://example.org:8443/fid/libs"]
        {
            assert_ne!(normalise_url(u), base, "{u}");
        }
        assert_eq!(normalise_url("https://GitHub.com/Org/Repo.git"), "github.com/org/repo");
        assert_eq!(normalise_url("git@github.com:ORG/repo"), "github.com/org/repo");
    }

    #[test]
    fn paths_resolve_inside_the_root_only() {
        assert_eq!(resolve("app", "../libs/util"), Some("libs/util".into()));
        assert_eq!(resolve("", "./a/./b/"), Some("a/b".into()));
        assert_eq!(resolve("a/b", "..\\c"), Some("a/c".into()));
        assert_eq!(resolve("app", "../.."), None);
        assert_eq!(resolve("app", "/abs/x"), None);
        assert_eq!(resolve("app", "C:/x"), None);
    }

    #[test]
    fn wildcards() {
        assert!(wildcard(b"*", b"fid-num"));
        assert!(wildcard(b"fid-*", b"fid-num"));
        assert!(wildcard(b"f?d-*m", b"fid-num"));
        assert!(!wildcard(b"fid-*", b"xtask"));
        assert!(!wildcard(b"a*b", b"a"));
    }

    #[test]
    fn bad_repo_maps_are_refused() {
        let ok = "[[repo]]\nid = \"a\"\nurl = \"https://h/a\"\npurpose = \"p\"\nprotected_tags = false\n";
        assert!(parse_repos(ok).is_ok());
        let second = ok.replace("id = \"a\"", "id = \"b\"").replace("https://h/a", "HTTPS://H/a.git");
        let dup_url = format!("{ok}{second}");
        for (text, why) in [
            ("", "empty"),
            ("[repo]\nid = \"a\"\n", "not an array of tables"),
            (&ok.replace("protected_tags = false", "protected_tags = \"no\"")[..], "flag not boolean"),
            (&ok.replace("purpose = \"p\"\n", "")[..], "missing purpose"),
            (&ok.replace("id = \"a\"", "id = \"a\"\nowner = \"x\"")[..], "unknown key"),
            (&format!("{ok}status = \"gone\"\n")[..], "bad status"),
            (&dup_url[..], "dup url"),
            (&format!("{ok}{}", ok.replace("https://h/a", "https://h/b"))[..], "dup id"),
            (&format!("x = 1\n{ok}")[..], "unknown top-level key"),
            ("a = [\n", "syntax"),
        ] {
            assert!(parse_repos(text).is_err(), "{why}");
        }
    }
}
